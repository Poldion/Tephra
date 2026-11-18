//! Vorgabe für den Modus „Nur lesen".
//!
//! Erlaubt ist eine feste Auswahl von Programmen, jeweils nur mit Optionen,
//! die nichts verändern, nichts starten und nicht endlos laufen. Ein Befehl
//! wird dazu in Wörter zerlegt und Wort für Wort geprüft – ein grobes Muster
//! wie „`journalctl` mit beliebigen Argumenten" ließe auch
//! `journalctl --vacuum-size=1` durch.
//!
//! Grenzen, die sich so nicht ziehen lassen: „Nur lesen" umfasst alles, was
//! der angemeldete Benutzer lesen darf. Pfade, die nach Zugangsdaten
//! aussehen, werden abgelehnt; das ist eine Hürde gegen Versehen, kein
//! Schutz vor einem Modell, das gezielt sucht.

/// Was außer Optionen noch in der Befehlszeile stehen darf.
#[derive(Clone, Copy)]
enum Args {
    None,
    Any,
    Only(&'static [&'static str]),
}

struct Rule {
    program: &'static str,
    /// Erstes Wort nach dem Programm, falls das Programm Unterbefehle hat.
    subcommands: &'static [&'static str],
    /// Erlaubte einbuchstabige Optionen (auch gebündelt, z. B. `-la`).
    short: &'static str,
    /// Erlaubte lange Optionen ohne die beiden Striche; ein Wert darf mit `=` folgen.
    long: &'static [&'static str],
    args: Args,
}

const fn rule(program: &'static str, short: &'static str, long: &'static [&'static str], args: Args) -> Rule {
    Rule { program, subcommands: &[], short, long, args }
}

const fn sub(
    program: &'static str,
    subcommands: &'static [&'static str],
    short: &'static str,
    long: &'static [&'static str],
    args: Args,
) -> Rule {
    Rule { program, subcommands, short, long, args }
}

