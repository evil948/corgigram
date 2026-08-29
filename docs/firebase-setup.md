# Firebase setup для Corgigram

Corgigram по умолчанию использует общий Realtime Database:

```
https://corgigram-shared-default-rtdb.europe-west1.firebasedatabase.app
```

На сервере хранится только **шифротекст** (signaling SDP + offline mailbox) и **публичные pre-key bundle** (для добавления контактов по ID). Секретные ключи остаются на устройствах.

Структура Firebase:
- `signaling/{userId}/offer|answer` — WebRTC SDP
- `mailboxes/{userId}/{msgId}` — offline ciphertext
- `directory/{userId}` — публичный bundle для поиска по ID
- `avatars/{ownerId}/{viewerId}` — **зашифрованный** аватар (E2E, только для viewer)
- `avatar_wants/{ownerId}/{viewerId}` — запрос на публикацию аватара

## Быстрый способ (скрипт)

```bash
chmod +x scripts/setup-firebase.sh
./scripts/setup-firebase.sh
```

Скрипт:
1. Откроет браузер для входа в Google (`firebase login`)
2. Создаст проект `corgigram-shared`
3. Создаст Realtime Database в `europe-west1`
4. Загрузит rules из `docs/firebase-rules.json`

## Ручной способ (Firebase Console)

1. Откройте [Firebase Console](https://console.firebase.google.com/)
2. **Add project** → ID: `corgigram-shared` → создать
3. **Build → Realtime Database → Create Database**
   - Location: `europe-west1 (Belgium)`
   - Start in **test mode** (rules заменим на следующем шаге)
4. **Rules** → вставьте содержимое [`firebase-rules.json`](firebase-rules.json) → **Publish**
5. Убедитесь, что Database URL совпадает с встроенным в приложении

## Deploy rules через CLI

```bash
npx firebase-tools login
npx firebase-tools deploy --only database --project corgigram-shared
```

## Свой Firebase

Если не хотите использовать общий проект:

1. Создайте свой проект и Realtime Database
2. Загрузите те же rules
3. В Corgigram: **Настройки → Firebase Database URL** → ваш URL

## Проверка

```bash
curl "https://corgigram-shared-default-rtdb.europe-west1.firebasedatabase.app/.json"
```

Ожидается `null` или JSON — не `"404 Not Found"`.

## Безопасность

Rules в репозитории открытые (read/write для всех) — подходит для прототипа с 2–5 друзьями. Для публичного использования добавьте Firebase Auth и ограничьте доступ по `auth.uid`.
