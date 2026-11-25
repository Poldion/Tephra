import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

import '../rust/api/app.dart';
import '../session.dart';
import '../state.dart';
import '../text.dart';
import 'common.dart';
import 'extra_keys.dart';
import 'files_view.dart';
import 'terminal_selection.dart';

/// Die offenen Terminalsitzungen als Reiter.
class TerminalPage extends StatelessWidget {
  const TerminalPage({super.key, required this.state, required this.onShowHosts});

  final AppState state;
  final VoidCallback onShowHosts;

  @override
  Widget build(BuildContext context) {
    final active = state.active;
    if (active == null) {
      return Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          EmptyState(
            icon: Icons.terminal_outlined,
            title: t('Keine offenen Sitzungen', 'No open sessions'),
            hint: t('Wähle einen Host aus, um dich zu verbinden.', 'Pick a host to connect.'),
          ),
          const SizedBox(height: 16),
          FilledButton.tonal(onPressed: onShowHosts, child: Text(t('Zu den Hosts', 'Go to hosts'))),
        ],
      );
    }
    final scheme = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          color: scheme.surfaceContainer,
          height: 42,
          child: Row(
            children: [
              Expanded(
                child: ListView(
                  scrollDirection: Axis.horizontal,
                  children: [
                    for (final session in state.sessions)
                      _Tab(
                        session: session,
                        selected: session == active,
                        onSelect: () => state.select(session),
                        onClose: () => state.close(session),
                      ),
                  ],
                ),
              ),
              _ViewSwitch(session: active),
            ],
          ),
        ),
        Expanded(
          child: SessionView(key: ObjectKey(active), session: active, onClose: () => state.close(active)),
        ),
      ],
    );
  }
}

/// Umschalter der aktiven Sitzung zwischen Terminal und Dateien.
class _ViewSwitch extends StatelessWidget {
  const _ViewSwitch({required this.session});

  final TerminalSession session;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    Widget button({required IconData icon, required String tooltip, required bool files}) {
      final selected = session.showFiles == files;
      return IconButton(
        key: ValueKey(files ? 'view-files' : 'view-terminal'),
        tooltip: tooltip,
        isSelected: selected,
        visualDensity: VisualDensity.compact,
        style: IconButton.styleFrom(
          backgroundColor: selected ? scheme.secondaryContainer : null,
          foregroundColor: selected ? scheme.onSecondaryContainer : scheme.onSurfaceVariant,
        ),
        icon: Icon(icon, size: 20),
        onPressed: () => session.showFiles = files,
      );
    }

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 6),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          button(icon: Icons.terminal_rounded, tooltip: 'Terminal', files: false),
          button(icon: Icons.folder_outlined, tooltip: t('Dateien', 'Files'), files: true),
        ],
      ),
    );
  }
}

Color _stateColor(TerminalSession session, ColorScheme scheme) {
  if (session.needsAttention) return scheme.error;
  return switch (session.state) {
    SessionState.connecting => Colors.amber,
    SessionState.connected => Colors.green,
    SessionState.closed => scheme.outline,
  };
}

class _Tab extends StatelessWidget {
  const _Tab({required this.session, required this.selected, required this.onSelect, required this.onClose});

  final TerminalSession session;
  final bool selected;
  final VoidCallback onSelect;
  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    return Material(
      color: selected ? _terminalBackground : Colors.transparent,
      child: InkWell(
        onTap: onSelect,
        child: Padding(
          padding: const EdgeInsets.only(left: 14, right: 4),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                width: 8,
                height: 8,
                decoration: BoxDecoration(color: _stateColor(session, scheme), shape: BoxShape.circle),
              ),
              const SizedBox(width: 8),
              ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 180),
                child: Text(
                  session.host.label,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.labelLarge?.copyWith(
                    color: selected ? _terminalForeground : scheme.onSurfaceVariant,
                  ),
                ),
              ),
              const SizedBox(width: 4),
              IconButton(
                tooltip: t('Sitzung schließen', 'Close session'),
                visualDensity: VisualDensity.compact,
                iconSize: 16,
                color: selected ? _terminalForeground : scheme.onSurfaceVariant,
                icon: const Icon(Icons.close_rounded),
                onPressed: onClose,
              ),
            ],
          ),
        ),
      ),
    );
  }
}

