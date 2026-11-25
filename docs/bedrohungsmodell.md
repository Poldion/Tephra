# Bedrohungsmodell

Was Tephra schützt, vor wem, und wo der Schutz endet. Dieses Dokument beschreibt den Code, wie er heute ist – die
Kommandozeile `tephra` und den Sync-Server `tephra-server`. Englische Fassung: [threat-model.md](threat-model.md).

Der Entwurf wurde noch von niemandem außerhalb des Projekts geprüft. Er verwendet ausschließlich etablierte Verfahren
(Argon2id, HKDF-SHA256, XChaCha20-Poly1305) und keine eigene Kryptografie, aber das ersetzt keine Durchsicht.

## Was geschützt wird

| Schutzgut | Wo es liegt |
|---|---|
| Passwörter der Identitäten, private SSH-Schlüssel und ihre Passphrasen, private und vorab geteilte VPN-Schlüssel, Proxy-Passwörter | Im Tresor, verschlüsselt |
| Hostliste: Namen, Adressen, Benutzernamen, Tags, Snippets, bekannte Hostschlüssel | Im Tresor, verschlüsselt |
| Master-Passwort und Wiederherstellungsschlüssel | Im Kopf und auf Papier; nirgends gespeichert |
| Vault Key | Im Arbeitsspeicher, solange der Tresor entsperrt ist; auf der Festplatte und beim Server nur verpackt |
| Sitzungstokens des Abgleichs, MCP-Clients und -Freigaben, MCP-Protokoll | Im Tresor eines Geräts, verschlüsselt, nie abgeglichen |
| Token eines MCP-Clients | Wird einmal angezeigt; der Tresor behält nur den SHA-256. Der Klartext steht in der Konfiguration des MCP-Clients |
| Die Hosts selbst | Erreichbar für jeden, der die Zugangsdaten hat – oder für ein Sprachmodell im Rahmen seiner Freigaben |

## Wie der Tresor aufgebaut ist

```
Master-Passwort ──Argon2id (64 MiB, 3 Durchläufe, Salt je Tresor)──► Master Key
   Master Key ──HKDF-SHA256──► Auth Key   (weist dem Sync-Server die Kenntnis des Passworts nach)
   Master Key ──HKDF-SHA256──► KEK        (verpackt den Vault Key, verlässt das Gerät nie)
   Vault Key (zufällig, 256 Bit)           verschlüsselt jeden Datensatz
   Recovery Key (zufällig, 256 Bit)        verpackt den Vault Key ein zweites Mal
```

- Jeder Datensatz wird mit XChaCha20-Poly1305 unter dem Vault Key und einer zufälligen Nonce verschlüsselt. Die
  Kennung des Tresors und die des Datensatzes werden mit authentifiziert; ein Chiffrat lässt sich also weder einem
  anderen Datensatz noch einem anderen Tresor unterschieben.
- Vor dem Verschlüsseln wird der Klartext aufgefüllt: auf mindestens 256 Byte, dann auf die nächste Zweierpotenz bis
  64 KiB, darüber in Schritten von 16 KiB. Die Art eines Datensatzes steht im Chiffrat.
- Der Kopf des Tresors (Salt, Ableitungsparameter, die beiden verpackten Fassungen des Vault Keys) ist nicht
  verschlüsselt, aber mit dem Vault Key authentifiziert. Ein Gerät lehnt einen Kopf ab, der nicht mit seinem Vault Key
  erzeugt wurde, ebenso Ableitungsparameter unter einem festen Minimum (19 MiB, 2 Durchläufe) oder über einem Maximum.
- Der Kopf trägt einen Zähler, der mit jedem Wechsel des Master-Passworts steigt und vom Echtheitsnachweis abgedeckt
  ist. Ein echter, aber älterer Kopf – einer, der sich noch mit einem früheren, womöglich bekannt gewordenen Passwort
  öffnen lässt – wird nicht wieder angenommen.
- Kennungen von Datensätzen sind zufällig. Datensätze, die es je Name nur einmal geben darf, etwa der bekannte
  Schlüssel eines Hosts, bekommen eine Kennung, die mit dem Vault Key aus diesem Namen abgeleitet wird. Ohne Vault Key
  kann niemand erkennen, welcher Name hinter einer Kennung steht.
- Ein Datensatz, dessen Zeitstempel mehr als 24 Stunden in der Zukunft liegt, wird abgelehnt. Sonst könnte ein Gerät
  mit falscher Uhr jeden späteren Konflikt für sich entscheiden.
