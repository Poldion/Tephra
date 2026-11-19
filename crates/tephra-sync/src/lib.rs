//! Abgleich des Tresors mit dem Sync-Server.
//!
//! Der Client holt Änderungen seit seinem letzten Stand ab, führt sie mit
//! lokalen Änderungen zusammen und lädt hoch, was dem Server fehlt. Alles,
//! was den Server erreicht, ist bereits verschlüsselt.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tephra_core::model::ItemId;
use tephra_core::{Core, CoreError, KdfParams, Vault, VaultError};
use tephra_crypto::MasterKey;
use tephra_sync_proto::{
    Bytes, ChangePasswordRequest, DeviceInfo, ErrorBody, EventsResponse, HeaderResponse, LoginRequest, LoginResponse,
    MAX_BATCH, MAX_WAIT_SECS, PreloginRequest, PreloginResponse, PullResponse, PushItem, PushOutcome, PushRequest,
    PushResponse, RefreshRequest, RegisterRequest, Session, codes,
};
use tephra_vault::{ApplyOutcome, VaultHeader};
use tokio::sync::watch;
use uuid::Uuid;

const SESSION_SECRET: &str = "sync/session";
/// Höchste Datenmenge einer Hochlade-Anfrage (vor der Base64-Kodierung).
const MAX_PUSH_BYTES: usize = 8 * 1024 * 1024;
/// Runden, in denen Konflikte zusammengeführt und erneut hochgeladen werden.
const MAX_ROUNDS: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("Dieses Gerät ist mit keinem Sync-Server verbunden")]
    NotConnected,
    #[error("Dieses Gerät ist bereits mit einem Sync-Server verbunden")]
    AlreadyConnected,
    #[error("Die Sitzung ist abgelaufen oder das Gerät wurde entfernt; bitte neu anmelden")]
    SessionExpired,
    #[error("Unverschlüsselte Verbindungen sind nur zum eigenen Rechner erlaubt – bitte eine https-Adresse verwenden")]
    InsecureUrl,
    #[error("Ungültige Serveradresse: {0}")]
    InvalidUrl(String),
    #[error("Server nicht erreichbar: {0}")]
    Network(String),
    #[error("{message}")]
    Server { code: String, message: String },
    #[error("Unerwartete Antwort des Servers: {0}")]
    Protocol(String),
    #[error(transparent)]
    Core(#[from] CoreError),
}

impl From<VaultError> for SyncError {
    fn from(e: VaultError) -> Self {
        SyncError::Core(e.into())
    }
}

pub type Result<T> = std::result::Result<T, SyncError>;

/// Verbindung dieses Geräts zum Sync-Server; liegt verschlüsselt im Tresor
/// und wird nie abgeglichen.
#[derive(Clone, Serialize, Deserialize)]
struct Stored {
    server: String,
    username: String,
    account_id: Uuid,
    device_id: Uuid,
    access_token: String,
    refresh_token: String,
    /// Stand des Tresorkopfes, den dieses Gerät zuletzt übernommen hat.
    header_rev: i64,
}

/// Auskunft über die Verbindung, ohne Tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub server: String,
    pub username: String,
    pub device_id: Uuid,
}

/// Ergebnis eines Abgleichs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Vom Server übernommene Datensätze.
    pub pulled: usize,
    /// Zum Server hochgeladene Datensätze.
    pub pushed: usize,
    /// Datensätze, bei denen beide Seiten Änderungen hatten.
    pub merged: usize,
    /// Datensätze, die sich nicht entschlüsseln ließen und übersprungen wurden.
    pub unreadable: usize,
    /// Vom Server abgelehnte Datensätze mit Begründung.
    pub rejected: Vec<(ItemId, String)>,
}

#[derive(Clone)]
pub struct Sync {
    core: Core,
    http: reqwest::Client,
    /// Sorgt dafür, dass immer nur eine Aufgabe die Sitzung verlängert.
    refreshing: std::sync::Arc<tokio::sync::Mutex<()>>,
}

