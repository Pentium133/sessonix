---
name: Claude Status via Hooks
description: Claude Code session status (running / idle / waiting / error) derived from Claude Code hook events, aligned with the hooks reference as of 2026-10
targets:
  - ../src-tauri/resources/hook-handler.sh
  - ../src-tauri/src/hooks.rs
  - ../src-tauri/src/jsonl.rs
  - ../src-tauri/src/lib.rs
---

# Claude Status via Hooks

Sessonix shows each Claude session's state (green dot = running, yellow = idle / waiting, red = error) and sends OS notifications on transitions (`useStatusPolling.ts`: running → idle = "Ready for input", → permission = "Waiting for permission"). For Claude the state comes from a hook handler that Claude Code runs on hook events, writing `~/.sessonix/hooks/<pty_id>.json`; the backend reads it in `get_session_status`, falling back to JSONL tail heuristics, then terminal patterns.

Reference: Claude Code hooks docs (code.claude.com/docs/en/hooks, hooks-guide), fetched via Context7 `/websites/code_claude` on 2026-10-29.

## Problem (observed)

- Green dot goes out while Claude is still working; "Ready for input" arrives when only a step finished.
- Root causes found:
  1. Hook `running` expires after 30 s, but between `UserPromptSubmit` and `Stop` Sessonix subscribes to no events, so any turn longer than 30 s falls back to JSONL, where `stop_reason: "tool_use"` (a tool currently executing) is read as "waiting for permission" → idle. Result: dot turns yellow mid-turn and a false notification fires.
  2. Hook `idle` expires after 5 min → fallback heuristics can report running for a session that is idle.
  3. The handler reads a non-existent `matcher` field from Notification input; the docs field is `notification_type`. Notification events are silently ignored.
  4. `StopFailure` (turn ended by API error — fires instead of `Stop`) is not handled → stays running.
  5. After a permission prompt is answered nothing marks the session running again until the next prompt; a `Stop` that another Stop hook overrides (Claude keeps working) leaves the session idle.
  6. `install_hooks` returns early if any Sessonix hook exists, so "Reinstall" never adds new events and the handler script is never updated for existing users.

## Event → status mapping (hook-handler.sh)

- `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PermissionDenied`, `SubagentStart`, `PreCompact` → `running`. Tool events act as a heartbeat during long turns and re-mark running after a permission answer or an overridden Stop.
  `[@test] ../src-tauri/src/hooks.rs`
- `SessionStart`, `Stop` → `idle`.
- `StopFailure` → `error`, with the `error` field (e.g. `rate_limit`) as the message.
- `PermissionRequest` → `waiting_permission`.
- `Notification` by `notification_type`: `permission_prompt`, `elicitation_dialog`, `elicitation_url_dialog`, `agent_needs_input` → `waiting_permission`; `idle_prompt` → `idle`; other types ignored.
- `SessionEnd` → `exited`.
- Unknown events → file untouched.
- Every handled event rewrites the file with the current `ts` (heartbeat).

## Staleness (hooks.rs `read_hook_status`)

- `idle`, `exited`, `error`, `waiting_permission`: never stale — they change only on the next event (a permission prompt can legitimately wait for hours).
  `[@test] ../src-tauri/src/hooks.rs`
- `running`: valid for 10 minutes since the last event (tool heartbeats keep long turns fresh; covers a single tool call up to 10 min).
  `[@test] ../src-tauri/src/hooks.rs`
- User interrupt (Esc) fires no event. If the hook says `running` and the session's JSONL tail ends with a user-interrupt entry (`[Request interrupted by user`), the status is `idle`.
  `[@test] ../src-tauri/src/jsonl.rs`

## Status returned to the UI (`get_session_status`, Claude)

- `running` → Running, "Working..."; `idle` → Idle, "Waiting for input"; `waiting_permission` → Idle, "Waiting for permission"; `error` → Error, `"API error: <error>"`; `exited` → Running, "Session ending..." (unchanged).

## JSONL fallback (no hook file: Windows, hooks not installed)

- An assistant entry with `stop_reason: "tool_use"` means a tool is running → Active (was: WaitingPermission). Permission waits are reported by hooks only.
  `[@test] ../src-tauri/src/jsonl.rs`
- A trailing user-interrupt entry → Idle.
  `[@test] ../src-tauri/src/jsonl.rs`

## Installation and upgrade

- `install_hooks` is idempotent and complete: adds every event above that lacks the Sessonix command, replaces the matcher on an existing Sessonix `Notification` entry (old `permission_prompt|elicitation_dialog`) with no matcher (handler filters by `notification_type`), always rewrites the handler script. Other tools' hooks are preserved.
  `[@test] ../src-tauri/src/hooks.rs`
- `check_installed` is true only when all events are present and the installed script equals the bundled one ("Reinstall" then actually upgrades).
  `[@test] ../src-tauri/src/hooks.rs`
- On app start, if Sessonix hooks are present but outdated, they are upgraded automatically (the user consented on first install). Never installs from scratch without the user.
- Sync/async: `PreCompact` stays sync; all others async.

## Out of scope

- Notification wording and the frontend transition logic in `useStatusPolling.ts` (unchanged; it starts receiving correct states).
- Other agents (Codex, Gemini, …) — their adapters are unchanged.
- Windows hook support (still JSONL-only).
