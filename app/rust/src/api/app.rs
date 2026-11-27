//! Schnittstelle zwischen Oberfläche und Kern.
//!
//! Alles, was die Oberfläche tun kann, läuft über die Funktionen dieses
//! Moduls. Geheimnisse bleiben im Kern: Die Oberfläche bekommt Passwörter und
//! private Schlüssel nie zurück, sie kann sie nur setzen.

use crate::frb_generated::StreamSink;
use crate::internal::{
    answer, app, core, next_id, optional_id, parse_id, run_session, session, store_key, Answer, RUNTIME,
};
use anyhow::{anyhow, bail, Context, Result};
use flutter_rust_bridge::frb;
use std::path::PathBuf;
use std::time::Duration;
use tephra_core::model::{AuthMethod, Host, Identity, SshKey, VpnProfile};
use tephra_core::ssh::keys;
use tephra_core::transport::wireguard;
use tephra_core::{Core, KdfParams, NonInteractive};

#[frb(init)]
pub fn init_app() {
    // Rücklaufspuren bleiben aus, auch wenn die Umgebung sie verlangt. Unter Linux bringt
    // die Flutter-Bibliothek einen eigenen Stack-Abwickler mit, der sich mit dem von Rust
    // nicht verträgt: Schon das Erfassen einer Spur – etwa für die Fehlermeldung nach
    // einem falschen Passwort – ließe die App abstürzen.
    std::env::set_var("RUST_BACKTRACE", "0");
    std::env::set_var("RUST_LIB_BACKTRACE", "0");
}

// ------------------------------------------------------------------ Tresor

/// Ort der Tresordatei: aus `TEPHRA_VAULT`, sonst nach den Gepflogenheiten des
/// Systems – derselbe Ort, den auch das Kommandozeilenprogramm verwendet.
/// Unter Android gibt die Oberfläche ihren eigenen Datenordner vor.
#[frb(sync)]
pub fn default_vault_path() -> String {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(path) = env("TEPHRA_VAULT") {
        return path.to_string_lossy().into_owned();
    }
    let dir = if cfg!(windows) {
        env("APPDATA").map(|dir| dir.join("Tephra").join("data"))
    } else {
        env("XDG_DATA_HOME")
            .or_else(|| env("HOME").map(|home| home.join(".local").join("share")))
            .map(|dir| dir.join("tephra"))
    };
    dir.unwrap_or_default().join("vault.tephra").to_string_lossy().into_owned()
}

#[frb(sync)]
pub fn vault_exists(path: String) -> bool {
    PathBuf::from(path).exists()
}

#[frb(sync)]
pub fn is_unlocked() -> bool {
    app().core.as_ref().is_some_and(Core::is_unlocked)
}

/// Legt einen neuen Tresor an und gibt den Wiederherstellungsschlüssel
/// zurück. Er wird nirgends gespeichert und muss jetzt angezeigt werden.
pub async fn create_vault(path: String, password: String) -> Result<String> {
    if password.chars().count() < 8 {
        bail!("Das Master-Passwort muss mindestens 8 Zeichen lang sein.");
    }
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Der Datenordner lässt sich nicht anlegen")?;
    }
    // Die Passwortableitung braucht bewusst Zeit; sie läuft abseits der Oberfläche.
    let (core, recovery) = RUNTIME.spawn_blocking(move || Core::create(&path, &password, KdfParams::DEFAULT)).await??;
    app().core = Some(core);
    Ok(recovery.to_display_string())
}

pub async fn unlock_vault(path: String, password: String) -> Result<()> {
    let existing = app().core.clone();
    let core = RUNTIME
        .spawn_blocking(move || -> Result<Core> {
            let core = match existing {
                Some(core) => core,
                None => Core::open(&PathBuf::from(path))?,
            };
            core.unlock(&password)?;
            Ok(core)
        })
        .await??;
    app().core = Some(core);
    Ok(())
}

