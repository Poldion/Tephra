import 'package:material_ui/material_ui.dart';

import '../rust/api/app.dart';
import '../text.dart';
import 'common.dart';
import 'sync_ui.dart';

/// Liste der Hosts mit Suche; ein Klick verbindet.
class HostsPage extends StatefulWidget {
  const HostsPage({super.key, required this.onConnect});

  final ValueChanged<HostView> onConnect;

  @override
  State<HostsPage> createState() => _HostsPageState();
}

class _HostsPageState extends State<HostsPage> with ReloadOnSync<HostsPage> {
  @override
  void reloadFromVault() => _reload();

  List<HostView> _hosts = [];
  Map<String, String> _vpnNames = {};
  String _filter = '';
  String? _error;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  void _reload() {
    try {
      _hosts = listHosts();
      _vpnNames = {for (final profile in listVpnProfiles()) profile.id: profile.label};
      _error = null;
    } catch (error) {
      _error = errorText(error);
    }
  }

  Future<void> _edit(HostView? host) async {
    final saved = await showDialog<bool>(
      context: context,
      builder: (_) => HostEditor(host: host, hosts: _hosts),
    );
    if (saved == true && mounted) setState(_reload);
  }

  Future<void> _delete(HostView host) async {
    final ok = await confirm(
      context,
      title: t('Host löschen?', 'Delete host?'),
      body: t('„${host.label}“ wird endgültig gelöscht.', '"${host.label}" will be deleted permanently.'),
      action: t('Löschen', 'Delete'),
      destructive: true,
    );
    if (!ok || !mounted) return;
    attempt(context, () => deleteItem(id: host.id));
    setState(_reload);
  }

  void _toggleFavorite(HostView host) {
    attempt(
      context,
      () => saveHost(
        input: HostInput(
          id: host.id,
          label: host.label,
          address: host.address,
          port: host.port,
          identityId: host.identityId,
          vpnProfileId: host.vpnProfileId,
          jumpHostIds: host.jumpHostIds,
          tags: host.tags,
          notes: host.notes,
          favorite: !host.favorite,
          startupCommand: host.startupCommand,
          keepSession: host.keepSession,
        ),
      ),
    );
    setState(_reload);
  }

  bool _matches(HostView host) {
    final needle = _filter.trim().toLowerCase();
    if (needle.isEmpty) return true;
    return host.label.toLowerCase().contains(needle) ||
        host.address.toLowerCase().contains(needle) ||
        host.username.toLowerCase().contains(needle) ||
        host.tags.any((tag) => tag.toLowerCase().contains(needle));
  }

