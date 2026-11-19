//! SSH-Schlüssel erzeugen, einlesen und beschreiben.

use crate::{Result, SshError};
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockModeDecrypt, KeyIvInit};
use base64::Engine;
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

/// Liest einen privaten Schlüssel ein: OpenSSH, PKCS#8, PuTTY und das alte
/// PEM-Format von OpenSSL (`RSA PRIVATE KEY`, `EC PRIVATE KEY`), auch mit
/// Passphrase.
///
/// Die Fehler unterscheiden, ob die Passphrase fehlt, nicht stimmt oder das
/// Format nicht unterstützt wird.
pub fn decode_private(pem: &str, passphrase: Option<&str>) -> Result<PrivateKey> {
    let passphrase = passphrase.filter(|p| !p.is_empty());
    if let Some(legacy) = LegacyPem::parse(pem)? {
        let passphrase = passphrase.ok_or(SshError::KeyPassphraseMissing)?;
        let plain = legacy.decrypt(passphrase)?;
        // Mit falscher Passphrase kommt fast immer schon die Auffüllung nicht hin; den Rest fängt das Einlesen.
        return russh::keys::decode_secret_key(&plain, None).map_err(|_| SshError::KeyPassphraseWrong);
    }
    let encrypted = is_encrypted(pem);
    russh::keys::decode_secret_key(pem, passphrase).map_err(|error| match error {
        russh::keys::Error::UnsupportedKeyType { key_type_string, .. } => {
            SshError::KeyFormatUnsupported(format!("Schlüsselart {key_type_string}"))
        }
        russh::keys::Error::KeyIsEncrypted => SshError::KeyPassphraseMissing,
        _ if encrypted && passphrase.is_none() => SshError::KeyPassphraseMissing,
        _ if encrypted => SshError::KeyPassphraseWrong,
        other => match foreign_format(pem) {
            Some(name) => SshError::KeyFormatUnsupported(name),
            None => SshError::Key(other.to_string()),
        },
    })
}

/// Beschreibt einen privaten Schlüssel. Bei verschlüsselten Schlüsseln wird
/// die Passphrase benötigt, um den öffentlichen Teil zu ermitteln.
pub fn inspect(pem: &str, passphrase: Option<&str>) -> Result<KeyInfo> {
    let key = decode_private(pem, passphrase)?;
    describe_public(key.public_key(), is_encrypted(pem))
}

/// Liest einen privaten Schlüssel ein und schreibt ihn ins OpenSSH-Format um,
/// ohne Passphrase: So liegt er im Tresor, der ihn seinerseits verschlüsselt,
/// und jede spätere Verbindung kann ihn lesen – gleich, in welchem Format er kam.
pub fn import(pem: &str, passphrase: Option<&str>) -> Result<(String, KeyInfo)> {
    let key = decode_private(pem, passphrase)?;
    let info = describe_public(key.public_key(), is_encrypted(pem))?;
    let openssh = key.to_openssh(LineEnding::LF).map_err(|e| SshError::Key(e.to_string()))?;
    Ok((openssh.to_string(), info))
}

pub(crate) fn describe_public(key: &PublicKey, encrypted: bool) -> Result<KeyInfo> {
    Ok(KeyInfo {
        algorithm: key.algorithm().as_str().to_string(),
        fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
        public_key: key.to_openssh().map_err(|e| SshError::Key(e.to_string()))?,
        encrypted,
    })
}

/// Ob der Schlüssel mit einer Passphrase geschützt ist, gleich in welchem Format.
fn is_encrypted(pem: &str) -> bool {
    pem.contains("ENCRYPTED")
        || pem.lines().any(|line| line.starts_with("Encryption:") && !line.trim_end().ends_with("none"))
        || PrivateKey::from_openssh(pem.as_bytes()).map(|key| key.is_encrypted()).unwrap_or(false)
}

/// Benennt Eingaben, die erkennbar etwas anderes sind als ein unterstützter privater Schlüssel.
fn foreign_format(text: &str) -> Option<String> {
    const KNOWN: [&str; 5] =
        ["OPENSSH PRIVATE KEY", "RSA PRIVATE KEY", "EC PRIVATE KEY", "PRIVATE KEY", "ENCRYPTED PRIVATE KEY"];
    let trimmed = text.trim_start();
    if ["ssh-", "ecdsa-", "sk-"].iter().any(|prefix| trimmed.starts_with(prefix)) {
        return Some("öffentlicher Schlüssel – gebraucht wird der private".into());
    }
    let label = text.lines().find_map(|line| line.trim().strip_prefix("-----BEGIN ")?.strip_suffix("-----"))?;
    if KNOWN.contains(&label) {
        return None;
    }
    Some(if label.contains("PUBLIC KEY") || label.contains("CERTIFICATE") {
        format!("{label} – gebraucht wird der private Schlüssel")
    } else {
        label.to_string()
    })
}

