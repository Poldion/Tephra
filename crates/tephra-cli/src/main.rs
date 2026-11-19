//! `tephra` – Tephra auf der Kommandozeile.

mod daemon;
mod term;
mod ui;

use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use tephra_core::model::{AuthMethod, Entity, Host, Identity, ItemId, KnownHost, Snippet, SshKey, VpnProfile};
use tephra_core::ssh::{ExecOptions, keys};
use tephra_core::transport::wireguard;
use tephra_core::{ConnectUi, Core, KdfParams, RecoveryKey};
use tephra_mcp::audit;
use tephra_mcp::policy::{AccessLevel, HostGrant, McpPolicy};
use tephra_sync::Sync;

/// SSH-Client mit Zugangsverwaltung, VPN-Tunneln, MCP-Server und Abgleich.
#[derive(Parser)]
#[command(
    name = "tephra",
    version,
    after_help = "Umgebungsvariablen für Skripte (in der Prozessliste und im Verlauf sichtbar – nur verwenden, wo das vertretbar ist):\n  TEPHRA_PASSWORD           Master-Passwort, statt es einzutippen\n  TEPHRA_NEW_PASSWORD       neues Master-Passwort für init, passwd und recover\n  TEPHRA_IDENTITY_PASSWORD  Passwort für „tephra identity add“\n  TEPHRA_KEY_PASSPHRASE     Passphrase für „tephra key import“"
)]
struct Cli {
    /// Pfad der Tresordatei. Vorgabe: der Datenordner des Benutzers
    /// (Windows: %APPDATA%\Tephra\data\vault.tephra,
    /// Linux: ~/.local/share/tephra/vault.tephra).
    #[arg(long, env = "TEPHRA_VAULT", global = true)]
    vault: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Legt einen neuen Tresor an.
    Init,
    /// Ändert das Master-Passwort.
    Passwd,
    /// Setzt mit dem Wiederherstellungsschlüssel ein neues Master-Passwort.
    Recover,
    /// Schreibt eine verschlüsselte Sicherung des Tresors.
    Export { file: PathBuf },
    /// Übernimmt Hosts aus einer OpenSSH-Konfiguration (Vorgabe: ~/.ssh/config).
    ImportSshConfig { file: Option<PathBuf> },
    /// Hosts verwalten.
    #[command(subcommand)]
    Host(HostCommand),
    /// Identitäten (Benutzername + Anmeldeverfahren) verwalten.
    #[command(subcommand)]
    Identity(IdentityCommand),
    /// SSH-Schlüssel verwalten.
    #[command(subcommand)]
    Key(KeyCommand),
    /// VPN-Profile (WireGuard) verwalten.
    #[command(subcommand)]
    Vpn(VpnCommand),
    /// Gespeicherte Befehle verwalten.
    #[command(subcommand)]
    Snippet(SnippetCommand),
    /// Bekannte Hostschlüssel anzeigen oder vergessen.
    #[command(subcommand)]
    KnownHosts(KnownHostsCommand),
    /// Öffnet eine interaktive Sitzung auf einem Host.
    Ssh { host: String },
    /// Führt einen einzelnen Befehl auf einem Host aus.
    Exec {
        host: String,
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,
    },
    /// Leitet Ports über einen Host weiter, bis das Programm mit Strg+C beendet wird.
    Forward {
        /// Bezeichnung des Hosts, über den weitergeleitet wird.
        host: String,
        /// Lokaler Port zu einem Ziel aus Sicht des Hosts: `PORT:ZIEL:ZIELPORT`.
        #[arg(short = 'L', long = "local", value_name = "PORT:ZIEL:ZIELPORT")]
        local: Vec<String>,
        /// Lokaler SOCKS5-Proxy auf diesem Port.
        #[arg(short = 'D', long = "dynamic", value_name = "PORT")]
        dynamic: Vec<u16>,
        /// Adresse, auf der gelauscht wird. Andere Adressen als die Vorgabe
        /// öffnen die Weiterleitung für das lokale Netz.
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
    },
    /// Abgleich mit einem Sync-Server.
    #[command(subcommand)]
    Sync(SyncCommand),
    /// Zugriff für Sprachmodelle (MCP).
    #[command(subcommand)]
    Mcp(McpCommand),
    /// Startet den Hintergrunddienst für MCP und Abgleich.
    Daemon,
}

#[derive(Subcommand)]
enum HostCommand {
    /// Fügt einen Host hinzu.
    Add {
        label: String,
        address: String,
        #[arg(long, default_value_t = 22)]
        port: u16,
        /// Bezeichnung der Identität für die Anmeldung.
        #[arg(long)]
        identity: Option<String>,
        /// Bezeichnung des VPN-Profils, durch das verbunden wird.
        #[arg(long)]
        vpn: Option<String>,
        /// Bezeichnungen der Jump-Hosts in Verbindungsreihenfolge.
        #[arg(long)]
        jump: Vec<String>,
        #[arg(long)]
        tag: Vec<String>,
    },
    /// Zeigt alle Einträge.
    List,
    /// Löscht einen Eintrag.
    Rm { label: String },
}

#[derive(Subcommand)]
enum IdentityCommand {
    /// Fügt eine Identität hinzu. Ohne --key und --ask wird ein Passwort abgefragt und gespeichert.
    Add {
        label: String,
        #[arg(long)]
        user: String,
        /// Bezeichnung des SSH-Schlüssels für die Anmeldung.
        #[arg(long, conflicts_with = "ask")]
        key: Option<String>,
        /// Passwort nicht speichern, sondern bei jeder Verbindung abfragen.
        #[arg(long)]
        ask: bool,
    },
    /// Zeigt alle Einträge.
    List,
    /// Löscht einen Eintrag.
    Rm { label: String },
}

