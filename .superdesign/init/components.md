# Shared UI Components (vanilla HTML/CSS)

No component library — UI built with semantic HTML + CSS classes in `apps/desktop/ui/`.

## Avatar (`.avatar`)
- File: `apps/desktop/ui/styles.css` (lines 133–184)
- Sizes: default 48px, `.avatar-sm` 40px, `.avatar-lg` 88px, `.avatar-xl` 96px
- Gradient fallback, image overlay via `.has-image`

## Buttons

### `.btn-primary`
```css
.btn-primary {
  width: 100%;
  margin-top: 1.5rem;
  padding: 0.8rem;
  background: linear-gradient(135deg, var(--accent) 0%, #d35400 100%);
  color: white;
  border: none;
  border-radius: var(--radius-sm);
  font-size: 1rem;
  font-weight: 600;
  cursor: pointer;
}
```

### `.btn-accent-sm`, `.btn-ghost`, `.icon-btn`, `.send-btn`
See `apps/desktop/ui/styles.css` lines 204–636.

## Contact item (`.contact-item`)
Sidebar list row: avatar + name + preview. Active state: orange left bar + soft background.

## Message bubble (`.bubble`, `.msg-row`)
Telegram-style rounded bubbles; outgoing blue gradient, incoming dark gray.

## Modal (`dialog`)
Native HTML dialog with backdrop blur. Used for profile, add contact, QR, safety number, settings.

## E2E badge (`.e2e-badge`)
Green lock icon + status text under chat title (Signal-inspired).
