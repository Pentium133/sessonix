---
name: Codex Status via Hooks
description: Codex CLI session status from Codex hook events through the shared Sessonix hook handler, with automatic hook installation
targets:
  - ../src-tauri/resources/hook-handler.sh
  - ../src-tauri/src/hooks.rs
  - ../src-tauri/src/lib.rs
  - ../src-tauri/src/session_manager.rs
---

# Codex Status via Hooks

Codex sessions currently get their status only from terminal pattern matching (`adapters/codex.rs`), which is unreliable. Codex CLI has Claude-style command hooks; this spec feeds them into the same status file and staleness rules as Claude (`claude-status-hooks.spec.md`).

Reference (as of 2026-10-29): learn.chatgpt.com/docs/hooks (redirect from developers.openai.com/codex/hooks); Codex source via Context7 `/openai/codex` (`codex-rs/hooks/src/engine/discovery.rs`). Verified locally with codex-cli 0.164.0 in a throwaway `CODEX_HOME`: hooks inherit the parent environment (`SESSONIX_PTY_ID` arrives), stdin JSON carries `hook_event_name`, `session_id`, `turn_id`, `source`; on an API error no `Stop` fires.

## Codex hook facts relied on

- User hooks live in `~/.codex/hooks.json` (`{"hooks": {Event: [{"matcher"?, "hooks": [{"type": "command", "command", "async"?, "timeout"?}]}]}}`), enabled by default.
- Events: `SessionStart` (session or subagent start; `source`), `SubagentStart`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `SubagentStop`, `Stop`, `Interrupt` (user interrupts an active main-thread turn), `SessionEnd`. No `Notification`, no `StopFailure`.
- Non-managed hooks run only after the user trusts them in Codex (`/hooks`); trust is a hash of the hook definition stored by Codex. Until then Codex skips them and prints a warning at startup.

## Event → status (shared hook-handler.sh)

- New mappings: `Interrupt` → `idle`; `PostCompact` → `running`. Codex `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `SubagentStart`, `PreCompact`, `PermissionRequest`, `Stop`, `SessionEnd` use the existing mapping.
  `[@test] ../src-tauri/src/hooks.rs`
- `SessionStart` never overrides a `running` status (Codex fires it for subagents mid-turn). Claude behaviour is unchanged by this: Claude's mid-turn `SessionStart` (compact) already maps to `running`.
  `[@test] ../src-tauri/src/hooks.rs`
- `Interrupt` counts as a turn end for the late-heartbeat guard, like `Stop`: a tool event whose handler finishes within 2 s after it does not overwrite it.
  `[@test] ../src-tauri/src/hooks.rs`
- The handler stays silent on stdout and exits 0 (valid for every Codex event, including `Stop`).

## Status in `get_session_status`

- Codex sessions use the hook status file first, with the same mapping and staleness as Claude (`running` 10 min; settled states never stale). Without a valid file (hooks not installed or not yet trusted), terminal pattern matching as today.
- No JSONL / interrupt-marker check for Codex (`Interrupt` event covers it).

## Installation

- `~/.codex/hooks.json` gets a Sessonix entry (command `bash ~/.sessonix/hook-handler.sh`, `async: true`, no matcher) for: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PermissionRequest`, `SubagentStart`, `PreCompact`, `PostCompact`, `Stop`, `Interrupt`, `SessionEnd`.
  `[@test] ../src-tauri/src/hooks.rs`
- Same merge rules as Claude: idempotent, key order preserved, other hooks kept, malformed shapes refused, atomic write that refuses if the file changed since read; created if missing.
  `[@test] ../src-tauri/src/hooks.rs`
- Installed automatically (backend, unix only) when a Codex session is created, and upgraded at app start if Sessonix Codex hooks are already present. Failures are logged, never block session creation. The handler script is the shared one.
- Sessonix does not touch Codex's trust state and does not pass `--dangerously-bypass-hook-trust`. The user trusts the hooks once via `/hooks` in Codex. Trust is a hash of the hook definition in `hooks.json` (command, async, …), not of the handler script, so handler updates need no re-trust; only a changed definition does (per `version_for_toml` in Codex source; confidence: medium).

## Known limitations

- Until trusted in Codex, status falls back to terminal patterns.
- An API error ends the turn without any hook event → status stays `running` until the next event or the 10-minute staleness.
- No permission notification beyond `PermissionRequest` (no Codex `Notification` event).

## Out of scope

- UI hint that Codex hooks await trust (frontend).
- Gemini, Cursor, OpenCode — separate specs.
- Windows (no bash handler; unchanged).
