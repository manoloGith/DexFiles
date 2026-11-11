//! The app: its state, the messages worker threads send back, the actions the interface asks
//! for (handled after each frame is drawn), and the keyboard.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use egui::{Key, Modifiers};

use super::dialogs::Dialog;
use super::pane::{Pane, Tab};
use super::thumbs::Thumbs;
use crate::archive::ArchiveFormat;
use crate::i18n::{t, tf};
use crate::ops::{self, Job, JobKind, Mode};
use crate::platform::Volume;
use crate::settings::{Favorite, Settings, ThemeChoice, ViewMode};
use crate::vfs::{self, Entry, Location, ServerId};

/// What the worker threads send back.
pub enum Msg {
    Listing { tab: u64, generation: u64, result: Result<Vec<Entry>, String>, stamp: Option<std::time::SystemTime> },
    Tree { location: Location, result: Result<Vec<(String, Location)>, String> },
    Preview { key: String, content: crate::preview::Content },
    Space { location: Location, space: Option<(u64, u64)> },
    Status(String),
}

/// What the interface asks for.
#[derive(Clone, Debug)]
pub enum Action {
    Activate(usize),
    Navigate(usize, Location),
    Open(usize, Entry),
    OpenInNewTab(Location),
    OpenInOtherPane(Location),
    Back(usize),
    Forward(usize),
    Up(usize),
    Refresh(usize),
    NewTab(usize),
    CloseTab(usize, usize),
    SelectTab(usize, usize),
    Copy(Vec<Location>),
    Cut(Vec<Location>),
    Paste(Location),
    Transfer { sources: Vec<Location>, dest: Location, mode: Mode },
    Trash(Vec<Location>),
    AskDelete(Vec<Location>),
    NewFolder(Location),
    NewFile(Location),
    Rename(Entry),
    AskCompress(Vec<Location>),
    Extract { archive: Location, into_folder: bool },
    Share(Vec<Location>, bool),
    OpenWith(Location),
    CopyPath(Vec<Location>),
    Pin(Location),
    Unpin(usize),
    MoveFavorite(usize, bool),
    Restore(Vec<Entry>),
    Purge(Vec<Entry>),
    AskEmptyTrash,
    Properties(Entry),
    Connect(ServerId),
    EditServer(Option<ServerId>),
    RemoveServer(ServerId),
    Disconnect(ServerId),
    Search(usize),
    ClearSearch(usize),
    SetView(ViewMode),
    QuickLook(Entry),
    Settings,
    About,
    CompressNow { sources: Vec<Location>, dest: Location, format: ArchiveFormat },
    DeleteNow(Vec<Location>),
    EmptyTrashNow,
    RenameNow { from: Location, to: Location },
    MkdirNow(Location, bool),
}

/// Files copied or cut, waiting to be pasted.
#[derive(Clone, Debug)]
pub struct FileClip {
    pub items: Vec<Location>,
    pub cut: bool,
}

/// Where a drag may be dropped this frame.
pub struct DropTarget {
    pub rect: egui::Rect,
    pub dest: Location,
    /// Folder items beat the pane behind them.
    pub priority: u8,
    pub trash: bool,
}

/// What is dragged: the items, and the pane they come from.
#[derive(Clone, Debug)]
pub struct DragPayload {
    pub items: Vec<Location>,
    pub pane: usize,
}

pub enum TreeNode {
    Loading,
    Loaded(Vec<(String, Location)>),
    Failed,
}

pub struct PreviewState {
    pub key: String,
    pub content: Option<crate::preview::Content>,
    pub texture: Option<egui::TextureHandle>,
    pub requested_at: Instant,
    pub loading: bool,
}

