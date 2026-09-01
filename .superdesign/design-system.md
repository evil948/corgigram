# Corgigram Orbit Design System (implemented)

## Thesis
Spatial depth over flat split-pane — private E2E messenger that feels like layered cards on a warm dark desk. Corgi orange is the sole accent.

## Tokens

### Brand
| Token | Value |
|-------|-------|
| `--corgi-orange` | `#e67e22` |
| `--corgi-orange-hover` | `#f39c12` |
| `--corgi-orange-soft` | `rgba(230, 126, 34, 0.15)` |
| `--corgi-orange-deep` | `#d35400` |
| `--corgi-glow` | `rgba(230, 126, 34, 0.4)` |

### Surfaces
| Token | Value |
|-------|-------|
| `--bg-canvas` | `#12100e` |
| `--surface-card` | `#1e1c1a` |
| `--surface-raised` | `#2a2826` |
| `--surface-profile` | `#252321` |

### Typography
- UI: Satoshi (Fontshare), system fallback
- Mono: JetBrains Mono — safety numbers only

### Layout
- Canvas padding: 24px (12px tablet, 8px mobile)
- Contacts card: 340px
- Card radius: 24px
- Gap between cards: 16px

### Semantic colors
- `--success` `#34c759` — online, live E2E
- `--danger` `#ff6b6b` — destructive actions

## Components
- **Orbit card** — floating panel with shadow + subtle border
- **Contact row** — button, raised surface when active, orange glow on avatar
- **E2E pill** — orange default, green when live connection
- **Outgoing bubble** — corgi orange (not blue)
- **Compose inner** — raised surface, rounded send button

## Responsive
- ≤960px: stack cards; chat-open hides contacts; back button
- ≤520px: tighter padding, wider message bubbles
