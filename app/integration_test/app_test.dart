// Fährt die echte App gegen den SSH-Testserver aus `tephra-ssh`:
//
//   cargo run -p tephra-ssh --features testing --example test_server
//   TEPHRA_VAULT=<leerer Pfad> flutter test integration_test -d windows \
//       --dart-define=TEPHRA_TEST_PORT=<Port des Testservers>
//
// Mit `--dart-define=TEPHRA_TEST_SHOTS=<Ordner>` legt der Test Bildschirmfotos ab.
// Mit `--dart-define=TEPHRA_TEST_UPDATE=true` lädt er zusätzlich eine echte
// Aktualisierung von GitHub und prüft ihre Prüfsumme (braucht Netz, rund 20 MB).
//
// Mit `--dart-define=TEPHRA_TEST_SYNC=<Adresse>` legt er am Ende ein Konto „tester“ auf einem
// Sync-Server an (`tephra-server --registration open`) und lädt den Tresor hoch. Ein zweiter Lauf
// mit leerem Tresorpfad und `--dart-define=TEPHRA_TEST_JOIN=<Adresse>` spielt das zweite Gerät:
// Er holt den Tresor dieses Kontos ab und prüft, dass der Host angekommen ist. Ist die Adresse
// unverschlüsselt und nicht der eigene Rechner (etwa `http://127.0.0.2:8790`), prüft der Test
// zusätzlich, dass die App sie erst mit dem Haken „Unverschlüsselt verbinden“ annimmt.
//
// Mit `--dart-define=TEPHRA_TEST_CLI=<Pfad zu tephra>` schaltet er außerdem den MCP-Dienst der App ein,
// legt einen Client an, gibt ihm den Host frei und startet die echte Brücke `tephra mcp serve`:
// Ein Befehl wird im Dialog der App bestätigt, ein zweiter abgelehnt.
//
// Mit `--dart-define=TEPHRA_TEST_SSHD=<Host>:<Port>:<Benutzer>:<Passwort>` prüft er „Sitzung halten“ gegen einen
// echten SSH-Server mit tmux: Die Verbindung läuft über einen Zwischenverteiler im Test, der sie kappt und
// einfriert; die App muss sich jedes Mal wieder an dieselbe tmux-Sitzung hängen.
//
// Auf einem Android-Emulator erreicht die App den Testserver des Rechners
// unter 10.0.2.2; der Tresor liegt im Datenordner der App und muss fehlen
// (App vorher deinstallieren):
//
//   flutter test integration_test -d emulator-5554 \
//       --dart-define=TEPHRA_TEST_HOST=10.0.2.2 --dart-define=TEPHRA_TEST_PORT=<Port>
//
// `TEPHRA_TEST_SHOTS=app` legt die Bildschirmfotos dort im Ordner `shots` neben dem
// Tresor ab.

import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:tephra/main.dart';
import 'package:tephra/src/device_unlock.dart';
import 'package:tephra/src/mcp.dart';
import 'package:tephra/src/paths.dart';
import 'package:tephra/src/rust/api/app.dart';
import 'package:tephra/src/rust/api/files.dart';
import 'package:tephra/src/rust/api/mcp.dart';
import 'package:tephra/src/rust/api/sync.dart' show syncIsUnencrypted;
import 'package:tephra/src/rust/frb_generated.dart';
import 'package:tephra/src/session.dart';
import 'package:tephra/src/settings.dart';
import 'package:tephra/src/text.dart';
import 'package:tephra/src/ui/device_unlock_ui.dart';
import 'package:tephra/src/ui/sync_ui.dart' show plainHttpRefused;
import 'package:tephra/src/ui/terminal_page.dart';
import 'package:tephra/src/update.dart';
import 'package:xterm/xterm.dart' show TerminalView;

import 'test_key.dart';

const _host = String.fromEnvironment('TEPHRA_TEST_HOST', defaultValue: '127.0.0.1');
const _port = int.fromEnvironment('TEPHRA_TEST_PORT');
const _shots = String.fromEnvironment('TEPHRA_TEST_SHOTS');
const _update = bool.fromEnvironment('TEPHRA_TEST_UPDATE');
const _sync = String.fromEnvironment('TEPHRA_TEST_SYNC');
const _join = String.fromEnvironment('TEPHRA_TEST_JOIN');
const _cli = String.fromEnvironment('TEPHRA_TEST_CLI');
const _sshd = String.fromEnvironment('TEPHRA_TEST_SSHD');
const _account = 'tester';
const _password = 'ein langes Master-Passwort';

Future<void> _shot(WidgetTester tester, String name) async {
  if (_shots.isEmpty) return;
  await tester.pump();
  final view = tester.binding.renderViews.first;
  final layer = view.debugLayer! as OffsetLayer;
  final image = await layer.toImage(view.paintBounds);
  final data = await image.toByteData(format: ui.ImageByteFormat.png);
  final dir = Directory(_shots == 'app' ? '${File(vaultPath).parent.path}/shots' : _shots)..createSync(recursive: true);
  File('${dir.path}/$name.png').writeAsBytesSync(data!.buffer.asUint8List());
}

/// Wartet, bis die Bedingung gilt; echte Zeit, denn der Kern arbeitet nebenher.
Future<void> _until(WidgetTester tester, bool Function() condition, String what) async {
  final deadline = DateTime.now().add(const Duration(seconds: 40));
  while (!condition()) {
    if (DateTime.now().isAfter(deadline)) fail('Zeitüberschreitung: $what');
    await tester.pump(const Duration(milliseconds: 50));
  }
  await tester.pump(const Duration(milliseconds: 300));
}