fn normalize_url(server: &str) -> Result<String> {
    let server = server.trim().trim_end_matches('/');
    let url = reqwest::Url::parse(server).map_err(|e| SyncError::InvalidUrl(e.to_string()))?;
    let host = url.host_str().unwrap_or_default();
    let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    match url.scheme() {
        "https" => Ok(server.to_string()),
        "http" if local => Ok(server.to_string()),
        "http" => Err(SyncError::InsecureUrl),
        other => Err(SyncError::InvalidUrl(format!("Schema „{other}“ wird nicht unterstützt"))),
    }
}

/// Wurzelzertifikate (DER), denen Verbindungen zum Server vertrauen, wenn das
/// Programm sie nicht selbst beim Betriebssystem erfragen kann.
static TRUST_ROOTS: std::sync::RwLock<Option<Vec<Vec<u8>>>> = std::sync::RwLock::new(None);

/// Legt fest, welchen Wurzelzertifikaten Verbindungen zum Sync-Server
/// vertrauen; sie ersetzen den Zertifikatsspeicher des Betriebssystems. Nötig
/// unter Android: Dort kennt nur die App selbst die Zertifikate des Systems
/// und des Nutzers und reicht sie hierher weiter. Unlesbare Einträge werden
/// übergangen; zurück kommt die Zahl der brauchbaren.
pub fn set_trust_roots(certificates: Vec<Vec<u8>>) -> usize {
    let mut store = rustls::RootCertStore::empty();
    let usable: Vec<Vec<u8>> = certificates
        .into_iter()
        .filter(|der| store.add(rustls::pki_types::CertificateDer::from(der.as_slice())).is_ok())
        .collect();
    let count = usable.len();
    *TRUST_ROOTS.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(usable);
    count
}

fn http_client() -> reqwest::Client {
    // Alle Verbindungen nutzen dieselbe Krypto-Bibliothek wie der Rest des Programms.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(MAX_WAIT_SECS + 20))
        .user_agent(concat!("Tephra/", env!("CARGO_PKG_VERSION")));
    let roots = TRUST_ROOTS.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    let builder = match roots {
        Some(roots) => builder.tls_certs_only(roots.iter().filter_map(|der| reqwest::Certificate::from_der(der).ok())),
        // Android nennt seine Zertifikate nur der App; solange sie fehlen, gilt keines als vertrauenswürdig.
        None if cfg!(target_os = "android") => builder.tls_certs_only(Vec::new()),
        None => builder,
    };
    builder.build().unwrap_or_default()
}

/// Wertet eine Antwort aus: Erfolg als `T`, Fehler als [`SyncError`].
async fn parse<T: DeserializeOwned>(response: reqwest::Result<reqwest::Response>) -> Result<T> {
    let response = response.map_err(|e| SyncError::Network(e.to_string()))?;
    let status = response.status();
    let body = response.bytes().await.map_err(|e| SyncError::Network(e.to_string()))?;
    if status.is_success() {
        let body: &[u8] = if body.is_empty() { b"null" } else { &body };
        return serde_json::from_slice(body).map_err(|e| SyncError::Protocol(e.to_string()));
    }
    match serde_json::from_slice::<ErrorBody>(&body) {
        Ok(error) => Err(SyncError::Server { code: error.error, message: error.message }),
        Err(_) => Err(SyncError::Protocol(format!("Status {status}"))),
    }
}

/// Ob ein Datensatz vom Server grundsätzlich nicht zu gebrauchen ist
/// (beschädigt, fremd, unbekanntes Format, unsinniger Zeitstempel) – im
/// Unterschied zu einem Fehler, der beim nächsten Versuch verschwunden sein kann.
fn is_unreadable(error: &CoreError) -> bool {
    matches!(error, CoreError::Vault(VaultError::Crypto(_) | VaultError::Record(_) | VaultError::ClockSkew))
}

