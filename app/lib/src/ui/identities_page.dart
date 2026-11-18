import 'package:material_ui/material_ui.dart';

import '../rust/api/app.dart';
import '../text.dart';
import 'common.dart';

String _methodName(String method) => switch (method) {
  'password' => t('Passwort', 'Password'),
  'key' => t('Schlüssel', 'Key'),
  'agent' => 'SSH-Agent',
  _ => t('Bei jeder Verbindung fragen', 'Ask on every connection'),
};

/// Identitäten: Benutzername samt Anmeldeverfahren.
class IdentitiesPage extends StatefulWidget {
  const IdentitiesPage({super.key});

  @override
  State<IdentitiesPage> createState() => _IdentitiesPageState();
}

class _IdentitiesPageState extends State<IdentitiesPage> {
  List<IdentityView> _identities = [];
  List<KeyView> _keys = [];
  String? _error;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  void _reload() {
    try {
      _identities = listIdentities();
      _keys = listKeys();
      _error = null;
    } catch (error) {
      _error = errorText(error);
    }
  }

  Future<void> _edit(IdentityView? identity) async {
    final saved = await showDialog<bool>(
      context: context,
      builder: (_) => IdentityEditor(identity: identity, keys: _keys),
    );
    if (saved == true && mounted) setState(_reload);
  }

  Future<void> _delete(IdentityView identity) async {
    final ok = await confirm(
      context,
      title: t('Identität löschen?', 'Delete identity?'),
      body: t(
        '„${identity.label}“ wird endgültig gelöscht. Hosts, die sie verwenden, haben danach keine Anmeldung mehr.',
        '"${identity.label}" will be deleted permanently. Hosts using it will be left without credentials.',
      ),
      action: t('Löschen', 'Delete'),
      destructive: true,
    );
    if (!ok || !mounted) return;
    attempt(context, () => deleteItem(id: identity.id));
    setState(_reload);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SectionPage(
      title: t('Identitäten', 'Identities'),
      actions: [
        FilledButton.icon(
          icon: const Icon(Icons.add_rounded),
          label: Text(t('Identität hinzufügen', 'Add identity')),
          onPressed: () => _edit(null),
        ),
      ],
      child: _error != null
          ? EmptyState(icon: Icons.error_outline_rounded, title: t('Fehler', 'Error'), hint: _error!)
          : _identities.isEmpty
          ? EmptyState(
              icon: Icons.badge_outlined,
              title: t('Noch keine Identitäten', 'No identities yet'),
              hint: t(
                'Eine Identität ist ein Benutzername mit Passwort oder Schlüssel. Du kannst sie mehreren Hosts zuweisen.',
                'An identity is a user name with a password or key. You can assign it to several hosts.',
              ),
            )
          : ListView.separated(
              padding: const EdgeInsets.only(bottom: 20),
              itemCount: _identities.length,
              separatorBuilder: (_, _) => const SizedBox(height: 8),
              itemBuilder: (context, index) {
                final identity = _identities[index];
                final key = _keys.where((key) => key.id == identity.keyId).firstOrNull;
                final method = identity.method == 'key' && key != null
                    ? '${_methodName('key')}: ${key.label}'
                    : _methodName(identity.method);
                // Die Anmeldung über den Agenten lässt sich hier nicht bearbeiten.
                final editable = identity.method != 'agent';
                return Card(
                  child: ListTile(
                    leading: Icon(
                      identity.method == 'key' ? Icons.key_rounded : Icons.person_outline_rounded,
                      color: theme.colorScheme.primary,
                    ),
                    title: Text(identity.label),
                    subtitle: Text(identity.label == identity.username ? method : '${identity.username} · $method'),
                    onTap: editable ? () => _edit(identity) : null,
                    trailing: IconButton(
                      tooltip: t('Löschen', 'Delete'),
                      icon: const Icon(Icons.delete_outline_rounded),
                      onPressed: () => _delete(identity),
                    ),
                  ),
                );
              },
            ),
    );
  }
}

