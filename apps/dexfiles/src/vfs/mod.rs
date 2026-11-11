//! Where files live: the local file system and network servers (FTP/FTPS, SFTP, SMB, WebDAV),
//! behind one blocking interface ([`Fs`]). Every call may be slow (a network round trip, a
//! sleeping SD card), so the interface only calls them from worker threads.

pub mod archive_fs;
pub mod local;
#[cfg(feature = "network")]
pub mod net;
pub mod servers;

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub use servers::{Server, ServerId, ServerKind};

/// A place: a folder or a file.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Location {
    /// A local path (absolute).
    Local(PathBuf),
    /// A path on a saved server, `/`-separated and absolute (`/` is the server's root, or the
    /// share's for SMB).
    Remote { server: ServerId, path: String },
    /// The trash (every local volume's).
    Trash,
    /// Inside an archive (ZIP, 7z, TAR…, RAR): `inner` is `/`-separated and absolute (`/` is
    /// the archive's top). Read only.
    Archive {
        archive: Box<Location>,
        inner: String,
    },
}

/// The last part of a `/`-separated path.
fn path_name(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// The parent of a `/`-separated absolute path (`None` for the root).
fn path_parent(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    Some(match trimmed.rfind('/') {
        Some(0) | None => "/".to_owned(),
        Some(i) => trimmed[..i].to_owned(),
    })
}

fn path_join(path: &str, name: &str) -> String {
    format!("{}/{name}", path.trim_end_matches('/'))
}

fn path_starts_with(a: &str, b: &str) -> bool {
    let b = b.trim_end_matches('/');
    a == b || a.starts_with(&format!("{b}/")) || b.is_empty()
}

impl Location {
    pub fn local(path: impl Into<PathBuf>) -> Self {
        Location::Local(path.into())
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Location::Local(_))
    }

    pub fn as_local(&self) -> Option<&Path> {
        match self {
            Location::Local(p) => Some(p),
            _ => None,
        }
    }

    /// The last component (the root's name is empty).
    pub fn name(&self) -> String {
        match self {
            Location::Local(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Location::Remote { path, .. } => path_name(path),
            Location::Trash => String::new(),
            Location::Archive { archive, inner } => {
                let n = path_name(inner);
                if n.is_empty() { archive.name() } else { n }
            }
        }
    }

    /// How it shows in its parent's listing: an archive's top is the archive file itself.
    pub fn as_listed(&self) -> Location {
        match self {
            Location::Archive { archive, inner } if inner.trim_end_matches('/').is_empty() => {
                (**archive).clone()
            }
            other => other.clone(),
        }
    }

    /// Inside an archive.
    pub fn in_archive(&self) -> bool {
        matches!(self, Location::Archive { .. })
    }

    pub fn parent(&self) -> Option<Location> {
        match self {
            Location::Local(p) => p.parent().map(|p| Location::Local(p.to_path_buf())),
            Location::Remote { server, path } => path_parent(path).map(|path| Location::Remote {
                server: *server,
                path,
            }),
            Location::Trash => None,
            // The archive's top goes up to the folder the archive is in.
            Location::Archive { archive, inner } => match path_parent(inner) {
                Some(inner) => Some(Location::Archive {
                    archive: archive.clone(),
                    inner,
                }),
                None => archive.parent(),
            },
        }
    }

    pub fn join(&self, name: &str) -> Location {
        match self {
            Location::Local(p) => Location::Local(p.join(name)),
            Location::Remote { server, path } => Location::Remote {
                server: *server,
                path: path_join(path, name),
            },
            Location::Trash => Location::Trash,
            Location::Archive { archive, inner } => Location::Archive {
                archive: archive.clone(),
                inner: path_join(inner, name),
            },
        }
    }

    /// Whether `self` is `other` or inside it.
    pub fn starts_with(&self, other: &Location) -> bool {
        match (self, other) {
            (Location::Local(a), Location::Local(b)) => a.starts_with(b),
            (
                Location::Remote {
                    server: s1,
                    path: a,
                },
                Location::Remote {
                    server: s2,
                    path: b,
                },
            ) => s1 == s2 && path_starts_with(a, b),
            (Location::Trash, Location::Trash) => true,
            (
                Location::Archive {
                    archive: x,
                    inner: a,
                },
                Location::Archive {
                    archive: y,
                    inner: b,
                },
            ) => x == y && path_starts_with(a, b),
            (Location::Archive { archive, .. }, other) => archive.starts_with(other),
            _ => false,
        }
    }

    /// Same backend (same server, or both local): moving may be a rename.
    pub fn same_backend(&self, other: &Location) -> bool {
        match (self, other) {
            (Location::Local(_), Location::Local(_)) => true,
            (Location::Remote { server: a, .. }, Location::Remote { server: b, .. }) => a == b,
            _ => false,
        }
    }

    /// The path the backend understands.
    pub fn backend_path(&self) -> String {
        match self {
            Location::Local(p) => p.to_string_lossy().into_owned(),
            Location::Remote { path, .. } => {
                if path.is_empty() {
                    "/".to_owned()
                } else {
                    path.clone()
                }
            }
            Location::Trash => String::new(),
            Location::Archive { inner, .. } => {
                if inner.is_empty() {
                    "/".to_owned()
                } else {
                    inner.clone()
                }
            }
        }
    }

    /// The address bar's text: a path, or a URL for a server.
    pub fn address(&self, servers: &[Server]) -> String {
        match self {
            Location::Local(p) => p.to_string_lossy().into_owned(),
            Location::Remote { server, path } => match servers.iter().find(|s| s.id == *server) {
                Some(s) => format!(
                    "{}{}",
                    s.url_base(),
                    if path.is_empty() { "/" } else { path }
                ),
                None => format!("?{path}"),
            },
            Location::Trash => "trash:///".to_owned(),
            Location::Archive { archive, inner } => format!(
                "{}{}",
                archive.address(servers),
                if inner == "/" { "" } else { inner }
            ),
        }
    }
}

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Dir,
    File,
    /// Something else (a device, a socket, a broken link).
    Other,
}

