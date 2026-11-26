//! MCP in der App: der Dienst, über den Sprachmodelle auf freigegebene Hosts
//! zugreifen, die Rückfragen an den Nutzer und die Verwaltung der Freigaben.
//!
//! Der Dienst ist derselbe wie hinter `tephra daemon`. Er läuft, solange die
//! Oberfläche ihn eingeschaltet hat und der Tresor offen ist; Rückfragen
//! gehen als Ereignisse an die Oberfläche.

use crate::frb_generated::StreamSink;
use crate::internal::{core, next_id, parse_id, RUNTIME};
use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use flutter_rust_bridge::frb;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tephra_core::model::{Host, ItemId};
use tephra_mcp::audit::{self, Outcome};
use tephra_mcp::policy::{AccessLevel, HostGrant, McpPolicy};
use tephra_mcp::remote;
use tephra_mcp::service::{Listener, Observer, ServiceEvent};
use tephra_mcp::{line_count, printable, printable_lines, Action, ApprovalAnswer, ApprovalRequest, Approver};
use tokio::sync::oneshot;
use tokio::task::AbortHandle;

/// So lange hat der Nutzer Zeit für eine Antwort; danach gilt die Anfrage als abgelehnt.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);
/// So lange nach dem Erscheinen einer Rückfrage zählt keine Antwort: Ein
/// Klick oder Tastendruck, der schon unterwegs war, galt nicht dieser Frage.
const SETTLE: Duration = Duration::from_millis(600);
/// Längste Anzeige von Befehlen, Eingabedaten und Dateiinhalten in der Rückfrage. Längere Befehle legt der Dienst
/// gar nicht erst vor; die Anzeige ist also vollständig.
const SHOWN_CHARS: usize = tephra_mcp::MAX_APPROVAL_CHARS;

// ------------------------------------------------------------------ Dienst

/// Worum eine Rückfrage bittet.
pub enum McpRequestKind {
    Command,
    /// Interaktive Sitzung: Danach sind alle Eingaben des Modells darin frei.
    Session,
    FileRead,
    FileWrite,
    /// Einem unbekannten Hostschlüssel vertrauen.
    HostKey,
}

