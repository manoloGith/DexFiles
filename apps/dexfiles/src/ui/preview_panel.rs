//! The preview panel on the right. What is shown fills the panel from the top; what is known
//! about it sits at the bottom. The player docks under the information (a third of the panel)
//! while it plays a file other than the one previewed, and fills the picture area when it plays
//! the one previewed.

use std::collections::BTreeMap;

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Ui, Vec2, pos2, vec2};

use super::app::{Action, App};
use super::icons::{self, Sym};
use super::theme;
use crate::archive::ArchiveItem;
use crate::fileinfo::{self, Category};
use crate::i18n::{t, tf};
use crate::preview::Content;
use crate::vfs::Entry;

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.active_pane;
    let sel = app.tab(p).selected_entries();
    let single = (sel.len() == 1).then(|| sel[0].clone());
    let playing_this = single.as_ref().is_some_and(|e| {
        app.player
            .as_ref()
            .and_then(|pl| pl.current())
            .is_some_and(|c| c.location == e.location)
    });
    let height = ui.available_height();
    if app.player.is_some() && !playing_this {
        egui::Panel::bottom("pv-player")
            .exact_size((height / 3.0).max(150.0))
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 0,
                right: 0,
                top: 10,
                bottom: 0,
            }))
            .show_separator_line(false)
            .show(ui, |ui| super::player::show(app, ui));
    }
    egui::Panel::bottom("pv-info")
        .frame(egui::Frame::NONE.inner_margin(egui::Margin {
            left: 0,
            right: 0,
            top: 10,
            bottom: 0,
        }))
        .show_separator_line(false)
        .show(ui, |ui| {
            // Sized by its content (a scroll area here would keep the panel from growing).
            info(app, ui, &sel);
        });
    egui::CentralPanel::no_frame().show(ui, |ui| match (&single, sel.len()) {
        // Quick look shows the preview over the window (and asks for it at its own size).
        _ if app.quick_look.is_some() => {}
        (Some(_), _) if playing_this => super::player::show(app, ui),
        (Some(e), _) => content(app, ui, e, false),
        (None, 0) => {
            let r = ui.available_rect_before_wrap();
            big_icon(ui, r, |p, rr| {
                icons::folder(p, rr, Category::Folder.color())
            });
        }
        (None, _) => {
            let r = ui.available_rect_before_wrap();
            let side = (r.width().min(r.height()) * 0.45).clamp(48.0, 160.0);
            let base = Rect::from_center_size(r.center(), Vec2::splat(side));
            for (i, e) in sel.iter().take(3).enumerate().rev() {
                let off = i as f32 * side * 0.1;
                let rr = base.translate(vec2(off, -off));
                icons::file_icon(
                    ui.painter(),
                    rr,
                    Category::of(&e.ext, e.is_dir()),
                    &e.ext,
                    app.dark,
                );
            }
        }
    });
}

fn big_icon(ui: &Ui, r: Rect, draw: impl FnOnce(&egui::Painter, Rect)) {
    let side = (r.width().min(r.height()) * 0.5).clamp(48.0, 180.0);
    draw(
        ui.painter(),
        Rect::from_center_size(r.center(), Vec2::splat(side)),
    );
}

