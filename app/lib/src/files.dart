import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';

import 'rust/api/files.dart';
import 'text.dart';
import 'ui/common.dart' show errorText;

/// Eine Datei oder ein Ordner auf diesem Gerät.
class LocalEntry {
  const LocalEntry({required this.name, required this.path, required this.isDir, this.size = 0, this.modified});

  final String name;
  final String path;
  final bool isDir;
  final int size;
  final DateTime? modified;
}

/// Eine laufende oder beendete Übertragung.
class Transfer {
  Transfer({required this.name, required this.upload, required this.remotePath, required this.localPath});

  final String name;
  final bool upload;
  final String remotePath;
  final String localPath;

  int? id;
  int done = 0;
  int? total;
  bool finished = false;
  bool cancelled = false;
  String? error;

  final Completer<bool> _completer = Completer<bool>();

  /// Wird mit `true` fertig, wenn die Datei vollständig übertragen ist.
  Future<bool> get result => _completer.future;

  bool get running => !finished && !cancelled && error == null;

  double? get progress => total == null || total == 0 ? null : (done / total!).clamp(0.0, 1.0);
}

/// Verbindet zwei Teile eines Pfads auf dem Host.
String remoteJoin(String dir, String name) => dir.endsWith('/') ? '$dir$name' : '$dir/$name';

/// Übergeordnetes Verzeichnis auf dem Host; die Wurzel bleibt die Wurzel.
String remoteParent(String path) {
  final trimmed = path.length > 1 && path.endsWith('/') ? path.substring(0, path.length - 1) : path;
  final cut = trimmed.lastIndexOf('/');
  return cut <= 0 ? '/' : trimmed.substring(0, cut);
}

/// Dateien eines Hosts und – am Desktop – dieses Geräts, samt Übertragungen.
/// Lebt so lange wie die Sitzung, zu der es gehört.
class FilesController extends ChangeNotifier {
  FilesController(this.hostId);

  final String hostId;

  String remotePath = '';
  List<FileEntry> remote = const [];
  bool remoteLoading = false;
  String? remoteError;

  /// Leer steht unter Windows für die Übersicht der Laufwerke.
  String localPath = '';
  List<LocalEntry> local = const [];
  String? localError;

  final List<Transfer> transfers = [];

  bool _opened = false;
  bool _disposed = false;

  void _changed() {
    if (!_disposed) notifyListeners();
  }

  /// Beim ersten Anzeigen: Startverzeichnis des Hosts und der eigene Benutzerordner.
  Future<void> open() async {
    if (_opened) return;
    _opened = true;
    final home = Platform.environment[Platform.isWindows ? 'USERPROFILE' : 'HOME'];
    unawaited(goLocal(home != null && home.isNotEmpty ? home : Directory.current.path));
    remoteLoading = true;
    _changed();
    try {
      remotePath = await filesHome(hostId: hostId);
    } catch (error) {
      remotePath = '/';
      remoteError = errorText(error);
    }
    await refreshRemote();
  }

  Future<void> goRemote(String path) async {
    final previous = remotePath;
    remotePath = path.isEmpty ? '/' : path;
    await refreshRemote();
    // Lässt sich das Ziel nicht lesen, bleibt die Ansicht, wo sie war – mit Fehlermeldung.
    if (remoteError != null && previous.isNotEmpty && previous != remotePath) {
      final message = remoteError;
      remotePath = previous;
      await refreshRemote();
      remoteError = message;
      _changed();
    }
  }

  Future<void> refreshRemote() async {
    remoteLoading = true;
    remoteError = null;
    _changed();
    try {
      remote = await filesList(hostId: hostId, path: remotePath);
    } catch (error) {
      remoteError = errorText(error);
    }
    remoteLoading = false;
    _changed();
  }

  Future<void> mkdir(String name) async {
    await filesMkdir(hostId: hostId, path: remoteJoin(remotePath, name));
    await refreshRemote();
  }

  Future<void> rename(FileEntry entry, String newName) async {
    await filesRename(hostId: hostId, from: remoteJoin(remotePath, entry.name), to: remoteJoin(remotePath, newName));
    await refreshRemote();
  }

  Future<void> delete(FileEntry entry) async {
    // Eine Verknüpfung wird selbst gelöscht, auch wenn sie auf einen Ordner zeigt.
    await filesDelete(hostId: hostId, path: remoteJoin(remotePath, entry.name), isDir: entry.isDir && !entry.isLink);
    await refreshRemote();
  }

  // ---------------------------------------------------------------- dieses Gerät

  Future<void> goLocal(String path) async {
    localPath = path;
    await refreshLocal();
  }

  String? get localParent {
    if (localPath.isEmpty) return null;
    final parent = Directory(localPath).parent.path;
    if (parent != localPath) return parent;
    return Platform.isWindows ? '' : null;
  }

