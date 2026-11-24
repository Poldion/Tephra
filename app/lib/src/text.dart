import 'dart:ui';

/// Name der App, wie ihn die Oberfläche zeigt.
const appName = 'Tephra';

/// Version der App; entspricht `version` in `pubspec.yaml`.
const appVersion = '0.5.0';

/// Ob die Oberfläche deutsch spricht; sonst englisch.
final bool german = PlatformDispatcher.instance.locale.languageCode == 'de';

/// Wählt den Text in der Sprache des Systems.
String t(String de, String en) => german ? de : en;

/// Entfernt Steuerzeichen und Zeichen, die die Leserichtung umkehren, aus
/// Texten fremder Herkunft (Server, Fehlermeldungen), bevor sie angezeigt
/// werden. Zeilenumbrüche bleiben erhalten.
String clean(String text) {
  final out = StringBuffer();
  for (final rune in text.runes) {
    final control = (rune < 0x20 && rune != 0x0A) || (rune >= 0x7F && rune < 0xA0);
    final bidi = (rune >= 0x202A && rune <= 0x202E) || (rune >= 0x2066 && rune <= 0x2069);
    if (!control && !bidi) out.writeCharCode(rune);
  }
  return out.toString();
}
