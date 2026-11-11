//! The toolbar: history buttons, the address bar (a clickable path that can be edited and
//! copied), views and sorting, the search field and its filters, the window's toggles.

use egui::{Color32, CornerRadius, FontId, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::app::{Action, App, DropTarget};
use super::icons::{self, Sym};
use super::theme;
use crate::fileinfo::Category;
use crate::i18n::{t, tf};
use crate::search::{DateFilter, SizeFilter};
use crate::settings::ViewMode;
use crate::vfs::{self, Location};

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.active_pane;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if icons::sym_button(
            ui,
            Sym::Sidebar,
            app.settings.show_sidebar,
            t("toggle_sidebar"),
        )
        .clicked()
        {
            app.settings.show_sidebar = !app.settings.show_sidebar;
        }
        let (can_back, can_fwd, can_up) = {
            let tab = app.tab(p);
            (
                tab.can_back(),
                tab.can_forward(),
                tab.location().parent().is_some(),
            )
        };
        if ui
            .add_enabled_ui(can_back, |ui| {
                icons::sym_button(ui, Sym::Back, false, t("back_tip"))
            })
            .inner
            .clicked()
        {
            app.actions.push(Action::Back(p));
        }
        if ui
            .add_enabled_ui(can_fwd, |ui| {
                icons::sym_button(ui, Sym::Forward, false, t("forward_tip"))
            })
            .inner
            .clicked()
        {
            app.actions.push(Action::Forward(p));
        }
        if ui
            .add_enabled_ui(can_up, |ui| {
                icons::sym_button(ui, Sym::Up, false, t("up_tip"))
            })
            .inner
            .clicked()
        {
            app.actions.push(Action::Up(p));
        }
        ui.add_space(4.0);
        // Right side first, so the address bar takes what is left.
        let right_width = if app.narrow { 120.0 } else { 520.0 };
        let addr_width = (ui.available_width() - right_width).max(160.0);
        address_bar(app, ui, p, addr_width);
        ui.add_space(6.0);
        if !app.narrow {
            view_buttons(app, ui, p);
        }
        ui.menu_button_sym(Sym::Sort, t("sort_by"), |ui| {
            super::view::sort_menu(app, ui)
        });
        search_field(app, ui, p);
        more_menu(app, ui, p);
    });
}

trait MenuSym {
    fn menu_button_sym(&mut self, s: Sym, tip: &str, add: impl FnOnce(&mut Ui));
}

impl MenuSym for Ui {
    fn menu_button_sym(&mut self, s: Sym, tip: &str, add: impl FnOnce(&mut Ui)) {
        let resp = icons::sym_button(self, s, false, tip);
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(200.0);
            add(ui);
        });
    }
}

fn view_buttons(app: &mut App, ui: &mut Ui, p: usize) {
    let current = app.view_mode(p);
    let pal = theme::palette(app.dark);
    egui::Frame::new()
        .fill(pal.sidebar)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(2)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let icons_on = current.is_grid();
            let resp = icons::sym_button(ui, Sym::Grid, icons_on, t("view_icons"));
            if resp.clicked() {
                let next = match current {
                    ViewMode::LargeIcons => ViewMode::MediumIcons,
                    ViewMode::MediumIcons => ViewMode::SmallIcons,
                    ViewMode::SmallIcons => ViewMode::LargeIcons,
                    _ => ViewMode::MediumIcons,
                };
                app.actions.push(Action::SetView(next));
            }
            resp.context_menu(|ui| {
                for v in [
                    ViewMode::LargeIcons,
                    ViewMode::MediumIcons,
                    ViewMode::SmallIcons,
                ] {
                    if ui.radio(current == v, t(v.key())).clicked() {
                        app.actions.push(Action::SetView(v));
                    }
                }
            });
            for (v, s) in [
                (ViewMode::List, Sym::List),
                (ViewMode::Details, Sym::Details),
                (ViewMode::Content, Sym::Content),
            ] {
                if icons::sym_button(ui, s, current == v, t(v.key())).clicked() {
                    app.actions.push(Action::SetView(v));
                }
            }
        });
}

