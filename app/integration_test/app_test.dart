// Fährt die echte App gegen den SSH-Testserver aus `tephra-ssh`:
//
//   cargo run -p tephra-ssh --features testing --example test_server
//   TEPHRA_VAULT=<leerer Pfad> flutter test integration_test -d windows \
//       --dart-define=TEPHRA_TEST_PORT=<Port des Testservers>
//
// Mit `--dart-define=TEPHRA_TEST_SHOTS=<Ordner>` legt der Test Bildschirmfotos ab.
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
import 'package:tephra/src/paths.dart';
import 'package:tephra/src/rust/api/app.dart';
import 'package:tephra/src/rust/frb_generated.dart';
import 'package:tephra/src/session.dart';
import 'package:tephra/src/settings.dart';
import 'package:tephra/src/text.dart';
import 'package:tephra/src/ui/terminal_page.dart';

const _host = String.fromEnvironment('TEPHRA_TEST_HOST', defaultValue: '127.0.0.1');
const _port = int.fromEnvironment('TEPHRA_TEST_PORT');
const _shots = String.fromEnvironment('TEPHRA_TEST_SHOTS');
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

String _screen(TerminalSession session) => session.terminal.buffer.getText();

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

    // ---- Tresor anlegen
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

    // ---- Shell beenden und neu verbinden: der Schlüssel ist jetzt bekannt
    session.terminal.textInput('exit');
    await _until(tester, () => session.state == SessionState.closed, 'Sitzungsende');
    await _shot(tester, '12-beendet');
    await _tap(tester, find.text(t('Neu verbinden', 'Reconnect')));
    await _until(tester, () => session.state == SessionState.connected, 'zweite Verbindung');
    expect(find.text(t('Unbekannter Host', 'Unknown host')), findsNothing);

    // ---- Sperren beendet die Sitzung; falsches Passwort wird abgewiesen
    await _tap(tester, find.byTooltip(t('Tresor sperren', 'Lock vault')));
    await _appears(tester, find.text(t('Tresor entsperren', 'Unlock vault')));
    expect(isUnlocked(), isFalse);
    await _type(tester, find.byType(TextField).first, 'falsch');
    await _tap(tester, find.widgetWithText(FilledButton, t('Entsperren', 'Unlock')));
    await _until(tester, () => tester.widget<FilledButton>(find.byType(FilledButton)).onPressed != null, 'Ablehnung');
    expect(isUnlocked(), isFalse);
    await _shot(tester, '13-falsches-passwort');
    await _type(tester, find.byType(TextField).first, _password);
    await _tap(tester, find.widgetWithText(FilledButton, t('Entsperren', 'Unlock')));
    await _appears(tester, find.text('Testserver'));
    expect(find.byType(SessionView), findsNothing);

    // ---- Nach dem Entsperren verbindet der Host ohne Rückfrage
    await _tap(tester, find.text('Testserver'));
    await _appears(tester, find.byType(SessionView));
    session = tester.widget<SessionView>(find.byType(SessionView)).session;
    await _until(tester, () => session.state == SessionState.connected, 'Verbindung nach dem Entsperren');
    await _until(tester, () => _screen(session).contains('bereit>'), 'Eingabeaufforderung');
    await _tap(tester, find.byTooltip(t('Sitzung schließen', 'Close session')));
    await _appears(tester, find.text(t('Keine offenen Sitzungen', 'No open sessions')));
  });
}
