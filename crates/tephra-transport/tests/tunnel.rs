//! Ende-zu-Ende-Tests des WireGuard-Tunnels: Zwei Tunnel-Enden im selben
//! Prozess sprechen über UDP auf der Loopback-Schnittstelle miteinander.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use tephra_model::{VpnPeer, VpnProfile};
use tephra_transport::wireguard::{WgConfig, WgTunnel, generate_keypair};
use tephra_transport::{Transport, TransportError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const CLIENT_IP: &str = "10.99.0.2";
const SERVER_IP: &str = "10.99.0.1";

fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").port()
}

struct Pair {
    client: WgTunnel,
    server: WgTunnel,
}

/// Baut Client und Server auf. `break_key` gibt dem Client einen falschen
/// Serverschlüssel, sodass kein Handshake zustande kommt.
async fn pair(break_key: bool) -> Pair {
    pair_with_loss(break_key, None).await
}

/// Wie [`pair`], zusätzlich verwerfen beide Seiten jedes `loss`-te Paket.
async fn pair_with_loss(break_key: bool, loss: Option<u32>) -> Pair {
    let (client_private, client_public) = generate_keypair().expect("keys");
    let (server_private, server_public) = generate_keypair().expect("keys");
    let (_, wrong_public) = generate_keypair().expect("keys");
    let server_port = free_udp_port();

    let server_profile = VpnProfile {
        label: "server".into(),
        private_key: server_private.into(),
        addresses: vec![format!("{SERVER_IP}/24")],
        peers: vec![VpnPeer {
            public_key: client_public,
            // Der Server lernt die echte Adresse des Clients aus dessen erstem Paket.
            endpoint: "127.0.0.1:9".into(),
            allowed_ips: vec![format!("{CLIENT_IP}/32")],
            ..VpnPeer::default()
        }],
        ..VpnProfile::default()
    };
    let mut server_config = WgConfig::from_profile(&server_profile).expect("server config");
    server_config.listen_port = Some(server_port);
    server_config.simulate_loss = loss;

    let client_profile = VpnProfile {
        label: "client".into(),
        private_key: client_private.into(),
        addresses: vec![format!("{CLIENT_IP}/32")],
        peers: vec![VpnPeer {
            public_key: if break_key { wrong_public } else { server_public },
            endpoint: format!("127.0.0.1:{server_port}"),
            allowed_ips: vec!["10.99.0.0/24".into()],
            persistent_keepalive: Some(25),
            ..VpnPeer::default()
        }],
        ..VpnProfile::default()
    };
    let mut client_config = WgConfig::from_profile(&client_profile).expect("client config");
    client_config.simulate_loss = loss;

    let server = WgTunnel::start(server_config).await.expect("server start");
    let client =
        WgTunnel::start(client_config).await.expect("client start").with_connect_timeout(Duration::from_secs(5));
    Pair { client, server }
}

fn server_addr(port: u16) -> SocketAddr {
    SocketAddr::new(SERVER_IP.parse::<IpAddr>().expect("ip"), port)
}

