//! Verbindungen über SOCKS5- und HTTP-Proxys.
//!
//! Ein Proxy-Transport setzt auf einem anderen Transport auf: Der Weg zum
//! Proxy selbst kann also wiederum durch einen Tunnel führen.

use crate::{BoxedStream, SharedTransport, Transport, TransportError};
use async_trait::async_trait;
use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Default)]
pub struct ProxyAuth {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for ProxyAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyAuth").field("username", &self.username).field("password", &"…").finish()
    }
}

/// Lehnt Hostnamen ab, mit denen sich zusätzliche Zeilen oder Felder in die
/// Anfrage an den Proxy einschleusen ließen.
fn check_host(host: &str) -> Result<(), TransportError> {
    if host.is_empty() || host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(proxy_err("Hostname enthält unzulässige Zeichen"));
    }
    Ok(())
}

/// Begrenzt die Aushandlung mit dem Proxy; ein stummer Proxy würde den
/// Verbindungsaufbau sonst für immer blockieren.
async fn limited<T>(
    target: String,
    work: impl std::future::Future<Output = Result<T, TransportError>>,
) -> Result<T, TransportError> {
    tokio::time::timeout(crate::DEFAULT_CONNECT_TIMEOUT, work).await.map_err(|_| TransportError::Timeout(target))?
}

/// SOCKS5 nach RFC 1928 (CONNECT), optional mit Benutzername/Passwort (RFC 1929).
/// Namen werden vom Proxy aufgelöst.
pub struct Socks5 {
    pub via: SharedTransport,
    pub host: String,
    pub port: u16,
    pub auth: Option<ProxyAuth>,
}

fn proxy_err(msg: impl Into<String>) -> TransportError {
    TransportError::Proxy(msg.into())
}

#[async_trait]
impl Transport for Socks5 {
    async fn connect_tcp(&self, host: &str, port: u16) -> Result<BoxedStream, TransportError> {
        check_host(host)?;
        limited(format!("{host}:{port} über SOCKS5-Proxy"), self.negotiate(host, port)).await
    }

    fn describe(&self) -> String {
        format!("{} → SOCKS5 {}:{}", self.via.describe(), self.host, self.port)
    }
}

impl Socks5 {
    async fn negotiate(&self, host: &str, port: u16) -> Result<BoxedStream, TransportError> {
        let mut s = self.via.connect_tcp(&self.host, self.port).await?;

        let methods: &[u8] = if self.auth.is_some() { &[0x00, 0x02] } else { &[0x00] };
        let mut hello = vec![0x05, methods.len() as u8];
        hello.extend_from_slice(methods);
        s.write_all(&hello).await?;
        let mut choice = [0u8; 2];
        s.read_exact(&mut choice).await?;
        if choice[0] != 0x05 {
            return Err(proxy_err("Gegenstelle spricht kein SOCKS5"));
        }
        match choice[1] {
            0x00 => {}
            0x02 => {
                let auth = self.auth.as_ref().ok_or_else(|| proxy_err("Proxy verlangt Anmeldung"))?;
                if auth.username.len() > 255 || auth.password.len() > 255 {
                    return Err(proxy_err("Benutzername oder Passwort zu lang"));
                }
                let mut msg = vec![0x01, auth.username.len() as u8];
                msg.extend_from_slice(auth.username.as_bytes());
                msg.push(auth.password.len() as u8);
                msg.extend_from_slice(auth.password.as_bytes());
                s.write_all(&msg).await?;
                let mut reply = [0u8; 2];
                s.read_exact(&mut reply).await?;
                if reply[1] != 0x00 {
                    return Err(proxy_err("Anmeldung am Proxy abgelehnt"));
                }
            }
            _ => return Err(proxy_err("Proxy akzeptiert keine der angebotenen Anmeldearten")),
        }

        let mut request = vec![0x05, 0x01, 0x00];
        match host.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V4(ip)) => {
                request.push(0x01);
                request.extend_from_slice(&ip.octets());
            }
            Ok(std::net::IpAddr::V6(ip)) => {
                request.push(0x04);
                request.extend_from_slice(&ip.octets());
            }
            Err(_) => {
                if host.len() > 255 {
                    return Err(proxy_err("Hostname zu lang"));
                }
                request.push(0x03);
                request.push(host.len() as u8);
                request.extend_from_slice(host.as_bytes());
            }
        }
        request.extend_from_slice(&port.to_be_bytes());
        s.write_all(&request).await?;

        let mut head = [0u8; 4];
        s.read_exact(&mut head).await?;
        if head[1] != 0x00 {
            let reason = match head[1] {
                0x01 => "allgemeiner Fehler",
                0x02 => "durch Regelwerk verboten",
                0x03 => "Netz nicht erreichbar",
                0x04 => "Host nicht erreichbar",
                0x05 => "Verbindung abgelehnt",
                0x06 => "Zeitüberschreitung",
                0x07 => "Befehl nicht unterstützt",
                0x08 => "Adresstyp nicht unterstützt",
                _ => "unbekannter Fehler",
            };
            return Err(proxy_err(format!("Proxy konnte {host}:{port} nicht erreichen: {reason}")));
        }
        // Gebundene Adresse überlesen.
        let skip = match head[3] {
            0x01 => 4,
            0x04 => 16,
            0x03 => {
                let mut len = [0u8; 1];
                s.read_exact(&mut len).await?;
                len[0] as usize
            }
            _ => return Err(proxy_err("ungültige Antwort des Proxys")),
        };
        let mut rest = vec![0u8; skip + 2];
        s.read_exact(&mut rest).await?;
        Ok(s)
    }
}

