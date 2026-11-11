//! The sidebar: favourites, devices (each a folder tree that unfolds), network locations and
//! the trash. Everything in it is a drop target.

use egui::{Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::app::{Action, App, DragPayload, DropTarget, TreeNode};
use super::icons::{self, Sym};
use super::theme;
use crate::i18n::t;
use crate::vfs::{self, Location};

const ROW: f32 = 28.0;

pub fn show(app: &mut App, ui: &mut Ui) {
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        section(app, ui, "favorites", |app, ui| favorites(app, ui));
        section(app, ui, "devices", |app, ui| devices(app, ui));
        section(app, ui, "network", |app, ui| network(app, ui));
        ui.add_space(6.0);
        let here = app.tab(app.active_pane).location().clone();
        let label = if app.trash_count > 0 { format!("{} ({})", t("trash"), app.trash_count) } else { t("trash").to_owned() };
        let resp = row(app, ui, 0, Sym::Trash, &label, here == Location::Trash, None);
        app.drop_targets.push(DropTarget { rect: resp.rect, dest: Location::Trash, priority: 3, trash: true });
        if resp.clicked() {
            let p = app.active_pane;
            app.actions.push(Action::Navigate(p, Location::Trash));
        }
        resp.context_menu(|ui| {
            if ui.button(t("empty_trash")).clicked() {
                app.actions.push(Action::AskEmptyTrash);
            }
        });
    });
}

fn section(app: &mut App, ui: &mut Ui, key: &str, body: impl FnOnce(&mut App, &mut Ui)) {
    let pal = theme::palette(app.dark);
    let id = ui.scope_id().with(("section", key));
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
    ui.add_space(8.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
    ui.painter().text(pos2(rect.left() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, t(key).to_uppercase(), FontId::proportional(11.0), pal.weak);
    if resp.hovered() {
        let r = Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), Vec2::splat(12.0));
        icons::sym(ui.painter(), r, if open { Sym::Up } else { Sym::Back }, pal.weak);
    }
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    if open {
        body(app, ui);
    }
}

/// One row: indent, an optional disclosure triangle (`Some(expanded)`), icon, label.
fn row(app: &mut App, ui: &mut Ui, depth: usize, s: Sym, label: &str, selected: bool, disclosure: Option<bool>) -> egui::Response {
    row_ex(app, ui, depth, s, label, selected, disclosure, None).0
}

/// As `row`; also returns whether the triangle was clicked. `usage`: a capacity bar.
#[allow(clippy::too_many_arguments)]
fn row_ex(app: &mut App, ui: &mut Ui, depth: usize, s: Sym, label: &str, selected: bool, disclosure: Option<bool>, usage: Option<f32>) -> (egui::Response, bool) {
    let pal = theme::palette(app.dark);
    let h = if usage.is_some() { ROW + 6.0 } else { ROW };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    let dragging = egui::DragAndDrop::has_payload_of_type::<DragPayload>(ui.ctx());
    if selected {
        ui.painter().rect_filled(rect, CornerRadius::same(6), pal.selection_inactive);
    } else if resp.hovered() || (dragging && resp.contains_pointer()) {
        ui.painter().rect_filled(rect, CornerRadius::same(6), if dragging { pal.drop } else { pal.hover });
    }
    let mut x = rect.left() + 6.0 + depth as f32 * 14.0;
    let mut toggled = false;
    if let Some(expanded) = disclosure {
        let tr = Rect::from_center_size(pos2(x + 6.0, rect.top() + ROW / 2.0), Vec2::splat(12.0));
        let tresp = ui.interact(tr.expand(4.0), resp.id.with("disclosure"), Sense::click());
        icons::sym(ui.painter(), tr, if expanded { Sym::Up } else { Sym::Forward }, if tresp.hovered() { theme::ACCENT } else { pal.weak });
        toggled = tresp.clicked();
    }
    x += 14.0;
    let ir = Rect::from_min_size(pos2(x, rect.top() + ROW / 2.0 - 9.0), Vec2::splat(18.0));
    icons::sym(ui.painter(), ir, s, theme::ACCENT);
    let text_x = ir.right() + 8.0;
    let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(13.5), ui.visuals().text_color());
    let clip = Rect::from_min_max(pos2(text_x, rect.top()), pos2(rect.right() - 4.0, rect.bottom()));
    ui.painter().with_clip_rect(clip.intersect(ui.clip_rect())).galley(pos2(text_x, rect.top() + ROW / 2.0 - galley.size().y / 2.0), galley, ui.visuals().text_color());
    if let Some(u) = usage {
        let bar = Rect::from_min_size(pos2(text_x, rect.top() + ROW - 2.0), vec2((rect.right() - text_x - 10.0).max(10.0), 4.0));
        ui.painter().rect_filled(bar, CornerRadius::same(2), pal.separator);
        let color = if u > 0.9 { Color32::from_rgb(0xff, 0x45, 0x3a) } else { theme::ACCENT };
        ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * u.clamp(0.0, 1.0), bar.height())), CornerRadius::same(2), color);
    }
    (resp, toggled)
}

