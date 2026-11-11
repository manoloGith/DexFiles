//! Dialogs: names (new folder, rename), confirmations, name conflicts of running jobs, network
//! locations and passwords, settings, properties, compression.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use egui::{RichText, Ui};

use super::app::{Action, App};
use super::theme;
use crate::archive::ArchiveFormat;
use crate::fileinfo;
use crate::i18n::{t, tf};
use crate::ops::{Answer, Conflict};
use crate::settings::{Language, StartMode, ThemeChoice, ViewMode};
use crate::trash::TrashInfo;
use crate::vfs::{self, Entry, Location, Server, ServerId, ServerKind};

pub enum NameKind {
    NewFolder { dest: Location, file: bool },
    Rename { from: Location },
}

pub enum Dialog {
    Name {
        kind: NameKind,
        text: String,
        original_ext: String,
        keep_ext: Option<String>,
        focus: bool,
    },
    ConfirmDelete {
        items: Vec<Location>,
        remote: bool,
    },
    ConfirmPurge {
        items: Vec<TrashInfo>,
    },
    ConfirmEmptyTrash,
    ConfirmClose,
    Server {
        server: Server,
        show_password: bool,
        is_new: bool,
    },
    Password {
        id: ServerId,
        text: String,
    },
    Settings,
    Properties(Box<Entry>),
    FolderSize {
        entry: Box<Entry>,
        bytes: Arc<AtomicU64>,
        items: Arc<AtomicU64>,
        done: Arc<AtomicBool>,
    },
    Compress {
        items: Vec<Location>,
        name: String,
        format: ArchiveFormat,
    },
    Message {
        title: String,
        text: String,
    },
    About,
}

impl Dialog {
    pub fn new_folder(dest: Location, file: bool) -> Dialog {
        let base = if file {
            t("new_text_file_name")
        } else {
            t("new_folder_name")
        };
        let taken = |n: &str| dest.as_local().is_some_and(|d| d.join(n).exists());
        let text = crate::ops::unique_name(base, taken);
        Dialog::Name {
            kind: NameKind::NewFolder { dest, file },
            text,
            original_ext: String::new(),
            keep_ext: None,
            focus: true,
        }
    }

    /// With extensions hidden, only the name is edited and the extension is kept.
    pub fn rename(e: &Entry, show_extensions: bool) -> Dialog {
        let (text, keep_ext) = if !show_extensions && !e.is_dir() && !e.ext.is_empty() {
            (vfs::stem(&e.name).to_owned(), Some(e.ext.clone()))
        } else {
            (e.name.clone(), None)
        };
        let original_ext = if e.is_dir() {
            String::new()
        } else {
            e.ext.clone()
        };
        Dialog::Name {
            kind: NameKind::Rename {
                from: e.location.clone(),
            },
            text,
            original_ext,
            keep_ext,
            focus: true,
        }
    }

    pub fn compress(items: Vec<Location>) -> Dialog {
        let name = if items.len() == 1 {
            vfs::stem(&items[0].name()).to_owned()
        } else {
            t("archive_name").to_owned()
        };
        Dialog::Compress {
            items,
            name,
            format: ArchiveFormat::Zip,
        }
    }

    pub fn server(server: Server) -> Dialog {
        let is_new = vfs::servers::server(server.id).is_none();
        Dialog::Server {
            server,
            show_password: false,
            is_new,
        }
    }

    pub fn password(id: ServerId) -> Dialog {
        Dialog::Password {
            id,
            text: String::new(),
        }
    }
}

fn modal(ctx: &egui::Context, id: &str, width: f32, add: impl FnOnce(&mut Ui)) -> bool {
    let r = egui::Modal::new(egui::Id::unique(id)).show(ctx, |ui| {
        ui.set_width(width.min(ctx.content_rect().width() - 40.0));
        add(ui);
    });
    r.should_close()
}

