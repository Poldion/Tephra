//! Portweiterleitungen über eine bestehende SSH-Verbindung.
//!
//! * [`local`] – ein lokaler Port führt zu einem festen Ziel aus Sicht des Servers
//! * [`dynamic`] – ein lokaler SOCKS5-Proxy, dessen Ziele der Server erreicht
//!
//! Beide lauschen standardmäßig nur auf der Loopback-Schnittstelle. Über die
//! Weiterleitung erreicht jedes Programm auf diesem Rechner das Zielnetz;
//! an anderen Schnittstellen zu lauschen, öffnet es zusätzlich für das
//! lokale Netz.

use crate::{Result, SshClient, SshError};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// Laufende Weiterleitung. Sie endet, wenn dieses Objekt verworfen wird;
/// bereits offene Verbindungen laufen zu Ende.
pub struct Forward {
    /// Adresse, auf der gelauscht wird (nützlich bei Port 0).
    pub local_addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for Forward {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn bind(host: &str, port: u16) -> Result<(TcpListener, SocketAddr)> {
    let listener = TcpListener::bind((host, port))
        .await
        .map_err(|e| SshError::Protocol(format!("Port {port} auf {host} lässt sich nicht öffnen: {e}")))?;
    let addr = listener.local_addr().map_err(|e| SshError::Protocol(e.to_string()))?;
    Ok((listener, addr))
}

/// Leitet Verbindungen auf `bind_host:bind_port` zu `target_host:target_port`
/// weiter, aufgebaut vom SSH-Server aus.
pub async fn local(
    client: Arc<SshClient>,
    bind_host: &str,
    bind_port: u16,
    target_host: &str,
    target_port: u16,
) -> Result<Forward> {
    let (listener, local_addr) = bind(bind_host, bind_port).await?;
    let target_host = target_host.to_string();
    let task = tokio::spawn(async move {
        while let Ok((mut inbound, _)) = listener.accept().await {
            let (client, target_host) = (client.clone(), target_host.clone());
            tokio::spawn(async move {
                match client.open_direct_tcpip(&target_host, target_port).await {
                    Ok(mut outbound) => {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                    Err(error) => tracing::debug!(%error, "Weiterleitung konnte das Ziel nicht erreichen"),
                }
            });
        }
    });
    Ok(Forward { local_addr, task })
}

/// Startet einen SOCKS5-Proxy auf `bind_host:bind_port`. Jedes Ziel, das ein
/// Programm über ihn anfragt, wird vom SSH-Server aus erreicht; Namen löst
/// der Server auf.
pub async fn dynamic(client: Arc<SshClient>, bind_host: &str, bind_port: u16) -> Result<Forward> {
    let (listener, local_addr) = bind(bind_host, bind_port).await?;
    let task = tokio::spawn(async move {
        while let Ok((inbound, _)) = listener.accept().await {
            let client = client.clone();
            tokio::spawn(async move {
                if let Err(error) = serve_socks(inbound, &client).await {
                    tracing::debug!(%error, "SOCKS-Anfrage abgebrochen");
                }
            });
        }
    });
    Ok(Forward { local_addr, task })
}

/// Bedient eine SOCKS5-Verbindung (nur `CONNECT`, ohne Anmeldung).
async fn serve_socks(mut inbound: TcpStream, client: &SshClient) -> std::io::Result<()> {
    let bad = |message: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, message.to_string());

    let mut head = [0u8; 2];
    inbound.read_exact(&mut head).await?;
    if head[0] != 0x05 {
        return Err(bad("kein SOCKS5"));
    }
    let mut methods = vec![0u8; usize::from(head[1])];
    inbound.read_exact(&mut methods).await?;
    if !methods.contains(&0x00) {
        inbound.write_all(&[0x05, 0xff]).await?;
        return Err(bad("Client verlangt Anmeldung"));
    }
    inbound.write_all(&[0x05, 0x00]).await?;

    let mut request = [0u8; 4];
    inbound.read_exact(&mut request).await?;
    let host = match request[3] {
        0x01 => {
            let mut ip = [0u8; 4];
            inbound.read_exact(&mut ip).await?;
            std::net::Ipv4Addr::from(ip).to_string()
        }
        0x04 => {
            let mut ip = [0u8; 16];
            inbound.read_exact(&mut ip).await?;
            std::net::Ipv6Addr::from(ip).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            inbound.read_exact(&mut len).await?;
            let mut name = vec![0u8; usize::from(len[0])];
            inbound.read_exact(&mut name).await?;
            String::from_utf8(name).map_err(|_| bad("ungültiger Hostname"))?
        }
        _ => return Err(bad("unbekannter Adresstyp")),
    };
    let mut port = [0u8; 2];
    inbound.read_exact(&mut port).await?;
    let port = u16::from_be_bytes(port);

    // Antwort: Version, Ergebnis, reserviert, Adresstyp IPv4, 0.0.0.0:0
    let reply = |code: u8| [0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
    if request[1] != 0x01 {
        inbound.write_all(&reply(0x07)).await?;
        return Err(bad("nur CONNECT wird unterstützt"));
    }
    match client.open_direct_tcpip(&host, port).await {
        Ok(mut outbound) => {
            inbound.write_all(&reply(0x00)).await?;
            let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
            Ok(())
        }
        Err(_) => {
            // 0x05: Verbindung abgelehnt
            inbound.write_all(&reply(0x05)).await?;
            Ok(())
        }
    }
}
