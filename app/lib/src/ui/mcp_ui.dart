import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../files.dart' show formatTime;
import '../mcp.dart';
import '../paths.dart';
import '../rust/api/app.dart';
import '../rust/api/mcp.dart';
import '../settings.dart';
import '../text.dart';
import 'common.dart';

String levelLabel(McpLevel level) => switch (level) {
  McpLevel.readOnly => t('Nur lesen', 'Read-only'),
  McpLevel.confirm => t('Mit Bestätigung', 'Confirm each'),
  McpLevel.auto => t('Automatisch', 'Automatic'),
};

String _levelHint(McpLevel level, {required bool ownPatterns}) => switch (level) {
  McpLevel.readOnly when ownPatterns => t(
    'Achtung: Mit eigenen Mustern entfällt die eingebaute Prüfung einzelner Optionen. Erlaubt ist dann alles, was auf ein Muster passt.',
    'Careful: with patterns of your own the built-in check of single options no longer applies. Everything that matches a pattern is allowed.',
  ),
  McpLevel.readOnly => t(
    'Eine feste Auswahl anzeigender Programme, ohne Rückfrage. Das Modell kann damit alles lesen, was der angemeldete Benutzer lesen darf; nur Pfade, die nach Zugangsdaten aussehen, werden abgelehnt.',
    'A fixed set of inspecting programs, without asking. The model can read whatever the signed-in user may read; only paths that look like credentials are refused.',
  ),
  McpLevel.confirm => t(
    'Jeder Befehl braucht dein Ja in dieser App. Befehle, die auf ein Muster unten passen, laufen ohne Rückfrage.',
    'Every command needs your yes in this app. Commands that match a pattern below run without asking.',
  ),
  McpLevel.auto => t(
    'Achtung: Das Modell führt Befehle ohne Rückfrage aus. Die Sperrliste verhindert Versehen, keinen gezielten Missbrauch.',
    'Careful: the model runs commands without asking. The deny list prevents accidents, not deliberate misuse.',
  ),
};

/// Der Befehl, mit dem ein MCP-Client die Brücke startet: die Kommandozeile neben der App, sonst die im `PATH`.
String _bridgeCommand() {
  final own = File(Platform.resolvedExecutable);
  final sibling = File('${own.parent.path}${Platform.pathSeparator}${Platform.isWindows ? 'tephra.exe' : 'tephra'}');
  return sibling.existsSync() && sibling.path.toLowerCase() != own.path.toLowerCase() ? sibling.path : 'tephra';
}

/// Eintrag für die Konfiguration eines MCP-Clients im verbreiteten JSON-Format.
String mcpConfigSnippet(String token) => const JsonEncoder.withIndent('  ').convert({
  'mcpServers': {
    'tephra': {
      'command': _bridgeCommand(),
      'args': ['mcp', 'serve'],
      'env': {'TEPHRA_MCP_TOKEN': token, 'TEPHRA_VAULT': vaultPath},
    },
  },
});

/// Eintrag für einen MCP-Client auf einem anderen Gerät: Dort liegt `tephra` im `PATH` und der Tresor am üblichen
/// Ort.
String remoteConfigSnippet(String token) => const JsonEncoder.withIndent('  ').convert({
  'mcpServers': {
    'tephra': {
      'command': 'tephra',
      'args': ['mcp', 'serve'],
      'env': {'TEPHRA_MCP_TOKEN': token},
    },
  },
});

/// MCP: der Dienst für Sprachmodelle, seine Clients und deren Freigaben – die dieses Geräts oder, mit [device],
/// die eines fernverwalteten Geräts. Auf dem Telefon, das selbst keinen Dienst anbietet, nur die Geräte.
class McpPage extends StatefulWidget {
  const McpPage({super.key, this.device});

  /// Ein fernverwaltetes Gerät (aus [mcpDevices]); `null` für dieses Gerät.
  final McpDeviceView? device;

  @override
  State<McpPage> createState() => _McpPageState();
}

class _McpPageState extends State<McpPage> {
  McpOverview? _overview;
  List<McpDeviceView> _devices = const [];
  String? _error;
  Timer? _refresh;

  String? get _device => widget.device?.id;

  /// Ob die Seite die Freigaben eines Geräts zeigt (dieses oder ein fernverwaltetes) statt nur die Geräteliste.
  bool get _policy => widget.device != null || mcpState.supported;

  @override
  void initState() {
    super.initState();
    _reload();
    // Was andere Geräte ändern, kommt mit dem Abgleich; die Seite zeigt es von selbst.
    _refresh = Timer.periodic(const Duration(seconds: 5), (_) {
      if (mounted && ModalRoute.of(context)?.isCurrent == true) setState(_reload);
    });
  }

  @override
  void dispose() {
    _refresh?.cancel();
    super.dispose();
  }

  void _reload() {
    try {
      _overview = _policy ? mcpOverview(device: _device) : null;
      _devices = widget.device == null ? mcpDevices().where((d) => !d.thisDevice).toList() : const [];
      _error = null;
    } catch (error) {
      _error = errorText(error);
    }
  }

