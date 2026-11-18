//! WireGuard-Konfiguration: Prüfen, Einlesen und Schreiben von Profilen.

use crate::TransportError;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use boringtun::x25519::{PublicKey, StaticSecret};
use std::net::IpAddr;
use tephra_model::{VpnPeer, VpnProfile};
use zeroize::Zeroize;

pub const DEFAULT_MTU: u16 = 1420;
/// Kleinste MTU, mit der IPv6 noch funktioniert.
const MIN_MTU: u16 = 1280;

/// Adressbereich in CIDR-Schreibweise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpNet {
    pub addr: IpAddr,
    pub prefix: u8,
}

impl IpNet {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (addr, prefix) = match text.split_once('/') {
            Some((a, p)) => (a.trim().parse::<IpAddr>().ok()?, Some(p.trim().parse::<u8>().ok()?)),
            None => (text.parse::<IpAddr>().ok()?, None),
        };
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = prefix.unwrap_or(max);
        (prefix <= max).then_some(Self { addr, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = if self.prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(self.prefix)) };
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = if self.prefix == 0 { 0 } else { u128::MAX << (128 - u32::from(self.prefix)) };
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

/// Geprüfte, einsatzbereite Tunnelkonfiguration.
pub struct WgConfig {
    pub(crate) private_key: [u8; 32],
    pub addresses: Vec<IpNet>,
    pub dns: Vec<IpAddr>,
    pub mtu: u16,
    /// Fester lokaler UDP-Port; ohne Angabe wählt das Betriebssystem einen freien.
    pub listen_port: Option<u16>,
    /// Nur für Tests: verwirft jedes n-te ausgehende UDP-Paket, um ein
    /// verlustbehaftetes Netz nachzustellen.
    #[doc(hidden)]
    pub simulate_loss: Option<u32>,
    pub peers: Vec<WgPeer>,
}

impl Drop for WgConfig {
    fn drop(&mut self) {
        self.private_key.zeroize();
        for peer in &mut self.peers {
            if let Some(psk) = peer.preshared_key.as_mut() {
                psk.zeroize();
            }
        }
    }
}

pub struct WgPeer {
    pub public_key: [u8; 32],
    pub(crate) preshared_key: Option<[u8; 32]>,
    /// `host:port`, wird beim Start des Tunnels aufgelöst.
    pub endpoint: String,
    pub allowed_ips: Vec<IpNet>,
    pub keepalive: Option<u16>,
}

fn config_err(msg: impl Into<String>) -> TransportError {
    TransportError::Config(msg.into())
}

fn decode_key(text: &str, what: &str) -> Result<[u8; 32], TransportError> {
    let mut bytes = B64.decode(text.trim()).map_err(|_| config_err(format!("{what} ist kein gültiges Base64")))?;
    let result =
        <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| config_err(format!("{what} muss 32 Byte lang sein")));
    bytes.zeroize();
    result
}

