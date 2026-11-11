//! Settings, kept by eframe's storage (`app.ron`).

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