  Future<void> _openDevice(McpDeviceView device) async {
    await Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => Scaffold(
          body: SafeArea(child: McpPage(device: device)),
        ),
      ),
    );
    if (mounted) setState(_reload);
  }

  Future<void> _removeDevice() async {
    final device = widget.device!;
    final ok = await confirm(
      context,
      title: t('Aus der Verwaltung nehmen?', 'Remove from management?'),
      body: t(
        'Auf „${device.name}“ ist danach kein Zugriff über MCP mehr erlaubt, bis du dort „tephra mcp remote on“ erneut ausführst. Die Freigaben gehen verloren.',
        'After that no access through MCP is allowed on "${device.name}" until you run "tephra mcp remote on" there again. Its access rules are lost.',
      ),
      action: t('Entfernen', 'Remove'),
      destructive: true,
    );
    if (!ok || !mounted) return;
    if (attempt(context, () => mcpRemoveDevice(device: device.id))) Navigator.of(context).pop();
  }

  void _change(void Function() action) {
    attempt(context, action);
    setState(_reload);
  }

  Future<void> _addClient() async {
    final created = await showDialog<(String, String)>(
      context: context,
      builder: (_) => _ClientDialog(device: _device),
    );
    if (created == null || !mounted) return;
    setState(_reload);
    await showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (_) => _TokenDialog(name: created.$1, token: created.$2, device: widget.device?.name),
    );
  }

  Future<void> _removeClient(McpClient client) async {
    final ok = await confirm(
      context,
      title: t('Client entfernen?', 'Remove client?'),
      body: t(
        '„${client.name}“ verliert alle Freigaben, und sein Token wird ungültig. Offene Sitzungen dieses Clients enden.',
        '"${client.name}" loses all its hosts and its token stops working. Open sessions of this client end.',
      ),
      action: t('Entfernen', 'Remove'),
      destructive: true,
    );
    if (ok && mounted) _change(() => mcpRemoveClient(device: _device, clientId: client.id));
  }

  Future<void> _grant(McpClient client, [McpGrant? existing]) async {
    final List<HostView> hosts;
    try {
      hosts = listHosts();
    } catch (error) {
      showMessage(context, errorText(error), error: true);
      return;
    }
    final free = hosts.where((host) => existing?.hostId == host.id || !client.grants.any((g) => g.hostId == host.id));
    if (existing == null && free.isEmpty) {
      showMessage(
        context,
        hosts.isEmpty
            ? t('Lege zuerst einen Host an.', 'Add a host first.')
            : t('Alle Hosts sind diesem Client schon freigegeben.', 'This client already has access to every host.'),
      );
      return;
    }
    final saved = await showDialog<bool>(
      context: context,
      builder: (_) => _GrantDialog(client: client, hosts: free.toList(), existing: existing, device: _device),
    );
    if (saved == true && mounted) setState(_reload);
  }

  Future<void> _revoke(McpClient client, McpGrant grant) async {
    final host = grant.hostLabel.isEmpty ? t('(gelöschter Host)', '(deleted host)') : grant.hostLabel;
    final ok = await confirm(
      context,
      title: t('Freigabe entziehen?', 'Withdraw access?'),
      body: t(
        '„${client.name}“ kann danach nicht mehr auf „$host“ zugreifen. Offene Sitzungen dorthin enden.',
        '"${client.name}" will no longer be able to reach "$host". Open sessions there end.',
      ),
      action: t('Entziehen', 'Withdraw'),
      destructive: true,
    );
    if (ok && mounted) _change(() => mcpRevoke(device: _device, clientId: client.id, hostId: grant.hostId));
  }

  @override
  Widget build(BuildContext context) {
    final overview = _overview;
    final device = widget.device;
    return SectionPage(
      title: device == null ? 'MCP' : device.name,
      back: device != null,
      actions: [
        if (device == null && mcpState.supported)
          OutlinedButton.icon(
            key: const Key('mcp-audit'),
            icon: const Icon(Icons.receipt_long_outlined),
            label: Text(t('Protokoll', 'Log')),
            onPressed: () => showDialog<void>(context: context, builder: (_) => const _AuditDialog()),
          ),
        if (device != null)
          OutlinedButton.icon(
            key: const Key('mcp-remove-device'),
            icon: const Icon(Icons.link_off_rounded),
            label: Text(t('Entfernen', 'Remove')),
            onPressed: _removeDevice,
          ),
        if (_policy)
          FilledButton.icon(
            key: const Key('mcp-add-client'),
            icon: const Icon(Icons.add_rounded),
            label: Text(t('Client anlegen', 'Add client')),
            onPressed: _addClient,
          ),
      ],
      child: _error != null || (_policy && overview == null)
          ? EmptyState(icon: Icons.error_outline_rounded, title: t('Fehler', 'Error'), hint: _error ?? '')
          : overview == null
          ? ListView(padding: const EdgeInsets.only(bottom: 20), children: [_devicesCard(context)])
          : ListView(
              padding: const EdgeInsets.only(bottom: 20),
              children: [
                if (device != null) ...[_deviceInfo(context, device), const SizedBox(height: 8)],
                _ServiceCard(
                  service: device == null,
                  enabled: overview.enabled,
                  onEnabled: (v) => _change(() => mcpSetEnabled(device: _device, enabled: v)),
                  maxTimeout: overview.maxTimeoutSecs,
                  onMaxTimeout: (secs) => _change(() => mcpSetMaxTimeout(device: _device, secs: secs)),
                ),
                if (device == null) ...[const SizedBox(height: 8), _devicesCard(context)],
                const SizedBox(height: 8),
                if (overview.clients.isEmpty)
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 28, horizontal: 12),
                    child: Text(
                      t(
                        'Noch kein Client. Jedes Programm, das sich verbindet – etwa ein Chat-Programm oder ein Assistent im Editor –, bekommt einen eigenen Namen und ein eigenes Token. Danach gibst du ihm einzelne Hosts frei.',
                        'No client yet. Every program that connects – such as a chat program or an assistant in your editor – gets a name and a token of its own. After that you give it access to single hosts.',
                      ),
                      textAlign: TextAlign.center,
                      style: TextStyle(color: Theme.of(context).colorScheme.onSurfaceVariant),
                    ),
                  ),
                for (final client in overview.clients) ...[_clientCard(context, client), const SizedBox(height: 8)],
              ],
            ),
    );
  }

  Widget _clientCard(BuildContext context, McpClient client) {
    final theme = Theme.of(context);
    final muted = theme.colorScheme.onSurfaceVariant;
    return Card(
      key: Key('mcp-client-${client.name}'),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 8, 4, 10),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(Icons.smart_toy_outlined, color: client.disabled ? muted : theme.colorScheme.primary),
                const SizedBox(width: 16),
                Expanded(
                  child: Text(
                    client.disabled ? '${client.name}  ·  ${t('gesperrt', 'blocked')}' : client.name,
                    style: theme.textTheme.titleMedium,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                TextButton.icon(
                  key: Key('mcp-grant-${client.name}'),
                  icon: const Icon(Icons.add_rounded),
                  label: Text(t('Host freigeben', 'Share a host')),
                  onPressed: () => _grant(client),
                ),
                PopupMenuButton<String>(
                  tooltip: t('Mehr', 'More'),
                  onSelected: (action) => action == 'remove'
                      ? _removeClient(client)
                      : _change(
                          () => mcpSetClientDisabled(device: _device, clientId: client.id, disabled: !client.disabled),
                        ),
                  itemBuilder: (_) => [
                    PopupMenuItem(
                      value: 'block',
                      child: Text(client.disabled ? t('Wieder zulassen', 'Allow again') : t('Sperren', 'Block')),
                    ),
                    PopupMenuItem(value: 'remove', child: Text(t('Entfernen', 'Remove'))),
                  ],
                ),
              ],
            ),
            if (client.grants.isEmpty)
              Padding(
                padding: const EdgeInsets.only(left: 40, top: 2, bottom: 6),
                child: Text(t('Noch kein Host freigegeben.', 'No host shared yet.'), style: TextStyle(color: muted)),
              ),
            for (final grant in client.grants)
              Padding(
                padding: const EdgeInsets.only(left: 40),
                child: Row(
                  children: [
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(grant.hostLabel.isEmpty ? t('(gelöschter Host)', '(deleted host)') : grant.hostLabel),
                          Text(_grantSummary(grant), style: theme.textTheme.bodySmall?.copyWith(color: muted)),
                        ],
                      ),
                    ),
                    if (grant.hostLabel.isNotEmpty)
                      IconButton(
                        tooltip: t('Freigabe ändern', 'Change access'),
                        icon: const Icon(Icons.edit_outlined),
                        onPressed: () => _grant(client, grant),
                      ),
                    IconButton(
                      tooltip: t('Freigabe entziehen', 'Withdraw access'),
                      icon: const Icon(Icons.delete_outline_rounded),
                      onPressed: () => _revoke(client, grant),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }

  /// Name, letztes Lebenszeichen und Version eines fernverwalteten Geräts.
  Widget _deviceInfo(BuildContext context, McpDeviceView device) {
    final muted = Theme.of(context).colorScheme.onSurfaceVariant;
    return Card(
      child: ListTile(
        leading: const Icon(Icons.dns_outlined),
        title: Text(t('Fernverwaltetes Gerät', 'Remotely managed device')),
        subtitle: Text(
          '${_seen(device)} · Tephra ${device.version}\n${t('Änderungen gelten dort, sobald es abgeglichen hat. Rückfragen „Mit Bestätigung“ erscheinen hier.', 'Changes apply there as soon as it has synced. Questions for "Confirm each" appear here.')}',
          style: TextStyle(color: muted),
        ),
        isThreeLine: true,
      ),
    );
  }

  static String _seen(McpDeviceView device) {
    if (device.seenMs <= 0) return t('noch nie gesehen', 'never seen');
    final ago = DateTime.now().difference(DateTime.fromMillisecondsSinceEpoch(device.seenMs));
    if (ago.inMinutes < 2) return t('gerade aktiv', 'active just now');
    if (ago.inHours < 2) return t('zuletzt vor ${ago.inMinutes} min', 'last seen ${ago.inMinutes} min ago');
    final when = formatTime(DateTime.fromMillisecondsSinceEpoch(device.seenMs));
    return t('zuletzt $when', 'last seen $when');
  }

  /// Die fernverwalteten Geräte des Tresors, oder wie man eins dazu bekommt.
  Widget _devicesCard(BuildContext context) {
    final theme = Theme.of(context);
    final muted = theme.colorScheme.onSurfaceVariant;
    return Card(
      key: const Key('mcp-devices'),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            ListTile(
              leading: const Icon(Icons.devices_other_rounded),
              title: Text(t('Andere Geräte', 'Other devices')),
              subtitle: Text(
                _devices.isEmpty
                    ? t(
                        'Noch keins. Auf einem Gerät mit „tephra daemon“ – etwa einem Container ohne Bildschirm – stimmst du einmal mit „tephra mcp remote on“ zu. Danach stellst du seine Freigaben hier ein, und seine Rückfragen erscheinen hier.',
                        'None yet. On a device running "tephra daemon" – such as a container without a screen – agree once with "tephra mcp remote on". Then you set its access here, and its questions appear here.',
                      )
                    : t(
                        'Geräte, die der Fernverwaltung zugestimmt haben. Ihre Freigaben stellst du hier ein.',
                        'Devices that agreed to remote management. You set their access here.',
                      ),
                style: TextStyle(color: muted),
              ),
            ),
            for (final device in _devices)
              ListTile(
                key: Key('mcp-device-${device.name}'),
                contentPadding: const EdgeInsets.only(left: 72, right: 16),
                title: Text(device.name),
                subtitle: Text(
                  [
                    if (!device.enabled) t('Not-Aus', 'kill switch on'),
                    t('${device.clients} Client(s)', '${device.clients} client(s)'),
                    _seen(device),
                  ].join(' · '),
                  style: TextStyle(color: device.enabled ? muted : theme.colorScheme.error),
                ),
                trailing: const Icon(Icons.chevron_right_rounded),
                onTap: () => _openDevice(device),
              ),
          ],
        ),
      ),
    );
  }

  String _grantSummary(McpGrant grant) {
    final files = switch ((grant.filesRead, grant.filesWrite)) {
      (true, true) => t('Dateien lesen und schreiben', 'read and write files'),
      (true, false) => t('Dateien lesen', 'read files'),
      (false, true) => t('Dateien schreiben', 'write files'),
      (false, false) => null,
    };
    final patterns = grant.allow.length + grant.deny.length;
    return [
      levelLabel(grant.level),
      ?files,
      if (patterns > 0) t('$patterns eigene Muster', '$patterns patterns of your own'),
    ].join(' · ');
  }
}

