//! Sync-Server von Tephra.
//!
//! Ein bewusst einfacher Dienst: Er meldet Geräte an, speichert verschlüsselte
//! Datensätze mit Revisionsnummern und sagt Bescheid, wenn es Neues gibt.
//! Inhalte kann er nicht lesen; Konflikte lösen die Clients.

pub mod db;
pub mod tls;

use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use db::{Db, DbError, Limits, NewAccount};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tephra_crypto::KdfParams;
use tephra_sync_proto::{
    Bytes, ChangePasswordRequest, DeleteAccountRequest, DeviceInfo, ErrorBody, EventsResponse, HeaderResponse,
    LoginRequest, LoginResponse, MAX_BATCH, MAX_BLOB_BYTES, MAX_WAIT_SECS, PreloginRequest, PreloginResponse,
    PullResponse, PushOutcome, PushRequest, PushResponse, RefreshRequest, RegisterRequest, RemoveDeviceRequest,
    Session, codes,
};
use tokio::sync::watch;
use uuid::Uuid;

/// Wer darf neue Konten anlegen?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    Open,
    /// Nur mit Einladungscode (`tephra-server invite`).
    Invite,
    Closed,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub registration: Registration,
    /// Höchstzahl an Datensätzen je Konto (Löschmarker eingeschlossen).
    pub max_items: u64,
    /// Höchste Datenmenge je Konto in Byte.
    pub max_bytes: u64,
    pub access_ttl: Duration,
    pub refresh_ttl: Duration,
    /// Fehlgeschlagene Anmeldeversuche je Benutzername und Absender
    /// innerhalb von [`Config::attempt_window`].
    pub max_attempts: usize,
    pub attempt_window: Duration,
    /// Der Server steht hinter einem Reverse-Proxy: Als Absender gilt dann
    /// die Adresse aus `X-Forwarded-For`. Nur einschalten, wenn der Proxy
    /// diesen Kopf selbst setzt – sonst könnte jeder Absender ihn fälschen.
    pub behind_proxy: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            registration: Registration::Invite,
            max_items: 50_000,
            max_bytes: 256 * 1024 * 1024,
            access_ttl: Duration::from_secs(3600),
            refresh_ttl: Duration::from_secs(60 * 24 * 3600),
            max_attempts: 10,
            attempt_window: Duration::from_secs(300),
            behind_proxy: false,
        }
    }
}

struct AppState {
    db: Mutex<Db>,
    config: Config,
    secret: [u8; 32],
    /// Je Konto der aktuelle Stand `(Revision, Kopf-Revision)`; weckt wartende `events`-Anfragen.
    changes: Mutex<HashMap<Uuid, watch::Sender<(i64, i64)>>>,
    attempts: Mutex<HashMap<String, VecDeque<Instant>>>,
}

type Shared = Arc<AppState>;

impl AppState {
    fn db(&self) -> MutexGuard<'_, Db> {
        self.db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn limits(&self) -> Limits {
        Limits { max_items: self.config.max_items, max_bytes: self.config.max_bytes }
    }

    fn session(&self, account: Uuid, tokens: db::Tokens) -> Session {
        Session {
            account_id: account,
            device_id: tokens.device_id,
            access_token: tokens.access,
            refresh_token: tokens.refresh,
            expires_in_secs: self.config.access_ttl.as_secs(),
        }
    }

    fn new_device(&self, account: Uuid, name: &str) -> Result<Session, ApiError> {
        let tokens = self.db().create_device(
            account,
            name,
            self.config.access_ttl.as_millis() as u64,
            self.config.refresh_ttl.as_millis() as u64,
        )?;
        Ok(self.session(account, tokens))
    }

    /// Meldet wartenden Geräten eines Kontos den neuen Stand.
    fn announce(&self, account: Uuid, rev: i64, header_rev: i64) {
        let mut changes = self.changes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        changes.retain(|id, sender| *id == account || sender.receiver_count() > 0);
        if let Some(sender) = changes.get(&account) {
            sender.send_replace((rev, header_rev));
        }
    }

    fn subscribe(&self, account: Uuid, current: (i64, i64)) -> watch::Receiver<(i64, i64)> {
        let mut changes = self.changes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let sender = changes.entry(account).or_insert_with(|| watch::channel(current).0);
        sender.send_if_modified(|state| {
            let newer = current.0 > state.0 || current.1 > state.1;
            if newer {
                *state = current;
            }
            newer
        });
        sender.subscribe()
    }

