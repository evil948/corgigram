# Mobile build (Flutter + Rust)

Phase 3 — Android/iOS клиент с тем же Rust core, что desktop.

## Зависимости

- Flutter SDK (stable)
- Android Studio / Xcode (для финальных сборок)
- Rust toolchain

```bash
# Arch — опционально
sudo pacman -S flutter android-tools
```

Или локальный SDK (уже в репозитории при разработке):

```bash
git clone https://github.com/flutter/flutter.git -b stable .flutter-sdk
export PATH="$PWD/.flutter-sdk/bin:$PATH"
```

## Быстрый старт

```bash
chmod +x scripts/setup-mobile.sh
./scripts/setup-mobile.sh
cd apps/mobile
flutter run -d linux   # или android / ios simulator
```

## Архитектура

```
apps/mobile/
  lib/main.dart          # UI (Telegram × Signal)
  lib/src/rust/          # сгенерировано flutter_rust_bridge
  rust/                  # corgigram-mobile crate → corgigram-core
```

## Push (FCM)

Payload **без текста сообщения**:

```json
{ "type": "new_message", "sender_id": "alice" }
```

Обработчик: `lib/services/push_handler.dart` — при push вызывает `syncMailbox` + `pollIncoming`.

Подключение `firebase_messaging` — на финальном релизе вместе с Windows/Linux desktop builds.

## Firebase

Используется тот же встроенный URL, что и desktop. Переопределение — в настройках приложения.

## Codegen после изменений Rust API

```bash
cd apps/mobile
flutter_rust_bridge_codegen generate
```