/// Schalter für den Dienst und der Not-Aus.
class _ServiceCard extends StatelessWidget {
  const _ServiceCard({
    required this.service,
    required this.enabled,
    required this.onEnabled,
    required this.maxTimeout,
    required this.onMaxTimeout,
  });

  /// Ob der Schalter für den Dienst dieser App dazugehört (nicht bei einem fernverwalteten Gerät).
  final bool service;
  final bool enabled;
  final ValueChanged<bool> onEnabled;
  final int maxTimeout;
  final ValueChanged<int> onMaxTimeout;

  static String _duration(int secs) => switch (secs) {
    < 3600 when secs % 60 == 0 => t('${secs ~/ 60} Minuten', '${secs ~/ 60} minutes'),
    3600 => t('1 Stunde', '1 hour'),
    _ when secs % 3600 == 0 => t('${secs ~/ 3600} Stunden', '${secs ~/ 3600} hours'),
    _ => t('$secs Sekunden', '$secs seconds'),
  };

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return ListenableBuilder(
      listenable: Listenable.merge([mcpState, settings]),
      builder: (context, _) {
        final on = settings.mcpService;
        final error = mcpState.error;
        final clients = mcpState.clients;
        final status = !on
            ? t(
                'Aus. Eingeschaltet läuft er, solange der Tresor offen ist – auch nach dem nächsten Start der App.',
                'Off. Switched on it runs while the vault is open – also after the next start of the app.',
              )
            : error ??
                  (!mcpState.running
                      ? t('Startet …', 'Starting …')
                      : clients.isEmpty
                      ? t('Läuft. Kein Client verbunden.', 'Running. No client connected.')
                      : t('Läuft. Verbunden: ${clients.join(', ')}', 'Running. Connected: ${clients.join(', ')}'));
        return Card(
          child: Column(
            children: [
              if (service)
                SwitchListTile(
                  key: const Key('mcp-service'),
                  value: on,
                  onChanged: mcpState.setEnabled,
                  secondary: const Icon(Icons.hub_outlined),
                  title: Text(t('Dienst für Sprachmodelle', 'Service for language models')),
                  subtitle: Text(
                    status,
                    key: const Key('mcp-status'),
                    style: TextStyle(color: on && error != null ? scheme.error : null),
                  ),
                ),
              SwitchListTile(
                key: const Key('mcp-enabled'),
                value: enabled,
                onChanged: onEnabled,
                secondary: const Icon(Icons.power_settings_new_rounded),
                title: Text(t('Zugriff über MCP erlaubt', 'Access through MCP allowed')),
                subtitle: Text(
                  t(
                    'Der Not-Aus: Ausgeschaltet wird jeder Zugriff abgelehnt, und laufende Sitzungen der Modelle enden binnen Sekunden.',
                    'The kill switch: switched off, every access is refused and running sessions of the models end within seconds.',
                  ),
                ),
              ),
              ListTile(
                leading: const Icon(Icons.timer_outlined),
                title: Text(t('Längste Laufzeit eines Befehls', 'Longest run time of a command')),
                subtitle: Text(
                  t(
                    'Das Modell wählt die Laufzeit je Befehl selbst, höchstens so lange. Ohne Angabe ist nach einer Minute Schluss.',
                    'The model chooses the run time of each command itself, at most this long. Without a choice a command stops after one minute.',
                  ),
                ),
                trailing: DropdownButton<int>(
                  key: const Key('mcp-max-timeout'),
                  value: maxTimeout,
                  underline: const SizedBox.shrink(),
                  onChanged: (secs) {
                    if (secs != null) onMaxTimeout(secs);
                  },
                  items: [
                    for (final secs in {600, 1800, 3600, 7200, 21600, 86400, maxTimeout}.toList()..sort())
                      DropdownMenuItem(value: secs, child: Text(_duration(secs))),
                  ],
                ),
              ),
            ],
          ),
        );
      },
    );
  }
}

