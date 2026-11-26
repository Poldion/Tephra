//! Fernverwaltung der MCP-Freigaben über den abgeglichenen Tresor.
//!
//! Grundsatz bleibt: Freigaben gelten je Gerät, und ein Gerät, auf dem LLM-Zugriff eingerichtet wird, schaltet ihn
//! nicht stillschweigend auf anderen frei. Ein Gerät kann aber selbst zustimmen, aus der Ferne verwaltet zu werden
//! (`tephra mcp remote on`). Dann
//!
//! * liegen seine Freigaben in einem abgeglichenen Datensatz [`McpDevice`] statt im lokalen Geheimnis, und jedes
//!   Gerät mit diesem Tresor kann sie ändern – etwa das Telefon für einen Dienst in einem Container ohne Bildschirm;
//! * gehen Rückfragen („Mit Bestätigung“) als Datensatz [`McpApproval`] an die anderen Geräte, und die erste
//!   Antwort gilt.
//!
//! Die Zustimmung selbst steht nur lokal (Geheimnis `mcp/remote`); kein anderes Gerät kann sie erteilen. Fehlt der
//! Datensatz eines fernverwalteten Geräts (von einem anderen Gerät entfernt), ist dort nichts mehr erlaubt.

use crate::policy::{ClientPolicy, Limits, McpPolicy, now_ms};
use crate::server::{Action, ApprovalAnswer, ApprovalRequest, Approver};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tephra_core::model::{ItemId, McpApproval, McpDevice};
use tephra_core::{Core, CoreError};

const REMOTE_SECRET: &str = "mcp/remote";
/// So lange wartet eine Rückfrage auf eine Antwort von einem anderen Gerät.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);
/// So oft schaut das Gerät nach, ob eine Antwort eingetroffen ist (nur im eigenen Tresor; der Abgleich holt sie).
const POLL: Duration = Duration::from_millis(500);
/// Längste Texte einer Rückfrage; der Datensatz soll klein bleiben.
const MAX_SUBJECT: usize = 8000;
const MAX_EXTRA: usize = 4000;
/// Rückfragen, deren Frist so lange vorbei ist, räumt jedes Gerät weg (etwa von einem Gerät, das abgestürzt ist).
const STALE_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Local {
    device: ItemId,
}

/// Der Datensatz, über den dieses Gerät verwaltet wird; `None`, wenn es nicht fernverwaltet ist.
pub fn managing_device(core: &Core) -> Result<Option<ItemId>, CoreError> {
    let Some(bytes) = core.with_vault(|v| v.get_secret(REMOTE_SECRET))? else { return Ok(None) };
    let local: Local = serde_json::from_slice(&bytes)
        .map_err(|e| CoreError::Config(format!("Die Einstellung der Fernverwaltung ist beschädigt: {e}")))?;
    Ok(Some(local.device))
}

/// Liest die Freigaben aus dem Datensatz eines Geräts. Was sich nicht lesen lässt, gilt als nichts erlaubt.
pub(crate) fn policy_of(device: &McpDevice) -> McpPolicy {
    let clients: Option<Vec<ClientPolicy>> = serde_json::from_value(device.clients.clone()).ok();
    let limits: Limits = serde_json::from_value(device.limits.clone()).unwrap_or_default();
    match clients {
        Some(clients) => McpPolicy { enabled: device.enabled, clients, limits },
        None => McpPolicy { enabled: false, clients: Vec::new(), limits },
    }
}

fn write_policy(device: &mut McpDevice, policy: &McpPolicy) -> Result<(), CoreError> {
    device.enabled = policy.enabled;
    device.clients = serde_json::to_value(&policy.clients).map_err(|e| CoreError::Config(e.to_string()))?;
    device.limits = serde_json::to_value(&policy.limits).map_err(|e| CoreError::Config(e.to_string()))?;
    Ok(())
}

/// Freigaben eines fernverwalteten Geräts; `None`, wenn es den Datensatz nicht (mehr) gibt.
pub fn load_device(core: &Core, id: ItemId) -> Result<Option<McpPolicy>, CoreError> {
    Ok(core.with_vault(|v| v.get::<McpDevice>(id))?.as_ref().map(policy_of))
}

