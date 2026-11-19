//! Dateien auf einem Host über SFTP: auflisten, verwalten, übertragen.
//!
//! Alles läuft über die Verbindung, die der Kern zu dem Host bereits hält –
//! auch durch Sprunghost oder Tunnel. Steht keine, baut der Kern sie ohne
//! Rückfragen neu auf; braucht das eine Eingabe, meldet er einen Fehler.

use crate::frb_generated::StreamSink;
use crate::internal::{app, forget_sftp, next_id, parse_id, sftp, RUNTIME};
use anyhow::{anyhow, Context, Result};
use flutter_rust_bridge::frb;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tephra_core::model::ItemId;
use tephra_core::ssh::SftpSession;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Ein Eintrag eines Verzeichnisses auf dem Host.
pub struct FileEntry {
    pub name: String,
    /// Verzeichnis – bei einer Verknüpfung: ob ihr Ziel eines ist.
    pub is_dir: bool,
    pub is_link: bool,
    pub size: u64,
    /// Letzte Änderung in Sekunden seit 1970, falls der Server sie nennt.
    pub modified: Option<i64>,
    /// Zugriffsrechte (untere zwölf Bit), falls der Server sie nennt.
    pub mode: Option<u32>,
}

/// Fortschritt einer Übertragung.
pub enum TransferEvent {
    /// Die Übertragung wurde angelegt; unter dieser Kennung lässt sie sich abbrechen.
    Started {
        transfer_id: u32,
    },
    Progress {
        done: u64,
        total: Option<u64>,
    },
    Finished {
        bytes: u64,
    },
    Failed {
        message: String,
    },
}

