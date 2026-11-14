//! Settings: a window with the sections in a menu on the left (like the app's sidebar) and their
//! options on the right, grouped in cards. Sections: General, View, Appearance, Network,
//! Default apps (by extension or by app) and Tags (the user's own, with their colours).

use egui::{
    Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2,
};

use super::app::{Action, App};
use super::dialogs::Dialog;
use super::icons::{self, Sym};
use super::theme;
use crate::i18n::{t, tf};
use crate::settings::{Language, StartMode, TagDef, ThemeChoice, ViewMode};
use crate::vfs::Location;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    General,
    View,
    Appearance,
    Network,
    Apps,
    Tags,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Tab::General,
        Tab::View,
        Tab::Appearance,
        Tab::Network,
        Tab::Apps,
        Tab::Tags,
    ];

    fn key(self) -> &'static str {
        match self {
            Tab::General => "settings_general",
            Tab::View => "view",
            Tab::Appearance => "appearance",
            Tab::Network => "network",
            Tab::Apps => "default_apps",
            Tab::Tags => "tags",
        }
    }

    /// The name in the menu (shorter where the title is long).
    fn menu_key(self) -> &'static str {
        match self {
            Tab::Apps => "settings_apps",
            other => other.key(),
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Tab::General => "settings_general_hint",
            Tab::View => "settings_view_hint",
            Tab::Appearance => "settings_appearance_hint",
            Tab::Network => "settings_network_hint",
            Tab::Apps => "default_apps_hint",
            Tab::Tags => "settings_tags_hint",
        }
    }

    fn sym(self) -> Sym {
        match self {
            Tab::General => Sym::Home,
            Tab::View => Sym::Details,
            Tab::Appearance => Sym::Contrast,
            Tab::Network => Sym::Network,
            Tab::Apps => Sym::Apps,
            Tab::Tags => Sym::Tag,
        }
    }
}

/// The settings' window state kept between frames.
#[derive(Default)]
pub struct State {
    pub tab: Tab,
    /// Default apps grouped by app (otherwise listed by extension).
    pub by_app: bool,
    /// The extension being typed to choose an app for it.
    pub new_ext: String,
    /// A tag whose deletion waits for confirmation.
    pub delete_tag: Option<u8>,
}

/// The settings window; false when it closes.
pub fn show(app: &mut App, ctx: &egui::Context) -> bool {
    let pal = theme::palette(app.dark);
    let screen = ctx.content_rect();
    let size = vec2(
        (screen.width() - 24.0).min(880.0),
        (screen.height() - 24.0).min(640.0),
    );
    let mut keep = true;
    let mut next: Option<Dialog> = None;
    let frame = egui::Frame::NONE
        .fill(pal.content)
        .corner_radius(12)
        .stroke(Stroke::new(1.0, pal.separator))
        .shadow(ctx.global_style().visuals.popup_shadow);
    let resp = egui::Modal::new(egui::Id::unique("settings-window"))
        .frame(frame)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let compact = size.x < 600.0;
            let menu_w = if compact { 56.0 } else { 210.0 };
            let menu = Rect::from_min_max(rect.min, pos2(rect.left() + menu_w, rect.bottom()));
            let body = Rect::from_min_max(pos2(menu.right(), rect.top()), rect.max);
            // The menu, on the sidebar's colour (rounded on the window's left corners).
            ui.painter().rect_filled(
                menu,
                CornerRadius {
                    nw: 12,
                    sw: 12,
                    ne: 0,
                    se: 0,
                },
                pal.sidebar,
            );
            ui.painter().line_segment(
                [menu.right_top(), menu.right_bottom()],
                Stroke::new(1.0, pal.separator),
            );
            let mut m =
                ui.new_child(egui::UiBuilder::new().max_rect(menu.shrink2(vec2(8.0, 12.0))));
            menu_ui(app, &mut m, compact);
            // The section: title, what it is about, the options, and Close.
            let inner = body.shrink2(vec2(if compact { 12.0 } else { 24.0 }, 18.0));
            let mut b = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            let tab = app.settings_ui.tab;
            b.label(RichText::new(t(tab.key())).size(20.0).strong());
            b.label(RichText::new(t(tab.hint())).small().color(pal.weak));
            b.add_space(10.0);
            let footer = 40.0;
            let scroll_h = (inner.bottom() - b.cursor().top() - footer).max(60.0);
            egui::ScrollArea::vertical()
                .id_salt(("settings-body", tab as u8))
                .max_height(scroll_h)
                .auto_shrink([false, false])
                .show(&mut b, |ui| {
                    ui.set_width(ui.available_width() - 6.0);
                    match tab {
                        Tab::General => general(app, ui),
                        Tab::View => view(app, ui),
                        Tab::Appearance => appearance(app, ui),
                        Tab::Network => network(app, ui),
                        Tab::Apps => apps(app, ui, &mut next),
                        Tab::Tags => tags(app, ui),
                    }
                    ui.add_space(8.0);
                });
            let close_r = Rect::from_min_max(
                pos2(inner.right() - 110.0, inner.bottom() - 30.0),
                inner.right_bottom(),
            );
            if b.put(
                close_r,
                egui::Button::new(RichText::new(t("close")).color(Color32::WHITE))
                    .fill(theme::ACCENT)
                    .corner_radius(6),
            )
            .clicked()
            {
                keep = false;
            }
        });
    if let Some(d) = next {
        app.dialog = Some(d);
        return false;
    }
    keep && !resp.should_close()
}

