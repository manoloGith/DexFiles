//! Saved network locations and their open connections.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use serde::{Deserialize, Serialize};

use super::Fs;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ServerId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServerKind {
    Ftp,
    /// FTP over TLS (explicit, AUTH TLS).
    Ftps,
    Sftp,
    Smb,
    WebDav,
    WebDavs,
}

impl ServerKind {
    pub const ALL: [ServerKind; 6] = [ServerKind::Sftp, ServerKind::Smb, ServerKind::WebDavs, ServerKind::WebDav, ServerKind::Ftp, ServerKind::Ftps];

    pub fn scheme(self) -> &'static str {
        match self {
            ServerKind::Ftp => "ftp",
            ServerKind::Ftps => "ftps",
            ServerKind::Sftp => "sftp",
            ServerKind::Smb => "smb",
            ServerKind::WebDav => "dav",
            ServerKind::WebDavs => "davs",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ServerKind::Ftp => "FTP",
            ServerKind::Ftps => "FTPS",
            ServerKind::Sftp => "SFTP (SSH)",
            ServerKind::Smb => "SMB (Windows, Samba)",
            ServerKind::WebDav => "WebDAV (http)",
            ServerKind::WebDavs => "WebDAV (https)",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            ServerKind::Ftp | ServerKind::Ftps => 21,
            ServerKind::Sftp => 22,
            ServerKind::Smb => 445,
            ServerKind::WebDav => 80,
            ServerKind::WebDavs => 443,
        }
    }

    pub fn from_scheme(s: &str) -> Option<ServerKind> {
        Some(match s.to_ascii_lowercase().as_str() {
            "ftp" => ServerKind::Ftp,
            "ftps" | "ftpes" => ServerKind::Ftps,
            "sftp" | "ssh" => ServerKind::Sftp,
            "smb" | "cifs" => ServerKind::Smb,
            "dav" | "webdav" | "http" => ServerKind::WebDav,
            "davs" | "webdavs" | "https" => ServerKind::WebDavs,
            _ => return None,
        })
    }
}

/// A saved network location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    pub id: ServerId,
    pub name: String,
    pub kind: ServerKind,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Kept only when the user asks to remember it (in the app's private settings).
    pub password: String,
    pub remember_password: bool,
    /// SMB: the share. WebDAV: the base path on the server (e.g. `/remote.php/dav/files/me`).
    pub share: String,
    /// The folder to open first.
    pub start_path: String,
    /// TLS (FTPS, WebDAV over https): accept a certificate the system doesn't trust (a NAS's own).
    #[serde(default)]
    pub accept_invalid_certs: bool,
}

impl Server {
    pub fn new(id: ServerId, kind: ServerKind) -> Self {
        Server {
            id,
            name: String::new(),
            kind,
            host: String::new(),
            port: kind.default_port(),
            user: String::new(),
            password: String::new(),
            remember_password: true,
            share: String::new(),
            start_path: "/".to_owned(),
            accept_invalid_certs: false,
        }
    }

    pub fn display_name(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.clone();
        }
        if self.kind == ServerKind::Smb && !self.share.is_empty() {
            return format!("{} ({})", self.share, self.host);
        }
        self.host.clone()
    }

    /// `sftp://user@host:port` (and the share or base path), for the address bar.
    pub fn url_base(&self) -> String {
        let user = if self.user.is_empty() { String::new() } else { format!("{}@", self.user) };
        let port = if self.port == self.kind.default_port() { String::new() } else { format!(":{}", self.port) };
        let share = match self.kind {
            ServerKind::Smb | ServerKind::WebDav | ServerKind::WebDavs if !self.share.is_empty() => {
                format!("/{}", self.share.trim_matches('/'))
            }
            _ => String::new(),
        };
        format!("{}://{user}{}{port}{share}", self.kind.scheme(), self.host)
    }

    /// The location of the folder to open first.
    pub fn start(&self) -> super::Location {
        let path = if self.start_path.trim().is_empty() { "/".to_owned() } else { self.start_path.clone() };
        let path = if path.starts_with('/') { path } else { format!("/{path}") };
        super::Location::Remote { server: self.id, path }
    }
}

/// A parsed address (`sftp://user@host:22/path`).
#[derive(Debug, PartialEq, Eq)]
pub struct Url {
    pub kind: ServerKind,
    pub user: String,
    pub host: String,
    pub port: u16,
    pub path: String,
}