/// Speichert die Freigaben eines fernverwalteten Geräts. Der Abgleich bringt sie dorthin.
pub fn save_device(core: &Core, id: ItemId, policy: &McpPolicy) -> Result<(), CoreError> {
    let mut device = core
        .with_vault(|v| v.get::<McpDevice>(id))?
        .ok_or_else(|| CoreError::Config("Dieses Gerät wird nicht mehr aus der Ferne verwaltet".into()))?;
    write_policy(&mut device, policy)?;
    core.with_vault(|v| v.update(id, &device).map(|_| ()))
}

/// Ein fernverwaltetes Gerät, wie die Verwaltung es zeigt.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceSummary {
    pub id: ItemId,
    pub name: String,
    pub enabled: bool,
    pub clients: usize,
    pub seen_ms: u64,
    pub version: String,
    pub remote_approvals: bool,
    /// Ob es dieses Gerät selbst ist.
    pub this_device: bool,
}

/// Alle fernverwalteten Geräte dieses Tresors, nach Namen.
pub fn devices(core: &Core) -> Result<Vec<DeviceSummary>, CoreError> {
    let own = managing_device(core)?;
    let mut list: Vec<DeviceSummary> = core
        .with_vault(|v| v.list::<McpDevice>())?
        .into_iter()
        .map(|(id, device)| {
            let policy = policy_of(&device);
            DeviceSummary {
                id,
                name: device.name,
                enabled: policy.enabled,
                clients: policy.clients.len(),
                seen_ms: device.seen_ms,
                version: device.version,
                remote_approvals: device.remote_approvals,
                this_device: own == Some(id),
            }
        })
        .collect();
    list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.0.cmp(&b.id.0)));
    Ok(list)
}

/// Stimmt auf diesem Gerät der Fernverwaltung zu. Die bisherigen Freigaben ziehen in den abgeglichenen Datensatz
/// um. Ist das Gerät schon fernverwaltet, wird nur der Name aktualisiert. Gibt die Kennung des Datensatzes zurück.
pub fn enable(core: &Core, name: &str, version: &str) -> Result<ItemId, CoreError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::Config("Das Gerät braucht einen Namen".into()));
    }
    if let Some(id) = managing_device(core)? {
        if let Some(mut device) = core.with_vault(|v| v.get::<McpDevice>(id))? {
            device.name = name.to_string();
            device.version = version.to_string();
            device.seen_ms = now_ms();
            core.with_vault(|v| v.update(id, &device).map(|_| ()))?;
            return Ok(id);
        }
        // Der Datensatz wurde inzwischen entfernt: neu anlegen, mit allem gesperrt.
        let mut device = new_device(name, version);
        write_policy(&mut device, &McpPolicy::locked())?;
        let id = core.with_vault(|v| v.insert(&device))?;
        store_local(core, id)?;
        return Ok(id);
    }
    let mut device = new_device(name, version);
    write_policy(&mut device, &McpPolicy::load_local(core)?)?;
    let id = core.with_vault(|v| v.insert(&device))?;
    store_local(core, id)?;
    Ok(id)
}

fn new_device(name: &str, version: &str) -> McpDevice {
    McpDevice {
        name: name.to_string(),
        seen_ms: now_ms(),
        version: version.to_string(),
        remote_approvals: true,
        ..McpDevice::default()
    }
}

fn store_local(core: &Core, device: ItemId) -> Result<(), CoreError> {
    let bytes = serde_json::to_vec(&Local { device }).map_err(|e| CoreError::Config(e.to_string()))?;
    core.with_vault(|v| v.put_secret(REMOTE_SECRET, &bytes))
}

/// Nimmt die Zustimmung zurück: Die Freigaben ziehen wieder in das lokale Geheimnis, der abgeglichene Datensatz
/// wird gelöscht. Hat ein anderes Gerät den Datensatz schon entfernt, bleibt der Zugriff hier abgeschaltet.
/// `false`, wenn das Gerät gar nicht fernverwaltet war.
pub fn disable(core: &Core) -> Result<bool, CoreError> {
    let Some(id) = managing_device(core)? else { return Ok(false) };
    let policy = match load_device(core, id)? {
        Some(policy) => policy,
        None => McpPolicy { enabled: false, ..McpPolicy::load_local(core)? },
    };
    policy.save_local(core)?;
    if core.with_vault(|v| v.get::<McpDevice>(id))?.is_some() {
        core.with_vault(|v| v.delete(id))?;
    }
    core.with_vault(|v| v.delete_secret(REMOTE_SECRET))?;
    Ok(true)
}

