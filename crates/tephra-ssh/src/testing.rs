//! SSH-Server für automatisierte Tests.
//!
//! Der Server läuft im selben Prozess und versteht eine Handvoll fest
//! verdrahteter Befehle. Er führt nichts auf dem Rechner aus und ist
//! ausschließlich für Tests gedacht.
//!
//! | Befehl          | Verhalten                                         |
//! |-----------------|---------------------------------------------------|
//! | `echo TEXT`     | gibt `TEXT` aus, Exit-Code 0                      |
//! | `whoami`        | gibt den Benutzernamen aus                        |
//! | `fail`          | schreibt `kaputt` auf die Fehlerausgabe, Code 3   |
//! | `cat`           | gibt die Standardeingabe zurück                   |
//! | `big N`         | gibt N Byte aus                                   |
//! | `sleep MS`      | endet nach MS Millisekunden                       |
//! | alles andere    | Fehlerausgabe, Code 127                           |
//!
//! Dazu kommt ein SFTP-Subsystem mit einem Dateisystem im Arbeitsspeicher
//! ([`TestServer::files`]).

use russh::keys::{Algorithm, HashAlg, PrivateKey};
use russh::server::{self, Auth, Msg, Session};
use russh::{Channel, ChannelId, ChannelOpenFailure};
use russh_sftp::protocol::{Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};

pub const USER: &str = "tester";
pub const PASSWORD: &str = "geheim";

/// Laufender Test-Server.
pub struct TestServer {
    /// Adresse, unter der der Server per TCP erreichbar ist.
    pub addr: SocketAddr,
    /// Fingerabdruck des Hostschlüssels (`SHA256:…`).
    pub host_key_fingerprint: String,
    /// Privater Schlüssel, mit dem sich [`USER`] anmelden darf.
    pub client_key_pem: String,
    /// Dateien des SFTP-Subsystems: absoluter Pfad → Inhalt.
    pub files: Files,
    config: Arc<server::Config>,
    authorized_fingerprint: String,
}

/// Dateisystem im Arbeitsspeicher, geteilt zwischen Test und Server.
pub type Files = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

impl TestServer {
    /// Startet den Server auf einem freien Port der Loopback-Schnittstelle.
    pub async fn start() -> Self {
        let server = Self::without_listener();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("Test-Server: Port belegen");
        let addr = listener.local_addr().expect("Test-Server: Adresse");
        let (config, authorized, files) =
            (server.config.clone(), server.authorized_fingerprint.clone(), server.files.clone());
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                spawn_session(config.clone(), authorized.clone(), files.clone(), stream);
            }
        });
        Self { addr, ..server }
    }

    /// Erzeugt einen Server ohne eigenen Port; Verbindungen werden über
    /// [`TestServer::serve`] übergeben (z. B. aus einem Tunnel).
    pub fn without_listener() -> Self {
        let host_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("Hostschlüssel");
        let (client_key_pem, client_info) = crate::keys::generate_ed25519("test").expect("Clientschlüssel");
        let config = server::Config {
            auth_rejection_time: Duration::from_millis(20),
            auth_rejection_time_initial: Some(Duration::ZERO),
            keys: vec![host_key.clone()],
            ..server::Config::default()
        };
        Self {
            addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            host_key_fingerprint: host_key.public_key().fingerprint(HashAlg::Sha256).to_string(),
            client_key_pem,
            files: Files::default(),
            config: Arc::new(config),
            authorized_fingerprint: client_info.fingerprint,
        }
    }

    /// Bedient eine einzelne, bereits aufgebaute Verbindung.
    pub fn serve<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(&self, stream: S) {
        spawn_session(self.config.clone(), self.authorized_fingerprint.clone(), self.files.clone(), stream);
    }
}

fn spawn_session<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    config: Arc<server::Config>,
    authorized_fingerprint: String,
    files: Files,
    stream: S,
) {
    tokio::spawn(async move {
        let handler = Handler { authorized_fingerprint, files, channels: HashMap::new() };
        if let Ok(session) = server::run_stream(config, stream, handler).await {
            let _ = session.await;
        }
    });
}

