//! Settings, kept by eframe's storage (`app.ron`).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::vfs::{Location, Server, ServerId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    LargeIcons,
    MediumIcons,
    SmallIcons,
    List,
    Details,
    Content,
}

impl ViewMode {
    pub const ALL: [ViewMode; 6] = [
        ViewMode::LargeIcons,
        ViewMode::MediumIcons,
        ViewMode::SmallIcons,
        ViewMode::List,
        ViewMode::Details,
        ViewMode::Content,
    ];

    pub fn key(self) -> &'static str {
        match self {
            ViewMode::LargeIcons => "view_large",
            ViewMode::MediumIcons => "view_medium",
            ViewMode::SmallIcons => "view_small",
            ViewMode::List => "view_list",
            ViewMode::Details => "view_details",
            ViewMode::Content => "view_content",
        }
    }

    pub fn is_grid(self) -> bool {
        matches!(
            self,
            ViewMode::LargeIcons | ViewMode::MediumIcons | ViewMode::SmallIcons
        )
    }

    /// The icon size of a grid view, in points.
    pub fn icon_size(self) -> f32 {
        match self {
            ViewMode::LargeIcons => 96.0,
            ViewMode::MediumIcons => 64.0,
            ViewMode::SmallIcons => 36.0,
            ViewMode::Content => 40.0,
            _ => 18.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortKey {
    Name,
    Modified,
    Kind,
    Size,
    /// Attributes: read-only, hidden, link.
    Status,
}

impl SortKey {
    pub const ALL: [SortKey; 5] = [
        SortKey::Name,
        SortKey::Modified,
        SortKey::Kind,
        SortKey::Size,
        SortKey::Status,
    ];

    pub fn key(self) -> &'static str {
        match self {
            SortKey::Name => "col_name",
            SortKey::Modified => "col_modified",
            SortKey::Kind => "col_kind",
            SortKey::Size => "col_size",
            SortKey::Status => "col_status",
        }
    }
}

/// How a folder is shown: remembered per folder (see [`Settings::folder_views`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderView {
    pub view: ViewMode,
    pub sort: SortKey,
    pub descending: bool,
}

/// Widths of the details view's columns (the name takes the rest), in points; 0 hides one.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Columns {
    pub modified: f32,
    pub kind: f32,
    pub size: f32,
    pub folder: f32,
    /// Row height of the list and details views.
    pub row: f32,
}

impl Default for Columns {
    fn default() -> Self {
        Columns {
            modified: 150.0,
            kind: 130.0,
            size: 90.0,
            folder: 200.0,
            row: 26.0,
        }
    }
}

/// The app DexFiles opens a kind of file with (its own choice, not Android's).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultApp {
    /// Lowercase extension without the dot.
    pub ext: String,
    /// Android: `package/activity`; desktop: the `.desktop` file's id.
    pub app: String,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartMode {
    Home,
    LastSession,
    Folder,
    Downloads,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    Auto,
    Spanish,
    English,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Favorite {
    pub name: String,
    pub location: Location,
}

/// The open tabs of both panes, to start where the last session ended.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub panes: Vec<Vec<Location>>,
    pub active_tabs: Vec<usize>,
    pub active_pane: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub start: StartMode,
    pub start_folder: String,
    pub show_hidden: bool,
    pub show_extensions: bool,
    pub folders_first: bool,
    pub view: ViewMode,
    pub sort: SortKey,
    pub sort_descending: bool,
    pub confirm_trash: bool,
    pub theme: ThemeChoice,
    pub language: Language,
    /// Interface zoom (DeX screens and phones differ a lot).
    pub zoom: f32,
    pub favorites: Vec<Favorite>,
    pub favorites_initialized: bool,
    pub servers: Vec<Server>,
    pub next_server_id: u64,
    pub show_preview: bool,
    pub dual_pane: bool,
    pub show_sidebar: bool,
    pub thumbnails: bool,
    /// Thumbnails of files on servers (downloads them).
    pub remote_thumbnails: bool,
    pub session: Session,
    /// Recent folders (most recent first), for the address bar's suggestions.
    pub recent: Vec<Location>,
    /// Remember the view and the order of each folder.
    pub remember_folder_views: bool,
    /// Folder key ([`folder_key`]) → how it was last shown.
    pub folder_views: HashMap<String, FolderView>,
    pub columns: Columns,
    /// Show (and compute in the background) the size of local folders.
    pub folder_sizes: bool,
    /// Typing in the search field searches below the folder too (otherwise Enter does).
    pub search_subfolders: bool,
    /// DexFiles' own default apps, by extension.
    pub default_apps: Vec<DefaultApp>,
    /// Extensions Dex apps declared, already offered as default apps (`ext|app`): one the user
    /// removed or changed is not set again.
    pub dex_offered: Vec<String>,
    /// Colour tags (see [`crate::tags`]).
    pub tags: HashMap<Location, Vec<u8>>,
}

