use crate::testing::{PASSWORD, TestServer, USER};
use crate::*;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tephra_model::{VpnPeer, VpnProfile};
use tephra_transport::wireguard::{WgConfig, WgTunnel, generate_keypair};
use tephra_transport::{Direct, Transport, TransportError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn any_key() -> Arc<dyn HostKeyVerifier> {
    Arc::new(AcceptAnyHostKey)
}

fn options(server: &TestServer) -> ConnectOptions {
    ConnectOptions::new("127.0.0.1", server.addr.port(), USER)
}

async fn login(server: &TestServer) -> SshClient {
    SshClient::connect(&Direct::default(), options(server).with(Credential::Password(PASSWORD.into())), any_key(), None)
        .await
        .expect("anmelden")
}

/// Merkt sich vorgelegte Hostschlüssel und entscheidet fest.
struct Recorder {
    decision: HostKeyDecision,
    seen: Mutex<Vec<HostKeyInfo>>,
}

#[async_trait]
impl HostKeyVerifier for Recorder {
    async fn verify(&self, key: &HostKeyInfo) -> HostKeyDecision {
        self.seen.lock().expect("lock").push(key.clone());
        self.decision
    }
}

#[tokio::test]
async fn passwort_anmeldung_und_befehl() {
    let server = TestServer::start().await;
    let client = login(&server).await;
    let out = client.exec("echo hallo welt", ExecOptions::default()).await.expect("exec");
    assert_eq!(out.stdout_lossy(), "hallo welt\n");
    assert!(out.success());
    assert!(client.description().contains("tester@127.0.0.1"));
    client.disconnect().await;
}

#[tokio::test]
async fn falsches_passwort() {
    let server = TestServer::start().await;
    let result = SshClient::connect(
        &Direct::default(),
        options(&server).with(Credential::Password("falsch".into())),
        any_key(),
        None,
    )
    .await;
    assert!(matches!(result, Err(SshError::AuthFailed { .. })), "unerwartet: {:?}", result.err());

    let result = SshClient::connect(&Direct::default(), options(&server), any_key(), None).await;
    assert!(matches!(result, Err(SshError::NoCredentials(_))));
}

#[tokio::test]
async fn schluessel_anmeldung_mit_rueckfall_auf_zweite_anmeldeart() {
    let server = TestServer::start().await;
    let key = Credential::PrivateKey { pem: server.client_key_pem.clone(), passphrase: None, certificate: None };
    let client = SshClient::connect(&Direct::default(), options(&server).with(key), any_key(), None)
        .await
        .expect("anmelden mit schlüssel");
    assert_eq!(client.exec("whoami", ExecOptions::default()).await.expect("exec").stdout_lossy(), "tester\n");

    // Ein fremder Schlüssel wird abgelehnt, das Passwort danach akzeptiert.
    let (foreign, _) = keys::generate_ed25519("fremd").expect("schlüssel");
    let client = SshClient::connect(
        &Direct::default(),
        options(&server)
            .with(Credential::PrivateKey { pem: foreign, passphrase: None, certificate: None })
            .with(Credential::Password(PASSWORD.into())),
        any_key(),
        None,
    )
    .await
    .expect("anmelden mit rückfall");
    assert!(!client.is_closed());
}

#[tokio::test]
async fn hostschluessel_wird_vorgelegt_und_ablehnung_bricht_ab() {
    let server = TestServer::start().await;
    let recorder = Arc::new(Recorder { decision: HostKeyDecision::Reject, seen: Mutex::new(Vec::new()) });
    let result = SshClient::connect(
        &Direct::default(),
        options(&server).with(Credential::Password(PASSWORD.into())),
        recorder.clone(),
        None,
    )
    .await;
    match result {
        Err(SshError::HostKeyRejected { fingerprint, .. }) => assert_eq!(fingerprint, server.host_key_fingerprint),
        other => panic!("unerwartet: {:?}", other.err()),
    }
    let seen = recorder.seen.lock().expect("lock");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].algorithm, "ssh-ed25519");
    assert_eq!(seen[0].port, server.addr.port());
    assert!(seen[0].public_key.starts_with("ssh-ed25519 "));
}

#[tokio::test]
async fn fehlercode_fehlerausgabe_eingabe() {
    let server = TestServer::start().await;
    let client = login(&server).await;

    let out = client.exec("fail", ExecOptions::default()).await.expect("exec");
    assert_eq!(out.exit_status, Some(3));
    assert_eq!(out.stderr_lossy(), "kaputt\n");
    assert!(out.stdout.is_empty());
    assert!(!out.success());

    let out = client.exec("gibtsnicht", ExecOptions::default()).await.expect("exec");
    assert_eq!(out.exit_status, Some(127));

    let out = client
        .exec("cat", ExecOptions { stdin: Some(b"eingabe".to_vec()), ..ExecOptions::default() })
        .await
        .expect("exec");
    assert_eq!(out.stdout, b"eingabe");
}