/// Eine Rückfrage an den Nutzer. Alle Texte, die vom Modell stammen, sind
/// schon so umgeschrieben, dass sie die Anzeige nicht verändern können.
pub struct McpRequest {
    pub id: u32,
    /// Name des Geräts, das fragt; leer für den Dienst dieser App.
    pub device: String,
    pub client: String,
    pub host: String,
    pub kind: McpRequestKind,
    /// Der Befehl, der Pfad oder der Hostschlüssel.
    pub subject: String,
    /// Eingabedaten eines Befehls oder der Anfang des Inhalts einer Datei.
    pub extra: Option<String>,
    /// Größe der Eingabedaten oder der Datei in Byte.
    pub bytes: Option<u32>,
    /// Anfang der SHA-256-Prüfsumme der Datei, die geschrieben werden soll.
    pub checksum: Option<String>,
    /// Zeilen und Zeichen von `subject` und `extra` im Original, damit die Oberfläche auf mehrzeilige Befehle
    /// hinweisen kann.
    pub subject_lines: u32,
    pub subject_chars: u32,
    pub extra_lines: u32,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

pub enum McpEvent {
    /// Der Kanal ist offen; Clients können sich verbinden.
    Started,
    Request {
        request: McpRequest,
    },
    /// Die Rückfrage hat sich erledigt (beantwortet, Frist abgelaufen oder Dienst beendet).
    Withdrawn {
        id: u32,
    },
    Connected {
        client: String,
    },
    Disconnected {
        client: String,
    },
    /// Der Dienst ist zu Ende; `message` nennt den Grund, wenn es ein Fehler war.
    Stopped {
        message: Option<String>,
    },
}

pub enum McpAnswer {
    Deny,
    /// Nur dieses eine Mal.
    Once,
    /// Genau dasselbe für den Rest dieser MCP-Verbindung ohne weitere Rückfrage.
    Session,
}

type Pending = Arc<Mutex<HashMap<u32, (Instant, oneshot::Sender<ApprovalAnswer>)>>>;

struct Service {
    task: AbortHandle,
    reaper: AbortHandle,
    pending: Pending,
}

static SERVICE: Mutex<Option<Service>> = Mutex::new(None);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn view(id: u32, request: &ApprovalRequest) -> McpRequest {
    let shown = |text: &str| printable_lines(text, SHOWN_CHARS);
    let size = |bytes: usize| Some(u32::try_from(bytes).unwrap_or(u32::MAX));
    let (kind, subject, extra, bytes, checksum) = match &request.action {
        Action::Command { command, stdin } => (
            McpRequestKind::Command,
            shown(command),
            stdin.as_deref().map(shown),
            stdin.as_ref().and_then(|data| size(data.len())),
            None,
        ),
        Action::Session => (McpRequestKind::Session, String::new(), None, None, None),
        Action::FileRead { path } => (McpRequestKind::FileRead, shown(path), None, None, None),
        Action::FileWrite { path, bytes, sha256, preview } => (
            McpRequestKind::FileWrite,
            shown(path),
            Some(shown(preview)),
            size(*bytes),
            Some(printable(&sha256[..16.min(sha256.len())], 16)),
        ),
        Action::TrustHostKey { algorithm, fingerprint } => {
            (McpRequestKind::HostKey, shown(&format!("{algorithm} {fingerprint}")), None, None, None)
        }
    };
    let (subject_raw, extra_raw): (&str, Option<&str>) = match &request.action {
        Action::Command { command, stdin } => (command, stdin.as_deref()),
        Action::FileRead { path } => (path, None),
        Action::FileWrite { path, preview, .. } => (path, Some(preview)),
        Action::Session | Action::TrustHostKey { .. } => ("", None),
    };
    McpRequest {
        id,
        subject_lines: count(line_count(subject_raw)),
        subject_chars: count(subject_raw.chars().count()),
        extra_lines: count(extra_raw.map(line_count).unwrap_or(0)),
        device: String::new(),
        client: printable(&request.client_name, 60),
        host: printable(&request.host_label, 60),
        kind,
        subject,
        extra,
        bytes,
        checksum,
    }
}

/// Reicht Rückfragen an die Oberfläche weiter und wartet auf die Antwort.
struct AppApprover {
    sink: Arc<StreamSink<McpEvent>>,
    pending: Pending,
    /// Immer nur eine Rückfrage zur selben Zeit.
    turn: tokio::sync::Mutex<()>,
}

/// Zieht eine Rückfrage zurück, sobald sie erledigt ist – auch dann, wenn die
/// wartende Aufgabe mit dem Dienst abgebrochen wird.
struct Withdraw<'a> {
    approver: &'a AppApprover,
    id: u32,
}

impl Drop for Withdraw<'_> {
    fn drop(&mut self) {
        lock(&self.approver.pending).remove(&self.id);
        let _ = self.approver.sink.add(McpEvent::Withdrawn { id: self.id });
    }
}

#[async_trait]
impl Approver for AppApprover {
    async fn approve(&self, request: &ApprovalRequest) -> ApprovalAnswer {
        let _turn = self.turn.lock().await;
        let id = next_id();
        let (answer, answered) = oneshot::channel();
        lock(&self.pending).insert(id, (Instant::now(), answer));
        let _withdraw = Withdraw { approver: self, id };
        if self.sink.add(McpEvent::Request { request: view(id, request) }).is_err() {
            return ApprovalAnswer::Deny;
        }
        match tokio::time::timeout(APPROVAL_TIMEOUT, answered).await {
            Ok(Ok(answer)) => answer,
            _ => ApprovalAnswer::Deny,
        }
    }
}

/// Ob dieses Gerät den Dienst anbieten kann. Auf einem Telefon gibt es keine
/// Programme, die sich mit ihm verbinden könnten.
#[frb(sync)]
pub fn mcp_supported() -> bool {
    cfg!(any(windows, target_os = "linux", target_os = "macos"))
}