/// The key a folder's remembered view is stored under.
pub fn folder_key(l: &Location) -> String {
    match l {
        Location::Local(p) => p.to_string_lossy().into_owned(),
        Location::Remote { server, path } => format!("server:{}:{path}", server.0),
        Location::Trash => "trash:".to_owned(),
        Location::Tag(c) => format!("tag:{c}"),
        Location::Archive { archive, inner } => format!("archive:{}:{inner}", folder_key(archive)),
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            start: StartMode::Home,
            start_folder: String::new(),
            show_hidden: false,
            show_extensions: true,
            folders_first: true,
            view: ViewMode::Details,
            sort: SortKey::Name,
            sort_descending: false,
            confirm_trash: false,
            theme: ThemeChoice::System,
            language: Language::Auto,
            zoom: 1.0,
            favorites: Vec::new(),
            favorites_initialized: false,
            servers: Vec::new(),
            next_server_id: 1,
            show_preview: true,
            dual_pane: false,
            show_sidebar: true,
            thumbnails: true,
            remote_thumbnails: false,
            session: Session::default(),
            recent: Vec::new(),
            remember_folder_views: true,
            folder_views: HashMap::new(),
            columns: Columns::default(),
            folder_sizes: true,
            search_subfolders: true,
            default_apps: Vec::new(),
            tags: HashMap::new(),
            dex_offered: Vec::new(),
        }
    }
}

impl Settings {
    pub const KEY: &'static str = "dexfiles-settings";

    /// First start: the standard folders as favourites.
    pub fn init_favorites(&mut self) {
        if self.favorites_initialized {
            return;
        }
        self.favorites_initialized = true;
        for (key, path) in crate::platform::standard_folders() {
            if key == "home" {
                continue;
            }
            self.favorites.push(Favorite {
                name: crate::i18n::t(key).to_owned(),
                location: Location::Local(path),
            });
        }
    }

    /// The folder to open first.
    pub fn start_location(&self) -> Location {
        match self.start {
            StartMode::Folder if !self.start_folder.trim().is_empty() => {
                Location::Local(PathBuf::from(self.start_folder.trim()))
            }
            StartMode::Downloads => crate::platform::standard_folders()
                .into_iter()
                .find(|(k, _)| *k == "downloads")
                .map(|(_, p)| Location::Local(p))
                .unwrap_or_else(|| Location::Local(crate::platform::home())),
            _ => Location::Local(crate::platform::home()),
        }
    }

    pub fn new_server_id(&mut self) -> ServerId {
        let id = ServerId(self.next_server_id.max(1));
        self.next_server_id = id.0 + 1;
        id
    }

    /// How `l` is shown: its remembered view, or the defaults.
    pub fn folder_view(&self, l: &Location) -> FolderView {
        let default = FolderView {
            view: self.view,
            sort: self.sort,
            descending: self.sort_descending,
        };
        if !self.remember_folder_views {
            return default;
        }
        self.folder_views
            .get(&folder_key(l))
            .copied()
            .unwrap_or(default)
    }

    /// Change how `l` is shown (remembered for it, or the defaults when not remembering).
    pub fn set_folder_view(&mut self, l: &Location, f: FolderView) {
        if self.remember_folder_views {
            if self.folder_views.len() > 5000 {
                self.folder_views.clear();
            }
            self.folder_views.insert(folder_key(l), f);
        } else {
            self.view = f.view;
            self.sort = f.sort;
            self.sort_descending = f.descending;
        }
    }

    pub fn default_app(&self, ext: &str) -> Option<&DefaultApp> {
        self.default_apps.iter().find(|d| d.ext == ext)
    }

    pub fn add_recent(&mut self, loc: &Location) {
        self.recent.retain(|l| l != loc);
        self.recent.insert(0, loc.clone());
        self.recent.truncate(30);
    }

    /// What is written to disk: passwords only for servers that remember them.
    pub fn for_storage(&self) -> Settings {
        let mut s = self.clone();
        for server in &mut s.servers {
            if !server.remember_password {
                server.password.clear();
            }
        }
        s
    }
}
