//! Entsperren ohne Master-Passwort auf diesem Gerät.
//!
//! Der Vault Key wird so abgelegt, dass ihn nur dieses Gerät wieder öffnen
//! kann. Unter Windows verschlüsselt ihn die Datenschutz-API (DPAPI) mit dem
//! Benutzerkonto; die Datei liegt neben dem Tresor. Unter Android übernimmt
//! das der Keystore des Systems in der App selbst – dafür gibt es hier nur
//! den Schlüssel heraus und nimmt ihn zum Entsperren wieder an.

use crate::internal::{app, core, RUNTIME};
use anyhow::{anyhow, Result};
use flutter_rust_bridge::frb;
use std::path::{Path, PathBuf};
use tephra_core::{Core, VaultKey};

/// Ob die Rust-Seite den Schlüssel auf diesem Gerät selbst schützen kann (Windows).
#[frb(sync)]
pub fn device_unlock_native() -> bool {
    cfg!(windows)
}

fn stored_file(vault_path: &str) -> PathBuf {
    let vault = Path::new(vault_path);
    let name = vault.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "vault.tephra".into());
    vault.with_file_name(format!("{name}.device-unlock"))
}

/// Ob für den Tresor unter `vault_path` ein geschützter Schlüssel abgelegt ist.
#[frb(sync)]
pub fn device_unlock_stored(vault_path: String) -> bool {
    device_unlock_native() && stored_file(&vault_path).is_file()
}

/// Legt den Schlüssel des offenen Tresors geschützt neben ihm ab.
#[frb(sync)]
pub fn device_unlock_protect(vault_path: String) -> Result<()> {
    let key = core()?.vault_key()?;
    let sealed = dpapi::protect(key.as_bytes())?;
    let file = stored_file(&vault_path);
    std::fs::write(&file, sealed).map_err(|e| anyhow!("Die Datei {} lässt sich nicht schreiben: {e}", file.display()))
}

/// Entfernt den abgelegten Schlüssel. Danach braucht dieses Gerät wieder das Master-Passwort.
#[frb(sync)]
pub fn device_unlock_forget(vault_path: String) -> Result<()> {
    match std::fs::remove_file(stored_file(&vault_path)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Entsperrt mit dem abgelegten Schlüssel.
pub async fn device_unlock_with_stored(vault_path: String) -> Result<()> {
    let sealed = std::fs::read(stored_file(&vault_path))?;
    let mut opened = dpapi::unprotect(&sealed)?;
    let key = key_from(&opened);
    opened.fill(0);
    unlock_with(vault_path, key?).await
}

/// Der Schlüssel des offenen Tresors, damit die App ihn im Keystore von Android ablegt.
#[frb(sync)]
pub fn device_unlock_key() -> Result<Vec<u8>> {
    Ok(core()?.vault_key()?.as_bytes().to_vec())
}

/// Entsperrt mit einem Schlüssel, den die App aus dem Keystore von Android geholt hat.
pub async fn device_unlock_with_key(vault_path: String, mut key: Vec<u8>) -> Result<()> {
    let parsed = key_from(&key);
    key.fill(0);
    unlock_with(vault_path, parsed?).await
}

fn key_from(bytes: &[u8]) -> Result<VaultKey> {
    let array: [u8; 32] = bytes.try_into().map_err(|_| anyhow!("Der abgelegte Schlüssel ist beschädigt"))?;
    Ok(VaultKey::from_bytes(array))
}

async fn unlock_with(vault_path: String, key: VaultKey) -> Result<()> {
    let existing = app().core.clone();
    let core = RUNTIME
        .spawn_blocking(move || -> Result<Core> {
            let core = match existing {
                Some(core) => core,
                None => Core::open(&PathBuf::from(vault_path))?,
            };
            core.unlock_with_key(key).map_err(|_| anyhow!("Der abgelegte Schlüssel passt nicht zu diesem Tresor"))?;
            Ok(core)
        })
        .await??;
    app().core = Some(core);
    Ok(())
}

#[cfg(windows)]
mod dpapi {
    use anyhow::{bail, Result};
    use windows_sys::Win32::Foundation::{LocalFree, HLOCAL};
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    /// Zusätzlicher Wert, ohne den sich der Inhalt nicht öffnen lässt – so
    /// öffnet kein anderes Programm desselben Benutzers ihn versehentlich mit
    /// seiner eigenen Erwartung.
    const ENTROPY: &[u8] = b"tephra/device-unlock/v1";

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr().cast_mut() }
    }

    /// Kopiert das Ergebnis und gibt den Speicher frei, den Windows dafür angelegt hat.
    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        // SAFETY: Windows hat `cbData` Byte ab `pbData` gefüllt; danach gehört der Speicher uns.
        let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        // SAFETY: Der Speicher stammt von LocalAlloc innerhalb von CryptProtectData/CryptUnprotectData.
        unsafe {
            std::ptr::write_bytes(out.pbData, 0, out.cbData as usize);
            LocalFree(out.pbData as HLOCAL);
        }
        bytes
    }

    pub(super) fn protect(data: &[u8]) -> Result<Vec<u8>> {
        let input = blob(data);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: Alle Zeiger gelten für die Dauer des Aufrufs; Windows liest sie nur.
        let ok = unsafe {
            CryptProtectData(
                &input,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            bail!("Windows konnte den Schlüssel nicht schützen ({})", std::io::Error::last_os_error());
        }
        Ok(take(out))
    }

    pub(super) fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
        let input = blob(data);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: wie oben.
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            bail!(
                "Windows konnte den abgelegten Schlüssel nicht öffnen – gehört er zu einem anderen Benutzerkonto? ({})",
                std::io::Error::last_os_error()
            );
        }
        Ok(take(out))
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn schuetzen_und_wieder_oeffnen() {
            let secret = [7u8; 32];
            let sealed = super::protect(&secret).expect("schützen");
            assert_ne!(sealed.as_slice(), secret.as_slice());
            assert!(!sealed.windows(32).any(|w| w == secret), "der Schlüssel steht im Klartext darin");
            assert_eq!(super::unprotect(&sealed).expect("öffnen"), secret);
            let mut broken = sealed.clone();
            let last = broken.len() - 1;
            broken[last] ^= 1;
            assert!(super::unprotect(&broken).is_err());
        }
    }
}

#[cfg(not(windows))]
mod dpapi {
    use anyhow::{bail, Result};

    pub(super) fn protect(_data: &[u8]) -> Result<Vec<u8>> {
        bail!("Auf diesem System schützt die App den Schlüssel selbst")
    }

    pub(super) fn unprotect(_data: &[u8]) -> Result<Vec<u8>> {
        bail!("Auf diesem System schützt die App den Schlüssel selbst")
    }
}

#[cfg(test)]
mod tests {
    use super::stored_file;

    #[test]
    fn die_datei_liegt_neben_dem_tresor() {
        let file = stored_file("/daten/vault.tephra");
        assert_eq!(file, std::path::Path::new("/daten/vault.tephra.device-unlock"));
    }
}