fn buttons(ui: &mut Ui, ok: &str, danger: bool) -> (bool, bool) {
    let mut out = (false, false);
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let fill = if danger {
            egui::Color32::from_rgb(0xe5, 0x39, 0x35)
        } else {
            theme::ACCENT
        };
        if ui
            .add(
                egui::Button::new(RichText::new(ok).color(egui::Color32::WHITE))
                    .fill(fill)
                    .min_size(egui::vec2(90.0, 28.0)),
            )
            .clicked()
        {
            out.0 = true;
        }
        if ui
            .add(egui::Button::new(t("cancel")).min_size(egui::vec2(90.0, 28.0)))
            .clicked()
        {
            out.1 = true;
        }
    });
    out
}

fn valid_name(name: &str) -> Result<(), &'static str> {
    let n = name.trim();
    if n.is_empty() {
        return Err("name_empty");
    }
    if n == "." || n == ".." || n.contains('/') || n.contains('\0') {
        return Err("name_invalid");
    }
    Ok(())
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    conflicts(app, ctx);
    let Some(mut dialog) = app.dialog.take() else {
        return;
    };
    let keep = match &mut dialog {
        Dialog::Name {
            kind,
            text,
            original_ext,
            keep_ext,
            focus,
        } => name_dialog(app, ctx, kind, text, original_ext, keep_ext, focus),
        Dialog::ConfirmDelete { items, remote } => {
            let mut keep = true;
            let close = modal(ctx, "confirm-delete", 420.0, |ui| {
                ui.heading(t("delete_forever_q"));
                let what = if items.len() == 1 {
                    format!("“{}”", items[0].name())
                } else {
                    tf("n_items", items.len())
                };
                ui.label(tf("delete_forever_text", what));
                if *remote {
                    ui.label(RichText::new(t("remote_no_trash")).small());
                }
                let (ok, cancel) = buttons(ui, t("delete"), true);
                if ok {
                    app.actions.push(Action::DeleteNow(items.clone()));
                    keep = false;
                }
                if cancel {
                    keep = false;
                }
            });
            keep && !close
        }
        Dialog::ConfirmPurge { items } => {
            let mut keep = true;
            let close = modal(ctx, "confirm-purge", 420.0, |ui| {
                ui.heading(t("delete_forever_q"));
                ui.label(tf("delete_forever_text", tf("n_items", items.len())));
                let (ok, cancel) = buttons(ui, t("delete"), true);
                if ok {
                    let items = std::mem::take(items);
                    app.start_job(crate::ops::JobKind::Purge { items });
                    keep = false;
                }
                if cancel {
                    keep = false;
                }
            });
            keep && !close
        }
        Dialog::ConfirmEmptyTrash => {
            let mut keep = true;
            let close = modal(ctx, "confirm-empty", 420.0, |ui| {
                ui.heading(t("empty_trash_q"));
                ui.label(tf("empty_trash_text", app.trash_count));
                let (ok, cancel) = buttons(ui, t("empty_trash"), true);
                if ok {
                    app.actions.push(Action::EmptyTrashNow);
                    keep = false;
                }
                if cancel {
                    keep = false;
                }
            });
            keep && !close
        }
        Dialog::ConfirmClose => {
            let mut keep = true;
            let close = modal(ctx, "confirm-close", 420.0, |ui| {
                ui.heading(t("close_q"));
                ui.label(t("close_jobs_text"));
                let (ok, cancel) = buttons(ui, t("close_anyway"), true);
                if ok {
                    for j in &app.jobs {
                        j.state.cancel();
                    }
                    app.close_now();
                    keep = false;
                }
                if cancel {
                    keep = false;
                }
            });
            keep && !close
        }
        Dialog::Server {
            server,
            show_password,
            is_new,
        } => server_dialog(app, ctx, server, show_password, *is_new),
        Dialog::Password { id, text } => {
            let mut keep = true;
            let name = vfs::servers::server(*id)
                .map(|s| s.display_name())
                .unwrap_or_default();
            let close = modal(ctx, "password", 380.0, |ui| {
                ui.heading(tf("password_for", name));
                let r = ui.add(
                    egui::TextEdit::singleline(text)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );
                r.request_focus();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let (ok, cancel) = buttons(ui, t("connect"), false);
                if ok || enter {
                    vfs::servers::set_session_password(*id, std::mem::take(text));
                    app.actions.push(Action::Connect(*id));
                    keep = false;
                }
                if cancel {
                    keep = false;
                }
            });
            keep && !close
        }
        Dialog::Settings => settings_dialog(app, ctx),
        Dialog::Properties(entry) => {
            let mut keep = true;
            let mut size_of: Option<Entry> = None;
            let close = modal(ctx, "properties", 460.0, |ui| {
                properties(app, ui, entry);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if entry.is_dir() && ui.button(t("calc_size")).clicked() {
                        size_of = Some((**entry).clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t("close")).clicked() {
                            keep = false;
                        }
                    });
                });
            });
            if let Some(e) = size_of {
                app.dialog = Some(folder_size(e));
                return;
            }
            keep && !close
        }
        Dialog::FolderSize {
            entry,
            bytes,
            items,
            done,
        } => {
            let mut keep = true;
            let close = modal(ctx, "folder-size", 420.0, |ui| {
                properties(app, ui, entry);
                ui.separator();
                ui.horizontal(|ui| {
                    if !done.load(Ordering::Relaxed) {
                        ui.add(egui::Spinner::new().size(14.0));
                        ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    }
                    ui.label(format!(
                        "{} · {}",
                        fileinfo::format_size(bytes.load(Ordering::Relaxed)),
                        tf("n_items", items.load(Ordering::Relaxed))
                    ));
                });
                if ui.button(t("close")).clicked() {
                    done.store(true, Ordering::Relaxed);
                    keep = false;
                }
            });
            if close {
                done.store(true, Ordering::Relaxed);
            }
            keep && !close
        }
        Dialog::Compress {
            items,
            name,
            format,
        } => {
            let mut keep = true;
            let close = modal(ctx, "compress", 420.0, |ui| {
                ui.heading(t("compress"));
                ui.label(tf("n_items", items.len()));
                ui.horizontal(|ui| {
                    ui.label(t("col_name"));
                    ui.add(egui::TextEdit::singleline(name).desired_width(200.0));
                    egui::ComboBox::from_id_salt("format")
                        .selected_text(format!(".{}", format.extension()))
                        .show_ui(ui, |ui| {
                            for f in ArchiveFormat::ALL {
                                ui.selectable_value(
                                    format,
                                    f,
                                    format!("{} (.{})", f.label(), f.extension()),
                                );
                            }
                        });
                });
                ui.small(t("compress_formats_hint"));
                let (ok, cancel) = buttons(ui, t("compress"), false);
                if ok
                    && valid_name(name).is_ok()
                    && let Some(parent) = items.first().and_then(Location::parent)
                {
                    let file = format!("{}.{}", name.trim(), format.extension());
                    let file = crate::ops::unique_name(&file, |n| {
                        parent.as_local().is_some_and(|d| d.join(n).exists())
                    });
                    app.actions.push(Action::CompressNow {
                        sources: items.clone(),
                        dest: parent.join(&file),
                        format: *format,
                    });
                    keep = false;
                }
                if cancel {
                    keep = false;
                }
            });
            keep && !close
        }
        Dialog::Message { title, text } => {
            let mut keep = true;
            let close = modal(ctx, "message", 460.0, |ui| {
                ui.heading(title.as_str());
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| ui.label(text.as_str()));
                ui.add_space(8.0);
                if ui.button(t("close")).clicked() {
                    keep = false;
                }
            });
            keep && !close
        }
        Dialog::About => {
            let mut keep = true;
            let close = modal(ctx, "about", 420.0, |ui| {
                ui.vertical_centered(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::Vec2::splat(72.0), egui::Sense::hover());
                    super::icons::folder(ui.painter(), r, theme::ACCENT);
                    ui.heading("DexFiles");
                    ui.label(format!("{} {}", t("version"), env!("CARGO_PKG_VERSION")));
                    ui.label(t("about_text"));
                    ui.hyperlink("https://github.com/manoloGith/DexFiles");
                });
                ui.add_space(8.0);
                if ui.button(t("close")).clicked() {
                    keep = false;
                }
            });
            keep && !close
        }
    };
    if keep && app.dialog.is_none() {
        app.dialog = Some(dialog);
    }
}

