//! Behälter für geheime Zeichenfolgen.

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// Geheime Zeichenfolge (Passwort, privater Schlüssel …).
///
/// Der Inhalt erscheint nie in Debug-Ausgaben oder Protokollen und wird beim
/// Verwerfen überschrieben. Zugriff nur ausdrücklich über [`Secret::expose`].
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Gibt den geheimen Inhalt heraus.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() { "Secret(leer)" } else { "Secret(…)" })
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_verraet_nichts_und_json_bleibt_ein_text() {
        let secret = Secret::new("hunter2");
        assert_eq!(format!("{secret:?}"), "Secret(…)");
        assert_eq!(serde_json::to_string(&secret).expect("json"), "\"hunter2\"");
        let back: Secret = serde_json::from_str("\"hunter2\"").expect("json");
        assert_eq!(back.expose(), "hunter2");
        assert_eq!(format!("{:?}", Secret::default()), "Secret(leer)");
    }
}