fn favorite_sym(l: &Location) -> Sym {
    let Location::Local(p) = l else { return Sym::Server };
    for (key, path) in crate::platform::standard_folders() {
        if *p == path {
            return match key {
                "home" => Sym::Home,
                "downloads" => Sym::Download,
                "documents" => Sym::Document,
                "pictures" => Sym::Image,
                "camera" => Sym::Camera,
                "music" => Sym::Music,
                "movies" => Sym::Film,
                "desktop" => Sym::Desktop,
                _ => Sym::Folder,
            };
        }
    }
    Sym::Folder
}

fn favorites(app: &mut App, ui: &mut Ui) {
    let p = app.active_pane;
    let here = app.tab(p).location().clone();
    let home = Location::Local(crate::platform::home());
    let resp = row(app, ui, 0, Sym::Home, t("home"), here == home, None);
    app.drop_targets.push(DropTarget { rect: resp.rect, dest: home.clone(), priority: 3, trash: false });
    if resp.clicked() {
        app.actions.push(Action::Navigate(p, home));
    }
    let favs = app.settings.favorites.clone();
    for (i, f) in favs.iter().enumerate() {
        let resp = row(app, ui, 0, favorite_sym(&f.location), &f.name, here == f.location, None);
        app.drop_targets.push(DropTarget { rect: resp.rect, dest: f.location.clone(), priority: 3, trash: false });
        if resp.clicked() {
            app.actions.push(Action::Navigate(p, f.location.clone()));
        }
        if resp.middle_clicked() {
            app.actions.push(Action::OpenInNewTab(f.location.clone()));
        }
        let resp = resp.on_hover_text(f.location.address(&app.settings.servers));
        resp.context_menu(|ui| {
            if ui.button(t("open_new_tab")).clicked() {
                app.actions.push(Action::OpenInNewTab(f.location.clone()));
            }
            if ui.button(t("open_other_pane")).clicked() {
                app.actions.push(Action::OpenInOtherPane(f.location.clone()));
            }
            ui.separator();
            if i > 0 && ui.button(t("move_up")).clicked() {
                app.actions.push(Action::MoveFavorite(i, true));
            }
            if i + 1 < favs.len() && ui.button(t("move_down")).clicked() {
                app.actions.push(Action::MoveFavorite(i, false));
            }
            if ui.button(t("remove_favorite")).clicked() {
                app.actions.push(Action::Unpin(i));
            }
        });
    }
    // Dropping a folder here (below the list) pins it.
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 10.0), Sense::hover());
    if let Some(payload) = egui::DragAndDrop::payload::<DragPayload>(ui.ctx()) {
        let over = ui.input(|i| i.pointer.hover_pos()).is_some_and(|pos| rect.expand2(vec2(0.0, 4.0)).contains(pos));
        if over {
            ui.painter().line_segment([rect.left_center(), rect.right_center()], Stroke::new(2.0, theme::ACCENT));
            if ui.input(|i| i.pointer.any_released()) {
                for l in payload.items.iter() {
                    if matches!(l, Location::Local(p) if p.is_dir()) || matches!(l, Location::Remote { .. }) {
                        app.actions.push(Action::Pin(l.clone()));
                    }
                }
                egui::DragAndDrop::clear_payload(ui.ctx());
            }
        }
    }
}

fn devices(app: &mut App, ui: &mut Ui) {
    let p = app.active_pane;
    let here = app.tab(p).location().clone();
    let volumes = app.volumes.clone();
    for v in &volumes {
        let loc = Location::Local(v.path.clone());
        let label = super::pane::volume_label(v);
        let s = if v.path == std::path::Path::new("/") { Sym::Root } else if v.removable { Sym::Sd } else { Sym::Drive };
        let usage = super::view::space_for(app, loc.clone()).filter(|(_, total)| *total > 0).map(|(free, total)| 1.0 - free as f32 / total as f32);
        tree_node(app, ui, 0, &loc, s, &label, &here, usage);
    }
}

