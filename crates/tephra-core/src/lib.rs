//! Kern von Tephra.
//!
//! [`Core`] bündelt Tresor, Tunnelverwaltung und SSH-Verbindungen zu einer
//! Schnittstelle, auf der Oberfläche, Kommandozeile und MCP-Server aufsetzen.

mod connect;
mod import;
mod keep;
#[cfg(test)]
mod tests;
mod tunnels;

pub use connect::{ConnectUi, HostKeyQuestion, HostKeyState, NonInteractive, ResolvedHost};
pub use import::{ImportReport, SshConfigHost, parse_ssh_config};
pub use keep::{session_command, tmux_session_name};
pub use tunnels::TunnelState;

pub use tephra_crypto::{KdfParams, RecoveryKey, VaultKey};
pub use tephra_model as model;
pub use tephra_ssh as ssh;
pub use tephra_transport as transport;
pub use tephra_vault::{Vault, VaultError};

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use tephra_model::ItemId;
use tephra_ssh::{SshClient, SshError};
use tephra_transport::TransportError;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    Ssh(#[from] SshError),
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error("{0} nicht gefunden")]
    NotFound(String),
    #[error("{0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;

fn settings_id() -> ItemId {
    ItemId(uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, b"tephra/settings"))
}

struct Inner {
    vault: Mutex<Vault>,
    tunnels: tokio::sync::Mutex<HashMap<ItemId, tunnels::Entry>>,
    /// Offene Verbindungen zur Wiederverwendung, je Host eine, mit dem
    /// Zeitpunkt der letzten Benutzung.
    pool: tokio::sync::Mutex<HashMap<ItemId, (Arc<SshClient>, std::time::Instant)>>,
}

/// Zentrale Instanz; günstig zu klonen und zwischen Aufgaben zu teilen.
#[derive(Clone)]
pub struct Core {
    inner: Arc<Inner>,
}

