---
name: OpenCode Status via Plugin
description: OpenCode session status from OpenCode server events through a Sessonix plugin that writes the shared status file, auto-installed into ~/.config/opencode/plugins/
targets:
  - ../src-tauri/resources/opencode-status-plugin.js
  - ../src-tauri/src/hooks.rs
  - ../src-tauri/src/lib.rs
  - ../src-tauri/src/session_manager.rs
---

# OpenCode Status via Plugin

OpenCode sessions get status from terminal patterns only (`adapters/opencode.rs`). OpenCode has no shell hooks; it loads JS/TS plugins that receive server events. A small Sessonix plugin writes the same status file the hook handler writes for other agents.

Reference (Context7 `/anomalyco/opencode`: plugins.mdx, plugins/migrate-v1.mdx, plugins/server.mdx, schema/session-status-event.ts, as of 2026-10-29). **Not verified locally** — OpenCode is not installed on the dev machine.

## OpenCode facts relied on

- Global plugins: files in `~/.config/opencode/plugins/`, loaded at startup.
- Two plugin APIs: V1 (`server()` / exported async function returning `{ event: async ({ event }) => … }`) and V2 (default export `{ id, setup(ctx) }`, events via `ctx.event.subscribe({ signal })`). V1 implementations don't run in V2; one module may carry both (`server()` for V1, `id`/`setup` for V2).
- Events: `session.status` (`sessionID`, `status.type`: `busy` | `retry` | `idle`), `session.idle` (deprecated), `session.error`, `permission.asked`, `permission.replied`. Subagents run as further sessions in the same process.

## Plugin behaviour (`resources/opencode-status-plugin.js`, plain JS, no imports beyond `node:fs`/`node:path`/`node:os`)

- No-op unless `process.env.SESSONIX_PTY_ID` is set and `process.env.SESSONIX_AGENT === "opencode"`.
- Tracks the set of busy session IDs: `session.status` busy/retry adds, idle removes. Status = `running` while any session is busy (a finishing subagent doesn't end the turn); `idle` when the set becomes empty.
- `permission.asked` → `waiting_permission`; `permission.replied` → `running`.
- `session.error` → `error` (message from the event's error name, else `agent error`) only when no other session is busy (a failing subagent doesn't end the parent's turn); the error stays over the `idle` that follows until the next busy.
- `session.status` / `session.idle` events without a `sessionID` are ignored.
- Writes `~/.sessonix/hooks/<pty_id>.json` in the shared format (`event`, `status`, `session_id`, `pty_id`, `ts`, `ts_ms`, optional `error`) via temp file + rename; any exception is swallowed (never breaks OpenCode).
- Exports both API shapes from one default export: `{ id: "sessonix-status", setup(ctx), server() }`.
- `[@test] ../src/__tests__/opencodeStatusPlugin.test.ts` (Vitest runs the plugin's event logic with a fake event stream and temp HOME).

## Installation

- Sessonix writes the plugin to `~/.config/opencode/plugins/sessonix-status.js` (directory created) when an OpenCode session is created, only if the content differs (temp + rename); at app start it updates an existing outdated copy. Failures logged only.
- First install only when `opencode --version` reports 1.18.29 or newer — the first version that loads object plugin entrypoints (migrate-v1.mdx); older loaders call every export as a function and would fail to load the plugin in every OpenCode run.
  `[@test] ../src-tauri/src/hooks.rs`
- `get_session_status`: OpenCode uses the hook status file first, terminal patterns otherwise.

## Known limitations

- Unverified against a real OpenCode; if the plugin API differs, OpenCode logs a plugin load error and status falls back to terminal patterns.
- Esc interrupt: expected to surface as `session.status` idle; unconfirmed.

## Out of scope

Frontend UI, Windows, other agents.
