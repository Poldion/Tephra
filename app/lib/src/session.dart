import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:xterm/xterm.dart';

import 'files.dart';
import 'keys.dart';
import 'rust/api/app.dart';
import 'text.dart';

enum SessionState { connecting, connected, closed }

/// Pausen zwischen den Versuchen, eine abgerissene Verbindung wiederherzustellen. Nach dem letzten gibt die
/// Sitzung auf, bis das Netz wechselt, die App wieder in den Vordergrund kommt oder der Nutzer es selbst versucht.
const reconnectDelays = [1, 3, 5, 10, 20, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30];

/// Setzt das Terminal nach einem Abbruch zurück: zurück aus dem Vollbild eines Programms (tmux, vim), keine
/// Mausmeldungen, kein Einfügemodus, sichtbarer Cursor, normale Schrift. Das abgerissene Programm kann das
/// selbst nicht mehr tun.
const _resetModes = '\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[?25h\x1b[0m';

/// Eine Terminalsitzung zu einem Host. Sie lebt unabhängig davon, ob ihr
/// Terminal gerade angezeigt wird; die Ausgabe sammelt sich im Puffer.
class TerminalSession extends ChangeNotifier {
  /// [slot] unterscheidet mehrere Terminals zum selben Host (ab 1). Bei Hosts mit „Sitzung halten“ gehört
  /// zu jeder Nummer eine eigene tmux-Sitzung, die nach einem Abbruch wiedergefunden wird.
  TerminalSession(this.host, {this.slot = 1}) {
    terminal.onOutput = _send;
    terminal.onResize = (cols, rows, _, _) => _resize(cols, rows);
    _start();
  }

  final HostView host;
  final int slot;
  final Terminal terminal = Terminal(maxLines: 10000);
  final TerminalController controller = TerminalController();

  SessionState state = SessionState.connecting;

  /// Kurze Beschreibung des Zustands für die Kopfzeile.
  String status = '';

  /// Offene Rückfrage (Hostschlüssel, Passwort, Fragen des Servers).
  SessionEvent? prompt;

  int? _id;
  int _run = 0;
  bool _disposed = false;

  /// Ob das Ende der Sitzung schon im Terminal steht (Shell beendet oder Fehler gemeldet).
  bool _explained = false;
  Future<void> _writes = Future.value();
  late ByteConversionSink _decoder;

  /// Wie oft in Folge schon versucht wurde, die abgerissene Verbindung wiederherzustellen; 0, solange sie steht
  /// oder nie stand.
  int _attempts = 0;
  Timer? _retry;

  /// Ob sich die Shell selbst beendet hat; dann wird nicht neu verbunden.
  bool _exited = false;

  /// Ob die Sitzung nach einem Abbruch aufgegeben hat. Ein Netzwechsel versucht es dann noch einmal.
  bool _gaveUp = false;

  /// Ob der Nutzer eine Rückfrage beim Wiederverbinden abgebrochen hat; dann versucht es auch ein Netzwechsel
  /// nicht von selbst noch einmal.
  bool _declined = false;
  bool _checking = false;

  /// Ob die Sitzung gerade versucht, eine abgerissene Verbindung wiederherzustellen.
  bool get reconnecting => _attempts > 0 && state == SessionState.connecting;

  bool get needsAttention => prompt != null;

  /// Ob statt des Terminals die Dateien des Hosts gezeigt werden.
  bool get showFiles => _showFiles;
  bool _showFiles = false;

  set showFiles(bool value) {
    if (_showFiles == value) return;
    _showFiles = value;
    notifyListeners();
  }

  /// Dateien des Hosts; entstehen beim ersten Zugriff.
  FilesController get files => _files ??= FilesController(host.id);
  FilesController? _files;

  /// Zusatztasten der Tastenleiste am Telefon.
  ModifierState get ctrl => _ctrl;
  ModifierState get alt => _alt;
  ModifierState _ctrl = ModifierState.off;
  ModifierState _alt = ModifierState.off;

  /// Ob die Tastenleiste gerade die Funktionstasten zeigt.
  bool get functionKeys => _functionKeys;
  bool _functionKeys = false;

  /// Solange eine Taste der Leiste gedrückt wird, hat das Terminal Strg und Alt schon eingerechnet.
  bool _keyPress = false;

  void cycleCtrl() {
    _ctrl = _ctrl.next;
    notifyListeners();
  }

  void cycleAlt() {
    _alt = _alt.next;
    notifyListeners();
  }

  void toggleFunctionKeys() {
    _functionKeys = !_functionKeys;
    notifyListeners();
  }

  /// Taste der Tastenleiste, zusammen mit den gerade aktiven Zusatztasten.
  void pressKey(TerminalKey key) {
    _keyPress = true;
    try {
      terminal.keyInput(key, ctrl: _ctrl.active, alt: _alt.active);
    } finally {
      _keyPress = false;
    }
    _releaseModifiers();
  }