fn folder_size(entry: Entry) -> Dialog {
    let bytes = Arc::new(AtomicU64::new(0));
    let items = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let (b, n, d, loc) = (
        bytes.clone(),
        items.clone(),
        done.clone(),
        entry.location.clone(),
    );
    let _ = std::thread::Builder::new()
        .name("dexfiles-size".into())
        .spawn(move || {
            let mut stack = vec![loc];
            while let Some(dir) = stack.pop() {
                if d.load(Ordering::Relaxed) {
                    return;
                }
                let Ok(list) = vfs::list(&dir) else { continue };
                for e in list {
                    n.fetch_add(1, Ordering::Relaxed);
                    if e.is_dir() && !e.symlink {
                        stack.push(e.location);
                    } else {
                        b.fetch_add(e.size, Ordering::Relaxed);
                    }
                }
            }
            d.store(true, Ordering::Relaxed);
        });
    Dialog::FolderSize {
        entry: Box::new(entry),
        bytes,
        items,
        done,
    }
}

fn properties(app: &App, ui: &mut Ui, e: &Entry) {
    let pal = theme::palette(app.dark);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(48.0), egui::Sense::hover());
        super::icons::file_icon(
            ui.painter(),
            r,
            crate::fileinfo::Category::of(&e.ext, e.is_dir()),
            &e.ext,
            app.dark,
        );
        ui.vertical(|ui| {
            ui.label(RichText::new(&e.name).strong().size(16.0));
            ui.label(RichText::new(fileinfo::kind_label(&e.ext, e.is_dir())).color(pal.weak));
        });
    });
    ui.separator();
    egui::Grid::new("props")
        .num_columns(2)
        .spacing([12.0, 5.0])
        .show(ui, |ui| {
            let mut line = |k: &str, v: String| {
                ui.label(RichText::new(t(k)).color(pal.weak));
                ui.add(egui::Label::new(v).wrap());
                ui.end_row();
            };
            if !e.is_dir() {
                line(
                    "col_size",
                    format!("{} ({} B)", fileinfo::format_size(e.size), e.size),
                );
            }
            line(
                "where",
                e.location
                    .parent()
                    .map(|p| p.address(&app.settings.servers))
                    .unwrap_or_default(),
            );
            line("col_modified", fileinfo::format_date_long(e.modified));
            line("created", fileinfo::format_date_long(e.created));
            if let Some(tr) = &e.trash {
                line("original_location", tr.original.display().to_string());
                line("deleted_on", fileinfo::format_date_long(tr.deleted));
            }
            line(
                "readonly",
                if e.readonly { t("yes") } else { t("no") }.to_owned(),
            );
            line(
                "hidden",
                if e.hidden { t("yes") } else { t("no") }.to_owned(),
            );
            if e.symlink {
                let target = e
                    .location
                    .as_local()
                    .and_then(|p| std::fs::read_link(p).ok())
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                line("symlink", target);
            }
            #[cfg(unix)]
            if let Some(p) = e.location.as_local() {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(m) = std::fs::symlink_metadata(p) {
                    line(
                        "permissions",
                        format!("{:o}", m.permissions().mode() & 0o7777),
                    );
                }
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn name_dialog(
    app: &mut App,
    ctx: &egui::Context,
    kind: &mut NameKind,
    text: &mut String,
    original_ext: &str,
    keep_ext: &Option<String>,
    focus: &mut bool,
) -> bool {
    let mut keep = true;
    let title = match kind {
        NameKind::NewFolder { file: false, .. } => t("new_folder"),
        NameKind::NewFolder { file: true, .. } => t("new_text_file"),
        NameKind::Rename { .. } => t("rename"),
    };
    let close = modal(ctx, "name", 420.0, |ui| {
        ui.heading(title);
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(text).desired_width(if keep_ext.is_some() {
                    300.0
                } else {
                    f32::INFINITY
                }),
            );
            if *focus {
                r.request_focus();
                // Select the name without its extension, so typing keeps it.
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), r.id) {
                    let n = if keep_ext.is_none() && !original_ext.is_empty() {
                        vfs::stem(text).chars().count()
                    } else {
                        text.chars().count()
                    };
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(n),
                        )));
                    state.store(ui.ctx(), r.id);
                    *focus = false;
                }
            }
            if let Some(ext) = keep_ext {
                ui.label(RichText::new(format!(".{ext}")).strong());
            }
        });
        let valid = valid_name(text);
        let final_name = match keep_ext {
            Some(ext) => format!("{}.{ext}", text.trim()),
            None => text.trim().to_owned(),
        };
        if let Err(k) = valid {
            ui.colored_label(ui.visuals().error_fg_color, t(k));
        } else if matches!(kind, NameKind::Rename { .. })
            && keep_ext.is_none()
            && !original_ext.is_empty()
            && vfs::extension(&final_name) != original_ext
        {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                tf("ext_change_warning", original_ext),
            );
        }
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let (ok, cancel) = buttons(
            ui,
            if matches!(kind, NameKind::Rename { .. }) {
                t("rename")
            } else {
                t("create")
            },
            false,
        );
        if (ok || enter) && valid.is_ok() {
            match kind {
                NameKind::NewFolder { dest, file } => app
                    .actions
                    .push(Action::MkdirNow(dest.join(&final_name), *file)),
                NameKind::Rename { from } => {
                    if from.name() != final_name
                        && let Some(parent) = from.parent()
                    {
                        app.actions.push(Action::RenameNow {
                            from: from.clone(),
                            to: parent.join(&final_name),
                        });
                    }
                }
            }
            keep = false;
        }
        if cancel {
            keep = false;
        }
    });
    keep && !close
}

