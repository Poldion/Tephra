//! Vertrauen in den Sync-Server über den Fingerabdruck seines Zertifikats.
//!
//! Der Server erzeugt sein Zertifikat selbst; keine Zertifizierungsstelle bürgt dafür. Stattdessen merkt sich ein
//! Gerät beim ersten Kontakt den SHA-256-Fingerabdruck des Zertifikats, wie SSH einen Hostschlüssel, und spricht
//! danach nur noch mit genau diesem. Ändert er sich, bricht die Verbindung ab, bis der Nutzer den neuen bestätigt.

use rustls::DigitallySignedStruct;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Kennung im Fehlertext, an der ein abweichender Fingerabdruck erkannt wird.
pub(crate) const MISMATCH: &str = "tephra: Zertifikat mit anderem Fingerabdruck";

pub(crate) type Pin = [u8; 32];

pub(crate) fn digest(der: &[u8]) -> Pin {
    Sha256::digest(der).into()
}

/// Fingerabdruck zum Anzeigen: `AB:CD:…`, wie ihn auch der Server beim Start ins Protokoll schreibt.
pub fn format(pin: &Pin) -> String {
    pin.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

pub(crate) fn to_text(pin: &Pin) -> String {
    pin.iter().map(|b| format!("{b:02x}")).collect()
}

/// Liest einen gespeicherten oder angezeigten Fingerabdruck (mit oder ohne Doppelpunkte).
pub(crate) fn from_text(text: &str) -> Option<Pin> {
    let hex: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 64 {
        return None;
    }
    let mut pin = [0u8; 32];
    for (i, byte) in pin.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(pin)
}

/// Prüft das Zertifikat des Servers gegen den Fingerabdruck. Ohne Fingerabdruck (nur für die Probe beim ersten
/// Kontakt) nimmt es jedes an und merkt sich, welches kam. Die Signatur im Handshake wird immer geprüft: Wer nur das
/// Zertifikat vorzeigt, ohne den Schlüssel dazu zu haben, kommt nicht durch.
#[derive(Debug)]
struct Verifier {
    pin: Option<Pin>,
    seen: Arc<Mutex<Option<Pin>>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let found = digest(end_entity.as_ref());
        *self.seen.lock().unwrap_or_else(|p| p.into_inner()) = Some(found);
        match self.pin {
            Some(pin) if pin != found => Err(rustls::Error::General(MISMATCH.into())),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// Ein Client, der nur mit dem Server dieses Fingerabdrucks spricht (oder, ohne, jeden annimmt und sich merkt).
pub(crate) fn client(pin: Option<Pin>, timeout: Duration) -> (reqwest::Client, Arc<Mutex<Option<Pin>>>) {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let seen = Arc::new(Mutex::new(None));
    let verifier = Verifier { pin, seen: seen.clone(), provider: provider.clone() };
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map(|builder| builder.dangerous().with_custom_certificate_verifier(Arc::new(verifier)).with_no_client_auth());
    let client = match config {
        Ok(config) => reqwest::Client::builder()
            .use_preconfigured_tls(config)
            .connect_timeout(Duration::from_secs(15))
            .timeout(timeout)
            .user_agent(concat!("Tephra/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default(),
        Err(_) => reqwest::Client::default(),
    };
    (client, seen)
}

/// Fragt den Server über HTTPS nach seinem Lebenszeichen und gibt den Fingerabdruck seines Zertifikats zurück;
/// `None`, wenn er kein HTTPS spricht oder nicht antwortet. Dabei geht nichts Geheimes über die Leitung.
pub(crate) async fn probe(server: &str) -> Option<Pin> {
    let (client, seen) = client(None, Duration::from_secs(15));
    let response = client.get(format!("{server}/healthz")).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    *seen.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerabdruck_hin_und_zurueck() {
        let pin = digest(b"zertifikat");
        assert_eq!(from_text(&to_text(&pin)), Some(pin));
        assert_eq!(from_text(&format(&pin)), Some(pin));
        assert_eq!(format(&pin).len(), 32 * 3 - 1);
        assert_eq!(from_text("ab:cd"), None);
    }
}