#[derive(Subcommand)]
enum KeyCommand {
    /// Erzeugt ein neues Ed25519-Schlüsselpaar.
    Generate {
        label: String,
        /// Schlüssel nicht abgleichen; er bleibt auf diesem Gerät.
        #[arg(long)]
        local_only: bool,
    },
    /// Liest einen vorhandenen privaten Schlüssel ein.
    Import {
        label: String,
        file: PathBuf,
        #[arg(long)]
        local_only: bool,
    },
    /// Zeigt alle Einträge.
    List,
    /// Zeigt den öffentlichen Schlüssel zum Eintragen auf Servern.
    Show { label: String },
    /// Trägt den öffentlichen Schlüssel auf einem Host in ~/.ssh/authorized_keys ein.
    Install {
        label: String,
        /// Host, an dem sich Tephra dafür mit seiner bisherigen Anmeldung anmeldet.
        host: String,
    },
    /// Löscht einen Eintrag.
    Rm { label: String },
}

#[derive(Subcommand)]
enum VpnCommand {
    /// Liest eine WireGuard-Konfiguration (wg-quick-Format) ein.
    Import { label: String, file: PathBuf },
    /// Zeigt alle Einträge.
    List,
    /// Zeigt die Konfiguration eines Profils im wg-quick-Format.
    Export { label: String },
    /// Erzeugt ein Schlüsselpaar und zeigt es an.
    Keygen,
    /// Baut den Tunnel eines Profils auf und zeigt, ob der Handshake gelingt.
    Test { label: String },
    /// Löscht einen Eintrag.
    Rm { label: String },
}

#[derive(Subcommand)]
enum SnippetCommand {
    /// Speichert einen Befehl; Platzhalter in der Form {{name}}.
    Add { label: String, command: String },
    /// Zeigt alle Einträge.
    List,
    /// Löscht einen Eintrag.
    Rm { label: String },
}

#[derive(Subcommand)]
enum KnownHostsCommand {
    /// Zeigt alle Einträge.
    List,
    /// Vergisst den Schlüssel eines Hosts; beim nächsten Verbinden wird neu gefragt.
    Forget { host: String },
}

#[derive(Subcommand)]
enum SyncCommand {
    /// Legt auf dem Server ein Konto für diesen Tresor an.
    Register {
        server: String,
        username: String,
        #[arg(long)]
        invite: Option<String>,
    },
    /// Richtet dieses Gerät als weiteres Gerät eines bestehenden Kontos ein.
    Join { server: String, username: String },
    /// Meldet dieses Gerät neu an (abgelaufene Sitzung, geändertes Passwort).
    Reconnect,
    /// Gleicht jetzt einmal ab.
    Now,
    /// Zeigt, mit welchem Server und Konto dieses Gerät verbunden ist.
    Status,
    /// Listet die Geräte des Kontos.
    Devices,
    /// Entfernt ein Gerät vom Konto; es kann danach nicht mehr abgleichen.
    RemoveDevice { id: uuid::Uuid },
    /// Trennt dieses Gerät vom Server; der lokale Tresor bleibt.
    Disconnect,
}

