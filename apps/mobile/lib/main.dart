import 'dart:async';

import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';

import 'src/rust/api.dart';
import 'src/rust/dto.dart';
import 'src/rust/frb_generated.dart';
import 'theme/app_theme.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  final dir = await getApplicationDocumentsDirectory();
  corgigramInit(dataDir: '${dir.path}/corgigram');
  runApp(const CorgigramApp());
}

class CorgigramApp extends StatelessWidget {
  const CorgigramApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'korki',
      theme: AppTheme.dark(),
      home: const RootScreen(),
      debugShowCheckedModeBanner: false,
    );
  }
}

class RootScreen extends StatefulWidget {
  const RootScreen({super.key});

  @override
  State<RootScreen> createState() => _RootScreenState();
}

class _RootScreenState extends State<RootScreen> {
  SnapshotDto? snapshot;
  String? activeContactId;
  List<MessageDto> messages = [];
  Timer? _pollTimer;
  bool connecting = false;

  @override
  void initState() {
    super.initState();
    _refresh();
    _pollTimer = Timer.periodic(const Duration(milliseconds: 900), (_) async {
      final incoming = await pollIncoming();
      if (!mounted) return;
      if (incoming.isNotEmpty) {
        if (activeContactId != null) {
          await _loadMessages(activeContactId!);
        }
        await _refresh(silent: true);
      }
    });
  }

  @override
  void dispose() {
    _pollTimer?.cancel();
    super.dispose();
  }

  Future<void> _refresh({bool silent = false}) async {
    final s = await getSnapshot();
    if (!mounted) return;
    setState(() => snapshot = s);
  }

  Future<void> _loadMessages(String contactId) async {
    final msgs = await getMessages(contactId: contactId);
    if (!mounted) return;
    setState(() => messages = msgs);
  }

  Future<void> _selectContact(ContactDto c) async {
    setState(() => activeContactId = c.userId);
    await _loadMessages(c.userId);
    final incoming = await syncMailbox(contactId: c.userId);
    if (incoming.isNotEmpty) await _loadMessages(c.userId);
  }

  @override
  Widget build(BuildContext context) {
    if (snapshot == null) {
      return const Scaffold(
        body: Center(child: CircularProgressIndicator()),
      );
    }
    if (!snapshot!.hasIdentity) {
      return OnboardingScreen(onDone: _refresh);
    }
    final contacts = snapshot!.contacts;
    final active = contacts.cast<ContactDto?>().firstWhere(
          (c) => c!.userId == activeContactId,
          orElse: () => null,
        );

    return Scaffold(
      appBar: AppBar(
        title: Text(active?.displayName ?? 'korki'),
        leading: Builder(
          builder: (ctx) => IconButton(
            icon: const Icon(Icons.menu),
            onPressed: () => Scaffold.of(ctx).openDrawer(),
          ),
        ),
        actions: [
          if (snapshot!.outboxCount > 0)
            Center(
              child: Padding(
                padding: const EdgeInsets.only(right: 8),
                child: Chip(
                  label: Text('${snapshot!.outboxCount} в очереди'),
                  visualDensity: VisualDensity.compact,
                ),
              ),
            ),
          IconButton(
            icon: const Icon(Icons.settings),
            onPressed: () => _openSettings(context),
          ),
        ],
      ),
      drawer: Drawer(
        backgroundColor: AppTheme.sidebar,
        child: SafeArea(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.all(16),
                child: Text(
                  snapshot!.profile?.displayName ?? 'korki',
                  style: const TextStyle(
                    fontSize: 18,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              ListTile(
                leading: const Icon(Icons.qr_code),
                title: const Text('Мой QR'),
                onTap: () => _showQr(context),
              ),
              ListTile(
                leading: const Icon(Icons.person_add),
                title: const Text('Добавить контакт'),
                onTap: () => _addContact(context),
              ),
              const Divider(),
              Expanded(
                child: ListView.builder(
                  itemCount: contacts.length,
                  itemBuilder: (_, i) {
                    final c = contacts[i];
                    final selected = c.userId == activeContactId;
                    return ListTile(
                      selected: selected,
                      leading: CircleAvatar(
                        child: Text(_initials(c.displayName)),
                      ),
                      title: Text(c.displayName),
                      subtitle: Text(c.userId, maxLines: 1, overflow: TextOverflow.ellipsis),
                      onTap: () {
                        Navigator.pop(context);
                        _selectContact(c);
                      },
                    );
                  },
                ),
              ),
            ],
          ),
        ),
      ),
      body: active == null
          ? const Center(
              child: Text(
                'Выберите чат в меню',
                style: TextStyle(color: AppTheme.textSecondary),
              ),
            )
          : Column(
              children: [
                _ChatHeader(
                  contact: active,
                  connected: snapshot!.connectedContactId == active.userId,
                  firebaseConfigured: snapshot!.firebaseConfigured,
                  connecting: connecting,
                  onConnect: () => _connect(active.userId),
                  onSafety: () => _showSafety(context, active.userId),
                ),
                Expanded(child: _MessageList(messages: messages)),
                _ComposeBar(
                  onSend: (text) async {
                    await sendMessage(contactId: active.userId, text: text);
                    await _loadMessages(active.userId);
                    await _refresh(silent: true);
                  },
                ),
              ],
            ),
    );
  }

