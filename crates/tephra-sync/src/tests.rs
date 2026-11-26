//! Ende-zu-Ende-Tests: Ein echter Sync-Server läuft im Testprozess, mehrere
//! Geräte gleichen über HTTP mit ihm ab.

use super::*;
use std::path::PathBuf;
use std::time::Instant;
use tephra_core::model::{Host, SshKey};
use tephra_server::db::Db;
use tephra_server::{Config, Registration};

const KDF: KdfParams = KdfParams::INSECURE_FOR_TESTS;
const PASSWORD: &str = "richtiges passwort";

struct Server {
    url: String,
    db_path: PathBuf,
    dir: tempfile::TempDir,
}

async fn server_with(config: Config) -> (Server, String) {
    let dir = tempfile::tempdir().expect("tmp");
    let db_path = dir.path().join("server.sqlite");
    let mut db = Db::open(&db_path).expect("db");
    let invite = db.create_invite().expect("einladung");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    tokio::spawn(tephra_server::serve(listener, db, config, std::future::pending()));
    (Server { url, db_path, dir }, invite)
}

async fn server() -> Server {
    server_with(Config { registration: Registration::Open, ..Config::default() }).await.0
}

/// Erstes Gerät: legt einen Tresor an und registriert das Konto „anna".
async fn first_device(server: &Server) -> Sync {
    let (core, _) = Core::create(&server.dir.path().join("a.tephra"), PASSWORD, KDF).expect("tresor");
    let sync = Sync::new(core);
    sync.register(&server.url, "anna", PASSWORD, "Laptop", None, false).await.expect("registrieren");
    sync
}

async fn join(server: &Server, file: &str, name: &str) -> Sync {
    let core =
        Sync::join(&server.dir.path().join(file), &server.url, "anna", PASSWORD, name, false).await.expect("beitreten");
    Sync::new(core)
}

fn host(label: &str) -> Host {
    Host { label: label.into(), address: format!("{label}.example.org"), ..Host::default() }
}

fn labels(sync: &Sync) -> Vec<String> {
    let mut labels: Vec<String> =
        sync.core.with_vault(|v| v.list::<Host>()).expect("liste").into_iter().map(|(_, h)| h.label).collect();
    labels.sort();
    labels
}

fn server_error(result: Result<impl Sized>) -> String {
    match result {
        Err(SyncError::Server { code, .. }) => code,
        Err(other) => panic!("anderer Fehler als erwartet: {other}"),
        Ok(_) => panic!("Fehler erwartet"),
    }
}

#[tokio::test]
async fn zwei_geraete_gleichen_ab() {
    let server = server().await;
    let a = first_device(&server).await;
    let web = a.core.with_vault(|v| v.insert(&host("web"))).expect("insert");
    a.core.with_vault(|v| v.insert(&host("db"))).expect("insert");
    let report = a.sync_once().await.expect("abgleich");
    assert_eq!(report.pushed, 2);

    // Das zweite Gerät bekommt beim Beitreten alles und kann den Tresor mit
    // dem Passwort öffnen.
    let b = join(&server, "b.tephra", "Handy").await;
    assert_eq!(labels(&b), vec!["db", "web"]);
    b.core.lock().await;
    b.core.unlock(PASSWORD).expect("entsperren");
    assert_eq!(a.connection().expect("verbindung").expect("verbunden").username, "anna");

    // Beide ändern unterschiedliche Felder desselben Hosts.
    a.core
        .with_vault(|v| {
            let mut h = v.get::<Host>(web)?.expect("da");
            h.label = "webserver".into();
            v.update(web, &h)
        })
        .expect("ändern");
    b.core
        .with_vault(|v| {
            let mut h = v.get::<Host>(web)?.expect("da");
            h.port = 2222;
            v.update(web, &h)
        })
        .expect("ändern");
    a.sync_once().await.expect("abgleich");
    let report = b.sync_once().await.expect("abgleich");
    assert_eq!(report.merged, 1);
    a.sync_once().await.expect("abgleich");
    for device in [&a, &b] {
        let merged = device.core.with_vault(|v| v.get::<Host>(web)).expect("get").expect("da");
        assert_eq!((merged.label.as_str(), merged.port), ("webserver", 2222));
    }

    // Löschen wirkt auf beiden Geräten.
    b.core.with_vault(|v| v.delete(web)).expect("löschen");
    b.sync_once().await.expect("abgleich");
    a.sync_once().await.expect("abgleich");
    assert_eq!(labels(&a), vec!["db"]);

    // Ein weiterer Abgleich ohne Änderungen tut nichts.
    assert_eq!(a.sync_once().await.expect("abgleich"), SyncReport::default());
    assert_eq!(b.sync_once().await.expect("abgleich"), SyncReport::default());
}

