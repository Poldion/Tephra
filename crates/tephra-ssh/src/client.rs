use crate::keys::{decode_private, describe_public};
use crate::{Result, SshError};
use async_trait::async_trait;
use russh::client::{self, Handle, KeyboardInteractiveAuthResponse, Msg};
use russh::keys::{Certificate, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, ChannelWriteHalf, Disconnect, MethodKind, MethodSet, Sig};
use std::borrow::Cow;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tephra_transport::{BoxedStream, Transport, TransportError};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

// ------------------------------------------------------------------ Hostschlüssel

/// Hostschlüssel, den ein Server beim Verbindungsaufbau vorzeigt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    /// z. B. `ssh-ed25519`.
    pub algorithm: String,
    /// Fingerabdruck in OpenSSH-Schreibweise (`SHA256:…`).
    pub fingerprint: String,
    /// Öffentlicher Schlüssel in OpenSSH-Schreibweise.
    pub public_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyDecision {
    Accept,
    Reject,
}

/// Entscheidet, ob einem Hostschlüssel vertraut wird (bekannte Hosts,
/// Rückfrage beim Nutzer …).
#[async_trait]
pub trait HostKeyVerifier: Send + Sync {
    async fn verify(&self, key: &HostKeyInfo) -> HostKeyDecision;
}

/// Akzeptiert jeden Hostschlüssel. Nur für Tests gedacht: Ohne Prüfung ist
/// eine Verbindung gegen Angreifer auf dem Übertragungsweg nicht geschützt.
#[doc(hidden)]
pub struct AcceptAnyHostKey;

#[async_trait]
impl HostKeyVerifier for AcceptAnyHostKey {
    async fn verify(&self, _key: &HostKeyInfo) -> HostKeyDecision {
        HostKeyDecision::Accept
    }
}

// ------------------------------------------------------------------ Anmeldung

/// Anmeldedaten; mehrere werden der Reihe nach versucht.
#[derive(Clone)]
pub enum Credential {
    Password(String),
    PrivateKey {
        /// Privater Schlüssel als PEM.
        pem: String,
        passphrase: Option<String>,
        /// OpenSSH-Zertifikat zum Schlüssel, falls vorhanden.
        certificate: Option<String>,
    },
    /// Rückfragen des Servers (z. B. Einmalcodes) an den Nutzer weiterreichen.
    Interactive,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Credential::Password(_) => "Password(…)",
            Credential::PrivateKey { .. } => "PrivateKey(…)",
            Credential::Interactive => "Interactive",
        })
    }
}

/// Einzelne Rückfrage des Servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub text: String,
    /// Ob die Eingabe sichtbar sein darf (bei Passwörtern `false`).
    pub echo: bool,
}

/// Beantwortet Rückfragen des Servers während der Anmeldung.
#[async_trait]
pub trait Interaction: Send + Sync {
    /// Gibt je Rückfrage eine Antwort zurück oder `None`, um abzubrechen.
    async fn answer(&self, name: &str, instructions: &str, prompts: &[Prompt]) -> Option<Vec<String>>;
}

#[derive(Debug, Clone)]
pub struct ConnectOptions {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub credentials: Vec<Credential>,
    /// Abstand der Lebenszeichen; hält Verbindungen durch NAT und Firewalls offen.
    pub keepalive: Option<Duration>,
    /// Frist für Schlüsselaustausch und Anmeldung.
    pub handshake_timeout: Duration,
}

impl ConnectOptions {
    pub fn new(host: impl Into<String>, port: u16, username: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port,
            username: username.into(),
            credentials: Vec::new(),
            keepalive: Some(Duration::from_secs(30)),
            handshake_timeout: Duration::from_secs(30),
        }
    }

    pub fn with(mut self, credential: Credential) -> Self {
        self.credentials.push(credential);
        self
    }
}

