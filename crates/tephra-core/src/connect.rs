//! Vom gespeicherten Host zur angemeldeten SSH-Verbindung.

use crate::{Core, CoreError, Result};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tephra_model::{AuthMethod, Group, Host, Identity, ItemId, KnownHost, Proxy, Secret, SshKey};
use tephra_ssh::{
    ConnectOptions, Credential, HostKeyDecision, HostKeyInfo, HostKeyVerifier, Interaction, JumpTransport, Prompt,
    SshClient,
};
use tephra_transport::proxy::{HttpConnect, ProxyAuth, Socks5};
use tephra_transport::{Direct, SharedTransport};

/// Höchste Verschachtelungstiefe von Gruppen und Jump-Hosts.
const MAX_DEPTH: usize = 16;

/// Verhältnis eines vorgelegten Hostschlüssels zu den bekannten Schlüsseln.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyState {
    /// Zu diesem Host ist noch kein Schlüssel bekannt.
    Unknown,
    /// Es ist ein anderer Schlüssel bekannt – möglicherweise ein Angriff.
    Changed { known_fingerprints: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyQuestion {
    pub key: HostKeyInfo,
    pub state: HostKeyState,
}

/// Rückfragen an den Nutzer während des Verbindungsaufbaus.
#[async_trait]
pub trait ConnectUi: Send + Sync {
    /// Soll dem vorgelegten Hostschlüssel vertraut werden?
    async fn confirm_host_key(&self, question: &HostKeyQuestion) -> bool;

    /// Passwort für eine Identität, die keines gespeichert hat.
    async fn ask_password(&self, prompt: &str) -> Option<String>;

    /// Rückfragen des Servers (z. B. Einmalcodes).
    async fn answer_prompts(&self, name: &str, instructions: &str, prompts: &[Prompt]) -> Option<Vec<String>>;
}

/// Für Abläufe ohne Nutzer: Unbekannte oder geänderte Hostschlüssel werden
/// abgelehnt, Rückfragen bleiben unbeantwortet.
pub struct NonInteractive;

#[async_trait]
impl ConnectUi for NonInteractive {
    async fn confirm_host_key(&self, _question: &HostKeyQuestion) -> bool {
        false
    }

    async fn ask_password(&self, _prompt: &str) -> Option<String> {
        None
    }

    async fn answer_prompts(&self, _name: &str, _instructions: &str, _prompts: &[Prompt]) -> Option<Vec<String>> {
        None
    }
}

/// Host mit allem, was für die Verbindung gebraucht wird – Vererbung aus
/// Gruppen ist bereits aufgelöst.
#[derive(Debug, Clone)]
pub struct ResolvedHost {
    pub id: ItemId,
    pub host: Host,
    pub identity: Option<Identity>,
    pub key: Option<SshKey>,
    pub vpn_profile_id: Option<ItemId>,
    pub jump_host_ids: Vec<ItemId>,
}

impl Core {
    /// Löst einen Host mitsamt geerbten Einstellungen seiner Gruppen auf.
    pub fn resolve_host(&self, host_id: ItemId) -> Result<ResolvedHost> {
        self.with_vault(|vault| {
            let Some(host) = vault.get::<Host>(host_id)? else { return Ok(None) };
            let mut identity_id = host.identity_id;
            let mut vpn_profile_id = host.vpn_profile_id;
            let mut jump_host_ids = host.jump_host_ids.clone();

            let mut group_id = host.group_id;
            for _ in 0..MAX_DEPTH {
                let Some(id) = group_id else { break };
                let Some(group) = vault.get::<Group>(id)? else { break };
                identity_id = identity_id.or(group.default_identity_id);
                vpn_profile_id = vpn_profile_id.or(group.default_vpn_profile_id);
                if jump_host_ids.is_empty() {
                    jump_host_ids = group.default_jump_host_ids.clone();
                }
                group_id = group.parent_id;
            }
            // Ein Host kann nicht sein eigener Jump-Host sein.
            jump_host_ids.retain(|id| *id != host_id);

            let identity = match identity_id {
                Some(id) => vault.get::<Identity>(id)?,
                None => None,
            };
            let key = match identity.as_ref().map(|i| &i.auth) {
                Some(AuthMethod::Key { key_id }) => vault.get::<SshKey>(*key_id)?,
                _ => None,
            };
            Ok(Some(ResolvedHost { id: host_id, host, identity, key, vpn_profile_id, jump_host_ids }))
        })?
        .ok_or_else(|| CoreError::NotFound("Host".into()))
    }

    /// Kennung, unter der der bekannte Schlüssel eines Hosts gespeichert wird.
    /// Sie ergibt sich aus Netz, Adresse und Port: Zwei Geräte, die demselben
    /// Host unabhängig voneinander vertrauen, schreiben so denselben
    /// Datensatz. Weichen ihre Schlüssel voneinander ab, gewinnt einer – und
    /// das andere Gerät bekommt beim nächsten Verbinden die Warnung
    /// „Schlüssel geändert“.
    pub fn known_host_id(&self, network: Option<ItemId>, host: &str, port: u16) -> Result<ItemId> {
        let network = network.map(|id| id.to_string()).unwrap_or_default();
        let name = format!("known-host/{network}/{}:{port}", host.to_lowercase());
        self.with_vault(|v| v.derived_id(&name))
    }

    /// Alle Geheimnisse, die beim Verbinden mit einem Host im Spiel sind:
    /// Passwörter, Passphrasen und Schlüssel des Hosts, seiner Jump-Hosts,
    /// seines Proxys und seines VPN-Profils. Dient dazu, sie aus Ausgaben zu
    /// entfernen, bevor diese das Gerät verlassen.
    pub fn connection_secrets(&self, host_id: ItemId) -> Result<Vec<String>> {
        let mut secrets = Vec::new();
        for resolved in self.connection_chain(host_id)? {
            if let Some(Identity { auth: AuthMethod::Password { password }, .. }) = &resolved.identity {
                secrets.push(password.expose().to_string());
            }
            if let Some(key) = &resolved.key {
                secrets.extend(key.passphrase.as_ref().map(|p| p.expose().to_string()));
                // Den Schlüsselkörper zeilenweise aufnehmen: So wird er auch
                // erkannt, wenn eine Ausgabe ihn anders umbricht.
                secrets.extend(
                    key.private_key
                        .expose()
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.starts_with("-----"))
                        .map(str::to_string),
                );
            }
            if let Some(Proxy::Socks5 { password, .. } | Proxy::Http { password, .. }) = &resolved.host.proxy {
                secrets.extend(password.as_ref().map(|p| p.expose().to_string()));
            }
            if let Some(profile_id) = resolved.vpn_profile_id
                && let Some(profile) = self.with_vault(|v| v.get::<tephra_model::VpnProfile>(profile_id))?
            {
                secrets.push(profile.private_key.expose().to_string());
                secrets.extend(
                    profile.peers.iter().filter_map(|p| p.preshared_key.as_ref()).map(|k| k.expose().to_string()),
                );
            }
        }
        Ok(secrets)
    }

    /// Reihenfolge, in der verbunden wird: alle Jump-Hosts (samt deren
    /// eigenen Jump-Hosts), zuletzt das Ziel.
    fn connection_chain(&self, host_id: ItemId) -> Result<Vec<ResolvedHost>> {
        fn expand(core: &Core, id: ItemId, path: &mut Vec<ItemId>, chain: &mut Vec<ResolvedHost>) -> Result<()> {
            if path.contains(&id) {
                return Err(CoreError::Config("Die Jump-Hosts verweisen im Kreis aufeinander".into()));
            }
            if path.len() >= MAX_DEPTH {
                return Err(CoreError::Config("Zu viele verschachtelte Jump-Hosts".into()));
            }
            let resolved = core.resolve_host(id)?;
            path.push(id);
            for jump in &resolved.jump_host_ids {
                expand(core, *jump, path, chain)?;
            }
            path.pop();
            chain.push(resolved);
            Ok(())
        }
        let mut chain = Vec::new();
        expand(self, host_id, &mut Vec::new(), &mut chain)?;
        Ok(chain)
    }

    /// Transport für den ersten Rechner einer Kette: direkt oder durch den
    /// Tunnel des zugewiesenen VPN-Profils, optional über einen Proxy.
    async fn base_transport(&self, resolved: &ResolvedHost) -> Result<SharedTransport> {
        let mut transport: SharedTransport = match resolved.vpn_profile_id {
            Some(profile_id) => Arc::new(self.tunnel(profile_id).await?),
            None => Arc::new(Direct::default()),
        };
        if let Some(proxy) = &resolved.host.proxy {
            let auth = |username: &Option<String>, password: &Option<Secret>| {
                username.as_ref().map(|username| ProxyAuth {
                    username: username.clone(),
                    password: password.as_ref().map(|p| p.expose().to_string()).unwrap_or_default(),
                })
            };
            transport = match proxy {
                Proxy::Socks5 { host, port, username, password } => {
                    Arc::new(Socks5 { via: transport, host: host.clone(), port: *port, auth: auth(username, password) })
                }
                Proxy::Http { host, port, username, password } => Arc::new(HttpConnect {
                    via: transport,
                    host: host.clone(),
                    port: *port,
                    auth: auth(username, password),
                }),
            };
        }
        Ok(transport)
    }

    async fn connect_options(&self, resolved: &ResolvedHost, ui: &dyn ConnectUi) -> Result<ConnectOptions> {
        let label = if resolved.host.label.is_empty() { &resolved.host.address } else { &resolved.host.label };
        let identity = resolved
            .identity
            .as_ref()
            .ok_or_else(|| CoreError::Config(format!("Dem Host „{label}“ ist keine Identität zugewiesen")))?;
        let mut options =
            ConnectOptions::new(resolved.host.address.clone(), resolved.host.port, identity.username.clone());
        options.keepalive = match resolved.host.keepalive_secs {
            Some(0) => None,
            Some(secs) => Some(Duration::from_secs(u64::from(secs))),
            None => options.keepalive,
        };
        match &identity.auth {
            AuthMethod::Password { password } => {
                options.credentials.push(Credential::Password(password.expose().to_string()));
            }
            AuthMethod::Key { .. } => {
                let key = resolved
                    .key
                    .as_ref()
                    .ok_or_else(|| CoreError::NotFound(format!("SSH-Schlüssel der Identität „{}“", identity.label)))?;
                options.credentials.push(Credential::PrivateKey {
                    pem: key.private_key.expose().to_string(),
                    passphrase: key.passphrase.as_ref().map(|p| p.expose().to_string()),
                    certificate: key.certificate.clone(),
                });
            }
            AuthMethod::Ask => {
                let prompt = format!("Passwort für {}@{label}", identity.username);
                if let Some(password) = ui.ask_password(&prompt).await {
                    options.credentials.push(Credential::Password(password));
                }
            }
            AuthMethod::Agent => {
                return Err(CoreError::Config("Anmeldung über den SSH-Agenten wird noch nicht unterstützt".into()));
            }
        }
        options.credentials.push(Credential::Interactive);
        Ok(options)
    }

    /// Baut eine neue Verbindung zu einem Host auf – durch Tunnel, Proxy und
    /// Jump-Hosts, wie im Tresor hinterlegt.
    pub async fn connect(&self, host_id: ItemId, ui: Arc<dyn ConnectUi>) -> Result<Arc<SshClient>> {
        let chain = self.connection_chain(host_id)?;
        let mut previous: Option<Arc<SshClient>> = None;
        for resolved in &chain {
            let (transport, network): (SharedTransport, Option<ItemId>) = match &previous {
                Some(jump) => (Arc::new(JumpTransport::new(jump.clone())), None),
                None => (self.base_transport(resolved).await?, resolved.vpn_profile_id),
            };
            let options = self.connect_options(resolved, ui.as_ref()).await?;
            let verifier = Arc::new(TrustOnFirstUse { core: self.clone(), ui: ui.clone(), network });
            let interaction = Arc::new(UiInteraction(ui.clone()));
            let client = SshClient::connect(transport.as_ref(), options, verifier, Some(interaction)).await?;
            previous = Some(Arc::new(client));
        }
        previous.ok_or_else(|| CoreError::NotFound("Host".into()))
    }
}