class _ClientDialog extends StatefulWidget {
  const _ClientDialog({this.device});

  final String? device;

  @override
  State<_ClientDialog> createState() => _ClientDialogState();
}

class _ClientDialogState extends State<_ClientDialog> {
  final _name = TextEditingController();
  String? _error;

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  void _save() {
    try {
      final name = _name.text.trim();
      final token = mcpAddClient(device: widget.device, name: name);
      Navigator.pop(context, (name, token));
    } catch (error) {
      setState(() => _error = errorText(error));
    }
  }

  @override
  Widget build(BuildContext context) {
    return FormDialog(
      title: t('Client anlegen', 'Add client'),
      action: t('Anlegen', 'Add'),
      onSubmit: _save,
      children: [
        TextField(
          key: const Key('mcp-client-name'),
          controller: _name,
          autofocus: true,
          decoration: InputDecoration(
            labelText: t('Name', 'Name'),
            helperText: t(
              'Für dich zum Wiedererkennen, etwa „desktop“. Er steht später in jeder Rückfrage.',
              'For you to recognise it, for example "desktop". It appears in every question later.',
            ),
          ),
          onSubmitted: (_) => _save(),
        ),
        if (_error != null) Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
      ],
    );
  }
}

/// Zeigt das Token eines neuen Clients – das einzige Mal, dass es zu sehen ist.
class _TokenDialog extends StatelessWidget {
  const _TokenDialog({required this.name, required this.token, this.device});

