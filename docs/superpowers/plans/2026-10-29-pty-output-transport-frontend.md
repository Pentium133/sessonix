# PTY Output Transport (frontend) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The frontend receives all PTY output (live and attach replay) as binary frames over the `subscribe_pty_output` channel instead of `pty-output` events and `attach_session` return values.

**Architecture:** New module `src/lib/ptyOutput.ts` decodes frames, owns the one-per-page-load `Channel` and exposes a memoized `ensurePtyOutputSubscribed()`. `usePtyOutput` starts the subscription on mount; `sessionStore.addSession` / `switchSession` await it before asking the backend for output. `attachSession` returns nothing.

**Tech Stack:** React 19, Zustand, `@tauri-apps/api` 2.10.1 (`Channel`, `invoke`), Vitest + jsdom.

**Spec:** `specs/pty-output-transport-frontend.spec.md` (backend contract: `specs/pty-output-transport.spec.md`)

## Global Constraints

- Branch `feat/pty-binary-output` (already checked out, no upstream). First push: `git push -u origin feat/pty-binary-output`.
- Frame: `[u32 little-endian session id][raw output bytes]`.
- Command `subscribe_pty_output({ onOutput: Channel<ArrayBuffer> })`; `attach_session({ id })` → `void`; `pty-exit` unchanged.
- No frontend code references `"pty-output"`; `PtyOutputPayload` removed.
- Payload bytes are never decoded as text.

## Review Focus

- Frame delivered as a typed-array view with non-zero `byteOffset` → id and payload read relative to the view, not the underlying buffer. Task 1 test `decodes typed-array view with offset`.
- UTF-8 sequence split across two frames → bytes passed through unchanged, xterm reassembles. Task 1 test `passes split multi-byte bytes unchanged`.
- Two callers race at startup (mount + first `switchSession`) → exactly one `subscribe_pty_output` invoke. Task 1 test `concurrent first calls share one subscription`.
- Subscription fails on mount → no unhandled rejection; a later `addSession` retries. Task 1 test `retries after failure`; Task 2 Step 3 adds `.catch(console.error)` on mount.
- Subscription fails during `switchSession` → current session stays attached (subscription awaited before detach). Task 2 test `switchSession fails before detaching when subscription fails`.

---

### Task 1: `ptyOutput` module

**Files:**
- Create: `src/lib/ptyOutput.ts`
- Modify: `src/lib/api.ts` (add `subscribePtyOutput`)
- Modify: `src/__tests__/setup.ts` (add `Channel` to the `@tauri-apps/api/core` mock)
- Test: `src/__tests__/ptyOutput.test.ts`

**Interfaces:**
- Produces:
  - `api.ts`: `export async function subscribePtyOutput(channel: Channel<ArrayBuffer>): Promise<void>` → `invoke("subscribe_pty_output", { onOutput: channel })`
  - `ptyOutput.ts`: `export function decodeFrame(frame: ArrayBuffer | ArrayBufferView): { id: number; data: Uint8Array } | null`
  - `ptyOutput.ts`: `export function ensurePtyOutputSubscribed(): Promise<void>`

- [ ] **Step 1: Extend setup mock.** In `setup.ts` the `@tauri-apps/api/core` mock also exports `Channel`: a class with a public `onmessage` (defaults to no-op, set from the optional constructor arg). Without it every test that reaches `ensurePtyOutputSubscribed` throws on `new Channel`.

- [ ] **Step 2: Failing tests** in `ptyOutput.test.ts`. Mock `../lib/api` (`subscribePtyOutput: vi.fn()`, capture the channel argument) and `../lib/terminalPool` (`writeToTerminal: vi.fn()`). In `beforeEach`: `vi.resetModules()` and `await import("../lib/ptyOutput")` so the memo is fresh per test. Helper `frame(id, bytes)` builds an `ArrayBuffer` with LE id.
  - `decodes id and payload` — `decodeFrame(frame(0x01020304, [104, 105]))` → `{ id: 0x01020304, data: Uint8Array [104,105] }`.
  - `decodes typed-array view with offset` — 8-byte buffer, view `new Uint8Array(buf, 2, 6)` holding id 7 + `[1,2]` → `{ id: 7, data: [1,2] }`.
  - `short frame returns null` — 3-byte buffer → `null`.
  - `routes frames to terminal in order` — `await ensurePtyOutputSubscribed()`; call captured `channel.onmessage` with `frame(1,[97])`, `frame(2,[98])`, `frame(1,[99])` → `writeToTerminal` calls are `(1,[97])`, `(2,[98])`, `(1,[99])` in that order.
  - `empty payload is not written` — `onmessage(frame(1, []))` → `writeToTerminal` not called.
  - `passes split multi-byte bytes unchanged` — frames `[0xE2,0x94]` then `[0x80]` for id 1 → written bytes equal exactly those arrays.
  - `concurrent first calls share one subscription` — call twice before resolving → `subscribePtyOutput` called once, both promises resolve.
  - `later calls reuse subscription` — after resolve, call again → still one call.
  - `retries after failure` — first `subscribePtyOutput` rejects → promise rejects; next call invokes again with a new channel (different object) and resolves.

