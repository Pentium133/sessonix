# PTY Output Transport (backend) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** PTY output reaches the frontend as coalesced binary frames over one Tauri `Channel`; attach replay returns raw bytes.

**Architecture:** New module `pty_output.rs` owns frame encoding, the app-wide `OutputHub` (current channel), and a per-session `OutputBatcher` that also owns the session's attach flag and ring buffer, so record/detach/attach are atomic under one lock. `PtySession` delegates to its batcher; the reader thread calls `batcher.record()` instead of emitting `pty-output`.

**Tech Stack:** Rust, Tauri 2.10 (`tauri::ipc::{Channel, InvokeResponseBody, Response}`), parking_lot.

**Spec:** `specs/pty-output-transport.spec.md`

## Global Constraints

- New branch `feat/pty-binary-output` created from `feat/xterm-webgl` with `git switch -c` (no upstream on main); first push `git push -u origin feat/pty-binary-output`.
- Frame = 4-byte little-endian `u32` session id + raw bytes.
- Flush thresholds: `MAX_BATCH_BYTES = 64 * 1024`, `MAX_BATCH_DELAY = Duration::from_millis(4)`.
- `pty-exit` stays a Tauri event; `pty-output` event is removed.
- Branch is not merged until the frontend spec is implemented (current frontend breaks).
- `cd src-tauri && cargo clippy -- -D warnings` must pass.

## Review Focus

- Bytes read while `detach` runs must end up either sent or in the ring buffer, never neither. Task 2 test `concurrent record during detach loses nothing`.
- Agent dies while output pending → last output arrives before `pty-exit`. Task 2 test `close with flush sends pending`.
- Killed session → no frame after close. Task 2 test `close without flush drops pending`.
- Frontend reload (HMR) re-subscribes → new channel replaces old. Task 1 test `subscribe replaces previous sink`.
- Burst > 64 KiB → several frames, order preserved. Task 2 test `large burst splits in order`.

---

### Task 1: Frame encoding and `OutputHub`

**Files:**
- Create: `src-tauri/src/pty_output.rs` (+ inline `#[cfg(test)] mod tests`)
- Modify: `src-tauri/src/lib.rs` (`mod pty_output;`)

**Interfaces:**
- Produces:
  - `pub fn encode_frame(id: u32, data: &[u8]) -> Vec<u8>`
  - `pub trait FrameSink: Send + Sync { fn send(&self, frame: Vec<u8>); }` — impl for `tauri::ipc::Channel<InvokeResponseBody>` (sends `InvokeResponseBody::Raw`, ignores error)
  - `pub struct OutputHub` (`Default`), `pub fn subscribe(&self, sink: Arc<dyn FrameSink>)`, `pub fn send(&self, frame: Vec<u8>)` (no-op without sink)

- [ ] **Step 1: Failing tests** with a `RecordingSink(Mutex<Vec<Vec<u8>>>)`:
  - `encode_frame_layout` — `encode_frame(0x01020304, b"hi") == [4,3,2,1,b'h',b'i']`
  - `send_without_sink_is_noop` — no panic.
  - `subscribe_replaces_previous_sink` — subscribe A, subscribe B, send → only B received.
- [ ] **Step 2: Run** `cd src-tauri && cargo test pty_output` — FAIL (unresolved).
- [ ] **Step 3: Implement**; hub holds `Mutex<Option<Arc<dyn FrameSink>>>`.
- [ ] **Step 4: Run** `cargo test pty_output` — PASS.
- [ ] **Step 5: Commit** `feat(pty): add binary frame encoding and output hub`

### Task 2: `OutputBatcher`

**Files:**
- Modify: `src-tauri/src/pty_output.rs`

