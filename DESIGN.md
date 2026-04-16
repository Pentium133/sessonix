# Design System — AICoder

## Product Context
- **What this is:** Desktop app for managing multiple AI coding agent sessions in parallel via PTY terminals
- **Who it's for:** Developers running Claude, Codex, Gemini, and custom CLI agents
- **Space/industry:** Developer tools, AI coding assistants, terminal management
- **Project type:** Desktop app (Tauri 2), terminal-centric dashboard

## Aesthetic Direction
- **Direction:** Industrial Operational
- **Decoration level:** Minimal — typography and status color do all the work, zero ornament
- **Mood:** Mission control, not text editor. Calm authority. The feeling of a well-built ops console where everything is exactly where you expect it. "Built by someone who uses this."
- **Reference sites:** Linear (theme system, status badges), Warp (blocks UI, dark depth), Ghostty (radical minimalism), Raycast (premium dark feel), Zed (performance-first)

## Typography
- **Display/Hero:** Geist 700 — clean geometric authority, letter-spacing -0.02em
- **Body/UI:** Geist 400/500 at 13px — invisible infrastructure, not the star
- **Labels/Captions:** Geist 600 at 11px — uppercase, letter-spacing 0.06em
- **Terminal/Code:** JetBrains Mono 400 at terminal default — ligatures on, the standard dev mono
- **Data/Metrics:** JetBrains Mono 400 at 11px — tabular-nums for aligned numbers
- **Loading:** Google Fonts (`family=Geist:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600`). For Tauri desktop, bundle woff2 files locally.
- **Scale:** 10px (caption) · 11px (label, badge, status) · 12px (sidebar, small body) · 13px (body) · 14px (launcher title) · 16px (subtitle) · 22px (section heading) · 28-32px (hero/welcome)

## Color
- **Approach:** Restrained — 1 accent + agent colors + semantic. Color is rare and meaningful.
- **Background:** `#0B0E14` — deep cold blue-black, darker than Tokyo Night
- **Surface:** `#11151C` — sidebar, titlebar, statusbar, modals
- **Hover:** `#1A1F2B` — interactive surface state
- **Border:** `#1D2433` — cool slate, structural dividers
- **Text:** `#BAC5D6` — primary body text
- **Text Bright:** `#E0E6F0` — headings, active session names, emphasis
- **Text Dim:** `#4A5670` — metadata, labels, secondary info
- **Accent:** `#38BDF8` — sky cyan. Focus rings, active borders, primary buttons, links. Distinctive in the category (nobody else uses cyan).
- **Accent Muted:** `rgba(56, 189, 248, 0.12)` — accent backgrounds, subtle highlights
- **Claude:** `#C4A7E7` — soft lavender purple (agent identity)
- **Codex:** `#7DD681` — phosphor green (agent identity)
- **Gemini:** `#5ABFEF` — bright sky blue (agent identity)
- **Success:** `#7DD681` — running sessions, completed actions
- **Warning:** `#F0B45A` — waiting permission, cost display
- **Error:** `#F07178` — crashed sessions, validation errors
- **Info:** `#38BDF8` — same as accent, informational alerts
- **Dark mode:** Primary (and only) mode. No light mode planned for Phase 1.

### CSS Variables
```css
:root {
  --bg: #0B0E14;
  --surface: #11151C;
  --hover: #1A1F2B;
  --border: #1D2433;
  --text: #BAC5D6;
  --text-bright: #E0E6F0;
  --text-dim: #4A5670;
  --accent: #38BDF8;
  --accent-muted: rgba(56, 189, 248, 0.12);
  --success: #7DD681;
  --warning: #F0B45A;
  --error: #F07178;
  --claude: #C4A7E7;
  --codex: #7DD681;
  --gemini: #5ABFEF;
  --radius: 6px;
  --radius-lg: 12px;
  --space: 4px;
  --font-ui: 'Geist', -apple-system, BlinkMacSystemFont, sans-serif;
  --font-mono: 'JetBrains Mono', monospace;
}
```