/// The sections, as rows like the sidebar's.
fn menu_ui(app: &mut App, ui: &mut Ui, compact: bool) {
    let pal = theme::palette(app.dark);
    if !compact {
        ui.label(RichText::new(t("settings")).size(15.0).strong());
        ui.add_space(10.0);
    }
    ui.spacing_mut().item_spacing.y = 2.0;
    for tab in Tab::ALL {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
        let selected = app.settings_ui.tab == tab;
        if selected {
            ui.painter()
                .rect_filled(r, CornerRadius::same(7), pal.selection_inactive);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(r, CornerRadius::same(7), pal.hover);
        }
        let ir = Rect::from_center_size(
            pos2(
                if compact {
                    r.center().x
                } else {
                    r.left() + 18.0
                },
                r.center().y,
            ),
            Vec2::splat(18.0),
        );
        icons::sym(ui.painter(), ir, tab.sym(), theme::ACCENT);
        if !compact {
            ui.painter().text(
                pos2(ir.right() + 10.0, r.center().y),
                Align2::LEFT_CENTER,
                t(tab.menu_key()),
                FontId::proportional(14.0),
                ui.visuals().text_color(),
            );
        }
        let resp = if compact {
            resp.on_hover_text(t(tab.key()))
        } else {
            resp
        };
        if resp.clicked() {
            app.settings_ui.tab = tab;
            app.settings_ui.delete_tag = None;
        }
    }
}

/// A group's small title.
fn caption(dark: bool, ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .color(theme::palette(dark).weak),
    );
    ui.add_space(2.0);
}

/// A card of options.
fn card<R>(dark: bool, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let pal = theme::palette(dark);
    egui::Frame::new()
        .fill(pal.sidebar)
        .stroke(Stroke::new(1.0, pal.separator))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// One row of a card: what it is (and a line explaining it) on the left, its control on the right.
fn row(ui: &mut Ui, label: &str, hint: Option<&str>, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(34.0);
        // The control first (from the right), so the text takes what is left and wraps.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            control(ui);
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.add_space(if hint.is_some() { 3.0 } else { 8.0 });
                ui.add(egui::Label::new(label).wrap());
                if let Some(h) = hint {
                    ui.add(egui::Label::new(RichText::new(h).small().weak()).wrap());
                }
                ui.add_space(3.0);
            });
        });
    });
}

