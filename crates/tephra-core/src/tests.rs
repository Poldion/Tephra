use crate::*;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tephra_model::{AuthMethod, Group, Host, Identity, ItemId, KnownHost, SshKey, VpnPeer, VpnProfile};
use tephra_ssh::testing::{PASSWORD, TestServer, USER};
use tephra_ssh::{ExecOptions, Prompt, SshError};
use tephra_transport::wireguard::{WgConfig, WgTunnel, generate_keypair};

/// Oberfläche für Tests: beantwortet Rückfragen fest und merkt sie sich.
struct TestUi {
    accept_keys: bool,
    password: Option<String>,
    questions: StdMutex<Vec<HostKeyQuestion>>,
}

impl TestUi {
    fn new(accept_keys: bool) -> Arc<Self> {
        Arc::new(Self { accept_keys, password: None, questions: StdMutex::new(Vec::new()) })
    }

    fn questions(&self) -> Vec<HostKeyQuestion> {
        self.questions.lock().expect("lock").clone()
    }
}

#[async_trait]
impl ConnectUi for TestUi {
    async fn confirm_host_key(&self, question: &HostKeyQuestion) -> bool {
        self.questions.lock().expect("lock").push(question.clone());
        self.accept_keys
    }

    async fn ask_password(&self, _prompt: &str) -> Option<String> {
        self.password.clone()
    }

    async fn answer_prompts(&self, _name: &str, _instructions: &str, _prompts: &[Prompt]) -> Option<Vec<String>> {
        None
    }
}

struct Fixture {
    core: Core,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("tmp");
    let (core, _) =
        Core::create(&dir.path().join("test.tephra"), "passwort", KdfParams::INSECURE_FOR_TESTS).expect("tresor");
    Fixture { core, _dir: dir }
}

fn password_identity(core: &Core) -> ItemId {
    core.with_vault(|v| {
        v.insert(&Identity {
            label: "Tester".into(),
            username: USER.into(),
            auth: AuthMethod::Password { password: PASSWORD.into() },
        })
    })
    .expect("identität")
}

fn add_host(core: &Core, label: &str, port: u16, edit: impl FnOnce(&mut Host)) -> ItemId {
    let mut host = Host { label: label.into(), address: "127.0.0.1".into(), port, ..Host::default() };
    edit(&mut host);
    core.with_vault(|v| v.insert(&host)).expect("host")
}

async fn echo(client: &tephra_ssh::SshClient, text: &str) -> String {
    client.exec(&format!("echo {text}"), ExecOptions::default()).await.expect("exec").stdout_lossy()
}

#[tokio::test]
async fn erstes_verbinden_fragt_nach_dem_hostschluessel_und_merkt_ihn_sich() {
    let Fixture { core, .. } = fixture();
    let server = TestServer::start().await;
    let identity = password_identity(&core);
    let host = add_host(&core, "test", server.addr.port(), |h| h.identity_id = Some(identity));

    // Ohne Nutzer wird ein unbekannter Schlüssel abgelehnt.
    let err = core.connect(host, Arc::new(NonInteractive)).await.err().expect("fehler");
    assert!(matches!(err, CoreError::Ssh(SshError::HostKeyRejected { .. })), "unerwartet: {err}");

    // Der Nutzer bestätigt; der Schlüssel wird gespeichert.
    let ui = TestUi::new(true);
    let client = core.connect(host, ui.clone()).await.expect("verbinden");
    assert_eq!(echo(&client, "eins").await, "eins\n");
    let questions = ui.questions();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].state, HostKeyState::Unknown);
    assert_eq!(questions[0].key.fingerprint, server.host_key_fingerprint);
    let known = core.with_vault(|v| v.list::<KnownHost>()).expect("liste");
    assert_eq!(known.len(), 1);
    assert_eq!(known[0].1.fingerprint, server.host_key_fingerprint);

    // Danach geht es ohne Rückfrage.
    let client = core.connect(host, Arc::new(NonInteractive)).await.expect("verbinden");
    assert_eq!(echo(&client, "zwei").await, "zwei\n");
}

