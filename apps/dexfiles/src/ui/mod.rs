//! The interface: a window in the spirit of a modern desktop file manager. Sidebar on the left
//! (favourites, devices, network, trash, folder tree), toolbar and address bar on top, one or two
//! panes with tabs in the middle, preview on the right, status bar below.

pub mod app;
pub mod dialogs;
pub mod icons;
pub mod pane;
pub mod player;
pub mod preview_panel;
pub mod quicklook;
pub mod settings_view;
pub mod sidebar;
pub mod theme;
pub mod thumbs;
pub mod toolbar;
pub mod view;
pub mod viewer;

pub use app::App;
