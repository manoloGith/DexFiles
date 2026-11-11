//! The window's layout, and the files of a pane in each view (large, medium and small icons,
//! list, details, content), with selection by mouse, keyboard and rubber band, drag and drop,
//! and context menus.

use egui::{
    Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

use super::app::{Action, App, DragPayload, DropTarget};
use super::icons::{self, Sym};
use super::theme;
use crate::archive;
use crate::fileinfo::{self, Category};
use crate::i18n::{t, tf};
use crate::settings::{SortKey, ViewMode};
use crate::vfs::{Entry, Location};

pub fn window(app: &mut App, ui: &mut Ui) {
    let pal = theme::palette(app.dark);
    if !app.storage_ok {
        egui::Panel::top("storage")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(0xff, 0xf4, 0xd6))
                    .inner_margin(8),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(Color32::from_rgb(0x6b, 0x4a, 0x00), t("storage_needed"));
                    if ui.button(t("grant_access")).clicked() {
                        crate::platform::request_storage_access();
                    }
                });
            });
    }
    egui::Panel::top("toolbar")
        .frame(
            egui::Frame::new()
                .fill(pal.toolbar)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .stroke(Stroke::new(1.0, pal.separator)),
        )
        .show(ui, |ui| super::toolbar::show(app, ui));
    egui::Panel::bottom("status")
        .frame(
            egui::Frame::new()
                .fill(pal.toolbar)
                .inner_margin(egui::Margin::symmetric(10, 3)),
        )
        .show(ui, |ui| status_bar(app, ui));
    if app.settings.show_sidebar {
        let width = if app.narrow { 190.0 } else { 230.0 };
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(width)
            .size_range(150.0..=420.0)
            .frame(
                egui::Frame::new()
                    .fill(pal.sidebar)
                    .inner_margin(egui::Margin::symmetric(6, 8)),
            )
            .show(ui, |ui| super::sidebar::show(app, ui));
    }
    // On a narrow window the player is a strip at the bottom.
    if app.narrow && app.player.is_some() {
        egui::Panel::bottom("player-strip")
            .exact_size(150.0)
            .frame(egui::Frame::new().fill(pal.sidebar).inner_margin(8))
            .show(ui, |ui| super::player::show(app, ui));
    }
    if (app.settings.show_preview || app.player.is_some()) && !app.narrow {
        egui::Panel::right("preview")
            .resizable(true)
            .default_size(300.0)
            .size_range(200.0..=700.0)
            .frame(
                egui::Frame::new()
                    .fill(pal.sidebar)
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ui, |ui| super::preview_panel::show(app, ui));
    }
    egui::CentralPanel::no_frame().show(ui, |ui| {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0, pal.content);
        if app.settings.dual_pane && !app.narrow {
            let mid = rect.center().x;
            let left = Rect::from_min_max(rect.min, pos2(mid - 1.0, rect.max.y));
            let right = Rect::from_min_max(pos2(mid + 1.0, rect.min.y), rect.max);
            ui.painter().line_segment(
                [pos2(mid, rect.top()), pos2(mid, rect.bottom())],
                Stroke::new(1.0, pal.separator),
            );
            for (p, r) in [(0usize, left), (1usize, right)] {
                let mut child =
                    ui.new_child(egui::UiBuilder::new().max_rect(r).id_salt(("pane", p)));
                child.set_clip_rect(r);
                pane(app, &mut child, p, true);
            }
        } else {
            let p = if app.narrow { 0 } else { app.active_pane };
            app.active_pane = p;
            pane(app, ui, p, false);
        }
    });
    super::dialogs::show(app, ui.ctx());
    super::quicklook::show(app, ui.ctx());
    super::viewer::show(app, ui.ctx());
    super::player::fullscreen(app, ui.ctx());
    drag_ghost(app, ui.ctx());
}

fn status_bar(app: &mut App, ui: &mut Ui) {
    let p = app.active_pane;
    ui.horizontal(|ui| {
        let tab = app.tab(p);
        let n = tab.visible.len();
        let sel = tab.selected.len();
        let text = if sel == 0 {
            tf("n_items", n)
        } else {
            let size: u64 = tab
                .selected_entries()
                .iter()
                .map(|e| {
                    if e.is_dir() {
                        app.dir_size(e).map(|s| s.bytes).unwrap_or(0)
                    } else {
                        e.size
                    }
                })
                .sum();
            let mut s = format!("{} {}", tf("n_selected", sel), tf("of_n", n));
            if size > 0 {
                s.push_str(&format!(" · {}", fileinfo::format_size(size)));
            }
            s
        };
        ui.small(text);
        if let Some((m, _)) = &app.message {
            ui.separator();
            ui.small(m.clone());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(space) = space_for(app, tab_location(app, p)) {
                ui.small(format!(
                    "{} {}",
                    fileinfo::format_size(space.0),
                    tf("free_of", fileinfo::format_size(space.1))
                ));
            }
            jobs_indicator(app, ui);
        });
    });
}

fn tab_location(app: &App, p: usize) -> Location {
    app.tab(p).location().clone()
}

pub fn space_for(app: &App, l: Location) -> Option<(u64, u64)> {
    match &l {
        Location::Local(p) => app
            .space
            .iter()
            .filter_map(|(k, v)| match k {
                Location::Local(root) if p.starts_with(root) => Some((root.as_os_str().len(), *v)),
                _ => None,
            })
            .max_by_key(|(n, _)| *n)
            .map(|(_, v)| v),
        Location::Remote { server, .. } => app
            .space
            .get(&Location::Remote {
                server: *server,
                path: "/".into(),
            })
            .copied(),
        Location::Trash => None,
        Location::Archive { archive, .. } => space_for(app, (**archive).clone()),
    }
}

