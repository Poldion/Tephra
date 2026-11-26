import 'dart:async';

import 'package:flutter/foundation.dart';

import 'paths.dart';
import 'platform.dart' show deviceName;
import 'rust/api/mcp.dart';
import 'settings.dart';
import 'ui/common.dart' show errorText;

/// Der MCP-Dienst der App: Über ihn greifen Sprachmodelle auf freigegebene
/// Hosts zu. Er läuft, solange er eingeschaltet und der Tresor offen ist.
///
/// Dazu kommen Rückfragen fernverwalteter Geräte (`tephra mcp remote on`): Solange der Tresor offen ist, schaut
/// die App alle paar Sekunden nach, ob eines auf eine Antwort wartet – auch auf dem Telefon, das selbst keinen
/// Dienst anbietet. Der Abgleich bringt Frage und Antwort hin und her.
class McpController extends ChangeNotifier {
  /// So oft wird im Tresor nach Rückfragen anderer Geräte geschaut.
  static const _remoteEvery = Duration(milliseconds: 1500);

  /// Ob dieses Gerät den Dienst anbieten kann (nicht auf dem Telefon).
  late final bool supported = mcpSupported();

  /// Ob der Kanal offen ist und Clients sich verbinden können.
  bool running = false;

  /// Warum der Dienst nicht läuft, obwohl er eingeschaltet ist.
  String? error;

  /// Namen der gerade verbundenen Clients.
  final List<String> clients = [];

  /// Die Rückfrage, die gerade auf eine Antwort wartet: zuerst die des eigenen Dienstes, sonst die älteste eines
  /// anderen Geräts.
  final ValueNotifier<McpRequest?> request = ValueNotifier(null);

  McpRequest? _local;
  List<McpRequest> _remote = const [];
  Timer? _remoteTimer;
  String _deviceName = '';

  StreamSubscription<McpEvent>? _events;
  int _run = 0;

  void _show() {
    final next = _local ?? (_remote.isEmpty ? null : _remote.first);
    if (next?.id != request.value?.id) request.value = next;
  }

  void _pollRemote() {
    try {
      _remote = mcpRemoteRequests();
    } catch (_) {
      _remote = const [];
    }
    _show();
  }

  /// Nach dem Entsperren: startet den Dienst, wenn er eingeschaltet ist, und das Nachschauen nach Rückfragen
  /// anderer Geräte.
  void start() {
    if (_remoteTimer == null) {
      _remoteTimer = Timer.periodic(_remoteEvery, (_) => _pollRemote());
      deviceName().then((name) => _deviceName = name).ignore();
      _pollRemote();
    }
    if (!supported || !settings.mcpService || _events != null) return;
    final run = ++_run;
    error = null;
    _events = mcpServiceStart(vaultPath: vaultPath).listen(
      (event) {
        if (run == _run) _onEvent(event);
      },
      onError: (Object failure) {
        if (run == _run) _ended(errorText(failure));
      },
      onDone: () {
        if (run == _run) _ended(null);
      },
    );
    notifyListeners();
  }

  /// Beendet den Dienst und alle Sitzungen der verbundenen Clients.
  void stop() {
    _run++;
    _remoteTimer?.cancel();
    _remoteTimer = null;
    _remote = const [];
    _events?.cancel();
    _events = null;
    mcpServiceStop();
    running = false;
    error = null;
    clients.clear();
    _local = null;
    request.value = null;
    notifyListeners();
  }

  /// Schaltet nur den eigenen Dienst ab; Rückfragen anderer Geräte kommen weiter.
  void _stopService() {
    _run++;
    _events?.cancel();
    _events = null;
    mcpServiceStop();
    running = false;
    error = null;
    clients.clear();
    _local = null;
    _show();
    notifyListeners();
  }

  /// Schaltet den Dienst ein oder aus; die Wahl gilt auch nach dem nächsten Start der App.
  void setEnabled(bool enabled) {
    settings.setMcpService(enabled);
    if (enabled) {
      start();
    } else {
      _stopService();
    }
  }

  /// Beantwortet die offene Rückfrage – die des eigenen Dienstes oder die eines anderen Geräts. `false`, wenn die
  /// Antwort nicht mehr zählt.
  bool answer(McpRequest asked, McpAnswer reply) {
    if (asked.device.isEmpty) return mcpAnswer(id: asked.id, answer: reply);
    try {
      final ok = mcpRemoteAnswer(id: asked.id, answer: reply, by: _deviceName.isEmpty ? 'Tephra' : _deviceName);
      _pollRemote();
      return ok;
    } catch (_) {
      return false;
    }
  }

  void _onEvent(McpEvent event) {
    switch (event) {
      case McpEvent_Started():
        running = true;
        error = null;
      case McpEvent_Request(:final request):
        _local = request;
        _show();
      case McpEvent_Withdrawn(:final id):
        if (_local?.id == id) _local = null;
        _show();
      case McpEvent_Connected(:final client):
        clients.add(client);
      case McpEvent_Disconnected(:final client):
        clients.remove(client);
      case McpEvent_Stopped(:final message):
        _ended(message);
        return;
    }
    notifyListeners();
  }

  void _ended(String? message) {
    _events?.cancel();
    _events = null;
    running = false;
    clients.clear();
    _local = null;
    _show();
    if (message != null) error = message;
    notifyListeners();
  }
}

final mcpState = McpController();
