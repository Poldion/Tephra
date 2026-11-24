//! Dienst, der MCP-Clients über einen lokalen Kanal bedient.
//!
//! Ein Sprachmodell startet `tephra mcp serve`. Dieses Programm ist nur eine
//! dünne Brücke: Es verbindet sich über einen lokalen Kanal (Named Pipe unter
//! Windows, Unix-Socket sonst) mit dem Dienst. Der Tresor wird so nur in
//! einem einzigen Prozess geöffnet, und Rückfragen erscheinen dort, wo der
//! Nutzer sitzt. Den Dienst stellt `tephra daemon` oder die App.
//!
//! Anmeldung am Kanal: Beide Seiten weisen einander nach, dass sie das Token
//! des Clients kennen, ohne es zu übertragen. Wer sich nur unter dem Namen
//! des Kanals als Dienst ausgibt, erfährt dadurch nichts, was er verwenden
//! könnte, und die Brücke gibt ihm keine MCP-Daten.
//!
//! ```text
//! Brücke → Dienst:  TEPHRA-MCP 2 <Hinweis> <Zufall C>
//! Dienst → Brücke:  OK <Zufall S> <Nachweis S>      oder   ERR <Grund>
//! Brücke → Dienst:  <Nachweis C>
//! ```

use crate::policy::{McpPolicy, token_hash};
use crate::util::printable;
use crate::{Approver, McpServer};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tephra_core::Core;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::task::JoinSet;

const HELLO: &str = "TEPHRA-MCP 2 ";

/// Was der Dienst über seine Clients meldet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceEvent {
    /// Ein Client hat sich ausgewiesen und ist verbunden.
    Connected {
        client: String,
    },
    Disconnected {
        client: String,
    },
}

/// Nimmt die Meldungen des Dienstes entgegen.
pub type Observer = Arc<dyn Fn(ServiceEvent) + Send + Sync>;

/// Name des lokalen Kanals. Er hängt vom Tresor ab, damit mehrere Tresore
/// nebeneinander laufen können.
pub fn endpoint(vault: &Path) -> String {
    // Derselbe Tresor soll unabhängig von der Schreibweise des Pfads zum
    // selben Kanal führen.
    let full = std::fs::canonicalize(vault).unwrap_or_else(|_| vault.to_path_buf());
    let name = full.to_string_lossy();
    #[cfg(windows)]
    let name = name.to_lowercase();
    let id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, name.as_bytes()).simple();
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\tephra-{id}")
    }
    #[cfg(not(windows))]
    {
        // Ein eigenes Verzeichnis, das nur dem Benutzer gehört: Der Socket
        // ist dadurch von Anfang an für niemanden sonst erreichbar.
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| full.parent().map(Path::to_path_buf))
            .unwrap_or_else(std::env::temp_dir);
        base.join(format!("tephra-{id}")).join("daemon.sock").to_string_lossy().into_owned()
    }
}

// ------------------------------------------------------------------ Anmeldung

fn nonce() -> Option<String> {
    Some(tephra_crypto::random_bytes::<16>().ok()?.iter().map(|b| format!("{b:02x}")).collect())
}

fn is_nonce(text: &str) -> bool {
    text.len() == 32 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Kennzeichen, an dem der Dienst den Client erkennt, ohne dass sich daraus
/// das Token oder dessen Hash zurückrechnen ließe.
fn hint(token_sha256: &str) -> String {
    token_hash(&format!("tephra-hint:{token_sha256}"))[..16].to_string()
}

/// Nachweis, das Token zu kennen, gebunden an die Zufallswerte beider Seiten.
fn proof(role: &str, token_sha256: &str, nonce_client: &str, nonce_server: &str) -> String {
    token_hash(&format!("tephra-{role}:{token_sha256}:{nonce_client}:{nonce_server}"))
}

/// Liest eine Zeile, höchstens `limit` Byte.
async fn read_line<S: AsyncRead + Unpin>(stream: &mut S, limit: usize) -> Option<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while line.len() < limit {
        if stream.read(&mut byte).await.ok()? == 0 {
            return None;
        }
        if byte[0] == b'\n' {
            return String::from_utf8(line).ok().map(|line| line.trim_end().to_string());
        }
        line.push(byte[0]);
    }
    None
}

