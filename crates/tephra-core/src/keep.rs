//! Terminalsitzungen, die einen Verbindungsabbruch überstehen.
//!
//! Ist an einem Host „Sitzung halten“ eingeschaltet, startet die Sitzung auf dem Host in tmux. Reißt die
//! Verbindung ab (Funkloch, Wechsel vom WLAN ins Mobilnetz), läuft dort alles weiter; beim nächsten Verbinden
//! hängt sich das Terminal wieder an dieselbe tmux-Sitzung.

use tephra_model::Host;

/// Befehl, mit dem eine Terminalsitzung auf dem Host startet; `None` heißt: die Login-Shell.
///
/// `slot` unterscheidet mehrere Terminals zum selben Host (ab 1). Jedes hängt an seiner eigenen
/// tmux-Sitzung `tephra-<slot>`, und dieselbe Nummer findet nach einem Abbruch dieselbe Sitzung wieder.
pub fn session_command(host: &Host, slot: u32) -> Option<String> {
    let startup = host.startup_command.as_deref().map(str::trim).filter(|c| !c.is_empty());
    if !host.keep_session {
        return startup.map(str::to_string);
    }
    Some(tmux_command(slot.max(1), startup))
}

/// Name der tmux-Sitzung auf dem Host.
pub fn tmux_session_name(slot: u32) -> String {
    format!("tephra-{}", slot.max(1))
}

/// Das eigentliche Skript läuft immer in `sh`, gleich welche Login-Shell der Nutzer hat. Damit es auch durch
/// fish und tcsh unverändert hindurchgeht, enthält es weder einfache Anführungszeichen noch Backslashes noch
/// Ausrufezeichen; der Startbefehl kommt Base64-kodiert hinein.
fn tmux_command(slot: u32, startup: Option<&str>) -> String {
    let name = tmux_session_name(slot);
    let (decode, create, fallback) = match startup {
        Some(command) => (
            format!("c=$(echo {} | base64 -d 2>/dev/null || echo {} | base64 -D); ", base64(command), base64(command)),
            format!("tmux new-session -d -s {name} \"$c\""),
            "exec \"${SHELL:-/bin/sh}\" -c \"$c\"",
        ),
        None => (String::new(), format!("tmux new-session -d -s {name}"), "exec \"${SHELL:-/bin/sh}\" -l"),
    };
    let script = [
        decode,
        "if command -v tmux >/dev/null 2>&1; then ".to_string(),
        format!("tmux has-session -t ={name} 2>/dev/null || {create} || exit 1; "),
        // Nur diese Sitzung: Wischen scrollt durch ihren Verlauf, und die Statuszeile kostet am Telefon eine
        // Zeile. Die übrige tmux-Einrichtung des Nutzers bleibt unberührt.
        format!("tmux set-option -t ={name} mouse on >/dev/null; "),
        format!("tmux set-option -t ={name} status off >/dev/null; "),
        format!("exec tmux attach-session -t ={name}; "),
        "fi; ".to_string(),
        "echo \"Tephra: tmux ist auf diesem Host nicht installiert, die Sitzung bleibt nach einem Abbruch nicht erhalten.\"; "
            .to_string(),
        fallback.to_string(),
    ]
    .concat();
    format!("sh -c '{script}'")
}

fn base64(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(keep: bool, startup: Option<&str>) -> Host {
        Host { keep_session: keep, startup_command: startup.map(str::to_string), ..Host::default() }
    }

    #[test]
    fn without_keeping_the_startup_command_is_unchanged() {
        assert_eq!(session_command(&host(false, None), 1), None);
        assert_eq!(session_command(&host(false, Some(" htop ")), 1).as_deref(), Some("htop"));
        assert_eq!(session_command(&host(false, Some("  ")), 1), None);
    }

    #[test]
    fn keeping_attaches_to_a_named_tmux_session() {
        let command = session_command(&host(true, None), 2).unwrap();
        assert!(command.starts_with("sh -c '") && command.ends_with('\''));
        assert!(command.contains("tmux has-session -t =tephra-2"));
        assert!(command.contains("tmux new-session -d -s tephra-2 ||"));
        assert!(command.contains("exec tmux attach-session -t =tephra-2"));
        assert!(command.contains("exec \"${SHELL:-/bin/sh}\" -l"));
        assert_eq!(session_command(&host(true, None), 0), session_command(&host(true, None), 1));
    }

    #[test]
    fn the_script_survives_every_login_shell() {
        let command = session_command(&host(true, Some("cd 'mein projekt' && claude \\ --resume!")), 1).unwrap();
        let script = &command["sh -c '".len()..command.len() - 1];
        assert!(!script.contains('\''), "{script}");
        assert!(!script.contains('\\'), "{script}");
        assert!(!script.contains('!'), "{script}");
        assert!(command.contains("tmux new-session -d -s tephra-1 \"$c\""));
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(""), "");
        assert_eq!(base64("f"), "Zg==");
        assert_eq!(base64("fo"), "Zm8=");
        assert_eq!(base64("foo"), "Zm9v");
        assert_eq!(base64("claude --resume"), "Y2xhdWRlIC0tcmVzdW1l");
        assert_eq!(base64("ä"), "w6Q=");
    }
}
