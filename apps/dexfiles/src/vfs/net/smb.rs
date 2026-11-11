//! SMB 2/3 (Windows shares, Samba, most NAS) with the pure-Rust `smb` crate, in its blocking
//! multi-threaded flavour. NTLM authentication (user and password, or guest).

use std::io::{self, Read, Write};
use std::time::Duration;

use smb::{
    Client, ClientConfig, ConnectionConfig, CreateOptions, DirAccessMask, Directory, FileAccessMask, FileAttributes,
    FileCreateArgs, FileDispositionInformation, FileFullDirectoryInformation, FileRenameInformation, ReadAt, Resource,
    UncPath, WriteAt,
};

use crate::vfs::servers::Server;
use crate::vfs::{self, Fs, Kind, Progress, RawEntry};

pub struct SmbFs {
    client: Client,
    share: UncPath,
}

fn err(e: smb::Error) -> io::Error {
    vfs::other(e)
}

impl SmbFs {
    pub fn connect(server: &Server) -> io::Result<Self> {
        let mut connection = ConnectionConfig::default();
        connection.timeout = Some(Duration::from_secs(20));
        connection.auth_methods.ntlm = true;
        connection.auth_methods.kerberos = false;
        if server.port != 445 {
            connection.port = Some(server.port);
        }
        if server.user.is_empty() {
            connection.allow_unsigned_guest_access = true;
        }
        let client = Client::new(ClientConfig { connection, ..Default::default() });
        let share_name = server.share.trim_matches(|c| c == '/' || c == '\\');
        if share_name.is_empty() {
            return Err(vfs::other("SMB: no share given"));
        }
        let share = UncPath::new(&server.host).and_then(|u| u.with_share(share_name)).map_err(err)?;
        let (user, password) = if server.user.is_empty() { ("Guest".to_owned(), String::new()) } else { (server.user.clone(), server.password.clone()) };
        client.share_connect(&share, &user, password).map_err(err)?;
        Ok(SmbFs { client, share })
    }

    fn unc(&self, path: &str) -> UncPath {
        let inner = path.trim_matches('/').replace('/', "\\");
        if inner.is_empty() { self.share.clone() } else { self.share.clone().with_path(&inner) }
    }

    fn open(&self, path: &str, args: &FileCreateArgs) -> io::Result<Resource> {
        self.client.create_file(&self.unc(path), args).map_err(err)
    }

    /// Mark for deletion and close (files and empty folders alike).
    fn delete(&self, path: &str, dir: bool) -> io::Result<()> {
        let mut args = FileCreateArgs::make_open_existing(FileAccessMask::new().with_delete(true).with_generic_read(true));
        if dir {
            args.options = CreateOptions::new().with_directory_file(true);
        }
        let res = self.open(path, &args)?;
        let result = match &res {
            Resource::File(f) => f.set_info(FileDispositionInformation { delete_pending: true.into() }),
            Resource::Directory(d) => d.set_info(FileDispositionInformation { delete_pending: true.into() }),
            _ => Ok(()),
        };
        close(res);
        result.map_err(err)
    }
}

fn close(res: Resource) {
    let r = match &res {
        Resource::File(f) => f.close(),
        Resource::Directory(d) => d.close(),
        Resource::Pipe(p) => p.close(),
    };
    if let Err(e) = r {
        log::debug!("dexfiles: smb close: {e}");
    }
}

fn filetime(t: time::PrimitiveDateTime) -> Option<i64> {
    let dt = t.assume_utc();
    let secs = dt.unix_timestamp();
    // 1601 (an unset time) is not a date to show.
    if secs < 0 { None } else { Some(secs) }
}

impl Fs for SmbFs {
    fn list(&self, path: &str) -> io::Result<Vec<RawEntry>> {
        let args = FileCreateArgs::make_open_existing(DirAccessMask::new().with_list_directory(true).with_synchronize(true).into());
        let dir = self.open(path, &args)?;
        let Resource::Directory(dir) = dir else {
            close(dir);
            return Err(io::Error::new(io::ErrorKind::NotADirectory, "not a folder"));
        };
        let mut out = Vec::new();
        let result = (|| -> smb::Result<()> {
            for item in Directory::query::<FileFullDirectoryInformation>(&dir, "*")? {
                let item = item?;
                let name = item.file_name.to_string();
                if name == "." || name == ".." {
                    continue;
                }
                let attrs = item.file_attributes;
                let is_dir = attrs.directory();
                out.push(RawEntry {
                    name,
                    kind: if is_dir { Kind::Dir } else { Kind::File },
                    size: if is_dir { 0 } else { item.end_of_file },
                    modified: filetime(item.last_write_time.date_time()),
                    created: filetime(item.creation_time.date_time()),
                    symlink: false,
                    readonly: attrs.readonly(),
                    hidden_attr: attrs.hidden() || attrs.system(),
                });
            }
            Ok(())
        })();
        let _ = dir.close();
        result.map_err(err)?;
        Ok(out)
    }