    /// Lehnt ab, wenn unter `key` bereits `max` Versuche im Zeitfenster
    /// vermerkt sind.
    fn guard(&self, key: &str, max: usize) -> Result<(), ApiError> {
        let attempts = self.attempts.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        let recent = attempts
            .get(key)
            .map_or(0, |times| times.iter().filter(|t| now.duration_since(**t) < self.config.attempt_window).count());
        if recent >= max {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                codes::TOO_MANY_REQUESTS,
                "Zu viele Versuche; bitte später erneut probieren",
            ));
        }
        Ok(())
    }

    /// Vermerkt einen Versuch unter `key`.
    fn note_attempt(&self, key: &str) {
        let mut attempts = self.attempts.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        let window = self.config.attempt_window;
        // Aufräumen nur gelegentlich, nicht bei jeder Anfrage über die ganze Tabelle.
        if attempts.len() > 4096 {
            attempts.retain(|_, times| times.back().is_some_and(|t| now.duration_since(*t) < window));
        }
        let times = attempts.entry(key.to_string()).or_default();
        while times.front().is_some_and(|t| now.duration_since(*t) >= window) {
            times.pop_front();
        }
        // Mehr als die Obergrenze braucht niemand zu behalten.
        if times.len() < 10_000 {
            times.push_back(now);
        }
    }

    /// Passwortparameter, die für einen unbekannten Benutzernamen gemeldet
    /// werden. Sie sind je Name stabil, damit sich bekannte und unbekannte
    /// Namen von außen nicht unterscheiden lassen.
    fn decoy_prelogin(&self, username: &str) -> PreloginResponse {
        let mut hasher = Sha256::new();
        hasher.update(self.secret);
        hasher.update(username.to_lowercase().as_bytes());
        PreloginResponse { kdf: KdfParams::DEFAULT, salt: Bytes(hasher.finalize()[..16].to_vec()) }
    }
}

// ------------------------------------------------------------------ Fehler

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into() }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, codes::INVALID_REQUEST, message)
    }

    fn credentials() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, codes::INVALID_CREDENTIALS, "Benutzername oder Passwort stimmt nicht")
    }
}

impl From<DbError> for ApiError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::UsernameTaken => Self::new(StatusCode::CONFLICT, codes::USERNAME_TAKEN, e.to_string()),
            DbError::InvalidInvite => Self::new(StatusCode::FORBIDDEN, codes::INVALID_INVITE, e.to_string()),
            DbError::NoSuchAccount => {
                Self::new(StatusCode::UNAUTHORIZED, codes::UNAUTHORIZED, "Konto existiert nicht mehr")
            }
            DbError::QuotaExceeded => Self::new(StatusCode::INSUFFICIENT_STORAGE, codes::QUOTA_EXCEEDED, e.to_string()),
            other => {
                tracing::error!(error = %other, "Interner Fehler");
                Self::new(StatusCode::INTERNAL_SERVER_ERROR, codes::INTERNAL, "Interner Fehler")
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorBody { error: self.code.to_string(), message: self.message })).into_response()
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

// ------------------------------------------------------------------ Anmeldung

/// Angemeldetes Gerät, ermittelt aus dem Zugriffstoken der Anfrage.
struct Device {
    account: Uuid,
    id: Uuid,
}

impl FromRequestParts<Shared> for Device {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, Self::Rejection> {
        let unauthorized = || ApiError::new(StatusCode::UNAUTHORIZED, codes::UNAUTHORIZED, "Anmeldung erforderlich");
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or_else(unauthorized)?;
        let (account, id) = state.db().authenticate(token)?.ok_or_else(unauthorized)?;
        Ok(Self { account, id })
    }
}

/// Adresse des anfragenden Rechners, für die Begrenzung von Anmeldeversuchen.
struct Remote(String);

impl FromRequestParts<Shared> for Remote {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, Self::Rejection> {
        let direct = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|info| info.0.ip().to_string());
        let forwarded = state
            .config
            .behind_proxy
            .then(|| parts.headers.get("x-forwarded-for").and_then(|value| value.to_str().ok()))
            .flatten()
            // Der letzte Eintrag stammt vom eigenen Proxy; frühere kann der Absender frei wählen.
            .and_then(|value| value.rsplit(',').next())
            .map(|value| value.trim().chars().take(64).collect::<String>())
            .filter(|value| !value.is_empty());
        Ok(Self(forwarded.or(direct).unwrap_or_else(|| "unbekannt".into())))
    }
}