impl WgConfig {
    /// Prüft ein gespeichertes Profil und macht es einsatzbereit.
    pub fn from_profile(profile: &VpnProfile) -> Result<Self, TransportError> {
        let private_key = decode_key(profile.private_key.expose(), "Privater Schlüssel")?;
        let addresses = profile
            .addresses
            .iter()
            .map(|a| IpNet::parse(a).ok_or_else(|| config_err(format!("Ungültige Adresse „{a}“"))))
            .collect::<Result<Vec<_>, _>>()?;
        if addresses.is_empty() {
            return Err(config_err("Das Profil braucht mindestens eine eigene Adresse"));
        }
        let dns = profile
            .dns
            .iter()
            .map(|d| d.trim().parse::<IpAddr>().map_err(|_| config_err(format!("Ungültiger DNS-Server „{d}“"))))
            .collect::<Result<Vec<_>, _>>()?;
        let mtu = profile.mtu.unwrap_or(DEFAULT_MTU);
        if !(MIN_MTU..=9000).contains(&mtu) {
            return Err(config_err(format!("MTU {mtu} liegt außerhalb von {MIN_MTU}–9000")));
        }
        if profile.peers.is_empty() {
            return Err(config_err("Das Profil braucht mindestens eine Gegenstelle (Peer)"));
        }
        let mut peers = Vec::new();
        for peer in &profile.peers {
            let endpoint = peer.endpoint.trim();
            if endpoint.is_empty() {
                return Err(config_err("Jede Gegenstelle braucht einen Endpoint (host:port)"));
            }
            // Ein Endpoint ist ein einzelnes Wort; alles andere könnte beim
            // Export weitere Zeilen in die Konfigurationsdatei schmuggeln.
            if endpoint.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err(config_err("Der Endpoint darf keine Leer- oder Steuerzeichen enthalten"));
            }
            let allowed_ips = peer
                .allowed_ips
                .iter()
                .map(|a| IpNet::parse(a).ok_or_else(|| config_err(format!("Ungültiger Eintrag in AllowedIPs: „{a}“"))))
                .collect::<Result<Vec<_>, _>>()?;
            peers.push(WgPeer {
                public_key: decode_key(&peer.public_key, "Öffentlicher Schlüssel der Gegenstelle")?,
                preshared_key: peer
                    .preshared_key
                    .as_ref()
                    .filter(|k| !k.expose().trim().is_empty())
                    .map(|k| decode_key(k.expose(), "Preshared Key"))
                    .transpose()?,
                endpoint: peer.endpoint.trim().to_string(),
                allowed_ips,
                keepalive: peer.persistent_keepalive.filter(|k| *k > 0),
            });
        }
        Ok(Self { private_key, addresses, dns, mtu, listen_port: None, simulate_loss: None, peers })
    }
}

/// Erzeugt ein neues Schlüsselpaar und gibt `(privat, öffentlich)` in Base64 zurück.
pub fn generate_keypair() -> Result<(String, String), TransportError> {
    let mut bytes = tephra_crypto::random_bytes::<32>().map_err(|e| config_err(e.to_string()))?;
    let secret = StaticSecret::from(bytes);
    bytes.zeroize();
    let public = PublicKey::from(&secret);
    Ok((B64.encode(secret.to_bytes()), B64.encode(public.as_bytes())))
}

/// Öffentlicher Schlüssel zu einem privaten Schlüssel (beide Base64).
pub fn public_key_of(private_key: &str) -> Result<String, TransportError> {
    let secret = StaticSecret::from(decode_key(private_key, "Privater Schlüssel")?);
    Ok(B64.encode(PublicKey::from(&secret).as_bytes()))
}

