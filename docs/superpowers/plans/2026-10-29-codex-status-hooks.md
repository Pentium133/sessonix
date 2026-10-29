# Codex Status via Hooks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Codex sessions get hook-driven status through the shared handler, with hooks auto-installed into `~/.codex/hooks.json`.

**Architecture:** The shared handler learns `Interrupt`/`PostCompact` and stops `SessionStart` from downgrading `running`. `hooks.rs` generalizes merge/complete/install over an event list and a target file; Codex gets its own event list and path. `get_session_status` routes Codex through the same hook-status mapping as Claude; `create_session` and app setup install/upgrade.

**Tech Stack:** Rust, bash + python3 handler.

**Spec:** `specs/codex-status-hooks.spec.md`

## Global Constraints

- Branch `feat/codex-status-hooks` (from `fix/claude-status-hooks`).
- Codex hooks file: `$CODEX_HOME/hooks.json` if `CODEX_HOME` is set, else `~/.codex/hooks.json`.
- Codex events: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PermissionRequest`, `SubagentStart`, `PreCompact`, `PostCompact`, `Stop`, `Interrupt`, `SessionEnd`; all `async: true`, no matcher.
- Never touch Codex trust state; never pass `--dangerously-bypass-hook-trust`.
- clippy `-D warnings` clean.

## Review Focus

- User's existing `~/.codex/hooks.json` with own hooks → kept, ours added once. Task 2 test `codex_merge_keeps_user_hooks`.
- No `~/.codex` dir yet → file created. Task 2 test `install_into_creates_missing_file`.
- Codex subagent `SessionStart` mid-turn → stays running. Task 1 test `handler_session_start_does_not_override_running`.
- Late tool heartbeat after `Interrupt` → stays idle. Task 1 test `handler_late_tool_event_does_not_override_fresh_interrupt`.

---

### Task 1: Handler

**Files:** `src-tauri/resources/hook-handler.sh`; tests in `src-tauri/src/hooks.rs`
- [ ] Tests: `handler_maps_interrupt_to_idle`, `handler_maps_post_compact_to_running`, `handler_session_start_does_not_override_running` (existing running file → stays running), `handler_session_start_sets_idle_when_not_running` (existing idle/absent → idle), `handler_late_tool_event_does_not_override_fresh_interrupt`.
- [ ] RED → map `Interrupt`→idle, `PostCompact`→running; `SessionStart` (non-compact) exits without writing when existing status is `running`; guard set gains `Interrupt` → GREEN → commit.

### Task 2: Generic installer + Codex

**Files:** `src-tauri/src/hooks.rs`
- **Produces:** `fn merge_hooks(settings: &mut Value, events: &[(&str, bool)]) -> Result<bool, String>`, `fn hooks_complete(settings: &Value, events: &[(&str, bool)]) -> bool`, `fn install_into(path: &Path, events: &[(&str, bool)]) -> Result<bool, String>` (read-or-`{}`, merge, atomic write with mtime guard, create parent dir), `const CODEX_HOOK_EVENTS`, `fn codex_hooks_path() -> Option<PathBuf>`, `pub fn install_codex_hooks() -> Result<bool, String>` (install_into + handler script), `pub fn upgrade_codex_if_installed() -> Result<bool, String>`. Claude's `install_hooks`/`check_installed`/`upgrade_if_installed` use the generic functions with `HOOK_EVENTS`. Windows: Codex fns are no-ops returning `Ok(false)`.
- [ ] Tests: existing Claude tests pass `HOOK_EVENTS`; `codex_merge_into_empty`, `codex_merge_keeps_user_hooks`, `install_into_creates_missing_file`, `install_into_is_idempotent` → RED → implement → GREEN → commit.

### Task 3: Wiring

**Files:** `src-tauri/src/lib.rs`, `src-tauri/src/session_manager.rs`
- [ ] Extract `fn hook_agent_status(hook: &hooks::HookStatus) -> types::AgentStatus` (the existing Claude mapping minus the interrupt check) — test `hook_agent_status_maps_error` in lib.rs tests; Claude branch uses it after its interrupt check; Codex: `if agent_type == "codex"` and hook present → `hook_agent_status`.
- [ ] `create_session`: when `is_codex`, `hooks::install_codex_hooks()`; on error `log::warn!`.
- [ ] setup: `hooks::upgrade_codex_if_installed()` next to the Claude upgrade.
- [ ] Full `cargo test` + clippy → commit.