fn check_username(username: &str) -> Result<&str, ApiError> {
    let username = username.trim();
    let valid = (3..=64).contains(&username.chars().count())
        && username.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '@' | '+'));
    if valid {
        Ok(username)
    } else {
        Err(ApiError::invalid("Benutzername: 3–64 Zeichen aus Buchstaben, Ziffern und . - _ @ +"))
    }
}

fn check_device_name(name: &str) -> String {
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(64).collect();
    if name.is_empty() { "Gerät".into() } else { name }
}

fn check_header(kdf: KdfParams, salt: &Bytes, header: &Bytes, auth_key: &Bytes) -> Result<(), ApiError> {
    // Die bewusst schwachen Testparameter gelten nur in Test-Builds.
    let test_params = cfg!(feature = "testing") && kdf == KdfParams::INSECURE_FOR_TESTS;
    if !kdf.is_acceptable() && !test_params {
        return Err(ApiError::invalid("Passwortparameter sind zu schwach oder unzulässig hoch"));
    }
    if !(8..=64).contains(&salt.0.len()) || auth_key.0.len() != 32 {
        return Err(ApiError::invalid("Salt oder Anmeldenachweis haben eine unzulässige Länge"));
    }
    if header.0.is_empty() || header.0.len() > 16 * 1024 {
        return Err(ApiError::invalid("Kopf des Tresors fehlt oder ist zu groß"));
    }
    Ok(())
}

async fn register(
    State(state): State<Shared>,
    remote: Remote,
    Json(request): Json<RegisterRequest>,
) -> ApiResult<Session> {
    let username = check_username(&request.username)?;
    check_header(request.kdf, &request.salt, &request.header, &request.auth_key)?;
    let key = format!("register:{}", remote.0);
    state.guard(&key, state.config.max_attempts)?;
    state.note_attempt(&key);
    let invite = match state.config.registration {
        Registration::Open => None,
        Registration::Closed => {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                codes::REGISTRATION_CLOSED,
                "Auf diesem Server lassen sich keine neuen Konten anlegen",
            ));
        }
        Registration::Invite => match request.invite.as_deref().filter(|code| !code.is_empty()) {
            Some(code) => Some(code),
            None => return Err(DbError::InvalidInvite.into()),
        },
    };
    let account = state.db().create_account(
        &NewAccount {
            username,
            auth_key: &request.auth_key.0,
            kdf: request.kdf,
            salt: &request.salt.0,
            header: &request.header.0,
        },
        invite,
    )?;
    Ok(Json(state.new_device(account, &check_device_name(&request.device_name))?))
}

async fn prelogin(
    State(state): State<Shared>,
    remote: Remote,
    Json(request): Json<PreloginRequest>,
) -> ApiResult<PreloginResponse> {
    let username = check_username(&request.username)?;
    // Großzügig bemessen, aber begrenzt: Die Abfrage braucht keine Anmeldung.
    let key = format!("prelogin:{}", remote.0);
    state.guard(&key, state.config.max_attempts * 20)?;
    state.note_attempt(&key);
    Ok(Json(match state.db().account_by_username(username)? {
        Some(account) => PreloginResponse { kdf: account.kdf, salt: Bytes(account.salt) },
        None => state.decoy_prelogin(username),
    }))
}

async fn login(
    State(state): State<Shared>,
    remote: Remote,
    Json(request): Json<LoginRequest>,
) -> ApiResult<LoginResponse> {
    let username = check_username(&request.username).map_err(|_| ApiError::credentials())?;
    // Gezählt werden nur Fehlversuche, getrennt nach Absender: Ein Dritter
    // kann den rechtmäßigen Nutzer so nicht durch falsche Passwörter
    // aussperren, und wer sich korrekt anmeldet, wird nie gebremst.
    let by_user = format!("login:{}|{}", username.to_lowercase(), remote.0);
    let by_remote = format!("login:*|{}", remote.0);
    state.guard(&by_user, state.config.max_attempts)?;
    state.guard(&by_remote, state.config.max_attempts * 10)?;
    let account = state.db().account_by_username(username)?;
    let verified = state.db().verify_auth(account.as_ref().map(|a| a.id), &request.auth_key.0)?;
    let Some(account) = account.filter(|_| verified) else {
        state.note_attempt(&by_user);
        state.note_attempt(&by_remote);
        return Err(ApiError::credentials());
    };
    let session = state.new_device(account.id, &check_device_name(&request.device_name))?;
    Ok(Json(LoginResponse { session, header: Bytes(account.header), header_rev: account.header_rev }))
}