Future<void> _appears(WidgetTester tester, Finder finder) =>
    _until(tester, () => finder.evaluate().isNotEmpty, finder.toString());

Future<void> _tap(WidgetTester tester, Finder finder) async {
  await tester.tap(finder);
  await tester.pump(const Duration(milliseconds: 400));
}

/// Gibt Text in ein Feld ein. Der Klick davor holt den Fokus zurück, falls er
/// inzwischen auf einem Knopf lag.
Future<void> _type(WidgetTester tester, Finder finder, String text) async {
  await tester.ensureVisible(finder);
  await tester.pump(const Duration(milliseconds: 100));
  await tester.tap(finder);
  await tester.pump(const Duration(milliseconds: 100));
  await tester.enterText(finder, text);
  await tester.pump(const Duration(milliseconds: 100));
}

/// Meldet den Abschnitt, der beginnt. Bricht der Lauf ab, zeigt die Ausgabe, wo.
void _step(String name) => debugPrint('Abschnitt: $name');

String _screen(TerminalSession session) => session.terminal.buffer.getText();

/// Leitet Verbindungen an einen SSH-Server weiter und kann sie kappen oder einfrieren, wie ein Funkloch oder
/// ein Wechsel vom WLAN ins Mobilnetz.
class _Relay {
  _Relay._(this._server, this._host, this._port) {
    _server.listen((client) async {
      try {
        final server = await Socket.connect(_host, _port);
        final pair = (client, server, <StreamSubscription<Uint8List>>[]);
        pair.$3.add(client.listen(server.add, onDone: server.destroy, onError: (Object _) => server.destroy()));
        pair.$3.add(server.listen(client.add, onDone: client.destroy, onError: (Object _) => client.destroy()));
        _pairs.add(pair);
      } catch (_) {
        client.destroy();
      }
    });
  }

  static Future<_Relay> start(String host, int port) async =>
      _Relay._(await ServerSocket.bind(InternetAddress.loopbackIPv4, 0), host, port);

  final ServerSocket _server;
  final String _host;
  final int _port;
  final _pairs = <(Socket, Socket, List<StreamSubscription<Uint8List>>)>[];

  int get port => _server.port;

  /// Bricht alle Verbindungen hart ab.
  void cut() {
    for (final (client, server, _) in _pairs) {
      client.destroy();
      server.destroy();
    }
    _pairs.clear();
  }

  /// Hält alle bestehenden Verbindungen an, ohne sie zu schließen: Es kommt einfach nichts mehr an.
  void freeze() {
    for (final (_, _, subscriptions) in _pairs) {
      for (final subscription in subscriptions) {
        subscription.pause();
      }
    }
  }

  Future<void> close() async {
    cut();
    await _server.close();
  }
}

/// „Sitzung halten“: Shell in tmux, Abbruch, Wiederverbinden an dieselbe Sitzung.
Future<void> _keepSession(WidgetTester tester) async {
  _step('Sitzung halten');
  final [host, port, user, password] = _sshd.split(':');
  final relay = await _Relay.start(host, int.parse(port));
  final identity = saveIdentity(
    input: IdentityInput(label: 'tmux', username: user, method: 'password', password: password),
  );
  final hostId = saveHost(
    input: HostInput(
      label: 'Mit tmux',
      address: '127.0.0.1',
      port: relay.port,
      identityId: identity,
      jumpHostIds: const [],
      tags: const [],
      notes: '',
      favorite: false,
      startupCommand: '',
      keepSession: true,
    ),
  );
  final view = listHosts().firstWhere((h) => h.id == hostId);
  expect(view.keepSession, isTrue);
  final session = TerminalSession(view, slot: 3);
  try {
    await _until(tester, () => session.prompt is SessionEvent_HostKey, 'Hostschlüssel');
    session.answerHostKey(true);
    await _until(tester, () => session.state == SessionState.connected, 'Verbindung');
    session.terminal.textInput('echo "tmux: #\$(tmux display-message -p \'#S\')" "marke-\$((6*7))"\r');
    await _until(tester, () => _screen(session).contains('tmux: #tephra-3 marke-42'), 'Ausgabe in tmux');
    expect(session.terminal.isUsingAltBuffer, isTrue, reason: 'tmux zeichnet im Vollbild');

    _step('Sitzung halten: Verbindung gekappt');
    relay.cut();
    await _until(tester, () => session.reconnecting, 'neuer Versuch');
    await _until(tester, () => session.state == SessionState.connected, 'wieder verbunden');
    // Dieselbe Sitzung: tmux zeichnet die Ausgabe von vorhin neu.
    await _until(tester, () => _screen(session).contains('marke-42'), 'alte Ausgabe nach dem Abbruch');
    expect(_screen(session), isNot(contains(t('Verbindung getrennt', 'Connection lost'))));

    _step('Sitzung halten: Verbindung eingefroren');
    relay.freeze();
    final started = DateTime.now();
    await session.checkConnection();
    expect(session.reconnecting, isTrue, reason: 'die Prüfung bemerkt die tote Verbindung');
    expect(DateTime.now().difference(started), lessThan(const Duration(seconds: 10)));
    await _until(tester, () => session.state == SessionState.connected, 'wieder verbunden nach dem Einfrieren');
    await _until(tester, () => _screen(session).contains('marke-42'), 'alte Ausgabe nach dem Einfrieren');
    // Eine stehende Verbindung übersteht die Prüfung.
    await session.checkConnection();
    expect(session.state, SessionState.connected);

    _step('Sitzung halten: Shell beendet');
    session.terminal.textInput('exit\r');
    await _until(tester, () => session.state == SessionState.closed, 'Sitzungsende');
    await tester.pump(const Duration(seconds: 3));
    expect(session.state, SessionState.closed, reason: 'nach einem Exit wird nicht neu verbunden');
    expect(session.reconnecting, isFalse);
  } finally {
    session.close();
    await relay.close();
    deleteItem(id: hostId);
    deleteItem(id: identity);
  }
}