fn server_dialog(
    app: &mut App,
    ctx: &egui::Context,
    server: &mut Server,
    show_password: &mut bool,
    is_new: bool,
) -> bool {
    let mut keep = true;
    let close = modal(ctx, "server", 480.0, |ui| {
        ui.heading(if is_new {
            t("connect_server")
        } else {
            t("edit_server")
        });
        ui.add_space(4.0);
        egui::Grid::new("server-grid")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label(t("protocol"));
                let before = server.kind;
                egui::ComboBox::from_id_salt("kind")
                    .selected_text(server.kind.label())
                    .show_ui(ui, |ui| {
                        for k in ServerKind::ALL {
                            ui.selectable_value(&mut server.kind, k, k.label());
                        }
                    });
                if before != server.kind && server.port == before.default_port() {
                    server.port = server.kind.default_port();
                }
                ui.end_row();
                ui.label(t("server_name"));
                ui.add(egui::TextEdit::singleline(&mut server.name).hint_text(t("optional")));
                ui.end_row();
                ui.label(t("host"));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut server.host)
                            .hint_text("nas.local / 192.168.1.10")
                            .desired_width(220.0),
                    );
                    ui.label(":");
                    ui.add(egui::DragValue::new(&mut server.port).range(1..=65535));
                });
                ui.end_row();
                match server.kind {
                    ServerKind::Smb => {
                        ui.label(t("share"));
                        ui.add(egui::TextEdit::singleline(&mut server.share).hint_text("Public"));
                        ui.end_row();
                    }
                    ServerKind::WebDav | ServerKind::WebDavs => {
                        ui.label(t("base_path"));
                        ui.add(
                            egui::TextEdit::singleline(&mut server.share)
                                .hint_text("/remote.php/dav/files/user"),
                        );
                        ui.end_row();
                    }
                    _ => {}
                }
                ui.label(t("user"));
                ui.add(egui::TextEdit::singleline(&mut server.user).hint_text(
                    if matches!(
                        server.kind,
                        ServerKind::Ftp | ServerKind::Ftps | ServerKind::Smb
                    ) {
                        t("anonymous_hint")
                    } else {
                        ""
                    },
                ));
                ui.end_row();
                ui.label(t("password"));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut server.password)
                            .password(!*show_password)
                            .desired_width(220.0),
                    );
                    ui.checkbox(show_password, t("show"));
                });
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut server.remember_password, t("remember_password"));
                ui.end_row();
                ui.label(t("start_folder"));
                ui.add(egui::TextEdit::singleline(&mut server.start_path).hint_text("/"));
                ui.end_row();
                if matches!(server.kind, ServerKind::Ftps | ServerKind::WebDavs) {
                    ui.label("");
                    ui.checkbox(&mut server.accept_invalid_certs, t("accept_invalid_certs"));
                    ui.end_row();
                }
            });
        if server.remember_password && !server.password.is_empty() {
            ui.small(RichText::new(t("password_stored_note")).color(theme::palette(app.dark).weak));
        }
        let (ok, cancel) = buttons(ui, t("save_connect"), false);
        if ok && !server.host.trim().is_empty() {
            server.host = server.host.trim().to_owned();
            if !server.remember_password && !server.password.is_empty() {
                vfs::servers::set_session_password(server.id, server.password.clone());
            }
            match app.settings.servers.iter_mut().find(|s| s.id == server.id) {
                Some(s) => *s = server.clone(),
                None => app.settings.servers.push(server.clone()),
            }
            vfs::servers::set_servers(&app.settings.servers);
            app.actions.push(Action::Connect(server.id));
            keep = false;
        }
        if cancel {
            keep = false;
        }
    });
    keep && !close
}

