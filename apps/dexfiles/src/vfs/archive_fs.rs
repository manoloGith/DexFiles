//! An archive (ZIP, 7z, TAR…, RAR with the `rar` feature) seen as a read-only folder tree:
//! browsing it, previewing and opening what is inside, and copying or dragging things out all
//! go through the usual [`Fs`] calls. The archive's list is read once and kept (the last few
//! archives opened, while they don't change).

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use super::{Fs, Kind, Location, Progress, RawEntry};

pub struct ArchiveFs {
    path: PathBuf,
    /// Folder (`/`-separated, `/` is the top) → what it holds.
    dirs: HashMap<String, Vec<RawEntry>>,
    /// A file's path here → its name inside the archive.
    names: HashMap<String, String>,
}

type Cache = Mutex<
    Vec<(
        (PathBuf, Option<std::time::SystemTime>, u64),
        Arc<ArchiveFs>,
    )>,
>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Whether DexFiles can browse an archive with this extension (not single compressed files).
pub fn browsable(ext: &str) -> bool {
    crate::archive::is_archive(ext) && !matches!(ext, "gz" | "bz2" | "xz" | "zst" | "apk")
}

/// The file system of the archive at `archive` (downloaded first if it is on a server).
pub fn open(archive: &Location) -> io::Result<Arc<dyn Fs>> {
    let path = super::local_copy(archive, &mut |_| true)?;
    let meta = std::fs::metadata(&path)?;
    let key = (path.clone(), meta.modified().ok(), meta.len());
    let mut c = cache().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, fs)) = c.iter().find(|(k, _)| *k == key) {
        return Ok(fs.clone());
    }
    let fs = Arc::new(ArchiveFs::read(path)?);
    c.retain(|(k, _)| k.0 != key.0);
    c.push((key, fs.clone()));
    if c.len() > 4 {
        c.remove(0);
    }
    Ok(fs)
}

fn normalize(name: &str) -> String {
    let parts: Vec<&str> = name
        .split(['/', '\\'])
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    format!("/{}", parts.join("/"))
}

fn split(path: &str) -> (String, String) {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => ("/".to_owned(), trimmed[1..].to_owned()),
        Some(i) => (trimmed[..i].to_owned(), trimmed[i + 1..].to_owned()),
        None => ("/".to_owned(), trimmed.to_owned()),
    }
}

impl ArchiveFs {
    fn read(path: PathBuf) -> io::Result<Self> {
        let (items, _) = crate::archive::list(&path, usize::MAX)?;
        let mut dirs: HashMap<String, Vec<RawEntry>> = HashMap::new();
        let mut names = HashMap::new();
        dirs.insert("/".to_owned(), Vec::new());
        let raw = |name: String, kind: Kind, size: u64| RawEntry {
            name,
            kind,
            size,
            modified: None,
            created: None,
            symlink: false,
            readonly: true,
            hidden_attr: false,
        };
        for it in items {
            let full = normalize(&it.name);
            if full == "/" {
                continue;
            }
            // Every folder above it exists, listed or not.
            let mut cur = full.clone();
            let mut is_dir = it.is_dir;
            loop {
                let (parent, name) = split(&cur);
                let siblings = dirs.entry(parent.clone()).or_default();
                match siblings.iter_mut().find(|e| e.name == name) {
                    Some(existing) => {
                        if is_dir {
                            existing.kind = Kind::Dir;
                        }
                    }
                    None => siblings.push(raw(
                        name,
                        if is_dir { Kind::Dir } else { Kind::File },
                        if is_dir { 0 } else { it.size },
                    )),
                }
                if is_dir {
                    dirs.entry(cur.clone()).or_default();
                }
                if parent == "/" {
                    break;
                }
                cur = parent;
                is_dir = true;
            }
            if !it.is_dir {
                names.insert(full, it.name.clone());
            }
        }
        Ok(ArchiveFs { path, dirs, names })
    }

    fn read_only() -> io::Error {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "an archive can't be changed here: extract it first",
        )
    }
}

