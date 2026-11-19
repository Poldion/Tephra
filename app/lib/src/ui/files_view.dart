import 'dart:io';

import 'package:material_ui/material_ui.dart';

import '../files.dart';
import '../platform.dart';
import '../rust/api/files.dart';
import '../text.dart';
import 'common.dart';

/// Dateien eines Hosts. Am Desktop steht links dieses Gerät, rechts der Host;
/// am Telefon nur der Host, Dateien wählt und speichert dort das System.
class FilesView extends StatefulWidget {
  const FilesView({super.key, required this.files, required this.hostLabel});

  final FilesController files;
  final String hostLabel;

  @override
  State<FilesView> createState() => _FilesViewState();
}

class _FilesViewState extends State<FilesView> {
  FilesController get files => widget.files;

  /// Am Telefon gibt es keine zweite Spalte mit den eigenen Dateien.
  static final bool _twoPanes = !Platform.isAndroid;

  @override
  void initState() {
    super.initState();
    files.open();
  }

  /// Führt eine Aktion aus und zeigt ihren Fehler, statt ihn zu verschlucken.
  Future<void> _run(Future<void> Function() action) async {
    try {
      await action();
    } catch (error) {
      if (mounted) showMessage(context, errorText(error), error: true);
    }
  }

  Future<String?> _askName({required String title, required String action, String initial = ''}) {
    final controller = TextEditingController(text: initial);
    return showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(title),
        content: SizedBox(
          width: 360,
          child: TextField(
            controller: controller,
            autofocus: true,
            autocorrect: false,
            decoration: InputDecoration(labelText: t('Name', 'Name')),
            onSubmitted: (value) => Navigator.pop(context, value),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Abbrechen', 'Cancel'))),
          FilledButton(onPressed: () => Navigator.pop(context, controller.text), child: Text(action)),
        ],
      ),
    );
  }

  /// Ein Name ohne Pfadanteile; alles andere landete woanders als gedacht.
  bool _validName(String? name) {
    if (name == null) return false;
    final trimmed = name.trim();
    if (trimmed.isEmpty || trimmed == '.' || trimmed == '..' || trimmed.contains('/')) {
      showMessage(context, t('Der Name ist ungültig.', 'The name is not valid.'), error: true);
      return false;
    }
    return true;
  }

  Future<void> _newFolder() async {
    final name = await _askName(title: t('Neuer Ordner', 'New folder'), action: t('Anlegen', 'Create'));
    if (!mounted || !_validName(name)) return;
    await _run(() => files.mkdir(name!.trim()));
  }

  Future<void> _rename(FileEntry entry) async {
    final name = await _askName(
      title: t('Umbenennen', 'Rename'),
      action: t('Umbenennen', 'Rename'),
      initial: entry.name,
    );
    if (!mounted || !_validName(name) || name!.trim() == entry.name) return;
    await _run(() => files.rename(entry, name.trim()));
  }

  Future<void> _delete(FileEntry entry) async {
    final folder = entry.isDir && !entry.isLink;
    final ok = await confirm(
      context,
      title: folder ? t('Ordner löschen?', 'Delete folder?') : t('Datei löschen?', 'Delete file?'),
      body: folder
          ? t(
              '„${entry.name}“ wird mit allem, was darin liegt, endgültig vom Host gelöscht.',
              '"${entry.name}" and everything in it will be deleted from the host permanently.',
            )
          : t(
              '„${entry.name}“ wird endgültig vom Host gelöscht.',
              '"${entry.name}" will be deleted from the host permanently.',
            ),
      action: t('Löschen', 'Delete'),
      destructive: true,
    );
    if (!ok || !mounted) return;
    await _run(() => files.delete(entry));
  }

  Future<bool> _confirmOverwrite(String name) => confirm(
    context,
    title: t('Datei ersetzen?', 'Replace file?'),
    body: t('„$name“ gibt es am Ziel bereits.', '"$name" already exists at the destination.'),
    action: t('Ersetzen', 'Replace'),
  );

  Future<void> _download(FileEntry entry) async {
    if (Platform.isAndroid) {
      // Erst in den Zwischenordner, dann fragt das System, wohin die Datei soll.
      final temp = '${await cacheDir()}/download/${entry.name}';
      final transfer = files.download(entry, temp);
      if (!await transfer.result) return;
      final saved = await saveFile(path: temp, name: entry.name);
      if (mounted && saved) showMessage(context, t('„${entry.name}“ gespeichert.', '"${entry.name}" saved.'));
      return;
    }
    if (files.localPath.isEmpty) {
      showMessage(context, t('Wähle links zuerst einen Zielordner.', 'Pick a destination folder on the left first.'));
      return;
    }
    final target = '${files.localPath}${Platform.pathSeparator}${entry.name}';
    if (File(target).existsSync() && !await _confirmOverwrite(entry.name)) return;
    final transfer = files.download(entry, target);
    if (await transfer.result) await files.refreshLocal();
  }

  Future<void> _upload(String path, String name, {bool temporary = false}) async {
    if (files.remote.any((entry) => entry.name == name)) {
      if (!await _confirmOverwrite(name)) return;
    }
    final transfer = files.upload(path, name: name);
    final done = await transfer.result;
    if (temporary) {
      try {
        File(path).deleteSync();
      } on FileSystemException {
        // Der Zwischenordner wird vom System ohnehin geleert.
      }
    }
    if (done) await files.refreshRemote();
  }

  Future<void> _pickAndUpload() async {
    final picked = await pickFile();
    if (picked == null || !mounted) return;
    await _upload(picked.path, picked.name, temporary: true);
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return ListenableBuilder(
      listenable: files,
      builder: (context, _) => ColoredBox(
        color: scheme.surface,
        child: Column(
          children: [
            Expanded(
              child: LayoutBuilder(
                builder: (context, constraints) {
                  if (!_twoPanes) return _remotePane(context);
                  final panes = [
                    Expanded(child: _localPane(context)),
                    constraints.maxWidth >= 720 ? const VerticalDivider(width: 1) : const Divider(height: 1),
                    Expanded(child: _remotePane(context)),
                  ];
                  return constraints.maxWidth >= 720 ? Row(children: panes) : Column(children: panes);
                },
              ),
            ),
            if (files.transfers.isNotEmpty) _TransferList(files: files),
          ],
        ),
      ),
    );
  }

  Widget _localPane(BuildContext context) {
    final parent = files.localParent;
    return _Pane(
      icon: Icons.computer_rounded,
      title: t('Dieses Gerät', 'This device'),
      path: files.localPath.isEmpty ? t('Laufwerke', 'Drives') : files.localPath,
      onPath: files.goLocal,
      onUp: parent == null ? null : () => files.goLocal(parent),
      onRefresh: files.refreshLocal,
      error: files.localError,
      empty: files.local.isEmpty,
      itemCount: files.local.length,
      itemBuilder: (context, index) {
        final entry = files.local[index];
        return _Row(
          icon: entry.isDir ? Icons.folder_rounded : Icons.insert_drive_file_outlined,
          folder: entry.isDir,
          name: entry.name,
          detail: entry.isDir ? '' : formatSize(entry.size),
          time: entry.modified,
          onTap: entry.isDir ? () => files.goLocal(entry.path) : null,
          actions: [
            if (!entry.isDir)
              (
                Icons.upload_rounded,
                t('Auf den Host laden', 'Upload to the host'),
                () => _upload(entry.path, entry.name),
              ),
          ],
        );
      },
    );
  }

  Widget _remotePane(BuildContext context) {
    return _Pane(
      icon: Icons.dns_rounded,
      title: widget.hostLabel,
      path: files.remotePath,
      onPath: files.goRemote,
      onUp: files.remotePath == '/' || files.remotePath.isEmpty
          ? null
          : () => files.goRemote(remoteParent(files.remotePath)),
      onRefresh: files.refreshRemote,
      loading: files.remoteLoading,
      error: files.remoteError,
      empty: files.remote.isEmpty,
      extra: [
        IconButton(
          tooltip: t('Neuer Ordner', 'New folder'),
          icon: const Icon(Icons.create_new_folder_outlined),
          onPressed: _newFolder,
        ),
        if (!_twoPanes)
          IconButton(
            tooltip: t('Datei hochladen', 'Upload a file'),
            icon: const Icon(Icons.upload_rounded),
            onPressed: _pickAndUpload,
          ),
      ],
      itemCount: files.remote.length,
      itemBuilder: (context, index) {
        final entry = files.remote[index];
        return _Row(
          icon: entry.isLink
              ? Icons.link_rounded
              : entry.isDir
              ? Icons.folder_rounded
              : Icons.insert_drive_file_outlined,
          folder: entry.isDir,
          name: entry.name,
          detail: entry.isDir ? '' : formatSize(entry.size.toInt()),
          time: entry.modified == null ? null : DateTime.fromMillisecondsSinceEpoch(entry.modified!.toInt() * 1000),
          onTap: entry.isDir ? () => files.goRemote(remoteJoin(files.remotePath, entry.name)) : null,
          actions: [
            if (!entry.isDir)
              (Icons.download_rounded, t('Herunterladen', 'Download'), () => _run(() => _download(entry))),
            (Icons.drive_file_rename_outline_rounded, t('Umbenennen', 'Rename'), () => _rename(entry)),
            (Icons.delete_outline_rounded, t('Löschen', 'Delete'), () => _delete(entry)),
          ],
        );
      },
    );
  }
}

