import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter/foundation.dart';

import 'platform.dart';
import 'rust/api/app.dart';
import 'settings.dart';
import 'text.dart';

/// Wie sich die App auf diesem Gerät aktualisieren lässt.
enum UpdateKind {
  /// Windows, mit dem Installer eingerichtet: neuer Installer, still ausgeführt.
  installer,

  /// Android: neue APK an die Paketinstallation des Systems.
  apk,

  /// Linux als AppImage: Die Datei wird durch die neue ersetzt.
  appImage,

  /// Alles andere (Zip, Debian-Paket): nur ein Hinweis mit Verweis auf die Seite.
  manual,
}

/// Eine veröffentlichte Fassung, die neuer ist als die laufende.
class Update {
  const Update({required this.version, required this.page, required this.kind, this.download, this.checksum});

  final String version;

  /// Seite der Veröffentlichung mit den Änderungen.
  final String page;
  final UpdateKind kind;

  /// Adresse der Datei für dieses Gerät und der zugehörigen Prüfsumme.
  final String? download;
  final String? checksum;
}

const _releases = 'https://api.github.com/repos/Poldion/Tephra/releases?per_page=20';

/// Vergleicht zwei Versionsnummern der Form `1.2.3`; ein vorangestelltes `v` zählt nicht.
int compareVersions(String a, String b) {
  List<int> parts(String version) {
    final core = version.startsWith('v') ? version.substring(1) : version;
    final numbers = core.split(RegExp(r'[-+]')).first.split('.');
    return [for (var i = 0; i < 3; i++) i < numbers.length ? int.tryParse(numbers[i]) ?? 0 : 0];
  }

  final (left, right) = (parts(a), parts(b));
  for (var i = 0; i < 3; i++) {
    if (left[i] != right[i]) return left[i].compareTo(right[i]);
  }
  return 0;
}

/// Wie die laufende App eingerichtet wurde.
UpdateKind currentKind() {
  if (Platform.isAndroid) return UpdateKind.apk;
  if (Platform.isWindows) {
    final folder = File(Platform.resolvedExecutable).parent.path;
    return File('$folder\\unins000.exe').existsSync() ? UpdateKind.installer : UpdateKind.manual;
  }
  if (Platform.isLinux && (Platform.environment['APPIMAGE'] ?? '').isNotEmpty) return UpdateKind.appImage;
  return UpdateKind.manual;
}

String? _assetName(UpdateKind kind, String version) => switch (kind) {
  UpdateKind.installer => 'Tephra-$version-windows-x64-setup.exe',
  UpdateKind.apk => 'Tephra-$version.apk',
  UpdateKind.appImage => 'Tephra-$version-x86_64.AppImage',
  UpdateKind.manual => null,
};

/// Sucht in der Liste der Veröffentlichungen die neueste, die neuer ist als
/// [current]. Fehlt dort die Datei für dieses Gerät, bleibt nur der Hinweis.
Update? pickUpdate(Object? releases, {required String current, required UpdateKind kind}) {
  if (releases is! List) return null;
  Map<String, dynamic>? newest;
  for (final release in releases) {
    if (release is! Map<String, dynamic> || release['draft'] == true) continue;
    final tag = release['tag_name'];
    if (tag is! String || !RegExp(r'^v\d+\.\d+\.\d+$').hasMatch(tag)) continue;
    if (newest == null || compareVersions(tag, newest['tag_name'] as String) > 0) newest = release;
  }
  if (newest == null) return null;
  final version = (newest['tag_name'] as String).substring(1);
  if (compareVersions(version, current) <= 0) return null;

  String? url(String? name) {
    if (name == null) return null;
    for (final asset in (newest!['assets'] as List? ?? const [])) {
      if (asset is Map && asset['name'] == name) return asset['browser_download_url'] as String?;
    }
    return null;
  }

  final name = _assetName(kind, version);
  final download = url(name);
  final checksum = url(name == null ? null : '$name.sha256');
  final complete = download != null && checksum != null;
  return Update(
    version: version,
    page: newest['html_url'] as String? ?? 'https://github.com/Poldion/Tephra/releases',
    kind: complete ? kind : UpdateKind.manual,
    download: complete ? download : null,
    checksum: complete ? checksum : null,
  );
}

HttpClient _client() => HttpClient()
  ..connectionTimeout = const Duration(seconds: 15)
  ..userAgent = 'Tephra/$appVersion';

