//! SFTP over SSH with russh and russh-sftp (async; run on the shared runtime).
//!
//! Host keys: trust on first use. The first key a server shows is recorded in
//! `known_hosts.txt` (the app's data folder); a different key later is refused.

use std::io::{self, Read, Write};
use std::sync::Arc;

use russh_sftp::client::SftpSession;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::vfs::servers::Server;
use crate::vfs::{self, Fs, Kind, Progress, RawEntry};

pub struct SftpFs {
    sftp: SftpSession,
    // Kept alive with the session.
    _handle: russh::client::Handle<Client>,
}

struct Client {
    host: String,
    /// Set when the key doesn't match the recorded one.
    mismatch: Arc<std::sync::atomic::AtomicBool>,
}

impl russh::client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &russh::keys::PublicKey) -> Result<bool, Self::Error> {
        let fingerprint = key.fingerprint(russh::keys::HashAlg::Sha256).to_string();
        let ok = known_hosts_check(&self.host, &fingerprint);
        if !ok {
            self.mismatch.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(ok)
    }
}

fn known_hosts_file() -> std::path::PathBuf {
    crate::platform::data_dir().join("known_hosts.txt")
}

/// True if `host` is unknown (then it is recorded) or known with this key.
fn known_hosts_check(host: &str, fingerprint: &str) -> bool {
    let file = known_hosts_file();
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    for line in text.lines() {
        if let Some((h, f)) = line.split_once(' ')
            && h == host
        {
            return f.trim() == fingerprint;
        }
    }
    let _ = std::fs::create_dir_all(file.parent().unwrap_or(std::path::Path::new(".")));
    let _ = std::fs::write(&file, format!("{text}{host} {fingerprint}\n"));
    true
}

/// Forget a server's recorded key (after the user accepts a new one).
pub fn forget_host(host: &str, port: u16) {
    let key = format!("{host}:{port}");
    let file = known_hosts_file();
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let kept: String = text.lines().filter(|l| !l.starts_with(&format!("{key} "))).map(|l| format!("{l}\n")).collect();
    let _ = std::fs::write(&file, kept);
}

fn err(e: impl std::fmt::Display) -> io::Error {
    vfs::other(e)
}

impl SftpFs {
    pub fn connect(server: &Server) -> io::Result<Self> {
        let rt = super::runtime()?;
        let host = format!("{}:{}", server.host, server.port);
        let mismatch = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let client = Client { host: host.clone(), mismatch: mismatch.clone() };
        let config = Arc::new(russh::client::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(600)),
            keepalive_interval: Some(std::time::Duration::from_secs(30)),
            ..Default::default()
        });
        let server = server.clone();
        rt.block_on(async move {
            let connecting = russh::client::connect(config, (server.host.as_str(), server.port), client);
            let mut handle = match tokio::time::timeout(std::time::Duration::from_secs(20), connecting).await {
                Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, "connection timed out")),
                Ok(Err(e)) => {
                    if mismatch.load(std::sync::atomic::Ordering::SeqCst) {
                        return Err(vfs::other(format!("HOST KEY CHANGED for {host}: refusing to connect")));
                    }
                    return Err(err(e));
                }
                Ok(Ok(h)) => h,
            };
            let auth = handle.authenticate_password(server.user.clone(), server.password.clone()).await.map_err(err)?;
            if !auth.success() {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "authentication failed"));
            }
            let channel = handle.channel_open_session().await.map_err(err)?;
            channel.request_subsystem(true, "sftp").await.map_err(err)?;
            let sftp = SftpSession::new(channel.into_stream()).await.map_err(err)?;
            Ok(SftpFs { sftp, _handle: handle })
        })
    }

    fn block<T>(&self, f: impl std::future::Future<Output = io::Result<T>>) -> io::Result<T> {
        super::runtime()?.block_on(f)
    }
}

fn raw(name: String, attrs: &russh_sftp::protocol::FileAttributes) -> RawEntry {
    let kind = if attrs.is_dir() {
        Kind::Dir
    } else if attrs.is_regular() || attrs.is_symlink() || attrs.permissions.is_none() {
        Kind::File
    } else {
        Kind::Other
    };
    RawEntry {
        name,
        kind,
        size: attrs.size.unwrap_or(0),
        modified: attrs.mtime.map(i64::from),
        created: None,
        symlink: attrs.is_symlink(),
        readonly: attrs.permissions.is_some_and(|p| p & 0o200 == 0),
        hidden_attr: false,
    }
}

impl Fs for SftpFs {
    fn list(&self, path: &str) -> io::Result<Vec<RawEntry>> {
        self.block(async {
            let dir = self.sftp.read_dir(path).await.map_err(err)?;
            let mut out = Vec::new();
            for entry in dir {
                let name = entry.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                let mut e = raw(name.clone(), &entry.metadata());
                if e.symlink {
                    // Follow the link for its kind (a link to a folder opens as one).
                    if let Ok(target) = self.sftp.metadata(super::join(path, &name)).await {
                        e.kind = if target.is_dir() { Kind::Dir } else { Kind::File };
                        e.size = target.size.unwrap_or(0);
                    }
                }
                out.push(e);
            }
            Ok(out)
        })
    }

    fn stat(&self, path: &str) -> io::Result<RawEntry> {
        self.block(async {
            let m = self.sftp.metadata(path).await.map_err(err)?;
            let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or("/").to_owned();
            Ok(raw(name, &m))
        })
    }

    fn read_into(&self, path: &str, out: &mut dyn Write, progress: Progress<'_>) -> io::Result<u64> {
        self.block(async {
            let mut file = self.sftp.open(path).await.map_err(err)?;
            let mut buf = vec![0u8; 256 * 1024];
            let mut total = 0u64;
            loop {
                let n = file.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])?;
                total += n as u64;
                if !progress(total) {
                    return Err(vfs::cancelled());
                }
            }
            out.flush()?;
            Ok(total)
        })
    }

    fn write_from(&self, path: &str, input: &mut dyn Read, _size: u64, progress: Progress<'_>) -> io::Result<u64> {
        self.block(async {
            let mut file = self.sftp.create(path).await.map_err(err)?;
            let mut buf = vec![0u8; 256 * 1024];
            let mut total = 0u64;
            loop {
                let n = match input.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                };
                file.write_all(&buf[..n]).await?;
                total += n as u64;
                if !progress(total) {
                    return Err(vfs::cancelled());
                }
            }
            file.shutdown().await?;
            Ok(total)
        })
    }

    fn mkdir(&self, path: &str) -> io::Result<()> {
        self.block(async { self.sftp.create_dir(path).await.map_err(err) })
    }

    fn remove_file(&self, path: &str) -> io::Result<()> {
        self.block(async { self.sftp.remove_file(path).await.map_err(err) })
    }

    fn remove_dir(&self, path: &str) -> io::Result<()> {
        self.block(async { self.sftp.remove_dir(path).await.map_err(err) })
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        self.block(async { self.sftp.rename(from, to).await.map_err(err) })
    }

    fn space(&self, path: &str) -> Option<(u64, u64)> {
        let info = self.block(async { self.sftp.fs_info(path).await.map_err(err) }).ok()??;
        Some((info.blocks_avail.saturating_mul(info.fragment_size), info.blocks.saturating_mul(info.fragment_size)))
    }
}
