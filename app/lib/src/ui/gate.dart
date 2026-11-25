import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../device_unlock.dart';
import '../paths.dart';
import '../rust/api/app.dart';
import '../rust/api/sync.dart' show syncIsUnencrypted;
import '../sync.dart';
import '../text.dart';
import 'common.dart';
import 'logo.dart';
import 'sync_ui.dart';

/// Anmeldung: Tresor anlegen, von einem Sync-Server abholen oder entsperren.
class GateScreen extends StatefulWidget {
  const GateScreen({super.key, required this.onUnlocked, this.autoUnlock = false});

  final VoidCallback onUnlocked;

  /// Ob der Tresor ohne Zutun aufgeht, wenn dieses Gerät ihn ohne Passwort entsperren darf – beim Start der App,
  /// nicht nach dem Sperren.
  final bool autoUnlock;

  @override
  State<GateScreen> createState() => _GateScreenState();
}

class _GateScreenState extends State<GateScreen> {
  final _path = vaultPath;
  late bool _exists = vaultExists(path: _path);
  final _password = TextEditingController();
  final _repeat = TextEditingController();
  final _server = TextEditingController();
  final _username = TextEditingController();

  /// Statt einen neuen Tresor anzulegen: den eines bestehenden Kontos vom Sync-Server abholen.
  bool _joining = false;
  bool _allowHttp = false;
  bool _busy = false;
  String? _error;

  /// Nach dem Anlegen: der Wiederherstellungsschlüssel, der einmalig gezeigt wird.
  String? _recoveryKey;
  bool _noted = false;

  /// Ob dieses Gerät den Tresor ohne Master-Passwort entsperren darf.
  bool _onDevice = false;

  @override
  void initState() {
    super.initState();
    if (_exists && deviceUnlock.supported) _checkDevice();
  }

  Future<void> _checkDevice() async {
    if (!await deviceUnlock.refresh() || !mounted) return;
    setState(() => _onDevice = true);
    if (widget.autoUnlock) await _unlockOnDevice();
  }

