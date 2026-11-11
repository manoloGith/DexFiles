//! The trash, after the freedesktop.org Trash specification: `files/` holds what was deleted,
//! `info/<name>.trashinfo` where it was and when. On a Linux desktop the home trash is the
//! system's own (`~/.local/share/Trash`), so both see the same items. Other volumes (and Android's
//! storage) keep a `.Trash-dexfiles` folder at their top, so deleting never copies across disks.

use std::io;
use std::path::{Path, PathBuf};

use crate::vfs::{Entry, Kind, Location};

#[derive(Clone, Debug, PartialEq)]
pub struct TrashInfo {
    pub original: PathBuf,
    /// Seconds since the epoch.
    pub deleted: Option<i64>,
    pub file: PathBuf,
    pub info: PathBuf,
}

/// The trash folder for things deleted from `path`.
pub fn trash_dir_for(path: &Path) -> PathBuf {
    let dev = crate::vfs::local::device(path);
    #[cfg(not(target_os = "android"))]
    {
        let home_trash = home_trash();
        let home_dev = crate::vfs::local::device(&crate::platform::home());
        if dev.is_some() && dev == home_dev {
            return home_trash;
        }
    }
    // The volume the path is on: the longest mount point above it on the same device.
    let mut best: Option<PathBuf> = None;
    for v in crate::platform::volumes() {
        if path.starts_with(&v.path)
            && crate::vfs::local::device(&v.path) == dev
            && best
                .as_ref()
                .is_none_or(|b| v.path.as_os_str().len() > b.as_os_str().len())
        {
            best = Some(v.path.clone());
        }
    }
    match best {
        Some(top) if top != Path::new("/") => top.join(volume_trash_name()),
        _ => fallback_trash(),
    }
}

#[cfg(not(target_os = "android"))]
fn home_trash() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::platform::home().join(".local/share"))
        .join("Trash")
}

fn fallback_trash() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        crate::platform::home().join(volume_trash_name())
    }
    #[cfg(not(target_os = "android"))]
    {
        home_trash()
    }
}

fn volume_trash_name() -> String {
    #[cfg(all(unix, not(target_os = "android")))]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(m) = std::fs::metadata(crate::platform::home()) {
            return format!(".Trash-{}", m.uid());
        }
    }
    ".Trash-dexfiles".to_owned()
}

/// Every trash folder that may hold something.
fn all_trash_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![fallback_trash()];
    for v in crate::platform::volumes() {
        let d = v.path.join(volume_trash_name());
        if !dirs.contains(&d) && v.path != Path::new("/") {
            dirs.push(d);
        }
    }
    dirs.retain(|d| d.join("files").is_dir());
    dirs
}

fn encode(p: &Path) -> String {
    crate::vfs::servers::percent_encode_path(&p.to_string_lossy())
}

/// A free name in `files/` for `name`.
fn free_name(trash: &Path, name: &str) -> String {
    let mut candidate = name.to_owned();
    let mut n = 1;
    while trash
        .join("files")
        .join(&candidate)
        .symlink_metadata()
        .is_ok()
        || trash
            .join("info")
            .join(format!("{candidate}.trashinfo"))
            .exists()
    {
        n += 1;
        let stem = crate::vfs::stem(name);
        let ext = crate::vfs::extension(name);
        candidate = if ext.is_empty() {
            format!("{stem}.{n}")
        } else {
            format!("{stem}.{n}.{ext}")
        };
    }
    candidate
}

/// Move a local file or folder to the trash.
pub fn trash(path: &Path) -> io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| crate::vfs::other("nothing to delete"))?;
    let trash = trash_dir_for(path);
    if path.starts_with(&trash) {
        return Err(crate::vfs::other("already in the trash"));
    }
    std::fs::create_dir_all(trash.join("files"))?;
    std::fs::create_dir_all(trash.join("info"))?;
    let stored = free_name(&trash, &name);
    let info = trash.join("info").join(format!("{stored}.trashinfo"));
    let date = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S");
    std::fs::write(
        &info,
        format!("[Trash Info]\nPath={}\nDeletionDate={date}\n", encode(path)),
    )?;
    let target = trash.join("files").join(&stored);
    if let Err(e) = std::fs::rename(path, &target) {
        // Another device after all (a bind mount): copy, then delete.
        if let Err(e2) = copy_tree(path, &target).and_then(|()| remove_tree(path)) {
            let _ = std::fs::remove_file(&info);
            return Err(io::Error::new(e2.kind(), format!("{e}; {e2}")));
        }
    }
    Ok(())
}

