import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../rust/api/app.dart';
import '../text.dart';
import 'common.dart';

/// SSH-Schlüssel: erzeugen, importieren, öffentlichen Teil kopieren.
class KeysPage extends StatefulWidget {
  const KeysPage({super.key});

  @override
  State<KeysPage> createState() => _KeysPageState();
}

class _KeysPageState extends State<KeysPage> {
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
                        IconButton(
                          tooltip: t('Löschen', 'Delete'),
                          icon: const Icon(Icons.delete_outline_rounded),
                          onPressed: () => _delete(key),
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