/// Bedient einen einzelnen Client: Anmeldung, dann MCP.
pub async fn handle<S>(mut stream: S, core: Core, approver: Arc<dyn Approver>, observer: Observer)
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let hello = tokio::time::timeout(Duration::from_secs(5), read_line(&mut stream, 256)).await.ok().flatten();
    let parts: Option<(String, String)> = hello.as_deref().and_then(|line| {
        let mut words = line.strip_prefix(HELLO)?.split(' ');
        let (hint, nonce_client) = (words.next()?, words.next()?);
        (words.next().is_none() && is_nonce(nonce_client)).then(|| (hint.to_string(), nonce_client.to_string()))
    });
    let client = parts.as_ref().and_then(|(wanted, _)| {
        let policy = McpPolicy::load(&core).ok()?;
        if !policy.enabled {
            return None;
        }
        policy
            .clients
            .iter()
            .find(|c| !c.disabled && hint(&c.token_sha256) == *wanted)
            .map(|c| (c.id.clone(), c.name.clone(), c.token_sha256.clone()))
    });
    let (Some((_, nonce_client)), Some((client_id, name, token_sha256)), Some(nonce_server)) = (parts, client, nonce())
    else {
        let reason = if core.is_unlocked() { "Token unbekannt oder MCP abgeschaltet" } else { "Tresor ist gesperrt" };
        let _ = stream.write_all(format!("ERR {reason}\n").as_bytes()).await;
        return;
    };
    let ours = proof("dienst", &token_sha256, &nonce_client, &nonce_server);
    if stream.write_all(format!("OK {nonce_server} {ours}\n").as_bytes()).await.is_err() {
        return;
    }
    let theirs = tokio::time::timeout(Duration::from_secs(5), read_line(&mut stream, 256)).await.ok().flatten();
    if theirs.as_deref() != Some(proof("bruecke", &token_sha256, &nonce_client, &nonce_server).as_str()) {
        return;
    }
    // Meldet das Ende auch dann, wenn die Aufgabe mitten in der Sitzung abgebrochen wird.
    struct Gone(Observer, String);
    impl Drop for Gone {
        fn drop(&mut self) {
            (self.0)(ServiceEvent::Disconnected { client: self.1.clone() });
        }
    }
    observer(ServiceEvent::Connected { client: name.clone() });
    let _gone = Gone(observer, name);
    let _ = McpServer::new(core, client_id, approver).serve_stream(stream).await;
}

/// Brückenseite der Anmeldung. Gibt den Strom erst frei, wenn die
/// Gegenstelle bewiesen hat, dass sie der Dienst ist.
pub async fn authenticate<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, token: &str) -> Result<(), String> {
    let token_sha256 = token_hash(token);
    let nonce_client = nonce().ok_or("Zufallsquelle nicht verfügbar")?;
    let hello = format!("{HELLO}{} {nonce_client}\n", hint(&token_sha256));
    stream.write_all(hello.as_bytes()).await.map_err(|e| e.to_string())?;
    let reply = read_line(stream, 256).await.ok_or("Dienst hat die Verbindung geschlossen")?;
    if let Some(reason) = reply.strip_prefix("ERR ") {
        return Err(format!("Dienst hat die Verbindung abgelehnt: {}", printable(reason, 200)));
    }
    let mut words = reply.strip_prefix("OK ").unwrap_or_default().split(' ');
    let (nonce_server, theirs) = (words.next().unwrap_or_default(), words.next().unwrap_or_default());
    if !is_nonce(nonce_server) || theirs != proof("dienst", &token_sha256, &nonce_client, nonce_server) {
        return Err("Die Gegenstelle konnte sich nicht als Dienst ausweisen – Verbindung abgebrochen.".into());
    }
    let ours = proof("bruecke", &token_sha256, &nonce_client, nonce_server);
    stream.write_all(format!("{ours}\n").as_bytes()).await.map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ Kanal

/// Der geöffnete Kanal des Dienstes. Solange er besteht, kann kein zweiter
/// Dienst für denselben Tresor starten.
pub struct Listener {
    endpoint: String,
    #[cfg(windows)]
    first: tokio::net::windows::named_pipe::NamedPipeServer,
    #[cfg(not(windows))]
    socket: tokio::net::UnixListener,
}

impl Listener {
    /// Öffnet den Kanal für den Tresor unter `vault`. Scheitert, wenn für
    /// ihn schon ein Dienst läuft.
    #[cfg(windows)]
    pub async fn bind(vault: &Path) -> Result<Self, String> {
        let endpoint = endpoint(vault);
        // `first_pipe_instance` verhindert, dass ein anderes Programm den Namen vorab besetzt.
        let first = tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&endpoint)
            .map_err(|e| format!("Der Dienst lässt sich nicht starten (läuft er schon?): {e}"))?;
        Ok(Self { endpoint, first })
    }

    /// Öffnet den Kanal für den Tresor unter `vault`. Scheitert, wenn für
    /// ihn schon ein Dienst läuft.
    #[cfg(not(windows))]
    pub async fn bind(vault: &Path) -> Result<Self, String> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let endpoint = endpoint(vault);
        let path = Path::new(&endpoint);
        if let Some(dir) = path.parent() {
            // Das Verzeichnis gehört nur dem Benutzer; der Socket darin ist damit
            // schon im Augenblick seiner Entstehung geschützt.
            let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir);
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| format!("Verzeichnis für den Dienst lässt sich nicht absichern: {e}"))?;
        }
        if path.exists() {
            if tokio::net::UnixStream::connect(&endpoint).await.is_ok() {
                return Err("Der Dienst läuft bereits.".into());
            }
            // Überbleibsel eines früheren Laufs.
            let _ = std::fs::remove_file(&endpoint);
        }
        let socket = tokio::net::UnixListener::bind(&endpoint)
            .map_err(|e| format!("Der Dienst lässt sich nicht starten: {e}"))?;
        std::fs::set_permissions(&endpoint, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
        Ok(Self { endpoint, socket })
    }

    /// Bedient Clients, bis ein Fehler auftritt oder die Aufgabe abgebrochen
    /// wird. Mit ihr enden auch alle Sitzungen der verbundenen Clients.
    #[cfg(windows)]
    pub async fn serve(self, core: Core, approver: Arc<dyn Approver>, observer: Observer) -> Result<(), String> {
        use tokio::net::windows::named_pipe::ServerOptions;
        let mut clients = JoinSet::new();
        let mut server = self.first;
        loop {
            server.connect().await.map_err(|e| e.to_string())?;
            let connected = server;
            server = ServerOptions::new()
                .reject_remote_clients(true)
                .create(&self.endpoint)
                .map_err(|e| format!("Der Dienst kann keine weiteren Clients annehmen: {e}"))?;
            while clients.try_join_next().is_some() {}
            clients.spawn(handle(connected, core.clone(), approver.clone(), observer.clone()));
        }
    }

    /// Bedient Clients, bis ein Fehler auftritt oder die Aufgabe abgebrochen
    /// wird. Mit ihr enden auch alle Sitzungen der verbundenen Clients.
    #[cfg(not(windows))]
    pub async fn serve(self, core: Core, approver: Arc<dyn Approver>, observer: Observer) -> Result<(), String> {
        // Räumt den Socket auch dann weg, wenn die Aufgabe abgebrochen wird.
        struct Remove(String);
        impl Drop for Remove {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _remove = Remove(self.endpoint.clone());
        let mut clients = JoinSet::new();
        loop {
            let (stream, _) = self.socket.accept().await.map_err(|e| e.to_string())?;
            while clients.try_join_next().is_some() {}
            clients.spawn(handle(stream, core.clone(), approver.clone(), observer.clone()));
        }
    }
}