#[tokio::test]
async fn geaenderter_hostschluessel_wird_als_solcher_gemeldet() {
    let Fixture { core, .. } = fixture();
    let server = TestServer::start().await;
    let identity = password_identity(&core);
    let host = add_host(&core, "test", server.addr.port(), |h| h.identity_id = Some(identity));
    core.with_vault(|v| {
        v.insert(&KnownHost {
            host: "127.0.0.1".into(),
            port: server.addr.port(),
            key_type: "ssh-ed25519".into(),
            fingerprint: "SHA256:frueher-bekannter-schluessel".into(),
            ..KnownHost::default()
        })
    })
    .expect("bekannt");

    let ui = TestUi::new(false);
    let err = core.connect(host, ui.clone()).await.err().expect("fehler");
    assert!(matches!(err, CoreError::Ssh(SshError::HostKeyRejected { .. })), "unerwartet: {err}");
    assert_eq!(
        ui.questions()[0].state,
        HostKeyState::Changed { known_fingerprints: vec!["SHA256:frueher-bekannter-schluessel".into()] }
    );

    // Bestätigt der Nutzer, ersetzt der neue Schlüssel den alten.
    core.connect(host, TestUi::new(true)).await.expect("verbinden");
    let known = core.with_vault(|v| v.list::<KnownHost>()).expect("liste");
    assert_eq!(known.len(), 1);
    assert_eq!(known[0].1.fingerprint, server.host_key_fingerprint);
}

#[tokio::test]
async fn einstellungen_werden_von_gruppen_geerbt() {
    let Fixture { core, .. } = fixture();
    let server = TestServer::start().await;
    let identity = password_identity(&core);
    let outer = core
        .with_vault(|v| {
            v.insert(&Group { name: "Kunde".into(), default_identity_id: Some(identity), ..Group::default() })
        })
        .expect("gruppe");
    let inner = core
        .with_vault(|v| v.insert(&Group { name: "Web".into(), parent_id: Some(outer), ..Group::default() }))
        .expect("gruppe");
    let host = add_host(&core, "web1", server.addr.port(), |h| h.group_id = Some(inner));

    let resolved = core.resolve_host(host).expect("auflösen");
    assert_eq!(resolved.identity.expect("geerbt").username, USER);
    core.connect(host, TestUi::new(true)).await.expect("verbinden");

    // Ohne Identität gibt es eine verständliche Meldung statt eines Verbindungsversuchs.
    let bare = add_host(&core, "nackt", server.addr.port(), |_| {});
    let err = core.connect(bare, TestUi::new(true)).await.err().expect("fehler");
    assert!(matches!(err, CoreError::Config(_)), "unerwartet: {err}");
    assert!(matches!(core.resolve_host(ItemId::new()), Err(CoreError::NotFound(_))));
}

#[tokio::test]
async fn schluessel_und_abgefragtes_passwort() {
    let Fixture { core, .. } = fixture();
    let server = TestServer::start().await;
    let key = core
        .with_vault(|v| {
            v.insert(&SshKey {
                label: "k".into(),
                private_key: server.client_key_pem.clone().into(),
                ..SshKey::default()
            })
        })
        .expect("schlüssel");
    let with_key = core
        .with_vault(|v| {
            v.insert(&Identity {
                label: "Schlüssel".into(),
                username: USER.into(),
                auth: AuthMethod::Key { key_id: key },
            })
        })
        .expect("identität");
    let asking = core
        .with_vault(|v| v.insert(&Identity { label: "Fragt".into(), username: USER.into(), auth: AuthMethod::Ask }))
        .expect("identität");

    let by_key = add_host(&core, "per schlüssel", server.addr.port(), |h| h.identity_id = Some(with_key));
    let by_ask = add_host(&core, "per abfrage", server.addr.port(), |h| h.identity_id = Some(asking));

    core.connect(by_key, TestUi::new(true)).await.expect("verbinden per schlüssel");

    let ui =
        Arc::new(TestUi { accept_keys: true, password: Some(PASSWORD.into()), questions: StdMutex::new(Vec::new()) });
    core.connect(by_ask, ui).await.expect("verbinden per abfrage");

    // Ohne Antwort auf die Passwortfrage schlägt die Anmeldung fehl.
    let err = core.connect(by_ask, TestUi::new(true)).await.err().expect("fehler");
    assert!(matches!(err, CoreError::Ssh(SshError::AuthFailed { .. })), "unerwartet: {err}");
}