/// Frist für den Verbindungsaufbau, die stillsteht, solange auf den Nutzer
/// gewartet wird. Sonst liefe sie ab, während er noch einen Hostschlüssel
/// vergleicht oder einen Einmalcode eintippt.
struct Deadline {
    state: Mutex<DeadlineState>,
}

struct DeadlineState {
    at: Instant,
    paused: Option<Instant>,
}

struct Paused<'a>(&'a Deadline);

impl Deadline {
    fn new(limit: Duration) -> Self {
        Self { state: Mutex::new(DeadlineState { at: Instant::now() + limit, paused: None }) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DeadlineState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Hält die Frist an, bis der zurückgegebene Wächter verworfen wird.
    fn pause(&self) -> Paused<'_> {
        self.lock().paused.get_or_insert_with(Instant::now);
        Paused(self)
    }

    fn expired(&self) -> bool {
        let state = self.lock();
        state.paused.is_none() && Instant::now() >= state.at
    }

    /// Wartet, bis die Frist – Pausen eingerechnet – abgelaufen ist.
    async fn elapsed(&self) {
        while !self.expired() {
            let wait = {
                let state = self.lock();
                match state.paused {
                    Some(_) => Duration::from_millis(200),
                    None => state.at.saturating_duration_since(Instant::now()).max(Duration::from_millis(1)),
                }
            };
            tokio::time::sleep(wait).await;
        }
    }
}

impl Drop for Paused<'_> {
    fn drop(&mut self) {
        let mut state = self.0.lock();
        if let Some(since) = state.paused.take() {
            state.at += since.elapsed();
        }
    }
}

struct ClientHandler {
    verifier: Arc<dyn HostKeyVerifier>,
    host: String,
    port: u16,
    /// Fingerabdruck eines abgelehnten Schlüssels, für die Fehlermeldung.
    rejected: Arc<Mutex<Option<String>>>,
    deadline: Arc<Deadline>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> std::result::Result<bool, Self::Error> {
        let public = key.public_key();
        let Ok(described) = describe_public(&public, false) else { return Ok(false) };
        let info = HostKeyInfo {
            host: self.host.clone(),
            port: self.port,
            algorithm: described.algorithm,
            fingerprint: described.fingerprint,
            public_key: described.public_key,
        };
        let accepted = {
            let _waiting_for_user = self.deadline.pause();
            self.verifier.verify(&info).await == HostKeyDecision::Accept
        };
        if !accepted && let Ok(mut slot) = self.rejected.lock() {
            *slot = Some(info.fingerprint);
        }
        Ok(accepted)
    }
}