- Die Tresordatei enthält genau die Chiffrate, die auch der Sync-Server bekommt. Entsperren entschlüsselt alle
  Datensätze in den Arbeitsspeicher, Sperren verwirft sie. Schlüssel und geheime Zeichenfolgen werden beim Verwerfen
  überschrieben.
- Jeder Befehl von `tephra` entsperrt den Tresor für seine eigene Dauer. `tephra daemon` hält ihn entsperrt, bis er
  beendet wird.
- Eine Sicherung aus `tephra export` enthält keine Schlüssel, die als „nur dieses Gerät“ gekennzeichnet sind, keine
  MCP-Freigaben und keine Sitzung beim Sync-Server.

**Der Vault Key wird nie ersetzt.** Ein Wechsel des Master-Passworts oder das Entfernen eines Geräts verpackt
denselben Schlüssel neu. Wer den Vault Key einmal hatte – ein entferntes Gerät, jemand, der ein altes Passwort kannte
und eine Kopie des Tresors aufbewahrt hat –, kann weiterhin jedes Chiffrat entschlüsseln, das er in die Hand bekommt.

## Angreifer

### Ein kompromittierter Sync-Server

Jemand kontrolliert den Server, seine Datenbank oder seine Sicherungen.

**Er sieht:**

- den Benutzernamen des Kontos, die Namen der Geräte, wann jedes angelegt und zuletzt gesehen wurde, und – zusammen
  mit dem Reverse-Proxy – die Netzwerkadressen, von denen die Geräte kommen
- den Kopf des Tresors: Salt, Ableitungsparameter und den verpackten Vault Key
- einen Hash des Auth Keys
- je Datensatz: seine Kennung, eine Revisionsnummer, ob er ein Löschmarker ist, und seine aufgefüllte Größe – und
  damit, wie viele Datensätze es gibt und wann sich welcher ändert

**Er kann nicht:**

- einen Datensatz lesen oder dessen Art, eine Hostadresse, einen Benutzernamen oder einen Schlüssel erfahren
- einen Datensatz fälschen oder verändern oder ein Chiffrat einem anderen Datensatz unterschieben: Das scheitert auf
  dem Gerät an der Echtheitsprüfung
- einen eigenen Kopf ausgeben oder einen älteren echten: Der erste scheitert an der Echtheitsprüfung, der zweite am
  Zähler
- einen neueren Wert auf einem Gerät überschreiben, das ihn schon hat: Zusammengeführt wird auf dem Gerät feldweise
  nach Zeitstempel; eine alte Fassung, die erneut geschickt wird, ändert dort nichts
- mit einem Abzug der Datenbank Sitzungen übernehmen: Sitzungstokens und Einladungscodes sind nur als Hash gespeichert

**Er kann, ohne dass ein Gerät es bemerkt:**

- **Änderungen zurückhalten.** Dazu gehören Löschmarker: Ein Host oder Schlüssel, den du auf einem Gerät gelöscht
  hast, bleibt dann auf den anderen bestehen.
- **einem neuen Gerät einen älteren Stand liefern.** Ältere Chiffrate sind echt; ein Gerät, das neu beitritt, hat
  nichts, womit es sie vergleichen könnte.
- **die Annahme eines Hochladens vortäuschen** und es verwerfen. Das hochladende Gerät hält die Änderung für
  gespeichert.
- Änderungen nur manchen Geräten ausliefern, sie verzögern oder alles löschen.

Einen signierten Nachweis über den Gesamtstand des Tresors, an dem das auffiele, gibt es nicht. Halte eine Sicherung
vor (`tephra export`).

**Außerdem kann er:**

- das **Master-Passwort offline erraten.** Der Kopf enthält alles, um einen Versuch zu prüfen; jeder Versuch kostet
  einen Argon2id-Durchlauf. Dagegen schützt nur ein langes, einmaliges Master-Passwort. Dasselbe gilt für den
  gespeicherten Hash des Auth Keys.
- einem neuen Gerät schwache Ableitungsparameter nennen: Das Gerät lehnt sie ab.
- die Nutzung beobachten: wann du abgleichst, wie viele Datensätze du hast, welche Größen sich ändern.

Der Server begrenzt Fehlversuche bei der Anmeldung je Benutzername und Absender; korrekte Anmeldungen werden nie
gebremst, ein Dritter kann dich also nicht mit falschen Passwörtern aussperren. Hinter einem Reverse-Proxy braucht es
dafür `--behind-proxy`, sonst scheinen alle Anfragen vom Proxy zu kommen.

### Ein gestohlenes oder verlorenes Gerät

