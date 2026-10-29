---
name: Cursor Status via Hooks
description: Cursor agent session status from Cursor hook events through the shared Sessonix hook handler, auto-installed into ~/.cursor/hooks.json
targets:
  - ../src-tauri/resources/hook-handler.sh
  - ../src-tauri/src/hooks.rs
  - ../src-tauri/src/lib.rs
  - ../src-tauri/src/session_manager.rs
---

# Cursor Status via Hooks

Cursor sessions get status from terminal patterns only (`adapters/cursor.rs`). Cursor has command hooks; feed the safe, observational ones into the shared status file.

Reference (Context7 `/websites/cursor` → cursor.com/docs/hooks, as of 2026-10-29). **Not verified locally** — the Cursor CLI is not installed on the dev machine, and the Cursor CLI docs (`/websites/cursor_cli`) do not mention hooks: whether `cursor-agent` runs them is unconfirmed.

## Cursor hook facts relied on

- User hooks: `~/.cursor/hooks.json` = `{"version": 1, "hooks": {event: [{"command", "type"?, "matcher"?, "timeout"?, "failClosed"?, "loop_limit"?}]}}` — entries are hook objects directly (no nested `hooks` array).
- Agent events: `sessionStart`, `sessionEnd`, `beforeSubmitPrompt`, `preToolUse`, `postToolUse`, `postToolUseFailure`, `subagentStart`, `subagentStop`, `beforeShellExecution`, `afterShellExecution`, `beforeMCPExecution`, `afterMCPExecution`, `beforeReadFile`, `afterFileEdit`, `preCompact`, `stop` (`status`: `completed` | `aborted` | `error`), `afterAgentResponse`, `afterAgentThought`.
- Common input: `conversation_id`, `generation_id`, `hook_event_name`, `workspace_roots`, …
- Exit 0 → JSON output is used. For permission hooks (`preToolUse`, `beforeShellExecution`, `beforeMCPExecution`, `beforeReadFile`, `subagentStart`) invalid JSON blocks the action; other non-2 exit codes fail open.

## Safety rule

- Sessonix never registers a permission hook (list above): empty output would block the action, and answering `allow` would override the user's permissions.
  `[@test] ../src-tauri/src/hooks.rs`

## Event → status (shared handler, agent `cursor`)

- `beforeSubmitPrompt` → `running`; the handler prints `{"continue": true}`.
- `postToolUse`, `postToolUseFailure`, `afterShellExecution`, `afterMCPExecution`, `afterFileEdit`, `preCompact` → `running` (heartbeats).
- `stop`: `completed` / `aborted` → `idle`; `error` → `error` (message `agent error`). Counts as a turn end for the late-heartbeat guard.
- `sessionStart` → `idle`; `sessionEnd` → `exited`.
  `[@test] ../src-tauri/src/hooks.rs`
- For every other agent-`cursor` event the handler prints `{}`; it always exits 0.
  `[@test] ../src-tauri/src/hooks.rs`

## Installation

- `~/.cursor/hooks.json` gets one flat Sessonix entry per event above (`{"command": "bash ~/.sessonix/hook-handler.sh cursor"}`), `"version": 1` set if absent. Merge rules as before (idempotent, order preserved, others kept, malformed refused, atomic, mtime guard, created if missing), adapted to the flat entry shape.
  `[@test] ../src-tauri/src/hooks.rs`
- Auto-installed (unix) when a Cursor session is created; upgraded at app start if present. Failures logged only.
- `get_session_status`: Cursor uses the hook status file first, terminal patterns otherwise.
- Cursor can also run Claude Code hooks from `~/.claude/settings.json`; agent scoping (`SESSONIX_AGENT=cursor` vs `claude`) makes the handler ignore those.

## Known limitations

- Unverified; CLI hook support unconfirmed (if unsupported, terminal fallback continues).
- No permission-wait status (no observational permission event).

## Out of scope

Frontend, Windows, other agents.
