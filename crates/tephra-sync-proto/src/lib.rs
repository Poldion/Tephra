//! Nachrichten zwischen Client und Sync-Server (HTTP + JSON, Version 1).
//!
//! Der Server sieht ausschließlich Chiffrate: Datensätze als verschlüsselte
//! Blobs mit Revisionsnummer, dazu den unverschlüsselten, aber
//! authentifizierten Kopf des Tresors. Er kennt weder Art noch Inhalt der
//! Datensätze.
//!
//! ```text
//! POST /v1/accounts            Konto anlegen
//! POST /v1/auth/prelogin       Passwortparameter abholen
//! POST /v1/auth/login          anmelden, Kopf des Tresors erhalten
//! POST /v1/auth/refresh        Sitzung verlängern
//! POST /v1/auth/logout         dieses Gerät abmelden
//! GET  /v1/devices             Geräteliste;  DELETE /v1/devices/{id}  Gerät entfernen
//! GET  /v1/header              aktueller Kopf des Tresors
//! GET  /v1/sync?since=&limit=  geänderte Datensätze seit einer Revision
//! POST /v1/sync/push           Änderungen hochladen
//! GET  /v1/events?since=&wait= wartet, bis es Neues gibt (Long Polling)
//! POST /v1/account/password    Passwort ändern
//! DELETE /v1/account           Konto samt aller Daten löschen
//! GET  /healthz                Lebenszeichen
//! ```

use base64::Engine;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tephra_crypto::KdfParams;
use uuid::Uuid;

/// Höchste Größe eines einzelnen Datensatzes (verschlüsselt).
pub const MAX_BLOB_BYTES: usize = 1024 * 1024;
/// Höchstzahl an Datensätzen je Abhol- oder Hochlade-Anfrage.
pub const MAX_BATCH: usize = 200;
/// Längste Wartezeit einer `events`-Anfrage in Sekunden.
pub const MAX_WAIT_SECS: u64 = 55;

/// Binärdaten, die in JSON als Base64 übertragen werden.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Bytes(pub Vec<u8>);

impl std::fmt::Debug for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bytes({} Byte)", self.0.len())
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        base64::engine::general_purpose::STANDARD.decode(text.as_bytes()).map(Bytes).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    /// Aus dem Master-Passwort abgeleiteter Nachweis; nie das Passwort selbst.
    pub auth_key: Bytes,
    pub kdf: KdfParams,
    pub salt: Bytes,
    /// Kopf des Tresors (JSON), für den Server undurchsichtig.
    pub header: Bytes,
    pub device_name: String,
    /// Einladungscode, falls der Server einen verlangt.
    #[serde(default)]
    pub invite: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub account_id: Uuid,
    pub device_id: Uuid,
    /// Kurzlebiges Token für jede Anfrage (`Authorization: Bearer …`).
    pub access_token: String,
    /// Langlebiges Token, mit dem sich ein neues Paar holen lässt. Es ist
    /// nur einmal gültig; jede Verlängerung liefert ein neues.
    pub refresh_token: String,
    /// Gültigkeit des Zugriffstokens in Sekunden.
    pub expires_in_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreloginRequest {
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreloginResponse {
    pub kdf: KdfParams,
    pub salt: Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub auth_key: Bytes,
    pub device_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub session: Session,
    pub header: Bytes,
    /// Zähler, der mit jeder Änderung des Kopfes steigt.
    pub header_rev: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: Uuid,
    pub name: String,
    pub created_ms: u64,
    pub last_seen_ms: u64,
    /// Ob es das Gerät ist, das gerade fragt.
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderResponse {
    pub header: Bytes,
    pub header_rev: i64,
}

/// Datensatz, wie ihn der Server kennt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteItem {
    pub id: Uuid,
    pub rev: i64,
    pub deleted: bool,
    pub blob: Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullResponse {
    pub items: Vec<RemoteItem>,
    /// Revision, bis zu der diese Antwort vollständig ist; als `since` der
    /// nächsten Anfrage verwenden.
    pub cursor: i64,
    /// Es gibt weitere Änderungen; gleich noch einmal fragen.
    pub more: bool,
    pub header_rev: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushItem {
    pub id: Uuid,
    /// Revision, auf der die Änderung beruht (0 = neu).
    pub base_rev: i64,
    pub deleted: bool,
    pub blob: Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    pub items: Vec<PushItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PushOutcome {
    /// Übernommen unter dieser Revision.
    Accepted { rev: i64 },
    /// Der Server hat inzwischen eine andere Fassung; sie liegt bei. Der
    /// Client führt zusammen und lädt erneut hoch.
    Conflict { current: RemoteItem },
    /// Abgelehnt (z. B. zu groß); der Datensatz bleibt lokal.
    Rejected { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushResult {
    pub id: Uuid,
    #[serde(flatten)]
    pub outcome: PushOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    pub results: Vec<PushResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventsResponse {
    /// Höchste Revision des Kontos.
    pub rev: i64,
    pub header_rev: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePasswordRequest {
    pub old_auth_key: Bytes,
    pub new_auth_key: Bytes,
    pub kdf: KdfParams,
    pub salt: Bytes,
    pub header: Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteAccountRequest {
    pub auth_key: Bytes,
}

/// Fehlerantwort des Servers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorBody {
    /// Maschinenlesbarer Code, z. B. `invalid_credentials`.
    pub error: String,
    pub message: String,
}

pub mod codes {
    pub const INVALID_CREDENTIALS: &str = "invalid_credentials";
    pub const UNAUTHORIZED: &str = "unauthorized";
    pub const USERNAME_TAKEN: &str = "username_taken";
    pub const REGISTRATION_CLOSED: &str = "registration_closed";
    pub const INVALID_INVITE: &str = "invalid_invite";
    pub const INVALID_REQUEST: &str = "invalid_request";
    pub const TOO_MANY_REQUESTS: &str = "too_many_requests";
    pub const QUOTA_EXCEEDED: &str = "quota_exceeded";
    pub const NOT_FOUND: &str = "not_found";
    pub const INTERNAL: &str = "internal";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_als_base64() {
        let json = serde_json::to_string(&Bytes(vec![0, 1, 2, 255])).expect("json");
        assert_eq!(json, "\"AAEC/w==\"");
        assert_eq!(serde_json::from_str::<Bytes>(&json).expect("json"), Bytes(vec![0, 1, 2, 255]));
        assert!(serde_json::from_str::<Bytes>("\"kein base64!\"").is_err());
        assert_eq!(format!("{:?}", Bytes(vec![1, 2, 3])), "Bytes(3 Byte)");
    }

    #[test]
    fn push_ergebnis_ist_flach() {
        let result = PushResult { id: Uuid::nil(), outcome: PushOutcome::Accepted { rev: 7 } };
        let json = serde_json::to_value(&result).expect("json");
        assert_eq!(json["status"], "accepted");
        assert_eq!(json["rev"], 7);
        assert_eq!(serde_json::from_value::<PushResult>(json).expect("json"), result);
    }
}
