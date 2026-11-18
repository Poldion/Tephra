//! Typisierte Objekte des Tresors.
//!
//! Alle Felder haben Standardwerte (`#[serde(default)]`), damit ältere
//! Datensätze nach dem Hinzufügen neuer Felder weiterhin lesbar bleiben.

use crate::record::{Entity, ItemId, ItemKind};
use crate::secret::Secret;
use serde::{Deserialize, Serialize};

fn default_ssh_port() -> u16 {
    22
}

/// Ein per SSH erreichbarer Rechner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Host {
    pub label: String,
    pub address: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub group_id: Option<ItemId>,
    pub tags: Vec<String>,
    pub identity_id: Option<ItemId>,
    /// VPN-Profil, durch das die Verbindung aufgebaut wird.
    pub vpn_profile_id: Option<ItemId>,
    /// Kette von Jump-Hosts, in Verbindungsreihenfolge.
    pub jump_host_ids: Vec<ItemId>,
    pub proxy: Option<Proxy>,
    /// Befehl, der nach dem Anmelden ausgeführt wird.
    pub startup_command: Option<String>,
    pub forwards: Vec<PortForward>,
    pub keepalive_secs: Option<u32>,
    pub notes: String,
    pub favorite: bool,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            label: String::new(),
            address: String::new(),
            port: default_ssh_port(),
            group_id: None,
            tags: Vec::new(),
            identity_id: None,
            vpn_profile_id: None,
            jump_host_ids: Vec::new(),
            proxy: None,
            startup_command: None,
            forwards: Vec::new(),
            keepalive_secs: None,
            notes: String::new(),
            favorite: false,
        }
    }
}

impl Entity for Host {
    const KIND: ItemKind = ItemKind::Host;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Proxy {
    Socks5 { host: String, port: u16, username: Option<String>, password: Option<Secret> },
    Http { host: String, port: u16, username: Option<String>, password: Option<Secret> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PortForward {
    /// Lokaler Port → Ziel aus Sicht des Servers.
    Local { bind_host: String, bind_port: u16, target_host: String, target_port: u16 },
    /// Port auf dem Server → Ziel aus Sicht des Clients.
    Remote { bind_host: String, bind_port: u16, target_host: String, target_port: u16 },
    /// Lokaler SOCKS5-Proxy durch die SSH-Verbindung.
    Dynamic { bind_host: String, bind_port: u16 },
}

/// Ordner für Hosts; vererbt Voreinstellungen an seine Mitglieder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Group {
    pub name: String,
    pub parent_id: Option<ItemId>,
    pub default_identity_id: Option<ItemId>,
    pub default_vpn_profile_id: Option<ItemId>,
    pub default_jump_host_ids: Vec<ItemId>,
}

impl Entity for Group {
    const KIND: ItemKind = ItemKind::Group;
}

/// Anmeldeinformationen: Benutzername plus Verfahren.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Identity {
    pub label: String,
    pub username: String,
    pub auth: AuthMethod,
}

impl Entity for Identity {
    const KIND: ItemKind = ItemKind::Identity;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthMethod {
    /// Passwort wird bei jeder Verbindung abgefragt.
    #[default]
    Ask,
    Password {
        password: Secret,
    },
    Key {
        key_id: ItemId,
    },
    /// Schlüssel aus dem laufenden SSH-Agenten des Systems.
    Agent,
}

/// Privater SSH-Schlüssel im OpenSSH-Format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SshKey {
    pub label: String,
    /// Privater Schlüssel als OpenSSH-PEM.
    pub private_key: Secret,
    pub passphrase: Option<Secret>,
    /// Öffentlicher Schlüssel in `authorized_keys`-Schreibweise.
    pub public_key: String,
    /// OpenSSH-Zertifikat zum Schlüssel, falls vorhanden.
    pub certificate: Option<String>,
}

impl Entity for SshKey {
    const KIND: ItemKind = ItemKind::SshKey;
}

/// WireGuard-Profil für den Tunnel innerhalb der App.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct VpnProfile {
    pub label: String,
    /// Privater Schlüssel, Base64.
    pub private_key: Secret,
    /// Eigene Adressen im Tunnel, z. B. `10.8.0.2/32`.
    pub addresses: Vec<String>,
    pub dns: Vec<String>,
    pub mtu: Option<u16>,
    pub peers: Vec<VpnPeer>,
}