  @override
  Widget build(BuildContext context) {
    final visible = _hosts.where(_matches).toList();
    return SectionPage(
      title: 'Hosts',
      actions: [
        FilledButton.icon(
          icon: const Icon(Icons.add_rounded),
          label: Text(t('Host hinzufügen', 'Add host')),
          onPressed: () => _edit(null),
        ),
      ],
      child: _error != null
          ? EmptyState(icon: Icons.error_outline_rounded, title: t('Fehler', 'Error'), hint: _error!)
          : _hosts.isEmpty
          ? EmptyState(
              icon: Icons.dns_outlined,
              title: t('Noch keine Hosts', 'No hosts yet'),
              hint: t(
                'Lege einen Host an, um dich per SSH zu verbinden – direkt, über einen Sprunghost oder durch einen WireGuard-Tunnel.',
                'Add a host to connect over SSH – directly, through a jump host or through a WireGuard tunnel.',
              ),
            )
          : Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                TextField(
                  decoration: InputDecoration(
                    hintText: t('Suchen …', 'Search …'),
                    prefixIcon: const Icon(Icons.search_rounded),
                  ),
                  onChanged: (value) => setState(() => _filter = value),
                ),
                const SizedBox(height: 12),
                Expanded(
                  child: visible.isEmpty
                      ? EmptyState(
                          icon: Icons.search_off_rounded,
                          title: t('Nichts gefunden', 'Nothing found'),
                          hint: t('Kein Host passt zur Suche.', 'No host matches the search.'),
                        )
                      : ListView.separated(
                          padding: const EdgeInsets.only(bottom: 20),
                          itemCount: visible.length,
                          separatorBuilder: (_, _) => const SizedBox(height: 8),
                          itemBuilder: (context, index) => _tile(context, visible[index]),
                        ),
                ),
              ],
            ),
    );
  }

  Widget _tile(BuildContext context, HostView host) {
    final theme = Theme.of(context);
    final target = [
      if (host.username.isNotEmpty) '${host.username}@',
      host.address,
      if (host.port != 22) ':${host.port}',
    ].join();
    final jumps = [
      for (final id in host.jumpHostIds)
        _hosts.where((other) => other.id == id).map((other) => other.label).firstOrNull,
    ].nonNulls.toList();
    final marks = [
      if (host.vpnProfileId != null) (Icons.vpn_lock_rounded, _vpnNames[host.vpnProfileId] ?? 'VPN'),
      if (jumps.isNotEmpty) (Icons.alt_route_rounded, jumps.join(' → ')),
      for (final tag in host.tags) (Icons.sell_outlined, tag),
    ];
    return Card(
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: () => widget.onConnect(host),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(4, 8, 4, 8),
          child: Row(
            children: [
              IconButton(
                tooltip: host.favorite
                    ? t('Favorit entfernen', 'Remove favourite')
                    : t('Als Favorit merken', 'Mark as favourite'),
                icon: Icon(
                  host.favorite ? Icons.star_rounded : Icons.star_outline_rounded,
                  color: host.favorite ? theme.colorScheme.tertiary : theme.colorScheme.outline,
                ),
                onPressed: () => _toggleFavorite(host),
              ),
              const SizedBox(width: 4),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(host.label, style: theme.textTheme.titleMedium, overflow: TextOverflow.ellipsis),
                    Text(
                      target,
                      style: monoStyle(context, size: 12.5).copyWith(color: theme.colorScheme.onSurfaceVariant),
                      overflow: TextOverflow.ellipsis,
                    ),
                    if (marks.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.only(top: 6),
                        child: Wrap(
                          spacing: 12,
                          runSpacing: 4,
                          children: [
                            for (final (icon, text) in marks)
                              Row(
                                mainAxisSize: MainAxisSize.min,
                                children: [
                                  Icon(icon, size: 14, color: theme.colorScheme.outline),
                                  const SizedBox(width: 4),
                                  Text(text, style: theme.textTheme.bodySmall),
                                ],
                              ),
                          ],
                        ),
                      ),
                  ],
                ),
              ),
              PopupMenuButton<String>(
                tooltip: t('Mehr', 'More'),
                onSelected: (action) => switch (action) {
                  'connect' => widget.onConnect(host),
                  'edit' => _edit(host),
                  _ => _delete(host),
                },
                itemBuilder: (_) => [
                  PopupMenuItem(value: 'connect', child: Text(t('Verbinden', 'Connect'))),
                  PopupMenuItem(value: 'edit', child: Text(t('Bearbeiten', 'Edit'))),
                  PopupMenuItem(value: 'delete', child: Text(t('Löschen', 'Delete'))),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Dialog zum Anlegen und Ändern eines Hosts.
class HostEditor extends StatefulWidget {
  const HostEditor({super.key, this.host, required this.hosts});

  final HostView? host;
  final List<HostView> hosts;

  @override
  State<HostEditor> createState() => _HostEditorState();
}

class _HostEditorState extends State<HostEditor> {
  late final _label = TextEditingController(text: widget.host?.label ?? '');
  late final _address = TextEditingController(text: widget.host?.address ?? '');
  late final _port = TextEditingController(text: '${widget.host?.port ?? 22}');
  late final _tags = TextEditingController(text: widget.host?.tags.join(', ') ?? '');
  late final _notes = TextEditingController(text: widget.host?.notes ?? '');
  late final _startup = TextEditingController(text: widget.host?.startupCommand ?? '');
  late bool _keepSession = widget.host?.keepSession ?? false;
  late String? _identityId = widget.host?.identityId;
  late String? _vpnId = widget.host?.vpnProfileId;
  late final List<String> _jumps = [...?widget.host?.jumpHostIds];
  late bool _favorite = widget.host?.favorite ?? false;

  List<IdentityView> _identities = [];
  List<VpnView> _profiles = [];
  String? _error;

  @override
  void initState() {
    super.initState();
    try {
      _identities = listIdentities();
      _profiles = listVpnProfiles();
    } catch (error) {
      _error = errorText(error);
    }
    // Verweise auf inzwischen gelöschte Einträge nicht als Auswahl anbieten.
    if (!_identities.any((identity) => identity.id == _identityId)) _identityId = null;
    if (!_profiles.any((profile) => profile.id == _vpnId)) _vpnId = null;
    _jumps.removeWhere((id) => !widget.hosts.any((host) => host.id == id));
  }

  @override
  void dispose() {
    for (final controller in [_label, _address, _port, _tags, _notes, _startup]) {
      controller.dispose();
    }
    super.dispose();
  }

  void _save() {
    final port = int.tryParse(_port.text.trim());
    if (port == null || port < 1 || port > 65535) {
      setState(() => _error = t('Der Port muss zwischen 1 und 65535 liegen.', 'The port must be between 1 and 65535.'));
      return;
    }
    try {
      saveHost(
        input: HostInput(
          id: widget.host?.id,
          label: _label.text.trim().isEmpty ? _address.text.trim() : _label.text,
          address: _address.text,
          port: port,
          identityId: _identityId,
          vpnProfileId: _vpnId,
          jumpHostIds: _jumps,
          tags: _tags.text.split(','),
          notes: _notes.text,
          favorite: _favorite,
          startupCommand: _startup.text,
          keepSession: _keepSession,
        ),
      );
      Navigator.pop(context, true);
    } catch (error) {
      setState(() => _error = errorText(error));
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final others = widget.hosts.where((host) => host.id != widget.host?.id).toList();
    return FormDialog(
      title: widget.host == null ? t('Host hinzufügen', 'Add host') : t('Host bearbeiten', 'Edit host'),
      action: t('Speichern', 'Save'),
      onSubmit: _save,
      children: [
        TextField(
          controller: _label,
          autofocus: widget.host == null,
          decoration: InputDecoration(labelText: t('Bezeichnung', 'Label')),
        ),
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(
              child: TextField(
                controller: _address,
                autocorrect: false,
                decoration: InputDecoration(labelText: t('Adresse', 'Address'), hintText: 'server.example.org'),
              ),
            ),
            const SizedBox(width: 12),
            SizedBox(
              width: 96,
              child: TextField(
                controller: _port,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(labelText: 'Port'),
              ),
            ),
          ],
        ),
        DropdownButtonFormField<String?>(
          initialValue: _identityId,
          isExpanded: true,
          decoration: InputDecoration(
            labelText: t('Identität', 'Identity'),
            helperText: _identities.isEmpty
                ? t(
                    'Identitäten legst du im gleichnamigen Bereich an.',
                    'Create identities in the section of the same name.',
                  )
                : null,
          ),
          items: [
            DropdownMenuItem(value: null, child: Text(t('Keine', 'None'))),
            for (final identity in _identities)
              DropdownMenuItem(
                value: identity.id,
                child: Text(
                  identity.label == identity.username ? identity.username : '${identity.label} (${identity.username})',
                  overflow: TextOverflow.ellipsis,
                ),
              ),
          ],
          onChanged: (value) => setState(() => _identityId = value),
        ),
        DropdownButtonFormField<String?>(
          initialValue: _vpnId,
          isExpanded: true,
          decoration: InputDecoration(
            labelText: t('VPN-Profil', 'VPN profile'),
            helperText: t(
              'Die Verbindung läuft dann durch den WireGuard-Tunnel dieses Profils.',
              'The connection then runs through the WireGuard tunnel of this profile.',
            ),
          ),
          items: [
            DropdownMenuItem(value: null, child: Text(t('Keines (direkt)', 'None (direct)'))),
            for (final profile in _profiles)
              DropdownMenuItem(
                value: profile.id,
                child: Text(profile.label, overflow: TextOverflow.ellipsis),
              ),
          ],
          onChanged: (value) => setState(() => _vpnId = value),
        ),
        if (others.isNotEmpty)
          Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                t('Sprunghosts (in dieser Reihenfolge)', 'Jump hosts (in this order)'),
                style: theme.textTheme.labelLarge,
              ),
              const SizedBox(height: 6),
              Wrap(
                spacing: 8,
                runSpacing: 4,
                children: [
                  for (final host in others)
                    FilterChip(
                      label: Text(
                        _jumps.contains(host.id) ? '${_jumps.indexOf(host.id) + 1}. ${host.label}' : host.label,
                      ),
                      selected: _jumps.contains(host.id),
                      showCheckmark: false,
                      onSelected: (selected) => setState(() => selected ? _jumps.add(host.id) : _jumps.remove(host.id)),
                    ),
                ],
              ),
            ],
          ),
        TextField(
          controller: _tags,
          decoration: InputDecoration(labelText: 'Tags', hintText: t('durch Komma getrennt', 'comma-separated')),
        ),
        TextField(
          controller: _notes,
          minLines: 2,
          maxLines: 5,
          decoration: InputDecoration(labelText: t('Notizen', 'Notes'), alignLabelWithHint: true),
        ),
        SwitchListTile(
          key: const Key('host-keep-session'),
          value: _keepSession,
          onChanged: (value) => setState(() => _keepSession = value),
          contentPadding: EdgeInsets.zero,
          title: Text(t('Sitzung auf dem Host halten (tmux)', 'Keep the session on the host (tmux)')),
          subtitle: Text(
            t(
              'Reißt die Verbindung ab, läuft auf dem Host alles weiter, und die App verbindet sich wieder mit '
                  'derselben Sitzung. Gut für Claude Code und lange Befehle am Telefon. Auf dem Host muss tmux '
                  'installiert sein.',
              'If the connection drops, everything keeps running on the host and the app reattaches to the same '
                  'session. Good for Claude Code and long commands on the phone. The host needs tmux installed.',
            ),
          ),
        ),
        TextField(
          key: const Key('host-startup-command'),
          controller: _startup,
          autocorrect: false,
          enableSuggestions: false,
          decoration: InputDecoration(
            labelText: t('Befehl nach dem Anmelden', 'Command after login'),
            hintText: t(
              'leer für die Shell, z. B. cd ~/projekt && claude',
              'empty for the shell, e.g. cd ~/project && claude',
            ),
            helperText: _keepSession
                ? t('Läuft nur, wenn die tmux-Sitzung neu entsteht.', 'Runs only when the tmux session is created.')
                : null,
          ),
        ),
        SwitchListTile(
          value: _favorite,
          onChanged: (value) => setState(() => _favorite = value),
          contentPadding: EdgeInsets.zero,
          title: Text(t('Favorit', 'Favourite')),
        ),
        if (_error != null) Text(_error!, style: TextStyle(color: theme.colorScheme.error)),
      ],
    );
  }
}