#[tokio::test]
async fn jump_hosts_aus_dem_tresor() {
    let Fixture { core, .. } = fixture();
    let (a, b, c) = (TestServer::start().await, TestServer::start().await, TestServer::start().await);
    let identity = password_identity(&core);
    let host_a = add_host(&core, "a", a.addr.port(), |h| h.identity_id = Some(identity));
    let host_b = add_host(&core, "b", b.addr.port(), |h| {
        h.identity_id = Some(identity);
        h.jump_host_ids = vec![host_a];
    });
    let host_c = add_host(&core, "c", c.addr.port(), |h| {
        h.identity_id = Some(identity);
        h.jump_host_ids = vec![host_b];
    });

    // c wird über a → b erreicht; alle drei Hostschlüssel werden vorgelegt.
    let ui = TestUi::new(true);
    let client = core.connect(host_c, ui.clone()).await.expect("verbinden");
    assert_eq!(echo(&client, "ziel").await, "ziel\n");
    let fingerprints: Vec<String> = ui.questions().into_iter().map(|q| q.key.fingerprint).collect();
    assert_eq!(
        fingerprints,
        vec![a.host_key_fingerprint.clone(), b.host_key_fingerprint.clone(), c.host_key_fingerprint.clone()]
    );

    // Ein Kreis wird erkannt, statt endlos zu verbinden.
    core.with_vault(|v| {
        let mut host = v.get::<Host>(host_a)?.expect("a");
        host.jump_host_ids = vec![host_c];
        v.update(host_a, &host)
    })
    .expect("ändern");
    let err = core.connect(host_c, TestUi::new(true)).await.err().expect("fehler");
    assert!(matches!(err, CoreError::Config(_)), "unerwartet: {err}");
}

#[tokio::test]
async fn verbindungen_werden_wiederverwendet_und_beim_sperren_getrennt() {
    let Fixture { core, .. } = fixture();
    let server = TestServer::start().await;
    let identity = password_identity(&core);
    let host = add_host(&core, "test", server.addr.port(), |h| h.identity_id = Some(identity));

    let first = core.connect_pooled(host, TestUi::new(true)).await.expect("verbinden");
    let second = core.connect_pooled(host, Arc::new(NonInteractive)).await.expect("verbinden");
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(core.pooled_hosts().await, vec![host]);

    // In Gebrauch befindliche Verbindungen bleiben, unbenutzte werden abgebaut.
    assert_eq!(core.disconnect_idle(Duration::ZERO).await, 0);
    drop((first, second));
    assert_eq!(core.disconnect_idle(Duration::from_secs(3600)).await, 0);
    assert_eq!(core.disconnect_idle(Duration::ZERO).await, 1);
    assert!(core.pooled_hosts().await.is_empty());
    core.connect_pooled(host, Arc::new(NonInteractive)).await.expect("verbinden");

    core.lock().await;
    assert!(!core.is_unlocked());
    assert!(core.pooled_hosts().await.is_empty());
    assert!(matches!(core.resolve_host(host), Err(CoreError::Vault(VaultError::Locked))));
    core.unlock("passwort").expect("entsperren");
    assert!(core.resolve_host(host).is_ok());
}