/// Kennzeichnet den Stand der noch nicht hochgeladenen Datensätze.
fn fingerprint(dirty: &[tephra_vault::DirtyItem]) -> Vec<(ItemId, usize, u8)> {
    dirty.iter().map(|item| (item.id, item.blob.len(), item.blob.last().copied().unwrap_or(0))).collect()
}

fn is_unauthorized(error: &SyncError) -> bool {
    matches!(error, SyncError::Server { code, .. } if code == codes::UNAUTHORIZED)
}

fn header_from_bytes(bytes: &[u8]) -> Result<VaultHeader> {
    serde_json::from_slice(bytes).map_err(|e| SyncError::Protocol(format!("Kopf des Tresors ist unlesbar: {e}")))
}

impl Sync {
    pub fn new(core: Core) -> Self {
        Self { core, http: http_client(), refreshing: Default::default() }
    }

    fn stored(&self) -> Result<Option<Stored>> {
        let bytes = self.core.with_vault(|v| v.get_secret(SESSION_SECRET))?;
        Ok(bytes.and_then(|b| serde_json::from_slice(&b).ok()))
    }

    fn require(&self) -> Result<Stored> {
        self.stored()?.ok_or(SyncError::NotConnected)
    }

    fn store(&self, stored: &Stored) -> Result<()> {
        let bytes = serde_json::to_vec(stored).map_err(|e| SyncError::Protocol(e.to_string()))?;
        Ok(self.core.with_vault(|v| v.put_secret(SESSION_SECRET, &bytes))?)
    }

    /// Mit welchem Server dieses Gerät verbunden ist, falls überhaupt.
    pub fn connection(&self) -> Result<Option<Connection>> {
        Ok(self.stored()?.map(|s| Connection { server: s.server, username: s.username, device_id: s.device_id }))
    }

    /// Legt auf dem Server ein Konto für den bestehenden Tresor an und
    /// verbindet dieses Gerät damit.
    pub async fn register(
        &self,
        server: &str,
        username: &str,
        password: &str,
        device_name: &str,
        invite: Option<&str>,
    ) -> Result<()> {
        if self.stored()?.is_some() {
            return Err(SyncError::AlreadyConnected);
        }
        let server = normalize_url(server)?;
        let header = self.core.with_vault(|v| Ok(v.header().clone()))?;
        // Das Passwort muss wirklich das des Tresors sein: Sonst entstünde ein
        // Konto, an dem sich kein weiteres Gerät je anmelden könnte.
        header.open_with_password(password)?;
        let request = RegisterRequest {
            username: username.trim().to_string(),
            auth_key: Bytes(header.auth_key(password)?.as_bytes().to_vec()),
            kdf: header.kdf,
            salt: Bytes(header.salt.clone()),
            header: Bytes(serde_json::to_vec(&header).map_err(|e| SyncError::Protocol(e.to_string()))?),
            device_name: device_name.to_string(),
            invite: invite.map(str::to_string),
        };
        let session: Session =
            parse(self.http.post(format!("{server}/v1/accounts")).json(&request).send().await).await?;
        // Alles, was lokal schon existiert, muss zum neuen Konto hochgeladen werden.
        self.core.with_vault(|v| v.reset_sync_state())?;
        self.store(&Stored {
            server,
            username: request.username,
            account_id: session.account_id,
            device_id: session.device_id,
            access_token: session.access_token,
            refresh_token: session.refresh_token,
            header_rev: 1,
        })
    }

