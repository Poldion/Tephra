//! Rückfragen an den Nutzer im Terminal.

use async_trait::async_trait;
use std::io::{BufRead, Write};
use tephra_core::ssh::Prompt;
use tephra_core::{ConnectUi, HostKeyQuestion, HostKeyState};

/// Liest eine Zeile von der Standardeingabe, ohne die Laufzeit zu blockieren.
pub async fn read_line(prompt: String) -> Option<String> {
    tokio::task::spawn_blocking(move || {
        eprint!("{prompt}");
        std::io::stderr().flush().ok()?;
        let mut line = String::new();
        let read = std::io::stdin().lock().read_line(&mut line).ok()?;
        (read > 0).then(|| line.trim().to_string())
    })
    .await
    .ok()
    .flatten()
}

/// Liest ein Passwort ohne Anzeige. Die Umgebungsvariable `TEPHRA_PASSWORD`
/// erspart die Eingabe in Skripten und Tests.
pub async fn read_password(prompt: String) -> Option<String> {
    tokio::task::spawn_blocking(move || rpassword::prompt_password(prompt).ok()).await.ok().flatten()
}

/// Zeichen, die nichts anzeigen oder die Leserichtung ändern. Mit ihnen
/// ließe sich Text so darstellen, dass er anders aussieht, als er ist.
fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00ad}' | '\u{061c}' | '\u{180e}' | '\u{feff}'
        | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{2069}'
        | '\u{fff9}'..='\u{fffb}' | '\u{e0000}'..='\u{e007f}')
}

fn make_printable(text: &str, max_chars: usize, keep_newlines: bool) -> String {
    let total = text.chars().count();
    let mut out = String::with_capacity(text.len());
    for (index, c) in text.chars().enumerate() {
        if index >= max_chars {
            out.push_str(&format!(" … (+{} weitere Zeichen)", total - index));
            break;
        }
        match c {
            '\n' if keep_newlines => out.push('\n'),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c if is_invisible(c) => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Macht fremden Text gefahrlos anzeigbar: Steuerzeichen, unsichtbare und
/// richtungsändernde Zeichen erscheinen als sichtbare Kürzel (`\x1b`), und
/// Überlänge wird mit ausdrücklichem Hinweis gekürzt. Das Ergebnis ist eine
/// einzige Zeile.
///
/// Alles, was von einem Sprachmodell oder einem Server stammt, muss vor der
/// Ausgabe hier durch – sonst könnte es die Anzeige überschreiben und dem
/// Nutzer etwas anderes zeigen, als tatsächlich geschieht.
pub fn printable(text: &str, max_chars: usize) -> String {
    make_printable(text, max_chars, false)
}

/// Wie [`printable`], behält aber Zeilenumbrüche. Der Aufrufer muss jede
/// Zeile als Teil des fremden Textes kennzeichnen.
pub fn printable_lines(text: &str, max_chars: usize) -> String {
    make_printable(text, max_chars, true)
}

pub fn yes(answer: Option<String>) -> bool {
    matches!(answer.as_deref().map(str::to_lowercase).as_deref(), Some("j" | "ja" | "y" | "yes"))
}

/// Beantwortet Rückfragen beim Verbindungsaufbau über das Terminal.
pub struct TerminalUi;

#[async_trait]
impl ConnectUi for TerminalUi {
    async fn confirm_host_key(&self, question: &HostKeyQuestion) -> bool {
        let key = &question.key;
        match &question.state {
            HostKeyState::Unknown => {
                eprintln!("Der Host {}:{} ist noch nicht bekannt.", key.host, key.port);
                eprintln!("  {} {}", key.algorithm, key.fingerprint);
                yes(read_line("Diesem Schlüssel vertrauen? [j/N] ".into()).await)
            }
            HostKeyState::Changed { known_fingerprints } => {
                eprintln!("ACHTUNG: Der Schlüssel von {}:{} hat sich geändert!", key.host, key.port);
                eprintln!("  bisher bekannt: {}", known_fingerprints.join(", "));
                eprintln!("  jetzt vorgelegt: {} {}", key.algorithm, key.fingerprint);
                eprintln!("Das kann ein Angriff auf die Verbindung sein. Nur fortfahren, wenn der");
                eprintln!("Schlüssel des Servers nachweislich erneuert wurde.");
                read_line("Zum Bestätigen „geändert“ eintippen: ".into()).await.as_deref() == Some("geändert")
            }
        }
    }

    async fn ask_password(&self, prompt: &str) -> Option<String> {
        read_password(format!("{prompt}: ")).await
    }

    async fn answer_prompts(&self, name: &str, instructions: &str, prompts: &[Prompt]) -> Option<Vec<String>> {
        // Diese Texte bestimmt der Server. Sie werden als solche
        // gekennzeichnet, damit er nicht etwa nach dem „Master-Passwort"
        // fragen kann, als käme die Frage von diesem Programm.
        for line in [name, instructions] {
            if !line.trim().is_empty() {
                eprintln!("[Server] {}", printable(line, 300));
            }
        }
        let mut answers = Vec::new();
        for prompt in prompts {
            let text = format!("[Rückfrage des Servers] {} ", printable(prompt.text.trim(), 200));
            let answer = if prompt.echo { read_line(text).await } else { read_password(text).await };
            answers.push(answer?);
        }
        Some(answers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fremder_text_wird_entschaerft() {
        assert_eq!(printable("ls -la", 100), "ls -la");
        assert_eq!(printable("a\u{1b}[2K\rb", 100), "a\\x1b[2K\\rb");
        assert_eq!(printable("a\nb\tc", 100), "a\\nb\\tc");
        assert_eq!(printable_lines("a\nb", 100), "a\nb");
        assert_eq!(printable("x\u{202e}y\u{200b}z\u{feff}", 100), "x\\u{202e}y\\u{200b}z\\u{feff}");
        assert_eq!(printable("\u{7}\u{0}", 100), "\\x07\\x00");
        assert_eq!(printable("äöü €", 100), "äöü €");
        assert_eq!(printable("abcdef", 3), "abc … (+3 weitere Zeichen)");
    }
}