/// The bottom part: name, kind and size, actions, details.
fn info(app: &mut App, ui: &mut Ui, sel: &[Entry]) {
    let pal = theme::palette(app.dark);
    match sel.len() {
        0 => {
            let tab = app.tab(app.active_pane);
            let (title, n) = (tab.title(), tab.visible.len());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(title).strong().size(15.0));
                ui.label(RichText::new(tf("n_items", n)).color(pal.weak));
            });
        }
        1 => {
            let e = sel[0].clone();
            ui.vertical_centered(|ui| {
                ui.add(egui::Label::new(RichText::new(&e.name).strong().size(15.0)).wrap());
                let mut sub = fileinfo::kind_label(&e.ext, e.is_dir());
                let size = super::view::size_text(app, &e);
                if size != "—" {
                    sub.push_str(&format!(" · {size}"));
                }
                if let Some(s) = app.dir_size(&e) {
                    sub.push_str(&format!(" · {}", tf("n_items", s.items)));
                }
                ui.label(RichText::new(sub).color(pal.weak));
                ui.horizontal_wrapped(|ui| {
                    let p = app.active_pane;
                    if super::player::playable(&e)
                        && crate::media::available()
                        && ui.small_button(t("play")).clicked()
                    {
                        app.actions.push(Action::Play(e.clone()));
                    }
                    if ui.small_button(t("open")).clicked() {
                        app.actions.push(Action::Open(p, e.clone()));
                    }
                    if !e.is_dir() && ui.small_button(t("share")).clicked() {
                        app.actions
                            .push(Action::Share(vec![e.location.clone()], false));
                    }
                    if icons::sym_button(ui, Sym::More, false, t("properties")).clicked() {
                        app.actions.push(Action::Properties(e.clone()));
                    }
                });
            });
            ui.add_space(6.0);
            details(app, ui, &e);
        }
        n => {
            let size: u64 = sel
                .iter()
                .map(|e| {
                    if e.is_dir() {
                        app.dir_size(e).map(|s| s.bytes).unwrap_or(0)
                    } else {
                        e.size
                    }
                })
                .sum();
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(tf("n_selected", n)).strong().size(15.0));
                if size > 0 {
                    ui.label(RichText::new(fileinfo::format_size(size)).color(pal.weak));
                }
                if sel.iter().filter(|e| super::player::playable(e)).count() > 1
                    && crate::media::available()
                    && ui.button(t("play_all")).clicked()
                    && let Some(first) = sel.iter().find(|e| super::player::playable(e))
                {
                    app.actions.push(Action::Play(first.clone()));
                }
            });
        }
    }
}

