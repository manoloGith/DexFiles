//! Compressing and extracting archives (see docs/compression.md for the choices): ZIP (`zip`,
//! with AES passwords), 7z (`sevenz-rust2`, with AES passwords), TAR with gzip (`flate2`), bzip2
//! (`bzip2` on `libbz2-rs-sys`), xz (`lzma-rust2`) and Zstandard (`ruzstd`), single compressed
//! files, and — with the `rar` feature, in the builds published on GitHub only — RAR extraction
//! (`unrar`, RARLAB's non-free UnRAR).

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::vfs;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ArchiveFormat {
    Zip,
    SevenZ,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
}

impl ArchiveFormat {
    pub const ALL: [ArchiveFormat; 7] = [
        ArchiveFormat::Zip,
        ArchiveFormat::SevenZ,
        ArchiveFormat::TarGz,
        ArchiveFormat::TarXz,
        ArchiveFormat::TarZst,
        ArchiveFormat::TarBz2,
        ArchiveFormat::Tar,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::SevenZ => "7z",
            ArchiveFormat::Tar => "tar",
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::TarBz2 => "tar.bz2",
            ArchiveFormat::TarXz => "tar.xz",
            ArchiveFormat::TarZst => "tar.zst",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ArchiveFormat::Zip => "ZIP",
            ArchiveFormat::SevenZ => "7z",
            ArchiveFormat::Tar => "TAR",
            ArchiveFormat::TarGz => "TAR.GZ",
            ArchiveFormat::TarBz2 => "TAR.BZ2",
            ArchiveFormat::TarXz => "TAR.XZ",
            ArchiveFormat::TarZst => "TAR.ZST",
        }
    }

    /// Whether the format can be protected with a password (AES-256).
    pub fn supports_password(self) -> bool {
        matches!(self, ArchiveFormat::Zip | ArchiveFormat::SevenZ)
    }
}

/// Whether this build extracts RAR archives (the `rar` feature).
pub const RAR: bool = cfg!(feature = "rar");

/// What can be extracted, by extension (as [`vfs::extension`] gives it).
pub fn is_archive(ext: &str) -> bool {
    matches!(
        ext,
        "zip" | "7z" | "tar" | "tar.gz" | "tgz" | "tar.bz2" | "tbz2" | "tar.xz" | "txz" | "tar.zst" | "tzst" | "gz" | "bz2" | "xz" | "zst" | "jar" | "apk" | "cbz" | "epub"
    ) || (RAR && matches!(ext, "rar" | "cbr"))
}

/// Done bytes, total bytes, current name; `false` cancels.
pub type Progress<'a> = &'a mut dyn FnMut(u64, u64, &str) -> bool;

const PASSWORD_NEEDED: &str = "archive password needed";
const PASSWORD_WRONG: &str = "wrong archive password";

/// The archive is encrypted and no password was given.
pub fn needs_password(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::PermissionDenied && (e.to_string() == PASSWORD_NEEDED || e.to_string() == PASSWORD_WRONG)
}

/// The password given was wrong.
pub fn wrong_password(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::PermissionDenied && e.to_string() == PASSWORD_WRONG
}

fn password_error(given: bool) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, if given { PASSWORD_WRONG } else { PASSWORD_NEEDED })
}

/// Files under `sources`, with their names inside the archive.
fn collect(sources: &[PathBuf]) -> io::Result<Vec<(PathBuf, String, bool, u64)>> {
    let mut out = Vec::new();
    fn walk(path: &Path, name: String, out: &mut Vec<(PathBuf, String, bool, u64)>) -> io::Result<()> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_dir() {
            out.push((path.to_path_buf(), name.clone(), true, 0));
            let mut children: Vec<_> = std::fs::read_dir(path)?.flatten().collect();
            children.sort_by_key(|c| c.file_name());
            for c in children {
                walk(&c.path(), format!("{name}/{}", c.file_name().to_string_lossy()), out)?;
            }
        } else if meta.is_file() {
            out.push((path.to_path_buf(), name, false, meta.len()));
        }
        Ok(())
    }
    for s in sources {
        let name = s.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "root".into());
        walk(s, name, &mut out)?;
    }
    Ok(out)
}

