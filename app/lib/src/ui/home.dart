import 'package:material_ui/material_ui.dart';

import '../device_unlock.dart';
import '../rust/api/app.dart';
import '../settings.dart';
import '../state.dart';
import '../update.dart';
import '../text.dart';
import 'hosts_page.dart';
import 'identities_page.dart';
import 'keys_page.dart';
import 'common.dart';
import 'device_unlock_ui.dart';
import 'logo.dart';
import 'mcp_ui.dart';
import 'sync_ui.dart';
import 'terminal_page.dart';
import 'update_ui.dart';
import 'vpn_page.dart';

/// Die App nach dem Entsperren: Navigation und die einzelnen Bereiche.
class HomeScreen extends StatefulWidget {
  const HomeScreen({super.key, required this.state, required this.onLock});

  final AppState state;
  final Future<void> Function() onLock;

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _Section {
  const _Section(this.icon, this.selectedIcon, this.label);

  final IconData icon;
  final IconData selectedIcon;
  final String label;
}

class _HomeScreenState extends State<HomeScreen> {
  static const _terminal = 1;
  int _index = 0;

  late final _sections = [
    _Section(Icons.dns_outlined, Icons.dns_rounded, 'Hosts'),
    _Section(Icons.terminal_outlined, Icons.terminal_rounded, 'Terminal'),
    _Section(Icons.badge_outlined, Icons.badge_rounded, t('Identitäten', 'Identities')),
    _Section(Icons.key_outlined, Icons.key_rounded, t('Schlüssel', 'Keys')),
    _Section(Icons.vpn_lock_outlined, Icons.vpn_lock_rounded, 'VPN'),
    // Auch auf dem Telefon: Dort verwaltet man die Freigaben fernverwalteter Geräte.
    _Section(Icons.smart_toy_outlined, Icons.smart_toy_rounded, 'MCP'),
  ];

  @override
  void initState() {
    super.initState();
    availableUpdate.addListener(_announceUpdate);
    checkForUpdateQuietly();
    if (deviceUnlock.upgradePending) WidgetsBinding.instance.addPostFrameCallback((_) => _upgradeDeviceUnlock());
  }

  /// Eine Ablage aus Version 0.8.0 ohne Fingerabdruck wird nach dem Entsperren auf den Fingerabdruck umgestellt.
  Future<void> _upgradeDeviceUnlock() async {
    final message = await deviceUnlock.upgrade();
    if (message != null && mounted) showMessage(context, message.text, error: message.error);
  }

  @override
  void dispose() {
    availableUpdate.removeListener(_announceUpdate);
    super.dispose();
  }

