import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:xterm/xterm.dart';

import 'files.dart';
import 'keys.dart';
import 'rust/api/app.dart';
import 'text.dart';

enum SessionState { connecting, connected, closed }

/// Eine Terminalsitzung zu einem Host. Sie lebt unabhängig davon, ob ihr
/// Terminal gerade angezeigt wird; die Ausgabe sammelt sich im Puffer.
class TerminalSession extends ChangeNotifier {
  TerminalSession(this.host) {
    terminal.onOutput = _send;
    terminal.onResize = (cols, rows, _, _) => _resize(cols, rows);
    _start();
  }

  final HostView host;
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
    _id = null;
    prompt = null;
    _explained = false;
    state = SessionState.connecting;
    status = t('Verbindung wird aufgebaut …', 'Connecting …');
    _writes = Future.value();
    _decoder = const Utf8Decoder(allowMalformed: true).startChunkedConversion(_TerminalSink(terminal));
    try {
      openSession(hostId: host.id, cols: terminal.viewWidth, rows: terminal.viewHeight).listen(
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
      case SessionEvent_Failed(:final message):
        _onFailure(run, message);
        return;
      case SessionEvent_Closed():
        _onClosed(run);
        return;
    }
    notifyListeners();
  }

  void _onFailure(int run, String message) {
    if (run != _run || _disposed) return;
    final text = clean(message).trim();
    status = text.split('\n').first;
    _note(text, error: true);
    _explained = true;
    notifyListeners();
  }

  void _onClosed(int run) {
    if (run != _run || _disposed || state == SessionState.closed) return;
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
    _clearPrompt();
  }

  /// Beantwortet die Passwortfrage; `null` bricht ab.
  void answerPassword(String? password) {
    final request = prompt;
    if (request is! SessionEvent_Password) return;
    answerText(requestId: request.requestId, text: password);
    _clearPrompt();
  }

  /// Beantwortet die Fragen des Servers; `null` bricht ab.
  void answerServerPrompts(List<String>? answers) {
    final request = prompt;
    if (request is! SessionEvent_ServerPrompts) return;
    answerTexts(requestId: request.requestId, texts: answers);
    _clearPrompt();
  }

  void _clearPrompt() {
    if (_disposed) return;
    prompt = null;
    notifyListeners();
  }

  /// Baut die Verbindung nach dem Ende einer Sitzung neu auf.
  void reconnect() {
    if (state != SessionState.closed) return;
    _start();
    notifyListeners();
  }

  /// Beendet die Sitzung endgültig.
  void close() {
    if (_disposed) return;
    _disposed = true;
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
