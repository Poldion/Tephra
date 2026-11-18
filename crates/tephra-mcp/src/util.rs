//! Aufbereitung von Ausgaben, bevor sie an ein Sprachmodell gehen.

use tephra_core::model::ItemId;
use tephra_core::{Core, CoreError};
use zeroize::Zeroize;

/// Geheimnisse müssen mindestens so lang sein, damit sie geschwärzt werden.
/// Kürzere Zeichenfolgen kämen zufällig in normaler Ausgabe vor.
const MIN_SECRET_LEN: usize = 6;
pub const REDACTED: &str = "[redacted]";

/// Ersetzt die Zugangsdaten eines Hosts in dessen Ausgaben. So gelangt ein
/// Passwort nicht ins Modell, nur weil ein Befehl es anzeigt.
///
/// Geschwärzt wird nur, was zur Verbindung mit genau diesem Host gehört
/// (samt Jump-Hosts, Proxy und VPN). Die Geheimnisse aller Hosts zu
/// schwärzen, würde umgekehrt verraten, ob ein beliebiger Text irgendwo im
/// Tresor als Passwort vorkommt.
///
/// Grenzen: Erkannt wird nur die wörtliche Zeichenfolge – keine kodierte
/// (Base64, Hex) und keine, die über zwei Abrufe verteilt ankommt.
pub struct Redactor {
    secrets: Vec<String>,
}

impl Drop for Redactor {
    fn drop(&mut self) {
        for secret in &mut self.secrets {
            secret.zeroize();
        }
    }
}

impl Redactor {
    /// Stellt die Geheimnisse für einen Host zusammen. Ein Fehler wird
    /// gemeldet statt verschluckt: Wer die Ausgabe nicht filtern kann, darf
    /// sie nicht weitergeben.
    pub fn for_host(core: &Core, host_id: ItemId) -> Result<Self, CoreError> {
        let mut secrets = core.connection_secrets(host_id)?;
        secrets.retain(|s| s.len() >= MIN_SECRET_LEN);
        // Längere zuerst, damit ein Geheimnis, das ein anderes enthält, ganz verschwindet.
        secrets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        secrets.dedup();
        Ok(Self { secrets })
    }

    pub fn apply(&self, text: &str) -> String {
        let mut out = text.to_string();
        for secret in &self.secrets {
            if out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), REDACTED);
            }
        }
        out
    }
}

/// Entfernt Steuersequenzen eines Terminals (Farben, Cursorbewegungen,
/// Fenstertitel), damit ein Sprachmodell nur den lesbaren Text sieht.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            // Übrige Steuerzeichen außer Zeilenumbruch und Tabulator verwerfen.
            if !c.is_control() || c == '\n' || c == '\t' {
                out.push(c);
            }
            continue;
        }
        match chars.peek() {
            // CSI: ESC [ … Schlusszeichen
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: ESC ] … BEL oder ESC \
            Some(']') => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\u{07}' {
                        break;
                    }
                    if c == '\u{1b}' {
                        chars.next();
                        break;
                    }
                }
            }
            // Zweistellige Sequenzen wie ESC ( B
            Some('(' | ')' | '#' | '%') => {
                chars.next();
                chars.next();
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// Setzt einen Wert so in einfache Anführungszeichen, dass eine POSIX-Shell
/// ihn als ein einziges, wörtliches Argument liest.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Teilt Text an einer Zeichengrenze: die ersten `chars` Zeichen ab `offset`.
pub fn page(text: &str, offset: usize, chars: usize) -> (String, usize) {
    let total = text.chars().count();
    let page: String = text.chars().skip(offset).take(chars).collect();
    (page, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steuersequenzen() {
        assert_eq!(strip_ansi("\u{1b}[1;32mgrün\u{1b}[0m normal"), "grün normal");
        assert_eq!(strip_ansi("\u{1b}]0;Fenstertitel\u{07}prompt$ "), "prompt$ ");
        assert_eq!(strip_ansi("\u{1b}]0;Titel\u{1b}\\text"), "text");
        assert_eq!(strip_ansi("a\r\nb\tc\u{08}"), "a\nb\tc");
        assert_eq!(strip_ansi("\u{1b}(Bx\u{1b}[?2004hy"), "xy");
        assert_eq!(strip_ansi("ende\u{1b}"), "ende");
    }

    proptest::proptest! {
        #[test]
        fn steuersequenzen_entfernen_stuerzt_nicht_ab(text in "(\u{1b}|\\[|\\]|\u{7}|;|[0-9?]|[a-zA-Z ]|\\PC){0,80}") {
            let clean = strip_ansi(&text);
            let only_text = clean.chars().all(|c| !c.is_control() || c == '\n' || c == '\t');
            proptest::prop_assert!(only_text, "Steuerzeichen übrig in {:?}", clean);
        }

        /// Ein in Anführungszeichen gesetzter Wert enthält außerhalb der
        /// Anführungszeichen nie etwas anderes als die Maskierung selbst.
        #[test]
        fn shell_anfuehrung_ist_dicht(value in "\\PC{0,40}") {
            let quoted = shell_quote(&value);
            proptest::prop_assert!(quoted.starts_with('\'') && quoted.ends_with('\''));
            let inner = &quoted[1..quoted.len() - 1];
            let stray_quote = inner.replace(r"'\''", "").contains('\'');
            proptest::prop_assert!(!stray_quote);
        }
    }

    #[test]
    fn shell_anfuehrung() {
        assert_eq!(shell_quote("nginx"), "'nginx'");
        assert_eq!(shell_quote("a b; rm -rf /"), "'a b; rm -rf /'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn seiten() {
        assert_eq!(page("äöüß", 1, 2), ("öü".to_string(), 4));
        assert_eq!(page("abc", 5, 2), (String::new(), 3));
    }

    #[test]
    fn schwaerzen() {
        let r = Redactor { secrets: vec!["langes-geheimnis".into(), "geheim".into()] };
        assert_eq!(r.apply("pw=langes-geheimnis und geheim!"), "pw=[redacted] und [redacted]!");
        assert_eq!(r.apply("nichts"), "nichts");
    }
}
