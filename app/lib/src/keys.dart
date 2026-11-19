import 'dart:typed_data';

/// Zustand einer Zusatztaste wie Strg oder Alt auf der Tastenleiste:
/// aus, für die nächste Taste, oder festgestellt bis zum Lösen.
enum ModifierState {
  off,
  once,
  locked;

  bool get active => this != ModifierState.off;

  /// Antippen schaltet weiter: aus → einmal → fest → aus.
  ModifierState get next => ModifierState.values[(index + 1) % ModifierState.values.length];

  /// Nach einer Taste: Nur „einmal“ fällt zurück.
  ModifierState get afterKey => this == ModifierState.once ? ModifierState.off : this;
}

/// Wendet Strg und Alt auf getippten Text an, wie es eine PC-Tastatur täte:
/// Strg macht aus einem einzelnen Zeichen das Steuerzeichen, Alt stellt ESC voran.
Uint8List applyModifiers(Uint8List bytes, {required bool ctrl, required bool alt}) {
  var out = bytes;
  if (ctrl && bytes.length == 1) {
    final byte = bytes[0];
    if (byte == 0x20) {
      out = Uint8List.fromList([0x00]); // Strg+Leertaste
    } else if (byte == 0x3F) {
      out = Uint8List.fromList([0x7F]); // Strg+?
    } else if (byte >= 0x40 && byte < 0x7F) {
      out = Uint8List.fromList([byte & 0x1F]);
    }
  }
  if (alt) out = Uint8List.fromList([0x1B, ...out]);
  return out;
}
