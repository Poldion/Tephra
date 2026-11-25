// Kopieren aus dem Terminal am Telefon: langes Antippen wählt aus, die Leiste kopiert.

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:tephra/src/ui/terminal_selection.dart';
import 'package:xterm/xterm.dart';

void main() {
  late Terminal terminal;
  late TerminalController controller;
  late List<String> sent;
  String? clipboard;

  Future<void> open(WidgetTester tester) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    terminal = Terminal();
    controller = TerminalController();
    sent = [];
    terminal.onOutput = sent.add;
    clipboard = 'aus der Ablage';
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      switch (call.method) {
        case 'Clipboard.getData':
          return {'text': clipboard};
        case 'Clipboard.setData':
          clipboard = (call.arguments as Map)['text'] as String?;
      }
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, null));
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Stack(
            children: [
              Positioned.fill(child: TerminalView(terminal, controller: controller)),
              Positioned(
                top: 8,
                left: 0,
                right: 0,
                child: Center(
                  child: TerminalSelectionBar(terminal: terminal, controller: controller),
                ),
              ),
            ],
          ),
        ),
      ),
    );
    terminal.write('Ausgabe vom Host\r\nzweite Zeile\r\n');
    await tester.pump();
  }

  /// Hält den Finger auf dem ersten Wort der ersten Zeile.
  Future<void> longPressFirstWord(WidgetTester tester) async {
    final origin = tester.getTopLeft(find.byType(TerminalView));
    await tester.longPressAt(origin + const Offset(12, 8));
    await tester.pumpAndSettle();
  }

  testWidgets('ohne Auswahl ist keine Leiste zu sehen', (tester) async {
    await open(tester);
    expect(find.byKey(const Key('terminal-selection-bar')), findsNothing);
  });

  testWidgets('langes Antippen wählt ein Wort aus, Kopieren legt es in die Zwischenablage', (tester) async {
    await open(tester);
    await longPressFirstWord(tester);
    expect(controller.selection, isNotNull);
    expect(find.byKey(const Key('terminal-selection-bar')), findsOneWidget);

    await tester.tap(find.byKey(const Key('terminal-copy')));
    await tester.pumpAndSettle();
    expect(clipboard, 'Ausgabe');
    expect(controller.selection, isNull, reason: 'nach dem Kopieren ist die Auswahl aufgehoben');
    expect(find.byKey(const Key('terminal-selection-bar')), findsNothing);
    expect(sent, isEmpty, reason: 'Kopieren schickt nichts an den Host');
  });

  testWidgets('„Alles“ kopiert den ganzen Inhalt ohne leere Zeilen am Ende', (tester) async {
    await open(tester);
    await longPressFirstWord(tester);
    await tester.tap(find.byKey(const Key('terminal-select-all')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('terminal-copy')));
    await tester.pumpAndSettle();
    expect(clipboard!.trimRight(), 'Ausgabe vom Host\nzweite Zeile');
  });

  testWidgets('Einfügen schickt die Zwischenablage an den Host', (tester) async {
    await open(tester);
    await pasteIntoTerminal(terminal);
    expect(sent.join(), 'aus der Ablage');
  });
}