fn method_names(methods: &MethodSet) -> Vec<String> {
    methods.iter().map(|m| <&'static str>::from(m).to_string()).collect()
}

// ------------------------------------------------------------------ Verbindung

/// Angemeldete SSH-Verbindung. Günstig über `Arc` zu teilen; alle Methoden
/// lassen sich nebenläufig aufrufen.
pub struct SshClient {
    handle: Handle<ClientHandler>,
    description: String,
}

impl SshClient {
    /// Baut die Verbindung über `transport` auf und meldet sich an.
    pub async fn connect(
        transport: &dyn Transport,
        options: ConnectOptions,
        verifier: Arc<dyn HostKeyVerifier>,
        interaction: Option<Arc<dyn Interaction>>,
    ) -> Result<Self> {
        if options.credentials.is_empty() {
            return Err(SshError::NoCredentials(options.username));
        }
        let target = format!("{}@{}:{}", options.username, options.host, options.port);
        let description = format!("{target} ({})", transport.describe());
        let stream =
            tokio::time::timeout(options.handshake_timeout, transport.connect_tcp(&options.host, options.port))
                .await
                .map_err(|_| SshError::Timeout(format!("Verbindungsaufbau zu {target}")))??;

        let config = Arc::new(client::Config {
            client_id: russh::SshId::Standard(Cow::Borrowed(concat!("SSH-2.0-Tephra_", env!("CARGO_PKG_VERSION")))),
            keepalive_interval: options.keepalive,
            keepalive_max: 3,
            ..client::Config::default()
        });
        let rejected = Arc::new(Mutex::new(None));
        let deadline = Arc::new(Deadline::new(options.handshake_timeout));
        let handler = ClientHandler {
            verifier,
            host: options.host.clone(),
            port: options.port,
            rejected: rejected.clone(),
            deadline: deadline.clone(),
        };

        let work = async {
            let mut handle = match client::connect_stream(config, stream, handler).await {
                Ok(handle) => handle,
                Err(e) => {
                    let fingerprint = rejected.lock().ok().and_then(|mut slot| slot.take());
                    return Err(match fingerprint {
                        Some(fingerprint) => SshError::HostKeyRejected { host: options.host.clone(), fingerprint },
                        None => e.into(),
                    });
                }
            };
            authenticate(&mut handle, &options, interaction.as_deref(), &deadline).await?;
            Ok(handle)
        };
        let handle = tokio::select! {
            result = work => result?,
            _ = deadline.elapsed() => return Err(SshError::Timeout(format!("Verbindungsaufbau zu {target}"))),
        };
        Ok(Self { handle, description })
    }

    /// Ziel und Weg der Verbindung für Anzeigen und Protokolle.
    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    /// Führt einen einzelnen Befehl aus und sammelt seine Ausgabe.
    pub async fn exec(&self, command: &str, options: ExecOptions) -> Result<ExecOutput> {
        let (mut reader, writer) = self.handle.channel_open_session().await?.split();
        let mut output = ExecOutput::default();
        let stdout_and_stderr = &mut output;
        let run = async {
            writer.exec(true, command).await?;
            // Eingabe und Ausgabe laufen nebeneinander: Ein Befehl, der viel
            // ausgibt, bevor er seine Eingabe zu Ende gelesen hat, würde
            // sonst beide Seiten blockieren.
            let feed = async {
                if let Some(stdin) = &options.stdin {
                    let _ = writer.data(&stdin[..]).await;
                }
                // Dateiende senden, damit Befehle, die von der Eingabe lesen, nicht hängen.
                let _ = writer.eof().await;
            };
            let collect = collect_output(&mut reader, stdout_and_stderr, options.max_output);
            tokio::pin!(feed);
            tokio::pin!(collect);
            let mut fed = false;
            loop {
                tokio::select! {
                    result = &mut collect => break result,
                    _ = &mut feed, if !fed => fed = true,
                }
            }
        };
        let finished = match options.timeout {
            Some(limit) => tokio::time::timeout(limit, run).await.ok(),
            None => Some(run.await),
        };
        let result = match finished {
            Some(result) => result,
            None => {
                output.timed_out = true;
                // Den entfernten Prozess bitten, sich zu beenden; nicht jeder
                // Server unterstützt das.
                let _ = writer.signal(Sig::TERM).await;
                Ok(())
            }
        };
        // Der Kanal wird in jedem Fall geschlossen, sonst belegt er auf dem
        // Server dauerhaft einen der wenigen Sitzungsplätze.
        let _ = writer.close().await;
        result?;
        Ok(output)
    }

    /// Öffnet eine interaktive Shell mit Pseudoterminal. Ist `command`
    /// angegeben, wird statt der Anmeldeshell dieser Befehl im Terminal
    /// ausgeführt.
    pub async fn shell(&self, pty: PtyOptions, command: Option<&str>) -> Result<Shell> {
        let channel = self.handle.channel_open_session().await?;
        channel.request_pty(true, &pty.term, pty.cols, pty.rows, 0, 0, &[]).await?;
        match command {
            Some(command) => channel.exec(true, command).await?,
            None => channel.request_shell(true).await?,
        }
        let (mut reader, writer) = channel.split();
        let (events_tx, events) = mpsc::channel(256);
        tokio::spawn(async move {
            // Die ersten beiden Antworten gehören zur Terminal- und zur Shell-Anfrage.
            let mut pending_replies = 2u8;
            while let Some(message) = reader.wait().await {
                let event = match message {
                    ChannelMsg::Data { data } => ShellEvent::Output(data.to_vec()),
                    ChannelMsg::ExtendedData { data, .. } => ShellEvent::Output(data.to_vec()),
                    ChannelMsg::ExitStatus { exit_status } => ShellEvent::Exit(Some(exit_status)),
                    ChannelMsg::ExitSignal { .. } => ShellEvent::Exit(None),
                    ChannelMsg::Success if pending_replies > 0 => {
                        pending_replies -= 1;
                        continue;
                    }
                    ChannelMsg::Failure if pending_replies > 0 => {
                        let _ = events_tx
                            .send(ShellEvent::Output(b"Server hat Terminal oder Shell abgelehnt.\r\n".to_vec()))
                            .await;
                        break;
                    }
                    ChannelMsg::Close => break,
                    _ => continue,
                };
                if events_tx.send(event).await.is_err() {
                    break;
                }
            }
            let _ = events_tx.send(ShellEvent::Closed).await;
        });
        Ok(Shell { input: ShellInput { writer: Arc::new(writer) }, events })
    }

    /// Öffnet über diese Verbindung eine TCP-Verbindung zu einem Ziel aus
    /// Sicht des Servers (Grundlage für Jump-Hosts und Weiterleitungen).
    pub async fn open_direct_tcpip(&self, host: &str, port: u16) -> Result<BoxedStream> {
        let channel = self.handle.channel_open_direct_tcpip(host, u32::from(port), "127.0.0.1", 0).await.map_err(
            |e| match e {
                russh::Error::ChannelOpenFailure(reason) => SshError::Transport(TransportError::Connect {
                    target: format!("{host}:{port}"),
                    reason: format!("{reason:?}"),
                }),
                other => other.into(),
            },
        )?;
        Ok(Box::new(channel.into_stream()))
    }

    /// Öffnet eine SFTP-Sitzung für Dateizugriffe.
    pub async fn sftp(&self) -> Result<russh_sftp::client::SftpSession> {
        let channel = self.handle.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        russh_sftp::client::SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| SshError::Protocol(format!("SFTP konnte nicht gestartet werden: {e}")))
    }

    /// Trennt die Verbindung ordentlich.
    pub async fn disconnect(&self) {
        let _ = self.handle.disconnect(Disconnect::ByApplication, "", "").await;
    }
}