**Tresor gesperrt** (kein `tephra daemon` läuft): Der Dieb hat die Tresordatei und kann das Master-Passwort offline
erraten, wie oben. Ohne Master-Passwort oder Wiederherstellungsschlüssel ist der Inhalt nicht lesbar. Schlüssel mit
`--local-only` gibt es nur in dieser Datei.

**Tresor entsperrt** (`tephra daemon` läuft, Sitzung nicht gesperrt): Die Schlüssel liegen im Arbeitsspeicher, und der
Dienst bedient MCP-Clients. Das ist zu behandeln wie ein entsperrter Passwortmanager.

Was zu tun ist: das Gerät mit `tephra sync remove-device` entfernen. Seine Sitzungstokens gelten sofort nicht mehr, und
es bekommt keine weiteren Änderungen. **Was es bereits abgeholt hat, bleibt dort, und der Vault Key wird nicht
ersetzt.** War das Master-Passwort schwach oder könnte es bekannt sein, müssen die Passwörter und Schlüssel der Hosts
selbst erneuert werden – das Master-Passwort zu ändern hilft nicht.

**Entsperren ohne Master-Passwort** (in der App eingeschaltet): Unter Windows liegt der Vault Key, verschlüsselt mit
dem Windows-Konto, neben dem Tresor. Wer die Platte ausbaut, braucht zusätzlich das Kennwort dieses Kontos; wer am
angemeldeten Konto sitzt oder darin Programme ausführt, kommt an den Vault Key. Unter Android liegt er verschlüsselt
im privaten Ordner der App; der Schlüssel dazu steckt im Keystore des Telefons, verlässt ihn nicht und arbeitet nur,
solange das Telefon entsperrt ist. Auf diesen Geräten schützt dann die Anmeldung am Gerät den Tresor, nicht mehr das
Master-Passwort.

Noch nicht vorhanden: Biometrie oder Windows Hello beim Entsperren und automatisches Sperren nach einer Zeit ohne
Nutzung.

### Ein bösartiger oder übernommener SSH-Host

- **Vortäuschen.** Hostschlüsseln wird beim ersten Verbinden vertraut, sie liegen im Tresor. Legt ein Host später
  einen anderen Schlüssel vor, hält die Verbindung an; `tephra` fährt erst fort, nachdem das Wort `geändert` eingetippt
  wurde. Über MCP wird ein geänderter Schlüssel immer abgelehnt, ein unbekannter braucht deine Bestätigung. Die erste
  Verbindung zu einem Host ist so vertrauenswürdig wie das Netz, über das sie läuft – vergleiche den Fingerabdruck.
- **Bekannte Schlüssel gelten auf allen deinen Geräten.** Ein Schlüssel, dem auf einem Gerät vertraut wurde, gilt auf
  allen. Zwei Geräte, die demselben Host unabhängig voneinander vertrauen, landen beim selben Datensatz; weichen ihre
  Schlüssel voneinander ab, gewinnt der später gespeicherte, und das andere Gerät sieht beim nächsten Verbinden
  „Schlüssel geändert“.
- **Was der Host bekommt.** Bei der Anmeldung mit Passwort erhält der Host das Passwort im Klartext; so funktioniert
  die Passwortanmeldung von SSH. Für Hosts, denen du nicht voll vertraust, sind Schlüssel die bessere Wahl.
  Agent-Weiterleitung gibt es nicht, ein Host kann deine Schlüssel also nicht für weitere Verbindungen benutzen.
- **Terminalausgabe.** `tephra ssh` reicht die Ausgabe des Hosts unverändert an dein Terminal weiter, einschließlich
  Steuersequenzen, wie jeder SSH-Client.
- **Anweisungen an ein Sprachmodell.** Ein Host kann Text in Befehlsausgaben oder Dateien legen, der ein Modell lenken
  soll. Die Beschreibungen der Werkzeuge kennzeichnen jede Ausgabe als nicht vertrauenswürdige Daten, Steuersequenzen
  werden aus Sitzungsausgaben entfernt, und die Zugangsdaten, mit denen der Host erreicht wird, werden geschwärzt.
  Immun macht das ein Modell nicht. Was ein fehlgeleitetes Modell anrichten kann, entscheidet die Zugriffsstufe.

### Ein fehlgeleitetes Sprachmodell

Das Modell gilt als nicht vertrauenswürdig: Es kann sich irren, oder es folgt Anweisungen, die es in der Ausgabe eines
Hosts gefunden hat.

**Vorhanden:**

- Ohne Freigabe ist nichts erlaubt. Freigaben gelten je MCP-Client und je Host, liegen nur auf diesem Gerät und werden
  nie abgeglichen. Ein Host, der einem Client nicht freigegeben ist, verhält sich genau wie einer, den es nicht gibt;
  der Versuch wird protokolliert.
