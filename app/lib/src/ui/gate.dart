import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../paths.dart';
import '../rust/api/app.dart';
import '../text.dart';
import 'common.dart';

/// Anmeldung: Tresor anlegen oder entsperren.
class GateScreen extends StatefulWidget {
  const GateScreen({super.key, required this.onUnlocked});

  final VoidCallback onUnlocked;

  @override
  State<GateScreen> createState() => _GateScreenState();
}

class _GateScreenState extends State<GateScreen> {
  final _path = vaultPath;
  late bool _exists = vaultExists(path: _path);
  final _password = TextEditingController();
  final _repeat = TextEditingController();
  bool _busy = false;
  String? _error;

  /// Nach dem Anlegen: der Wiederherstellungsschlüssel, der einmalig gezeigt wird.
  String? _recoveryKey;
  bool _noted = false;

  @override
  void dispose() {
    _password.dispose();
    _repeat.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (_busy) return;
    final password = _password.text;
    if (!_exists) {
      if (password.length < 8) {
        setState(() => _error = t('Mindestens 8 Zeichen.', 'At least 8 characters.'));
        return;
      }
      if (password != _repeat.text) {
        setState(() => _error = t('Die Passwörter stimmen nicht überein.', 'The passwords do not match.'));
        return;
      }
    } else if (password.isEmpty) {
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      if (_exists) {
        await unlockVault(path: _path, password: password);
        _password.clear();
        widget.onUnlocked();
      } else {
        final key = await createVault(path: _path, password: password);
        _password.clear();
        _repeat.clear();
        if (mounted) {
          setState(() {
            _recoveryKey = key;
            _exists = true;
          });
        }
      }
    } catch (error) {
      if (mounted) setState(() => _error = errorText(error));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      body: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(24),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 420),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Icon(Icons.terminal_rounded, size: 56, color: theme.colorScheme.primary),
                const SizedBox(height: 12),
                Text(appName, style: theme.textTheme.headlineMedium, textAlign: TextAlign.center),
                const SizedBox(height: 28),
                if (_recoveryKey != null) ..._recovery(theme) else ..._form(theme),
              ],
            ),
          ),
        ),
      ),
    );
  }

  List<Widget> _form(ThemeData theme) {
    final muted = theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant);
    return [
      Text(
        _exists ? t('Tresor entsperren', 'Unlock vault') : t('Neuen Tresor anlegen', 'Create a new vault'),
        style: theme.textTheme.titleLarge,
      ),
      const SizedBox(height: 6),
      Text(
        _exists
            ? t('Gib dein Master-Passwort ein.', 'Enter your master password.')
            : t(
                'Das Master-Passwort verschlüsselt alle Zugänge. Es wird nirgends gespeichert und lässt sich nicht zurücksetzen.',
                'The master password encrypts all your credentials. It is not stored anywhere and cannot be reset.',
              ),
        style: muted,
      ),
      const SizedBox(height: 20),
      SecretField(
        controller: _password,
        label: t('Master-Passwort', 'Master password'),
        autofocus: true,
        enabled: !_busy,
        onSubmitted: (_) => _exists ? _submit() : null,
      ),
      if (!_exists) ...[
        const SizedBox(height: 14),
        SecretField(
          controller: _repeat,
          label: t('Passwort wiederholen', 'Repeat password'),
          enabled: !_busy,
          onSubmitted: (_) => _submit(),
        ),
      ],
      if (_error != null) ...[
        const SizedBox(height: 12),
        Text(_error!, style: TextStyle(color: theme.colorScheme.error)),
      ],
      const SizedBox(height: 20),
      FilledButton(
        onPressed: _busy ? null : _submit,
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 10),
          child: _busy
              ? const SizedBox(width: 20, height: 20, child: CircularProgressIndicator(strokeWidth: 2.5))
              : Text(_exists ? t('Entsperren', 'Unlock') : t('Tresor anlegen', 'Create vault')),
        ),
      ),
      const SizedBox(height: 16),
      Text(
        _path,
        style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.outline),
        textAlign: TextAlign.center,
      ),
    ];
  }

  List<Widget> _recovery(ThemeData theme) {
    final key = _recoveryKey!;
    return [
      Text(t('Wiederherstellungsschlüssel', 'Recovery key'), style: theme.textTheme.titleLarge),
      const SizedBox(height: 6),
      Text(
        t(
          'Mit diesem Schlüssel kommst du an deinen Tresor, falls du das Master-Passwort vergisst. Er wird nur jetzt angezeigt. Bewahre ihn getrennt von diesem Gerät auf.',
          'This key gets you back into your vault if you forget the master password. It is shown only now. Keep it somewhere apart from this device.',
        ),
        style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant),
      ),
      const SizedBox(height: 16),
      Container(
        padding: const EdgeInsets.all(16),
        decoration: BoxDecoration(
          color: theme.colorScheme.surfaceContainerHighest,
          borderRadius: BorderRadius.circular(12),
        ),
        child: SelectableText(key, style: monoStyle(context, size: 15), textAlign: TextAlign.center),
      ),
      const SizedBox(height: 8),
      Align(
        alignment: Alignment.centerRight,
        child: TextButton.icon(
          icon: const Icon(Icons.copy_rounded, size: 18),
          label: Text(t('Kopieren', 'Copy')),
          onPressed: () async {
            await Clipboard.setData(ClipboardData(text: key));
            if (mounted) showMessage(context, t('In die Zwischenablage kopiert.', 'Copied to the clipboard.'));
          },
        ),
      ),
      CheckboxListTile(
        value: _noted,
        onChanged: (value) => setState(() => _noted = value ?? false),
        controlAffinity: ListTileControlAffinity.leading,
        contentPadding: EdgeInsets.zero,
        title: Text(t('Ich habe den Schlüssel sicher notiert.', 'I have stored the key safely.')),
      ),
      const SizedBox(height: 12),
      FilledButton(
        onPressed: _noted
            ? () {
                _recoveryKey = null;
                widget.onUnlocked();
              }
            : null,
        child: Padding(padding: const EdgeInsets.symmetric(vertical: 10), child: Text(t('Weiter', 'Continue'))),
      ),
    ];
  }
}
