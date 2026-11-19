import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../rust/api/app.dart';
import '../text.dart';
import 'common.dart';
import 'sync_ui.dart';

/// SSH-Schlüssel: erzeugen, importieren, öffentlichen Teil kopieren.
class KeysPage extends StatefulWidget {
  const KeysPage({super.key});

  @override
  State<KeysPage> createState() => _KeysPageState();
}

class _KeysPageState extends State<KeysPage> with ReloadOnSync<KeysPage> {
  @override
  void reloadFromVault() => _reload();

  List<KeyView> _keys = [];
  String? _error;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  void _reload() {
    try {
      _keys = listKeys();
      _error = null;
    } catch (error) {
      _error = errorText(error);
    }
  }

  Future<void> _add({required bool import}) async {
    final saved = await showDialog<bool>(
      context: context,
      builder: (_) => KeyDialog(import: import),
    );
    if (saved == true && mounted) setState(_reload);
  }

  Future<void> _copy(KeyView key) async {
    await Clipboard.setData(ClipboardData(text: key.publicKey));
    if (mounted) showMessage(context, t('Öffentlicher Schlüssel kopiert.', 'Public key copied.'));
  }

  /// Trägt den öffentlichen Teil des Schlüssels auf einem Host ein.
  Future<void> _install(KeyView key) async {
    final List<HostView> hosts;
    try {
      hosts = listHosts();
    } catch (error) {
      showMessage(context, errorText(error), error: true);
      return;
    }
    if (hosts.isEmpty) {
      showMessage(context, t('Lege zuerst einen Host an.', 'Add a host first.'));
      return;
    }
    final host = await showDialog<HostView>(
      context: context,
      builder: (_) => _InstallDialog(keyLabel: key.label, hosts: hosts),
    );
    if (host == null || !mounted) return;
    try {
      final added = await installKeyOnHost(keyId: key.id, hostId: host.id);
      if (!mounted) return;
      showMessage(
        context,
        added
            ? t(
                '„${key.label}“ ist auf „${host.label}“ eingetragen.',
                '"${key.label}" is now installed on "${host.label}".',
              )
            : t(
                '„${key.label}“ war auf „${host.label}“ schon eingetragen.',
                '"${key.label}" was already installed on "${host.label}".',
              ),
      );
    } catch (error) {
      if (mounted) showMessage(context, errorText(error), error: true);
    }
  }

