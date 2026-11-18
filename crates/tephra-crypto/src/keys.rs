use crate::CryptoError;
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const SALT_LEN: usize = 16;
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const WRAP_VERSION: u8 = 1;

const INFO_AUTH: &[u8] = b"tephra/v1/auth";
const INFO_KEK: &[u8] = b"tephra/v1/kek";
const INFO_RECOVERY: &[u8] = b"tephra/v1/recovery-kek";
const AAD_WRAP: &[u8] = b"tephra/v1/wrapped-vault-key";

/// Füllt einen Puffer mit Zufall aus dem Betriebssystem.
pub fn random_bytes<const N: usize>() -> Result<[u8; N], CryptoError> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|_| CryptoError::Random)?;
    Ok(buf)
}

pub fn random_salt() -> Result<[u8; SALT_LEN], CryptoError> {
    random_bytes::<SALT_LEN>()
}

/// Parameter der Passwortableitung. Sie werden pro Konto gespeichert, damit
/// sie später angehoben werden können, ohne alte Konten auszusperren.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Speicherbedarf in KiB.
    pub memory_kib: u32,
    /// Anzahl der Durchläufe.
    pub iterations: u32,
    /// Parallelität.
    pub parallelism: u32,
}

impl KdfParams {
    /// Empfohlene Einstellung für Desktop und aktuelle Mobilgeräte
    /// (64 MiB, 3 Durchläufe).
    pub const DEFAULT: KdfParams = KdfParams { memory_kib: 64 * 1024, iterations: 3, parallelism: 1 };

    /// Untergrenze, die beim Anmelden akzeptiert wird. Verhindert, dass ein
    /// manipulierter Server den Client zu einer schwachen Ableitung zwingt.
    pub const MINIMUM: KdfParams = KdfParams { memory_kib: 19 * 1024, iterations: 2, parallelism: 1 };

    /// Obergrenze, damit ein manipulierter Server den Client nicht durch
    /// absurde Parameter lahmlegen kann.
    pub const MAXIMUM: KdfParams = KdfParams { memory_kib: 1024 * 1024, iterations: 16, parallelism: 8 };

    pub fn is_acceptable(&self) -> bool {
        self.memory_kib >= Self::MINIMUM.memory_kib
            && self.iterations >= Self::MINIMUM.iterations
            && self.parallelism >= 1
            && self.memory_kib <= Self::MAXIMUM.memory_kib
            && self.iterations <= Self::MAXIMUM.iterations
            && self.parallelism <= Self::MAXIMUM.parallelism
    }

    /// Sehr schwache Parameter ausschließlich für automatisierte Tests.
    #[doc(hidden)]
    pub const INSECURE_FOR_TESTS: KdfParams = KdfParams { memory_kib: 64, iterations: 1, parallelism: 1 };
}

impl Default for KdfParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

macro_rules! secret_key {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Zeroize, ZeroizeOnDrop)]
        pub struct $name([u8; KEY_LEN]);

        impl $name {
            pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
                Self(bytes)
            }

            pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
                &self.0
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($name), "(…)"))
            }
        }
    };
}

secret_key!(
    /// Aus dem Master-Passwort abgeleiteter Schlüssel. Verlässt das Gerät nie.
    MasterKey
);
secret_key!(
    /// Nachweis gegenüber dem Sync-Server, dass das Passwort bekannt ist.
    AuthKey
);
secret_key!(
    /// Schlüssel, der den Vault Key verpackt.
    Kek
);
secret_key!(
    /// Zufälliger Schlüssel, mit dem alle Datensätze verschlüsselt werden.
    VaultKey
);
secret_key!(
    /// Zufälliger Schlüssel für die Wiederherstellung bei vergessenem Passwort.
    RecoveryKey
);

impl MasterKey {
    /// Leitet den Master Key aus dem Passwort ab (Argon2id).
    pub fn derive(password: &str, salt: &[u8], params: KdfParams) -> Result<Self, CryptoError> {
        let params = Params::new(params.memory_kib, params.iterations, params.parallelism, Some(KEY_LEN))
            .map_err(|_| CryptoError::Kdf)?;
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut out = [0u8; KEY_LEN];
        argon.hash_password_into(password.as_bytes(), salt, &mut out).map_err(|_| CryptoError::Kdf)?;
        Ok(Self(out))
    }

    pub fn auth_key(&self) -> AuthKey {
        AuthKey(expand(&self.0, INFO_AUTH))
    }

    pub fn kek(&self) -> Kek {
        Kek(expand(&self.0, INFO_KEK))
    }
}