struct UiInteraction(Arc<dyn ConnectUi>);

#[async_trait]
impl Interaction for UiInteraction {
    async fn answer(&self, name: &str, instructions: &str, prompts: &[Prompt]) -> Option<Vec<String>> {
        if prompts.is_empty() {
            return Some(Vec::new());
        }
        self.0.answer_prompts(name, instructions, prompts).await
    }
}

/// Prüft Hostschlüssel gegen die im Tresor bekannten: Bekannte Schlüssel
/// werden akzeptiert, unbekannte oder geänderte dem Nutzer vorgelegt.
struct TrustOnFirstUse {
    core: Core,
    ui: Arc<dyn ConnectUi>,
    /// VPN-Profil, über das der Host erreicht wird.
    network: Option<ItemId>,
}

#[async_trait]
impl HostKeyVerifier for TrustOnFirstUse {
    async fn verify(&self, key: &HostKeyInfo) -> HostKeyDecision {
        let known: Vec<(ItemId, KnownHost)> = match self.core.with_vault(|v| v.list::<KnownHost>()) {
            Ok(all) => all
                .into_iter()
                .filter(|(_, k)| {
                    k.host.eq_ignore_ascii_case(&key.host) && k.port == key.port && k.network == self.network
                })
                .collect(),
            Err(_) => return HostKeyDecision::Reject,
        };
        if known.iter().any(|(_, k)| k.key_type == key.algorithm && k.fingerprint == key.fingerprint) {
            return HostKeyDecision::Accept;
        }
        let state = if known.is_empty() {
            HostKeyState::Unknown
        } else {
            HostKeyState::Changed { known_fingerprints: known.iter().map(|(_, k)| k.fingerprint.clone()).collect() }
        };
        if !self.ui.confirm_host_key(&HostKeyQuestion { key: key.clone(), state }).await {
            return HostKeyDecision::Reject;
        }
        let entry = KnownHost {
            host: key.host.clone(),
            port: key.port,
            network: self.network,
            key_type: key.algorithm.clone(),
            fingerprint: key.fingerprint.clone(),
            public_key: key.public_key.clone(),
            first_seen_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        };
        let Ok(entry_id) = self.core.known_host_id(self.network, &key.host, key.port) else {
            return HostKeyDecision::Reject;
        };
        let stored = self.core.with_vault(|vault| {
            // Der bestätigte Schlüssel ersetzt alle bisher bekannten.
            for (id, _) in known.iter().filter(|(id, _)| *id != entry_id) {
                vault.delete(*id)?;
            }
            vault.upsert_with_id(entry_id, &entry).map(|_| ())
        });
        match stored {
            Ok(()) => HostKeyDecision::Accept,
            Err(_) => HostKeyDecision::Reject,
        }
    }
}