    /// Richtet ein weiteres Gerät ein: meldet sich am Server an, legt unter
    /// `path` einen leeren Tresor mit dem Schlüssel des Kontos an und holt
    /// alle Daten ab.
    pub async fn join(path: &Path, server: &str, username: &str, password: &str, device_name: &str) -> Result<Core> {
        if path.exists() {
            return Err(VaultError::AlreadyExists.into());
        }
        let server = normalize_url(server)?;
        let http = http_client();
        let username = username.trim().to_string();
        let prelogin: PreloginResponse = parse(
            http.post(format!("{server}/v1/auth/prelogin"))
                .json(&PreloginRequest { username: username.clone() })
                .send()
                .await,
        )
        .await?;
        // Der Server nennt die Passwortparameter, bevor er sich ausgewiesen
        // hat. Zu schwache Parameter würden den Anmeldenachweis angreifbar
        // machen und werden deshalb gar nicht erst verwendet.
        tephra_vault::check_kdf(prelogin.kdf)?;
        let auth_key =
            MasterKey::derive(password, &prelogin.salt.0, prelogin.kdf).map_err(VaultError::from)?.auth_key();
        let login: LoginResponse = parse(
            http.post(format!("{server}/v1/auth/login"))
                .json(&LoginRequest {
                    username: username.clone(),
                    auth_key: Bytes(auth_key.as_bytes().to_vec()),
                    device_name: device_name.to_string(),
                })
                .send()
                .await,
        )
        .await?;
        let header = header_from_bytes(&login.header.0)?;
        // Öffnet den Vault Key und prüft zugleich, dass der Kopf echt ist.
        let key = header.open_with_password(password)?;
        let core = Core::from_vault(Vault::create_with_header(path, header, key)?);
        let sync = Sync { core: core.clone(), http, refreshing: Default::default() };
        sync.store(&Stored {
            server,
            username,
            account_id: login.session.account_id,
            device_id: login.session.device_id,
            access_token: login.session.access_token,
            refresh_token: login.session.refresh_token,
            header_rev: login.header_rev,
        })?;
        sync.sync_once().await?;
        Ok(core)
    }

    /// Meldet ein bereits eingerichtetes Gerät neu an – nötig, wenn seine
    /// Sitzung abgelaufen ist oder das Passwort auf einem anderen Gerät
    /// geändert wurde. Der lokale Tresor bleibt erhalten; der Kopf vom
    /// Server wird nur übernommen, wenn er zu diesem Tresor gehört.
    pub async fn reconnect(&self, password: &str, device_name: &str) -> Result<()> {
        let stored = self.require()?;
        let prelogin: PreloginResponse = parse(
            self.http
                .post(format!("{}/v1/auth/prelogin", stored.server))
                .json(&PreloginRequest { username: stored.username.clone() })
                .send()
                .await,
        )
        .await?;
        tephra_vault::check_kdf(prelogin.kdf)?;
        let auth_key =
            MasterKey::derive(password, &prelogin.salt.0, prelogin.kdf).map_err(VaultError::from)?.auth_key();
        let login: LoginResponse = parse(
            self.http
                .post(format!("{}/v1/auth/login", stored.server))
                .json(&LoginRequest {
                    username: stored.username.clone(),
                    auth_key: Bytes(auth_key.as_bytes().to_vec()),
                    device_name: device_name.to_string(),
                })
                .send()
                .await,
        )
        .await?;
        let header = header_from_bytes(&login.header.0)?;
        self.core.with_vault(|v| v.replace_header(header))?;
        self.store(&Stored {
            account_id: login.session.account_id,
            device_id: login.session.device_id,
            access_token: login.session.access_token,
            refresh_token: login.session.refresh_token,
            header_rev: login.header_rev,
            ..stored
        })
    }

