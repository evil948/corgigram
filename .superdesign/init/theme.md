# Corgigram Theme Tokens

## Compact Token Summary

### Colors (dark theme)
| Token | Value | Usage |
|-------|-------|-------|
| `--bg-app` | `#0f0f11` | App background |
| `--bg-sidebar` | `#161618` | Sidebar, chat header, compose |
| `--bg-chat` | `#0a0a0c` | Chat pane |
| `--bg-bubble-in` | `#26262a` | Incoming messages |
| `--bg-bubble-out` | `#2563eb` | Outgoing messages (blue gradient) |
| `--bg-input` | `#222226` | Inputs, search |
| `--bg-hover` | `#1f1f23` | Hover states |
| `--bg-active` | `#2a2a30` | Active states |
| `--bg-elevated` | `#1c1c20` | Cards, modals, footer |
| `--text-primary` | `#f4f4f5` | Primary text |
| `--text-secondary` | `#9898a0` | Secondary text |
| `--text-muted` | `#6b6b73` | Muted text |
| `--accent` | `#e67e22` | Corgi brand orange |
| `--accent-soft` | `rgba(230,126,34,0.15)` | Active contact highlight |
| `--accent-blue` | `#2563eb` | Outgoing bubbles, badges |
| `--success` | `#34c759` | E2E badge (Signal-like) |
| `--danger` | `#ff6b6b` | Destructive actions |
| `--border` | `#2a2a30` | Borders |

### Typography
- Font: Inter, system-ui stack
- Base size: 15px
- Contact name: 0.95rem, weight 600
- Chat title: 1rem, weight 600
- Labels: 0.72rem uppercase, letter-spacing 0.06em

### Spacing & Layout
- Sidebar width: 320px
- Border radius bubble: 18px
- Border radius sm: 10px
- Border radius lg: 16px
- Message max width: 72%

### Shadows
- `--shadow`: `0 8px 32px rgba(0,0,0,0.35)`

### Design intent
Telegram layout (sidebar + chat) × Signal privacy cues (E2E badge, safety number) × corgi orange accent.

## Raw CSS Variables

```css
:root {
  --bg-app: #0f0f11;
  --bg-sidebar: #161618;
  --bg-chat: #0a0a0c;
  --bg-bubble-in: #26262a;
  --bg-bubble-out: #2563eb;
  --bg-input: #222226;
  --bg-hover: #1f1f23;
  --bg-active: #2a2a30;
  --bg-elevated: #1c1c20;
  --radius-bubble: 18px;
  --radius-sm: 10px;
  --radius-lg: 16px;
  --sidebar-width: 320px;
  --text-primary: #f4f4f5;
  --text-secondary: #9898a0;
  --text-muted: #6b6b73;
  --accent: #e67e22;
  --accent-soft: rgba(230, 126, 34, 0.15);
  --accent-hover: #f39c12;
  --accent-blue: #2563eb;
  --accent-blue-hover: #3b82f6;
  --success: #34c759;
  --danger: #ff6b6b;
  --border: #2a2a30;
  --shadow: 0 8px 32px rgba(0, 0, 0, 0.35);
  font-family: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  font-size: 15px;
  color: var(--text-primary);
}
```
