use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::process::ExitCode;
use tephra_server::db::Db;
use tephra_server::{Config, Registration};

/// Sync-Server von Tephra.
///
/// Der Server spricht HTTPS mit einem Zertifikat, das er beim ersten Start
/// selbst erzeugt; die Geräte merken sich dessen Fingerabdruck. Auf demselben
/// Port nimmt er (mit `--tls auto`) auch noch einfaches HTTP an, damit ältere
/// Geräte umsteigen können. Hinter einem Reverse-Proxy: `--tls off`.
#[derive(Parser)]
#[command(name = "tephra-server", version)]
struct Cli {
    /// Pfad der Datenbankdatei.
    #[arg(long, env = "TEPHRA_SERVER_DB", default_value = "tephra-server.sqlite", global = true)]
    db: PathBuf,

    /// Adresse, auf der der Server lauscht.
    #[arg(long, env = "TEPHRA_SERVER_BIND", default_value = "127.0.0.1:8787", global = true)]
    bind: String,

    /// Wer neue Konten anlegen darf.
    #[arg(long, env = "TEPHRA_SERVER_REGISTRATION", value_enum, default_value_t = RegistrationArg::Invite, global = true)]
    registration: RegistrationArg,

    /// Höchstzahl an Datensätzen je Konto.
    #[arg(long, env = "TEPHRA_SERVER_MAX_ITEMS", default_value_t = 50_000, global = true)]
    max_items: u64,

    /// Höchste Datenmenge je Konto in MiB.
    #[arg(long, env = "TEPHRA_SERVER_MAX_MIB", default_value_t = 256, global = true)]
    max_mib: u64,

    /// Der Server steht hinter einem Reverse-Proxy, der `X-Forwarded-For`
    /// setzt; Anmeldeversuche werden dann nach dieser Adresse begrenzt.
    #[arg(long, env = "TEPHRA_SERVER_BEHIND_PROXY", global = true)]
    behind_proxy: bool,

    /// Verschlüsselung: `auto` spricht HTTPS und HTTP auf demselben Port, `only` nur HTTPS, `off` nur HTTP.
    #[arg(long, env = "TEPHRA_SERVER_TLS", value_enum, default_value_t = TlsArg::Auto, global = true)]
    tls: TlsArg,

    /// Ordner für das selbst erzeugte Zertifikat (Vorgabe: Ordner `tls` neben der Datenbank).
    #[arg(long, env = "TEPHRA_SERVER_TLS_DIR", global = true)]
    tls_dir: Option<PathBuf>,

    /// Eigenes Zertifikat (PEM, mit Kette) statt des selbst erzeugten; braucht `--tls-key`.
    #[arg(long, env = "TEPHRA_SERVER_TLS_CERT", requires = "tls_key", global = true)]
    tls_cert: Option<PathBuf>,

    /// Privater Schlüssel zu `--tls-cert` (PEM).
    #[arg(long, env = "TEPHRA_SERVER_TLS_KEY", requires = "tls_cert", global = true)]
    tls_key: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum TlsArg {
    Auto,
    Only,
    Off,
}

#[derive(Subcommand)]
enum Command {
    /// Startet den Server (Standard, wenn kein Befehl angegeben ist).
    Serve,
    /// Erzeugt einen Einladungscode für ein neues Konto.
    Invite,
    /// Listet alle Konten mit Geräte- und Datenmenge.
    Accounts,
    /// Löscht ein Konto mit allen Daten.
    DeleteAccount { username: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum RegistrationArg {
    Open,
    Invite,
    Closed,
}

/// Wartet auf das Signal zum Beenden: Strg+C oder – unter Unix – SIGTERM,
/// wie es systemd und Docker senden.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = terminate.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    tracing::info!("Server wird beendet");
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mut db = Db::open(&cli.db)?;
    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => {
            let config = Config {
                registration: match cli.registration {
                    RegistrationArg::Open => Registration::Open,
                    RegistrationArg::Invite => Registration::Invite,
                    RegistrationArg::Closed => Registration::Closed,
                },
                max_items: cli.max_items,
                max_bytes: cli.max_mib * 1024 * 1024,
                behind_proxy: cli.behind_proxy,
                ..Config::default()
            };
            let listener = tokio::net::TcpListener::bind(&cli.bind).await?;
            let acceptor = if cli.tls == TlsArg::Off {
                None
            } else {
                let dir = cli.tls_dir.clone().unwrap_or_else(|| {
                    cli.db
                        .parent()
                        .filter(|p| !p.as_os_str().is_empty())
                        .unwrap_or(std::path::Path::new("."))
                        .join("tls")
                });
                let own = cli.tls_cert.clone().zip(cli.tls_key.clone());
                let identity = tephra_server::tls::identity(&dir, own)?;
                // Damit lässt sich prüfen, ob die Geräte wirklich mit diesem Server sprechen.
                tracing::info!(fingerprint = %identity.fingerprint, "TLS-Fingerabdruck (SHA-256)");
                Some(identity.acceptor)
            };
            let mode = match cli.tls {
                TlsArg::Auto => "HTTPS und HTTP",
                TlsArg::Only => "nur HTTPS",
                TlsArg::Off => "nur HTTP",
            };
            tracing::info!(bind = %listener.local_addr()?, db = %cli.db.display(), tls = mode, "Sync-Server gestartet");
            tephra_server::serve_tls(listener, db, config, acceptor, cli.tls != TlsArg::Only, shutdown_signal())
                .await?;
        }
        Command::Invite => println!("{}", db.create_invite()?),
        Command::Accounts => {
            let accounts = db.list_accounts()?;
            if accounts.is_empty() {
                println!("Keine Konten vorhanden.");
            }
            for account in accounts {
                println!(
                    "{:<32} {:>3} Geräte  {:>6} Datensätze  {:>8} KiB",
                    account.username,
                    account.devices,
                    account.items,
                    account.bytes / 1024
                );
            }
        }
        Command::DeleteAccount { username } => {
            db.delete_account_by_username(&username)?;
            println!("Konto {username} wurde gelöscht.");
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Fehler: {e}");
            ExitCode::FAILURE
        }
    }
}