fn checksum(offset: usize, data: &[u8]) -> u64 {
    data.iter()
        .enumerate()
        .fold(0u64, |sum, (i, byte)| sum.wrapping_add(u64::from(*byte) * ((offset + i) as u64 % 251 + 1)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn echo_durch_den_tunnel() {
    let Pair { client, server } = pair(false).await;
    let mut listener = server.listen(2222).expect("listen");
    tokio::spawn(async move {
        loop {
            let Ok(mut stream) = listener.accept().await else { break };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16 * 1024];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if stream.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                let _ = stream.shutdown().await;
            });
        }
    });

    // Über die allgemeine Transport-Schnittstelle, so wie SSH sie benutzt.
    let mut stream = client.connect_tcp(SERVER_IP, 2222).await.expect("connect");
    stream.write_all(b"hallo tunnel").await.expect("write");
    let mut buf = [0u8; 12];
    stream.read_exact(&mut buf).await.expect("read");
    assert_eq!(&buf, b"hallo tunnel");

    // Zwei gleichzeitige Verbindungen durch denselben Tunnel.
    let mut second = client.connect(server_addr(2222)).await.expect("connect 2");
    second.write_all(b"zweite").await.expect("write");
    stream.write_all(b"erste!").await.expect("write");
    let mut a = [0u8; 6];
    let mut b = [0u8; 6];
    second.read_exact(&mut a).await.expect("read");
    stream.read_exact(&mut b).await.expect("read");
    assert_eq!((&a, &b), (b"zweite", b"erste!"));

    // Ordentliches Schließen kommt als Dateiende an.
    second.shutdown().await.expect("shutdown");
    let mut rest = Vec::new();
    second.read_to_end(&mut rest).await.expect("eof");
    assert!(rest.is_empty());

    let status = client.status();
    assert_eq!(status.len(), 1);
    assert!(status[0].last_handshake.is_some());
    assert!(status[0].tx_bytes > 0 && status[0].rx_bytes > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn grosse_uebertragung_ist_vollstaendig_und_zuegig() {
    let mbit = transfer(16 * 1024 * 1024, None).await;
    assert!(mbit > 20.0, "Durchsatz von {mbit:.1} Mbit/s ist für Dateiübertragungen zu gering");
}

/// Zwei Prozent Paketverlust in beide Richtungen dürfen eine Übertragung
/// verlangsamen, aber weder verfälschen noch zum Stillstand bringen.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uebertragung_uebersteht_paketverlust() {
    let mbit = tokio::time::timeout(Duration::from_secs(120), transfer(2 * 1024 * 1024, Some(50)))
        .await
        .expect("Übertragung ist bei Paketverlust stehen geblieben");
    assert!(mbit > 0.5, "Durchsatz von {mbit:.2} Mbit/s bei 2 % Verlust ist unbrauchbar");
}

/// Überträgt `size` Byte vom Client zum Server, prüft die Unversehrtheit und
/// gibt den Durchsatz in Mbit/s zurück.
async fn transfer(size: usize, loss: Option<u32>) -> f64 {
    #[allow(non_snake_case)]
    let SIZE = size;
    let Pair { client, server } = pair_with_loss(false, loss).await;
    let mut listener = server.listen(9000).expect("listen");

    // Der Server empfängt alles und meldet eine Prüfsumme zurück.
    let receiver = tokio::spawn(async move {
        let mut stream = listener.accept().await.expect("accept");
        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0usize;
        let mut sum = 0u64;
        while total < SIZE {
            let n = stream.read(&mut buf).await.expect("read");
            assert!(n > 0, "Verbindung endete nach {total} Byte");
            sum = sum.wrapping_add(checksum(total, &buf[..n]));
            total += n;
        }
        stream.write_all(&sum.to_le_bytes()).await.expect("write");
        // Offen halten, bis der Client gelesen hat.
        let _ = stream.read(&mut buf).await;
    });

    let data: Vec<u8> = (0..SIZE).map(|i| (i.wrapping_mul(31) ^ (i >> 7)) as u8).collect();
    let expected = checksum(0, &data);

    let mut stream = client.connect(server_addr(9000)).await.expect("connect");
    let started = Instant::now();
    stream.write_all(&data).await.expect("write");
    let mut reply = [0u8; 8];
    stream.read_exact(&mut reply).await.expect("read");
    let elapsed = started.elapsed();
    assert_eq!(u64::from_le_bytes(reply), expected, "Daten kamen verändert an");

    let mbit = (SIZE as f64 * 8.0 / 1_000_000.0) / elapsed.as_secs_f64();
    println!("Durchsatz (Verlust: {loss:?}): {mbit:.1} Mbit/s ({elapsed:?} für {} MiB)", SIZE / 1024 / 1024);
    drop(stream);
    let _ = receiver.await;
    mbit
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn geschlossener_port_wird_als_abgelehnt_gemeldet() {
    let Pair { client, server: _server } = pair(false).await;
    client.wait_ready(SERVER_IP.parse().expect("ip"), Duration::from_secs(5)).await.expect("handshake");
    let err = client.connect(server_addr(4444)).await.err().expect("fehler");
    assert!(matches!(err, TransportError::Connect { .. }), "unerwartet: {err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn falscher_schluessel_ergibt_verstaendlichen_fehler() {
    let Pair { client, server: _server } = pair(true).await;
    let err = client.wait_ready(SERVER_IP.parse().expect("ip"), Duration::from_secs(2)).await.expect_err("fehler");
    assert!(matches!(err, TransportError::NoHandshake(_)), "unerwartet: {err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ziel_ausserhalb_der_allowed_ips() {
    let Pair { client, server: _server } = pair(false).await;
    let err = client.connect("192.168.1.1:22".parse().expect("addr")).await.err().expect("fehler");
    assert!(matches!(err, TransportError::NoRoute(_)), "unerwartet: {err}");
    let err = client.connect_tcp("192.168.1.1", 22).await.err().expect("fehler");
    assert!(matches!(err, TransportError::NoRoute(_)), "unerwartet: {err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abgebrochene_verbindung_meldet_reset() {
    let Pair { client, server } = pair(false).await;
    let mut listener = server.listen(7000).expect("listen");
    let (connected, client_connected) = tokio::sync::oneshot::channel::<()>();
    let accepted = tokio::spawn(async move {
        let stream = listener.accept().await.expect("accept");
        // Erst abbrechen, wenn die Gegenseite die Verbindung hat; sonst sähe sie den
        // Abbruch je nach Zeitpunkt schon beim Verbindungsaufbau.
        let _ = client_connected.await;
        drop(stream); // ohne ordentliches Schließen
    });
    let mut stream = client.connect(server_addr(7000)).await.expect("connect");
    connected.send(()).expect("Gegenseite wartet");
    accepted.await.expect("task");
    let mut buf = [0u8; 1];
    let result = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await.expect("kein Hänger");
    assert!(result.is_err(), "Abbruch der Gegenseite muss als Fehler ankommen");
}

/// Namen hinter dem Tunnel werden nie über die Namensauflösung des Geräts
/// aufgelöst – auch nicht, wenn das Profil keinen DNS-Server nennt. Sonst
/// erführe ein fremder DNS-Server interne Hostnamen.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hostname_ohne_dns_im_profil_wird_nicht_ueber_das_geraet_aufgeloest() {
    let Pair { client, server: _server } = pair(false).await;
    // „localhost" ließe sich über das Gerät jederzeit auflösen.
    let err = client.connect_tcp("localhost", 22).await.err().expect("fehler");
    assert!(matches!(err, TransportError::Resolve(_)), "unerwartet: {err}");
    let err = client.resolve("intern.example").await.expect_err("fehler");
    assert!(matches!(err, TransportError::Resolve(_)), "unerwartet: {err}");
}