fn jobs_indicator(app: &mut App, ui: &mut Ui) {
    let visible: Vec<usize> = app
        .jobs
        .iter()
        .enumerate()
        .filter(|(_, j)| j.visible)
        .map(|(i, _)| i)
        .collect();
    if visible.is_empty() {
        return;
    }
    let (mut done, mut total) = (0u64, 0u64);
    for &i in &visible {
        let s = app.jobs[i].state.status();
        done += s.done_bytes;
        total += s.total_bytes;
    }
    let frac = if total > 0 {
        done as f32 / total as f32
    } else {
        0.0
    };
    let label = tf("jobs_running", visible.len());
    let resp = ui.add(
        egui::ProgressBar::new(frac)
            .desired_width(160.0)
            .text(label)
            .corner_radius(4),
    );
    let resp = resp.interact(Sense::click());
    if resp.clicked() {
        app.show_jobs = !app.show_jobs;
    }
    ui.add(egui::Spinner::new().size(12.0));
    if app.show_jobs {
        egui::Window::new(t("operations"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::RIGHT_BOTTOM, vec2(-12.0, -36.0))
            .default_width(340.0)
            .show(ui.ctx(), |ui| {
                for &i in &visible {
                    let job = &app.jobs[i];
                    let s = job.state.status();
                    ui.label(egui::RichText::new(job_title(&job.kind)).strong());
                    let frac = if s.total_bytes > 0 {
                        s.done_bytes as f32 / s.total_bytes as f32
                    } else if s.total_items > 0 {
                        s.done_items as f32 / s.total_items as f32
                    } else {
                        0.0
                    };
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .desired_width(250.0)
                                .show_percentage(),
                        );
                        if icons::sym_button(ui, Sym::Close, false, t("cancel")).clicked() {
                            job.state.cancel();
                        }
                    });
                    let detail = if s.total_bytes > 0 {
                        format!(
                            "{} / {} · {}",
                            fileinfo::format_size(s.done_bytes),
                            fileinfo::format_size(s.total_bytes),
                            s.current
                        )
                    } else {
                        s.current.clone()
                    };
                    ui.small(detail);
                    ui.separator();
                }
                if ui.button(t("close")).clicked() {
                    app.show_jobs = false;
                }
            });
    }
}

pub fn job_title(kind: &crate::ops::JobKind) -> String {
    use crate::ops::{JobKind, Mode};
    match kind {
        JobKind::Transfer {
            sources,
            dest,
            mode,
        } => {
            let key = if *mode == Mode::Copy {
                "job_copy"
            } else {
                "job_move"
            };
            format!(
                "{} · {}",
                tf(key, sources.len()),
                super::pane::location_title(dest)
            )
        }
        JobKind::Delete { items } => tf("job_delete", items.len()),
        JobKind::Trash { items } => tf("job_trash", items.len()),
        JobKind::Restore { items } => tf("job_restore", items.len()),
        JobKind::Purge { items } => tf("job_delete", items.len()),
        JobKind::EmptyTrash => t("empty_trash").to_owned(),
        JobKind::Compress { dest, .. } => tf("job_compress", dest.name()),
        JobKind::Extract { archive, .. } => tf("job_extract", archive.name()),
        JobKind::Mkdir { location } | JobKind::NewFile { location } => location.name(),
        JobKind::Rename { from, .. } => from.name(),
    }
}

/// One pane: tabs, (in two-pane mode) its own path, the search bar, and the files.
fn pane(app: &mut App, ui: &mut Ui, p: usize, dual: bool) {
    let pal = theme::palette(app.dark);
    let full = ui.max_rect();
    let active = app.active_pane == p;
    // A click anywhere in the pane makes it the active one.
    if ui.input(|i| i.pointer.any_pressed())
        && ui
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|pos| full.contains(pos))
    {
        app.active_pane = p;
    }
    if dual || app.panes[p].tabs.len() > 1 {
        tab_strip(app, ui, p, dual, active);
    }
    if dual {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let title = app.tab(p).title();
            let color = if active { theme::ACCENT } else { pal.weak };
            ui.label(egui::RichText::new(title).strong().color(color));
            ui.small(
                egui::RichText::new(app.tab(p).location().address(&app.settings.servers))
                    .color(pal.weak),
            );
        });
    }
    search_bar(app, ui, p);
    let rect = ui.available_rect_before_wrap();
    let here = app.tab(p).location().clone();
    if here != Location::Trash {
        app.drop_targets.push(DropTarget {
            rect,
            dest: here.clone(),
            priority: 1,
            trash: false,
        });
    } else {
        app.drop_targets.push(DropTarget {
            rect,
            dest: here.clone(),
            priority: 1,
            trash: true,
        });
    }
    // Highlight a pane a drag is over.
    if egui::DragAndDrop::has_payload_of_type::<DragPayload>(ui.ctx())
        && ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| rect.contains(pos))
    {
        ui.painter().rect_stroke(
            rect.shrink(2.0),
            6,
            Stroke::new(2.0, theme::ACCENT),
            StrokeKind::Inside,
        );
    }
    let tab = app.tab(p);
    if tab.loading && tab.visible.is_empty() {
        ui.centered_and_justified(|ui| ui.add(egui::Spinner::new().size(28.0)));
        return;
    }
    if let Some(err) = tab.error.clone() {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.heading(t("cant_open_folder"));
            ui.label(egui::RichText::new(&err).color(pal.weak));
            ui.add_space(8.0);
            if ui.button(t("retry")).clicked() {
                if let Location::Remote { server, .. } = &here {
                    crate::vfs::servers::disconnect(*server);
                }
                app.actions.push(Action::Refresh(p));
            }
            // An SFTP server whose key changed (reinstalled, or someone in between): only the
            // user can say it is fine.
            #[cfg(feature = "network")]
            if err.contains(crate::vfs::net::sftp::HOST_KEY_CHANGED)
                && let Location::Remote { server, .. } = &here
                && let Some(s) = crate::vfs::servers::server(*server)
            {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(t("host_key_changed_text"))
                        .color(ui.visuals().warn_fg_color),
                );
                if ui.button(t("trust_new_key")).clicked() {
                    crate::vfs::net::sftp::forget_host(&s.host, s.port);
                    crate::vfs::servers::disconnect(*server);
                    app.actions.push(Action::Refresh(p));
                }
            }
        });
        background_menu_area(app, ui, p, rect);
        return;
    }
    if tab.visible.is_empty() {
        let key = if tab.search.is_some() {
            if tab.search.as_ref().is_some_and(|s| s.is_done()) {
                "no_results"
            } else {
                "searching"
            }
        } else if !tab.filter.is_empty() {
            "no_results"
        } else {
            "empty_folder"
        };
        let r = ui.available_rect_before_wrap();
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            t(key),
            FontId::proportional(15.0),
            pal.weak,
        );
        background_menu_area(app, ui, p, r);
        return;
    }
    match app.view_mode(p) {
        ViewMode::Details => details(app, ui, p),
        ViewMode::List => rows(app, ui, p, false),
        ViewMode::Content => rows(app, ui, p, true),
        mode => grid(app, ui, p, mode),
    }
}