- Kein Werkzeug gibt je Zugangsdaten zurück.
- `read-only`: eine feste Liste anzeigender Programme, jedes mit einer festen Liste von Optionen, die nichts
  verändern, nichts starten und nicht endlos laufen. Ein Befehl wird in Wörter zerlegt und Wort für Wort geprüft.
  Abgelehnt werden Verkettungen, Umleitungen und Ersetzungen (`; & | > < $ ( ) { }`, Backticks, Backslashes,
  Zeilenumbrüche), Anführungszeichen, Platzhalter und `~`, Eingabedaten, interaktive Sitzungen, das Schreiben von
  Dateien und Pfade, die nach Zugangsdaten aussehen (`.ssh`, `shadow`, `.env`, `.key`, `/proc/` und Ähnliches).
- `confirm`: Jeder Befehl, jede Sitzung und jeder Dateizugriff erscheint im Terminal von `tephra daemon` und braucht ein
  Ja – außer der Befehl passt auf ein Muster, das du vorab erlaubt hast. Die Rückfrage zeigt den Befehl und seine
  Eingabedaten, bei einer Datei Pfad, Größe, Prüfsumme und den Anfang des Inhalts. Steuerzeichen, unsichtbare Zeichen
  und solche, die die Leserichtung ändern, erscheinen als sichtbare Kürzel, damit die Anzeige nichts anderes zeigen
  kann, als ausgeführt wird. Was getippt wurde, bevor die Frage zu sehen war, zählt nicht als Antwort; keine Antwort
  binnen zwei Minuten gilt als Nein.
- `auto`: Befehle laufen, sofern sie nicht auf der eingebauten Sperrliste stehen oder auf eines deiner eigenen Muster
  passen.
- Lesen und Schreiben von Dateien sind getrennte Rechte.
- Grenzen: Aufrufe pro Minute und Client, parallele Sitzungen, Laufzeit, Größe von Ausgaben und Dateien.
- **Protokoll.** Jede Aktion wird protokolliert, bevor sie läuft; lässt sich der Eintrag nicht schreiben, läuft sie
  nicht. Ein zweiter Eintrag hält ihr Ende fest. Abgelehnte Aufrufe werden ebenfalls protokolliert.
- **Not-Aus und Entzug.** `tephra mcp disable`, das Entfernen eines Clients, der Entzug eines Hosts oder das Sperren des
  Tresors beenden laufende Sitzungen und Verbindungen binnen weniger Sekunden, nicht erst beim nächsten Aufruf.
- **Schwärzung.** Ausgaben werden um die Zugangsdaten bereinigt, die beim Verbinden mit diesem Host im Spiel sind –
  seine eigenen, die seiner Jump-Hosts, seines Proxys und seines VPN-Profils. Lässt sich die Liste nicht
  zusammenstellen, wird nichts ausgeführt.
- **Brücke und Dienst** weisen einander nach, dass sie das Token des Clients kennen; das Token selbst wird nicht
  übertragen. Ein Programm, das nur den Namen des Kanals besetzt, erfährt nichts Verwertbares und bekommt keine
  MCP-Daten.

**Nicht geschützt:**

- **`auto` ist keine Sandbox.** Eine Sperrliste fängt ein `rm -rf /` ab, das auf die naheliegende Art getippt wird,
  und nichts, das an ihr vorbei soll. In einer interaktiven Sitzung wird sie zeilenweise geprüft und lässt sich schon
  durch Aufteilen einer Eingabe umgehen. Sie verhindert Versehen, keinen Missbrauch.
- **`read-only` heißt „verändert nichts“, nicht „verrät nichts“.** Es umfasst alles, was der SSH-Benutzer lesen darf.
  Dass Pfade abgelehnt werden, die nach Zugangsdaten aussehen, ist eine Hürde gegen Versehen, kein Schutz vor einem
  Modell, das gezielt sucht. Gib Hosts über einen SSH-Benutzer mit wenigen Rechten frei.
- **Eigene Muster mit `--allow` ersetzen die eingebaute Prüfung von `read-only` vollständig** – einschließlich der
  Prüfung einzelner Optionen und der Pfadsperre. Es bleiben nur die Verbote von Verkettungen, Umleitungen und
  Eingabedaten. `*` passt auf alles.
- **Eine bestätigte interaktive Sitzung ist offen.** Sie wird einmal beim Öffnen bestätigt; was danach hineingetippt
  wird, erscheint nicht zur Bestätigung. Das steht in der Rückfrage.