  Future<void> _delete(KeyView key) async {
    final ok = await confirm(
      context,
      title: t('Schlüssel löschen?', 'Delete key?'),
      body: t(
        '„${key.label}“ wird endgültig gelöscht. Der private Schlüssel lässt sich nicht wiederherstellen.',
        '"${key.label}" will be deleted permanently. The private key cannot be restored.',
      ),
      action: t('Löschen', 'Delete'),
      destructive: true,
    );
    if (!ok || !mounted) return;
    attempt(context, () => deleteItem(id: key.id));
    setState(_reload);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SectionPage(
      title: t('Schlüssel', 'Keys'),
      actions: [
        OutlinedButton.icon(
          icon: const Icon(Icons.file_download_outlined),
          label: Text(t('Importieren', 'Import')),
          onPressed: () => _add(import: true),
        ),
        FilledButton.icon(
          icon: const Icon(Icons.add_rounded),
          label: Text(t('Schlüssel erzeugen', 'Generate key')),
          onPressed: () => _add(import: false),
        ),
      ],
      child: _error != null
          ? EmptyState(icon: Icons.error_outline_rounded, title: t('Fehler', 'Error'), hint: _error!)
          : _keys.isEmpty
          ? EmptyState(
              icon: Icons.key_outlined,
              title: t('Noch keine Schlüssel', 'No keys yet'),
              hint: t(
                'Erzeuge ein neues Schlüsselpaar oder importiere einen vorhandenen privaten Schlüssel. Der private Teil verlässt den Tresor nicht.',
                'Generate a new key pair or import an existing private key. The private part never leaves the vault.',
              ),
            )
          : ListView.separated(
              padding: const EdgeInsets.only(bottom: 20),
              itemCount: _keys.length,
              separatorBuilder: (_, _) => const SizedBox(height: 8),
              itemBuilder: (context, index) {
                final key = _keys[index];
                return Card(
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(16, 10, 4, 10),
                    child: Row(
                      children: [
                        Icon(Icons.key_rounded, color: theme.colorScheme.primary),
                        const SizedBox(width: 16),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Row(
                                children: [
                                  Flexible(
                                    child: Text(
                                      key.label,
                                      style: theme.textTheme.titleMedium,
                                      overflow: TextOverflow.ellipsis,
                                    ),
                                  ),
                                  if (key.localOnly) ...[
                                    const SizedBox(width: 8),
                                    Tooltip(
                                      message: t(
                                        'Bleibt auf diesem Gerät und wird nicht synchronisiert.',
                                        'Stays on this device and is not synchronised.',
                                      ),
                                      child: Icon(
                                        Icons.phonelink_lock_rounded,
                                        size: 16,
                                        color: theme.colorScheme.outline,
                                      ),
                                    ),
                                  ],
                                ],
                              ),
                              Text(
                                key.publicKey,
                                style: monoStyle(context, size: 12).copyWith(color: theme.colorScheme.onSurfaceVariant),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ],
                          ),
                        ),
                        IconButton(
                          tooltip: t('Öffentlichen Schlüssel kopieren', 'Copy public key'),
                          icon: const Icon(Icons.copy_rounded),
                          onPressed: () => _copy(key),
                        ),
                        PopupMenuButton<String>(
                          tooltip: t('Mehr', 'More'),
                          onSelected: (action) => action == 'install' ? _install(key) : _delete(key),
                          itemBuilder: (_) => [
                            PopupMenuItem(
                              value: 'install',
                              child: Text(t('Auf einem Host einrichten', 'Install on a host')),
                            ),
                            PopupMenuItem(value: 'delete', child: Text(t('Löschen', 'Delete'))),
                          ],
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

/// Fragt, auf welchem Host ein Schlüssel eingerichtet werden soll.
class _InstallDialog extends StatefulWidget {
  const _InstallDialog({required this.keyLabel, required this.hosts});

  final String keyLabel;
  final List<HostView> hosts;

  @override
  State<_InstallDialog> createState() => _InstallDialogState();
}

class _InstallDialogState extends State<_InstallDialog> {
  late HostView _host = widget.hosts.first;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return AlertDialog(
      title: Text(t('„${widget.keyLabel}“ auf einem Host einrichten', 'Install "${widget.keyLabel}" on a host')),
      content: SizedBox(
        width: 440,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            DropdownButtonFormField<HostView>(
              initialValue: _host,
              isExpanded: true,
              decoration: const InputDecoration(labelText: 'Host'),
              items: [
                for (final host in widget.hosts)
                  DropdownMenuItem(
                    value: host,
                    child: Text(host.label, overflow: TextOverflow.ellipsis),
                  ),
              ],
              onChanged: (host) => setState(() => _host = host ?? _host),
            ),
            const SizedBox(height: 14),
            Text(
              t(
                'Tephra meldet sich mit der bisherigen Anmeldung des Hosts an und trägt den öffentlichen Schlüssel dort in ~/.ssh/authorized_keys ein. Braucht die Anmeldung eine Eingabe von dir, öffne vorher eine Sitzung zu dem Host. Danach kannst du dem Host eine Identität mit diesem Schlüssel zuweisen.',
                'Tephra signs in with the current credentials of the host and adds the public key to ~/.ssh/authorized_keys there. If signing in needs input from you, open a session to the host first. Afterwards you can give the host an identity that uses this key.',
              ),
              style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Abbrechen', 'Cancel'))),
        FilledButton(onPressed: () => Navigator.pop(context, _host), child: Text(t('Einrichten', 'Install'))),
      ],
    );
  }
}

/// Dialog zum Erzeugen oder Importieren eines Schlüssels.
class KeyDialog extends StatefulWidget {
  const KeyDialog({super.key, required this.import});

  final bool import;

  @override
  State<KeyDialog> createState() => _KeyDialogState();
}

class _KeyDialogState extends State<KeyDialog> {
  final _label = TextEditingController();
  final _pem = TextEditingController();
  final _passphrase = TextEditingController();
  bool _localOnly = false;
  String? _error;

  @override
  void dispose() {
    _label.dispose();
    _pem.dispose();
    _passphrase.dispose();
    super.dispose();
  }

  void _save() {
    try {
      if (widget.import) {
        importKey(label: _label.text, pem: _pem.text, passphrase: _passphrase.text, localOnly: _localOnly);
      } else {
        generateKey(label: _label.text, localOnly: _localOnly);
      }
      _pem.clear();
      _passphrase.clear();
      Navigator.pop(context, true);
    } catch (error) {
      setState(() => _error = errorText(error));
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return FormDialog(
      title: widget.import ? t('Schlüssel importieren', 'Import key') : t('Schlüssel erzeugen', 'Generate key'),
      action: widget.import ? t('Importieren', 'Import') : t('Erzeugen', 'Generate'),
      onSubmit: _save,
      children: [
        TextField(
          controller: _label,
          autofocus: true,
          decoration: InputDecoration(labelText: t('Bezeichnung', 'Label')),
        ),
        if (widget.import) ...[
          TextField(
            controller: _pem,
            minLines: 5,
            maxLines: 8,
            autocorrect: false,
            enableSuggestions: false,
            style: monoStyle(context, size: 12),
            decoration: InputDecoration(
              labelText: t('Privater Schlüssel', 'Private key'),
              hintText: '-----BEGIN OPENSSH PRIVATE KEY-----',
              alignLabelWithHint: true,
            ),
          ),
          SecretField(controller: _passphrase, label: t('Passphrase (falls vorhanden)', 'Passphrase (if any)')),
        ] else
          Text(
            t('Es wird ein Ed25519-Schlüsselpaar erzeugt.', 'An Ed25519 key pair will be generated.'),
            style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant),
          ),
        SwitchListTile(
          value: _localOnly,
          onChanged: (value) => setState(() => _localOnly = value),
          contentPadding: EdgeInsets.zero,
          title: Text(t('Nur auf diesem Gerät', 'This device only')),
          subtitle: Text(
            t(
              'Der Schlüssel wird nicht mit anderen Geräten synchronisiert.',
              'The key is not synchronised to other devices.',
            ),
          ),
        ),
        if (_error != null) Text(_error!, style: TextStyle(color: theme.colorScheme.error)),
      ],
    );
  }
}
