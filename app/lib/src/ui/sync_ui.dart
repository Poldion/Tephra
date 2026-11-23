import 'package:material_ui/material_ui.dart';

import '../files.dart' show formatTime;
import '../rust/api/sync.dart';
import '../sync.dart';
import '../text.dart';
import 'common.dart';

/// Was die App sagt, wenn eine unverschlüsselte Adresse eingegeben, aber nicht erlaubt wurde.
String get plainHttpRefused => t(
  'Die Adresse ist unverschlüsselt (http). Setze den Haken unter der Adresse, wenn der Server nur in deinem eigenen Netz erreichbar ist – oder verwende eine https-Adresse.',
  'The address is unencrypted (http). Tick the box below the address if the server can only be reached in your own network – or use an https address.',
);

/// Haken, mit dem eine unverschlüsselte Serveradresse ausdrücklich erlaubt wird. Er erscheint nur, wenn die
/// eingegebene Adresse eine ist.
class PlainHttpConsent extends StatelessWidget {
  const PlainHttpConsent({
    super.key,
    required this.server,
    required this.value,
    required this.onChanged,
    this.enabled = true,
  });

  final TextEditingController server;
  final bool value;
  final ValueChanged<bool> onChanged;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: server,
      builder: (context, _) {
        if (!syncIsUnencrypted(server: server.text)) return const SizedBox.shrink();
        return Padding(
          padding: const EdgeInsets.only(top: 6),
          child: CheckboxListTile(
            key: const Key('sync-allow-http'),
            value: value,
            onChanged: enabled ? (checked) => onChanged(checked ?? false) : null,
            contentPadding: EdgeInsets.zero,
            controlAffinity: ListTileControlAffinity.leading,
            dense: true,
            title: Text(t('Unverschlüsselt verbinden', 'Connect without encryption')),
            subtitle: Text(
              t(
                'Nur für einen Server im eigenen Netz. Wer in diesem Netz mitliest, kann sich am Server als dein Gerät anmelden. Der Inhalt des Tresors bleibt verschlüsselt.',
                'Only for a server in your own network. Whoever listens in on that network can sign in to the server as your device. The content of the vault stays encrypted.',
              ),
            ),
          ),
        );
      },
    );
  }
}

/// Lädt die Liste einer Seite neu, sobald der Abgleich Daten von anderen Geräten gebracht hat.
mixin ReloadOnSync<T extends StatefulWidget> on State<T> {
  /// Liest die angezeigten Daten neu aus dem Tresor.
  void reloadFromVault();

  @override
  void initState() {
    super.initState();
    syncState.remoteChanges.addListener(_remoteChanged);
  }

  @override
  void dispose() {
    syncState.remoteChanges.removeListener(_remoteChanged);
    super.dispose();
  }

  void _remoteChanged() {
    if (mounted) setState(reloadFromVault);
  }
}

/// Zeigt den Zustand des Abgleichs und öffnet seinen Dialog.
class SyncButton extends StatelessWidget {
  const SyncButton({super.key});

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: syncState,
      builder: (context, _) {
        final failed = syncState.connection != null && syncState.error != null;
        final icon = syncState.connection == null
            ? Icons.cloud_outlined
            : failed
            ? Icons.cloud_off_outlined
            : syncState.working
            ? Icons.cloud_sync_outlined
            : Icons.cloud_done_outlined;
        return IconButton(
          key: const Key('sync-button'),
          tooltip: t('Abgleich', 'Sync'),
          icon: Badge(isLabelVisible: failed, backgroundColor: Theme.of(context).colorScheme.error, child: Icon(icon)),
          onPressed: () => showDialog<void>(context: context, builder: (_) => const SyncDialog()),
        );
      },
    );
  }
}

/// Abgleich einrichten und verwalten.
class SyncDialog extends StatefulWidget {
  const SyncDialog({super.key});

  @override
  State<SyncDialog> createState() => _SyncDialogState();
}

class _SyncDialogState extends State<SyncDialog> {
  final _server = TextEditingController();
  final _username = TextEditingController();
  final _invite = TextEditingController();
  final _password = TextEditingController();
  bool _allowHttp = false;
  bool _busy = false;
  String? _error;
  Future<List<SyncDevice>>? _devices;

  @override
  void initState() {
    super.initState();
    if (syncState.connection != null && !syncState.expired) _devices = syncState.devices();
  }

  @override
  void dispose() {
    _server.dispose();
    _username.dispose();
    _invite.dispose();
    _password.dispose();
    super.dispose();
  }