#[frb(sync)]
pub fn mcp_service_running() -> bool {
    lock(&SERVICE).as_ref().is_some_and(|service| !service.task.is_finished())
}

/// Startet den Dienst für den offenen Tresor unter `vault_path`. Scheitert,
/// wenn für diesen Tresor schon ein Dienst läuft, etwa `tephra daemon`.
pub async fn mcp_service_start(vault_path: String, sink: StreamSink<McpEvent>) -> Result<()> {
    if !mcp_supported() {
        bail!("Der MCP-Dienst steht auf diesem Gerät nicht zur Verfügung");
    }
    let core = core()?;
    if mcp_service_running() {
        return Ok(());
    }
    let listener = RUNTIME
        .spawn(async move { Listener::bind(&PathBuf::from(vault_path)).await })
        .await?
        .map_err(|message| anyhow!(message))?;
    let sink = Arc::new(sink);
    let _ = sink.add(McpEvent::Started);
    let pending = Pending::default();
    let approver = Arc::new(AppApprover { sink: sink.clone(), pending: pending.clone(), turn: Default::default() });
    let events = sink.clone();
    let observer: Observer = Arc::new(move |event| {
        let _ = events.add(match event {
            ServiceEvent::Connected { client } => McpEvent::Connected { client: printable(&client, 60) },
            ServiceEvent::Disconnected { client } => McpEvent::Disconnected { client: printable(&client, 60) },
        });
    });
    let serving = core.clone();
    let task = RUNTIME.spawn(async move {
        let result = listener.serve(serving, approver, observer).await;
        let _ = sink.add(McpEvent::Stopped { message: result.err() });
    });
    // Unbenutzte Verbindungen und Tunnel nach einigen Minuten abbauen. Was
    // eine offene Sitzung der App noch hält, bleibt dabei bestehen.
    let reaper = RUNTIME.spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            core.disconnect_idle(Duration::from_secs(600)).await;
            core.close_idle_tunnels(Duration::from_secs(300)).await;
        }
    });
    let service = Service { task: task.abort_handle(), reaper: reaper.abort_handle(), pending };
    if let Some(previous) = lock(&SERVICE).replace(service) {
        previous.task.abort();
        previous.reaper.abort();
    }
    Ok(())
}

/// Beendet den Dienst und mit ihm alle Sitzungen der verbundenen Clients.
#[frb(sync)]
pub fn mcp_service_stop() {
    if let Some(service) = lock(&SERVICE).take() {
        service.task.abort();
        service.reaper.abort();
        lock(&service.pending).clear();
    }
}

/// Beantwortet eine Rückfrage. `false`, wenn es sie nicht mehr gibt oder die
/// Antwort zu früh kam, um ihr gegolten zu haben.
#[frb(sync)]
pub fn mcp_answer(id: u32, answer: McpAnswer) -> bool {
    let Some(pending) = lock(&SERVICE).as_ref().map(|service| service.pending.clone()) else { return false };
    let mut pending = lock(&pending);
    // Ablehnen gilt immer sofort; nur eine Zustimmung muss der Frage gegolten haben.
    let deny = matches!(answer, McpAnswer::Deny);
    match pending.get(&id) {
        Some((asked, _)) if deny || asked.elapsed() >= SETTLE => {}
        _ => return false,
    }
    let Some((_, sender)) = pending.remove(&id) else { return false };
    sender
        .send(match answer {
            McpAnswer::Deny => ApprovalAnswer::Deny,
            McpAnswer::Once => ApprovalAnswer::Once,
            McpAnswer::Session => ApprovalAnswer::Session,
        })
        .is_ok()
}

// ------------------------------------------------------------------ Freigaben

pub enum McpLevel {
    ReadOnly,
    Confirm,
    Auto,
}

pub struct McpGrant {
    pub host_id: String,
    /// Bezeichnung des Hosts; leer, wenn es ihn nicht mehr gibt.
    pub host_label: String,
    pub level: McpLevel,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub files_read: bool,
    pub files_write: bool,
}

