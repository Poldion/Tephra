# Lizenzen der Abhängigkeiten

Dieses Dokument hält fest, unter welchen Lizenzen die Rust-Abhängigkeiten des Workspaces stehen, ob sie
sich mit einer MIT-lizenzierten, als Binary verteilten Anwendung (Desktop-App, Android-APK,
Server-Binary/Docker-Image) vertragen und welche Pflichten bei der Verteilung entstehen.

Es ist eine technische Bestandsaufnahme und keine Rechtsberatung.

**Stand:** `Cargo.lock` vom 18.11.2026 (SHA-256 beginnt mit `78e77c9cfadc6ef2`), 415 externe Pakete. Der
Workspace ist im Aufbau; die Zahlen ändern sich mit jeder neuen Abhängigkeit. Verbindlich ist die
laufende Prüfung mit `cargo deny --locked check` (Konfiguration: [`deny.toml`](../deny.toml)).

**Vorgehen:** Die erste Fassung (Lockfile mit 299 Paketen) beruhte auf `cargo metadata --locked`,
`cargo tree --target … -e normal,build` je Zielplattform, der Durchsicht der Lizenz- und NOTICE-Dateien
in den Crate-Quellen, einer Gegenprüfung mit `cargo-deny` 0.20.2 und einem Probelauf mit `cargo-about`
0.9.3. Seitdem sind Sync-Server, Abgleich-Client und Kommandozeile hinzugekommen. Für den jetzigen
Stand wurden Abschnitt 1 und 2 aus dem Lockfile und den Manifesten der Pakete (Feld `license`) neu
ausgezählt; **die Prüfung mit `cargo-deny`, die Zahlen je Zielplattform und der Probelauf mit
`cargo-about` sind für diesen Stand noch nicht wiederholt** (siehe Offene Punkte).

**Ergebnis in Kürze:** Unter den 415 externen Paketen ist keines unter GPL, AGPL oder SSPL (LGPL nur als
abwählbare Alternative bei einem nicht eingebundenen Crate), kein Paket ohne Lizenzangabe und keines,
das statt eines SPDX-Ausdrucks nur auf eine eigene Lizenzdatei verweist. Alle Pakete stammen von
crates.io. Das zwischenzeitlich eingebundene Paket unter MPL-2.0 (`option-ext`, über `directories`)
ist wieder entfallen: Den Datenordner ermittelt die Kommandozeile jetzt selbst. `cargo deny check licenses`
läuft damit ohne Beanstandung.
Zu beachten sind außerdem die üblichen Hinweispflichten (Lizenztexte und Copyright-Vermerke beilegen).

## 1. Überblick nach Lizenz

Gezählt sind alle 415 externen Pakete aus `Cargo.lock` – also alle Plattformen, einschließlich
Test-Abhängigkeiten und optionaler Pakete, die derzeit kein Feature aktiviert. Schreibweisen wie
`MIT/Apache-2.0` und `Apache-2.0 OR MIT` sind zusammengefasst.

