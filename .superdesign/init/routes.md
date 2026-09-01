# Corgigram Desktop UI Routes

Tauri desktop app — single-page vanilla HTML/JS (no router). Screens toggled via CSS classes `.hidden`.

| Screen | DOM id | File | Description |
|--------|--------|------|-------------|
| Onboarding | `#onboarding` | `apps/desktop/ui/index.html` | First-run identity creation |
| Main app | `#app` | `apps/desktop/ui/index.html` | Sidebar + chat pane |
| Empty chat | `#empty-state` | inside `#app` | No contact selected |
| Active chat | `#chat-view` | inside `#app` | Messages + compose |
| Modals | `dialog#modal-*` | `apps/desktop/ui/index.html` | Profile, add contact, QR, connect, safety, settings |

## Entry files
- `apps/desktop/ui/index.html` — structure
- `apps/desktop/ui/styles.css` — all styles
- `apps/desktop/ui/app.js` — logic (not passed to design)
