//! MCP-Server: kontrollierter SSH-Zugriff für Sprachmodelle.
//!
//! Der Server läuft im selben Prozess wie der entsperrte Tresor. Zugangsdaten
//! verlassen ihn nie; ein Modell sieht nur freigegebene Hosts und bekommt nur
//! Ausgaben zurück.
//!
//! * [`policy`] – Freigaben pro Client und Host, Beurteilung von Befehlen
//! * [`Approver`] – Rückfrage beim Nutzer für alles, was Bestätigung braucht
//! * [`McpServer`] – die Werkzeuge selbst
//! * [`audit`] – Protokoll jedes Aufrufs

pub mod audit;
pub mod policy;
pub mod readonly;
mod server;
mod util;

pub use server::{Action, ApprovalAnswer, ApprovalRequest, Approver, DenyAll, McpServer};
pub use util::{Redactor, strip_ansi};

#[cfg(test)]
mod tests;
