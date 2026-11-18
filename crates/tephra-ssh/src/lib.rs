//! SSH-Verbindungen auf Basis eines beliebigen [`Transport`](tephra_transport::Transport).
//!
//! * [`SshClient`] – Verbindung, Anmeldung, Befehle, interaktive Shell, Weiterleitungen
//! * [`JumpTransport`] – macht eine bestehende Verbindung zum Sprungbrett für die nächste
//! * [`forward`] – Portweiterleitungen und SOCKS-Proxy über eine Verbindung
//! * [`keys`] – Schlüssel erzeugen, einlesen und prüfen

mod client;
pub mod forward;
pub mod keys;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

pub use client::{
    AcceptAnyHostKey, ConnectOptions, Credential, ExecOptions, ExecOutput, HostKeyDecision, HostKeyInfo,
    HostKeyVerifier, Interaction, JumpTransport, Prompt, PtyOptions, Shell, ShellEvent, ShellInput, SshClient,
};
pub use russh_sftp::client::SftpSession;

use tephra_transport::TransportError;

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error("Hostschlüssel von {host} wurde nicht akzeptiert ({fingerprint})")]
    HostKeyRejected { host: String, fingerprint: String },
    #[error("Anmeldung als {user} fehlgeschlagen{}", hint(.methods))]
    AuthFailed { user: String, methods: Vec<String> },
    #[error("Keine Anmeldedaten für {0} vorhanden")]
    NoCredentials(String),
    #[error("Schlüssel ist ungültig oder die Passphrase stimmt nicht: {0}")]
    Key(String),
    #[error("Zeitüberschreitung: {0}")]
    Timeout(String),
    #[error("Verbindung wurde getrennt")]
    Closed,
    #[error("SSH-Fehler: {0}")]
    Protocol(String),
}

fn hint(methods: &[String]) -> String {
    if methods.is_empty() { String::new() } else { format!(" (der Server akzeptiert: {})", methods.join(", ")) }
}

impl From<russh::Error> for SshError {
    fn from(e: russh::Error) -> Self {
        match e {
            russh::Error::Disconnect | russh::Error::SendError | russh::Error::HUP => SshError::Closed,
            other => SshError::Protocol(other.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, SshError>;