/// Liest die Ausgabe eines Befehls, bis der Server den Kanal schließt.
async fn collect_output(reader: &mut russh::ChannelReadHalf, output: &mut ExecOutput, max_output: usize) -> Result<()> {
    while let Some(message) = reader.wait().await {
        match message {
            ChannelMsg::Data { data } => append(&mut output.stdout, &data, max_output, &mut output.truncated),
            ChannelMsg::ExtendedData { data, ext: 1 } => {
                append(&mut output.stderr, &data, max_output, &mut output.truncated);
            }
            ChannelMsg::ExitStatus { exit_status } => output.exit_status = Some(exit_status),
            ChannelMsg::ExitSignal { signal_name, .. } => output.signal = Some(format!("{signal_name:?}")),
            ChannelMsg::Failure => return Err(SshError::Protocol("Server hat die Ausführung abgelehnt".into())),
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    Ok(())
}

fn append(buffer: &mut Vec<u8>, data: &[u8], limit: usize, truncated: &mut bool) {
    let room = limit.saturating_sub(buffer.len());
    if data.len() > room {
        *truncated = true;
    }
    buffer.extend_from_slice(&data[..data.len().min(room)]);
}

async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    options: &ConnectOptions,
    interaction: Option<&dyn Interaction>,
    deadline: &Deadline,
) -> Result<()> {
    let user = options.username.as_str();
    let mut remaining: Vec<String> = Vec::new();

    for credential in &options.credentials {
        let result = match credential {
            Credential::Password(password) => {
                let result = handle.authenticate_password(user, password.as_str()).await?;
                match &result {
                    russh::client::AuthResult::Failure { remaining_methods, .. }
                        if remaining_methods.contains(&MethodKind::KeyboardInteractive) =>
                    {
                        // Viele Server fragen das Passwort nur über Rückfragen ab.
                        if password_via_prompts(handle, user, password).await? {
                            return Ok(());
                        }
                        result
                    }
                    _ => result,
                }
            }
            Credential::PrivateKey { pem, passphrase, certificate } => {
                let key = Arc::new(decode_private(pem, passphrase.as_deref())?);
                match certificate.as_deref().filter(|c| !c.trim().is_empty()) {
                    Some(certificate) => {
                        let certificate =
                            Certificate::from_openssh(certificate).map_err(|e| SshError::Key(e.to_string()))?;
                        handle.authenticate_openssh_cert(user, key, certificate).await?
                    }
                    None => {
                        let hash = handle.best_supported_rsa_hash().await?.flatten();
                        handle.authenticate_publickey(user, PrivateKeyWithHashAlg::new(key, hash)).await?
                    }
                }
            }
            Credential::Interactive => {
                let Some(interaction) = interaction else { continue };
                let mut response = handle.authenticate_keyboard_interactive_start(user, None).await?;
                loop {
                    match response {
                        KeyboardInteractiveAuthResponse::Success => return Ok(()),
                        KeyboardInteractiveAuthResponse::Failure { remaining_methods, .. } => {
                            remaining = method_names(&remaining_methods);
                            break;
                        }
                        KeyboardInteractiveAuthResponse::InfoRequest { name, instructions, prompts } => {
                            let prompts: Vec<Prompt> =
                                prompts.into_iter().map(|p| Prompt { text: p.prompt, echo: p.echo }).collect();
                            let answers = {
                                let _waiting_for_user = deadline.pause();
                                interaction.answer(&name, &instructions, &prompts).await
                            };
                            let Some(answers) = answers else {
                                return Err(SshError::AuthFailed { user: user.to_string(), methods: Vec::new() });
                            };
                            response = handle.authenticate_keyboard_interactive_respond(answers).await?;
                        }
                    }
                }
                continue;
            }
        };
        match result {
            russh::client::AuthResult::Success => return Ok(()),
            russh::client::AuthResult::Failure { remaining_methods, .. } => {
                remaining = method_names(&remaining_methods);
            }
        }
    }
    Err(SshError::AuthFailed { user: user.to_string(), methods: remaining })
}

/// Beantwortet genau eine verdeckte Rückfrage des Servers mit dem Passwort.
///
/// Viele Server fragen das Passwort nicht direkt, sondern als Rückfrage ab.
/// Jede weitere Rückfrage (etwa nach einem Einmalcode) gehört dem Nutzer:
/// Das Passwort ein zweites Mal zu senden, wäre falsch und könnte auf dem
/// Server eine Sperre wegen Fehlversuchen auslösen.
async fn password_via_prompts(handle: &mut Handle<ClientHandler>, user: &str, password: &str) -> Result<bool> {
    let mut response = handle.authenticate_keyboard_interactive_start(user, None).await?;
    let mut answered = false;
    for _ in 0..4 {
        match response {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let answers = match prompts.as_slice() {
                    [] => Vec::new(),
                    [only] if !only.echo && !answered => {
                        answered = true;
                        vec![password.to_string()]
                    }
                    _ => return Ok(false),
                };
                response = handle.authenticate_keyboard_interactive_respond(answers).await?;
            }
        }
    }
    Ok(false)
}