/// Wartet im Dialog des Abgleichs, bis er einen abgeschlossenen Abgleich mit `server` meldet.
Future<void> _syncDone(WidgetTester tester, String server) async {
  await _appears(tester, find.byKey(const Key('sync-connected')));
  await _appears(tester, find.byKey(const Key('sync-done')));
  await _appears(tester, find.text(t('Dieses Gerät', 'This device')));
  expect(find.byKey(const Key('sync-error')), findsNothing);
  expect(find.byKey(const Key('sync-unencrypted')), syncIsUnencrypted(server: server) ? findsOneWidget : findsNothing);
}

/// Eine unverschlüsselte Adresse nimmt die App erst an, wenn der Haken dafür gesetzt ist; für eine
/// verschlüsselte oder den eigenen Rechner erscheint er gar nicht.
Future<void> _allowPlainHttp(WidgetTester tester, String server, Key submit) async {
  final consent = find.byKey(const Key('sync-allow-http'));
  if (!syncIsUnencrypted(server: server)) {
    expect(consent, findsNothing);
    return;
  }
  await _appears(tester, consent);
  await _tap(tester, find.byKey(submit));
  await _appears(tester, find.text(plainHttpRefused));
  await _tap(tester, consent);
}

/// Das zweite Gerät: holt den Tresor eines bestehenden Kontos ab.
Future<void> _joinDevice(WidgetTester tester) async {
  _step('Tresor abholen');
  await _tap(tester, find.byKey(const Key('gate-join')));
  await _type(tester, find.byKey(const Key('join-server')), _join);
  await _type(tester, find.byKey(const Key('join-username')), _account);
  await _type(tester, find.byKey(const Key('join-password')), 'falsches Passwort');
  await _allowPlainHttp(tester, _join, const Key('join-submit'));
  expect(vaultExists(path: vaultPath), isFalse);
  await _tap(tester, find.byKey(const Key('join-submit')));
  await _until(
    tester,
    () => tester.widget<FilledButton>(find.byKey(const Key('join-submit'))).onPressed != null,
    'Ablehnung',
  );
  expect(isUnlocked(), isFalse);
  expect(vaultExists(path: vaultPath), isFalse, reason: 'ohne Anmeldung entsteht kein Tresor');
  await _type(tester, find.byKey(const Key('join-password')), _password);
  await _tap(tester, find.byKey(const Key('join-submit')));
  await _appears(tester, find.text('Testserver'));
  expect(isUnlocked(), isTrue);
  expect(listKeys(), isNotEmpty);
  expect(listIdentities().map((identity) => identity.label), contains('tester'));

  _step('Abgleich auf dem zweiten Gerät');
  await _tap(tester, find.byKey(const Key('sync-button')));
  await _syncDone(tester, _join);
  expect(find.byIcon(Icons.devices_other_rounded), findsWidgets, reason: 'das erste Gerät steht in der Liste');
  await _tap(tester, find.text(t('Schließen', 'Close')));

  // Der Tresor lässt sich mit dem Master-Passwort des Kontos sperren und wieder öffnen.
  await _tap(tester, find.byTooltip(t('Tresor sperren', 'Lock vault')));
  await _appears(tester, find.text(t('Tresor entsperren', 'Unlock vault')));
  await _type(tester, find.byType(TextField).first, _password);
  await _tap(tester, find.widgetWithText(FilledButton, t('Entsperren', 'Unlock')));
  await _appears(tester, find.text('Testserver'));
}

/// Schaltet im Dialog „Über Tephra“ das Entsperren ohne Passwort ein oder aus.
Future<void> _setDeviceUnlock(WidgetTester tester, bool on) async {
  await _tap(tester, find.byTooltip(t('Über $appName', 'About $appName')));
  await _appears(tester, find.byKey(const Key('device-unlock-switch')));
  await tester.ensureVisible(find.byKey(const Key('device-unlock-switch')));
  await _tap(tester, find.byKey(const Key('device-unlock-switch')));
  if (on) await _tap(tester, find.widgetWithText(FilledButton, t('Einschalten', 'Switch on')));
  await _until(tester, () => deviceUnlock.enabled == on, on ? 'eingeschaltet' : 'ausgeschaltet');
  Navigator.of(tester.element(find.byType(DeviceUnlockSection))).pop();
  await tester.pump(const Duration(milliseconds: 400));
}