/// What a file holds, filling `ui`'s space (also the quick look's, `large`).
pub fn content(app: &mut App, ui: &mut Ui, e: &Entry, large: bool) {
    let pal = theme::palette(app.dark);
    let area = ui.available_rect_before_wrap();
    if app.preview.loading || app.preview.content.is_none() {
        if e.is_dir() {
            big_icon(ui, area, |p, r| {
                icons::folder(p, r, Category::Folder.color())
            });
        } else {
            ui.centered_and_justified(|ui| ui.add(egui::Spinner::new().size(20.0)));
        }
        return;
    }
    let mut pdf_wanted: Option<(std::path::PathBuf, usize, u32)> = None;
    let mut play = false;
    let mut fullscreen = false;
    let mut open_viewer = false;
    match &app.preview.content {
        Some(content @ (Content::Image { .. } | Content::Pdf { .. } | Content::Media { .. })) => {
            let reserve = if matches!(content, Content::Pdf { pages, .. } if *pages > 1)
                || matches!(content, Content::Image { .. })
            {
                22.0
            } else {
                0.0
            };
            let pic = Rect::from_min_max(area.min, pos2(area.right(), area.bottom() - reserve));
            let pic_resp = ui.allocate_rect(area, Sense::click());
            // A double click: the image viewer, or the player over the whole window.
            if pic_resp.double_clicked() {
                if super::viewer::viewable(e) {
                    open_viewer = true;
                } else if super::player::playable(e) && crate::media::available() {
                    play = true;
                    fullscreen = true;
                }
            }
            match &app.preview.texture {
                Some(tex) => {
                    let size = tex.size_vec2();
                    // As big as fits; photos and covers are not blown up past twice their size.
                    let cap = if matches!(content, Content::Pdf { .. }) {
                        f32::INFINITY
                    } else {
                        2.0
                    };
                    let scale = (pic.width() / size.x).min(pic.height() / size.y).min(cap);
                    let r = Rect::from_center_size(pic.center(), size * scale);
                    if matches!(content, Content::Pdf { .. }) {
                        ui.painter().rect_filled(r.expand(1.0), 2, pal.separator);
                    }
                    egui::Image::from_texture(tex)
                        .corner_radius(if matches!(content, Content::Pdf { .. }) {
                            0
                        } else {
                            6
                        })
                        .paint_at(ui, r);
                    if let Content::Pdf { path, page, px, .. } = content {
                        // Render again at the size it is shown, in device pixels: sharp text.
                        let want = (r.width().max(r.height()) * ui.ctx().pixels_per_point()).round()
                            as u32;
                        let ratio = want as f32 / (*px).max(1) as f32;
                        if !(0.9..=1.1).contains(&ratio) {
                            pdf_wanted = Some((path.clone(), *page, want));
                        }
                    }
                    if matches!(content, Content::Media { .. })
                        && super::player::playable(e)
                        && crate::media::available()
                    {
                        play |= play_button(ui, r);
                    }
                }
                None => {
                    let cat = Category::of(&e.ext, false);
                    big_icon(ui, pic, |p, rr| {
                        icons::file_icon(p, rr, cat, &e.ext, app.dark)
                    });
                    if super::player::playable(e) && crate::media::available() {
                        play |= play_button(ui, pic);
                    }
                }
            }
            let caption_rect =
                Rect::from_min_max(pos2(area.left(), area.bottom() - reserve), area.max);
            match content {
                Content::Image { width, height, .. } => {
                    ui.painter().text(
                        caption_rect.center(),
                        Align2::CENTER_CENTER,
                        format!("{width} × {height} px"),
                        FontId::proportional(11.0),
                        pal.weak,
                    );
                }
                Content::Pdf {
                    pages,
                    page,
                    path,
                    px,
                    ..
                } if *pages > 1 => {
                    let c = caption_rect.center();
                    ui.painter().text(
                        c,
                        Align2::CENTER_CENTER,
                        format!("{} / {}", page + 1, pages),
                        FontId::proportional(12.0),
                        pal.weak,
                    );
                    for (dx, s, target, enabled) in [
                        (-52.0, Sym::Back, page.saturating_sub(1), *page > 0),
                        (52.0, Sym::Forward, page + 1, page + 1 < *pages),
                    ] {
                        let br = Rect::from_center_size(c + vec2(dx, 0.0), Vec2::splat(20.0));
                        let resp = ui.interact(
                            br,
                            ui.scope_id()
                                .with(("pdf-page", s as u8 as i32 + if large { 100 } else { 0 })),
                            if enabled {
                                Sense::click()
                            } else {
                                Sense::hover()
                            },
                        );
                        if resp.hovered() && enabled {
                            ui.painter().circle_filled(br.center(), 10.0, pal.hover);
                        }
                        icons::sym(
                            ui.painter(),
                            br.shrink(4.0),
                            s,
                            if enabled {
                                ui.visuals().text_color()
                            } else {
                                pal.weak.gamma_multiply(0.4)
                            },
                        );
                        if resp.clicked() {
                            pdf_wanted = Some((path.clone(), target, *px));
                        }
                    }
                }
                _ => {}
            }
        }
        Some(Content::Text { text, truncated } | Content::Document { text, truncated }) => {
            let mono = matches!(app.preview.content, Some(Content::Text { .. }));
            let shown: String = if large {
                text.clone()
            } else {
                text.chars().take(20_000).collect()
            };
            let truncated = *truncated;
            egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .corner_radius(6)
                .inner_margin(8)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("pv-text", large))
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            let rt = if mono {
                                RichText::new(shown).monospace().size(12.0)
                            } else {
                                RichText::new(shown).size(13.0)
                            };
                            ui.add(egui::Label::new(rt).wrap());
                            if truncated {
                                ui.label(
                                    RichText::new(t("preview_truncated"))
                                        .small()
                                        .color(pal.weak),
                                );
                            }
                        });
                });
        }
        Some(Content::Table {
            sheets,
            rows,
            truncated,
        }) => {
            if sheets.len() > 1 {
                ui.label(
                    RichText::new(tf("sheets", sheets.join(", ")))
                        .small()
                        .color(pal.weak),
                );
            }
            let truncated = *truncated;
            egui::ScrollArea::both()
                .id_salt(("pv-table", large))
                .auto_shrink(false)
                .show(ui, |ui| {
                    egui::Grid::new(("table-grid", large))
                        .striped(true)
                        .spacing([10.0, 3.0])
                        .show(ui, |ui| {
                            for row in rows.iter().take(if large { 1000 } else { 200 }) {
                                for cell in row {
                                    let c: String = cell.chars().take(40).collect();
                                    ui.label(RichText::new(c).size(12.0));
                                }
                                ui.end_row();
                            }
                        });
                    if truncated {
                        ui.label(
                            RichText::new(t("preview_truncated"))
                                .small()
                                .color(pal.weak),
                        );
                    }
                });
        }
        Some(Content::Archive { items, count }) => {
            ui.label(RichText::new(tf("archive_items", count)).strong());
            let lines = archive_tree(items);
            let dark = app.dark;
            egui::ScrollArea::vertical()
                .id_salt(("pv-archive", large))
                .auto_shrink(false)
                .show_rows(ui, 20.0, lines.len(), |ui, range| {
                    for line in &lines[range] {
                        tree_line(ui, line, dark, pal.weak, pal.separator);
                    }
                });
        }
        Some(Content::Folder { items, folders }) => {
            let r = Rect::from_min_max(area.min, pos2(area.right(), area.bottom() - 24.0));
            big_icon(ui, r, |p, rr| {
                icons::folder(p, rr, Category::Folder.color())
            });
            ui.painter().text(
                pos2(area.center().x, area.bottom() - 12.0),
                Align2::CENTER_CENTER,
                format!("{} · {}", tf("n_items", items), tf("n_folders", folders)),
                FontId::proportional(12.0),
                pal.weak,
            );
        }
        Some(Content::None(key)) => {
            let galley = ui.painter().layout(
                t(key).to_owned(),
                FontId::proportional(12.0),
                pal.weak,
                area.width() - 16.0,
            );
            let text_h = galley.size().y + 8.0;
            let r = Rect::from_min_max(area.min, pos2(area.right(), area.bottom() - text_h));
            let cat = Category::of(&e.ext, e.is_dir());
            big_icon(ui, r, |p, rr| {
                icons::file_icon(p, rr, cat, &e.ext, app.dark)
            });
            ui.painter().galley(
                pos2(
                    area.center().x - galley.size().x / 2.0,
                    area.bottom() - text_h + 4.0,
                ),
                galley,
                pal.weak,
            );
            if super::player::playable(e) && crate::media::available() {
                play |= play_button(ui, r);
            }
        }
        Some(Content::Error(err)) => {
            ui.label(RichText::new(err).color(ui.visuals().error_fg_color));
        }
        None => {}
    }
    if let Some((path, page, px)) = pdf_wanted {
        app.request_pdf(path, page, px);
    }
    if play {
        let (list, index) = super::player::queue_for(app, e);
        super::player::start(app, list, index);
        if let Some(p) = app.player.as_mut() {
            p.fullscreen = fullscreen || large;
        }
    }
    if open_viewer {
        super::viewer::open(app, e);
    }
    if large && (play || open_viewer) {
        app.quick_look = None;
    }
}

