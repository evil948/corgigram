# Extractable Components

## Sidebar
- Source: `apps/desktop/ui/index.html` (`.sidebar` block)
- Category: layout
- Description: Contact list sidebar with search, actions, profile footer
- Extractable props: activeContactId (string), searchQuery (string), hasInvitations (boolean)
- Hardcoded: Corgigram logo text, Russian labels, contact rows

## ChatHeader
- Source: `apps/desktop/ui/index.html` (`.chat-header`)
- Category: layout
- Description: Chat title, avatar, E2E status badge, safety button
- Extractable props: contactName (string), statusText (string), isConnected (boolean)
- Hardcoded: lock SVG, shield emoji button

## MessageBubble
- Source: `apps/desktop/ui/styles.css` (`.msg-row`, `.bubble`)
- Category: basic
- Description: Telegram-style message bubble in/out
- Extractable props: direction ("in"|"out"), body (string), time (string)
- Hardcoded: bubble colors, border-radius tails

## ComposeBar
- Source: `apps/desktop/ui/index.html` (`.compose-bar`)
- Category: layout
- Description: Rounded textarea + circular send button
- Extractable props: placeholder (string)
- Hardcoded: send SVG icon, orange gradient button

## OnboardingCard
- Source: `apps/desktop/ui/index.html` (`.onboard-card`)
- Category: layout
- Description: First-run identity creation card
- Extractable props: none
- Hardcoded: Corgi emoji fallback, form fields, gradient title

## ContactItem
- Source: `apps/desktop/ui/styles.css` (`.contact-item`)
- Category: basic
- Description: Sidebar contact row with avatar and preview
- Extractable props: name, userId, isActive, isOnline, avatarUrl
- Hardcoded: avatar gradient, hover/active styles