  Future<void> _connect(String contactId) async {
    setState(() => connecting = true);
    try {
      final result = await connectAuto(contactId: contactId);
      if (!result.connected && mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Не удалось подключиться автоматически')),
        );
      }
      await _refresh();
    } finally {
      if (mounted) setState(() => connecting = false);
    }
  }

  Future<void> _showQr(BuildContext context) async {
    final qr = await getBundleQr();
    if (!context.mounted) return;
    showDialog(
      context: context,
      builder: (_) => AlertDialog(
        title: const Text('QR для pairing'),
        content: SingleChildScrollView(
          child: SelectableText(qr, style: const TextStyle(fontSize: 10)),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('OK')),
        ],
      ),
    );
  }

  Future<void> _addContact(BuildContext context) async {
    final controller = TextEditingController();
    await showDialog(
      context: context,
      builder: (_) => AlertDialog(
        title: const Text('Добавить контакт'),
        content: TextField(
          controller: controller,
          maxLines: 8,
          decoration: const InputDecoration(hintText: 'JSON bundle'),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('Отмена')),
          FilledButton(
            onPressed: () async {
              await addContact(bundleJson: controller.text.trim());
              if (context.mounted) Navigator.pop(context);
              await _refresh();
            },
            child: const Text('Добавить'),
          ),
        ],
      ),
    );
  }

  Future<void> _showSafety(BuildContext context, String contactId) async {
    final num = await getSafetyNumber(contactId: contactId);
    if (!context.mounted) return;
    showDialog(
      context: context,
      builder: (_) => AlertDialog(
        title: const Text('Safety number'),
        content: SelectableText(num),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('OK')),
        ],
      ),
    );
  }

  Future<void> _openSettings(BuildContext context) async {
    final urlCtrl = TextEditingController(
      text: snapshot?.firebaseDatabaseUrlOverride ?? '',
    );
    await showDialog(
      context: context,
      builder: (_) => AlertDialog(
        title: const Text('Настройки'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              snapshot!.firebaseUsesDefaultUrl
                  ? 'Firebase: встроенный URL'
                  : 'Firebase: свой URL',
              style: const TextStyle(color: AppTheme.textSecondary, fontSize: 13),
            ),
            const SizedBox(height: 8),
            TextField(
              controller: urlCtrl,
              decoration: InputDecoration(
                labelText: 'Database URL',
                hintText: defaultFirebaseUrl(),
              ),
            ),
          ],
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('Отмена')),
          FilledButton(
            onPressed: () async {
              await saveConfig(
                firebaseDatabaseUrl: urlCtrl.text.trim().isEmpty ? null : urlCtrl.text.trim(),
              );
              if (context.mounted) Navigator.pop(context);
              await _refresh();
            },
            child: const Text('Сохранить'),
          ),
        ],
      ),
    );
  }

  String _initials(String name) {
    return name
        .split(' ')
        .where((w) => w.isNotEmpty)
        .map((w) => w[0])
        .take(2)
        .join()
        .toUpperCase();
  }
}

class OnboardingScreen extends StatefulWidget {
  const OnboardingScreen({super.key, required this.onDone});

  final Future<void> Function() onDone;

  @override
  State<OnboardingScreen> createState() => _OnboardingScreenState();
}