  /// Zeichen der Tastenleiste; Strg und Alt wirken wie bei getipptem Text.
  void typeText(String text) => terminal.textInput(text);

  void _releaseModifiers() {
    final (ctrl, alt) = (_ctrl.afterKey, _alt.afterKey);
    if (ctrl == _ctrl && alt == _alt) return;
    _ctrl = ctrl;
    _alt = alt;
    notifyListeners();
  }

  void _start() {
    final run = ++_run;
    _retry?.cancel();
    _retry = null;
    _id = null;
    prompt = null;
    _explained = false;
    _exited = false;
    _gaveUp = false;
    state = SessionState.connecting;
    status = _attempts == 0
        ? t('Verbindung wird aufgebaut …', 'Connecting …')
        : t('Verbindung wird wiederhergestellt …', 'Reconnecting …');
    _writes = Future.value();
    _decoder = const Utf8Decoder(allowMalformed: true).startChunkedConversion(_TerminalSink(terminal));
    try {
      openSession(hostId: host.id, slot: slot, cols: terminal.viewWidth, rows: terminal.viewHeight).listen(
        (event) => _onEvent(run, event),
        onError: (Object error) => _onFailure(run, error.toString()),
        onDone: () => _onClosed(run),
      );
    } catch (error) {
      _onFailure(run, error.toString());
      _onClosed(run);
    }
  }

  void _onEvent(int run, SessionEvent event) {
    if (run != _run || _disposed) {
      // Ereignis einer verworfenen Sitzung: nur noch aufräumen.
      if (event is SessionEvent_Opened) sessionClose(sessionId: event.sessionId).ignore();
      return;
    }
    switch (event) {
      case SessionEvent_Opened(:final sessionId):
        _id = sessionId;
      case SessionEvent_Connected(:final description):
        if (_attempts > 0) {
          terminal.write(_resetModes);
          // Mit tmux zeichnet die wiedergefundene Sitzung sich gleich selbst; ohne beginnt eine neue Shell.
          if (!host.keepSession) {
            _note(t('Neu verbunden. Die vorige Shell ist beendet.', 'Reconnected. The previous shell has ended.'));
          }
        }
        _attempts = 0;
        state = SessionState.connected;
        status = clean(description);
        _resize(terminal.viewWidth, terminal.viewHeight);
      case SessionEvent_Output(:final data):
        _decoder.add(data);
        return;
      case SessionEvent_HostKey() || SessionEvent_Password() || SessionEvent_ServerPrompts():
        prompt = event;
      case SessionEvent_Exit(:final code):
        final suffix = code == null ? '' : ' (${t('Code', 'code')} $code)';
        _note('${t('Sitzung beendet', 'Session ended')}$suffix');
        _explained = true;
        _exited = true;
      case SessionEvent_Failed(:final message):
        _onFailure(run, message);
        return;
      case SessionEvent_Closed():
        _onClosed(run);
        return;
    }
    notifyListeners();
  }

  /// Ob ein Ende dieser Verbindung als Abbruch gilt, nach dem neu verbunden wird: Sie stand schon (oder es
  /// ist bereits ein neuer Versuch), und die Shell hat sich nicht selbst beendet.
  bool get _wouldRetry => !_exited && (state == SessionState.connected || _attempts > 0);

  void _onFailure(int run, String message) {
    if (run != _run || _disposed) return;
    final text = clean(message).trim();
    status = text.split('\n').first;
    // Bei einem Abbruch steht der Grund nur in der Statuszeile; ins Terminal kommt erst etwas, wenn die
    // Sitzung aufgibt. Sonst füllte jeder gescheiterte Versuch im Funkloch eine Zeile.
    if (!_wouldRetry) {
      _note(text, error: true);
      _explained = true;
    }
    notifyListeners();
  }

  void _onClosed(int run) {
    if (run != _run || _disposed || state == SessionState.closed) return;
    if (_wouldRetry) {
      _connectionLost();
      return;
    }
    // Reißt die Verbindung ab, endet die Sitzung ohne ein Wort des Servers.
    if (state == SessionState.connected && !_explained) {
      _note(t('Verbindung getrennt', 'Connection lost'), error: true);
    }
    state = SessionState.closed;
    prompt = null;
    _id = null;
    if (status.isEmpty || status == t('Verbindung wird aufgebaut …', 'Connecting …')) {
      status = t('Getrennt', 'Disconnected');
    }
    notifyListeners();
  }

  /// Die Verbindung ist abgerissen: Nach einer kurzen Pause wird sie neu aufgebaut, mit wachsendem Abstand.
  void _connectionLost() {
    final id = _id;
    // Ereignisse der alten Verbindung zählen nicht mehr.
    _run++;
    _id = null;
    prompt = null;
    if (id != null) sessionClose(sessionId: id).ignore();
    if (_attempts >= reconnectDelays.length) {
      _giveUp();
      return;
    }
    final delay = reconnectDelays[_attempts];
    _attempts++;
    state = SessionState.connecting;
    status = t('Verbindung unterbrochen, neuer Versuch in $delay s', 'Connection interrupted, retrying in $delay s');
    _retry?.cancel();
    _retry = Timer(Duration(seconds: delay), () {
      _retry = null;
      if (_disposed || state != SessionState.connecting) return;
      _start();
      notifyListeners();
    });
    notifyListeners();
  }