    /// Führt eine angemeldete Anfrage aus. Ist das Zugriffstoken abgelaufen,
    /// wird die Sitzung einmal verlängert und die Anfrage wiederholt.
    async fn authed<T: DeserializeOwned>(
        &self,
        build: impl Fn(&reqwest::Client, &str) -> reqwest::RequestBuilder,
    ) -> Result<T> {
        let stored = self.require()?;
        match parse(build(&self.http, &stored.server).bearer_auth(&stored.access_token).send().await).await {
            Err(error) if is_unauthorized(&error) => {}
            other => return other,
        }
        // Verlängern darf immer nur eine Aufgabe. Hat eine andere das
        // inzwischen erledigt, wird einfach deren neues Token benutzt.
        let _refreshing = self.refreshing.lock().await;
        let current = self.require()?;
        if current.access_token != stored.access_token {
            return match parse(build(&self.http, &current.server).bearer_auth(&current.access_token).send().await).await
            {
                Err(error) if is_unauthorized(&error) => Err(SyncError::SessionExpired),
                other => other,
            };
        }
        let refreshed: Result<Session> = parse(
            self.http
                .post(format!("{}/v1/auth/refresh", stored.server))
                .json(&RefreshRequest { refresh_token: stored.refresh_token.clone() })
                .send()
                .await,
        )
        .await;
        let session = match refreshed {
            Ok(session) => session,
            Err(error) if is_unauthorized(&error) => return Err(SyncError::SessionExpired),
            Err(error) => return Err(error),
        };
        let stored = Stored { access_token: session.access_token, refresh_token: session.refresh_token, ..stored };
        self.store(&stored)?;
        match parse(build(&self.http, &stored.server).bearer_auth(&stored.access_token).send().await).await {
            Err(error) if is_unauthorized(&error) => Err(SyncError::SessionExpired),
            other => other,
        }
    }

    /// Gleicht einmal vollständig ab: abholen, zusammenführen, hochladen.
    pub async fn sync_once(&self) -> Result<SyncReport> {
        let mut report = SyncReport::default();
        self.pull(&mut report).await?;

        for _ in 0..MAX_ROUNDS {
            let dirty = self.core.with_vault(|v| v.dirty_items())?;
            if dirty.is_empty() {
                break;
            }
            let mut conflicts = 0;
            let mut batch: Vec<PushItem> = Vec::new();
            let mut batch_bytes = 0;
            let mut batches: Vec<Vec<PushItem>> = Vec::new();
            for item in dirty {
                // Was der Server gerade abgelehnt hat, wird in diesem Lauf nicht erneut versucht.
                if report.rejected.iter().any(|(id, _)| *id == item.id) {
                    continue;
                }
                // Ein übergroßer Datensatz ließe sonst die ganze Anfrage scheitern.
                if item.blob.len() > tephra_sync_proto::MAX_BLOB_BYTES {
                    report.rejected.push((item.id, "Der Datensatz ist zu groß für den Abgleich".into()));
                    continue;
                }
                if batch.len() >= MAX_BATCH || (batch_bytes + item.blob.len() > MAX_PUSH_BYTES && !batch.is_empty()) {
                    batches.push(std::mem::take(&mut batch));
                    batch_bytes = 0;
                }
                batch_bytes += item.blob.len();
                batch.push(PushItem {
                    id: item.id.0,
                    base_rev: item.base_rev,
                    deleted: item.deleted,
                    blob: Bytes(item.blob),
                });
            }
            batches.extend((!batch.is_empty()).then_some(batch));
            if batches.is_empty() {
                break;
            }
            for items in batches {
                let request = PushRequest { items };
                let response: PushResponse =
                    self.authed(|http, server| http.post(format!("{server}/v1/sync/push")).json(&request)).await?;
                for result in response.results {
                    let id = ItemId(result.id);
                    let Some(sent) = request.items.iter().find(|item| item.id == result.id) else { continue };
                    match result.outcome {
                        PushOutcome::Accepted { rev } => {
                            self.core.with_vault(|v| v.mark_pushed(id, rev, &sent.blob.0))?;
                            report.pushed += 1;
                        }
                        PushOutcome::Conflict { current } => {
                            conflicts += 1;
                            match self.core.with_vault(|v| v.apply_remote(id, current.rev, &current.blob.0)) {
                                Ok(ApplyOutcome::Merged) => report.merged += 1,
                                Ok(_) => {}
                                Err(error) if is_unreadable(&error) => report.unreadable += 1,
                                Err(error) => return Err(error.into()),
                            }
                        }
                        PushOutcome::Rejected { reason } => report.rejected.push((id, reason)),
                    }
                }
            }
            if conflicts == 0 {
                break;
            }
        }
        Ok(report)
    }