  final String name;
  final String token;

  /// Name des fernverwalteten Geräts, für das der Client angelegt wurde; `null` für dieses Gerät.
  final String? device;

  Future<void> _copy(BuildContext context, String text, String message) async {
    await Clipboard.setData(ClipboardData(text: text));
    if (context.mounted) showMessage(context, message);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final snippet = device == null ? mcpConfigSnippet(token) : remoteConfigSnippet(token);
    Widget block(String text, {Key? key}) => Container(
      width: double.infinity,
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: theme.colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(8),
      ),
      child: SelectableText(text, key: key, style: monoStyle(context, size: 12)),
    );
    return AlertDialog(
      title: Text(t('Token für „$name“', 'Token for "$name"')),
      scrollable: true,
      content: SizedBox(
        width: 520,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              t(
                'Das Token ist nur jetzt zu sehen; der Tresor behält davon bloß einen Hash. Trage es im MCP-Client ein und gib dem Client danach Hosts frei.',
                'The token is shown only now; the vault keeps nothing but a hash of it. Enter it in the MCP client, then share hosts with the client.',
              ),
            ),
            const SizedBox(height: 12),
            block(token, key: const Key('mcp-token')),
            const SizedBox(height: 16),
            Text(
              device == null
                  ? t('Eintrag für die Konfiguration des Clients:', 'Entry for the configuration of the client:')
                  : t(
                      'Eintrag für die Konfiguration des Clients auf „$device“. Das Gerät übernimmt den neuen Client mit dem nächsten Abgleich.',
                      'Entry for the configuration of the client on "$device". The device picks up the new client with its next sync.',
                    ),
            ),
            const SizedBox(height: 8),
            block(snippet),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => _copy(context, token, t('Token kopiert.', 'Token copied.')),
          child: Text(t('Token kopieren', 'Copy token')),
        ),
        TextButton(
          onPressed: () => _copy(context, snippet, t('Eintrag kopiert.', 'Entry copied.')),
          child: Text(t('Eintrag kopieren', 'Copy entry')),
        ),
        FilledButton(
          key: const Key('mcp-token-done'),
          onPressed: () => Navigator.pop(context),
          child: Text(t('Fertig', 'Done')),
        ),
      ],
    );
  }
}

/// Gibt einem Client einen Host frei oder ändert eine Freigabe.
class _GrantDialog extends StatefulWidget {
  const _GrantDialog({required this.client, required this.hosts, this.existing, this.device});

  final String? device;
  final McpClient client;
  final List<HostView> hosts;
  final McpGrant? existing;

  @override
  State<_GrantDialog> createState() => _GrantDialogState();
}

class _GrantDialogState extends State<_GrantDialog> {
  late String? _host = widget.existing?.hostId ?? (widget.hosts.length == 1 ? widget.hosts.first.id : null);
  late McpLevel _level = widget.existing?.level ?? McpLevel.confirm;
  late final _allow = TextEditingController(text: widget.existing?.allow.join('\n') ?? '');
  late final _deny = TextEditingController(text: widget.existing?.deny.join('\n') ?? '');
  late bool _filesRead = widget.existing?.filesRead ?? false;
  late bool _filesWrite = widget.existing?.filesWrite ?? false;
  String? _error;

  @override
  void dispose() {
    _allow.dispose();
    _deny.dispose();
    super.dispose();
  }