#[derive(Default)]
struct ChannelState {
    shell: bool,
    cat: Option<Vec<u8>>,
    /// Hält den Kanal offen, solange die Sitzung läuft.
    _channel: Option<Channel<Msg>>,
}

struct Handler {
    authorized_fingerprint: String,
    files: Files,
    channels: HashMap<ChannelId, ChannelState>,
}

impl Handler {
    fn finish(session: &mut Session, channel: ChannelId, code: u32) -> Result<(), russh::Error> {
        session.exit_status_request(channel, code)?;
        session.eof(channel)?;
        session.close(channel)
    }
}

impl server::Handler for Handler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == USER && password == PASSWORD { Auth::Accept } else { Auth::reject() })
    }

    async fn auth_publickey(&mut self, user: &str, key: server::VerifiedPublicKey<'_>) -> Result<Auth, Self::Error> {
        let fingerprint = key.public_key.fingerprint(HashAlg::Sha256).to_string();
        Ok(if user == USER && fingerprint == self.authorized_fingerprint { Auth::Accept } else { Auth::reject() })
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), ChannelState { _channel: Some(channel), ..ChannelState::default() });
        reply.accept().await;
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        match TcpStream::connect((host_to_connect, port_to_connect as u16)).await {
            Ok(mut target) => {
                reply.accept().await;
                tokio::spawn(async move {
                    let mut stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut target).await;
                });
            }
            Err(_) => reply.reject(ChannelOpenFailure::ConnectFailed).await,
        }
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        self.channels.entry(channel).or_default().shell = true;
        session.channel_success(channel)?;
        session.data(channel, b"bereit> ".to_vec())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.data(channel, format!("[{col_width}x{row_height}]").into_bytes())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        let command = String::from_utf8_lossy(data).into_owned();
        let (name, argument) = command.split_once(' ').unwrap_or((command.as_str(), ""));
        match name {
            "echo" => {
                session.data(channel, format!("{argument}\n").into_bytes())?;
                Self::finish(session, channel, 0)
            }
            "whoami" => {
                session.data(channel, format!("{USER}\n").into_bytes())?;
                Self::finish(session, channel, 0)
            }
            "fail" => {
                session.extended_data(channel, 1, b"kaputt\n".to_vec())?;
                Self::finish(session, channel, 3)
            }
            "cat" => {
                self.channels.entry(channel).or_default().cat = Some(Vec::new());
                Ok(())
            }
            "big" => {
                let size: usize = argument.trim().parse().unwrap_or(0);
                for chunk in vec![b'x'; size].chunks(16 * 1024) {
                    session.data(channel, chunk.to_vec())?;
                }
                Self::finish(session, channel, 0)
            }
            "sleep" => {
                let millis: u64 = argument.trim().parse().unwrap_or(0);
                let handle = session.handle();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(millis)).await;
                    let _ = handle.exit_status_request(channel, 0).await;
                    let _ = handle.eof(channel).await;
                    let _ = handle.close(channel).await;
                });
                Ok(())
            }
            _ => {
                session.extended_data(channel, 1, format!("unbekannter Befehl: {name}\n").into_bytes())?;
                Self::finish(session, channel, 127)
            }
        }
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let stream = self.channels.get_mut(&channel).and_then(|state| state._channel.take());
        match (name, stream) {
            ("sftp", Some(stream)) => {
                session.channel_success(channel)?;
                let sftp = Sftp { files: self.files.clone(), listed: HashSet::new() };
                russh_sftp::server::run(stream.into_stream(), sftp).await;
                Ok(())
            }
            _ => session.channel_failure(channel),
        }
    }

    async fn data(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        let Some(state) = self.channels.get_mut(&channel) else { return Ok(()) };
        if let Some(buffer) = state.cat.as_mut() {
            buffer.extend_from_slice(data);
        } else if state.shell {
            // Die Shell gibt jede Eingabe zurück; `exit` beendet sie.
            session.data(channel, data.to_vec())?;
            if data.starts_with(b"exit") {
                Self::finish(session, channel, 0)?;
            }
        }
        Ok(())
    }

    async fn channel_eof(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        if let Some(buffer) = self.channels.get_mut(&channel).and_then(|state| state.cat.take()) {
            session.data(channel, buffer)?;
            Self::finish(session, channel, 0)?;
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ SFTP

/// SFTP auf dem Dateisystem im Arbeitsspeicher. Verzeichnisse ergeben sich
/// aus den Pfaden der Dateien.
struct Sftp {
    files: Files,
    /// Verzeichnisse, deren Inhalt bereits vollständig geliefert wurde.
    listed: HashSet<String>,
}

fn done(id: u32) -> Status {
    Status { id, status_code: StatusCode::Ok, error_message: "Ok".into(), language_tag: "en-US".into() }
}

fn file_attrs(size: usize) -> FileAttributes {
    let mut attrs = FileAttributes { size: Some(size as u64), permissions: Some(0o644), ..FileAttributes::default() };
    attrs.set_regular(true);
    attrs
}

fn dir_attrs() -> FileAttributes {
    let mut attrs = FileAttributes { permissions: Some(0o755), ..FileAttributes::default() };
    attrs.set_dir(true);
    attrs
}

fn dir_prefix(path: &str) -> String {
    format!("{}/", path.trim_end_matches('/'))
}

impl Sftp {
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Vec<u8>>> {
        self.files.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn attrs_of(&self, path: &str) -> Result<FileAttributes, StatusCode> {
        let files = self.lock();
        if let Some(content) = files.get(path) {
            return Ok(file_attrs(content.len()));
        }
        let prefix = dir_prefix(path);
        if path == "/" || files.keys().any(|name| name.starts_with(&prefix)) {
            return Ok(dir_attrs());
        }
        Err(StatusCode::NoSuchFile)
    }
}

impl russh_sftp::server::Handler for Sftp {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(&mut self, _version: u32, _extensions: HashMap<String, String>) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let mut files = self.lock();
        if pflags.contains(OpenFlags::CREATE) {
            files.entry(filename.clone()).or_default();
        }
        let content = files.get_mut(&filename).ok_or(StatusCode::NoSuchFile)?;
        if pflags.contains(OpenFlags::TRUNCATE) {
            content.clear();
        }
        Ok(Handle { id, handle: filename })
    }

    async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
        Ok(done(id))
    }

    async fn read(&mut self, id: u32, handle: String, offset: u64, len: u32) -> Result<Data, Self::Error> {
        let files = self.lock();
        let content = files.get(&handle).ok_or(StatusCode::NoSuchFile)?;
        let start = offset as usize;
        if start >= content.len() {
            return Err(StatusCode::Eof);
        }
        let end = (start + len as usize).min(content.len());
        Ok(Data { id, data: content[start..end].to_vec() })
    }

    async fn write(&mut self, id: u32, handle: String, offset: u64, data: Vec<u8>) -> Result<Status, Self::Error> {
        let mut files = self.lock();
        let content = files.get_mut(&handle).ok_or(StatusCode::NoSuchFile)?;
        let end = offset as usize + data.len();
        if content.len() < end {
            content.resize(end, 0);
        }
        content[offset as usize..end].copy_from_slice(&data);
        Ok(done(id))
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        Ok(Attrs { id, attrs: self.attrs_of(&path)? })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        Ok(Attrs { id, attrs: self.attrs_of(&path)? })
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        Ok(Attrs { id, attrs: self.attrs_of(&handle)? })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        if !self.attrs_of(&path)?.is_dir() {
            return Err(StatusCode::NoSuchFile);
        }
        self.listed.remove(&path);
        Ok(Handle { id, handle: path })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        if !self.listed.insert(handle.clone()) {
            return Err(StatusCode::Eof);
        }
        let prefix = dir_prefix(&handle);
        let mut entries: BTreeMap<String, FileAttributes> = BTreeMap::new();
        for (path, content) in self.lock().iter() {
            let Some(rest) = path.strip_prefix(&prefix) else { continue };
            match rest.split_once('/') {
                Some((dir, _)) => entries.insert(dir.to_string(), dir_attrs()),
                None => entries.insert(rest.to_string(), file_attrs(content.len())),
            };
        }
        Ok(Name { id, files: entries.into_iter().map(|(name, attrs)| File::new(name, attrs)).collect() })
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let path = if path.is_empty() || path == "." { "/".to_string() } else { path };
        Ok(Name { id, files: vec![File::dummy(path)] })
    }
}
