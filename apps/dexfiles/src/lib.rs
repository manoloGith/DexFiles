//! DexFiles: a file manager for Samsung DeX and Android's desktop windowing (and any desktop),
//! in Rust with egui, on craft-android's Android layer like the other Dex apps.

pub mod archive;
pub mod fileinfo;
pub mod i18n;
pub mod ops;
pub mod platform;
pub mod preview;
pub mod search;
pub mod settings;
pub mod trash;
pub mod vfs;

#[cfg(target_os = "android")]
pub(crate) mod android;