const RULES: &[Rule] = &[
    rule(
        "ls",
        "alhtrSR1dFiGgnop",
        &["all", "almost-all", "human-readable", "color", "directory", "recursive", "reverse"],
        Args::Any,
    ),
    rule("pwd", "", &[], Args::None),
    rule("whoami", "", &[], Args::None),
    rule("id", "ugGnr", &[], Args::Any),
    rule("hostname", "fiIsdA", &[], Args::None),
    rule("uname", "asnrvmpio", &[], Args::None),
    rule("uptime", "ps", &[], Args::None),
    rule("date", "uR", &["utc", "iso-8601", "rfc-3339"], Args::None),
    rule("nproc", "", &["all"], Args::None),
    rule("lscpu", "", &[], Args::None),
    rule("w", "", &[], Args::None),
    rule("who", "abHqu", &[], Args::None),
    rule("last", "n0123456789aFiw", &[], Args::Any),
    rule("df", "hHTkmilaP", &["human-readable", "total", "inodes"], Args::Any),
    rule("du", "shkmcadx0123456789", &["max-depth", "summarize", "human-readable", "total"], Args::Any),
    rule("free", "hbkmgtw", &["human", "total"], Args::None),
    rule("cat", "nAbsETv", &[], Args::Any),
    rule("head", "ncqv0123456789", &["lines", "bytes"], Args::Any),
    rule("tail", "ncqv0123456789", &["lines", "bytes"], Args::Any),
    rule("wc", "lwcmL", &[], Args::Any),
    rule("stat", "Lft", &[], Args::Any),
    rule("file", "biLkz", &[], Args::Any),
    rule("readlink", "fenm", &[], Args::Any),
    rule("realpath", "esm", &[], Args::Any),
    rule("tree", "aLdfhDpug0123456789", &[], Args::Any),
    rule("sha256sum", "b", &[], Args::Any),
    rule("md5sum", "b", &[], Args::Any),
    rule("which", "a", &[], Args::Any),
    rule("echo", "neE", &[], Args::Any),
    rule("getent", "", &[], Args::Any),
    rule(
        "grep",
        "inrvclEFwxHhoABCRsqm0123456789",
        &[
            "ignore-case",
            "recursive",
            "count",
            "line-number",
            "invert-match",
            "files-with-matches",
            "max-count",
            "context",
            "after-context",
            "before-context",
        ],
        Args::Any,
    ),
    rule(
        "ps",
        "efaAuxlFwHLT",
        &["forest", "sort", "no-headers"],
        Args::Only(&["aux", "ax", "axu", "auxf", "auxw", "auxww", "axjf"]),
    ),
    rule("top", "bn0123456789", &[], Args::None),
    rule("lsblk", "fabmlpS", &["fs", "all", "bytes", "list", "paths"], Args::Any),
    rule("findmnt", "lnoDAT", &[], Args::Any),
    rule("mount", "", &[], Args::None),
    rule("dmesg", "THkux", &["human", "kernel", "ctime", "nopager", "level", "facility"], Args::Any),
    rule(
        "ss",
        "tulnpa46Heimsxwr",
        &["tcp", "udp", "listening", "all", "numeric", "processes", "summary", "ipv4", "ipv6", "no-header"],
        Args::None,
    ),
    rule(
        "netstat",
        "tulnpa46rsicev",
        &["tcp", "udp", "listening", "all", "numeric", "programs", "route", "interfaces", "statistics"],
        Args::None,
    ),
    rule("nginx", "tTvVq", &[], Args::None),
    rule("dpkg", "lLsS", &[], Args::Any),
    rule("rpm", "qalf", &[], Args::Any),
    rule("crontab", "l", &[], Args::None),
    rule("timedatectl", "", &["no-pager"], Args::Only(&["status", "timesync-status"])),
    rule("hostnamectl", "", &["no-pager"], Args::Only(&["status"])),
    rule(
        "journalctl",
        "unrekbxpqo0123456789",
        &[
            "unit",
            "lines",
            "no-pager",
            "since",
            "until",
            "priority",
            "boot",
            "dmesg",
            "reverse",
            "output",
            "grep",
            "utc",
            "no-hostname",
            "catalog",
            "quiet",
        ],
        Args::Any,
    ),
    sub(
        "systemctl",
        &[
            "status",
            "is-active",
            "is-enabled",
            "is-failed",
            "list-units",
            "list-unit-files",
            "list-timers",
            "list-sockets",
        ],
        "lna",
        &["no-pager", "full", "all", "type", "state", "lines", "failed", "plain", "no-legend"],
        Args::Any,
    ),
    sub(
        "docker",
        &["ps"],
        "aqlns",
        &["all", "quiet", "no-trunc", "latest", "last", "size", "filter", "format"],
        Args::Any,
    ),
    sub("docker", &["logs"], "tn0123456789", &["tail", "since", "until", "timestamps", "details"], Args::Any),
    sub("docker", &["images"], "aq", &["all", "quiet", "no-trunc", "digests"], Args::Any),
    sub("apt", &["list", "policy", "show", "search"], "a", &["installed", "upgradable", "all-versions"], Args::Any),
];

/// Optionen von `ip`, die nur die Darstellung betreffen.
const IP_OPTIONS: &[&str] = &[
    "-4", "-6", "-br", "-brief", "-o", "-oneline", "-c", "-color", "-s", "-stats", "-d", "-details", "-j", "-json",
    "-p", "-pretty",
];
const IP_OBJECTS: &[&str] = &["a", "addr", "address", "r", "route", "l", "link", "n", "neigh", "neighbor", "rule"];
const IP_SHOW: &[&str] = &["show", "list", "ls", "sh", "lst", "s"];

/// Teile von Pfaden und Namen, hinter denen üblicherweise Zugangsdaten stehen.
const SENSITIVE: &[&str] = &[
    ".ssh",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
    "authorized_keys",
    "shadow",
    "gshadow",
    "sudoers",
    "environ",
    ".env",
    "private",
    "secret",
    "credential",
    "passw",
    "token",
    ".pem",
    ".key",
    ".p12",
    ".pfx",
    ".kube",
    ".aws",
    ".gnupg",
    ".docker/config",
    ".netrc",
    ".pgpass",
    ".my.cnf",
    "htpasswd",
    "wallet",
    "keyring",
    "/proc/",
];