#[tokio::test]
async fn server_sieht_keinen_klartext() {
    let server = server().await;
    let a = first_device(&server).await;
    a.core
        .with_vault(|v| {
            v.insert(&SshKey {
                label: "EINDEUTIGES-ETIKETT".into(),
                private_key: "STRENG-GEHEIMER-SCHLUESSEL".into(),
                ..SshKey::default()
            })
        })
        .expect("insert");
    a.sync_once().await.expect("abgleich");

    let mut stored = std::fs::read(&server.db_path).expect("lesen");
    if let Ok(wal) = std::fs::read(format!("{}-wal", server.db_path.display())) {
        stored.extend(wal);
    }
    assert!(stored.len() > 4096);
    for needle in [&b"EINDEUTIGES-ETIKETT"[..], b"STRENG-GEHEIMER-SCHLUESSEL", PASSWORD.as_bytes(), b"ssh_key"] {
        assert!(!stored.windows(needle.len()).any(|w| w == needle), "Klartext beim Server gefunden");
    }
}

#[tokio::test]
async fn nur_lokale_schluessel_verlassen_das_geraet_nicht() {
    let server = server().await;
    let a = first_device(&server).await;
    a.core
        .with_vault(|v| v.insert_local_only(&SshKey { label: "nur hier".into(), ..SshKey::default() }))
        .expect("insert");
    a.core.with_vault(|v| v.insert(&host("web"))).expect("insert");
    assert_eq!(a.sync_once().await.expect("abgleich").pushed, 1);
    let b = join(&server, "b.tephra", "Handy").await;
    assert!(b.core.with_vault(|v| v.list::<SshKey>()).expect("liste").is_empty());
    assert_eq!(labels(&b), vec!["web"]);
}

#[tokio::test]
async fn falsches_passwort_und_unbekanntes_konto_sehen_gleich_aus() {
    let server = server().await;
    first_device(&server).await;
    let dir = &server.dir;
    let wrong = Sync::join(&dir.path().join("x.tephra"), &server.url, "anna", "falsch", "X", false).await;
    let unknown = Sync::join(&dir.path().join("y.tephra"), &server.url, "niemand", "falsch", "X", false).await;
    assert_eq!(server_error(wrong.map(|_| ())), codes::INVALID_CREDENTIALS);
    assert_eq!(server_error(unknown.map(|_| ())), codes::INVALID_CREDENTIALS);
    assert!(!dir.path().join("x.tephra").exists(), "nach fehlgeschlagener Anmeldung darf kein Tresor liegen bleiben");

    // Die Passwortparameter eines unbekannten Namens sind stabil und plausibel.
    let http = http_client();
    let prelogin = |name: &'static str| {
        let (http, url) = (http.clone(), server.url.clone());
        async move {
            parse::<PreloginResponse>(
                http.post(format!("{url}/v1/auth/prelogin"))
                    .json(&PreloginRequest { username: name.into() })
                    .send()
                    .await,
            )
            .await
            .expect("prelogin")
        }
    };
    let (first, second) = (prelogin("niemand").await, prelogin("niemand").await);
    assert_eq!(first.salt, second.salt);
    assert_eq!(first.salt.0.len(), prelogin("anna").await.salt.0.len());
    assert_ne!(first.salt, prelogin("jemand.anderes").await.salt);
}

