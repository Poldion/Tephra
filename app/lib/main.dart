import 'package:material_ui/material_ui.dart';

import 'src/paths.dart';
import 'src/rust/api/app.dart';
import 'src/rust/frb_generated.dart';
import 'src/settings.dart';
import 'src/state.dart';
import 'src/text.dart';
import 'src/ui/gate.dart';
import 'src/ui/home.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  await initPaths();
  settings.load();
  runApp(const TerminalApp());
}

class TerminalApp extends StatelessWidget {
  const TerminalApp({super.key});

  static const _seed = Color(0xFF2FB8A6);

  ThemeData _theme(Brightness brightness) {
    final scheme = ColorScheme.fromSeed(seedColor: _seed, brightness: brightness);
    return ThemeData(
      colorScheme: scheme,
      useMaterial3: true,
      inputDecorationTheme: const InputDecorationTheme(border: OutlineInputBorder(), isDense: true, helperMaxLines: 3),
      cardTheme: CardThemeData(
        elevation: 0,
        color: scheme.surfaceContainerLow,
        margin: EdgeInsets.zero,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: settings,
      builder: (context, _) => MaterialApp(
        title: appName,
        debugShowCheckedModeBanner: false,
        theme: _theme(Brightness.light),
        darkTheme: _theme(Brightness.dark),
        themeMode: settings.themeMode,
        home: const Root(),
      ),
    );
  }
}

/// Zeigt je nach Zustand des Tresors die Anmeldung oder die eigentliche App.
class Root extends StatefulWidget {
  const Root({super.key});

  @override
  State<Root> createState() => _RootState();
}

class _RootState extends State<Root> {
  final _state = AppState();
  late bool _unlocked = isUnlocked();

  Future<void> _lock() async {
    _state.closeAll();
    try {
      await lockVault();
    } finally {
      if (mounted) setState(() => _unlocked = false);
    }
  }

  @override
  void dispose() {
    _state.closeAll();
    _state.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (!_unlocked) {
      return GateScreen(onUnlocked: () => setState(() => _unlocked = true));
    }
    return HomeScreen(state: _state, onLock: _lock);
  }
}
