//! WebDAV (Nextcloud, ownCloud, NAS, Apache mod_dav…) over ureq: PROPFIND, GET, PUT, MKCOL,
//! DELETE and MOVE, with Basic authentication.

use std::io::{self, Read, Write};
use std::time::Duration;

use ureq::http;

use crate::vfs::servers::{Server, ServerKind, percent_decode, percent_encode_path};
use crate::vfs::{self, Fs, Kind, Progress, RawEntry};

pub struct WebDavFs {
    agent: ureq::Agent,
    /// `https://host:port`
    origin: String,
    /// The base path on the server, without a trailing slash (`/remote.php/dav/files/me`).
    base: String,
    auth: Option<String>,
}

fn err(e: ureq::Error) -> io::Error {
    match e {
        ureq::Error::Io(e) => e,
        other => vfs::other(other),
    }
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/><d:getlastmodified/><d:creationdate/></d:prop></d:propfind>"#;

impl WebDavFs {
    pub fn connect(server: &Server) -> io::Result<Self> {
        let tls = ureq::tls::TlsConfig::builder()
            .disable_verification(server.accept_invalid_certs)
            .build();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60 * 60)))
            .timeout_connect(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .allow_non_standard_methods(true)
            .tls_config(tls)
            .build()
            .into();
        let scheme = if server.kind == ServerKind::WebDavs {
            "https"
        } else {
            "http"
        };
        let default = server.kind.default_port();
        let port = if server.port == default {
            String::new()
        } else {
            format!(":{}", server.port)
        };
        let base = format!("/{}", server.share.trim_matches('/'));
        let fs = WebDavFs {
            agent,
            origin: format!("{scheme}://{}{port}", server.host),
            base: base.trim_end_matches('/').to_owned(),
            auth: (!server.user.is_empty()).then(|| {
                format!(
                    "Basic {}",
                    base64(format!("{}:{}", server.user, server.password).as_bytes())
                )
            }),
        };
        // Check the address and the credentials now, so a mistake shows when connecting.
        fs.propfind("/", "0")?;
        Ok(fs)
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}{}",
            self.origin,
            percent_encode_path(&format!(
                "{}{}",
                self.base,
                if path.starts_with('/') {
                    path.to_owned()
                } else {
                    format!("/{path}")
                }
            ))
        )
    }

    fn request(&self, method: &str, path: &str) -> http::request::Builder {
        let mut b = http::Request::builder().method(method).uri(self.url(path));
        if let Some(a) = &self.auth {
            b = b.header("Authorization", a);
        }
        b
    }

    fn check(status: http::StatusCode) -> io::Result<()> {
        match status.as_u16() {
            200..=299 => Ok(()),
            401 | 403 => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("HTTP {status}"),
            )),
            404 => Err(io::Error::new(io::ErrorKind::NotFound, "not found")),
            405 | 409 | 412 => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("HTTP {status}"),
            )),
            _ => Err(vfs::other(format!("HTTP {status}"))),
        }
    }

    fn simple(&self, req: http::Request<()>) -> io::Result<()> {
        let resp = self.agent.run(req).map_err(err)?;
        Self::check(resp.status())
    }

    fn propfind(&self, path: &str, depth: &str) -> io::Result<Vec<(String, RawEntry)>> {
        let req = self
            .request("PROPFIND", path)
            .header("Depth", depth)
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(PROPFIND_BODY)
            .map_err(vfs::other)?;
        let resp = self.agent.run(req).map_err(err)?;
        Self::check(resp.status())?;
        let text = resp.into_body().read_to_string().map_err(err)?;
        Ok(parse_multistatus(&text))
    }

    /// The path below the base of an `href` (absolute URL or absolute path), decoded.
    fn relative(&self, href: &str) -> String {
        let path = match href.find("://") {
            Some(i) => href[i + 3..]
                .find('/')
                .map(|j| &href[i + 3 + j..])
                .unwrap_or("/"),
            None => href,
        };
        let decoded = percent_decode(path);
        let rel = decoded.strip_prefix(&self.base).unwrap_or(&decoded);
        let rel = rel.trim_end_matches('/');
        if rel.is_empty() {
            "/".to_owned()
        } else {
            rel.to_owned()
        }
    }
}