/// Ob das Master-Passwort zum offenen Tresor passt – vor Schritten, die sich nicht rückgängig machen lassen.
#[frb(sync)]
pub fn verify_master_password(password: String) -> Result<bool> {
    let header = core()?.with_vault(|v| Ok(v.header().clone()))?;
    Ok(header.open_with_password(&password).is_ok())
}

/// Löscht den Tresor von diesem Gerät: sperrt ihn, beendet alle Verbindungen und entfernt die Tresordatei samt
/// Begleitdateien. Was auf einem Sync-Server liegt, bleibt dort; was nur hier lag, ist weg.
pub async fn delete_vault(path: String, password: String) -> Result<()> {
    if !verify_master_password(password)? {
        bail!("Das Master-Passwort stimmt nicht");
    }
    lock_vault().await?;
    app().core = None;
    let path = PathBuf::from(path);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    for suffix in ["", "-wal", "-shm", "-journal", ".device-unlock"] {
        let file = path.with_file_name(format!("{name}{suffix}"));
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(anyhow!("{} lässt sich nicht löschen: {error}", file.display())),
        }
    }
    Ok(())
}

/// Sperrt den Tresor und beendet alle Verbindungen.
pub async fn lock_vault() -> Result<()> {
    let core = {
        let mut app = app();
        app.sessions.clear();
        app.pending.clear();
        for (_, task) in app.tasks.drain() {
            task.abort();
        }
        for (_, task) in app.transfers.drain() {
            task.abort();
        }
        if let Some(task) = app.sync.take() {
            task.abort();
        }
        app.files.clear();
        crate::api::mcp::mcp_service_stop();
        app.core.clone()
    };
    if let Some(core) = core {
        RUNTIME.spawn(async move { core.lock().await }).await?;
    }
    Ok(())
}

// ------------------------------------------------------------------ Hosts

pub struct HostView {
    pub id: String,
    pub label: String,
    pub address: String,
    pub port: u16,
    /// Benutzername der zugewiesenen (oder geerbten) Identität.
    pub username: String,
    pub identity_id: Option<String>,
    pub vpn_profile_id: Option<String>,
    pub jump_host_ids: Vec<String>,
    pub tags: Vec<String>,
    pub notes: String,
    pub favorite: bool,
    /// Befehl, der nach dem Anmelden statt der Login-Shell läuft.
    pub startup_command: String,
    /// Terminalsitzungen laufen auf dem Host in tmux und überstehen einen Verbindungsabbruch.
    pub keep_session: bool,
}

pub struct HostInput {
    /// Leer für einen neuen Host.
    pub id: Option<String>,
    pub label: String,
    pub address: String,
    pub port: u16,
    pub identity_id: Option<String>,
    pub vpn_profile_id: Option<String>,
    pub jump_host_ids: Vec<String>,
    pub tags: Vec<String>,
    pub notes: String,
    pub favorite: bool,
    pub startup_command: String,
    pub keep_session: bool,
}

#[frb(sync)]
pub fn list_hosts() -> Result<Vec<HostView>> {
    let core = core()?;
    let mut hosts = Vec::new();
    for (id, host) in core.with_vault(|v| v.list::<Host>())? {
        let username = core.resolve_host(id).ok().and_then(|r| r.identity).map(|i| i.username).unwrap_or_default();
        hosts.push(HostView {
            id: id.to_string(),
            label: host.label,
            address: host.address,
            port: host.port,
            username,
            identity_id: host.identity_id.map(|id| id.to_string()),
            vpn_profile_id: host.vpn_profile_id.map(|id| id.to_string()),
            jump_host_ids: host.jump_host_ids.iter().map(ToString::to_string).collect(),
            tags: host.tags,
            notes: host.notes,
            favorite: host.favorite,
            startup_command: host.startup_command.unwrap_or_default(),
            keep_session: host.keep_session,
        });
    }
    hosts.sort_by(|a, b| b.favorite.cmp(&a.favorite).then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase())));
    Ok(hosts)
}