  List<String> _patterns(TextEditingController field) =>
      field.text.split('\n').map((line) => line.trim()).where((line) => line.isNotEmpty).toList();

  void _save() {
    final host = _host;
    if (host == null) {
      setState(() => _error = t('Wähle einen Host.', 'Choose a host.'));
      return;
    }
    try {
      mcpGrant(
        device: widget.device,
        clientId: widget.client.id,
        hostId: host,
        level: _level,
        allow: _patterns(_allow),
        // Die Sperrliste gilt nur für die Stufe „Automatisch“.
        deny: _level == McpLevel.auto ? _patterns(_deny) : const [],
        filesRead: _filesRead,
        // Schreiben gibt es mit „Nur lesen“ nie.
        filesWrite: _level != McpLevel.readOnly && _filesWrite,
      );
      Navigator.pop(context, true);
    } catch (error) {
      setState(() => _error = errorText(error));
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final warn = _level == McpLevel.auto || (_level == McpLevel.readOnly && _patterns(_allow).isNotEmpty);
    return FormDialog(
      title: widget.existing == null
          ? t('Host für „${widget.client.name}“ freigeben', 'Share a host with "${widget.client.name}"')
          : t('Freigabe ändern', 'Change access'),
      action: widget.existing == null ? t('Freigeben', 'Share') : t('Speichern', 'Save'),
      onSubmit: _save,
      children: [
        DropdownButtonFormField<String>(
          key: const Key('mcp-grant-host'),
          initialValue: _host,
          decoration: const InputDecoration(labelText: 'Host'),
          items: [for (final host in widget.hosts) DropdownMenuItem(value: host.id, child: Text(host.label))],
          onChanged: widget.existing != null ? null : (value) => setState(() => _host = value),
        ),
        DropdownButtonFormField<McpLevel>(
          key: const Key('mcp-grant-level'),
          initialValue: _level,
          decoration: InputDecoration(labelText: t('Zugriffsstufe', 'Access level')),
          items: [for (final level in McpLevel.values) DropdownMenuItem(value: level, child: Text(levelLabel(level)))],
          onChanged: (value) => setState(() => _level = value ?? _level),
        ),
        Text(
          _levelHint(_level, ownPatterns: _patterns(_allow).isNotEmpty),
          style: theme.textTheme.bodySmall?.copyWith(
            color: warn ? theme.colorScheme.error : theme.colorScheme.onSurfaceVariant,
          ),
        ),
        TextField(
          controller: _allow,
          minLines: 2,
          maxLines: 5,
          autocorrect: false,
          style: monoStyle(context),
          onChanged: (_) => setState(() {}),
          decoration: InputDecoration(
            labelText: switch (_level) {
              McpLevel.readOnly => t('Eigene erlaubte Befehle (optional)', 'Allowed commands of your own (optional)'),
              McpLevel.confirm => t('Ohne Rückfrage erlaubt (optional)', 'Allowed without asking (optional)'),
              McpLevel.auto => t('Erlaubte Befehle (optional)', 'Allowed commands (optional)'),
            },
            helperText: t(
              'Ein Muster je Zeile; * steht für beliebige Zeichen, etwa: systemctl status *',
              'One pattern per line; * stands for any characters, for example: systemctl status *',
            ),
          ),
        ),
        if (_level == McpLevel.auto)
          TextField(
            controller: _deny,
            minLines: 2,
            maxLines: 5,
            autocorrect: false,
            style: monoStyle(context),
            decoration: InputDecoration(
              labelText: t('Zusätzlich gesperrt (optional)', 'Additionally denied (optional)'),
              helperText: t(
                'Kommt zur eingebauten Sperrliste hinzu. Ein Muster je Zeile.',
                'Added to the built-in deny list. One pattern per line.',
              ),
            ),
          ),
        CheckboxListTile(
          value: _filesRead,
          onChanged: (value) => setState(() => _filesRead = value ?? false),
          contentPadding: EdgeInsets.zero,
          controlAffinity: ListTileControlAffinity.leading,
          dense: true,
          title: Text(t('Dateien lesen (SFTP)', 'Read files (SFTP)')),
        ),
        if (_level != McpLevel.readOnly)
          CheckboxListTile(
            value: _filesWrite,
            onChanged: (value) => setState(() => _filesWrite = value ?? false),
            contentPadding: EdgeInsets.zero,
            controlAffinity: ListTileControlAffinity.leading,
            dense: true,
            title: Text(t('Dateien schreiben (SFTP)', 'Write files (SFTP)')),
          ),
        if (_error != null) Text(_error!, style: TextStyle(color: theme.colorScheme.error)),
      ],
    );
  }
}

/// Die jüngsten Einträge des Protokolls.
class _AuditDialog extends StatelessWidget {
  const _AuditDialog();

