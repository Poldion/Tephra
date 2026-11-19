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

  /// Ob die App von sich aus nach neuen Fassungen sucht (höchstens einmal am Tag).
  bool get checkUpdates => _checkUpdates;
  bool _checkUpdates = true;

  /// Wann zuletzt nach einer neuen Fassung gesucht wurde.
  DateTime? get lastUpdateCheck => _lastUpdateCheck;
  DateTime? _lastUpdateCheck;

  File get _file => File('${File(vaultPath).parent.path}${Platform.pathSeparator}app-settings.json');

  /// Liest die gespeicherten Einstellungen; fehlen sie, bleiben die Vorgaben.
  void load() {
    try {
      final data = jsonDecode(_file.readAsStringSync());
      if (data is! Map) return;
      _themeMode = ThemeMode.values.asNameMap()[data['theme']] ?? _themeMode;
      if (data['checkUpdates'] is bool) _checkUpdates = data['checkUpdates'] as bool;
      if (data['lastUpdateCheck'] is String) _lastUpdateCheck = DateTime.tryParse(data['lastUpdateCheck'] as String);
    } catch (_) {
      // Keine oder unlesbare Datei: Vorgaben behalten.
    }
  }

  void _save() {
    try {
      _file.parent.createSync(recursive: true);
      _file.writeAsStringSync(
        jsonEncode({
          'theme': _themeMode.name,
          'checkUpdates': _checkUpdates,
          'lastUpdateCheck': ?_lastUpdateCheck?.toUtc().toIso8601String(),
        }),
      );
    } catch (_) {
      // Lässt sich die Wahl nicht speichern, gilt sie bis zum Beenden.
    }
  }

  void setThemeMode(ThemeMode mode) {
    if (mode == _themeMode) return;
    _themeMode = mode;
    notifyListeners();
    _save();
  }

  void setCheckUpdates(bool value) {
    if (value == _checkUpdates) return;
    _checkUpdates = value;
    notifyListeners();
    _save();
  }

  void noteUpdateCheck() {
    _lastUpdateCheck = DateTime.now();
    _save();
  }
}

final settings = AppSettings();