    fn stat(&self, path: &str) -> io::Result<RawEntry> {
        let trimmed = path.trim_end_matches('/');
        let (parent, name) = match trimmed.rfind('/') {
            Some(i) => (if i == 0 { "/" } else { &trimmed[..i] }, &trimmed[i + 1..]),
            None => ("/", trimmed),
        };
        if name.is_empty() {
            return Ok(RawEntry { name: "/".into(), kind: Kind::Dir, size: 0, modified: None, created: None, symlink: false, readonly: false, hidden_attr: false });
        }
        self.list(parent)?.into_iter().find(|e| e.name == name).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not found"))
    }

    fn read_into(&self, path: &str, out: &mut dyn Write, progress: Progress<'_>) -> io::Result<u64> {
        let res = self.open(path, &FileCreateArgs::make_open_existing(FileAccessMask::new().with_generic_read(true)))?;
        let Resource::File(file) = res else {
            close(res);
            return Err(io::Error::new(io::ErrorKind::IsADirectory, "not a file"));
        };
        let mut buf = vec![0u8; 256 * 1024];
        let mut offset = 0u64;
        let result = loop {
            match file.read_at(&mut buf, offset) {
                Ok(0) => break Ok(offset),
                Ok(n) => {
                    if let Err(e) = out.write_all(&buf[..n]) {
                        break Err(e);
                    }
                    offset += n as u64;
                    if !progress(offset) {
                        break Err(vfs::cancelled());
                    }
                }
                // End of file is reported as an error status by some servers.
                Err(e) if e.to_string().to_ascii_lowercase().contains("end of file") || e.to_string().contains("EndOfFile") => break Ok(offset),
                Err(e) => break Err(err(e)),
            }
        };
        let _ = file.close();
        result
    }

    fn write_from(&self, path: &str, input: &mut dyn Read, _size: u64, progress: Progress<'_>) -> io::Result<u64> {
        let res = self.open(path, &FileCreateArgs::make_overwrite(FileAttributes::new(), CreateOptions::new()))?;
        let Resource::File(file) = res else {
            close(res);
            return Err(io::Error::new(io::ErrorKind::IsADirectory, "not a file"));
        };
        let mut buf = vec![0u8; 256 * 1024];
        let mut offset = 0u64;
        let result = loop {
            let n = match input.read(&mut buf) {
                Ok(0) => break Ok(offset),
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => break Err(e),
            };
            let mut written = 0;
            while written < n {
                match file.write_at(&buf[written..n], offset + written as u64) {
                    Ok(0) => break,
                    Ok(w) => written += w,
                    Err(e) => return Err(err(e)),
                }
            }
            offset += n as u64;
            if !progress(offset) {
                break Err(vfs::cancelled());
            }
        };
        let _ = file.close();
        result
    }

    fn mkdir(&self, path: &str) -> io::Result<()> {
        let res = self.open(
            path,
            &FileCreateArgs::make_create_new(FileAttributes::new().with_directory(true), CreateOptions::new().with_directory_file(true)),
        )?;
        close(res);
        Ok(())
    }

    fn remove_file(&self, path: &str) -> io::Result<()> {
        self.delete(path, false)
    }

    fn remove_dir(&self, path: &str) -> io::Result<()> {
        self.delete(path, true)
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let res = self.open(from, &FileCreateArgs::make_open_existing(FileAccessMask::new().with_delete(true).with_generic_read(true)))?;
        let target = to.trim_matches('/').replace('/', "\\");
        let info = FileRenameInformation { replace_if_exists: false.into(), root_directory: 0, file_name: target.as_str().into() };
        let result = match &res {
            Resource::File(f) => f.set_info(info),
            Resource::Directory(d) => d.set_info(info),
            _ => Ok(()),
        };
        close(res);
        result.map_err(err)
    }
}