/// HTTP-Proxy mit `CONNECT`.
pub struct HttpConnect {
    pub via: SharedTransport,
    pub host: String,
    pub port: u16,
    pub auth: Option<ProxyAuth>,
}

#[async_trait]
impl Transport for HttpConnect {
    async fn connect_tcp(&self, host: &str, port: u16) -> Result<BoxedStream, TransportError> {
        check_host(host)?;
        limited(format!("{host}:{port} über HTTP-Proxy"), self.negotiate(host, port)).await
    }

    fn describe(&self) -> String {
        format!("{} → HTTP-Proxy {}:{}", self.via.describe(), self.host, self.port)
    }
}

impl HttpConnect {
    async fn negotiate(&self, host: &str, port: u16) -> Result<BoxedStream, TransportError> {
        let mut s = self.via.connect_tcp(&self.host, self.port).await?;
        let authority = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
        let mut request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
        if let Some(auth) = &self.auth {
            let token =
                base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", auth.username, auth.password));
            request.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
        }
        request.push_str("\r\n");
        s.write_all(request.as_bytes()).await?;

        // Antwortkopf byteweise lesen, damit nichts vom anschließenden
        // Nutzdatenstrom verschluckt wird.
        let mut head = Vec::with_capacity(256);
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if head.len() > 16 * 1024 {
                return Err(proxy_err("Antwortkopf des Proxys ist zu groß"));
            }
            s.read_exact(&mut byte).await?;
            head.push(byte[0]);
        }
        let text = String::from_utf8_lossy(&head);
        let status_line = text.lines().next().unwrap_or_default();
        let code = status_line.split_whitespace().nth(1).unwrap_or_default();
        // Jede 2xx-Antwort bedeutet: Der Tunnel steht.
        if !(code.len() == 3 && code.starts_with('2')) {
            return Err(proxy_err(format!("Proxy antwortete mit „{status_line}“")));
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Direct;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    /// Minimaler SOCKS5-Proxy für genau eine Verbindung.
    async fn socks_server(listener: TcpListener, expect_auth: bool) {
        let (mut s, _) = listener.accept().await.expect("accept");
        let mut head = [0u8; 2];
        s.read_exact(&mut head).await.expect("hello");
        let mut methods = vec![0u8; head[1] as usize];
        s.read_exact(&mut methods).await.expect("methods");
        if expect_auth {
            s.write_all(&[5, 2]).await.expect("choice");
            let mut v = [0u8; 2];
            s.read_exact(&mut v).await.expect("auth");
            let mut user = vec![0u8; v[1] as usize];
            s.read_exact(&mut user).await.expect("user");
            let mut l = [0u8; 1];
            s.read_exact(&mut l).await.expect("len");
            let mut pass = vec![0u8; l[0] as usize];
            s.read_exact(&mut pass).await.expect("pass");
            let ok = user == b"u" && pass == b"p";
            s.write_all(&[1, if ok { 0 } else { 1 }]).await.expect("reply");
            if !ok {
                return;
            }
        } else {
            s.write_all(&[5, 0]).await.expect("choice");
        }
        let mut req = [0u8; 4];
        s.read_exact(&mut req).await.expect("req");
        assert_eq!(req[3], 3, "Name muss dem Proxy überlassen werden");
        let mut l = [0u8; 1];
        s.read_exact(&mut l).await.expect("len");
        let mut name = vec![0u8; l[0] as usize + 2];
        s.read_exact(&mut name).await.expect("name");
        s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await.expect("ok");
        let mut buf = [0u8; 5];
        s.read_exact(&mut buf).await.expect("data");
        s.write_all(&buf).await.expect("echo");
    }

    #[tokio::test]
    async fn socks5_mit_und_ohne_anmeldung() {
        for auth in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let port = listener.local_addr().expect("addr").port();
            tokio::spawn(socks_server(listener, auth));
            let proxy = Socks5 {
                via: Arc::new(Direct::default()),
                host: "127.0.0.1".into(),
                port,
                auth: auth.then(|| ProxyAuth { username: "u".into(), password: "p".into() }),
            };
            let mut s = proxy.connect_tcp("ziel.intern", 22).await.expect("connect");
            s.write_all(b"hallo").await.expect("write");
            let mut buf = [0u8; 5];
            s.read_exact(&mut buf).await.expect("read");
            assert_eq!(&buf, b"hallo");
        }
    }

