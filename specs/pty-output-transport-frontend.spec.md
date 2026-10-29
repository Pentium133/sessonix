---
name: PTY Output Transport (frontend)
description: Frontend consumer of binary PTY output frames from the Tauri channel, including attach replay and subscription ordering
targets:
  - ../src/lib/ptyOutput.ts
  - ../src/lib/api.ts
  - ../src/hooks/usePtyOutput.ts
  - ../src/store/sessionStore.ts
  - ../src/lib/types.ts
---

# PTY Output Transport (frontend)

The backend (`specs/pty-output-transport.spec.md`) no longer emits `pty-output` events and `attach_session` no longer returns buffered bytes. All PTY output — live batches and attach replay — arrives as binary frames on one Tauri `Channel` registered with `subscribe_pty_output`. Until the frontend subscribes, the backend drops frames. This spec moves the frontend to that channel.

## Consumed backend interface

```ts
// command: subscribe_pty_output — later call replaces the previous channel
invoke("subscribe_pty_output", { onOutput: Channel<ArrayBuffer> }): Promise<void>
// command: attach_session — replay (if any) is sent on the channel, not returned
invoke("attach_session", { id }): Promise<void>
// frame: [u32 little-endian session id][raw output bytes]
// event: pty-exit { id } — unchanged
```

## Scope

In scope:
- Frame decoding and routing to `writeToTerminal`.
- Subscribing once per page load and ordering it before any session output is requested.
- `attachSession` / `switchSession` no longer write replay themselves.
- Removing the `pty-output` listener and `PtyOutputPayload`.

Out of scope:
- Backend changes.
- `pty-exit` handling (notifications, worktree cleanup) — unchanged.
- `terminalPool` write path (`pendingWrites`, LRU eviction) — unchanged.

## Frame decoding

```ts
// src/lib/ptyOutput.ts
export function decodeFrame(frame: ArrayBuffer | ArrayBufferView): { id: number; data: Uint8Array } | null;
```

- Session id is the first 4 bytes read as little-endian `u32`; `data` is the rest, as a view without copying.
  `[@test] ../src/__tests__/ptyOutput.test.ts`
- Accepts both `ArrayBuffer` and a typed-array view (respecting its `byteOffset`/`byteLength`).
  `[@test] ../src/__tests__/ptyOutput.test.ts`
- A frame shorter than 4 bytes returns `null` and is ignored; a frame of exactly 4 bytes yields empty `data` and is not written.
  `[@test] ../src/__tests__/ptyOutput.test.ts`
- Each decoded frame is passed to `writeToTerminal(id, data)` in arrival order. Output for a session with no terminal in the pool is dropped (same as today).
  `[@test] ../src/__tests__/ptyOutput.test.ts`

## Subscription

```ts
// src/lib/ptyOutput.ts
export function ensurePtyOutputSubscribed(): Promise<void>;
// src/lib/api.ts
export function subscribePtyOutput(channel: Channel<ArrayBuffer>): Promise<void>;
```

- `ensurePtyOutputSubscribed` creates the channel and invokes `subscribe_pty_output` on first call; later calls return the same promise (one channel per page load).
  `[@test] ../src/__tests__/ptyOutput.test.ts`
- If the invoke fails, the promise rejects and the next call retries with a new channel.
  `[@test] ../src/__tests__/ptyOutput.test.ts`
- `usePtyOutput` calls it on mount so the subscription starts at app startup. A page reload or HMR reload of the module creates a new channel, which replaces the old one on the backend.
- `usePtyOutput` no longer listens for `pty-output`; it keeps the `pty-exit` listener.

## Ordering

- `addSession` awaits `ensurePtyOutputSubscribed()` before `createSession`, so the first output of a new session is not dropped.
  `[@test] ../src/__tests__/sessionStore.test.ts`
- `switchSession` awaits `ensurePtyOutputSubscribed()` before `attachSession`, so the replay frame is not dropped.
  `[@test] ../src/__tests__/sessionStore.test.ts`
- If the subscription fails, the operation fails with that error (session not created / not attached) instead of running with no output.
  `[@test] ../src/__tests__/sessionStore.test.ts`

## Attach

- `attachSession(id)` returns `Promise<void>`.
- `switchSession` does not call `writeToTerminal` for the attached session; replay arrives through the channel.
  `[@test] ../src/__tests__/sessionStore.test.ts`

## Cleanup

- `PtyOutputPayload` is removed from `src/lib/types.ts`; no frontend code references `"pty-output"`.

## Rollout

- Lands on `feat/pty-binary-output` together with the backend. Before merging, `main` is merged into the branch; `npm run typecheck`, `npx vitest run`, `cargo test` and `cargo clippy -- -D warnings` pass.
- Manual check in `npm run tauri dev`: agent output streams; switching sessions shows output produced while hidden, without duplicates; an agent's last output appears before the session shows exited; output keeps flowing after an HMR reload.
