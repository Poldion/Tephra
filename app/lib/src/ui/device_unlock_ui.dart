import 'dart:io';

import 'package:material_ui/material_ui.dart';

import '../device_unlock.dart';
import '../text.dart';
import 'common.dart';

/// Schalter im Dialog „Über Tephra“: dieses Gerät entsperrt den Tresor ohne Master-Passwort.
class DeviceUnlockSection extends StatefulWidget {
  const DeviceUnlockSection({super.key});

  @override
  State<DeviceUnlockSection> createState() => _DeviceUnlockSectionState();
}

class _DeviceUnlockSectionState extends State<DeviceUnlockSection> {
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    deviceUnlock.refresh();
  }

  bool get _finger => Platform.isAndroid && deviceUnlock.fingerprintSupported;

  Future<void> _set(bool on) async {
    if (on && _finger) {
      final problem = await deviceUnlock.fingerprintProblem();
      if (!mounted) return;
      if (problem != null) {
        showMessage(context, problem.toString(), error: true);
        return;
      }
    }
    if (on) {
      final ok = await confirm(
        context,
        title: _finger
            ? t('Mit Fingerabdruck entsperren?', 'Unlock with your fingerprint?')
            : t('Ohne Passwort entsperren?', 'Unlock without the password?'),
        body: _finger
            ? t(
                'Tephra geht auf diesem Telefon dann mit deinem Fingerabdruck auf statt mit dem Master-Passwort. Den Schlüssel dafür gibt Android nur nach einem Fingerabdruck heraus; wer das Telefon nur entsperrt in der Hand hat, kommt nicht hinein. Fügst du am Telefon einen Fingerabdruck hinzu oder entfernst einen, fragt Tephra einmal nach dem Master-Passwort. Auf anderen Geräten ändert sich nichts, und das Master-Passwort bleibt gültig.',
                'Tephra then opens on this phone with your fingerprint instead of the master password. Android releases the key for this only after a fingerprint; holding the phone unlocked is not enough. If you add or remove a fingerprint on the phone, Tephra asks for the master password once. Nothing changes on other devices, and the master password stays valid.',
              )
            : Platform.isAndroid
            ? t(
                'Tephra geht auf diesem Telefon dann ohne Master-Passwort auf. Geschützt ist der Tresor hier nur noch durch die Sperre des Telefons: Wer es entsperrt in der Hand hat, kommt an alle Zugänge. Auf anderen Geräten ändert sich nichts, und das Master-Passwort bleibt gültig.',
                'Tephra then opens on this phone without the master password. Here the vault is protected only by the lock of the phone: whoever holds it unlocked gets to all credentials. Nothing changes on other devices, and the master password stays valid.',
              )
            : t(
                'Tephra geht auf diesem Rechner dann beim Start ohne Master-Passwort auf. Geschützt ist der Tresor hier nur noch durch dein Windows-Konto: Wer an deinem angemeldeten Konto sitzt oder es übernimmt, kommt an alle Zugänge. Auf anderen Geräten ändert sich nichts, und das Master-Passwort bleibt gültig.',
                'Tephra then opens on this computer at start without the master password. Here the vault is protected only by your Windows account: whoever sits at your signed-in account or takes it over gets to all credentials. Nothing changes on other devices, and the master password stays valid.',
              ),
        action: t('Einschalten', 'Switch on'),
      );
      if (!ok || !mounted) return;
    }
    setState(() => _busy = true);
    try {
      if (on) {
        await deviceUnlock.enable();
      } else {
        await deviceUnlock.disable();
      }
    } catch (error) {
      // Im Fingerabdruck-Dialog abgebrochen: bleibt aus, ohne Meldung.
      final cancelled = error is DeviceUnlockException && error.code == 'cancelled';
      if (mounted && !cancelled) showMessage(context, errorText(error), error: true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: deviceUnlock,
      builder: (context, _) => SwitchListTile(
        key: const Key('device-unlock-switch'),
        value: deviceUnlock.enabled,
        onChanged: _busy ? null : _set,
        contentPadding: EdgeInsets.zero,
        title: Text(
          _finger
              ? t('Mit Fingerabdruck entsperren', 'Unlock with fingerprint')
              : t('Ohne Master-Passwort entsperren', 'Unlock without the master password'),
        ),
        subtitle: Text(
          _finger
              ? t(
                  'Nur auf diesem Telefon, statt des Master-Passworts.',
                  'Only on this phone, instead of the master password.',
                )
              : Platform.isAndroid
              ? t('Nur auf diesem Telefon, solange es entsperrt ist.', 'Only on this phone, while it is unlocked.')
              : t(
                  'Nur auf diesem Rechner, geschützt durch dein Windows-Konto.',
                  'Only on this computer, protected by your Windows account.',
                ),
        ),
      ),
    );
  }
}
