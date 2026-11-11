//! What differs between Android and a desktop: storage volumes and standard folders, opening a
//! file with another app, sharing, the storage permission.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A storage volume (internal storage, an SD card, a USB drive, a desktop mount).
#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    pub path: PathBuf,
    pub label: String,
    pub removable: bool,
}

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Set at start: where the app keeps its own files (Android: private storage).
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

pub fn data_dir() -> PathBuf {
    DATA_DIR.get().cloned().unwrap_or_else(|| {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(std::env::temp_dir)
            .join("dexfiles")
    })
}

pub fn cache_dir() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        data_dir().join("cache")
    }
    #[cfg(not(target_os = "android"))]
    {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir)
            .join("dexfiles")
    }
}

/// The user's home: Android's shared storage, or `$HOME`.
pub fn home() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        PathBuf::from("/storage/emulated/0")
    }
    #[cfg(not(target_os = "android"))]
    {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
    }
}

/// Standard folders, as (key for the label, path), those that exist.
pub fn standard_folders() -> Vec<(&'static str, PathBuf)> {
    let home = home();
    #[cfg(target_os = "android")]
    let list = [
        ("downloads", "Download"),
        ("documents", "Documents"),
        ("pictures", "Pictures"),
        ("camera", "DCIM"),
        ("music", "Music"),
        ("movies", "Movies"),
    ];
    #[cfg(not(target_os = "android"))]
    let list = [
        ("downloads", "Downloads"),
        ("documents", "Documents"),
        ("pictures", "Pictures"),
        ("desktop", "Desktop"),
        ("music", "Music"),
        ("movies", "Videos"),
    ];
    let mut out = vec![("home", home.clone())];
    for (key, dir) in list {
        let p = home.join(dir);
        if p.is_dir() {
            out.push((key, p));
        }
    }
    out
}

/// Mounted volumes.
pub fn volumes() -> Vec<Volume> {
    #[cfg(target_os = "android")]
    {
        let mut v = crate::android::volumes();
        if v.is_empty() {
            v.push(Volume { path: home(), label: String::new(), removable: false });
        }
        v
    }
    #[cfg(not(target_os = "android"))]
    {
        let mut out = vec![Volume { path: PathBuf::from("/"), label: String::new(), removable: false }];
        if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
            for line in mounts.lines() {
                let mut parts = line.split_whitespace();
                let (Some(_dev), Some(mount)) = (parts.next(), parts.next()) else { continue };
                let mount = mount.replace("\\040", " ");
                if ["/media/", "/run/media/", "/mnt/"].iter().any(|p| mount.starts_with(p)) {
                    let path = PathBuf::from(&mount);
                    let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    out.push(Volume { path, label, removable: true });
                }
            }
        }
        out
    }
}

/// Whether the app may read and write shared storage (Android's "All files access").
pub fn storage_access() -> bool {
    #[cfg(target_os = "android")]
    {
        crate::android::storage_access()
    }
    #[cfg(not(target_os = "android"))]
    {
        true
    }
}

/// Ask for it (opens the system's settings page on Android).
pub fn request_storage_access() {
    #[cfg(target_os = "android")]
    crate::android::request_storage_access();
}

/// Open a local file with the app that handles its type (`chooser`: always ask which).
pub fn open_file(path: &Path, chooser: bool) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android::open_file(path, chooser)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = chooser;
        open_desktop(path)
    }
}

#[cfg(all(not(target_os = "android"), feature = "desktop"))]
fn open_desktop(path: &Path) -> Result<(), String> {
    open::that_detached(path).map_err(|e| e.to_string())
}

#[cfg(all(not(target_os = "android"), not(feature = "desktop")))]
fn open_desktop(path: &Path) -> Result<(), String> {
    std::process::Command::new("xdg-open").arg(path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Share local files with another app (`email`: only mail apps, as attachments).
pub fn share(paths: &[PathBuf], email: bool) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android::share(paths, email)
    }
    #[cfg(not(target_os = "android"))]
    {
        if email {
            let mut cmd = std::process::Command::new("xdg-email");
            for p in paths {
                cmd.arg("--attach").arg(p);
            }
            return cmd.spawn().map(|_| ()).map_err(|e| format!("xdg-email: {e}"));
        }
        // No share sheet on a desktop: show the files in the system's file manager.
        match paths.first().and_then(|p| p.parent()) {
            Some(dir) => open_desktop(dir),
            None => Ok(()),
        }
    }
}

/// Tell the media index about changed files (Android's gallery and music apps).
pub fn media_changed(paths: &[PathBuf]) {
    #[cfg(target_os = "android")]
    crate::android::media_scan(paths);
    #[cfg(not(target_os = "android"))]
    let _ = paths;
}

/// Put text on the system clipboard.
pub fn copy_text(ctx: &egui::Context, text: String) {
    #[cfg(target_os = "android")]
    {
        let _ = ctx;
        let _ = craft_android::android::clipboard::set(craft_android::android::clipboard::Clip::Text(text));
    }
    #[cfg(not(target_os = "android"))]
    ctx.copy_text(text);
}

/// A short message from the system (Android's toast); elsewhere the status bar shows it.
pub fn toast(text: &str) {
    #[cfg(target_os = "android")]
    craft_android::android::toast(text);
    #[cfg(not(target_os = "android"))]
    log::info!("dexfiles: {text}");
}
