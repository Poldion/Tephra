//! Interaktive SSH-Sitzung im lokalen Terminal.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tephra_core::ssh::{PtyOptions, ShellEvent, SshClient};
use tokio::sync::mpsc;

/// Stellt den ursprünglichen Zustand des Terminals wieder her – auch wenn
/// die Sitzung mit einem Fehler endet.
struct RawMode;

impl RawMode {
    fn enable() -> std::io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

enum Input {
    Bytes(Vec<u8>),
    Resize(u16, u16),
}

/// Übersetzt einen Tastendruck in die Bytefolge, die ein Terminal dafür sendet.
pub fn key_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let mut bytes: Vec<u8> = match key.code {
        KeyCode::Char(c) if ctrl => {
            let c = c.to_ascii_lowercase();
            match c {
                'a'..='z' => vec![c as u8 - b'a' + 1],
                ' ' | '@' | '2' => vec![0],
                '[' | '3' => vec![0x1b],
                '\\' | '4' => vec![0x1c],
                ']' | '5' => vec![0x1d],
                '^' | '6' => vec![0x1e],
                '_' | '7' | '/' => vec![0x1f],
                '8' | '?' => vec![0x7f],
                _ => c.to_string().into_bytes(),
            }
        }
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6 => b"\x1b[17~".to_vec(),
            7 => b"\x1b[18~".to_vec(),
            8 => b"\x1b[19~".to_vec(),
            9 => b"\x1b[20~".to_vec(),
            10 => b"\x1b[21~".to_vec(),
            11 => b"\x1b[23~".to_vec(),
            12 => b"\x1b[24~".to_vec(),
            _ => return None,
        },
        _ => return None,
    };
    if alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

/// Verbindet das lokale Terminal mit einer Shell auf dem Server und gibt
/// deren Exit-Code zurück.
pub async fn interactive(client: &SshClient, startup_command: Option<&str>) -> Result<Option<u32>, String> {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let pty = PtyOptions {
        term: std::env::var("TERM").ok().filter(|t| !t.is_empty()).unwrap_or_else(|| "xterm-256color".into()),
        cols: u32::from(cols),
        rows: u32::from(rows),
    };
    let mut shell = client.shell(pty, startup_command).await.map_err(|e| e.to_string())?;
    let _raw = RawMode::enable().map_err(|e| format!("Terminal lässt sich nicht umschalten: {e}"))?;

    // Tastatur und Größenänderungen kommen aus einem eigenen Thread, weil
    // das Lesen vom Terminal blockiert.
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, mut input) = mpsc::channel::<Input>(256);
    let reader_stop = stop.clone();
    let reader = std::thread::spawn(move || {
        while !reader_stop.load(Ordering::Relaxed) {
            match crossterm::event::poll(Duration::from_millis(100)) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(_) => break,
            }
            let message = match crossterm::event::read() {
                Ok(Event::Key(key)) => key_bytes(key).map(Input::Bytes),
                Ok(Event::Paste(text)) => Some(Input::Bytes(text.into_bytes())),
                Ok(Event::Resize(cols, rows)) => Some(Input::Resize(cols, rows)),
                Ok(_) => None,
                Err(_) => break,
            };
            if let Some(message) = message
                && tx.blocking_send(message).is_err()
            {
                break;
            }
        }
    });

    let mut exit = None;
    let mut stdout = std::io::stdout();
    loop {
        tokio::select! {
            event = shell.events.recv() => match event {
                Some(ShellEvent::Output(data)) => {
                    let _ = stdout.write_all(&data);
                    let _ = stdout.flush();
                }
                Some(ShellEvent::Exit(status)) => exit = status,
                Some(ShellEvent::Closed) | None => break,
            },
            message = input.recv() => match message {
                Some(Input::Bytes(bytes)) => {
                    if shell.input.send(&bytes).await.is_err() {
                        break;
                    }
                }
                Some(Input::Resize(cols, rows)) => {
                    let _ = shell.input.resize(u32::from(cols), u32::from(rows)).await;
                }
                None => break,
            },
        }
    }
    stop.store(true, Ordering::Relaxed);
    let _ = reader.join();
    Ok(exit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn tasten_werden_wie_im_terminal_kodiert() {
        assert_eq!(key_bytes(key(KeyCode::Char('a'), KeyModifiers::NONE)), Some(b"a".to_vec()));
        assert_eq!(key_bytes(key(KeyCode::Char('ä'), KeyModifiers::NONE)), Some("ä".as_bytes().to_vec()));
        assert_eq!(key_bytes(key(KeyCode::Char('c'), KeyModifiers::CONTROL)), Some(vec![3]));
        assert_eq!(key_bytes(key(KeyCode::Char('D'), KeyModifiers::CONTROL)), Some(vec![4]));
        assert_eq!(key_bytes(key(KeyCode::Char('x'), KeyModifiers::ALT)), Some(b"\x1bx".to_vec()));
        assert_eq!(key_bytes(key(KeyCode::Enter, KeyModifiers::NONE)), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(key(KeyCode::Backspace, KeyModifiers::NONE)), Some(vec![0x7f]));
        assert_eq!(key_bytes(key(KeyCode::Up, KeyModifiers::NONE)), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(key(KeyCode::F(5), KeyModifiers::NONE)), Some(b"\x1b[15~".to_vec()));
        assert_eq!(key_bytes(key(KeyCode::CapsLock, KeyModifiers::NONE)), None);
        let mut release = key(KeyCode::Char('a'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert_eq!(key_bytes(release), None);
    }
}
