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
- Fonts, themes, sizes (settings store, unchanged).
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

## Unicode 11 widths

- Every terminal loads the Unicode 11 addon and sets `terminal.unicode.activeVersion = "11"` at creation, before any PTY output or restored scrollback is written to it.
  `[@test] ../src/__tests__/terminalRenderer.test.ts`
- Requires `allowProposedApi: true` on the terminal (already set).
- Scrollback restored via SerializeAddon renders with the same widths as live output, because the version is set before the restore write.

## Implementation notes

- Addon setup lives in `src/lib/terminalRenderer.ts` (`loadUnicodeWidths(terminal)`, `loadWebglRenderer(terminal)`) so it can be unit-tested with a mocked `Terminal`; `TerminalPane.tsx` calls them.
