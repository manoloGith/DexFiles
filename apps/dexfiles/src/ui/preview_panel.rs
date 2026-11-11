//! The preview panel on the right: what is selected, its details, and its content (an image,
//! text, a document's text, a table, an archive's list).

use egui::{RichText, Ui, Vec2};

use super::app::{Action, App};
use super::theme;
use crate::fileinfo::{self, Category};
use crate::i18n::{t, tf};
use crate::preview::Content;
use crate::vfs::Entry;

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.active_pane;
    let sel = app.tab(p).selected_entries();
    let pal = theme::palette(app.dark);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| match sel.len() {
            0 => {
                let tab = app.tab(p);
                let title = tab.title();
                let n = tab.visible.len();
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(96.0), egui::Sense::hover());
                    super::icons::folder(ui.painter(), r, Category::Folder.color());
                    ui.add_space(8.0);
                    ui.label(RichText::new(title).strong().size(16.0));
                    ui.label(RichText::new(tf("n_items", n)).color(pal.weak));
                });
            }
            1 => {
                let e = sel[0].clone();
                header(app, ui, &e);
                ui.add_space(8.0);
                content(app, ui, &e, false);
                ui.add_space(10.0);
                details(app, ui, &e);
            }
            n => {
                let size: u64 = sel.iter().filter(|e| !e.is_dir()).map(|e| e.size).sum();
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(96.0), egui::Sense::hover());
                    for (i, e) in sel.iter().take(3).enumerate().rev() {
                        let off = i as f32 * 8.0;
                        let rr = egui::Rect::from_min_size(
                            r.min + egui::vec2(off, -off + 16.0),
                            Vec2::splat(80.0),
                        );
                        super::icons::file_icon(
                            ui.painter(),
                            rr,
                            Category::of(&e.ext, e.is_dir()),
                            &e.ext,
                            app.dark,
                        );
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new(tf("n_selected", n)).strong().size(16.0));
                    if size > 0 {
                        ui.label(RichText::new(fileinfo::format_size(size)).color(pal.weak));
                    }
                });
            }
        });
}

fn header(app: &mut App, ui: &mut Ui, e: &Entry) {
    let pal = theme::palette(app.dark);
    ui.vertical_centered(|ui| {
        let has_image = matches!(app.preview.content, Some(Content::Image { .. }))
            && app.preview.texture.is_some();
        if !has_image {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(96.0), egui::Sense::hover());
            super::view::entry_icon(app, ui, r, e, true);
        }
        ui.add(egui::Label::new(RichText::new(&e.name).strong().size(15.0)).wrap());
        let mut sub = fileinfo::kind_label(&e.ext, e.is_dir());
        if !e.is_dir() {
            sub.push_str(&format!(" · {}", fileinfo::format_size(e.size)));
        }
        ui.label(RichText::new(sub).color(pal.weak));
        ui.horizontal(|ui| {
            let w = ui.available_width();
            ui.add_space((w - 200.0).max(0.0) / 2.0);
            if ui.button(t("open")).clicked() {
                let p = app.active_pane;
                app.actions.push(Action::Open(p, e.clone()));
            }
            if !e.is_dir() && ui.button(t("share")).clicked() {
                app.actions
                    .push(Action::Share(vec![e.location.clone()], false));
            }
            if ui.button(t("properties")).clicked() {
                app.actions.push(Action::Properties(e.clone()));
            }
        });
    });
}