#[derive(Subcommand)]
enum McpCommand {
    /// Legt einen MCP-Client an und zeigt sein Token (nur dieses eine Mal).
    AddClient { name: String },
    /// Zeigt alle MCP-Clients mit ihren Freigaben.
    Clients,
    /// Entfernt einen MCP-Client; sein Token wird ungültig und seine Verbindungen enden.
    RmClient { name: String },
    /// Gibt einen Host für einen Client frei.
    Grant {
        /// Name des MCP-Clients.
        client: String,
        /// Bezeichnung des Hosts.
        host: String,
        /// Wie viel der Client auf diesem Host ohne Rückfrage darf.
        #[arg(long, value_enum, default_value_t = Level::Confirm)]
        level: Level,
        /// Befehlsmuster, die erlaubt sind (`*` = beliebige Zeichen).
        #[arg(long)]
        allow: Vec<String>,
        /// Befehlsmuster, die gesperrt sind (nur Stufe auto).
        #[arg(long)]
        deny: Vec<String>,
        /// Erlaubt, Dateien zu lesen und Verzeichnisse aufzulisten.
        #[arg(long)]
        files_read: bool,
        /// Erlaubt, Dateien zu schreiben (nicht auf der Stufe read-only).
        #[arg(long)]
        files_write: bool,
    },
    /// Entzieht einem Client die Freigabe eines Hosts.
    Revoke { client: String, host: String },
    /// Schaltet den MCP-Zugriff insgesamt ein.
    Enable,
    /// Schaltet den MCP-Zugriff insgesamt ab (Not-Aus).
    Disable,
    /// Zeigt das Protokoll der Zugriffe.
    Audit {
        #[arg(short, long, default_value_t = 30)]
        number: usize,
    },
    /// Verbindet einen MCP-Client über Standardein-/-ausgabe mit dem laufenden Dienst.
    Serve {
        #[arg(long, env = "TEPHRA_MCP_TOKEN", hide_env_values = true)]
        token: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Level {
    /// Nur eine feste Auswahl anzeigender Programme, keine Verkettungen oder Umleitungen.
    ReadOnly,
    /// Jeder Befehl wird im Terminal des Dienstes zur Bestätigung vorgelegt.
    Confirm,
    /// Befehle laufen ohne Rückfrage; nur eine Sperrliste gegen Versehen.
    Auto,
}

type Fallible<T = ()> = Result<T, String>;

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// Benutzerverzeichnis, wie es das Betriebssystem vorgibt.
fn home_dir() -> Option<PathBuf> {
    env_path(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
}

/// Ordner für die Daten des Programms nach den Gepflogenheiten des Systems:
/// `%APPDATA%\Tephra\data` unter Windows,
/// `~/Library/Application Support/Tephra` unter macOS, sonst
/// `$XDG_DATA_HOME/tephra` (Vorgabe `~/.local/share`).
fn data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        env_path("APPDATA").map(|dir| dir.join("Tephra").join("data"))
    } else if cfg!(target_os = "macos") {
        home_dir().map(|home| home.join("Library").join("Application Support").join("Tephra"))
    } else {
        env_path("XDG_DATA_HOME")
            .or_else(|| home_dir().map(|home| home.join(".local").join("share")))
            .map(|dir| dir.join("tephra"))
    }
}

fn default_vault() -> PathBuf {
    data_dir().map(|dir| dir.join("vault.tephra")).unwrap_or_else(|| PathBuf::from("vault.tephra"))
}

/// Master-Passwort: aus `TEPHRA_PASSWORD` (für Skripte) oder vom Terminal.
async fn password(prompt: &str) -> Fallible<String> {
    if let Ok(value) = std::env::var("TEPHRA_PASSWORD") {
        return Ok(value);
    }
    ui::read_password(format!("{prompt}: ")).await.ok_or_else(|| "Keine Eingabe".to_string())
}

async fn new_password() -> Fallible<String> {
    let too_short = || "Das Master-Passwort muss mindestens 8 Zeichen lang sein.".to_string();
    if let Ok(value) = std::env::var("TEPHRA_NEW_PASSWORD") {
        return if value.chars().count() < 8 { Err(too_short()) } else { Ok(value) };
    }
    let first = ui::read_password("Neues Master-Passwort: ".into()).await.ok_or("Keine Eingabe")?;
    if first.chars().count() < 8 {
        return Err(too_short());
    }
    let second = ui::read_password("Wiederholen: ".into()).await.ok_or("Keine Eingabe")?;
    if first != second {
        return Err("Die Eingaben stimmen nicht überein.".into());
    }
    Ok(first)
}

fn show_recovery_key(recovery: &RecoveryKey) {
    println!("Wiederherstellungsschlüssel – bitte jetzt aufschreiben und sicher verwahren:");
    println!();
    println!("    {}", recovery.to_display_string());
    println!();
    println!("Er wird nirgends gespeichert und nicht noch einmal angezeigt. Ohne ihn sind die");
    println!("Daten bei vergessenem Master-Passwort verloren.");
}

async fn open(path: &Path) -> Fallible<Core> {
    if !path.exists() {
        return Err(format!("Kein Tresor unter {} – zuerst „tephra init“ ausführen.", path.display()));
    }
    let core = Core::open(path).map_err(text)?;
    core.unlock(&password("Master-Passwort").await?).map_err(text)?;
    Ok(core)
}

/// Objekte, die der Nutzer über ihre Bezeichnung anspricht.
trait Labeled: Entity {
    const WHAT: &'static str;
    fn label(&self) -> &str;
}

macro_rules! labeled {
    ($($type:ty => $what:literal),* $(,)?) => {
        $(impl Labeled for $type {
            const WHAT: &'static str = $what;
            fn label(&self) -> &str { &self.label }
        })*
    };
}
labeled!(Host => "Host", Identity => "Identität", SshKey => "SSH-Schlüssel", VpnProfile => "VPN-Profil", Snippet => "Snippet");

/// Findet ein Objekt über seine Bezeichnung (ohne Groß-/Kleinschreibung) oder Kennung.
fn find<T: Labeled>(core: &Core, name: &str) -> Fallible<(ItemId, T)> {
    let mut matches: Vec<(ItemId, T)> = core
        .with_vault(|v| v.list::<T>())
        .map_err(text)?
        .into_iter()
        .filter(|(id, item)| item.label().eq_ignore_ascii_case(name) || id.to_string() == name)
        .collect();
    match matches.len() {
        0 => Err(format!("{} „{name}“ nicht gefunden.", T::WHAT)),
        1 => Ok(matches.remove(0)),
        _ => Err(format!("Mehrere Einträge heißen „{name}“; bitte die Kennung verwenden.")),
    }
}

fn ensure_new<T: Labeled>(core: &Core, name: &str) -> Fallible {
    match find::<T>(core, name) {
        Ok(_) => Err(format!("{} „{name}“ gibt es bereits.", T::WHAT)),
        Err(_) => Ok(()),
    }
}

fn remove<T: Labeled>(core: &Core, name: &str) -> Fallible {
    let (id, _) = find::<T>(core, name)?;
    core.with_vault(|v| v.delete(id)).map_err(text)?;
    println!("{} „{name}“ gelöscht.", T::WHAT);
    Ok(())
}

async fn run(cli: Cli) -> Fallible<ExitCode> {
    let path = cli.vault.unwrap_or_else(default_vault);
    match cli.command {
        Command::Init => {
            if path.exists() {
                return Err(format!("Unter {} liegt bereits ein Tresor.", path.display()));
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(text)?;
            }
            let (_, recovery) = Core::create(&path, &new_password().await?, KdfParams::DEFAULT).map_err(text)?;
            println!("Tresor angelegt: {}", path.display());
            show_recovery_key(&recovery);
        }
        Command::Passwd => {
            let core = Core::open(&path).map_err(text)?;
            let old = password("Bisheriges Master-Passwort").await?;
            core.unlock(&old).map_err(text)?;
            let new = new_password().await?;
            let sync = Sync::new(core.clone());
            if sync.connection().map_err(text)?.is_some() {
                sync.change_password(&old, &new, KdfParams::DEFAULT).await.map_err(text)?;
                println!("Passwort geändert. Andere Geräte müssen sich mit „tephra sync reconnect“ neu anmelden.");
            } else {
                core.with_vault(|v| v.change_password(&new, KdfParams::DEFAULT)).map_err(text)?;
                println!("Passwort geändert.");
            }
        }
        Command::Recover => {
            let core = Core::open(&path).map_err(text)?;
            let typed = ui::read_line("Wiederherstellungsschlüssel: ".into()).await.ok_or("Keine Eingabe")?;
            let recovery = RecoveryKey::parse(&typed).map_err(text)?;
            let new = new_password().await?;
            core.with_vault(|v| v.recover(&recovery, &new, KdfParams::DEFAULT)).map_err(text)?;
            println!("Neues Passwort gesetzt.");
            if Sync::new(core).connection().map_err(text)?.is_some() {
                println!("Hinweis: Der Sync-Server kennt weiterhin nur das alte Passwort, und ohne dieses");
                println!("lässt es sich dort nicht ändern. Dieses Gerät gleicht weiter ab, solange seine");
                println!("Sitzung gilt. Um wieder Geräte anmelden zu können: „tephra sync disconnect“ und mit");
                println!("„tephra sync register“ ein neues Konto anlegen.");
            }
        }
        Command::Export { file } => {
            open(&path).await?.with_vault(|v| v.export_to(&file)).map_err(text)?;
            println!("Sicherung geschrieben: {}", file.display());
            println!("Nicht enthalten: Schlüssel, die als „nur dieses Gerät“ gekennzeichnet sind,");
            println!("MCP-Freigaben und die Verbindung zum Sync-Server.");
        }
        Command::ImportSshConfig { file } => {
            let home = home_dir();
            let file = match (file, &home) {
                (Some(file), _) => file,
                (None, Some(home)) => home.join(".ssh").join("config"),
                (None, None) => return Err("Das Benutzerverzeichnis ist unbekannt; bitte die Datei angeben.".into()),
            };
            let config = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let report = open(&path).await?.import_ssh_config(&config, home.as_deref()).map_err(text)?;
            println!(
                "{} Host(s), {} Identität(en) und {} Schlüssel übernommen.",
                report.hosts.len(),
                report.identities,
                report.keys
            );
            if !report.skipped.is_empty() {
                println!("Bereits vorhanden und unverändert: {}", report.skipped.join(", "));
            }
            for warning in report.warnings {
                println!("Hinweis – {warning}");
            }
        }
        Command::Host(command) => host(&open(&path).await?, command)?,
        Command::Identity(command) => identity(&open(&path).await?, command).await?,
        Command::Key(command) => key(&open(&path).await?, command).await?,
        Command::Vpn(command) => vpn(&open(&path).await?, command).await?,
        Command::Snippet(command) => {
            let core = open(&path).await?;
            match command {
                SnippetCommand::Add { label, command } => {
                    ensure_new::<Snippet>(&core, &label)?;
                    core.with_vault(|v| v.insert(&Snippet { label, command, ..Snippet::default() })).map_err(text)?;
                    println!("Snippet gespeichert.");
                }
                SnippetCommand::List => {
                    for (_, snippet) in core.with_vault(|v| v.list::<Snippet>()).map_err(text)? {
                        println!("{:<24} {}", snippet.label, snippet.command);
                    }
                }
                SnippetCommand::Rm { label } => remove::<Snippet>(&core, &label)?,
            }
        }
        Command::KnownHosts(command) => {
            let core = open(&path).await?;
            let known = core.with_vault(|v| v.list::<KnownHost>()).map_err(text)?;
            match command {
                KnownHostsCommand::List => {
                    for (_, entry) in known.iter().filter(|(_, k)| !k.fingerprint.is_empty()) {
                        let via = if entry.network.is_some() { " (über VPN)" } else { "" };
                        println!("{}:{}{via}  {} {}", entry.host, entry.port, entry.key_type, entry.fingerprint);
                    }
                }
                KnownHostsCommand::Forget { host } => {
                    let targets: Vec<ItemId> =
                        known.iter().filter(|(_, k)| k.host.eq_ignore_ascii_case(&host)).map(|(id, _)| *id).collect();
                    if targets.is_empty() {
                        return Err(format!("Zu „{host}“ ist kein Schlüssel bekannt."));
                    }
                    core.with_vault(|v| targets.iter().try_for_each(|id| v.delete(*id))).map_err(text)?;
                    println!("{} Eintrag/Einträge vergessen.", targets.len());
                }
            }
        }
        Command::Ssh { host } => {
            let core = open(&path).await?;
            let (id, entry) = find::<Host>(&core, &host)?;
            let ui: Arc<dyn ConnectUi> = Arc::new(ui::TerminalUi);
            let client = core.connect(id, ui).await.map_err(text)?;
            let exit = term::interactive(&client, entry.startup_command.as_deref()).await?;
            client.disconnect().await;
            core.lock().await;
            return Ok(ExitCode::from(exit.unwrap_or(0).min(255) as u8));
        }
        Command::Exec { host, command } => {
            let core = open(&path).await?;
            let (id, _) = find::<Host>(&core, &host)?;
            let ui: Arc<dyn ConnectUi> = Arc::new(ui::TerminalUi);
            let client = core.connect(id, ui).await.map_err(text)?;
            let options = ExecOptions { timeout: None, max_output: 256 * 1024 * 1024, stdin: None };
            let output = client.exec(&command.join(" "), options).await.map_err(text)?;
            client.disconnect().await;
            core.lock().await;
            use std::io::Write;
            std::io::stdout().write_all(&output.stdout).map_err(text)?;
            std::io::stderr().write_all(&output.stderr).map_err(text)?;
            return Ok(ExitCode::from(output.exit_status.unwrap_or(255).min(255) as u8));
        }
        Command::Forward { host, local, dynamic, bind } => {
            if local.is_empty() && dynamic.is_empty() {
                return Err("Bitte mindestens eine Weiterleitung angeben (-L oder -D).".into());
            }
            let core = open(&path).await?;
            let (id, _) = find::<Host>(&core, &host)?;
            let ui: Arc<dyn ConnectUi> = Arc::new(ui::TerminalUi);
            let client = core.connect(id, ui).await.map_err(text)?;
            let mut forwards = Vec::new();
            for spec in &local {
                let (port, target_host, target_port) = parse_forward(spec)?;
                let forward = tephra_core::ssh::forward::local(client.clone(), &bind, port, &target_host, target_port)
                    .await
                    .map_err(text)?;
                println!("{} → {target_host}:{target_port} (über {host})", forward.local_addr);
                forwards.push(forward);
            }
            for port in dynamic {
                let forward = tephra_core::ssh::forward::dynamic(client.clone(), &bind, port).await.map_err(text)?;
                println!("SOCKS5-Proxy auf {} (über {host})", forward.local_addr);
                forwards.push(forward);
            }
            println!("Beenden mit Strg+C.");
            let _ = tokio::signal::ctrl_c().await;
            drop(forwards);
            client.disconnect().await;
            core.lock().await;
        }
        Command::Sync(command) => sync(&path, command).await?,
        Command::Mcp(McpCommand::Serve { token }) => daemon::bridge(&path, &token).await?,
        Command::Mcp(command) => mcp(&open(&path).await?, command)?,
        Command::Daemon => {
            let core = open(&path).await?;
            daemon::run(core, &path).await?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn host(core: &Core, command: HostCommand) -> Fallible {
    match command {
        HostCommand::Add { label, address, port, identity, vpn, jump, tag } => {
            ensure_new::<Host>(core, &label)?;
            let identity_id = identity.map(|name| find::<Identity>(core, &name).map(|(id, _)| id)).transpose()?;
            let vpn_profile_id = vpn.map(|name| find::<VpnProfile>(core, &name).map(|(id, _)| id)).transpose()?;
            let jump_host_ids =
                jump.iter().map(|name| find::<Host>(core, name).map(|(id, _)| id)).collect::<Fallible<Vec<_>>>()?;
            let entry =
                Host { label, address, port, identity_id, vpn_profile_id, jump_host_ids, tags: tag, ..Host::default() };
            core.with_vault(|v| v.insert(&entry)).map_err(text)?;
            println!("Host „{}“ gespeichert.", entry.label);
            if identity_id.is_none() {
                println!("Hinweis: Ohne Identität (--identity) ist keine Anmeldung möglich.");
            }
        }
        HostCommand::List => {
            let hosts = core.with_vault(|v| v.list::<Host>()).map_err(text)?;
            if hosts.is_empty() {
                println!("Noch keine Hosts. Anlegen mit „tephra host add <name> <adresse> --identity <identität>“.");
            }
            for (id, entry) in hosts {
                let resolved = core.resolve_host(id).map_err(text)?;
                let user = resolved.identity.map(|i| format!("{}@", i.username)).unwrap_or_default();
                let via = if resolved.vpn_profile_id.is_some() { "  [VPN]" } else { "" };
                let jumps = if resolved.jump_host_ids.is_empty() { "" } else { "  [Jump-Host]" };
                println!("{:<24} {user}{}:{}{via}{jumps}", entry.label, entry.address, entry.port);
            }
        }
        HostCommand::Rm { label } => remove::<Host>(core, &label)?,
    }
    Ok(())
}

async fn identity(core: &Core, command: IdentityCommand) -> Fallible {
    match command {
        IdentityCommand::Add { label, user, key, ask } => {
            ensure_new::<Identity>(core, &label)?;
            let auth = match (key, ask) {
                (Some(name), _) => AuthMethod::Key { key_id: find::<SshKey>(core, &name)?.0 },
                (None, true) => AuthMethod::Ask,
                (None, false) => {
                    let typed = match std::env::var("TEPHRA_IDENTITY_PASSWORD") {
                        Ok(value) => value,
                        Err(_) => ui::read_password(format!("Passwort für {user}: ")).await.ok_or("Keine Eingabe")?,
                    };
                    AuthMethod::Password { password: typed.into() }
                }
            };
            core.with_vault(|v| v.insert(&Identity { label, username: user, auth })).map_err(text)?;
            println!("Identität gespeichert.");
        }
        IdentityCommand::List => {
            for (_, entry) in core.with_vault(|v| v.list::<Identity>()).map_err(text)? {
                let method = match entry.auth {
                    AuthMethod::Ask => "Passwort wird abgefragt",
                    AuthMethod::Password { .. } => "gespeichertes Passwort",
                    AuthMethod::Key { .. } => "SSH-Schlüssel",
                    AuthMethod::Agent => "SSH-Agent",
                };
                println!("{:<24} {:<20} {method}", entry.label, entry.username);
            }
        }
        IdentityCommand::Rm { label } => remove::<Identity>(core, &label)?,
    }
    Ok(())
}

fn store_key(core: &Core, entry: &SshKey, local_only: bool) -> Fallible {
    core.with_vault(|v| if local_only { v.insert_local_only(entry) } else { v.insert(entry) }).map_err(text)?;
    println!("Schlüssel „{}“ gespeichert{}.", entry.label, if local_only { " (nur auf diesem Gerät)" } else { "" });
    println!("{}", entry.public_key);
    Ok(())
}

async fn key(core: &Core, command: KeyCommand) -> Fallible {
    match command {
        KeyCommand::Generate { label, local_only } => {
            ensure_new::<SshKey>(core, &label)?;
            let (pem, info) = keys::generate_ed25519(&label).map_err(text)?;
            let entry = SshKey { label, private_key: pem.into(), public_key: info.public_key, ..SshKey::default() };
            store_key(core, &entry, local_only)?;
        }
        KeyCommand::Import { label, file, local_only } => {
            ensure_new::<SshKey>(core, &label)?;
            let pem = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            // Abgelegt wird der Schlüssel im OpenSSH-Format und ohne Passphrase; der Tresor verschlüsselt ihn.
            let (openssh, info) = match keys::import(&pem, None) {
                Err(tephra_core::ssh::SshError::KeyPassphraseMissing) => {
                    let typed = match std::env::var("TEPHRA_KEY_PASSPHRASE") {
                        Ok(value) => value,
                        Err(_) => {
                            ui::read_password("Passphrase des Schlüssels: ".into()).await.ok_or("Keine Eingabe")?
                        }
                    };
                    keys::import(&pem, Some(&typed)).map_err(text)?
                }
                other => other.map_err(text)?,
            };
            let entry = SshKey { label, private_key: openssh.into(), public_key: info.public_key, ..SshKey::default() };
            store_key(core, &entry, local_only)?;
        }
        KeyCommand::List => {
            for (id, entry) in core.with_vault(|v| v.list::<SshKey>()).map_err(text)? {
                let local =
                    if core.with_vault(|v| v.is_local_only(id)).map_err(text)? { "  [nur dieses Gerät]" } else { "" };
                let kind = entry.public_key.split_whitespace().next().unwrap_or("?");
                println!("{:<24} {kind}{local}", entry.label);
            }
        }
        KeyCommand::Show { label } => println!("{}", find::<SshKey>(core, &label)?.1.public_key),
        KeyCommand::Install { label, host } => {
            let (_, entry) = find::<SshKey>(core, &label)?;
            let (id, target) = find::<Host>(core, &host)?;
            let ui: Arc<dyn ConnectUi> = Arc::new(ui::TerminalUi);
            let client = core.connect(id, ui).await.map_err(text)?;
            let added = client.authorize_key(&entry.public_key).await;
            client.disconnect().await;
            if added.map_err(text)? {
                println!("Schlüssel „{}“ ist auf „{}“ eingetragen.", entry.label, target.label);
            } else {
                println!("Schlüssel „{}“ war auf „{}“ schon eingetragen.", entry.label, target.label);
            }
        }
        KeyCommand::Rm { label } => remove::<SshKey>(core, &label)?,
    }
    Ok(())
}

async fn vpn(core: &Core, command: VpnCommand) -> Fallible {
    match command {
        VpnCommand::Import { label, file } => {
            ensure_new::<VpnProfile>(core, &label)?;
            let conf = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let profile = wireguard::parse_conf(&conf, &label).map_err(text)?;
            core.with_vault(|v| v.insert(&profile)).map_err(text)?;
            println!("VPN-Profil „{label}“ gespeichert ({} Gegenstelle(n)).", profile.peers.len());
        }
        VpnCommand::List => {
            for (_, profile) in core.with_vault(|v| v.list::<VpnProfile>()).map_err(text)? {
                let endpoints: Vec<&str> = profile.peers.iter().map(|p| p.endpoint.as_str()).collect();
                println!("{:<24} {:<20} → {}", profile.label, profile.addresses.join(", "), endpoints.join(", "));
            }
        }
        VpnCommand::Export { label } => {
            print!("{}", wireguard::to_conf(&find::<VpnProfile>(core, &label)?.1).map_err(text)?)
        }
        VpnCommand::Keygen => {
            let (private, public) = wireguard::generate_keypair().map_err(text)?;
            println!("PrivateKey = {private}\nPublicKey  = {public}");
        }
        VpnCommand::Test { label } => {
            let (id, profile) = find::<VpnProfile>(core, &label)?;
            let tunnel = core.tunnel(id).await.map_err(text)?;
            // Als Ziel dient eine Adresse aus dem ersten erlaubten Bereich.
            let target = profile
                .peers
                .iter()
                .flat_map(|p| &p.allowed_ips)
                .find_map(|net| wireguard::IpNet::parse(net))
                .map(|net| net.addr)
                .ok_or("Das Profil enthält keine AllowedIPs.")?;
            tunnel.wait_ready(target, std::time::Duration::from_secs(10)).await.map_err(text)?;
            for peer in tunnel.status() {
                let rtt = peer.rtt_ms.map(|ms| format!(", {ms} ms")).unwrap_or_default();
                println!("Handshake mit {} erfolgreich{rtt}.", peer.endpoint);
            }
        }
        VpnCommand::Rm { label } => remove::<VpnProfile>(core, &label)?,
    }
    Ok(())
}

/// Zerlegt `PORT:ZIEL:ZIELPORT`. Das Ziel darf eine IPv6-Adresse in eckigen Klammern sein.
fn parse_forward(spec: &str) -> Fallible<(u16, String, u16)> {
    let invalid = || format!("Ungültige Weiterleitung „{spec}“ – erwartet wird PORT:ZIEL:ZIELPORT.");
    let (port, rest) = spec.split_once(':').ok_or_else(invalid)?;
    let (target_host, target_port) = rest.rsplit_once(':').ok_or_else(invalid)?;
    let target_host = target_host.trim_start_matches('[').trim_end_matches(']');
    if target_host.is_empty() {
        return Err(invalid());
    }
    Ok((port.parse().map_err(|_| invalid())?, target_host.to_string(), target_port.parse().map_err(|_| invalid())?))
}

fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Kommandozeile".into())
}

async fn sync(path: &Path, command: SyncCommand) -> Fallible {
    if let SyncCommand::Join { server, username } = &command {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(text)?;
        }
        let core = Sync::join(path, server, username, &password("Master-Passwort des Kontos").await?, &device_name())
            .await
            .map_err(text)?;
        let hosts = core.with_vault(|v| v.list::<Host>()).map_err(text)?.len();
        println!("Gerät eingerichtet, {hosts} Host(s) abgeholt. Tresor: {}", path.display());
        return Ok(());
    }
    let core = Core::open(path).map_err(text)?;
    let master = password("Master-Passwort").await?;
    core.unlock(&master).map_err(text)?;
    let sync = Sync::new(core);
    match command {
        SyncCommand::Join { .. } => {}
        SyncCommand::Register { server, username, invite } => {
            sync.register(&server, &username, &master, &device_name(), invite.as_deref()).await.map_err(text)?;
            let report = sync.sync_once().await.map_err(text)?;
            let noun = if report.pushed == 1 { "Datensatz" } else { "Datensätze" };
            println!("Konto „{username}“ angelegt, {} {noun} hochgeladen.", report.pushed);
        }
        SyncCommand::Reconnect => {
            sync.reconnect(&password("Aktuelles Master-Passwort des Kontos").await?, &device_name())
                .await
                .map_err(text)?;
            println!("Neu angemeldet.");
        }
        SyncCommand::Now => {
            let report = sync.sync_once().await.map_err(text)?;
            println!("{} geholt, {} gesendet, {} zusammengeführt.", report.pulled, report.pushed, report.merged);
            for (id, reason) in report.rejected {
                println!("Vom Server abgelehnt ({id}): {reason}");
            }
            if report.unreadable > 0 {
                println!("{} Datensätze vom Server waren unlesbar und wurden übersprungen.", report.unreadable);
            }
        }
        SyncCommand::Status => match sync.connection().map_err(text)? {
            Some(connection) => println!("Verbunden mit {} als {}.", connection.server, connection.username),
            None => println!("Nicht mit einem Sync-Server verbunden."),
        },
        SyncCommand::Devices => {
            for device in sync.devices().await.map_err(text)? {
                let current = if device.current { "  (dieses Gerät)" } else { "" };
                println!("{}  {}{current}", device.id, device.name);
            }
        }
        SyncCommand::RemoveDevice { id } => {
            sync.remove_device(id).await.map_err(text)?;
            println!("Gerät entfernt. Bereits abgeholte Daten bleiben dort lesbar – bei Verlust des");
            println!("Geräts sollten Passwörter und Schlüssel der Hosts erneuert werden.");
        }
        SyncCommand::Disconnect => {
            sync.disconnect().await.map_err(text)?;
            println!("Vom Server getrennt.");
        }
    }
    Ok(())
}

fn mcp(core: &Core, command: McpCommand) -> Fallible {
    let mut policy = McpPolicy::load(core).map_err(text)?;
    let client_id = |policy: &McpPolicy, name: &str| {
        policy
            .clients
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name) || c.id == name)
            .map(|c| c.id.clone())
            .ok_or_else(|| format!("MCP-Client „{name}“ nicht gefunden."))
    };
    match command {
        McpCommand::Serve { .. } => {}
        McpCommand::AddClient { name } => {
            if client_id(&policy, &name).is_ok() {
                return Err(format!("MCP-Client „{name}“ gibt es bereits."));
            }
            let (_, token) = policy.add_client(&name).map_err(text)?;
            policy.save(core).map_err(text)?;
            println!("Client „{name}“ angelegt. Token (wird nur jetzt angezeigt):");
            println!();
            println!("    {token}");
            println!();
            println!("Eintrag für die MCP-Konfiguration des Clients:");
            println!("    Befehl:   tephra mcp serve");
            println!("    Umgebung: TEPHRA_MCP_TOKEN=<Token>");
            println!("Noch ist kein Host freigegeben – siehe „tephra mcp grant“.");
        }
        McpCommand::Clients => {
            println!("MCP-Zugriff ist {}.", if policy.enabled { "eingeschaltet" } else { "ABGESCHALTET" });
            for client in &policy.clients {
                println!("{}{}", client.name, if client.disabled { "  [gesperrt]" } else { "" });
                for (host_id, grant) in &client.hosts {
                    let label = core
                        .with_vault(|v| v.get::<Host>(*host_id))
                        .ok()
                        .flatten()
                        .map(|h| h.label)
                        .unwrap_or_else(|| "(gelöschter Host)".into());
                    let level = match grant.level {
                        AccessLevel::ReadOnly => "nur lesen",
                        AccessLevel::Confirm => "mit Bestätigung",
                        AccessLevel::Auto => "automatisch",
                    };
                    let files = match (grant.files_read, grant.files_write) {
                        (true, true) => ", Dateien lesen und schreiben",
                        (true, false) => ", Dateien lesen",
                        (false, true) => ", Dateien schreiben",
                        (false, false) => "",
                    };
                    println!("    {label:<24} {level}{files}");
                }
            }
        }
        McpCommand::RmClient { name } => {
            let id = client_id(&policy, &name)?;
            policy.remove_client(&id);
            policy.save(core).map_err(text)?;
            println!("Client entfernt; sein Token ist ungültig.");
        }
        McpCommand::Grant { client, host, level, allow, deny, files_read, files_write } => {
            let id = client_id(&policy, &client)?;
            let (host_id, _) = find::<Host>(core, &host)?;
            let grant = HostGrant {
                level: match level {
                    Level::ReadOnly => AccessLevel::ReadOnly,
                    Level::Confirm => AccessLevel::Confirm,
                    Level::Auto => AccessLevel::Auto,
                },
                allow,
                deny,
                files_read,
                files_write,
            };
            match grant.level {
                AccessLevel::Auto => {
                    println!("Achtung: „auto“ lässt das Modell Befehle ohne Rückfrage ausführen. Die");
                    println!("Sperrliste verhindert Versehen, keinen gezielten Missbrauch.");
                }
                AccessLevel::ReadOnly if grant.allow.is_empty() => {
                    println!("„read-only“ erlaubt eine feste Auswahl anzeigender Programme. Das Modell kann");
                    println!("damit alles lesen, was der angemeldete Benutzer lesen darf; nur Pfade, die");
                    println!("nach Zugangsdaten aussehen, werden abgelehnt.");
                }
                AccessLevel::ReadOnly => {
                    println!("Achtung: Mit eigenen --allow-Mustern entfällt die eingebaute Prüfung einzelner");
                    println!("Optionen. Erlaubt ist dann alles, was auf ein Muster passt.");
                }
                AccessLevel::Confirm => {}
            }
            if let Some(entry) = policy.client_mut(&id) {
                entry.hosts.insert(host_id, grant);
            }
            policy.save(core).map_err(text)?;
            println!("Host „{host}“ für „{client}“ freigegeben.");
        }
        McpCommand::Revoke { client, host } => {
            let id = client_id(&policy, &client)?;
            let (host_id, _) = find::<Host>(core, &host)?;
            if let Some(entry) = policy.client_mut(&id) {
                entry.hosts.remove(&host_id);
            }
            policy.save(core).map_err(text)?;
            println!("Freigabe entzogen.");
        }
        McpCommand::Enable | McpCommand::Disable => {
            policy.enabled = matches!(command, McpCommand::Enable);
            policy.save(core).map_err(text)?;
            println!("MCP-Zugriff ist jetzt {}.", if policy.enabled { "eingeschaltet" } else { "abgeschaltet" });
        }
        McpCommand::Audit { number } => {
            let entries = audit::read(core, number, None).map_err(text)?;
            if entries.is_empty() {
                println!("Noch keine Zugriffe protokolliert.");
            }
            for (_, entry) in entries.into_iter().rev() {
                let outcome = match entry.outcome {
                    audit::Outcome::Allowed => "erlaubt",
                    audit::Outcome::Approved => "bestätigt",
                    audit::Outcome::Denied => "ABGELEHNT",
                    audit::Outcome::Rejected => "VOM NUTZER ABGELEHNT",
                    audit::Outcome::Failed => "fehlgeschlagen",
                    audit::Outcome::Completed => "beendet",
                };
                let exit = entry.exit_status.map(|code| format!(" → {code}")).unwrap_or_default();
                // Befehle und Fehlertexte stammen von Modell und Server; sie
                // dürfen die Anzeige des Protokolls nicht verändern können.
                let note = if entry.note.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", ui::printable(&entry.note, 300))
                };
                println!(
                    "{}  {:<12} {:<16} {:<18} {outcome}{exit}  {}{note}",
                    timestamp(entry.time_ms),
                    ui::printable(&entry.client_name, 40),
                    ui::printable(&entry.host_label, 40),
                    ui::printable(&entry.tool, 30),
                    ui::printable(&entry.detail, 500)
                );
            }
        }
    }
    Ok(())
}