pub struct McpClient {
    pub id: String,
    pub name: String,
    pub disabled: bool,
    pub created_ms: i64,
    pub grants: Vec<McpGrant>,
}

pub struct McpOverview {
    /// `false` ist der Not-Aus: Dann wird jeder Zugriff abgelehnt.
    pub enabled: bool,
    pub clients: Vec<McpClient>,
    /// Längste Laufzeit, die ein Modell für einen Befehl wählen darf, in Sekunden.
    pub max_timeout_secs: u32,
}

/// `device` ist die Kennung eines fernverwalteten Geräts (siehe [`mcp_devices`]); `None` meint dieses Gerät.
fn device_id(device: &Option<String>) -> Result<Option<ItemId>> {
    device.as_deref().filter(|d| !d.is_empty()).map(parse_id).transpose()
}

fn load(core: &tephra_core::Core, device: Option<ItemId>) -> Result<McpPolicy> {
    match device {
        Some(id) => remote::load_device(core, id)?
            .ok_or_else(|| anyhow!("Dieses Gerät wird nicht mehr aus der Ferne verwaltet")),
        None => Ok(McpPolicy::load(core)?),
    }
}

fn policy(device: &Option<String>) -> Result<McpPolicy> {
    load(&core()?, device_id(device)?)
}

/// Lädt die Freigaben (dieses oder eines fernverwalteten Geräts), ändert sie und speichert sie wieder.
fn change(device: &Option<String>, edit: impl FnOnce(&mut McpPolicy) -> Result<()>) -> Result<()> {
    let core = core()?;
    let id = device_id(device)?;
    let mut policy = load(&core, id)?;
    edit(&mut policy)?;
    match id {
        Some(id) => remote::save_device(&core, id, &policy)?,
        None => policy.save(&core)?,
    }
    Ok(())
}

fn unknown_client() -> anyhow::Error {
    anyhow!("Diesen MCP-Client gibt es nicht mehr")
}

/// Clients mit ihren Freigaben und der Stand des Not-Aus.
#[frb(sync)]
pub fn mcp_overview(device: Option<String>) -> Result<McpOverview> {
    let core = core()?;
    let policy = load(&core, device_id(&device)?)?;
    let clients = policy
        .clients
        .iter()
        .map(|client| McpClient {
            id: client.id.clone(),
            name: client.name.clone(),
            disabled: client.disabled,
            created_ms: i64::try_from(client.created_ms).unwrap_or(i64::MAX),
            grants: client
                .hosts
                .iter()
                .map(|(host_id, grant)| McpGrant {
                    host_id: host_id.to_string(),
                    host_label: core
                        .with_vault(|v| v.get::<Host>(*host_id))
                        .ok()
                        .flatten()
                        .map(|host| host.label)
                        .unwrap_or_default(),
                    level: match grant.level {
                        AccessLevel::ReadOnly => McpLevel::ReadOnly,
                        AccessLevel::Confirm => McpLevel::Confirm,
                        AccessLevel::Auto => McpLevel::Auto,
                    },
                    allow: grant.allow.clone(),
                    deny: grant.deny.clone(),
                    files_read: grant.files_read,
                    files_write: grant.files_write,
                })
                .collect(),
        })
        .collect();
    let max_timeout_secs = u32::try_from(policy.limits.max_timeout_secs).unwrap_or(u32::MAX);
    Ok(McpOverview { enabled: policy.enabled, clients, max_timeout_secs })
}

/// Setzt die längste Laufzeit, die ein Modell für einen Befehl wählen darf (eine Minute bis ein Tag).
#[frb(sync)]
pub fn mcp_set_max_timeout(device: Option<String>, secs: u32) -> Result<()> {
    change(&device, |policy| {
        if !policy.limits.set_max_timeout_secs(u64::from(secs)) {
            bail!("Die Grenze muss zwischen einer Minute und einem Tag liegen");
        }
        Ok(())
    })
}