#[tokio::test]
async fn anmeldeversuche_werden_gebremst() {
    let (server, _) =
        server_with(Config { registration: Registration::Open, max_attempts: 3, ..Config::default() }).await;
    first_device(&server).await;
    let attempt = |n: usize| {
        let (path, url) = (server.dir.path().join(format!("v{n}.tephra")), server.url.clone());
        async move { Sync::join(&path, &url, "anna", "falsch", "X", false).await.map(|_| ()) }
    };
    for n in 0..3 {
        assert_eq!(server_error(attempt(n).await), codes::INVALID_CREDENTIALS);
    }
    assert_eq!(server_error(attempt(3).await), codes::TOO_MANY_REQUESTS);
    // Auch das richtige Passwort wird jetzt vorerst abgewiesen.
    let right = Sync::join(&server.dir.path().join("r.tephra"), &server.url, "anna", PASSWORD, "X", false).await;
    assert_eq!(server_error(right.map(|_| ())), codes::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn registrierung_nach_einladung_und_doppelte_namen() {
    let (server, invite) = server_with(Config::default()).await;
    let new_vault =
        |file: &str| Sync::new(Core::create(&server.dir.path().join(file), PASSWORD, KDF).expect("tresor").0);
    let a = new_vault("a.tephra");
    assert_eq!(server_error(a.register(&server.url, "anna", PASSWORD, "L", None, false).await), codes::INVALID_INVITE);
    assert_eq!(
        server_error(a.register(&server.url, "anna", PASSWORD, "L", Some("falsch"), false).await),
        codes::INVALID_INVITE
    );
    a.register(&server.url, "anna", PASSWORD, "L", Some(&invite), false).await.expect("registrieren");
    assert!(matches!(
        a.register(&server.url, "anna", PASSWORD, "L", Some(&invite), false).await,
        Err(SyncError::AlreadyConnected)
    ));

    // Der Code gilt nur einmal.
    let b = new_vault("b.tephra");
    assert_eq!(
        server_error(b.register(&server.url, "bert", PASSWORD, "L", Some(&invite), false).await),
        codes::INVALID_INVITE
    );

    let (open, _) = server_with(Config { registration: Registration::Open, ..Config::default() }).await;
    let c = Sync::new(Core::create(&open.dir.path().join("c.tephra"), PASSWORD, KDF).expect("tresor").0);
    c.register(&open.url, "Carl", PASSWORD, "L", None, false).await.expect("registrieren");
    let d = Sync::new(Core::create(&open.dir.path().join("d.tephra"), PASSWORD, KDF).expect("tresor").0);
    assert_eq!(server_error(d.register(&open.url, "carl", PASSWORD, "L", None, false).await), codes::USERNAME_TAKEN);
    assert_eq!(server_error(d.register(&open.url, "x", PASSWORD, "L", None, false).await), codes::INVALID_REQUEST);
    // Mit falschem Passwort wird gar nicht erst registriert.
    assert!(matches!(
        d.register(&open.url, "dora", "falsch", "L", None, false).await,
        Err(SyncError::Core(CoreError::Vault(VaultError::WrongPassword)))
    ));

    let (closed, _) = server_with(Config { registration: Registration::Closed, ..Config::default() }).await;
    let e = Sync::new(Core::create(&closed.dir.path().join("e.tephra"), PASSWORD, KDF).expect("tresor").0);
    assert_eq!(
        server_error(e.register(&closed.url, "emil", PASSWORD, "L", None, false).await),
        codes::REGISTRATION_CLOSED
    );
}

#[tokio::test]
async fn abgelaufene_zugriffstokens_werden_unbemerkt_erneuert() {
    let (server, _) = server_with(Config {
        registration: Registration::Open,
        access_ttl: Duration::from_millis(400),
        ..Config::default()
    })
    .await;
    let a = first_device(&server).await;
    a.core.with_vault(|v| v.insert(&host("web"))).expect("insert");
    // Das Zugriffstoken läuft ab; der Abgleich holt sich von selbst ein neues.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(a.sync_once().await.expect("abgleich").pushed, 1);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(a.devices().await.expect("geräte").len(), 1);
}

#[tokio::test]
async fn geraet_entfernen_und_abmelden() {
    let server = server().await;
    let a = first_device(&server).await;
    let b = join(&server, "b.tephra", "Handy").await;
    let devices = a.devices().await.expect("geräte");
    assert_eq!(devices.len(), 2);
    assert_eq!(devices.iter().filter(|d| d.current).count(), 1);
    let phone = devices.iter().find(|d| d.name == "Handy").expect("handy");

    assert!(a.remove_device(phone.id, "falsch").await.is_err(), "nur mit dem Master-Passwort");
    // Der Server selbst lehnt ohne Nachweis ab – wie bei jemandem, der nur ein Sitzungstoken mitgeschnitten hat.
    let bare = a.authed::<Option<()>>(|http, url| http.delete(format!("{url}/v1/devices/{}", phone.id))).await;
    assert_eq!(server_error(bare), codes::INVALID_REQUEST);
    let wrong = RemoveDeviceRequest { auth_key: Bytes(vec![0; 32]) };
    let forged =
        a.authed::<Option<()>>(|http, url| http.delete(format!("{url}/v1/devices/{}", phone.id)).json(&wrong)).await;
    assert_eq!(server_error(forged), codes::INVALID_CREDENTIALS);
    a.remove_device(phone.id, PASSWORD).await.expect("entfernen");
    assert!(matches!(b.sync_once().await, Err(SyncError::SessionExpired)));
    assert_eq!(a.devices().await.expect("geräte").len(), 1);
    // Das entfernte Gerät kann sich mit dem Passwort wieder anmelden; sein Tresor bleibt.
    b.reconnect(PASSWORD, "Handy").await.expect("neu anmelden");
    b.sync_once().await.expect("abgleich");

    a.disconnect().await.expect("abmelden");
    assert!(a.connection().expect("verbindung").is_none());
    assert!(matches!(a.sync_once().await, Err(SyncError::NotConnected)));
}

#[tokio::test]
async fn passwortwechsel_meldet_andere_geraete_ab() {
    let server = server().await;
    let a = first_device(&server).await;
    a.core.with_vault(|v| v.insert(&host("web"))).expect("insert");
    a.sync_once().await.expect("abgleich");
    let b = join(&server, "b.tephra", "Handy").await;

    assert!(matches!(
        a.change_password("falsch", "neu", KDF).await,
        Err(SyncError::Core(CoreError::Vault(VaultError::WrongPassword)))
    ));
    a.change_password(PASSWORD, "neues passwort", KDF).await.expect("ändern");
    a.core.lock().await;
    assert!(a.core.unlock(PASSWORD).is_err());
    a.core.unlock("neues passwort").expect("entsperren");
    a.sync_once().await.expect("abgleich nach wechsel");

    // Gerät B kannte nur das alte Passwort und ist abgemeldet.
    assert!(matches!(b.sync_once().await, Err(SyncError::SessionExpired)));
    assert_eq!(server_error(b.reconnect(PASSWORD, "Handy").await), codes::INVALID_CREDENTIALS);
    b.reconnect("neues passwort", "Handy").await.expect("neu anmelden");
    b.sync_once().await.expect("abgleich");
    b.core.lock().await;
    b.core.unlock("neues passwort").expect("entsperren mit neuem passwort");
    assert_eq!(labels(&b), vec!["web"]);

    // Ein neues Gerät meldet sich nur noch mit dem neuen Passwort an.
    let old = Sync::join(&server.dir.path().join("c.tephra"), &server.url, "anna", PASSWORD, "C", false).await;
    assert_eq!(server_error(old.map(|_| ())), codes::INVALID_CREDENTIALS);
    Sync::join(&server.dir.path().join("d.tephra"), &server.url, "anna", "neues passwort", "D", false)
        .await
        .expect("beitreten");
}

#[tokio::test]
async fn server_meldet_aenderungen_sofort() {
    let server = server().await;
    let a = first_device(&server).await;
    let b = join(&server, "b.tephra", "Handy").await;

    // Ohne Änderung läuft die Wartezeit ab.
    assert!(!b.wait_for_changes(Duration::from_secs(1)).await.expect("warten"));

    let waiting = {
        let b = b.clone();
        tokio::spawn(async move {
            let started = Instant::now();
            (b.wait_for_changes(Duration::from_secs(30)).await.expect("warten"), started.elapsed())
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    a.core.with_vault(|v| v.insert(&host("neu"))).expect("insert");
    a.sync_once().await.expect("abgleich");
    let (changed, elapsed) = waiting.await.expect("task");
    assert!(changed);
    assert!(elapsed < Duration::from_secs(10), "Benachrichtigung kam erst nach {elapsed:?}");
    b.sync_once().await.expect("abgleich");
    assert_eq!(labels(&b), vec!["neu"]);
}

#[tokio::test]
async fn laufender_abgleich_uebertraegt_aenderungen_von_selbst() {
    let server = server().await;
    let a = first_device(&server).await;
    let b = join(&server, "b.tephra", "Handy").await;
    let (stop, stopped) = watch::channel(false);
    let runners: Vec<_> = [a.clone(), b.clone()]
        .into_iter()
        .map(|sync| {
            let stopped = stopped.clone();
            tokio::spawn(async move { sync.run(stopped, |_| {}).await })
        })
        .collect();

    a.core.with_vault(|v| v.insert(&host("automatisch"))).expect("insert");
    let started = Instant::now();
    while labels(&b).is_empty() {
        assert!(started.elapsed() < Duration::from_secs(20), "Änderung kam nicht von selbst an");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    stop.send(true).expect("stoppen");
    for runner in runners {
        tokio::time::timeout(Duration::from_secs(10), runner).await.expect("Abgleich endet").expect("task");
    }
}

#[tokio::test]
async fn kontingent_wird_gemeldet_und_nichts_geht_verloren() {
    let (server, _) = server_with(Config { registration: Registration::Open, max_items: 2, ..Config::default() }).await;
    let a = first_device(&server).await;
    for n in 0..3 {
        a.core.with_vault(|v| v.insert(&host(&format!("h{n}")))).expect("insert");
    }
    let report = a.sync_once().await.expect("abgleich");
    assert_eq!(report.pushed, 2);
    assert_eq!(report.rejected.len(), 1);
    assert_eq!(labels(&a).len(), 3, "lokal bleibt alles erhalten");
    assert_eq!(a.core.with_vault(|v| v.dirty_items()).expect("dirty").len(), 1);
}

#[tokio::test]
async fn viele_datensaetze_werden_in_mehreren_anfragen_uebertragen() {
    let server = server().await;
    let a = first_device(&server).await;
    let count = MAX_BATCH + 50;
    a.core
        .with_vault(|v| {
            for n in 0..count {
                v.insert(&host(&format!("h{n:04}")))?;
            }
            Ok(())
        })
        .expect("insert");
    assert_eq!(a.sync_once().await.expect("abgleich").pushed, count);
    let b = join(&server, "b.tephra", "Handy").await;
    assert_eq!(labels(&b).len(), count);
}

#[test]
fn nur_verschluesselte_oder_lokale_adressen() {
    assert_eq!(normalize_url("https://sync.example.org/", false).expect("ok"), "https://sync.example.org");
    assert!(normalize_url("http://127.0.0.1:8787", false).is_ok());
    assert!(normalize_url("http://localhost:8787", false).is_ok());
    assert!(normalize_url("http://[::1]:8787", false).is_ok());
    assert!(matches!(normalize_url("http://sync.example.org", false), Err(SyncError::InsecureUrl)));
    assert!(matches!(normalize_url("http://192.168.1.10:8787", false), Err(SyncError::InsecureUrl)));
    assert!(matches!(normalize_url("HTTP://192.168.1.10:8787", false), Err(SyncError::InsecureUrl)));
    assert!(matches!(normalize_url("ftp://sync.example.org", false), Err(SyncError::InvalidUrl(_))));
    assert!(matches!(normalize_url("kein url", false), Err(SyncError::InvalidUrl(_))));
}

#[test]
fn unverschluesselt_nur_mit_ausdruecklicher_erlaubnis() {
    assert_eq!(normalize_url("http://192.168.1.10:8787/", true).expect("ok"), "http://192.168.1.10:8787");
    assert!(normalize_url("http://nas.local:8787", true).is_ok());
    assert!(normalize_url("https://sync.example.org", true).is_ok());
    // Die Erlaubnis gilt nur für http, nicht für beliebige Schemata.
    assert!(matches!(normalize_url("ftp://192.168.1.10", true), Err(SyncError::InvalidUrl(_))));

    assert!(is_unencrypted("http://192.168.1.10:8787"));
    assert!(is_unencrypted(" http://nas.local "));
    assert!(!is_unencrypted("http://localhost:8787"));
    assert!(!is_unencrypted("http://127.0.0.1:8787"));
    assert!(!is_unencrypted("https://sync.example.org"));
    assert!(!is_unencrypted("192.168.1.10"));
}

#[tokio::test]
async fn unverschluesselter_server_im_eigenen_netz() {
    // 127.0.0.2 liegt auf diesem Rechner, gilt der Prüfung aber wie jede
    // Adresse im Netz als fremd.
    let dir = tempfile::tempdir().expect("tmp");
    let db = Db::open(&dir.path().join("server.sqlite")).expect("db");
    let listener = tokio::net::TcpListener::bind("127.0.0.2:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let config = Config { registration: Registration::Open, ..Config::default() };
    tokio::spawn(tephra_server::serve(listener, db, config, std::future::pending()));
    let (core, _) = Core::create(&dir.path().join("a.tephra"), PASSWORD, KDF).expect("tresor");
    let a = Sync::new(core);
    let second = dir.path().join("b.tephra");

    // Ohne Erlaubnis geht nichts an den Server, und nichts wird angelegt.
    let refused = a.register(&url, "anna", PASSWORD, "Laptop", None, false).await;
    assert!(matches!(refused, Err(SyncError::InsecureUrl)));
    assert!(a.connection().expect("verbindung").is_none());
    assert!(matches!(Sync::join(&second, &url, "anna", PASSWORD, "Handy", false).await, Err(SyncError::InsecureUrl)));
    assert!(!second.exists());

    a.register(&url, "anna", PASSWORD, "Laptop", None, true).await.expect("registrieren");
    a.core.with_vault(|v| v.insert(&host("nas"))).expect("insert");
    assert_eq!(a.sync_once().await.expect("abgleich").pushed, 1);
    let b = Sync::new(Sync::join(&second, &url, "anna", PASSWORD, "Handy", true).await.expect("beitreten"));
    assert_eq!(labels(&b), vec!["nas"]);
    // Die Erlaubnis steckt in der gespeicherten Adresse: Späteres Abgleichen fragt nicht erneut.
    assert_eq!(b.connection().expect("verbindung").expect("verbunden").server, url);
    b.sync_once().await.expect("abgleich");
}

#[tokio::test]
async fn einladung_gilt_auch_bei_gleichzeitigen_anfragen_nur_einmal() {
    let (server, invite) = server_with(Config::default()).await;
    let mut tasks = Vec::new();
    for n in 0..6 {
        let (url, invite, path) = (server.url.clone(), invite.clone(), server.dir.path().join(format!("p{n}.tephra")));
        tasks.push(tokio::spawn(async move {
            let sync = Sync::new(Core::create(&path, PASSWORD, KDF).expect("tresor").0);
            sync.register(&url, &format!("nutzer{n}"), PASSWORD, "L", Some(&invite), false).await.is_ok()
        }));
    }
    let mut created = 0;
    for task in tasks {
        created += usize::from(task.await.expect("task"));
    }
    assert_eq!(created, 1, "ein Einladungscode hat {created} Konten erzeugt");
}

#[tokio::test]
async fn richtige_anmeldung_wird_nie_gebremst_und_fehlversuche_zaehlen_je_name() {
    let (server, _) =
        server_with(Config { registration: Registration::Open, max_attempts: 3, ..Config::default() }).await;
    first_device(&server).await;
    // Viele korrekte Anmeldungen hintereinander sind kein Angriff.
    for n in 0..6 {
        Sync::join(&server.dir.path().join(format!("ok{n}.tephra")), &server.url, "anna", PASSWORD, "X", false)
            .await
            .expect("korrekte anmeldung");
    }
    // Fehlversuche für einen anderen Namen sperren „anna" nicht aus.
    for n in 0..3 {
        let wrong =
            Sync::join(&server.dir.path().join(format!("w{n}.tephra")), &server.url, "berta", "falsch", "X", false)
                .await;
        assert_eq!(server_error(wrong.map(|_| ())), codes::INVALID_CREDENTIALS);
    }
    Sync::join(&server.dir.path().join("danach.tephra"), &server.url, "anna", PASSWORD, "X", false)
        .await
        .expect("anna");
}

#[tokio::test]
async fn ueberlange_namen_und_grosse_anfragen_werden_abgewiesen() {
    let server = server().await;
    let http = http_client();
    let long = "x".repeat(100_000);
    let response = http
        .post(format!("{}/v1/auth/prelogin", server.url))
        .json(&PreloginRequest { username: long.clone() })
        .send()
        .await
        .expect("anfrage");
    assert!(response.status().is_client_error());
    // Anmelde-Routen nehmen keine großen Anfragen an.
    let huge = format!("{{\"username\":\"{}\"}}", "y".repeat(200_000));
    let response = http
        .post(format!("{}/v1/auth/prelogin", server.url))
        .header("content-type", "application/json")
        .body(huge)
        .send()
        .await
        .expect("anfrage");
    assert_eq!(response.status().as_u16(), 413);
}

#[tokio::test]
async fn gleichzeitige_anfragen_ueberstehen_ein_abgelaufenes_token() {
    let (server, _) = server_with(Config {
        registration: Registration::Open,
        access_ttl: Duration::from_millis(300),
        ..Config::default()
    })
    .await;
    let a = first_device(&server).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    // Acht Aufgaben stoßen gleichzeitig auf das abgelaufene Token.
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let a = a.clone();
            tokio::spawn(async move { a.devices().await.map(|d| d.len()) })
        })
        .collect();
    for task in tasks {
        assert_eq!(task.await.expect("task").expect("anfrage nach erneuerung"), 1);
    }
}

#[tokio::test]
async fn zu_grosser_datensatz_haelt_den_rest_nicht_auf() {
    let server = server().await;
    let a = first_device(&server).await;
    a.core.with_vault(|v| v.insert(&host("klein"))).expect("insert");
    let big =
        Host { label: "riesig".into(), notes: "n".repeat(tephra_sync_proto::MAX_BLOB_BYTES + 10), ..Host::default() };
    a.core.with_vault(|v| v.insert(&big)).expect("insert");
    let report = a.sync_once().await.expect("abgleich");
    assert_eq!(report.pushed, 1);
    assert_eq!(report.rejected.len(), 1);
    let b = join(&server, "b.tephra", "Handy").await;
    assert_eq!(labels(&b), vec!["klein"]);
}

#[tokio::test]
async fn konten_sind_voneinander_getrennt() {
    let server = server().await;
    let anna = first_device(&server).await;
    let (core, _) = Core::create(&server.dir.path().join("bert.tephra"), "anderes passwort", KDF).expect("tresor");
    let bert = Sync::new(core);
    bert.register(&server.url, "bert", "anderes passwort", "PC", None, false).await.expect("registrieren");

    let annas_host = anna.core.with_vault(|v| v.insert(&host("annas-server"))).expect("insert");
    anna.sync_once().await.expect("abgleich");

    // Bert sieht nichts von Anna …
    assert_eq!(bert.sync_once().await.expect("abgleich"), SyncReport::default());
    assert!(labels(&bert).is_empty());
    // … kann ihr Gerät nicht entfernen …
    let annas_device = anna.devices().await.expect("geräte")[0].id;
    assert_eq!(server_error(bert.remove_device(annas_device, "anderes passwort").await), codes::NOT_FOUND);
    assert_eq!(anna.devices().await.expect("geräte").len(), 1);
    // … und ein Datensatz unter derselben Kennung landet nur in seinem eigenen Konto.
    let stored = bert.require().expect("sitzung");
    let request = PushRequest {
        items: vec![PushItem { id: annas_host.0, base_rev: 0, deleted: true, blob: Bytes(vec![1, 2, 3]) }],
    };
    let response: PushResponse = parse(
        bert.http
            .post(format!("{}/v1/sync/push", server.url))
            .bearer_auth(&stored.access_token)
            .json(&request)
            .send()
            .await,
    )
    .await
    .expect("push");
    assert!(matches!(response.results[0].outcome, PushOutcome::Accepted { .. }));
    anna.sync_once().await.expect("abgleich");
    assert_eq!(labels(&anna), vec!["annas-server"], "fremder Löschmarker hat Annas Daten erreicht");
    // Berts Passwort gilt nicht für Annas Konto.
    let wrong =
        Sync::join(&server.dir.path().join("x.tephra"), &server.url, "anna", "anderes passwort", "X", false).await;
    assert_eq!(server_error(wrong.map(|_| ())), codes::INVALID_CREDENTIALS);
}

#[tokio::test]
async fn konto_loeschen_verlangt_das_passwort_und_entfernt_alles() {
    let server = server().await;
    let anna = first_device(&server).await;
    let (core, _) = Core::create(&server.dir.path().join("bert.tephra"), "anderes passwort", KDF).expect("tresor");
    let bert = Sync::new(core);
    bert.register(&server.url, "bert", "anderes passwort", "PC", None, false).await.expect("registrieren");
    anna.core.with_vault(|v| v.insert(&host("web"))).expect("insert");
    anna.sync_once().await.expect("abgleich");

    let header = anna.core.with_vault(|v| Ok(v.header().clone())).expect("kopf");
    let delete = |password: &'static str| {
        let (sync, header) = (anna.clone(), header.clone());
        async move {
            let stored = sync.require().expect("sitzung");
            let body = tephra_sync_proto::DeleteAccountRequest {
                auth_key: Bytes(header.auth_key(password).expect("nachweis").as_bytes().to_vec()),
            };
            parse::<Option<()>>(
                sync.http
                    .delete(format!("{}/v1/account", stored.server))
                    .bearer_auth(&stored.access_token)
                    .json(&body)
                    .send()
                    .await,
            )
            .await
        }
    };
    // Ein gestohlenes Sitzungstoken allein genügt nicht.
    assert_eq!(server_error(delete("falsches passwort").await), codes::INVALID_CREDENTIALS);
    anna.sync_once().await.expect("konto besteht noch");
    delete(PASSWORD).await.expect("löschen");

    assert!(matches!(anna.sync_once().await, Err(SyncError::SessionExpired)));
    let gone = Sync::join(&server.dir.path().join("neu.tephra"), &server.url, "anna", PASSWORD, "X", false).await;
    assert_eq!(server_error(gone.map(|_| ())), codes::INVALID_CREDENTIALS);
    // Das andere Konto ist unberührt, der lokale Tresor ebenfalls.
    bert.sync_once().await.expect("bert gleicht weiter ab");
    assert_eq!(labels(&anna), vec!["web"]);
}

#[tokio::test]
async fn zerstoerte_serverkopien_werden_wiederhergestellt() {
    let server = server().await;
    let a = first_device(&server).await;
    let web = a.core.with_vault(|v| v.insert(&host("web"))).expect("insert");
    a.sync_once().await.expect("abgleich");
    let b = join(&server, "b.tephra", "Handy").await;

    // Jemand mit einem gestohlenen Sitzungstoken überschreibt die Kopie beim Server mit Datenmüll.
    let current: PullResponse = a
        .authed(|http, url| http.get(format!("{url}/v1/sync")).query(&[("since", 0i64), ("limit", 200)]))
        .await
        .expect("holen");
    let rev = current.items.iter().find(|item| item.id == web.0).expect("da").rev;
    let garbage =
        PushRequest { items: vec![PushItem { id: web.0, base_rev: rev, deleted: false, blob: Bytes(vec![7; 300]) }] };
    let _: PushResponse =
        a.authed(|http, url| http.post(format!("{url}/v1/sync/push")).json(&garbage)).await.expect("müll hochladen");

    // Das nächste Gerät, das abgleicht, stellt seine heile Fassung wieder her.
    let report = b.sync_once().await.expect("abgleich");
    assert_eq!(report.reclaimed, 1, "{report:?}");
    assert_eq!(labels(&b), vec!["web"]);
    // Ein neues Gerät bekommt wieder einen lesbaren Datensatz.
    let c = join(&server, "c.tephra", "Neu").await;
    assert_eq!(labels(&c), vec!["web"]);
    // Und auch ein Gerät, das lokal ändert, bleibt nicht hängen.
    a.core.with_vault(|v| v.update(web, &host("web2")).map(|_| ())).expect("ändern");
    let report = a.sync_once().await.expect("abgleich");
    assert!(report.rejected.is_empty() && report.unreadable == 0, "{report:?}");
    c.sync_once().await.expect("abgleich");
    assert_eq!(labels(&c), vec!["web2"]);
}

#[tokio::test]
async fn nach_einem_update_wird_alles_neu_abgeholt() {
    let server = server().await;
    let a = first_device(&server).await;
    a.core.with_vault(|v| v.insert(&host("web"))).expect("insert");
    a.sync_once().await.expect("abgleich");
    let b = join(&server, "b.tephra", "Handy").await;
    b.sync_once().await.expect("abgleich");
    assert!(b.core.with_vault(|v| v.sync_cursor()).expect("stand") > 0);
    // Eine ältere Version hinterließ einen anderen Vermerk: Der Stand beginnt wieder bei null und alles kommt neu.
    b.core.with_vault(|v| v.put_secret(VERSION_SECRET, b"0.10.0")).expect("vermerk");
    let report = b.sync_once().await.expect("abgleich");
    assert_eq!(report.pulled + report.merged, 0, "nichts Neues, nur erneut gelesen: {report:?}");
    assert_eq!(
        b.core.with_vault(|v| v.get_secret(VERSION_SECRET)).expect("vermerk").as_deref(),
        Some(env!("CARGO_PKG_VERSION").as_bytes())
    );
}

// ------------------------------------------------------------------ TLS mit festem Fingerabdruck

/// Server mit selbst erzeugtem Zertifikat auf 127.0.0.2 – das gilt nicht als der eigene Rechner, eine
/// http-Adresse dorthin wäre also unverschlüsselt. `plain` lässt zusätzlich HTTP zu.
async fn tls_server(plain: bool) -> (Server, String, String) {
    let dir = tempfile::tempdir().expect("tmp");
    let db_path = dir.path().join("server.sqlite");
    let db = Db::open(&db_path).expect("db");
    let identity = tephra_server::tls::identity(&dir.path().join("tls"), None).expect("zertifikat");
    let listener = tokio::net::TcpListener::bind("127.0.0.2:0").await.expect("bind");
    let address = listener.local_addr().expect("addr");
    let config = Config { registration: Registration::Open, ..Config::default() };
    tokio::spawn(tephra_server::serve_tls(
        listener,
        db,
        config,
        Some(identity.acceptor),
        plain,
        std::future::pending(),
    ));
    let http = format!("http://{address}");
    let https = format!("https://{address}");
    (Server { url: http, db_path, dir }, https, identity.fingerprint)
}

#[tokio::test]
async fn http_adresse_wird_verschluesselt_mit_fingerabdruck() {
    let (server, https, fingerprint) = tls_server(true).await;
    let (core, _) = Core::create(&server.dir.path().join("a.tephra"), PASSWORD, KDF).expect("tresor");
    let a = Sync::new(core);
    // Ohne „unverschlüsselt erlauben“: Der Server kann HTTPS, also geht es auch so.
    a.register(&server.url, "anna", PASSWORD, "Laptop", None, false).await.expect("registrieren");
    let connection = a.connection().expect("lesen").expect("verbunden");
    assert_eq!(connection.server, https);
    assert!(connection.encrypted);
    assert_eq!(connection.fingerprint.as_deref(), Some(fingerprint.as_str()));
    a.core.with_vault(|v| v.insert(&host("web"))).expect("insert");
    a.sync_once().await.expect("abgleich");

    // Ein zweites Gerät tritt mit der http-Adresse bei und landet ebenso verschlüsselt.
    let b = Sync::new(
        Sync::join(&server.dir.path().join("b.tephra"), &server.url, "anna", PASSWORD, "Handy", false)
            .await
            .expect("beitreten"),
    );
    assert_eq!(b.connection().expect("lesen").expect("da").server, https);
    assert_eq!(labels(&b), vec!["web"]);
}

#[tokio::test]
async fn altes_geraet_steigt_von_selbst_um() {
    let (server, https, fingerprint) = tls_server(true).await;
    let (core, _) = Core::create(&server.dir.path().join("a.tephra"), PASSWORD, KDF).expect("tresor");
    let a = Sync::new(core);
    a.register(&server.url, "anna", PASSWORD, "Laptop", None, false).await.expect("registrieren");
    // So sah die Verbindung eines Geräts vor Version 0.12 aus.
    a.store(&Stored { server: server.url.clone(), pin: None, ..a.require().expect("da") }).expect("zurückdrehen");
    assert!(!a.connection().expect("lesen").expect("da").encrypted);
    a.sync_once().await.expect("abgleich");
    let connection = a.connection().expect("lesen").expect("da");
    assert_eq!(
        (connection.server.as_str(), connection.fingerprint.as_deref()),
        (https.as_str(), Some(fingerprint.as_str()))
    );
}

#[tokio::test]
async fn anderes_zertifikat_wird_erkannt_und_erst_nach_bestaetigung_angenommen() {
    let (server, _, fingerprint) = tls_server(true).await;
    let (core, _) = Core::create(&server.dir.path().join("a.tephra"), PASSWORD, KDF).expect("tresor");
    let a = Sync::new(core);
    a.register(&server.url, "anna", PASSWORD, "Laptop", None, false).await.expect("registrieren");
    // Jemand anderes antwortet – hier nachgestellt durch einen falschen gespeicherten Fingerabdruck.
    let stored = a.require().expect("da");
    a.store(&Stored { pin: Some("00".repeat(32)), ..stored }).expect("ändern");
    assert!(matches!(a.sync_once().await, Err(SyncError::CertificateChanged)));
    assert!(matches!(a.devices().await, Err(SyncError::CertificateChanged)));
    // Der Nutzer vergleicht und bestätigt.
    let shown = a.server_fingerprint().await.expect("probe").expect("https");
    assert_eq!(shown, fingerprint);
    a.trust_fingerprint(&shown).expect("übernehmen");
    a.sync_once().await.expect("abgleich");
    assert!(a.trust_fingerprint("kein fingerabdruck").is_err());
}

#[tokio::test]
async fn nur_https_weist_http_ab() {
    let (server, https, _) = tls_server(false).await;
    let plain = http_client().get(format!("{}/healthz", server.url)).send().await;
    assert!(plain.is_err(), "HTTP wird nicht beantwortet");
    let (core, _) = Core::create(&server.dir.path().join("a.tephra"), PASSWORD, KDF).expect("tresor");
    let a = Sync::new(core);
    a.register(&server.url, "anna", PASSWORD, "Laptop", None, false).await.expect("registrieren über https");
    assert_eq!(a.connection().expect("lesen").expect("da").server, https);
}

#[tokio::test]
async fn ohne_https_beim_server_bleibt_http_eine_ausdrueckliche_wahl() {
    let dir = tempfile::tempdir().expect("tmp");
    let db = Db::open(&dir.path().join("server.sqlite")).expect("db");
    let listener = tokio::net::TcpListener::bind("127.0.0.2:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let config = Config { registration: Registration::Open, ..Config::default() };
    tokio::spawn(tephra_server::serve(listener, db, config, std::future::pending()));
    let (core, _) = Core::create(&dir.path().join("a.tephra"), PASSWORD, KDF).expect("tresor");
    let a = Sync::new(core);
    let refused = a.register(&url, "anna", PASSWORD, "Laptop", None, false).await;
    assert!(matches!(refused, Err(SyncError::InsecureUrl)), "{refused:?}");
    a.register(&url, "anna", PASSWORD, "Laptop", None, true).await.expect("mit ausdrücklicher Erlaubnis");
    let connection = a.connection().expect("lesen").expect("da");
    assert!(!connection.encrypted && connection.fingerprint.is_none());
}
