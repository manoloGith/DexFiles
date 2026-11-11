//! The image viewer: an image over the whole window on a translucent black backdrop, with
//! previous, next and close (and ←, →, Home, End, Esc). The images are those of the folder,
//! decoded at the window's size in device pixels.

use std::time::Instant;

use egui::{Align2, Color32, FontId, Rect, Sense, Vec2, pos2, vec2};

use super::app::{App, Msg};
use super::icons::{self, Sym};
use crate::vfs::Entry;

pub struct Viewer {
    pub list: Vec<Entry>,
    pub index: usize,
    texture: Option<egui::TextureHandle>,
    /// Which entry (and size) the texture is of, and which one is being loaded.
    shown: Option<(usize, u32)>,
    loading: Option<(usize, u32)>,
    dims: (u32, u32),
    error: Option<String>,
    opened: Instant,
    last_wheel: Instant,
}

/// Whether the viewer can show this file.
pub fn viewable(e: &Entry) -> bool {
    !e.is_dir()
        && matches!(
            e.ext.as_str(),
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff"
        )
}

/// Open the viewer at `e`, with the other images of its folder (as listed) to go through.
pub fn open(app: &mut App, e: &Entry) {
    let tab = app.tab(app.active_pane);
    let mut list: Vec<Entry> = tab.items().filter(|x| viewable(x)).cloned().collect();
    let index = match list.iter().position(|x| x.location == e.location) {
        Some(i) => i,
        None => {
            list = vec![e.clone()];
            0
        }
    };
    app.viewer = Some(Viewer {
        list,
        index,
        texture: None,
        shown: None,
        loading: None,
        dims: (0, 0),
        error: None,
        opened: Instant::now(),
        last_wheel: Instant::now(),
    });
}

pub fn loaded(
    app: &mut App,
    index: usize,
    px: u32,
    result: Result<(egui::ColorImage, u32, u32), String>,
) {
    let ctx = app.ctx.clone();
    let Some(v) = app.viewer.as_mut() else { return };
    if v.loading != Some((index, px)) {
        return;
    }
    v.loading = None;
    match result {
        Ok((image, w, h)) => {
            let options = egui::TextureOptions {
                mipmap_mode: Some(egui::TextureFilter::Linear),
                ..egui::TextureOptions::LINEAR
            };
            v.texture = Some(ctx.load_texture("viewer", image, options));
            v.shown = Some((index, px));
            v.dims = (w, h);
            v.error = None;
        }
        Err(e) => {
            v.texture = None;
            v.shown = Some((index, px));
            v.error = Some(e);
        }
    }
}

fn request(app: &mut App, px: u32) {
    let Some(v) = app.viewer.as_mut() else { return };
    let want = (v.index, px);
    if v.shown == Some(want) || v.loading == Some(want) {
        return;
    }
    v.loading = Some(want);
    let Some(entry) = v.list.get(v.index).cloned() else {
        return;
    };
    let (tx, ctx, index) = (app.tx.clone(), app.ctx.clone(), v.index);
    let _ = std::thread::Builder::new()
        .name("dexfiles-viewer".into())
        .spawn(move || {
            let result = crate::vfs::fs_for(&entry.location)
                .and_then(|fs| fs.read_head(&entry.location.backend_path(), 200 * 1024 * 1024))
                .map_err(|e| e.to_string())
                .and_then(|bytes| crate::preview::decode_image(&bytes, px));
            let _ = tx.send(Msg::ViewerImage { index, px, result });
            ctx.request_repaint();
        });
}

pub fn go(app: &mut App, to: isize) {
    if let Some(v) = app.viewer.as_mut() {
        let n = v.list.len() as isize;
        v.index = to.clamp(0, (n - 1).max(0)) as usize;
    }
}

