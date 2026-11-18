//! Schlüsselhierarchie und Verschlüsselung des Tresors.
//!
//! ```text
//! Master-Passwort ──Argon2id──► Master Key
//!    Master Key ──HKDF──► Auth Key   (Anmeldung am Sync-Server)
//!    Master Key ──HKDF──► KEK        (verpackt den Vault Key)
//!    Vault Key (zufällig)            verschlüsselt jeden Datensatz
//!    Recovery Key (zufällig)         verpackt den Vault Key ein zweites Mal
//! ```
//!
//! Es werden ausschließlich etablierte Verfahren verwendet: Argon2id,
//! HKDF-SHA256 und XChaCha20-Poly1305.

mod keys;
mod seal;

pub use keys::{
    AuthKey, KdfParams, Kek, MasterKey, RecoveryKey, SALT_LEN, VaultKey, WrappedKey, random_bytes, random_salt,
};
pub use seal::{authenticate, open_item, seal_item, verify};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("Schlüsselableitung fehlgeschlagen: ungültige Parameter")]
    Kdf,
    #[error("Entschlüsselung fehlgeschlagen: falscher Schlüssel oder beschädigte Daten")]
    Decrypt,
    #[error("Unbekanntes Datenformat (Version {0})")]
    UnknownVersion(u8),
    #[error("Daten sind zu kurz oder fehlerhaft aufgebaut")]
    Malformed,
    #[error("Wiederherstellungsschlüssel ist ungültig")]
    InvalidRecoveryKey,
    #[error("Zufallsquelle des Betriebssystems nicht verfügbar")]
    Random,
}
