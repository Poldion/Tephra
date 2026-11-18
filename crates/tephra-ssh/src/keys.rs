//! SSH-Schlüssel erzeugen, einlesen und beschreiben.

use crate::{Result, SshError};
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, HashAlg, PrivateKey, PublicKey};

/// Beschreibung eines Schlüssels für Listen und Dialoge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    /// z. B. `ssh-ed25519`.
    pub algorithm: String,
    /// Fingerabdruck in OpenSSH-Schreibweise (`SHA256:…`).
    pub fingerprint: String,
    /// Öffentlicher Schlüssel in `authorized_keys`-Schreibweise.
    pub public_key: String,
    /// Ob der private Schlüssel mit einer Passphrase geschützt ist.
    pub encrypted: bool,
}

/// Erzeugt ein neues Ed25519-Schlüsselpaar und gibt den privaten Schlüssel
/// als OpenSSH-PEM zusammen mit seiner Beschreibung zurück.
pub fn generate_ed25519(comment: &str) -> Result<(String, KeyInfo)> {
    let mut key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).map_err(|e| SshError::Key(e.to_string()))?;
    key.set_comment(comment);
    let pem = key.to_openssh(LineEnding::LF).map_err(|e| SshError::Key(e.to_string()))?;
    let info = describe_public(key.public_key(), false)?;
    Ok((pem.to_string(), info))
}

/// Liest einen privaten Schlüssel ein (OpenSSH-, PKCS#8- oder PEM-Format).
pub fn decode_private(pem: &str, passphrase: Option<&str>) -> Result<PrivateKey> {
    russh::keys::decode_secret_key(pem, passphrase.filter(|p| !p.is_empty())).map_err(|e| SshError::Key(e.to_string()))
}

/// Beschreibt einen privaten Schlüssel. Bei verschlüsselten Schlüsseln wird
/// die Passphrase benötigt, um den öffentlichen Teil zu ermitteln.
pub fn inspect(pem: &str, passphrase: Option<&str>) -> Result<KeyInfo> {
    let encrypted = pem.contains("ENCRYPTED")
        || PrivateKey::from_openssh(pem.as_bytes()).map(|k| k.is_encrypted()).unwrap_or(false);
    let key = decode_private(pem, passphrase)?;
    describe_public(key.public_key(), encrypted)
}

pub(crate) fn describe_public(key: &PublicKey, encrypted: bool) -> Result<KeyInfo> {
    Ok(KeyInfo {
        algorithm: key.algorithm().as_str().to_string(),
        fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
        public_key: key.to_openssh().map_err(|e| SshError::Key(e.to_string()))?,
        encrypted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erzeugen_und_wieder_einlesen() {
        let (pem, info) = generate_ed25519("test@gerät").expect("erzeugen");
        assert!(pem.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert_eq!(info.algorithm, "ssh-ed25519");
        assert!(info.fingerprint.starts_with("SHA256:"));
        assert!(info.public_key.starts_with("ssh-ed25519 "));
        assert!(!info.encrypted);

        let again = inspect(&pem, None).expect("einlesen");
        assert_eq!(again.fingerprint, info.fingerprint);
    }

    #[test]
    fn unbrauchbare_eingaben() {
        assert!(matches!(inspect("kein schlüssel", None), Err(SshError::Key(_))));
        assert!(matches!(decode_private("", None), Err(SshError::Key(_))));
    }
}