fn divider(dark: bool, ui: &mut Ui) {
    let r = ui.available_rect_before_wrap();
    let y = ui.cursor().top() + 2.0;
    ui.painter().line_segment(
        [pos2(r.left(), y), pos2(r.right(), y)],
        Stroke::new(1.0, theme::palette(dark).separator),
    );
    ui.add_space(5.0);
}

/// An on/off switch; true when it changed.
fn switch(ui: &mut Ui, on: &mut bool) -> bool {
    let size = vec2(38.0, 22.0);
    let (r, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let off = if ui.visuals().dark_mode {
        Color32::from_gray(80)
    } else {
        Color32::from_gray(200)
    };
    let fill = egui::lerp(egui::Rgba::from(off)..=egui::Rgba::from(theme::ACCENT), t);
    ui.painter()
        .rect_filled(r, CornerRadius::same(11), Color32::from(fill));
    let x = egui::lerp(r.left() + 11.0..=r.right() - 11.0, t);
    ui.painter()
        .circle_filled(pos2(x, r.center().y), 8.5, Color32::WHITE);
    resp.changed()
}

/// Buttons side by side, one of them chosen.
fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, String)]) -> bool {
    let mut changed = false;
    ui.spacing_mut().item_spacing.x = 2.0;
    // Right to left in a row: add them reversed so they read in order.
    for (v, label) in options.iter().rev() {
        let on = *value == *v;
        let b = egui::Button::new(if on {
            RichText::new(label).color(Color32::WHITE)
        } else {
            RichText::new(label)
        })
        .fill(if on {
            theme::ACCENT
        } else {
            ui.visuals().widgets.inactive.weak_bg_fill
        })
        .corner_radius(6);
        if ui.add(b).clicked() && !on {
            *value = *v;
            changed = true;
        }
    }
    changed
}

fn general(app: &mut App, ui: &mut Ui) {
    let here = app.tab(app.active_pane).location().clone();
    caption(app.dark, ui, t("on_start"));
    card(app.dark, ui, |ui| {
        let s = &mut app.settings;
        for (i, (mode, key)) in [
            (StartMode::Home, "start_home"),
            (StartMode::Downloads, "start_downloads"),
            (StartMode::LastSession, "start_last"),
            (StartMode::Folder, "start_folder_opt"),
        ]
        .into_iter()
        .enumerate()
        {
            if i > 0 {
                ui.add_space(2.0);
            }
            ui.horizontal(|ui| {
                ui.set_min_height(30.0);
                ui.radio_value(&mut s.start, mode, t(key));
                if mode == StartMode::Folder {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Location::Local(p) = &here
                            && ui.button(t("use_current")).clicked()
                        {
                            s.start = StartMode::Folder;
                            s.start_folder = p.display().to_string();
                        }
                        ui.add_enabled(
                            s.start == StartMode::Folder,
                            egui::TextEdit::singleline(&mut s.start_folder)
                                .hint_text("/storage/emulated/0/…")
                                .desired_width(ui.available_width().min(320.0)),
                        );
                    });
                }
            });
        }
    });
    caption(app.dark, ui, t("search"));
    card(app.dark, ui, |ui| {
        let s = &mut app.settings;
        row(ui, t("search_while_typing"), None, |ui| {
            switch(ui, &mut s.search_subfolders);
        });
    });
}