  static String _outcome(McpOutcome outcome) => switch (outcome) {
    McpOutcome.allowed => t('erlaubt', 'allowed'),
    McpOutcome.approved => t('bestätigt', 'approved'),
    McpOutcome.denied => t('abgelehnt', 'refused'),
    McpOutcome.rejected => t('von dir abgelehnt', 'refused by you'),
    McpOutcome.failed => t('fehlgeschlagen', 'failed'),
    McpOutcome.completed => t('beendet', 'finished'),
  };

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    List<McpAuditEntry> entries = const [];
    String? error;
    try {
      entries = mcpAudit(limit: 200);
    } catch (failure) {
      error = errorText(failure);
    }
    return AlertDialog(
      title: Text(t('Protokoll der Zugriffe', 'Access log')),
      content: SizedBox(
        width: 640,
        height: 460,
        child: error != null
            ? Text(error, style: TextStyle(color: theme.colorScheme.error))
            : entries.isEmpty
            ? Center(child: Text(t('Noch keine Zugriffe protokolliert.', 'Nothing logged yet.')))
            : ListView.separated(
                itemCount: entries.length,
                separatorBuilder: (_, _) => const Divider(height: 12),
                itemBuilder: (context, index) {
                  final entry = entries[index];
                  final bad = entry.outcome == McpOutcome.denied || entry.outcome == McpOutcome.rejected;
                  final exit = entry.exitStatus == null ? '' : ' → ${entry.exitStatus}';
                  final time = formatTime(DateTime.fromMillisecondsSinceEpoch(entry.timeMs.toInt()));
                  return Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '$time  ·  ${entry.client} → ${entry.host}  ·  ${entry.tool}  ·  ${_outcome(entry.outcome)}$exit',
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: bad ? theme.colorScheme.error : theme.colorScheme.onSurfaceVariant,
                        ),
                      ),
                      if (entry.detail.isNotEmpty) Text(entry.detail, style: monoStyle(context, size: 12)),
                      if (entry.note.isNotEmpty) Text(entry.note, style: theme.textTheme.bodySmall),
                    ],
                  );
                },
              ),
      ),
      actions: [TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Schließen', 'Close')))],
    );
  }
}

// ---------------------------------------------------------------------------- Rückfragen

/// Zeigt die Rückfragen des Dienstes als Dialog über der App, gleich welcher Bereich gerade offen ist.
class McpApprovals extends StatefulWidget {
  const McpApprovals({super.key, required this.child});

  final Widget child;

  @override
  State<McpApprovals> createState() => _McpApprovalsState();
}

class _McpApprovalsState extends State<McpApprovals> {
  int? _shown;

  @override
  void initState() {
    super.initState();
    mcpState.request.addListener(_changed);
    WidgetsBinding.instance.addPostFrameCallback((_) => _changed());
  }

  @override
  void dispose() {
    mcpState.request.removeListener(_changed);
    super.dispose();
  }

