//! Quick look (Space on a file that is neither an image nor audio or video): the preview over the
//! whole window on the same translucent black backdrop as the image viewer, with previous, next
//! and close. Going through the files moves the selection along.

use egui::{Align2, Color32, FontId, Rect, Sense, pos2, vec2};

use super::app::App;
use crate::vfs::Entry;

/// Space on `e`: the image viewer, the full-window player, or quick look.
pub fn open(app: &mut App, e: Entry) {
    if super::viewer::viewable(&e) {
        super::viewer::open(app, &e);
    } else if super::player::playable(&e) && crate::media::available() {
        let (list, index) = super::player::queue_for(app, &e);
        super::player::start(app, list, index);
        if let Some(p) = app.player.as_mut() {
            p.fullscreen = true;
        }
    } else {
        app.quick_look = Some(e);
    }
}

/// Where the quick-looked file is in the active pane, and how many there are.
fn position(app: &App) -> Option<(usize, usize)> {
    let e = app.quick_look.as_ref()?;
    let tab = app.tab(app.active_pane);
    let n = tab.visible.len();
    (0..n)
        .find(|&i| tab.item(i).is_some_and(|x| x.location == e.location))
        .map(|i| (i, n))
}

fn go(app: &mut App, to: isize) {
    let Some((_, n)) = position(app) else { return };
    let to = to.clamp(0, n as isize - 1) as usize;
    let p = app.active_pane;
    let e = app.tab(p).item(to).cloned();
    if let Some(e) = e {
        let tab = app.tab_mut(p);
        tab.select_only(to);
        tab.scroll_to_cursor = true;
        app.quick_look = Some(e);
    }
}

pub fn keys(app: &mut App, ctx: &egui::Context) {
    use egui::{Key, Modifiers};
    let pressed = |k: Key| ctx.input_mut(|inp| inp.consume_key(Modifiers::NONE, k));
    let i = position(app).map(|(i, _)| i as isize).unwrap_or(0);
    if pressed(Key::Escape) || pressed(Key::Space) {
        app.quick_look = None;
    } else if pressed(Key::ArrowRight) || pressed(Key::ArrowDown) {
        go(app, i + 1);
    } else if pressed(Key::ArrowLeft) || pressed(Key::ArrowUp) {
        go(app, i - 1);
    } else if pressed(Key::Home) {
        go(app, 0);
    } else if pressed(Key::End) {
        go(app, isize::MAX / 2);
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(entry) = app.quick_look.clone() else {
        return;
    };
    let screen = ctx.content_rect();
    let pos = position(app);
    let mut nav = (false, false, false);
    let mut close = false;
    egui::Area::new(egui::Id::unique("quick-look"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            // Over the whole window (the area would otherwise clip short of its edge), in dark.
            ui.set_clip_rect(screen);
            *ui.visuals_mut() = egui::Visuals::dark();
            let backdrop = ui.allocate_rect(screen, Sense::click());
            let painter = ui.painter().clone();
            painter.rect_filled(screen, 0, Color32::from_black_alpha(225));
            let area = Rect::from_min_max(
                screen.min + vec2(88.0, 56.0),
                screen.max - vec2(88.0, 48.0),
            );
            // Pictures sit on the backdrop; text, tables and lists on a card.
            let picture = matches!(
                app.preview.content,
                Some(
                    crate::preview::Content::Image { .. }
                        | crate::preview::Content::Pdf { .. }
                        | crate::preview::Content::Media { .. }
                        | crate::preview::Content::Folder { .. }
                        | crate::preview::Content::None(_)
                )
            ) || app.preview.loading;
            let inner = if picture {
                area
            } else {
                let card = Rect::from_center_size(
                    area.center(),
                    vec2(area.width().min(1100.0), area.height()),
                );
                painter.rect_filled(card, 10, Color32::from_rgb(0x1c, 0x1c, 0x1f));
                card.shrink(14.0)
            };
            // Always dark, whatever the theme: it is drawn on black.
            let dark = app.dark;
            app.dark = true;
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            child.set_clip_rect(inner);
            *child.visuals_mut() = egui::Visuals::dark();
            super::preview_panel::content(app, &mut child, &entry, true);
            app.dark = dark;
            // The name, and where it is in the folder.
            let mut caption = entry.name.clone();
            if let Some((i, n)) = pos {
                caption.push_str(&format!("   ·   {} / {}", i + 1, n));
            }
            painter.text(
                pos2(screen.center().x, screen.top() + 28.0),
                Align2::CENTER_CENTER,
                caption,
                FontId::proportional(14.0),
                Color32::from_gray(225),
            );
            let (has_prev, has_next) = match pos {
                Some((i, n)) => (i > 0, i + 1 < n),
                None => (false, false),
            };
            nav = super::viewer::nav_buttons(ui, &painter, screen, has_prev, has_next);
            if backdrop.double_clicked() {
                close = true;
            }
        });
    let i = pos.map(|(i, _)| i as isize).unwrap_or(0);
    if nav.0 {
        go(app, i - 1);
    } else if nav.1 {
        go(app, i + 1);
    }
    if nav.2 || close {
        app.quick_look = None;
    }
}