/// The content preview (also the quick look's, `large`).
pub fn content(app: &mut App, ui: &mut Ui, e: &Entry, large: bool) {
    let pal = theme::palette(app.dark);
    if app.preview.loading || app.preview.content.is_none() {
        ui.vertical_centered(|ui| ui.add(egui::Spinner::new().size(20.0)));
        return;
    }
    let width = ui.available_width();
    match &app.preview.content {
        Some(Content::Image {
            width: w,
            height: h,
            ..
        }) => {
            if let Some(tex) = &app.preview.texture {
                let size = tex.size_vec2();
                let max_h = if large {
                    ui.available_height().max(300.0)
                } else {
                    420.0
                };
                let scale = (width / size.x)
                    .min(max_h / size.y)
                    .min(if large { 4.0 } else { 1.0 });
                ui.vertical_centered(|ui| {
                    ui.add(
                        egui::Image::from_texture(tex)
                            .fit_to_exact_size(size * scale)
                            .corner_radius(6),
                    );
                    ui.label(
                        RichText::new(format!("{w} × {h} px"))
                            .color(pal.weak)
                            .small(),
                    );
                });
            }
        }
        Some(Content::Text { text, truncated }) | Some(Content::Document { text, truncated }) => {
            let mono = matches!(app.preview.content, Some(Content::Text { .. }));
            let shown: String = if large {
                text.clone()
            } else {
                text.chars().take(8000).collect()
            };
            egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .corner_radius(6)
                .inner_margin(8)
                .show(ui, |ui| {
                    let rt = if mono {
                        RichText::new(shown).monospace().size(12.0)
                    } else {
                        RichText::new(shown).size(13.0)
                    };
                    ui.add(egui::Label::new(rt).wrap());
                });
            if *truncated {
                ui.label(
                    RichText::new(t("preview_truncated"))
                        .small()
                        .color(pal.weak),
                );
            }
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
            egui::ScrollArea::horizontal()
                .id_salt(("table", large))
                .show(ui, |ui| {
                    egui::Grid::new(("table-grid", large))
                        .striped(true)
                        .spacing([10.0, 3.0])
                        .show(ui, |ui| {
                            for row in rows.iter().take(if large { 200 } else { 40 }) {
                                for cell in row {
                                    let c: String = cell.chars().take(40).collect();
                                    ui.label(RichText::new(c).size(12.0));
                                }
                                ui.end_row();
                            }
                        });
                });
            if *truncated {
                ui.label(
                    RichText::new(t("preview_truncated"))
                        .small()
                        .color(pal.weak),
                );
            }
        }
        Some(Content::Archive { items, count }) => {
            ui.label(RichText::new(tf("archive_items", count)).strong());
            for it in items.iter().take(if large { 500 } else { 60 }) {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                    let ext = crate::vfs::extension(&it.name);
                    super::icons::file_icon(
                        ui.painter(),
                        r,
                        Category::of(&ext, it.is_dir),
                        &ext,
                        app.dark,
                    );
                    ui.label(RichText::new(&it.name).size(12.0));
                    if !it.is_dir {
                        ui.label(
                            RichText::new(fileinfo::format_size(it.size))
                                .size(11.0)
                                .color(pal.weak),
                        );
                    }
                });
            }
        }
        Some(Content::Folder { items, folders }) => {
            ui.label(
                RichText::new(format!(
                    "{} · {}",
                    tf("n_items", items),
                    tf("n_folders", folders)
                ))
                .color(pal.weak),
            );
        }
        Some(Content::None(key)) => {
            ui.vertical_centered(|ui| ui.label(RichText::new(t(key)).color(pal.weak)));
        }
        Some(Content::Error(err)) => {
            ui.label(RichText::new(err).color(ui.visuals().error_fg_color));
        }
        None => {}
    }
    let _ = e;
}

fn details(app: &App, ui: &mut Ui, e: &Entry) {
    let pal = theme::palette(app.dark);
    ui.label(RichText::new(t("information")).strong());
    ui.separator();
    egui::Grid::new("preview-details")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            let mut line = |k: &str, v: String| {
                ui.label(RichText::new(t(k)).color(pal.weak));
                ui.add(egui::Label::new(v).wrap());
                ui.end_row();
            };
            line("col_kind", fileinfo::kind_label(&e.ext, e.is_dir()));
            if !e.is_dir() {
                line(
                    "col_size",
                    format!("{} ({} B)", fileinfo::format_size(e.size), e.size),
                );
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
