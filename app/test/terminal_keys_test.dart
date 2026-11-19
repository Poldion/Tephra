// Tastenkürzel des Terminals für die Zwischenablage, ohne Gerät und ohne Kern.

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:tephra/src/ui/terminal_page.dart';
import 'package:xterm/xterm.dart';

void main() {
  late Terminal terminal;
  late TerminalController controller;
  late List<String> sent;
  String? clipboard;

  Future<void> open(WidgetTester tester) async {
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
          body: TerminalView(
            terminal,
            controller: controller,
            autofocus: true,
            shortcuts: terminalShortcuts,
            hardwareKeyboardOnly: true,
          ),
        ),
      ),
    );
    await tester.pump();
    terminal.write('Zeile zum Kopieren');
    await tester.pump();
  }

  Future<void> press(WidgetTester tester, LogicalKeyboardKey modifier, LogicalKeyboardKey key) async {
    await tester.sendKeyDownEvent(modifier);
    await tester.sendKeyEvent(key);
    await tester.sendKeyUpEvent(modifier);
    await tester.pump();
  }

  void select() => controller.setSelection(terminal.buffer.createAnchor(0, 0), terminal.buffer.createAnchor(5, 0));

  for (final (name, key) in [
    ('Einfg', LogicalKeyboardKey.insert),
    ('Einfg des Ziffernblocks', LogicalKeyboardKey.numpad0),
  ]) {
    testWidgets('Umschalt+$name fügt die Zwischenablage ein', (tester) async {
      await open(tester);
      await press(tester, LogicalKeyboardKey.shiftLeft, key);
      expect(sent.join(), 'aus der Ablage');
    });

    testWidgets('Strg+$name kopiert die Auswahl und schickt nichts an den Host', (tester) async {
      await open(tester);
      select();
      await press(tester, LogicalKeyboardKey.controlLeft, key);
      expect(clipboard, 'Zeile');
      expect(sent, isEmpty);
    });
  }

  testWidgets('Strg+Umschalt+C und Strg+Umschalt+V kopieren und fügen ein', (tester) async {
    await open(tester);
    select();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await press(tester, LogicalKeyboardKey.shiftLeft, LogicalKeyboardKey.keyC);
    expect(clipboard, 'Zeile');
    await press(tester, LogicalKeyboardKey.shiftLeft, LogicalKeyboardKey.keyV);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    expect(sent.join(), 'Zeile');
  });

  testWidgets('Strg+C bleibt beim Programm auf dem Host', (tester) async {
    await open(tester);
    select();
    await press(tester, LogicalKeyboardKey.controlLeft, LogicalKeyboardKey.keyC);
    expect(sent.join().codeUnits, [3]);
    expect(clipboard, 'aus der Ablage');
  });
}