fn settings_dialog(app: &mut App, ctx: &egui::Context) -> bool {
    let mut keep = true;
    let here = app.tab(app.active_pane).location().clone();
    let close = modal(ctx, "settings", 540.0, |ui| {
        ui.heading(t("settings"));
        egui::ScrollArea::vertical()
            .max_height(ctx.content_rect().height() * 0.7)
            .show(ui, |ui| {
                let s = &mut app.settings;
                ui.add_space(4.0);
                ui.label(RichText::new(t("on_start")).strong());
                ui.radio_value(&mut s.start, StartMode::Home, t("start_home"));
                ui.radio_value(&mut s.start, StartMode::Downloads, t("start_downloads"));
                ui.radio_value(&mut s.start, StartMode::LastSession, t("start_last"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.start, StartMode::Folder, t("start_folder_opt"));
                    ui.add_enabled(
                        s.start == StartMode::Folder,
                        egui::TextEdit::singleline(&mut s.start_folder).desired_width(220.0),
                    );
                    if let Location::Local(p) = &here
                        && ui.small_button(t("use_current")).clicked()
                    {
                        s.start = StartMode::Folder;
                        s.start_folder = p.display().to_string();
                    }
                });
                ui.separator();
                ui.label(RichText::new(t("view")).strong());
                ui.horizontal(|ui| {
                    ui.label(t("default_view"));
                    egui::ComboBox::from_id_salt("default-view")
                        .selected_text(t(s.view.key()))
                        .show_ui(ui, |ui| {
                            for v in ViewMode::ALL {
                                ui.selectable_value(&mut s.view, v, t(v.key()));
                            }
                        });
                });
                let mut dirty = false;
                dirty |= ui.checkbox(&mut s.show_hidden, t("show_hidden")).changed();
                dirty |= ui
                    .checkbox(&mut s.show_extensions, t("show_extensions"))
                    .changed();
                dirty |= ui
                    .checkbox(&mut s.folders_first, t("folders_first"))
                    .changed();
                ui.checkbox(&mut s.thumbnails, t("show_thumbnails"));
                ui.checkbox(&mut s.remote_thumbnails, t("remote_thumbnails"));
                dirty |= ui.checkbox(&mut s.folder_sizes, t("show_folder_sizes")).changed();
                ui.horizontal(|ui| {
                    dirty |= ui.checkbox(&mut s.remember_folder_views, t("remember_views")).changed();
                    if !s.folder_views.is_empty() && ui.small_button(tf("forget_views", s.folder_views.len())).clicked() {
                        s.folder_views.clear();
                        dirty = true;
                    }
                });
                ui.checkbox(&mut s.search_subfolders, t("search_while_typing"));
                ui.separator();
                ui.label(RichText::new(t("appearance")).strong());
                ui.horizontal(|ui| {
                    ui.label(t("theme"));
                    ui.radio_value(&mut s.theme, ThemeChoice::System, t("theme_system"));
                    ui.radio_value(&mut s.theme, ThemeChoice::Light, t("theme_light"));
                    ui.radio_value(&mut s.theme, ThemeChoice::Dark, t("theme_dark"));
                });
                ui.horizontal(|ui| {
                    ui.label(t("language"));
                    let before = s.language;
                    ui.radio_value(&mut s.language, Language::Auto, t("lang_auto"));
                    ui.radio_value(&mut s.language, Language::Spanish, "Español");
                    ui.radio_value(&mut s.language, Language::English, "English");
                    if before != s.language {
                        crate::i18n::set_language(s.language);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(t("zoom"));
                    ui.add(
                        egui::Slider::new(&mut s.zoom, 0.6..=2.5)
                            .step_by(0.05)
                            .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
                    );
                });
                ui.separator();
                ui.label(RichText::new(t("network")).strong());
                let servers = s.servers.clone();
                if servers.is_empty() {
                    ui.label(RichText::new(t("no_servers")).color(theme::palette(app.dark).weak));
                }
                for srv in &servers {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} — {}", srv.display_name(), srv.url_base()));
                        if ui.small_button(t("edit")).clicked() {
                            app.actions.push(Action::EditServer(Some(srv.id)));
                        }
                        if ui.small_button(t("remove_server")).clicked() {
                            app.actions.push(Action::RemoveServer(srv.id));
                        }
                    });
                }
                if ui.button(t("connect_server")).clicked() {
                    app.actions.push(Action::EditServer(None));
                }
                if dirty {
                    app.mark_dirty();
                }
            });
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t("close")).clicked() {
                keep = false;
            }
        });
    });
    keep && !close
}