fn tab_strip(app: &mut App, ui: &mut Ui, p: usize, _dual: bool, active_pane: bool) {
    let pal = theme::palette(app.dark);
    let n = app.panes[p].tabs.len();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(4.0);
        let avail = ui.available_width() - 34.0;
        let w = (avail / n as f32).clamp(80.0, 220.0);
        for i in 0..n {
            let selected = app.panes[p].active == i;
            let title = app.panes[p].tabs[i].title();
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
            let fill = if selected {
                pal.content
            } else if resp.hovered() {
                pal.hover
            } else {
                Color32::TRANSPARENT
            };
            ui.painter()
                .rect_filled(rect.shrink2(vec2(0.0, 2.0)), CornerRadius::same(6), fill);
            if selected && active_pane {
                ui.painter().line_segment(
                    [
                        pos2(rect.left() + 8.0, rect.bottom() - 2.0),
                        pos2(rect.right() - 8.0, rect.bottom() - 2.0),
                    ],
                    Stroke::new(2.0, theme::ACCENT),
                );
            }
            let text_rect = rect.shrink2(vec2(10.0, 0.0));
            let galley = ui.painter().layout(
                title.clone(),
                FontId::proportional(13.0),
                ui.visuals().text_color(),
                text_rect.width() - 16.0,
            );
            ui.painter().galley(
                pos2(text_rect.left(), rect.center().y - galley.size().y / 2.0),
                galley,
                ui.visuals().text_color(),
            );
            // close button
            let close = Rect::from_center_size(
                pos2(rect.right() - 14.0, rect.center().y),
                Vec2::splat(16.0),
            );
            let close_resp = ui.interact(
                close,
                ui.scope_id().with(("close-tab", p, i)),
                Sense::click(),
            );
            if (resp.hovered() || close_resp.hovered()) && n > 1 {
                icons::sym(
                    ui.painter(),
                    close,
                    Sym::Close,
                    if close_resp.hovered() {
                        theme::ACCENT
                    } else {
                        pal.weak
                    },
                );
            }
            if close_resp.clicked() || resp.middle_clicked() {
                app.actions.push(Action::CloseTab(p, i));
            } else if resp.clicked() {
                app.actions.push(Action::SelectTab(p, i));
            }
            // Dropping onto a tab moves into its folder.
            let dest = app.panes[p].tabs[i].location().clone();
            app.drop_targets.push(DropTarget {
                rect,
                dest,
                priority: 3,
                trash: false,
            });
            resp.on_hover_text(title);
        }
        if icons::sym_button(ui, Sym::Plus, false, t("new_tab")).clicked() {
            app.actions.push(Action::NewTab(p));
        }
    });
}

fn search_bar(app: &mut App, ui: &mut Ui, p: usize) {
    let pal = theme::palette(app.dark);
    let Some(search) = &app.tab(p).search else {
        return;
    };
    let done = search.is_done();
    let scanned = search.scanned.load(std::sync::atomic::Ordering::Relaxed);
    let root = super::pane::location_title(&search.root);
    let found = app.tab(p).visible.len();
    egui::Frame::new()
        .fill(pal.sidebar)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if !done {
                    ui.add(egui::Spinner::new().size(14.0));
                }
                ui.label(format!(
                    "{} · {} · {}",
                    tf("results_in", root),
                    tf("n_found", found),
                    tf("n_scanned", scanned)
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t("close_search")).clicked() {
                        app.actions.push(Action::ClearSearch(p));
                    }
                    if !done
                        && ui.button(t("stop")).clicked()
                        && let Some(s) = &app.tab(p).search
                    {
                        s.cancel();
                    }
                });
            });
        });
}

/// Draws the icon of an entry: its thumbnail when there is one.
pub fn entry_icon(app: &mut App, ui: &Ui, rect: Rect, e: &Entry, allow_thumb: bool) {
    let cat = Category::of(&e.ext, e.is_dir());
    let thumb = allow_thumb
        && app.settings.thumbnails
        && (matches!(cat, Category::Image | Category::Pdf)
            || (matches!(cat, Category::Audio | Category::Video) && e.location.is_local()))
        && (e.location.is_local() || app.settings.remote_thumbnails)
        && rect.width() >= 30.0;
    let painter = ui.painter();
    if thumb && let Some(tex) = app.thumbs.get(e) {
        let size = tex.size_vec2();
        let scale = (rect.width() / size.x).min(rect.height() / size.y);
        let r = Rect::from_center_size(rect.center(), size * scale);
        painter.rect_filled(
            r.expand(1.0),
            CornerRadius::same(3),
            theme::palette(app.dark).separator,
        );
        egui::Image::from_texture(&tex)
            .corner_radius(3)
            .paint_at(ui, r);
    } else {
        icons::file_icon(painter, rect, cat, &e.ext, app.dark);
    }
    if e.symlink {
        let b = Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - rect.height() * 0.3),
            Vec2::splat(rect.height() * 0.3),
        );
        painter.circle_filled(b.center(), b.width() / 2.0, Color32::WHITE);
        icons::sym(painter, b.shrink(1.0), Sym::Forward, Color32::from_gray(60));
    }
}

fn display_name(app: &App, e: &Entry) -> String {
    if !app.settings.show_extensions && !e.is_dir() && !e.ext.is_empty() {
        crate::vfs::stem(&e.name).to_owned()
    } else {
        e.name.clone()
    }
}

/// Clicks, drags, drops and the context menu of one item.
fn item_interact(app: &mut App, ui: &mut Ui, p: usize, i: usize, resp: &egui::Response, e: &Entry) {
    let m = ui.input(|inp| inp.modifiers);
    if resp.clicked() {
        app.active_pane = p;
        let tab = app.tab_mut(p);
        if m.command {
            tab.toggle(i);
        } else if m.shift {
            tab.select_range(i, false);
        } else {
            tab.select_only(i);
        }
    }
    if resp.double_clicked() {
        app.actions.push(Action::Open(p, e.clone()));
    }
    if resp.middle_clicked() && e.is_dir() {
        app.actions.push(Action::OpenInNewTab(e.location.clone()));
    }
    if resp.secondary_clicked() || resp.long_touched() {
        app.active_pane = p;
        if !app.tab(p).selected.contains(&e.location) {
            app.tab_mut(p).select_only(i);
        }
    }
    if resp.drag_started() {
        if !app.tab(p).selected.contains(&e.location) {
            app.tab_mut(p).select_only(i);
        }
        let items = app.tab(p).selected_locations();
        resp.dnd_set_drag_payload(DragPayload { items, pane: p });
    }
    if e.is_dir() && e.trash.is_none() {
        app.drop_targets.push(DropTarget {
            rect: resp.rect,
            dest: e.location.clone(),
            priority: 2,
            trash: false,
        });
        if egui::DragAndDrop::payload::<DragPayload>(ui.ctx())
            .is_some_and(|d| !d.items.contains(&e.location))
            && resp.contains_pointer()
        {
            ui.painter().rect_filled(
                resp.rect,
                CornerRadius::same(6),
                theme::palette(app.dark).drop,
            );
        }
    }
    resp.context_menu(|ui| item_menu(app, ui, p, e));
}