/// Breadcrumbs of the path: (label, location) from the top.
fn crumbs(app: &App, loc: &Location) -> Vec<(String, Location)> {
    let mut out = Vec::new();
    match loc {
        Location::Trash => out.push((t("trash").to_owned(), Location::Trash)),
        Location::Local(_) => {
            // Stop at a volume's top or the home folder, named as the sidebar names them.
            let mut cur = Some(loc.clone());
            while let Some(l) = cur {
                let Location::Local(path) = &l else { break };
                let vol = app.volumes.iter().find(|v| v.path == *path);
                if let Some(v) = vol {
                    out.push((super::pane::volume_label(v), l.clone()));
                    break;
                }
                if *path == crate::platform::home() && cfg!(not(target_os = "android")) {
                    out.push((t("home").to_owned(), l.clone()));
                    break;
                }
                let name = l.name();
                out.push((
                    if name.is_empty() {
                        "/".to_owned()
                    } else {
                        name
                    },
                    l.clone(),
                ));
                cur = l.parent();
            }
            out.reverse();
        }
        Location::Remote { .. } => {
            let mut cur = Some(loc.clone());
            while let Some(l) = cur {
                let label = super::pane::location_title(&l);
                out.push((label, l.clone()));
                cur = l.parent();
            }
            out.reverse();
        }
    }
    out
}

fn address_bar(app: &mut App, ui: &mut Ui, p: usize, width: f32) {
    let pal = theme::palette(app.dark);
    let here = app.tab(p).location().clone();
    let height = 30.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let editing = app.address_edit.is_some();
    ui.painter().rect(
        rect,
        CornerRadius::same(8),
        ui.visuals().extreme_bg_color,
        Stroke::new(
            1.0,
            if editing {
                theme::ACCENT
            } else {
                pal.separator
            },
        ),
        egui::StrokeKind::Inside,
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(6.0, 2.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(rect);
    if let Some(mut text) = app.address_edit.take() {
        let edit = egui::TextEdit::singleline(&mut text)
            .frame(egui::Frame::NONE)
            .desired_width(rect.width() - 50.0)
            .font(FontId::proportional(14.0))
            .hint_text(t("address_hint"));
        let r = child.add(edit);
        if app.focus_address {
            r.request_focus();
            app.focus_address = false;
            // Select it all, so typing replaces it.
            if let Some(mut state) = egui::TextEdit::load_state(child.ctx(), r.id) {
                let n = text.chars().count();
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(n),
                    )));
                state.store(child.ctx(), r.id);
            }
        }
        let enter = r.lost_focus() && child.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = child.input(|i| i.key_pressed(egui::Key::Escape));
        if enter {
            go_to_address(app, p, &text);
        } else if escape || (r.lost_focus() && !enter) {
            // dropped
        } else {
            app.address_edit = Some(text);
        }
        return;
    }
    // The path as buttons.
    let list = crumbs(app, &here);
    let mut shown = list.len();
    // Keep the last parts when the path is long.
    let budget = rect.width() - 60.0;
    let widths: Vec<f32> = list
        .iter()
        .map(|(l, _)| l.chars().count() as f32 * 7.5 + 26.0)
        .collect();
    while shown > 1 && widths[list.len() - shown..].iter().sum::<f32>() > budget {
        shown -= 1;
    }
    child.spacing_mut().item_spacing.x = 0.0;
    let skipped = list.len() - shown;
    if skipped > 0 {
        child.label(egui::RichText::new("…").color(pal.weak));
    }
    let mut first = true;
    for (label, loc) in list.into_iter().skip(skipped) {
        if !first {
            let (r, _) = child.allocate_exact_size(vec2(14.0, height), Sense::hover());
            icons::sym(
                child.painter(),
                egui::Rect::from_center_size(r.center(), Vec2::splat(12.0)),
                Sym::Forward,
                pal.weak,
            );
        }
        first = false;
        let last = loc == here;
        let text = if last {
            egui::RichText::new(label).strong()
        } else {
            egui::RichText::new(label)
        };
        let b = child.add(
            egui::Button::new(text)
                .frame(false)
                .min_size(vec2(0.0, 24.0)),
        );
        if b.clicked() && !last {
            app.actions.push(Action::Navigate(p, loc.clone()));
        }
        if loc != Location::Trash {
            app.drop_targets.push(DropTarget {
                rect: b.rect,
                dest: loc.clone(),
                priority: 3,
                trash: false,
            });
        }
        b.context_menu(|ui| {
            if ui.button(t("open_new_tab")).clicked() {
                app.actions.push(Action::OpenInNewTab(loc.clone()));
            }
            if ui.button(t("copy_path")).clicked() {
                app.actions.push(Action::CopyPath(vec![loc.clone()]));
            }
        });
    }
    // Copy and edit buttons at the right end.
    let copy_rect = egui::Rect::from_center_size(
        pos2(rect.right() - 16.0, rect.center().y),
        Vec2::splat(24.0),
    );
    let copy = ui
        .interact(
            copy_rect,
            ui.scope_id().with("copy-address"),
            Sense::click(),
        )
        .on_hover_text(t("copy_address"));
    icons::sym(
        ui.painter(),
        copy_rect.shrink(4.0),
        Sym::Copy,
        if copy.hovered() {
            theme::ACCENT
        } else {
            pal.weak
        },
    );
    if copy.clicked() {
        app.actions.push(Action::CopyPath(vec![here.clone()]));
    }
    if resp.clicked() || resp.double_clicked() {
        app.address_edit = Some(here.address(&app.settings.servers));
        app.focus_address = true;
    }
}