  Future<void> _changed() async {
    final asked = mcpState.request.value;
    if (asked == null || !mounted || _shown == asked.id) return;
    _shown = asked.id;
    mcpRequestAttention();
    final reply = await showDialog<McpAnswer>(
      context: context,
      barrierDismissible: false,
      builder: (_) => _ApprovalDialog(request: asked),
    );
    // Ohne ausdrückliche Antwort gilt eine Anfrage des eigenen Dienstes als abgelehnt. Die eines anderen Geräts
    // hat sich dann anderswo erledigt oder kommt gleich wieder (eine eigene ging vor).
    if (reply != null) {
      mcpState.answer(asked, reply);
    } else if (asked.device.isEmpty) {
      mcpState.answer(asked, McpAnswer.deny);
    } else {
      _shown = null;
    }
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

class _ApprovalDialog extends StatefulWidget {
  const _ApprovalDialog({required this.request});

  final McpRequest request;

  @override
  State<_ApprovalDialog> createState() => _ApprovalDialogState();
}

class _ApprovalDialogState extends State<_ApprovalDialog> {
  /// Erst nach einem Augenblick lässt sich antworten: Ein Klick oder Tastendruck, der schon unterwegs war,
  /// galt nicht dieser Frage.
  bool _ready = false;
  bool _closed = false;
  Timer? _settle;

  /// Ob die grauen Kästen bis zum Ende zu sehen waren. Erst dann lässt sich erlauben – sonst könnte unten im
  /// Kasten etwas stehen, das niemand gelesen hat.
  bool _subjectSeen = false;
  bool _extraSeen = false;

  @override
  void initState() {
    super.initState();
    _settle = Timer(const Duration(milliseconds: 800), () {
      if (mounted) setState(() => _ready = true);
    });
    mcpState.request.addListener(_withdrawn);
  }

  @override
  void dispose() {
    _settle?.cancel();
    mcpState.request.removeListener(_withdrawn);
    super.dispose();
  }

  /// Schließt den Dialog, wenn die Frage sich erledigt hat – etwa weil die Frist abgelaufen ist.
  void _withdrawn() {
    if (_closed || !mounted || mcpState.request.value?.id == widget.request.id) return;
    if (ModalRoute.of(context)?.isCurrent != true) return;
    _closed = true;
    Navigator.pop(context);
  }

  void _answer(McpAnswer reply) {
    if (_closed) return;
    _closed = true;
    Navigator.pop(context, reply);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final request = widget.request;
    Widget block(String text, {Key? key, required VoidCallback onSeen}) =>
        _GatedBlock(text: text, textKey: key, onSeen: onSeen);
    final size = request.bytes;
    final hasExtra = request.extra != null;
    final allSeen = (request.subject.isEmpty || _subjectSeen) && (!hasExtra || _extraSeen);
    final canAllow = _ready && allSeen;
    final lines = request.subjectLines;
    final (String what, String? more) = switch (request.kind) {
      McpRequestKind.command => (
        t('Befehl ausführen:', 'Run a command:'),
        request.extra == null ? null : t('mit $size Byte Eingabedaten:', 'with $size bytes of input:'),
      ),
      McpRequestKind.session => (t('Interaktive Sitzung öffnen.', 'Open an interactive session.'), null),
      McpRequestKind.fileRead => (t('Datei lesen:', 'Read a file:'), null),
      McpRequestKind.fileWrite => (
        t(
          'Datei schreiben ($size Byte, SHA-256 ${request.checksum}…):',
          'Write a file ($size bytes, SHA-256 ${request.checksum}…):',
        ),
        t('Inhalt (Anfang):', 'Content (beginning):'),
      ),
      McpRequestKind.hostKey => (t('Unbekanntem Hostschlüssel vertrauen:', 'Trust an unknown host key:'), null),
    };
    return AlertDialog(
      key: const Key('mcp-approval'),
      title: Text(
        request.device.isEmpty
            ? t(
                'Anfrage von „${request.client}“ für „${request.host}“',
                'Request from "${request.client}" for "${request.host}"',
              )
            : t(
                'Anfrage auf „${request.device}“ von „${request.client}“ für „${request.host}“',
                'Request on "${request.device}" from "${request.client}" for "${request.host}"',
              ),
      ),
      scrollable: true,
      content: SizedBox(
        width: 520,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(what),
            if (request.subject.isNotEmpty && request.kind == McpRequestKind.command && lines > 1) ...[
              const SizedBox(height: 4),
              Text(
                t(
                  'Achtung: $lines Zeilen, ${request.subjectChars} Zeichen – jede Zeile wird ausgeführt.',
                  'Careful: $lines lines, ${request.subjectChars} characters – every line runs.',
                ),
                key: const Key('mcp-approval-lines'),
                style: TextStyle(color: theme.colorScheme.error, fontWeight: FontWeight.w600),
              ),
            ],
            if (request.subject.isNotEmpty) ...[
              const SizedBox(height: 8),
              block(
                request.subject,
                key: const Key('mcp-approval-subject'),
                onSeen: () => setState(() => _subjectSeen = true),
              ),
            ],
            if (request.kind == McpRequestKind.session) ...[
              const SizedBox(height: 8),
              Text(
                t(
                  'Achtung: Danach sind alle Eingaben des Modells in dieser Sitzung frei – es wird nicht mehr einzeln gefragt.',
                  'Careful: after that everything the model types in this session is free – there will be no further questions.',
                ),
                style: TextStyle(color: theme.colorScheme.error),
              ),
            ],
            if (more != null && request.extra != null) ...[
              const SizedBox(height: 12),
              Text(more),
              const SizedBox(height: 8),
              block(request.extra!, onSeen: () => setState(() => _extraSeen = true)),
            ],
            if (!allSeen) ...[
              const SizedBox(height: 8),
              Text(
                t(
                  'Scrolle im grauen Feld bis zum Ende, um erlauben zu können.',
                  'Scroll to the end of the grey box to be able to allow.',
                ),
                key: const Key('mcp-approval-scroll'),
                style: TextStyle(color: theme.colorScheme.error),
              ),
            ],
            const SizedBox(height: 12),
            Text(
              t(
                'Was im grauen Feld steht, stammt vom Modell. Ohne Antwort wird die Anfrage nach zwei Minuten abgelehnt.',
                'What is shown in the grey box comes from the model. Without an answer the request is refused after two minutes.',
              ),
              style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          key: const Key('mcp-approval-session'),
          onPressed: canAllow ? () => _answer(McpAnswer.session) : null,
          child: Text(t('Genau dies für diese Verbindung', 'Exactly this for this connection')),
        ),
        TextButton(
          key: const Key('mcp-approval-once'),
          onPressed: canAllow ? () => _answer(McpAnswer.once) : null,
          child: Text(t('Einmal erlauben', 'Allow once')),
        ),
        FilledButton(
          key: const Key('mcp-approval-deny'),
          autofocus: true,
          onPressed: () => _answer(McpAnswer.deny),
          child: Text(t('Ablehnen', 'Refuse')),
        ),
      ],
    );
  }
}

/// Grauer Kasten mit Text vom Modell. Meldet einmal, sobald sein Ende zu sehen war – sofort, wenn alles
/// hineinpasst, sonst nach dem Scrollen bis unten.
class _GatedBlock extends StatefulWidget {
  const _GatedBlock({required this.text, required this.onSeen, this.textKey});

  final String text;
  final Key? textKey;
  final VoidCallback onSeen;

  @override
  State<_GatedBlock> createState() => _GatedBlockState();
}

class _GatedBlockState extends State<_GatedBlock> {
  final _scroll = ScrollController();
  bool _reported = false;

  @override
  void initState() {
    super.initState();
    _scroll.addListener(_check);
    WidgetsBinding.instance.addPostFrameCallback((_) => _check());
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  void _check() {
    if (_reported || !mounted || !_scroll.hasClients) return;
    final position = _scroll.position;
    if (position.pixels >= position.maxScrollExtent - 4) {
      _reported = true;
      widget.onSeen();
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      width: double.infinity,
      constraints: const BoxConstraints(maxHeight: 220),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: theme.colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Scrollbar(
        controller: _scroll,
        thumbVisibility: true,
        child: SingleChildScrollView(
          controller: _scroll,
          child: SelectableText(widget.text, key: widget.textKey, style: monoStyle(context)),
        ),
      ),
    );
  }
}