/// Legt einen Host an oder ändert ihn; gibt seine Kennung zurück.
#[frb(sync)]
pub fn save_host(input: HostInput) -> Result<String> {
    let core = core()?;
    if input.label.trim().is_empty() || input.address.trim().is_empty() {
        bail!("Bezeichnung und Adresse dürfen nicht leer sein.");
    }
    let existing = optional_id(&input.id)?;
    // Felder, die die Oberfläche nicht kennt (Proxy, Weiterleitungen …), bleiben erhalten.
    let mut host = match existing {
        Some(id) => core.with_vault(|v| v.get::<Host>(id))?.ok_or_else(|| anyhow!("Host nicht gefunden"))?,
        None => Host::default(),
    };
    host.label = input.label.trim().to_string();
    host.address = input.address.trim().to_string();
    host.port = input.port;
    host.identity_id = optional_id(&input.identity_id)?;
    host.vpn_profile_id = optional_id(&input.vpn_profile_id)?;
    host.jump_host_ids = input.jump_host_ids.iter().map(|id| parse_id(id)).collect::<Result<_>>()?;
    host.tags = input.tags.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    host.notes = input.notes;
    host.favorite = input.favorite;
    host.startup_command = Some(input.startup_command.trim().to_string()).filter(|c| !c.is_empty());
    host.keep_session = input.keep_session;
    let id = match existing {
        Some(id) => {
            core.with_vault(|v| v.update(id, &host))?;
            id
        }
        None => core.with_vault(|v| v.insert(&host))?,
    };
    Ok(id.to_string())
}

#[frb(sync)]
pub fn delete_item(id: String) -> Result<()> {
    let id = parse_id(&id)?;
    Ok(core()?.with_vault(|v| v.delete(id))?)
}

// ------------------------------------------------------------------ Identitäten

pub struct IdentityView {
    pub id: String,
    pub label: String,
    pub username: String,
    /// `password`, `key`, `ask` oder `agent`.
    pub method: String,
    pub key_id: Option<String>,
}

pub struct IdentityInput {
    pub id: Option<String>,
    pub label: String,
    pub username: String,
    /// `password`, `key` oder `ask`.
    pub method: String,
    /// Neues Passwort; leer lässt ein bereits gespeichertes unverändert.
    pub password: Option<String>,
    pub key_id: Option<String>,
}

#[frb(sync)]
pub fn list_identities() -> Result<Vec<IdentityView>> {
    let mut identities: Vec<IdentityView> = core()?
        .with_vault(|v| v.list::<Identity>())?
        .into_iter()
        .map(|(id, identity)| {
            let (method, key_id) = match &identity.auth {
                AuthMethod::Ask => ("ask", None),
                AuthMethod::Password { .. } => ("password", None),
                AuthMethod::Key { key_id } => ("key", Some(key_id.to_string())),
                AuthMethod::Agent => ("agent", None),
            };
            IdentityView {
                id: id.to_string(),
                label: identity.label,
                username: identity.username,
                method: method.into(),
                key_id,
            }
        })
        .collect();
    identities.sort_by_key(|i| i.label.to_lowercase());
    Ok(identities)
}

