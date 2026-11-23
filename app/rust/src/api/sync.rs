//! Abgleich des Tresors mit einem Sync-Server.
//!
//! Die Oberfläche richtet die Verbindung ein und lässt den Abgleich laufen,
//! solange der Tresor offen ist. Was den Server erreicht, hat der Kern vorher
//! verschlüsselt; das Master-Passwort verlässt das Gerät nicht.

use crate::frb_generated::StreamSink;
use crate::internal::{app, core, RUNTIME};
use anyhow::{anyhow, Context, Result};
use flutter_rust_bridge::frb;
use std::path::PathBuf;
use tephra_sync::{Sync, SyncError, SyncReport};
use tokio::sync::watch;

/// Der Server, mit dem dieses Gerät abgleicht.
pub struct SyncConnection {
    pub server: String,
    pub username: String,
    pub device_id: String,
}

/// Ein Gerät des Kontos.
pub struct SyncDevice {
    pub id: String,
    pub name: String,
    /// Letzter Abgleich in Millisekunden seit 1970.
    pub last_seen_ms: i64,
    /// Ob es dieses Gerät ist.
    pub current: bool,
}

/// Was ein Abgleich bewegt hat.
pub struct SyncSummary {
    pub pulled: u32,
    pub pushed: u32,
    pub merged: u32,
    /// Datensätze vom Server, die sich nicht lesen ließen.
    pub unreadable: u32,
    /// Begründungen des Servers für abgelehnte Datensätze.
    pub rejected: Vec<String>,
}

/// Verlauf des fortlaufenden Abgleichs.
pub enum SyncEvent {
    Synced {
        summary: SyncSummary,
    },
    /// Ein Abgleich ist gescheitert. Er wird von selbst wiederholt – außer die
    /// Sitzung ist abgelaufen: Dann endet der Abgleich, bis das Gerät neu angemeldet ist.
    Failed {
        message: String,
        expired: bool,
    },
}

fn summary(report: SyncReport) -> SyncSummary {
    SyncSummary {
        pulled: report.pulled as u32,
        pushed: report.pushed as u32,
        merged: report.merged as u32,
        unreadable: report.unreadable as u32,
        rejected: report.rejected.into_iter().map(|(_, reason)| reason).collect(),
    }
}

fn client() -> Result<Sync> {
    Ok(Sync::new(core()?))
}

/// Nennt dem Abgleich die Zertifikate, denen das Betriebssystem vertraut. Nötig
/// unter Android, wo sie nur die Oberfläche erfragen kann. Gibt zurück, wie viele brauchbar sind.
#[frb(sync)]
pub fn sync_trust_roots(certificates: Vec<Vec<u8>>) -> u32 {
    tephra_sync::set_trust_roots(certificates) as u32
}

/// Mit welchem Server dieses Gerät abgleicht, falls überhaupt.
#[frb(sync)]
pub fn sync_connection() -> Result<Option<SyncConnection>> {
    Ok(client()?.connection()?.map(|connection| SyncConnection {
        server: connection.server,
        username: connection.username,
        device_id: connection.device_id.to_string(),
    }))
}

/// Ob die Adresse eine unverschlüsselte Verbindung zu einem anderen Rechner
/// bedeutet; die muss der Nutzer ausdrücklich erlauben.
#[frb(sync)]
pub fn sync_is_unencrypted(server: String) -> bool {
    tephra_sync::is_unencrypted(&server)
}

/// Legt für den offenen Tresor ein Konto auf dem Server an und lädt ihn hoch.
/// Das Passwort muss das Master-Passwort des Tresors sein. `allow_http` lässt
/// eine unverschlüsselte Adresse im eigenen Netz zu.
pub async fn sync_register(
    server: String,
    username: String,
    password: String,
    device_name: String,
    invite: Option<String>,
    allow_http: bool,
) -> Result<SyncSummary> {
    let sync = client()?;
    RUNTIME
        .spawn(async move {
            let invite = invite.as_deref().map(str::trim).filter(|code| !code.is_empty());
            sync.register(&server, &username, &password, &device_name, invite, allow_http).await?;
            Ok(summary(sync.sync_once().await?))
        })
        .await?
}

/// Richtet dieses Gerät für ein bestehendes Konto ein: legt unter `path` den
/// Tresor des Kontos an, holt alles ab und lässt ihn entsperrt.
pub async fn sync_join(
    path: String,
    server: String,
    username: String,
    password: String,
    device_name: String,
    allow_http: bool,
) -> Result<()> {
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Der Datenordner lässt sich nicht anlegen")?;
    }
    let core = RUNTIME
        .spawn(async move { Sync::join(&path, &server, &username, &password, &device_name, allow_http).await })
        .await??;
    app().core = Some(core);
    Ok(())
}

/// Meldet dieses Gerät neu an, wenn seine Sitzung abgelaufen ist oder das
/// Master-Passwort auf einem anderen Gerät geändert wurde.
pub async fn sync_reconnect(password: String, device_name: String) -> Result<()> {
    let sync = client()?;
    RUNTIME.spawn(async move { sync.reconnect(&password, &device_name).await }).await??;
    Ok(())
}

pub async fn sync_devices() -> Result<Vec<SyncDevice>> {
    let sync = client()?;
    let devices = RUNTIME.spawn(async move { sync.devices().await }).await??;
    Ok(devices
        .into_iter()
        .map(|device| SyncDevice {
            id: device.id.to_string(),
            name: device.name,
            last_seen_ms: device.last_seen_ms as i64,
            current: device.current,
        })
        .collect())
}

/// Entfernt ein Gerät vom Konto. Was es schon abgeholt hat, bleibt dort lesbar.
pub async fn sync_remove_device(device_id: String) -> Result<()> {
    let device = device_id.parse().map_err(|_| anyhow!("Ungültige Kennung"))?;
    let sync = client()?;
    RUNTIME.spawn(async move { sync.remove_device(device).await }).await??;
    Ok(())
}

/// Trennt dieses Gerät vom Server. Der Tresor bleibt, wie er ist.
pub async fn sync_disconnect() -> Result<()> {
    sync_unwatch();
    let sync = client()?;
    RUNTIME.spawn(async move { sync.disconnect().await }).await??;
    Ok(())
}

/// Gleicht fortlaufend ab: sofort, dann bei jeder Änderung hier oder auf dem
/// Server. Ein schon laufender Abgleich wird dabei ersetzt. Der Strom endet,
/// wenn der Abgleich angehalten wird oder die Sitzung abgelaufen ist.
pub fn sync_watch(sink: StreamSink<SyncEvent>) -> Result<()> {
    let sync = client()?;
    let (stop, stopped) = watch::channel(false);
    let task = RUNTIME.spawn(async move {
        // Der Sender lebt so lange wie der Abgleich; angehalten wird durch Abbruch der Aufgabe.
        let _stop = stop;
        sync.run(stopped, |result| {
            let event = match result {
                Ok(report) => SyncEvent::Synced { summary: summary(report.clone()) },
                Err(error) => SyncEvent::Failed {
                    message: error.to_string(),
                    expired: matches!(error, SyncError::SessionExpired | SyncError::NotConnected),
                },
            };
            let _ = sink.add(event);
        })
        .await;
    });
    if let Some(previous) = app().sync.replace(task.abort_handle()) {
        previous.abort();
    }
    Ok(())
}

/// Hält den fortlaufenden Abgleich an.
#[frb(sync)]
pub fn sync_unwatch() {
    if let Some(task) = app().sync.take() {
        task.abort();
    }
}
