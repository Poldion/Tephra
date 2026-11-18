//! Verschlüsselung einzelner Datensätze.
//!
//! Format: `version(1) | nonce(24) | XChaCha20-Poly1305(aufgefüllter Klartext)`.
//! Konto- und Datensatzkennung fließen als zusätzliche authentifizierte Daten
//! ein: Ein Chiffrat lässt sich weder einem anderen Datensatz noch einem
//! anderen Konto unterschieben.
//!
//! Der Klartext wird vor dem Verschlüsseln auf Größenstufen aufgefüllt, damit
//! der Server aus der Länge nicht auf den Inhalt schließen kann.

use crate::CryptoError;
use crate::keys::{VaultKey, random_bytes};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::Zeroize;

const VERSION: u8 = 1;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const LEN_PREFIX: usize = 4;
const MIN_BUCKET: usize = 256;
const LARGE_STEP: usize = 16 * 1024;
const LARGE_FROM: usize = 64 * 1024;

fn aad(account_id: &[u8], item_id: &[u8; 16]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(8 + account_id.len() + 16);
    aad.extend_from_slice(b"tephra-item\x01");
    aad.extend_from_slice(&(account_id.len() as u32).to_le_bytes());
    aad.extend_from_slice(account_id);
    aad.extend_from_slice(item_id);
    aad
}

/// Größe, auf die ein Klartext der Länge `len` (inkl. Längenfeld) aufgefüllt wird.
fn bucket(len: usize) -> usize {
    if len <= MIN_BUCKET {
        MIN_BUCKET
    } else if len <= LARGE_FROM {
        len.next_power_of_two()
    } else {
        len.div_ceil(LARGE_STEP) * LARGE_STEP
    }
}