/// Keys while the viewer is open (it takes them all).
pub fn keys(app: &mut App, ctx: &egui::Context) {
    use egui::{Key, Modifiers};
    let Some(v) = app.viewer.as_ref() else { return };
    let (i, n) = (v.index as isize, v.list.len() as isize);
    let pressed = |k: Key| ctx.input_mut(|inp| inp.consume_key(Modifiers::NONE, k));
    if pressed(Key::Escape) || pressed(Key::Space) {
        app.viewer = None;
    } else if pressed(Key::ArrowRight) || pressed(Key::ArrowDown) || pressed(Key::PageDown) {
        go(app, i + 1);
    } else if pressed(Key::ArrowLeft) || pressed(Key::ArrowUp) || pressed(Key::PageUp) {
        go(app, i - 1);
    } else if pressed(Key::Home) {
        go(app, 0);
    } else if pressed(Key::End) {
        go(app, n - 1);
    }
    // The wheel goes through the images too, one per turn.
    let wheel: f32 = ctx.input(|inp| {
        inp.events
            .iter()
            .map(|e| {
                if let egui::Event::MouseWheel { delta, .. } = e {
                    delta.y + delta.x
                } else {
                    0.0
                }
            })
            .sum()
    });
    if wheel != 0.0
        && let Some(v) = app.viewer.as_mut()
        && v.last_wheel.elapsed().as_millis() > 250
    {
        v.last_wheel = Instant::now();
        let to = v.index as isize + if wheel < 0.0 { 1 } else { -1 };
        go(app, to);
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    if app.viewer.is_none() {
        return;
    }
    let screen = ctx.content_rect();
    let ppp = ctx.pixels_per_point();
    let px = (screen.width().max(screen.height()) * ppp).round() as u32;
    request(app, px);
    let mut action: Option<isize> = None;
    let mut close = false;
    egui::Area::new(egui::Id::unique("image-viewer"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let Some(v) = app.viewer.as_ref() else { return };
            // The backdrop takes every click under it.
            let backdrop = ui.allocate_rect(screen, Sense::click());
            let painter = ui.painter().clone();
            painter.rect_filled(screen, 0, Color32::from_black_alpha(225));
            let area = screen.shrink2(vec2(64.0, 48.0));
            if let Some(tex) = &v.texture {
                let size = tex.size_vec2() / ppp;
                let scale = (area.width() / size.x)
                    .min(area.height() / size.y)
                    .min(1.0 * ppp.max(1.0));
                let r = Rect::from_center_size(area.center(), size * scale);
                egui::Image::from_texture(tex).paint_at(ui, r);
            }
            if v.loading.is_some() && v.shown.map(|(i, _)| i) != Some(v.index) {
                ui.put(
                    Rect::from_center_size(area.center(), Vec2::splat(32.0)),
                    egui::Spinner::new().size(32.0).color(Color32::WHITE),
                );
            }
            if let Some(e) = &v.error {
                painter.text(
                    area.center(),
                    Align2::CENTER_CENTER,
                    e,
                    FontId::proportional(14.0),
                    Color32::from_rgb(0xff, 0x8a, 0x80),
                );
            }
            // Caption: name, position, size.
            if let Some(e) = v.list.get(v.index) {
                let mut caption = format!("{}   ·   {} / {}", e.name, v.index + 1, v.list.len());
                if v.dims.0 > 0 && v.shown.map(|(i, _)| i) == Some(v.index) {
                    caption.push_str(&format!("   ·   {} × {}", v.dims.0, v.dims.1));
                }
                painter.text(
                    pos2(screen.center().x, screen.bottom() - 22.0),
                    Align2::CENTER_CENTER,
                    caption,
                    FontId::proportional(13.0),
                    Color32::from_gray(220),
                );
            }
            // Buttons: previous and next at the sides, close at the top right.
            let button = |r: Rect, s: Sym, enabled: bool, id: &str| -> bool {
                let resp = ui.interact(
                    r,
                    ui.scope_id().with(id),
                    if enabled {
                        Sense::click()
                    } else {
                        Sense::hover()
                    },
                );
                if !enabled {
                    return false;
                }
                let bg = if resp.hovered() {
                    Color32::from_white_alpha(46)
                } else {
                    Color32::from_white_alpha(18)
                };
                painter.circle_filled(r.center(), r.width() / 2.0, bg);
                icons::sym(&painter, r.shrink(r.width() * 0.28), s, Color32::WHITE);
                resp.clicked()
            };
            let i = v.index as isize;
            let n = v.list.len();
            if button(
                Rect::from_center_size(
                    pos2(screen.left() + 34.0, screen.center().y),
                    Vec2::splat(44.0),
                ),
                Sym::Back,
                v.index > 0,
                "viewer-prev",
            ) {
                action = Some(i - 1);
            }
            if button(
                Rect::from_center_size(
                    pos2(screen.right() - 34.0, screen.center().y),
                    Vec2::splat(44.0),
                ),
                Sym::Forward,
                v.index + 1 < n,
                "viewer-next",
            ) {
                action = Some(i + 1);
            }
            if button(
                Rect::from_center_size(
                    pos2(screen.right() - 30.0, screen.top() + 30.0),
                    Vec2::splat(36.0),
                ),
                Sym::Close,
                true,
                "viewer-close",
            ) {
                close = true;
            }
            if backdrop.double_clicked() && v.opened.elapsed().as_millis() > 400 {
                close = true;
            }
        });
    if let Some(to) = action {
        go(app, to);
    }
    if close {
        app.viewer = None;
    }
}
