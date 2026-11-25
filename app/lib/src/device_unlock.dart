import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'paths.dart';
import 'rust/api/device_unlock.dart';

/// Entsperren ohne Master-Passwort auf diesem Gerät.
///
/// Unter Windows schützt die Datenschutz-API von Windows den Schlüssel des Tresors mit dem Benutzerkonto, unter
/// Android der Keystore des Telefons, nutzbar nur, solange das Telefon entsperrt ist. Die Einstellung gilt nur für
/// dieses eine Gerät und wird nicht abgeglichen.
class DeviceUnlock extends ChangeNotifier {
  static const _channel = MethodChannel('tephra/system');

  /// Ob es das auf diesem System gibt.
  bool get supported => Platform.isWindows || Platform.isAndroid;

  /// Ob gerade ein Schlüssel für dieses Gerät abgelegt ist.
  bool enabled = false;

  /// Liest, ob ein Schlüssel abgelegt ist.
  Future<bool> refresh() async {
    var stored = false;
    if (supported) {
      try {
        stored = Platform.isWindows
            ? deviceUnlockStored(vaultPath: vaultPath)
            : await _channel.invokeMethod<bool>('deviceUnlockStored') ?? false;
      } catch (_) {
        stored = false;
      }
    }
    if (stored != enabled) {
      enabled = stored;
      notifyListeners();
    }
    return stored;
  }

  /// Legt den Schlüssel des offenen Tresors für dieses Gerät ab.
  Future<void> enable() async {
    if (Platform.isWindows) {
      deviceUnlockProtect(vaultPath: vaultPath);
    } else {
      final key = deviceUnlockKey();
      try {
        await _channel.invokeMethod<void>('deviceUnlockSave', {'key': key});
      } finally {
        key.fillRange(0, key.length, 0);
      }
    }
    enabled = true;
    notifyListeners();
  }

  /// Entfernt den abgelegten Schlüssel; danach braucht dieses Gerät wieder das Master-Passwort.
  Future<void> disable() async {
    try {
      if (Platform.isWindows) {
        deviceUnlockForget(vaultPath: vaultPath);
      } else if (Platform.isAndroid) {
        await _channel.invokeMethod<void>('deviceUnlockForget');
      }
    } finally {
      enabled = false;
      notifyListeners();
    }
  }

  /// Entsperrt mit dem abgelegten Schlüssel. Wirft, wenn das nicht geht.
  Future<void> unlock() async {
    if (Platform.isWindows) {
      await deviceUnlockWithStored(vaultPath: vaultPath);
      return;
    }
    final loaded = await _channel.invokeMethod<Uint8List>('deviceUnlockLoad');
    if (loaded == null) throw StateError('Auf diesem Gerät ist kein Schlüssel abgelegt');
    // Was der Kanal liefert, lässt sich nicht überschreiben; gelöscht wird deshalb die eigene Kopie.
    final key = Uint8List.fromList(loaded);
    try {
      await deviceUnlockWithKey(vaultPath: vaultPath, key: key);
    } finally {
      key.fillRange(0, key.length, 0);
    }
  }
}

final deviceUnlock = DeviceUnlock();