- [ ] **Step 3: Run** `npx vitest run src/__tests__/ptyOutput.test.ts` — Expected: FAIL (module not found).

- [ ] **Step 4: Implement** `subscribePtyOutput` in `api.ts` and `ptyOutput.ts`. Module-level `let subscription: Promise<void> | null`; on rejection reset to `null` before rethrowing. `onmessage` → `decodeFrame` → skip `null` / empty `data` → `writeToTerminal`. Use `DataView(...).getUint32(0, true)` and `new Uint8Array(buffer, byteOffset + 4, byteLength - 4)` (no copy).

- [ ] **Step 5: Run** `npx vitest run src/__tests__/ptyOutput.test.ts` — Expected: 9 passed.

- [ ] **Step 6: Commit** `feat(pty): decode binary output frames from the subscription channel` (include `specs/pty-output-transport-frontend.spec.md` and this plan).

### Task 2: Wire into hook and store

**Files:**
- Modify: `src/hooks/usePtyOutput.ts` (drop `pty-output` listener and `writeToTerminal` import; call `ensurePtyOutputSubscribed().catch(console.error)` on mount; keep `pty-exit`)
- Modify: `src/store/sessionStore.ts` (`addSession` ~L166, `switchSession` ~L327-347)
- Modify: `src/lib/api.ts` (`attachSession(id): Promise<void>`)
- Modify: `src/lib/types.ts` (remove `PtyOutputPayload`)
- Test: `src/__tests__/sessionStore.test.ts`

**Interfaces:**
- Consumes: `ensurePtyOutputSubscribed(): Promise<void>` from Task 1.

- [ ] **Step 1: Failing tests** in `sessionStore.test.ts`. Add `vi.mock("../lib/ptyOutput", () => ({ ensurePtyOutputSubscribed: vi.fn().mockResolvedValue(undefined) }))`; change the `attachSession` mock to resolve `undefined`.
  - `addSession subscribes before createSession` — `ensurePtyOutputSubscribed` called, and its `mock.invocationCallOrder[0]` < `createSession`'s.
  - `addSession fails when subscription fails` — `mockRejectedValueOnce(new Error("sub"))` → `addSession` rejects with `"sub"`; `createSession` not called.
  - `switchSession subscribes before attach` — two running sessions, switch → subscribe order < `attachSession` order.
  - `switchSession fails before detaching when subscription fails` — rejects with `"sub"`; `detachSession` and `attachSession` not called; `activeSessionId` unchanged.
  - `switchSession does not write replay` — after switch, `writeToTerminal` (mocked `terminalPool`) not called.

- [ ] **Step 2: Run** `npx vitest run src/__tests__/sessionStore.test.ts` — Expected: the 5 new tests FAIL.

- [ ] **Step 3: Implement.**
  - `addSession`: `await ensurePtyOutputSubscribed()` as the first statement (before the detach), so a failure leaves state untouched.
  - `switchSession`: `await ensurePtyOutputSubscribed()` right after the `targetId === activeSessionId` early return.
  - `switchSession`: replace the `buffered` block with `await attachSession(targetId)`; remove the now-unused `writeToTerminal` import from the store if nothing else uses it.
  - `attachSession` → `invoke<void>("attach_session", { id })`; delete `PtyOutputPayload`; edit `usePtyOutput` as listed in Files.

- [ ] **Step 4: Verify** `npm run typecheck && npx vitest run` — clean typecheck, all tests pass. `grep -rn '"pty-output"\|PtyOutputPayload' src` → no matches.

- [ ] **Step 5: Commit** `feat(pty): consume output over the binary channel; attach replay via channel`

### Task 3: Merge main and verify the app

**Files:** none beyond merge resolution.

- [ ] **Step 1: Merge** `git fetch origin && git merge main` (no conflicts expected; `git merge-tree` was clean on 2026-10-29). Resolve any conflict in favour of both sides' intent.

- [ ] **Step 2: Verify** `npm run typecheck && npx vitest run && cd src-tauri && cargo test && cargo clippy -- -D warnings` — all pass, no warnings.

- [ ] **Step 3: Manual check** in `npm run tauri dev`:
  - Claude session: output streams, spinner animates smoothly.
  - Two sessions, A busy; switch to B, wait, back to A → output produced while hidden is present, nothing duplicated.
  - Session whose terminal was evicted (open > 5 sessions, return to the oldest) → scrollback + missed output render.
  - Agent prints then exits (`custom` session running `echo bye`) → `bye` visible before the session shows exited.
  - Save a frontend file to trigger HMR → output keeps flowing in the active session.

- [ ] **Step 4: Commit** the merge (if not auto-committed) and push `git push -u origin feat/pty-binary-output`; check `git branch -vv` shows `[origin/feat/pty-binary-output]`.