  /// Weist einmal auf eine neu gefundene Fassung hin; danach erinnert der Punkt am Info-Symbol.
  void _announceUpdate() {
    final update = availableUpdate.value;
    if (update == null || !mounted) return;
    setState(() {});
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(t('Tephra ${update.version} ist verfügbar.', 'Tephra ${update.version} is available.')),
        behavior: SnackBarBehavior.floating,
        width: 520,
        duration: const Duration(seconds: 10),
        action: SnackBarAction(label: t('Anzeigen', 'Show'), onPressed: () => showUpdateDialog(context, update)),
      ),
    );
  }

  Widget _aboutButton() {
    return IconButton(
      tooltip: t('Über $appName', 'About $appName'),
      icon: Badge(isLabelVisible: availableUpdate.value != null, child: const Icon(Icons.info_outline_rounded)),
      onPressed: _about,
    );
  }

  void _connect(HostView host) {
    widget.state.open(host);
    setState(() => _index = _terminal);
  }

  Widget _icon(int index, {required bool selected}) {
    final section = _sections[index];
    final icon = Icon(selected ? section.selectedIcon : section.icon);
    if (index != _terminal) return icon;
    final sessions = widget.state.sessions.length;
    final waiting = widget.state.waiting;
    return Badge(
      isLabelVisible: sessions > 0,
      label: Text(waiting > 0 ? '!' : '$sessions'),
      backgroundColor: waiting > 0 ? Theme.of(context).colorScheme.error : Theme.of(context).colorScheme.primary,
      textColor: waiting > 0 ? Theme.of(context).colorScheme.onError : Theme.of(context).colorScheme.onPrimary,
      child: icon,
    );
  }

  void _about() {
    showAboutDialog(
      context: context,
      applicationName: appName,
      applicationVersion: appVersion,
      applicationIcon: const TephraLogo(size: 44),
      applicationLegalese: [
        'Copyright © 2026 Pold',
        t('Veröffentlicht unter der MIT-Lizenz.', 'Released under the MIT license.'),
      ].join('\n'),
      children: [
        const SizedBox(height: 16),
        Text(
          t(
            'SSH-Zugänge, WireGuard-Tunnel und Terminal in einer App. Alle Daten liegen verschlüsselt in deinem Tresor.',
            'SSH credentials, WireGuard tunnels and a terminal in one app. All data is stored encrypted in your vault.',
          ),
        ),
        const UpdateSection(),
        if (deviceUnlock.supported) const DeviceUnlockSection(),
      ],
    );
  }

  /// Auswahl zwischen dunklem und hellem Design.
  Widget _themeButton() {
    return ListenableBuilder(
      listenable: settings,
      builder: (context, _) => PopupMenuButton<ThemeMode>(
        tooltip: t('Design', 'Theme'),
        icon: Icon(switch (settings.themeMode) {
          ThemeMode.dark => Icons.dark_mode_outlined,
          ThemeMode.light => Icons.light_mode_outlined,
          ThemeMode.system => Icons.brightness_auto_outlined,
        }),
        initialValue: settings.themeMode,
        onSelected: settings.setThemeMode,
        itemBuilder: (_) => [
          PopupMenuItem(value: ThemeMode.dark, child: Text(t('Dunkel', 'Dark'))),
          PopupMenuItem(value: ThemeMode.light, child: Text(t('Hell', 'Light'))),
          PopupMenuItem(value: ThemeMode.system, child: Text(t('Wie das System', 'Follow the system'))),
        ],
      ),
    );
  }

  Widget _body() {
    return switch (_index) {
      0 => HostsPage(onConnect: _connect),
      1 => TerminalPage(state: widget.state, onShowHosts: () => setState(() => _index = 0)),
      2 => const IdentitiesPage(),
      3 => const KeysPage(),
      4 => const VpnPage(),
      _ => const McpPage(),
    };
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.state,
      builder: (context, _) => LayoutBuilder(
        builder: (context, constraints) => constraints.maxWidth >= 720 ? _wide(context) : _narrow(context),
      ),
    );
  }

  Widget _wide(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Scaffold(
      body: SafeArea(
        child: Row(
          children: [
            NavigationRail(
              selectedIndex: _index,
              onDestinationSelected: (index) => setState(() => _index = index),
              labelType: NavigationRailLabelType.all,
              backgroundColor: scheme.surfaceContainer,
              leading: Padding(padding: const EdgeInsets.only(top: 8, bottom: 12), child: const TephraLogo(size: 34)),
              trailing: Expanded(
                child: Align(
                  alignment: Alignment.bottomCenter,
                  child: Padding(
                    padding: const EdgeInsets.only(bottom: 12),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        const SyncButton(),
                        _themeButton(),
                        _aboutButton(),
                        IconButton(
                          tooltip: t('Tresor sperren', 'Lock vault'),
                          icon: const Icon(Icons.lock_outline_rounded),
                          onPressed: widget.onLock,
                        ),
                      ],
                    ),
                  ),
                ),
              ),
              destinations: [
                for (final (index, section) in _sections.indexed)
                  NavigationRailDestination(
                    icon: _icon(index, selected: false),
                    selectedIcon: _icon(index, selected: true),
                    label: Text(section.label),
                  ),
              ],
            ),
            Expanded(child: _body()),
          ],
        ),
      ),
    );
  }

  Widget _narrow(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text(appName),
        actions: [
          const SyncButton(),
          _themeButton(),
          _aboutButton(),
          IconButton(
            tooltip: t('Tresor sperren', 'Lock vault'),
            icon: const Icon(Icons.lock_outline_rounded),
            onPressed: widget.onLock,
          ),
        ],
      ),
      body: _body(),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _index,
        // Sechs Beschriftungen passen auf ein schmales Telefon nicht nebeneinander.
        labelBehavior: _sections.length > 5
            ? NavigationDestinationLabelBehavior.onlyShowSelected
            : NavigationDestinationLabelBehavior.alwaysShow,
        onDestinationSelected: (index) => setState(() => _index = index),
        destinations: [
          for (final (index, section) in _sections.indexed)
            NavigationDestination(
              icon: _icon(index, selected: false),
              selectedIcon: _icon(index, selected: true),
              label: section.label,
            ),
        ],
      ),
    );
  }
}