/// Prüft Optionen und Argumente nach einer Regel.
fn words_allowed(words: &[&str], rule: &Rule) -> bool {
    words.iter().all(|word| {
        if let Some(name) = word.strip_prefix("--") {
            // Lange Optionen müssen ausgeschrieben sein: Abkürzungen wie
            // `--rot` für `--rotate` kämen sonst an der Liste vorbei.
            let name = name.split('=').next().unwrap_or_default();
            !name.is_empty() && rule.long.contains(&name)
        } else if let Some(cluster) = word.strip_prefix('-') {
            !cluster.is_empty() && cluster.chars().all(|c| rule.short.contains(c))
        } else {
            match rule.args {
                Args::None => false,
                Args::Any => true,
                Args::Only(allowed) => allowed.contains(word),
            }
        }
    })
}

fn ip_allowed(words: &[&str]) -> bool {
    let options = words.iter().take_while(|w| w.starts_with('-')).count();
    if !words[..options].iter().all(|w| IP_OPTIONS.contains(w)) {
        return false;
    }
    let rest = &words[options..];
    let Some((object, rest)) = rest.split_first() else { return false };
    if !IP_OBJECTS.contains(object) {
        return false;
    }
    match rest.split_first() {
        None => true,
        // Nach `show` folgen nur noch Filter (Schnittstelle, Tabelle …), keine Optionen.
        Some((verb, filters)) => IP_SHOW.contains(verb) && filters.iter().all(|w| !w.starts_with('-')),
    }
}

/// Ob ein Befehl nach der Vorgabe für „Nur lesen" erlaubt ist. Gibt bei
/// Ablehnung den Grund zurück. Shell-Operatoren müssen vorher ausgeschlossen
/// worden sein.
pub(crate) fn check(command: &str) -> Result<(), &'static str> {
    // Anführungszeichen und Platzhalter würden die Shell Wörter bilden
    // lassen, die hier anders aussehen als bei der Ausführung.
    if command.contains(['\'', '"', '*', '?', '[', ']', '~']) {
        return Err("Anführungszeichen, Platzhalter und ~ sind im Modus „Nur lesen“ nicht erlaubt");
    }
    let lowered = command.to_lowercase();
    if SENSITIVE.iter().any(|part| lowered.contains(part)) {
        return Err("Der Befehl greift auf einen Pfad zu, hinter dem üblicherweise Zugangsdaten liegen");
    }
    let words: Vec<&str> = command.split_whitespace().collect();
    let Some((program, rest)) = words.split_first() else { return Err("Leerer Befehl") };
    if *program == "ip" {
        return if ip_allowed(rest) { Ok(()) } else { Err("Von `ip` sind nur anzeigende Aufrufe erlaubt") };
    }
    let mut known = false;
    for rule in RULES.iter().filter(|rule| rule.program == *program) {
        known = true;
        let allowed = if rule.subcommands.is_empty() {
            words_allowed(rest, rule)
        } else {
            // Der Unterbefehl ist das erste Wort, das keine Option ist.
            match rest.iter().position(|w| !w.starts_with('-')) {
                Some(index) if rule.subcommands.contains(&rest[index]) => {
                    words_allowed(&rest[..index], rule) && words_allowed(&rest[index + 1..], rule)
                }
                _ => false,
            }
        };
        if allowed {
            return Ok(());
        }
    }
    if known {
        Err("Diese Optionen oder Argumente sind im Modus „Nur lesen“ nicht erlaubt")
    } else {
        Err("Das Programm steht nicht auf der Erlaubnisliste für „Nur lesen“")
    }
}