/// Legt einen Client an. Zurück kommt sein Token – nur dieses eine Mal; der
/// Tresor behält davon bloß einen Hash.
#[frb(sync)]
pub fn mcp_add_client(device: Option<String>, name: String) -> Result<String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        bail!("Der Client braucht einen Namen");
    }
    let mut token = String::new();
    change(&device, |policy| {
        if policy.clients.iter().any(|client| client.name.eq_ignore_ascii_case(&name)) {
            bail!("Einen MCP-Client „{name}“ gibt es bereits");
        }
        token = policy.add_client(&name)?.1;
        Ok(())
    })?;
    Ok(token)
}

/// Entfernt einen Client; sein Token wird damit ungültig.
#[frb(sync)]
pub fn mcp_remove_client(device: Option<String>, client_id: String) -> Result<()> {
    change(&device, |policy| policy.remove_client(&client_id).then_some(()).ok_or_else(unknown_client))
}

/// Sperrt einen Client oder gibt ihn wieder frei, ohne seine Freigaben zu verlieren.
#[frb(sync)]
pub fn mcp_set_client_disabled(device: Option<String>, client_id: String, disabled: bool) -> Result<()> {
    change(&device, |policy| {
        policy.client_mut(&client_id).ok_or_else(unknown_client)?.disabled = disabled;
        Ok(())
    })
}

/// Gibt einem Client einen Host frei. Eine bestehende Freigabe desselben Hosts wird ersetzt.
// Die Brücke zur Oberfläche reicht die Felder einzeln durch.
#[allow(clippy::too_many_arguments)]
#[frb(sync)]
pub fn mcp_grant(
    device: Option<String>,
    client_id: String,
    host_id: String,
    level: McpLevel,
    allow: Vec<String>,
    deny: Vec<String>,
    files_read: bool,
    files_write: bool,
) -> Result<()> {
    let host = parse_id(&host_id)?;
    if core()?.with_vault(|v| v.get::<Host>(host))?.is_none() {
        bail!("Diesen Host gibt es nicht mehr");
    }
    let patterns = |list: Vec<String>| -> Vec<String> {
        list.into_iter().map(|pattern| pattern.trim().to_string()).filter(|pattern| !pattern.is_empty()).collect()
    };
    let grant = HostGrant {
        level: match level {
            McpLevel::ReadOnly => AccessLevel::ReadOnly,
            McpLevel::Confirm => AccessLevel::Confirm,
            McpLevel::Auto => AccessLevel::Auto,
        },
        allow: patterns(allow),
        deny: patterns(deny),
        files_read,
        files_write,
    };
    change(&device, |policy| {
        policy.client_mut(&client_id).ok_or_else(unknown_client)?.hosts.insert(host, grant);
        Ok(())
    })
}

/// Entzieht einem Client einen Host.
#[frb(sync)]
pub fn mcp_revoke(device: Option<String>, client_id: String, host_id: String) -> Result<()> {
    let host = parse_id(&host_id)?;
    change(&device, |policy| {
        policy.client_mut(&client_id).ok_or_else(unknown_client)?.hosts.remove(&host);
        Ok(())
    })
}

/// Not-Aus: Mit `false` wird jeder weitere Zugriff abgelehnt, und laufende
/// Sitzungen und Verbindungen des Dienstes enden binnen Sekunden.
#[frb(sync)]
pub fn mcp_set_enabled(device: Option<String>, enabled: bool) -> Result<()> {
    change(&device, |policy| {
        policy.enabled = enabled;
        Ok(())
    })
}

#[frb(sync)]
pub fn mcp_enabled() -> Result<bool> {
    Ok(policy(&None)?.enabled)
}

// ------------------------------------------------------------------ Fernverwaltung

/// Ein Gerät, das der Fernverwaltung zugestimmt hat (`tephra mcp remote on`).
pub struct McpDeviceView {
    pub id: String,
    pub name: String,
    /// `false` ist der Not-Aus.
    pub enabled: bool,
    pub clients: u32,
    /// Letztes Lebenszeichen (Unix-Zeit in ms); 0, wenn keins.
    pub seen_ms: i64,
    pub version: String,
    /// Ob es dieses Gerät selbst ist.
    pub this_device: bool,
}