struct ProgressReader<'a, 'b, R> {
    inner: R,
    done: &'a mut u64,
    total: u64,
    name: &'a str,
    progress: &'a mut Progress<'b>,
}

impl<R: Read> Read for ProgressReader<'_, '_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        *self.done += n as u64;
        if !(self.progress)(*self.done, self.total, self.name) {
            return Err(vfs::cancelled());
        }
        Ok(n)
    }
}

fn zip_time(path: &Path) -> Option<zip::DateTime> {
    use chrono::{Datelike, Timelike};
    let t: chrono::DateTime<chrono::Local> = std::fs::metadata(path).ok()?.modified().ok()?.into();
    zip::DateTime::from_date_and_time(
        u16::try_from(t.year()).ok()?,
        u8::try_from(t.month()).ok()?,
        u8::try_from(t.day()).ok()?,
        u8::try_from(t.hour()).ok()?,
        u8::try_from(t.minute()).ok()?,
        u8::try_from(t.second()).ok()?,
    )
    .ok()
}

fn zerr(e: zip::result::ZipError) -> io::Error {
    match e {
        zip::result::ZipError::Io(e) => e,
        zip::result::ZipError::InvalidPassword => password_error(true),
        other => vfs::other(other),
    }
}

fn serr(e: sevenz_rust2::Error, password_given: bool) -> io::Error {
    match e {
        sevenz_rust2::Error::PasswordRequired => password_error(false),
        sevenz_rust2::Error::MaybeBadPassword(_) => password_error(password_given),
        other => vfs::other(other),
    }
}

/// Compress `sources` into the new archive `target` (`password`: ZIP and 7z only, AES-256).
pub fn compress(sources: &[PathBuf], target: &Path, format: ArchiveFormat, password: Option<&str>, mut progress: Progress<'_>) -> io::Result<()> {
    let items = collect(sources)?;
    let total: u64 = items.iter().map(|i| i.3).sum();
    let mut done = 0u64;
    let password = password.filter(|p| !p.is_empty() && format.supports_password());
    match format {
        ArchiveFormat::Zip => {
            let mut zip = zip::ZipWriter::new(BufWriter::new(File::create(target)?));
            for (path, name, is_dir, _) in &items {
                let mut options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
                if let Some(t) = zip_time(path) {
                    options = options.last_modified_time(t);
                }
                if let Some(pw) = password {
                    options = options.with_aes_encryption(zip::AesMode::Aes256, pw);
                }
                if *is_dir {
                    zip.add_directory(format!("{name}/"), options).map_err(zerr)?;
                } else {
                    zip.start_file(name.clone(), options).map_err(zerr)?;
                    let mut r = ProgressReader { inner: File::open(path)?, done: &mut done, total, name, progress: &mut progress };
                    io::copy(&mut r, &mut zip)?;
                }
            }
            zip.finish().map_err(zerr)?.flush()?;
        }
        ArchiveFormat::SevenZ => {
            let mut sz = sevenz_rust2::ArchiveWriter::create(target).map_err(|e| serr(e, false))?;
            if let Some(pw) = password {
                sz.set_content_methods(vec![
                    sevenz_rust2::encoder_options::AesEncoderOptions::new(pw.into()).into(),
                    sevenz_rust2::encoder_options::Lzma2Options::default().into(),
                ]);
                sz.set_encrypt_header(true);
            }
            for (path, name, is_dir, _) in &items {
                let entry = sevenz_rust2::ArchiveEntry::from_path(path, name.clone());
                if *is_dir {
                    sz.push_archive_entry::<File>(entry, None).map_err(|e| serr(e, false))?;
                } else {
                    let r = ProgressReader { inner: File::open(path)?, done: &mut done, total, name, progress: &mut progress };
                    sz.push_archive_entry(entry, Some(r)).map_err(|e| serr(e, false))?;
                }
            }
            sz.finish()?;
        }
        ArchiveFormat::Tar | ArchiveFormat::TarGz | ArchiveFormat::TarBz2 | ArchiveFormat::TarXz | ArchiveFormat::TarZst => {
            // Zstandard's encoder takes a reader: the TAR goes to a temporary file first.
            let zst_tmp = (format == ArchiveFormat::TarZst).then(|| target.with_extension("tar.part"));
            let file = BufWriter::new(File::create(zst_tmp.as_deref().unwrap_or(target))?);
            let out: Box<dyn Write> = match format {
                ArchiveFormat::TarGz => Box::new(flate2::write::GzEncoder::new(file, flate2::Compression::default())),
                ArchiveFormat::TarBz2 => Box::new(bzip2::write::BzEncoder::new(file, bzip2::Compression::default())),
                ArchiveFormat::TarXz => Box::new(lzma_rust2::XzWriter::new(file, lzma_rust2::XzOptions::with_preset(6)).map_err(vfs::other)?),
                _ => Box::new(file),
            };
            let mut tar = tar::Builder::new(out);
            tar.follow_symlinks(false);
            for (path, name, is_dir, size) in &items {
                if *is_dir {
                    tar.append_dir(name, path)?;
                } else {
                    let mut header = tar::Header::new_gnu();
                    header.set_metadata(&std::fs::metadata(path)?);
                    header.set_size(*size);
                    header.set_cksum();
                    let r = ProgressReader { inner: File::open(path)?, done: &mut done, total, name, progress: &mut progress };
                    tar.append_data(&mut header, name, r)?;
                }
            }
            let out = tar.into_inner()?;
            finish_writer(out)?;
            if let Some(tmp) = zst_tmp {
                let result = (|| {
                    let input = BufReader::new(File::open(&tmp)?);
                    let mut output = BufWriter::new(File::create(target)?);
                    ruzstd::encoding::compress(input, &mut output, ruzstd::encoding::CompressionLevel::Fastest);
                    output.flush()
                })();
                let _ = std::fs::remove_file(&tmp);
                result?;
            }
        }
    }
    Ok(())
}