/// Seite der Brücke: verbindet sich mit dem Dienst für den Tresor unter
/// `vault` und weist sich mit dem Token aus. Zurück kommt der Strom, über
/// den danach MCP läuft.
pub async fn connect(vault: &Path, token: &str) -> Result<impl AsyncRead + AsyncWrite + Send + Unpin, String> {
    let endpoint = endpoint(vault);
    let not_running = |e: std::io::Error| {
        format!(
            "Der Dienst ist nicht erreichbar ({e}). Bitte zuerst „tephra daemon“ starten oder in der App den MCP-Dienst einschalten, und den Tresor entsperren."
        )
    };
    #[cfg(windows)]
    let mut stream = tokio::net::windows::named_pipe::ClientOptions::new().open(&endpoint).map_err(not_running)?;
    #[cfg(not(windows))]
    let mut stream = tokio::net::UnixStream::connect(&endpoint).await.map_err(not_running)?;
    authenticate(&mut stream, token).await?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DenyAll;
    use std::sync::Mutex;
    use tephra_core::KdfParams;

    fn quiet() -> Observer {
        Arc::new(|_| {})
    }

    fn vault_with_client() -> (tempfile::TempDir, Core, String) {
        let dir = tempfile::tempdir().expect("tmp");
        let (core, _) =
            Core::create(&dir.path().join("t.tephra"), "pw", KdfParams::INSECURE_FOR_TESTS).expect("tresor");
        let mut policy = McpPolicy::default();
        let (_, token) = policy.add_client("Test").expect("client");
        policy.save(&core).expect("speichern");
        (dir, core, token)
    }

    #[tokio::test]
    async fn bruecke_und_dienst_weisen_sich_gegenseitig_aus() {
        let (_dir, core, token) = vault_with_client();
        let connect = |core: Core| {
            let (client, server) = tokio::io::duplex(4096);
            tokio::spawn(handle(server, core, Arc::new(DenyAll), quiet()));
            client
        };
        authenticate(&mut connect(core.clone()), &token).await.expect("anmelden");
        let wrong = authenticate(&mut connect(core.clone()), "tephramcp_falsch").await.expect_err("abgelehnt");
        assert!(wrong.contains("abgelehnt"), "{wrong}");

        // Not-Aus und gesperrter Tresor weisen auch ein gültiges Token ab.
        let mut off = McpPolicy::load(&core).expect("laden");
        off.enabled = false;
        off.save(&core).expect("speichern");
        assert!(authenticate(&mut connect(core.clone()), &token).await.is_err());
        core.lock().await;
        let locked = authenticate(&mut connect(core.clone()), &token).await.expect_err("gesperrt");
        assert!(locked.contains("gesperrt"), "{locked}");
    }

    /// Ein Programm, das nur den Namen des Kanals besetzt hat, lernt das
    /// Token nicht und bekommt keine MCP-Daten.
    #[tokio::test]
    async fn falscher_dienst_erfaehrt_nichts_und_wird_erkannt() {
        let (_dir, _core, token) = vault_with_client();
        let (mut bridge_side, mut impostor) = tokio::io::duplex(4096);
        let seen = tokio::spawn(async move {
            let hello = read_line(&mut impostor, 256).await.expect("hello");
            // Der Hochstapler antwortet mit etwas, das wie eine Zusage aussieht.
            impostor.write_all(format!("OK {} {}\n", "0".repeat(32), "0".repeat(64)).as_bytes()).await.expect("senden");
            let mut rest = Vec::new();
            let _ = impostor.read_to_end(&mut rest).await;
            (hello, rest)
        });
        let error = authenticate(&mut bridge_side, &token).await.expect_err("erkannt");
        assert!(error.contains("nicht als Dienst ausweisen"), "{error}");
        drop(bridge_side);
        let (hello, rest) = seen.await.expect("task");
        assert!(!hello.contains(&token) && !hello.contains(&token_hash(&token)), "Token oder Hash wurde preisgegeben");
        assert!(rest.is_empty(), "Brücke hat nach gescheiterter Prüfung noch gesendet");
    }

    #[tokio::test]
    async fn unsinnige_begruessungen_werden_abgewiesen() {
        let (_dir, core, _token) = vault_with_client();
        for hello in [
            "GET / HTTP/1.1\n".to_string(),
            "TEPHRA-MCP 2 abc\n".into(),
            "TEPHRA-MCP 1 tephramcp_x\n".into(),
            "x".repeat(1000),
        ] {
            let (mut client, server) = tokio::io::duplex(4096);
            tokio::spawn(handle(server, core.clone(), Arc::new(DenyAll), quiet()));
            client.write_all(hello.as_bytes()).await.expect("senden");
            let reply = read_line(&mut client, 256).await.expect("antwort");
            assert!(reply.starts_with("ERR"), "{hello:?} → {reply}");
        }
    }

    #[test]
    fn kanalname_haengt_vom_tresor_ab() {
        let a = endpoint(Path::new("/tmp/a.tephra"));
        assert_ne!(a, endpoint(Path::new("/tmp/b.tephra")));
        assert!(a.contains("tephra-"));
    }

    /// Der Dienst über den echten Kanal: Ein zweiter Dienst für denselben
    /// Tresor startet nicht, die Brücke kommt durch, und mit dem Dienst
    /// enden die Verbindungen seiner Clients.
    #[tokio::test]
    async fn dienst_nimmt_die_bruecke_an_und_endet_mit_seinen_clients() {
        let (dir, core, token) = vault_with_client();
        let vault = dir.path().join("t.tephra");
        let listener = Listener::bind(&vault).await.expect("kanal");
        assert!(Listener::bind(&vault).await.is_err(), "ein zweiter Dienst darf nicht starten");

        let seen: Arc<Mutex<Vec<ServiceEvent>>> = Arc::default();
        let log = seen.clone();
        let observer: Observer = Arc::new(move |event| log.lock().expect("log").push(event));
        let service = tokio::spawn(listener.serve(core.clone(), Arc::new(DenyAll), observer));

        assert!(connect(&vault, "tephramcp_falsch").await.is_err());
        let mut bridge = connect(&vault, &token).await.expect("verbinden");
        let connected = ServiceEvent::Connected { client: "Test".into() };
        for _ in 0..100 {
            if seen.lock().expect("log").contains(&connected) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(*seen.lock().expect("log"), vec![connected.clone()]);

        service.abort();
        let _ = service.await;
        // Die Verbindung des Clients ist mit dem Dienst zu Ende.
        let mut rest = Vec::new();
        let closed = tokio::time::timeout(Duration::from_secs(5), bridge.read_to_end(&mut rest)).await;
        assert!(closed.is_ok(), "die Verbindung blieb nach dem Ende des Dienstes offen");
        assert_eq!(*seen.lock().expect("log"), vec![connected, ServiceEvent::Disconnected { client: "Test".into() }]);
        // Danach ist der Kanal wieder frei.
        drop(Listener::bind(&vault).await.expect("kanal ist wieder frei"));
    }
}