impl VaultKey {
    pub fn generate() -> Result<Self, CryptoError> {
        Ok(Self(random_bytes::<KEY_LEN>()?))
    }

    /// Verpackt den Vault Key mit dem aus dem Passwort abgeleiteten KEK.
    ///
    /// `context` bindet die Verpackung an ihre Umgebung (Tresorkennung,
    /// Ableitungsparameter, Salt): Mit anderem Kontext lässt sie sich nicht
    /// öffnen, ein vertauschter oder veränderter Kopf fällt also auf.
    pub fn wrap(&self, kek: &Kek, context: &[u8]) -> Result<WrappedKey, CryptoError> {
        wrap(&kek.0, &self.0, context)
    }

    pub fn unwrap(wrapped: &WrappedKey, kek: &Kek, context: &[u8]) -> Result<Self, CryptoError> {
        unwrap(&kek.0, wrapped, context).map(Self)
    }

    /// Verpackt den Vault Key mit dem Wiederherstellungsschlüssel.
    pub fn wrap_for_recovery(&self, recovery: &RecoveryKey, context: &[u8]) -> Result<WrappedKey, CryptoError> {
        wrap(&expand(&recovery.0, INFO_RECOVERY), &self.0, context)
    }

    pub fn unwrap_with_recovery(
        wrapped: &WrappedKey,
        recovery: &RecoveryKey,
        context: &[u8],
    ) -> Result<Self, CryptoError> {
        unwrap(&expand(&recovery.0, INFO_RECOVERY), wrapped, context).map(Self)
    }
}

impl VaultKey {
    /// Leitet aus einem Namen eine Kennung ab, die nur mit dem Vault Key
    /// nachzurechnen ist. Für Datensätze mit fester Kennung: Alle Geräte des
    /// Kontos kommen zum selben Ergebnis, der Server kann aber nicht prüfen,
    /// ob eine vermutete Angabe (etwa eine Hostadresse) dahintersteckt.
    pub fn derive_id(&self, name: &[u8]) -> [u8; 16] {
        let hk = Hkdf::<Sha256>::new(None, &self.0);
        let mut info = b"tephra/v1/derived-id/".to_vec();
        info.extend_from_slice(name);
        let mut out = [0u8; 16];
        hk.expand(&info, &mut out).expect("16 Byte sind eine gültige HKDF-Ausgabelänge");
        out
    }
}

impl RecoveryKey {
    pub fn generate() -> Result<Self, CryptoError> {
        Ok(Self(random_bytes::<KEY_LEN>()?))
    }

    /// Darstellung zum Aufschreiben: Base32 (Crockford) in Vierergruppen mit
    /// angehängter Prüfsumme, z. B. `7K2M-9QXA-…`.
    pub fn to_display_string(&self) -> String {
        let mut data = self.0.to_vec();
        data.extend_from_slice(&checksum(&self.0));
        let encoded = base32_encode(&data);
        data.zeroize();
        let mut out = String::with_capacity(encoded.len() + encoded.len() / 4);
        for (i, c) in encoded.chars().enumerate() {
            if i > 0 && i % 4 == 0 {
                out.push('-');
            }
            out.push(c);
        }
        out
    }

    /// Liest die aufgeschriebene Darstellung wieder ein. Groß-/Kleinschreibung,
    /// Bindestriche und Leerzeichen sind egal; leicht verwechselbare Zeichen
    /// (O/0, I/L/1) werden toleriert.
    pub fn parse(text: &str) -> Result<Self, CryptoError> {
        let mut data = base32_decode(text).ok_or(CryptoError::InvalidRecoveryKey)?;
        if data.len() != KEY_LEN + 2 {
            data.zeroize();
            return Err(CryptoError::InvalidRecoveryKey);
        }
        let mut key = [0u8; KEY_LEN];
        key.copy_from_slice(&data[..KEY_LEN]);
        let ok = checksum(&key) == data[KEY_LEN..];
        data.zeroize();
        if !ok {
            key.zeroize();
            return Err(CryptoError::InvalidRecoveryKey);
        }
        Ok(Self(key))
    }
}

/// Mit XChaCha20-Poly1305 verpackter Schlüssel: `version | nonce | chiffrat`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WrappedKey(pub Vec<u8>);

