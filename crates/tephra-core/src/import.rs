//! Übernahme vorhandener Einstellungen aus einer OpenSSH-Konfiguration
//! (`~/.ssh/config`).

use crate::{Core, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tephra_model::{AuthMethod, Host, Identity, ItemId, SshKey};

/// Ein Host, wie er sich aus der Konfigurationsdatei ergibt.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SshConfigHost {
    /// Name aus der `Host`-Zeile.
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    /// Sprungziele aus `ProxyJump`, in Verbindungsreihenfolge.
    pub proxy_jump: Vec<String>,
}

/// Was beim Übernehmen geschehen ist.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub hosts: Vec<String>,
    pub identities: usize,
    pub keys: usize,
    /// Hosts, die es unter diesem Namen schon gab.
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

/// Prüft ein Muster einer `Host`-Zeile (`*` beliebig viele, `?` genau ein Zeichen).
fn pattern_matches(pattern: &str, name: &str) -> bool {
    fn matches(pattern: &[char], name: &[char]) -> bool {
        match pattern.split_first() {
            None => name.is_empty(),
            Some(('*', rest)) => (0..=name.len()).any(|skip| matches(rest, &name[skip..])),
            Some(('?', rest)) => !name.is_empty() && matches(rest, &name[1..]),
            Some((c, rest)) => name.first().is_some_and(|n| n.eq_ignore_ascii_case(c)) && matches(rest, &name[1..]),
        }
    }
    matches(&pattern.chars().collect::<Vec<_>>(), &name.chars().collect::<Vec<_>>())
}

/// Ob ein `Host`-Block für einen Namen gilt: mindestens ein Muster passt und
/// kein verneintes (`!muster`).
fn block_applies(patterns: &[String], name: &str) -> bool {
    let negated = patterns.iter().filter_map(|p| p.strip_prefix('!')).any(|p| pattern_matches(p, name));
    !negated && patterns.iter().filter(|p| !p.starts_with('!')).any(|p| pattern_matches(p, name))
}

/// Ein `Host`-Block: seine Muster und die Einstellungen darunter.
type Block = (Vec<String>, Vec<(String, String)>);

/// Liest eine OpenSSH-Konfiguration. Wie bei OpenSSH gilt je Einstellung der
/// erste passende Wert; Blöcke mit Platzhaltern (`Host *`) liefern
/// Voreinstellungen, erzeugen aber selbst keinen Host. `Match`-Blöcke und
/// `Include` werden nicht ausgewertet.
pub fn parse_ssh_config(text: &str) -> Vec<SshConfigHost> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut in_match = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((key, value)) => (key.to_ascii_lowercase(), value.trim_start_matches(['=', ' ', '\t']).trim()),
            None => continue,
        };
        let value = value.trim_matches('"');
        match key.as_str() {
            "host" => {
                in_match = false;
                blocks.push((value.split_whitespace().map(str::to_string).collect(), Vec::new()));
            }
            "match" => in_match = true,
            _ if in_match => {}
            _ => {
                if let Some((_, options)) = blocks.last_mut() {
                    options.push((key, value.to_string()));
                }
            }
        }
    }

    let mut hosts: Vec<SshConfigHost> = Vec::new();
    for (patterns, _) in &blocks {
        for alias in patterns.iter().filter(|p| !p.contains(['*', '?', '!'])) {
            if hosts.iter().any(|h| h.alias.eq_ignore_ascii_case(alias)) {
                continue;
            }
            let mut host = SshConfigHost { alias: alias.clone(), ..SshConfigHost::default() };
            for (_, options) in blocks.iter().filter(|(patterns, _)| block_applies(patterns, alias)) {
                for (key, value) in options {
                    match key.as_str() {
                        "hostname" if host.hostname.is_none() => host.hostname = Some(value.clone()),
                        "user" if host.user.is_none() => host.user = Some(value.clone()),
                        "port" if host.port.is_none() => host.port = value.parse().ok(),
                        "identityfile" if host.identity_file.is_none() => host.identity_file = Some(value.clone()),
                        "proxyjump" if host.proxy_jump.is_empty() && !value.eq_ignore_ascii_case("none") => {
                            host.proxy_jump = value.split(',').map(|s| s.trim().to_string()).collect();
                        }
                        _ => {}
                    }
                }
            }
            hosts.push(host);
        }
    }
    hosts
}