impl Core {
    /// Öffnet einen bestehenden Tresor im gesperrten Zustand.
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self::from_vault(Vault::open(path)?))
    }

    /// Legt einen neuen Tresor an. Der zurückgegebene
    /// Wiederherstellungsschlüssel muss dem Nutzer angezeigt werden.
    pub fn create(path: &Path, password: &str, kdf: KdfParams) -> Result<(Self, RecoveryKey)> {
        let (vault, recovery) = Vault::create(path, password, kdf)?;
        Ok((Self::from_vault(vault), recovery))
    }

    pub fn from_vault(vault: Vault) -> Self {
        Self {
            inner: Arc::new(Inner {
                vault: Mutex::new(vault),
                tunnels: tokio::sync::Mutex::new(HashMap::new()),
                pool: tokio::sync::Mutex::new(HashMap::new()),
            }),
        }
    }

    fn vault_guard(&self) -> MutexGuard<'_, Vault> {
        self.inner.vault.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Führt eine Operation auf dem Tresor aus. Der Tresor ist währenddessen
    /// gesperrt; die Funktion sollte deshalb kurz sein und nicht warten.
    pub fn with_vault<R>(&self, f: impl FnOnce(&mut Vault) -> std::result::Result<R, VaultError>) -> Result<R> {
        let mut vault = self.vault_guard();
        // Änderungen anderer Prozesse an derselben Tresordatei übernehmen.
        vault.refresh()?;
        Ok(f(&mut vault)?)
    }

    pub fn is_unlocked(&self) -> bool {
        self.vault_guard().is_unlocked()
    }

    /// Geräteübergreifende Einstellungen. Es gibt genau einen Datensatz mit
    /// fester Kennung, damit ihn alle Geräte gemeinsam bearbeiten.
    pub fn settings(&self) -> Result<tephra_model::Settings> {
        Ok(self.with_vault(|v| v.get::<tephra_model::Settings>(settings_id()))?.unwrap_or_default())
    }

    pub fn set_settings(&self, settings: &tephra_model::Settings) -> Result<()> {
        self.with_vault(|v| v.upsert_with_id(settings_id(), settings).map(|_| ()))
    }

    pub fn unlock(&self, password: &str) -> Result<()> {
        Ok(self.vault_guard().unlock(password)?)
    }

    /// Entsperrt mit dem Vault Key selbst, etwa aus dem geschützten
    /// Speicher dieses Geräts. Ein Schlüssel, der nicht zu diesem Tresor
    /// gehört, wird abgelehnt, bevor er irgendetwas versiegelt.
    pub fn unlock_with_key(&self, key: VaultKey) -> Result<()> {
        Ok(self.vault_guard().unlock_with_key(key)?)
    }

    /// Kopie des Vault Keys des entsperrten Tresors, um ihn geschützt auf
    /// diesem Gerät abzulegen. Er bleibt gleich, wenn das Master-Passwort
    /// geändert wird.
    pub fn vault_key(&self) -> Result<VaultKey> {
        Ok(self.vault_guard().vault_key()?)
    }

    /// Sperrt den Tresor und beendet alle Verbindungen und Tunnel, damit
    /// nach dem Sperren nichts mehr mit den Zugangsdaten arbeitet.
    pub async fn lock(&self) {
        // Zuerst sperren, damit währenddessen keine neue Verbindung entsteht.
        self.vault_guard().lock();
        self.disconnect_all().await;
        self.close_all_tunnels().await;
    }

    /// Verbindung zu einem Host, bevorzugt eine bereits bestehende.
    pub async fn connect_pooled(&self, host_id: ItemId, ui: Arc<dyn ConnectUi>) -> Result<Arc<SshClient>> {
        if let Some((existing, used)) = self.inner.pool.lock().await.get_mut(&host_id)
            && !existing.is_closed()
        {
            *used = std::time::Instant::now();
            return Ok(existing.clone());
        }
        let client = self.connect(host_id, ui).await?;
        // Wurde der Tresor währenddessen gesperrt, darf die Verbindung nicht bestehen bleiben.
        if !self.is_unlocked() {
            client.disconnect().await;
            return Err(VaultError::Locked.into());
        }
        let previous = self.inner.pool.lock().await.insert(host_id, (client.clone(), std::time::Instant::now()));
        // Haben zwei Aufrufe gleichzeitig verbunden, bleibt nur eine Verbindung im Bestand.
        if let Some((previous, _)) = previous
            && Arc::strong_count(&previous) == 1
        {
            previous.disconnect().await;
        }
        Ok(client)
    }

    /// Hosts, zu denen gerade eine wiederverwendbare Verbindung besteht.
    pub async fn pooled_hosts(&self) -> Vec<ItemId> {
        self.inner.pool.lock().await.iter().filter(|(_, (c, _))| !c.is_closed()).map(|(id, _)| *id).collect()
    }

    pub async fn disconnect(&self, host_id: ItemId) {
        if let Some((client, _)) = self.inner.pool.lock().await.remove(&host_id) {
            client.disconnect().await;
        }
    }

    /// Trennt Verbindungen, die seit `idle` niemand mehr benutzt hat und die
    /// gerade nirgends in Gebrauch sind. Gibt die Anzahl zurück.
    pub async fn disconnect_idle(&self, idle: std::time::Duration) -> usize {
        let stale: Vec<Arc<SshClient>> = {
            let mut pool = self.inner.pool.lock().await;
            let ids: Vec<ItemId> = pool
                .iter()
                .filter(|(_, (client, used))| {
                    client.is_closed() || (Arc::strong_count(client) == 1 && used.elapsed() >= idle)
                })
                .map(|(id, _)| *id)
                .collect();
            ids.into_iter().filter_map(|id| pool.remove(&id)).map(|(client, _)| client).collect()
        };
        for client in &stale {
            client.disconnect().await;
        }
        stale.len()
    }

    pub async fn disconnect_all(&self) {
        let clients: Vec<_> = self.inner.pool.lock().await.drain().map(|(_, (c, _))| c).collect();
        for client in clients {
            client.disconnect().await;
        }
    }
}
