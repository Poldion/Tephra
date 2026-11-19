import 'dart:io';

import 'package:flutter/services.dart';

import 'text.dart';

/// Was die App vom Betriebssystem braucht und Flutter nicht von sich aus kann.
/// Unter Android beantwortet es die eigene `MainActivity`.

const _paths = MethodChannel('tephra/paths');
const _files = MethodChannel('tephra/files');
const _system = MethodChannel('tephra/system');

/// Ordner für Zwischendateien, die jederzeit gelöscht werden dürfen.
Future<String> cacheDir() async {
  if (Platform.isAndroid) return (await _paths.invokeMethod<String>('cacheDir'))!;
  return Directory.systemTemp.path;
}

/// Lässt den Nutzer unter Android eine Datei wählen. Sie wird in den
/// Zwischenordner kopiert; zurück kommen ihr Pfad dort und ihr Name.
Future<({String path, String name})?> pickFile() async {
  final picked = await _files.invokeMapMethod<String, String>('pick');
  if (picked == null) return null;
  return (path: picked['path']!, name: picked['name']!);
}

/// Lässt den Nutzer unter Android einen Ort wählen und legt die Datei dort ab.
/// `false`, wenn er abbricht.
Future<bool> saveFile({required String path, required String name}) async =>
    await _files.invokeMethod<bool>('save', {'path': path, 'name': name}) ?? false;

/// Öffnet eine Adresse im Browser des Systems.
Future<void> openUrl(String url) async {
  if (Platform.isAndroid) {
    await _system.invokeMethod<void>('openUrl', {'url': url});
  } else if (Platform.isWindows) {
    await Process.run('rundll32', ['url.dll,FileProtocolHandler', url]);
  } else {
    await Process.run('xdg-open', [url]);
  }
}

/// Meldet Android, wie viele Sitzungen offen sind. Solange es welche gibt, hält ein Dienst mit
/// Benachrichtigung die App im Vordergrund – sonst friert Android sie im Hintergrund nach wenigen
/// Sekunden ein und schließt ihre Verbindungen.
Future<void> keepSessions(int count) async {
  if (!Platform.isAndroid) return;
  try {
    await _system.invokeMethod<void>('keepSessions', {
      'count': count,
      'title': appName,
      'text': count == 1 ? t('1 Sitzung offen', '1 session open') : t('$count Sitzungen offen', '$count sessions open'),
      'channel': t('Offene Sitzungen', 'Open sessions'),
    });
  } on PlatformException {
    // Ohne den Dienst laufen die Sitzungen weiter, solange die App sichtbar ist.
  } on MissingPluginException {
    // Kein Android-Gegenstück, etwa in Tests ohne Gerät.
  }
}

/// Name dieses Geräts, wie ihn der Sync-Server in der Geräteliste führt.
Future<String> deviceName() async {
  if (Platform.isAndroid) {
    try {
      final name = await _system.invokeMethod<String>('deviceName');
      return name == null || name.isEmpty ? 'Android' : name;
    } catch (_) {
      return 'Android';
    }
  }
  final name = Platform.localHostname;
  return name.isEmpty ? appName : name;
}

/// Zertifikate (DER), denen Android vertraut: die des Systems und die vom Nutzer hinzugefügten.
Future<List<Uint8List>> trustedCertificates() async =>
    await _system.invokeListMethod<Uint8List>('trustedCertificates') ?? const [];

/// Übergibt unter Android eine heruntergeladene APK an die Paketinstallation des Systems.
Future<void> installApk(String path) => _system.invokeMethod<void>('installApk', {'path': path});