async fn refresh(State(state): State<Shared>, Json(request): Json<RefreshRequest>) -> ApiResult<Session> {
    let refreshed = state.db().refresh(
        &request.refresh_token,
        state.config.access_ttl.as_millis() as u64,
        state.config.refresh_ttl.as_millis() as u64,
    )?;
    match refreshed {
        Some((account, tokens)) => Ok(Json(state.session(account, tokens))),
        None => Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            codes::UNAUTHORIZED,
            "Sitzung ist abgelaufen; bitte neu anmelden",
        )),
    }
}

async fn logout(State(state): State<Shared>, device: Device) -> Result<StatusCode, ApiError> {
    state.db().delete_device(device.account, device.id)?;
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------------ Geräte und Konto

async fn devices(State(state): State<Shared>, device: Device) -> ApiResult<Vec<DeviceInfo>> {
    Ok(Json(state.db().list_devices(device.account, device.id)?))
}

async fn remove_device(
    State(state): State<Shared>,
    device: Device,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> Result<StatusCode, ApiError> {
    // Sich selbst abmelden darf jedes Gerät; ein anderes nur mit dem Anmeldenachweis. Sonst könnte jemand mit
    // einem mitgeschnittenen Sitzungstoken alle echten Geräte hinauswerfen.
    if id != device.id {
        let request: RemoveDeviceRequest = serde_json::from_slice(&body).map_err(|_| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                codes::INVALID_REQUEST,
                "Ein anderes Gerät abzumelden verlangt das Master-Passwort (Version 0.12 oder neuer)",
            )
        })?;
        let key = format!("password:{}", device.account);
        state.guard(&key, state.config.max_attempts)?;
        if !state.db().verify_auth(Some(device.account), &request.auth_key.0)? {
            state.note_attempt(&key);
            return Err(ApiError::credentials());
        }
    }
    if state.db().delete_device(device.account, id)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::new(StatusCode::NOT_FOUND, codes::NOT_FOUND, "Gerät nicht gefunden"))
    }
}

async fn get_header(State(state): State<Shared>, device: Device) -> ApiResult<HeaderResponse> {
    let account = state.db().account(device.account)?.ok_or(DbError::NoSuchAccount)?;
    Ok(Json(HeaderResponse { header: Bytes(account.header), header_rev: account.header_rev }))
}

async fn change_password(
    State(state): State<Shared>,
    device: Device,
    Json(request): Json<ChangePasswordRequest>,
) -> ApiResult<HeaderResponse> {
    check_header(request.kdf, &request.salt, &request.header, &request.new_auth_key)?;
    let key = format!("password:{}", device.account);
    state.guard(&key, state.config.max_attempts)?;
    if !state.db().verify_auth(Some(device.account), &request.old_auth_key.0)? {
        state.note_attempt(&key);
        return Err(ApiError::credentials());
    }
    let header_rev = state.db().change_password(
        device.account,
        &request.new_auth_key.0,
        request.kdf,
        &request.salt.0,
        &request.header.0,
        device.id,
    )?;
    let seq = state.db().account(device.account)?.map_or(0, |a| a.seq);
    state.announce(device.account, seq, header_rev);
    Ok(Json(HeaderResponse { header: request.header, header_rev }))
}