/// Entfernt ein Gerät aus der Verwaltung (von einem anderen Gerät aus). Dort ist danach nichts mehr erlaubt, bis
/// es selbst wieder zustimmt.
pub fn remove_device(core: &Core, id: ItemId) -> Result<(), CoreError> {
    if core.with_vault(|v| v.get::<McpDevice>(id))?.is_none() {
        return Err(CoreError::Config("Dieses Gerät gibt es in der Verwaltung nicht mehr".into()));
    }
    core.with_vault(|v| v.delete(id))
}

/// Hinterlässt ein Lebenszeichen im Datensatz dieses Geräts, damit die Verwaltung zeigt, wann es zuletzt lief.
pub fn heartbeat(core: &Core, version: &str) -> Result<(), CoreError> {
    let Some(id) = managing_device(core)? else { return Ok(()) };
    let Some(mut device) = core.with_vault(|v| v.get::<McpDevice>(id))? else { return Ok(()) };
    device.seen_ms = now_ms();
    device.version = version.to_string();
    device.remote_approvals = true;
    core.with_vault(|v| v.update(id, &device).map(|_| ()))
}

/// Räumt Rückfragen weg: alle eigenen (nach einem Neustart wartet niemand mehr darauf) und fremde, deren Frist
/// lange vorbei ist. Gibt die Zahl der gelöschten zurück.
pub fn cleanup(core: &Core) -> Result<usize, CoreError> {
    let own = managing_device(core)?.map(|id| id.to_string());
    let now = now_ms();
    let stale: Vec<ItemId> = core
        .with_vault(|v| v.list::<McpApproval>())?
        .into_iter()
        .filter(|(_, a)| own.as_deref() == Some(a.device.as_str()) || a.expires_ms.saturating_add(STALE_MS) < now)
        .map(|(id, _)| id)
        .collect();
    for id in &stale {
        core.with_vault(|v| v.delete(*id))?;
    }
    Ok(stale.len())
}

/// Räumt nur Rückfragen weg, deren Frist lange vorbei ist; für den laufenden Betrieb, in dem eigene Rückfragen
/// noch offen sein können.
pub fn cleanup_stale(core: &Core) -> Result<usize, CoreError> {
    let now = now_ms();
    let stale: Vec<ItemId> = core
        .with_vault(|v| v.list::<McpApproval>())?
        .into_iter()
        .filter(|(_, a)| a.expires_ms.saturating_add(STALE_MS) < now)
        .map(|(id, _)| id)
        .collect();
    for id in &stale {
        core.with_vault(|v| v.delete(*id))?;
    }
    Ok(stale.len())
}

/// Rückfragen anderer Geräte, auf die noch niemand geantwortet hat und deren Frist läuft, älteste zuerst.
pub fn pending_approvals(core: &Core) -> Result<Vec<(ItemId, McpApproval)>, CoreError> {
    let own = managing_device(core)?.map(|id| id.to_string());
    let now = now_ms();
    let mut list: Vec<(ItemId, McpApproval)> = core
        .with_vault(|v| v.list::<McpApproval>())?
        .into_iter()
        .filter(|(_, a)| a.answer.is_empty() && a.expires_ms > now && own.as_deref() != Some(a.device.as_str()))
        .collect();
    list.sort_by_key(|(_, a)| a.created_ms);
    Ok(list)
}

/// Beantwortet die Rückfrage eines anderen Geräts. `false`, wenn sie schon beantwortet, abgelaufen oder weg ist.
pub fn answer(core: &Core, id: ItemId, reply: ApprovalAnswer, by: &str) -> Result<bool, CoreError> {
    let Some(mut approval) = core.with_vault(|v| v.get::<McpApproval>(id))? else { return Ok(false) };
    if !approval.answer.is_empty() || approval.expires_ms <= now_ms() {
        return Ok(false);
    }
    approval.answer = match reply {
        ApprovalAnswer::Deny => "deny",
        ApprovalAnswer::Once => "once",
        ApprovalAnswer::Session => "session",
    }
    .to_string();
    approval.answered_by = by.trim().to_string();
    core.with_vault(|v| v.update(id, &approval).map(|_| ()))?;
    Ok(true)
}

fn cut(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push_str(" …[gekürzt]");
    out
}