/// Go to a typed address: a path, `trash:`, or a server URL (a saved server, or a new one).
fn go_to_address(app: &mut App, p: usize, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if text.starts_with("trash:") {
        app.actions.push(Action::Navigate(p, Location::Trash));
        return;
    }
    if let Some(url) = vfs::servers::parse_url(text) {
        let found = app.settings.servers.iter().find(|s| {
            s.kind == url.kind
                && s.host.eq_ignore_ascii_case(&url.host)
                && s.port == url.port
                && (url.user.is_empty() || s.user == url.user)
        });
        if let Some(s) = found {
            // For SMB and WebDAV the first part of the path is the share / base.
            let base = s.share.trim_matches('/');
            let mut path = url.path.clone();
            if !base.is_empty()
                && let Some(rest) = path.strip_prefix(&format!("/{base}"))
            {
                path = if rest.is_empty() {
                    "/".to_owned()
                } else {
                    rest.to_owned()
                };
            }
            app.actions
                .push(Action::Navigate(p, Location::Remote { server: s.id, path }));
        } else {
            let id = app.settings.new_server_id();
            let mut s = vfs::Server::new(id, url.kind);
            s.host = url.host;
            s.port = url.port;
            s.user = url.user;
            let mut parts = url.path.trim_matches('/').splitn(2, '/');
            if matches!(url.kind, vfs::ServerKind::Smb) {
                s.share = parts.next().unwrap_or_default().to_owned();
                s.start_path = format!("/{}", parts.next().unwrap_or_default());
            } else {
                s.start_path = url.path.clone();
            }
            app.dialog = Some(super::dialogs::Dialog::server(s));
        }
        return;
    }
    let path = if let Some(rest) = text.strip_prefix('~') {
        format!("{}{rest}", crate::platform::home().display())
    } else {
        text.to_owned()
    };
    let path = std::path::PathBuf::from(path);
    if path.exists() {
        app.actions.push(Action::Navigate(p, Location::Local(path)));
    } else {
        app.say(tf("not_found", text));
    }
}