/// Privater Schlüssel im alten PEM-Format von OpenSSL, mit Passphrase geschützt:
///
/// ```text
/// -----BEGIN RSA PRIVATE KEY-----
/// Proc-Type: 4,ENCRYPTED
/// DEK-Info: AES-256-CBC,<IV als Hex>
///
/// <Base64>
/// -----END RSA PRIVATE KEY-----
/// ```
struct LegacyPem {
    /// `RSA` oder `EC`.
    kind: String,
    cipher: String,
    iv: Vec<u8>,
    data: Vec<u8>,
}

impl LegacyPem {
    /// `None`, wenn der Text kein passphrasegeschützter Schlüssel dieses Formats ist.
    fn parse(pem: &str) -> Result<Option<Self>> {
        let mut lines = pem.lines().map(str::trim);
        let Some(kind) = lines.find_map(|line| line.strip_prefix("-----BEGIN ")?.strip_suffix(" PRIVATE KEY-----"))
        else {
            return Ok(None);
        };
        let (mut encrypted, mut dek, mut body) = (false, None, String::new());
        for line in lines {
            if line.starts_with("-----END ") {
                break;
            }
            match line.split_once(':') {
                Some(("Proc-Type", value)) => encrypted = value.contains("ENCRYPTED"),
                Some(("DEK-Info", value)) => dek = Some(value.trim().to_string()),
                Some(_) => {}
                None => body.push_str(line),
            }
        }
        if !encrypted {
            return Ok(None);
        }
        if kind != "RSA" && kind != "EC" {
            return Err(SshError::KeyFormatUnsupported(format!("{kind} PRIVATE KEY mit Passphrase")));
        }
        let broken = || SshError::Key("die Kopfzeilen des Schlüssels sind unvollständig".into());
        let dek = dek.ok_or_else(broken)?;
        let (cipher, iv) = dek.split_once(',').ok_or_else(broken)?;
        let iv = decode_hex(iv.trim()).ok_or_else(broken)?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(body.as_bytes())
            .map_err(|_| SshError::Key("der Inhalt des Schlüssels ist nicht lesbar".into()))?;
        Ok(Some(Self { kind: kind.to_string(), cipher: cipher.trim().to_string(), iv, data }))
    }

    /// Entschlüsselt und gibt den Schlüssel als PEM ohne Passphrase zurück.
    fn decrypt(&self, passphrase: &str) -> Result<String> {
        let unsupported = || SshError::KeyFormatUnsupported(format!("Verschlüsselung {}", self.cipher));
        let (key_len, block) = match self.cipher.as_str() {
            "AES-128-CBC" => (16, 16),
            "AES-192-CBC" => (24, 16),
            "AES-256-CBC" => (32, 16),
            "DES-EDE3-CBC" => (24, 8),
            _ => return Err(unsupported()),
        };
        if self.iv.len() != block || self.data.is_empty() || !self.data.len().is_multiple_of(block) {
            return Err(SshError::Key("der Schlüssel ist beschädigt".into()));
        }
        let key = derive_key(passphrase.as_bytes(), &self.iv[..8], key_len);
        let mut buffer = self.data.clone();
        let wrong = |_| SshError::KeyPassphraseWrong;
        let broken = |_| SshError::Key("der Schlüssel ist beschädigt".into());
        let plain = match self.cipher.as_str() {
            "AES-128-CBC" => cbc::Decryptor::<aes::Aes128>::new_from_slices(&key, &self.iv)
                .map_err(broken)?
                .decrypt_padded::<Pkcs7>(&mut buffer)
                .map_err(wrong)?,
            "AES-192-CBC" => cbc::Decryptor::<aes::Aes192>::new_from_slices(&key, &self.iv)
                .map_err(broken)?
                .decrypt_padded::<Pkcs7>(&mut buffer)
                .map_err(wrong)?,
            "AES-256-CBC" => cbc::Decryptor::<aes::Aes256>::new_from_slices(&key, &self.iv)
                .map_err(broken)?
                .decrypt_padded::<Pkcs7>(&mut buffer)
                .map_err(wrong)?,
            _ => cbc::Decryptor::<des::TdesEde3>::new_from_slices(&key, &self.iv)
                .map_err(broken)?
                .decrypt_padded::<Pkcs7>(&mut buffer)
                .map_err(wrong)?,
        };
        // Ein entschlüsselter Schlüssel ist eine ASN.1-Sequenz; alles andere heißt: falsche Passphrase.
        if plain.first() != Some(&0x30) {
            return Err(SshError::KeyPassphraseWrong);
        }
        let body = base64::engine::general_purpose::STANDARD.encode(plain);
        let mut pem = format!("-----BEGIN {} PRIVATE KEY-----\n", self.kind);
        for chunk in body.as_bytes().chunks(64) {
            pem.push_str(&String::from_utf8_lossy(chunk));
            pem.push('\n');
        }
        pem.push_str(&format!("-----END {} PRIVATE KEY-----\n", self.kind));
        Ok(pem)
    }
}