/// One entry of a folder, as a backend reports it.
#[derive(Clone, Debug)]
pub struct RawEntry {
    pub name: String,
    pub kind: Kind,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: Option<i64>,
    pub created: Option<i64>,
    pub symlink: bool,
    pub readonly: bool,
    /// Hidden by an attribute (SMB), besides a leading dot.
    pub hidden_attr: bool,
}

/// One entry of a listing, ready for the interface.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub location: Location,
    pub kind: Kind,
    pub size: u64,
    pub modified: Option<i64>,
    pub created: Option<i64>,
    pub symlink: bool,
    pub readonly: bool,
    pub hidden: bool,
    /// Lowercase extension without the dot ("" for folders and files without one).
    pub ext: String,
    /// In the trash: where it was, and when it was deleted.
    pub trash: Option<crate::trash::TrashInfo>,
}

impl Entry {
    pub fn from_raw(parent: &Location, raw: RawEntry) -> Self {
        let ext = if raw.kind == Kind::Dir {
            String::new()
        } else {
            extension(&raw.name)
        };
        let hidden = raw.name.starts_with('.') || raw.hidden_attr;
        Entry {
            location: parent.join(&raw.name),
            name: raw.name,
            kind: raw.kind,
            size: raw.size,
            modified: raw.modified,
            created: raw.created,
            symlink: raw.symlink,
            readonly: raw.readonly,
            hidden,
            ext,
            trash: None,
        }
    }

    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }
}

/// Lowercase extension of a file name ("" if none; a leading dot alone is not one).
pub fn extension(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => {
            let ext = name[i + 1..].to_ascii_lowercase();
            // .tar.gz and friends count as one extension
            let lower = name.to_ascii_lowercase();
            for double in ["tar.gz", "tar.bz2", "tar.xz", "tar.zst"] {
                if lower.ends_with(&format!(".{double}")) && lower.len() > double.len() + 1 {
                    return double.to_owned();
                }
            }
            ext
        }
        _ => String::new(),
    }
}

/// Name without its extension (as [`extension`] splits it).
pub fn stem(name: &str) -> &str {
    let ext = extension(name);
    if ext.is_empty() {
        name
    } else {
        &name[..name.len() - ext.len() - 1]
    }
}

/// Bytes moved so far; return `false` to cancel.
pub type Progress<'a> = &'a mut dyn FnMut(u64) -> bool;