fn item_menu(app: &mut App, ui: &mut Ui, p: usize, e: &Entry) {
    ui.set_min_width(220.0);
    let sel = {
        let s = app.selection(p);
        if s.is_empty() {
            vec![e.location.clone()]
        } else {
            s
        }
    };
    let many = sel.len() > 1;
    if e.trash.is_some() {
        if ui.button(t("restore")).clicked() {
            let entries = app.tab(p).selected_entries();
            app.actions.push(Action::Restore(entries));
        }
        if ui.button(t("delete_forever")).clicked() {
            let entries = app.tab(p).selected_entries();
            app.actions.push(Action::Purge(entries));
        }
        ui.separator();
        if ui.button(t("properties")).clicked() {
            app.actions.push(Action::Properties(e.clone()));
        }
        return;
    }
    if ui.button(t("open")).clicked() {
        app.actions.push(Action::Open(p, e.clone()));
    }
    if super::player::playable(e) && crate::media::available() && ui.button(t("play")).clicked() {
        app.actions.push(Action::Play(e.clone()));
    }
    if e.is_dir() {
        if ui.button(t("open_new_tab")).clicked() {
            app.actions.push(Action::OpenInNewTab(e.location.clone()));
        }
        if ui.button(t("open_other_pane")).clicked() {
            app.actions
                .push(Action::OpenInOtherPane(e.location.clone()));
        }
    } else if ui.button(t("open_with")).clicked() {
        app.actions.push(Action::OpenWith(e.location.clone()));
    }
    if search_active(app, p)
        && let Some(parent) = e.location.parent()
        && ui.button(t("show_in_folder")).clicked()
    {
        app.actions.push(Action::ClearSearch(p));
        app.actions.push(Action::Navigate(p, parent));
        app.actions.push(Action::Navigate(p, e.location.clone()));
    }
    ui.separator();
    if ui.button(t("copy")).clicked() {
        app.actions.push(Action::Copy(sel.clone()));
    }
    if ui.button(t("cut")).clicked() {
        app.actions.push(Action::Cut(sel.clone()));
    }
    if e.is_dir() && app.clip.is_some() && ui.button(t("paste_into")).clicked() {
        app.actions.push(Action::Paste(e.location.clone()));
    }
    if ui.button(t("duplicate")).clicked()
        && let Some(parent) = e.location.parent()
    {
        app.actions.push(Action::Transfer {
            sources: sel.clone(),
            dest: parent,
            mode: crate::ops::Mode::Copy,
        });
    }
    if app.settings.dual_pane {
        let other = app.tab(1 - p).location().clone();
        if other != Location::Trash {
            if ui.button(t("copy_to_other")).clicked() {
                app.actions.push(Action::Transfer {
                    sources: sel.clone(),
                    dest: other.clone(),
                    mode: crate::ops::Mode::Copy,
                });
            }
            if ui.button(t("move_to_other")).clicked() {
                app.actions.push(Action::Transfer {
                    sources: sel.clone(),
                    dest: other,
                    mode: crate::ops::Mode::Move,
                });
            }
        }
    }
    if !many && ui.button(t("rename")).clicked() {
        app.actions.push(Action::Rename(e.clone()));
    }
    ui.separator();
    ui.menu_button(t("compress"), |ui| {
        if ui.button(t("compress_options")).clicked() {
            app.actions.push(Action::AskCompress(sel.clone()));
        }
        ui.separator();
        for f in [
            archive::ArchiveFormat::Zip,
            archive::ArchiveFormat::SevenZ,
            archive::ArchiveFormat::TarGz,
        ] {
            if ui
                .button(format!("{} .{}", t("compress_as"), f.extension()))
                .clicked()
                && let Some(parent) = e.location.parent()
            {
                let base = if many {
                    t("archive_name").to_owned()
                } else {
                    crate::vfs::stem(&e.name).to_owned()
                };
                let name = format!("{base}.{}", f.extension());
                let name = crate::ops::unique_name(&name, |n| {
                    parent.as_local().is_some_and(|d| d.join(n).exists())
                });
                if sel.iter().all(Location::is_local) {
                    app.actions.push(Action::CompressNow {
                        sources: sel.clone(),
                        dest: parent.join(&name),
                        format: f,
                        password: None,
                    });
                } else {
                    app.say(t("compress_local_only"));
                }
            }
        }
    });
    if !e.is_dir() && archive::is_archive(&e.ext) {
        if ui.button(t("extract_here")).clicked() {
            app.actions.push(Action::Extract {
                archive: e.location.clone(),
                into_folder: false,
            });
        }
        if ui
            .button(tf("extract_to", archive::folder_name(&e.name)))
            .clicked()
        {
            app.actions.push(Action::Extract {
                archive: e.location.clone(),
                into_folder: true,
            });
        }
    }
    ui.separator();
    if ui.button(t("share")).clicked() {
        app.actions.push(Action::Share(sel.clone(), false));
    }
    if ui.button(t("send_email")).clicked() {
        app.actions.push(Action::Share(sel.clone(), true));
    }
    if ui.button(t("copy_path")).clicked() {
        app.actions.push(Action::CopyPath(sel.clone()));
    }
    if e.is_dir() && ui.button(t("add_favorite")).clicked() {
        app.actions.push(Action::Pin(e.location.clone()));
    }
    ui.separator();
    if e.location.is_local() && ui.button(t("move_to_trash")).clicked() {
        app.actions.push(Action::Trash(sel.clone()));
    }
    if ui.button(t("delete_forever")).clicked() {
        app.actions.push(Action::AskDelete(sel.clone()));
    }
    ui.separator();
    if ui.button(t("properties")).clicked() {
        app.actions.push(Action::Properties(e.clone()));
    }
}

fn search_active(app: &App, p: usize) -> bool {
    app.tab(p).search.is_some()
}

pub fn background_menu(app: &mut App, ui: &mut Ui, p: usize) {
    ui.set_min_width(220.0);
    let here = app.tab(p).location().clone();
    if here == Location::Trash {
        if ui.button(t("empty_trash")).clicked() {
            app.actions.push(Action::AskEmptyTrash);
        }
        return;
    }
    if ui.button(t("new_folder")).clicked() {
        app.actions.push(Action::NewFolder(here.clone()));
    }
    if ui.button(t("new_text_file")).clicked() {
        app.actions.push(Action::NewFile(here.clone()));
    }
    ui.add_enabled_ui(app.clip.is_some(), |ui| {
        if ui.button(t("paste")).clicked() {
            app.actions.push(Action::Paste(here.clone()));
        }
    });
    ui.separator();
    if ui.button(t("select_all")).clicked() {
        app.tab_mut(p).select_all();
    }
    ui.menu_button(t("view"), |ui| {
        for v in ViewMode::ALL {
            if ui.radio(app.view_mode(p) == v, t(v.key())).clicked() {
                app.actions.push(Action::SetView(v));
            }
        }
    });
    ui.menu_button(t("sort_by"), |ui| sort_menu(app, ui));
    if ui
        .checkbox(&mut app.settings.show_hidden, t("show_hidden"))
        .changed()
    {
        app.mark_dirty();
    }
    ui.separator();
    if ui.button(t("open_new_tab")).clicked() {
        app.actions.push(Action::OpenInNewTab(here.clone()));
    }
    if ui.button(t("add_favorite")).clicked() {
        app.actions.push(Action::Pin(here.clone()));
    }
    if ui.button(t("copy_path")).clicked() {
        app.actions.push(Action::CopyPath(vec![here.clone()]));
    }
    if ui.button(t("refresh")).clicked() {
        app.actions.push(Action::Refresh(p));
    }
}

