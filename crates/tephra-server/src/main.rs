use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::process::ExitCode;
use tephra_server::db::Db;
use tephra_server::{Config, Registration};

/// Sync-Server von Tephra.
///
/// Der Server spricht einfaches HTTP. Für den Betrieb im Netz gehört ein
/// Reverse-Proxy mit TLS davor (siehe `deploy/`).
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

    #[command(subcommand)]
    command: Option<Command>,
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
            tracing::info!(bind = %listener.local_addr()?, db = %cli.db.display(), "Sync-Server gestartet");
            tephra_server::serve(listener, db, config, shutdown_signal()).await?;
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
