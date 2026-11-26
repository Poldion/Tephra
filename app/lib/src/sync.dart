import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';

import 'paths.dart';
import 'platform.dart';
import 'rust/api/sync.dart';
import 'text.dart';
import 'ui/common.dart' show errorText;

/// Abgleich mit dem Sync-Server. Er läuft, solange der Tresor offen und das
/// Gerät mit einem Server verbunden ist.
class SyncController extends ChangeNotifier {
  /// Der Server, mit dem dieses Gerät abgleicht; `null` ohne Verbindung.
  SyncConnection? connection;

  /// Ob gerade ein Abgleich läuft oder ansteht.
  bool working = false;

  /// Ob die Sitzung beim Server abgelaufen ist und das Gerät neu angemeldet werden muss.
  bool expired = false;

  /// Fehler des letzten Abgleichs.
  String? error;

  /// Wann zuletzt erfolgreich abgeglichen wurde.
  DateTime? lastSync;

  /// Datensätze, die beim letzten Abgleich liegen blieben: unlesbar oder vom Server abgelehnt.
  int skipped = 0;

  /// Zählt hoch, sobald der Abgleich Daten von anderen Geräten gebracht hat;
  /// die Listen der App laden sich dann neu.
  final ValueNotifier<int> remoteChanges = ValueNotifier(0);

  StreamSubscription<SyncEvent>? _events;
  Future<void>? _trust;
  int _run = 0;

  /// Unter Android kennt nur die App die Zertifikate, denen das System
  /// vertraut; der Kern braucht sie vor der ersten Verbindung.
  Future<void> _ready() => _trust ??= _loadTrust();

  Future<void> _loadTrust() async {
    if (!Platform.isAndroid) return;
    try {
      syncTrustRoots(certificates: await trustedCertificates());
    } catch (_) {
      // Ohne Zertifikate scheitert eine https-Verbindung mit einer Fehlermeldung.
    }
  }

  /// Nach dem Entsperren: liest die Verbindung und gleicht ab, falls es eine gibt.
  void start() {
    _reset();
    try {
      connection = syncConnection();
    } catch (_) {
      connection = null;
    }
    if (connection != null) {
      _watch();
    } else {
      notifyListeners();
    }
  }

  /// Beim Sperren: hält den Abgleich an.
  void stop() {
    _reset();
    syncUnwatch();
    notifyListeners();
  }

  void _reset() {
    _run++;
    _events?.cancel();
    _events = null;
    connection = null;
    working = false;
    expired = false;
    error = null;
    lastSync = null;
    skipped = 0;
  }

  /// Startet den fortlaufenden Abgleich neu; sein erster Lauf beginnt sofort.
  Future<void> _watch() async {
    final run = ++_run;
    await _events?.cancel();
    _events = null;
    working = true;
    notifyListeners();
    await _ready();
    if (run != _run) return;
    try {
      _events = syncWatch().listen(
        (event) => _onEvent(run, event),
        onError: (Object error) => _onFailure(run, errorText(error), false),
        onDone: () {
          if (run != _run) return;
          working = false;
          notifyListeners();
        },
      );
    } catch (error) {
      _onFailure(run, errorText(error), false);
    }
  }

  void _onEvent(int run, SyncEvent event) {
    if (run != _run) return;
    switch (event) {
      case SyncEvent_Synced(:final summary):
        working = false;
        error = null;
        expired = false;
        lastSync = DateTime.now();
        skipped = summary.unreadable + summary.rejected.length;
        if (summary.pulled + summary.merged > 0) remoteChanges.value++;
        notifyListeners();
      case SyncEvent_Failed(:final message, :final expired):
        _onFailure(run, clean(message), expired);
    }
  }

  void _onFailure(int run, String message, bool expired) {
    if (run != _run) return;
    working = false;
    error = message;
    this.expired = expired;
    notifyListeners();
  }

  /// Gleicht jetzt ab, statt auf die nächste Änderung zu warten.
  void syncNow() {
    if (connection != null) _watch();
  }

  /// Legt für den offenen Tresor ein Konto an und lädt ihn hoch.
  Future<SyncSummary> register({
    required String server,
    required String username,
    required String password,
    String? invite,
    bool allowHttp = false,
  }) async {
    await _ready();
    final summary = await syncRegister(
      server: server.trim(),
      username: username.trim(),
      password: password,
      deviceName: await deviceName(),
      invite: invite,
      allowHttp: allowHttp,
    );
    start();
    return summary;
  }

  /// Holt den Tresor eines bestehenden Kontos auf dieses Gerät. Danach ist er entsperrt.
  Future<void> join({
    required String server,
    required String username,
    required String password,
    bool allowHttp = false,
  }) async {
    await _ready();
    await syncJoin(
      path: vaultPath,
      server: server.trim(),
      username: username.trim(),
      password: password,
      deviceName: await deviceName(),
      allowHttp: allowHttp,
    );
  }

  /// Meldet dieses Gerät mit dem aktuellen Master-Passwort des Kontos neu an.
  Future<void> reconnect(String password) async {
    await _ready();
    await syncReconnect(password: password, deviceName: await deviceName());
    start();
  }

  /// Trennt dieses Gerät vom Server; der Tresor bleibt.
  Future<void> disconnect() async {
    try {
      await syncDisconnect();
    } finally {
      start();
    }
  }

  Future<List<SyncDevice>> devices() => syncDevices();

  Future<void> removeDevice(String id, String password) => syncRemoveDevice(deviceId: id, password: password);
}

final syncState = SyncController();