/// The details header's menu: which columns show, how tall the rows are.
fn column_menu(app: &mut App, ui: &mut Ui) {
    ui.set_min_width(220.0);
    let c = &mut app.settings.columns;
    let d = crate::settings::Columns::default();
    for (label, slot, default) in [
        ("col_modified", &mut c.modified, d.modified),
        ("col_kind", &mut c.kind, d.kind),
        ("col_size", &mut c.size, d.size),
    ] {
        let mut on = *slot > 0.0;
        if ui.checkbox(&mut on, t(label)).changed() {
            *slot = if on { default } else { 0.0 };
        }
    }
    ui.separator();
    ui.label(egui::RichText::new(t("row_height")).small());
    ui.horizontal(|ui| {
        for (key, h) in [
            ("rows_compact", 22.0),
            ("rows_normal", 26.0),
            ("rows_large", 34.0),
        ] {
            if ui
                .selectable_label((c.row - h).abs() < 0.5, t(key))
                .clicked()
            {
                c.row = h;
            }
        }
    });
    ui.add(egui::Slider::new(&mut c.row, 20.0..=48.0).show_value(false));
    ui.separator();
    if ui.button(t("reset_columns")).clicked() {
        *c = d;
    }
}

pub fn sort_menu(app: &mut App, ui: &mut Ui) {
    let p = app.active_pane;
    let mut f = app.folder_view(p);
    let before = f;
    for k in SortKey::ALL {
        if ui.radio(f.sort == k, t(k.key())).clicked() {
            f.sort = k;
        }
    }
    ui.separator();
    if ui.radio(!f.descending, t("ascending")).clicked() {
        f.descending = false;
    }
    if ui.radio(f.descending, t("descending")).clicked() {
        f.descending = true;
    }
    if f != before {
        app.set_folder_view(p, f);
    }
    ui.separator();
    if ui
        .checkbox(&mut app.settings.folders_first, t("folders_first"))
        .changed()
    {
        app.mark_dirty();
    }
    if ui
        .checkbox(&mut app.settings.remember_folder_views, t("remember_views"))
        .changed()
    {
        app.mark_dirty();
    }
}

/// The empty area of a view: clicking clears the selection, dragging draws a rubber band, the
/// right button opens the folder's menu.
fn background(app: &mut App, ui: &mut Ui, p: usize, rect: Rect) -> egui::Response {
    let resp = ui.interact(rect, ui.scope_id().with(("bg", p)), Sense::click_and_drag());
    if resp.clicked() && !ui.input(|i| i.modifiers.command || i.modifiers.shift) {
        app.active_pane = p;
        app.tab_mut(p).selected.clear();
    }
    if resp.secondary_clicked() {
        app.active_pane = p;
    }
    resp
}

fn background_menu_area(app: &mut App, ui: &mut Ui, p: usize, rect: Rect) {
    let resp = background(app, ui, p, rect);
    resp.context_menu(|ui| background_menu(app, ui, p));
}

/// Rubber-band selection: the items whose rects meet the band are selected.
fn rubber_band(app: &mut App, ui: &Ui, p: usize, bg: &egui::Response, rects: &[(usize, Rect)]) {
    let id = ui.scope_id().with(("band", p));
    if bg.drag_started() {
        let start = ui.input(|i| i.pointer.press_origin()).unwrap_or_default();
        let additive = ui.input(|i| i.modifiers.command || i.modifiers.shift);
        if !additive {
            app.tab_mut(p).selected.clear();
        }
        let base: Vec<Location> = app.tab(p).selected.iter().cloned().collect();
        ui.data_mut(|d| d.insert_temp(id, (start, base)));
    }
    if bg.dragged() {
        let Some((start, base)) = ui.data(|d| d.get_temp::<(egui::Pos2, Vec<Location>)>(id)) else {
            return;
        };
        let Some(now) = ui.input(|i| i.pointer.interact_pos()) else {
            return;
        };
        let band = Rect::from_two_pos(start, now);
        ui.painter().rect(
            band,
            2,
            theme::palette(app.dark).drop,
            Stroke::new(1.0, theme::ACCENT),
            StrokeKind::Inside,
        );
        let tab = app.tab_mut(p);
        tab.selected = base.into_iter().collect();
        let mut last = None;
        for (i, r) in rects {
            if r.intersects(band)
                && let Some(l) = tab.item(*i).map(|e| e.location.clone())
            {
                tab.selected.insert(l);
                last = Some(*i);
            }
        }
        if last.is_some() {
            tab.cursor = last;
        }
    }
}

/// Keep the cursor's row visible: the scroll offset that shows rows `top..bottom`.
fn scroll_offset(
    app: &mut App,
    ui: &Ui,
    p: usize,
    id: egui::Id,
    row_top: f32,
    row_bottom: f32,
    viewport: f32,
) -> Option<f32> {
    if !app.tab(p).scroll_to_cursor {
        return None;
    }
    app.tab_mut(p).scroll_to_cursor = false;
    let current = egui::scroll_area::State::load(ui.ctx(), id)
        .map(|s| s.clamped_offset().y)
        .unwrap_or(0.0);
    if row_top < current {
        Some(row_top)
    } else if row_bottom > current + viewport {
        Some(row_bottom - viewport)
    } else {
        None
    }
}

fn selection_fill(app: &App, p: usize, selected: bool, hovered: bool) -> Option<Color32> {
    let pal = theme::palette(app.dark);
    if selected {
        Some(if app.active_pane == p {
            pal.selection
        } else {
            pal.selection_inactive
        })
    } else if hovered {
        Some(pal.hover)
    } else {
        None
    }
}