  Future<void> _unlockOnDevice() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await deviceUnlock.unlock();
      widget.onUnlocked();
    } catch (error) {
      // Ein Schlüssel, der nicht mehr passt oder sich nicht mehr öffnen lässt, wird nicht wieder versucht.
      await deviceUnlock.disable();
      if (mounted) {
        setState(() {
          _onDevice = false;
          _error = t(
            'Entsperren ohne Passwort ging nicht (${errorText(error)}). Es ist auf diesem Gerät jetzt abgeschaltet; gib das Master-Passwort ein und schalte es bei Bedarf unter „Über $appName“ wieder ein.',
            'Unlocking without the password failed (${errorText(error)}). It is now switched off on this device; enter the master password and switch it on again under "About $appName" if you like.',
          );
        });
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  void dispose() {
    _password.dispose();
    _repeat.dispose();
    _server.dispose();
    _username.dispose();
    super.dispose();
  }

  void _setJoining(bool value) {
    setState(() {
      _joining = value;
      _error = null;
      _password.clear();
      _repeat.clear();
    });
  }

  Future<void> _join() async {
    if (_busy) return;
    if (_server.text.trim().isEmpty || _username.text.trim().isEmpty || _password.text.isEmpty) {
      setState(
        () => _error = t(
          'Serveradresse, Benutzername und Master-Passwort werden gebraucht.',
          'Server address, username and master password are required.',
        ),
      );
      return;
    }
    if (syncIsUnencrypted(server: _server.text) && !_allowHttp) {
      setState(() => _error = plainHttpRefused);
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      // Ein abgelegter Schlüssel gehört zu einem Tresor, den es hier nicht mehr gibt.
      if (deviceUnlock.supported) await deviceUnlock.disable();
      await syncState.join(
        server: _server.text,
        username: _username.text,
        password: _password.text,
        allowHttp: _allowHttp,
      );
      _password.clear();
      widget.onUnlocked();
    } catch (error) {
      if (mounted) {
        setState(() {
          _error = errorText(error);
          // Scheitert erst das Abholen der Daten, gibt es den Tresor schon: Er lässt sich entsperren.
          _exists = vaultExists(path: _path);
          if (_exists) _joining = false;
        });
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
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
        if (deviceUnlock.supported) await deviceUnlock.disable();
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
                const Center(child: TephraLogo(size: 64)),
                const SizedBox(height: 12),
                Text(appName, style: theme.textTheme.headlineMedium, textAlign: TextAlign.center),
                const SizedBox(height: 28),
                if (_recoveryKey != null)
                  ..._recovery(theme)
                else if (_joining)
                  ..._joinForm(theme)
                else
                  ..._form(theme),
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
      if (_exists && _onDevice) ...[
        FilledButton.tonalIcon(
          key: const Key('gate-device-unlock'),
          icon: const Icon(Icons.lock_open_rounded),
          label: Padding(
            padding: const EdgeInsets.symmetric(vertical: 10),
            child: Text(t('Ohne Passwort entsperren', 'Unlock without the password')),
          ),
          onPressed: _busy ? null : _unlockOnDevice,
        ),
        const SizedBox(height: 16),
      ],
      SecretField(
        controller: _password,
        label: t('Master-Passwort', 'Master password'),
        autofocus: !_onDevice,
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
      if (!_exists) ...[
        const SizedBox(height: 8),
        TextButton(
          key: const Key('gate-join'),
          onPressed: _busy ? null : () => _setJoining(true),
          child: Text(t('Vorhandenen Tresor abholen', 'Fetch an existing vault')),
        ),
      ],
      const SizedBox(height: 16),
      Text(
        _path,
        style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.outline),
        textAlign: TextAlign.center,
      ),
    ];
  }

  List<Widget> _joinForm(ThemeData theme) {
    return [
      Text(t('Vorhandenen Tresor abholen', 'Fetch an existing vault'), style: theme.textTheme.titleLarge),
      const SizedBox(height: 6),
      Text(
        t(
          'Du gleichst deinen Tresor schon auf einem anderen Gerät mit einem Sync-Server ab? Melde dieses Gerät an demselben Konto an; es holt dann alles ab.',
          'You already sync your vault with a sync server on another device? Sign this device in to the same account and it fetches everything.',
        ),
        style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant),
      ),
      const SizedBox(height: 20),
      TextField(
        key: const Key('join-server'),
        controller: _server,
        enabled: !_busy,
        autofocus: true,
        autocorrect: false,
        keyboardType: TextInputType.url,
        decoration: InputDecoration(
          labelText: t('Serveradresse', 'Server address'),
          hintText: 'https://sync.example.org',
        ),
      ),
      PlainHttpConsent(
        server: _server,
        value: _allowHttp,
        enabled: !_busy,
        onChanged: (value) => setState(() => _allowHttp = value),
      ),
      const SizedBox(height: 14),
      TextField(
        key: const Key('join-username'),
        controller: _username,
        enabled: !_busy,
        autocorrect: false,
        decoration: InputDecoration(labelText: t('Benutzername', 'Username')),
      ),
      const SizedBox(height: 14),
      SecretField(
        key: const Key('join-password'),
        controller: _password,
        label: t('Master-Passwort des Kontos', 'Master password of the account'),
        enabled: !_busy,
        onSubmitted: (_) => _join(),
      ),
      if (_error != null) ...[
        const SizedBox(height: 12),
        Text(_error!, style: TextStyle(color: theme.colorScheme.error)),
      ],
      const SizedBox(height: 20),
      FilledButton(
        key: const Key('join-submit'),
        onPressed: _busy ? null : _join,
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 10),
          child: _busy
              ? const SizedBox(width: 20, height: 20, child: CircularProgressIndicator(strokeWidth: 2.5))
              : Text(t('Abholen', 'Fetch')),
        ),
      ),
      const SizedBox(height: 8),
      TextButton(
        onPressed: _busy ? null : () => _setJoining(false),
        child: Text(t('Stattdessen einen neuen Tresor anlegen', 'Create a new vault instead')),
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
