import 'package:flutter/foundation.dart';

import 'platform.dart';
import 'rust/api/app.dart';
import 'session.dart';

/// Zustand, der über die einzelnen Bildschirme hinaus lebt: die offenen
/// Terminalsitzungen.
class AppState extends ChangeNotifier {
  final List<TerminalSession> sessions = [];
  int _active = 0;
  int _kept = 0;

  /// Sagt dem System bei jeder Änderung, wie viele Sitzungen noch eine Verbindung halten.
  @override
  void notifyListeners() {
    _keep();
    super.notifyListeners();
  }

  void _keep() {
    final open = sessions.where((s) => s.state != SessionState.closed).length;
    if (open == _kept) return;
    _kept = open;
    keepSessions(open);
  }

  TerminalSession? get active => sessions.isEmpty ? null : sessions[_active.clamp(0, sessions.length - 1)];

  /// Anzahl der Sitzungen, die auf eine Antwort warten.
  int get waiting => sessions.where((s) => s.needsAttention).length;

  void open(HostView host) {
    final session = TerminalSession(host);
    session.addListener(notifyListeners);
    sessions.add(session);
    _active = sessions.length - 1;
    notifyListeners();
  }

  void select(TerminalSession session) {
    final index = sessions.indexOf(session);
    if (index < 0 || index == _active) return;
    _active = index;
    notifyListeners();
  }

  void close(TerminalSession session) {
    final index = sessions.indexOf(session);
    if (index < 0) return;
    sessions.removeAt(index);
    if (_active > index || _active >= sessions.length) _active = (_active - 1).clamp(0, sessions.length);
    session.removeListener(notifyListeners);
    session.close();
    notifyListeners();
  }

  void closeAll() {
    for (final session in sessions) {
      session.removeListener(notifyListeners);
      session.close();
    }
    sessions.clear();
    _active = 0;
    _keep();
  }
}
