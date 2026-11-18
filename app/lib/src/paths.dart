import 'dart:io';

import 'package:flutter/services.dart';

import 'rust/api/app.dart';

/// Ort der Tresordatei. Steht nach [initPaths] fest.
late final String vaultPath;

/// Ermittelt den Ort der Tresordatei: unter Android der private Datenordner
/// der App, sonst derselbe Ort wie bei der Kommandozeile.
Future<void> initPaths() async {
  if (Platform.isAndroid) {
    final dir = await const MethodChannel('tephra/paths').invokeMethod<String>('filesDir');
    vaultPath = '$dir/vault.tephra';
  } else {
    vaultPath = defaultVaultPath();
  }
}