pub struct App {
    pub settings: Settings,
    pub panes: [Pane; 2],
    pub active_pane: usize,
    pub jobs: Vec<Job>,
    pub clip: Option<FileClip>,
    pub dialog: Option<Dialog>,
    pub tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub thumbs: Thumbs,
    pub preview: PreviewState,
    pub tree_expanded: HashSet<Location>,
    pub tree: HashMap<Location, TreeNode>,
    pub volumes: Vec<Volume>,
    volumes_checked: Instant,
    pub space: HashMap<Location, (u64, u64)>,
    pub message: Option<(String, Instant)>,
    pub address_edit: Option<String>,
    pub focus_address: bool,
    pub focus_search: bool,
    type_ahead: (String, Instant),
    pub storage_ok: bool,
    storage_checked: Instant,
    pub dark: bool,
    pub actions: Vec<Action>,
    pub drop_targets: Vec<DropTarget>,
    pub ctx: egui::Context,
    pub show_jobs: bool,
    pub narrow: bool,
    /// Grid columns of each pane's view last frame (arrow keys move by rows).
    pub columns: [usize; 2],
    /// Rows a page holds (Page Up/Down).
    pub page_rows: [usize; 2],
    pub trash_count: usize,
    trash_checked: Instant,
    applied_theme: Option<bool>,
    applied_zoom: f32,
    pub quick_look: Option<Entry>,
    closing: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Vec<PathBuf>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let mut settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, Settings::KEY)).unwrap_or_default();
        crate::i18n::set_language(settings.language);
        settings.init_favorites();
        vfs::servers::set_servers(&settings.servers);
        super::theme::install_fonts(&ctx);
        egui_extras_free_image_loaders(&ctx);
        let (tx, rx) = channel();
        let start = settings.start_location();
        let mut panes = [Pane::new(start.clone()), Pane::new(start)];
        let mut active_pane = 0;
        if settings.start == crate::settings::StartMode::LastSession && !settings.session.panes.is_empty() {
            for (i, tabs) in settings.session.panes.iter().enumerate().take(2) {
                let tabs: Vec<Tab> = tabs.iter().filter(|l| !matches!(l, Location::Local(p) if !p.exists())).cloned().map(Tab::new).collect();
                if !tabs.is_empty() {
                    panes[i].tabs = tabs;
                    panes[i].active = settings.session.active_tabs.get(i).copied().unwrap_or(0).min(panes[i].tabs.len() - 1);
                }
            }
            active_pane = settings.session.active_pane.min(1);
        }
        // Files opened with the app (a folder opens; a file opens its folder, selected).
        for p in open {
            let loc = if p.is_dir() { Location::Local(p) } else { Location::Local(p.parent().map(PathBuf::from).unwrap_or_default()) };
            panes[0].open_tab(loc);
        }
        let mut app = App {
            thumbs: Thumbs::new(&ctx),
            settings,
            panes,
            active_pane,
            jobs: Vec::new(),
            clip: None,
            dialog: None,
            tx,
            rx,
            preview: PreviewState { key: String::new(), content: None, texture: None, requested_at: Instant::now(), loading: false },
            tree_expanded: HashSet::new(),
            tree: HashMap::new(),
            volumes: crate::platform::volumes(),
            volumes_checked: Instant::now(),
            space: HashMap::new(),
            message: None,
            address_edit: None,
            focus_address: false,
            focus_search: false,
            type_ahead: (String::new(), Instant::now()),
            storage_ok: crate::platform::storage_access(),
            storage_checked: Instant::now(),
            dark: false,
            actions: Vec::new(),
            drop_targets: Vec::new(),
            ctx: ctx.clone(),
            show_jobs: false,
            narrow: false,
            columns: [1, 1],
            page_rows: [10, 10],
            trash_count: crate::trash::count(),
            trash_checked: Instant::now(),
            applied_theme: None,
            applied_zoom: 0.0,
            quick_look: None,
            closing: false,
        };
        for p in 0..2 {
            for i in 0..app.panes[p].tabs.len() {
                app.load(p, i);
            }
        }
        app
    }

    pub fn tab(&self, pane: usize) -> &Tab {
        self.panes[pane].tab()
    }

    pub fn tab_mut(&mut self, pane: usize) -> &mut Tab {
        self.panes[pane].tab_mut()
    }

    pub fn view_mode(&self, pane: usize) -> ViewMode {
        self.tab(pane).view.unwrap_or(self.settings.view)
    }

    pub fn other_pane(&self) -> usize {
        1 - self.active_pane
    }

    pub fn say(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), Instant::now()));
    }

    /// List a tab's folder on a worker.
    pub fn load(&mut self, pane: usize, tab_index: usize) {
        let Some(tab) = self.panes[pane].tabs.get_mut(tab_index) else { return };
        let location = tab.location().clone();
        if let Location::Remote { server, .. } = &location
            && vfs::servers::needs_password(*server)
        {
            tab.error = Some(t("needs_password").to_owned());
            tab.loading = false;
            let id = *server;
            self.dialog = Some(Dialog::password(id));
            return;
        }
        tab.generation += 1;
        tab.loading = true;
        let (id, generation) = (tab.id, tab.generation);
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let _ = std::thread::Builder::new().name("dexfiles-list".into()).spawn(move || {
            let stamp = location.as_local().and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
            let result = vfs::list(&location).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Listing { tab: id, generation, result, stamp });
            ctx.request_repaint();
        });
        self.request_space(&self.tab(pane).location().clone());
    }

    pub fn reload_active(&mut self, pane: usize) {
        let i = self.panes[pane].active;
        self.load(pane, i);
    }

    fn request_space(&mut self, location: &Location) {
        let key = match location {
            Location::Local(p) => {
                // The volume it is on.
                let vol = self.volumes.iter().filter(|v| p.starts_with(&v.path)).max_by_key(|v| v.path.as_os_str().len()).map(|v| v.path.clone());
                Location::Local(vol.unwrap_or_else(|| p.clone()))
            }
            Location::Remote { server, .. } => Location::Remote { server: *server, path: "/".into() },
            Location::Trash => return,
        };
        let tx = self.tx.clone();
        let target = location.clone();
        let _ = std::thread::Builder::new().name("dexfiles-space".into()).spawn(move || {
            let space = vfs::fs_for(&target).ok().and_then(|fs| fs.space(&target.backend_path()));
            let _ = tx.send(Msg::Space { location: key, space });
        });
    }

    pub fn load_tree(&mut self, location: &Location) {
        if self.tree.contains_key(location) {
            return;
        }
        self.tree.insert(location.clone(), TreeNode::Loading);
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let loc = location.clone();
        let show_hidden = self.settings.show_hidden;
        let _ = std::thread::Builder::new().name("dexfiles-tree".into()).spawn(move || {
            let result = vfs::list(&loc)
                .map(|entries| {
                    let mut dirs: Vec<(String, Location)> =
                        entries.into_iter().filter(|e| e.is_dir() && (show_hidden || !e.hidden)).map(|e| (e.name, e.location)).collect();
                    dirs.sort_by(|a, b| super::pane::natural_cmp(&a.0, &b.0));
                    dirs
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Tree { location: loc, result });
            ctx.request_repaint();
        });
    }

    fn handle_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Listing { tab, generation, result, stamp } => {
                    for p in 0..2 {
                        for tb in &mut self.panes[p].tabs {
                            if tb.id != tab || tb.generation != generation {
                                continue;
                            }
                            tb.loading = false;
                            tb.dir_stamp = stamp;
                            tb.last_check = Instant::now();
                            match &result {
                                Ok(entries) => {
                                    tb.entries = entries.clone();
                                    tb.error = None;
                                }
                                Err(e) => {
                                    tb.entries.clear();
                                    tb.error = Some(e.clone());
                                }
                            }
                            tb.dirty = true;
                            if let Location::Remote { server, .. } = tb.location()
                                && result.is_ok()
                            {
                                let _ = server;
                            }
                        }
                    }
                }
                Msg::Tree { location, result } => {
                    let node = match result {
                        Ok(children) => TreeNode::Loaded(children),
                        Err(_) => TreeNode::Failed,
                    };
                    self.tree.insert(location, node);
                }
                Msg::Preview { key, content } => {
                    if key == self.preview.key {
                        self.preview.loading = false;
                        self.preview.texture = None;
                        if let crate::preview::Content::Image { image, .. } = &content {
                            self.preview.texture = Some(self.ctx.load_texture("preview", image.clone(), egui::TextureOptions::LINEAR));
                        }
                        self.preview.content = Some(content);
                    }
                }
                Msg::Space { location, space } => {
                    if let Some(s) = space {
                        self.space.insert(location, s);
                    }
                }
                Msg::Status(text) => self.say(text),
            }
        }
    }

    /// Jobs: refresh what they touched, select what they made, forget finished ones.
    fn update_jobs(&mut self) {
        let mut touched: Vec<Location> = Vec::new();
        let mut created: Vec<Location> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        self.jobs.retain(|job| {
            let s = job.state.status();
            if !s.finished {
                return true;
            }
            touched.extend(s.touched.iter().cloned());
            created.extend(s.created.iter().cloned());
            errors.extend(s.errors.iter().cloned());
            false
        });
        if !errors.is_empty() {
            let text = errors.iter().take(8).cloned().collect::<Vec<_>>().join("\n");
            let more = if errors.len() > 8 { format!("\n… (+{})", errors.len() - 8) } else { String::new() };
            self.dialog = Some(Dialog::Message { title: t("errors_title").to_owned(), text: format!("{text}{more}") });
        }
        if touched.is_empty() {
            return;
        }
        if touched.contains(&Location::Trash) {
            self.trash_count = crate::trash::count();
        }
        for p in 0..2 {
            for i in 0..self.panes[p].tabs.len() {
                let here = self.panes[p].tabs[i].location().clone();
                if touched.iter().any(|t| *t == here) {
                    let tab = &mut self.panes[p].tabs[i];
                    let mine: Vec<Location> = created.iter().filter(|c| c.parent().as_ref() == Some(&here)).cloned().collect();
                    if !mine.is_empty() {
                        tab.pending_select = mine;
                    }
                    self.load(p, i);
                }
            }
        }
        // Folder tree: forget the touched folders' children.
        for t in &touched {
            self.tree.remove(t);
        }
        self.tree.retain(|_, n| !matches!(n, TreeNode::Failed));
        let expanded: Vec<Location> = self.tree_expanded.iter().cloned().collect();
        for e in expanded {
            if touched.contains(&e) {
                self.load_tree(&e);
            }
        }
    }

    /// Local folders changed by someone else are listed again (checked every two seconds).
    fn watch_folders(&mut self) {
        for p in 0..2 {
            let i = self.panes[p].active;
            let tab = &self.panes[p].tabs[i];
            if tab.loading || tab.search.is_some() || tab.last_check.elapsed() < Duration::from_secs(2) {
                continue;
            }
            let Some(path) = tab.location().as_local().map(PathBuf::from) else { continue };
            let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            let changed = stamp != tab.dir_stamp;
            self.panes[p].tabs[i].last_check = Instant::now();
            if changed {
                self.load(p, i);
            }
        }
        self.ctx.request_repaint_after(Duration::from_secs(2));
    }

    pub fn start_job(&mut self, kind: JobKind) {
        let job = ops::start(&self.ctx, kind);
        self.jobs.push(job);
    }

    /// The selected items of the active pane (or the item under the cursor).
    pub fn selection(&self, pane: usize) -> Vec<Location> {
        self.tab(pane).selected_locations()
    }

    fn open_entry(&mut self, pane: usize, entry: Entry) {
        if entry.is_dir() {
            if entry.trash.is_some() {
                // A folder in the trash: show its info rather than browse into it.
                self.dialog = Some(Dialog::Properties(Box::new(entry)));
                return;
            }
            self.navigate(pane, entry.location);
            return;
        }
        self.open_file(entry.location, false);
    }

    pub fn open_file(&mut self, location: Location, chooser: bool) {
        match location {
            Location::Local(p) => {
                if let Err(e) = crate::platform::open_file(&p, chooser) {
                    self.say(tf("cant_open", e));
                }
            }
            remote => {
                self.say(tf("downloading", remote.name()));
                let tx = self.tx.clone();
                let _ = std::thread::Builder::new().name("dexfiles-open".into()).spawn(move || match vfs::local_copy(&remote, &mut |_| true) {
                    Ok(p) => {
                        if let Err(e) = crate::platform::open_file(&p, chooser) {
                            let _ = tx.send(Msg::Status(tf("cant_open", e)));
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Status(tf("cant_open", e)));
                    }
                });
            }
        }
    }

    pub fn navigate(&mut self, pane: usize, location: Location) {
        // A path to a file opens its folder with the file selected.
        if let Location::Local(p) = &location
            && p.is_file()
        {
            let parent = p.parent().map(PathBuf::from).unwrap_or_default();
            self.navigate(pane, Location::Local(parent));
            self.tab_mut(pane).pending_select = vec![location];
            return;
        }
        self.active_pane = pane;
        if self.tab_mut(pane).navigate(location.clone()) {
            self.settings.add_recent(&location);
            self.reload_active(pane);
        }
        self.address_edit = None;
    }

    fn share(&mut self, items: Vec<Location>, email: bool) {
        let tx = self.tx.clone();
        let _ = std::thread::Builder::new().name("dexfiles-share".into()).spawn(move || {
            let mut paths = Vec::new();
            for l in &items {
                match vfs::local_copy(l, &mut |_| true) {
                    Ok(p) if p.is_file() => paths.push(p),
                    Ok(_) => {}
                    Err(e) => {
                        let _ = tx.send(Msg::Status(format!("{}: {e}", l.name())));
                    }
                }
            }
            if paths.is_empty() {
                let _ = tx.send(Msg::Status(t("share_files_only").to_owned()));
                return;
            }
            if let Err(e) = crate::platform::share(&paths, email) {
                let _ = tx.send(Msg::Status(e));
            }
        });
    }

    fn handle_action(&mut self, action: Action) {
        match action {
            Action::Activate(p) => self.active_pane = p,
            Action::Navigate(p, l) => self.navigate(p, l),
            Action::Open(p, e) => self.open_entry(p, e),
            Action::OpenInNewTab(l) => {
                let p = self.active_pane;
                self.panes[p].open_tab(l.clone());
                self.settings.add_recent(&l);
                self.reload_active(p);
            }
            Action::OpenInOtherPane(l) => {
                self.settings.dual_pane = true;
                let o = self.other_pane();
                self.navigate(o, l);
            }
            Action::Back(p) => {
                if self.tab_mut(p).back() {
                    self.reload_active(p);
                }
            }
            Action::Forward(p) => {
                if self.tab_mut(p).forward() {
                    self.reload_active(p);
                }
            }
            Action::Up(p) => {
                let here = self.tab(p).location().clone();
                if let Some(parent) = here.parent() {
                    self.navigate(p, parent);
                    self.tab_mut(p).pending_select = vec![here];
                }
            }
            Action::Refresh(p) => {
                self.thumbs.clear();
                self.tree.clear();
                self.volumes = crate::platform::volumes();
                self.reload_active(p);
            }
            Action::NewTab(p) => {
                let l = self.tab(p).location().clone();
                self.panes[p].open_tab(l);
                self.reload_active(p);
            }
            Action::CloseTab(p, i) => self.panes[p].close_tab(i),
            Action::SelectTab(p, i) => {
                self.active_pane = p;
                self.panes[p].active = i.min(self.panes[p].tabs.len() - 1);
            }
            Action::Copy(items) if !items.is_empty() => {
                self.say(tf("copied_n", items.len()));
                self.clip_paths(&items);
                self.clip = Some(FileClip { items, cut: false });
            }
            Action::Cut(items) if !items.is_empty() => {
                self.say(tf("cut_n", items.len()));
                self.clip_paths(&items);
                self.clip = Some(FileClip { items, cut: true });
            }
            Action::Copy(_) | Action::Cut(_) => {}
            Action::Paste(dest) => {
                if let Some(clip) = self.clip.clone() {
                    let mode = if clip.cut { Mode::Move } else { Mode::Copy };
                    if clip.cut {
                        self.clip = None;
                    }
                    self.start_job(JobKind::Transfer { sources: clip.items, dest, mode });
                }
            }
            Action::Transfer { sources, dest, mode } => {
                if !sources.is_empty() {
                    self.start_job(JobKind::Transfer { sources, dest, mode });
                }
            }
            Action::Trash(items) => {
                let (local, remote): (Vec<Location>, Vec<Location>) = items.into_iter().partition(Location::is_local);
                if !local.is_empty() {
                    let paths: Vec<PathBuf> = local.iter().filter_map(|l| l.as_local().map(PathBuf::from)).collect();
                    self.say(tf("trashed_n", paths.len()));
                    self.start_job(JobKind::Trash { items: paths });
                }
                if !remote.is_empty() {
                    // Servers have no trash: deleting there is for good, so ask.
                    self.dialog = Some(Dialog::ConfirmDelete { items: remote, remote: true });
                }
            }
            Action::AskDelete(items) if !items.is_empty() => {
                if items.iter().all(|l| matches!(l, Location::Local(p) if self.is_in_trash(p))) {
                    let entries: Vec<Entry> = self.tab(self.active_pane).selected_entries();
                    self.handle_action(Action::Purge(entries));
                } else {
                    self.dialog = Some(Dialog::ConfirmDelete { items, remote: false });
                }
            }
            Action::AskDelete(_) => {}
            Action::DeleteNow(items) => self.start_job(JobKind::Delete { items }),
            Action::NewFolder(dest) => self.dialog = Some(Dialog::new_folder(dest, false)),
            Action::NewFile(dest) => self.dialog = Some(Dialog::new_folder(dest, true)),
            Action::MkdirNow(location, file) => {
                let p = self.active_pane;
                self.tab_mut(p).pending_select = vec![location.clone()];
                if file {
                    self.start_job(JobKind::NewFile { location });
                } else {
                    self.start_job(JobKind::Mkdir { location });
                }
            }
            Action::Rename(e) => self.dialog = Some(Dialog::rename(&e, self.settings.show_extensions)),
            Action::RenameNow { from, to } => self.start_job(JobKind::Rename { from, to }),
            Action::AskCompress(items) if !items.is_empty() => {
                if items.iter().any(|l| !l.is_local()) {
                    self.say(t("compress_local_only"));
                } else {
                    self.dialog = Some(Dialog::compress(items));
                }
            }
            Action::AskCompress(_) => {}
            Action::CompressNow { sources, dest, format } => self.start_job(JobKind::Compress { sources, dest, format }),
            Action::Extract { archive, into_folder } => {
                let Some(parent) = archive.parent() else { return };
                let parent = if parent.is_local() { parent } else { Location::Local(crate::platform::home().join("Download")) };
                let dest = if into_folder {
                    let base = crate::archive::folder_name(&archive.name());
                    let name = ops::unique_name(&base, |n| parent.as_local().is_some_and(|p| p.join(n).exists()));
                    parent.join(&name)
                } else {
                    parent
                };
                self.start_job(JobKind::Extract { archive, dest });
            }
            Action::Share(items, email) => self.share(items, email),
            Action::OpenWith(l) => self.open_file(l, true),
            Action::CopyPath(items) => {
                let text = items.iter().map(|l| l.address(&self.settings.servers)).collect::<Vec<_>>().join("\n");
                crate::platform::copy_text(&self.ctx, text);
                self.say(t("path_copied"));
            }
            Action::Pin(l) => {
                if !self.settings.favorites.iter().any(|f| f.location == l) {
                    let name = super::pane::location_title(&l);
                    self.settings.favorites.push(Favorite { name, location: l });
                }
            }
            Action::Unpin(i) => {
                if i < self.settings.favorites.len() {
                    self.settings.favorites.remove(i);
                }
            }
            Action::MoveFavorite(i, up) => {
                let n = self.settings.favorites.len();
                if up && i > 0 && i < n {
                    self.settings.favorites.swap(i, i - 1);
                } else if !up && i + 1 < n {
                    self.settings.favorites.swap(i, i + 1);
                }
            }
            Action::Restore(entries) => {
                let items: Vec<_> = entries.into_iter().filter_map(|e| e.trash).collect();
                if !items.is_empty() {
                    self.start_job(JobKind::Restore { items });
                }
            }
            Action::Purge(entries) => {
                let items: Vec<_> = entries.into_iter().filter_map(|e| e.trash).collect();
                if !items.is_empty() {
                    self.dialog = Some(Dialog::ConfirmPurge { items });
                }
            }
            Action::AskEmptyTrash => self.dialog = Some(Dialog::ConfirmEmptyTrash),
            Action::EmptyTrashNow => self.start_job(JobKind::EmptyTrash),
            Action::Properties(e) => self.dialog = Some(Dialog::Properties(Box::new(e))),
            Action::Connect(id) => {
                if let Some(s) = vfs::servers::server(id) {
                    let p = self.active_pane;
                    if vfs::servers::needs_password(id) {
                        self.dialog = Some(Dialog::password(id));
                    } else {
                        self.navigate(p, s.start());
                    }
                }
            }
            Action::EditServer(id) => {
                let server = match id.and_then(vfs::servers::server) {
                    Some(s) => s,
                    None => {
                        let id = self.settings.new_server_id();
                        vfs::Server::new(id, vfs::ServerKind::Sftp)
                    }
                };
                self.dialog = Some(Dialog::server(server));
            }
            Action::RemoveServer(id) => {
                self.settings.servers.retain(|s| s.id != id);
                vfs::servers::set_servers(&self.settings.servers);
            }
            Action::Disconnect(id) => {
                vfs::servers::disconnect(id);
                self.say(t("disconnected"));
            }
            Action::Search(p) => {
                let tab = self.tab_mut(p);
                let q = tab.query.clone();
                if q.is_filtering() || q.content {
                    let root = tab.location().clone();
                    let query = crate::search::Query { recursive: true, include_hidden: self.settings.show_hidden, ..q };
                    let search = crate::search::start(&self.ctx, root, query);
                    let tab = self.tab_mut(p);
                    tab.search = Some(search);
                    let tab = self.tab_mut(p);
                    tab.search_results.clear();
                    tab.filter.clear();
                    tab.selected.clear();
                    tab.dirty = true;
                }
            }
            Action::ClearSearch(p) => {
                let tab = self.tab_mut(p);
                tab.search = None;
                tab.search_results.clear();
                tab.filter.clear();
                tab.query.text.clear();
                tab.dirty = true;
            }
            Action::SetView(v) => {
                let p = self.active_pane;
                self.settings.view = v;
                self.tab_mut(p).view = None;
                self.tab_mut(p).scroll_to_cursor = true;
            }
            Action::QuickLook(e) => {
                self.quick_look = Some(e);
            }
            Action::Settings => self.dialog = Some(Dialog::Settings),
            Action::About => self.dialog = Some(Dialog::About),
        }
    }

    /// Copied files also go to the system clipboard as their paths (to paste in other apps;
    /// and on a desktop egui only reports Ctrl+V when the clipboard holds text).
    fn clip_paths(&self, items: &[Location]) {
        let text = items.iter().map(|l| l.address(&self.settings.servers)).collect::<Vec<_>>().join("\n");
        crate::platform::copy_text(&self.ctx, text);
    }

    pub fn is_in_trash(&self, p: &std::path::Path) -> bool {
        p.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            s == "Trash" || s.starts_with(".Trash-")
        }) && p.to_string_lossy().contains("/files/")
    }

    /// Theme, zoom and language follow the settings.
    fn apply_settings(&mut self, ctx: &egui::Context) {
        let dark = match self.settings.theme {
            ThemeChoice::Dark => true,
            ThemeChoice::Light => false,
            ThemeChoice::System => ctx.input(|i| i.raw.system_theme).map(|t| t == egui::Theme::Dark).unwrap_or(false),
        };
        if self.applied_theme != Some(dark) {
            super::theme::apply(ctx, dark);
            self.applied_theme = Some(dark);
            self.dark = dark;
        }
        let zoom = self.settings.zoom.clamp(0.6, 2.5);
        if (zoom - self.applied_zoom).abs() > f32::EPSILON {
            ctx.set_zoom_factor(zoom);
            self.applied_zoom = zoom;
        }
    }

    fn periodic(&mut self) {
        if self.volumes_checked.elapsed() > Duration::from_secs(5) {
            self.volumes_checked = Instant::now();
            let v = crate::platform::volumes();
            if v != self.volumes {
                self.volumes = v;
            }
        }
        if self.trash_checked.elapsed() > Duration::from_secs(10) {
            self.trash_checked = Instant::now();
            self.trash_count = crate::trash::count();
        }
        if !self.storage_ok && self.storage_checked.elapsed() > Duration::from_secs(1) {
            self.storage_checked = Instant::now();
            if crate::platform::storage_access() {
                self.storage_ok = true;
                self.volumes = crate::platform::volumes();
                for p in 0..2 {
                    self.reload_active(p);
                }
            }
            self.ctx.request_repaint_after(Duration::from_secs(1));
        }
        // Search results stream in.
        for p in 0..2 {
            for tab in &mut self.panes[p].tabs {
                if let Some(s) = &tab.search {
                    let n = s.results.lock().map(|r| r.len()).unwrap_or(0);
                    if n != tab.search_results.len() {
                        tab.search_results = s.snapshot();
                        tab.dirty = true;
                    }
                }
            }
        }
        if self.jobs.iter().any(|j| !j.state.status().finished) {
            self.ctx.request_repaint_after(Duration::from_millis(250));
        }
        if let Some((_, at)) = &self.message {
            if at.elapsed() > Duration::from_secs(4) {
                self.message = None;
            } else {
                self.ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
    }

    /// Keyboard shortcuts (when no text field has the keyboard).
    fn keyboard(&mut self, ctx: &egui::Context) {
        let p = self.active_pane;
        let pressed = |ctx: &egui::Context, m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        // Mouse back and forward buttons work everywhere.
        let (extra1, extra2) = ctx.input(|i| (i.pointer.button_pressed(egui::PointerButton::Extra1), i.pointer.button_pressed(egui::PointerButton::Extra2)));
        if extra1 {
            self.actions.push(Action::Back(p));
        }
        if extra2 {
            self.actions.push(Action::Forward(p));
        }
        if self.dialog.is_some() {
            return;
        }
        if self.quick_look.is_some() {
            if pressed(ctx, Modifiers::NONE, Key::Space) || pressed(ctx, Modifiers::NONE, Key::Escape) {
                self.quick_look = None;
            }
            return;
        }
        // Even while typing in a field.
        if pressed(ctx, Modifiers::COMMAND, Key::L) {
            self.focus_address = true;
            self.address_edit = Some(self.tab(p).location().address(&self.settings.servers));
        }
        if pressed(ctx, Modifiers::COMMAND, Key::F) {
            self.focus_search = true;
        }
        if pressed(ctx, Modifiers::COMMAND, Key::T) {
            self.actions.push(Action::NewTab(p));
        }
        if pressed(ctx, Modifiers::COMMAND, Key::W) {
            let i = self.panes[p].active;
            self.actions.push(Action::CloseTab(p, i));
        }
        if pressed(ctx, Modifiers::COMMAND | Modifiers::SHIFT, Key::Tab) {
            let n = self.panes[p].tabs.len();
            self.panes[p].active = (self.panes[p].active + n - 1) % n;
        } else if pressed(ctx, Modifiers::COMMAND, Key::Tab) {
            let n = self.panes[p].tabs.len();
            self.panes[p].active = (self.panes[p].active + 1) % n;
        }
        if pressed(ctx, Modifiers::COMMAND, Key::Comma) {
            self.actions.push(Action::Settings);
        }
        if pressed(ctx, Modifiers::COMMAND, Key::Plus) || pressed(ctx, Modifiers::COMMAND, Key::Equals) {
            self.settings.zoom = (self.settings.zoom + 0.1).min(2.5);
        }
        if pressed(ctx, Modifiers::COMMAND, Key::Minus) {
            self.settings.zoom = (self.settings.zoom - 0.1).max(0.6);
        }
        if pressed(ctx, Modifiers::COMMAND, Key::Num0) {
            self.settings.zoom = 1.0;
        }
        if pressed(ctx, Modifiers::NONE, Key::F5) || pressed(ctx, Modifiers::COMMAND, Key::R) {
            self.actions.push(Action::Refresh(p));
        }
        if pressed(ctx, Modifiers::ALT, Key::ArrowLeft) {
            self.actions.push(Action::Back(p));
        }
        if pressed(ctx, Modifiers::ALT, Key::ArrowRight) {
            self.actions.push(Action::Forward(p));
        }
        if pressed(ctx, Modifiers::ALT, Key::ArrowUp) {
            self.actions.push(Action::Up(p));
        }
        if pressed(ctx, Modifiers::NONE, Key::F3) {
            self.settings.dual_pane = !self.settings.dual_pane;
        }
        if pressed(ctx, Modifiers::COMMAND, Key::H) {
            self.settings.show_hidden = !self.settings.show_hidden;
            self.mark_dirty();
        }
        for (i, key) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6].into_iter().enumerate() {
            if pressed(ctx, Modifiers::COMMAND, key) {
                self.actions.push(Action::SetView(ViewMode::ALL[i]));
            }
        }
        // A text field has the keyboard, or a menu is open (Escape and arrows are its own).
        if ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) {
            return;
        }
        let here = self.tab(p).location().clone();
        let sel = self.selection(p);
        if pressed(ctx, Modifiers::COMMAND | Modifiers::SHIFT, Key::C) {
            self.actions.push(Action::CopyPath(if sel.is_empty() { vec![here.clone()] } else { sel.clone() }));
        }
        if pressed(ctx, Modifiers::COMMAND | Modifiers::SHIFT, Key::N) {
            self.actions.push(Action::NewFolder(here.clone()));
        }
        if pressed(ctx, Modifiers::COMMAND, Key::A) {
            self.tab_mut(p).select_all();
        }
        // On a desktop egui turns Ctrl+C, X and V into clipboard events; Android's keyboard
        // (craft-android) sends them as keys.
        let (copy_ev, cut_ev, paste_ev) = ctx.input(|i| {
            let mut out = (false, false, false);
            for e in &i.events {
                match e {
                    egui::Event::Copy => out.0 = true,
                    egui::Event::Cut => out.1 = true,
                    egui::Event::Paste(_) => out.2 = true,
                    _ => {}
                }
            }
            out
        });
        if pressed(ctx, Modifiers::COMMAND, Key::C) || copy_ev {
            self.actions.push(Action::Copy(sel.clone()));
        }
        if pressed(ctx, Modifiers::COMMAND, Key::X) || cut_ev {
            self.actions.push(Action::Cut(sel.clone()));
        }
        if (pressed(ctx, Modifiers::COMMAND, Key::V) || paste_ev) && here != Location::Trash {
            self.actions.push(Action::Paste(here.clone()));
        }
        if pressed(ctx, Modifiers::COMMAND, Key::I) {
            let entry = self.tab(p).selected_entries().into_iter().next();
            if let Some(e) = entry {
                self.actions.push(Action::Properties(e));
            }
        }
        if pressed(ctx, Modifiers::SHIFT, Key::Delete) {
            self.actions.push(Action::AskDelete(sel.clone()));
        } else if pressed(ctx, Modifiers::NONE, Key::Delete) && !sel.is_empty() {
            if here == Location::Trash {
                let e = self.tab(p).selected_entries();
                self.actions.push(Action::Purge(e));
            } else {
                self.actions.push(Action::Trash(sel.clone()));
            }
        }
        if pressed(ctx, Modifiers::NONE, Key::F2) {
            let entry = self.tab(p).selected_entries().into_iter().next();
            if let Some(e) = entry {
                self.actions.push(Action::Rename(e));
            }
        }
        if pressed(ctx, Modifiers::NONE, Key::Backspace) {
            self.actions.push(Action::Up(p));
        }
        if self.settings.dual_pane && !self.narrow && pressed(ctx, Modifiers::NONE, Key::Tab) {
            self.active_pane = 1 - p;
        }
        if pressed(ctx, Modifiers::NONE, Key::Escape) {
            if self.tab(p).search.is_some() || !self.tab(p).filter.is_empty() {
                self.actions.push(Action::ClearSearch(p));
            } else {
                self.tab_mut(p).selected.clear();
            }
        }
        if pressed(ctx, Modifiers::NONE, Key::Enter) || pressed(ctx, Modifiers::COMMAND, Key::O) {
            let entries = self.tab(p).selected_entries();
            if entries.len() == 1 {
                self.actions.push(Action::Open(p, entries[0].clone()));
            } else {
                for e in entries.into_iter().filter(|e| !e.is_dir()) {
                    self.actions.push(Action::Open(p, e));
                }
            }
        }
        if pressed(ctx, Modifiers::NONE, Key::Space) {
            let e = self.tab(p).cursor.and_then(|c| self.tab(p).item(c).cloned());
            if let Some(e) = e {
                self.actions.push(Action::QuickLook(e));
            }
        }
        self.cursor_keys(ctx, p);
        self.type_to_select(ctx, p);
    }

    fn cursor_keys(&mut self, ctx: &egui::Context, p: usize) {
        let grid = self.view_mode(p).is_grid();
        let cols = if grid { self.columns[p].max(1) } else { 1 };
        let page = self.page_rows[p].max(1) * cols;
        let n = self.tab(p).visible.len();
        if n == 0 {
            return;
        }
        let mut moves: Vec<(isize, bool, bool)> = Vec::new(); // (delta or absolute, shift, absolute)
        ctx.input_mut(|i| {
            for (key, delta) in [
                (Key::ArrowDown, cols as isize),
                (Key::ArrowUp, -(cols as isize)),
                (Key::PageDown, page as isize),
                (Key::PageUp, -(page as isize)),
            ] {
                for m in [Modifiers::NONE, Modifiers::SHIFT] {
                    if i.consume_key(m, key) {
                        moves.push((delta, m.shift, false));
                    }
                }
            }
            if grid {
                for (key, delta) in [(Key::ArrowRight, 1isize), (Key::ArrowLeft, -1)] {
                    for m in [Modifiers::NONE, Modifiers::SHIFT] {
                        if i.consume_key(m, key) {
                            moves.push((delta, m.shift, false));
                        }
                    }
                }
            }
            for m in [Modifiers::NONE, Modifiers::SHIFT] {
                if i.consume_key(m, Key::Home) {
                    moves.push((0, m.shift, true));
                }
                if i.consume_key(m, Key::End) {
                    moves.push((n as isize - 1, m.shift, true));
                }
            }
        });
        // In a list, Right opens a folder and Left goes up (as column-less managers do).
        if !grid {
            let right = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowRight));
            let left = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowLeft));
            if right {
                let e = self.tab(p).cursor.and_then(|c| self.tab(p).item(c).cloned());
                if let Some(e) = e.filter(Entry::is_dir) {
                    self.actions.push(Action::Open(p, e));
                }
            }
            if left {
                self.actions.push(Action::Up(p));
            }
        }
        for (delta, shift, absolute) in moves {
            let tab = self.tab_mut(p);
            let cur = tab.cursor.map(|c| c as isize).unwrap_or(-1);
            let next = if absolute { delta } else if cur < 0 { 0 } else { cur + delta };
            let next = next.clamp(0, n as isize - 1) as usize;
            if shift {
                if tab.anchor.is_none() {
                    tab.anchor = Some(cur.max(0) as usize);
                }
                tab.select_range(next, false);
            } else {
                tab.select_only(next);
            }
            tab.scroll_to_cursor = true;
        }
    }

    fn type_to_select(&mut self, ctx: &egui::Context, p: usize) {
        let typed: String = ctx.input(|i| {
            if i.modifiers.command || i.modifiers.alt {
                return String::new();
            }
            i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }).collect()
        });
        let typed: String = typed.chars().filter(|c| !c.is_control() && *c != ' ').collect();
        if typed.is_empty() {
            return;
        }
        if self.type_ahead.1.elapsed() > Duration::from_millis(900) {
            self.type_ahead.0.clear();
        }
        self.type_ahead.0.push_str(&typed.to_lowercase());
        self.type_ahead.1 = Instant::now();
        let prefix = self.type_ahead.0.clone();
        let found = self.tab(p).items().position(|e| e.name.to_lowercase().starts_with(&prefix));
        if let Some(i) = found {
            let tab = self.tab_mut(p);
            tab.select_only(i);
            tab.scroll_to_cursor = true;
        }
    }

    pub fn mark_dirty(&mut self) {
        for p in &mut self.panes {
            for t in &mut p.tabs {
                t.dirty = true;
            }
        }
        self.tree.clear();
    }

    /// The preview of the active pane's selection (loaded on a worker, shortly after it settles).
    fn update_preview(&mut self) {
        if !self.settings.show_preview && self.quick_look.is_none() {
            return;
        }
        let entry = match &self.quick_look {
            Some(e) => Some(e.clone()),
            None => {
                let tab = self.tab(self.active_pane);
                let sel = tab.selected_entries();
                if sel.len() == 1 { sel.into_iter().next() } else { None }
            }
        };
        let Some(entry) = entry else {
            self.preview.key.clear();
            self.preview.content = None;
            self.preview.texture = None;
            return;
        };
        let key = format!("{:?}|{:?}|{}", entry.location, entry.modified, entry.size);
        if key == self.preview.key {
            return;
        }
        if self.preview.requested_at.elapsed() < Duration::from_millis(120) && !self.preview.key.is_empty() {
            self.ctx.request_repaint_after(Duration::from_millis(130));
            return;
        }
        self.preview.key = key.clone();
        self.preview.requested_at = Instant::now();
        self.preview.content = None;
        self.preview.texture = None;
        self.preview.loading = true;
        if !entry.location.is_local() && !entry.is_dir() && entry.size > 50 * 1024 * 1024 {
            self.preview.loading = false;
            self.preview.content = Some(crate::preview::Content::None("preview_too_big"));
            return;
        }
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let _ = std::thread::Builder::new().name("dexfiles-preview".into()).spawn(move || {
            let content = crate::preview::load(&entry, 1600);
            let _ = tx.send(Msg::Preview { key, content });
            ctx.request_repaint();
        });
    }

    /// Files dropped onto the window from outside (desktop) or opened with the app (Android):
    /// copied into the folder under the pointer, or the active one.
    fn external_drops(&mut self, ctx: &egui::Context) {
        #[allow(unused_mut)]
        let mut paths: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        #[cfg(target_os = "android")]
        paths.extend(craft_android::android::events::opened().into_iter().map(PathBuf::from));
        if paths.is_empty() {
            return;
        }
        let pos = ctx.input(|i| i.pointer.hover_pos());
        let dest = pos
            .and_then(|pos| self.drop_targets.iter().filter(|t| t.rect.contains(pos) && !t.trash).max_by_key(|t| t.priority).map(|t| t.dest.clone()))
            .unwrap_or_else(|| self.tab(self.active_pane).location().clone());
        if dest == Location::Trash {
            return;
        }
        let sources: Vec<Location> = paths.into_iter().map(Location::Local).collect();
        self.start_job(JobKind::Transfer { sources, dest, mode: Mode::Copy });
    }

    /// A drag of items released over a target.
    fn finish_drag(&mut self, ctx: &egui::Context) {
        let Some(payload) = egui::DragAndDrop::payload::<DragPayload>(ctx) else { return };
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }
        egui::DragAndDrop::clear_payload(ctx);
        let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) else { return };
        let Some(target) = self.drop_targets.iter().filter(|t| t.rect.contains(pos)).max_by_key(|t| t.priority) else { return };
        let dest = target.dest.clone();
        if target.trash {
            self.actions.push(Action::Trash(payload.items.clone()));
            return;
        }
        let items: Vec<Location> = payload.items.iter().filter(|l| **l != dest && l.parent().as_ref() != Some(&dest)).cloned().collect();
        if items.is_empty() || dest == Location::Trash {
            return;
        }
        // Ctrl copies, Shift moves; otherwise a move on the same disk or server, a copy elsewhere.
        let m = ctx.input(|i| i.modifiers);
        let mode = if m.command {
            Mode::Copy
        } else if m.shift {
            Mode::Move
        } else {
            let same = items.iter().all(|l| match (l, &dest) {
                (Location::Local(a), Location::Local(b)) => a.parent().and_then(crate::vfs::local::device) == crate::vfs::local::device(b),
                _ => l.same_backend(&dest),
            });
            if same { Mode::Move } else { Mode::Copy }
        };
        self.actions.push(Action::Transfer { sources: items, dest, mode });
    }

    fn save_session(&mut self) {
        self.settings.session = crate::settings::Session {
            panes: self.panes.iter().map(|p| p.tabs.iter().map(|t| t.location().clone()).collect()).collect(),
            active_tabs: self.panes.iter().map(|p| p.active).collect(),
            active_pane: self.active_pane,
        };
    }

    pub fn refresh_visible_all(&mut self) {
        let (sort, desc, ff, hidden) = (self.settings.sort, self.settings.sort_descending, self.settings.folders_first, self.settings.show_hidden);
        for p in &mut self.panes {
            for t in &mut p.tabs {
                if t.dirty {
                    t.refresh_visible(sort, desc, ff, hidden);
                }
            }
        }
    }

    fn window_title(&self) -> String {
        format!("{} — DexFiles", self.tab(self.active_pane).title())
    }

    /// Back, or the window's close button: with jobs running, ask first.
    fn closing(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            let running = self.jobs.iter().any(|j| !j.state.status().finished);
            if running && !self.closing {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.dialog = Some(Dialog::ConfirmClose);
            }
        }
    }

    pub fn close_now(&mut self) {
        self.closing = true;
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// egui_extras isn't a dependency: images are decoded by the app itself.
fn egui_extras_free_image_loaders(_ctx: &egui::Context) {}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_settings(ctx);
        self.handle_messages();
        self.update_jobs();
        self.periodic();
        self.watch_folders();
        self.thumbs.update(ctx);
        self.closing(ctx);
        self.keyboard(ctx);
        self.refresh_visible_all();
        self.update_preview();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.narrow = ctx.content_rect().width() < 720.0;
        self.drop_targets.clear();
        super::view::window(self, ui);
        self.external_drops(&ctx);
        self.finish_drag(&ctx);
        let actions = std::mem::take(&mut self.actions);
        for a in actions {
            self.handle_action(a);
        }
        self.refresh_visible_all();
        let title = self.window_title();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_session();
        eframe::set_value(storage, Settings::KEY, &self.settings.for_storage());
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(20)
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}

impl App {
    /// The window's title (DeX's taskbar), for craft-android's Shell.
    pub fn title(&self) -> String {
        self.window_title()
    }
}