fn text_color(app: &App, p: usize, selected: bool, ui: &Ui, e: &Entry) -> Color32 {
    let base = if selected && app.active_pane == p {
        Color32::WHITE
    } else {
        ui.visuals().text_color()
    };
    let cut = app
        .clip
        .as_ref()
        .is_some_and(|c| c.cut && c.items.contains(&e.location));
    if e.hidden || cut {
        base.gamma_multiply(0.55)
    } else {
        base
    }
}

fn grid(app: &mut App, ui: &mut Ui, p: usize, mode: ViewMode) {
    let icon = mode.icon_size();
    let small = mode == ViewMode::SmallIcons;
    let cell = if small {
        vec2(220.0, 30.0)
    } else {
        vec2(icon + 44.0, icon + 46.0)
    };
    let width = ui.available_width() - 16.0;
    let cols = ((width / cell.x).floor() as usize).max(1);
    let cell = vec2(
        if small {
            width / cols as f32
        } else {
            cell.x + (width - cols as f32 * cell.x) / cols as f32
        },
        cell.y,
    );
    let n = app.tab(p).visible.len();
    let rows_n = n.div_ceil(cols);
    app.columns[p] = cols;
    let viewport = ui.available_height();
    app.page_rows[p] = ((viewport / cell.y) as usize).max(1);
    let full = ui.available_rect_before_wrap();
    let bg = background(app, ui, p, full);
    let scroll_id = ui.scope_id().with(("grid-scroll", p));
    let mut area = egui::ScrollArea::vertical()
        .id_salt(("grid-scroll", p))
        .auto_shrink(false);
    if let Some(c) = app.tab(p).cursor {
        let row = c / cols;
        if let Some(off) = scroll_offset(
            app,
            ui,
            p,
            scroll_id,
            row as f32 * cell.y,
            (row + 1) as f32 * cell.y + 8.0,
            viewport,
        ) {
            area = area.vertical_scroll_offset(off);
        }
    }
    let mut rects = Vec::new();
    // show_rows adds the item spacing to every row: none, so its rows and ours line up.
    ui.spacing_mut().item_spacing.y = 0.0;
    area.show_rows(ui, cell.y, rows_n, |ui, range| {
        ui.add_space(4.0);
        for row in range {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.add_space(8.0);
                for col in 0..cols {
                    let i = row * cols + col;
                    let Some(e) = app.tab(p).item(i).cloned() else {
                        break;
                    };
                    let (rect, resp) = ui.allocate_exact_size(cell, Sense::click_and_drag());
                    rects.push((i, rect));
                    let selected = app.tab(p).selected.contains(&e.location);
                    let name = display_name(app, &e);
                    if small {
                        if let Some(fill) = selection_fill(app, p, selected, resp.hovered()) {
                            ui.painter()
                                .rect_filled(rect.shrink(1.0), CornerRadius::same(5), fill);
                        }
                        let ir = Rect::from_min_size(
                            pos2(rect.left() + 6.0, rect.center().y - 11.0),
                            Vec2::splat(22.0),
                        );
                        entry_icon(app, ui, ir, &e, false);
                        let color = text_color(app, p, selected, ui, &e);
                        let galley = ui.painter().layout(
                            name,
                            FontId::proportional(13.5),
                            color,
                            rect.width() - 40.0,
                        );
                        ui.painter().galley(
                            pos2(ir.right() + 6.0, rect.center().y - galley.size().y / 2.0),
                            galley,
                            color,
                        );
                    } else {
                        let ir = Rect::from_center_size(
                            pos2(rect.center().x, rect.top() + 6.0 + icon / 2.0),
                            Vec2::splat(icon),
                        );
                        if selected || resp.hovered() {
                            let pal = theme::palette(app.dark);
                            let fill = if selected {
                                pal.selection_inactive
                            } else {
                                pal.hover
                            };
                            ui.painter()
                                .rect_filled(ir.expand(5.0), CornerRadius::same(8), fill);
                        }
                        entry_icon(app, ui, ir, &e, true);
                        let color = text_color(app, p, selected, ui, &e);
                        let font = FontId::proportional(if icon > 80.0 { 13.0 } else { 12.5 });
                        let mut job = egui::text::LayoutJob::simple(
                            name,
                            font,
                            if selected && app.active_pane == p {
                                Color32::WHITE
                            } else {
                                color
                            },
                            rect.width() - 8.0,
                        );
                        job.halign = egui::Align::Center;
                        job.wrap.max_rows = 2;
                        job.wrap.break_anywhere = true;
                        let galley = ui.painter().layout_job(job);
                        let tr = Rect::from_center_size(
                            pos2(rect.center().x, ir.bottom() + 8.0 + galley.size().y / 2.0),
                            galley.size() + vec2(8.0, 2.0),
                        );
                        if selected {
                            let fill = selection_fill(app, p, true, false).unwrap_or(theme::ACCENT);
                            ui.painter().rect_filled(tr, CornerRadius::same(4), fill);
                        }
                        ui.painter()
                            .galley(pos2(tr.center().x, tr.top() + 1.0), galley, color);
                    }
                    let resp = resp.on_hover_text_at_pointer(hover_text(&e));
                    item_interact(app, ui, p, i, &resp, &e);
                }
            });
        }
    });
    rubber_band(app, ui, p, &bg, &rects);
    bg.context_menu(|ui| background_menu(app, ui, p));
}

/// The size column: a file's size, a folder's contents' (computed in the background: "…" until
/// then), or a dash.
pub fn size_text(app: &App, e: &Entry) -> String {
    if !e.is_dir() {
        return fileinfo::format_size(e.size);
    }
    let local = e.location.is_local() && !e.symlink && e.trash.is_none();
    if !app.settings.folder_sizes || !local {
        return "—".to_owned();
    }
    match app.dir_size(e) {
        Some(s) => fileinfo::format_size(s.bytes),
        None => "…".to_owned(),
    }
}

fn hover_text(e: &Entry) -> String {
    let mut s = e.name.clone();
    if !e.is_dir() {
        s.push_str(&format!(
            "\n{} · {}",
            fileinfo::format_size(e.size),
            fileinfo::format_date(e.modified)
        ));
    }
    s
}