/// Host, der nur über ein VPN-Profil erreichbar ist: Der Tunnel wird beim
/// Verbinden automatisch gestartet und nach Gebrauch wieder abgebaut.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn host_hinter_vpn_profil() {
    let Fixture { core, .. } = fixture();
    let (client_private, client_public) = generate_keypair().expect("keys");
    let (gateway_private, gateway_public) = generate_keypair().expect("keys");
    let port = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").port();

    let mut gateway_config = WgConfig::from_profile(&VpnProfile {
        private_key: gateway_private.into(),
        addresses: vec!["10.55.0.1/24".into()],
        peers: vec![VpnPeer {
            public_key: client_public,
            endpoint: "127.0.0.1:9".into(),
            allowed_ips: vec!["10.55.0.2/32".into()],
            ..VpnPeer::default()
        }],
        ..VpnProfile::default()
    })
    .expect("config");
    gateway_config.listen_port = Some(port);
    let gateway = WgTunnel::start(gateway_config).await.expect("gateway");
    let server = TestServer::without_listener();
    let fingerprint = server.host_key_fingerprint.clone();
    let mut listener = gateway.listen(22).expect("listen");
    tokio::spawn(async move {
        while let Ok(stream) = listener.accept().await {
            server.serve(stream);
        }
    });

    let profile = core
        .with_vault(|v| {
            v.insert(&VpnProfile {
                label: "Büro".into(),
                private_key: client_private.into(),
                addresses: vec!["10.55.0.2/32".into()],
                peers: vec![VpnPeer {
                    public_key: gateway_public,
                    endpoint: format!("127.0.0.1:{port}"),
                    allowed_ips: vec!["10.55.0.0/24".into()],
                    ..VpnPeer::default()
                }],
                ..VpnProfile::default()
            })
        })
        .expect("profil");
    let identity = password_identity(&core);
    let host = core
        .with_vault(|v| {
            v.insert(&Host {
                label: "intern".into(),
                address: "10.55.0.1".into(),
                identity_id: Some(identity),
                vpn_profile_id: Some(profile),
                ..Host::default()
            })
        })
        .expect("host");

    // Derselben Adresse wurde früher schon einmal vertraut – aber ohne VPN.
    // Im Netz hinter dem Tunnel kann sie einem ganz anderen Rechner gehören;
    // das alte Vertrauen darf deshalb nicht gelten.
    core.with_vault(|v| {
        v.insert(&KnownHost {
            host: "10.55.0.1".into(),
            port: 22,
            network: None,
            key_type: "ssh-ed25519".into(),
            fingerprint: fingerprint.clone(),
            ..KnownHost::default()
        })
    })
    .expect("bekannt");

    assert!(core.tunnel_states().await.is_empty());
    let ui = TestUi::new(true);
    let client = core.connect(host, ui.clone()).await.expect("verbinden durch den tunnel");
    assert_eq!(ui.questions().len(), 1, "Vertrauen ohne VPN darf im VPN nicht gelten");
    assert_eq!(ui.questions()[0].state, HostKeyState::Unknown);
    assert_eq!(echo(&client, "intern").await, "intern\n");
    assert!(client.description().contains("WireGuard"));

    let states = core.tunnel_states().await;
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].label, "Büro");
    assert_eq!(states[0].connections, 1);
    assert!(states[0].peers[0].last_handshake.is_some());

    // Der bekannte Hostschlüssel ist an das VPN-Profil gebunden.
    let known = core.with_vault(|v| v.list::<KnownHost>()).expect("liste");
    assert_eq!(known.len(), 2);
    assert!(known.iter().any(|(_, k)| k.network == Some(profile) && k.fingerprint == fingerprint));

    // Solange die Verbindung steht, bleibt der Tunnel; danach wird er abgebaut.
    assert_eq!(core.close_idle_tunnels(Duration::ZERO).await, 0);
    client.disconnect().await;
    drop(client);
    let mut closed = 0;
    for _ in 0..50 {
        closed = core.close_idle_tunnels(Duration::ZERO).await;
        if closed == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(closed, 1, "Tunnel wurde nach Verbindungsende nicht frei");
    assert!(core.tunnel_states().await.is_empty());
}
