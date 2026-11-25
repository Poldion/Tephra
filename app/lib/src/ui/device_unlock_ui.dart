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

  Future<void> _set(bool on) async {
    if (on) {
      final ok = await confirm(
        context,
        title: t('Ohne Passwort entsperren?', 'Unlock without the password?'),
        body: Platform.isAndroid
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
      if (mounted) showMessage(context, errorText(error), error: true);
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
        title: Text(t('Ohne Master-Passwort entsperren', 'Unlock without the master password')),
        subtitle: Text(
          Platform.isAndroid
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
