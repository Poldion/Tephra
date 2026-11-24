# Handbuch

Wie die Kommandozeile `tephra` benutzt und der Sync-Server `tephra-server` betrieben wird. Englische Fassung:
[manual.md](manual.md).

Die grafische App gibt es bisher für Windows und Android und in einer ersten Fassung; dieses Handbuch beschreibt die
Kommandozeile. Unter Windows verwenden beide denselben Tresor.

- [Installation](#installation)
- [Der Tresor](#der-tresor)
- [SSH-Schlüssel](#ssh-schlüssel)
- [Identitäten](#identitäten)
- [Hosts](#hosts)
- [Übernahme aus OpenSSH](#übernahme-aus-openssh)
- [Verbinden](#verbinden)
- [Portweiterleitungen](#portweiterleitungen)
- [Bekannte Hostschlüssel](#bekannte-hostschlüssel)
- [VPN-Profile](#vpn-profile)
- [Snippets](#snippets)
- [Abgleich](#abgleich)
- [MCP: Zugriff für Sprachmodelle](#mcp-zugriff-für-sprachmodelle)
- [Sync-Server betreiben](#sync-server-betreiben)
- [Skripte und Umgebungsvariablen](#skripte-und-umgebungsvariablen)

## Installation

Fertige Programme liegen auf der [Releases-Seite](https://github.com/Poldion/Tephra/releases). Unter Windows kann der
Installer (`…-setup.exe`) `tephra` in den `PATH` aufnehmen; unter Ubuntu und Debian legt das Paket
(`tephra_…_amd64.deb`) es nach `/usr/bin`. Aus den Archiven (`…-windows-x64.zip`, `…-linux-x64.tar.gz`) kopierst du
`tephra` selbst in einen Ordner, der im `PATH` steht. Oder aus dem Quelltext bauen, mit Rust 1.90 oder neuer und einem
C-Compiler:

```bash
git clone https://github.com/Poldion/Tephra.git
cd Tephra
cargo build --release
```

Das ergibt `target/release/tephra` und `target/release/tephra-server` (unter Windows `tephra.exe` und
`tephra-server.exe`). Kopiere `tephra` in einen Ordner, der im `PATH` steht. `tephra --help` und `tephra <befehl>
--help` zeigen alle Befehle und Optionen.

## Der Tresor

Alles, was `tephra` weiß, liegt in einer verschlüsselten Datei, dem Tresor.

```bash
tephra init
```

fragt nach einem neuen Master-Passwort (mindestens 8 Zeichen, zweimal einzugeben), legt den Tresor an und zeigt den
**Wiederherstellungsschlüssel**:

```
Wiederherstellungsschlüssel – bitte jetzt aufschreiben und sicher verwahren:

    7K2M-9QXA-…
```

Schreib ihn jetzt auf. Er wird nirgends gespeichert und nie wieder angezeigt. Ohne Master-Passwort und
Wiederherstellungsschlüssel sind die Daten verloren.

| Befehl | Was er tut |
|---|---|
| `tephra init` | Legt einen neuen Tresor an |
| `tephra passwd` | Ändert das Master-Passwort |
| `tephra recover` | Setzt mit dem Wiederherstellungsschlüssel ein neues Master-Passwort |
| `tephra export <datei>` | Schreibt eine verschlüsselte Sicherung des Tresors |

**Wo der Tresor liegt.** Ohne Angabe als `vault.tephra` im Datenordner deines Benutzers: unter Windows
`%APPDATA%\Tephra\data`, unter Linux `~/.local/share/tephra`. Eine andere Datei wählst du mit
`--vault <pfad>` oder der Umgebungsvariable `TEPHRA_VAULT`; das geht bei jedem Befehl:

```bash
tephra --vault ~/arbeit.tephra host list
```

**Entsperren.** Jeder Befehl fragt nach dem Master-Passwort und sperrt den Tresor wieder, wenn er endet. Nur
`tephra daemon` hält ihn entsperrt.

**Sicherung.** `tephra export sicherung.tephra` schreibt eine Kopie, die weiterhin mit demselben Master-Passwort und
Wiederherstellungsschlüssel verschlüsselt ist. Sie enthält Hosts, Identitäten, Schlüssel, VPN-Profile, Snippets und
bekannte Hostschlüssel, aber nichts, das zu einem einzelnen Gerät gehört: keine Schlüssel mit `--local-only`, keine
Sitzung beim Sync-Server, keine MCP-Clients oder -Freigaben, kein Protokoll. Zum Wiederherstellen wird die Datei als
Tresor benutzt: an den Standardort kopieren oder mit `--vault` angeben.

**Wiederherstellen.** `tephra recover` fragt nach dem Wiederherstellungsschlüssel (Groß- und Kleinschreibung,
Bindestriche und Leerzeichen sind egal) und dann nach einem neuen Master-Passwort. Der Wiederherstellungsschlüssel
bleibt danach gültig.

## SSH-Schlüssel

```bash
tephra key generate laptop                       # neues Ed25519-Schlüsselpaar, gibt den öffentlichen Schlüssel aus
tephra key import alt ~/.ssh/id_ed25519          # vorhandener privater Schlüssel; fragt nach der Passphrase, falls er eine hat
tephra key list
tephra key show laptop                           # öffentlicher Schlüssel für authorized_keys auf dem Server
tephra key install laptop web                    # trägt den öffentlichen Schlüssel auf dem Host „web“ in ~/.ssh/authorized_keys ein
tephra key rm alt
```

Eingelesene Schlüssel dürfen im OpenSSH-, PKCS#8- oder PuTTY-Format vorliegen oder im alten PEM-Format von OpenSSL
(`RSA PRIVATE KEY`, `EC PRIVATE KEY`), auch wenn dieses mit einer Passphrase per AES oder Triple DES geschützt ist.
`tephra` fragt einmal nach der Passphrase und legt den Schlüssel im OpenSSH-Format ohne sie ab – der Tresor
verschlüsselt ihn –, du wirst also nicht erneut gefragt. Schlägt das Einlesen fehl, sagt die Meldung, ob die Passphrase
fehlt oder nicht stimmt oder welches Format nicht unterstützt wird.

`tephra key install` meldet sich mit der bisherigen Identität am Host an und hängt den öffentlichen Schlüssel dort an
`~/.ssh/authorized_keys` des Benutzers an; vorhandene Einträge bleiben unberührt, und ein Schlüssel, der schon dort
steht, wird nicht doppelt eingetragen. Danach kannst du dem Host eine Identität mit diesem Schlüssel zuweisen.

`--local-only` bei `generate` oder `import` behält einen Schlüssel auf diesem Gerät: Er geht nie an den Sync-Server
und erreicht deine anderen Geräte nicht.

```bash
tephra key generate arbeitsplatz --local-only
```

## Identitäten

Eine Identität ist ein Benutzername samt Anmeldeverfahren. Mehrere Hosts können sich eine teilen.

```bash
tephra identity add admin --user admin --key laptop      # Anmeldung mit dem SSH-Schlüssel „laptop“
tephra identity add deploy --user deploy                 # fragt nach einem Passwort und speichert es im Tresor
tephra identity add root --user root --ask               # speichert kein Passwort, fragt bei jeder Verbindung
tephra identity list
tephra identity rm deploy
```

## Hosts

```bash
tephra host add web 192.0.2.10 --identity admin
tephra host add db db.example.org --port 2222 --identity admin --tag prod --tag datenbank
tephra host list
tephra host rm db
```

| Option | Bedeutung |
|---|---|
| `--port <port>` | SSH-Port, Standard 22 |
| `--identity <name>` | Identität für die Anmeldung. Ohne sie ist keine Anmeldung möglich |
| `--vpn <name>` | VPN-Profil, durch das verbunden wird |
| `--jump <name>` | Jump-Host; mehrfach möglich, in Verbindungsreihenfolge |
| `--tag <tag>` | Tag; mehrfach möglich |

`tephra host list` zeigt Bezeichnung, Benutzer, Adresse und Port und kennzeichnet Hosts mit VPN-Profil (`[VPN]`) oder
Jump-Hosts (`[Jump-Host]`).

Angesprochen wird alles über seine Bezeichnung, ohne Rücksicht auf Groß- und Kleinschreibung. Bezeichnungen müssen
innerhalb ihrer Art eindeutig sein. Einen Befehl zum Ändern eines Eintrags gibt es noch nicht: löschen und neu
anlegen. Ein neu angelegter Host ist ein neuer Host – MCP-Freigaben für den alten gelten nicht mehr.

**Jump-Hosts.** Ein Host, der nur über einen anderen erreichbar ist:

```bash
tephra host add bastion bastion.example.org --identity admin
tephra host add innen 10.0.0.7 --identity admin --jump bastion
tephra ssh innen                                 # verbindet erst zu bastion, dann hindurch zu innen
```

Ein Jump-Host kann eigene Jump-Hosts haben. Jeder Rechner der Kette meldet sich mit seiner eigenen Identität an, und
sein Hostschlüssel wird geprüft.

## Übernahme aus OpenSSH

```bash
tephra import-ssh-config                         # liest ~/.ssh/config
tephra import-ssh-config ./andere-config
```

Für jeden `Host`-Eintrag ohne Platzhalter legt `tephra` einen Host mit passender Identität an:

| In der Konfiguration | Wird zu |
|---|---|
| `Host web` | Host mit der Bezeichnung `web` |
| `HostName`, `Port` | Adresse und Port; ohne `HostName` ist die Bezeichnung die Adresse |
| `User` | Benutzername der Identität; ohne `User` der Name, mit dem du angemeldet bist |
| `IdentityFile` | Der private Schlüssel wird in den Tresor eingelesen und von der Identität benutzt |
| `ProxyJump` | Jump-Hosts, sofern sie als Einträge derselben Datei oder als bereits vorhandene Hosts im Tresor benannt sind; sonst meldet `tephra` es und setzt keine |

- Blöcke mit Platzhaltern (`Host *`, `Host *.example.org`) liefern wie bei OpenSSH Voreinstellungen für die Einträge,
  auf die sie passen, erzeugen aber selbst keine Hosts. `Match`-Blöcke und `Include` werden nicht ausgewertet; alle
  übrigen Einstellungen werden ignoriert.
- Ein Schlüssel mit Passphrase wird hier nicht übernommen. `tephra` meldet das; lies ihn mit `tephra key import` ein und
  lege die Identität selbst an. Bis dahin fragt die Identität eines solchen Hosts nach einem Passwort.
- Übernommene Schlüssel sind gewöhnliche Schlüssel im Tresor und werden abgeglichen. Die Dateien in `~/.ssh` bleiben
  unberührt.
- Hosts, deren Bezeichnung es schon gibt, bleiben unverändert und werden aufgelistet. Die Übernahme zweimal
  auszuführen schadet nicht.

## Verbinden

```bash
tephra ssh web                                   # interaktive Sitzung
tephra exec web -- df -h                         # ein Befehl; Ausgabe und Exit-Code werden durchgereicht
```

Setze `--` vor den Befehl, damit dessen Optionen nicht für Optionen von `tephra` gehalten werden. `tephra exec` gibt den
Exit-Code des entfernten Befehls zurück, `tephra ssh` den der Shell.

**Erste Verbindung.** Ist der Host noch nicht bekannt, zeigt `tephra` den Fingerabdruck seines Schlüssels und fragt:

```
Der Host 192.0.2.10:22 ist noch nicht bekannt.
  ssh-ed25519 SHA256:…
Diesem Schlüssel vertrauen? [j/N]
```

Vergleiche den Fingerabdruck mit dem auf dem Server (`ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub`) und antworte
mit `j`. Der Schlüssel wird im Tresor gespeichert und mit deinen anderen Geräten abgeglichen.

**Geänderter Schlüssel.** Legt ein bekannter Host einen anderen Schlüssel vor, warnt `tephra`, dass das ein Angriff sein
kann, und fährt erst fort, wenn du das Wort `geändert` eintippst. Tu das nur, wenn du weißt, dass der Schlüssel des
Servers erneuert wurde.

**Einmalcodes.** Stellt der Server bei der Anmeldung Rückfragen (keyboard-interactive, etwa nach einem Einmalcode),
erscheinen sie im Terminal.

Noch nicht vorhanden: Anmeldung über einen SSH-Agenten und Dateiübertragung von der Kommandozeile.

## Portweiterleitungen

`tephra forward` verbindet sich mit einem Host und leitet Ports über ihn weiter, bis du es mit Strg+C beendest.

```bash
tephra forward db -L 5432:localhost:5432         # lokaler Port 5432 → Port 5432 auf dem Host selbst
tephra forward bastion -L 8080:10.0.0.7:80       # lokaler Port 8080 → ein Webserver hinter dem Host
tephra forward bastion -D 1080                   # SOCKS5-Proxy auf Port 1080; Ziele werden vom Host aus erreicht
tephra forward bastion -L 8080:10.0.0.7:80 -L 8443:10.0.0.7:443 -D 1080
```

| Option | Bedeutung |
|---|---|
| `-L PORT:ZIEL:ZIELPORT` | Lokaler Port zu einem Ziel aus Sicht des Hosts. Mehrfach möglich |
| `-D PORT` | Lokaler SOCKS5-Proxy auf diesem Port. Mehrfach möglich |
| `--bind <adresse>` | Adresse, auf der gelauscht wird, Standard `127.0.0.1` |

- Die Verbindung wird aufgebaut wie jede andere: durch das VPN-Profil und die Jump-Hosts des Hosts.
- Bei `-D` löst der Host die Namen auf, nicht dein Rechner. Der Proxy beherrscht `CONNECT` ohne Anmeldung.
- **Jeder an deinem Rechner kann die Weiterleitung benutzen,** solange sie läuft. Mit `--bind 0.0.0.0` oder einer
  anderen Adresse deines Rechners kann es auch jeder in deinem lokalen Netz – tu das nur mit Absicht.
- Entfernte Weiterleitungen (ein Port auf dem Host, der zu dir zurückführt) gibt es nicht.

## Bekannte Hostschlüssel

```bash
tephra known-hosts list
tephra known-hosts forget 192.0.2.10             # bei der nächsten Verbindung wird neu gefragt
```

Schlüssel von Hosts, die über ein VPN-Profil erreicht werden, werden getrennt geführt (gekennzeichnet mit
`(über VPN)`), weil dieselbe private Adresse in verschiedenen Netzen zu verschiedenen Rechnern gehören kann.

## VPN-Profile

Ein VPN-Profil beschreibt einen Tunnel über das WireGuard-Protokoll. Der Tunnel läuft in `tephra` selbst: Er legt keinen
Netzwerkadapter an, braucht keine Administratorrechte und trägt nur die Verbindungen von `tephra`. Andere Programme
sehen ihn nicht.

```bash
tephra vpn import buero buero.conf               # Konfiguration im wg-quick-Format
tephra vpn list
tephra vpn test buero                            # baut den Tunnel auf und meldet, ob der Handshake gelingt
tephra vpn export buero                          # gibt das Profil im wg-quick-Format aus, samt privatem Schlüssel
tephra vpn keygen                                # gibt ein neues Schlüsselpaar aus, speichert nichts
tephra vpn rm buero
```

Aus der Konfigurationsdatei liest `tephra` in `[Interface]` die Einträge `PrivateKey`, `Address`, `DNS` und `MTU`, in
jedem `[Peer]` die Einträge `PublicKey`, `PresharedKey`, `Endpoint`, `AllowedIPs` und `PersistentKeepalive`. Einträge,
die nur für ein systemweites VPN Sinn ergeben (`PostUp`, `Table`, `FwMark` …), werden ignoriert; `tephra` führt nie
Befehle aus einem Profil aus.

Das Profil einem Host zuweisen:

```bash
tephra host add nas 10.8.0.5 --identity admin --vpn buero
tephra ssh nas                                   # startet den Tunnel und verbindet hindurch
```

- Der Tunnel startet mit der ersten Verbindung, die ihn braucht. Mehrere Profile können gleichzeitig in Gebrauch
  sein.
- Ist der Host mit Namen angegeben, wird der Name durch den Tunnel von den DNS-Servern des Profils aufgelöst. Steht im
  Profil kein `DNS`, nimm eine Adresse.
- Die Adresse des Hosts muss in den `AllowedIPs` einer Gegenstelle liegen.
- Eine neue Gegenstelle einrichten: `tephra vpn keygen`, den öffentlichen Schlüssel auf der anderen Seite eintragen, den
  privaten in eine Konfigurationsdatei schreiben und diese einlesen.

## Snippets

Ein Snippet ist ein gespeicherter Befehl. Platzhalter werden als `{{name}}` geschrieben.

```bash
tephra snippet add neustart "systemctl restart {{dienst}}"
tephra snippet list
tephra snippet rm neustart
```

Snippets werden abgeglichen und lassen sich von Sprachmodellen über MCP ausführen (Werkzeug `run_snippet`); die Werte
der Platzhalter werden dabei als wörtliche Shell-Argumente eingesetzt. Ein Snippet von der Kommandozeile auszuführen
geht noch nicht.

## Abgleich

Der Abgleich ist freiwillig. Der Tresor funktioniert ohne ihn; mit ihm haben alle deine Geräte dieselben Hosts,
Identitäten, Schlüssel, VPN-Profile, Snippets und bekannten Hostschlüssel. Du brauchst einen Sync-Server – siehe
[Sync-Server betreiben](#sync-server-betreiben).

**Erstes Gerät.** Den Tresor gibt es schon; lege für ihn ein Konto auf dem Server an:

```bash
tephra sync register https://sync.example.org alice --invite 4f7c…
```

Der Benutzername hat 3 bis 64 Zeichen (Buchstaben, Ziffern und `. - _ @ +`). Den Einladungscode bekommst du von dem,
der den Server betreibt; bei offener Registrierung lässt du `--invite` weg. Das Master-Passwort des Tresors wird zum
Passwort des Kontos. Alles im Tresor wird verschlüsselt hochgeladen.

**Weitere Geräte.** Auf einem Gerät ohne Tresor:

```bash
tephra sync join https://sync.example.org alice
```

fragt nach dem Master-Passwort des Kontos, legt den Tresor an und holt alles ab.

| Befehl | Was er tut |
|---|---|
| `tephra sync now` | Gleicht einmal ab: abholen, zusammenführen, hochladen |
| `tephra sync status` | Zeigt Server und Benutzername, mit denen dieses Gerät verbunden ist |
| `tephra sync devices` | Listet die Geräte des Kontos mit ihren Kennungen |
| `tephra sync remove-device <kennung>` | Entfernt ein Gerät vom Konto |
| `tephra sync reconnect` | Meldet dieses Gerät neu an (abgelaufene Sitzung, geändertes Passwort) |
| `tephra sync disconnect` | Trennt dieses Gerät vom Server; der lokale Tresor bleibt |

`tephra sync now` gleicht einmal ab. Für fortlaufenden Abgleich lässt du `tephra daemon` laufen: Er lädt lokale
Änderungen binnen Sekunden hoch und erfährt vom Server, wenn ein anderes Gerät etwas ändert.

**In der App.** Das Wolkensymbol öffnet dasselbe. Auf dem ersten Gerät trägst du Serveradresse, Benutzername,
Einladungscode und das Master-Passwort des Tresors ein und wählst „Konto anlegen“. Ein weiteres Gerät darf noch keinen
Tresor haben: Wähle dort beim ersten Start „Vorhandenen Tresor abholen“ und melde dich mit Serveradresse, Benutzername
und Master-Passwort an. Solange der Tresor offen ist, gleicht die App fortlaufend ab wie `tephra daemon`; ihr Dialog
zeigt die Geräte des Kontos, entfernt eines, meldet dieses Gerät neu an und trennt es. Ein Telefon, das schon einen
eigenen Tresor hat, kann keinem Konto beitreten – lösche zuerst die Daten der App (oder installiere sie neu) und hole
den Tresor dann ab. Unter Android vertraut die App den Zertifikaten des Systems und denen, die du selbst installiert
hast; ein Server hinter einer eigenen Zertifizierungsstelle funktioniert also, sobald deren Wurzelzertifikat auf dem
Telefon installiert ist.

Gut zu wissen:

- **Adressen beginnen mit `https://`.** Einfaches `http://` wird nur für `localhost` angenommen – und für einen Server
  im eigenen Netz, wenn du es ausdrücklich erlaubst: mit `--allow-http` bei `tephra sync register` und
  `tephra sync join`, in der App mit dem Haken „Unverschlüsselt verbinden“, der unter einer solchen Adresse erscheint.
  Die Erlaubnis gilt für diese Verbindung, bis du sie trennst. Was du damit aufgibst, steht unter
  „Sync-Server betreiben“.
- **Konflikte.** Ändern zwei Geräte denselben Eintrag, werden die Änderungen feldweise zusammengeführt; beim selben
  Feld gewinnt die spätere. Eine Löschung gewinnt gegen eine gleichzeitige Änderung.
- **Nicht abgeglichen** werden Schlüssel mit `--local-only`, MCP-Clients, Freigaben und Protokoll sowie die Sitzung
  beim Sync-Server selbst.
- **Das Master-Passwort ändern** mit `tephra passwd` ändert es auch für das Konto. Andere Geräte werden abgemeldet und
  brauchen `tephra sync reconnect` mit dem neuen Passwort.
- **Nach `tephra recover`** kennt der Server weiterhin nur das alte Passwort, und ohne dieses lässt es sich dort nicht
  ändern. Dieses Gerät gleicht weiter ab, solange seine Sitzung gilt, anmelden kann sich aber kein Gerät mehr. Der Weg
  zurück: `tephra sync disconnect`, dann mit `tephra sync register` ein neues Konto anlegen.
- **Datensätze, die der Server als zu groß ablehnen würde,** werden übersprungen und von `tephra sync now` gemeldet;
  alles andere wird abgeglichen. Ein Datensatz, der mehr als 24 Stunden in der Zukunft datiert ist, wird vom Server
  nicht übernommen – prüfe die Uhren deiner Geräte, wenn Datensätze als unlesbar gemeldet werden.
- **Ein Gerät, das 60 Tage nicht abgeglichen hat,** wird abgemeldet; `tephra sync reconnect` meldet es neu an.
- **Ein Gerät entfernen** beendet seinen Abgleich sofort. Was es bereits abgeholt hat, bleibt dort lesbar – ging das
  Gerät verloren, erneuere die Passwörter und Schlüssel deiner Hosts.
- **Gerätenamen** stammen aus der Umgebungsvariable `COMPUTERNAME` oder `HOSTNAME`. Ist keine gesetzt, heißt das Gerät
  `Kommandozeile`.
- **Grenzen des Servers:** 1 MiB je Datensatz, ohne andere Einstellung 50.000 Datensätze und 256 MiB je Konto.

## MCP: Zugriff für Sprachmodelle

Über das Model Context Protocol kann ein Sprachmodell Befehle auf deinen Hosts ausführen – auf den Hosts, die du ihm
freigibst, in den Grenzen, die du setzt. Lies vorher den Abschnitt über Sprachmodelle im
[Bedrohungsmodell](bedrohungsmodell.md).

So greifen die Teile ineinander:

```
MCP-Client (startet) ──► tephra mcp serve ──lokaler Kanal──► tephra daemon ──SSH──► Host
                         dünne Brücke,                   Tresor entsperrt,
                         kennt nur ihr Token             prüft jeden Aufruf, fragt dich
```

### Schritt für Schritt

**1. Client anlegen.** Jedes Programm, das sich verbindet, bekommt einen eigenen Namen und ein eigenes Token.

```bash
tephra mcp add-client desktop
```

Das Token (`tephramcp_…`) wird ein einziges Mal angezeigt. Der Tresor behält nur einen Hash davon.

**2. Hosts freigeben.** Zunächst ist nichts freigegeben.

```bash
tephra mcp grant desktop web --level read-only
tephra mcp grant desktop db                              # Stufe confirm ist der Standard
tephra mcp grant desktop build --level confirm --allow "docker ps*" --files-read
```

**3. Den Dienst starten**, in einem Terminal, das du im Blick hast, und laufen lassen:

```bash
tephra daemon
```

Er fragt nach dem Master-Passwort, hält den Tresor entsperrt und wartet. Rückfragen erscheinen hier. Beendet wird er
mit Strg+C; das sperrt den Tresor und schließt alle Verbindungen und Tunnel.

Statt `tephra daemon` kann auch die App den Dienst stellen (siehe „In der App“). Für denselben Tresor läuft immer nur
einer von beiden; der zweite meldet beim Start, dass der Dienst schon läuft.

**4. Den Server im MCP-Client eintragen.** Der Client muss den Befehl `tephra mcp serve` mit der Umgebungsvariable
`TEPHRA_MCP_TOKEN` starten. Im verbreiteten JSON-Format:

```json
{
  "mcpServers": {
    "tephra": {
      "command": "tephra",
      "args": ["mcp", "serve"],
      "env": { "TEPHRA_MCP_TOKEN": "tephramcp_…" }
    }
  }
}
```

Steht `tephra` nicht im `PATH` des Clients, gib den vollen Pfad an (`C:\\Tools\\tephra.exe`). Benutzt du einen anderen
Tresor als den am Standardort, ergänze `"TEPHRA_VAULT": "<pfad>"` unter `env` – Brücke und Dienst finden sich über den
Pfad des Tresors, beide müssen also denselben verwenden.

**5. Prüfen.** Das Modell sollte jetzt das Werkzeug `list_hosts` und die freigegebenen Hosts sehen. `tephra mcp audit`
zeigt, was es getan hat.

### Zugriffsstufen

| Stufe | Befehle | Interaktive Sitzungen | Dateien |
|---|---|---|---|
| `read-only` | Nur anzeigende Programme von einer festen Liste, mit festgelegten Optionen | Nein | Lesen mit `--files-read`; Schreiben nie |
| `confirm` | Nach deiner Bestätigung; Muster aus `--allow` ohne Rückfrage | Nach Bestätigung beim Öffnen | Mit `--files-read` / `--files-write`, jeder Zugriff nach Bestätigung |
| `auto` | Ohne Rückfrage, sofern nicht auf der Sperrliste | Ja; jede eingegebene Zeile wird gegen die Sperrliste geprüft | Mit `--files-read` / `--files-write`, ohne Rückfrage |

Eine erneute Freigabe desselben Hosts ersetzt die bisherige.

#### read-only

Ein Befehl wird in Wörter zerlegt und Wort für Wort geprüft. Erlaubt sind diese Programme, jeweils nur mit Optionen,
die nichts verändern, nichts starten und nicht endlos laufen:

| Bereich | Programme |
|---|---|
| Dateien | `ls`, `cat`, `head`, `tail`, `wc`, `stat`, `file`, `readlink`, `realpath`, `tree`, `grep`, `du`, `df`, `sha256sum`, `md5sum`, `which`, `echo`, `pwd` |
| System | `uname`, `hostname`, `uptime`, `date`, `nproc`, `lscpu`, `free`, `lsblk`, `findmnt`, `mount` (ohne Argumente), `dmesg`, `timedatectl`, `hostnamectl` |
| Benutzer | `whoami`, `id`, `w`, `who`, `last`, `getent`, `crontab -l` |
| Prozesse und Dienste | `ps`, `top -bn1`, `systemctl` mit `status`, `is-active`, `is-enabled`, `is-failed`, `list-units`, `list-unit-files`, `list-timers`, `list-sockets`, `journalctl` |
| Netz | `ip` (nur anzeigend: `addr`, `route`, `link`, `neigh`, `rule`), `ss`, `netstat` |
| Software | `docker` mit `ps`, `logs`, `images`; `dpkg -l/-L/-s/-S`, `rpm -q…`, `apt` mit `list`, `policy`, `show`, `search`; `nginx -t/-T/-v/-V` |

Immer abgelehnt werden:

- Verkettungen, Umleitungen und Ersetzungen: `; & | > < $ ( ) { }`, Backticks, Backslashes, Zeilenumbrüche
- Anführungszeichen, die Platzhalter `*`, `?`, `[ ]` und `~` – die Shell würde daraus Wörter bilden, die hier anders
  aussehen als bei der Ausführung
- Optionen, die nicht auf der Liste stehen, etwa `tail -f`, `journalctl --vacuum-size`, `date -s`
- Eingabedaten (das `stdin` von `ssh_exec`) und interaktive Sitzungen
- Pfade, die nach Zugangsdaten aussehen, darunter `.ssh`, `id_rsa`, `authorized_keys`, `shadow`, `sudoers`, `.env`,
  `.pem`, `.key`, `.aws`, `.kube`, `.gnupg`, `.netrc`, `/proc/` und alles, was `passw`, `secret`, `token`, `credential`
  oder `private` enthält

„Nur lesen“ umfasst alles, was der SSH-Benutzer lesen darf. Die Pfadsperre ist eine Hürde gegen Versehen; ein Modell,
das gezielt sucht, hält sie nicht auf. Wenn es darauf ankommt, gib den Host über einen Benutzer mit wenigen Rechten
frei.

Mit `--allow` gibst du stattdessen eigene Muster an. **Sie ersetzen die eingebaute Prüfung vollständig**: Erlaubt ist
dann, was auf ein Muster passt, ohne die Prüfung einzelner Optionen und ohne die Pfadsperre. Verboten bleiben nur
Verkettungen, Umleitungen und Eingabedaten. `tephra` gibt dabei eine Warnung aus.

```bash
tephra mcp grant desktop web --level read-only --allow "systemctl status *" --allow "df -h"
```

#### confirm

Jeder Befehl braucht dein Ja. Befehle, die auf ein Muster aus `--allow` passen, laufen ohne Rückfrage, solange sie
keine Verkettungen oder Umleitungen enthalten.

#### auto

Befehle laufen ohne Rückfrage, sofern sie nicht auf der eingebauten Sperrliste stehen oder auf eines deiner Muster aus
`--deny` passen; deine Muster kommen zu den eingebauten hinzu. Eingebaut sind unter anderem `rm -rf /`, `mkfs`,
`dd … of=/dev/…`, `shutdown`, `reboot`, `poweroff`, `passwd`, `userdel`, `visudo`, `iptables -F`, `ufw disable` und das
Stoppen oder Abschalten von SSH. `tephra` gibt beim Freigeben mit `auto` eine Warnung aus: Die Sperrliste verhindert
Versehen, keinen gezielten Missbrauch. In einer interaktiven Sitzung wird sie zeilenweise geprüft und lässt sich schon
durch Aufteilen einer Eingabe umgehen.

**Muster.** In `--allow` und `--deny` steht `*` für beliebige Zeichen; das Muster muss auf den ganzen Befehl passen.
Beide Optionen dürfen mehrfach angegeben werden.

### Bestätigen

Bei Stufe `confirm` erscheint jede Anfrage im Terminal von `tephra daemon`:

```
── Anfrage von „desktop“ für Host „db“ ──
  Befehl ausführen:
    │ systemctl restart postgresql
  Erlauben? [j] einmal · [s] genau dies für diese Sitzung · [N] ablehnen:
```

Alles, was vom Modell stammt, steht in den mit `│` gekennzeichneten Zeilen:

- bei einem Befehl der Befehl und – falls vorhanden – seine Eingabedaten mit ihrer Größe
- bei einer Datei, die geschrieben werden soll, der Pfad, die Größe, der Anfang der SHA-256-Prüfsumme und der Anfang
  des Inhalts
- bei einer Datei, die gelesen werden soll, der Pfad
- bei einer interaktiven Sitzung der Hinweis, dass danach alle Eingaben des Modells in dieser Sitzung frei sind und
  nicht mehr einzeln gefragt wird

Steuerzeichen, unsichtbare Zeichen und solche, die die Leserichtung ändern, erscheinen als sichtbare Kürzel wie
`\x1b` oder `\u{202e}`; eine Anfrage kann sich auf dem Bildschirm also nicht selbst überschreiben. Text über 2.000
Zeichen (bei Dateiinhalten 400) wird mit ausdrücklichem Hinweis gekürzt.

`j` erlaubt es einmal. `s` erlaubt genau diese Aktion für den Rest dieser MCP-Verbindung: denselben Befehl mit
denselben Eingabedaten oder dieselbe Datei mit demselben Inhalt. Alles andere, oder keine Antwort binnen zwei Minuten,
lehnt ab. Was du getippt hast, bevor die Frage zu sehen war, zählt nicht. Dieselbe Frage erscheint, bevor einem
unbekannten Hostschlüssel vertraut wird; ein geänderter Hostschlüssel wird für MCP immer abgelehnt.

Der Dienst braucht ein Terminal, um zu fragen. Läuft er ohne, wird jede Rückfrage abgelehnt und im Protokoll des
Dienstes vermerkt – nutzbar sind dann nur Hosts mit `read-only` und `auto`.

### In der App

Unter Windows und Linux hat die App einen eigenen Bereich „MCP“. Dort schaltest du den Dienst ein; er läuft dann,
solange der Tresor offen ist, und startet nach dem Entsperren wieder von selbst – auch nach dem nächsten Start der App.
`tephra daemon` brauchst du dafür nicht.

- **Rückfragen** erscheinen als Dialog über der App, gleich welcher Bereich gerade offen ist. Was im grauen Feld steht,
  stammt vom Modell; Steuer- und unsichtbare Zeichen sind dort sichtbar gemacht wie im Terminal. „Einmal erlauben“
  und „Genau dies für diese Verbindung“ lassen sich erst nach einem kurzen Augenblick drücken, damit ein Klick, der
  schon unterwegs war, nicht als Antwort zählt; „Ablehnen“ geht sofort. Ohne Antwort binnen zwei Minuten wird
  abgelehnt. Unter Windows blinkt die App in der Taskleiste, solange eine Frage wartet und das Fenster im Hintergrund
  ist.
- **Clients** legst du dort an. Das Token erscheint einmal, zusammen mit dem fertigen Eintrag für die Konfiguration
  des Clients.
- **Freigaben** gibst du je Client und Host, mit Zugriffsstufe, eigenen Mustern und Dateizugriff – wie mit
  `tephra mcp grant`. Ein Client lässt sich sperren, ohne seine Freigaben zu verlieren.
- **Not-Aus** und **Protokoll** gibt es dort ebenfalls.

Der Bereich und der Dienst ändern dieselben Freigaben wie `tephra mcp`; was du an einer Stelle änderst, gilt an der
anderen.

### Auf einem Linux-Server ohne Anmeldung

[`deploy/tephra-daemon.service`](../deploy/tephra-daemon.service) startet `tephra daemon` als Dienst des Benutzers,
schon beim Hochfahren und ohne dass sich jemand anmeldet. MCP-Clients desselben Benutzers verbinden sich wie gewohnt mit
`tephra mcp serve`, und der Tresor wird abgeglichen, wenn das Gerät mit einem Sync-Server verbunden ist.

```bash
mkdir -p ~/.config/tephra ~/.config/systemd/user
(umask 077; read -rs -p "Master-Passwort: " pw && printf '%s' "$pw" > ~/.config/tephra/master-password)
install -m 0644 deploy/tephra-daemon.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now tephra-daemon
sudo loginctl enable-linger "$USER"
```

Das Master-Passwort liest der Dienst aus der Datei, die `TEPHRA_PASSWORD_FILE` nennt. Sie darf nur für den Benutzer
lesbar sein; sonst lehnt `tephra` sie ab. Wer sie lesen kann – dieser Benutzer und root –, kann den Tresor öffnen. Gib
diesem Tresor deshalb nur die Hosts und Schlüssel, die der Server wirklich braucht. Rückfragen sind ohne Terminal nicht
möglich: Nutzbar sind Hosts mit `read-only` und `auto`, abgelehnte Anfragen stehen im Protokoll
(`journalctl --user -u tephra-daemon`). `systemctl --user stop tephra-daemon` beendet den Dienst sauber und sperrt den
Tresor.

### Was das Modell bekommt

| Werkzeug | Zweck |
|---|---|
| `list_hosts` | Hosts, die diesem Client freigegeben sind: Kennung, Bezeichnung, Adresse, Tags, Zugriffsstufe. Nie Zugangsdaten |
| `ssh_exec` | Führt einen Befehl aus; liefert Exit-Code, stdout und stderr |
| `read_output` | Liest eine abgeschnittene Ausgabe weiter |
| `list_snippets`, `run_snippet` | Listet Snippets (Bezeichnung, Beschreibung, Platzhalter) und führt eines auf einem Host aus, nach den Regeln dieses Hosts |
| `ssh_session_open`, `ssh_session_send`, `ssh_session_read`, `ssh_session_close` | Interaktive Terminalsitzung für Programme, die eine brauchen |
| `tunnel_status` | Ob ein Host durch einen Tunnel erreicht wird und in welchem Zustand der Tunnel ist |
| `files_list`, `files_read`, `files_write` | Verzeichnis auflisten, Textdateien lesen und schreiben über SFTP |

Grenzen: 120 Aufrufe pro Minute und Client, 4 offene Sitzungen je Verbindung, 60 Sekunden je Befehl ohne andere Angabe
(höchstens 600), 2 MiB Ausgabe je Befehl und Strom, 1 MiB je Datei. Lange Ausgaben kommen in Seiten zu 20.000 Zeichen.

**Schwärzung.** Die Zugangsdaten, die beim Verbinden mit einem Host im Spiel sind – sein Passwort, Schlüssel und
dessen Passphrase, die seiner Jump-Hosts, sein Proxy-Passwort und die Schlüssel seines VPN-Profils –, werden durch
`[redacted]` ersetzt, wenn sie in der Ausgabe dieses Hosts oder in einer von dort gelesenen Datei auftauchen. Kann
`tephra` diese Liste nicht zusammenstellen, wird die Aktion nicht ausgeführt. Gefunden werden nur wörtliche Treffer ab
sechs Zeichen: keine kodierte Form und kein Geheimnis, das nur auf dem Host liegt.

### Zugriff verwalten

```bash
tephra mcp clients                               # Clients, ihre Hosts und Stufen; ob MCP eingeschaltet ist
tephra mcp revoke desktop db                     # einen Host entziehen
tephra mcp rm-client desktop                     # Client entfernen; sein Token wird ungültig
tephra mcp disable                               # Not-Aus
tephra mcp enable
tephra mcp audit                                 # die letzten 30 Einträge
tephra mcp audit -n 200
```

**Not-Aus und Entzug.** `tephra mcp disable` lehnt jeden weiteren Aufruf ab und beendet binnen weniger Sekunden alle
interaktiven Sitzungen und SSH-Verbindungen des Dienstes – Befehle, die noch laufen, enden mit ihnen. Der Entzug eines
Hosts beendet die Sitzungen und die Verbindung zu diesem Host; das Entfernen oder Sperren eines Clients schließt dessen
Sitzungen und die von ihm benutzten Verbindungen; das Herabstufen eines Hosts auf `read-only` schließt die Sitzungen
darauf. All das wirkt, während der Dienst läuft, aus jedem anderen Terminal.

**Protokoll.** Jede Aktion wird ins Protokoll eingetragen, bevor sie läuft. Lässt sich der Eintrag nicht schreiben,
wird die Aktion nicht ausgeführt. Ist sie beendet, folgt ein zweiter Eintrag. Das Protokoll nennt Zeit (UTC), Client,
Host, Werkzeug, Ergebnis, Exit-Code und den Befehl oder Pfad:

| Ergebnis | Bedeutung |
|---|---|
| `erlaubt` | Gestartet, weil die Freigabe es ohne Rückfrage erlaubt |
| `bestätigt` | Gestartet, nachdem du zugestimmt hast |
| `beendet` | Eine gestartete Aktion ist zu Ende; mit Exit-Code |
| `ABGELEHNT` | Von den Regeln abgelehnt – auch ein Versuch auf einen nicht freigegebenen Host |
| `VOM NUTZER ABGELEHNT` | Du hast abgelehnt |
| `fehlgeschlagen` | Erlaubt, aber gescheitert, etwa weil der Host nicht erreichbar war |

Das Protokoll liegt verschlüsselt im Tresor dieses Geräts.

**Änderungen bei laufendem Dienst.** Der Dienst bemerkt, was andere `tephra`-Aufrufe an derselben Tresordatei ändern –
neue Hosts, geänderte Identitäten, Freigaben – und verwendet es ab dem nächsten Aufruf. Ein Neustart ist nicht nötig.

**Brücke und Dienst** weisen einander nach, dass sie das Token des Clients kennen; das Token selbst geht nie über den
Kanal. Antwortet unter dem Namen des Kanals etwas anderes als der Dienst, bricht `tephra mcp serve` ab.

Freigaben gehören zu einem Gerät. Sie werden nicht abgeglichen: MCP auf einem Rechner einzurichten öffnet deine Hosts
nicht auf einem anderen.

## Sync-Server betreiben

`tephra-server` ist ein einzelnes Programm mit einer einzelnen SQLite-Datei. Er meldet Geräte an, speichert
verschlüsselte Datensätze mit Revisionsnummern und sagt Geräten Bescheid, wenn es Neues gibt. Was er speichert, kann er
nicht lesen.

```bash
tephra-server serve                              # lauscht auf 127.0.0.1:8787, Datenbank tephra-server.sqlite im aktuellen Ordner
tephra-server invite                             # gibt einen Einladungscode für ein neues Konto aus
tephra-server accounts                           # Konten mit Anzahl der Geräte, Datensätze und Größe
tephra-server delete-account alice               # löscht ein Konto mit allen Daten
```

| Option | Variable | Standard | Bedeutung |
|---|---|---|---|
| `--db <datei>` | `TEPHRA_SERVER_DB` | `tephra-server.sqlite` | Datenbankdatei |
| `--bind <adresse>` | `TEPHRA_SERVER_BIND` | `127.0.0.1:8787` | Adresse, auf der der Server lauscht |
| `--registration <art>` | `TEPHRA_SERVER_REGISTRATION` | `invite` | `open`: jeder darf sich registrieren; `invite`: nur mit Einladungscode; `closed`: niemand |
| `--max-items <n>` | `TEPHRA_SERVER_MAX_ITEMS` | `50000` | Datensätze je Konto, Löschmarker eingeschlossen |
| `--max-mib <n>` | `TEPHRA_SERVER_MAX_MIB` | `256` | Datenmenge je Konto in MiB |
| `--behind-proxy` | `TEPHRA_SERVER_BEHIND_PROXY` | aus | Der Server steht hinter einem Reverse-Proxy, der `X-Forwarded-For` setzt |

Die Optionen gelten mit und ohne den Befehl `serve`; `tephra-server` allein startet den Server.

**TLS.** Der Server spricht einfaches HTTP und soll auf `127.0.0.1` hinter einem Reverse-Proxy lauschen, der TLS
bereitstellt. Clients lehnen `http://`-Adressen ab, außer für `localhost`, und prüfen das Zertifikat gegen den
Zertifikatsspeicher des Betriebssystems. Anfragen an `/v1/events` bleiben bis zu 55 Sekunden offen, der Proxy darf
Verbindungen also nicht früher trennen. `GET /healthz` antwortet mit `ok`.

**Im eigenen Netz ohne TLS.** Ist der Server nur in deinem eigenen Netz erreichbar – zu Hause oder über ein VPN –,
kann er ohne Proxy laufen: mit `--bind 0.0.0.0:8787` und ohne `--behind-proxy`. Die Clients verbinden sich dann mit
`http://<adresse>:8787` und brauchen dafür deine ausdrückliche Erlaubnis (`--allow-http`, in der App der Haken
„Unverschlüsselt verbinden“). Der Inhalt des Tresors bleibt auch so verschlüsselt. Wer in diesem Netz mitliest, sieht
aber den Anmeldenachweis und die Sitzungstokens. Damit kann er sich am Server als eines deiner Geräte anmelden, die
verschlüsselten Datensätze abholen und daran Master-Passwörter durchprobieren, Geräte entfernen und das Konto auf dem
Server löschen. Über das Internet gehört deshalb immer TLS davor.

**Hinter einem Proxy `--behind-proxy` setzen.** Fehlversuche bei der Anmeldung sind je Benutzername und Absender
begrenzt: 10 innerhalb von 5 Minuten. Ohne die Option scheint jede Anfrage vom Proxy zu kommen, und ein Einzelner, der
Passwörter rät, würde alle bremsen. Mit ihr gilt als Absender die letzte Adresse in `X-Forwarded-For` – die, die dein
Proxy angefügt hat. Setze sie nicht, wenn der Server direkt erreichbar ist; dann könnte jeder eine beliebige Adresse
in diesen Kopf schreiben. Korrekte Anmeldungen werden nie gebremst; niemand kann dich also aussperren, indem er für
deinen Namen falsche Passwörter probiert.

**Einladungscodes** gelten einmal. Der Server speichert nur ihren Hash, wie bei allen Sitzungstokens.

**Beenden.** Der Server fährt bei Strg+C und bei SIGTERM, wie systemd und Docker es senden, sauber herunter.

**Sicherung.** Halte den Server an und kopiere die Datenbankdatei zusammen mit den Dateien daneben, die auf `-wal` und
`-shm` enden. Die Sicherung enthält nur Chiffrate, dazu Benutzernamen und Gerätenamen.

### Mit Docker

Der Ordner [`deploy/`](../deploy/) enthält ein `Dockerfile`, eine Compose-Datei mit [Caddy](https://caddyserver.com) als
TLS-Proxy und ein `Caddyfile`. Die Compose-Datei verwendet das veröffentlichte Image `ghcr.io/poldion/tephra-server`
(amd64 und arm64); mit `--build` baut sie es stattdessen aus dem Quelltext.

1. Ersetze `sync.example.org` in `deploy/Caddyfile` durch deinen eigenen Namen. Er muss auf den Rechner zeigen, und
   die Ports 80 und 443 müssen erreichbar sein, damit Caddy ein Zertifikat holen kann.
2. Starten:

   ```bash
   cd deploy
   docker compose up -d
   ```

3. Einen Einladungscode erzeugen:

   ```bash
   docker compose exec sync tephra-server invite
   ```

Die Datenbank liegt im Volume `sync-data`. Der Server läuft als unprivilegierter Benutzer in einem schreibgeschützten
Container, mit gesetztem `TEPHRA_SERVER_BEHIND_PROXY`, weil Caddy davorsteht. `docker compose exec sync tephra-server
accounts` listet die Konten.

Für einen Server nur im eigenen Netz ohne TLS (siehe oben) entfällt der Dienst `caddy`. Der Dienst `sync` bekommt dann
`ports: ["8787:8787"]` statt `expose`, und die Zeile `TEPHRA_SERVER_BEHIND_PROXY` wird entfernt.

### Mit systemd

[`deploy/tephra-server.service`](../deploy/tephra-server.service) betreibt den Server unter einem eigenen Benutzer mit
einer gehärteten Unit.

```bash
sudo install -m 0755 target/release/tephra-server /usr/local/bin/tephra-server
sudo useradd --system --user-group --home-dir /var/lib/tephra-server --shell /usr/sbin/nologin tephra-server
sudo install -m 0644 deploy/tephra-server.service /etc/systemd/system/tephra-server.service
sudo systemctl daemon-reload
sudo systemctl enable --now tephra-server
sudo -u tephra-server tephra-server --db /var/lib/tephra-server/tephra-server.sqlite invite
```

Die Datenbank ist `/var/lib/tephra-server/tephra-server.sqlite`. Der Server lauscht auf `127.0.0.1:8787`; davor gehört
Caddy, nginx oder ein anderer Proxy mit TLS. Für Caddy nimmst du das `Caddyfile` aus `deploy/` mit `127.0.0.1:8787` als
Ziel. Die Unit setzt `TEPHRA_SERVER_BEHIND_PROXY`; dein Proxy muss `X-Forwarded-For` setzen (Caddy tut das, bei nginx
ergänzt du `proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;`).

## Skripte und Umgebungsvariablen

| Variable | Verwendet von | Bedeutung |
|---|---|---|
| `TEPHRA_VAULT` | allen Befehlen | Pfad der Tresordatei |
| `TEPHRA_PASSWORD` | allen Befehlen, die entsperren | Master-Passwort statt Abfrage |
| `TEPHRA_PASSWORD_FILE` | allen Befehlen, die entsperren | Datei mit dem Master-Passwort, etwa für `tephra daemon` unter systemd; unter Linux nur für den Benutzer lesbar |
| `TEPHRA_NEW_PASSWORD` | `tephra init`, `tephra passwd`, `tephra recover` | Neues Master-Passwort statt Abfrage |
| `TEPHRA_IDENTITY_PASSWORD` | `tephra identity add` | Zu speicherndes Passwort statt Abfrage |
| `TEPHRA_KEY_PASSPHRASE` | `tephra key import` | Passphrase des Schlüssels statt Abfrage |
| `TEPHRA_MCP_TOKEN` | `tephra mcp serve` | Token des MCP-Clients |

```bash
export TEPHRA_PASSWORD='…'
tephra exec web -- uptime
tephra sync now
```

Ein Passwort in einer Umgebungsvariable können andere Programme desselben Benutzers lesen, und es kann im Verlauf
deiner Shell landen. Nimm es für Skripte und Tests, nicht aus Gewohnheit.

Abfragen gehen auf die Fehlerausgabe; die Ausgabe eines Befehls lässt sich also umleiten, während du noch nach dem
Passwort gefragt wirst. Fehler beginnen mit `Fehler:` und ergeben den Exit-Code 1; `tephra exec` und `tephra ssh` geben
den Exit-Code der Gegenseite zurück.