/// Flush a compressor (its `Drop` would finish it silently, hiding errors).
fn finish_writer(mut w: Box<dyn Write>) -> io::Result<()> {
    w.flush()?;
    drop(w);
    Ok(())
}

/// A path inside `dest` for an entry name, refusing names that would leave it.
fn safe_join(dest: &Path, name: &str) -> Option<PathBuf> {
    let mut out = dest.to_path_buf();
    for c in Path::new(&name.replace('\\', "/")).components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (out != dest).then_some(out)
}

/// One entry of an archive, for the preview.
#[derive(Clone, Debug)]
pub struct ArchiveItem {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

fn kind_of(path: &Path) -> String {
    vfs::extension(&path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

/// The entries of a local archive (up to `limit`), and how many there are.
pub fn list(path: &Path, limit: usize) -> io::Result<(Vec<ArchiveItem>, usize)> {
    let ext = kind_of(path);
    let mut out = Vec::new();
    let mut count = 0usize;
    let mut push = |name: String, size: u64, is_dir: bool| {
        count += 1;
        if out.len() < limit {
            out.push(ArchiveItem { name, size, is_dir });
        }
    };
    match ext.as_str() {
        "zip" | "jar" | "apk" | "cbz" | "epub" => {
            let mut zip = zip::ZipArchive::new(BufReader::new(File::open(path)?)).map_err(zerr)?;
            for i in 0..zip.len() {
                let f = zip.by_index_raw(i).map_err(zerr)?;
                push(f.name().map(|n| n.into_owned()).unwrap_or_default(), f.size(), f.is_dir());
            }
        }
        "7z" => {
            let archive = sevenz_rust2::Archive::open(path).map_err(|e| serr(e, false))?;
            for f in &archive.files {
                push(f.name().to_owned(), f.size(), f.is_directory());
            }
        }
        "tar" | "tar.gz" | "tgz" | "tar.bz2" | "tbz2" | "tar.xz" | "txz" | "tar.zst" | "tzst" => {
            let mut tar = tar::Archive::new(decoder(path, &ext)?);
            for e in tar.entries()? {
                let e = e?;
                let name = e.path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                push(name, e.size(), e.header().entry_type().is_dir());
            }
        }
        "gz" | "bz2" | "xz" | "zst" => {
            let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            push(name, 0, false);
        }
        #[cfg(feature = "rar")]
        "rar" | "cbr" => {
            let archive = unrar::Archive::new(path).open_for_listing().map_err(rar_err)?;
            for e in archive {
                let e = e.map_err(rar_err)?;
                push(e.filename.to_string_lossy().into_owned(), e.unpacked_size, e.is_directory());
            }
        }
        _ => return Err(vfs::other("not an archive")),
    }
    Ok((out, count))
}

#[cfg(feature = "rar")]
fn rar_err(e: unrar::error::UnrarError) -> io::Error {
    match e.code {
        unrar::error::Code::MissingPassword => password_error(false),
        unrar::error::Code::BadPassword => password_error(true),
        _ => vfs::other(e),
    }
}

fn decoder(path: &Path, ext: &str) -> io::Result<Box<dyn Read>> {
    let file = BufReader::new(File::open(path)?);
    decompressor(file, ext)
}

/// The decompressing reader for a compressed stream's extension.
fn decompressor<R: Read + 'static>(r: R, ext: &str) -> io::Result<Box<dyn Read>> {
    Ok(match ext {
        "tar.gz" | "tgz" | "gz" => Box::new(flate2::read::MultiGzDecoder::new(r)),
        "tar.bz2" | "tbz2" | "bz2" => Box::new(bzip2::read::MultiBzDecoder::new(r)),
        "tar.xz" | "txz" | "xz" => Box::new(lzma_rust2::XzReader::new(r, true)),
        "tar.zst" | "tzst" | "zst" => Box::new(ruzstd::decoding::StreamingDecoder::new(r).map_err(vfs::other)?),
        _ => Box::new(r),
    })
}

/// Extract a local archive into `dest` (created if missing). An encrypted archive without the
/// right `password` fails with an error [`needs_password`] recognises.
pub fn extract(path: &Path, dest: &Path, password: Option<&str>, mut progress: Progress<'_>) -> io::Result<()> {
    let ext = kind_of(path);
    std::fs::create_dir_all(dest)?;
    let total = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut done = 0u64;
    let password = password.filter(|p| !p.is_empty());
    match ext.as_str() {
        "zip" | "jar" | "apk" | "cbz" | "epub" => {
            let mut zip = zip::ZipArchive::new(BufReader::new(File::open(path)?)).map_err(zerr)?;
            let total: u64 = (0..zip.len()).filter_map(|i| zip.by_index_raw(i).ok().map(|f| f.size())).sum();
            for i in 0..zip.len() {
                let encrypted = zip.by_index_raw(i).map(|f| f.encrypted()).unwrap_or(false);
                if encrypted && password.is_none() {
                    return Err(password_error(false));
                }
                let mut f = match password {
                    Some(pw) if encrypted => zip.by_index_decrypt(i, pw.as_bytes()).map_err(zerr)?,
                    _ => zip.by_index(i).map_err(zerr)?,
                };
                let name = f.name().map(|n| n.into_owned()).unwrap_or_default();
                let Some(out) = f.enclosed_name().and_then(|n| safe_join(dest, &n.to_string_lossy())) else { continue };
                if f.is_dir() {
                    std::fs::create_dir_all(&out)?;
                    continue;
                }
                if let Some(p) = out.parent() {
                    std::fs::create_dir_all(p)?;
                }
                let mut w = BufWriter::new(File::create(&out)?);
                let mut r = ProgressReader { inner: &mut f, done: &mut done, total, name: &name, progress: &mut progress };
                // AES checks the password while reading: a wrong one fails here.
                if let Err(e) = io::copy(&mut r, &mut w) {
                    drop(w);
                    let _ = std::fs::remove_file(&out);
                    return Err(if encrypted && !vfs::is_cancelled(&e) { password_error(true) } else { e });
                }
                w.flush()?;
            }
        }
        "7z" => {
            let mut failure: Option<io::Error> = None;
            let total = sevenz_rust2::Archive::open_with_password(path, &password.unwrap_or_default().into())
                .map(|a| a.files.iter().map(|f| f.size()).sum())
                .unwrap_or(total);
            let file = BufReader::new(File::open(path)?);
            let pw: sevenz_rust2::Password = password.unwrap_or_default().into();
            let r = sevenz_rust2::decompress_with_extract_fn_and_password(file, dest, pw, |entry, reader, _suggested| {
                let Some(out) = safe_join(dest, entry.name()) else { return Ok(true) };
                if entry.is_directory() {
                    std::fs::create_dir_all(&out)?;
                    return Ok(true);
                }
                if let Some(p) = out.parent() {
                    std::fs::create_dir_all(p)?;
                }
                let mut w = BufWriter::new(File::create(&out)?);
                let mut pr = ProgressReader { inner: reader, done: &mut done, total, name: entry.name(), progress: &mut progress };
                match io::copy(&mut pr, &mut w) {
                    Ok(_) => Ok(true),
                    Err(e) => {
                        failure = Some(e);
                        Ok(false)
                    }
                }
            });
            if let Some(e) = failure {
                return Err(e);
            }
            r.map_err(|e| serr(e, password.is_some()))?;
        }
        "tar" | "tar.gz" | "tgz" | "tar.bz2" | "tbz2" | "tar.xz" | "txz" | "tar.zst" | "tzst" => {
            let counting = ProgressReader { inner: File::open(path)?, done: &mut done, total, name: "", progress: &mut progress };
            // `decompressor` needs 'static; read through a bounded scope instead.
            let reader: Box<dyn Read + '_> = match ext.as_str() {
                "tar.gz" | "tgz" => Box::new(flate2::read::MultiGzDecoder::new(BufReader::new(counting))),
                "tar.bz2" | "tbz2" => Box::new(bzip2::read::MultiBzDecoder::new(BufReader::new(counting))),
                "tar.xz" | "txz" => Box::new(lzma_rust2::XzReader::new(BufReader::new(counting), true)),
                "tar.zst" | "tzst" => Box::new(ruzstd::decoding::StreamingDecoder::new(BufReader::new(counting)).map_err(vfs::other)?),
                _ => Box::new(counting),
            };
            let mut tar = tar::Archive::new(reader);
            tar.set_preserve_permissions(false);
            for e in tar.entries()? {
                let mut e = e?;
                // `unpack_in` refuses paths that leave `dest`.
                e.unpack_in(dest)?;
            }
        }
        "gz" | "bz2" | "xz" | "zst" => {
            let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
            let counting = ProgressReader { inner: File::open(path)?, done: &mut done, total, name: &name, progress: &mut progress };
            let mut reader: Box<dyn Read + '_> = match ext.as_str() {
                "gz" => Box::new(flate2::read::MultiGzDecoder::new(BufReader::new(counting))),
                "bz2" => Box::new(bzip2::read::MultiBzDecoder::new(BufReader::new(counting))),
                "zst" => Box::new(ruzstd::decoding::StreamingDecoder::new(BufReader::new(counting)).map_err(vfs::other)?),
                _ => Box::new(lzma_rust2::XzReader::new(BufReader::new(counting), true)),
            };
            let mut w = BufWriter::new(File::create(dest.join(&name))?);
            io::copy(&mut reader, &mut w)?;
            w.flush()?;
        }
        #[cfg(feature = "rar")]
        "rar" | "cbr" => {
            let total: u64 = unrar::Archive::new(path).open_for_listing().map(|a| a.flatten().map(|e| e.unpacked_size).sum()).unwrap_or(total);
            let pw = password.unwrap_or_default();
            let archive = if password.is_some() { unrar::Archive::with_password(path, pw) } else { unrar::Archive::new(path) };
            let mut cursor = archive.open_for_processing().map_err(rar_err)?;
            while let Some(header) = cursor.read_header().map_err(rar_err)? {
                let entry = header.entry();
                let name = entry.filename.to_string_lossy().into_owned();
                if entry.is_encrypted() && password.is_none() {
                    return Err(password_error(false));
                }
                if !progress(done, total, &name) {
                    return Err(vfs::cancelled());
                }
                done += entry.unpacked_size;
                cursor = match safe_join(dest, &name) {
                    Some(out) if entry.is_file() => {
                        if let Some(p) = out.parent() {
                            std::fs::create_dir_all(p)?;
                        }
                        header.extract_to(&out).map_err(rar_err)?
                    }
                    Some(out) => {
                        std::fs::create_dir_all(&out)?;
                        header.skip().map_err(rar_err)?
                    }
                    None => header.skip().map_err(rar_err)?,
                };
            }
        }
        _ => return Err(vfs::other("not an archive this app can open")),
    }
    Ok(())
}