const _terminalBackground = Color(0xFF1E1E1E);
const _terminalForeground = Color(0xFFCCCCCC);

const _paste = PasteTextIntent(SelectionChangedCause.keyboard);

/// Tastenkürzel des Terminals für die Zwischenablage. Strg+C und Strg+V
/// gehören dem Programm auf dem Host; kopiert und eingefügt wird deshalb mit
/// der Umschalttaste dazu oder, wie unter Windows und Linux üblich, mit Einfg.
/// Die Einfg-Taste des Ziffernblocks meldet sich bei ausgeschaltetem Num als
/// eigene Taste und steht darum doppelt da.
final terminalShortcuts = <ShortcutActivator, Intent>{
  const SingleActivator(LogicalKeyboardKey.keyC, control: true, shift: true): CopySelectionTextIntent.copy,
  const SingleActivator(LogicalKeyboardKey.keyV, control: true, shift: true): _paste,
  const SingleActivator(LogicalKeyboardKey.insert, control: true): CopySelectionTextIntent.copy,
  const SingleActivator(LogicalKeyboardKey.insert, shift: true): _paste,
  const SingleActivator(LogicalKeyboardKey.numpad0, control: true, numLock: LockState.unlocked):
      CopySelectionTextIntent.copy,
  const SingleActivator(LogicalKeyboardKey.numpad0, shift: true, numLock: LockState.unlocked): _paste,
};

/// Am Desktop kommen die Tasten direkt von der Tastatur. Die Texteingabe des
/// Systems (Bildschirmtastatur) braucht nur das Telefon.
bool get _hardwareKeyboard => switch (defaultTargetPlatform) {
  TargetPlatform.windows || TargetPlatform.linux || TargetPlatform.macOS => true,
  _ => false,
};

/// Terminal einer Sitzung samt Rückfragen und Statuszeile.
class SessionView extends StatefulWidget {
  const SessionView({super.key, required this.session, required this.onClose});

  final TerminalSession session;
  final VoidCallback onClose;

  @override
  State<SessionView> createState() => _SessionViewState();
}

class _SessionViewState extends State<SessionView> {
  final _focus = FocusNode();
  bool _hadPrompt = false;

  @override
  void dispose() {
    _focus.dispose();
    super.dispose();
  }

  /// Rechtsklick: Auswahl kopieren, sonst einfügen.
  Future<void> _secondaryTap() async {
    final session = widget.session;
    if (!await copyTerminalSelection(session.terminal, session.controller)) await pasteIntoTerminal(session.terminal);
  }

  @override
  Widget build(BuildContext context) {
    final session = widget.session;
    return ListenableBuilder(
      listenable: session,
      builder: (context, _) {
        final prompt = session.prompt;
        if (_hadPrompt && prompt == null) {
          // Nach einer Rückfrage gehören die Tasten wieder dem Terminal.
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (mounted) _focus.requestFocus();
          });
        }
        _hadPrompt = prompt != null;
        if (session.showFiles) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Expanded(
                child: session.state == SessionState.connected
                    ? FilesView(files: session.files, hostLabel: session.host.label)
                    : EmptyState(
                        icon: Icons.folder_off_outlined,
                        title: t('Keine Verbindung', 'Not connected'),
                        hint: t(
                          'Die Dateien des Hosts stehen bereit, sobald die Sitzung verbunden ist.',
                          'The files of the host are available as soon as the session is connected.',
                        ),
                      ),
              ),
              _StatusBar(session: session, onClose: widget.onClose),
            ],
          );
        }
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Expanded(
              child: Stack(
                children: [
                  Positioned.fill(
                    child: ColoredBox(
                      color: _terminalBackground,
                      child: TerminalView(
                        session.terminal,
                        controller: session.controller,
                        focusNode: _focus,
                        autofocus: prompt == null,
                        padding: const EdgeInsets.all(8),
                        textStyle: TerminalStyle(
                          fontSize: 14,
                          fontFamily: monoFonts.first,
                          fontFamilyFallback: monoFonts,
                        ),
                        shortcuts: terminalShortcuts,
                        hardwareKeyboardOnly: _hardwareKeyboard,
                        onSecondaryTapDown: (_, _) => _secondaryTap(),
                      ),
                    ),
                  ),
                  // Am Telefon gibt es weder Rechtsklick noch Tastenkürzel zum Kopieren.
                  if (!_hardwareKeyboard && prompt == null)
                    Positioned(
                      top: 8,
                      left: 0,
                      right: 0,
                      child: Center(
                        child: TerminalSelectionBar(terminal: session.terminal, controller: session.controller),
                      ),
                    ),
                  if (prompt != null)
                    Positioned.fill(
                      child: ColoredBox(
                        color: Colors.black54,
                        child: Center(
                          child: SingleChildScrollView(
                            padding: const EdgeInsets.all(20),
                            child: _PromptCard(key: ObjectKey(prompt), session: session, prompt: prompt),
                          ),
                        ),
                      ),
                    ),
                ],
              ),
            ),
            if (!_hardwareKeyboard && session.state == SessionState.connected) ExtraKeys(session: session),
            _StatusBar(session: session, onClose: widget.onClose),
          ],
        );
      },
    );
  }
}

