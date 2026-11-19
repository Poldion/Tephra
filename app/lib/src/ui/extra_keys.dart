import 'dart:async';

import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../keys.dart';
import '../session.dart';
import '../text.dart';
import 'common.dart';

/// Tasten, die der Bildschirmtastatur fehlen: Esc, Tab, Strg, Alt, Pfeile,
/// häufige Zeichen und – hinter „Fn“ – die Funktionstasten.
///
/// Strg und Alt gelten für die nächste Taste; ein zweites Antippen stellt sie
/// fest, ein drittes löst sie. Pfeile und Blättern wiederholen sich beim Halten.
class ExtraKeys extends StatelessWidget {
  const ExtraKeys({super.key, required this.session});

  final TerminalSession session;

  static const _functionKeys = [
    TerminalKey.f1,
    TerminalKey.f2,
    TerminalKey.f3,
    TerminalKey.f4,
    TerminalKey.f5,
    TerminalKey.f6,
    TerminalKey.f7,
    TerminalKey.f8,
    TerminalKey.f9,
    TerminalKey.f10,
    TerminalKey.f11,
    TerminalKey.f12,
  ];

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    Widget key(String label, TerminalKey key, {bool repeat = false}) =>
        _Key(label: label, onPress: () => session.pressKey(key), repeat: repeat);
    Widget modifier(String label, ModifierState state, VoidCallback onPress) =>
        _Key(label: label, onPress: onPress, active: state.active, locked: state == ModifierState.locked);
    return Container(
      color: scheme.surfaceContainer,
      height: 44,
      child: ListView(
        scrollDirection: Axis.horizontal,
        padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 5),
        children: [
          key('Esc', TerminalKey.escape),
          key('Tab', TerminalKey.tab),
          modifier(t('Strg', 'Ctrl'), session.ctrl, session.cycleCtrl),
          modifier('Alt', session.alt, session.cycleAlt),
          _Key(label: 'Fn', onPress: session.toggleFunctionKeys, active: session.functionKeys),
          if (session.functionKeys)
            for (final (index, function) in _functionKeys.indexed) key('F${index + 1}', function)
          else ...[
            key('↑', TerminalKey.arrowUp, repeat: true),
            key('↓', TerminalKey.arrowDown, repeat: true),
            key('←', TerminalKey.arrowLeft, repeat: true),
            key('→', TerminalKey.arrowRight, repeat: true),
            for (final char in ['|', '/', '-', '~', '_', ':', '*', '&', '\\', '<', '>'])
              _Key(label: char, onPress: () => session.typeText(char)),
            key(t('Pos1', 'Home'), TerminalKey.home),
            key(t('Ende', 'End'), TerminalKey.end),
            key(t('Bild↑', 'PgUp'), TerminalKey.pageUp, repeat: true),
            key(t('Bild↓', 'PgDn'), TerminalKey.pageDown, repeat: true),
            key(t('Entf', 'Del'), TerminalKey.delete, repeat: true),
            key(t('Einfg', 'Ins'), TerminalKey.insert),
          ],
        ],
      ),
    );
  }
}

class _Key extends StatefulWidget {
  const _Key({
    required this.label,
    required this.onPress,
    this.active = false,
    this.locked = false,
    this.repeat = false,
  });

  final String label;
  final VoidCallback onPress;
  final bool active;

  /// Festgestellte Zusatztaste: bleibt, bis sie wieder angetippt wird.
  final bool locked;

  /// Wiederholt sich, solange die Taste gehalten wird.
  final bool repeat;

  @override
  State<_Key> createState() => _KeyState();
}

class _KeyState extends State<_Key> {
  Timer? _repeating;

  void _stop() {
    _repeating?.cancel();
    _repeating = null;
  }

  @override
  void dispose() {
    _stop();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 3),
      child: Listener(
        onPointerUp: (_) => _stop(),
        onPointerCancel: (_) => _stop(),
        child: Material(
          color: widget.active ? scheme.primary : scheme.surfaceContainerHighest,
          borderRadius: BorderRadius.circular(6),
          clipBehavior: Clip.antiAlias,
          child: InkWell(
            // Der Fokus bleibt beim Terminal, sonst verschwände die Tastatur.
            canRequestFocus: false,
            onTap: widget.onPress,
            onLongPress: widget.repeat
                ? () {
                    widget.onPress();
                    _repeating ??= Timer.periodic(const Duration(milliseconds: 70), (_) => widget.onPress());
                  }
                : null,
            child: Container(
              constraints: const BoxConstraints(minWidth: 42),
              padding: const EdgeInsets.symmetric(horizontal: 10),
              alignment: Alignment.center,
              decoration: widget.locked
                  ? BoxDecoration(
                      border: Border(bottom: BorderSide(color: scheme.tertiary, width: 3)),
                    )
                  : null,
              child: Text(
                widget.label,
                style: monoStyle(
                  context,
                  size: 14,
                ).copyWith(color: widget.active ? scheme.onPrimary : scheme.onSurface),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
