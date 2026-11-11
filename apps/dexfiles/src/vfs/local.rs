//! The local file system.

use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use super::{Fs, Kind, Progress, RawEntry};

pub struct LocalFs;

pub fn shared() -> Arc<dyn Fs> {
    static FS: OnceLock<Arc<dyn Fs>> = OnceLock::new();
    FS.get_or_init(|| Arc::new(LocalFs)).clone()
}

fn secs(t: io::Result<std::time::SystemTime>) -> Option<i64> {
    let t = t.ok()?;
    Some(match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX),
    })
}

/// An entry for `path` (following a symlink for its kind and size; a broken one is `Other`).
pub fn raw_entry(path: &Path, name: String) -> io::Result<RawEntry> {
    let link = std::fs::symlink_metadata(path)?;
    let symlink = link.file_type().is_symlink();
    let meta = if symlink {
        std::fs::metadata(path).unwrap_or(link)
    } else {
        link
    };
    let kind = if meta.is_dir() {
        Kind::Dir
    } else if meta.is_file() {
        Kind::File
    } else {
        Kind::Other
    };
    Ok(RawEntry {
        name,
        kind,
        size: if kind == Kind::File { meta.len() } else { 0 },
        modified: secs(meta.modified()),
        created: secs(meta.created()),
        symlink,
        readonly: meta.permissions().readonly(),
        hidden_attr: false,
    })
}

/// The device a path is on (moves within one device are renames).
#[cfg(unix)]
pub fn device(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.dev())
}

#[cfg(not(unix))]
pub fn device(_path: &Path) -> Option<u64> {
    Some(0)
}

impl Fs for LocalFs {
    fn list(&self, path: &str) -> io::Result<Vec<RawEntry>> {
        let mut out = Vec::new();
        for item in std::fs::read_dir(path)? {
            let Ok(item) = item else { continue };
            let name = item.file_name().to_string_lossy().into_owned();
            match raw_entry(&item.path(), name) {
                Ok(e) => out.push(e),
                Err(e) => log::debug!("dexfiles: {}: {e}", item.path().display()),
            }
        }
        Ok(out)
    }

    fn stat(&self, path: &str) -> io::Result<RawEntry> {
        let p = Path::new(path);
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_owned());
        raw_entry(p, name)
    }

    fn read_into(
        &self,
        path: &str,
        out: &mut dyn Write,
        progress: Progress<'_>,
    ) -> io::Result<u64> {
        let mut f = std::fs::File::open(path)?;
        super::copy_stream(&mut f, out, progress)
    }

    fn read_head(&self, path: &str, limit: usize) -> io::Result<Vec<u8>> {
        let f = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        f.take(limit as u64).read_to_end(&mut buf)?;
        Ok(buf)
    }

    fn write_from(
        &self,
        path: &str,
        input: &mut dyn Read,
        _size: u64,
        progress: Progress<'_>,
    ) -> io::Result<u64> {
        let mut f = std::fs::File::create(path)?;
        let n = super::copy_stream(input, &mut f, progress)?;
        f.sync_all().ok();
        Ok(n)
    }

    fn mkdir(&self, path: &str) -> io::Result<()> {
        std::fs::create_dir(path)
    }

    fn remove_file(&self, path: &str) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn remove_dir(&self, path: &str) -> io::Result<()> {
        std::fs::remove_dir(path)
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn space(&self, path: &str) -> Option<(u64, u64)> {
        Some((
            fs4::available_space(path).ok()?,
            fs4::total_space(path).ok()?,
        ))
    }
}