| Lizenz | Anzahl Crates | Bewertung |
|---|---:|---|
| `MIT OR Apache-2.0` | 288 | Unproblematisch. Wahlrecht; wir wählen MIT. |
| `MIT` | 58 | Unproblematisch. Copyright-Vermerk und Lizenztext beilegen. |
| `Unicode-3.0` | 18 | Vereinbar, Unicode-Lizenzhinweis beilegen. Die ICU4X-Crates (`icu_*`, `zerovec`, `yoke`, `tinystr`, `litemap` …) kommen über `url` → `idna` herein, also über `reqwest` (Abgleich-Client) und `tower-http`. |
| `Apache-2.0` | 12 | Vereinbar, mit Auflagen (Abschnitt 4). Betrifft `russh`, `russh-sftp`, `russh-cryptovec`, `russh-util`, `pageant`, `rmcp`, `rmcp-macros`, `rpassword`, `rtoolbox`, `sync_wrapper`, `unicode-general-category`; `borsh-derive` ist nicht aktiviert. |
| `BSD-3-Clause` | 7 | Vereinbar, mit Auflagen: Vermerk beilegen, keine Werbung mit den Namen der Urheber. Betrifft `boringtun`, `curve25519-dalek` (4.1.3 und 5.0.0), `ed25519-dalek`, `x25519-dalek`, `subtle`, `sha1_smol`. |
| `MIT OR Unlicense` | 5 | Unproblematisch (`byteorder`, `memchr`, `same-file`, `walkdir`, `winapi-util`); wir wählen MIT. |
| `MIT OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception` | 3 | Unproblematisch (`rustix`, `linux-raw-sys`, `wasi`). |
| `Apache-2.0 OR ISC OR MIT` | 3 | Unproblematisch (`rustls`, `hyper-rustls`, `rustls-native-certs`); wir wählen MIT. |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 2 | Vereinbar, Unicode-Lizenzhinweis beilegen (`unicode-normalization`, `unicode-ident`). |
| `0BSD` | 2 | Unproblematisch, keine Auflagen (`smoltcp`, `managed`). |
| `BSD-2-Clause` | 2 | Vereinbar, Vermerk beilegen (`ip_network`, `ip_network_table`, beide über `boringtun`). |
| `ISC` | 2 | Unproblematisch, Vermerk beilegen (`untrusted`, `rustls-webpki`). |
| `MIT OR Apache-2.0 OR BSD-1-Clause` | 2 | Unproblematisch (`fiat-crypto`). |
| `MIT OR Apache-2.0 OR Zlib` | 2 | Unproblematisch (`miniz_oxide`, `tinyvec`). |
| `Zlib` | 2 | Unproblematisch (`foldhash`, `zlib-rs`). |
| `Apache-2.0 AND ISC` | 1 | `ring` – vereinbar, mit Auflagen (Abschnitt 3). |
| `MIT AND BSD-3-Clause` | 1 | `matchit` (über `axum`) – vereinbar, beide Vermerke beilegen. |
| `CDLA-Permissive-2.0` | 1 | `webpki-root-certs` – nicht in der Erlaubnisliste, auf den Zielplattformen aber nicht eingebunden (Abschnitt 3). |
| `Apache-2.0 OR BSL-1.0` | 1 | Unproblematisch (`ryu`); wir wählen Apache-2.0. |
| `MIT OR Apache-2.0 OR 0BSD` | 1 | Unproblematisch (`adler2`). |
| `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | 1 | `r-efi` – LGPL ist nur eine von drei Alternativen; wir wählen MIT. Das Crate wird ausschließlich für UEFI-Ziele gebaut. |
| GPL, AGPL, SSPL, unbekannt, fehlend | 0 | Nicht vorhanden. |

### Tatsächlich eingebundene Pakete je Zielplattform

Die folgende Tabelle gibt den **früheren Stand** wieder (Lockfile mit 299 Paketen, vor Sync-Server,
Abgleich-Client und Kommandozeile). Sie ist mit `cargo tree --target …` neu zu ermitteln.

| Umfang | Pakete (früherer Stand) |
|---|---:|
| `Cargo.lock` gesamt | 299 |
| aktiviert, alle Plattformen, mit Tests | 286 |
| aktiviert, alle Plattformen, ohne Tests | 268 |
| Windows x86_64 (`x86_64-pc-windows-msvc`) | 237 |
| Linux x86_64 (`x86_64-unknown-linux-gnu`) | 227 |
| Android aarch64 (`aarch64-linux-android`) | 226 |
| Vereinigung der drei Zielplattformen | 241 |

Zum früheren Stand kamen in den drei Zielplattformen ausschließlich die acht Lizenzen der Erlaubnisliste
von `deny.toml` vor (MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, 0BSD, Zlib, Unicode-3.0). Das gilt
nach dem Entfernen von `directories` weiterhin.

Die vollständige Liste aller Pakete mit Lizenz lässt sich jederzeit mit `cargo deny --locked list`
ausgeben.

## 2. Direkte Abhängigkeiten

| Komponente | Version | Lizenz | Zweck |
|---|---|---|---|
| `argon2` | 0.6.0 | MIT OR Apache-2.0 | Schlüsselableitung aus dem Master-Passwort (`tephra-crypto`) |
| `async-trait` | 0.1.92 | MIT OR Apache-2.0 | Asynchrone Trait-Methoden (Transport, SSH, MCP, Kern, Kommandozeile) |
| `axum` | 0.8.9 | MIT | HTTP-Schnittstelle des Sync-Servers (`tephra-server`) |
| `base64` | 0.23.1 | MIT OR Apache-2.0 | Kodierung der Tunnel-Schlüssel und der Binärdaten im Sync-Protokoll (`tephra-transport`, `tephra-sync-proto`, `tephra-server`) |
| `boringtun` | 0.7.1 | BSD-3-Clause | Userspace-Implementierung des WireGuard-Protokolls (`tephra-transport`) |
| `chacha20poly1305` | 0.11.0 | MIT OR Apache-2.0 | Authentifizierte Verschlüsselung der Vault-Einträge (`tephra-crypto`) |
| `clap` | 4.6.8 | MIT OR Apache-2.0 | Auswertung der Kommandozeile (`tephra-cli`, `tephra-server`) |
| `crossterm` | 0.29.0 | MIT | Terminalsteuerung für `tephra ssh` (`tephra-cli`) |
| `getrandom` | 0.4.3 | MIT OR Apache-2.0 | Zufallszahlen des Betriebssystems (`tephra-crypto`) |
| `hex` | 0.4.3 | MIT OR Apache-2.0 | Tokens des Sync-Servers (`tephra-server`); Testvektoren (`tephra-crypto`) |
| `hkdf` | 0.13.0 | MIT OR Apache-2.0 | Schlüsselableitung HKDF (`tephra-crypto`) |
| `proptest` | 1.12.0 | MIT OR Apache-2.0 | Property-Tests – nur Tests |
| `rand` | 0.10.4 | MIT OR Apache-2.0 | Zufallszahlen (`tephra-ssh`) |
| `reqwest` | 0.13.5 | MIT OR Apache-2.0 | HTTP-Client des Abgleichs (`tephra-sync`), mit `rustls` ohne eigenen Krypto-Unterbau |
| `rmcp` | 3.11.0 | Apache-2.0 (Teile MIT, siehe Abschnitt 3) | MCP-Server, Rust-SDK des Model Context Protocol (`tephra-mcp`) |
| `rpassword` | 7.5.5 | Apache-2.0 | Passwortabfrage ohne Anzeige (`tephra-cli`) |
| `rusqlite` | 0.43.0 | MIT | Lokale Datenbank und Datenbank des Sync-Servers, SQLite mitkompiliert (`tephra-vault`, `tephra-server`) |
| `russh` | 0.65.2 | Apache-2.0 | SSH-Protokoll (`tephra-ssh`) |
| `russh-sftp` | 3.0.1 | Apache-2.0 | SFTP-Subsystem (`tephra-ssh`) |
| `rustls` | 0.23.46 | Apache-2.0 OR ISC OR MIT | TLS des Abgleich-Clients mit `ring` als Krypto-Unterbau (`tephra-sync`) |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | Serialisierung |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 | JSON |
| `sha2` | 0.11.1 | MIT OR Apache-2.0 | SHA-2-Hashfunktionen (`tephra-crypto`, `tephra-mcp`, `tephra-server`) |
| `smoltcp` | 0.14.0 | 0BSD | TCP/IP-Stack im Userspace für den Tunnel (`tephra-transport`) |
| `socket2` | 0.6.5 | MIT OR Apache-2.0 | Socket-Optionen (`tephra-transport`) |
| `subtle` | 2.6.1 | BSD-3-Clause | Vergleiche in konstanter Zeit (`tephra-crypto`, `tephra-server`) |
| `tempfile` | 3.27.0 | MIT OR Apache-2.0 | Temporäre Dateien – nur Tests |
| `thiserror` | 2.0.21 | MIT OR Apache-2.0 | Fehlertypen |
| `tokio` | 1.54.1 | MIT | Asynchrone Laufzeit |
| `tracing` | 0.1.44 | MIT | Protokollierung und Diagnose |
| `tracing-subscriber` | 0.3.23 | MIT | Ausgabe der Diagnosemeldungen (`tephra-cli`, `tephra-server`) |
| `uuid` | 1.29.3 | MIT OR Apache-2.0 | Kennungen (UUID v4, v5, v7) |
| `zeroize` | 1.9.2 | MIT OR Apache-2.0 | Überschreiben von Geheimnissen im Speicher |

## 3. Einzelbefunde

**`ring` 0.17.14** (über `russh` und `boringtun`) – Lizenzausdruck `Apache-2.0 AND ISC`, im Manifest als
gültiger SPDX-Ausdruck hinterlegt; eine Klarstellung in `deny.toml` ist deshalb nicht nötig. Die
mitgelieferten Dateien (`LICENSE`, `LICENSE-BoringSSL`, `LICENSE-other-bits`, `third_party/fiat/LICENSE`,
`src/polyfill/once_cell/LICENSE-*`) enthalten Apache-2.0, ISC, MIT und BSD-artige Texte, aber keine
OpenSSL-/SSLeay-Lizenz. `ring` enthält C- und Assembler-Code aus BoringSSL, der statisch in unsere
Binaries gelinkt wird. Auflage: die genannten Lizenztexte mit ihren Copyright-Vermerken beilegen.

**`aws-lc-rs`, `aws-lc-sys`, `openssl-sys`** – nicht im Abhängigkeitsgraphen. `reqwest` und `rustls`
werden ohne eigenen Krypto-Unterbau bzw. mit `ring` eingebunden.

**`webpki-root-certs` 1.0.11** (über `rustls-platform-verifier`) – `CDLA-Permissive-2.0`, nicht in der
Erlaubnisliste. Das Crate ist nur für WebAssembly-Ziele und für Tests eine Abhängigkeit von
`rustls-platform-verifier` und auf Windows, Linux und Android nicht eingebunden; dort prüft der
Abgleich-Client Zertifikate gegen den Zertifikatsspeicher des Betriebssystems (Linux: über
`rustls-native-certs`). Eine Liste von Wurzelzertifikaten wird nicht einkompiliert.

**`rustls-webpki` 0.103.16** – `ISC`, bereits erlaubt.

**`matchit` 0.8.4** (über `axum`) – `MIT AND BSD-3-Clause`; beide Vermerke beilegen.

**ICU4X-Crates** (`icu_normalizer`, `icu_properties`, `zerovec`, `yoke` … – 18 Pakete, über `url` →
`idna`) – `Unicode-3.0`, bereits erlaubt; Lizenzhinweis beilegen.

**`boringtun` 0.7.1** – `BSD-3-Clause`. Das veröffentlichte Crate enthält **keine Lizenzdatei**; der
Vermerk steht nur in den Quelldateien („Copyright (c) 2019 Cloudflare, Inc. All rights reserved.",
`SPDX-License-Identifier: BSD-3-Clause`). Für die Drittanbieter-Übersicht muss der Lizenztext mit diesem
Vermerk von Hand ergänzt werden (siehe Offene Punkte).

**`smoltcp` 0.14.0, `managed` 0.8.0** – `0BSD`, keine Auflagen.

**`russh` 0.65.2, `russh-sftp` 3.0.1, `russh-cryptovec` 0.62.0, `russh-util` 0.52.0, `pageant` 0.2.5** –
`Apache-2.0`. Bis auf `russh-sftp` liefern diese Crates keine Lizenzdatei mit, und keines enthält eine
NOTICE-Datei. Es genügt, den Standardtext der Apache-Lizenz beizulegen. `pageant` wird nur unter Windows
eingebunden.

**`ssh-key` 0.7.0-rc.12** (über `russh`) – `Apache-2.0 OR MIT`, lizenzrechtlich unauffällig. Es handelt
sich um eine Vorabversion; das ist eine Frage des Pflegezustands, nicht der Lizenz.

**`rusqlite` 0.43.0 / `libsqlite3-sys` 0.40.0** – beide `MIT`. Mit dem Feature `bundled` wird die
mitgelieferte SQLite-Amalgamation (Version 3.54.1) übersetzt und statisch gelinkt. SQLite ist gemeinfrei
(„The author disclaims copyright to this source code"); daraus entstehen keine Pflichten. SQLCipher ist
nicht aktiviert. Eine Erwähnung von SQLite in der Drittanbieter-Übersicht ist üblich, aber nicht
erforderlich.

**`rmcp` 3.11.0, `rmcp-macros` 3.11.0** – Das Manifest nennt `Apache-2.0`. Die mitgelieferte
`LICENSE`-Datei beschreibt jedoch einen laufenden Lizenzwechsel von MIT zu Apache-2.0: Beiträge ohne
Zustimmung zur Umlizenzierung stehen weiter unter MIT. Faktisch gilt `Apache-2.0 AND MIT`; beide Lizenzen
sind erlaubt. In `deny.toml` ist das als Klarstellung hinterlegt. Auflage: die `LICENSE`-Datei des Crates
unverändert beilegen (sie enthält beide Texte samt Copyright-Vermerk), nicht nur den Apache-Standardtext.

**`curve25519-dalek` 4.1.3 und 5.0.0** – `BSD-3-Clause`; die Lizenzdatei enthält zusätzlich den
BSD-Vermerk der Go-Autoren für übernommene Teile. Datei vollständig beilegen.

**`unicode-ident` 1.0.27, `unicode-normalization` 0.1.26** – zusätzlich `Unicode-3.0` für die enthaltenen
Unicode-Tabellen. Freizügig; der Lizenzhinweis ist beizulegen.

**`-sys`-Crates** – Auf den Zielplattformen sind nur `libsqlite3-sys` (siehe oben) und `windows-sys`
(`MIT OR Apache-2.0`, reine Schnittstellendefinitionen) eingebunden. `core-foundation-sys`, `js-sys` und
`linux-raw-sys` stehen nur für andere Plattformen bzw. Tests im Lockfile. Fremde Systembibliotheken
werden nicht mitgeliefert.

**Doppelte Versionen** – 22 Crates liegen in zwei Versionen vor (u. a. `curve25519-dalek`,
`chacha20poly1305`, `digest`, `getrandom`, `syn`), weil `boringtun` und `russh` unterschiedliche
Generationen der Krypto-Crates verwenden. Lizenzrechtlich ohne Belang; `cargo deny check bans` warnt.

## 4. Auflagen und wie sie erfüllt werden

Keine der vorkommenden Lizenzen verlangt, Quelltext offenzulegen oder die eigene Anwendung unter eine
bestimmte Lizenz zu stellen. Es entstehen ausschließlich Hinweispflichten:

| Lizenz | Pflicht bei Verteilung von Binaries |
|---|---|
| MIT, ISC | Copyright-Vermerk und Lizenztext in allen Kopien mitgeben. |
| BSD-2-Clause, BSD-3-Clause | Copyright-Vermerk, Bedingungen und Haftungsausschluss in der Dokumentation oder den Begleitmaterialien wiedergeben. BSD-3 zusätzlich: Namen der Urheber (z. B. Cloudflare) nicht ohne Erlaubnis zur Bewerbung des Produkts verwenden. |
| Apache-2.0 | Kopie der Lizenz mitgeben; Inhalt einer NOTICE-Datei übernehmen, falls das Werk eine enthält (derzeit bei keinem Crate der Fall); geänderte Dateien kennzeichnen (wir ändern keine). Die Lizenz gewährt keine Markenrechte. |
| Unicode-3.0 | Copyright- und Lizenzhinweis mitgeben. |
| Zlib | Für Binaries keine Hinweispflicht; wir führen den Text trotzdem auf. |
| 0BSD, Gemeinfreiheit (SQLite) | Keine. |

### Vorgesehene Umsetzung

1. **Lizenzprüfung in der CI.** `cargo deny --locked check` läuft bei jeder Änderung. Eine neue Lizenz,
   eine Git-Quelle oder eine Sicherheitsmeldung lässt den Lauf fehlschlagen.
2. **Drittanbieter-Übersicht aus dem Lockfile.** Bei jedem Release-Build wird mit `cargo-about` aus dem
   tatsächlichen Abhängigkeitsgraphen eine Datei mit allen Lizenztexten und den zugehörigen Crates
   erstellt (`cargo about generate --locked --workspace -c about.toml -o THIRD-PARTY-LICENSES.txt
   <Vorlage>.hbs`), nicht von Hand gepflegt. Ein Probelauf mit folgender Konfiguration lief fehlerfrei
   durch und ergab 155 Lizenzblöcke für die drei Zielplattformen:

   ```toml
   accepted = ["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "0BSD", "Zlib", "Unicode-3.0"]
   targets = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "aarch64-linux-android"]
   ignore-build-dependencies = false
   ignore-dev-dependencies = true
   workarounds = ["ring"]
   private = { ignore = true }
   ```

   Die Reihenfolge in `accepted` legt fest, welche Alternative bei Wahlrechten gezogen wird (MIT zuerst).
3. **Ergänzungen von Hand.** Für einige Crates findet das Werkzeug keine eindeutig zuordenbare
   Lizenzdatei und setzt den Standardtext ohne Copyright-Zeile ein. Diese Fälle brauchen einen Eintrag
   in `about.toml` oder eine gepflegte Ergänzungsdatei:
   - BSD-3-Clause: `boringtun`, `curve25519-dalek` 4.1.3, `sha1_smol`
   - MIT: `chrono`, `delegate`, `md5`, die `windows-*`-Crates
   - Apache-2.0: `rmcp`, `rmcp-macros` (Datei wörtlich übernehmen). Bei `russh`, `russh-cryptovec`,
     `russh-util` und `pageant` genügt der Standardtext.
4. **Wo die Übersicht landet.**
   - *Flutter-App (Desktop und Android):* Die Datei wird als Asset eingebunden und beim Start über
     `LicenseRegistry.addLicense` registriert. Damit erscheint sie zusammen mit den Lizenzen der
     Dart-Pakete und der Flutter-Engine auf der Lizenzseite, die aus dem „Über"-Dialog erreichbar ist.
   - *CLI und Server:* Die Datei wird einkompiliert und über einen Schalter ausgegeben (z. B.
     `tephra licenses`, `tephra-server --licenses`).
   - *Release-Archive und Installer:* `LICENSE` (eigene MIT-Lizenz) und `THIRD-PARTY-LICENSES.txt`
     liegen neben dem Binary.
   - *Docker-Image:* beide Dateien unter `/usr/share/doc/tephra-server/`, dazu das Label
     `org.opencontainers.image.licenses`.
5. **README.** Abschnitt „Lizenz" mit Verweis auf `LICENSE` und dieses Dokument sowie dem Markenhinweis
   aus Abschnitt 5.

## 5. Marke „WireGuard"

„WireGuard" und das WireGuard-Logo sind eingetragene Marken von Jason A. Donenfeld. Die
Markenrichtlinie (<https://www.wireguard.com/trademark-policy/>, abgerufen am 18.11.2026) verbietet ohne
Genehmigung unter anderem die Verwendung in Produkt- und Firmennamen, verwechslungsfähige Namen, jede
Andeutung einer Unterstützung oder Verbindung sowie die Verwendung des Logos in App-Grafiken und
Werbematerial. Sie verlangt die Schreibweise mit großem W und G und einen Hinweis auf den Markeninhaber.

Für das Projekt heißt das:

- „WireGuard" kommt weder im Produktnamen noch in Namen von Binaries, Paketen, Anwendungskennungen,
  Domains oder Store-Einträgen vor. Das Logo wird nicht verwendet.
- In Oberfläche und Dokumentation wird der Begriff nur beschreibend für das Protokoll benutzt
  („Tunnel über das WireGuard-Protokoll", „Import von WireGuard-Konfigurationsdateien"), nie als
  Bezeichnung einer eigenen Funktion oder eines eigenen Produkts.
- README, „Über"-Dialog und Store-Beschreibung erhalten den Hinweis: „WireGuard ist eine eingetragene
  Marke von Jason A. Donenfeld. Dieses Projekt steht in keiner Verbindung zum WireGuard-Projekt und wird
  von ihm weder unterstützt noch empfohlen."
- Die Richtlinie ist enger gefasst, als „rein beschreibende Verwendung" vermuten lässt: Sie nennt auch
  Aussagen zur Kompatibilität und die Verwendung im Zusammenhang mit Software, die nicht von
  wireguard.com stammt, als genehmigungspflichtig. Vor der ersten Veröffentlichung sollte deshalb über
  die in der Richtlinie genannte Adresse eine Freigabe für die geplanten Formulierungen eingeholt werden
  (siehe Offene Punkte).

## 6. Geplante Abhängigkeiten

Die folgenden Bausteine sind vorgesehen, aber **noch nicht eingebunden**, und wurden nur anhand der
Angaben auf crates.io bzw. pub.dev geprüft (Abruf 18.11.2026). Ihre transitiven Abhängigkeiten sind
ungeprüft; maßgeblich ist `cargo deny check` nach der Aufnahme. `rmcp`, `axum`, `reqwest`, `rustls` und
`rustls-webpki` standen ebenfalls auf dieser Liste, sind inzwischen eingebunden und oben aufgeführt. Der
Sync-Server verwendet `rusqlite` statt des ursprünglich vorgesehenen `sqlx`; `rustls` wird mit `ring`
gebaut, `aws-lc-rs` kommt also nicht hinzu.

Die App unter `app/` ist inzwischen angelegt. `flutter_rust_bridge` (Crate und Dart-Paket, MIT), `xterm`
(MIT) und die übrigen Dart-Pakete aus `app/pubspec.lock` sind eingebunden; sie stehen laut den
Lizenzdateien der Pakete unter MIT, BSD-3-Clause oder Apache-2.0 (Stand 18.11.2026) und sind in
`THIRD_PARTY_NOTICES.md` unter „Graphical app" genannt.

| Baustein | Version bei Abruf | Lizenz laut Registry | Anmerkung |
|---|---|---|---|
| `webpki-roots` | 1.0.12 | CDLA-Permissive-2.0 | Freizügige Datenlizenz, **nicht** in der Erlaubnisliste. Nur nötig, wenn die Mozilla-Wurzelzertifikate einkompiliert werden sollen (derzeit prüft der Client gegen den Zertifikatsspeicher des Betriebssystems); dann prüfen und aufnehmen. |
| `alacritty_terminal` | 0.26.0 | Apache-2.0 | Nur Rückfallebene, falls das Terminal-Paket `xterm` nicht weiter gepflegt wird (letzte Veröffentlichung laut pub.dev vor rund zwei Jahren). |
| `local_auth` (Dart) | 3.0.2 | BSD-3-Clause | – |
| `mobile_scanner` (Dart) | 7.5.0 | BSD-3-Clause | Nutzt unter Android die Barcode-Erkennung von Google ML Kit. Diese Bibliothek ist nicht quelloffen und unterliegt eigenen Nutzungsbedingungen; relevant für die Lizenzübersicht der APK und für eine Verteilung über F-Droid. |
| Flutter-SDK und -Engine | – | BSD-3-Clause (SDK) | Eingebunden. Die Lizenzen der Engine und ihrer Bestandteile wurden nicht einzeln untersucht; Flutter bündelt sie zusammen mit denen der Dart-Pakete und zeigt sie auf der Lizenzseite der App an. |

## 7. Offene Punkte

1. **Erledigt: `option-ext` unter MPL-2.0.** Das Crate kam über `directories` herein und ist mit dessen
   Entfernen entfallen; die Erlaubnisliste bleibt unverändert.
   Erledigt ist der frühere Punkt an dieser Stelle: Die eigene Lizenz ist hinterlegt (`LICENSE`, MIT,
   Copyright-Inhaber „Pold"; `license = "MIT"` und `publish = false` unter `[workspace.package]`, von
   allen elf Crates übernommen).
2. **Prüfung für den aktuellen Stand wiederholen.** `cargo deny --locked check`, die Paketzahlen je
   Zielplattform (Abschnitt 1) und der Probelauf mit `cargo-about` beziehen sich noch auf das frühere
   Lockfile. Abschnitt 1 und 2 sind aus dem Lockfile und den Manifesten neu ausgezählt, aber nicht mit
   `cargo-deny` gegengeprüft.
3. **Sicherheitsmeldung RUSTSEC-2026-0543:** `chrono` 0.4.45 ist als nicht mehr gepflegt gemeldet
   (eingebunden über `russh-sftp`, `russh-util`, `rmcp` und `schemars`; laut Meldung keine bereinigte
   Version). Dafür steht eine begründete Ausnahme in `deny.toml`: Es ist keine Sicherheitslücke gemeldet,
   der eigene Code verwendet `chrono` nicht, und ein Ersatz liegt bei den genannten Crates. Die Ausnahme
   entfällt, sobald sie umgestellt haben. Kein Lizenzthema.
4. **`boringtun`: Lizenztext fehlt im Crate.** Den BSD-3-Clause-Text mit Copyright-Vermerk aus dem
   Quell-Repository (<https://github.com/cloudflare/boringtun>, Stand des veröffentlichten Crates:
   Commit `051c9d47`) übernehmen. Die dortige Lizenzdatei wurde in diesem Durchgang nicht eingesehen.
5. **`about.toml` und Vorlage für die Drittanbieter-Übersicht** sind noch anzulegen, einschließlich der
   in Abschnitt 4 genannten Ergänzungen; anschließend Einbindung in App, CLI/Server und Release-Ablauf.
   Bis dahin führt `THIRD_PARTY_NOTICES.md` nur die direkten Abhängigkeiten auf.
6. **Rust-Standardbibliothek.** Sie wird statisch in jedes Binary gelinkt (MIT OR Apache-2.0 samt
   einiger Drittanteile) und von `cargo-about` nicht erfasst. Ein entsprechender Abschnitt in der
   Übersicht ist zu ergänzen; die genauen Texte wurden nicht geprüft.
7. **Marke „WireGuard":** Freigabe der Formulierungen beim Markeninhaber anfragen, endgültigen
   Produktnamen daraufhin prüfen (Abschnitt 5).
8. **Weitere Zielplattformen.** Gesondert ausgewertet wurden nur die drei oben genannten Ziele. Für
   weitere Android-ABIs, ARM-Linux oder `x86_64-unknown-linux-musl` ist `deny.toml` zu ergänzen. Der
   plattformübergreifende Gesamtgraph (415 Pakete) enthält über das oben Genannte hinaus keine weitere
   Lizenz außerhalb der Erlaubnisliste.
9. **Docker-Image.** `deploy/Dockerfile` baut auf `debian:bookworm-slim` auf. Die Lizenzen der darin
   enthaltenen Programme und Bibliotheken (glibc, coreutils …) sind nicht Teil dieser Prüfung; sie
   liegen im Image unter `/usr/share/doc`. Ein Image ohne fremde Programme würde die Pflichten bei
   einer Veröffentlichung des Images kleiner halten.
10. **Android- und Flutter-Seite.** Die Dart-Pakete sind geprüft (Abschnitt 6). Offen sind die Lizenzen
    der Gradle-/AndroidX-Bibliotheken und der Bestandteile der Flutter-Engine sowie Google ML Kit, falls
    der QR-Scanner hinzukommt.
11. **Geplante Rust-Abhängigkeiten** (Abschnitt 6) nach Aufnahme erneut prüfen; für `webpki-roots` wäre
    die Erlaubnisliste zu erweitern.
12. **Rechtliche Durchsicht** vor der ersten öffentlichen Version, insbesondere zu Marke und
    Store-Verteilung.