/// Entsperren ohne Master-Passwort: per Knopf nach dem Sperren, von selbst beim Start der App.
Future<void> _deviceUnlock(WidgetTester tester) async {
  _step('ohne Passwort entsperren');
  await _setDeviceUnlock(tester, true);
  await _shot(tester, '13-ohne-passwort');

  // Nach dem Sperren geht der Tresor nicht von selbst auf, aber mit einem Knopfdruck.
  await _tap(tester, find.byTooltip(t('Tresor sperren', 'Lock vault')));
  await _appears(tester, find.byKey(const Key('gate-device-unlock')));
  await tester.pump(const Duration(seconds: 1));
  expect(isUnlocked(), isFalse);
  await _shot(tester, '13-sperre-ohne-passwort');
  await _tap(tester, find.byKey(const Key('gate-device-unlock')));
  await _appears(tester, find.text('Testserver'));
  expect(isUnlocked(), isTrue);

  // Beim Start der App geht er von selbst auf.
  await _tap(tester, find.byTooltip(t('Tresor sperren', 'Lock vault')));
  await _appears(tester, find.byKey(const Key('gate-device-unlock')));
  await tester.pumpWidget(const SizedBox());
  await tester.pumpWidget(const TerminalApp());
  await _appears(tester, find.text('Testserver'));
  expect(isUnlocked(), isTrue);

  // Ausgeschaltet braucht es wieder das Master-Passwort.
  await _setDeviceUnlock(tester, false);
  await _tap(tester, find.byTooltip(t('Tresor sperren', 'Lock vault')));
  await _appears(tester, find.text(t('Tresor entsperren', 'Unlock vault')));
  expect(find.byKey(const Key('gate-device-unlock')), findsNothing);
  await _type(tester, find.byType(TextField).first, _password);
  await _tap(tester, find.widgetWithText(FilledButton, t('Entsperren', 'Unlock')));
  await _appears(tester, find.text('Testserver'));
}

/// Die Brücke `tephra mcp serve`, wie ein MCP-Client sie startet: JSON-RPC zeilenweise über Ein- und Ausgabe.
class _Bridge {
  _Bridge._(this._process) {
    _process.stdout.transform(utf8.decoder).transform(const LineSplitter()).listen((line) {
      final message = jsonDecode(line) as Map<String, dynamic>;
      _answers.remove(message['id'])?.complete(message);
    });
    _process.stderr.transform(utf8.decoder).listen((text) => debugPrint('Brücke: ${text.trim()}'));
  }

  static Future<_Bridge> start(String token) async => _Bridge._(
    await Process.start(_cli, ['mcp', 'serve'], environment: {'TEPHRA_MCP_TOKEN': token, 'TEPHRA_VAULT': vaultPath}),
  );

  final Process _process;
  final _answers = <int, Completer<Map<String, dynamic>>>{};
  var _next = 0;

  void _send(Map<String, dynamic> message) => _process.stdin.writeln(jsonEncode({'jsonrpc': '2.0', ...message}));

  Future<Map<String, dynamic>> call(String method, [Map<String, dynamic>? params]) {
    final id = ++_next;
    final answer = _answers[id] = Completer();
    _send({'id': id, 'method': method, 'params': ?params});
    return answer.future;
  }

  Future<void> initialize() async {
    final reply = await call('initialize', {
      'protocolVersion': '2025-06-18',
      'capabilities': <String, dynamic>{},
      'clientInfo': {'name': 'app-test', 'version': '1'},
    });
    expect(reply['result'], isNotNull, reason: '$reply');
    _send({'method': 'notifications/initialized'});
  }

  /// Ruft ein Werkzeug auf; zurück kommen sein Text und ob es ein Fehler war.
  Future<(String, bool)> tool(String name, Map<String, dynamic> arguments) async {
    final reply = await call('tools/call', {'name': name, 'arguments': arguments});
    final result = reply['result'] as Map<String, dynamic>? ?? {};
    final text = [for (final part in result['content'] as List? ?? const []) (part as Map)['text'] ?? ''].join();
    return (text, result['isError'] == true || reply['error'] != null);
  }

  Future<int> close() async {
    await _process.stdin.close();
    return _process.exitCode.timeout(
      const Duration(seconds: 10),
      onTimeout: () {
        _process.kill();
        return -1;
      },
    );
  }
}

/// Wartet, bis der Befehl einer Rückfrage im Dialog steht, und beantwortet sie mit dem Knopf `answer`.
Future<void> _answerApproval(WidgetTester tester, String command, Key answer) async {
  await _appears(tester, find.byKey(const Key('mcp-approval')));
  expect(tester.widget<SelectableText>(find.byKey(const Key('mcp-approval-subject'))).data, command);
  await _until(tester, () => tester.widget<ButtonStyleButton>(find.byKey(answer)).onPressed != null, 'Antwortknopf');
  await _tap(tester, find.byKey(answer));
  await _until(tester, () => find.byKey(const Key('mcp-approval')).evaluate().isEmpty, 'Dialog geschlossen');
}