/// Alle fernverwalteten Geräte dieses Tresors.
#[frb(sync)]
pub fn mcp_devices() -> Result<Vec<McpDeviceView>> {
    Ok(remote::devices(&core()?)?
        .into_iter()
        .map(|d| McpDeviceView {
            id: d.id.to_string(),
            name: printable(&d.name, 60),
            enabled: d.enabled,
            clients: u32::try_from(d.clients).unwrap_or(u32::MAX),
            seen_ms: i64::try_from(d.seen_ms).unwrap_or(i64::MAX),
            version: printable(&d.version, 20),
            this_device: d.this_device,
        })
        .collect())
}

/// Nimmt ein Gerät aus der Verwaltung. Dort ist danach nichts mehr erlaubt, bis es selbst wieder zustimmt.
#[frb(sync)]
pub fn mcp_remove_device(device: String) -> Result<()> {
    Ok(remote::remove_device(&core()?, parse_id(&device)?)?)
}

/// Rückfragen anderer Geräte, jeweils unter einer festen Nummer für die Oberfläche, und wann sie zuerst
/// angezeigt wurden.
static REMOTE: Mutex<Vec<(u32, ItemId, Instant)>> = Mutex::new(Vec::new());

/// Rückfragen anderer Geräte, auf die noch niemand geantwortet hat, älteste zuerst. Die Texte sind entschärft.
#[frb(sync)]
pub fn mcp_remote_requests() -> Result<Vec<McpRequest>> {
    let pending = remote::pending_approvals(&core()?)?;
    let mut known = lock(&REMOTE);
    known.retain(|(_, id, _)| pending.iter().any(|(p, _)| p == id));
    let shown = |text: &str| printable_lines(text, SHOWN_CHARS);
    Ok(pending
        .into_iter()
        .map(|(item, approval)| {
            let number = match known.iter().find(|(_, id, _)| *id == item) {
                Some((number, _, _)) => *number,
                None => {
                    let number = next_id();
                    known.push((number, item, Instant::now()));
                    number
                }
            };
            McpRequest {
                id: number,
                subject_lines: count(line_count(&approval.subject)),
                subject_chars: count(approval.subject.chars().count()),
                extra_lines: count(approval.extra.as_deref().map(line_count).unwrap_or(0)),
                device: printable(&approval.device_name, 60),
                client: printable(&approval.client, 60),
                host: printable(&approval.host, 60),
                kind: match approval.kind.as_str() {
                    "session" => McpRequestKind::Session,
                    "file_read" => McpRequestKind::FileRead,
                    "file_write" => McpRequestKind::FileWrite,
                    "host_key" => McpRequestKind::HostKey,
                    _ => McpRequestKind::Command,
                },
                subject: shown(&approval.subject),
                extra: approval.extra.as_deref().map(shown),
                bytes: approval.bytes.map(|b| u32::try_from(b).unwrap_or(u32::MAX)),
                checksum: approval.checksum.as_deref().map(|c| printable(c, 16)),
            }
        })
        .collect())
}

/// Beantwortet die Rückfrage eines anderen Geräts. `by` ist der Name dieses Geräts. `false`, wenn sie sich
/// inzwischen erledigt hat oder die Zustimmung zu früh kam, um ihr gegolten zu haben.
#[frb(sync)]
pub fn mcp_remote_answer(id: u32, answer: McpAnswer, by: String) -> Result<bool> {
    let deny = matches!(answer, McpAnswer::Deny);
    let item = match lock(&REMOTE).iter().find(|(number, _, _)| *number == id) {
        Some((_, item, shown)) if deny || shown.elapsed() >= SETTLE => *item,
        _ => return Ok(false),
    };
    let reply = match answer {
        McpAnswer::Deny => ApprovalAnswer::Deny,
        McpAnswer::Once => ApprovalAnswer::Once,
        McpAnswer::Session => ApprovalAnswer::Session,
    };
    Ok(remote::answer(&core()?, item, reply, &by)?)
}

