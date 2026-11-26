import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'paths.dart';
import 'rust/api/device_unlock.dart';
import 'text.dart';

/// Warum das Entsperren auf diesem Gerät nicht ging.
class DeviceUnlockException implements Exception {
  DeviceUnlockException(this.code, [this.detail = '']);

  /// `cancelled`, `lockout`, `invalidated`, `noneenrolled`, `nohardware` oder `deviceUnlock` (sonstiger Fehler).
  final String code;
  final String detail;

  /// Abgebrochen oder vorübergehend gesperrt: Der abgelegte Schlüssel bleibt gültig und wird nicht entfernt.
  bool get temporary => code == 'cancelled' || code == 'lockout';

  @override
  String toString() => switch (code) {
    'cancelled' => t('Abgebrochen.', 'Cancelled.'),
    'lockout' => t(
      'Zu viele Versuche mit dem Fingerabdruck. Nimm das Master-Passwort oder warte etwas.',
      'Too many fingerprint attempts. Use the master password or wait a while.',
    ),
    'invalidated' => t(
      'Am Telefon wurde seit dem Einschalten ein Fingerabdruck hinzugefügt oder entfernt.',
      'A fingerprint was added to or removed from the phone since this was switched on.',
    ),
    'noneenrolled' => t(
      'Auf dem Telefon ist kein Fingerabdruck eingerichtet. Richte zuerst in den Einstellungen von Android einen ein.',
      'No fingerprint is set up on the phone. Set one up in the Android settings first.',
    ),
    'nohardware' => t('Der Fingerabdrucksensor ist nicht nutzbar.', 'The fingerprint sensor is not available.'),
    _ => detail.isEmpty ? code : detail,
  };
}

/// Entsperren ohne Master-Passwort auf diesem Gerät.
///
/// Unter Windows schützt die Datenschutz-API von Windows den Schlüssel des Tresors mit dem Benutzerkonto. Unter
/// Android hält ihn der Keystore des Telefons und gibt ihn ab Android 9 nur nach einem Fingerabdruck heraus;
/// darunter genügt, dass das Telefon entsperrt ist. Die Einstellung gilt nur für dieses eine Gerät und wird nicht
/// abgeglichen.
class DeviceUnlock extends ChangeNotifier {
  static const _channel = MethodChannel('tephra/system');

  /// Ob es das auf diesem System gibt.
  bool get supported => Platform.isWindows || Platform.isAndroid;

  /// Ob gerade ein Schlüssel für dieses Gerät abgelegt ist.
  bool enabled = false;

  /// Ob der abgelegte Schlüssel erst nach einem Fingerabdruck herauskommt (Android 9 und neuer).
  bool fingerprint = false;

  /// Ob Android hier den Fingerabdruck-Dialog kennt; dann legt [enable] den Schlüssel immer mit Fingerabdruck ab.
  bool get fingerprintSupported => _fingerprintSupported;
  bool _fingerprintSupported = false;

  /// Nach dem Entsperren mit einer Ablage aus Version 0.8.0 (ohne Fingerabdruck): Sie soll auf den Fingerabdruck
  /// umgestellt werden, siehe [upgrade].
  bool upgradePending = false;

  /// Liest, ob ein Schlüssel abgelegt ist.
  Future<bool> refresh() async {
    var stored = false;
    var finger = false;
    if (supported) {
      try {
        if (Platform.isWindows) {
          stored = deviceUnlockStored(vaultPath: vaultPath);
        } else {
          final kind = await _channel.invokeMethod<String>('deviceUnlockKind') ?? 'none';
          stored = kind != 'none';
          finger = kind == 'fingerprint';
          _fingerprintSupported = await _channel.invokeMethod<String>('deviceUnlockFingerprint') != 'unsupported';
        }
      } catch (_) {
        stored = false;
      }
    }
    if (stored != enabled || finger != fingerprint) {
      enabled = stored;
      fingerprint = finger;
      notifyListeners();
    }
    return stored;
  }

