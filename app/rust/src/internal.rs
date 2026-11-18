//! Zustand und Hilfen hinter der Schnittstelle zur Oberfläche. Nichts hiervon
//! ist von der Oberfläche aus sichtbar.

use crate::api::app::{PromptView, SessionEvent};
use crate::frb_generated::StreamSink;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use tephra_core::model::{Host, ItemId, SshKey};
use tephra_core::ssh::{Prompt, PtyOptions, ShellEvent, ShellInput};
use tephra_core::{ConnectUi, Core, HostKeyQuestion, HostKeyState};
use tokio::sync::oneshot;

/// Eigene Laufzeit für alles Asynchrone im Kern (Verbindungen, Tunnel).
pub(crate) static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("tephra-core")
        .build()
        .expect("Laufzeit lässt sich nicht starten")
});

#[derive(Default)]
pub(crate) struct App {
    pub(crate) core: Option<Core>,
    pub(crate) sessions: HashMap<u32, ShellInput>,
    /// Laufende Sitzungsaufgaben, um auch einen noch laufenden Verbindungsaufbau abbrechen zu können.
    pub(crate) tasks: HashMap<u32, tokio::task::AbortHandle>,
    /// Offene Rückfragen an den Nutzer, nach ihrer Kennung.
    pub(crate) pending: HashMap<u32, oneshot::Sender<Answer>>,
    pub(crate) next_id: u32,
}

pub(crate) enum Answer {
    Decision(bool),
    Text(Option<String>),
    Texts(Option<Vec<String>>),
}

pub(crate) static APP: LazyLock<Mutex<App>> = LazyLock::new(|| Mutex::new(App::default()));

pub(crate) fn app() -> MutexGuard<'static, App> {
    APP.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn core() -> Result<Core> {
    app().core.clone().ok_or_else(|| anyhow!("Der Tresor ist nicht geöffnet"))
}

pub(crate) fn next_id() -> u32 {
    let mut app = app();
    app.next_id = app.next_id.wrapping_add(1).max(1);
    app.next_id
}

pub(crate) fn parse_id(id: &str) -> Result<ItemId> {
    id.parse().map_err(|_| anyhow!("Ungültige Kennung"))
}

pub(crate) fn optional_id(id: &Option<String>) -> Result<Option<ItemId>> {
    id.as_deref().filter(|id| !id.is_empty()).map(parse_id).transpose()
}

/// Reicht Rückfragen des Verbindungsaufbaus als Ereignisse an die Oberfläche
/// weiter und wartet auf deren Antwort.
pub(crate) struct SinkUi {
    pub(crate) sink: Arc<StreamSink<SessionEvent>>,
}

impl SinkUi {
    async fn ask(&self, build: impl FnOnce(u32) -> SessionEvent) -> Option<Answer> {
        let (tx, rx) = oneshot::channel();
        let id = next_id();
        app().pending.insert(id, tx);
        if self.sink.add(build(id)).is_err() {
            app().pending.remove(&id);
            return None;
        }
        rx.await.ok()
    }
}

#[async_trait]
impl ConnectUi for SinkUi {
    async fn confirm_host_key(&self, question: &HostKeyQuestion) -> bool {
        let known_fingerprints = match &question.state {
            HostKeyState::Unknown => Vec::new(),
            HostKeyState::Changed { known_fingerprints } => known_fingerprints.clone(),
        };
        let key = question.key.clone();
        let answer = self
            .ask(|request_id| SessionEvent::HostKey {
                request_id,
                host: key.host,
                port: key.port,
                algorithm: key.algorithm,
                fingerprint: key.fingerprint,
                known_fingerprints,
            })
            .await;
        matches!(answer, Some(Answer::Decision(true)))
    }

    async fn ask_password(&self, prompt: &str) -> Option<String> {
        let prompt = prompt.to_string();
        match self.ask(|request_id| SessionEvent::Password { request_id, prompt }).await {
            Some(Answer::Text(text)) => text,
            _ => None,
        }
    }

    async fn answer_prompts(&self, name: &str, instructions: &str, prompts: &[Prompt]) -> Option<Vec<String>> {
        let (name, instructions) = (name.to_string(), instructions.to_string());
        let prompts: Vec<PromptView> =
            prompts.iter().map(|p| PromptView { text: p.text.clone(), echo: p.echo }).collect();
        match self.ask(|request_id| SessionEvent::ServerPrompts { request_id, name, instructions, prompts }).await {
            Some(Answer::Texts(texts)) => texts,
            _ => None,
        }
    }
}

pub(crate) fn answer(request_id: u32, answer: Answer) {
    if let Some(waiting) = app().pending.remove(&request_id) {
        let _ = waiting.send(answer);
    }
}

pub(crate) fn store_key(key: &SshKey, local_only: bool) -> Result<String> {
    let id = core()?.with_vault(|v| if local_only { v.insert_local_only(key) } else { v.insert(key) })?;
    Ok(id.to_string())
}

pub(crate) async fn run_session(
    core: Core,
    host_id: ItemId,
    session_id: u32,
    cols: u32,
    rows: u32,
    sink: &Arc<StreamSink<SessionEvent>>,
) -> Result<()> {
    let startup = core.with_vault(|v| v.get::<Host>(host_id))?.and_then(|h| h.startup_command);
    let ui: Arc<dyn ConnectUi> = Arc::new(SinkUi { sink: sink.clone() });
    let client = core.connect(host_id, ui).await?;
    let pty = PtyOptions { cols: cols.clamp(10, 1000), rows: rows.clamp(2, 500), ..PtyOptions::default() };
    let mut shell = client.shell(pty, startup.as_deref()).await?;
    app().sessions.insert(session_id, shell.input.clone());
    let _ = sink.add(SessionEvent::Connected { description: client.description().to_string() });

    while let Some(event) = shell.events.recv().await {
        let event = match event {
            ShellEvent::Output(mut data) => {
                // Dicht aufeinanderfolgende Ausgaben bündeln: Jedes Ereignis
                // kostet einen Wechsel zur Oberfläche.
                while data.len() < 64 * 1024 {
                    match shell.events.try_recv() {
                        Ok(ShellEvent::Output(more)) => data.extend(more),
                        Ok(ShellEvent::Exit(code)) => {
                            let _ = sink.add(SessionEvent::Output { data: std::mem::take(&mut data) });
                            let _ = sink.add(SessionEvent::Exit { code });
                            break;
                        }
                        Ok(ShellEvent::Closed) | Err(_) => break,
                    }
                }
                if data.is_empty() {
                    continue;
                }
                SessionEvent::Output { data }
            }
            ShellEvent::Exit(code) => SessionEvent::Exit { code },
            ShellEvent::Closed => break,
        };
        if sink.add(event).is_err() {
            break;
        }
    }
    client.disconnect().await;
    Ok(())
}

pub(crate) fn session(session_id: u32) -> Result<ShellInput> {
    app().sessions.get(&session_id).cloned().ok_or_else(|| anyhow!("Die Sitzung ist beendet"))
}