/// A folder of the tree, and (when unfolded) its subfolders.
#[allow(clippy::too_many_arguments)]
fn tree_node(app: &mut App, ui: &mut Ui, depth: usize, loc: &Location, s: Sym, label: &str, here: &Location, usage: Option<f32>) {
    let p = app.active_pane;
    let expanded = app.tree_expanded.contains(loc);
    let (resp, toggled) = row_ex(app, ui, depth, s, label, here == loc, Some(expanded), usage);
    app.drop_targets.push(DropTarget { rect: resp.rect, dest: loc.clone(), priority: 3, trash: false });
    if toggled || (resp.double_clicked()) {
        if expanded {
            app.tree_expanded.remove(loc);
        } else {
            app.tree_expanded.insert(loc.clone());
            app.load_tree(loc);
        }
    } else if resp.clicked() {
        app.actions.push(Action::Navigate(p, loc.clone()));
    }
    if resp.middle_clicked() {
        app.actions.push(Action::OpenInNewTab(loc.clone()));
    }
    resp.context_menu(|ui| {
        if ui.button(t("open_new_tab")).clicked() {
            app.actions.push(Action::OpenInNewTab(loc.clone()));
        }
        if ui.button(t("open_other_pane")).clicked() {
            app.actions.push(Action::OpenInOtherPane(loc.clone()));
        }
        if ui.button(t("add_favorite")).clicked() {
            app.actions.push(Action::Pin(loc.clone()));
        }
        if ui.button(t("copy_path")).clicked() {
            app.actions.push(Action::CopyPath(vec![loc.clone()]));
        }
    });
    if !app.tree_expanded.contains(loc) {
        return;
    }
    let children = match app.tree.get(loc) {
        Some(TreeNode::Loaded(c)) => c.clone(),
        Some(TreeNode::Loading) => {
            ui.horizontal(|ui| {
                ui.add_space(30.0 + depth as f32 * 14.0);
                ui.add(egui::Spinner::new().size(12.0));
            });
            return;
        }
        Some(TreeNode::Failed) => return,
        None => {
            app.load_tree(loc);
            return;
        }
    };
    for (name, child) in children.iter().take(500) {
        tree_node(app, ui, depth + 1, child, Sym::Folder, name, here, None);
    }
}

fn network(app: &mut App, ui: &mut Ui) {
    let p = app.active_pane;
    let here = app.tab(p).location().clone();
    let servers = app.settings.servers.clone();
    for s in &servers {
        let start = s.start();
        let inside = matches!(&here, Location::Remote { server, .. } if *server == s.id);
        let connected = vfs::servers::is_connected(s.id);
        let (resp, _) = row_ex(app, ui, 0, Sym::Server, &s.display_name(), inside, None, None);
        if connected {
            ui.painter().circle_filled(pos2(resp.rect.right() - 12.0, resp.rect.center().y), 3.5, Color32::from_rgb(0x34, 0xc7, 0x59));
        }
        app.drop_targets.push(DropTarget { rect: resp.rect, dest: start.clone(), priority: 3, trash: false });
        if resp.clicked() {
            app.actions.push(Action::Connect(s.id));
        }
        let resp = resp.on_hover_text(s.url_base());
        resp.context_menu(|ui| {
            if ui.button(t("connect")).clicked() {
                app.actions.push(Action::Connect(s.id));
            }
            if ui.button(t("open_new_tab")).clicked() {
                app.actions.push(Action::OpenInNewTab(start.clone()));
            }
            if ui.button(t("edit")).clicked() {
                app.actions.push(Action::EditServer(Some(s.id)));
            }
            if connected && ui.button(t("disconnect")).clicked() {
                app.actions.push(Action::Disconnect(s.id));
            }
            if ui.button(t("add_favorite")).clicked() {
                app.actions.push(Action::Pin(start.clone()));
            }
            ui.separator();
            if ui.button(t("remove_server")).clicked() {
                app.actions.push(Action::RemoveServer(s.id));
            }
        });
    }
    let resp = row(app, ui, 0, Sym::Plus, t("connect_server"), false, None);
    if resp.clicked() {
        app.actions.push(Action::EditServer(None));
    }
}