// ------------------------------------------------------------------ Befehle

#[derive(Debug, Clone)]
pub struct ExecOptions {
    /// Nach dieser Zeit wird der Befehl abgebrochen.
    pub timeout: Option<Duration>,
    /// Obergrenze je Ausgabestrom in Byte; der Rest wird verworfen.
    pub max_output: usize,
    /// Daten für die Standardeingabe des Befehls.
    pub stdin: Option<Vec<u8>>,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self { timeout: Some(Duration::from_secs(60)), max_output: 1024 * 1024, stdin: None }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_status: Option<u32>,
    /// Name des Signals, falls der Befehl dadurch beendet wurde.
    pub signal: Option<String>,
    /// Ausgabe überschritt die Obergrenze und wurde gekürzt.
    pub truncated: bool,
    /// Befehl lief länger als erlaubt und wurde abgebrochen.
    pub timed_out: bool,
}

impl ExecOutput {
    pub fn success(&self) -> bool {
        self.exit_status == Some(0) && !self.timed_out
    }

    pub fn stdout_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

// ------------------------------------------------------------------ Shell

#[derive(Debug, Clone)]
pub struct PtyOptions {
    /// Terminaltyp, z. B. `xterm-256color`.
    pub term: String,
    pub cols: u32,
    pub rows: u32,
}

impl Default for PtyOptions {
    fn default() -> Self {
        Self { term: "xterm-256color".into(), cols: 80, rows: 24 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    /// Ausgabe für das Terminal.
    Output(Vec<u8>),
    /// Die Shell hat sich beendet (`None`: durch ein Signal).
    Exit(Option<u32>),
    /// Der Kanal ist geschlossen; es folgen keine Ereignisse mehr.
    Closed,
}

/// Interaktive Shell: Eingaben über [`ShellInput`], Ausgaben als Ereignisse.
pub struct Shell {
    pub input: ShellInput,
    pub events: mpsc::Receiver<ShellEvent>,
}

/// Eingabeseite einer Shell; lässt sich klonen und aus mehreren Aufgaben nutzen.
#[derive(Clone)]
pub struct ShellInput {
    writer: Arc<ChannelWriteHalf<Msg>>,
}

impl ShellInput {
    /// Sendet Tastatureingaben.
    pub async fn send(&self, data: &[u8]) -> Result<()> {
        Ok(self.writer.data(data).await?)
    }

    /// Meldet eine neue Terminalgröße.
    pub async fn resize(&self, cols: u32, rows: u32) -> Result<()> {
        Ok(self.writer.window_change(cols, rows, 0, 0).await?)
    }

    /// Schließt die Shell.
    pub async fn close(&self) -> Result<()> {
        let _ = self.writer.eof().await;
        Ok(self.writer.close().await?)
    }
}

// ------------------------------------------------------------------ Jump-Host

/// Nutzt eine bestehende SSH-Verbindung als Sprungbrett: Weitere
/// Verbindungen laufen als Kanal durch sie hindurch.
pub struct JumpTransport {
    client: Arc<SshClient>,
}

impl JumpTransport {
    pub fn new(client: Arc<SshClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Transport for JumpTransport {
    async fn connect_tcp(&self, host: &str, port: u16) -> std::result::Result<BoxedStream, TransportError> {
        let stream = self.client.open_direct_tcpip(host, port).await.map_err(|e| match e {
            SshError::Transport(inner) => inner,
            other => TransportError::Connect { target: format!("{host}:{port}"), reason: other.to_string() },
        })?;
        // Der Strom hält die Sprungverbindung am Leben.
        Ok(Box::new(Tethered { stream, _owner: self.client.clone() }))
    }

    fn describe(&self) -> String {
        format!("über {}", self.client.description())
    }
}

struct Tethered {
    stream: BoxedStream,
    _owner: Arc<SshClient>,
}

impl AsyncRead for Tethered {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for Tethered {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, data)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