  void _giveUp() {
    _retry?.cancel();
    _retry = null;
    _attempts = 0;
    _gaveUp = !_declined;
    _declined = false;
    terminal.write(_resetModes);
    _note(t('Verbindung getrennt', 'Connection lost'), error: true);
    state = SessionState.closed;
    status = t('Getrennt', 'Disconnected');
    notifyListeners();
  }

  /// Nach einem Netzwechsel oder wenn die App wieder sichtbar wird: Eine stehende Verbindung wird geprüft und
  /// bei Bedarf neu aufgebaut, ein wartender Versuch startet sofort, und eine nach einem Abbruch aufgegebene
  /// Sitzung versucht es noch einmal.
  Future<void> checkConnection() async {
    if (_disposed) return;
    if (_retry != null) {
      _attempts = 1;
      _start();
      notifyListeners();
      return;
    }
    if (_gaveUp && state == SessionState.closed) {
      _attempts = 1;
      _start();
      notifyListeners();
      return;
    }
    final id = _id;
    if (id == null || state != SessionState.connected || _checking) return;
    _checking = true;
    final run = _run;
    try {
      final alive = await sessionCheck(sessionId: id, timeoutMs: 4000);
      if (!alive && run == _run && !_disposed && state == SessionState.connected) _connectionLost();
    } catch (_) {
      // Eine Prüfung, die selbst scheitert, ändert nichts.
    } finally {
      _checking = false;
    }
  }

  /// Schreibt einen Hinweis der App (nicht des Servers) ins Terminal.
  void _note(String text, {bool error = false}) {
    final color = error ? '\x1b[31m' : '\x1b[90m';
    terminal.write('\r\n$color${text.replaceAll('\n', '\r\n')}\x1b[0m\r\n');
  }

  void _send(String data) {
    final id = _id;
    if (id == null || state != SessionState.connected) return;
    var bytes = utf8.encode(data);
    if (!_keyPress && (_ctrl.active || _alt.active)) {
      bytes = applyModifiers(bytes, ctrl: _ctrl.active, alt: _alt.active);
      _releaseModifiers();
    }
    // Eingaben nacheinander senden, damit ihre Reihenfolge erhalten bleibt.
    _writes = _writes.then((_) => sessionWrite(sessionId: id, data: bytes)).catchError((Object _) {});
  }

  void _resize(int cols, int rows) {
    final id = _id;
    if (id == null || state != SessionState.connected) return;
    sessionResize(sessionId: id, cols: cols, rows: rows).ignore();
  }

  /// Beantwortet die Frage nach dem Hostschlüssel.
  void answerHostKey(bool accept) {
    final request = prompt;
    if (request is! SessionEvent_HostKey) return;
    answerDecision(requestId: request.requestId, accept: accept);
    if (!accept) _stopRetrying();
    _clearPrompt();
  }

  /// Beantwortet die Passwortfrage; `null` bricht ab.
  void answerPassword(String? password) {
    final request = prompt;
    if (request is! SessionEvent_Password) return;
    answerText(requestId: request.requestId, text: password);
    if (password == null) _stopRetrying();
    _clearPrompt();
  }

  /// Beantwortet die Fragen des Servers; `null` bricht ab.
  void answerServerPrompts(List<String>? answers) {
    final request = prompt;
    if (request is! SessionEvent_ServerPrompts) return;
    answerTexts(requestId: request.requestId, texts: answers);
    if (answers == null) _stopRetrying();
    _clearPrompt();
  }

  /// Bricht der Nutzer eine Rückfrage beim Wiederverbinden ab, will er nicht, dass es weiter versucht wird.
  void _stopRetrying() {
    if (_attempts == 0) return;
    _attempts = reconnectDelays.length;
    _declined = true;
  }

  void _clearPrompt() {
    if (_disposed) return;
    prompt = null;
    notifyListeners();
  }

  /// Baut die Verbindung nach dem Ende einer Sitzung neu auf; wartet gerade ein neuer Versuch, beginnt er sofort.
  void reconnect() {
    if (_retry != null) {
      _start();
      notifyListeners();
      return;
    }
    if (state != SessionState.closed) return;
    _attempts = 0;
    _start();
    notifyListeners();
  }

  /// Beendet die Sitzung endgültig.
  void close() {
    if (_disposed) return;
    _disposed = true;
    _retry?.cancel();
    _retry = null;
    final id = _id;
    if (id != null) sessionClose(sessionId: id).ignore();
    _files?.dispose();
    dispose();
  }
}

class _TerminalSink implements Sink<String> {
  _TerminalSink(this.terminal);

  final Terminal terminal;

  @override
  void add(String data) => terminal.write(data);

  @override
  void close() {}
}