/// Zeitpunkt als `JJJJ-MM-TT hh:mm:ss` (UTC).
fn timestamp(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Umrechnung von Tagen seit 1970 in ein Kalenderdatum (Algorithmus nach Howard Hinnant).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[tokio::main]
async fn main() -> ExitCode {
    // Meldungen gehen auf die Fehlerausgabe: Die Standardausgabe gehört bei
    // „tephra mcp serve“ dem MCP-Protokoll.
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_max_level(tracing::Level::WARN).init();
    match run(Cli::parse()).await {
        Ok(code) => code,
        Err(message) => {
            eprintln!("Fehler: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zeitstempel() {
        assert_eq!(timestamp(0), "1970-01-01 00:00:00");
        assert_eq!(timestamp(951_782_400_000), "2000-02-29 00:00:00");
        assert_eq!(timestamp(1_795_046_399_000), "2026-11-18 23:59:59");
    }

    #[test]
    fn weiterleitungsangaben() {
        assert_eq!(parse_forward("8080:intern.example:80"), Ok((8080, "intern.example".into(), 80)));
        assert_eq!(parse_forward("5432:[fd00::5]:5432"), Ok((5432, "fd00::5".into(), 5432)));
        for bad in ["8080", "8080:ziel", "x:ziel:80", "8080:ziel:x", "8080::80", "99999:ziel:80"] {
            assert!(parse_forward(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn kommandozeile_ist_widerspruchsfrei() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