class _OnboardingScreenState extends State<OnboardingScreen> {
  final userId = TextEditingController();
  final name = TextEditingController();

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              const Spacer(),
              const Text(
                'korki',
                textAlign: TextAlign.center,
                style: TextStyle(fontSize: 28, fontWeight: FontWeight.bold, letterSpacing: -0.5),
              ),
              const SizedBox(height: 8),
              const Text(
                'Приватный чат для своих. E2E шифрование.',
                textAlign: TextAlign.center,
                style: TextStyle(color: AppTheme.textSecondary),
              ),
              const SizedBox(height: 32),
              TextField(
                controller: userId,
                decoration: const InputDecoration(labelText: 'ID пользователя'),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: name,
                decoration: const InputDecoration(labelText: 'Имя'),
              ),
              const SizedBox(height: 24),
              FilledButton(
                onPressed: () async {
                  await createIdentity(
                    userId: userId.text.trim(),
                    displayName: name.text.trim(),
                  );
                  await widget.onDone();
                },
                child: const Text('Создать профиль'),
              ),
              const Spacer(flex: 2),
            ],
          ),
        ),
      ),
    );
  }
}

class _ChatHeader extends StatelessWidget {
  const _ChatHeader({
    required this.contact,
    required this.connected,
    required this.firebaseConfigured,
    required this.connecting,
    required this.onConnect,
    required this.onSafety,
  });

  final ContactDto contact;
  final bool connected;
  final bool firebaseConfigured;
  final bool connecting;
  final VoidCallback onConnect;
  final VoidCallback onSafety;

  @override
  Widget build(BuildContext context) {
    final status = connected
        ? 'Защищено E2E · подключено'
        : firebaseConfigured
            ? 'Защищено E2E · offline mailbox'
            : 'Защищено E2E · не подключено';

    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      decoration: const BoxDecoration(
        border: Border(bottom: BorderSide(color: AppTheme.border)),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(contact.displayName, style: const TextStyle(fontWeight: FontWeight.w600)),
                Text(status, style: const TextStyle(color: AppTheme.textSecondary, fontSize: 12)),
              ],
            ),
          ),
          TextButton(
            onPressed: connecting ? null : onConnect,
            child: Text(connecting ? '…' : 'Подключиться'),
          ),
          IconButton(onPressed: onSafety, icon: const Icon(Icons.shield_outlined)),
        ],
      ),
    );
  }
}

class _MessageList extends StatelessWidget {
  const _MessageList({required this.messages});

  final List<MessageDto> messages;

  @override
  Widget build(BuildContext context) {
    return ListView.builder(
      padding: const EdgeInsets.all(12),
      itemCount: messages.length,
      itemBuilder: (_, i) {
        final m = messages[i];
        final out = m.direction == 'out';
        final pending = m.status == 'pending' ||
            m.status == 'queued_firebase' ||
            m.status == 'queued_local';
        return Align(
          alignment: out ? Alignment.centerRight : Alignment.centerLeft,
          child: Container(
            margin: const EdgeInsets.only(bottom: 8),
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            constraints: BoxConstraints(maxWidth: MediaQuery.of(context).size.width * 0.78),
            decoration: BoxDecoration(
              color: out ? AppTheme.bubbleOut : AppTheme.bubbleIn,
              borderRadius: BorderRadius.circular(14),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.end,
              children: [
                Text(m.body),
                const SizedBox(height: 4),
                Text(
                  pending ? '⏳' : '',
                  style: const TextStyle(fontSize: 11, color: AppTheme.textSecondary),
                ),
              ],
            ),
          ),
        );
      },
    );
  }
}

class _ComposeBar extends StatefulWidget {
  const _ComposeBar({required this.onSend});

  final Future<void> Function(String text) onSend;

  @override
  State<_ComposeBar> createState() => _ComposeBarState();
}

class _ComposeBarState extends State<_ComposeBar> {
  final controller = TextEditingController();

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.all(8),
        child: Row(
          children: [
            Expanded(
              child: TextField(
                controller: controller,
                decoration: const InputDecoration(hintText: 'Сообщение…'),
                minLines: 1,
                maxLines: 4,
              ),
            ),
            IconButton(
              icon: const Icon(Icons.send),
              onPressed: () async {
                final text = controller.text.trim();
                if (text.isEmpty) return;
                controller.clear();
                await widget.onSend(text);
              },
            ),
          ],
        ),
      ),
    );
  }
}