async fn delete_account(
    State(state): State<Shared>,
    device: Device,
    Json(request): Json<DeleteAccountRequest>,
) -> Result<StatusCode, ApiError> {
    let key = format!("password:{}", device.account);
    state.guard(&key, state.config.max_attempts)?;
    if !state.db().verify_auth(Some(device.account), &request.auth_key.0)? {
        state.note_attempt(&key);
        return Err(ApiError::credentials());
    }
    state.db().delete_account(device.account)?;
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------------ Abgleich

#[derive(Deserialize)]
struct PullQuery {
    #[serde(default)]
    since: i64,
    #[serde(default)]
    limit: Option<usize>,
}

async fn pull(State(state): State<Shared>, device: Device, Query(query): Query<PullQuery>) -> ApiResult<PullResponse> {
    let limit = query.limit.unwrap_or(MAX_BATCH).clamp(1, MAX_BATCH);
    let db = state.db();
    let account = db.account(device.account)?.ok_or(DbError::NoSuchAccount)?;
    let (items, more) = db.pull(device.account, query.since.max(0), limit)?;
    // Ist alles geliefert, gilt der Stand des Kontos; sonst die letzte gelieferte Revision.
    let cursor = if more { items.last().map_or(query.since, |item| item.rev) } else { account.seq.max(query.since) };
    Ok(Json(PullResponse { items, cursor, more, header_rev: account.header_rev }))
}

async fn push(
    State(state): State<Shared>,
    device: Device,
    Json(request): Json<PushRequest>,
) -> ApiResult<PushResponse> {
    if request.items.len() > MAX_BATCH {
        return Err(ApiError::invalid(format!("Höchstens {MAX_BATCH} Datensätze je Anfrage")));
    }
    if request.items.iter().any(|item| item.blob.0.is_empty() || item.blob.0.len() > MAX_BLOB_BYTES) {
        return Err(ApiError::invalid(format!("Datensätze müssen zwischen 1 und {MAX_BLOB_BYTES} Byte groß sein")));
    }
    let results = state.db().push(device.account, &request.items, &state.limits())?;
    let accepted = results.iter().filter_map(|r| match r.outcome {
        PushOutcome::Accepted { rev } => Some(rev),
        _ => None,
    });
    if let Some(rev) = accepted.max() {
        let header_rev = state.db().account(device.account)?.map_or(0, |a| a.header_rev);
        state.announce(device.account, rev, header_rev);
    }
    Ok(Json(PushResponse { results }))
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    since: i64,
    #[serde(default)]
    header_rev: i64,
    #[serde(default)]
    wait: Option<u64>,
}

/// Antwortet, sobald das Konto einen neueren Stand hat als vom Client
/// genannt – oder nach Ablauf der Wartezeit mit dem unveränderten Stand.
async fn events(
    State(state): State<Shared>,
    device: Device,
    Query(query): Query<EventsQuery>,
) -> ApiResult<EventsResponse> {
    let account = state.db().account(device.account)?.ok_or(DbError::NoSuchAccount)?;
    let mut receiver = state.subscribe(device.account, (account.seq, account.header_rev));
    let wait = Duration::from_secs(query.wait.unwrap_or(MAX_WAIT_SECS).min(MAX_WAIT_SECS));
    let is_newer = |(rev, header_rev): (i64, i64)| rev > query.since || header_rev > query.header_rev;
    let _ = tokio::time::timeout(wait, receiver.wait_for(|current| is_newer(*current))).await;
    let (rev, header_rev) = *receiver.borrow();
    Ok(Json(EventsResponse { rev, header_rev }))
}

async fn healthz() -> &'static str {
    "ok"
}

/// Baut die Anwendung. `db` muss bereits initialisiert sein.
pub fn app(db: Db, config: Config) -> Result<Router, DbError> {
    let secret = db.server_secret()?;
    let state: Shared = Arc::new(AppState {
        db: Mutex::new(db),
        config,
        secret,
        changes: Mutex::new(HashMap::new()),
        attempts: Mutex::new(HashMap::new()),
    });
    Ok(Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/accounts", post(register))
        .route("/v1/auth/prelogin", post(prelogin))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/refresh", post(refresh))
        .route("/v1/auth/logout", post(logout))
        .route("/v1/devices", get(devices))
        .route("/v1/devices/{id}", delete(remove_device))
        .route("/v1/header", get(get_header))
        .route("/v1/account/password", post(change_password))
        .route("/v1/account", delete(delete_account))
        .route("/v1/sync", get(pull))
        // Nur das Hochladen braucht große Anfragen (eine volle Charge in
        // Base64 plus etwas Luft); alles andere kommt mit wenig aus.
        .route("/v1/sync/push", post(push).layer(DefaultBodyLimit::max(MAX_BATCH * MAX_BLOB_BYTES / 8 + 1024 * 1024)))
        .route("/v1/events", get(events))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .with_state(state))
}

/// Bedient Anfragen auf `listener`, bis `shutdown` fertig wird.
pub async fn serve(
    listener: tokio::net::TcpListener,
    db: Db,
    config: Config,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    serve_tls(listener, db, config, None, true, shutdown).await
}

/// Wie [`serve`], mit TLS: `acceptor` nimmt verschlüsselte Verbindungen an; `plain` lässt auf demselben Port
/// zusätzlich einfaches HTTP zu (siehe [`tls::TlsMode`]).
pub async fn serve_tls(
    listener: tokio::net::TcpListener,
    db: Db,
    config: Config,
    acceptor: Option<tokio_rustls::TlsAcceptor>,
    plain: bool,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    use axum::serve::ListenerExt;
    let app = app(db, config).map_err(std::io::Error::other)?;
    let listener = tls::MixedListener::new(listener, acceptor, plain)?.tap_io(|_| {});
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown)
        .await
}