/// Löst `~` am Anfang eines Pfads auf.
fn expand_home(path: &str, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

impl Core {
    /// Legt für jeden Host der Konfiguration einen Eintrag im Tresor an,
    /// samt passender Identität und – soweit lesbar – dem privaten Schlüssel.
    /// Bereits vorhandene Hosts gleichen Namens bleiben unverändert.
    pub fn import_ssh_config(&self, text: &str, home: Option<&Path>) -> Result<ImportReport> {
        let parsed = parse_ssh_config(text);
        let mut report = ImportReport::default();
        let default_user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();

        let existing: Vec<String> =
            self.with_vault(|v| v.list::<Host>())?.into_iter().map(|(_, h)| h.label.to_lowercase()).collect();
        let mut keys: BTreeMap<String, Option<ItemId>> = BTreeMap::new();
        let mut identities: BTreeMap<(String, Option<ItemId>), ItemId> = BTreeMap::new();
        let mut created: BTreeMap<String, ItemId> = BTreeMap::new();

        for entry in &parsed {
            if existing.contains(&entry.alias.to_lowercase()) {
                report.skipped.push(entry.alias.clone());
                continue;
            }
            // Schlüssel: je Datei einmal einlesen.
            let key_id = match &entry.identity_file {
                None => None,
                Some(file) => match keys.get(file) {
                    Some(id) => *id,
                    None => {
                        let path = expand_home(file, home);
                        let id = match std::fs::read_to_string(&path) {
                            Err(_) => {
                                report
                                    .warnings
                                    .push(format!("{}: Schlüsseldatei {file} ist nicht lesbar", entry.alias));
                                None
                            }
                            Ok(pem) => match tephra_ssh::keys::import(&pem, None) {
                                Err(tephra_ssh::SshError::KeyPassphraseMissing) => {
                                    report.warnings.push(format!(
                                        "{}: Schlüssel {file} ist mit einer Passphrase geschützt – bitte gesondert einlesen",
                                        entry.alias
                                    ));
                                    None
                                }
                                Err(error) => {
                                    report.warnings.push(format!("{}: Schlüssel {file}: {error}", entry.alias));
                                    None
                                }
                                Ok((pem, info)) => {
                                    let label =
                                        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                                    let key = SshKey {
                                        label,
                                        private_key: pem.into(),
                                        public_key: info.public_key,
                                        ..SshKey::default()
                                    };
                                    report.keys += 1;
                                    Some(self.with_vault(|v| v.insert(&key))?)
                                }
                            },
                        };
                        keys.insert(file.clone(), id);
                        id
                    }
                },
            };
            // Identität: je Kombination aus Benutzer und Schlüssel einmal.
            let user = entry.user.clone().unwrap_or_else(|| default_user.clone());
            let identity_id = match identities.get(&(user.clone(), key_id)) {
                Some(id) => *id,
                None => {
                    let identity = Identity {
                        label: match key_id {
                            Some(_) => format!("{user} (Schlüssel)"),
                            None => user.clone(),
                        },
                        username: user.clone(),
                        auth: key_id.map_or(AuthMethod::Ask, |key_id| AuthMethod::Key { key_id }),
                    };
                    let id = self.with_vault(|v| v.insert(&identity))?;
                    report.identities += 1;
                    identities.insert((user, key_id), id);
                    id
                }
            };
            let host = Host {
                label: entry.alias.clone(),
                address: entry.hostname.clone().unwrap_or_else(|| entry.alias.clone()),
                port: entry.port.unwrap_or(22),
                identity_id: Some(identity_id),
                ..Host::default()
            };
            created.insert(entry.alias.to_lowercase(), self.with_vault(|v| v.insert(&host))?);
            report.hosts.push(entry.alias.clone());
        }

        // Jump-Hosts erst jetzt verknüpfen, wenn alle Einträge existieren.
        let all: BTreeMap<String, ItemId> =
            self.with_vault(|v| v.list::<Host>())?.into_iter().map(|(id, h)| (h.label.to_lowercase(), id)).collect();
        for entry in parsed.iter().filter(|e| !e.proxy_jump.is_empty()) {
            let Some(host_id) = created.get(&entry.alias.to_lowercase()) else { continue };
            let jumps: Vec<ItemId> =
                entry.proxy_jump.iter().filter_map(|j| all.get(&j.to_lowercase()).copied()).collect();
            if jumps.len() != entry.proxy_jump.len() {
                report.warnings.push(format!(
                    "{}: ProxyJump „{}“ verweist auf Ziele, die nicht als Host eingetragen sind",
                    entry.alias,
                    entry.proxy_jump.join(",")
                ));
                continue;
            }
            self.with_vault(|v| {
                let Some(mut host) = v.get::<Host>(*host_id)? else { return Ok(()) };
                host.jump_host_ids = jumps;
                v.update(*host_id, &host).map(|_| ())
            })?;
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tephra_crypto::KdfParams;

    const CONFIG: &str = r#"
# Beispiel
Host bastion
    HostName bastion.example.org
    User admin
    Port 2222
    IdentityFile ~/.ssh/id_test

Host web db
    HostName=%h.intern.example.org
    ProxyJump bastion

Host alt
    HostName 192.0.2.7
    ProxyJump jemand@unbekannt:22
    User "root"

Match host *.example.org
    User darf-nicht-zaehlen

Host *.intern !db
    User falsch

Host *
    User deploy
    Port 22
"#;

    #[test]
    fn konfiguration_wird_wie_bei_openssh_ausgewertet() {
        let hosts = parse_ssh_config(CONFIG);
        let names: Vec<&str> = hosts.iter().map(|h| h.alias.as_str()).collect();
        assert_eq!(names, vec!["bastion", "web", "db", "alt"]);

        let bastion = &hosts[0];
        assert_eq!(bastion.hostname.as_deref(), Some("bastion.example.org"));
        // Der erste passende Wert gewinnt – nicht der aus `Host *`.
        assert_eq!((bastion.user.as_deref(), bastion.port), (Some("admin"), Some(2222)));
        assert_eq!(bastion.identity_file.as_deref(), Some("~/.ssh/id_test"));

        let web = &hosts[1];
        assert_eq!(web.user.as_deref(), Some("deploy"));
        assert_eq!(web.port, Some(22));
        assert_eq!(web.proxy_jump, vec!["bastion"]);
        assert_eq!(hosts[3].user.as_deref(), Some("root"));
    }

    #[test]
    fn muster() {
        assert!(pattern_matches("*", "irgendwas"));
        assert!(pattern_matches("web-?", "web-1"));
        assert!(!pattern_matches("web-?", "web-10"));
        assert!(pattern_matches("*.Example.org", "a.example.ORG"));
        assert!(block_applies(&["*.intern".into(), "!db.intern".into()], "web.intern"));
        assert!(!block_applies(&["*.intern".into(), "!db.intern".into()], "db.intern"));
        assert!(!block_applies(&["!x".into()], "y"));
    }

    proptest::proptest! {
        #[test]
        fn beliebige_ssh_konfiguration_stuerzt_nicht_ab(text in "(Host|HostName|User|Port|ProxyJump|IdentityFile|Match|=|\\*|\\?|!|,|\"|#|[ \\n\\t]|[a-z0-9.~/-]{0,12}){0,80}") {
            for host in parse_ssh_config(&text) {
                proptest::prop_assert!(!host.alias.is_empty());
                proptest::prop_assert!(!host.alias.contains(['*', '?', '!']));
            }
        }
    }

    #[test]
    fn uebernahme_in_den_tresor() {
        let dir = tempfile::tempdir().expect("tmp");
        let (core, _) =
            Core::create(&dir.path().join("t.tephra"), "passwort", KdfParams::INSECURE_FOR_TESTS).expect("tresor");
        std::fs::create_dir_all(dir.path().join(".ssh")).expect("ordner");
        let (pem, _) = tephra_ssh::keys::generate_ed25519("test").expect("schlüssel");
        std::fs::write(dir.path().join(".ssh/id_test"), pem).expect("schreiben");

        let report = core.import_ssh_config(CONFIG, Some(dir.path())).expect("import");
        assert_eq!(report.hosts, vec!["bastion", "web", "db", "alt"]);
        assert_eq!(report.keys, 1);
        // admin+Schlüssel, deploy, root
        assert_eq!(report.identities, 3);
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        assert!(report.warnings[0].starts_with("alt:"));

        let hosts = core.with_vault(|v| v.list::<Host>()).expect("liste");
        let by_label = |label: &str| hosts.iter().find(|(_, h)| h.label == label).expect("host").clone();
        let (bastion_id, bastion) = by_label("bastion");
        assert_eq!((bastion.address.as_str(), bastion.port), ("bastion.example.org", 2222));
        assert_eq!(by_label("web").1.jump_host_ids, vec![bastion_id]);
        assert!(by_label("alt").1.jump_host_ids.is_empty());
        let resolved = core.resolve_host(bastion_id).expect("auflösen");
        assert_eq!(resolved.identity.expect("identität").username, "admin");
        assert!(resolved.key.is_some());

        // Ein zweiter Lauf legt nichts doppelt an.
        let again = core.import_ssh_config(CONFIG, Some(dir.path())).expect("import");
        assert!(again.hosts.is_empty());
        assert_eq!(again.skipped.len(), 4);
        assert_eq!(core.with_vault(|v| v.list::<Host>()).expect("liste").len(), 4);
    }
}