  Future<void> refreshLocal() async {
    localError = null;
    try {
      if (localPath.isEmpty) {
        local = [
          for (final letter in 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split(''))
            if (Directory('$letter:\\').existsSync()) LocalEntry(name: '$letter:', path: '$letter:\\', isDir: true),
        ];
      } else {
        final entries = <LocalEntry>[];
        await for (final entity in Directory(localPath).list(followLinks: false)) {
          final name = entity.uri.pathSegments.lastWhere((part) => part.isNotEmpty, orElse: () => entity.path);
          try {
            final stat = await entity.stat();
            final isDir = stat.type == FileSystemEntityType.directory;
            entries.add(
              LocalEntry(
                name: name,
                path: entity.path,
                isDir: isDir,
                size: isDir ? 0 : stat.size,
                modified: stat.modified,
              ),
            );
          } on FileSystemException {
            // Einträge, die sich nicht lesen lassen (gesperrte Systemdateien), bleiben weg.
          }
        }
        entries.sort((a, b) {
          if (a.isDir != b.isDir) return a.isDir ? -1 : 1;
          return a.name.toLowerCase().compareTo(b.name.toLowerCase());
        });
        local = entries;
      }
    } on FileSystemException catch (error) {
      local = const [];
      localError = error.osError?.message ?? error.message;
    }
    _changed();
  }

  // ---------------------------------------------------------------- Übertragungen

  /// Lädt eine Datei des aktuellen Verzeichnisses nach [localTarget].
  Transfer download(FileEntry entry, String localTarget) {
    final transfer = Transfer(
      name: entry.name,
      upload: false,
      remotePath: remoteJoin(remotePath, entry.name),
      localPath: localTarget,
    )..total = entry.size.toInt();
    _run(transfer, filesDownload(hostId: hostId, remotePath: transfer.remotePath, localPath: localTarget));
    return transfer;
  }

  /// Lädt eine Datei dieses Geräts ins aktuelle Verzeichnis des Hosts.
  Transfer upload(String localFile, {required String name}) {
    final transfer = Transfer(name: name, upload: true, remotePath: remoteJoin(remotePath, name), localPath: localFile);
    _run(transfer, filesUpload(hostId: hostId, localPath: localFile, remotePath: transfer.remotePath));
    return transfer;
  }

  void _run(Transfer transfer, Stream<TransferEvent> events) {
    transfers.insert(0, transfer);
    _changed();
    void finish(bool ok) {
      if (!transfer._completer.isCompleted) transfer._completer.complete(ok);
      _changed();
    }

    events.listen(
      (event) {
        switch (event) {
          case TransferEvent_Started(:final transferId):
            transfer.id = transferId;
            // Wurde schon abgebrochen, bevor die Kennung da war, jetzt nachholen.
            if (transfer.cancelled) filesCancel(transferId: transferId);
          case TransferEvent_Progress(:final done, :final total):
            transfer.done = done.toInt();
            transfer.total = total?.toInt() ?? transfer.total;
          case TransferEvent_Finished(:final bytes):
            transfer.done = bytes.toInt();
            transfer.total = bytes.toInt();
            transfer.finished = true;
            finish(true);
            return;
          case TransferEvent_Failed(:final message):
            transfer.error = clean(message);
            finish(false);
            return;
        }
        _changed();
      },
      onError: (Object error) {
        transfer.error = errorText(error);
        finish(false);
      },
      onDone: () {
        // Endet der Strom ohne Abschluss, wurde die Übertragung abgebrochen.
        if (transfer.running) transfer.cancelled = true;
        finish(transfer.finished);
      },
    );
  }

  void cancel(Transfer transfer) {
    if (!transfer.running) return;
    transfer.cancelled = true;
    final id = transfer.id;
    if (id != null) filesCancel(transferId: id);
    // Ein halb hochgeladener Rest soll nicht auf dem Host liegen bleiben.
    if (transfer.upload) filesDelete(hostId: hostId, path: transfer.remotePath, isDir: false).ignore();
    if (!transfer._completer.isCompleted) transfer._completer.complete(false);
    _changed();
  }

  void clearFinished() {
    transfers.removeWhere((transfer) => !transfer.running);
    _changed();
  }

  @override
  void dispose() {
    _disposed = true;
    for (final transfer in transfers) {
      final id = transfer.id;
      if (transfer.running && id != null) filesCancel(transferId: id);
    }
    super.dispose();
  }
}

String formatSize(int bytes) {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  var value = bytes.toDouble();
  var unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  final text = unit == 0 ? '$bytes' : value.toStringAsFixed(value >= 100 ? 0 : 1);
  return '${german ? text.replaceAll('.', ',') : text} ${units[unit]}';
}

String formatTime(DateTime time) {
  String two(int value) => value.toString().padLeft(2, '0');
  final local = time.toLocal();
  return '${local.year}-${two(local.month)}-${two(local.day)} ${two(local.hour)}:${two(local.minute)}';
}
