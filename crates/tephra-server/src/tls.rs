//! TLS ohne Handarbeit: Der Server erzeugt beim ersten Start selbst ein Zertifikat und spricht auf demselben Port
//! HTTPS und – solange noch Geräte es brauchen – einfaches HTTP. Die Geräte merken sich den Fingerabdruck des
//! Zertifikats beim ersten Kontakt, wie SSH einen Hostschlüssel; eine Zertifizierungsstelle braucht es nicht.
//!
//! Ob eine Verbindung TLS spricht, zeigt ihr erstes Byte: Ein TLS-Handshake beginnt immer mit `0x16`, eine
//! HTTP-Anfrage mit einem Buchstaben.

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

/// Wie der Server Verbindungen annimmt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsMode {
    /// Nur einfaches HTTP (etwa hinter einem Reverse-Proxy, der TLS übernimmt).
    Off,
    /// HTTPS und HTTP auf demselben Port; Geräte steigen beim nächsten Abgleich von selbst auf HTTPS um.
    #[default]
    Auto,
    /// Nur HTTPS; einfaches HTTP wird abgewiesen.
    Only,
}

/// Zertifikat und Schlüssel des Servers samt Fingerabdruck.
pub struct Identity {
    pub acceptor: TlsAcceptor,
    /// SHA-256 des Zertifikats, als Hex mit Doppelpunkten – so zeigen ihn auch die Geräte.
    pub fingerprint: String,
}

/// Formatiert einen SHA-256-Fingerabdruck wie `AB:CD:…`.
pub fn format_fingerprint(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

/// Lädt Zertifikat und Schlüssel aus `dir` (`cert.pem`, `key.pem`) oder erzeugt sie dort beim ersten Start.
/// Eigene Dateien (`cert`, `key`) haben Vorrang, etwa ein Zertifikat einer echten Zertifizierungsstelle.
pub fn identity(dir: &Path, own: Option<(PathBuf, PathBuf)>) -> io::Result<Identity> {
    let (cert_path, key_path) = match own {
        Some(paths) => paths,
        None => {
            let paths = (dir.join("cert.pem"), dir.join("key.pem"));
            if !paths.0.exists() || !paths.1.exists() {
                generate(dir, &paths.0, &paths.1)?;
            }
            paths
        }
    };
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(&cert_path)
        .map_err(|e| io::Error::other(format!("{}: {e}", cert_path.display())))?
        .collect::<Result<_, _>>()
        .map_err(|e| io::Error::other(format!("{}: {e}", cert_path.display())))?;
    let first = certs.first().ok_or_else(|| io::Error::other(format!("{}: kein Zertifikat", cert_path.display())))?;
    let fingerprint = format_fingerprint(&Sha256::digest(first.as_ref()));
    let key = PrivateKeyDer::from_pem_file(&key_path)
        .map_err(|e| io::Error::other(format!("{}: {e}", key_path.display())))?;
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(io::Error::other)?;
    Ok(Identity { acceptor: TlsAcceptor::from(Arc::new(config)), fingerprint })
}

/// Erzeugt ein selbst signiertes Zertifikat. Der Name darin spielt keine Rolle: Die Geräte prüfen den
/// Fingerabdruck, nicht den Namen.
fn generate(dir: &Path, cert_path: &Path, key_path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let generated = rcgen::generate_simple_self_signed(vec!["tephra-sync".to_string()]).map_err(io::Error::other)?;
    write_private(key_path, generated.signing_key.serialize_pem().as_bytes())?;
    std::fs::write(cert_path, generated.cert.pem())?;
    tracing::info!(cert = %cert_path.display(), "Neues TLS-Zertifikat erzeugt");
    Ok(())
}

/// Schreibt den Schlüssel nur für den eigenen Benutzer lesbar.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        file.write_all(bytes)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)
    }
}

/// Eine angenommene Verbindung: verschlüsselt oder nicht.
pub enum Io {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for Io {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Io::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            Io::Tls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Io {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Io::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            Io::Tls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Io::Plain(stream) => Pin::new(stream).poll_flush(cx),
            Io::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Io::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            Io::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
        }
    }
}

/// So lange hat eine neue Verbindung Zeit, ihr erstes Byte zu schicken und den Handshake abzuschließen.
const HANDSHAKE: Duration = Duration::from_secs(10);

/// Nimmt Verbindungen an und reicht sie nach erkanntem Protokoll (und abgeschlossenem Handshake) weiter. Jede
/// Verbindung wird in einer eigenen Aufgabe vorbereitet, damit eine, die nichts schickt, keine andere aufhält.
pub struct MixedListener {
    ready: mpsc::Receiver<(Io, SocketAddr)>,
    local: SocketAddr,
}

impl MixedListener {
    pub fn new(listener: TcpListener, acceptor: Option<TlsAcceptor>, plain: bool) -> io::Result<Self> {
        let local = listener.local_addr()?;
        let (sender, ready) = mpsc::channel(64);
        tokio::spawn(async move {
            loop {
                let (stream, addr) = match listener.accept().await {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        tracing::warn!(%error, "Verbindung ließ sich nicht annehmen");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let (to, acceptor) = (sender.clone(), acceptor.clone());
                tokio::spawn(async move {
                    let prepared = tokio::time::timeout(HANDSHAKE, prepare(stream, acceptor, plain)).await;
                    if let Ok(Some(io)) = prepared {
                        let _ = to.send((io, addr)).await;
                    }
                });
                if sender.is_closed() {
                    break;
                }
            }
        });
        Ok(Self { ready, local })
    }
}

async fn prepare(stream: TcpStream, acceptor: Option<TlsAcceptor>, plain: bool) -> Option<Io> {
    let mut first = [0u8; 1];
    let read = stream.peek(&mut first).await.ok()?;
    if read == 0 {
        return None;
    }
    match (first[0] == 0x16, acceptor) {
        (true, Some(acceptor)) => acceptor.accept(stream).await.ok().map(|tls| Io::Tls(Box::new(tls))),
        (false, _) if plain => Some(Io::Plain(stream)),
        _ => None,
    }
}

impl axum::serve::Listener for MixedListener {
    type Io = Io;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(accepted) => accepted,
            // Die Annahme ist beendet; der Server wartet dann nur noch auf sein Ende.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local)
    }
}