#[tokio::test]
async fn zeitlimit_und_ausgabegrenze() {
    let server = TestServer::start().await;
    let client = login(&server).await;

    let out = client
        .exec("sleep 5000", ExecOptions { timeout: Some(Duration::from_millis(200)), ..ExecOptions::default() })
        .await
        .expect("exec");
    assert!(out.timed_out);
    assert_eq!(out.exit_status, None);

    let out =
        client.exec("big 100000", ExecOptions { max_output: 1000, ..ExecOptions::default() }).await.expect("exec");
    assert_eq!(out.stdout.len(), 1000);
    assert!(out.truncated);
    assert_eq!(out.exit_status, Some(0));

    // Die Verbindung bleibt nach einem abgebrochenen Befehl benutzbar.
    assert!(client.exec("echo ok", ExecOptions::default()).await.expect("exec").success());
}

#[tokio::test]
async fn mehrere_befehle_gleichzeitig() {
    let server = TestServer::start().await;
    let client = Arc::new(login(&server).await);
    let mut tasks = Vec::new();
    for i in 0..8 {
        let client = client.clone();
        tasks.push(tokio::spawn(async move {
            client.exec(&format!("echo nummer {i}"), ExecOptions::default()).await.expect("exec").stdout_lossy()
        }));
    }
    for (i, task) in tasks.into_iter().enumerate() {
        assert_eq!(task.await.expect("task"), format!("nummer {i}\n"));
    }
}

async fn read_until(shell: &mut Shell, needle: &str) -> String {
    let mut seen = String::new();
    while !seen.contains(needle) {
        match tokio::time::timeout(Duration::from_secs(5), shell.events.recv()).await {
            Ok(Some(ShellEvent::Output(data))) => seen.push_str(&String::from_utf8_lossy(&data)),
            other => panic!("Warten auf „{needle}“ endete mit {other:?}; bisher: {seen:?}"),
        }
    }
    seen
}

#[tokio::test]
async fn interaktive_shell() {
    let server = TestServer::start().await;
    let client = login(&server).await;
    let mut shell = client.shell(PtyOptions::default(), None).await.expect("shell");

    read_until(&mut shell, "bereit> ").await;
    shell.input.send(b"ls -la\r").await.expect("senden");
    read_until(&mut shell, "ls -la").await;
    shell.input.resize(120, 40).await.expect("größe");
    read_until(&mut shell, "[120x40]").await;

    shell.input.send(b"exit\r").await.expect("senden");
    let mut exit = None;
    loop {
        match tokio::time::timeout(Duration::from_secs(5), shell.events.recv()).await.expect("kein Hänger") {
            Some(ShellEvent::Exit(code)) => exit = code,
            Some(ShellEvent::Closed) | None => break,
            Some(ShellEvent::Output(_)) => {}
        }
    }
    assert_eq!(exit, Some(0));
}

#[tokio::test]
async fn weiterleitung_und_jump_host() {
    // Ziel, das nur „hinter“ dem ersten Server erreichbar sein soll.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let echo_port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf).await.expect("read");
        stream.write_all(&buf).await.expect("write");
    });

    let first = TestServer::start().await;
    let second = TestServer::start().await;
    let jump = Arc::new(login(&first).await);

    // Einfache Weiterleitung.
    let mut stream = jump.open_direct_tcpip("127.0.0.1", echo_port).await.expect("weiterleitung");
    stream.write_all(b"ping").await.expect("write");
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await.expect("read");
    assert_eq!(&buf, b"ping");

    // Geschlossener Port wird als Verbindungsfehler gemeldet.
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind").local_addr().expect("addr").port();
    let err = jump.open_direct_tcpip("127.0.0.1", closed).await.err().expect("fehler");
    assert!(matches!(err, SshError::Transport(TransportError::Connect { .. })), "unerwartet: {err}");

    // Zweite SSH-Verbindung durch die erste hindurch.
    let transport = JumpTransport::new(jump.clone());
    let inner =
        SshClient::connect(&transport, options(&second).with(Credential::Password(PASSWORD.into())), any_key(), None)
            .await
            .expect("anmelden über jump-host");
    drop(jump); // der Strom hält die Sprungverbindung selbst am Leben
    assert_eq!(inner.exec("echo durch", ExecOptions::default()).await.expect("exec").stdout_lossy(), "durch\n");
    assert!(inner.description().contains("über tester@127.0.0.1"));
}