/// Der MCP-Dienst der App mit der echten Brücke der Kommandozeile.
Future<void> _mcp(WidgetTester tester) async {
  _step('MCP');
  await _tap(tester, find.text('MCP').first);
  await _appears(tester, find.byKey(const Key('mcp-service')));
  await _tap(tester, find.byKey(const Key('mcp-service')));
  await _until(tester, () => mcpState.running, 'Dienst läuft');
  expect(settings.mcpService, isTrue, reason: 'die Wahl gilt auch nach dem nächsten Start');

  // Client anlegen: Das Token ist nur jetzt zu sehen.
  await _tap(tester, find.byKey(const Key('mcp-add-client')));
  await _type(tester, find.byKey(const Key('mcp-client-name')), 'modell');
  await _tap(tester, find.widgetWithText(FilledButton, t('Anlegen', 'Add')));
  await _appears(tester, find.byKey(const Key('mcp-token')));
  final token = tester.widget<SelectableText>(find.byKey(const Key('mcp-token'))).data!;
  expect(token, startsWith('tephramcp_'));
  await _shot(tester, '14-mcp-token');
  await _tap(tester, find.byKey(const Key('mcp-token-done')));

  // Den Host freigeben, mit Bestätigung jedes Befehls.
  await _tap(tester, find.byKey(const Key('mcp-grant-modell')));
  await _tap(tester, find.widgetWithText(FilledButton, t('Freigeben', 'Share')));
  await _appears(tester, find.text(t('Mit Bestätigung', 'Confirm each')));
  await _shot(tester, '15-mcp');

  // Ein falsches Token kommt nicht durch.
  final stranger = await _Bridge.start('tephramcp_falsch');
  expect(await stranger.close(), isNot(0), reason: 'ein unbekanntes Token muss abgewiesen werden');

  final bridge = await _Bridge.start(token);
  await bridge.initialize();
  await _until(tester, () => mcpState.clients.contains('modell'), 'Client verbunden');
  final (hosts, _) = await bridge.tool('list_hosts', {});
  expect(hosts, contains('Testserver'));
  expect(jsonDecode(hosts)['limits'], {'default_timeout_secs': 60, 'max_timeout_secs': 3600});

  // Die längste Laufzeit stellt der Nutzer ein; das Modell erfährt sie über list_hosts.
  await _tap(tester, find.byKey(const Key('mcp-max-timeout')));
  await _tap(tester, find.text(t('2 Stunden', '2 hours')).last);
  final (again, _) = await bridge.tool('list_hosts', {});
  expect(jsonDecode(again)['limits']['max_timeout_secs'], 7200);

  // Bestätigt: Der Befehl läuft und seine Ausgabe kommt zurück.
  final approved = bridge.tool('ssh_exec', {'host': 'Testserver', 'command': 'echo aus dem Modell'});
  await _answerApproval(tester, 'echo aus dem Modell', const Key('mcp-approval-once'));
  final (output, failed) = await approved;
  expect(failed, isFalse, reason: output);
  expect(output, contains('aus dem Modell'));

  // Abgelehnt: Der Befehl läuft nicht.
  final refused = bridge.tool('ssh_exec', {'host': 'Testserver', 'command': 'echo nicht erlaubt'});
  await _answerApproval(tester, 'echo nicht erlaubt', const Key('mcp-approval-deny'));
  final (refusal, denied) = await refused;
  expect(denied, isTrue, reason: refusal);
  expect(refusal, isNot(contains('nicht erlaubt\n')));

  expect(await bridge.close(), 0);
  await _until(tester, () => mcpState.clients.isEmpty, 'Client getrennt');
  final log = mcpAudit(limit: 20);
  expect(log.any((entry) => entry.outcome == McpOutcome.approved && entry.detail == 'echo aus dem Modell'), isTrue);
  expect(log.any((entry) => entry.outcome == McpOutcome.rejected && entry.detail == 'echo nicht erlaubt'), isTrue);

  // Sperren beendet den Dienst; danach findet die Brücke ihn nicht mehr.
  await _tap(tester, find.byTooltip(t('Tresor sperren', 'Lock vault')));
  await _appears(tester, find.text(t('Tresor entsperren', 'Unlock vault')));
  expect(mcpServiceRunning(), isFalse);
  final late = await _Bridge.start(token);
  expect(await late.close(), isNot(0), reason: 'ohne offenen Tresor gibt es keinen Dienst');

  // Nach dem Entsperren läuft er wieder von selbst.
  await _type(tester, find.byType(TextField).first, _password);
  await _tap(tester, find.widgetWithText(FilledButton, t('Entsperren', 'Unlock')));
  await _appears(tester, find.text('Testserver'));
  await _until(tester, () => mcpState.running, 'Dienst läuft wieder');
}

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('Tresor anlegen, verbinden, sperren und wieder entsperren', (tester) async {
    expect(_port, isNonZero, reason: 'TEPHRA_TEST_PORT fehlt');
    await RustLib.init();
    await initPaths();
    settings.load();
    expect(vaultExists(path: vaultPath), isFalse, reason: 'Der Tresor darf noch nicht existieren');

    await tester.pumpWidget(const TerminalApp());
    await tester.pump(const Duration(milliseconds: 300));

    if (_join.isNotEmpty) {
      await _joinDevice(tester);
      return;
    }

    // ---- Tresor anlegen
    _step('Tresor anlegen');
    expect(find.byType(TextField), findsNWidgets(2));
    await _type(tester, find.byType(TextField).at(0), _password);
    await _type(tester, find.byType(TextField).at(1), 'etwas anderes');
    await _tap(tester, find.widgetWithText(FilledButton, t('Tresor anlegen', 'Create vault')));
    expect(find.text(t('Die Passwörter stimmen nicht überein.', 'The passwords do not match.')), findsOneWidget);
    await _type(tester, find.byType(TextField).at(1), _password);
    await _shot(tester, '01-tresor-anlegen');
    await _tap(tester, find.widgetWithText(FilledButton, t('Tresor anlegen', 'Create vault')));
    await _appears(tester, find.text(t('Wiederherstellungsschlüssel', 'Recovery key')));
    await _shot(tester, '02-wiederherstellung');
    final next = find.widgetWithText(FilledButton, t('Weiter', 'Continue'));
    expect(tester.widget<FilledButton>(next).onPressed, isNull, reason: 'erst nach Bestätigung weiter');
    await _tap(tester, find.byType(CheckboxListTile));
    await _tap(tester, next);
    await _appears(tester, find.text(t('Noch keine Hosts', 'No hosts yet')));
    await _shot(tester, '03-leer');

    // ---- Design: ohne gespeicherte Wahl dunkel, umschaltbar auf hell und zurück
    Brightness brightness() => Theme.of(tester.element(find.text(t('Noch keine Hosts', 'No hosts yet')))).brightness;
    expect(brightness(), Brightness.dark);
    await _tap(tester, find.byTooltip(t('Design', 'Theme')));
    await _tap(tester, find.text(t('Hell', 'Light')));
    await tester.pump(const Duration(milliseconds: 400));
    expect(brightness(), Brightness.light);
    await _shot(tester, '03-hell');
    await _tap(tester, find.byTooltip(t('Design', 'Theme')));
    await _tap(tester, find.text(t('Dunkel', 'Dark')));
    await tester.pump(const Duration(milliseconds: 400));
    expect(brightness(), Brightness.dark);

    // ---- Schlüssel erzeugen
    await _tap(tester, find.text(t('Schlüssel', 'Keys')).first);
    await _tap(tester, find.text(t('Schlüssel erzeugen', 'Generate key')));
    await _type(tester, find.byType(TextField).first, 'Laptop');
    await _tap(tester, find.widgetWithText(FilledButton, t('Erzeugen', 'Generate')));
    await _appears(tester, find.textContaining('ssh-ed25519'));
    await _shot(tester, '04-schluessel');

    // ---- Alten, passphrasegeschützten Schlüssel einlesen: Die Fehler sagen, woran es liegt
    _step('Schlüssel importieren');
    String failure(String? passphrase) {
      try {
        importKey(label: 'Alt', pem: legacyKeyPem, passphrase: passphrase, localOnly: false);
        return '';
      } catch (error) {
        return error.toString();
      }
    }

    expect(failure(null), contains(t('sie fehlt', 'sie fehlt')));
    expect(failure('falsch'), contains('stimmt nicht'));
    expect(failure(legacyKeyPassphrase), isEmpty);
    final imported = listKeys().firstWhere((key) => key.label == 'Alt');
    expect(imported.publicKey, startsWith(legacyKeyPublic));
    deleteItem(id: imported.id);

    // ---- VPN-Profil importieren
    final random = Random.secure();
    String key() => base64.encode([for (var i = 0; i < 32; i++) random.nextInt(256)]);
    await _tap(tester, find.text('VPN').first);
    await _tap(tester, find.text(t('Profil importieren', 'Import profile')));
    await _type(tester, find.byType(TextField).at(0), 'Büro');
    await _type(
      tester,
      find.byType(TextField).at(1),
      '[Interface]\nPrivateKey = ${key()}\nAddress = 10.7.0.2/32\n\n'
      '[Peer]\nPublicKey = ${key()}\nEndpoint = 127.0.0.1:51820\nAllowedIPs = 10.7.0.0/24\n',
    );
    await _shot(tester, '05-vpn-import');
    await _tap(tester, find.widgetWithText(FilledButton, t('Importieren', 'Import')));
    await _appears(tester, find.text('Büro'));
    await _shot(tester, '06-vpn');

    // ---- Identität anlegen
    await _tap(tester, find.text(t('Identitäten', 'Identities')).first);
    await _tap(tester, find.text(t('Identität hinzufügen', 'Add identity')));
    await _type(tester, find.byType(TextField).at(0), 'tester');
    await _type(tester, find.byType(TextField).at(2), 'geheim');
    await _shot(tester, '07-identitaet');
    await _tap(tester, find.widgetWithText(FilledButton, t('Speichern', 'Save')));
    await _appears(tester, find.text('tester'));

    // ---- Host anlegen
    await _tap(tester, find.text('Hosts').first);
    await _tap(tester, find.text(t('Host hinzufügen', 'Add host')));
    await _type(tester, find.byType(TextField).at(0), 'Testserver');
    await _type(tester, find.byType(TextField).at(1), _host);
    await _type(tester, find.byType(TextField).at(2), '$_port');
    await _tap(tester, find.byType(DropdownButtonFormField<String?>).first);
    await _tap(tester, find.text('tester').last);
    await _type(tester, find.byType(TextField).at(3), 'test, lokal');
    await _shot(tester, '08-host');
    await _tap(tester, find.widgetWithText(FilledButton, t('Speichern', 'Save')));
    await _appears(tester, find.text('Testserver'));
    expect(find.text('tester@$_host:$_port'), findsOneWidget);
    await _shot(tester, '09-hosts');

    // ---- Verbinden: unbekannter Hostschlüssel, dann Shell
    _step('verbinden');
    await _tap(tester, find.text('Testserver'));
    await _appears(tester, find.text(t('Unbekannter Host', 'Unknown host')));
    await _shot(tester, '10-hostschluessel');
    var session = tester.widget<SessionView>(find.byType(SessionView)).session;
    await _tap(tester, find.widgetWithText(FilledButton, t('Vertrauen', 'Trust')));
    await _until(tester, () => session.state == SessionState.connected, 'Verbindung');
    await _until(tester, () => _screen(session).contains('bereit>'), 'Eingabeaufforderung');
    // Der Testserver meldet jede Größenänderung: Die echte Fenstergröße ist angekommen.
    await _until(tester, () => RegExp(r'\[\d+x\d+\]').hasMatch(_screen(session)), 'Fenstergröße');
    expect(_screen(session), contains('[${session.terminal.viewWidth}x${session.terminal.viewHeight}]'));
    // Tasten der echten Tastatur landen nach der Rückfrage wieder im Terminal.
    // Am Telefon kommen Buchstaben über die Bildschirmtastatur, nicht als Tastenereignis.
    if (!Platform.isAndroid) {
      await tester.sendKeyEvent(LogicalKeyboardKey.keyQ, character: 'q');
      await tester.sendKeyEvent(LogicalKeyboardKey.keyX, character: 'x');
      await _until(tester, () => _screen(session).contains('qx'), 'Tastatureingabe');
    }
    session.terminal.textInput(' Grüße aus der App ✓');
    await _until(tester, () => _screen(session).contains('Grüße aus der App ✓'), 'Echo');
    await _shot(tester, '11-terminal');

    // ---- Am Telefon: langes Antippen wählt aus, die Leiste kopiert
    if (Platform.isAndroid) {
      _step('Kopieren am Telefon');
      await tester.longPressAt(tester.getTopLeft(find.byType(TerminalView)) + const Offset(16, 16));
      await _appears(tester, find.byKey(const Key('terminal-selection-bar')));
      await _tap(tester, find.byKey(const Key('terminal-select-all')));
      await _shot(tester, '11-auswahl');
      await _tap(tester, find.byKey(const Key('terminal-copy')));
      await _until(tester, () => find.byKey(const Key('terminal-selection-bar')).evaluate().isEmpty, 'Leiste weg');
      // Solange Android seine eigene Kopiert-Anzeige zeigt, lässt es die Zwischenablage nicht lesen.
      var copied = '';
      for (var i = 0; i < 40 && copied.isEmpty; i++) {
        await tester.pump(const Duration(milliseconds: 500));
        copied = (await Clipboard.getData(Clipboard.kTextPlain))?.text ?? '';
      }
      expect(copied, contains('Grüße aus der App ✓'));
      expect(copied, contains('bereit>'));

      // Die Taste „Einfügen“ der Zusatzleiste schickt es zurück an den Host.
      // Am schmalen Bildschirm bricht die Zeile um; gezählt wird deshalb ohne Zeilenumbrüche.
      int shown() => 'Grüße aus der App ✓'.allMatches(_screen(session).split('\n').join()).length;
      final before = shown();
      await _tap(tester, find.text(t('Einfügen', 'Paste')));
      await _until(tester, () => shown() > before, 'Eingefügt');
      await _shot(tester, '11-eingefuegt');
    }

    // ---- Dateien des Hosts: anlegen, übertragen, umbenennen, löschen
    _step('Dateien');
    await _tap(tester, find.byKey(const ValueKey('view-files')));
    await _appears(tester, find.text('etc'));
    await _shot(tester, '11-dateien');
    final files = session.files;
    await _tap(tester, find.byTooltip(t('Neuer Ordner', 'New folder')));
    await _type(tester, find.byType(TextField).last, 'ablage');
    await _tap(tester, find.widgetWithText(FilledButton, t('Anlegen', 'Create')));
    await _appears(tester, find.text('ablage'));
    await _tap(tester, find.text('ablage'));
    await _until(tester, () => files.remotePath == '/ablage' && !files.remoteLoading, 'Ordner öffnen');
    expect(files.remote, isEmpty);

    // Größer als ein Lesepuffer, damit die Übertragung über mehrere Stücke geht.
    final temp = Directory.systemTemp.createTempSync('tephra-test');
    final source = File('${temp.path}/quelle.bin')
      ..writeAsBytesSync([for (var i = 0; i < 700 * 1024; i++) (i * 31 + 7) % 251]);
    final up = files.upload(source.path, name: 'quelle.bin');
    await _until(tester, () => !up.running, 'Hochladen');
    expect(up.error, isNull);
    expect(up.finished, isTrue);
    await files.refreshRemote();
    expect(files.remote.single.name, 'quelle.bin');
    expect(files.remote.single.size.toInt(), source.lengthSync());

    final back = File('${temp.path}/zurueck.bin');
    final down = files.download(files.remote.single, back.path);
    await _until(tester, () => !down.running, 'Herunterladen');
    expect(down.error, isNull);
    expect(back.readAsBytesSync(), source.readAsBytesSync());
    expect(File('${back.path}.part').existsSync(), isFalse, reason: 'die unfertige Datei darf nicht liegen bleiben');
    await _shot(tester, '11-uebertragungen');

    await files.rename(files.remote.single, 'kopie.bin');
    expect(files.remote.single.name, 'kopie.bin');
    final missing = files.download(files.remote.single, '${temp.path}/x/../\u0000');
    await _until(tester, () => !missing.running, 'Fehlschlag');
    expect(missing.error, isNotNull, reason: 'ein unmögliches Ziel muss als Fehler ankommen');
    await files.goRemote('/');
    await files.delete(files.remote.firstWhere((entry) => entry.name == 'ablage'));
    expect(files.remote.any((entry) => entry.name == 'ablage'), isFalse, reason: 'der Ordner samt Inhalt ist weg');
    await files.goRemote('/gibt-es-nicht');
    expect(files.remotePath, '/', reason: 'ein unlesbares Ziel lässt die Ansicht, wo sie war');
    expect(files.remoteError, isNotNull);
    temp.deleteSync(recursive: true);
    await _tap(tester, find.byKey(const ValueKey('view-terminal')));
    await _appears(tester, find.byType(TerminalView));

    // ---- Öffentlichen Schlüssel auf dem Host eintragen
    _step('Schlüssel einrichten');
    final keyId = listKeys().single.id;
    expect(await installKeyOnHost(keyId: keyId, hostId: session.host.id), isTrue);
    expect(
      await installKeyOnHost(keyId: keyId, hostId: session.host.id),
      isFalse,
      reason: 'steht schon dort',
    );
    expect((await filesList(hostId: session.host.id, path: '/.ssh')).single.name, 'authorized_keys');

    // ---- Shell beenden und neu verbinden: der Schlüssel ist jetzt bekannt
    _step('Shell beenden');
    session.terminal.textInput('exit');
    await _until(tester, () => session.state == SessionState.closed, 'Sitzungsende');
    await _shot(tester, '12-beendet');
    _step('neu verbinden');
    await _tap(tester, find.text(t('Neu verbinden', 'Reconnect')));
    await _until(tester, () => session.state == SessionState.connected, 'zweite Verbindung');
    expect(find.text(t('Unbekannter Host', 'Unknown host')), findsNothing);

    // ---- Sperren beendet die Sitzung; falsches Passwort wird abgewiesen
    _step('sperren');
    await _tap(tester, find.byTooltip(t('Tresor sperren', 'Lock vault')));
    await _appears(tester, find.text(t('Tresor entsperren', 'Unlock vault')));
    expect(isUnlocked(), isFalse);
    _step('falsches Passwort');
    await _type(tester, find.byType(TextField).first, 'falsch');
    await _tap(tester, find.widgetWithText(FilledButton, t('Entsperren', 'Unlock')));
    await _until(tester, () => tester.widget<FilledButton>(find.byType(FilledButton)).onPressed != null, 'Ablehnung');
    expect(isUnlocked(), isFalse);
    await _shot(tester, '13-falsches-passwort');
    _step('entsperren');
    await _type(tester, find.byType(TextField).first, _password);
    await _tap(tester, find.widgetWithText(FilledButton, t('Entsperren', 'Unlock')));
    await _appears(tester, find.text('Testserver'));
    expect(find.byType(SessionView), findsNothing);

    // ---- Nach dem Entsperren verbindet der Host ohne Rückfrage
    _step('verbinden nach dem Entsperren');
    await _tap(tester, find.text('Testserver'));
    await _appears(tester, find.byType(SessionView));
    session = tester.widget<SessionView>(find.byType(SessionView)).session;
    await _until(tester, () => session.state == SessionState.connected, 'Verbindung nach dem Entsperren');
    await _until(tester, () => _screen(session).contains('bereit>'), 'Eingabeaufforderung');
    await _tap(tester, find.byTooltip(t('Sitzung schließen', 'Close session')));
    await _appears(tester, find.text(t('Keine offenen Sitzungen', 'No open sessions')));

    // ---- Sitzung halten: tmux auf einem echten SSH-Server, Abbruch und Wiederverbinden
    if (_sshd.isNotEmpty) await _keepSession(tester);

    // ---- Entsperren ohne Master-Passwort (Windows und Android)
    if (deviceUnlock.supported) await _deviceUnlock(tester);

    // ---- MCP: Dienst der App, Client, Freigabe, Rückfragen über die echte Brücke
    if (_cli.isNotEmpty) await _mcp(tester);

    // ---- Abgleich: Konto auf dem Sync-Server anlegen, der Tresor wird hochgeladen
    if (_sync.isNotEmpty) {
      _step('Abgleich');
      await _tap(tester, find.byKey(const Key('sync-button')));
      await _type(tester, find.byKey(const Key('sync-server')), _sync);
      await _type(tester, find.byKey(const Key('sync-username')), _account);
      await _type(tester, find.byKey(const Key('sync-password')), 'falsches Passwort');
      await _allowPlainHttp(tester, _sync, const Key('sync-register'));
      await _tap(tester, find.byKey(const Key('sync-register')));
      await _appears(tester, find.byKey(const Key('sync-error')));
      expect(find.byKey(const Key('sync-connected')), findsNothing);
      await _type(tester, find.byKey(const Key('sync-password')), _password);
      await _tap(tester, find.byKey(const Key('sync-register')));
      await _syncDone(tester, _sync);
      await _tap(tester, find.text(t('Schließen', 'Close')));
    }

    // ---- Aktualisierung: echte Veröffentlichung finden, laden und ihre Prüfsumme prüfen
    if (_update) {
      _step('Aktualisierung');
      final update = (await checkForUpdate(current: '0.1.0', kind: UpdateKind.installer))!;
      expect(compareVersions(update.version, '0.2.0'), greaterThanOrEqualTo(0));
      expect(update.kind, UpdateKind.installer);
      final folder = Directory.systemTemp.createTempSync('tephra-update');
      final target = '${folder.path}/setup.exe';
      await downloadUpdate(update, target);
      expect(File(target).lengthSync(), greaterThan(10 * 1024 * 1024));
      folder.deleteSync(recursive: true);
      expect(await checkForUpdate(current: '99.0.0'), isNull);
    }
  });
}
