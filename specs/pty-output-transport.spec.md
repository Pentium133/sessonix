---
name: PTY Output Transport
description: Backend delivery of PTY output to the frontend as batched binary frames over a Tauri IPC channel, including attach replay
targets:
  - ../src-tauri/src/pty_output.rs
  - ../src-tauri/src/pty_manager.rs
  - ../src-tauri/src/session_manager.rs
  - ../src-tauri/src/lib.rs
---

# PTY Output Transport (backend)

Today each `read()` from a PTY (up to 4 KiB) is sent as a separate `pty-output` Tauri event with the bytes serialized as a JSON array of numbers (`pty_manager.rs` reader thread). That inflates payloads ~3–4×, costs serialization on the Rust side and parsing on the JS side, and sends one IPC message per tiny chunk while agents stream tokens and spinners. `attach_session` returns up to 1 MiB of buffered output the same way (≈4 MB of JSON per session switch).

This spec moves live output to a binary Tauri `Channel`, coalesces small chunks, and delivers attach replay on the same channel so it can never be overtaken by live output. It covers the backend only; the frontend consumer is a separate spec written after this one ships.

## Scope

In scope:
- New IPC command to subscribe to output over a channel; binary frame format.
- Coalescing of output chunks per session before sending.
- Correct interaction of coalescing with detach / attach / exit.
- Attach replay delivered on the output channel.
- Removing the `pty-output` event.

Out of scope:
- Frontend changes (`usePtyOutput`, `api.ts`, `sessionStore`) — separate frontend spec.
- Input path (`write_to_session`), resize, ring buffer size, `last_lines` status tracking — unchanged.
- `pty-exit` stays a Tauri event.
- Multi-window delivery (single main window).

## Subscription

- New command `subscribe_pty_output(on_output: Channel<InvokeResponseBody>)` stores the channel in app state. A later call replaces the previous channel (frontend reload / HMR re-subscribes).
  `[@test] ../src-tauri/src/pty_output.rs`
- With no subscribed channel, output frames are dropped (same as an event with no listener today); the ring buffer still records them.
  `[@test] ../src-tauri/src/pty_output.rs`
- If sending on the channel fails, the frame is dropped and the reader thread keeps running.

## Frame format

- Each message is `InvokeResponseBody::Raw`: 4-byte little-endian `u32` session (PTY) id, followed by the output bytes. No other framing.
  `[@test] ../src-tauri/src/pty_output.rs`
- A frame never mixes output from two sessions.
- Payload bytes are passed through unchanged (no UTF-8 decoding, split multi-byte sequences are fine — xterm reassembles them).

## Coalescing

- Output read from a session's PTY is appended to that session's pending batch instead of being sent immediately.
- A batch is sent when it reaches 64 KiB, or 4 ms after the first byte entered the empty batch — whichever comes first. Constants live in `pty_output.rs`.
  `[@test] ../src-tauri/src/pty_output.rs`
- Byte order within a session is preserved across batches.
  `[@test] ../src-tauri/src/pty_output.rs`
- Ring buffer and `last_lines` are still written immediately on every read, before coalescing (status detection and detach recovery do not wait for a flush).
- A batch is only sent if the session is attached at send time.

## Detach / attach / exit

- `detach` sends the session's pending batch (if attached) before marking the session detached and draining the ring buffer. Otherwise those bytes would be discarded from the ring buffer without ever reaching the terminal.
  `[@test] ../src-tauri/src/pty_output.rs`
- `detach` on an already detached session is a no-op: output recorded while detached stays in the ring buffer for the next attach.
  `[@test] ../src-tauri/src/pty_output.rs`
- Output recorded while detached goes only to the ring buffer, never to the pending batch (otherwise attach would replay it and the flusher would send it again).
  `[@test] ../src-tauri/src/pty_output.rs`
- `attach_session` returns nothing. If the session was detached, the drained ring buffer is sent as one frame on the output channel, under the same lock as live output, so it reaches the frontend before any output recorded after the attach. No frame when the buffer is empty or the session was already attached.
  `[@test] ../src-tauri/src/pty_output.rs`
- On PTY EOF the pending batch is sent before `pty-exit` is emitted.
  `[@test] ../src-tauri/src/pty_output.rs`
- Session kill drops any pending batch; no frame for a session is sent after its `pty-exit`.
- The batch is closed only after the child process was killed successfully; if the kill fails, the session keeps delivering output.

## Rollout

- `attach_session` no longer returns the replay (the current frontend writes the returned bytes) and live output stops arriving as events, so the backend change does not work with the current frontend. Backend and frontend land on the same feature branch, which is merged only after the frontend spec is implemented.