fn split_list(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// Liest eine Konfiguration im `wg-quick`-Format (`[Interface]` / `[Peer]`).
///
/// Einträge, die nur für ein systemweites VPN Sinn ergeben (`PostUp`,
/// `Table`, `FwMark` …), werden ignoriert: Der Tunnel läuft in der App und
/// führt nie Befehle aus.
pub fn parse_conf(text: &str, label: &str) -> Result<VpnProfile, TransportError> {
    #[derive(PartialEq)]
    enum Section {
        None,
        Interface,
        Peer,
        Other,
    }
    let mut profile = VpnProfile { label: label.to_string(), ..VpnProfile::default() };
    let mut section = Section::None;
    let mut saw_interface = false;

    for (number, raw) in text.lines().enumerate() {
        let line = raw.split(['#', ';']).next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = match name.trim().to_ascii_lowercase().as_str() {
                "interface" => {
                    saw_interface = true;
                    Section::Interface
                }
                "peer" => {
                    profile.peers.push(VpnPeer::default());
                    Section::Peer
                }
                _ => Section::Other,
            };
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(config_err(format!("Zeile {}: „Schlüssel = Wert“ erwartet", number + 1)));
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match section {
            Section::Interface => match key.as_str() {
                "privatekey" => profile.private_key = value.into(),
                "address" => profile.addresses.extend(split_list(value)),
                "dns" => {
                    // wg-quick erlaubt hier auch Suchdomänen; nur Adressen übernehmen.
                    profile.dns.extend(split_list(value).into_iter().filter(|d| d.parse::<IpAddr>().is_ok()));
                }
                "mtu" => {
                    profile.mtu =
                        Some(value.parse().map_err(|_| config_err(format!("Zeile {}: ungültige MTU", number + 1)))?);
                }
                _ => {}
            },
            Section::Peer => {
                let Some(peer) = profile.peers.last_mut() else { continue };
                match key.as_str() {
                    "publickey" => peer.public_key = value.to_string(),
                    "presharedkey" => peer.preshared_key = Some(value.into()),
                    "endpoint" => peer.endpoint = value.to_string(),
                    "allowedips" => peer.allowed_ips.extend(split_list(value)),
                    "persistentkeepalive" => {
                        peer.persistent_keepalive = match value.to_ascii_lowercase().as_str() {
                            "off" | "0" => None,
                            v => Some(v.parse().map_err(|_| {
                                config_err(format!("Zeile {}: ungültiges PersistentKeepalive", number + 1))
                            })?),
                        };
                    }
                    _ => {}
                }
            }
            Section::None => return Err(config_err(format!("Zeile {}: Eintrag vor dem ersten Abschnitt", number + 1))),
            Section::Other => {}
        }
    }
    if !saw_interface {
        return Err(config_err("Abschnitt [Interface] fehlt"));
    }
    // Vollständige Prüfung, damit der Import nichts Unbrauchbares speichert.
    WgConfig::from_profile(&profile)?;
    Ok(profile)
}

/// Schreibt ein Profil im `wg-quick`-Format. Das Profil wird vorher
/// geprüft, damit kein Feld zusätzliche Zeilen einschleusen kann.
pub fn to_conf(profile: &VpnProfile) -> Result<String, TransportError> {
    WgConfig::from_profile(profile)?;
    let line = |text: &str| text.replace(['\r', '\n'], " ");
    let mut out = String::from("[Interface]\n");
    out.push_str(&format!("PrivateKey = {}\n", line(profile.private_key.expose())));
    if !profile.addresses.is_empty() {
        out.push_str(&format!("Address = {}\n", line(&profile.addresses.join(", "))));
    }
    if !profile.dns.is_empty() {
        out.push_str(&format!("DNS = {}\n", line(&profile.dns.join(", "))));
    }
    if let Some(mtu) = profile.mtu {
        out.push_str(&format!("MTU = {mtu}\n"));
    }
    for peer in &profile.peers {
        out.push_str("\n[Peer]\n");
        out.push_str(&format!("PublicKey = {}\n", line(&peer.public_key)));
        if let Some(psk) = &peer.preshared_key {
            out.push_str(&format!("PresharedKey = {}\n", line(psk.expose())));
        }
        if !peer.allowed_ips.is_empty() {
            out.push_str(&format!("AllowedIPs = {}\n", line(&peer.allowed_ips.join(", "))));
        }
        out.push_str(&format!("Endpoint = {}\n", line(&peer.endpoint)));
        if let Some(keepalive) = peer.persistent_keepalive {
            out.push_str(&format!("PersistentKeepalive = {keepalive}\n"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        let (private, _) = generate_keypair().expect("keys");
        let (_, peer_public) = generate_keypair().expect("keys");
        format!(
            "# Beispiel\n[Interface]\nPrivateKey = {private}\nAddress = 10.8.0.2/24, fd00::2/64\nDNS = 10.8.0.1, intern.example\nMTU = 1400\nPostUp = rm -rf / ; darf nie ausgeführt werden\n\n[Peer]\nPublicKey = {peer_public}\nAllowedIPs = 10.8.0.0/24, 192.168.50.0/24\nEndpoint = vpn.example.org:51820\nPersistentKeepalive = 25\n"
        )
    }

    #[test]
    fn conf_einlesen_und_schreiben() {
        let profile = parse_conf(&sample(), "Büro").expect("parse");
        assert_eq!(profile.label, "Büro");
        assert_eq!(profile.addresses, vec!["10.8.0.2/24", "fd00::2/64"]);
        assert_eq!(profile.dns, vec!["10.8.0.1"]);
        assert_eq!(profile.mtu, Some(1400));
        assert_eq!(profile.peers.len(), 1);
        assert_eq!(profile.peers[0].endpoint, "vpn.example.org:51820");
        assert_eq!(profile.peers[0].persistent_keepalive, Some(25));

        let again = parse_conf(&to_conf(&profile).expect("schreiben"), "Büro").expect("parse");
        assert_eq!(again, profile);

        // Ein Endpoint mit Zeilenumbruch darf keine weiteren Einträge einschleusen.
        let mut evil = profile.clone();
        evil.peers[0].endpoint = "vpn.example.org:51820\nPostUp = rm -rf /".into();
        assert!(to_conf(&evil).is_err());
        assert!(WgConfig::from_profile(&evil).is_err());
    }

    #[test]
    fn fehlerhafte_konfigurationen() {
        assert!(parse_conf("", "x").is_err());
        assert!(parse_conf("PrivateKey = abc", "x").is_err());
        assert!(parse_conf("[Interface]\nPrivateKey = kein-base64\nAddress = 10.0.0.2\n", "x").is_err());
        let no_peer = sample().split("[Peer]").next().expect("teil").to_string();
        assert!(parse_conf(&no_peer, "x").is_err());
        let bad_mtu = sample().replace("MTU = 1400", "MTU = 100");
        assert!(parse_conf(&bad_mtu, "x").is_err());
        let bad_ip = sample().replace("192.168.50.0/24", "192.168.50.0/99");
        assert!(parse_conf(&bad_ip, "x").is_err());
    }

    #[test]
    fn schluesselpaar() {
        let (private, public) = generate_keypair().expect("keys");
        assert_eq!(public_key_of(&private).expect("pub"), public);
        assert_eq!(B64.decode(&private).expect("b64").len(), 32);
        assert!(public_key_of("AAAA").is_err());
    }

    proptest::proptest! {
        /// Beliebiger Text darf den Parser nie zum Absturz bringen, und was
        /// er annimmt, muss sich wieder schreiben und erneut lesen lassen.
        #[test]
        fn beliebige_konfiguration_stuerzt_nicht_ab(text in "(\\[Interface\\]|\\[Peer\\]|PrivateKey|Address|Endpoint|AllowedIPs|MTU|DNS|=|,|/|:|\\.|#|[ \\n\\t]|[A-Za-z0-9+/=]{0,44}){0,60}") {
            if let Ok(profile) = parse_conf(&text, "x") {
                let written = to_conf(&profile).expect("angenommenes Profil muss sich schreiben lassen");
                proptest::prop_assert_eq!(parse_conf(&written, "x").expect("erneut lesen"), profile);
            }
        }

        #[test]
        fn beliebiger_adressbereich_stuerzt_nicht_ab(text in "\\PC{0,40}", ip in proptest::prelude::any::<u32>()) {
            if let Some(net) = IpNet::parse(&text) {
                let _ = net.contains(IpAddr::V4(ip.into()));
            }
        }
    }

    #[test]
    fn adressbereiche() {
        let net = IpNet::parse("192.168.50.0/24").expect("net");
        assert!(net.contains("192.168.50.77".parse().expect("ip")));
        assert!(!net.contains("192.168.51.1".parse().expect("ip")));
        assert!(!net.contains("fd00::1".parse().expect("ip")));
        assert!(IpNet::parse("0.0.0.0/0").expect("net").contains("8.8.8.8".parse().expect("ip")));
        assert!(IpNet::parse("::/0").expect("net").contains("2001:db8::1".parse().expect("ip")));
        assert_eq!(IpNet::parse("10.0.0.1").expect("net").prefix, 32);
        assert!(IpNet::parse("10.0.0.1/33").is_none());
        assert!(IpNet::parse("unsinn").is_none());
    }
}