#[frb(sync)]
pub fn save_identity(input: IdentityInput) -> Result<String> {
    let core = core()?;
    if input.username.trim().is_empty() {
        bail!("Der Benutzername darf nicht leer sein.");
    }
    let existing = optional_id(&input.id)?;
    let previous = match existing {
        Some(id) => core.with_vault(|v| v.get::<Identity>(id))?,
        None => None,
    };
    let auth = match input.method.as_str() {
        "ask" => AuthMethod::Ask,
        "key" => AuthMethod::Key {
            key_id: optional_id(&input.key_id)?.ok_or_else(|| anyhow!("Bitte einen Schlüssel auswählen."))?,
        },
        "password" => match (input.password.filter(|p| !p.is_empty()), previous.map(|p| p.auth)) {
            (Some(password), _) => AuthMethod::Password { password: password.into() },
            (None, Some(AuthMethod::Password { password })) => AuthMethod::Password { password },
            (None, _) => bail!("Bitte ein Passwort eingeben."),
        },
        other => bail!("Unbekanntes Anmeldeverfahren: {other}"),
    };
    let label = if input.label.trim().is_empty() { input.username.trim() } else { input.label.trim() };
    let identity = Identity { label: label.to_string(), username: input.username.trim().to_string(), auth };
    let id = match existing {
        Some(id) => {
            core.with_vault(|v| v.update(id, &identity))?;
            id
        }
        None => core.with_vault(|v| v.insert(&identity))?,
    };
    Ok(id.to_string())
}

// ------------------------------------------------------------------ Schlüssel

pub struct KeyView {
    pub id: String,
    pub label: String,
    /// z. B. `ssh-ed25519`.
    pub algorithm: String,
    /// Öffentlicher Schlüssel zum Eintragen auf Servern.
    pub public_key: String,
    pub local_only: bool,
}

#[frb(sync)]
pub fn list_keys() -> Result<Vec<KeyView>> {
    let core = core()?;
    let mut out = Vec::new();
    for (id, key) in core.with_vault(|v| v.list::<SshKey>())? {
        out.push(KeyView {
            id: id.to_string(),
            algorithm: key.public_key.split_whitespace().next().unwrap_or_default().to_string(),
            label: key.label,
            public_key: key.public_key,
            local_only: core.with_vault(|v| v.is_local_only(id))?,
        });
    }
    out.sort_by_key(|k| k.label.to_lowercase());
    Ok(out)
}

/// Erzeugt ein neues Ed25519-Schlüsselpaar.
#[frb(sync)]
pub fn generate_key(label: String, local_only: bool) -> Result<String> {
    if label.trim().is_empty() {
        bail!("Die Bezeichnung darf nicht leer sein.");
    }
    let (pem, info) = keys::generate_ed25519(label.trim())?;
    let key = SshKey {
        label: label.trim().to_string(),
        private_key: pem.into(),
        public_key: info.public_key,
        ..SshKey::default()
    };
    store_key(&key, local_only)
}

/// Übernimmt einen vorhandenen privaten Schlüssel (OpenSSH- oder PEM-Format).
#[frb(sync)]
pub fn import_key(label: String, pem: String, passphrase: Option<String>, local_only: bool) -> Result<String> {
    if label.trim().is_empty() {
        bail!("Die Bezeichnung darf nicht leer sein.");
    }
    // Abgelegt wird der Schlüssel im OpenSSH-Format und ohne Passphrase; der Tresor verschlüsselt ihn.
    let (openssh, info) = keys::import(&pem, passphrase.as_deref())?;
    let key = SshKey {
        label: label.trim().to_string(),
        private_key: openssh.into(),
        public_key: info.public_key,
        ..SshKey::default()
    };
    store_key(&key, local_only)
}

/// Trägt den öffentlichen Teil eines Schlüssels auf einem Host in
/// `~/.ssh/authorized_keys` ein. `false`: Er stand dort schon.
///
/// Angemeldet wird über die Verbindung, die zu dem Host schon steht; sonst
/// ohne Rückfragen mit dessen gespeicherter Anmeldung.
pub async fn install_key_on_host(key_id: String, host_id: String) -> Result<bool> {
    let (key_id, host_id) = (parse_id(&key_id)?, parse_id(&host_id)?);
    let core = core()?;
    RUNTIME
        .spawn(async move {
            let key =
                core.with_vault(|v| v.get::<SshKey>(key_id))?.ok_or_else(|| anyhow!("Schlüssel nicht gefunden"))?;
            let client = core.connect_pooled(host_id, std::sync::Arc::new(NonInteractive)).await?;
            Ok(client.authorize_key(&key.public_key).await?)
        })
        .await?
}