fn view(app: &mut App, ui: &mut Ui) {
    let mut dirty = false;
    caption(app.dark, ui, t("settings_files"));
    card(app.dark, ui, |ui| {
        let s = &mut app.settings;
        row(ui, t("default_view"), None, |ui| {
            egui::ComboBox::from_id_salt("settings-default-view")
                .selected_text(t(s.view.key()))
                .show_ui(ui, |ui| {
                    for v in ViewMode::ALL {
                        dirty |= ui.selectable_value(&mut s.view, v, t(v.key())).changed();
                    }
                });
        });
        divider_raw(ui);
        row(ui, t("show_hidden"), None, |ui| {
            dirty |= switch(ui, &mut s.show_hidden);
        });
        divider_raw(ui);
        row(ui, t("show_extensions"), None, |ui| {
            dirty |= switch(ui, &mut s.show_extensions);
        });
        divider_raw(ui);
        row(ui, t("folders_first"), None, |ui| {
            dirty |= switch(ui, &mut s.folders_first);
        });
    });
    caption(app.dark, ui, t("settings_folders"));
    card(app.dark, ui, |ui| {
        let s = &mut app.settings;
        row(ui, t("show_folder_sizes"), None, |ui| {
            dirty |= switch(ui, &mut s.folder_sizes);
        });
        divider_raw(ui);
        row(ui, t("remember_views"), None, |ui| {
            dirty |= switch(ui, &mut s.remember_folder_views);
            if !s.folder_views.is_empty()
                && ui
                    .small_button(tf("forget_views", s.folder_views.len()))
                    .clicked()
            {
                s.folder_views.clear();
                dirty = true;
            }
        });
    });
    caption(app.dark, ui, t("settings_thumbnails"));
    card(app.dark, ui, |ui| {
        let s = &mut app.settings;
        row(ui, t("show_thumbnails"), None, |ui| {
            switch(ui, &mut s.thumbnails);
        });
        divider_raw(ui);
        row(ui, t("remote_thumbnails"), None, |ui| {
            switch(ui, &mut s.remote_thumbnails);
        });
    });
    if dirty {
        app.mark_dirty();
    }
}

/// A divider inside a card (without the app, for closures that hold the settings).
fn divider_raw(ui: &mut Ui) {
    let r = ui.available_rect_before_wrap();
    let y = ui.cursor().top() + 1.0;
    ui.painter().line_segment(
        [pos2(r.left(), y), pos2(r.right(), y)],
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
    ui.add_space(3.0);
}

fn appearance(app: &mut App, ui: &mut Ui) {
    caption(app.dark, ui, t("settings_interface"));
    card(app.dark, ui, |ui| {
        let s = &mut app.settings;
        row(ui, t("theme"), None, |ui| {
            segmented(
                ui,
                &mut s.theme,
                &[
                    (ThemeChoice::System, t("theme_system").to_owned()),
                    (ThemeChoice::Light, t("theme_light").to_owned()),
                    (ThemeChoice::Dark, t("theme_dark").to_owned()),
                ],
            );
        });
        divider_raw(ui);
        row(ui, t("language"), None, |ui| {
            if segmented(
                ui,
                &mut s.language,
                &[
                    (Language::Auto, t("lang_auto").to_owned()),
                    (Language::Spanish, "Español".to_owned()),
                    (Language::English, "English".to_owned()),
                ],
            ) {
                crate::i18n::set_language(s.language);
            }
        });
        divider_raw(ui);
        row(ui, t("zoom"), Some(t("zoom_hint")), |ui| {
            for (label, delta) in [("+", 0.2f32), ("−", -0.2)] {
                if ui.small_button(label).clicked() {
                    s.zoom = ((s.zoom + delta) * 5.0).round().clamp(3.0, 12.0) / 5.0;
                }
            }
            // Steps of 20 %, applied when the slider is let go: applying it while dragging moves
            // the slider under the pointer, and it runs away.
            let id = egui::Id::unique("zoom-edit");
            let mut v: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(s.zoom);
            let r = ui.add(
                egui::Slider::new(&mut v, 0.6..=2.4)
                    .step_by(0.2)
                    .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
            );
            if r.dragged() {
                ui.data_mut(|d| d.insert_temp(id, v));
            } else {
                if (v - s.zoom).abs() > 0.001 {
                    s.zoom = (v * 5.0).round() / 5.0;
                }
                ui.data_mut(|d| d.remove::<f32>(id));
            }
        });
    });
}

fn network(app: &mut App, ui: &mut Ui) {
    let pal = theme::palette(app.dark);
    let servers = app.settings.servers.clone();
    caption(app.dark, ui, t("settings_servers"));
    card(app.dark, ui, |ui| {
        if servers.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(t("no_servers")).color(pal.weak));
            ui.add_space(6.0);
        }
        for (i, srv) in servers.iter().enumerate() {
            if i > 0 {
                divider(app.dark, ui);
            }
            ui.horizontal(|ui| {
                ui.set_min_height(40.0);
                let (ir, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                icons::sym(ui.painter(), ir, Sym::Server, theme::ACCENT);
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    ui.label(srv.display_name());
                    ui.label(RichText::new(srv.url_base()).small().color(pal.weak));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t("remove_server")).clicked() {
                        app.actions.push(Action::RemoveServer(srv.id));
                    }
                    if ui.button(t("edit")).clicked() {
                        app.actions.push(Action::EditServer(Some(srv.id)));
                    }
                });
            });
        }
    });
    ui.add_space(8.0);
    if ui.button(format!("+  {}", t("connect_server"))).clicked() {
        app.actions.push(Action::EditServer(None));
    }
}