impl Entity for VpnProfile {
    const KIND: ItemKind = ItemKind::VpnProfile;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct VpnPeer {
    /// Öffentlicher Schlüssel der Gegenstelle, Base64.
    pub public_key: String,
    pub preshared_key: Option<Secret>,
    /// `host:port` der Gegenstelle.
    pub endpoint: String,
    pub allowed_ips: Vec<String>,
    pub persistent_keepalive: Option<u16>,
}

/// Bekannter Hostschlüssel (Trust on first use).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KnownHost {
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// VPN-Profil, über das der Host erreicht wurde. Dieselbe private Adresse
    /// kann in verschiedenen Netzen zu verschiedenen Rechnern gehören.
    pub network: Option<ItemId>,
    /// Schlüsseltyp, z. B. `ssh-ed25519`.
    pub key_type: String,
    /// Fingerabdruck in OpenSSH-Schreibweise (`SHA256:…`).
    pub fingerprint: String,
    /// Öffentlicher Hostschlüssel in OpenSSH-Schreibweise.
    pub public_key: String,
    /// Millisekunden seit der Unix-Epoche.
    pub first_seen_ms: u64,
}

impl Entity for KnownHost {
    const KIND: ItemKind = ItemKind::KnownHost;
}

/// Gespeicherter Befehl, optional mit Platzhaltern der Form `{{name}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Snippet {
    pub label: String,
    pub command: String,
    pub description: String,
    pub tags: Vec<String>,
}

impl Entity for Snippet {
    const KIND: ItemKind = ItemKind::Snippet;
}

impl Snippet {
    /// Namen aller Platzhalter in Reihenfolge ihres ersten Auftretens.
    pub fn variables(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut rest = self.command.as_str();
        while let Some(start) = rest.find("{{") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else { break };
            let name = after[..end].trim();
            if !name.is_empty() && !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
            rest = &after[end + 2..];
        }
        names
    }

    /// Setzt Werte für die Platzhalter ein. Unbekannte Platzhalter bleiben stehen.
    pub fn render(&self, values: &std::collections::BTreeMap<String, String>) -> String {
        let mut out = String::with_capacity(self.command.len());
        let mut rest = self.command.as_str();
        while let Some(start) = rest.find("{{") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else { break };
            out.push_str(&rest[..start]);
            let name = after[..end].trim();
            match values.get(name) {
                Some(value) => out.push_str(value),
                None => out.push_str(&rest[start..start + 2 + end + 2]),
            }
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        out
    }
}

/// Geräteübergreifende Einstellungen (genau ein Datensatz pro Tresor).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    pub font_family: String,
    pub font_size: u16,
    pub scrollback_lines: u32,
    /// Belegung der Zusatztastenleiste auf Android.
    pub extra_keys: Vec<String>,
}

impl Entity for Settings {
    const KIND: ItemKind = ItemKind::Settings;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn alter_datensatz_ohne_neue_felder_bleibt_lesbar() {
        let host: Host = serde_json::from_str(r#"{"label":"alt","address":"example.org"}"#).expect("parse");
        assert_eq!(host.port, 22);
        assert!(host.tags.is_empty());
    }

    #[test]
    fn snippet_platzhalter() {
        let s = Snippet {
            command: "systemctl restart {{ dienst }} && journalctl -u {{dienst}} -n {{zeilen}}".into(),
            ..Snippet::default()
        };
        assert_eq!(s.variables(), vec!["dienst", "zeilen"]);
        let mut values = BTreeMap::new();
        values.insert("dienst".to_string(), "nginx".to_string());
        assert_eq!(s.render(&values), "systemctl restart nginx && journalctl -u nginx -n {{zeilen}}");
    }
}
