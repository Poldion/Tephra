//! Verwaltung der WireGuard-Tunnel: ein Tunnel je VPN-Profil, gestartet bei
//! Bedarf, wiederverwendet von allen Verbindungen, abgebaut nach Leerlauf.

use crate::{Core, CoreError, Result};
use std::time::{Duration, Instant};
use tephra_model::{ItemId, VpnProfile};
use tephra_transport::wireguard::{PeerStatus, WgConfig, WgTunnel};

pub(crate) struct Entry {
    tunnel: WgTunnel,
    /// Profil, mit dem der Tunnel gestartet wurde; ändert es sich, wird neu gestartet.
    profile: VpnProfile,
    last_used: Instant,
}

/// Zustand eines laufenden Tunnels für Statusanzeigen.
#[derive(Debug, Clone, PartialEq)]
pub struct TunnelState {
    pub profile_id: ItemId,
    pub label: String,
    /// Anzahl der Verbindungen, die den Tunnel gerade benutzen.
    pub connections: usize,
    pub peers: Vec<PeerStatus>,
}

impl Core {
    /// Liefert den Tunnel zu einem VPN-Profil und startet ihn bei Bedarf.
    pub async fn tunnel(&self, profile_id: ItemId) -> Result<WgTunnel> {
        let profile = self
            .with_vault(|v| v.get::<VpnProfile>(profile_id))?
            .ok_or_else(|| CoreError::NotFound("VPN-Profil".into()))?;
        let mut tunnels = self.inner.tunnels.lock().await;
        if let Some(entry) = tunnels.get_mut(&profile_id)
            && entry.profile == profile
        {
            entry.last_used = Instant::now();
            return Ok(entry.tunnel.clone());
        }
        let tunnel = WgTunnel::start(WgConfig::from_profile(&profile)?).await?;
        tunnels.insert(profile_id, Entry { tunnel: tunnel.clone(), profile, last_used: Instant::now() });
        Ok(tunnel)
    }

    pub async fn tunnel_states(&self) -> Vec<TunnelState> {
        self.inner
            .tunnels
            .lock()
            .await
            .iter()
            .map(|(id, entry)| TunnelState {
                profile_id: *id,
                label: entry.profile.label.clone(),
                // Die Verwaltung hält selbst eine Referenz.
                connections: entry.tunnel.holders().saturating_sub(1),
                peers: entry.tunnel.status(),
            })
            .collect()
    }

    /// Baut Tunnel ab, die seit `idle` von keiner Verbindung mehr benutzt
    /// werden. Gibt die Anzahl der abgebauten Tunnel zurück.
    pub async fn close_idle_tunnels(&self, idle: Duration) -> usize {
        let mut tunnels = self.inner.tunnels.lock().await;
        let before = tunnels.len();
        tunnels.retain(|_, entry| entry.tunnel.holders() > 1 || entry.last_used.elapsed() < idle);
        before - tunnels.len()
    }

    /// Entfernt einen Tunnel aus der Verwaltung. Er endet, sobald die letzte
    /// Verbindung, die ihn benutzt, geschlossen ist.
    pub async fn close_tunnel(&self, profile_id: ItemId) {
        self.inner.tunnels.lock().await.remove(&profile_id);
    }

    pub async fn close_all_tunnels(&self) {
        self.inner.tunnels.lock().await.clear();
    }
}