fn apps(app: &mut App, ui: &mut Ui, next: &mut Option<Dialog>) {
    let pal = theme::palette(app.dark);
    ui.horizontal(|ui| {
        ui.label(t("apps_show"));
        let mut by_app = app.settings_ui.by_app;
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            for (v, key) in [(false, "apps_by_ext"), (true, "apps_by_app")] {
                let on = by_app == v;
                let b = egui::Button::new(if on {
                    RichText::new(t(key)).color(Color32::WHITE)
                } else {
                    RichText::new(t(key))
                })
                .fill(if on {
                    theme::ACCENT
                } else {
                    ui.visuals().widgets.inactive.weak_bg_fill
                })
                .corner_radius(6);
                if ui.add(b).clicked() {
                    by_app = v;
                }
            }
        });
        app.settings_ui.by_app = by_app;
    });
    ui.add_space(6.0);
    let mut defaults = app.settings.default_apps.clone();
    defaults.sort_by(|a, b| {
        a.ext
            .cmp(&b.ext)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    // (extension, app, on) switched this frame.
    let mut set: Vec<(String, String, bool)> = Vec::new();
    card(app.dark, ui, |ui| {
        if defaults.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(t("no_default_apps")).color(pal.weak));
            ui.add_space(6.0);
        } else if !app.settings_ui.by_app {
            // By extension: one row each, with its switch.
            for (i, d) in defaults.iter().enumerate() {
                if i > 0 {
                    divider_raw(ui);
                }
                ui.horizontal(|ui| {
                    ui.set_min_height(34.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut on = d.enabled;
                        if switch(ui, &mut on) {
                            set.push((d.ext.clone(), d.app.clone(), on));
                        }
                        if ui.small_button(t("change")).clicked() {
                            *next = Some(Dialog::open_with(d.ext.clone(), None));
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ext_badge(ui, &d.ext);
                            ui.add_space(8.0);
                            let text = if d.enabled {
                                RichText::new(&d.label)
                            } else {
                                RichText::new(&d.label).color(pal.weak)
                            };
                            ui.add(egui::Label::new(text).truncate());
                        });
                    });
                });
            }
        } else {
            // By app: each app with a switch for all its extensions, and under it each one.
            let mut groups: Vec<AppGroup> = Vec::new();
            for d in &defaults {
                let item = (d.ext.clone(), d.enabled);
                match groups.iter_mut().find(|g| g.0 == d.app) {
                    Some(g) => g.2.push(item),
                    None => groups.push((d.app.clone(), d.label.clone(), vec![item])),
                }
            }
            groups.sort_by_key(|g| g.1.to_lowercase());
            for (i, (id, label, exts)) in groups.iter().enumerate() {
                if i > 0 {
                    divider_raw(ui);
                }
                let state = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    ui.make_persistent_id(("apps-by-app", id)),
                    true,
                );
                let any_on = exts.iter().any(|(_, on)| *on);
                state
                    .show_header(ui, |ui| {
                        ui.set_min_height(32.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let mut on = any_on;
                            if switch(ui, &mut on) {
                                set.extend(exts.iter().map(|(e, _)| (e.clone(), id.clone(), on)));
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let (ir, _) =
                                        ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                                    icons::sym(ui.painter(), ir, Sym::Apps, theme::ACCENT);
                                    let name = RichText::new(label).strong();
                                    ui.label(if any_on { name } else { name.color(pal.weak) });
                                    let on_count = exts.iter().filter(|(_, on)| *on).count();
                                    ui.label(
                                        RichText::new(tf2("n_of_m_on", on_count, exts.len()))
                                            .small()
                                            .color(pal.weak),
                                    );
                                },
                            );
                        });
                    })
                    .body(|ui| {
                        for (e, enabled) in exts {
                            ui.horizontal(|ui| {
                                ui.set_min_height(30.0);
                                // The tree's guide.
                                let r = ui.available_rect_before_wrap();
                                ui.painter().line_segment(
                                    [
                                        pos2(r.left() + 2.0, r.top()),
                                        pos2(r.left() + 2.0, r.top() + 30.0),
                                    ],
                                    Stroke::new(1.0, pal.separator),
                                );
                                ui.add_space(14.0);
                                ext_badge(ui, e);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let mut on = *enabled;
                                        if switch(ui, &mut on) {
                                            set.push((e.clone(), id.clone(), on));
                                        }
                                        if ui.small_button(t("change")).clicked() {
                                            *next = Some(Dialog::open_with(e.clone(), None));
                                        }
                                    },
                                );
                            });
                        }
                    });
            }
        }
    });
    for (ext, id, on) in set {
        app.settings.switch_app(&ext, &id, on);
    }
    ui.add_space(4.0);
    ui.label(RichText::new(t("apps_switch_hint")).small().color(pal.weak));
    caption(app.dark, ui, t("apps_add"));
    card(app.dark, ui, |ui| {
        row(ui, t("extension"), Some(t("apps_add_hint")), |ui| {
            let clean = app
                .settings_ui
                .new_ext
                .trim()
                .trim_start_matches('.')
                .to_lowercase();
            if ui
                .add_enabled(!clean.is_empty(), egui::Button::new(t("choose_app")))
                .clicked()
            {
                *next = Some(Dialog::open_with(clean, None));
                app.settings_ui.new_ext.clear();
            }
            ui.add(
                egui::TextEdit::singleline(&mut app.settings_ui.new_ext)
                    .hint_text("pdf")
                    .desired_width(80.0),
            );
        });
        divider_raw(ui);
        row(
            ui,
            t("dex_apps_again"),
            Some(t("dex_apps_again_hint")),
            |ui| {
                if ui.button(t("apply")).clicked() {
                    app.settings.dex_offered.clear();
                    app.rescan_dex_apps();
                }
            },
        );
    });
}

