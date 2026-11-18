//! Transportschicht: Wie wird ein Host erreicht?
//!
//! SSH selbst arbeitet auf einem beliebigen Byte-Strom. Dieses Crate liefert
//! solche Ströme – direkt, über einen Proxy oder durch einen WireGuard-Tunnel,
//! der vollständig innerhalb des Prozesses läuft. Transporte lassen sich
//! verketten (z. B. Tunnel → Proxy → Ziel).

pub mod proxy;
pub mod wireguard;

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

/// Byte-Strom zu einem Ziel.
pub trait AsyncStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> AsyncStream for T {}

pub type BoxedStream = Box<dyn AsyncStream>;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("Zeitüberschreitung beim Verbinden mit {0}")]
    Timeout(String),
    #[error("Name {0} konnte nicht aufgelöst werden")]
    Resolve(String),
    #[error("Verbindung zu {target} abgelehnt oder nicht möglich: {reason}")]
    Connect { target: String, reason: String },
    #[error("VPN-Profil ist ungültig: {0}")]
    Config(String),
    #[error("Kein WireGuard-Handshake mit {0} – Schlüssel, Endpoint und Erreichbarkeit prüfen")]
    NoHandshake(String),
    #[error("Kein Peer des VPN-Profils ist für {0} zuständig (AllowedIPs)")]
    NoRoute(String),
    #[error("Tunnel wurde beendet")]
    TunnelClosed,
    #[error("Proxy-Fehler: {0}")]
    Proxy(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Stellt TCP-Verbindungen zu einem Ziel her.
#[async_trait]
pub trait Transport: Send + Sync {
    async fn connect_tcp(&self, host: &str, port: u16) -> Result<BoxedStream, TransportError>;

    /// Kurzbeschreibung für Statusanzeigen und Protokolle.
    fn describe(&self) -> String;
}

pub type SharedTransport = Arc<dyn Transport>;

/// Zeit, die ein Verbindungsaufbau höchstens dauern darf.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Direkte Verbindung über das Netzwerk des Geräts.
#[derive(Debug, Clone)]
pub struct Direct {
    pub timeout: Duration,
}

impl Default for Direct {
    fn default() -> Self {
        Self { timeout: DEFAULT_CONNECT_TIMEOUT }
    }
}

#[async_trait]
impl Transport for Direct {
    async fn connect_tcp(&self, host: &str, port: u16) -> Result<BoxedStream, TransportError> {
        let target = format!("{host}:{port}");
        let stream = tokio::time::timeout(self.timeout, TcpStream::connect((host, port)))
            .await
            .map_err(|_| TransportError::Timeout(target.clone()))?
            .map_err(|e| TransportError::Connect { target, reason: e.to_string() })?;
        // Interaktive Sitzungen sollen Tastendrücke sofort senden.
        let _ = stream.set_nodelay(true);
        Ok(Box::new(stream))
    }

    fn describe(&self) -> String {
        "direkt".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn direkt_verbinden() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.expect("accept");
            let mut buf = [0u8; 4];
            s.read_exact(&mut buf).await.expect("read");
            s.write_all(&buf).await.expect("write");
        });
        let mut stream = Direct::default().connect_tcp("127.0.0.1", port).await.expect("connect");
        stream.write_all(b"ping").await.expect("write");
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf).await.expect("read");
        assert_eq!(&buf, b"ping");
    }

    #[tokio::test]
    async fn direkt_abgelehnt() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        let err = Direct::default().connect_tcp("127.0.0.1", port).await.err().expect("fehler");
        assert!(matches!(err, TransportError::Connect { .. }));
    }
}
