# Terminal Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unicode 11 character widths for every session terminal, and the existing WebGL renderer setup moved into a tested module.

**Architecture:** Addon setup moves from `TerminalPane.tsx` into `src/lib/terminalRenderer.ts` (two functions taking a `Terminal`). `TerminalPane` calls `loadUnicodeWidths` at creation and `loadWebglRenderer` after `terminal.open()`.

**Tech Stack:** React 19, @xterm/xterm 5.5, @xterm/addon-webgl 0.19, @xterm/addon-unicode11 0.9, Vitest + jsdom.

**Spec:** `specs/terminal-rendering.spec.md`

## Global Constraints

- Branch: `feat/xterm-webgl` (already contains the WebGL commit `a56811b`).
- `allowProposedApi: true` stays on the `Terminal` options.
- Unicode version is set before any write to the terminal (live output or restored scrollback).

## Review Focus

- WebGL unavailable (jsdom, old GPU) → terminal still renders on DOM, no thrown error. Covered by Task 1 test `webgl constructor failure falls back`.
- GL context loss after many session switches → addon disposed, no crash. Covered by Task 1 test `context loss disposes addon`.
- Scrollback restored after LRU eviction renders with same widths → ordering in Task 2 Step 1 (unicode loaded in `getOrCreateTerminal`, before `open` and `flushPendingWrites`).

---

### Task 1: `terminalRenderer` module

**Files:**
- Create: `src/lib/terminalRenderer.ts`
- Create: `src/__tests__/terminalRenderer.test.ts`
- Modify: `package.json` (add `@xterm/addon-unicode11@^0.9.0`)

**Interfaces:**
- Produces: `loadUnicodeWidths(terminal: Terminal): void`, `loadWebglRenderer(terminal: Terminal): void`

- [ ] **Step 1: Install** `npm install @xterm/addon-unicode11@^0.9.0`

- [ ] **Step 2: Write failing tests** in `src/__tests__/terminalRenderer.test.ts`, mocking `@xterm/addon-webgl` and `@xterm/addon-unicode11` with `vi.mock`; `terminal` is a stub `{ loadAddon: vi.fn(), unicode: { activeVersion: "6" } }`:
  - `loadUnicodeWidths loads addon and activates v11` — `loadAddon` called once with the Unicode11Addon instance; `terminal.unicode.activeVersion === "11"`.
  - `loadWebglRenderer loads webgl addon` — `loadAddon` called with the WebglAddon instance.
  - `webgl constructor failure falls back` — WebglAddon mock constructor throws; call does not throw; `console.warn` called once.
  - `context loss disposes addon` — capture the callback passed to `onContextLoss`, invoke it; mocked `dispose` called once.

- [ ] **Step 3: Run** `npx vitest run src/__tests__/terminalRenderer.test.ts` — Expected: FAIL (module not found).

- [ ] **Step 4: Implement** `src/lib/terminalRenderer.ts`: move `loadWebglRenderer` (with its doc comment) out of `TerminalPane.tsx` unchanged; add `loadUnicodeWidths` (load `Unicode11Addon`, set `terminal.unicode.activeVersion = "11"`).

- [ ] **Step 5: Run** `npx vitest run src/__tests__/terminalRenderer.test.ts` — Expected: 4 passed.

- [ ] **Step 6: Commit** `feat(terminal): add terminalRenderer module with Unicode 11 widths`

### Task 2: Wire into `TerminalPane`

**Files:**
- Modify: `src/components/TerminalPane.tsx` (remove local `loadWebglRenderer` + `WebglAddon` import; import both functions from `../lib/terminalRenderer`)

**Interfaces:**
- Consumes: `loadUnicodeWidths`, `loadWebglRenderer` from Task 1.

- [ ] **Step 1: Edit** — in `getOrCreateTerminal`, call `loadUnicodeWidths(terminal)` right after `loadAddon(serializeAddon)`; keep `loadWebglRenderer(terminal)` right after `terminal.open(wrapper)`.

- [ ] **Step 2: Verify** `npm run typecheck && npx vitest run` — Expected: clean typecheck, all tests pass (301 + 4).

- [ ] **Step 3: Manual check** `npm run tauri dev`, open a Claude session: box borders and emoji in the agent UI aligned, cursor on the right column.

- [ ] **Step 4: Commit** `feat(terminal): enable Unicode 11 widths in session terminals` (include `specs/terminal-rendering.spec.md`).
