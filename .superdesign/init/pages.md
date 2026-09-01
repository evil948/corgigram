# Page Dependency Trees

## Main Chat (primary design target)
Entry: `apps/desktop/ui/index.html` (lines 32–101, `#app` branch)
Dependencies:
- `apps/desktop/ui/styles.css` (full stylesheet)
- `apps/desktop/ui/app.js` (logic only — strip for design context)

Rendered branch: desktop two-pane with sidebar visible + chat-view active (not empty-state).

## Onboarding
Entry: `apps/desktop/ui/index.html` (lines 11–29, `#onboarding`)
Dependencies:
- `apps/desktop/ui/styles.css` (onboarding + avatar + button sections)

## Modals (secondary)
Entry: `apps/desktop/ui/index.html` (lines 105–201)
Dependencies:
- `apps/desktop/ui/styles.css` (dialog, modal-tabs, profile-edit sections)