/// Namen aller Programme der Vorgabe, für Anzeigen und Dokumentation.
pub fn programs() -> Vec<&'static str> {
    let mut names: Vec<&str> = RULES.iter().map(|rule| rule.program).chain(["ip"]).collect();
    names.sort_unstable();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uebliche_abfragen_sind_erlaubt() {
        for command in [
            "ls",
            "ls -la /var/log",
            "df -h",
            "du -sh /var/lib --max-depth=1",
            "free -m",
            "uptime",
            "cat /etc/os-release",
            "head -n 20 /var/log/syslog",
            "tail -n50 /var/log/nginx/error.log",
            "grep -ri error /var/log/nginx",
            "ps aux",
            "ps -ef",
            "top -bn1",
            "systemctl status nginx",
            "systemctl --no-pager status nginx.service",
            "systemctl list-units --type=service --state=failed",
            "journalctl -u nginx -n 100 --no-pager",
            "journalctl --since=-1h --priority=err",
            "docker ps -a",
            "docker logs --tail 50 web",
            "ip a",
            "ip -br addr show dev eth0",
            "ip route",
            "ss -tulpn",
            "nginx -t",
            "dpkg -l",
            "apt list --upgradable",
            "echo hallo",
        ] {
            assert_eq!(check(command), Ok(()), "{command}");
        }
    }

    #[test]
    fn veraendernde_startende_und_endlose_aufrufe_sind_gesperrt() {
        for command in [
            // nicht auf der Liste
            "rm -rf /tmp/x",
            "sudo ls",
            "sh",
            "bash -c ls",
            "env",
            "printenv",
            "find / -delete",
            "sed -i s/a/b/ f",
            "awk 1 f",
            "less /etc/hosts",
            "vi /etc/hosts",
            "curl http://example.org",
            "systemctl restart nginx",
            "docker inspect web",
            "docker exec web sh",
            "docker rm web",
            "apt install x",
            "sysctl -w vm.swappiness=1",
            "kill 1",
            // Optionen, die schreiben
            "journalctl --vacuum-size=1M",
            "journalctl --rotate",
            "journalctl --rot",
            "journalctl --flu",
            "journalctl --cursor-file=/tmp/x",
            "journalctl -D /tmp",
            "ss -K dst 192.0.2.1",
            "ss -aK",
            "ss -D /tmp/x",
            "file -C -m /tmp/x",
            "date -s 2000-01-01",
            "hostname gekapert",
            "dmesg -c",
            "dmesg -C",
            "crontab -r",
            "crontab /tmp/x",
            "nginx -s stop",
            "rpm -e bash",
            "rpm -i x.rpm",
            "dpkg -i x.deb",
            "mount /dev/sda1 /mnt",
            "ip addr add 192.0.2.9/24 dev eth0",
            "ip route del default",
            "ip link set eth0 down",
            "ip -batch /tmp/x",
            "ip netns exec x sh",
            "ip addr show -batch",
            // startet etwas auf einem anderen Rechner
            "systemctl status nginx -H root@192.0.2.5",
            "systemctl -M container status x",
            // läuft endlos
            "tail -f /var/log/syslog",
            "tail --follow /var/log/syslog",
            "journalctl -f",
            "journalctl --follow",
            "docker logs -f web",
            "ps eww",
            // Wortgrenzen, die die Shell anders sähe
            "cat '/etc/hosts'",
            "grep \"-r\" x /",
            "ls /var/log/*.log",
            "cat ~/notizen",
            "ls --",
            "ls -",
        ] {
            assert!(check(command).is_err(), "durchgelassen: {command}");
        }
    }

    #[test]
    fn pfade_mit_zugangsdaten_sind_gesperrt() {
        for command in [
            "cat /home/anna/.ssh/id_ed25519",
            "cat /root/.ssh/authorized_keys",
            "cat /etc/shadow",
            "cat /proc/1/environ",
            "cat /srv/app/.env",
            "head /etc/ssl/private/server.key",
            "grep -r password /etc",
            "cat /home/anna/.aws/credentials",
            "ls /root/.gnupg",
            "cat /var/lib/app/API_TOKEN",
        ] {
            assert!(check(command).is_err(), "durchgelassen: {command}");
        }
    }

    #[test]
    fn programmliste() {
        let names = programs();
        assert!(names.contains(&"ls") && names.contains(&"ip") && names.contains(&"journalctl"));
        assert!(!names.contains(&"rm") && !names.contains(&"sudo"));
    }
}
