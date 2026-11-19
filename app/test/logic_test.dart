// Prüft die Teile der Oberfläche, die ohne Gerät und ohne Kern auskommen.

import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:tephra/src/files.dart';
import 'package:tephra/src/keys.dart';
import 'package:tephra/src/update.dart';

Uint8List _bytes(String text) => Uint8List.fromList(text.codeUnits);

void main() {
  group('Zusatztasten', () {
    test('Strg macht aus einem Buchstaben das Steuerzeichen', () {
      expect(applyModifiers(_bytes('c'), ctrl: true, alt: false), [0x03]);
      expect(applyModifiers(_bytes('C'), ctrl: true, alt: false), [0x03]);
      expect(applyModifiers(_bytes('['), ctrl: true, alt: false), [0x1B]);
      expect(applyModifiers(_bytes(' '), ctrl: true, alt: false), [0x00]);
      expect(applyModifiers(_bytes('?'), ctrl: true, alt: false), [0x7F]);
    });

    test('Strg lässt Ziffern und mehrere Zeichen unverändert', () {
      expect(applyModifiers(_bytes('1'), ctrl: true, alt: false), _bytes('1'));
      expect(applyModifiers(_bytes('ab'), ctrl: true, alt: false), _bytes('ab'));
    });

    test('Alt stellt ESC voran, auch zusammen mit Strg', () {
      expect(applyModifiers(_bytes('x'), ctrl: false, alt: true), [0x1B, 0x78]);
      expect(applyModifiers(_bytes('c'), ctrl: true, alt: true), [0x1B, 0x03]);
    });

    test('Antippen schaltet weiter, nur „einmal“ fällt nach einer Taste zurück', () {
      expect(ModifierState.off.next, ModifierState.once);
      expect(ModifierState.once.next, ModifierState.locked);
      expect(ModifierState.locked.next, ModifierState.off);
      expect(ModifierState.once.afterKey, ModifierState.off);
      expect(ModifierState.locked.afterKey, ModifierState.locked);
      expect(ModifierState.off.active, isFalse);
      expect(ModifierState.locked.active, isTrue);
    });
  });

  group('Pfade auf dem Host', () {
    test('verbinden', () {
      expect(remoteJoin('/', 'etc'), '/etc');
      expect(remoteJoin('/etc', 'motd'), '/etc/motd');
      expect(remoteJoin('/etc/', 'motd'), '/etc/motd');
    });

    test('übergeordnetes Verzeichnis', () {
      expect(remoteParent('/etc/ssh'), '/etc');
      expect(remoteParent('/etc/ssh/'), '/etc');
      expect(remoteParent('/etc'), '/');
      expect(remoteParent('/'), '/');
    });
  });

  group('Aktualisierung', () {
    test('Versionen werden als Zahlen verglichen', () {
      expect(compareVersions('0.10.0', '0.9.9'), greaterThan(0));
      expect(compareVersions('v1.2.3', '1.2.3'), 0);
      expect(compareVersions('1.2', '1.2.1'), lessThan(0));
      expect(compareVersions('2.0.0-beta', '2.0.0'), 0);
    });

    Map<String, dynamic> release(String tag, List<String> assets, {bool draft = false}) => {
      'tag_name': tag,
      'draft': draft,
      'html_url': 'https://example.org/$tag',
      'assets': [
        for (final name in assets) {'name': name, 'browser_download_url': 'https://example.org/$tag/$name'},
      ],
    };

    final releases = [
      release('v0.2.0', ['Tephra-0.2.0.apk', 'Tephra-0.2.0.apk.sha256', 'Tephra-0.2.0-windows-x64-setup.exe']),
      release('v0.3.0', [
        'Tephra-0.3.0.apk',
        'Tephra-0.3.0.apk.sha256',
        'Tephra-0.3.0-windows-x64-setup.exe',
        'Tephra-0.3.0-windows-x64-setup.exe.sha256',
      ]),
      release('v0.9.0', ['Tephra-0.9.0.apk', 'Tephra-0.9.0.apk.sha256'], draft: true),
      release('nightly', ['Tephra-nightly.apk']),
    ];

    test('die neueste veröffentlichte Fassung zählt, Entwürfe und fremde Tags nicht', () {
      final update = pickUpdate(releases, current: '0.1.0', kind: UpdateKind.apk)!;
      expect(update.version, '0.3.0');
      expect(update.kind, UpdateKind.apk);
      expect(update.download, endsWith('/Tephra-0.3.0.apk'));
      expect(update.checksum, endsWith('/Tephra-0.3.0.apk.sha256'));
    });

    test('wer aktuell ist, bekommt nichts angeboten', () {
      expect(pickUpdate(releases, current: '0.3.0', kind: UpdateKind.apk), isNull);
      expect(pickUpdate(releases, current: '1.0.0', kind: UpdateKind.apk), isNull);
      expect(pickUpdate('kein Verzeichnis', current: '0.1.0', kind: UpdateKind.apk), isNull);
    });

    test('ohne Datei oder Prüfsumme für dieses Gerät bleibt nur der Hinweis', () {
      final update = pickUpdate(releases, current: '0.1.0', kind: UpdateKind.appImage)!;
      expect(update.kind, UpdateKind.manual);
      expect(update.download, isNull);
      expect(update.page, 'https://example.org/v0.3.0');
      // Im Stand 0.2.0 fehlt dem Installer die Prüfsumme.
      final older = pickUpdate([releases.first], current: '0.1.0', kind: UpdateKind.installer)!;
      expect(older.kind, UpdateKind.manual);
    });
  });
}