/// List (compact rows) and content (icon with details) views.
fn rows(app: &mut App, ui: &mut Ui, p: usize, content: bool) {
    let h = if content {
        54.0
    } else {
        app.settings.columns.row.clamp(20.0, 48.0)
    };
    let n = app.tab(p).visible.len();
    app.columns[p] = 1;
    let viewport = ui.available_height();
    app.page_rows[p] = ((viewport / h) as usize).max(1);
    let full = ui.available_rect_before_wrap();
    let bg = background(app, ui, p, full);
    let scroll_id = ui.scope_id().with(("rows-scroll", p));
    let mut area = egui::ScrollArea::vertical()
        .id_salt(("rows-scroll", p))
        .auto_shrink(false);
    if let Some(c) = app.tab(p).cursor
        && let Some(off) = scroll_offset(
            app,
            ui,
            p,
            scroll_id,
            c as f32 * h,
            (c + 1) as f32 * h,
            viewport,
        )
    {
        area = area.vertical_scroll_offset(off);
    }
    let in_search = search_active(app, p);
    let pal = theme::palette(app.dark);
    let mut rects = Vec::new();
    // show_rows adds the item spacing to every row: none, so its rows and ours line up.
    ui.spacing_mut().item_spacing.y = 0.0;
    area.show_rows(ui, h, n, |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in range {
            let Some(e) = app.tab(p).item(i).cloned() else {
                break;
            };
            let w = ui.available_width();
            let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
            rects.push((i, rect));
            let selected = app.tab(p).selected.contains(&e.location);
            let row = rect.shrink2(vec2(6.0, 1.0));
            if let Some(fill) = selection_fill(app, p, selected, resp.hovered()) {
                ui.painter().rect_filled(row, CornerRadius::same(6), fill);
            }
            let color = text_color(app, p, selected, ui, &e);
            let weak = if selected && app.active_pane == p {
                Color32::from_white_alpha(200)
            } else {
                pal.weak
            };
            let isz = if content { 40.0 } else { 18.0 };
            let ir = Rect::from_min_size(
                pos2(row.left() + 8.0, row.center().y - isz / 2.0),
                Vec2::splat(isz),
            );
            entry_icon(app, ui, ir, &e, content);
            let x = ir.right() + 10.0;
            let right_w = if content { 180.0 } else { 90.0 };
            let name_w = (row.right() - x - right_w - 8.0).max(40.0);
            let name = display_name(app, &e);
            if content {
                let g = ui
                    .painter()
                    .layout(name, FontId::proportional(14.0), color, name_w);
                ui.painter().galley(pos2(x, row.top() + 8.0), g, color);
                let sub = if in_search {
                    e.location
                        .parent()
                        .map(|l| l.address(&app.settings.servers))
                        .unwrap_or_default()
                } else {
                    fileinfo::kind_label(&e.ext, e.is_dir())
                };
                let g = ui
                    .painter()
                    .layout(sub, FontId::proportional(12.0), weak, name_w);
                ui.painter().galley(pos2(x, row.top() + 29.0), g, weak);
                let date = fileinfo::format_date(e.modified);
                ui.painter().text(
                    pos2(row.right() - 10.0, row.top() + 16.0),
                    Align2::RIGHT_CENTER,
                    date,
                    FontId::proportional(12.0),
                    weak,
                );
                ui.painter().text(
                    pos2(row.right() - 10.0, row.top() + 36.0),
                    Align2::RIGHT_CENTER,
                    size_text(app, &e),
                    FontId::proportional(12.0),
                    weak,
                );
            } else {
                let g = ui
                    .painter()
                    .layout_no_wrap(name, FontId::proportional(13.5), color);
                let clip = Rect::from_min_size(pos2(x, row.top()), vec2(name_w, row.height()));
                ui.painter().with_clip_rect(clip).galley(
                    pos2(x, row.center().y - g.size().y / 2.0),
                    g,
                    color,
                );
                let right = size_text(app, &e);
                ui.painter().text(
                    pos2(row.right() - 10.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    right,
                    FontId::proportional(12.0),
                    weak,
                );
            }
            let resp = resp.on_hover_text_at_pointer(hover_text(&e));
            item_interact(app, ui, p, i, &resp, &e);
        }
    });
    rubber_band(app, ui, p, &bg, &rects);
    bg.context_menu(|ui| background_menu(app, ui, p));
}