/// A folder name to extract `archive_name` into ("photos.tar.gz" → "photos").
pub fn folder_name(archive_name: &str) -> String {
    let stem = vfs::stem(archive_name);
    if stem.is_empty() { "archive".to_owned() } else { stem.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(root: &Path) -> PathBuf {
        let src = root.join("docs");
        std::fs::create_dir_all(src.join("inner")).unwrap_or_default();
        std::fs::write(src.join("a.txt"), b"alpha").unwrap_or_default();
        std::fs::write(src.join("inner/b.bin"), vec![3u8; 70_000]).unwrap_or_default();
        src
    }

    #[test]
    fn every_format_round_trips() {
        let root = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let src = sample(root.path());
        for format in ArchiveFormat::ALL {
            let archive = root.path().join(format!("out.{}", format.extension()));
            compress(&[src.clone()], &archive, format, None, &mut |_, _, _| true).unwrap_or_else(|e| unreachable!("{format:?}: {e}"));
            let (items, count) = list(&archive, 100).unwrap_or_else(|e| unreachable!("{format:?}: {e}"));
            assert!(count >= 2, "{format:?}: {items:?}");
            let dest = root.path().join(format!("x-{}", format.extension()));
            extract(&archive, &dest, None, &mut |_, _, _| true).unwrap_or_else(|e| unreachable!("{format:?}: {e}"));
            assert_eq!(std::fs::read(dest.join("docs/a.txt")).unwrap_or_default(), b"alpha", "{format:?}");
            assert_eq!(std::fs::read(dest.join("docs/inner/b.bin")).map(|v| v.len()).unwrap_or(0), 70_000, "{format:?}");
        }
    }

    #[test]
    fn passwords() {
        let root = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let src = sample(root.path());
        for format in [ArchiveFormat::Zip, ArchiveFormat::SevenZ] {
            let archive = root.path().join(format!("secret.{}", format.extension()));
            compress(&[src.clone()], &archive, format, Some("ñandú 2026"), &mut |_, _, _| true).unwrap_or_else(|e| unreachable!("{format:?}: {e}"));
            let dest = root.path().join(format!("p-{}", format.extension()));
            let e = extract(&archive, &dest, None, &mut |_, _, _| true).err();
            assert!(e.as_ref().is_some_and(needs_password), "{format:?}: {e:?}");
            let e = extract(&archive, &dest, Some("wrong"), &mut |_, _, _| true).err();
            assert!(e.as_ref().is_some_and(needs_password), "{format:?}: {e:?}");
            extract(&archive, &dest, Some("ñandú 2026"), &mut |_, _, _| true).unwrap_or_else(|e| unreachable!("{format:?}: {e}"));
            assert_eq!(std::fs::read(dest.join("docs/a.txt")).unwrap_or_default(), b"alpha", "{format:?}");
        }
    }

    #[cfg(feature = "rar")]
    #[test]
    fn rar_archives() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
        let root = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let (items, count) = list(&data.join("version.rar"), 10).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(count, 1, "{items:?}");
        extract(&data.join("version.rar"), root.path(), None, &mut |_, _, _| true).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(root.path().join(&items[0].name).is_file());
        let locked = data.join("crypted.rar");
        let dest = root.path().join("locked");
        let e = extract(&locked, &dest, None, &mut |_, _, _| true).err();
        assert!(e.as_ref().is_some_and(needs_password), "{e:?}");
        extract(&locked, &dest, Some("unrar"), &mut |_, _, _| true).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(std::fs::read_dir(&dest).map(|d| d.count()).unwrap_or(0) > 0);
    }

    #[test]
    fn unsafe_names_are_refused() {
        let d = Path::new("/tmp/x");
        assert_eq!(safe_join(d, "a/b.txt"), Some(PathBuf::from("/tmp/x/a/b.txt")));
        assert_eq!(safe_join(d, "../evil"), None);
        assert_eq!(safe_join(d, "/etc/passwd"), None);
        assert_eq!(folder_name("photos.tar.gz"), "photos");
    }
}