/// Eine Spalte des Dateibrowsers: Kopf mit Pfad, darunter die Einträge.
class _Pane extends StatefulWidget {
  const _Pane({
    required this.icon,
    required this.title,
    required this.path,
    required this.onPath,
    required this.onUp,
    required this.onRefresh,
    required this.itemCount,
    required this.itemBuilder,
    required this.empty,
    this.error,
    this.loading = false,
    this.extra = const [],
  });

  final IconData icon;
  final String title;
  final String path;
  final ValueChanged<String> onPath;
  final VoidCallback? onUp;
  final VoidCallback onRefresh;
  final int itemCount;
  final IndexedWidgetBuilder itemBuilder;
  final bool empty;
  final String? error;
  final bool loading;
  final List<Widget> extra;

  @override
  State<_Pane> createState() => _PaneState();
}

class _PaneState extends State<_Pane> {
  late final _path = TextEditingController(text: widget.path);

  @override
  void didUpdateWidget(_Pane old) {
    super.didUpdateWidget(old);
    if (old.path != widget.path) _path.text = widget.path;
  }

  @override
  void dispose() {
    _path.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 8, 4, 0),
          child: Row(
            children: [
              Icon(widget.icon, size: 18, color: scheme.primary),
              const SizedBox(width: 8),
              Expanded(
                child: Text(widget.title, style: theme.textTheme.titleSmall, overflow: TextOverflow.ellipsis),
              ),
              ...widget.extra,
              IconButton(
                tooltip: t('Aktualisieren', 'Refresh'),
                icon: const Icon(Icons.refresh_rounded),
                onPressed: widget.onRefresh,
              ),
            ],
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(4, 0, 12, 8),
          child: Row(
            children: [
              IconButton(
                tooltip: t('Eine Ebene höher', 'Up one level'),
                icon: const Icon(Icons.arrow_upward_rounded),
                onPressed: widget.onUp,
              ),
              Expanded(
                child: TextField(
                  controller: _path,
                  autocorrect: false,
                  style: monoStyle(context, size: 12.5),
                  decoration: const InputDecoration(contentPadding: EdgeInsets.symmetric(horizontal: 10, vertical: 10)),
                  onSubmitted: widget.onPath,
                ),
              ),
            ],
          ),
        ),
        if (widget.loading) const LinearProgressIndicator(minHeight: 2) else const SizedBox(height: 2),
        Expanded(
          child: widget.error != null
              ? Padding(
                  padding: const EdgeInsets.all(16),
                  child: Text(widget.error!, style: TextStyle(color: scheme.error)),
                )
              : widget.empty
              ? Center(
                  child: Text(
                    widget.loading ? '' : t('Dieser Ordner ist leer.', 'This folder is empty.'),
                    style: theme.textTheme.bodyMedium?.copyWith(color: scheme.onSurfaceVariant),
                  ),
                )
              : ListView.builder(itemCount: widget.itemCount, itemExtent: 44, itemBuilder: widget.itemBuilder),
        ),
      ],
    );
  }
}

