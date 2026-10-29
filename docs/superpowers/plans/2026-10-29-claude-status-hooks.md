# Claude Status via Hooks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Claude session status follows the current Claude Code hook events, so the running dot stays on during long turns and "Ready for input" fires only when the turn really ended.

**Architecture:** The bash/python hook handler maps more events (tool heartbeats, StopFailure, Notification by `notification_type`). `hooks.rs` gets pure, testable helpers for staleness and settings merging; `jsonl.rs` learns the user-interrupt marker; `get_session_status` combines them; app setup auto-upgrades outdated installs.

**Tech Stack:** Rust, bash + python3 handler, serde_json, tempfile (dev-dep, already present).

**Spec:** `specs/claude-status-hooks.spec.md`

## Global Constraints

- Branch `fix/claude-status-hooks` from `main` (no upstream on main).
- `running` max age 600 s; `idle`/`exited`/`error`/`waiting_permission` never stale.
- Interrupt marker: user text block starting with `[Request interrupted by user`.
- `PreCompact` sync; every other Sessonix hook `async: true`; Sessonix `Notification` entry has no matcher.
- `cd src-tauri && ~/.cargo/bin/cargo clippy --all-targets -- -D warnings` clean.

## Review Focus

- Existing settings.json with old Sessonix entries + other tools' hooks → upgrade adds missing events, drops the old Notification matcher, keeps others untouched, no duplicates. Task 4 test `upgrade_old_install`.
- Upgrade runs twice → second run changes nothing. Task 4 test `merge_is_idempotent`.
- Hook file with unknown status string → treated like running (existing default), not a crash. Task 2 test `unknown_status_uses_running_age`.
- Interrupt followed by a new prompt → not idle (new user entry after marker). Task 3 test `interrupt_then_new_prompt_not_interrupted`.

---

### Task 1: Handler event mapping

**Files:** Modify `src-tauri/resources/hook-handler.sh`; Test: `src-tauri/src/hooks.rs` (unix-only test module)

- [ ] **Step 1: Failing tests** — helper `run_handler(input: serde_json::Value) -> Option<serde_json::Value>` runs `bash <bundled script>` with `HOME=<tempdir>`, `SESSONIX_PTY_ID=42`, stdin = input; returns parsed `<tempdir>/.sessonix/hooks/42.json`. Tests (`handler_maps_*`): `PreToolUse`→`running`; `StopFailure` with `error:"rate_limit"`→`status:"error"`, `error:"rate_limit"`; `Notification` `notification_type:"permission_prompt"`→`waiting_permission`; `notification_type:"idle_prompt"`→`idle`; `notification_type:"auth_success"`→ no file; `Stop`→`idle`.
- [ ] **Step 2:** `cargo test hooks::` → FAIL.
- [ ] **Step 3:** Update the python `status_map` per spec; Notification uses `data.get('notification_type')`; output adds `error` (string or omitted).
- [ ] **Step 4:** `cargo test hooks::` → PASS. **Step 5:** commit `fix(hooks): map current Claude Code hook events to session status`.

### Task 2: Staleness

**Files:** Modify `src-tauri/src/hooks.rs`

- **Produces:** `fn parse_hook_status(content: &str, now: i64) -> Option<HookStatus>`; `read_hook_status` calls it. `HookStatus` gains `#[serde(default)] pub error: Option<String>`.
- [ ] Tests: `running_fresh_within_10_min` (age 599 → Some), `running_stale_after_10_min` (601 → None), `idle_never_stale` / `waiting_permission_never_stale` / `error_never_stale` (age 86400 → Some), `unknown_status_uses_running_age` (601 → None). RED → implement → GREEN → commit `fix(hooks): status staleness per spec`.

### Task 3: JSONL interrupt + tool_use

**Files:** Modify `src-tauri/src/jsonl.rs`, `src-tauri/src/lib.rs`

- **Produces:** `pub fn ends_with_interrupt(path: &PathBuf) -> bool` (last user/assistant entry in tail is a user interrupt). `detect_status`: interrupt → `Idle`; `stop_reason:"tool_use"` → `Active`.
- [ ] Tests: rename `test_detect_status_waiting_permission` → `test_detect_status_tool_use_is_active` expecting `Active`; `test_detect_status_interrupt_is_idle` (both marker variants); `interrupt_then_new_prompt_not_interrupted` (`ends_with_interrupt` false when a later user text entry exists); `ends_with_interrupt_true`.
- [ ] `get_session_status` (Claude): hook `running` + JSONL path known + `ends_with_interrupt` → Idle "Waiting for input"; hook `error` → `SessionStatus::Error`, `"API error: <error>"` (`"API error"` when absent). No unit test (needs Tauri state); covered by helper tests.
- [ ] RED → implement → GREEN → commit `fix(status): detect user interrupt; tool_use means active`.

### Task 4: Install / upgrade

**Files:** Modify `src-tauri/src/hooks.rs`, `src-tauri/src/lib.rs`

- **Produces:** `const HOOK_EVENTS: &[(&str, bool /*async*/)]` (spec list incl. `Notification`); `fn merge_hooks(settings: &mut serde_json::Value) -> bool` (changed?); `fn hooks_complete(settings: &serde_json::Value) -> bool`; `pub fn upgrade_if_installed() -> Result<bool, String>` (no-op unless `is_installed`; merges + rewrites script when settings incomplete or script differs). `install_hooks` always merges + writes script; `check_installed` = complete && script matches bundled.
- [ ] Tests: `merge_into_empty_adds_all_events`, `upgrade_old_install` (fixture: current user's old layout + an `obsidian` SessionStart hook → all events present once, Notification Sessonix entry without `matcher`, other hook kept), `merge_is_idempotent` (second call returns false), `hooks_complete_false_when_event_missing`.
- [ ] `lib.rs` setup: call `hooks::upgrade_if_installed()` once, log errors (`log::warn!`), never fail startup.
- [ ] RED → implement → GREEN; full `cargo test` + clippy → commit `fix(hooks): idempotent install that upgrades existing hooks`.
