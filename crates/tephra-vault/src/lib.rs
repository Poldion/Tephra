//! Lokaler, verschlüsselter Tresor.
//!
//! Die Datenbank (SQLite) enthält ausschließlich Chiffrate – dieselben, die
//! auch der Sync-Server zu sehen bekommt. Beim Entsperren werden die
//! Datensätze in den Arbeitsspeicher entschlüsselt, beim Sperren wieder
//! verworfen.

mod store;

pub use store::{ApplyOutcome, DirtyItem, HistoryEntry, Vault, VaultHeader, check_kdf};

use tephra_crypto::CryptoError;
use tephra_model::RecordError;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("Tresor ist gesperrt")]
    Locked,
    #[error("Falsches Passwort")]
    WrongPassword,
    #[error("Tresordatei existiert bereits")]
    AlreadyExists,
    #[error("Datei ist kein Tresor oder beschädigt")]
    NotAVault,
    #[error("Tresor wurde mit einer neueren Programmversion angelegt (Schema {0})")]
    TooNew(i64),
    #[error("Datensatz {0} nicht gefunden")]
    NotFound(tephra_model::ItemId),
    #[error("Die Passwortparameter des Tresors sind unzulässig")]
    WeakKdf,
    #[error("Der Kopf des Tresors wurde verändert und passt nicht zum Schlüssel")]
    HeaderTampered,
    #[error("Der angebotene Kopf des Tresors ist älter als der vorhandene und wird nicht übernommen")]
    HeaderOutdated,
    #[error("Datensatz trägt einen Zeitstempel weit in der Zukunft und wird nicht übernommen")]
    ClockSkew,
    #[error("Datensatz {0} wurde bereits abgeglichen und kann nicht mehr auf dieses Gerät beschränkt werden")]
    AlreadySynced(tephra_model::ItemId),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error(transparent)]
    Record(#[from] RecordError),
    #[error("Datenbankfehler: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("Serialisierung fehlgeschlagen: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, VaultError>;
