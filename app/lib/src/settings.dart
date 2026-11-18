import 'dart:convert';
import 'dart:io';

import 'package:material_ui/material_ui.dart';

import 'paths.dart';

/// Einstellungen der Oberfläche, die schon vor dem Entsperren gelten und
/// deshalb neben dem Tresor liegen statt in ihm. Nichts davon ist vertraulich.
class AppSettings extends ChangeNotifier {
  /// Helles oder dunkles Design; ohne gespeicherte Wahl dunkel.
  ThemeMode get themeMode => _themeMode;
  ThemeMode _themeMode = ThemeMode.dark;

  File get _file => File('${File(vaultPath).parent.path}${Platform.pathSeparator}app-settings.json');

  /// Liest die gespeicherten Einstellungen; fehlen sie, bleiben die Vorgaben.
  void load() {
    try {
      final data = jsonDecode(_file.readAsStringSync());
      if (data is Map) _themeMode = ThemeMode.values.asNameMap()[data['theme']] ?? _themeMode;
    } catch (_) {
      // Keine oder unlesbare Datei: Vorgaben behalten.
    }
  }

  void setThemeMode(ThemeMode mode) {
    if (mode == _themeMode) return;
    _themeMode = mode;
    notifyListeners();
    try {
      _file.parent.createSync(recursive: true);
      _file.writeAsStringSync(jsonEncode({'theme': mode.name}));
    } catch (_) {
      // Lässt sich die Wahl nicht speichern, gilt sie bis zum Beenden.
    }
  }
}

final settings = AppSettings();