**Interfaces:**
- Consumes: `OutputHub`, `encode_frame`, `crate::ring_buffer::RingBuffer`.
- Produces:
  - `pub struct OutputBatcher`; `pub fn new(id: u32, ring_capacity: usize, hub: Arc<OutputHub>) -> Arc<Self>` (starts attached; spawns a flusher thread)
  - `pub fn record(&self, data: &[u8])` — under one lock: write ring buffer, append to pending; if pending ≥ `MAX_BATCH_BYTES` send it now; otherwise wake flusher.
  - `pub fn detach(&self)` — under the same lock: send pending if attached, set detached, drain ring buffer.
  - `pub fn attach_and_drain(&self) -> Vec<u8>` — same semantics as today's `PtySession::attach_and_drain`, under the lock.
  - `pub fn is_attached(&self) -> bool`
  - `pub fn close(&self, flush: bool)` — send pending iff `flush && attached`, else drop; stop flusher; later `record` calls are ignored.
- Flusher: `Condvar` on the pending lock; once pending is non-empty, wait until `MAX_BATCH_DELAY` after the first byte entered it (or close), then send if attached else clear.

- [ ] **Step 1: Failing tests** (RecordingSink, decode id from first 4 bytes):
  - `small_writes_coalesce` — `record(b"a")`, `record(b"b")`, sleep 50 ms → exactly one frame with payload `b"ab"`.
  - `large_burst_splits_in_order` — record 3 × 40 KiB distinct-byte chunks, sleep 50 ms → concatenated payloads equal input, every frame ≤ 128 KiB, ≥ 2 frames.
  - `detached_does_not_send` — `detach()`, `record(b"x")`, sleep 50 ms → no frames; `attach_and_drain() == b"x"`.
  - `detach_flushes_pending` — `record(b"x")`, immediately `detach()` → one frame `b"x"` without sleeping.
  - `concurrent_record_during_detach_loses_nothing` — thread records 1000 × 1-byte numbered chunks while main thread calls `detach()` midway; afterwards `sent payload ++ attach_and_drain()` equals all bytes in order.
  - `close_with_flush_sends_pending` — `record(b"z")`, `close(true)` → frame `b"z"` immediately.
  - `close_without_flush_drops_pending` — `record(b"z")`, `close(false)`, sleep 50 ms → no frames; later `record` ignored.
- [ ] **Step 2: Run** `cargo test pty_output` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test pty_output` 5 times in a row — PASS each time (timing tests not flaky).
- [ ] **Step 5: Commit** `feat(pty): coalesce PTY output per session`

### Task 3: Wire into `PtyManager` and IPC

**Files:**
- Modify: `src-tauri/src/pty_manager.rs` (replace `is_attached` + `ring_buffer` fields with `batcher: Arc<OutputBatcher>`; `detach`/`attach_and_drain` delegate; reader thread calls `batcher.record(data)` after the `last_lines` update and deletes the `pty-output` emit; on EOF `batcher.close(true)` then emit `pty-exit`; `kill` calls `batcher.close(false)`; `create_session` takes `hub: Arc<OutputHub>`)
- Modify: `src-tauri/src/session_manager.rs` (own `Arc<OutputHub>`, pass to `create_session`)
- Modify: `src-tauri/src/lib.rs` (new command `subscribe_pty_output(state, on_output: Channel<InvokeResponseBody>)` → `hub.subscribe(Arc::new(on_output))`; register in `generate_handler!`; `attach_session` returns `Result<tauri::ipc::Response, String>` wrapping the drained bytes)

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces for the frontend spec: command `subscribe_pty_output({ onOutput: Channel<ArrayBuffer> })`; frame layout per Global Constraints; `attach_session` → `ArrayBuffer`; `pty-exit` event unchanged.

- [ ] **Step 1: Implement** the edits above. Keep `is_attached`-related call sites compiling via `batcher.is_attached()`.
- [ ] **Step 2: Verify** `cd src-tauri && cargo test && cargo clippy -- -D warnings` — all pass, no warnings; `grep -rn '"pty-output"' src-tauri/src` → no matches.
- [ ] **Step 3: Commit** `feat(pty): stream output over binary IPC channel; raw attach replay` (include the spec file).
- [ ] **Step 4: Note for frontend session** — app is non-functional on this branch until the frontend spec is implemented; do not merge.
