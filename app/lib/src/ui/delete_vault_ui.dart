import 'package:material_ui/material_ui.dart';

import '../rust/api/app.dart';
import '../sync.dart';
import '../text.dart';
import 'common.dart';

/// Knopf im Dialog „Über Tephra“: löscht den Tresor von diesem Gerät, nach Warnung und mit dem Master-Passwort.
/// Danach beginnt die App wie beim ersten Start: neuen Tresor anlegen oder einen vorhandenen abholen.
class DeleteVaultSection extends StatelessWidget {
  const DeleteVaultSection({super.key, required this.onDelete});

  /// Löscht den Tresor; das Passwort ist zu diesem Zeitpunkt schon geprüft.
  final Future<void> Function(String password) onDelete;

  Future<void> _start(BuildContext context) async {
    final synced = syncState.connection != null;
    final password = await showDialog<String>(
      context: context,
      builder: (_) => _DeleteDialog(synced: synced),
    );
    if (password == null || !context.mounted) return;
    Navigator.of(context).popUntil((route) => route.isFirst);
    await onDelete(password);
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(top: 12),
      child: Align(
        alignment: Alignment.centerLeft,
        child: OutlinedButton.icon(
          key: const Key('delete-vault'),
          style: OutlinedButton.styleFrom(foregroundColor: scheme.error),
          icon: const Icon(Icons.delete_forever_outlined),
          label: Text(t('Tresor von diesem Gerät löschen', 'Delete the vault from this device')),
          onPressed: () => _start(context),
        ),
      ),
    );
  }
}

class _DeleteDialog extends StatefulWidget {
  const _DeleteDialog({required this.synced});

  final bool synced;

  @override
  State<_DeleteDialog> createState() => _DeleteDialogState();
}

class _DeleteDialogState extends State<_DeleteDialog> {
  final _password = TextEditingController();
  String? _error;

  @override
  void dispose() {
    _password.clear();
    _password.dispose();
    super.dispose();
  }

  void _submit() {
    bool ok;
    try {
      ok = verifyMasterPassword(password: _password.text);
    } catch (error) {
      setState(() => _error = errorText(error));
      return;
    }
    if (!ok) {
      setState(() => _error = t('Das Master-Passwort stimmt nicht.', 'The master password is wrong.'));
      return;
    }
    Navigator.pop(context, _password.text);
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return AlertDialog(
      title: Text(t('Tresor von diesem Gerät löschen?', 'Delete the vault from this device?')),
      scrollable: true,
      content: SizedBox(
        width: 440,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              widget.synced
                  ? t(
                      'Auf dem Sync-Server bleibt alles, und du kannst den Tresor danach wieder abholen („Vorhandenen Tresor abholen“). Verloren geht, was nur auf diesem Gerät lag: Schlüssel, die du als „nur auf diesem Gerät“ angelegt hast, Änderungen, die noch nicht abgeglichen sind, und die Einstellungen dieses Geräts (etwa der Fingerabdruck).',
                      'Everything on the sync server stays, and you can fetch the vault again afterwards ("Fetch an existing vault"). Lost is what was only on this device: keys you created as "only on this device", changes not synced yet, and the settings of this device (such as the fingerprint).',
                    )
                  : t(
                      'Dieses Gerät ist mit keinem Sync-Server verbunden. Der Tresor ist danach unwiederbringlich weg – es sei denn, du hast eine Sicherung (tephra export).',
                      'This device is not connected to a sync server. The vault is then gone for good – unless you have a backup (tephra export).',
                    ),
              style: TextStyle(color: widget.synced ? null : scheme.error),
            ),
            const SizedBox(height: 16),
            SecretField(
              key: const Key('delete-vault-password'),
              controller: _password,
              label: t('Master-Passwort', 'Master password'),
              autofocus: true,
              onSubmitted: (_) => _submit(),
            ),
            if (_error != null) ...[const SizedBox(height: 8), Text(_error!, style: TextStyle(color: scheme.error))],
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Abbrechen', 'Cancel'))),
        FilledButton(
          key: const Key('delete-vault-confirm'),
          style: FilledButton.styleFrom(backgroundColor: scheme.error, foregroundColor: scheme.onError),
          onPressed: _submit,
          child: Text(t('Endgültig löschen', 'Delete for good')),
        ),
      ],
    );
  }
}