## Spacing
- **Base unit:** 4px
- **Density:** Comfortable (not cramped, not spacious — ops console density)
- **Scale:** 2xs(2px) · xs(4px) · sm(8px) · md(16px) · lg(24px) · xl(32px) · 2xl(48px) · 3xl(64px)
- **Usage:** Use `calc(var(--space) * N)` for consistency. Sidebar padding: 3-4 units. Session items: 2 units vertical. Modals: 5 units internal padding.

## Layout
- **Approach:** Grid-disciplined — strict columns, terminal flex-grow
- **Navigation model:** Slack-style two-column. ProjectRail (48px, always visible) + SessionPanel (260px, hideable). Clicking a project in the rail scopes the entire UI (session panel, summary bar, keyboard shortcuts) to that project only. No session mixing across projects.
- **Project Rail:** 48px fixed, left edge. Shows project letter icons with running badge. Active project highlighted with accent border + muted bg. "+" button at bottom for adding projects.
- **Session Panel:** 260px default (resizable 180-500px). Shows only sessions for the active project. Project name in header. Collapse via chevron hides panel (rail stays).
- **Summary bar:** 44px fixed height, horizontal scroll. Scoped to active project.
- **Status bar:** 26px fixed height
- **Terminal:** flex: 1 (takes all remaining space)
- **Min window:** 900x600
- **Max content width:** None (full bleed terminal)
- **Border radius:** sm: 3px (tiny badges) · md: 6px (cards, buttons, inputs) · lg: 12px (modals, launcher)
- **Keyboard shortcuts:** Cmd+1-9 switches sessions within active project (not global). Cmd+Shift+T creates session in active project.

## Motion
- **Approach:** Minimal-functional — only transitions that aid comprehension
- **Easing:** enter: ease-out · exit: ease-in · move: ease-in-out
- **Duration:** micro: 100ms (opacity, color) · short: 150ms (hover, focus, badge) · medium: 250ms (modal appear, toast slide)
- **Rules:** No bounce, no spring, no parallax. Collapse arrow rotation: 150ms. Toast entrance: 200ms translateY(8px). Opacity reveals on hover elements: 150ms.

## Component Patterns

### Status Badges
Pill-shaped (border-radius 10px), 10px font, semi-transparent background matching status color at 15% opacity. Text color = full status color.

### Session Items
Left border accent (2px, transparent default, accent on active). Hover shows action buttons (kill, fork, relaunch) via opacity 0 -> 1 transition. Agent type shown via official brand icon (20px sidebar, 18px summary bar).

### Project Rail Items
36x36px square buttons with 1px border. Active: accent border + accent-muted bg. Running badge: 6px green dot, top-right. Add button: dashed border, "+" icon, pinned to bottom via margin-top: auto.

### Buttons
- Primary: accent bg, dark text, 600 weight
- Secondary: hover bg, text color, 1px border
- Ghost: transparent bg, dim text, 1px border
- Danger: error bg, dark text
- All: 6px radius, 12px font, 6px 16px padding

### Alerts
Left border accent (3px), semi-transparent bg at 8% opacity, matching status color for both border and text.

### Toasts
Bottom-right stack, 12px font, status-colored background, dark text. Auto-dismiss 3-5s. Optional action button with semi-transparent white styling.

## Decisions Log
| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-13 | Initial design system created | /design-consultation based on competitive research (Warp, Linear, Zed, Ghostty, Raycast) + Claude subagent "Mission Glass" proposal |
| 2026-04-13 | Sky cyan (#38BDF8) over blue (#7aa2f7) | Category differentiation — every competitor uses blue or violet. Cyan reads "ops center". |
| 2026-04-13 | Geist over system fonts | Product identity — system fonts are invisible but generic. Geist is free, proven (Vercel), +50KB. |
| 2026-04-13 | Deeper background (#0B0E14 vs #1a1b26) | More contrast with terminal content. Terminal is the product — make it pop. |
| 2026-04-13 | No light mode in Phase 1 | 100% of target users (developers managing AI agents) prefer dark. Revisit in Phase 2. |
| 2026-04-14 | Slack-style project rail + session panel | Replace mixed-project sidebar with two-column nav. Projects always visible in 48px rail, sessions scoped to active project. Inspired by competitor analysis (project-focused UX). |