fn expand(ikm: &[u8; KEY_LEN], info: &[u8]) -> [u8; KEY_LEN] {
    let hk = Hkdf::<Sha256>::new(None, ikm);
    let mut out = [0u8; KEY_LEN];
    // Die Ausgabelänge von 32 Byte ist für HKDF-SHA256 immer zulässig.
    hk.expand(info, &mut out).expect("32 Byte sind eine gültige HKDF-Ausgabelänge");
    out
}

fn wrap_aad(context: &[u8]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_WRAP.len() + context.len());
    aad.extend_from_slice(AAD_WRAP);
    aad.extend_from_slice(context);
    aad
}

fn wrap(wrapping_key: &[u8; KEY_LEN], key: &[u8; KEY_LEN], context: &[u8]) -> Result<WrappedKey, CryptoError> {
    let cipher = XChaCha20Poly1305::new(wrapping_key.into());
    let nonce = random_bytes::<NONCE_LEN>()?;
    let ciphertext = cipher
        .encrypt(&XNonce::from(nonce), Payload { msg: key, aad: &wrap_aad(context) })
        .map_err(|_| CryptoError::Decrypt)?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
    out.push(WRAP_VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(WrappedKey(out))
}

fn unwrap(wrapping_key: &[u8; KEY_LEN], wrapped: &WrappedKey, context: &[u8]) -> Result<[u8; KEY_LEN], CryptoError> {
    let data = &wrapped.0;
    let (&version, rest) = data.split_first().ok_or(CryptoError::Malformed)?;
    if version != WRAP_VERSION {
        return Err(CryptoError::UnknownVersion(version));
    }
    if rest.len() < NONCE_LEN {
        return Err(CryptoError::Malformed);
    }
    let (nonce, ciphertext) = rest.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| CryptoError::Malformed)?;
    let cipher = XChaCha20Poly1305::new(wrapping_key.into());
    let mut plain = cipher
        .decrypt(&XNonce::from(nonce), Payload { msg: ciphertext, aad: &wrap_aad(context) })
        .map_err(|_| CryptoError::Decrypt)?;
    if plain.len() != KEY_LEN {
        plain.zeroize();
        return Err(CryptoError::Malformed);
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&plain);
    plain.zeroize();
    Ok(key)
}