class _StatusBar extends StatelessWidget {
  const _StatusBar({required this.session, required this.onClose});

  final TerminalSession session;
  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final closed = session.state == SessionState.closed;
    return Container(
      color: scheme.surfaceContainer,
      padding: const EdgeInsets.symmetric(horizontal: 12),
      constraints: const BoxConstraints(minHeight: 32),
      child: Row(
        children: [
          Container(
            width: 8,
            height: 8,
            decoration: BoxDecoration(color: _stateColor(session, scheme), shape: BoxShape.circle),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              session.status,
              style: theme.textTheme.bodySmall?.copyWith(color: scheme.onSurfaceVariant),
              overflow: TextOverflow.ellipsis,
            ),
          ),
          if (closed) ...[
            TextButton(onPressed: session.reconnect, child: Text(t('Neu verbinden', 'Reconnect'))),
            TextButton(onPressed: onClose, child: Text(t('Schließen', 'Close'))),
          ],
        ],
      ),
    );
  }
}

/// Rückfrage während des Verbindungsaufbaus.
class _PromptCard extends StatefulWidget {
  const _PromptCard({super.key, required this.session, required this.prompt});

  final TerminalSession session;
  final SessionEvent prompt;

  @override
  State<_PromptCard> createState() => _PromptCardState();
}

class _PromptCardState extends State<_PromptCard> {
  late final List<TextEditingController> _fields = [for (var i = 0; i < _fieldCount; i++) TextEditingController()];

  int get _fieldCount => switch (widget.prompt) {
    SessionEvent_Password() => 1,
    SessionEvent_ServerPrompts(:final prompts) => prompts.length,
    _ => 0,
  };

  @override
  void dispose() {
    for (final field in _fields) {
      field.clear();
      field.dispose();
    }
    super.dispose();
  }

  void _submit() {
    switch (widget.prompt) {
      case SessionEvent_Password():
        widget.session.answerPassword(_fields.first.text);
      case SessionEvent_ServerPrompts():
        widget.session.answerServerPrompts([for (final field in _fields) field.text]);
      default:
        widget.session.answerHostKey(true);
    }
  }