fn details(app: &mut App, ui: &mut Ui, p: usize) {
    let pal = theme::palette(app.dark);
    let in_search = search_active(app, p);
    let cols_cfg = app.settings.columns;
    let h = cols_cfg.row.clamp(20.0, 48.0);
    let fv = app.folder_view(p);
    // Columns: name (takes what is left), [folder when searching], modified, kind, size. Their
    // widths are the user's (drag a header's edge); on narrow windows the optional ones hide.
    let width = ui.available_width();
    let date_w = if app.narrow { 0.0 } else { cols_cfg.modified };
    let kind_w = if app.narrow || width < 640.0 {
        0.0
    } else {
        cols_cfg.kind
    };
    let size_w = cols_cfg
        .size
        .max(if cols_cfg.size > 0.0 { 50.0 } else { 0.0 });
    let folder_w = if in_search && width > 560.0 {
        cols_cfg.folder.min(width * 0.3)
    } else {
        0.0
    };
    let name_w = (width - date_w - kind_w - size_w - folder_w - 34.0).max(120.0);
    // header
    let (hrect, hresp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    ui.painter().line_segment(
        [hrect.left_bottom(), hrect.right_bottom()],
        Stroke::new(1.0, pal.separator),
    );
    let mut x = hrect.left() + 34.0;
    #[derive(Clone, Copy, PartialEq)]
    enum Col {
        Name,
        Folder,
        Modified,
        Kind,
        Size,
    }
    let cols: [(Col, Option<SortKey>, &str, f32, bool); 5] = [
        (Col::Name, Some(SortKey::Name), "col_name", name_w, false),
        (Col::Folder, None, "col_folder", folder_w, false),
        (
            Col::Modified,
            Some(SortKey::Modified),
            "col_modified",
            date_w,
            false,
        ),
        (Col::Kind, Some(SortKey::Kind), "col_kind", kind_w, false),
        (Col::Size, Some(SortKey::Size), "col_size", size_w, true),
    ];
    for (col, key, label, w, right) in cols {
        if w <= 0.0 {
            continue;
        }
        let r = Rect::from_min_size(pos2(x, hrect.top()), vec2(w, hrect.height()));
        let resp = ui.interact(r, ui.scope_id().with(("hdr", p, label)), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 4, pal.hover);
        }
        let sorted = key == Some(fv.sort);
        let (pos, align) = if right {
            (pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER)
        } else {
            (pos2(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER)
        };
        let tr = ui.painter().text(
            pos,
            align,
            t(label),
            FontId::proportional(12.0),
            if sorted {
                ui.visuals().text_color()
            } else {
                pal.weak
            },
        );
        if sorted {
            // a small chevron after the label (before it for right-aligned columns)
            let cx = if right {
                tr.left() - 9.0
            } else {
                tr.right() + 9.0
            };
            let rr = Rect::from_center_size(pos2(cx, r.center().y), Vec2::splat(10.0));
            if fv.descending {
                ui.painter().add(egui::Shape::line(
                    vec![
                        pos2(rr.left(), rr.top() + 3.0),
                        pos2(rr.center().x, rr.bottom() - 2.0),
                        pos2(rr.right(), rr.top() + 3.0),
                    ],
                    Stroke::new(1.5, pal.weak),
                ));
            } else {
                icons::sym(ui.painter(), rr, Sym::Up, pal.weak);
            }
        }
        if resp.clicked()
            && let Some(k) = key
        {
            let f = if fv.sort == k {
                crate::settings::FolderView {
                    descending: !fv.descending,
                    ..fv
                }
            } else {
                crate::settings::FolderView {
                    sort: k,
                    descending: matches!(k, SortKey::Modified | SortKey::Size),
                    ..fv
                }
            };
            app.set_folder_view(p, f);
        }
        // The edge on the left of every column after the name resizes it (dragging left makes
        // it wider); a double click restores its width.
        if col != Col::Name {
            let edge = Rect::from_center_size(pos2(r.left(), r.center().y), vec2(8.0, r.height()));
            let er = ui
                .interact(
                    edge,
                    ui.scope_id().with(("hdr-edge", p, label)),
                    Sense::click_and_drag(),
                )
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            if er.hovered() || er.dragged() {
                ui.painter().line_segment(
                    [
                        pos2(r.left(), r.top() + 4.0),
                        pos2(r.left(), r.bottom() - 4.0),
                    ],
                    Stroke::new(2.0, theme::ACCENT),
                );
            } else {
                ui.painter().line_segment(
                    [
                        pos2(r.left(), r.top() + 6.0),
                        pos2(r.left(), r.bottom() - 6.0),
                    ],
                    Stroke::new(1.0, pal.separator),
                );
            }
            let c = &mut app.settings.columns;
            let slot = match col {
                Col::Folder => &mut c.folder,
                Col::Modified => &mut c.modified,
                Col::Kind => &mut c.kind,
                _ => &mut c.size,
            };
            if er.dragged() {
                *slot = (*slot - er.drag_delta().x).clamp(50.0, 600.0);
            }
            if er.double_clicked() {
                let d = crate::settings::Columns::default();
                *slot = match col {
                    Col::Folder => d.folder,
                    Col::Modified => d.modified,
                    Col::Kind => d.kind,
                    _ => d.size,
                };
            }
        }
        resp.context_menu(|ui| column_menu(app, ui));
        x += w;
    }
    hresp.context_menu(|ui| column_menu(app, ui));
    let n = app.tab(p).visible.len();
    app.columns[p] = 1;
    let viewport = ui.available_height();
    app.page_rows[p] = ((viewport / h) as usize).max(1);
    let full = ui.available_rect_before_wrap();
    let bg = background(app, ui, p, full);
    let scroll_id = ui.scope_id().with(("details-scroll", p));
    let mut area = egui::ScrollArea::vertical()
        .id_salt(("details-scroll", p))
        .auto_shrink(false);
    if let Some(c) = app.tab(p).cursor
        && let Some(off) = scroll_offset(
            app,
            ui,
            p,
            scroll_id,
            c as f32 * h,
            (c + 1) as f32 * h,
            viewport,
        )
    {
        area = area.vertical_scroll_offset(off);
    }
    let mut rects = Vec::new();
    // show_rows adds the item spacing to every row: none, so its rows and ours line up.
    ui.spacing_mut().item_spacing.y = 0.0;
    area.show_rows(ui, h, n, |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in range {
            let Some(e) = app.tab(p).item(i).cloned() else {
                break;
            };
            let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click_and_drag());
            rects.push((i, rect));
            let selected = app.tab(p).selected.contains(&e.location);
            let row = rect.shrink2(vec2(4.0, 0.5));
            if i % 2 == 1 && !selected {
                ui.painter()
                    .rect_filled(row, CornerRadius::same(5), pal.stripe);
            }
            if let Some(fill) = selection_fill(app, p, selected, resp.hovered()) {
                ui.painter().rect_filled(row, CornerRadius::same(5), fill);
            }
            let color = text_color(app, p, selected, ui, &e);
            let weak = if selected && app.active_pane == p {
                Color32::from_white_alpha(210)
            } else {
                pal.weak
            };
            let ir = Rect::from_min_size(
                pos2(row.left() + 8.0, row.center().y - 9.0),
                Vec2::splat(18.0),
            );
            entry_icon(app, ui, ir, &e, false);
            let mut x = rect.left() + 34.0;
            let painter = ui.painter();
            let mut cell = |text: String, w: f32, right: bool, c: Color32| {
                if w <= 0.0 {
                    return;
                }
                let clip = Rect::from_min_size(pos2(x, row.top()), vec2(w - 8.0, row.height()));
                let g = painter.layout_no_wrap(text, FontId::proportional(13.0), c);
                let px = if right {
                    x + w - 10.0 - g.size().x
                } else {
                    x + 4.0
                };
                painter
                    .with_clip_rect(clip.intersect(painter.clip_rect()))
                    .galley(pos2(px, row.center().y - g.size().y / 2.0), g, c);
                x += w;
            };
            let size_label = size_text(app, &e);
            let name_label = display_name(app, &e);
            cell(name_label, name_w, false, color);
            let folder = if folder_w > 0.0 {
                e.location
                    .parent()
                    .map(|l| super::pane::location_title(&l))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            cell(folder, folder_w, false, weak);
            cell(fileinfo::format_date(e.modified), date_w, false, weak);
            cell(
                fileinfo::kind_label(&e.ext, e.is_dir()),
                kind_w,
                false,
                weak,
            );
            cell(size_label, size_w, true, weak);
            let resp = resp.on_hover_text_at_pointer(hover_text(&e));
            item_interact(app, ui, p, i, &resp, &e);
        }
    });
    rubber_band(app, ui, p, &bg, &rects);
    bg.context_menu(|ui| background_menu(app, ui, p));
}

/// The dragged items follow the pointer.
fn drag_ghost(app: &App, ctx: &egui::Context) {
    let Some(payload) = egui::DragAndDrop::payload::<DragPayload>(ctx) else {
        return;
    };
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let copy = ctx.input(|i| i.modifiers.command);
    egui::Area::new(egui::Id::unique("drag-ghost"))
        .fixed_pos(pos + vec2(14.0, 10.0))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                    icons::folder(ui.painter(), r, theme::ACCENT);
                    let name = payload
                        .items
                        .first()
                        .map(Location::name)
                        .unwrap_or_default();
                    let text = if payload.items.len() > 1 {
                        tf("n_items_drag", payload.items.len())
                    } else {
                        name
                    };
                    ui.label(text);
                    if copy {
                        ui.label(egui::RichText::new("+").strong().color(theme::ACCENT));
                    }
                });
            });
        });
    let _ = app;
}