Future<HttpClientResponse> _get(HttpClient client, String url) async {
  final request = await client.getUrl(Uri.parse(url));
  request.headers.set(HttpHeaders.acceptHeader, 'application/vnd.github+json, application/octet-stream');
  final response = await request.close();
  if (response.statusCode != 200) {
    await response.drain<void>();
    throw HttpException(
      t('Der Server antwortet mit ${response.statusCode}.', 'The server answered ${response.statusCode}.'),
    );
  }
  return response;
}

/// Fragt nach, ob es eine neuere Fassung gibt. `null`: Die App ist aktuell.
Future<Update?> checkForUpdate({String current = appVersion, UpdateKind? kind}) async {
  final client = _client();
  try {
    final response = await _get(client, _releases).timeout(const Duration(seconds: 20));
    final body = await utf8.decodeStream(response).timeout(const Duration(seconds: 20));
    settings.noteUpdateCheck();
    return pickUpdate(jsonDecode(body), current: current, kind: kind ?? currentKind());
  } finally {
    client.close(force: true);
  }
}

/// Lädt die Datei einer Aktualisierung nach [target] und prüft ihre Prüfsumme.
Future<void> downloadUpdate(Update update, String target, {void Function(int done, int? total)? onProgress}) async {
  final client = _client();
  final file = File(target);
  try {
    final expected = (await utf8.decodeStream(await _get(client, update.checksum!))).trim().split(RegExp(r'\s+')).first;
    final response = await _get(client, update.download!);
    final total = response.contentLength > 0 ? response.contentLength : null;
    await file.parent.create(recursive: true);
    final sink = file.openWrite();
    var done = 0;
    try {
      await for (final chunk in response) {
        sink.add(chunk);
        done += chunk.length;
        onProgress?.call(done, total);
      }
    } finally {
      await sink.close();
    }
    final actual = (await sha256.bind(file.openRead()).first).toString();
    if (actual != expected.toLowerCase()) {
      throw const FormatException('checksum');
    }
  } catch (error) {
    if (file.existsSync()) file.deleteSync();
    if (error is FormatException) {
      throw Exception(
        t(
          'Die heruntergeladene Datei stimmt nicht mit ihrer Prüfsumme überein und wurde verworfen.',
          'The downloaded file does not match its checksum and was discarded.',
        ),
      );
    }
    rethrow;
  } finally {
    client.close(force: true);
  }
}

/// Lädt die Aktualisierung und übergibt sie dem System. Unter Windows und
/// beim AppImage endet die App dabei und startet in der neuen Fassung wieder.
Future<void> installUpdate(Update update, {void Function(int done, int? total)? onProgress}) async {
  switch (update.kind) {
    case UpdateKind.manual:
      await openUrl(update.page);
    case UpdateKind.apk:
      final target = '${await cacheDir()}/updates/Tephra-${update.version}.apk';
      await downloadUpdate(update, target, onProgress: onProgress);
      await installApk(target);
    case UpdateKind.installer:
      final target = '${await cacheDir()}\\Tephra-${update.version}-setup.exe';
      await downloadUpdate(update, target, onProgress: onProgress);
      await _lockQuietly();
      // Der Installer schließt die App, ersetzt die Dateien und startet sie wieder.
      await Process.start(target, ['/SILENT', '/RELAUNCH=1'], mode: ProcessStartMode.detached);
      exit(0);
    case UpdateKind.appImage:
      final current = Platform.environment['APPIMAGE']!;
      final fresh = '$current.new';
      await downloadUpdate(update, fresh, onProgress: onProgress);
      await Process.run('chmod', ['+x', fresh]);
      await File(fresh).rename(current);
      await _lockQuietly();
      await Process.start(current, const [], mode: ProcessStartMode.detached);
      exit(0);
  }
}

Future<void> _lockQuietly() async {
  try {
    await lockVault();
  } catch (_) {
    // Beim Beenden zählt nur, dass nichts offen bleibt; ein Fehler hier hält die Aktualisierung nicht auf.
  }
}

/// Die zuletzt gefundene neuere Fassung, für den Hinweis in der Oberfläche.
final ValueNotifier<Update?> availableUpdate = ValueNotifier(null);

/// Sucht beim Start nach einer neueren Fassung, wenn das eingeschaltet ist und
/// die letzte Suche mehr als einen Tag her ist. Fehler bleiben still: Ohne
/// Netz gibt es eben keinen Hinweis.
Future<void> checkForUpdateQuietly() async {
  if (!settings.checkUpdates) return;
  final last = settings.lastUpdateCheck;
  if (last != null && DateTime.now().difference(last) < const Duration(hours: 24)) return;
  try {
    availableUpdate.value = await checkForUpdate();
  } catch (_) {
    // siehe oben
  }
}