/// A round play button in the middle of `r`.
fn play_button(ui: &mut Ui, r: Rect) -> bool {
    let br = Rect::from_center_size(r.center(), Vec2::splat(54.0));
    let resp = ui
        .interact(br, ui.scope_id().with("pv-play"), Sense::click())
        .on_hover_text(t("play"));
    let alpha = if resp.hovered() { 230 } else { 170 };
    ui.painter()
        .circle_filled(br.center(), 27.0, Color32::from_black_alpha(alpha));
    icons::sym(
        ui.painter(),
        br.shrink(15.0).translate(vec2(2.0, 0.0)),
        Sym::Play,
        Color32::WHITE,
    );
    resp.clicked()
}

/// One line of an archive's contents, shown as a tree: (depth, name, is a folder, size).
pub struct TreeLine {
    pub depth: usize,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// The last child of its folder (the guide stops here).
    pub last: bool,
}

/// An archive's flat list of paths as an indented tree, folders first (folders only implied by
/// the paths inside them appear too).
pub fn archive_tree(items: &[ArchiveItem]) -> Vec<TreeLine> {
    #[derive(Default)]
    struct Node {
        children: BTreeMap<String, Node>,
        is_dir: bool,
        size: u64,
    }
    let mut root = Node::default();
    for it in items {
        let parts: Vec<&str> = it
            .name
            .split(['/', '\\'])
            .filter(|p| !p.is_empty() && *p != ".")
            .collect();
        let mut node = &mut root;
        for (i, part) in parts.iter().enumerate() {
            node = node.children.entry((*part).to_owned()).or_default();
            if i + 1 < parts.len() {
                node.is_dir = true;
            }
        }
        if !parts.is_empty() {
            node.is_dir |= it.is_dir;
            if !it.is_dir {
                node.size = it.size;
            }
        }
    }
    fn walk(node: &Node, depth: usize, out: &mut Vec<TreeLine>) {
        let mut kids: Vec<(&String, &Node)> = node.children.iter().collect();
        kids.sort_by(|a, b| {
            b.1.is_dir
                .cmp(&a.1.is_dir)
                .then_with(|| super::pane::natural_cmp(a.0, b.0))
        });
        let n = kids.len();
        for (i, (name, child)) in kids.into_iter().enumerate() {
            if out.len() >= 5000 {
                return;
            }
            out.push(TreeLine {
                depth,
                name: name.clone(),
                is_dir: child.is_dir,
                size: child.size,
                last: i + 1 == n,
            });
            walk(child, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(&root, 0, &mut out);
    out
}

fn tree_line(ui: &mut Ui, line: &TreeLine, dark: bool, weak: Color32, guide: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let indent = 14.0;
    let x0 = r.left() + 4.0 + line.depth as f32 * indent;
    // a guide from the parent folder
    if line.depth > 0 {
        let gx = x0 - indent / 2.0 - 1.0;
        let bottom = if line.last { r.center().y } else { r.bottom() };
        ui.painter().line_segment(
            [pos2(gx, r.top()), pos2(gx, bottom)],
            egui::Stroke::new(1.0, guide),
        );
        ui.painter().line_segment(
            [pos2(gx, r.center().y), pos2(x0 - 2.0, r.center().y)],
            egui::Stroke::new(1.0, guide),
        );
    }
    let ir = Rect::from_min_size(pos2(x0, r.center().y - 7.0), Vec2::splat(14.0));
    if line.is_dir {
        icons::folder(ui.painter(), ir, Category::Folder.color());
    } else {
        let ext = crate::vfs::extension(&line.name);
        icons::file_icon(ui.painter(), ir, Category::of(&ext, false), &ext, dark);
    }
    let text_x = ir.right() + 6.0;
    let size = if line.is_dir {
        String::new()
    } else {
        fileinfo::format_size(line.size)
    };
    let size_w = if size.is_empty() { 0.0 } else { 60.0 };
    let g = ui.painter().layout_no_wrap(
        line.name.clone(),
        FontId::proportional(12.0),
        ui.visuals().text_color(),
    );
    let clip = Rect::from_min_max(pos2(text_x, r.top()), pos2(r.right() - size_w, r.bottom()));
    ui.painter()
        .with_clip_rect(clip.intersect(ui.clip_rect()))
        .galley(
            pos2(text_x, r.center().y - g.size().y / 2.0),
            g,
            ui.visuals().text_color(),
        );
    if !size.is_empty() {
        ui.painter().text(
            pos2(r.right() - 4.0, r.center().y),
            Align2::RIGHT_CENTER,
            size,
            FontId::proportional(11.0),
            weak,
        );
    }
}

fn details(app: &App, ui: &mut Ui, e: &Entry) {
    let pal = theme::palette(app.dark);
    egui::Grid::new("preview-details")
        .num_columns(2)
        .spacing([10.0, 3.0])
        .show(ui, |ui| {
            let mut line = |k: &str, v: String| {
                ui.label(RichText::new(t(k)).color(pal.weak).size(12.5));
                ui.add(egui::Label::new(RichText::new(v).size(12.5)).wrap());
                ui.end_row();
            };
            // What the preview found inside (tags of a song, a video's resolution…).
            if let Some(Content::Media { info, .. }) = &app.preview.content {
                for (k, v) in info {
                    line(k, v.clone());
                }
            }
            line("col_modified", fileinfo::format_date_long(e.modified));
            if e.created.is_some() {
                line("created", fileinfo::format_date_long(e.created));
            }
            if let Some(tr) = &e.trash {
                line("original_location", tr.original.display().to_string());
                line("deleted_on", fileinfo::format_date_long(tr.deleted));
            } else if let Some(parent) = e.location.parent() {
                line("where", parent.address(&app.settings.servers));
            }
            let mut attrs = Vec::new();
            if e.readonly {
                attrs.push(t("readonly"));
            }
            if e.hidden {
                attrs.push(t("hidden"));
            }
            if e.symlink {
                attrs.push(t("symlink"));
            }
            if !attrs.is_empty() {
                line("col_status", attrs.join(", "));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_as_trees() {
        let item = |name: &str, is_dir: bool| ArchiveItem {
            name: name.into(),
            size: 1,
            is_dir,
        };
        let lines = archive_tree(&[
            item("b.txt", false),
            item("docs/a/x.txt", false),
            item("docs/", true),
            item("docs/z.txt", false),
            item("a.txt", false),
        ]);
        let shown: Vec<(usize, &str, bool)> = lines
            .iter()
            .map(|l| (l.depth, l.name.as_str(), l.is_dir))
            .collect();
        assert_eq!(
            shown,
            vec![
                (0, "docs", true),
                (1, "a", true),
                (2, "x.txt", false),
                (1, "z.txt", false),
                (0, "a.txt", false),
                (0, "b.txt", false)
            ]
        );
    }
}
