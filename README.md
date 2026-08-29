# Corgigram

E2E-мессенджер для 2–5 человек. Дизайн — **Telegram × Signal**: двухколоночный layout как в Telegram, тёмная палитра и privacy-индикаторы как в Signal.

## Release (Linux + Windows)

**Сборка:**

```bash
# Linux
sudo pacman -S webkit2gtk-4.1 gtk3 libappindicator-gtk3 base-devel
./scripts/build-release-linux.sh
```

```powershell
# Windows (на Windows-машине)
.\scripts\build-release-windows.ps1
```

**Тест Linux ↔ Windows:** [`docs/release-test.md`](docs/release-test.md)

Артефакты: `dist/corgigram-0.1.0-{linux,windows}-x86_64/`

## Build (dev)

```bash
cargo build --release
cargo test
```

### Desktop (Tauri) — зависимости Linux

```bash
# Arch / CachyOS
sudo pacman -S webkit2gtk-4.1 gtk3 libappindicator-gtk3 base-devel

cargo build -p corgigram-desktop --release
./target/release/corgigram-desktop
```

## CLI quick test

```bash
./target/release/corgigram demo --message "Привет!"
```

Двухпроцессный тест (два терминала) — см. команды `ping` / `pong` в `--help`.

## Desktop UI

| Элемент | Откуда |
|---------|--------|
| Sidebar + список чатов | Telegram |
| Пузыри сообщений | Telegram |
| Тёмная тема, E2E badge, safety number | Signal |
| QR для pairing | Signal-style |

**Первый запуск:** создайте профиль → «Мой QR» / добавьте контакт по bundle JSON → «Подключиться» → пишите сообщения.

**С Firebase (по умолчанию):** автоматический обмен SDP + offline mailbox — настраивать ничего не нужно.

**Первый раз (один человек в команде):** поднимите Firebase-проект — см. [`docs/firebase-setup.md`](docs/firebase-setup.md) или `./scripts/setup-firebase.sh`.

**Свой Firebase:** Настройки → другой Database URL (+ опционально auth token).

### Firebase setup (только для своего сервера)

По умолчанию используется общий URL:

`https://corgigram-shared-default-rtdb.europe-west1.firebasedatabase.app`

Если поднимаете свой Firebase:

1. Создайте проект в [Firebase Console](https://console.firebase.google.com/) → Realtime Database.
2. Скопируйте Database URL.
3. В Rules вставьте содержимое [`docs/firebase-rules.json`](docs/firebase-rules.json).
4. В приложении: **Настройки** → свой URL.

**Что хранится на Firebase:** SDP для signaling, зашифрованные blob'ы mailbox. Сервер не видит plaintext.

**Offline:** если WebRTC недоступен, сообщение уходит в Firebase mailbox (E2E) и локальный outbox; доставится при следующем подключении.

Данные: `~/.local/share/corgigram/` (identity, SQLite, config).

## Структура

```
crates/crypto/      # E2E шифрование + mailbox
crates/storage/     # SQLite (messages, outbox)
crates/core/        # логика приложения + Firebase
crates/transport/   # WebRTC
crates/cli/         # CLI
apps/desktop/       # Tauri UI
apps/mobile/        # Flutter + flutter_rust_bridge
docs/               # firebase-rules.json, mobile-build.md
scripts/            # setup-firebase.sh, setup-mobile.sh
```

## Mobile (Phase 3)

Сборка и codegen — [`docs/mobile-build.md`](docs/mobile-build.md). Тестирование на устройствах — вместе с финальными Windows/Linux релизами.

## Security

- `identity.json` — **приватные ключи**, не делитесь
- `*.bundle.json` — публичные ключи для pairing
- Safety number — сверяйте с собеседником (как Signal)
- Firebase rules в репозитории — только для dev; ограничьте доступ перед реальным использованием

## Release

См. [Release (Linux + Windows)](#release-linux--windows) и [`docs/release-test.md`](docs/release-test.md).
