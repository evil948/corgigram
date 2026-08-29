# Release test plan — Linux + Windows

Тест после финальных сборок. Firebase уже настроен (`corgigram-shared`).

## Сборка

### Linux (CachyOS / Arch)

```bash
# Зависимости GUI (один раз)
sudo pacman -S webkit2gtk-4.1 gtk3 libappindicator-gtk3 base-devel

# Опционально: Tauri bundler
cargo install tauri-cli --locked

chmod +x scripts/build-release-linux.sh
./scripts/build-release-linux.sh
```

Артефакты: `dist/corgigram-0.1.0-linux-x86_64/`

### Windows

На машине с Windows 10/11:

1. [Rust](https://rustup.rs/)
2. [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) — C++ workload
3. [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/) (обычно уже есть в Win11)

```powershell
cargo install tauri-cli --locked
.\scripts\build-release-windows.ps1
```

Артефакты: `dist\corgigram-0.1.0-windows-x86_64\`

Передайте папку `dist/...` друг другу (USB, zip) — **не** делитесь `identity.json`.

---

## Checklist: первый запуск

| # | Шаг | Linux | Windows |
|---|-----|-------|---------|
| 1 | Запустить `corgigram-desktop` | `./corgigram-desktop` | `corgigram-desktop.exe` |
| 2 | Создать профиль (разные user ID!) | alice / bob | alice / bob |
| 3 | «Мой QR» → скопировать bundle JSON | ✓ | ✓ |
| 4 | Добавить контакт (bundle второго) | ✓ | ✓ |
| 5 | Safety number — сверить | ✓ | ✓ |

Firebase настраивать **не нужно** — URL встроен.

**Rules (один раз):** скопируйте [`docs/firebase-rules.json`](firebase-rules.json) в Firebase Console → Realtime Database → Rules → Publish. Без `.read` на `mailboxes/$userId` входящие сообщения не подтягиваются (отправка работает, получение — нет).

---

## Checklist: live chat (WebRTC)

| # | Действие | Ожидание |
|---|----------|----------|
| 1 | **Bob** открывает чат с Alice | Статус «offline mailbox» или «не подключено» |
| 2 | **Alice** нажимает «Подключиться» | Авто-offer через Firebase |
| 3 | **Bob** (фоновый poll ~1 с) | Авто-answer, статус «подключено» |
| 4 | Alice → сообщение «Привет» | Bob видит в чате, E2E |
| 5 | Bob → ответ | Alice видит |

Если auto-connect не сработал за 30 с — проверьте интернет и Firebase rules.

---

## Checklist: offline mailbox

| # | Действие | Ожидание |
|---|----------|----------|
| 1 | Закрыть desktop на одной стороне | — |
| 2 | Отправить сообщение с другой | ⏳ pending / «в очереди» |
| 3 | Открыть desktop получателя | Сообщение появляется (sync mailbox) |

---

## Checklist: CLI (опционально)

Без GUI — проверка crypto + WebRTC:

```bash
./corgigram demo --message "test"
```

Двухмашинный SDP-тест — `ping` / `pong` в `--help` (ручной обмен SDP-файлами).

---

## Данные на диске

| OS | Путь |
|----|------|
| Linux | `~/.local/share/corgigram/` |
| Windows | `%LOCALAPPDATA%\corgigram\` |

Содержимое: `identity.json` (секрет!), `corgigram.db`, `config.json`.

---

## Известные ограничения (v0.1.0)

- Групповой чат 3–5 — не в этом релизе
- FCM push на mobile — stub, без продакшен FCM
- Firebase rules открытые — только для своей компании 2–5 человек
- NAT: иногда нужен TURN (уже встроен в IceConfig)

---

## Если что-то сломалось

1. `curl "https://corgigram-shared-default-rtdb.europe-west1.firebasedatabase.app/signaling/test/offer.json"` — не должно быть 404 project
2. Оба клиента — один Firebase URL (или дефолт на обоих)
3. Разные `user_id` при создании профиля
4. Safety number совпадает → MITM маловероятен