impl Fs for ArchiveFs {
    fn list(&self, path: &str) -> io::Result<Vec<RawEntry>> {
        let key = normalize(path);
        self.dirs
            .get(&key)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not found"))
    }

    fn stat(&self, path: &str) -> io::Result<RawEntry> {
        let key = normalize(path);
        if key == "/" {
            return Ok(RawEntry {
                name: "/".into(),
                kind: Kind::Dir,
                size: 0,
                modified: None,
                created: None,
                symlink: false,
                readonly: true,
                hidden_attr: false,
            });
        }
        let (parent, name) = split(&key);
        self.dirs
            .get(&parent)
            .and_then(|v| v.iter().find(|e| e.name == name).cloned())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not found"))
    }

    fn read_into(
        &self,
        path: &str,
        out: &mut dyn Write,
        progress: Progress<'_>,
    ) -> io::Result<u64> {
        let name = self
            .names
            .get(&normalize(path))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not found"))?;
        let mut counting = CountingWriter {
            inner: out,
            n: 0,
            progress,
        };
        crate::archive::read_entry(&self.path, name, &mut counting)?;
        Ok(counting.n)
    }

    fn write_from(&self, _: &str, _: &mut dyn Read, _: u64, _: Progress<'_>) -> io::Result<u64> {
        Err(Self::read_only())
    }

    fn mkdir(&self, _: &str) -> io::Result<()> {
        Err(Self::read_only())
    }

    fn remove_file(&self, _: &str) -> io::Result<()> {
        Err(Self::read_only())
    }

    fn remove_dir(&self, _: &str) -> io::Result<()> {
        Err(Self::read_only())
    }

    fn rename(&self, _: &str, _: &str) -> io::Result<()> {
        Err(Self::read_only())
    }
}

struct CountingWriter<'a, 'b> {
    inner: &'a mut dyn Write,
    n: u64,
    progress: Progress<'b>,
}

impl Write for CountingWriter<'_, '_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.n += n as u64;
        if !(self.progress)(self.n) {
            return Err(super::cancelled());
        }
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_browse_like_folders() {
        let root = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let src = root.path().join("docs");
        std::fs::create_dir_all(src.join("inner/deep")).unwrap_or_default();
        std::fs::write(src.join("a.txt"), b"alpha").unwrap_or_default();
        std::fs::write(src.join("inner/deep/b.txt"), b"beta").unwrap_or_default();
        for format in crate::archive::ArchiveFormat::ALL {
            let archive = root.path().join(format!("t.{}", format.extension()));
            crate::archive::compress(
                std::slice::from_ref(&src),
                &archive,
                format,
                None,
                &mut |_, _, _| true,
            )
            .unwrap_or_else(|e| unreachable!("{e}"));
            let at = Location::Archive {
                archive: Box::new(Location::Local(archive.clone())),
                inner: "/".into(),
            };
            let top = super::super::list(&at).unwrap_or_else(|e| unreachable!("{format:?}: {e}"));
            assert_eq!(
                top.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
                vec!["docs"],
                "{format:?}"
            );
            let docs = at.join("docs");
            let mut names: Vec<String> = super::super::list(&docs)
                .unwrap_or_default()
                .into_iter()
                .map(|e| e.name)
                .collect();
            names.sort();
            assert_eq!(names, vec!["a.txt", "inner"], "{format:?}");
            let deep = docs.join("inner").join("deep").join("b.txt");
            let fs = super::super::fs_for(&deep).unwrap_or_else(|e| unreachable!("{e}"));
            let mut out = Vec::new();
            fs.read_into(&deep.backend_path(), &mut out, &mut |_| true)
                .unwrap_or_else(|e| unreachable!("{format:?}: {e}"));
            assert_eq!(out, b"beta", "{format:?}");
            assert!(fs.mkdir("/x").is_err());
            assert_eq!(
                deep.parent()
                    .and_then(|p| p.parent())
                    .and_then(|p| p.parent())
                    .and_then(|p| p.parent()),
                Some(at.clone())
            );
            assert_eq!(
                at.parent(),
                Some(Location::Local(root.path().to_path_buf()))
            );
        }
    }
}
