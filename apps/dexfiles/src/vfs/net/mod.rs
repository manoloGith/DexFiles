//! Network file systems: FTP/FTPS, SFTP, SMB 2/3 and WebDAV, all in Rust (no C libraries but
//! `ring`'s, which builds with the NDK).

mod ftp;
mod sftp;
mod smb;
mod webdav;

use std::io;
use std::sync::{Arc, OnceLock};

use super::Fs;
use super::servers::{Server, ServerKind};

pub fn connect(server: &Server) -> io::Result<Arc<dyn Fs>> {
    log::info!(
        "dexfiles: connecting to {} ({:?})",
        server.host,
        server.kind
    );
    Ok(match server.kind {
        ServerKind::Ftp | ServerKind::Ftps => Arc::new(ftp::FtpFs::connect(server)?),
        ServerKind::Sftp => Arc::new(sftp::SftpFs::connect(server)?),
        ServerKind::Smb => Arc::new(smb::SmbFs::connect(server)?),
        ServerKind::WebDav | ServerKind::WebDavs => Arc::new(webdav::WebDavFs::connect(server)?),
    })
}

/// The runtime the async clients (SFTP) run on.
pub(crate) fn runtime() -> io::Result<&'static tokio::runtime::Runtime> {
    static RT: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("dexfiles-net")
            .enable_all()
            .build()
            .ok()
    })
    .as_ref()
    .ok_or_else(|| super::other("could not start the network runtime"))
}

/// rustls' client configuration: Mozilla's roots (Android's own store isn't reachable from
/// native code without the Java side), or no verification at all when the user said so.
pub(crate) fn tls_config(accept_invalid: bool) -> io::Result<Arc<rustls::ClientConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(super::other)?;
    let config = if accept_invalid {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAll(provider)))
            .with_no_client_auth()
    } else {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        builder.with_root_certificates(roots).with_no_client_auth()
    };
    Ok(Arc::new(config))
}

#[derive(Debug)]
struct AcceptAll(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AcceptAll {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Seconds since the epoch of a `SystemTime`.
pub(crate) fn unix(t: std::time::SystemTime) -> Option<i64> {
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
}

/// `parent/name` for `/`-separated paths.
pub(crate) fn join(parent: &str, name: &str) -> String {
    format!("{}/{}", parent.trim_end_matches('/'), name)
}
