//! FTP and FTPS (explicit TLS) with suppaftp.

use std::io::{self, Read, Write};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use suppaftp::list::File as ListFile;
use suppaftp::{FtpError, RustlsConnector, RustlsFtpStream};

use crate::vfs::servers::{Server, ServerKind};
use crate::vfs::{self, Fs, Kind, Progress, RawEntry};

pub struct FtpFs {
    server: Server,
    stream: Mutex<Option<RustlsFtpStream>>,
}

fn err(e: FtpError) -> io::Error {
    match e {
        FtpError::ConnectionError(e) => e,
        other => vfs::other(other),
    }
}

impl FtpFs {
    pub fn connect(server: &Server) -> io::Result<Self> {
        let fs = FtpFs {
            server: server.clone(),
            stream: Mutex::new(None),
        };
        let stream = fs.open()?;
        *fs.stream.lock().unwrap_or_else(PoisonError::into_inner) = Some(stream);
        Ok(fs)
    }

    fn open(&self) -> io::Result<RustlsFtpStream> {
        let s = &self.server;
        let addr = std::net::ToSocketAddrs::to_socket_addrs(&(s.host.as_str(), s.port))?
            .next()
            .ok_or_else(|| vfs::other("host not found"))?;
        let mut ftp =
            RustlsFtpStream::connect_timeout(addr, Duration::from_secs(15)).map_err(err)?;
        if s.kind == ServerKind::Ftps {
            let config = super::tls_config(s.accept_invalid_certs)?;
            ftp = ftp
                .into_secure(RustlsConnector::from(config), &s.host)
                .map_err(err)?;
        }
        let user = if s.user.is_empty() {
            "anonymous"
        } else {
            s.user.as_str()
        };
        let password = if s.user.is_empty() && s.password.is_empty() {
            "dexfiles@"
        } else {
            s.password.as_str()
        };
        ftp.login(user, password).map_err(err)?;
        ftp.transfer_type(suppaftp::types::FileType::Binary)
            .map_err(err)?;
        Ok(ftp)
    }

    /// After an aborted transfer servers answer unevenly (226 or 225, sometimes both), which
    /// leaves the control channel out of step: start the next operation on a fresh connection.
    fn drop_connection(&self) {
        *self.stream.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Run `f` on the connection; if the connection dropped, reconnect once and retry.
    fn with<T>(
        &self,
        mut f: impl FnMut(&mut RustlsFtpStream) -> Result<T, FtpError>,
    ) -> io::Result<T> {
        let mut guard = self.stream.lock().unwrap_or_else(PoisonError::into_inner);
        for attempt in 0..2 {
            if guard.is_none() {
                *guard = Some(self.open()?);
            }
            let Some(ftp) = guard.as_mut() else { continue };
            match f(ftp) {
                Ok(v) => return Ok(v),
                Err(FtpError::ConnectionError(e)) if attempt == 0 => {
                    log::info!("dexfiles: ftp connection lost ({e}); reconnecting");
                    *guard = None;
                }
                Err(e) => return Err(err(e)),
            }
        }
        Err(vfs::other("FTP connection lost"))
    }
}

fn to_raw(f: &ListFile) -> RawEntry {
    let kind = if f.is_directory() {
        Kind::Dir
    } else if f.is_symlink() {
        // A link's target kind is unknown without following it: treat as a folder if it has no
        // size (most servers list links to folders that way).
        if f.size() == 0 { Kind::Dir } else { Kind::File }
    } else {
        Kind::File
    };
    RawEntry {
        name: f.name().to_owned(),
        kind,
        size: f.size() as u64,
        modified: super::unix(f.modified()),
        created: None,
        symlink: f.is_symlink(),
        readonly: false,
        hidden_attr: false,
    }
}

impl Fs for FtpFs {
    fn list(&self, path: &str) -> io::Result<Vec<RawEntry>> {
        let lines = self.with(|ftp| match ftp.mlsd(Some(path)) {
            Ok(lines) => Ok((true, lines)),
            Err(FtpError::UnexpectedResponse(_)) => ftp.list(Some(path)).map(|l| (false, l)),
            Err(e) => Err(e),
        })?;
        let (mlsd, lines) = lines;
        let mut out = Vec::new();
        for line in lines {
            let parsed = if mlsd {
                suppaftp::list::ListParser::parse_mlsd(&line).ok()
            } else {
                suppaftp::list::ListParser::parse_posix(&line)
                    .or_else(|_| suppaftp::list::ListParser::parse_dos(&line))
                    .ok()
            };
            let Some(f) = parsed else { continue };
            if f.name() == "." || f.name() == ".." || f.name().is_empty() {
                continue;
            }
            out.push(to_raw(&f));
        }
        Ok(out)
    }

    fn stat(&self, path: &str) -> io::Result<RawEntry> {
        let trimmed = path.trim_end_matches('/');
        let (parent, name) = match trimmed.rfind('/') {
            Some(i) => (if i == 0 { "/" } else { &trimmed[..i] }, &trimmed[i + 1..]),
            None => ("/", trimmed),
        };
        if name.is_empty() {
            return Ok(RawEntry {
                name: "/".into(),
                kind: Kind::Dir,
                size: 0,
                modified: None,
                created: None,
                symlink: false,
                readonly: false,
                hidden_attr: false,
            });
        }
        self.list(parent)?
            .into_iter()
            .find(|e| e.name == name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not found"))
    }

    fn read_into(
        &self,
        path: &str,
        out: &mut dyn Write,
        progress: Progress<'_>,
    ) -> io::Result<u64> {
        let mut result = Ok(0);
        let mut aborted = false;
        self.with(|ftp| {
            let mut stream = ftp.retr_as_stream(path)?;
            result = vfs::copy_stream(&mut stream, out, progress);
            if result.is_err() {
                aborted = true;
                let _ = ftp.abort(stream);
                return Ok(());
            }
            stream.finish()
        })?;
        if aborted {
            self.drop_connection();
        }
        result
    }

    fn write_from(
        &self,
        path: &str,
        input: &mut dyn Read,
        _size: u64,
        progress: Progress<'_>,
    ) -> io::Result<u64> {
        let mut result = Ok(0);
        let mut aborted = false;
        self.with(|ftp| {
            let mut stream = ftp.put_with_stream(path)?;
            result = vfs::copy_stream(input, &mut stream, progress);
            if result.is_err() {
                aborted = true;
                let _ = ftp.abort(stream);
                return Ok(());
            }
            stream.finish()
        })?;
        if aborted {
            self.drop_connection();
        }
        result
    }

    fn mkdir(&self, path: &str) -> io::Result<()> {
        self.with(|ftp| ftp.mkdir(path))
    }

    fn remove_file(&self, path: &str) -> io::Result<()> {
        self.with(|ftp| ftp.rm(path))
    }

    fn remove_dir(&self, path: &str) -> io::Result<()> {
        self.with(|ftp| ftp.rmdir(path))
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        self.with(|ftp| ftp.rename(from, to))
    }
}