    async fn pull(&self, report: &mut SyncReport) -> Result<()> {
        loop {
            let cursor = self.core.with_vault(|v| v.sync_cursor())?;
            let response: PullResponse = self
                .authed(|http, server| {
                    http.get(format!("{server}/v1/sync")).query(&[("since", cursor), ("limit", MAX_BATCH as i64)])
                })
                .await?;
            for item in &response.items {
                match self.core.with_vault(|v| v.apply_remote(ItemId(item.id), item.rev, &item.blob.0)) {
                    Ok(ApplyOutcome::Inserted | ApplyOutcome::Updated) => report.pulled += 1,
                    Ok(ApplyOutcome::Merged) => report.merged += 1,
                    Ok(ApplyOutcome::Unchanged) => {}
                    // Ein unlesbarer Datensatz darf den Abgleich aller übrigen nicht aufhalten.
                    Err(error) if is_unreadable(&error) => {
                        tracing::warn!(id = %item.id, %error, "Datensatz vom Server ließ sich nicht übernehmen");
                        report.unreadable += 1;
                    }
                    // Alles andere (Datenbank, gesperrter Tresor) ist vorübergehend:
                    // abbrechen, ohne den Stand vorzurücken, und später erneut holen.
                    Err(error) => return Err(error.into()),
                }
            }
            // Ein Server, der „es gibt mehr" meldet, ohne voranzukommen, würde
            // den Abgleich sonst endlos im Kreis schicken.
            if response.more && response.cursor <= cursor {
                return Err(SyncError::Protocol("Der Server kommt beim Abholen nicht voran".into()));
            }
            self.core.with_vault(|v| v.set_sync_cursor(response.cursor))?;
            self.adopt_header(response.header_rev).await?;
            if !response.more {
                return Ok(());
            }
        }
    }

    /// Übernimmt einen neueren Kopf des Tresors (nach einem Passwortwechsel
    /// auf einem anderen Gerät) – aber nur, wenn er nachweislich echt ist.
    async fn adopt_header(&self, header_rev: i64) -> Result<()> {
        let stored = self.require()?;
        if header_rev <= stored.header_rev {
            return Ok(());
        }
        let response: HeaderResponse = self.authed(|http, server| http.get(format!("{server}/v1/header"))).await?;
        let header = header_from_bytes(&response.header.0)?;
        self.core.with_vault(|v| v.replace_header(header))?;
        self.store(&Stored { header_rev: response.header_rev, ..self.require()? })
    }

    /// Wartet, bis der Server Neues meldet oder `wait` verstrichen ist.
    /// Gibt zurück, ob es etwas abzuholen gibt.
    pub async fn wait_for_changes(&self, wait: Duration) -> Result<bool> {
        let stored = self.require()?;
        let cursor = self.core.with_vault(|v| v.sync_cursor())?;
        let wait = wait.as_secs().clamp(1, MAX_WAIT_SECS);
        let response: EventsResponse = self
            .authed(|http, server| {
                http.get(format!("{server}/v1/events")).query(&[
                    ("since", cursor),
                    ("header_rev", stored.header_rev),
                    ("wait", wait as i64),
                ])
            })
            .await?;
        Ok(response.rev > cursor || response.header_rev > stored.header_rev)
    }