/// A file system. All calls block; paths are the backend's own ([`Location::backend_path`]).
pub trait Fs: Send + Sync {
    fn list(&self, path: &str) -> io::Result<Vec<RawEntry>>;
    fn stat(&self, path: &str) -> io::Result<RawEntry>;
    /// Copy the file at `path` into `out`.
    fn read_into(&self, path: &str, out: &mut dyn Write, progress: Progress<'_>)
    -> io::Result<u64>;
    /// Up to `limit` bytes from the start of the file.
    fn read_head(&self, path: &str, limit: usize) -> io::Result<Vec<u8>> {
        let mut out = LimitedWriter {
            buf: Vec::new(),
            limit,
        };
        match self.read_into(path, &mut out, &mut |_| true) {
            Ok(_) => Ok(out.buf),
            Err(e) if e.kind() == io::ErrorKind::WriteZero => Ok(out.buf),
            Err(e) => Err(e),
        }
    }
    /// Create or replace the file at `path` with what `input` holds.
    fn write_from(
        &self,
        path: &str,
        input: &mut dyn Read,
        size: u64,
        progress: Progress<'_>,
    ) -> io::Result<u64>;
    fn mkdir(&self, path: &str) -> io::Result<()>;
    fn remove_file(&self, path: &str) -> io::Result<()>;
    /// Remove an empty folder.
    fn remove_dir(&self, path: &str) -> io::Result<()>;
    fn rename(&self, from: &str, to: &str) -> io::Result<()>;
    /// (available, total) bytes, if the backend knows.
    fn space(&self, _path: &str) -> Option<(u64, u64)> {
        None
    }
}

/// A writer that stops (WriteZero) after `limit` bytes.
struct LimitedWriter {
    buf: Vec<u8>,
    limit: usize,
}

impl Write for LimitedWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let room = self.limit.saturating_sub(self.buf.len());
        if room == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "limit reached"));
        }
        let n = room.min(data.len());
        self.buf.extend_from_slice(&data[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Copy with progress reports and cancellation (between chunks).
pub fn copy_stream(
    input: &mut dyn Read,
    out: &mut dyn Write,
    progress: Progress<'_>,
) -> io::Result<u64> {
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        out.write_all(&buf[..n])?;
        total += n as u64;
        if !progress(total) {
            return Err(cancelled());
        }
    }
    out.flush()?;
    Ok(total)
}

pub fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

pub fn is_cancelled(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::Interrupted && e.to_string() == "cancelled"
}

pub fn other(msg: impl std::fmt::Display) -> io::Error {
    io::Error::other(msg.to_string())
}

/// The file system behind a location (connecting to its server if needed: may block).
pub fn fs_for(location: &Location) -> io::Result<Arc<dyn Fs>> {
    match location {
        Location::Local(_) => Ok(local::shared()),
        Location::Remote { server, .. } => servers::connection(*server),
        Location::Trash => Ok(local::shared()),
        Location::Archive { archive, .. } => archive_fs::open(archive),
    }
}

/// List a folder as entries.
pub fn list(location: &Location) -> io::Result<Vec<Entry>> {
    if *location == Location::Trash {
        return Ok(crate::trash::list());
    }
    let fs = fs_for(location)?;
    let raw = fs.list(&location.backend_path())?;
    Ok(raw
        .into_iter()
        .map(|r| Entry::from_raw(location, r))
        .collect())
}

pub fn stat(location: &Location) -> io::Result<Entry> {
    let fs = fs_for(location)?;
    let raw = fs.stat(&location.backend_path())?;
    let parent = location.parent().unwrap_or_else(|| location.clone());
    let mut e = Entry::from_raw(&parent, raw);
    e.location = location.clone();
    Ok(e)
}

/// A local copy of a file (itself when local; downloaded to the cache otherwise), for opening it
/// with another app, sharing or previewing.
pub fn local_copy(location: &Location, progress: Progress<'_>) -> io::Result<PathBuf> {
    if let Location::Local(p) = location {
        return Ok(p.clone());
    }
    let fs = fs_for(location)?;
    let dir = crate::platform::cache_dir()
        .join("remote")
        .join(format!("{:016x}", hash(&location.address(&[]))));
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(location.name());
    let partial = dir.join(format!(".{}.part", location.name()));
    let mut file = std::fs::File::create(&partial)?;
    fs.read_into(&location.backend_path(), &mut file, progress)?;
    drop(file);
    std::fs::rename(&partial, &target)?;
    Ok(target)
}

pub fn hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_paths_join_and_climb() {
        let root = Location::Remote {
            server: ServerId(1),
            path: "/".into(),
        };
        let a = root.join("a");
        assert_eq!(
            a,
            Location::Remote {
                server: ServerId(1),
                path: "/a".into()
            }
        );
        let b = a.join("b c");
        assert_eq!(b.name(), "b c");
        assert_eq!(b.parent(), Some(a.clone()));
        assert_eq!(
            a.parent(),
            Some(Location::Remote {
                server: ServerId(1),
                path: "/".into()
            })
        );
        assert!(b.starts_with(&a));
        assert!(b.starts_with(&root));
        assert!(!a.starts_with(&b));
    }

    #[test]
    fn extensions() {
        assert_eq!(extension("photo.JPG"), "jpg");
        assert_eq!(extension(".bashrc"), "");
        assert_eq!(extension("archive.tar.gz"), "tar.gz");
        assert_eq!(stem("archive.tar.gz"), "archive");
        assert_eq!(stem("noext"), "noext");
        assert_eq!(extension("dot."), "");
    }
}
