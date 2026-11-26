//! Protokoll aller Zugriffe über MCP.
//!
//! Die Einträge liegen verschlüsselt im Tresor dieses Geräts und werden nicht
//! abgeglichen.

use serde::{Deserialize, Serialize};
use tephra_core::model::ItemId;
use tephra_core::{Core, CoreError};

/// Längste Befehls- oder Pfadangabe, die protokolliert wird.
const MAX_DETAIL: usize = 4000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Ausgeführt, weil die Freigabe es ohne Rückfrage erlaubt.
    Allowed,
    /// Ausgeführt, nachdem der Nutzer zugestimmt hat.
    Approved,
    /// Von der Freigabe abgelehnt.
    Denied,
    /// Vom Nutzer abgelehnt.
    Rejected,
    /// Erlaubt, aber fehlgeschlagen (z. B. Verbindung nicht möglich).
    Failed,
    /// Abschluss einer zuvor als erlaubt oder bestätigt vermerkten Aktion.
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub time_ms: u64,
    pub client_id: String,
    pub client_name: String,
    pub host_id: Option<ItemId>,
    pub host_label: String,
    pub tool: String,
    /// Befehl, Pfad oder Kurzbeschreibung.
    pub detail: String,
    pub outcome: Outcome,
    pub exit_status: Option<u32>,
    pub duration_ms: u64,
    pub output_bytes: u64,
    /// Grund einer Ablehnung oder Fehlermeldung.
    #[serde(default)]
    pub note: String,
}

/// Schreibt einen Eintrag. Ein Fehler wird gemeldet, damit der Aufrufer eine
/// Aktion, die sich nicht protokollieren lässt, gar nicht erst ausführt.
pub fn append(core: &Core, mut entry: AuditEntry) -> Result<(), CoreError> {
    // Lange Befehle: Anfang, Ende, Länge und Prüfsumme, damit sich ein angehängter Teil nicht unbemerkt
    // wegkürzen lässt.
    let total = entry.detail.chars().count();
    if total > MAX_DETAIL {
        use sha2::{Digest, Sha256};
        let hash: String = Sha256::digest(entry.detail.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect();
        let head: String = entry.detail.chars().take(MAX_DETAIL - 1000).collect();
        let tail: String = entry.detail.chars().skip(total - 800).collect();
        entry.detail = format!("{head} … [gekürzt: {total} Zeichen, SHA-256 {hash}…] … {tail}");
    }
    let bytes = serde_json::to_vec(&entry).map_err(|e| CoreError::Config(e.to_string()))?;
    core.with_vault(|v| v.append_journal(&bytes).map(|_| ()))
}

/// Jüngste Einträge, neueste zuerst. `before` blättert weiter zurück.
pub fn read(core: &Core, limit: usize, before: Option<i64>) -> Result<Vec<(i64, AuditEntry)>, CoreError> {
    let rows = core.with_vault(|v| v.read_journal(limit, before))?;
    Ok(rows.into_iter().filter_map(|(seq, bytes)| Some((seq, serde_json::from_slice(&bytes).ok()?))).collect())
}
