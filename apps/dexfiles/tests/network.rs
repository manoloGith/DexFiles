//! Round trips against real servers, run by hand (they need one):
//!
//! ```text
//! DEXFILES_TEST_SERVER=ftp://user:pass@127.0.0.1:2121/ cargo test -p dexfiles --test network -- --ignored
//! ```
//!
//! The URL's scheme picks the protocol (ftp, ftps, sftp, smb, dav, davs); for SMB the first path
//! part is the share, for WebDAV the path is the base path. Everything happens in a new folder
//! `dexfiles-test-<time>` that is removed at the end.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)] // a test: failing loudly is the point

use std::io::Cursor;

use dexfiles::vfs::servers::{self, Server, ServerId, ServerKind};
use dexfiles::vfs::{self, Kind, Location};

fn server_from_env() -> Option<Server> {
    let url = std::env::var("DEXFILES_TEST_SERVER").ok()?;
    let (scheme, rest) = url.split_once("://")?;
    let kind = ServerKind::from_scheme(scheme)?;
    let (auth, hostpath) = rest.rsplit_once('@').unwrap_or(("", rest));
    let (user, password) = auth.split_once(':').unwrap_or((auth, ""));
    let (hostport, path) = hostpath.split_once('/').unwrap_or((hostpath, ""));
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h.to_owned(), p.parse().ok()?),
        None => (hostport.to_owned(), kind.default_port()),
    };
    let mut s = Server::new(ServerId(1), kind);
    s.host = host;
    s.port = port;
    s.user = user.to_owned();
    s.password = password.to_owned();
    s.share = path.trim_matches('/').to_owned();
    s.accept_invalid_certs = true;
    Some(s)
}

#[test]
#[ignore = "needs a server: DEXFILES_TEST_SERVER"]
fn round_trip() {
    let Some(server) = server_from_env() else {
        return;
    };
    servers::set_servers(std::slice::from_ref(&server));
    let root = Location::Remote {
        server: server.id,
        path: "/".into(),
    };
    let fs = vfs::fs_for(&root).unwrap_or_else(|e| panic!("connect: {e}"));
    let dir = root.join(&format!("dexfiles-test-{}", std::process::id()));
    fs.mkdir(&dir.backend_path())
        .unwrap_or_else(|e| panic!("mkdir: {e}"));

    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let file = dir.join("datos ñ.bin");
    fs.write_from(
        &file.backend_path(),
        &mut Cursor::new(&data),
        data.len() as u64,
        &mut |_| true,
    )
    .unwrap_or_else(|e| panic!("write: {e}"));

    let listed = vfs::list(&dir).unwrap_or_else(|e| panic!("list: {e}"));
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].name, "datos ñ.bin");
    assert_eq!(listed[0].kind, Kind::File);
    assert_eq!(listed[0].size, data.len() as u64);

    let mut back = Vec::new();
    fs.read_into(&file.backend_path(), &mut back, &mut |_| true)
        .unwrap_or_else(|e| panic!("read: {e}"));
    assert_eq!(back, data);
    assert_eq!(
        fs.read_head(&file.backend_path(), 1000).unwrap_or_default(),
        data[..1000].to_vec()
    );

    let renamed = dir.join("renamed.bin");
    fs.rename(&file.backend_path(), &renamed.backend_path())
        .unwrap_or_else(|e| panic!("rename: {e}"));
    let sub = dir.join("sub");
    fs.mkdir(&sub.backend_path())
        .unwrap_or_else(|e| panic!("mkdir sub: {e}"));
    let names: Vec<String> = vfs::list(&dir)
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert!(
        names.contains(&"renamed.bin".to_owned()) && names.contains(&"sub".to_owned()),
        "{names:?}"
    );
    assert_eq!(vfs::stat(&sub).map(|e| e.kind).ok(), Some(Kind::Dir));

    fs.remove_file(&renamed.backend_path())
        .unwrap_or_else(|e| panic!("rm: {e}"));
    fs.remove_dir(&sub.backend_path())
        .unwrap_or_else(|e| panic!("rmdir sub: {e}"));
    fs.remove_dir(&dir.backend_path())
        .unwrap_or_else(|e| panic!("rmdir: {e}"));
    assert!(
        !vfs::list(&root)
            .unwrap_or_default()
            .iter()
            .any(|e| e.location == dir)
    );
}
