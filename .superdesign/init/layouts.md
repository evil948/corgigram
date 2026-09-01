# Layout Components

## App Shell (`#app.app`)
File: `apps/desktop/ui/index.html` + `styles.css`

Two-pane desktop layout:
- Left: `.sidebar` (320px) — search, invitations, contacts, profile footer
- Right: `.chat-pane` — empty state OR active chat

```html
<div id="app" class="app hidden">
  <aside class="sidebar">...</aside>
  <main class="chat-pane">...</main>
</div>
```

## Sidebar
- Header: logo "Corgigram", search input
- Invitations panel (conditional)
- Actions: + Контакт, Мой QR
- Contact list
- Footer: profile card with edit button

## Chat pane
- Empty state: lock icon, "Выберите чат", E2E hint
- Chat view: header (avatar, title, E2E status, safety button), messages, compose bar

## Onboarding screen (`#onboarding`)
Centered card on gradient background — avatar picker, user ID, display name, create button.