    #[tokio::test]
    async fn socks5_falsches_passwort() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(socks_server(listener, true));
        let proxy = Socks5 {
            via: Arc::new(Direct::default()),
            host: "127.0.0.1".into(),
            port,
            auth: Some(ProxyAuth { username: "u".into(), password: "falsch".into() }),
        };
        assert!(matches!(proxy.connect_tcp("ziel", 22).await, Err(TransportError::Proxy(_))));
    }

    #[tokio::test]
    async fn http_connect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            for status in ["200 Connection established", "407 Proxy Authentication Required"] {
                let (mut s, _) = listener.accept().await.expect("accept");
                let mut head = Vec::new();
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    s.read_exact(&mut b).await.expect("read");
                    head.push(b[0]);
                }
                assert!(String::from_utf8_lossy(&head).starts_with("CONNECT ziel.intern:22 HTTP/1.1\r\n"));
                s.write_all(format!("HTTP/1.1 {status}\r\n\r\n").as_bytes()).await.expect("write");
                if status.starts_with("200") {
                    let mut buf = [0u8; 2];
                    s.read_exact(&mut buf).await.expect("data");
                    s.write_all(&buf).await.expect("echo");
                }
            }
        });
        let proxy = HttpConnect { via: Arc::new(Direct::default()), host: "127.0.0.1".into(), port, auth: None };
        let mut s = proxy.connect_tcp("ziel.intern", 22).await.expect("connect");
        s.write_all(b"ok").await.expect("write");
        let mut buf = [0u8; 2];
        s.read_exact(&mut buf).await.expect("read");
        assert_eq!(&buf, b"ok");
        assert!(matches!(proxy.connect_tcp("ziel.intern", 22).await, Err(TransportError::Proxy(_))));
    }

    #[tokio::test]
    async fn hostname_kann_keine_kopfzeilen_einschleusen() {
        let via: crate::SharedTransport = Arc::new(Direct::default());
        let http = HttpConnect { via: via.clone(), host: "127.0.0.1".into(), port: 9, auth: None };
        let socks = Socks5 { via, host: "127.0.0.1".into(), port: 9, auth: None };
        for evil in ["ziel\r\nX-Injected: 1", "ziel intern", "", "ziel\tx"] {
            assert!(matches!(http.connect_tcp(evil, 22).await, Err(TransportError::Proxy(_))), "{evil:?}");
            assert!(matches!(socks.connect_tcp(evil, 22).await, Err(TransportError::Proxy(_))), "{evil:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn stummer_proxy_blockiert_nicht_ewig() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            // Verbindung annehmen und nie antworten.
            let (_stream, _) = listener.accept().await.expect("accept");
            std::future::pending::<()>().await;
        });
        let proxy = Socks5 { via: Arc::new(Direct::default()), host: "127.0.0.1".into(), port, auth: None };
        assert!(matches!(proxy.connect_tcp("ziel", 22).await, Err(TransportError::Timeout(_))));
    }
}
