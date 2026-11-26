import 'package:flutter_rust_bridge/flutter_rust_bridge.dart';
import 'package:material_ui/material_ui.dart';

import '../text.dart';

/// Lesbarer Text einer Fehlermeldung aus dem Kern.
String errorText(Object error) {
  var text = error is AnyhowException ? error.message : error.toString();
  final trace = text.indexOf('Stack backtrace');
  if (trace > 0) text = text.substring(0, trace);
  return clean(text).trim();
}

void showMessage(BuildContext context, String text, {bool error = false}) {
  final scheme = Theme.of(context).colorScheme;
  ScaffoldMessenger.of(context)
    ..hideCurrentSnackBar()
    ..showSnackBar(
      SnackBar(
        content: Text(text, style: TextStyle(color: error ? scheme.onErrorContainer : null)),
        backgroundColor: error ? scheme.errorContainer : null,
        behavior: SnackBarBehavior.floating,
        width: 520,
        duration: Duration(seconds: error ? 8 : 4),
        showCloseIcon: error,
        closeIconColor: scheme.onErrorContainer,
      ),
    );
}

/// Führt eine Aktion aus und zeigt einen Fehler an, statt ihn weiterzureichen.
bool attempt(BuildContext context, void Function() action) {
  try {
    action();
    return true;
  } catch (error) {
    showMessage(context, errorText(error), error: true);
    return false;
  }
}

Future<bool> confirm(
  BuildContext context, {
  required String title,
  required String body,
  required String action,
  bool destructive = false,
}) async {
  final scheme = Theme.of(context).colorScheme;
  final result = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(title),
      content: ConstrainedBox(constraints: const BoxConstraints(maxWidth: 420), child: Text(body)),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(t('Abbrechen', 'Cancel'))),
        FilledButton(
          style: destructive
              ? FilledButton.styleFrom(backgroundColor: scheme.error, foregroundColor: scheme.onError)
              : null,
          onPressed: () => Navigator.pop(context, true),
          child: Text(action),
        ),
      ],
    ),
  );
  return result ?? false;
}

/// Eingabefeld für Geheimnisse mit Schalter zum Anzeigen.
class SecretField extends StatefulWidget {
  const SecretField({
    super.key,
    required this.controller,
    required this.label,
    this.helper,
    this.autofocus = false,
    this.onSubmitted,
    this.enabled = true,
  });

  final TextEditingController controller;
  final String label;
  final String? helper;
  final bool autofocus;
  final ValueChanged<String>? onSubmitted;
  final bool enabled;

  @override
  State<SecretField> createState() => _SecretFieldState();
}

class _SecretFieldState extends State<SecretField> {
  bool _visible = false;

  @override
  Widget build(BuildContext context) {
    return TextField(
      controller: widget.controller,
      obscureText: !_visible,
      autofocus: widget.autofocus,
      enabled: widget.enabled,
      enableSuggestions: false,
      autocorrect: false,
      onSubmitted: widget.onSubmitted,
      decoration: InputDecoration(
        labelText: widget.label,
        helperText: widget.helper,
        helperMaxLines: 3,
        suffixIcon: IconButton(
          tooltip: _visible ? t('Verbergen', 'Hide') : t('Anzeigen', 'Show'),
          icon: Icon(_visible ? Icons.visibility_off_outlined : Icons.visibility_outlined),
          onPressed: () => setState(() => _visible = !_visible),
        ),
      ),
    );
  }
}

/// Rahmen eines Verwaltungsbildschirms: Überschrift, Aktionen, Inhalt.
class SectionPage extends StatelessWidget {
  const SectionPage({super.key, required this.title, this.actions = const [], required this.child, this.back = false});

  final String title;

  /// Mit einem Knopf „Zurück“ vor dem Titel, für Seiten, die über einer anderen liegen.
  final bool back;
  final List<Widget> actions;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Wrap(
            alignment: WrapAlignment.spaceBetween,
            crossAxisAlignment: WrapCrossAlignment.center,
            spacing: 12,
            runSpacing: 8,
            children: [
              Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  if (back) ...[const BackButton(), const SizedBox(width: 4)],
                  Text(title, style: Theme.of(context).textTheme.headlineSmall),
                ],
              ),
              Wrap(spacing: 8, runSpacing: 8, children: actions),
            ],
          ),
          const SizedBox(height: 16),
          Expanded(child: child),
        ],
      ),
    );
  }
}

/// Platzhalter für leere Listen.
class EmptyState extends StatelessWidget {
  const EmptyState({super.key, required this.icon, required this.title, required this.hint});

  final IconData icon;
  final String title;
  final String hint;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 380),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 48, color: theme.colorScheme.outline),
            const SizedBox(height: 12),
            Text(title, style: theme.textTheme.titleMedium, textAlign: TextAlign.center),
            const SizedBox(height: 6),
            Text(
              hint,
              style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant),
              textAlign: TextAlign.center,
            ),
          ],
        ),
      ),
    );
  }
}

/// Dialog mit Formular, einheitlich für alle Editoren.
class FormDialog extends StatelessWidget {
  const FormDialog({
    super.key,
    required this.title,
    required this.children,
    required this.action,
    required this.onSubmit,
  });

  final String title;
  final List<Widget> children;
  final String action;
  final VoidCallback? onSubmit;

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(title),
      scrollable: true,
      content: SizedBox(
        width: 460,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            for (final (index, child) in children.indexed) ...[if (index > 0) const SizedBox(height: 14), child],
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(t('Abbrechen', 'Cancel'))),
        FilledButton(onPressed: onSubmit, child: Text(action)),
      ],
    );
  }
}

const monoFonts = ['Cascadia Mono', 'Consolas', 'DejaVu Sans Mono', 'Liberation Mono', 'Noto Sans Mono', 'monospace'];

TextStyle monoStyle(BuildContext context, {double size = 13}) =>
    TextStyle(fontFamily: monoFonts.first, fontFamilyFallback: monoFonts, fontSize: size, height: 1.35);