  /// Führt eine Aktion aus und zeigt ihren Fehler im Dialog.
  Future<bool> _run(Future<void> Function() action) async {
    if (_busy) return false;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await action();
      return true;
    } catch (error) {
      if (mounted) setState(() => _error = errorText(error));
      return false;
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _register() async {
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
    SyncSummary? summary;
    final ok = await _run(() async {
      summary = await syncState.register(
        server: _server.text,
        username: _username.text,
        password: _password.text,
        invite: _invite.text.trim().isEmpty ? null : _invite.text.trim(),
        allowHttp: _allowHttp,
      );
    });
    if (!ok || !mounted) return;
    _password.clear();
    setState(() {
      _devices = syncState.devices();
    });
    final count = summary?.pushed ?? 0;
    showMessage(
      context,
      t('Konto angelegt, $count Datensätze hochgeladen.', 'Account created, $count records uploaded.'),
    );
  }

  Future<void> _reconnect() async {
    if (_password.text.isEmpty) return;
    final ok = await _run(() => syncState.reconnect(_password.text));
    if (!ok || !mounted) return;
    _password.clear();
    setState(() {
      _devices = syncState.devices();
    });
  }

  Future<void> _disconnect() async {
    final ok = await confirm(
      context,
      title: t('Vom Server trennen?', 'Disconnect from the server?'),
      body: t(
        'Dieses Gerät gleicht dann nicht mehr ab. Der Tresor bleibt hier vollständig erhalten, die Daten auf dem Server ebenfalls.',
        'This device will stop syncing. The vault stays here in full, and so does the data on the server.',
      ),
      action: t('Trennen', 'Disconnect'),
    );
    if (!ok || !mounted) return;
    await _run(syncState.disconnect);
    if (mounted) setState(() => _devices = null);
  }

  Future<void> _removeDevice(SyncDevice device) async {
    final ok = await confirm(
      context,
      title: t('Gerät entfernen?', 'Remove device?'),
      body: t(
        '„${device.name}“ kann danach nicht mehr abgleichen. Was es schon abgeholt hat, bleibt dort lesbar – ging das Gerät verloren, erneuere die Passwörter und Schlüssel deiner Hosts.',
        '"${device.name}" will no longer be able to sync. What it already fetched stays readable there – if the device was lost, renew the passwords and keys of your hosts.',
      ),
      action: t('Entfernen', 'Remove'),
      destructive: true,
    );
    if (!ok || !mounted) return;
    await _run(() => syncState.removeDevice(device.id));
    if (!mounted) return;
    setState(() {
      _devices = syncState.devices();
    });
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: syncState,
      builder: (context, _) {
        final connection = syncState.connection;
        final theme = Theme.of(context);
        return AlertDialog(
          title: Text(t('Abgleich', 'Sync')),
          scrollable: true,
          content: SizedBox(
            width: 460,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (connection == null) ..._setup(theme) else ..._status(theme, connection),
                if (_error != null) ...[
                  const SizedBox(height: 12),
                  Text(
                    _error!,
                    key: const Key('sync-error'),
                    style: TextStyle(color: theme.colorScheme.error),
                  ),
                ],
              ],
            ),
          ),
          actions: [
            if (connection != null)
              TextButton(
                key: const Key('sync-disconnect'),
                onPressed: _busy ? null : _disconnect,
                child: Text(t('Trennen', 'Disconnect')),
              ),
            TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Schließen', 'Close'))),
            if (connection == null)
              FilledButton(
                key: const Key('sync-register'),
                onPressed: _busy ? null : _register,
                child: Text(t('Konto anlegen', 'Create account')),
              )
            else if (!syncState.expired)
              FilledButton(
                key: const Key('sync-now'),
                onPressed: _busy || syncState.working ? null : syncState.syncNow,
                child: Text(t('Jetzt abgleichen', 'Sync now')),
              ),
          ],
        );
      },
    );
  }

  List<Widget> _setup(ThemeData theme) {
    final muted = theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant);
    return [
      Text(
        t(
          'Mit einem Sync-Server haben alle deine Geräte dieselben Hosts, Identitäten, Schlüssel und VPN-Profile. Der Server bekommt nur verschlüsselte Daten.',
          'With a sync server all your devices share the same hosts, identities, keys and VPN profiles. The server only ever receives encrypted data.',
        ),
        style: muted,
      ),
      const SizedBox(height: 16),
      TextField(
        key: const Key('sync-server'),
        controller: _server,
        enabled: !_busy,
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
        key: const Key('sync-username'),
        controller: _username,
        enabled: !_busy,
        autocorrect: false,
        decoration: InputDecoration(labelText: t('Benutzername', 'Username')),
      ),
      const SizedBox(height: 14),
      TextField(
        key: const Key('sync-invite'),
        controller: _invite,
        enabled: !_busy,
        autocorrect: false,
        decoration: InputDecoration(
          labelText: t('Einladungscode', 'Invite code'),
          helperText: t(
            'Vom Betreiber des Servers. Bei offener Registrierung leer lassen.',
            'From whoever runs the server. Leave empty if registration is open.',
          ),
        ),
      ),
      const SizedBox(height: 14),
      SecretField(
        key: const Key('sync-password'),
        controller: _password,
        label: t('Master-Passwort', 'Master password'),
        helper: t(
          'Das Master-Passwort dieses Tresors. Es wird auch das Passwort des Kontos; an den Server geht nur ein daraus abgeleiteter Nachweis.',
          'The master password of this vault. It becomes the password of the account too; the server only receives a proof derived from it.',
        ),
        enabled: !_busy,
        onSubmitted: (_) => _register(),
      ),
      const SizedBox(height: 16),
      Text(
        t(
          'Das Konto gibt es schon? Ein weiteres Gerät holt den Tresor ab, solange es selbst noch keinen hat: dort beim ersten Start „Vorhandenen Tresor abholen“ wählen.',
          'The account already exists? Another device fetches the vault as long as it has none of its own: choose "Fetch an existing vault" there on first start.',
        ),
        style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
      ),
    ];
  }

  List<Widget> _status(ThemeData theme, SyncConnection connection) {
    final muted = theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant);
    final lastSync = syncState.lastSync;
    return [
      Text(connection.server, key: const Key('sync-connected'), style: theme.textTheme.titleMedium),
      const SizedBox(height: 2),
      Text(t('Angemeldet als ${connection.username}', 'Signed in as ${connection.username}'), style: muted),
      if (syncIsUnencrypted(server: connection.server))
        Text(
          t('Unverschlüsselte Verbindung (http)', 'Unencrypted connection (http)'),
          key: const Key('sync-unencrypted'),
          style: muted,
        ),
      const SizedBox(height: 12),
      if (syncState.error != null)
        Text(syncState.error!, style: TextStyle(color: theme.colorScheme.error))
      else if (syncState.working || lastSync == null)
        Text(t('Abgleich läuft …', 'Syncing …'), style: muted)
      else
        Text(
          t('Zuletzt abgeglichen: ${formatTime(lastSync)}', 'Last synced: ${formatTime(lastSync)}'),
          key: const Key('sync-done'),
          style: muted,
        ),
      if (syncState.skipped > 0) ...[
        const SizedBox(height: 4),
        Text(
          t(
            '${syncState.skipped} Datensätze ließen sich nicht abgleichen (unlesbar oder vom Server abgelehnt).',
            '${syncState.skipped} records could not be synced (unreadable or rejected by the server).',
          ),
          style: TextStyle(color: theme.colorScheme.error),
        ),
      ],
      if (syncState.expired) ...[
        const SizedBox(height: 16),
        SecretField(
          key: const Key('sync-password'),
          controller: _password,
          label: t('Master-Passwort des Kontos', 'Master password of the account'),
          helper: t(
            'Die Anmeldung dieses Geräts gilt nicht mehr – etwa weil das Master-Passwort auf einem anderen Gerät geändert wurde.',
            'This device is no longer signed in – for example because the master password was changed on another device.',
          ),
          enabled: !_busy,
          onSubmitted: (_) => _reconnect(),
        ),
        const SizedBox(height: 12),
        Align(
          alignment: Alignment.centerRight,
          child: FilledButton.tonal(
            onPressed: _busy ? null : _reconnect,
            child: Text(t('Neu anmelden', 'Sign in again')),
          ),
        ),
      ] else ...[
        const SizedBox(height: 16),
        Text(t('Geräte', 'Devices'), style: theme.textTheme.titleSmall),
        const SizedBox(height: 4),
        FutureBuilder<List<SyncDevice>>(
          future: _devices,
          builder: (context, snapshot) {
            if (snapshot.hasError) {
              return Text(errorText(snapshot.error!), style: TextStyle(color: theme.colorScheme.error));
            }
            final loaded = snapshot.data;
            if (loaded == null) {
              return const Padding(padding: EdgeInsets.symmetric(vertical: 12), child: LinearProgressIndicator());
            }
            // Dieses Gerät zuerst, die übrigen nach ihrem letzten Abgleich.
            final devices = [...loaded]
              ..sort((a, b) {
                if (a.current != b.current) return a.current ? -1 : 1;
                return b.lastSeenMs.toInt().compareTo(a.lastSeenMs.toInt());
              });
            return Column(
              children: [
                for (final device in devices)
                  ListTile(
                    contentPadding: EdgeInsets.zero,
                    dense: true,
                    leading: Icon(device.current ? Icons.smartphone_rounded : Icons.devices_other_rounded),
                    title: Text(clean(device.name)),
                    subtitle: Text(
                      device.current
                          ? t('Dieses Gerät', 'This device')
                          : t(
                              'Zuletzt abgeglichen: ${formatTime(DateTime.fromMillisecondsSinceEpoch(device.lastSeenMs.toInt()))}',
                              'Last synced: ${formatTime(DateTime.fromMillisecondsSinceEpoch(device.lastSeenMs.toInt()))}',
                            ),
                    ),
                    trailing: device.current
                        ? null
                        : IconButton(
                            tooltip: t('Gerät entfernen', 'Remove device'),
                            icon: const Icon(Icons.delete_outline_rounded),
                            onPressed: _busy ? null : () => _removeDevice(device),
                          ),
                  ),
              ],
            );
          },
        ),
      ],
    ];
  }
}