/// Der Kern des Vorhabens: SSH zu einem Server, der ausschließlich durch den
/// WireGuard-Tunnel erreichbar ist – ohne Netzwerkadapter, ohne besondere Rechte.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ssh_durch_den_wireguard_tunnel() {
    let (client_private, client_public) = generate_keypair().expect("keys");
    let (server_private, server_public) = generate_keypair().expect("keys");
    let port = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").port();

    let mut gateway_config = WgConfig::from_profile(&VpnProfile {
        private_key: server_private.into(),
        addresses: vec!["10.77.0.1/24".into()],
        peers: vec![VpnPeer {
            public_key: client_public,
            endpoint: "127.0.0.1:9".into(),
            allowed_ips: vec!["10.77.0.2/32".into()],
            ..VpnPeer::default()
        }],
        ..VpnProfile::default()
    })
    .expect("config");
    gateway_config.listen_port = Some(port);
    let gateway = WgTunnel::start(gateway_config).await.expect("gateway");

    // Der SSH-Server hat keinen eigenen Port; er ist nur im Tunnelnetz erreichbar.
    let server = TestServer::without_listener();
    let fingerprint = server.host_key_fingerprint.clone();
    let mut listener = gateway.listen(22).expect("listen");
    tokio::spawn(async move {
        while let Ok(stream) = listener.accept().await {
            server.serve(stream);
        }
    });

    let tunnel = WgTunnel::start(
        WgConfig::from_profile(&VpnProfile {
            private_key: client_private.into(),
            addresses: vec!["10.77.0.2/32".into()],
            peers: vec![VpnPeer {
                public_key: server_public,
                endpoint: format!("127.0.0.1:{port}"),
                allowed_ips: vec!["10.77.0.0/24".into()],
                persistent_keepalive: Some(25),
                ..VpnPeer::default()
            }],
            ..VpnProfile::default()
        })
        .expect("config"),
    )
    .await
    .expect("tunnel");
    assert!(tunnel.describe().starts_with("WireGuard"));

    let recorder = Arc::new(Recorder { decision: HostKeyDecision::Accept, seen: Mutex::new(Vec::new()) });
    let client = SshClient::connect(
        &tunnel,
        ConnectOptions::new("10.77.0.1", 22, USER).with(Credential::Password(PASSWORD.into())),
        recorder.clone(),
        None,
    )
    .await
    .expect("anmelden durch den tunnel");
    assert_eq!(recorder.seen.lock().expect("lock")[0].fingerprint, fingerprint);

    let out = client.exec("echo durch den tunnel", ExecOptions::default()).await.expect("exec");
    assert_eq!(out.stdout_lossy(), "durch den tunnel\n");

    // Größere Ausgabe und interaktive Shell über dieselbe Verbindung.
    let out = client
        .exec("big 2000000", ExecOptions { max_output: 4_000_000, ..ExecOptions::default() })
        .await
        .expect("exec");
    assert_eq!(out.stdout.len(), 2_000_000);
    let mut shell = client.shell(PtyOptions::default(), None).await.expect("shell");
    read_until(&mut shell, "bereit> ").await;
    shell.input.send(b"uptime\r").await.expect("senden");
    read_until(&mut shell, "uptime").await;

    let status = tunnel.status();
    assert!(status[0].last_handshake.is_some());
    assert!(status[0].rx_bytes > 2_000_000);
}

/// Echo-Dienst, der nur über den SSH-Server erreichbar sein soll.
async fn echo_service() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 64];
                while let Ok(n) = stream.read(&mut buf).await {
                    if n == 0 || stream.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

#[tokio::test]
async fn lokale_portweiterleitung() {
    let server = TestServer::start().await;
    let client = Arc::new(login(&server).await);
    let target = echo_service().await;

    let forward = forward::local(client.clone(), "127.0.0.1", 0, "127.0.0.1", target).await.expect("weiterleitung");
    assert!(forward.local_addr.ip().is_loopback());
    for round in 0..3 {
        let mut stream = tokio::net::TcpStream::connect(forward.local_addr).await.expect("verbinden");
        let message = format!("runde {round}");
        stream.write_all(message.as_bytes()).await.expect("write");
        let mut buf = vec![0u8; message.len()];
        stream.read_exact(&mut buf).await.expect("read");
        assert_eq!(buf, message.as_bytes());
    }

    // Nach dem Verwerfen ist der Port wieder geschlossen.
    let addr = forward.local_addr;
    drop(forward);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
}

#[tokio::test]
async fn socks_proxy_ueber_die_verbindung() {
    let server = TestServer::start().await;
    let client = Arc::new(login(&server).await);
    let target = echo_service().await;
    let proxy = forward::dynamic(client.clone(), "127.0.0.1", 0).await.expect("proxy");

    // Der eigene SOCKS5-Client aus der Transportschicht dient als Gegenprobe.
    let socks = tephra_transport::proxy::Socks5 {
        via: Arc::new(Direct::default()),
        host: "127.0.0.1".into(),
        port: proxy.local_addr.port(),
        auth: None,
    };
    let mut stream = socks.connect_tcp("127.0.0.1", target).await.expect("über socks verbinden");
    stream.write_all(b"durch socks").await.expect("write");
    let mut buf = [0u8; 11];
    stream.read_exact(&mut buf).await.expect("read");
    assert_eq!(&buf, b"durch socks");

    // Ein nicht erreichbares Ziel wird dem Client als abgelehnt gemeldet.
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind").local_addr().expect("addr").port();
    assert!(matches!(socks.connect_tcp("127.0.0.1", closed).await, Err(TransportError::Proxy(_))));
}