// ------------------------------------------------------------------ VPN-Profile

pub struct VpnView {
    pub id: String,
    pub label: String,
    pub addresses: Vec<String>,
    pub endpoints: Vec<String>,
    pub dns: Vec<String>,
}

#[frb(sync)]
pub fn list_vpn_profiles() -> Result<Vec<VpnView>> {
    let mut profiles: Vec<VpnView> = core()?
        .with_vault(|v| v.list::<VpnProfile>())?
        .into_iter()
        .map(|(id, profile)| VpnView {
            id: id.to_string(),
            label: profile.label,
            addresses: profile.addresses,
            endpoints: profile.peers.into_iter().map(|p| p.endpoint).collect(),
            dns: profile.dns,
        })
        .collect();
    profiles.sort_by_key(|p| p.label.to_lowercase());
    Ok(profiles)
}

/// Liest eine WireGuard-Konfiguration im `wg-quick`-Format ein.
#[frb(sync)]
pub fn import_vpn_profile(label: String, conf: String) -> Result<String> {
    if label.trim().is_empty() {
        bail!("Die Bezeichnung darf nicht leer sein.");
    }
    let profile = wireguard::parse_conf(&conf, label.trim())?;
    Ok(core()?.with_vault(|v| v.insert(&profile))?.to_string())
}

/// Baut den Tunnel eines Profils auf und meldet, ob der Handshake gelingt.
pub async fn test_vpn_profile(id: String) -> Result<String> {
    let id = parse_id(&id)?;
    let core = core()?;
    RUNTIME
        .spawn(async move {
            let profile =
                core.with_vault(|v| v.get::<VpnProfile>(id))?.ok_or_else(|| anyhow!("VPN-Profil nicht gefunden"))?;
            let target = profile
                .peers
                .iter()
                .flat_map(|p| &p.allowed_ips)
                .find_map(|net| wireguard::IpNet::parse(net))
                .map(|net| net.addr)
                .ok_or_else(|| anyhow!("Das Profil enthält keine AllowedIPs."))?;
            let tunnel = core.tunnel(id).await?;
            tunnel.wait_ready(target, Duration::from_secs(10)).await?;
            let status = tunnel.status();
            let rtt = status.first().and_then(|p| p.rtt_ms).map(|ms| format!(" ({ms} ms)")).unwrap_or_default();
            Ok(format!("Handshake erfolgreich{rtt}"))
        })
        .await?
}

// ------------------------------------------------------------------ Sitzungen

pub struct PromptView {
    pub text: String,
    /// Ob die Eingabe sichtbar sein darf.
    pub echo: bool,
}

/// Was in einer Terminalsitzung geschieht.
pub enum SessionEvent {
    /// Die Sitzung wurde angelegt; unter dieser Kennung lässt sie sich
    /// ansprechen und auch während des Verbindungsaufbaus abbrechen.
    Opened { session_id: u32 },
    /// Die Verbindung steht; es folgt Ausgabe.
    Connected { description: String },
    /// Ausgabe für das Terminal.
    Output { data: Vec<u8> },
    /// Rückfrage: Soll dem Hostschlüssel vertraut werden?
    /// Antwort über [`answer_decision`].
    HostKey {
        request_id: u32,
        host: String,
        port: u16,
        algorithm: String,
        fingerprint: String,
        /// Zuvor bekannte Fingerabdrücke; nicht leer heißt: Der Schlüssel hat sich geändert.
        known_fingerprints: Vec<String>,
    },
    /// Rückfrage nach einem Passwort. Antwort über [`answer_text`].
    Password { request_id: u32, prompt: String },
    /// Rückfragen des Servers (z. B. Einmalcode). Antwort über [`answer_texts`].
    ServerPrompts { request_id: u32, name: String, instructions: String, prompts: Vec<PromptView> },
    /// Die Shell hat sich beendet.
    Exit { code: Option<u32> },
    /// Verbindungsaufbau oder Sitzung sind gescheitert.
    Failed { message: String },
    /// Die Sitzung ist zu Ende; es folgen keine Ereignisse mehr.
    Closed,
}