/// An app (id, name) and its extensions, each switched on or off.
type AppGroup = (String, String, Vec<(String, bool)>);

/// A string with two `{}`.
fn tf2(key: &str, a: usize, b: usize) -> String {
    t(key)
        .replacen("{}", &a.to_string(), 1)
        .replacen("{}", &b.to_string(), 1)
}

/// `.ext` in a small rounded badge.
fn ext_badge(ui: &mut Ui, ext: &str) {
    let text = format!(".{ext}");
    let g = ui
        .painter()
        .layout_no_wrap(text, FontId::monospace(12.0), ui.visuals().text_color());
    let (r, _) = ui.allocate_exact_size(vec2(g.size().x + 14.0, 22.0), Sense::hover());
    ui.painter().rect_filled(
        r,
        CornerRadius::same(5),
        ui.visuals().widgets.inactive.weak_bg_fill,
    );
    ui.painter().galley(
        pos2(r.left() + 7.0, r.center().y - g.size().y / 2.0),
        g,
        ui.visuals().text_color(),
    );
}

fn tags(app: &mut App, ui: &mut Ui) {
    let pal = theme::palette(app.dark);
    let mut counts = [0usize; 256];
    for v in app.settings.tags.values() {
        for &c in v {
            counts[c as usize] += 1;
        }
    }
    let mut defs = app.settings.tag_defs.clone();
    let mut changed = false;
    let mut delete: Option<u8> = None;
    let mut swap: Option<(usize, usize)> = None;
    caption(app.dark, ui, t("settings_your_tags"));
    card(app.dark, ui, |ui| {
        if defs.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(t("no_tags_yet")).color(pal.weak));
            ui.add_space(6.0);
        }
        let n = defs.len();
        for (i, d) in defs.iter_mut().enumerate() {
            if i > 0 {
                divider_raw(ui);
            }
            ui.horizontal(|ui| {
                ui.set_min_height(36.0);
                // The colour: a swatch that opens a colour picker.
                let mut rgb = d.color;
                if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                    d.color = rgb;
                    changed = true;
                }
                let default = crate::tags::default_name(d.id);
                // The buttons take about 110 points; on a narrow window the count goes.
                let room = ui.available_width() - 110.0;
                let wide = room > 300.0;
                let r = ui.add(
                    egui::TextEdit::singleline(&mut d.name)
                        .hint_text(default)
                        .desired_width(if wide { 200.0 } else { (room - 8.0).max(60.0) }),
                );
                changed |= r.changed();
                if wide {
                    ui.label(
                        RichText::new(tf("n_items", counts[d.id as usize]))
                            .small()
                            .color(pal.weak),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if app.settings_ui.delete_tag == Some(d.id) {
                        if ui
                            .add(
                                egui::Button::new(RichText::new(t("delete")).color(Color32::WHITE))
                                    .fill(Color32::from_rgb(0xe5, 0x39, 0x35)),
                            )
                            .clicked()
                        {
                            delete = Some(d.id);
                        }
                        if ui.small_button(t("cancel")).clicked() {
                            app.settings_ui.delete_tag = None;
                        }
                        ui.label(RichText::new(t("delete_tag_q")).small());
                    } else {
                        if icons::sym_button(ui, Sym::Trash, false, t("delete")).clicked() {
                            if counts[d.id as usize] == 0 {
                                delete = Some(d.id);
                            } else {
                                app.settings_ui.delete_tag = Some(d.id);
                            }
                        }
                        ui.add_enabled_ui(i + 1 < n, |ui| {
                            if icons::sym_button(ui, Sym::Down, false, t("move_down")).clicked() {
                                swap = Some((i, i + 1));
                            }
                        });
                        ui.add_enabled_ui(i > 0, |ui| {
                            if icons::sym_button(ui, Sym::Up, false, t("move_up")).clicked() {
                                swap = Some((i, i - 1));
                            }
                        });
                    }
                });
            });
        }
    });
    if let Some((a, b)) = swap {
        defs.swap(a, b);
        changed = true;
    }
    ui.add_space(8.0);
    if ui.button(format!("+  {}", t("new_tag"))).clicked()
        && let Some(id) = crate::tags::free_id(&defs, &app.settings.tags)
    {
        // A colour no tag has yet, if there is one.
        let color = crate::tags::PALETTE
            .iter()
            .find(|c| !defs.iter().any(|d| d.color == **c))
            .copied()
            .unwrap_or(crate::tags::PALETTE[defs.len() % crate::tags::PALETTE.len()]);
        defs.push(TagDef {
            id,
            name: tf("new_tag_name", defs.len() + 1),
            color,
        });
        changed = true;
    }
    ui.add_space(6.0);
    ui.label(RichText::new(t("tags_hint")).small().color(pal.weak));
    if let Some(id) = delete {
        defs.retain(|d| d.id != id);
        for v in app.settings.tags.values_mut() {
            v.retain(|c| *c != id);
        }
        app.settings.tags.retain(|_, v| !v.is_empty());
        app.settings_ui.delete_tag = None;
        // Tabs showing it go home.
        for p in 0..2 {
            if *app.tab(p).location() == Location::Tag(id) {
                app.actions.push(Action::Navigate(
                    p,
                    Location::Local(crate::platform::home()),
                ));
            }
        }
        changed = true;
    }
    if changed {
        app.settings.tag_defs = defs;
        app.tags_changed();
    }
}
