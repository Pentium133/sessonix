---
name: Gemini Status via Hooks
description: Gemini CLI session status from Gemini hook events through the shared Sessonix hook handler, auto-installed into ~/.gemini/settings.json
targets:
  - ../src-tauri/resources/hook-handler.sh
  - ../src-tauri/src/hooks.rs
  - ../src-tauri/src/lib.rs
  - ../src-tauri/src/session_manager.rs
---

# Gemini Status via Hooks

Gemini sessions get status from terminal patterns only (`adapters/gemini.rs`). Gemini CLI has command hooks; feed them into the shared status file like Claude and Codex (`claude-status-hooks.spec.md`, `codex-status-hooks.spec.md`).

Reference (Context7 `/google-gemini/gemini-cli`, docs/hooks/*, docs/reference/configuration.md, as of 2026-10-29). **Not verified locally** — Gemini CLI is not installed on the dev machine.

## Gemini hook facts relied on

- Hooks live under `hooks` in `~/.gemini/settings.json`: `{Event: [{"matcher"?, "hooks": [{"name"?, "type": "command", "command", "timeout"?}]}]}`; enabled by default (`hooksConfig.enabled`, default `true`).
- Events: `SessionStart` (startup/resume/clear), `SessionEnd`, `BeforeAgent` (after prompt submit), `AfterAgent` (agent loop ends), `BeforeModel`, `AfterModel`, `BeforeToolSelection`, `BeforeTool`, `AfterTool`, `PreCompress`, `Notification` (`notification_type: "ToolPermission"`).
- Input: JSON on stdin with `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `timestamp`.
- Exit 0 → stdout parsed as JSON; stdout must contain nothing but that JSON.
- Environment is passed through unless the user enables `security.environmentVariableRedaction` (off by default).
- No documented async option: hooks run synchronously.

## Event → status (shared handler, agent `gemini`)

- `BeforeAgent`, `BeforeTool`, `AfterTool`, `PreCompress` → `running` (tool events are heartbeats).
- `AfterAgent` → `idle`; counts as a turn end for the late-heartbeat guard.
- `Notification` with `notification_type: "ToolPermission"` → `waiting_permission`.
- `SessionStart` → `idle`; `SessionEnd` → `exited`.
  `[@test] ../src-tauri/src/hooks.rs`
- For agent `gemini` the handler prints exactly `{}` on stdout and exits 0, for every event (handled or not).
  `[@test] ../src-tauri/src/hooks.rs`

## Installation

- `~/.gemini/settings.json` gets a Sessonix entry (command `bash ~/.sessonix/hook-handler.sh gemini`, no matcher, no `async`) for: `SessionStart`, `BeforeAgent`, `BeforeTool`, `AfterTool`, `PreCompress`, `AfterAgent`, `Notification`, `SessionEnd`. Same merge rules as Claude/Codex (idempotent, order preserved, others kept, malformed refused, atomic write, mtime guard; file and directory created if missing).
  `[@test] ../src-tauri/src/hooks.rs`
- Auto-installed (unix) when a Gemini session is created; upgraded at app start if present. Failures logged only.
- `get_session_status`: Gemini uses the hook status file first, terminal patterns otherwise.

## Known limitations

- Unverified against a real Gemini CLI.
- Esc interrupt: no documented event → `running` until the next event or 10-minute staleness.
- With env redaction enabled, `SESSONIX_PTY_ID` may not reach the hook → terminal fallback.
- Sync hooks add one bash+python start (~30–50 ms) per tool call.

## Out of scope

Frontend, Windows, other agents.