- **Du bestätigst, was du siehst.** Lange Befehle und Eingabedaten werden bis 2.000 Zeichen gezeigt, Dateiinhalte bis
  400; der Rest wird angekündigt, nicht angezeigt. „Für diese Sitzung“ gilt dann nur für exakt dieselbe Aktion –
  denselben Befehl mit derselben Eingabe oder dieselbe Datei mit demselben Inhalt.
- **Die Schwärzung findet nur wörtliche Treffer** ab sechs Zeichen: keine kodierten Formen (Base64, Hex), kein
  Geheimnis, das auf zwei Abrufe verteilt ankommt, und keine Geheimnisse, die nur auf dem Host liegen.
- **Alles, was das Modell liest, verlässt deinen Rechner** und geht an den Betreiber des Modells.

### Ein anderer Benutzer am selben Rechner

- Der Kanal zwischen `tephra mcp serve` und `tephra daemon` ist eine Named Pipe, die entfernte Clients abweist
  (Windows), oder ein Unix-Socket in einem Verzeichnis, das nur sein Besitzer betreten darf (Linux). Eine Verbindung
  wird erst nach dem oben beschriebenen gegenseitigen Nachweis bedient.
- Die Tresordatei ist unabhängig von Dateirechten verschlüsselt.
- **Portweiterleitungen stehen dem ganzen Rechner offen.** Solange `tephra forward` läuft, kann jedes Programm und jeder
  Benutzer des Rechners den weitergeleiteten Port oder den SOCKS5-Proxy benutzen; eine Anmeldung gibt es nicht. Sie
  lauschen ohne andere Angabe auf 127.0.0.1; `--bind` mit einer anderen Adresse öffnet sie zusätzlich für das lokale
  Netz.
- Nicht geschützt: ein Administrator oder root, der den Speicher des laufenden Dienstes lesen kann. Ein Token in
  `TEPHRA_MCP_TOKEN` oder ein Passwort in `TEPHRA_PASSWORD` sieht jeder, der die Umgebung dieses Prozesses oder die
  Datei lesen kann, in der es eingetragen ist.

## Ausdrücklich außerhalb des Schutzumfangs

- **Wer die Veröffentlichungen auf GitHub kontrolliert.** Die App sucht dort einmal am Tag nach Aktualisierungen
  (abschaltbar) und installiert eine nur, wenn du zustimmst. Die Prüfsumme, die sie prüft, stammt vom selben Ort wie
  die Datei: Sie schützt vor einem beschädigten Download, nicht vor einer gefälschten Veröffentlichung. Unter Android
  lehnt das System zusätzlich eine App ab, die nicht mit demselben Schlüssel signiert ist; die Windows-Programme und
  das AppImage sind nicht signiert.
- **Schadsoftware auf dem Gerät** oder jedes Programm, das unter deinem eigenen Konto läuft, während der Tresor
  entsperrt ist. Es kann Speicher lesen, das Master-Passwort beim Tippen mitschneiden, das MCP-Token aus der
  Konfiguration des Clients lesen und mit dem Dienst sprechen.
- **Ein entferntes Gerät behält, was es hat.** Entfernen ist kein Fernlöschen, und der Vault Key wird nicht erneuert.
- **Vollständigkeit und Aktualität dessen, was der Sync-Server liefert.** Er kann Daten verlieren, zurückhalten oder
  ältere erneut ausliefern. Der lokale Tresor funktioniert ohne ihn weiter.
- **Metadaten beim Sync-Server:** Größen in Stufen, Zeitpunkte, Löschmarker, Anzahl der Datensätze, Gerätenamen.
- **Schwache Master-Passwörter.** Die Mindestlänge beträgt acht Zeichen; das ist eine Untergrenze, keine Empfehlung.
- **Der Transport zum Sync-Server** ist das TLS, das dein Reverse-Proxy bereitstellt; der Client prüft das Zertifikat
  gegen den Zertifikatsspeicher des Betriebssystems. Clients lehnen einfaches `http://` ab, außer für `localhost` –
  und außer du erlaubst es für einen Server im eigenen Netz ausdrücklich (`--allow-http`, in der App „Unverschlüsselt
  verbinden“). Ohne TLS bleiben die Inhalte des Tresors verschlüsselt, Sitzungstokens und Auth Key aber nicht: Wer
  mitliest, kann sich als Gerät anmelden, die Chiffrate abholen und Master-Passwörter daran durchprobieren, Geräte
  entfernen und das Konto löschen. Die Entscheidung ist deshalb nur für ein Netz gedacht, dem du vertraust.
- **Seitenkanäle und forensische Spuren** in Arbeitsspeicher, Auslagerungsdatei oder Absturzabbildern des
  Betriebssystems.