/// Beschreibt eine Anfrage für den Datensatz. Gekürzt wird nur, was zu groß wäre; entschärft wird beim Anzeigen.
fn describe(request: &ApprovalRequest, device: ItemId, device_name: &str) -> McpApproval {
    let size = |bytes: usize| Some(bytes as u64);
    let (kind, subject, extra, bytes, checksum) = match &request.action {
        Action::Command { command, stdin } => (
            "command",
            cut(command, MAX_SUBJECT),
            stdin.as_deref().map(|s| cut(s, MAX_EXTRA)),
            stdin.as_ref().and_then(|s| size(s.len())),
            None,
        ),
        Action::Session => ("session", String::new(), None, None, None),
        Action::FileRead { path } => ("file_read", cut(path, MAX_SUBJECT), None, None, None),
        Action::FileWrite { path, bytes, sha256, preview } => (
            "file_write",
            cut(path, MAX_SUBJECT),
            Some(cut(preview, MAX_EXTRA)),
            size(*bytes),
            Some(sha256.chars().take(16).collect()),
        ),
        Action::TrustHostKey { algorithm, fingerprint } => {
            ("host_key", format!("{algorithm} {fingerprint}"), None, None, None)
        }
    };
    let now = now_ms();
    McpApproval {
        device: device.to_string(),
        device_name: device_name.to_string(),
        client: cut(&request.client_name, 200),
        host: cut(&request.host_label, 200),
        kind: kind.to_string(),
        subject,
        extra,
        bytes,
        checksum,
        created_ms: now,
        expires_ms: now + APPROVAL_TIMEOUT.as_millis() as u64,
        answer: String::new(),
        answered_by: String::new(),
    }
}

/// Gibt Rückfragen an die anderen Geräte weiter und wartet auf die erste Antwort. Ohne Antwort binnen
/// [`APPROVAL_TIMEOUT`] gilt die Anfrage als abgelehnt. Die Rückfrage wird danach immer gelöscht.
pub struct RemoteApprover {
    core: Core,
}

impl RemoteApprover {
    pub fn new(core: Core) -> Self {
        Self { core }
    }
}

/// Löscht die Rückfrage, sobald sie erledigt ist – auch wenn die wartende Aufgabe abgebrochen wird.
struct Withdraw(Core, ItemId);

impl Drop for Withdraw {
    fn drop(&mut self) {
        let _ = self.0.with_vault(|v| match v.get::<McpApproval>(self.1)? {
            Some(_) => v.delete(self.1),
            None => Ok(()),
        });
    }
}

#[async_trait]
impl Approver for RemoteApprover {
    async fn approve(&self, request: &ApprovalRequest) -> ApprovalAnswer {
        let Ok(Some(device)) = managing_device(&self.core) else { return ApprovalAnswer::Deny };
        let name = self.core.with_vault(|v| v.get::<McpDevice>(device)).ok().flatten().map(|d| d.name);
        let Some(name) = name else { return ApprovalAnswer::Deny };
        let approval = describe(request, device, &name);
        let expires = approval.expires_ms;
        let Ok(id) = self.core.with_vault(|v| v.insert(&approval)) else { return ApprovalAnswer::Deny };
        let _withdraw = Withdraw(self.core.clone(), id);
        loop {
            tokio::time::sleep(POLL).await;
            let current = match self.core.with_vault(|v| v.get::<McpApproval>(id)) {
                Ok(Some(current)) => current,
                // Weg (von einem anderen Gerät aufgeräumt) oder nicht lesbar: niemand hat zugestimmt.
                _ => return ApprovalAnswer::Deny,
            };
            match current.answer.as_str() {
                "once" => return ApprovalAnswer::Once,
                "session" => return ApprovalAnswer::Session,
                "deny" => return ApprovalAnswer::Deny,
                _ if now_ms() >= expires => return ApprovalAnswer::Deny,
                _ => {}
            }
        }
    }
}

/// Fragt zwei Stellen zugleich (etwa das Terminal des Dienstes und die anderen Geräte); die erste Antwort gilt.
pub struct FirstAnswer(pub Arc<dyn Approver>, pub Arc<dyn Approver>);

#[async_trait]
impl Approver for FirstAnswer {
    async fn approve(&self, request: &ApprovalRequest) -> ApprovalAnswer {
        tokio::select! {
            answer = self.0.approve(request) => answer,
            answer = self.1.approve(request) => answer,
        }
    }
}