// ------------------------------------------------------------------ Protokoll

pub enum McpOutcome {
    Allowed,
    Approved,
    Denied,
    Rejected,
    Failed,
    Completed,
}

/// Ein Eintrag des Protokolls. Texte von Modell und Server sind entschärft.
pub struct McpAuditEntry {
    pub time_ms: i64,
    pub client: String,
    pub host: String,
    pub tool: String,
    pub detail: String,
    pub outcome: McpOutcome,
    pub exit_status: Option<u32>,
    pub note: String,
}

/// Die jüngsten Einträge des Protokolls, neueste zuerst.
#[frb(sync)]
pub fn mcp_audit(limit: u32) -> Result<Vec<McpAuditEntry>> {
    let entries = audit::read(&core()?, limit as usize, None)?;
    Ok(entries
        .into_iter()
        .map(|(_, entry)| McpAuditEntry {
            time_ms: i64::try_from(entry.time_ms).unwrap_or(i64::MAX),
            client: printable(&entry.client_name, 40),
            host: printable(&entry.host_label, 40),
            tool: printable(&entry.tool, 30),
            detail: printable(&entry.detail, 500),
            outcome: match entry.outcome {
                Outcome::Allowed => McpOutcome::Allowed,
                Outcome::Approved => McpOutcome::Approved,
                Outcome::Denied => McpOutcome::Denied,
                Outcome::Rejected => McpOutcome::Rejected,
                Outcome::Failed => McpOutcome::Failed,
                Outcome::Completed => McpOutcome::Completed,
            },
            exit_status: entry.exit_status,
            note: printable(&entry.note, 300),
        })
        .collect())
}

// ------------------------------------------------------------------ Aufmerksamkeit

/// Macht auf das Fenster der App aufmerksam, wenn es nicht im Vordergrund
/// ist: Eine Rückfrage wartet nur zwei Minuten. Unter Windows blinkt der
/// Eintrag in der Taskleiste; anderswo geschieht nichts.
#[frb(sync)]
pub fn mcp_request_attention() {
    #[cfg(windows)]
    attention::flash();
}

#[cfg(windows)]
mod attention {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FlashWindowEx, GetForegroundWindow, GetWindow, GetWindowThreadProcessId, IsWindowVisible,
        FLASHWINFO, FLASHW_TIMERNOFG, FLASHW_TRAY, GW_OWNER,
    };

    /// Merkt sich das erste sichtbare Hauptfenster dieses Prozesses.
    unsafe extern "system" fn own_window(window: HWND, found: LPARAM) -> BOOL {
        let mut process = 0u32;
        // SAFETY: `window` stammt aus der laufenden Aufzählung, `process` ist ein gültiges Ziel.
        unsafe { GetWindowThreadProcessId(window, &mut process) };
        // SAFETY: wie oben; beide Aufrufe lesen nur.
        let main = unsafe { IsWindowVisible(window) != 0 && GetWindow(window, GW_OWNER).is_null() };
        if process == unsafe { GetCurrentProcessId() } && main {
            // SAFETY: `found` ist der Zeiger auf das `HWND`, den `flash` übergeben hat, und lebt bis zum Ende der Aufzählung.
            unsafe { *(found as *mut HWND) = window };
            return 0;
        }
        1
    }

    pub(super) fn flash() {
        let mut window: HWND = std::ptr::null_mut();
        // SAFETY: Die Rückruffunktion schreibt nur in `window`, das diesen Aufruf überlebt.
        unsafe { EnumWindows(Some(own_window), &mut window as *mut HWND as LPARAM) };
        // SAFETY: liest nur den Zustand des Fenstersystems.
        if window.is_null() || window == unsafe { GetForegroundWindow() } {
            return;
        }
        let info = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd: window,
            // Blinkt, bis das Fenster in den Vordergrund kommt.
            dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
            uCount: 0,
            dwTimeout: 0,
        };
        // SAFETY: `info` ist vollständig gefüllt und gültig für die Dauer des Aufrufs.
        unsafe { FlashWindowEx(&info) };
    }
}