pub fn parse_url(text: &str) -> Option<Url> {
    let (scheme, rest) = text.trim().split_once("://")?;
    let kind = ServerKind::from_scheme(scheme)?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_owned()),
        None => (rest, "/".to_owned()),
    };
    let (user, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (u.split(':').next().unwrap_or_default().to_owned(), h),
        None => (String::new(), authority),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.contains(']') || h.ends_with(']') => match p.parse() {
            Ok(port) => (h.to_owned(), port),
            Err(_) => (hostport.to_owned(), kind.default_port()),
        },
        _ => (hostport.to_owned(), kind.default_port()),
    };
    if host.is_empty() {
        return None;
    }
    Some(Url { kind, user, host, port, path: percent_decode(&path) })
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~!$&'()*+,;=:@".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The servers the connections are made to (the settings' list; the interface keeps it current).
static SERVERS: RwLock<Vec<Server>> = RwLock::new(Vec::new());
/// Passwords typed this session for servers that don't remember theirs.
static SESSION_PASSWORDS: Mutex<Option<HashMap<ServerId, String>>> = Mutex::new(None);
/// Open connections.
static CONNECTIONS: Mutex<Option<HashMap<ServerId, Arc<dyn Fs>>>> = Mutex::new(None);

pub fn set_servers(servers: &[Server]) {
    let mut current = SERVERS.write().unwrap_or_else(PoisonError::into_inner);
    // A changed server's connection is dropped, so the next use reconnects with the new details.
    let mut conns = CONNECTIONS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(conns) = conns.as_mut() {
        conns.retain(|id, _| {
            let old = current.iter().find(|s| s.id == *id);
            let new = servers.iter().find(|s| s.id == *id);
            old.is_some() && old == new
        });
    }
    *current = servers.to_vec();
}

pub fn server(id: ServerId) -> Option<Server> {
    SERVERS.read().unwrap_or_else(PoisonError::into_inner).iter().find(|s| s.id == id).cloned()
}

pub fn set_session_password(id: ServerId, password: String) {
    SESSION_PASSWORDS.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert_with(HashMap::new).insert(id, password);
    disconnect(id);
}

/// Whether connecting needs a password the app doesn't have.
pub fn needs_password(id: ServerId) -> bool {
    let Some(s) = server(id) else { return false };
    if s.remember_password || s.user.is_empty() {
        return false;
    }
    !SESSION_PASSWORDS.lock().unwrap_or_else(PoisonError::into_inner).as_ref().is_some_and(|m| m.contains_key(&id))
}

pub fn disconnect(id: ServerId) {
    if let Some(conns) = CONNECTIONS.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        conns.remove(&id);
    }
}

pub fn is_connected(id: ServerId) -> bool {
    CONNECTIONS.lock().unwrap_or_else(PoisonError::into_inner).as_ref().is_some_and(|c| c.contains_key(&id))
}

/// The open connection to a server, connecting first if needed (blocks).
pub fn connection(id: ServerId) -> io::Result<Arc<dyn Fs>> {
    if let Some(fs) = CONNECTIONS.lock().unwrap_or_else(PoisonError::into_inner).as_ref().and_then(|c| c.get(&id)) {
        return Ok(fs.clone());
    }
    let mut server = server(id).ok_or_else(|| super::other("unknown server"))?;
    if !server.remember_password {
        if let Some(p) = SESSION_PASSWORDS.lock().unwrap_or_else(PoisonError::into_inner).as_ref().and_then(|m| m.get(&id)) {
            server.password = p.clone();
        }
    }
    let fs = connect(&server)?;
    CONNECTIONS.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert_with(HashMap::new).insert(id, fs.clone());
    Ok(fs)
}

#[cfg(feature = "network")]
fn connect(server: &Server) -> io::Result<Arc<dyn Fs>> {
    super::net::connect(server)
}

#[cfg(not(feature = "network"))]
fn connect(_server: &Server) -> io::Result<Arc<dyn Fs>> {
    Err(super::other("this build has no network support"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let u = parse_url("sftp://ana@nas.local:2222/home/ana").unwrap_or_else(|| unreachable!());
        assert_eq!(u, Url { kind: ServerKind::Sftp, user: "ana".into(), host: "nas.local".into(), port: 2222, path: "/home/ana".into() });
        let u = parse_url("smb://192.168.1.5/Public%20Docs").unwrap_or_else(|| unreachable!());
        assert_eq!(u.port, 445);
        assert_eq!(u.path, "/Public Docs");
        assert!(parse_url("/storage/emulated/0").is_none());
        assert!(parse_url("gopher://x").is_none());
    }

    #[test]
    fn percent() {
        assert_eq!(percent_encode_path("/a b/ñ"), "/a%20b/%C3%B1");
        assert_eq!(percent_decode("/a%20b/%C3%B1"), "/a b/ñ");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