  void _cancel() {
    switch (widget.prompt) {
      case SessionEvent_Password():
        widget.session.answerPassword(null);
      case SessionEvent_ServerPrompts():
        widget.session.answerServerPrompts(null);
      default:
        widget.session.answerHostKey(false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final prompt = widget.prompt;
    final (title, body, action) = switch (prompt) {
      SessionEvent_HostKey() => _hostKey(theme, prompt),
      SessionEvent_Password(:final prompt) => (
        t('Passwort', 'Password'),
        <Widget>[
          SecretField(
            controller: _fields.first,
            label: clean(prompt).trim().isEmpty ? t('Passwort', 'Password') : clean(prompt).trim(),
            autofocus: true,
            onSubmitted: (_) => _submit(),
          ),
        ],
        t('Anmelden', 'Sign in'),
      ),
      SessionEvent_ServerPrompts(:final name, :final instructions, :final prompts) => (
        clean(name).trim().isEmpty ? t('Rückfrage des Servers', 'Question from the server') : clean(name).trim(),
        <Widget>[
          if (clean(instructions).trim().isNotEmpty) Text(clean(instructions).trim()),
          for (final (index, question) in prompts.indexed)
            question.echo
                ? TextField(
                    controller: _fields[index],
                    autofocus: index == 0,
                    autocorrect: false,
                    decoration: InputDecoration(labelText: clean(question.text).trim()),
                    onSubmitted: (_) => index == prompts.length - 1 ? _submit() : null,
                  )
                : SecretField(
                    controller: _fields[index],
                    label: clean(question.text).trim(),
                    autofocus: index == 0,
                    onSubmitted: (_) => index == prompts.length - 1 ? _submit() : null,
                  ),
        ],
        t('Senden', 'Send'),
      ),
      _ => ('', const <Widget>[], ''),
    };
    final changed = prompt is SessionEvent_HostKey && prompt.knownFingerprints.isNotEmpty;
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 480),
      child: Card(
        color: scheme.surfaceContainerHigh,
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(title, style: theme.textTheme.titleLarge),
              Text(
                widget.session.host.label,
                style: theme.textTheme.bodySmall?.copyWith(color: scheme.onSurfaceVariant),
              ),
              const SizedBox(height: 16),
              for (final (index, child) in body.indexed) ...[if (index > 0) const SizedBox(height: 12), child],
              const SizedBox(height: 20),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                    autofocus: changed,
                    onPressed: _cancel,
                    child: Text(prompt is SessionEvent_HostKey ? t('Ablehnen', 'Reject') : t('Abbrechen', 'Cancel')),
                  ),
                  const SizedBox(width: 8),
                  FilledButton(
                    style: changed
                        ? FilledButton.styleFrom(backgroundColor: scheme.error, foregroundColor: scheme.onError)
                        : null,
                    onPressed: _submit,
                    child: Text(action),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  (String, List<Widget>, String) _hostKey(ThemeData theme, SessionEvent_HostKey prompt) {
    final scheme = theme.colorScheme;
    final changed = prompt.knownFingerprints.isNotEmpty;
    final muted = theme.textTheme.bodySmall?.copyWith(color: scheme.onSurfaceVariant);
    Widget fingerprint(String label, String value) => Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(label, style: muted),
        const SizedBox(height: 2),
        SelectableText(clean(value), style: monoStyle(context, size: 12.5)),
      ],
    );
    return (
      changed ? t('Hostschlüssel hat sich geändert', 'Host key has changed') : t('Unbekannter Host', 'Unknown host'),
      [
        if (changed)
          Container(
            padding: const EdgeInsets.all(12),
            decoration: BoxDecoration(color: scheme.errorContainer, borderRadius: BorderRadius.circular(8)),
            child: Text(
              t(
                'Der Server weist sich mit einem anderen Schlüssel aus als bisher. Das kann auf einen Angriff hindeuten. Vertraue dem neuen Schlüssel nur, wenn du weißt, warum er sich geändert hat.',
                'The server presents a different key than before. This may indicate an attack. Trust the new key only if you know why it changed.',
              ),
              style: TextStyle(color: scheme.onErrorContainer),
            ),
          )
        else
          Text(
            t(
              'Mit diesem Server wurde noch keine Verbindung aufgebaut. Vergleiche den Fingerabdruck mit dem des Servers, bevor du ihm vertraust.',
              'There has been no connection to this server before. Compare the fingerprint with the server’s before you trust it.',
            ),
          ),
        fingerprint('Host', '${prompt.host}:${prompt.port}'),
        fingerprint('${t('Fingerabdruck', 'Fingerprint')} (${clean(prompt.algorithm)})', prompt.fingerprint),
        for (final known in prompt.knownFingerprints) fingerprint(t('Bisher bekannt', 'Previously known'), known),
      ],
      changed ? t('Trotzdem vertrauen', 'Trust anyway') : t('Vertrauen', 'Trust'),
    );
  }
}