/// Schlüsselableitung des alten Formats (`EVP_BytesToKey` von OpenSSL mit MD5,
/// ein Durchlauf): Jeder Block ist der Hash aus dem vorigen Block, der
/// Passphrase und dem Salz.
fn derive_key(passphrase: &[u8], salt: &[u8], len: usize) -> Vec<u8> {
    let mut key = Vec::with_capacity(len + 16);
    let mut previous: Vec<u8> = Vec::new();
    while key.len() < len {
        let mut hash = md5::Context::new();
        hash.consume(&previous);
        hash.consume(passphrase);
        hash.consume(salt);
        previous = hash.finalize().0.to_vec();
        key.extend_from_slice(&previous);
    }
    key.truncate(len);
    key
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok()).collect()
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

    // Eigens für diese Tests erzeugte Schlüssel; sie sind nirgends im Einsatz.
    const PASSPHRASE: &str = "nur zum Testen";
    const RSA_PUBLIC: &str = include_str!("testdata/rsa.pub");
    const EC_PUBLIC: &str = include_str!("testdata/ec.pub");
    const RSA_AES256: &str = include_str!("testdata/rsa-aes256.pem");

    /// Typ und Schlüssel ohne den Kommentar am Ende.
    fn bare(public: &str) -> String {
        public.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn altes_format_mit_passphrase() {
        for (name, pem) in [
            ("AES-128", include_str!("testdata/rsa-aes128.pem")),
            ("AES-192", include_str!("testdata/rsa-aes192.pem")),
            ("AES-256", RSA_AES256),
            ("3DES", include_str!("testdata/rsa-des3.pem")),
        ] {
            let info = inspect(pem, Some(PASSPHRASE)).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(bare(&info.public_key), bare(RSA_PUBLIC), "{name}");
            assert!(info.encrypted, "{name}");
        }
        let info = inspect(include_str!("testdata/ec-aes256.pem"), Some(PASSPHRASE)).expect("EC");
        assert_eq!(bare(&info.public_key), bare(EC_PUBLIC));
    }

    #[test]
    fn altes_format_ohne_passphrase_wird_weiter_gelesen() {
        let info = inspect(include_str!("testdata/rsa-plain.pem"), None).expect("einlesen");
        assert_eq!(bare(&info.public_key), bare(RSA_PUBLIC));
        assert!(!info.encrypted);
    }

    #[test]
    fn fehler_werden_unterschieden() {
        assert!(matches!(inspect(RSA_AES256, None), Err(SshError::KeyPassphraseMissing)));
        assert!(matches!(inspect(RSA_AES256, Some("")), Err(SshError::KeyPassphraseMissing)));
        assert!(matches!(inspect(RSA_AES256, Some("falsch")), Err(SshError::KeyPassphraseWrong)));
        match inspect(include_str!("testdata/rsa-camellia.pem"), Some(PASSPHRASE)) {
            Err(SshError::KeyFormatUnsupported(what)) => assert!(what.contains("CAMELLIA-256-CBC"), "{what}"),
            other => panic!("unerwartet: {other:?}"),
        }
        match inspect(RSA_PUBLIC, None) {
            Err(SshError::KeyFormatUnsupported(what)) => assert!(what.contains("öffentlicher"), "{what}"),
            other => panic!("unerwartet: {other:?}"),
        }
        match inspect("-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n", None) {
            Err(SshError::KeyFormatUnsupported(what)) => assert!(what.contains("CERTIFICATE"), "{what}"),
            other => panic!("unerwartet: {other:?}"),
        }
    }

    #[test]
    fn passphrase_bei_openssh_schluesseln() {
        let pem = include_str!("testdata/ed25519-encrypted.pem");
        assert!(matches!(inspect(pem, None), Err(SshError::KeyPassphraseMissing)));
        assert!(matches!(inspect(pem, Some("falsch")), Err(SshError::KeyPassphraseWrong)));
        assert!(inspect(pem, Some(PASSPHRASE)).expect("einlesen").encrypted);
    }

    #[test]
    fn import_schreibt_ins_openssh_format_um() {
        let (openssh, info) = import(RSA_AES256, Some(PASSPHRASE)).expect("import");
        assert!(openssh.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert!(info.encrypted, "die Herkunft bleibt erkennbar");
        // Was im Tresor landet, lässt sich ohne Passphrase lesen und ist derselbe Schlüssel.
        let stored = inspect(&openssh, None).expect("wieder einlesen");
        assert_eq!(bare(&stored.public_key), bare(RSA_PUBLIC));
        assert!(!stored.encrypted);
    }

    #[test]
    fn schluesselableitung_wie_openssl() {
        // openssl enc -aes-256-cbc -md md5 -S 0102030405060708 -pass pass:geheim -P
        let key = derive_key(b"geheim", &[1, 2, 3, 4, 5, 6, 7, 8], 32);
        assert_eq!(key.len(), 32);
        assert_eq!(key[..16], md5::compute([b"geheim".as_slice(), &[1, 2, 3, 4, 5, 6, 7, 8]].concat()).0);
    }
}