class _Row extends StatelessWidget {
  const _Row({
    required this.icon,
    required this.folder,
    required this.name,
    required this.detail,
    required this.time,
    required this.onTap,
    required this.actions,
  });

  final IconData icon;
  final bool folder;
  final String name;
  final String detail;
  final DateTime? time;
  final VoidCallback? onTap;
  final List<(IconData, String, VoidCallback)> actions;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final muted = theme.textTheme.bodySmall?.copyWith(color: scheme.onSurfaceVariant);
    return InkWell(
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.only(left: 14),
        child: LayoutBuilder(
          builder: (context, constraints) => Row(
            children: [
              Icon(icon, size: 20, color: folder ? scheme.primary : scheme.onSurfaceVariant),
              const SizedBox(width: 10),
              Expanded(child: Text(name, overflow: TextOverflow.ellipsis)),
              if (detail.isNotEmpty) Text(detail, style: muted),
              if (time != null && constraints.maxWidth >= 420) ...[
                const SizedBox(width: 16),
                Text(formatTime(time!), style: muted),
              ],
              if (actions.isEmpty)
                const SizedBox(width: 48)
              else
                PopupMenuButton<int>(
                  tooltip: t('Aktionen', 'Actions'),
                  icon: const Icon(Icons.more_vert_rounded, size: 20),
                  onSelected: (index) => actions[index].$3(),
                  itemBuilder: (_) => [
                    for (final (index, action) in actions.indexed)
                      PopupMenuItem(
                        value: index,
                        child: Row(children: [Icon(action.$1, size: 18), const SizedBox(width: 12), Text(action.$2)]),
                      ),
                  ],
                ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Laufende und beendete Übertragungen am unteren Rand.
class _TransferList extends StatelessWidget {
  const _TransferList({required this.files});

  final FilesController files;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    return Container(
      color: scheme.surfaceContainer,
      constraints: const BoxConstraints(maxHeight: 148),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 4, 4, 0),
            child: Row(
              children: [
                Expanded(child: Text(t('Übertragungen', 'Transfers'), style: theme.textTheme.labelLarge)),
                TextButton(onPressed: files.clearFinished, child: Text(t('Erledigte ausblenden', 'Hide finished'))),
              ],
            ),
          ),
          Flexible(
            child: ListView(
              shrinkWrap: true,
              padding: const EdgeInsets.only(bottom: 6),
              children: [for (final transfer in files.transfers) _row(context, transfer)],
            ),
          ),
        ],
      ),
    );
  }

  Widget _row(BuildContext context, Transfer transfer) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final status = transfer.error != null
        ? transfer.error!
        : transfer.cancelled
        ? t('abgebrochen', 'cancelled')
        : transfer.finished
        ? formatSize(transfer.done)
        : transfer.total == null
        ? formatSize(transfer.done)
        : '${formatSize(transfer.done)} / ${formatSize(transfer.total!)}';
    return Padding(
      padding: const EdgeInsets.fromLTRB(14, 2, 4, 2),
      child: Row(
        children: [
          Icon(
            transfer.upload ? Icons.upload_rounded : Icons.download_rounded,
            size: 18,
            color: transfer.error != null ? scheme.error : scheme.onSurfaceVariant,
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Expanded(child: Text(transfer.name, overflow: TextOverflow.ellipsis)),
                    const SizedBox(width: 12),
                    Flexible(
                      child: Text(
                        status,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: transfer.error != null ? scheme.error : scheme.onSurfaceVariant,
                        ),
                      ),
                    ),
                  ],
                ),
                if (transfer.running) ...[
                  const SizedBox(height: 4),
                  LinearProgressIndicator(value: transfer.progress, minHeight: 3),
                ],
              ],
            ),
          ),
          if (transfer.running)
            IconButton(
              tooltip: t('Abbrechen', 'Cancel'),
              icon: const Icon(Icons.close_rounded, size: 18),
              onPressed: () => files.cancel(transfer),
            )
          else
            SizedBox(
              width: 48,
              child: Icon(
                transfer.finished ? Icons.check_rounded : Icons.error_outline_rounded,
                size: 18,
                color: transfer.finished ? Colors.green : scheme.error,
              ),
            ),
        ],
      ),
    );
  }
}