/// `(href, entry)` for each `<response>` of a PROPFIND answer (the name is the href's last part).
fn parse_multistatus(xml: &str) -> Vec<(String, RawEntry)> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = Vec::new();
    let mut current: Option<(String, RawEntry)> = None;
    let mut field = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let local = e.local_name();
                let name = AsRef::<str>::as_ref(&local).to_ascii_lowercase();
                match name.as_str() {
                    "response" => {
                        current = Some((
                            String::new(),
                            RawEntry {
                                name: String::new(),
                                kind: Kind::File,
                                size: 0,
                                modified: None,
                                created: None,
                                symlink: false,
                                readonly: false,
                                hidden_attr: false,
                            },
                        ));
                    }
                    "collection" => {
                        if let Some((_, e)) = current.as_mut() {
                            e.kind = Kind::Dir;
                        }
                    }
                    _ => {}
                }
                field = name;
            }
            Ok(Event::Empty(e)) => {
                if AsRef::<str>::as_ref(&e.local_name()).eq_ignore_ascii_case("collection")
                    && let Some((_, e)) = current.as_mut()
                {
                    e.kind = Kind::Dir;
                }
            }
            Ok(Event::Text(t)) => {
                let text = t.xml10_content().into_owned();
                let text = text.trim();
                if let Some((href, e)) = current.as_mut() {
                    match field.as_str() {
                        "href" => href.push_str(text),
                        "getcontentlength" => e.size = text.parse().unwrap_or(0),
                        "getlastmodified" => {
                            e.modified = chrono::DateTime::parse_from_rfc2822(text)
                                .ok()
                                .map(|d| d.timestamp())
                        }
                        "creationdate" => {
                            e.created = chrono::DateTime::parse_from_rfc3339(text)
                                .ok()
                                .map(|d| d.timestamp())
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                if AsRef::<str>::as_ref(&e.local_name()).eq_ignore_ascii_case("response")
                    && let Some((href, mut entry)) = current.take()
                {
                    let decoded = percent_decode(href.trim_end_matches('/'));
                    entry.name = decoded.rsplit('/').next().unwrap_or_default().to_owned();
                    out.push((href, entry));
                }
                field.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

impl Fs for WebDavFs {
    fn list(&self, path: &str) -> io::Result<Vec<RawEntry>> {
        let here = self.relative(path);
        let entries = self.propfind(path, "1")?;
        Ok(entries
            .into_iter()
            .filter(|(href, _)| self.relative(href) != here)
            .map(|(_, e)| e)
            .filter(|e| !e.name.is_empty())
            .collect())
    }

    fn stat(&self, path: &str) -> io::Result<RawEntry> {
        let mut entries = self.propfind(path, "0")?;
        let (_, mut e) = entries
            .pop()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not found"))?;
        if e.name.is_empty() {
            e.name = "/".into();
        }
        Ok(e)
    }

    fn read_into(
        &self,
        path: &str,
        out: &mut dyn Write,
        progress: Progress<'_>,
    ) -> io::Result<u64> {
        let req = self.request("GET", path).body(()).map_err(vfs::other)?;
        let resp = self.agent.run(req).map_err(err)?;
        Self::check(resp.status())?;
        let mut body = resp.into_body();
        let mut reader = body.with_config().limit(u64::MAX).reader();
        vfs::copy_stream(&mut reader, out, progress)
    }

    fn write_from(
        &self,
        path: &str,
        input: &mut dyn Read,
        size: u64,
        progress: Progress<'_>,
    ) -> io::Result<u64> {
        let mut counted = Counting {
            inner: input,
            n: 0,
            progress,
            cancelled: false,
        };
        let req = self
            .request("PUT", path)
            .header("Content-Length", size.to_string())
            .header("Content-Type", "application/octet-stream")
            .body(ureq::SendBody::from_reader(&mut counted))
            .map_err(vfs::other)?;
        let result = self.agent.run(req);
        if counted.cancelled {
            return Err(vfs::cancelled());
        }
        let resp = result.map_err(err)?;
        Self::check(resp.status())?;
        Ok(counted.n)
    }

    fn mkdir(&self, path: &str) -> io::Result<()> {
        self.simple(self.request("MKCOL", path).body(()).map_err(vfs::other)?)
    }

    fn remove_file(&self, path: &str) -> io::Result<()> {
        self.simple(self.request("DELETE", path).body(()).map_err(vfs::other)?)
    }

    fn remove_dir(&self, path: &str) -> io::Result<()> {
        let p = format!("{}/", path.trim_end_matches('/'));
        self.simple(self.request("DELETE", &p).body(()).map_err(vfs::other)?)
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let req = self
            .request("MOVE", from)
            .header("Destination", self.url(to))
            .header("Overwrite", "F")
            .body(())
            .map_err(vfs::other)?;
        self.simple(req)
    }
}

/// Counts what is read, reports progress, and stops (EOF) when cancelled.
struct Counting<'a, 'b> {
    inner: &'a mut dyn Read,
    n: u64,
    progress: Progress<'b>,
    cancelled: bool,
}

impl Read for Counting<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancelled {
            return Err(vfs::cancelled());
        }
        let n = self.inner.read(buf)?;
        self.n += n as u64;
        if !(self.progress)(self.n) {
            self.cancelled = true;
            return Err(vfs::cancelled());
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multistatus() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
 <d:response><d:href>/dav/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
 <d:response><d:href>/dav/My%20Docs/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype><d:getlastmodified>Tue, 10 Nov 2026 10:00:00 GMT</d:getlastmodified></d:prop></d:propstat></d:response>
 <d:response><d:href>/dav/a.txt</d:href><d:propstat><d:prop><d:resourcetype/><d:getcontentlength>12</d:getcontentlength></d:prop></d:propstat></d:response>
</d:multistatus>"#;
        let r = parse_multistatus(xml);
        assert_eq!(r.len(), 3);
        assert_eq!(r[1].1.name, "My Docs");
        assert_eq!(r[1].1.kind, Kind::Dir);
        assert!(r[1].1.modified.is_some());
        assert_eq!(r[2].1.name, "a.txt");
        assert_eq!(r[2].1.size, 12);
        assert_eq!(r[2].1.kind, Kind::File);
    }

    #[test]
    fn basic_auth() {
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64(b"a"), "YQ==");
    }
}