/// Verschlüsselt den Inhalt eines Datensatzes.
pub fn seal_item(
    key: &VaultKey,
    account_id: &[u8],
    item_id: &[u8; 16],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let len = u32::try_from(plaintext.len()).map_err(|_| CryptoError::Malformed)?;
    let mut padded = vec![0u8; bucket(LEN_PREFIX + plaintext.len())];
    padded[..LEN_PREFIX].copy_from_slice(&len.to_le_bytes());
    padded[LEN_PREFIX..LEN_PREFIX + plaintext.len()].copy_from_slice(plaintext);

    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    let nonce = random_bytes::<NONCE_LEN>()?;
    let result = cipher.encrypt(&XNonce::from(nonce), Payload { msg: &padded, aad: &aad(account_id, item_id) });
    padded.zeroize();
    let ciphertext = result.map_err(|_| CryptoError::Decrypt)?;

    let mut out = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
    out.push(VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Entschlüsselt den Inhalt eines Datensatzes.
pub fn open_item(key: &VaultKey, account_id: &[u8], item_id: &[u8; 16], blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let (&version, rest) = blob.split_first().ok_or(CryptoError::Malformed)?;
    if version != VERSION {
        return Err(CryptoError::UnknownVersion(version));
    }
    if rest.len() < NONCE_LEN + TAG_LEN {
        return Err(CryptoError::Malformed);
    }
    let (nonce, ciphertext) = rest.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| CryptoError::Malformed)?;
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    let mut padded = cipher
        .decrypt(&XNonce::from(nonce), Payload { msg: ciphertext, aad: &aad(account_id, item_id) })
        .map_err(|_| CryptoError::Decrypt)?;

    let result = (|| {
        let prefix: [u8; LEN_PREFIX] = padded.get(..LEN_PREFIX)?.try_into().ok()?;
        let len = u32::from_le_bytes(prefix) as usize;
        padded.get(LEN_PREFIX..LEN_PREFIX.checked_add(len)?).map(<[u8]>::to_vec)
    })();
    padded.zeroize();
    result.ok_or(CryptoError::Malformed)
}

const AUTH_ITEM: [u8; 16] = *b"tephra-auth-tag1";

/// Erzeugt einen Echtheitsnachweis für unverschlüsselt gespeicherte Daten
/// (z. B. den Kopf des Tresors). Nur wer den Vault Key kennt, kann ihn
/// erzeugen oder prüfen.
pub fn authenticate(key: &VaultKey, context: &[u8], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    use sha2::Digest;
    seal_item(key, context, &AUTH_ITEM, &sha2::Sha256::digest(data))
}

/// Prüft einen mit [`authenticate`] erzeugten Nachweis.
pub fn verify(key: &VaultKey, context: &[u8], data: &[u8], tag: &[u8]) -> bool {
    use sha2::Digest;
    use subtle::ConstantTimeEq;
    match open_item(key, context, &AUTH_ITEM, tag) {
        Ok(digest) => digest.ct_eq(&sha2::Sha256::digest(data)).into(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: &[u8] = b"konto-1";

    #[test]
    fn echtheitsnachweis() {
        let key = VaultKey::generate().expect("zufall");
        let tag = authenticate(&key, b"kopf", b"inhalt").expect("tag");
        assert!(verify(&key, b"kopf", b"inhalt", &tag));
        assert!(!verify(&key, b"kopf", b"anderer inhalt", &tag));
        assert!(!verify(&key, b"anderer kontext", b"inhalt", &tag));
        assert!(!verify(&VaultKey::generate().expect("zufall"), b"kopf", b"inhalt", &tag));
        assert!(!verify(&key, b"kopf", b"inhalt", b"unsinn"));
    }
    const ITEM: [u8; 16] = [7u8; 16];

    #[test]
    fn rundreise() {
        let key = VaultKey::generate().expect("zufall");
        for len in [0usize, 1, 251, 252, 253, 1000, 70_000] {
            let plain: Vec<u8> = (0..len).map(|i| i as u8).collect();
            let blob = seal_item(&key, ACCOUNT, &ITEM, &plain).expect("seal");
            assert_eq!(open_item(&key, ACCOUNT, &ITEM, &blob).expect("open"), plain);
        }
    }

    #[test]
    fn laenge_verraet_nur_die_groessenstufe() {
        let key = VaultKey::generate().expect("zufall");
        let a = seal_item(&key, ACCOUNT, &ITEM, b"kurz").expect("seal");
        let b = seal_item(&key, ACCOUNT, &ITEM, &[b'x'; 200]).expect("seal");
        assert_eq!(a.len(), b.len());
        assert_eq!(a.len(), 1 + NONCE_LEN + MIN_BUCKET + TAG_LEN);
        assert_eq!(bucket(257), 512);
        assert_eq!(bucket(LARGE_FROM + 1), LARGE_FROM + LARGE_STEP);
    }

    #[test]
    fn gleicher_klartext_ergibt_verschiedene_chiffrate() {
        let key = VaultKey::generate().expect("zufall");
        let a = seal_item(&key, ACCOUNT, &ITEM, b"geheim").expect("seal");
        let b = seal_item(&key, ACCOUNT, &ITEM, b"geheim").expect("seal");
        assert_ne!(a, b);
    }

    #[test]
    fn chiffrat_ist_an_konto_und_datensatz_gebunden() {
        let key = VaultKey::generate().expect("zufall");
        let blob = seal_item(&key, ACCOUNT, &ITEM, b"geheim").expect("seal");
        assert_eq!(open_item(&key, b"konto-2", &ITEM, &blob).unwrap_err(), CryptoError::Decrypt);
        assert_eq!(open_item(&key, ACCOUNT, &[8u8; 16], &blob).unwrap_err(), CryptoError::Decrypt);
        let other = VaultKey::generate().expect("zufall");
        assert_eq!(open_item(&other, ACCOUNT, &ITEM, &blob).unwrap_err(), CryptoError::Decrypt);
    }

    proptest::proptest! {
        #[test]
        fn beliebige_bytes_werden_abgelehnt_statt_abzustuerzen(blob in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..400)) {
            let key = VaultKey::from_bytes([7u8; 32]);
            proptest::prop_assert!(open_item(&key, ACCOUNT, &ITEM, &blob).is_err());
            proptest::prop_assert!(!verify(&key, b"kopf", b"inhalt", &blob));
        }

        #[test]
        fn jede_laenge_uebersteht_die_rundreise(plain in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..3000)) {
            let key = VaultKey::from_bytes([7u8; 32]);
            let blob = seal_item(&key, ACCOUNT, &ITEM, &plain).expect("seal");
            proptest::prop_assert_eq!(open_item(&key, ACCOUNT, &ITEM, &blob).expect("open"), plain);
        }
    }

    #[test]
    fn beschaedigte_daten() {
        let key = VaultKey::generate().expect("zufall");
        let mut blob = seal_item(&key, ACCOUNT, &ITEM, b"geheim").expect("seal");
        assert_eq!(open_item(&key, ACCOUNT, &ITEM, &[]).unwrap_err(), CryptoError::Malformed);
        assert_eq!(open_item(&key, ACCOUNT, &ITEM, &blob[..20]).unwrap_err(), CryptoError::Malformed);
        blob[30] ^= 0x80;
        assert_eq!(open_item(&key, ACCOUNT, &ITEM, &blob).unwrap_err(), CryptoError::Decrypt);
        blob[0] = 2;
        assert_eq!(open_item(&key, ACCOUNT, &ITEM, &blob).unwrap_err(), CryptoError::UnknownVersion(2));
    }
}
