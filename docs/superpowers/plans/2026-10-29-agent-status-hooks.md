# Gemini / Cursor / OpenCode Status Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Hook/plugin-driven status for Gemini, Cursor and OpenCode, auto-installed like Claude/Codex.

**Specs:** `specs/gemini-status-hooks.spec.md`, `specs/cursor-status-hooks.spec.md`, `specs/opencode-status-plugin.spec.md`

## Global Constraints

- Branch `feat/agent-status-hooks` (from `feat/codex-status-hooks`).
- Handler always runs python so Gemini/Cursor get their stdout response even outside Sessonix (`SESSONIX_PTY_ID` unset) or on agent mismatch; the status file is written only when allowed.
- Gemini stdout `{}`; Cursor `{"continue": true}` for `beforeSubmitPrompt`, else `{}`; Claude/Codex print nothing.
- Cursor: never register `preToolUse`, `beforeShellExecution`, `beforeMCPExecution`, `beforeReadFile`, `subagentStart`.
- Paths: `~/.gemini/settings.json`; `~/.cursor/hooks.json`; `$XDG_CONFIG_HOME/opencode/plugins/sessonix-status.js` (default `~/.config`).

## Review Focus

- Gemini/Cursor hooks firing outside Sessonix must still print valid JSON. Task 1/2 tests `*_outside_sessonix_prints_json`.
- Cursor flat format merge keeps user entries and `version`. Task 2 `cursor_merge_keeps_user_hooks`.
- OpenCode subagent idle while parent busy → still running. Task 3 `subagent idle keeps running`.

### Task 1: Gemini — handler mapping + output, `GEMINI_HOOK_EVENTS` (sync), install/upgrade into nested format. Tests: mapping, `{}` output, outside-Sessonix output, merge.
### Task 2: Cursor — handler mapping (`stop.status`), outputs, flat-format merge (`merge_flat_hooks`, `flat_hooks_complete`), install/upgrade, safety test on event list.
### Task 3: OpenCode — `resources/opencode-status-plugin.js` (busy set, permission, error; V1 `server` + V2 `setup`), Vitest `src/__tests__/opencodeStatusPlugin.test.ts`; Rust install/upgrade via write-if-changed.
### Task 4: Wiring — `get_session_status` hook-first for gemini/cursor/opencode; `create_session` installs per agent; setup upgrades. Full cargo test + clippy + vitest + tsc.