/// Führt Arbeit mit der SFTP-Sitzung des Hosts aus. Nach einem Fehler wird die
/// Sitzung verworfen, damit der nächste Aufruf eine frische öffnet – sie kann
/// mit der Verbindung gestorben sein.
async fn with_sftp<T, F, Fut>(host_id: String, work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(Arc<SftpSession>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T>> + Send,
{
    let host = parse_id(&host_id)?;
    RUNTIME
        .spawn(async move {
            let session = sftp(host).await?;
            let result = work(session).await;
            if result.is_err() {
                forget_sftp(host);
            }
            result
        })
        .await?
}

fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Das Verzeichnis, in dem eine Sitzung auf dem Host beginnt.
pub async fn files_home(host_id: String) -> Result<String> {
    with_sftp(host_id, move |session| async move {
        session.canonicalize(".").await.map_err(|e| anyhow!("Das Startverzeichnis lässt sich nicht ermitteln: {e}"))
    })
    .await
}

/// Inhalt eines Verzeichnisses: Ordner zuerst, sonst nach Namen.
pub async fn files_list(host_id: String, path: String) -> Result<Vec<FileEntry>> {
    with_sftp(host_id, move |session| async move {
        let listing = session.read_dir(path.clone()).await.map_err(|e| anyhow!("{path}: {e}"))?;
        let mut entries = Vec::new();
        for entry in listing {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let meta = entry.metadata();
            let is_link = meta.is_symlink();
            // Bei einer Verknüpfung zählt, worauf sie zeigt; ein totes Ziel gilt als Datei.
            let is_dir = if is_link {
                session.metadata(join(&path, &name)).await.map(|target| target.is_dir()).unwrap_or(false)
            } else {
                meta.is_dir()
            };
            entries.push(FileEntry {
                name,
                is_dir,
                is_link,
                size: meta.size.unwrap_or(0),
                modified: meta.mtime.map(i64::from),
                mode: meta.permissions.map(|mode| mode & 0o7777),
            });
        }
        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(entries)
    })
    .await
}

pub async fn files_mkdir(host_id: String, path: String) -> Result<()> {
    with_sftp(host_id, move |session| async move {
        session.create_dir(path.clone()).await.map_err(|e| anyhow!("{path}: {e}"))
    })
    .await
}

pub async fn files_rename(host_id: String, from: String, to: String) -> Result<()> {
    with_sftp(host_id, move |session| async move {
        if session.try_exists(to.clone()).await.unwrap_or(false) {
            return Err(anyhow!("{to} gibt es bereits."));
        }
        session.rename(from.clone(), to).await.map_err(|e| anyhow!("{from}: {e}"))
    })
    .await
}

/// Löscht eine Datei oder ein Verzeichnis samt Inhalt. Verknüpfungen werden
/// selbst gelöscht, ihr Ziel bleibt.
pub async fn files_delete(host_id: String, path: String, is_dir: bool) -> Result<()> {
    with_sftp(host_id, move |session| async move {
        if !is_dir {
            return session.remove_file(path.clone()).await.map_err(|e| anyhow!("{path}: {e}"));
        }
        // Erst den Inhalt, dann das Verzeichnis: `true` heißt, der Inhalt ist schon abgearbeitet.
        let mut stack = vec![(path, false)];
        while let Some((dir, emptied)) = stack.pop() {
            if emptied {
                session.remove_dir(dir.clone()).await.map_err(|e| anyhow!("{dir}: {e}"))?;
                continue;
            }
            stack.push((dir.clone(), true));
            for entry in session.read_dir(dir.clone()).await.map_err(|e| anyhow!("{dir}: {e}"))? {
                let name = entry.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                let child = join(&dir, &name);
                if entry.metadata().is_dir() {
                    stack.push((child, false));
                } else {
                    session.remove_file(child.clone()).await.map_err(|e| anyhow!("{child}: {e}"))?;
                }
            }
        }
        Ok(())
    })
    .await
}

/// Lädt eine Datei vom Host. Geschrieben wird zuerst neben das Ziel; erst die
/// vollständige Datei bekommt ihren Namen.
pub fn files_download(
    host_id: String,
    remote_path: String,
    local_path: String,
    sink: StreamSink<TransferEvent>,
) -> Result<()> {
    let host = parse_id(&host_id)?;
    start_transfer(host, sink, move |sink| async move {
        let session = sftp(host).await?;
        let remote = session.open(remote_path.clone()).await.map_err(|e| anyhow!("{remote_path}: {e}"))?;
        let total = remote.metadata().await.ok().and_then(|meta| meta.size);
        let target = PathBuf::from(&local_path);
        let partial = Partial::new(PathBuf::from(format!("{local_path}.part")));
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await.context("Der Zielordner lässt sich nicht anlegen")?;
        }
        let local = tokio::fs::File::create(&partial.path)
            .await
            .with_context(|| format!("{local_path} lässt sich nicht anlegen"))?;
        let bytes = copy(remote, local, total, &sink).await?;
        tokio::fs::rename(&partial.path, &target)
            .await
            .with_context(|| format!("{local_path} lässt sich nicht schreiben"))?;
        partial.keep();
        Ok(bytes)
    });
    Ok(())
}

/// Lädt eine Datei auf den Host; eine vorhandene wird ersetzt.
pub fn files_upload(
    host_id: String,
    local_path: String,
    remote_path: String,
    sink: StreamSink<TransferEvent>,
) -> Result<()> {
    let host = parse_id(&host_id)?;
    start_transfer(host, sink, move |sink| async move {
        let local =
            tokio::fs::File::open(&local_path).await.with_context(|| format!("{local_path} lässt sich nicht lesen"))?;
        let total = local.metadata().await.ok().map(|meta| meta.len());
        let session = sftp(host).await?;
        let remote = session.create(remote_path.clone()).await.map_err(|e| anyhow!("{remote_path}: {e}"))?;
        copy(local, remote, total, &sink).await
    });
    Ok(())
}

/// Bricht eine laufende Übertragung ab.
#[frb(sync)]
pub fn files_cancel(transfer_id: u32) {
    if let Some(task) = app().transfers.remove(&transfer_id) {
        task.abort();
    }
}

/// Startet eine Übertragung im Hintergrund und meldet ihren Verlauf. Nach
/// einem Fehler wird die SFTP-Sitzung des Hosts verworfen wie in `with_sftp`.
fn start_transfer<F, Fut>(host: ItemId, sink: StreamSink<TransferEvent>, work: F)
where
    F: FnOnce(Arc<StreamSink<TransferEvent>>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<u64>> + Send,
{
    let transfer_id = next_id();
    let sink = Arc::new(sink);
    let _ = sink.add(TransferEvent::Started { transfer_id });
    let task = RUNTIME.spawn(async move {
        let event = match work(sink.clone()).await {
            Ok(bytes) => TransferEvent::Finished { bytes },
            Err(error) => {
                forget_sftp(host);
                TransferEvent::Failed { message: format!("{error:#}") }
            }
        };
        app().transfers.remove(&transfer_id);
        let _ = sink.add(event);
    });
    app().transfers.insert(transfer_id, task.abort_handle());
}

/// Kopiert in Stücken und meldet höchstens zehnmal je Sekunde den Stand.
async fn copy<R, W>(mut from: R, mut to: W, total: Option<u64>, sink: &StreamSink<TransferEvent>) -> Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    let mut reported = Instant::now();
    loop {
        let read = from.read(&mut buffer).await.context("Lesen ist fehlgeschlagen")?;
        if read == 0 {
            break;
        }
        to.write_all(&buffer[..read]).await.context("Schreiben ist fehlgeschlagen")?;
        done += read as u64;
        if reported.elapsed() >= Duration::from_millis(100) {
            reported = Instant::now();
            let _ = sink.add(TransferEvent::Progress { done, total });
        }
    }
    to.shutdown().await.context("Schreiben ist fehlgeschlagen")?;
    Ok(done)
}

/// Unfertige Datei eines Herunterladens. Wird sie nicht ausdrücklich
/// behalten – bei einem Fehler oder Abbruch –, verschwindet sie wieder.
struct Partial {
    path: PathBuf,
    keep: bool,
}

impl Partial {
    fn new(path: PathBuf) -> Self {
        Self { path, keep: false }
    }

    fn keep(mut self) {
        self.keep = true;
    }
}

impl Drop for Partial {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