fn parse_info(text: &str) -> (Option<PathBuf>, Option<i64>) {
    let mut path = None;
    let mut date = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("Path=") {
            path = Some(PathBuf::from(crate::vfs::servers::percent_decode(p.trim())));
        } else if let Some(d) = line.strip_prefix("DeletionDate=") {
            date = chrono::NaiveDateTime::parse_from_str(d.trim(), "%Y-%m-%dT%H:%M:%S")
                .ok()
                .and_then(|n| n.and_local_timezone(chrono::Local).single())
                .map(|t| t.timestamp());
        }
    }
    (path, date)
}

/// What the trash holds, as entries named after the original files.
pub fn list() -> Vec<Entry> {
    let mut out = Vec::new();
    for dir in all_trash_dirs() {
        let Ok(items) = std::fs::read_dir(dir.join("info")) else {
            continue;
        };
        for item in items.flatten() {
            let info = item.path();
            let Some(stored) = info
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".trashinfo"))
                .map(str::to_owned)
            else {
                continue;
            };
            let file = dir.join("files").join(&stored);
            let Ok(raw) = crate::vfs::local::raw_entry(&file, stored.clone()) else {
                continue;
            };
            let (original, deleted) =
                parse_info(&std::fs::read_to_string(&info).unwrap_or_default());
            // On a volume's trash, a relative path is relative to the volume's top.
            let original = match original {
                Some(p) if p.is_relative() => dir.parent().map(|top| top.join(&p)).unwrap_or(p),
                Some(p) => p,
                None => PathBuf::from(&stored),
            };
            let name = original
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(stored);
            let mut e = Entry::from_raw(&Location::Local(dir.join("files")), raw);
            e.name = name.clone();
            e.ext = if e.kind == Kind::Dir {
                String::new()
            } else {
                crate::vfs::extension(&name)
            };
            e.hidden = false;
            e.location = Location::Local(file.clone());
            e.trash = Some(TrashInfo {
                original,
                deleted,
                file,
                info,
            });
            out.push(e);
        }
    }
    out
}

/// Put an item back where it was (with another name if that place is taken). Its new path.
pub fn restore(item: &TrashInfo) -> io::Result<PathBuf> {
    let mut target = item.original.clone();
    if target.symlink_metadata().is_ok() {
        let parent = target.parent().map(Path::to_path_buf).unwrap_or_default();
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        target = parent.join(crate::ops::unique_name(&name, |n| {
            parent.join(n).symlink_metadata().is_ok()
        }));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(&item.file, &target).is_err() {
        copy_tree(&item.file, &target)?;
        remove_tree(&item.file)?;
    }
    let _ = std::fs::remove_file(&item.info);
    Ok(target)
}

/// Delete an item from the trash for good.
pub fn purge(item: &TrashInfo) -> io::Result<()> {
    remove_tree(&item.file)?;
    let _ = std::fs::remove_file(&item.info);
    Ok(())
}

pub fn empty() -> io::Result<usize> {
    let items = list();
    let n = items.len();
    for e in items {
        if let Some(t) = e.trash {
            purge(&t)?;
        }
    }
    Ok(n)
}

pub fn count() -> usize {
    all_trash_dirs()
        .iter()
        .filter_map(|d| std::fs::read_dir(d.join("info")).ok())
        .map(|r| r.count())
        .sum()
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.is_dir() {
        std::fs::create_dir_all(to)?;
        for item in std::fs::read_dir(from)? {
            let item = item?;
            copy_tree(&item.path(), &to.join(item.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

fn remove_tree(path: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_files_round_trip() {
        let (p, d) = parse_info(
            "[Trash Info]\nPath=/home/a/My%20file.txt\nDeletionDate=2026-11-10T12:30:00\n",
        );
        assert_eq!(p, Some(PathBuf::from("/home/a/My file.txt")));
        assert!(d.is_some());
    }

    #[test]
    fn free_names_skip_taken_ones() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        std::fs::create_dir_all(dir.path().join("files")).unwrap_or_default();
        std::fs::create_dir_all(dir.path().join("info")).unwrap_or_default();
        assert_eq!(free_name(dir.path(), "a.txt"), "a.txt");
        std::fs::write(dir.path().join("files/a.txt"), b"x").unwrap_or_default();
        assert_eq!(free_name(dir.path(), "a.txt"), "a.2.txt");
    }
}