class IdentityEditor extends StatefulWidget {
  const IdentityEditor({super.key, this.identity, required this.keys});

  final IdentityView? identity;
  final List<KeyView> keys;

  @override
  State<IdentityEditor> createState() => _IdentityEditorState();
}

class _IdentityEditorState extends State<IdentityEditor> {
  late final _label = TextEditingController(
    text: widget.identity == null || widget.identity!.label == widget.identity!.username ? '' : widget.identity!.label,
  );
  late final _username = TextEditingController(text: widget.identity?.username ?? '');
  final _password = TextEditingController();
  late String _method = widget.identity?.method ?? 'password';
  late String? _keyId = widget.keys.any((key) => key.id == widget.identity?.keyId) ? widget.identity?.keyId : null;
  String? _error;

  @override
  void dispose() {
    _label.dispose();
    _username.dispose();
    _password.dispose();
    super.dispose();
  }

  void _save() {
    try {
      saveIdentity(
        input: IdentityInput(
          id: widget.identity?.id,
          label: _label.text,
          username: _username.text,
          method: _method,
          password: _method == 'password' ? _password.text : null,
          keyId: _method == 'key' ? _keyId : null,
        ),
      );
      _password.clear();
      Navigator.pop(context, true);
    } catch (error) {
      setState(() => _error = errorText(error));
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final hadPassword = widget.identity?.method == 'password';
    return FormDialog(
      title: widget.identity == null
          ? t('Identität hinzufügen', 'Add identity')
          : t('Identität bearbeiten', 'Edit identity'),
      action: t('Speichern', 'Save'),
      onSubmit: _save,
      children: [
        TextField(
          controller: _username,
          autofocus: widget.identity == null,
          autocorrect: false,
          decoration: InputDecoration(labelText: t('Benutzername', 'User name')),
        ),
        TextField(
          controller: _label,
          decoration: InputDecoration(
            labelText: t('Bezeichnung (optional)', 'Label (optional)'),
            hintText: t('z. B. „root, Produktion“', 'e.g. "root, production"'),
          ),
        ),
        SegmentedButton<String>(
          segments: [
            ButtonSegment(value: 'password', label: Text(t('Passwort', 'Password'))),
            ButtonSegment(value: 'key', label: Text(t('Schlüssel', 'Key'))),
            ButtonSegment(value: 'ask', label: Text(t('Fragen', 'Ask'))),
          ],
          selected: {_method},
          showSelectedIcon: false,
          onSelectionChanged: (selection) => setState(() => _method = selection.first),
        ),
        if (_method == 'password')
          SecretField(
            controller: _password,
            label: t('Passwort', 'Password'),
            helper: hadPassword
                ? t(
                    'Leer lassen, um das gespeicherte Passwort zu behalten.',
                    'Leave empty to keep the stored password.',
                  )
                : null,
          ),
        if (_method == 'key')
          DropdownButtonFormField<String?>(
            initialValue: _keyId,
            isExpanded: true,
            decoration: InputDecoration(
              labelText: t('Schlüssel', 'Key'),
              helperText: widget.keys.isEmpty
                  ? t(
                      'Schlüssel erzeugst oder importierst du im Bereich „Schlüssel“.',
                      'Generate or import keys in the "Keys" section.',
                    )
                  : null,
            ),
            items: [
              for (final key in widget.keys)
                DropdownMenuItem(
                  value: key.id,
                  child: Text('${key.label} (${key.algorithm})', overflow: TextOverflow.ellipsis),
                ),
            ],
            onChanged: (value) => setState(() => _keyId = value),
          ),
        if (_method == 'ask')
          Text(
            t(
              'Das Passwort wird bei jeder Verbindung abgefragt und nicht gespeichert.',
              'The password is requested on every connection and never stored.',
            ),
            style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant),
          ),
        if (_error != null) Text(_error!, style: TextStyle(color: theme.colorScheme.error)),
      ],
    );
  }
}