/// A running job asks what to do with a name that is taken.
fn conflicts(app: &mut App, ctx: &egui::Context) {
    let Some(job) = app
        .jobs
        .iter()
        .find(|j| j.state.status().question.is_some())
    else {
        return;
    };
    let Some(q) = job.state.status().question else {
        return;
    };
    let state = job.state.clone();
    let id = egui::Id::unique(("conflict-all", job.id));
    let mut all = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    egui::Modal::new(egui::Id::unique(("conflict", job.id))).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.heading(t("name_taken"));
        ui.label(tf(
            if q.is_dir {
                "conflict_folder"
            } else {
                "conflict_file"
            },
            &q.name,
        ));
        ui.checkbox(&mut all, t("apply_to_all"));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let mut answer = None;
            if ui
                .button(t(if q.is_dir { "merge" } else { "replace" }))
                .clicked()
            {
                answer = Some(Conflict::Replace);
            }
            if ui.button(t("keep_both")).clicked() {
                answer = Some(Conflict::KeepBoth);
            }
            if ui.button(t("skip")).clicked() {
                answer = Some(Conflict::Skip);
            }
            if ui.button(t("cancel")).clicked() {
                state.cancel();
            }
            if let Some(c) = answer {
                state.answer(Answer {
                    conflict: c,
                    apply_to_all: all,
                });
            }
        });
    });
    ctx.data_mut(|d| d.insert_temp(id, all));
}