fn search_field(app: &mut App, ui: &mut Ui, p: usize) {
    let pal = theme::palette(app.dark);
    let w = if app.narrow { 90.0 } else { 220.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
    ui.painter().rect(
        rect,
        CornerRadius::same(8),
        ui.visuals().extreme_bg_color,
        Stroke::new(1.0, pal.separator),
        egui::StrokeKind::Inside,
    );
    icons::sym(
        ui.painter(),
        egui::Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(16.0)),
        Sym::Search,
        pal.weak,
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                pos2(rect.left() + 26.0, rect.top() + 3.0),
                pos2(rect.right() - 28.0, rect.bottom() - 3.0),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let here = super::pane::location_title(app.tab(p).location());
    let tab = app.tab_mut(p);
    let mut text = tab.query.text.clone();
    let r = child.add(
        egui::TextEdit::singleline(&mut text)
            .frame(egui::Frame::NONE)
            .hint_text(tf("search_in", here))
            .desired_width(f32::INFINITY),
    );
    if app.focus_search {
        r.request_focus();
        app.focus_search = false;
    }
    let tab = app.tab_mut(p);
    if r.changed() {
        tab.query.text = text.clone();
        if tab.search.is_none() {
            // Typing filters the folder at once; Enter searches below it.
            tab.filter = text;
            tab.dirty = true;
        }
    }
    if r.lost_focus() && child.input(|i| i.key_pressed(egui::Key::Enter)) {
        app.actions.push(Action::Search(p));
    }
    if r.has_focus() && child.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.actions.push(Action::ClearSearch(p));
    }
    // Filters
    let fr = egui::Rect::from_center_size(
        pos2(rect.right() - 14.0, rect.center().y),
        Vec2::splat(22.0),
    );
    let fresp = ui
        .interact(
            fr,
            ui.scope_id().with(("search-filters", p)),
            Sense::click(),
        )
        .on_hover_text(t("search_options"));
    let active = {
        let q = &app.tab(p).query;
        q.content || q.category.is_some() || q.date != DateFilter::Any || q.size != SizeFilter::Any
    };
    icons::sym(
        ui.painter(),
        fr.shrink(3.0),
        Sym::Settings,
        if active || fresp.hovered() {
            theme::ACCENT
        } else {
            pal.weak
        },
    );
    egui::Popup::menu(&fresp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(260.0);
            ui.label(egui::RichText::new(t("search_options")).strong());
            let tab = app.tab_mut(p);
            ui.horizontal(|ui| {
                ui.radio_value(&mut tab.query.content, false, t("by_name"));
                ui.radio_value(&mut tab.query.content, true, t("by_content"));
            });
            egui::Grid::new("search-grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label(t("col_kind"));
                    egui::ComboBox::from_id_salt("q-kind")
                        .selected_text(
                            tab.query
                                .category
                                .map(|c| t(c.key()))
                                .unwrap_or(t("any_kind")),
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut tab.query.category, None, t("any_kind"));
                            for c in Category::FILTERS {
                                ui.selectable_value(&mut tab.query.category, Some(c), t(c.key()));
                            }
                        });
                    ui.end_row();
                    ui.label(t("col_modified"));
                    egui::ComboBox::from_id_salt("q-date")
                        .selected_text(t(tab.query.date.key()))
                        .show_ui(ui, |ui| {
                            for d in DateFilter::ALL {
                                ui.selectable_value(&mut tab.query.date, d, t(d.key()));
                            }
                        });
                    ui.end_row();
                    ui.label(t("col_size"));
                    egui::ComboBox::from_id_salt("q-size")
                        .selected_text(t(tab.query.size.key()))
                        .show_ui(ui, |ui| {
                            for s in SizeFilter::ALL {
                                ui.selectable_value(&mut tab.query.size, s, t(s.key()));
                            }
                        });
                    ui.end_row();
                });
            ui.small(egui::RichText::new(t("search_hint")).color(pal.weak));
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .button(egui::RichText::new(t("search_below")).color(Color32::WHITE))
                    .clicked()
                {
                    app.actions.push(Action::Search(p));
                    ui.close();
                }
                if ui.button(t("reset")).clicked() {
                    app.actions.push(Action::ClearSearch(p));
                    let tab = app.tab_mut(p);
                    tab.query = Default::default();
                }
            });
        });
}

fn more_menu(app: &mut App, ui: &mut Ui, p: usize) {
    if !app.narrow {
        if icons::sym_button(ui, Sym::DualPane, app.settings.dual_pane, t("dual_pane")).clicked() {
            app.settings.dual_pane = !app.settings.dual_pane;
        }
        if icons::sym_button(
            ui,
            Sym::Preview,
            app.settings.show_preview,
            t("preview_panel"),
        )
        .clicked()
        {
            app.settings.show_preview = !app.settings.show_preview;
        }
    }
    let resp = icons::sym_button(ui, Sym::More, false, t("more"));
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(240.0);
        let here = app.tab(p).location().clone();
        if here != Location::Trash {
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
        }
        if ui.button(t("new_tab")).clicked() {
            app.actions.push(Action::NewTab(p));
        }
        if app.narrow {
            ui.menu_button(t("view"), |ui| {
                for v in ViewMode::ALL {
                    if ui.radio(app.view_mode(p) == v, t(v.key())).clicked() {
                        app.actions.push(Action::SetView(v));
                    }
                }
            });
        }
        if ui
            .checkbox(&mut app.settings.show_hidden, t("show_hidden"))
            .changed()
        {
            app.mark_dirty();
        }
        if ui
            .checkbox(&mut app.settings.show_extensions, t("show_extensions"))
            .changed()
        {
            app.mark_dirty();
        }
        ui.checkbox(&mut app.settings.dual_pane, t("dual_pane"));
        ui.checkbox(&mut app.settings.show_preview, t("preview_panel"));
        ui.separator();
        if ui.button(t("connect_server")).clicked() {
            app.actions.push(Action::EditServer(None));
        }
        if ui.button(t("refresh")).clicked() {
            app.actions.push(Action::Refresh(p));
        }
        ui.separator();
        if ui.button(t("settings")).clicked() {
            app.actions.push(Action::Settings);
        }
        if ui.button(t("about")).clicked() {
            app.actions.push(Action::About);
        }
    });
}
