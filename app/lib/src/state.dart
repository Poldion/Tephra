import 'dart:async';

import 'package:flutter/widgets.dart';

import 'platform.dart';
import 'rust/api/app.dart';
import 'session.dart';

/// Zustand, der über die einzelnen Bildschirme hinaus lebt: die offenen
/// Terminalsitzungen.
class AppState extends ChangeNotifier {
  AppState() {
    // Nach einem Netzwechsel (WLAN ↔ Mobilnetz) ist eine Verbindung oft schon tot, ohne dass es jemand merkt;
    // ebenso, wenn die App lange im Hintergrund war. Dann gleich prüfen statt auf die Lebenszeichen zu warten.
    _network = networkChanges().listen((_) => checkConnections());
    _lifecycle = AppLifecycleListener(onResume: checkConnections);
  }

  final List<TerminalSession> sessions = [];
  int _active = 0;
  int _kept = 0;
  StreamSubscription<void>? _network;
  AppLifecycleListener? _lifecycle;

  /// Prüft alle Sitzungen und verbindet abgerissene neu.
  void checkConnections() {
    for (final session in sessions) {
      session.checkConnection();
    }
  }

  @override
  void dispose() {
    _network?.cancel();
    _lifecycle?.dispose();
    super.dispose();
  }

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
    // Die kleinste Nummer, die kein anderes offenes Terminal zu diesem Host hat.
    final used = {
      for (final s in sessions)
        if (s.host.id == host.id) s.slot,
    };
    var slot = 1;
    while (used.contains(slot)) {
      slot++;
    }
    final session = TerminalSession(host, slot: slot);
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
