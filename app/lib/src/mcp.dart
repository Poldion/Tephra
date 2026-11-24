import 'dart:async';

import 'package:flutter/foundation.dart';

import 'paths.dart';
import 'rust/api/mcp.dart';
import 'settings.dart';
import 'ui/common.dart' show errorText;

/// Der MCP-Dienst der App: Über ihn greifen Sprachmodelle auf freigegebene
/// Hosts zu. Er läuft, solange er eingeschaltet und der Tresor offen ist.
class McpController extends ChangeNotifier {
  /// Ob dieses Gerät den Dienst anbieten kann (nicht auf dem Telefon).
  late final bool supported = mcpSupported();

  /// Ob der Kanal offen ist und Clients sich verbinden können.
  bool running = false;

  /// Warum der Dienst nicht läuft, obwohl er eingeschaltet ist.
  String? error;

  /// Namen der gerade verbundenen Clients.
  final List<String> clients = [];

  /// Die Rückfrage, die gerade auf eine Antwort wartet.
  final ValueNotifier<McpRequest?> request = ValueNotifier(null);

  StreamSubscription<McpEvent>? _events;
  int _run = 0;

  /// Nach dem Entsperren: startet den Dienst, wenn er eingeschaltet ist.
  void start() {
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
    _events?.cancel();
    _events = null;
    mcpServiceStop();
    running = false;
    error = null;
    clients.clear();
    request.value = null;
    notifyListeners();
  }

  /// Schaltet den Dienst ein oder aus; die Wahl gilt auch nach dem nächsten Start der App.
  void setEnabled(bool enabled) {
    settings.setMcpService(enabled);
    if (enabled) {
      start();
    } else {
      stop();
    }
  }

  /// Beantwortet die offene Rückfrage. `false`, wenn die Antwort nicht mehr zählt.
  bool answer(McpRequest asked, McpAnswer reply) => mcpAnswer(id: asked.id, answer: reply);

  void _onEvent(McpEvent event) {
    switch (event) {
      case McpEvent_Started():
        running = true;
        error = null;
      case McpEvent_Request(:final request):
        this.request.value = request;
      case McpEvent_Withdrawn(:final id):
        if (request.value?.id == id) request.value = null;
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
    request.value = null;
    if (message != null) error = message;
    notifyListeners();
  }
}

final mcpState = McpController();