fn checksum(key: &[u8; KEY_LEN]) -> [u8; 2] {
    use sha2::Digest;
    let digest = Sha256::digest(key);
    [digest[0], digest[1]]
}

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn base32_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in data {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(CROCKFORD[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(CROCKFORD[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in text.chars() {
        let value = match c.to_ascii_uppercase() {
            '-' | ' ' | '\t' | '\r' | '\n' => continue,
            'O' => 0,
            'I' | 'L' => 1,
            c => CROCKFORD.iter().position(|&x| x as char == c)? as u32,
        };
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST: KdfParams = KdfParams::INSECURE_FOR_TESTS;

    #[test]
    fn ableitung_ist_deterministisch_und_passwort_und_salt_abhaengig() {
        let key = MasterKey::derive("passwort", b"saltsaltsaltsalt", TEST).expect("kdf");
        let again = MasterKey::derive("passwort", b"saltsaltsaltsalt", TEST).expect("kdf");
        assert_eq!(key.as_bytes(), again.as_bytes());
        let other = MasterKey::derive("passwort2", b"saltsaltsaltsalt", TEST).expect("kdf");
        assert_ne!(key.as_bytes(), other.as_bytes());
        let other_salt = MasterKey::derive("passwort", b"saltsaltsaltsalu", TEST).expect("kdf");
        assert_ne!(key.as_bytes(), other_salt.as_bytes());
    }

    #[test]
    fn abgeleitete_schluessel_sind_getrennt() {
        let master = MasterKey::derive("passwort", b"saltsaltsaltsalt", TEST).expect("kdf");
        assert_ne!(master.auth_key().as_bytes(), master.kek().as_bytes());
        assert_ne!(master.auth_key().as_bytes(), master.as_bytes());
    }

    #[test]
    fn vault_key_verpacken_und_auspacken() {
        let master = MasterKey::derive("passwort", b"saltsaltsaltsalt", TEST).expect("kdf");
        let vault = VaultKey::generate().expect("zufall");
        let wrapped = vault.wrap(&master.kek(), b"tresor-1").expect("wrap");
        let back = VaultKey::unwrap(&wrapped, &master.kek(), b"tresor-1").expect("unwrap");
        assert_eq!(vault.as_bytes(), back.as_bytes());

        let wrong = MasterKey::derive("falsch", b"saltsaltsaltsalt", TEST).expect("kdf");
        assert_eq!(VaultKey::unwrap(&wrapped, &wrong.kek(), b"tresor-1").unwrap_err(), CryptoError::Decrypt);
        // In anderem Kontext (z. B. einem anderen Tresor) ist die Verpackung wertlos.
        assert_eq!(VaultKey::unwrap(&wrapped, &master.kek(), b"tresor-2").unwrap_err(), CryptoError::Decrypt);
    }

    #[test]
    fn manipulation_wird_erkannt() {
        let master = MasterKey::derive("passwort", b"saltsaltsaltsalt", TEST).expect("kdf");
        let vault = VaultKey::generate().expect("zufall");
        let mut wrapped = vault.wrap(&master.kek(), b"").expect("wrap");
        let last = wrapped.0.len() - 1;
        wrapped.0[last] ^= 1;
        assert_eq!(VaultKey::unwrap(&wrapped, &master.kek(), b"").unwrap_err(), CryptoError::Decrypt);
        wrapped.0[0] = 9;
        assert_eq!(VaultKey::unwrap(&wrapped, &master.kek(), b"").unwrap_err(), CryptoError::UnknownVersion(9));
        assert_eq!(VaultKey::unwrap(&WrappedKey(vec![]), &master.kek(), b"").unwrap_err(), CryptoError::Malformed);
    }

    #[test]
    fn wiederherstellungsschluessel() {
        let vault = VaultKey::generate().expect("zufall");
        let recovery = RecoveryKey::generate().expect("zufall");
        let wrapped = vault.wrap_for_recovery(&recovery, b"tresor-1").expect("wrap");

        let text = recovery.to_display_string();
        assert!(text.contains('-'));
        let sloppy = text.to_lowercase().replace('-', " ");
        let parsed = RecoveryKey::parse(&sloppy).expect("parse");
        let back = VaultKey::unwrap_with_recovery(&wrapped, &parsed, b"tresor-1").expect("unwrap");
        assert_eq!(vault.as_bytes(), back.as_bytes());
    }

    #[test]
    fn tippfehler_im_wiederherstellungsschluessel_faellt_auf() {
        let recovery = RecoveryKey::generate().expect("zufall");
        let text = recovery.to_display_string();
        let mut chars: Vec<char> = text.chars().collect();
        chars[0] = if chars[0] == '7' { '8' } else { '7' };
        let broken: String = chars.into_iter().collect();
        assert_eq!(RecoveryKey::parse(&broken).unwrap_err(), CryptoError::InvalidRecoveryKey);
        assert_eq!(RecoveryKey::parse("zu kurz").unwrap_err(), CryptoError::InvalidRecoveryKey);
        assert_eq!(RecoveryKey::parse("!!!").unwrap_err(), CryptoError::InvalidRecoveryKey);
    }

    proptest::proptest! {
        #[test]
        fn beliebiger_wiederherstellungsschluessel_stuerzt_nicht_ab(text in "\\PC{0,90}") {
            let _ = RecoveryKey::parse(&text);
        }

        #[test]
        fn wiederherstellungsschluessel_rundreise(bytes in proptest::prelude::any::<[u8; 32]>()) {
            let key = RecoveryKey::from_bytes(bytes);
            let parsed = RecoveryKey::parse(&key.to_display_string()).expect("parse");
            proptest::prop_assert_eq!(parsed.as_bytes(), key.as_bytes());
        }
    }

    #[test]
    fn abgeleitete_kennungen() {
        let key = VaultKey::from_bytes([1u8; 32]);
        assert_eq!(key.derive_id(b"host:22"), key.derive_id(b"host:22"));
        assert_ne!(key.derive_id(b"host:22"), key.derive_id(b"host:23"));
        assert_ne!(key.derive_id(b"host:22"), VaultKey::from_bytes([2u8; 32]).derive_id(b"host:22"));
    }

    #[test]
    fn kdf_parameter_grenzen() {
        assert!(KdfParams::DEFAULT.is_acceptable());
        assert!(!KdfParams::INSECURE_FOR_TESTS.is_acceptable());
        assert!(!KdfParams { memory_kib: u32::MAX, ..KdfParams::DEFAULT }.is_acceptable());
    }

    #[test]
    fn debug_ausgabe_verraet_nichts() {
        let vault = VaultKey::generate().expect("zufall");
        assert_eq!(format!("{vault:?}"), "VaultKey(…)");
    }

    #[test]
    fn base32_rundreise() {
        for len in 0..40 {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let decoded = base32_decode(&base32_encode(&data)).expect("decode");
            assert_eq!(&decoded[..data.len()], &data[..]);
        }
    }
}
