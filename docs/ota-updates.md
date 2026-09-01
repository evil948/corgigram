# OTA-обновления korki

Приложение при каждом запуске (release-сборка) проверяет GitHub Releases и предлагает установить новую версию.

## Для пользователей

1. **Первый раз** установите сборку из GitHub Releases:
   - **Linux** — `korki_*_amd64.AppImage` (или legacy `Corgigram_*`)
   - **Windows** — `korki_*_x64-setup.exe` (или legacy `Corgigram_*`)
2. Дальше обновления ставятся **из самого приложения** (диалог «Установить» → перезапуск).
3. Сырой `corgigram-desktop.exe` из zip **не поддерживает** автообновление — нужен NSIS/AppImage.

## Для разработчика: выпуск новой версии

1. Поднимите версию в корневом `Cargo.toml` (`[workspace.package] version`).
2. Закоммитьте и запушьте в `master` — workflow **Release** соберёт артефакты и `latest.json`.
3. Или запустите workflow вручную: Actions → Release → Run workflow.

## Однократная настройка подписи (GitHub Secrets)

Ключи уже сгенерированы в `.tauri/` (приватный ключ **не** в git).

Добавьте в GitHub → Settings → Secrets → Actions:

| Secret | Значение |
|--------|----------|
| `TAURI_SIGNING_PRIVATE_KEY` | **одна строка** из файла `.tauri/updater.key` (скопируйте целиком, без переносов) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | пустая строка (ключ создан без пароля) |

Установка через CLI:

```bash
gh secret set TAURI_SIGNING_PRIVATE_KEY < .tauri/updater.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --body ""
```

Пересоздать ключи:

```bash
CI=true cargo tauri signer generate -w .tauri/updater.key -f --password ""
```

Публичный ключ вставьте в `apps/desktop/tauri.conf.json` → `plugins.updater.pubkey` (файл `.tauri/updater.key.pub`).

## Локальная release-сборка

**Arch / CachyOS:** linuxdeploy падает на `failed to run linuxdeploy` — нужен `NO_STRIP=1`:

```bash
./scripts/build-appimage.sh
```

Или вручную:

```bash
export NO_STRIP=1
export TAURI_SIGNING_PRIVATE_KEY="$(cat .tauri/updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
cd apps/desktop && cargo tauri build
```

Артефакты: `target/release/bundle/appimage/` и `target/release/bundle/nsis/`.

## Серый/пустой экран в AppImage (Arch / CachyOS / Wayland)

AppImage из CI упаковывает старый `libwayland-client` — на современном Mesa WebKit не рисует UI.

**Быстрый обход для уже скачанного AppImage:**

```bash
LD_PRELOAD=/usr/lib/libwayland-client.so ./korki_*.AppImage
```

**Правильно:** после `cargo tauri build` скрипт `./scripts/build-appimage.sh` автоматически вызывает `fix-appimage-wayland.sh`. Вручную:

```bash
./scripts/fix-appimage-wayland.sh target/release/bundle/appimage/korki_*.AppImage
```

## Endpoint обновлений

`https://github.com/evil948/corgigram/releases/latest/download/latest.json`

Формируется автоматически при публикации Release через `tauri-action`.