    /// Gleicht fortlaufend ab, bis `stop` auf `true` gesetzt wird: sofort bei
    /// lokalen Änderungen und sobald der Server Neues meldet. Jedes Ergebnis
    /// geht an `observe`.
    pub async fn run(&self, mut stop: watch::Receiver<bool>, observe: impl Fn(&Result<SyncReport>)) {
        let mut backoff = Duration::from_secs(2);
        while !*stop.borrow() {
            let result = self.sync_once().await;
            observe(&result);
            let pause = match &result {
                Ok(_) => {
                    backoff = Duration::from_secs(2);
                    Duration::ZERO
                }
                // Ohne gültige Sitzung hilft kein Wiederholen.
                Err(SyncError::SessionExpired | SyncError::NotConnected) => return,
                Err(_) => {
                    backoff = (backoff * 2).min(Duration::from_secs(300));
                    backoff
                }
            };
            let idle = async {
                tokio::time::sleep(pause).await;
                // Was nach dem Abgleich noch aussteht (vom Server abgelehnt
                // oder unlesbar), gilt nicht als neue Änderung – sonst würde
                // es alle zwei Sekunden erneut hochgeladen.
                let pending =
                    |sync: &Sync| sync.core.with_vault(|v| v.dirty_items()).map(|dirty| fingerprint(&dirty)).ok();
                let stuck = pending(self);
                let local_change = async {
                    loop {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        let now = pending(self);
                        if now.is_some() && now != stuck {
                            return;
                        }
                    }
                };
                tokio::select! {
                    _ = local_change => {}
                    result = self.wait_for_changes(Duration::from_secs(MAX_WAIT_SECS)) => {
                        if result.is_err() {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
            };
            tokio::select! {
                _ = stop.changed() => {}
                _ = idle => {}
            }
        }
    }

    pub async fn devices(&self) -> Result<Vec<DeviceInfo>> {
        self.authed(|http, server| http.get(format!("{server}/v1/devices"))).await
    }

    /// Entfernt ein Gerät vom Konto; es kann danach nicht mehr abgleichen.
    /// Daten, die es bereits abgeholt hat, bleiben dort lesbar.
    pub async fn remove_device(&self, device: Uuid) -> Result<()> {
        self.authed::<Option<()>>(|http, server| http.delete(format!("{server}/v1/devices/{device}"))).await?;
        Ok(())
    }

    /// Meldet dieses Gerät ab. Der lokale Tresor bleibt unverändert.
    pub async fn disconnect(&self) -> Result<()> {
        let result = self.authed::<Option<()>>(|http, server| http.post(format!("{server}/v1/auth/logout"))).await;
        self.core.with_vault(|v| v.delete_secret(SESSION_SECRET))?;
        match result {
            Ok(_) | Err(SyncError::SessionExpired) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Ändert das Master-Passwort – auf dem Server und lokal. Alle anderen
    /// Geräte werden abgemeldet und müssen sich mit dem neuen Passwort neu
    /// anmelden.
    pub async fn change_password(&self, old_password: &str, new_password: &str, kdf: KdfParams) -> Result<()> {
        let old_header = self.core.with_vault(|v| Ok(v.header().clone()))?;
        old_header.open_with_password(old_password)?;
        let new_header = self.core.with_vault(|v| v.prepare_password_change(new_password, kdf))?;
        let request = ChangePasswordRequest {
            old_auth_key: Bytes(old_header.auth_key(old_password)?.as_bytes().to_vec()),
            new_auth_key: Bytes(new_header.auth_key(new_password)?.as_bytes().to_vec()),
            kdf: new_header.kdf,
            salt: Bytes(new_header.salt.clone()),
            header: Bytes(serde_json::to_vec(&new_header).map_err(|e| SyncError::Protocol(e.to_string()))?),
        };
        let response: HeaderResponse =
            self.authed(|http, server| http.post(format!("{server}/v1/account/password")).json(&request)).await?;
        // Erst wenn der Server zugestimmt hat, gilt das neue Passwort auch lokal.
        self.core.with_vault(|v| v.replace_header(new_header))?;
        self.store(&Stored { header_rev: response.header_rev, ..self.require()? })
    }
}

#[cfg(test)]
mod tests;
