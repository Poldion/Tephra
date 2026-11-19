import 'package:material_ui/material_ui.dart';

import '../files.dart' show formatSize;
import '../platform.dart';
import '../settings.dart';
import '../text.dart';
import '../update.dart';
import 'common.dart';

/// Zeigt eine verfügbare Aktualisierung und führt sie auf Wunsch aus.
Future<void> showUpdateDialog(BuildContext context, Update update) => showDialog<void>(
  context: context,
  barrierDismissible: false,
  builder: (_) => _UpdateDialog(update: update),
);

class _UpdateDialog extends StatefulWidget {
  const _UpdateDialog({required this.update});

  final Update update;

  @override
  State<_UpdateDialog> createState() => _UpdateDialogState();
}

class _UpdateDialogState extends State<_UpdateDialog> {
  bool _busy = false;
  int _done = 0;
  int? _total;
  String? _error;

  Future<void> _install() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await installUpdate(
        widget.update,
        onProgress: (done, total) {
          if (mounted) {
            setState(() {
              _done = done;
              _total = total;
            });
          }
        },
      );
      // Bis hierher kommt nur, wer nicht neu gestartet wird: Android und der Verweis auf die Seite.
      if (mounted) Navigator.pop(context);
    } catch (error) {
      if (mounted) {
        setState(() {
          _busy = false;
          _error = errorText(error);
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final update = widget.update;
    final how = switch (update.kind) {
      UpdateKind.installer => t(
        'Tephra lädt den Installer, schließt sich und startet in der neuen Fassung wieder. Offene Sitzungen enden dabei.',
        'Tephra downloads the installer, closes and starts again in the new version. Open sessions end.',
      ),
      UpdateKind.appImage => t(
        'Tephra ersetzt seine AppImage-Datei und startet neu. Offene Sitzungen enden dabei.',
        'Tephra replaces its AppImage file and restarts. Open sessions end.',
      ),
      UpdateKind.apk => t(
        'Tephra lädt die neue Fassung und übergibt sie an Android. Beim ersten Mal fragt Android, ob Tephra Apps installieren darf.',
        'Tephra downloads the new version and hands it to Android. The first time, Android asks whether Tephra may install apps.',
      ),
      UpdateKind.manual => t(
        'Diese Fassung von Tephra kann sich nicht selbst aktualisieren. Die neue Fassung liegt auf der Seite der Veröffentlichung.',
        'This build of Tephra cannot update itself. The new version is on the release page.',
      ),
    };
    return AlertDialog(
      title: Text(t('Tephra ${update.version} ist verfügbar', 'Tephra ${update.version} is available')),
      content: SizedBox(
        width: 420,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(t('Installiert ist $appVersion.', 'Installed is $appVersion.')),
            const SizedBox(height: 8),
            Text(how, style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant)),
            if (_busy) ...[
              const SizedBox(height: 16),
              LinearProgressIndicator(value: _total == null || _total == 0 ? null : _done / _total!),
              const SizedBox(height: 6),
              Text(
                _total == null ? formatSize(_done) : '${formatSize(_done)} / ${formatSize(_total!)}',
                style: theme.textTheme.bodySmall,
              ),
            ],
            if (_error != null) ...[
              const SizedBox(height: 12),
              Text(_error!, style: TextStyle(color: theme.colorScheme.error)),
            ],
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: _busy ? null : () => Navigator.pop(context), child: Text(t('Später', 'Later'))),
        if (update.kind != UpdateKind.manual)
          TextButton(
            onPressed: _busy ? null : () => openUrl(update.page),
            child: Text(t('Änderungen ansehen', 'View changes')),
          ),
        FilledButton(
          onPressed: _busy ? null : _install,
          child: Text(
            update.kind == UpdateKind.manual ? t('Zur Seite', 'Open page') : t('Jetzt aktualisieren', 'Update now'),
          ),
        ),
      ],
    );
  }
}

/// Abschnitt im Über-Dialog: Stand, Suche von Hand und der Schalter für die automatische Suche.
class UpdateSection extends StatefulWidget {
  const UpdateSection({super.key});

  @override
  State<UpdateSection> createState() => _UpdateSectionState();
}

class _UpdateSectionState extends State<UpdateSection> {
  bool _checking = false;
  String? _message;

  Future<void> _check() async {
    setState(() {
      _checking = true;
      _message = null;
    });
    try {
      final update = await checkForUpdate();
      availableUpdate.value = update;
      if (mounted && update == null) setState(() => _message = t('Tephra ist aktuell.', 'Tephra is up to date.'));
    } catch (error) {
      if (mounted) setState(() => _message = errorText(error));
    } finally {
      if (mounted) setState(() => _checking = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return ListenableBuilder(
      listenable: Listenable.merge([availableUpdate, settings]),
      builder: (context, _) {
        final update = availableUpdate.value;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Divider(height: 28),
            if (update != null)
              FilledButton.tonalIcon(
                icon: const Icon(Icons.system_update_alt_rounded),
                label: Text(t('Version ${update.version} ist verfügbar', 'Version ${update.version} is available')),
                onPressed: () => showUpdateDialog(context, update),
              )
            else
              OutlinedButton(
                onPressed: _checking ? null : _check,
                child: Text(
                  _checking ? t('Suche läuft …', 'Checking …') : t('Nach Aktualisierungen suchen', 'Check for updates'),
                ),
              ),
            if (_message != null) ...[
              const SizedBox(height: 8),
              Text(_message!, style: theme.textTheme.bodySmall, textAlign: TextAlign.center),
            ],
            SwitchListTile(
              value: settings.checkUpdates,
              onChanged: settings.setCheckUpdates,
              contentPadding: EdgeInsets.zero,
              title: Text(t('Automatisch suchen', 'Check automatically')),
              subtitle: Text(
                t(
                  'Einmal am Tag fragt Tephra bei GitHub nach, ob es eine neuere Fassung gibt.',
                  'Once a day Tephra asks GitHub whether there is a newer version.',
                ),
              ),
            ),
          ],
        );
      },
    );
  }
}
