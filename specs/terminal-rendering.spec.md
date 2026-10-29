---
name: Terminal Rendering
description: xterm.js renderer and Unicode width configuration for session terminals — WebGL renderer with DOM fallback, Unicode 11 character widths
targets:
  - ../src/lib/terminalRenderer.ts
  - ../src/components/TerminalPane.tsx
---

# Terminal Rendering

Session terminals are xterm.js instances created in `TerminalPane.tsx`. This spec covers how they render: which renderer draws the grid and how character widths are computed. Agent TUIs (Claude, Codex, Gemini) draw boxes, spinners, and emoji; wrong widths shift borders and the cursor.

## Scope

In scope:
- WebGL renderer (`@xterm/addon-webgl`) with fallback to xterm's built-in DOM renderer.
- Unicode 11 width tables (`@xterm/addon-unicode11`).

Out of scope:
- Font choice, themes, sizes (settings store, unchanged). Making the WebGL glyph atlas use the configured font once it loads *is* in scope (below).
- Replacing xterm.js (ghostty-web evaluated 2026-10-29 and deferred: no official WebKit/WKWebView support, no serialize equivalent needed for scrollback restore).
- PTY output transport — see `pty-output-transport.spec.md`.

## WebGL renderer

- The WebGL addon is loaded after `terminal.open()` for every newly opened terminal, not before (the addon needs a mounted element).
  `[@test] ../src/__tests__/terminalRenderer.test.ts`
- If constructing or loading the WebGL addon throws (no WebGL2 context), the terminal keeps working on the DOM renderer and a warning is logged; no error surfaces to the user.
  `[@test] ../src/__tests__/terminalRenderer.test.ts`
- On WebGL context loss the addon is disposed, which returns the terminal to the DOM renderer.
  `[@test] ../src/__tests__/terminalRenderer.test.ts`
- At most `MAX_LIVE_TERMINALS` (5) WebGL contexts exist at once, since each live terminal holds one and evicted terminals are disposed with their addons.

## Web fonts and the glyph atlas

The WebGL renderer rasterizes each glyph (per weight/style) into a texture atlas once and reuses it. A glyph drawn before its web-font face has loaded is rasterized with a fallback font and stays that way — unlike the DOM renderer, which the browser re-renders when fonts arrive. Faces load lazily, so the bold face is typically still missing when the first bold text arrives; bold text then appears in a different font with different metrics.

- When the font set finishes loading (`document.fonts` `loadingdone`), every live terminal clears its texture atlas (`terminal.clearTextureAtlas()`) and re-fits, so glyphs are re-rasterized with the loaded faces.
  `[@test] ../src/__tests__/terminalRenderer.test.ts`
- At terminal creation the regular and bold faces of the configured `fontFamily` at the configured `fontSize` are requested via `document.fonts.load(...)`, so the bold face starts loading before any bold output instead of on first use.
  `[@test] ../src/__tests__/terminalRenderer.test.ts`
- Changing `fontFamily` in settings triggers the same preload for the new family.
- The Google Fonts request includes `JetBrains Mono` weight 700, so xterm's default bold (`fontWeightBold: "bold"`) maps to a real bold face instead of the nearest (600).
- If `document.fonts` is unavailable or a load rejects, terminals keep rendering with whatever is cached; nothing surfaces to the user.
  `[@test] ../src/__tests__/terminalRenderer.test.ts`

## Unicode 11 widths

- Every terminal loads the Unicode 11 addon and sets `terminal.unicode.activeVersion = "11"` at creation, before any PTY output or restored scrollback is written to it.
  `[@test] ../src/__tests__/terminalRenderer.test.ts`
- Requires `allowProposedApi: true` on the terminal (already set).
- Scrollback restored via SerializeAddon renders with the same widths as live output, because the version is set before the restore write.

## Implementation notes

- Addon setup lives in `src/lib/terminalRenderer.ts` (`loadUnicodeWidths(terminal)`, `loadWebglRenderer(terminal)`) so it can be unit-tested with a mocked `Terminal`; `TerminalPane.tsx` calls them.
