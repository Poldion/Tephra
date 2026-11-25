import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../text.dart';

/// Kopiert die Auswahl im Terminal in die Zwischenablage und hebt sie auf.
/// `false`, wenn nichts ausgewählt war.
Future<bool> copyTerminalSelection(Terminal terminal, TerminalController controller) async {
  final selection = controller.selection;
  if (selection == null) return false;
  final text = terminal.buffer.getText(selection);
  controller.clearSelection();
  await Clipboard.setData(ClipboardData(text: text));
  return true;
}

/// Wählt den ganzen Inhalt des Terminals aus, samt allem, was schon hinausgescrollt ist.
void selectAllOfTerminal(Terminal terminal, TerminalController controller) {
  final buffer = terminal.buffer;
  // Leere Zeilen am Ende gehören nicht dazu.
  var last = buffer.height - 1;
  while (last > 0 && buffer.lines[last].getText().trim().isEmpty) {
    last--;
  }
  controller.setSelection(buffer.createAnchor(0, 0), buffer.createAnchor(terminal.viewWidth, last));
}

/// Fügt den Text der Zwischenablage ins Terminal ein, als käme er von der Tastatur.
Future<void> pasteIntoTerminal(Terminal terminal) async {
  final text = (await Clipboard.getData(Clipboard.kTextPlain))?.text;
  if (text != null && text.isNotEmpty) terminal.paste(text);
}

/// Leiste über dem Terminal, solange etwas ausgewählt ist: kopieren, alles auswählen, Auswahl aufheben.
///
/// Am Telefon gibt es weder Rechtsklick noch Tastenkürzel; ausgewählt wird dort durch langes Antippen, und
/// Halten und Ziehen erweitert die Auswahl.
class TerminalSelectionBar extends StatelessWidget {
  const TerminalSelectionBar({super.key, required this.terminal, required this.controller});

  final Terminal terminal;
  final TerminalController controller;

  // Bestätigt wird das Kopieren vom System selbst (Android ab Version 13). Ein eigener Hinweis läge unten
  // über der Zusatzleiste und verdeckte genau die Taste zum Einfügen.
  Future<void> _copy() => copyTerminalSelection(terminal, controller);

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        if (controller.selection == null) return const SizedBox.shrink();
        return Material(
          key: const Key('terminal-selection-bar'),
          color: scheme.surfaceContainerHighest,
          elevation: 3,
          borderRadius: BorderRadius.circular(24),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 4),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextButton.icon(
                  key: const Key('terminal-copy'),
                  icon: const Icon(Icons.copy_rounded, size: 18),
                  label: Text(t('Kopieren', 'Copy')),
                  onPressed: _copy,
                ),
                TextButton(
                  key: const Key('terminal-select-all'),
                  onPressed: () => selectAllOfTerminal(terminal, controller),
                  child: Text(t('Alles', 'All')),
                ),
                IconButton(
                  tooltip: t('Auswahl aufheben', 'Clear selection'),
                  icon: const Icon(Icons.close_rounded, size: 20),
                  onPressed: controller.clearSelection,
                ),
              ],
            ),
          ),
        );
      },
    );
  }
}
