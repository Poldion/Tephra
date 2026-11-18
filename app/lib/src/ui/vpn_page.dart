import 'package:material_ui/material_ui.dart';

import '../rust/api/app.dart';
import '../text.dart';
import 'common.dart';

/// WireGuard-Profile: importieren, prüfen, löschen.
class VpnPage extends StatefulWidget {
  const VpnPage({super.key});

  @override
  State<VpnPage> createState() => _VpnPageState();
}

class _VpnPageState extends State<VpnPage> {
  List<VpnView> _profiles = [];
  String? _error;

  /// Profile, deren Tunnel gerade geprüft wird.
  final Set<String> _testing = {};

  @override
  void initState() {
    super.initState();
    _reload();
  }

  void _reload() {
    try {
      _profiles = listVpnProfiles();
      _error = null;
    } catch (error) {
      _error = errorText(error);
    }
  }

  Future<void> _import() async {
    final saved = await showDialog<bool>(context: context, builder: (_) => const VpnImportDialog());
    if (saved == true && mounted) setState(_reload);
  }

  Future<void> _test(VpnView profile) async {
    setState(() => _testing.add(profile.id));
    try {
      final result = await testVpnProfile(id: profile.id);
      if (mounted) showMessage(context, '${profile.label}: ${clean(result)}');
    } catch (error) {
      if (mounted) showMessage(context, '${profile.label}: ${errorText(error)}', error: true);
    } finally {
      if (mounted) setState(() => _testing.remove(profile.id));
    }
  }

  Future<void> _delete(VpnView profile) async {
    final ok = await confirm(
      context,
      title: t('VPN-Profil löschen?', 'Delete VPN profile?'),
      body: t(
        '„${profile.label}“ wird endgültig gelöscht. Hosts, die es verwenden, verbinden sich danach direkt.',
        '"${profile.label}" will be deleted permanently. Hosts using it will connect directly afterwards.',
      ),
      action: t('Löschen', 'Delete'),
      destructive: true,
    );
    if (!ok || !mounted) return;
    attempt(context, () => deleteItem(id: profile.id));
    setState(_reload);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SectionPage(
      title: t('VPN-Profile', 'VPN profiles'),
      actions: [
        FilledButton.icon(
          icon: const Icon(Icons.add_rounded),
          label: Text(t('Profil importieren', 'Import profile')),
          onPressed: _import,
        ),
      ],
      child: _error != null
          ? EmptyState(icon: Icons.error_outline_rounded, title: t('Fehler', 'Error'), hint: _error!)
          : _profiles.isEmpty
          ? EmptyState(
              icon: Icons.vpn_lock_outlined,
              title: t('Noch keine VPN-Profile', 'No VPN profiles yet'),
              hint: t(
                'Importiere eine WireGuard-Konfiguration. Der Tunnel läuft nur innerhalb dieser App – ohne Administratorrechte und ohne das System-VPN zu berühren.',
                'Import a WireGuard configuration. The tunnel runs inside this app only – no administrator rights, and the system VPN stays untouched.',
              ),
            )
          : ListView.separated(
              padding: const EdgeInsets.only(bottom: 20),
              itemCount: _profiles.length,
              separatorBuilder: (_, _) => const SizedBox(height: 8),
              itemBuilder: (context, index) {
                final profile = _profiles[index];
                final testing = _testing.contains(profile.id);
                final details = [
                  if (profile.addresses.isNotEmpty) profile.addresses.join(', '),
                  if (profile.endpoints.isNotEmpty) '→ ${profile.endpoints.join(', ')}',
                ].join('  ');
                return Card(
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(16, 10, 4, 10),
                    child: Row(
                      children: [
                        Icon(Icons.vpn_lock_rounded, color: theme.colorScheme.primary),
                        const SizedBox(width: 16),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(profile.label, style: theme.textTheme.titleMedium, overflow: TextOverflow.ellipsis),
                              Text(
                                details,
                                style: monoStyle(context, size: 12).copyWith(color: theme.colorScheme.onSurfaceVariant),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ],
                          ),
                        ),
                        TextButton.icon(
                          icon: testing
                              ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
                              : const Icon(Icons.network_check_rounded, size: 18),
                          label: Text(t('Prüfen', 'Test')),
                          onPressed: testing ? null : () => _test(profile),
                        ),
                        IconButton(
                          tooltip: t('Löschen', 'Delete'),
                          icon: const Icon(Icons.delete_outline_rounded),
                          onPressed: () => _delete(profile),
                        ),
                      ],
                    ),
                  ),
                );
              },
            ),
    );
  }
}

class VpnImportDialog extends StatefulWidget {
  const VpnImportDialog({super.key});

  @override
  State<VpnImportDialog> createState() => _VpnImportDialogState();
}

class _VpnImportDialogState extends State<VpnImportDialog> {
  final _label = TextEditingController();
  final _conf = TextEditingController();
  String? _error;

  @override
  void dispose() {
    _label.dispose();
    _conf.dispose();
    super.dispose();
  }

  void _save() {
    try {
      importVpnProfile(label: _label.text, conf: _conf.text);
      _conf.clear();
      Navigator.pop(context, true);
    } catch (error) {
      setState(() => _error = errorText(error));
    }
  }

  @override
  Widget build(BuildContext context) {
    return FormDialog(
      title: t('WireGuard-Profil importieren', 'Import WireGuard profile'),
      action: t('Importieren', 'Import'),
      onSubmit: _save,
      children: [
        TextField(
          controller: _label,
          autofocus: true,
          decoration: InputDecoration(labelText: t('Bezeichnung', 'Label')),
        ),
        TextField(
          controller: _conf,
          minLines: 8,
          maxLines: 12,
          autocorrect: false,
          enableSuggestions: false,
          style: monoStyle(context, size: 12),
          decoration: InputDecoration(
            labelText: t('Konfiguration', 'Configuration'),
            hintText: '[Interface]\nPrivateKey = …\nAddress = 10.0.0.2/32\n\n[Peer]\nPublicKey = …\nEndpoint = vpn.example.org:51820\nAllowedIPs = 10.0.0.0/24',
            alignLabelWithHint: true,
          ),
        ),
        if (_error != null) Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
      ],
    );
  }
}