  /// Warum der Fingerabdruck auf diesem Telefon nicht geht; `null`, wenn er geht oder es ihn hier nicht gibt.
  Future<DeviceUnlockException?> fingerprintProblem() async {
    if (!Platform.isAndroid) return null;
    final problem = await _channel.invokeMethod<String>('deviceUnlockFingerprint');
    if (problem == null || problem == 'unsupported') return null;
    return DeviceUnlockException(problem);
  }

  Map<String, String> _prompt({required String title, String subtitle = ''}) => {
    'title': title,
    'subtitle': subtitle,
    'cancel': t('Master-Passwort verwenden', 'Use master password'),
  };

  /// Legt den Schlüssel des offenen Tresors für dieses Gerät ab. Unter Android ab Version 9 fragt das System
  /// dabei nach dem Fingerabdruck.
  Future<void> enable() async {
    if (Platform.isWindows) {
      deviceUnlockProtect(vaultPath: vaultPath);
    } else {
      final key = deviceUnlockKey();
      try {
        await _channel.invokeMethod<void>('deviceUnlockSave', {
          'key': key,
          ..._prompt(
            title: t('Fingerabdruck für $appName', 'Fingerprint for $appName'),
            subtitle: t(
              'Damit entsperrst du den Tresor auf diesem Telefon.',
              'You will use it to unlock the vault on this phone.',
            ),
          ),
        });
      } on PlatformException catch (error) {
        throw DeviceUnlockException(error.code, error.message ?? '');
      } finally {
        key.fillRange(0, key.length, 0);
      }
    }
    upgradePending = false;
    await refresh();
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
      fingerprint = false;
      upgradePending = false;
      notifyListeners();
    }
  }

  /// Entsperrt mit dem abgelegten Schlüssel, unter Android gegebenenfalls nach dem Fingerabdruck. Wirft
  /// [DeviceUnlockException] oder einen anderen Fehler, wenn das nicht geht.
  Future<void> unlock() async {
    if (Platform.isWindows) {
      await deviceUnlockWithStored(vaultPath: vaultPath);
      return;
    }
    final Uint8List? loaded;
    try {
      loaded = await _channel.invokeMethod<Uint8List>(
        'deviceUnlockLoad',
        _prompt(title: t('$appName entsperren', 'Unlock $appName')),
      );
    } on PlatformException catch (error) {
      throw DeviceUnlockException(error.code, error.message ?? '');
    }
    if (loaded == null) throw StateError('Auf diesem Gerät ist kein Schlüssel abgelegt');
    // Was der Kanal liefert, lässt sich nicht überschreiben; gelöscht wird deshalb die eigene Kopie.
    final key = Uint8List.fromList(loaded);
    try {
      await deviceUnlockWithKey(vaultPath: vaultPath, key: key);
    } finally {
      key.fillRange(0, key.length, 0);
    }
    upgradePending = enabled && !fingerprint && fingerprintSupported;
  }

  /// Stellt eine Ablage ohne Fingerabdruck um, sobald der Tresor offen ist. Gelingt das nicht (abgebrochen, kein
  /// Fingerabdruck eingerichtet), wird sie entfernt: Ohne Fingerabdruck geht der Tresor auf diesem Telefon nicht
  /// mehr ohne Passwort auf. Gibt eine Meldung für den Nutzer zurück.
  Future<({String text, bool error})?> upgrade() async {
    if (!upgradePending) return null;
    upgradePending = false;
    try {
      await enable();
      return (
        text: t(
          'Entsperren ohne Passwort verlangt ab jetzt deinen Fingerabdruck.',
          'Unlocking without the password now asks for your fingerprint.',
        ),
        error: false,
      );
    } catch (error) {
      await disable();
      return (
        text: t(
          'Entsperren ohne Passwort geht jetzt nur noch mit Fingerabdruck und ist darum ausgeschaltet ($error). Unter „Über $appName“ kannst du es wieder einschalten.',
          'Unlocking without the password now needs your fingerprint and has been switched off ($error). You can switch it on again under "About $appName".',
        ),
        error: true,
      );
    }
  }
}

final deviceUnlock = DeviceUnlock();