#[frb(sync)]
pub fn answer_decision(request_id: u32, accept: bool) {
    answer(request_id, Answer::Decision(accept));
}

#[frb(sync)]
pub fn answer_text(request_id: u32, text: Option<String>) {
    answer(request_id, Answer::Text(text));
}

#[frb(sync)]
pub fn answer_texts(request_id: u32, texts: Option<Vec<String>>) {
    answer(request_id, Answer::Texts(texts));
}

/// Öffnet eine Terminalsitzung zu einem Host. Die Verbindung wird im
/// Hintergrund aufgebaut; alles Weitere kommt als Ereignis, zuerst
/// [`SessionEvent::Opened`] mit der Kennung der Sitzung.
///
/// `slot` unterscheidet mehrere Terminals zum selben Host (ab 1); bei Hosts mit „Sitzung halten“ hängt jedes an
/// seiner eigenen tmux-Sitzung, und dieselbe Nummer findet sie nach einem Abbruch wieder.
pub fn open_session(host_id: String, slot: u32, cols: u32, rows: u32, sink: StreamSink<SessionEvent>) -> Result<()> {
    let host_id = parse_id(&host_id)?;
    let core = core()?;
    let session_id = next_id();
    let sink = std::sync::Arc::new(sink);
    let _ = sink.add(SessionEvent::Opened { session_id });
    let task = RUNTIME.spawn(async move {
        let result = run_session(core, host_id, session_id, slot, cols, rows, &sink).await;
        {
            let mut app = app();
            app.sessions.remove(&session_id);
            app.tasks.remove(&session_id);
        }
        if let Err(error) = result {
            let _ = sink.add(SessionEvent::Failed { message: error.to_string() });
        }
        let _ = sink.add(SessionEvent::Closed);
    });
    app().tasks.insert(session_id, task.abort_handle());
    Ok(())
}

/// Sendet Tastatureingaben an eine Sitzung.
pub async fn session_write(session_id: u32, data: Vec<u8>) -> Result<()> {
    let input = session(session_id)?;
    RUNTIME.spawn(async move { input.send(&data).await }).await??;
    Ok(())
}

pub async fn session_resize(session_id: u32, cols: u32, rows: u32) -> Result<()> {
    let input = session(session_id)?;
    RUNTIME.spawn(async move { input.resize(cols, rows).await }).await??;
    Ok(())
}

/// Prüft, ob die Verbindung einer Sitzung noch antwortet, etwa nach einem Netzwechsel: `false`, wenn sie
/// nicht binnen `timeout_ms` antwortet oder die Sitzung schon beendet ist.
pub async fn session_check(session_id: u32, timeout_ms: u32) -> Result<bool> {
    let Some(client) = app().sessions.get(&session_id).map(|(_, client)| client.clone()) else {
        return Ok(false);
    };
    let timeout = std::time::Duration::from_millis(u64::from(timeout_ms));
    Ok(RUNTIME.spawn(async move { client.ping(timeout).await }).await?)
}

pub async fn session_close(session_id: u32) -> Result<()> {
    let (input, task) = {
        let mut app = app();
        (app.sessions.remove(&session_id), app.tasks.remove(&session_id))
    };
    if let Some(input) = input {
        let _ = RUNTIME.spawn(async move { input.0.close().await }).await;
    }
    // Bricht auch einen Verbindungsaufbau ab, der noch auf eine Antwort wartet.
    if let Some(task) = task {
        task.abort();
    }
    Ok(())
}
