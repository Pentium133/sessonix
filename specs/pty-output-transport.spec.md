---
name: PTY Output Transport
description: Backend delivery of PTY output to the frontend as batched binary frames over a Tauri IPC channel, and binary replay on attach
targets:
  - ../src-tauri/src/pty_output.rs
  - ../src-tauri/src/pty_manager.rs
  - ../src-tauri/src/session_manager.rs
  - ../src-tauri/src/lib.rs
---

# PTY Output Transport (backend)

Today each `read()` from a PTY (up to 4 KiB) is sent as a separate `pty-output` Tauri event with the bytes serialized as a JSON array of numbers (`pty_manager.rs` reader thread). That inflates payloads ~3–4×, costs serialization on the Rust side and parsing on the JS side, and sends one IPC message per tiny chunk while agents stream tokens and spinners. `attach_session` returns up to 1 MiB of buffered output the same way (≈4 MB of JSON per session switch).

This spec moves live output to a binary Tauri `Channel`, coalesces small chunks, and returns attach replay as raw bytes. It covers the backend only; the frontend consumer is a separate spec written after this one ships.

## Scope

In scope:
- New IPC command to subscribe to output over a channel; binary frame format.
- Coalescing of output chunks per session before sending.
- Correct interaction of coalescing with detach / attach / exit.
- `attach_session` returning raw bytes.
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
- `attach_session` returns the drained ring buffer as raw bytes (`tauri::ipc::Response`), not `Vec<u8>` serialized to JSON. Empty when the session was already attached.
- On PTY EOF the pending batch is sent before `pty-exit` is emitted.
  `[@test] ../src-tauri/src/pty_output.rs`
- Session kill drops any pending batch; no frame for a session is sent after its `pty-exit`.

## Rollout

- `attach_session`'s return type changes (the current frontend reads `.length` on the result) and live output stops arriving as events, so the backend change does not work with the current frontend. Backend and frontend land on the same feature branch, which is merged only after the frontend spec is implemented.
