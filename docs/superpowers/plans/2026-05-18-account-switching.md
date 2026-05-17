# Account Switching for Claude and Codex — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-session selection and live-session hot-swap of Claude / Codex
accounts (isolated `CLAUDE_CONFIG_DIR` / `CODEX_HOME`) to AICoder.

**Architecture:** A new `accounts` table holds `(agent_type, name, config_dir)`.
`sessions` gets a nullable `account_id` (FK ON DELETE SET NULL). At spawn,
`PtyManager` accepts `extra_env` which is merged after the existing whitelist
loop. `SessionManager` resolves `account_id` → `(env_key, canonical_path)` pair
via a server-side `env_key_for_agent` constant. Frontend gains an
`accountStore`, a badge in `TerminalPane`, a confirm dialog, and an
`AccountsManager` section in `SettingsModal`. Switching on a live session is
`kill + create_session(..., replace_id)` — reuses the existing `replaceId`
plumbing.

**Tech Stack:** Rust (rusqlite, portable_pty, dunce, tauri 2), TypeScript
(Zustand, React, Vitest + RTL).

**Spec:** `docs/superpowers/specs/2026-05-18-account-switching-design.md`

**Project naming note:** The codebase identifies itself as `sessonix` in
paths (DB file `sessonix.db`, app dir `com.sessonix.app`, GitHub repo
`Pentium133/sessonix`) but the product is "AICoder" in user-facing text.
Paths use the on-disk name.

---

## File Structure

### Created

**Backend (Rust):**
- `src-tauri/src/accounts/mod.rs` — module root, re-exports
- `src-tauri/src/accounts/detect.rs` — auto-detection of `~/.claude*` / `~/.codex*`

**Frontend (TypeScript / React):**
- `src/store/accountStore.ts`
- `src/components/AccountBadge.tsx`
- `src/components/AccountSwitchDialog.tsx`
- `src/components/AccountsManager.tsx`
- `src/store/__tests__/accountStore.test.ts`
- `src/components/__tests__/AccountBadge.test.tsx`
- `src/components/__tests__/AccountsManager.test.tsx`
- `src/hooks/__tests__/useSessionActions.switchAccount.test.ts`

### Modified

**Backend:**
- `src-tauri/src/db.rs` — accounts table + migration adds `sessions.account_id` + AccountRow + CRUD
- `src-tauri/src/error.rs` — 3 new AppError variants
- `src-tauri/src/adapters/mod.rs` — `env` field on `LaunchConfig`; `env_key_for_agent` constant
- `src-tauri/src/pty_manager.rs` — `extra_env` parameter on `create_session`; merge loop
- `src-tauri/src/session_manager.rs` — `account_id` on `CreateSessionParams`; env resolution
- `src-tauri/src/types.rs` — `account_id: Option<i64>` on `CreateSessionRequest`
- `src-tauri/src/lib.rs` — six new IPC commands; register accounts module; thread `account_id` through `create_session`

**Frontend:**
- `src/lib/types.ts` — `AccountInfo`, `DetectedAccount`; `account_id` on `Session`
- `src/lib/api.ts` — wrappers for the six new IPC commands; `account_id` on `CreateSessionRequest`
- `src/store/sessionStore.ts` — `account_id` plumbed through `restore` and `addSession`
- `src/components/TerminalPane.tsx` — render `AccountBadge` in header
- `src/components/SessionLauncher.tsx` — account dropdown
- `src/components/SettingsModal.tsx` — Accounts section
- `src/hooks/useSessionActions.ts` — `switchSessionAccount` action

---

## Task 1: DB schema — `accounts` table + `sessions.account_id` migration

**Files:**
- Modify: `src-tauri/src/db.rs:111-...` (the `migrate` function)

- [ ] **Step 1: Write failing migration tests**

Append to the end of `src-tauri/src/db.rs` (inside an existing or new `#[cfg(test)] mod migration_tests`):

```rust
#[cfg(test)]
mod accounts_migration_tests {
    use super::Db;
    use rusqlite::Connection;

    #[test]
    fn migrate_creates_accounts_table_with_unique_indexes() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock();

        // Columns exist
        conn.prepare(
            "SELECT id, agent_type, name, config_dir, sort_order, created_at FROM accounts LIMIT 0",
        )
        .expect("accounts table should exist with all columns");

        // UNIQUE(agent_type, name)
        conn.execute(
            "INSERT INTO accounts (agent_type, name, config_dir) VALUES ('claude','work','/a')",
            [],
        )
        .unwrap();
        let err = conn
            .execute(
                "INSERT INTO accounts (agent_type, name, config_dir) VALUES ('claude','work','/b')",
                [],
            )
            .expect_err("duplicate (agent_type,name) must fail");
        assert!(err.to_string().to_lowercase().contains("unique"));

        // UNIQUE(agent_type, config_dir)
        let err = conn
            .execute(
                "INSERT INTO accounts (agent_type, name, config_dir) VALUES ('claude','other','/a')",
                [],
            )
            .expect_err("duplicate (agent_type,config_dir) must fail");
        assert!(err.to_string().to_lowercase().contains("unique"));
    }

    #[test]
    fn migrate_adds_account_id_column_to_sessions() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn.lock();
        conn.prepare("SELECT account_id FROM sessions LIMIT 0")
            .expect("sessions.account_id should exist");
    }

    #[test]
    fn deleting_account_sets_session_account_id_to_null() {
        let db = Db::open_in_memory().unwrap();
        let project_id = db.insert_project("p", "/tmp/p").unwrap();
        // Pre-insert an account and a session referencing it via raw SQL
        // (insert_account / InsertSession changes land in Task 2 and Task 7).
        let conn = db.conn.lock();
        conn.execute(
            "INSERT INTO accounts (id, agent_type, name, config_dir) VALUES (42, 'claude', 'work', '/tmp/cdir')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions
               (project_id, pty_id, agent_type, task_name, working_dir, launch_command, account_id)
             VALUES (?1, 1, 'claude', 't', '/tmp/p', 'claude', 42)",
            rusqlite::params![project_id],
        )
        .unwrap();
        conn.execute("DELETE FROM accounts WHERE id = 42", []).unwrap();

        let account_id: Option<i64> = conn
            .query_row(
                "SELECT account_id FROM sessions WHERE pty_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(account_id, None, "FK SET NULL must clear session.account_id");
    }
}
```

- [ ] **Step 2: Verify tests fail**

Run: `cd src-tauri && cargo test accounts_migration_tests -- --nocapture`
Expected: all three FAIL — `no such table: accounts`, `no such column: account_id`, etc.

- [ ] **Step 3: Add migrations to `Db::migrate`**

In `src-tauri/src/db.rs`, inside `fn migrate(&self)`, after the existing `tasks` table block and before the closing of the function, add:

```rust
// --- Accounts table (Claude/Codex configDir isolation) ---
conn.execute_batch(
    "CREATE TABLE IF NOT EXISTS accounts (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        agent_type  TEXT NOT NULL,
        name        TEXT NOT NULL,
        config_dir  TEXT NOT NULL,
        sort_order  INTEGER NOT NULL DEFAULT 0,
        created_at  TEXT NOT NULL DEFAULT (datetime('now')),
        UNIQUE(agent_type, name),
        UNIQUE(agent_type, config_dir)
    );",
)?;

// Migration: add account_id to sessions
let has_account_id_col: bool = conn
    .prepare("SELECT account_id FROM sessions LIMIT 0")
    .is_ok();
if !has_account_id_col {
    conn.execute_batch(
        "ALTER TABLE sessions ADD COLUMN account_id INTEGER REFERENCES accounts(id) ON DELETE SET NULL;",
    )?;
}
```

The existing `PRAGMA foreign_keys = ON;` at the top of `migrate` makes
`ON DELETE SET NULL` effective.

- [ ] **Step 4: Verify tests pass**

Run: `cd src-tauri && cargo test accounts_migration_tests -- --nocapture`
Expected: all three PASS.

- [ ] **Step 5: Run full test suite (regression check)**

Run: `cd src-tauri && cargo test`
Expected: all pre-existing tests still pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/db.rs
git commit -m "$(cat <<'EOF'
db: add accounts table + sessions.account_id migration

Two UNIQUE constraints prevent duplicate (agent_type, name) and
(agent_type, config_dir) entries. account_id on sessions has
ON DELETE SET NULL so removing an account preserves historical rows.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Account CRUD on `Db`

**Files:**
- Modify: `src-tauri/src/db.rs`

- [ ] **Step 1: Write failing CRUD tests**

Append to `src-tauri/src/db.rs`:

```rust
#[cfg(test)]
mod account_crud_tests {
    use super::Db;

    #[test]
    fn insert_account_returns_id_and_list_finds_it() {
        let db = Db::open_in_memory().unwrap();
        let id = db.insert_account("claude", "work", "/tmp/cdir-1").unwrap();
        assert!(id > 0);

        let all = db.list_accounts().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, id);
        assert_eq!(all[0].agent_type, "claude");
        assert_eq!(all[0].name, "work");
        assert_eq!(all[0].config_dir, "/tmp/cdir-1");
    }

    #[test]
    fn list_by_agent_filters_and_sorts() {
        let db = Db::open_in_memory().unwrap();
        let claude_b = db.insert_account("claude", "b", "/tmp/b").unwrap();
        let claude_a = db.insert_account("claude", "a", "/tmp/a").unwrap();
        let _codex = db.insert_account("codex", "c", "/tmp/c").unwrap();
        // Make 'a' sort after 'b'
        db.reorder_account(claude_b, 0).unwrap();
        db.reorder_account(claude_a, 1).unwrap();

        let only_claude = db.list_accounts_by_agent("claude").unwrap();
        assert_eq!(only_claude.len(), 2);
        assert_eq!(only_claude[0].id, claude_b);
        assert_eq!(only_claude[1].id, claude_a);
    }

    #[test]
    fn get_account_by_id_returns_some_and_none() {
        let db = Db::open_in_memory().unwrap();
        let id = db.insert_account("claude", "work", "/tmp/x").unwrap();
        let row = db.get_account_by_id(id).unwrap().unwrap();
        assert_eq!(row.name, "work");
        assert!(db.get_account_by_id(99999).unwrap().is_none());
    }

    #[test]
    fn delete_account_removes_row() {
        let db = Db::open_in_memory().unwrap();
        let id = db.insert_account("claude", "work", "/tmp/x").unwrap();
        db.delete_account(id).unwrap();
        assert!(db.get_account_by_id(id).unwrap().is_none());
    }

    #[test]
    fn update_account_name_changes_name() {
        let db = Db::open_in_memory().unwrap();
        let id = db.insert_account("claude", "work", "/tmp/x").unwrap();
        db.update_account_name(id, "renamed").unwrap();
        let row = db.get_account_by_id(id).unwrap().unwrap();
        assert_eq!(row.name, "renamed");
    }

    #[test]
    fn duplicate_name_per_agent_rejected() {
        let db = Db::open_in_memory().unwrap();
        db.insert_account("claude", "work", "/tmp/a").unwrap();
        assert!(db.insert_account("claude", "work", "/tmp/b").is_err());
        // Same name allowed for a different agent_type
        db.insert_account("codex", "work", "/tmp/c").unwrap();
    }

    #[test]
    fn duplicate_config_dir_per_agent_rejected() {
        let db = Db::open_in_memory().unwrap();
        db.insert_account("claude", "a", "/tmp/x").unwrap();
        assert!(db.insert_account("claude", "b", "/tmp/x").is_err());
    }
}
```

- [ ] **Step 2: Verify tests fail**

Run: `cd src-tauri && cargo test account_crud_tests`
Expected: compile errors — methods not defined.

- [ ] **Step 3: Add `AccountRow` and CRUD methods**

Near the other row structs (around `db.rs:9-19`, after `ProjectRow`), add:

```rust
#[derive(Debug, Clone)]
pub struct AccountRow {
    pub id: i64,
    pub agent_type: String,
    pub name: String,
    pub config_dir: String,
    pub sort_order: u32,
}
```

In `impl Db` (anywhere — convention: near the end of the impl block), add:

```rust
pub fn insert_account(
    &self,
    agent_type: &str,
    name: &str,
    config_dir: &str,
) -> Result<i64, rusqlite::Error> {
    let conn = self.conn.lock();
    // sort_order = MAX+1 within the agent_type so new accounts append.
    let next_order: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM accounts WHERE agent_type = ?1",
        params![agent_type],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO accounts (agent_type, name, config_dir, sort_order)
         VALUES (?1, ?2, ?3, ?4)",
        params![agent_type, name, config_dir, next_order],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_accounts(&self) -> Result<Vec<AccountRow>, rusqlite::Error> {
    let conn = self.conn.lock();
    let mut stmt = conn.prepare(
        "SELECT id, agent_type, name, config_dir, sort_order
         FROM accounts
         ORDER BY agent_type ASC, sort_order ASC, id ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(AccountRow {
                id: row.get(0)?,
                agent_type: row.get(1)?,
                name: row.get(2)?,
                config_dir: row.get(3)?,
                sort_order: row.get::<_, i64>(4)? as u32,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn list_accounts_by_agent(
    &self,
    agent_type: &str,
) -> Result<Vec<AccountRow>, rusqlite::Error> {
    let conn = self.conn.lock();
    let mut stmt = conn.prepare(
        "SELECT id, agent_type, name, config_dir, sort_order
         FROM accounts
         WHERE agent_type = ?1
         ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt
        .query_map(params![agent_type], |row| {
            Ok(AccountRow {
                id: row.get(0)?,
                agent_type: row.get(1)?,
                name: row.get(2)?,
                config_dir: row.get(3)?,
                sort_order: row.get::<_, i64>(4)? as u32,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_account_by_id(&self, id: i64) -> Result<Option<AccountRow>, rusqlite::Error> {
    let conn = self.conn.lock();
    let result = conn.query_row(
        "SELECT id, agent_type, name, config_dir, sort_order
         FROM accounts WHERE id = ?1",
        params![id],
        |row| {
            Ok(AccountRow {
                id: row.get(0)?,
                agent_type: row.get(1)?,
                name: row.get(2)?,
                config_dir: row.get(3)?,
                sort_order: row.get::<_, i64>(4)? as u32,
            })
        },
    );
    match result {
        Ok(r) => Ok(Some(r)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn delete_account(&self, id: i64) -> Result<(), rusqlite::Error> {
    let conn = self.conn.lock();
    conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn update_account_name(&self, id: i64, name: &str) -> Result<(), rusqlite::Error> {
    let conn = self.conn.lock();
    conn.execute(
        "UPDATE accounts SET name = ?1 WHERE id = ?2",
        params![name, id],
    )?;
    Ok(())
}

pub fn reorder_account(&self, id: i64, sort_order: i64) -> Result<(), rusqlite::Error> {
    let conn = self.conn.lock();
    conn.execute(
        "UPDATE accounts SET sort_order = ?1 WHERE id = ?2",
        params![sort_order, id],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Verify tests pass**

Run: `cd src-tauri && cargo test account_crud_tests`
Expected: all 7 tests PASS.

- [ ] **Step 5: Clippy + full test suite**

Run: `cd src-tauri && cargo clippy -- -D warnings && cargo test`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/db.rs
git commit -m "$(cat <<'EOF'
db: account CRUD (insert/list/get/delete/rename/reorder)

AccountRow struct + 7 methods. sort_order auto-assigned per
agent_type on insert; UNIQUE constraints from Task 1 enforce
distinctness.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: New `AppError` variants

**Files:**
- Modify: `src-tauri/src/error.rs`

- [ ] **Step 1: Write failing display test**

Append to `src-tauri/src/error.rs`:

```rust
#[cfg(test)]
mod account_error_tests {
    use super::AppError;

    #[test]
    fn account_not_found_formats_with_id() {
        let e = AppError::AccountNotFound(42);
        assert_eq!(e.to_string(), "account 42 not found");
    }

    #[test]
    fn invalid_agent_for_account_includes_type() {
        let e = AppError::InvalidAgentForAccount("shell".into());
        assert!(e.to_string().contains("shell"));
    }

    #[test]
    fn account_config_dir_missing_includes_path() {
        let e = AppError::AccountConfigDirMissing("/tmp/nope".into());
        assert!(e.to_string().contains("/tmp/nope"));
    }
}
```

- [ ] **Step 2: Verify tests fail to compile**

Run: `cd src-tauri && cargo test account_error_tests`
Expected: errors — variants don't exist.

- [ ] **Step 3: Add variants**

Modify `src-tauri/src/error.rs`. Replace the `AppError` enum and `Display` impl:

```rust
#[derive(Debug)]
pub enum AppError {
    Pty(String),
    Db(String),
    SessionNotFound(u32),
    AdapterNotFound(String),
    Io(std::io::Error),
    AccountNotFound(i64),
    InvalidAgentForAccount(String),
    AccountConfigDirMissing(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Pty(msg) => write!(f, "pty: {msg}"),
            AppError::Db(msg) => write!(f, "db: {msg}"),
            AppError::SessionNotFound(id) => write!(f, "session {id} not found"),
            AppError::AdapterNotFound(name) => write!(f, "agent adapter '{name}' not found"),
            AppError::Io(e) => write!(f, "io: {e}"),
            AppError::AccountNotFound(id) => write!(f, "account {id} not found"),
            AppError::InvalidAgentForAccount(at) => {
                write!(f, "agent type '{at}' does not support accounts")
            }
            AppError::AccountConfigDirMissing(p) => {
                write!(f, "account configDir does not exist: {p}")
            }
        }
    }
}
```

- [ ] **Step 4: Verify tests pass**

Run: `cd src-tauri && cargo test account_error_tests`
Expected: 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/error.rs
git commit -m "$(cat <<'EOF'
error: add AccountNotFound, InvalidAgentForAccount, AccountConfigDirMissing

Used by SessionManager when resolving account_id at spawn and by
Db CRUD callers in the IPC layer.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `env_key_for_agent` + `env` on `LaunchConfig`

**Files:**
- Modify: `src-tauri/src/adapters/mod.rs`

- [ ] **Step 1: Write failing test**

Append to the existing `#[cfg(test)] mod tests` block in `src-tauri/src/adapters/mod.rs`:

```rust
#[test]
fn env_key_for_claude_is_claude_config_dir() {
    assert_eq!(env_key_for_agent("claude"), Some("CLAUDE_CONFIG_DIR"));
}

#[test]
fn env_key_for_codex_is_codex_home() {
    assert_eq!(env_key_for_agent("codex"), Some("CODEX_HOME"));
}

#[test]
fn env_key_for_unsupported_returns_none() {
    assert_eq!(env_key_for_agent("shell"), None);
    assert_eq!(env_key_for_agent("cursor"), None);
    assert_eq!(env_key_for_agent("gemini"), None);
    assert_eq!(env_key_for_agent("opencode"), None);
}
```

- [ ] **Step 2: Verify failure**

Run: `cd src-tauri && cargo test env_key_for`
Expected: compile error — `env_key_for_agent` not defined.

- [ ] **Step 3: Add function + `env` field**

In `src-tauri/src/adapters/mod.rs`:

Add field to `LaunchConfig`:

```rust
#[allow(dead_code)]
pub struct LaunchConfig {
    pub working_dir: String,
    pub prompt: Option<String>,
    pub extra_args: Vec<String>,
    pub env: HashMap<String, String>,
}
```

Add public function just before `pub trait AgentAdapter`:

```rust
/// Map an agent type to its account-isolation env variable.
/// Only Claude and Codex are supported in this iteration.
/// Returns `None` for agents whose accounts AICoder doesn't manage.
pub fn env_key_for_agent(agent_type: &str) -> Option<&'static str> {
    match agent_type {
        "claude" => Some("CLAUDE_CONFIG_DIR"),
        "codex"  => Some("CODEX_HOME"),
        _        => None,
    }
}
```

Update each adapter's test constructor of `LaunchConfig` to include
`env: HashMap::new()`. Files to grep first to locate every site:

```bash
cd src-tauri && grep -rn "LaunchConfig {" src/adapters/
```

Add `env: HashMap::new(),` as the last field in every construction (Claude,
Codex, Cursor, Gemini, OpenCode, Shell adapter tests).

- [ ] **Step 4: Verify tests pass**

Run: `cd src-tauri && cargo test`
Expected: new tests pass; existing adapter tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/adapters/
git commit -m "$(cat <<'EOF'
adapters: env_key_for_agent + env on LaunchConfig

env_key_for_agent maps claude→CLAUDE_CONFIG_DIR and codex→CODEX_HOME.
Returns None for agents whose accounts we don't manage (shell, cursor,
gemini, opencode).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: `PtyManager::create_session` accepts `extra_env`

**Files:**
- Modify: `src-tauri/src/pty_manager.rs`

- [ ] **Step 1: Write failing test**

Add to `src-tauri/src/pty_manager.rs` inside the `#[cfg(test)] mod tests` block (or a new sibling test module if signatures get unwieldy):

```rust
#[test]
fn extra_env_is_passed_to_child_process() {
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::collections::HashMap;

    // Use `printenv` to read a custom env var into the PTY output. This is a
    // tiny integration: open PTY, spawn `printenv CLAUDE_CONFIG_DIR`, expect
    // the configured value to appear in stdout.
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
        .unwrap();

    let mut cmd = CommandBuilder::new("printenv");
    cmd.arg("CLAUDE_CONFIG_DIR");
    cmd.cwd("/tmp");
    let mut env = HashMap::new();
    env.insert("CLAUDE_CONFIG_DIR".to_string(), "/tmp/test-cdir-xyz".to_string());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut buf = [0u8; 256];
    let mut out = String::new();
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 { break; }
        out.push_str(&String::from_utf8_lossy(&buf[..n]));
        if out.contains("test-cdir-xyz") { break; }
    }
    let _ = child.wait();
    assert!(
        out.contains("/tmp/test-cdir-xyz"),
        "child must see injected CLAUDE_CONFIG_DIR; got: {:?}", out
    );
}

#[test]
fn extra_env_overrides_parent_when_keys_collide() {
    // Set a parent-env value that the whitelist accepts (e.g. LANG), then
    // pass the same key via extra_env and confirm the override wins.
    std::env::set_var("LANG", "parent_value");

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
        .unwrap();

    let mut cmd = CommandBuilder::new("printenv");
    cmd.arg("LANG");
    cmd.cwd("/tmp");
    // Simulate the whitelist loop:
    for (key, value) in std::env::vars() {
        if super::is_safe_env_key(&key) {
            cmd.env(key, value);
        }
    }
    cmd.env("LANG", "extra_env_wins");

    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut buf = [0u8; 128];
    let mut out = String::new();
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 { break; }
        out.push_str(&String::from_utf8_lossy(&buf[..n]));
        if out.contains("extra_env_wins") || out.contains("parent_value") { break; }
    }
    let _ = child.wait();
    assert!(out.contains("extra_env_wins"), "extra_env must win; got: {:?}", out);
    assert!(!out.contains("parent_value"));
}
```

These tests prove the *behaviour we want from `PtyManager::create_session`*
even before changing its signature — they shake out at the `CommandBuilder`
level. The signature-level test is exercised by Task 7 (SessionManager).

- [ ] **Step 2: Verify tests fail**

Run: `cd src-tauri && cargo test --lib pty_manager`
Expected: `extra_env_is_passed_to_child_process` and
`extra_env_overrides_parent_when_keys_collide` may pass on their own (they
construct CommandBuilder directly) — but they assert the behaviour the
production code must keep when we change the signature. If they pass now,
good — that means the underlying portable_pty mechanism behaves as
expected. **Mark this step as "PASS expected — confirming portable_pty
semantics."**

- [ ] **Step 3: Change `PtyManager::create_session` signature**

In `src-tauri/src/pty_manager.rs`, modify the function signature:

```rust
pub fn create_session(
    &self,
    command: &str,
    args: &[String],
    working_dir: &str,
    cols: u16,
    rows: u16,
    extra_env: HashMap<String, String>,
    app_handle: AppHandle,
) -> Result<u32, AppError> {
```

Inside the body, after the existing whitelist loop (which ends with the
line `cmd.env("TERM", "xterm-256color");` and before the
`SESSONIX_PTY_ID` line), append:

```rust
// Per-session env injection. Placed AFTER the whitelist so caller-supplied
// keys win on collision (e.g. CLAUDE_CONFIG_DIR from account selection
// overrides a parent shell that happened to export the same name).
for (key, value) in extra_env {
    cmd.env(key, value);
}
```

- [ ] **Step 4: Update existing callers**

There is one caller: `SessionManager::create_session` in
`src-tauri/src/session_manager.rs:249`. Change the call to:

```rust
let pty_id = self.pty.create_session(
    params.command,
    &args,
    pty_cwd,
    params.cols,
    params.rows,
    HashMap::new(),                 // extra_env — wired up in Task 6
    params.app_handle,
)?;
```

Add `use std::collections::HashMap;` to the imports at the top of
`session_manager.rs` if not present.

- [ ] **Step 5: Verify build + tests**

Run: `cd src-tauri && cargo test`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/pty_manager.rs src-tauri/src/session_manager.rs
git commit -m "$(cat <<'EOF'
pty_manager: accept extra_env, merge after whitelist

Per-session env-var injection for account isolation. Whitelist is
unchanged — extra_env is the new opt-in channel that overrides on
key collision. Existing SessionManager caller passes empty map;
account-driven env wiring lands in Task 6.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: `SessionManager` — resolve `account_id` to env, persist on session

**Files:**
- Modify: `src-tauri/src/session_manager.rs`
- Modify: `src-tauri/src/db.rs` (`InsertSession` gets `account_id` field)

- [ ] **Step 1: Write failing test for env resolution**

In `src-tauri/src/session_manager.rs`, add inside `#[cfg(test)] mod task_worktree_resolution_tests` (or a new sibling module `account_env_resolution_tests`):

```rust
#[cfg(test)]
mod account_env_resolution_tests {
    use super::*;
    use crate::db::Db;
    use std::sync::Arc;

    fn fresh_db() -> Arc<Db> {
        Arc::new(Db::open_in_memory().expect("open in-memory db"))
    }

    #[test]
    fn resolves_claude_account_to_claude_config_dir() {
        let db = fresh_db();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().to_string_lossy().to_string();
        let id = db.insert_account("claude", "work", &path).unwrap();

        let env = SessionManager::resolve_account_env(&db, Some(id)).unwrap();
        assert_eq!(env.len(), 1);
        let v = env.get("CLAUDE_CONFIG_DIR").expect("must inject CLAUDE_CONFIG_DIR");
        // Path is canonicalised so we compare against the canonical form.
        let canon = dunce::canonicalize(&path).unwrap();
        assert_eq!(v, &canon.to_string_lossy().to_string());
    }

    #[test]
    fn resolves_codex_account_to_codex_home() {
        let db = fresh_db();
        let tmp = tempfile::tempdir().unwrap();
        let id = db
            .insert_account("codex", "personal", &tmp.path().to_string_lossy())
            .unwrap();
        let env = SessionManager::resolve_account_env(&db, Some(id)).unwrap();
        assert!(env.contains_key("CODEX_HOME"));
    }

    #[test]
    fn none_account_id_returns_empty_env() {
        let db = fresh_db();
        let env = SessionManager::resolve_account_env(&db, None).unwrap();
        assert!(env.is_empty());
    }

    #[test]
    fn missing_account_returns_account_not_found() {
        let db = fresh_db();
        let err = SessionManager::resolve_account_env(&db, Some(9999)).unwrap_err();
        assert!(matches!(err, AppError::AccountNotFound(9999)));
    }

    #[test]
    fn account_for_unsupported_agent_returns_invalid_agent() {
        let db = fresh_db();
        // Bypass the (future) IPC-level guard and insert a raw "shell"
        // account row — exercises the defence-in-depth check at spawn.
        let tmp = tempfile::tempdir().unwrap();
        let id = db
            .insert_account("shell", "bogus", &tmp.path().to_string_lossy())
            .unwrap();
        let err = SessionManager::resolve_account_env(&db, Some(id)).unwrap_err();
        assert!(matches!(err, AppError::InvalidAgentForAccount(_)));
    }

    #[test]
    fn missing_config_dir_returns_account_config_dir_missing() {
        let db = fresh_db();
        let id = db.insert_account("claude", "ghost", "/tmp/does-not-exist-xyz-aicoder").unwrap();
        let err = SessionManager::resolve_account_env(&db, Some(id)).unwrap_err();
        assert!(matches!(err, AppError::AccountConfigDirMissing(_)));
    }
}
```

- [ ] **Step 2: Verify failure**

Run: `cd src-tauri && cargo test account_env_resolution_tests`
Expected: compile error — `SessionManager::resolve_account_env` undefined.

- [ ] **Step 3: Add `account_id` to `CreateSessionParams` + `InsertSession`**

In `src-tauri/src/session_manager.rs` extend `CreateSessionParams`:

```rust
pub struct CreateSessionParams<'a> {
    pub command: &'a str,
    pub args: &'a [String],
    pub working_dir: &'a str,
    pub cols: u16,
    pub rows: u16,
    pub app_handle: AppHandle,
    pub task_name: &'a str,
    pub agent_type: &'a str,
    pub worktree_path: Option<&'a str>,
    pub base_commit: Option<&'a str>,
    pub prompt: Option<&'a str>,
    pub task_id: Option<i64>,
    pub account_id: Option<i64>,
}
```

In `src-tauri/src/db.rs`, extend `InsertSession`:

```rust
pub struct InsertSession<'a> {
    pub project_id: i64,
    pub pty_id: u32,
    pub agent_type: &'a str,
    pub task_name: &'a str,
    pub working_dir: &'a str,
    pub command: &'a str,
    pub args: &'a str,
    pub agent_session_id: Option<&'a str>,
    pub worktree_path: Option<&'a str>,
    pub base_commit: Option<&'a str>,
    pub initial_prompt: Option<&'a str>,
    pub task_id: Option<i64>,
    pub account_id: Option<i64>,
}
```

Grep for `insert_session` to find its implementation in `db.rs`, then add
`account_id` to the INSERT column list, the placeholders, and the params
binding. Schematically (locate by `INSERT INTO sessions`):

```rust
conn.execute(
    "INSERT INTO sessions (
        project_id, pty_id, agent_type, task_name, working_dir,
        launch_command, launch_args, agent_session_id,
        worktree_path, base_commit, initial_prompt, task_id, account_id
     ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
    params![
        s.project_id, s.pty_id, s.agent_type, s.task_name, s.working_dir,
        s.command, s.args, s.agent_session_id,
        s.worktree_path, s.base_commit, s.initial_prompt, s.task_id, s.account_id,
    ],
)?;
```

Also extend `SessionRow` and the row-mapper used by `list_sessions_by_project_path`
and friends to include `account_id: Option<i64>`. Grep for `SessionRow {` to find
every site that constructs it.

- [ ] **Step 4: Implement `resolve_account_env` + wire it into `create_session`**

In `src-tauri/src/session_manager.rs`, add to `impl SessionManager`:

```rust
/// Resolve `account_id` to the env-var pair the spawned process must see.
/// Extracted so it can be unit-tested without spawning a PTY.
pub(crate) fn resolve_account_env(
    db: &Db,
    account_id: Option<i64>,
) -> Result<std::collections::HashMap<String, String>, AppError> {
    let Some(aid) = account_id else {
        return Ok(std::collections::HashMap::new());
    };
    let acc = db
        .get_account_by_id(aid)
        .map_err(|e| AppError::Db(e.to_string()))?
        .ok_or(AppError::AccountNotFound(aid))?;
    let key = crate::adapters::env_key_for_agent(&acc.agent_type)
        .ok_or_else(|| AppError::InvalidAgentForAccount(acc.agent_type.clone()))?;
    let canonical = dunce::canonicalize(&acc.config_dir)
        .map_err(|_| AppError::AccountConfigDirMissing(acc.config_dir.clone()))?;
    Ok(std::collections::HashMap::from([(
        key.to_string(),
        canonical.to_string_lossy().into_owned(),
    )]))
}
```

In `SessionManager::create_session`, replace the existing PTY-spawn block:

```rust
let pty_id = self.pty.create_session(
    params.command,
    &args,
    pty_cwd,
    params.cols,
    params.rows,
    HashMap::new(),
    params.app_handle,
)?;
```

with:

```rust
let extra_env = Self::resolve_account_env(&self.db, params.account_id)?;
let pty_id = self.pty.create_session(
    params.command,
    &args,
    pty_cwd,
    params.cols,
    params.rows,
    extra_env,
    params.app_handle,
)?;
```

Then in the `self.db.insert_session(...)` call lower down, add
`account_id: params.account_id,` as the final field.

- [ ] **Step 5: Verify tests pass**

Run: `cd src-tauri && cargo test account_env_resolution_tests && cargo test --lib`
Expected: all PASS.

- [ ] **Step 6: Clippy**

Run: `cd src-tauri && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/
git commit -m "$(cat <<'EOF'
session_manager: resolve account_id to per-session env at spawn

CreateSessionParams gains account_id; resolved server-side to
(CLAUDE_CONFIG_DIR | CODEX_HOME, canonical_path) and passed as
extra_env to PtyManager. InsertSession + SessionRow gain the
column. Validation produces AccountNotFound /
InvalidAgentForAccount / AccountConfigDirMissing.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Auto-detect module — `accounts/detect.rs`

**Files:**
- Create: `src-tauri/src/accounts/mod.rs`
- Create: `src-tauri/src/accounts/detect.rs`
- Modify: `src-tauri/src/lib.rs` (declare module)

- [ ] **Step 1: Write failing tests**

Create `src-tauri/src/accounts/detect.rs` with this content (tests included; implementation stubs):

```rust
//! Auto-detection of pre-existing Claude/Codex configDirs in the user's
//! home directory. Returns candidates the user can add as accounts; never
//! mutates anything on disk.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct DetectedAccount {
    pub agent_type: String,
    pub config_dir: String,
    pub suggested_name: String,
}

/// Scan `$HOME` shallow (depth 1) for directories matching
/// `^\.claude.*` / `^\.codex.*` that contain a known marker file.
/// Caller passes `already_registered` (canonical absolute paths) to filter
/// duplicates so the UI never proposes adding the same configDir twice.
pub fn detect_accounts(
    home: &Path,
    already_registered: &[String],
) -> Vec<DetectedAccount> {
    todo!()
}

/// Strip the leading dot + the "claude-" / "codex-" prefix from a dirname
/// to produce a friendly default name. `.claude` → "default";
/// `.claude-work` → "work"; `.codex` → "default"; `.codex-personal` → "personal".
pub fn suggest_name(agent_type: &str, dirname: &str) -> String {
    todo!()
}

fn has_claude_marker(p: &Path) -> bool {
    todo!()
}

fn has_codex_marker(p: &Path) -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"").unwrap();
    }

    #[test]
    fn detects_claude_with_credentials_marker() {
        let home = tempfile::tempdir().unwrap();
        touch(&home.path().join(".claude-work").join(".credentials.json"));
        let result = detect_accounts(home.path(), &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].agent_type, "claude");
        assert_eq!(result[0].suggested_name, "work");
        assert!(result[0].config_dir.ends_with(".claude-work"));
    }

    #[test]
    fn detects_claude_with_projects_dir_marker() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".claude-x").join("projects")).unwrap();
        let result = detect_accounts(home.path(), &[]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn detects_default_claude_dir() {
        let home = tempfile::tempdir().unwrap();
        touch(&home.path().join(".claude").join("settings.json"));
        let result = detect_accounts(home.path(), &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].suggested_name, "default");
    }

    #[test]
    fn detects_codex_with_state_db_marker() {
        let home = tempfile::tempdir().unwrap();
        touch(&home.path().join(".codex").join("state_5.sqlite"));
        let result = detect_accounts(home.path(), &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].agent_type, "codex");
        assert_eq!(result[0].suggested_name, "default");
    }

    #[test]
    fn skips_dirs_without_marker_files() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".claude-empty")).unwrap();
        fs::create_dir_all(home.path().join(".codex-empty")).unwrap();
        let result = detect_accounts(home.path(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn excludes_already_registered() {
        let home = tempfile::tempdir().unwrap();
        let claude_dir = home.path().join(".claude-work");
        touch(&claude_dir.join(".credentials.json"));
        let registered = vec![dunce::canonicalize(&claude_dir).unwrap()
            .to_string_lossy()
            .to_string()];
        let result = detect_accounts(home.path(), &registered);
        assert!(result.is_empty());
    }

    #[test]
    fn ignores_non_claude_non_codex_dirs() {
        let home = tempfile::tempdir().unwrap();
        touch(&home.path().join(".config").join("anything"));
        touch(&home.path().join(".bashrc"));
        let result = detect_accounts(home.path(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn suggest_name_strips_prefix() {
        assert_eq!(suggest_name("claude", ".claude"), "default");
        assert_eq!(suggest_name("claude", ".claude-work"), "work");
        assert_eq!(suggest_name("codex", ".codex"), "default");
        assert_eq!(suggest_name("codex", ".codex-personal"), "personal");
    }
}
```

Create `src-tauri/src/accounts/mod.rs`:

```rust
pub mod detect;
```

Declare in `src-tauri/src/lib.rs` near the other `mod` lines (around line 1):

```rust
mod accounts;
```

- [ ] **Step 2: Verify tests fail**

Run: `cd src-tauri && cargo test accounts::detect`
Expected: panics (`todo!()`).

- [ ] **Step 3: Implement**

Replace the stub bodies in `src-tauri/src/accounts/detect.rs`:

```rust
pub fn detect_accounts(
    home: &Path,
    already_registered: &[String],
) -> Vec<DetectedAccount> {
    let registered: std::collections::HashSet<&String> = already_registered.iter().collect();
    let Ok(entries) = std::fs::read_dir(home) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() { continue; }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else { continue };

        let agent_type = if (name == ".claude" || name.starts_with(".claude-"))
            && has_claude_marker(&path)
        {
            "claude"
        } else if (name == ".codex" || name.starts_with(".codex-"))
            && has_codex_marker(&path)
        {
            "codex"
        } else {
            continue;
        };

        let Ok(canonical) = dunce::canonicalize(&path) else { continue };
        let config_dir = canonical.to_string_lossy().to_string();
        if registered.contains(&config_dir) { continue; }

        out.push(DetectedAccount {
            agent_type: agent_type.to_string(),
            config_dir,
            suggested_name: suggest_name(agent_type, name),
        });
    }
    out
}

pub fn suggest_name(agent_type: &str, dirname: &str) -> String {
    let prefix = match agent_type {
        "claude" => ".claude",
        "codex"  => ".codex",
        _ => return dirname.trim_start_matches('.').to_string(),
    };
    if dirname == prefix {
        return "default".to_string();
    }
    dirname
        .strip_prefix(prefix)
        .and_then(|s| s.strip_prefix('-'))
        .unwrap_or(dirname)
        .to_string()
}

fn has_claude_marker(p: &Path) -> bool {
    p.join(".credentials.json").exists()
        || p.join("settings.json").exists()
        || p.join("projects").is_dir()
}

fn has_codex_marker(p: &Path) -> bool {
    p.join("auth.json").exists() || p.join("state_5.sqlite").exists()
}

/// Cross-platform discovery of the user's home directory plus already-registered
/// paths from DB. Thin wrapper used by the IPC layer.
pub fn detect_accounts_with_home(
    already_registered: &[String],
) -> Vec<DetectedAccount> {
    let Some(home) = dirs::home_dir() else { return Vec::new(); };
    detect_accounts(&home, already_registered)
}
```

- [ ] **Step 4: Verify tests pass**

Run: `cd src-tauri && cargo test accounts::detect`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/accounts/ src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
accounts: auto-detect ~/.claude*/~/.codex* configDirs

Shallow scan with marker-file filter (Claude: .credentials.json |
settings.json | projects/; Codex: auth.json | state_5.sqlite).
suggest_name turns '.claude-work' into 'work'; '.claude' becomes
'default'. Already-registered paths are excluded.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: IPC commands — `list_accounts`, `add_account`, `delete_account`, `rename_account`, `detect_accounts_cmd`, `set_session_account`

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/db.rs` (add `set_session_account` method)
- Modify: `src-tauri/src/session_manager.rs` (small helper for tilde expansion)

> **Naming:** the existing `detect_agents` command stays; the new
> account-detection IPC is named `detect_accounts` (no clash).

- [ ] **Step 1: Add `set_session_account` method on `Db`**

In `src-tauri/src/db.rs`, with the other session updaters:

```rust
pub fn set_session_account(
    &self,
    pty_id: u32,
    account_id: Option<i64>,
) -> Result<(), rusqlite::Error> {
    let conn = self.conn.lock();
    conn.execute(
        "UPDATE sessions SET account_id = ?1 WHERE pty_id = ?2",
        params![account_id, pty_id],
    )?;
    Ok(())
}
```

Test (append next to other db tests):

```rust
#[cfg(test)]
mod set_session_account_tests {
    use super::*;
    #[test]
    fn updates_account_id_for_session() {
        let db = Db::open_in_memory().unwrap();
        let project_id = db.insert_project("p", "/tmp/p").unwrap();
        let aid = db.insert_account("claude", "work", "/tmp/cdir").unwrap();
        db.insert_session(&InsertSession {
            project_id, pty_id: 1, agent_type: "claude", task_name: "t",
            working_dir: "/tmp/p", command: "claude", args: "[]",
            agent_session_id: None, worktree_path: None, base_commit: None,
            initial_prompt: None, task_id: None, account_id: None,
        }).unwrap();
        db.set_session_account(1, Some(aid)).unwrap();
        let conn = db.conn.lock();
        let got: Option<i64> = conn.query_row(
            "SELECT account_id FROM sessions WHERE pty_id=1", [], |r| r.get(0),
        ).unwrap();
        assert_eq!(got, Some(aid));
        drop(conn);
        db.set_session_account(1, None).unwrap();
        let conn = db.conn.lock();
        let got: Option<i64> = conn.query_row(
            "SELECT account_id FROM sessions WHERE pty_id=1", [], |r| r.get(0),
        ).unwrap();
        assert_eq!(got, None);
    }
}
```

- [ ] **Step 2: Add tilde-expansion helper**

In `src-tauri/src/accounts/mod.rs`, add:

```rust
pub fn expand_tilde_and_canonicalize(input: &str) -> Result<String, String> {
    let expanded: std::path::PathBuf = if let Some(rest) = input.strip_prefix("~/") {
        let home = dirs::home_dir().ok_or_else(|| "could not resolve home directory".to_string())?;
        home.join(rest)
    } else if input == "~" {
        dirs::home_dir().ok_or_else(|| "could not resolve home directory".to_string())?
    } else {
        std::path::PathBuf::from(input)
    };
    let canonical = dunce::canonicalize(&expanded)
        .map_err(|_| format!("path does not exist: {}", expanded.display()))?;
    if !canonical.is_dir() {
        return Err(format!("not a directory: {}", canonical.display()));
    }
    Ok(canonical.to_string_lossy().into_owned())
}

#[cfg(test)]
mod expand_tests {
    use super::*;

    #[test]
    fn tilde_expands_to_home() {
        let s = expand_tilde_and_canonicalize("~").unwrap();
        assert_eq!(s, dunce::canonicalize(dirs::home_dir().unwrap()).unwrap().to_string_lossy());
    }

    #[test]
    fn relative_unknown_path_errors() {
        let r = expand_tilde_and_canonicalize("/tmp/aicoder-does-not-exist-xyz");
        assert!(r.is_err());
    }

    #[test]
    fn existing_absolute_path_passthrough() {
        let tmp = tempfile::tempdir().unwrap();
        let s = expand_tilde_and_canonicalize(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(s, dunce::canonicalize(tmp.path()).unwrap().to_string_lossy());
    }
}
```

- [ ] **Step 3: Add the six IPC commands in `lib.rs`**

In `src-tauri/src/lib.rs`, near other `#[derive(serde::Serialize)]` IPC
structs:

```rust
#[derive(serde::Serialize)]
struct AccountInfo {
    id: i64,
    agent_type: String,
    name: String,
    config_dir: String,
    sort_order: u32,
}

#[derive(serde::Serialize)]
struct DetectedAccountInfo {
    agent_type: String,
    config_dir: String,
    suggested_name: String,
}

#[tauri::command]
fn list_accounts(state: tauri::State<'_, SessionManager>) -> Result<Vec<AccountInfo>, String> {
    let rows = state.db.list_accounts().map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|r| AccountInfo {
        id: r.id, agent_type: r.agent_type, name: r.name,
        config_dir: r.config_dir, sort_order: r.sort_order,
    }).collect())
}

#[tauri::command]
fn add_account(
    state: tauri::State<'_, SessionManager>,
    agent_type: String,
    name: String,
    config_dir: String,
) -> Result<i64, String> {
    // Guard: only agents we know how to inject env for.
    if adapters::env_key_for_agent(&agent_type).is_none() {
        return Err(format!("agent type '{}' does not support accounts", agent_type));
    }
    let name = name.trim().to_string();
    if name.is_empty() { return Err("name is required".into()); }
    let canonical = accounts::expand_tilde_and_canonicalize(&config_dir)?;
    state.db.insert_account(&agent_type, &name, &canonical)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_account(state: tauri::State<'_, SessionManager>, id: i64) -> Result<(), String> {
    state.db.delete_account(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_account(state: tauri::State<'_, SessionManager>, id: i64, name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() { return Err("name is required".into()); }
    state.db.update_account_name(id, &name).map_err(|e| e.to_string())
}

#[tauri::command]
fn detect_accounts(state: tauri::State<'_, SessionManager>) -> Result<Vec<DetectedAccountInfo>, String> {
    let registered: Vec<String> = state.db.list_accounts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|r| r.config_dir)
        .collect();
    Ok(accounts::detect::detect_accounts_with_home(&registered)
        .into_iter()
        .map(|d| DetectedAccountInfo {
            agent_type: d.agent_type,
            config_dir: d.config_dir,
            suggested_name: d.suggested_name,
        })
        .collect())
}

#[tauri::command]
fn set_session_account(
    state: tauri::State<'_, SessionManager>,
    pty_id: u32,
    account_id: Option<i64>,
) -> Result<(), String> {
    state.db.set_session_account(pty_id, account_id).map_err(|e| e.to_string())
}
```

Register them in `invoke_handler` near the existing entries:

```rust
.invoke_handler(tauri::generate_handler![
    // ...existing entries...
    list_accounts,
    add_account,
    delete_account,
    rename_account,
    detect_accounts,
    set_session_account,
])
```

- [ ] **Step 4: Verify build + tests**

Run: `cd src-tauri && cargo test && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/
git commit -m "$(cat <<'EOF'
ipc: six account commands (list/add/delete/rename/detect/set_session_account)

add_account expands tilde, canonicalises, and rejects agent types
unknown to env_key_for_agent. detect_accounts excludes paths already
in DB. set_session_account updates DB only — relaunch is driven from
the frontend.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Thread `account_id` through `create_session` IPC

**Files:**
- Modify: `src-tauri/src/types.rs`
- Modify: `src-tauri/src/lib.rs` (`create_session` handler)
- Modify: `src-tauri/src/lib.rs` (`SessionInfo` + `list_sessions` mapping)

- [ ] **Step 1: Extend `CreateSessionRequest`**

In `src-tauri/src/types.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: String,
    pub task_name: Option<String>,
    pub agent_type: Option<String>,
    pub worktree_path: Option<String>,
    pub base_commit: Option<String>,
    pub prompt: Option<String>,
    pub task_id: Option<i64>,
    pub account_id: Option<i64>,
}
```

- [ ] **Step 2: Pass it through the handler**

In `src-tauri/src/lib.rs`, in `fn create_session`, add the field:

```rust
.create_session(CreateSessionParams {
    command: &request.command,
    args: &request.args,
    working_dir: &request.working_dir,
    cols: 120,
    rows: 30,
    app_handle: app,
    task_name: request.task_name.as_deref().unwrap_or(&request.command),
    agent_type: request.agent_type.as_deref().unwrap_or("custom"),
    worktree_path: request.worktree_path.as_deref(),
    base_commit: request.base_commit.as_deref(),
    prompt: request.prompt.as_deref(),
    task_id: request.task_id,
    account_id: request.account_id,
})
```

- [ ] **Step 3: Expose `account_id` in `SessionInfo`**

In `src-tauri/src/lib.rs`, add `account_id: Option<i64>` to the
`SessionInfo` struct and to the row→info mapping in `list_sessions`:

```rust
#[derive(serde::Serialize)]
struct SessionInfo {
    // ...existing fields...
    task_id: Option<i64>,
    account_id: Option<i64>,
}
```

```rust
SessionInfo {
    // ...existing fields...
    task_id: s.task_id,
    account_id: s.account_id,
}
```

- [ ] **Step 4: Verify build + tests**

Run: `cd src-tauri && cargo build && cargo test`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/
git commit -m "$(cat <<'EOF'
ipc: thread account_id through create_session + SessionInfo

Optional account_id on CreateSessionRequest; defaults to None
(system default behaviour). SessionInfo gains account_id so the
frontend can render the badge after restore.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Frontend types + API wrappers

**Files:**
- Modify: `src/lib/types.ts`
- Modify: `src/lib/api.ts`

- [ ] **Step 1: Add types**

In `src/lib/types.ts`, add (and modify the existing `Session` interface to
include `account_id`):

```ts
export interface Account {
  id: number;
  agent_type: string;
  name: string;
  config_dir: string;
  sort_order: number;
}

export interface DetectedAccount {
  agent_type: string;
  config_dir: string;
  suggested_name: string;
}
```

Find the existing `Session` interface in this file and add:

```ts
  account_id: number | null;
```

next to the other nullable fields (e.g. near `task_id: number | null`).

- [ ] **Step 2: Add wrappers in `src/lib/api.ts`**

Append:

```ts
export async function listAccounts(): Promise<Account[]> {
  return invoke<Account[]>("list_accounts");
}

export async function addAccount(
  agentType: string,
  name: string,
  configDir: string,
): Promise<number> {
  return invoke<number>("add_account", { agentType, name, configDir });
}

export async function deleteAccount(id: number): Promise<void> {
  return invoke<void>("delete_account", { id });
}

export async function renameAccount(id: number, name: string): Promise<void> {
  return invoke<void>("rename_account", { id, name });
}

export async function detectAccounts(): Promise<DetectedAccount[]> {
  return invoke<DetectedAccount[]>("detect_accounts");
}

export async function setSessionAccount(
  ptyId: number,
  accountId: number | null,
): Promise<void> {
  return invoke<void>("set_session_account", { ptyId, accountId });
}
```

Add the import at the top:

```ts
import type { Account, DetectedAccount } from "./types";
```

Update `SessionInfo` (also in `api.ts`) to include `account_id: number | null;`
and `CreateSessionRequest`-shaped objects to allow `account_id?: number | null`.
**Locate by grep:** `grep -n "SessionInfo" src/lib/api.ts`.

- [ ] **Step 3: Update `sessionStore.ts`**

In `src/store/sessionStore.ts`:

1. In the `addSession` params interface, add `account_id?: number | null;`.
2. In the call to `createSession({...})` inside `addSession`, pass through
   `account_id: params.account_id ?? null,`.
3. In both `Session` literals (new and replaced), set
   `account_id: params.account_id ?? oldSession?.account_id ?? null,`
   (for the new path use `params.account_id ?? null`).
4. In `restore`, when building the `Session` literal from `s` (the DB row),
   add `account_id: s.account_id ?? null,`.

- [ ] **Step 4: Typecheck**

Run: `npm run typecheck`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/lib/ src/store/sessionStore.ts
git commit -m "$(cat <<'EOF'
frontend: account types + api wrappers + sessionStore plumbing

Account / DetectedAccount types; six IPC wrappers; account_id
plumbed through createSession, addSession, and restore. No UI
yet — that lands in subsequent tasks.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: `accountStore` (Zustand)

**Files:**
- Create: `src/store/accountStore.ts`
- Create: `src/store/__tests__/accountStore.test.ts`

- [ ] **Step 1: Write failing tests**

Create `src/store/__tests__/accountStore.test.ts`:

```ts
import { describe, expect, it, vi, beforeEach } from "vitest";
import { useAccountStore } from "../accountStore";
import * as api from "../../lib/api";

vi.mock("../../lib/api");

const mockedApi = vi.mocked(api);

const acct = (overrides: Partial<{
  id: number; agent_type: string; name: string; config_dir: string; sort_order: number;
}> = {}) => ({
  id: 1,
  agent_type: "claude",
  name: "work",
  config_dir: "/tmp/work",
  sort_order: 0,
  ...overrides,
});

describe("accountStore", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useAccountStore.setState({ accounts: [], lastUsedByAgent: {} });
  });

  it("loadAccounts populates state", async () => {
    mockedApi.listAccounts.mockResolvedValueOnce([acct()]);
    mockedApi.getAllSettings.mockResolvedValueOnce([]);
    await useAccountStore.getState().loadAccounts();
    expect(useAccountStore.getState().accounts).toHaveLength(1);
  });

  it("loadAccounts hydrates lastUsedByAgent from settings", async () => {
    mockedApi.listAccounts.mockResolvedValueOnce([acct({ id: 7 })]);
    mockedApi.getAllSettings.mockResolvedValueOnce([
      ["last_account_claude", "7"],
      ["unrelated_setting", "ignore"],
    ]);
    await useAccountStore.getState().loadAccounts();
    expect(useAccountStore.getState().lastUsedByAgent).toEqual({ claude: 7 });
  });

  it("forAgent filters and sorts by sort_order", () => {
    useAccountStore.setState({
      accounts: [
        acct({ id: 1, sort_order: 2 }),
        acct({ id: 2, sort_order: 0 }),
        acct({ id: 3, agent_type: "codex", sort_order: 0 }),
      ],
    });
    const claude = useAccountStore.getState().forAgent("claude");
    expect(claude.map(a => a.id)).toEqual([2, 1]);
  });

  it("resolveDefault returns last-used when valid", () => {
    useAccountStore.setState({
      accounts: [acct({ id: 5 }), acct({ id: 6, name: "b" })],
      lastUsedByAgent: { claude: 6 },
    });
    expect(useAccountStore.getState().resolveDefault("claude")).toBe(6);
  });

  it("resolveDefault falls back to first when last-used is stale", () => {
    useAccountStore.setState({
      accounts: [acct({ id: 5 }), acct({ id: 6, name: "b" })],
      lastUsedByAgent: { claude: 999 },
    });
    expect(useAccountStore.getState().resolveDefault("claude")).toBe(5);
  });

  it("resolveDefault returns null when agent has no accounts", () => {
    useAccountStore.setState({ accounts: [], lastUsedByAgent: {} });
    expect(useAccountStore.getState().resolveDefault("claude")).toBeNull();
  });

  it("rememberLastUsed persists to settings", async () => {
    mockedApi.setSetting.mockResolvedValueOnce(undefined);
    await useAccountStore.getState().rememberLastUsed("claude", 42);
    expect(mockedApi.setSetting).toHaveBeenCalledWith("last_account_claude", "42");
    expect(useAccountStore.getState().lastUsedByAgent.claude).toBe(42);
  });

  it("addAccount appends and reloads", async () => {
    mockedApi.addAccount.mockResolvedValueOnce(11);
    mockedApi.listAccounts.mockResolvedValueOnce([acct({ id: 11 })]);
    mockedApi.getAllSettings.mockResolvedValueOnce([]);
    await useAccountStore.getState().addAccount("claude", "x", "/tmp/x");
    expect(useAccountStore.getState().accounts).toHaveLength(1);
  });

  it("deleteAccount removes from state", async () => {
    useAccountStore.setState({ accounts: [acct({ id: 7 })], lastUsedByAgent: {} });
    mockedApi.deleteAccount.mockResolvedValueOnce(undefined);
    await useAccountStore.getState().deleteAccount(7);
    expect(useAccountStore.getState().accounts).toHaveLength(0);
  });
});
```

- [ ] **Step 2: Run tests → fail**

Run: `npm run test -- accountStore`
Expected: `accountStore.ts` doesn't exist.

- [ ] **Step 3: Implement `accountStore.ts`**

Create `src/store/accountStore.ts`:

```ts
import { create } from "zustand";
import {
  listAccounts,
  addAccount as apiAddAccount,
  deleteAccount as apiDeleteAccount,
  renameAccount as apiRenameAccount,
  detectAccounts as apiDetectAccounts,
  getAllSettings,
  setSetting,
} from "../lib/api";
import type { Account, DetectedAccount } from "../lib/types";

interface AccountState {
  accounts: Account[];
  lastUsedByAgent: Record<string, number>;
  loaded: boolean;

  loadAccounts: () => Promise<void>;
  addAccount: (agentType: string, name: string, configDir: string) => Promise<number>;
  deleteAccount: (id: number) => Promise<void>;
  renameAccount: (id: number, name: string) => Promise<void>;
  detectAccounts: () => Promise<DetectedAccount[]>;
  rememberLastUsed: (agentType: string, id: number) => Promise<void>;

  forAgent: (agentType: string) => Account[];
  resolveDefault: (agentType: string) => number | null;
}

const SETTINGS_PREFIX = "last_account_";

export const useAccountStore = create<AccountState>((set, get) => ({
  accounts: [],
  lastUsedByAgent: {},
  loaded: false,

  loadAccounts: async () => {
    const [accounts, settings] = await Promise.all([
      listAccounts(),
      getAllSettings(),
    ]);
    const lastUsedByAgent: Record<string, number> = {};
    for (const [key, value] of settings) {
      if (key.startsWith(SETTINGS_PREFIX)) {
        const agent = key.slice(SETTINGS_PREFIX.length);
        const parsed = Number.parseInt(value, 10);
        if (Number.isFinite(parsed)) lastUsedByAgent[agent] = parsed;
      }
    }
    set({ accounts, lastUsedByAgent, loaded: true });
  },

  addAccount: async (agentType, name, configDir) => {
    const id = await apiAddAccount(agentType, name, configDir);
    const accounts = await listAccounts();
    set({ accounts });
    return id;
  },

  deleteAccount: async (id) => {
    await apiDeleteAccount(id);
    set((state) => ({
      accounts: state.accounts.filter((a) => a.id !== id),
    }));
  },

  renameAccount: async (id, name) => {
    await apiRenameAccount(id, name);
    set((state) => ({
      accounts: state.accounts.map((a) => (a.id === id ? { ...a, name } : a)),
    }));
  },

  detectAccounts: async () => apiDetectAccounts(),

  rememberLastUsed: async (agentType, id) => {
    await setSetting(`${SETTINGS_PREFIX}${agentType}`, id.toString());
    set((state) => ({
      lastUsedByAgent: { ...state.lastUsedByAgent, [agentType]: id },
    }));
  },

  forAgent: (agentType) =>
    get()
      .accounts.filter((a) => a.agent_type === agentType)
      .sort((a, b) => a.sort_order - b.sort_order || a.id - b.id),

  resolveDefault: (agentType) => {
    const list = get().forAgent(agentType);
    if (list.length === 0) return null;
    const last = get().lastUsedByAgent[agentType];
    if (last != null && list.some((a) => a.id === last)) return last;
    return list[0].id;
  },
}));
```

- [ ] **Step 4: Tests pass**

Run: `npm run test -- accountStore`
Expected: all PASS.

- [ ] **Step 5: Wire load into App startup**

Find where `useSessionStore.getState().restore()` is called (likely
`src/App.tsx` startup `useEffect`). Add next to it:

```ts
import { useAccountStore } from "./store/accountStore";
// ...
useAccountStore.getState().loadAccounts().catch(console.error);
```

- [ ] **Step 6: Typecheck + commit**

Run: `npm run typecheck && npm run test`
Expected: clean.

```bash
git add src/store/accountStore.ts src/store/__tests__/accountStore.test.ts src/App.tsx
git commit -m "$(cat <<'EOF'
frontend: accountStore + startup load

Zustand store with load, CRUD, detect, rememberLastUsed,
forAgent, resolveDefault (last-used → first → null). Settings
key prefix 'last_account_' keyed by agent_type.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: `AccountBadge` component

**Files:**
- Create: `src/components/AccountBadge.tsx`
- Create: `src/components/__tests__/AccountBadge.test.tsx`

- [ ] **Step 1: Write failing tests**

Create `src/components/__tests__/AccountBadge.test.tsx`:

```tsx
import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { AccountBadge } from "../AccountBadge";
import { useAccountStore } from "../../store/accountStore";

const acct = (id: number, name: string, agent_type = "claude") => ({
  id, agent_type, name, config_dir: `/tmp/${name}`, sort_order: 0,
});

describe("AccountBadge", () => {
  beforeEach(() => {
    useAccountStore.setState({
      accounts: [acct(1, "work"), acct(2, "personal"), acct(3, "ignored", "codex")],
      lastUsedByAgent: {},
      loaded: true,
    });
  });

  it("renders nothing for unsupported agent types", () => {
    const { container } = render(
      <AccountBadge agentType="shell" accountId={null} onSwitch={vi.fn()} onManage={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders 'default' when accountId is null", () => {
    render(
      <AccountBadge agentType="claude" accountId={null} onSwitch={vi.fn()} onManage={vi.fn()} />,
    );
    expect(screen.getByRole("button")).toHaveTextContent(/default/i);
  });

  it("renders the account name when accountId resolves", () => {
    render(
      <AccountBadge agentType="claude" accountId={1} onSwitch={vi.fn()} onManage={vi.fn()} />,
    );
    expect(screen.getByRole("button")).toHaveTextContent(/work/);
  });

  it("falls back to 'default' label when accountId is stale", () => {
    render(
      <AccountBadge agentType="claude" accountId={999} onSwitch={vi.fn()} onManage={vi.fn()} />,
    );
    expect(screen.getByRole("button")).toHaveTextContent(/default/i);
  });

  it("popover lists only accounts for the matching agent", () => {
    render(
      <AccountBadge agentType="claude" accountId={1} onSwitch={vi.fn()} onManage={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(screen.getByText("work")).toBeInTheDocument();
    expect(screen.getByText("personal")).toBeInTheDocument();
    expect(screen.queryByText("ignored")).not.toBeInTheDocument();
  });

  it("clicking a non-current account calls onSwitch with id", () => {
    const onSwitch = vi.fn();
    render(
      <AccountBadge agentType="claude" accountId={1} onSwitch={onSwitch} onManage={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("button"));
    fireEvent.click(screen.getByText("personal"));
    expect(onSwitch).toHaveBeenCalledWith(2);
  });

  it("clicking the current account does not call onSwitch", () => {
    const onSwitch = vi.fn();
    render(
      <AccountBadge agentType="claude" accountId={1} onSwitch={onSwitch} onManage={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("button"));
    fireEvent.click(screen.getByText("work"));
    expect(onSwitch).not.toHaveBeenCalled();
  });

  it("Manage… invokes onManage", () => {
    const onManage = vi.fn();
    render(
      <AccountBadge agentType="claude" accountId={1} onSwitch={vi.fn()} onManage={onManage} />,
    );
    fireEvent.click(screen.getByRole("button"));
    fireEvent.click(screen.getByText(/Manage/i));
    expect(onManage).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Verify fail**

Run: `npm run test -- AccountBadge`
Expected: module not found.

- [ ] **Step 3: Implement the component**

Create `src/components/AccountBadge.tsx`:

```tsx
import { useState, useRef, useEffect } from "react";
import { useAccountStore } from "../store/accountStore";
import { env_key_supported } from "../lib/accountUtils";

interface Props {
  agentType: string;
  accountId: number | null;
  onSwitch: (newAccountId: number | null) => void;
  onManage: () => void;
}

export function AccountBadge({ agentType, accountId, onSwitch, onManage }: Props) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const accounts = useAccountStore((s) => s.forAgent(agentType));

  // Close on outside click
  useEffect(() => {
    if (!open) return;
    const onDocClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, [open]);

  if (!env_key_supported(agentType)) return null;

  const current = accounts.find((a) => a.id === accountId);
  const label = current?.name ?? "default";
  const agentLabel = agentType === "claude" ? "Claude" : agentType === "codex" ? "Codex" : agentType;

  return (
    <div ref={ref} className="account-badge">
      <button
        type="button"
        className={`account-badge__btn${current ? "" : " account-badge__btn--muted"}`}
        onClick={() => setOpen((v) => !v)}
      >
        {agentLabel} · {label} ▾
      </button>
      {open && (
        <div className="account-badge__menu" role="menu">
          <button
            type="button"
            role="menuitem"
            className={`account-badge__item${accountId === null ? " account-badge__item--current" : ""}`}
            onClick={() => {
              setOpen(false);
              if (accountId !== null) onSwitch(null);
            }}
          >
            {accountId === null ? "● default" : "○ default"}
          </button>
          {accounts.map((a) => (
            <button
              key={a.id}
              type="button"
              role="menuitem"
              className={`account-badge__item${a.id === accountId ? " account-badge__item--current" : ""}`}
              onClick={() => {
                setOpen(false);
                if (a.id !== accountId) onSwitch(a.id);
              }}
            >
              {a.id === accountId ? `● ${a.name}` : `○ ${a.name}`}
            </button>
          ))}
          <div className="account-badge__sep" />
          <button
            type="button"
            role="menuitem"
            className="account-badge__item"
            onClick={() => {
              setOpen(false);
              onManage();
            }}
          >
            ⚙ Manage accounts…
          </button>
        </div>
      )}
    </div>
  );
}
```

Create the tiny utility module `src/lib/accountUtils.ts`:

```ts
const SUPPORTED = new Set(["claude", "codex"]);
export function env_key_supported(agentType: string): boolean {
  return SUPPORTED.has(agentType);
}
```

(This mirrors the server-side `env_key_for_agent` whitelist. Two sources of
truth is acceptable here because the contract — Claude+Codex only — is
explicit in the spec and small. If a third agent is ever added, the test
suite catches the omission.)

- [ ] **Step 4: Add minimal styles**

Append to `src/App.css` (keep aligned with existing token usage —
`--surface`, `--accent`, etc.; if unsure, follow the pattern of similar
small buttons in the file):

```css
.account-badge { position: relative; display: inline-block; }
.account-badge__btn {
  font-size: 12px;
  padding: 2px 8px;
  background: var(--surface);
  color: var(--accent);
  border: 1px solid var(--accent);
  border-radius: 4px;
  cursor: pointer;
}
.account-badge__btn--muted { opacity: 0.6; }
.account-badge__menu {
  position: absolute;
  top: 100%;
  right: 0;
  margin-top: 4px;
  background: var(--surface);
  border: 1px solid var(--accent);
  border-radius: 4px;
  min-width: 180px;
  z-index: 10;
  padding: 4px 0;
}
.account-badge__item {
  display: block;
  width: 100%;
  text-align: left;
  padding: 6px 12px;
  background: transparent;
  border: none;
  color: inherit;
  cursor: pointer;
  font-size: 13px;
}
.account-badge__item:hover { background: rgba(255,255,255,0.05); }
.account-badge__item--current { font-weight: 600; }
.account-badge__sep { height: 1px; background: var(--accent); opacity: 0.2; margin: 4px 0; }
```

- [ ] **Step 5: Tests pass**

Run: `npm run test -- AccountBadge`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/components/AccountBadge.tsx src/components/__tests__/AccountBadge.test.tsx src/lib/accountUtils.ts src/App.css
git commit -m "$(cat <<'EOF'
ui: AccountBadge — popover account picker for terminal header

Hidden for non-Claude/non-Codex agents. Shows '<Agent> · <name>'
(or muted '<Agent> · default'). Popover lists same-agent accounts
plus a Manage entry. onSwitch/onManage are callbacks — wiring
lands in Task 14.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 13: `AccountSwitchDialog` component

**Files:**
- Create: `src/components/AccountSwitchDialog.tsx`

- [ ] **Step 1: Implement the dialog**

Create `src/components/AccountSwitchDialog.tsx`:

```tsx
interface Props {
  targetAccountName: string;
  agentLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export function AccountSwitchDialog({
  targetAccountName,
  agentLabel,
  onConfirm,
  onCancel,
}: Props) {
  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div
        className="modal account-switch-dialog"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-labelledby="account-switch-title"
      >
        <h2 id="account-switch-title">Switch to account "{targetAccountName}"?</h2>
        <p>This will restart {agentLabel} in this session.</p>
        <p>
          The conversation context will be lost — your current chat history
          won't carry over to the new account.
        </p>
        <p className="account-switch-dialog__note">
          The terminal scrollback is not preserved; the session starts fresh.
        </p>
        <div className="modal-buttons">
          <button type="button" onClick={onCancel}>Cancel</button>
          <button type="button" className="primary" onClick={onConfirm}>
            Restart with {targetAccountName}
          </button>
        </div>
      </div>
    </div>
  );
}
```

The `modal`, `modal-backdrop`, `modal-buttons`, `.primary` classes already
exist in `App.css` (used by other dialogs — e.g. `TaskCreateModal`,
`QuickPromptSaveModal`). If `account-switch-dialog__note` needs styling,
add it next to other dialog rules:

```css
.account-switch-dialog__note { opacity: 0.7; font-style: italic; font-size: 12px; }
```

- [ ] **Step 2: Typecheck**

Run: `npm run typecheck`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add src/components/AccountSwitchDialog.tsx src/App.css
git commit -m "$(cat <<'EOF'
ui: AccountSwitchDialog — confirm modal for live-session swap

Plain confirm with explicit 'context will be lost' wording. No
'don't ask again' option — the action's consequence is irreversible
each time and warrants a deliberate click.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 14: `useSessionActions.switchSessionAccount`

**Files:**
- Modify: `src/hooks/useSessionActions.ts`
- Create: `src/lib/argsCleaning.ts`
- Create: `src/lib/__tests__/argsCleaning.test.ts`
- Create: `src/hooks/__tests__/useSessionActions.switchAccount.test.ts`

> **Why a separate `argsCleaning.ts` module?** The arg-stripping logic
> has its own edge cases (Claude `--resume <id>` pair, `--continue`,
> `--session-id <uuid>`, Codex `resume <id>` subcommand, `--last`) and is
> the highest-risk piece of the swap flow. Isolating it lets us pin
> behaviour in unit tests without mounting React.

- [ ] **Step 1: Write failing tests for `argsCleaning`**

Create `src/lib/__tests__/argsCleaning.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { stripResumeArgsForRelaunch } from "../argsCleaning";

describe("stripResumeArgsForRelaunch — claude", () => {
  it("strips --resume <id> pair", () => {
    expect(stripResumeArgsForRelaunch("claude",
      ["--dangerously-skip-permissions", "--resume", "abc-123", "-p", "hi"]
    )).toEqual(["--dangerously-skip-permissions", "-p", "hi"]);
  });

  it("strips --continue", () => {
    expect(stripResumeArgsForRelaunch("claude", ["--continue", "-p", "go"]))
      .toEqual(["-p", "go"]);
  });

  it("strips --session-id <uuid> pair", () => {
    expect(stripResumeArgsForRelaunch("claude",
      ["--session-id", "uuid-x", "-p", "hi"]
    )).toEqual(["-p", "hi"]);
  });

  it("strips multiple resume-like args in one go", () => {
    expect(stripResumeArgsForRelaunch("claude",
      ["--session-id", "u", "--resume", "x", "--continue"]
    )).toEqual([]);
  });

  it("leaves unrelated args untouched", () => {
    expect(stripResumeArgsForRelaunch("claude", ["-p", "hello", "--verbose"]))
      .toEqual(["-p", "hello", "--verbose"]);
  });

  it("--resume without a following id strips just the flag", () => {
    // Defensive: real Claude rejects bare --resume, but we shouldn't crash.
    expect(stripResumeArgsForRelaunch("claude", ["--resume"])).toEqual([]);
  });
});

describe("stripResumeArgsForRelaunch — codex", () => {
  it("strips 'resume <id>' subcommand pair", () => {
    expect(stripResumeArgsForRelaunch("codex", ["resume", "thread-1", "--prompt", "hi"]))
      .toEqual(["--prompt", "hi"]);
  });

  it("strips 'resume --last'", () => {
    expect(stripResumeArgsForRelaunch("codex", ["resume", "--last"]))
      .toEqual([]);
  });

  it("does not touch a non-leading 'resume' token", () => {
    expect(stripResumeArgsForRelaunch("codex", ["--prompt", "talk about resume"]))
      .toEqual(["--prompt", "talk about resume"]);
  });
});

describe("stripResumeArgsForRelaunch — other agents", () => {
  it("returns args unchanged for unsupported agent types", () => {
    expect(stripResumeArgsForRelaunch("shell", ["--foo", "bar"]))
      .toEqual(["--foo", "bar"]);
  });
});
```

- [ ] **Step 2: Implement `argsCleaning.ts`**

Create `src/lib/argsCleaning.ts`:

```ts
/**
 * Strip resume/session-id args that reference state stored in the OLD
 * configDir, so a relaunch under a NEW account doesn't try to look up
 * a non-existent session.
 *
 * Pure function — exercised exclusively in unit tests.
 */
export function stripResumeArgsForRelaunch(
  agentType: string,
  args: readonly string[],
): string[] {
  if (agentType === "claude") return stripClaude(args);
  if (agentType === "codex") return stripCodex(args);
  return [...args];
}

function stripClaude(args: readonly string[]): string[] {
  const out: string[] = [];
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === "--continue") continue;
    if (a === "--resume" || a === "--session-id") {
      // Skip the flag and its value (if value present and not another flag).
      const next = args[i + 1];
      if (next != null && !next.startsWith("-")) i++;
      continue;
    }
    out.push(a);
  }
  return out;
}

function stripCodex(args: readonly string[]): string[] {
  // Codex `resume` is a leading subcommand. Recognised only at index 0.
  if (args.length === 0 || args[0] !== "resume") return [...args];
  // 'resume <thread-id>' or 'resume --last'
  if (args.length >= 2) {
    return [...args.slice(2)];
  }
  return [];
}
```

- [ ] **Step 3: Tests pass**

Run: `npm run test -- argsCleaning`
Expected: all PASS.

- [ ] **Step 4: Write failing test for the action**

Create `src/hooks/__tests__/useSessionActions.switchAccount.test.ts`:

```ts
import { describe, expect, it, vi, beforeEach } from "vitest";
import { switchSessionAccount } from "../useSessionActions";
import { useSessionStore } from "../../store/sessionStore";
import { useAccountStore } from "../../store/accountStore";

vi.mock("../../store/sessionStore");
vi.mock("../../store/accountStore");

describe("switchSessionAccount", () => {
  beforeEach(() => vi.resetAllMocks());

  it("kills + recreates with replaceId and cleans resume args", async () => {
    const addSession = vi.fn().mockResolvedValue(99);
    const rememberLastUsed = vi.fn().mockResolvedValue(undefined);
    (useSessionStore.getState as unknown as vi.Mock).mockReturnValue({
      sessions: [{
        id: 10,
        agent_type: "claude",
        command: "claude",
        args: ["--resume", "abc", "-p", "hi"],
        working_dir: "/tmp/p",
        worktree_path: null,
        task_id: null,
      }],
      addSession,
    });
    (useAccountStore.getState as unknown as vi.Mock).mockReturnValue({
      rememberLastUsed,
    });

    await switchSessionAccount(10, 7);

    expect(addSession).toHaveBeenCalledWith(expect.objectContaining({
      command: "claude",
      args: ["-p", "hi"],
      working_dir: "/tmp/p",
      replaceId: 10,
      account_id: 7,
    }));
    expect(rememberLastUsed).toHaveBeenCalledWith("claude", 7);
  });

  it("noop when target session missing", async () => {
    const addSession = vi.fn();
    (useSessionStore.getState as unknown as vi.Mock).mockReturnValue({
      sessions: [], addSession,
    });
    await switchSessionAccount(10, 7);
    expect(addSession).not.toHaveBeenCalled();
  });

  it("does NOT remember last-used when switching to default (null)", async () => {
    const addSession = vi.fn().mockResolvedValue(99);
    const rememberLastUsed = vi.fn();
    (useSessionStore.getState as unknown as vi.Mock).mockReturnValue({
      sessions: [{
        id: 10, agent_type: "claude", command: "claude", args: [],
        working_dir: "/tmp/p", worktree_path: null, task_id: null,
      }],
      addSession,
    });
    (useAccountStore.getState as unknown as vi.Mock).mockReturnValue({ rememberLastUsed });
    await switchSessionAccount(10, null);
    expect(rememberLastUsed).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 5: Implement the action**

In `src/hooks/useSessionActions.ts`, append the export:

```ts
import { stripResumeArgsForRelaunch } from "../lib/argsCleaning";
import { useSessionStore } from "../store/sessionStore";
import { useAccountStore } from "../store/accountStore";

/**
 * Hot-swap the account of a live session. Kills the running process and
 * starts a new one in the same UI slot (replaceId) under the new account.
 * Conversation context is NOT preserved — see brainstorming spec.
 */
export async function switchSessionAccount(
  ptyId: number,
  newAccountId: number | null,
): Promise<void> {
  const { sessions, addSession } = useSessionStore.getState();
  const session = sessions.find((s) => s.id === ptyId);
  if (!session) return;

  const cleanedArgs = stripResumeArgsForRelaunch(session.agent_type, session.args);

  await addSession({
    command: session.command,
    args: cleanedArgs,
    working_dir: session.working_dir,
    task_name: session.task_name,
    agent_type: session.agent_type,
    worktree_path: session.worktree_path ?? undefined,
    base_commit: session.base_commit ?? undefined,
    task_id: session.task_id ?? undefined,
    replaceId: ptyId,
    account_id: newAccountId,
  });

  if (newAccountId !== null) {
    await useAccountStore.getState().rememberLastUsed(session.agent_type, newAccountId);
  }
}
```

- [ ] **Step 6: Tests pass**

Run: `npm run test -- switchAccount argsCleaning`
Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add src/hooks/ src/lib/argsCleaning.ts src/lib/__tests__/argsCleaning.test.ts
git commit -m "$(cat <<'EOF'
hooks: switchSessionAccount + arg-cleaning for relaunch

stripResumeArgsForRelaunch removes Claude --resume/--continue/
--session-id and Codex 'resume <id>'/'--last'. switchSessionAccount
recreates the session with replaceId so the UI slot stays put.
rememberLastUsed only fires when switching to a concrete account,
not when reverting to default.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 15: `AccountsManager` modal + integration with `SettingsModal`

**Files:**
- Create: `src/components/AccountsManager.tsx`
- Create: `src/components/__tests__/AccountsManager.test.tsx`
- Modify: `src/components/SettingsModal.tsx`

- [ ] **Step 1: Write the most critical failing tests**

Create `src/components/__tests__/AccountsManager.test.tsx`:

```tsx
import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { AccountsManager } from "../AccountsManager";
import { useAccountStore } from "../../store/accountStore";
import { useSessionStore } from "../../store/sessionStore";

const acct = (id: number, name: string, agent = "claude") => ({
  id, agent_type: agent, name, config_dir: `/tmp/${name}`, sort_order: 0,
});

describe("AccountsManager", () => {
  beforeEach(() => {
    useAccountStore.setState({
      accounts: [acct(1, "work"), acct(2, "personal", "codex")],
      lastUsedByAgent: {},
      loaded: true,
    });
    useSessionStore.setState?.({ sessions: [] });
  });

  it("renders sections per agent type", () => {
    render(<AccountsManager onClose={vi.fn()} />);
    expect(screen.getByText(/Claude/)).toBeInTheDocument();
    expect(screen.getByText(/Codex/)).toBeInTheDocument();
    expect(screen.getByText("work")).toBeInTheDocument();
    expect(screen.getByText("personal")).toBeInTheDocument();
  });

  it("delete with active sessions warns with count", async () => {
    useSessionStore.setState?.({ sessions: [
      { id: 1, agent_type: "claude", account_id: 1 } as any,
      { id: 2, agent_type: "claude", account_id: 1 } as any,
    ]});
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<AccountsManager onClose={vi.fn()} />);
    fireEvent.click(screen.getAllByText(/Delete/i)[0]);
    expect(confirmSpy).toHaveBeenCalledWith(expect.stringContaining("2"));
    confirmSpy.mockRestore();
  });

  it("add form validation rejects empty name", async () => {
    render(<AccountsManager onClose={vi.fn()} />);
    fireEvent.click(screen.getByText(/Add manually/i));
    const submit = await screen.findByText(/^Add$/);
    fireEvent.click(submit);
    expect(await screen.findByText(/name is required/i)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Verify failure**

Run: `npm run test -- AccountsManager`
Expected: module not found.

- [ ] **Step 3: Implement `AccountsManager.tsx`**

Create `src/components/AccountsManager.tsx`:

```tsx
import { useState, useEffect } from "react";
import { useAccountStore } from "../store/accountStore";
import { useSessionStore } from "../store/sessionStore";
import type { DetectedAccount } from "../lib/types";
import { open as openFolder } from "@tauri-apps/plugin-dialog";

interface Props {
  onClose: () => void;
}

export function AccountsManager({ onClose }: Props) {
  const accounts = useAccountStore((s) => s.accounts);
  const addAccount = useAccountStore((s) => s.addAccount);
  const deleteAccount = useAccountStore((s) => s.deleteAccount);
  const renameAccount = useAccountStore((s) => s.renameAccount);
  const detect = useAccountStore((s) => s.detectAccounts);
  const sessions = useSessionStore((s) => s.sessions);

  const [detected, setDetected] = useState<DetectedAccount[]>([]);
  const [showAdd, setShowAdd] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    detect().then(setDetected).catch(console.error);
  }, [detect]);

  const groupsByAgent = (["claude", "codex"] as const).map((agentType) => ({
    agentType,
    label: agentType === "claude" ? "Claude" : "Codex",
    items: accounts.filter((a) => a.agent_type === agentType),
  }));

  const handleDelete = async (id: number) => {
    const liveCount = sessions.filter(
      (s) => s.account_id === id && s.status !== "exited",
    ).length;
    const msg = liveCount > 0
      ? `Used in ${liveCount} running session${liveCount === 1 ? "" : "s"}. They will continue running, but the badge will switch to "default". Delete this account?`
      : "Delete this account?";
    if (!window.confirm(msg)) return;
    await deleteAccount(id);
  };

  const handleRename = async (id: number, current: string) => {
    const next = window.prompt("New name:", current)?.trim();
    if (!next || next === current) return;
    try {
      await renameAccount(id, next);
    } catch (e: unknown) {
      setError(String(e));
    }
  };

  const handleAddDetected = async (d: DetectedAccount) => {
    try {
      await addAccount(d.agent_type, d.suggested_name, d.config_dir);
      setDetected((prev) => prev.filter((x) => x.config_dir !== d.config_dir));
    } catch (e: unknown) {
      setError(String(e));
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal accounts-manager"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-labelledby="accounts-manager-title"
      >
        <h2 id="accounts-manager-title">Accounts</h2>
        <p className="accounts-manager__help">
          To create credentials, run <code>CLAUDE_CONFIG_DIR=~/.claude-work claude</code> in
          your terminal and complete <code>/login</code>. Then add the account here.
        </p>

        {error && <div className="accounts-manager__error">{error}</div>}

        {groupsByAgent.map(({ agentType, label, items }) => (
          <section key={agentType} className="accounts-manager__section">
            <h3>{label}</h3>
            {items.length === 0 && (
              <div className="accounts-manager__empty">No accounts.</div>
            )}
            {items.map((a) => (
              <div key={a.id} className="accounts-manager__row">
                <div>
                  <div className="accounts-manager__name">{a.name}</div>
                  <div className="accounts-manager__path" title={a.config_dir}>
                    {a.config_dir}
                  </div>
                </div>
                <div className="accounts-manager__actions">
                  <button type="button" onClick={() => handleRename(a.id, a.name)}>Rename</button>
                  <button type="button" onClick={() => handleDelete(a.id)}>Delete</button>
                </div>
              </div>
            ))}
          </section>
        ))}

        {detected.length > 0 && (
          <section className="accounts-manager__section">
            <h3>Detected on disk</h3>
            {detected.map((d) => (
              <div key={d.config_dir} className="accounts-manager__row">
                <div>
                  <div className="accounts-manager__name">{d.suggested_name} ({d.agent_type})</div>
                  <div className="accounts-manager__path">{d.config_dir}</div>
                </div>
                <button type="button" onClick={() => handleAddDetected(d)}>Add</button>
              </div>
            ))}
          </section>
        )}

        <div className="accounts-manager__footer">
          {!showAdd ? (
            <button type="button" onClick={() => setShowAdd(true)}>Add manually</button>
          ) : (
            <AddAccountForm
              onCancel={() => setShowAdd(false)}
              onSubmit={async (agentType, name, configDir) => {
                try {
                  await addAccount(agentType, name, configDir);
                  setShowAdd(false);
                } catch (e: unknown) {
                  setError(String(e));
                  throw e;
                }
              }}
            />
          )}
          <button type="button" onClick={onClose}>Close</button>
        </div>
      </div>
    </div>
  );
}

function AddAccountForm({
  onCancel,
  onSubmit,
}: {
  onCancel: () => void;
  onSubmit: (agentType: string, name: string, configDir: string) => Promise<void>;
}) {
  const [agentType, setAgentType] = useState("claude");
  const [name, setName] = useState("");
  const [configDir, setConfigDir] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);

  const handlePickFolder = async () => {
    const selected = await openFolder({ directory: true, multiple: false });
    if (typeof selected === "string") setConfigDir(selected);
  };

  const handleSubmit = async () => {
    setLocalError(null);
    if (!name.trim()) {
      setLocalError("name is required");
      return;
    }
    if (!configDir.trim()) {
      setLocalError("configDir is required");
      return;
    }
    try { await onSubmit(agentType, name.trim(), configDir.trim()); }
    catch { /* surfaced by parent */ }
  };

  return (
    <div className="accounts-manager__add-form">
      <label>
        Agent
        <select value={agentType} onChange={(e) => setAgentType(e.target.value)}>
          <option value="claude">Claude</option>
          <option value="codex">Codex</option>
        </select>
      </label>
      <label>
        Name
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="work" />
      </label>
      <label>
        Config directory
        <div className="accounts-manager__path-row">
          <input
            value={configDir}
            onChange={(e) => setConfigDir(e.target.value)}
            placeholder="~/.claude-work"
          />
          <button type="button" onClick={handlePickFolder}>Browse…</button>
        </div>
      </label>
      {localError && <div className="accounts-manager__error">{localError}</div>}
      <div className="accounts-manager__form-buttons">
        <button type="button" onClick={onCancel}>Cancel</button>
        <button type="button" className="primary" onClick={handleSubmit}>Add</button>
      </div>
    </div>
  );
}
```

Add CSS to `src/App.css`:

```css
.accounts-manager { min-width: 480px; max-width: 600px; }
.accounts-manager__help { font-size: 12px; opacity: 0.7; }
.accounts-manager__help code { background: rgba(255,255,255,0.06); padding: 1px 4px; border-radius: 3px; }
.accounts-manager__error { color: #f87171; background: rgba(248,113,113,0.1); padding: 6px 10px; border-radius: 4px; margin: 8px 0; }
.accounts-manager__section { margin: 16px 0; }
.accounts-manager__section h3 { margin-bottom: 8px; font-size: 13px; opacity: 0.7; text-transform: uppercase; letter-spacing: 0.5px; }
.accounts-manager__row { display: flex; justify-content: space-between; align-items: center; padding: 8px; border-bottom: 1px solid rgba(255,255,255,0.06); }
.accounts-manager__name { font-weight: 500; }
.accounts-manager__path { font-size: 11px; opacity: 0.6; font-family: monospace; }
.accounts-manager__actions { display: flex; gap: 6px; }
.accounts-manager__empty { font-style: italic; opacity: 0.5; font-size: 12px; padding: 6px 0; }
.accounts-manager__footer { display: flex; justify-content: space-between; gap: 8px; margin-top: 16px; }
.accounts-manager__add-form { display: flex; flex-direction: column; gap: 8px; }
.accounts-manager__add-form label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; }
.accounts-manager__path-row { display: flex; gap: 6px; }
.accounts-manager__path-row input { flex: 1; }
.accounts-manager__form-buttons { display: flex; gap: 8px; justify-content: flex-end; margin-top: 4px; }
```

- [ ] **Step 4: Wire into `SettingsModal`**

Open `src/components/SettingsModal.tsx`. Find the section structure
(grep for `Detect agents` or another existing section). Add a new entry
to its tab/section list:

```tsx
import { AccountsManager } from "./AccountsManager";

// inside the modal body, alongside existing sections:
<section className="settings-section">
  <h3>Accounts</h3>
  <p>Manage isolated Claude / Codex accounts (CLAUDE_CONFIG_DIR / CODEX_HOME).</p>
  <button type="button" onClick={() => setShowAccountsManager(true)}>
    Manage accounts…
  </button>
</section>
{showAccountsManager && (
  <AccountsManager onClose={() => setShowAccountsManager(false)} />
)}
```

with the corresponding `useState`:

```tsx
const [showAccountsManager, setShowAccountsManager] = useState(false);
```

- [ ] **Step 5: Tests pass + typecheck**

Run: `npm run test -- AccountsManager && npm run typecheck`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/components/AccountsManager.tsx src/components/__tests__/AccountsManager.test.tsx src/components/SettingsModal.tsx src/App.css
git commit -m "$(cat <<'EOF'
ui: AccountsManager modal + Settings integration

Sections per agent type, detected-on-disk list with one-click add,
manual add form with folder picker, rename/delete inline. Delete
warns when live sessions are using the account. Mounted as a child
modal from a new 'Accounts' section in SettingsModal.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 16: Mount `AccountBadge` in `TerminalPane` header

**Files:**
- Modify: `src/components/TerminalPane.tsx`

- [ ] **Step 1: Locate the header**

Run: `grep -n "task_name\|status_line\|TerminalPane" src/components/TerminalPane.tsx | head -40`

Find the JSX block that renders the session title / status row (likely
near the top of the returned tree). This is where the badge belongs.

- [ ] **Step 2: Add the badge**

In `src/components/TerminalPane.tsx`:

```tsx
import { useState } from "react";
import { AccountBadge } from "./AccountBadge";
import { AccountSwitchDialog } from "./AccountSwitchDialog";
import { AccountsManager } from "./AccountsManager";
import { useAccountStore } from "../store/accountStore";
import { switchSessionAccount } from "../hooks/useSessionActions";

// Inside the component body (assumes `session` is in scope; if the
// existing code uses a different name, adapt accordingly):
const [pendingSwitch, setPendingSwitch] = useState<{ targetId: number | null; targetName: string } | null>(null);
const [showManager, setShowManager] = useState(false);
const accounts = useAccountStore((s) => s.accounts);

const handleSwitch = (newAccountId: number | null) => {
  const acc = accounts.find((a) => a.id === newAccountId);
  const targetName = acc?.name ?? "default";
  setPendingSwitch({ targetId: newAccountId, targetName });
};

const confirmSwitch = async () => {
  if (!pendingSwitch) return;
  await switchSessionAccount(session.id, pendingSwitch.targetId);
  setPendingSwitch(null);
};
```

Inside the existing header JSX (next to the task name and any other
indicators), insert:

```tsx
<AccountBadge
  agentType={session.agent_type}
  accountId={session.account_id ?? null}
  onSwitch={handleSwitch}
  onManage={() => setShowManager(true)}
/>
```

After the existing modal/dialog children of the component:

```tsx
{pendingSwitch && (
  <AccountSwitchDialog
    targetAccountName={pendingSwitch.targetName}
    agentLabel={session.agent_type === "claude" ? "Claude" : "Codex"}
    onConfirm={confirmSwitch}
    onCancel={() => setPendingSwitch(null)}
  />
)}
{showManager && <AccountsManager onClose={() => setShowManager(false)} />}
```

- [ ] **Step 3: Typecheck + build**

Run: `npm run typecheck`
Expected: clean.

- [ ] **Step 4: Manual sanity check**

Run: `npm run tauri dev`. Once the app is up:
- Open a Claude session → the header shows `Claude · default`.
- Click the badge → popover with `default` option only (no accounts yet).
- Open Settings → Accounts → Manage → see the modal.

- [ ] **Step 5: Commit**

```bash
git add src/components/TerminalPane.tsx
git commit -m "$(cat <<'EOF'
ui: mount AccountBadge in TerminalPane header

Clicking the badge opens the popover; selecting an entry that
isn't the current one opens AccountSwitchDialog; confirming
calls switchSessionAccount and the session relaunches in the
same UI slot. Manage… opens the AccountsManager modal.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 17: Account dropdown in new-session launcher

**Files:**
- Modify: `src/components/SessionLauncher.tsx`

- [ ] **Step 1: Locate where the agent type is chosen**

Run: `grep -n "agent_type\|agentType" src/components/SessionLauncher.tsx`

Identify the state variable holding the selected agent type.

- [ ] **Step 2: Add account state and dropdown**

In `src/components/SessionLauncher.tsx`:

```tsx
import { useAccountStore } from "../store/accountStore";

// Inside the component body:
const accountsForAgent = useAccountStore((s) => s.forAgent(agentType));
const resolveDefault = useAccountStore((s) => s.resolveDefault);
const [accountId, setAccountId] = useState<number | null>(() => resolveDefault(agentType));

useEffect(() => {
  // When the user changes agent type, re-resolve default
  setAccountId(resolveDefault(agentType));
}, [agentType, resolveDefault]);
```

JSX (insert near the other form fields, only when there are accounts for
this agent type):

```tsx
{accountsForAgent.length > 0 && (
  <label className="form-field">
    Account
    <select
      value={accountId ?? ""}
      onChange={(e) => setAccountId(e.target.value === "" ? null : Number(e.target.value))}
    >
      <option value="">default (system ~/.claude or ~/.codex)</option>
      {accountsForAgent.map((a) => (
        <option key={a.id} value={a.id}>{a.name}</option>
      ))}
    </select>
  </label>
)}
```

Wherever the `addSession({...})` call is made, pass:

```tsx
account_id: accountId,
```

After the session is launched, persist last-used (only if a specific
account was picked):

```tsx
if (accountId !== null) {
  useAccountStore.getState().rememberLastUsed(agentType, accountId).catch(console.error);
}
```

- [ ] **Step 3: Typecheck**

Run: `npm run typecheck`
Expected: clean.

- [ ] **Step 4: Manual sanity check**

Restart `npm run tauri dev`:
- Open Settings → Add a Claude account pointing to (e.g.) `~/.claude` (your real one).
- Cmd+Shift+T → New session launcher → Claude agent → see the `Account`
  dropdown with `default` and `<your-name>` choices.
- Pick `<your-name>` → launch → header shows `Claude · <your-name>`.
- Next launch → dropdown defaults to `<your-name>` (last-used).

- [ ] **Step 5: Commit**

```bash
git add src/components/SessionLauncher.tsx
git commit -m "$(cat <<'EOF'
ui: account dropdown in SessionLauncher

Hidden when no accounts exist for the selected agent type. Default
resolves via accountStore.resolveDefault (last-used → first → null).
On launch, persists last-used for future sessions.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 18: Manual smoke checklist

**No code, no tests — but DO run through every item before declaring
done.** Append the results as a comment on the PR (or in the commit
message of the final tidy-up commit).

- [ ] In a real shell:
  ```bash
  CLAUDE_CONFIG_DIR=~/.claude-aicoder-smoke-1 claude
  ```
  Complete `/login` for a test account. `Ctrl+C` to exit.
- [ ] In AICoder → Settings → Accounts → Manage. Confirm
  `.claude-aicoder-smoke-1` shows up under "Detected on disk". Click
  Add. Name it `smoke1`.
- [ ] Start a new Claude session → in the new-session launcher pick
  `smoke1`. After launch, run `/status` inside Claude — confirm it
  reports the `smoke1` account email.
- [ ] In that running session, click the badge (`Claude · smoke1`) →
  pick `default` → confirm dialog → click confirm. Claude restarts in
  the same UI slot under `default`.
- [ ] Run `/status` again — confirms the default account.
- [ ] Open Settings → Accounts → Delete `smoke1`. Confirm the
  warning text says "Used in 0 running sessions" (or N if a session
  is still up). After delete, the badge of any session that had
  `smoke1` shows `Claude · default` (muted).
- [ ] In a real shell, `mv ~/.claude-aicoder-smoke-1 ~/.claude-aicoder-smoke-1-moved`.
  Re-add the (now-moved) path manually — confirm a clear "path does not exist"
  error from `add_account`.
- [ ] In a real shell, `export CLAUDE_CONFIG_DIR=/tmp/should-not-leak`.
  Open AICoder, launch a Claude session without selecting any account.
  Inside Claude, run `/status` — must NOT show `/tmp/should-not-leak`;
  must show the user's normal `~/.claude` identity.
- [ ] Repeat the start/switch/delete cycle with **Codex** (verify
  `CODEX_HOME` is honoured by checking
  `ls $CODEX_HOME/auth.json` from inside the Codex session via shell).

- [ ] **Final commit (only if anything was tweaked)**

```bash
git add -A
git commit -m "$(cat <<'EOF'
chore: smoke-checklist notes for account switching

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review

**1. Spec coverage:**

| Spec section | Implementing task(s) |
|---|---|
| Data model — `accounts` table | Task 1 |
| Data model — `sessions.account_id` migration + SET NULL | Task 1 |
| Data model — settings keys for `last_account_<agent>` | Task 11 |
| `env_key_for_agent` constant | Task 4 |
| `LaunchConfig.env` | Task 4 |
| `PtyManager` extra_env injection | Task 5 |
| `SessionManager` resolve env from account_id | Task 6 |
| Six IPC commands | Task 8 |
| `account_id` in `CreateSessionRequest` + `SessionInfo` | Task 9 |
| Auto-detect | Task 7 |
| Tilde + canonicalize | Task 8 |
| Frontend types / api / sessionStore plumbing | Task 10 |
| accountStore (load, CRUD, defaults) | Task 11 |
| AccountBadge | Task 12 |
| AccountSwitchDialog | Task 13 |
| switchSessionAccount action + args cleaning | Task 14 |
| AccountsManager modal + Settings entry | Task 15 |
| TerminalPane integration | Task 16 |
| SessionLauncher account dropdown | Task 17 |
| Manual smoke | Task 18 |
| Edge case: delete-while-active (FK SET NULL) | Tasks 1 + 15 (warning UI) |
| Edge case: missing configDir | Task 6 (resolve) + Task 8 (add validation) |
| Edge case: unsupported agent | Task 6 + Task 8 |
| Edge case: parent shell CLAUDE_CONFIG_DIR doesn't leak | Task 5 (whitelist preserved) + Task 18 smoke |

All spec requirements covered.

**2. Placeholder scan:** No `TBD` / `TODO` / "implement later" in task
bodies. Every step has explicit code or commands.

**3. Type consistency:**
- Backend `AccountRow.id: i64`; frontend `Account.id: number`. ✓
- `account_id: Option<i64>` ↔ `account_id: number | null`. ✓
- IPC param names use `snake_case` in Rust (`agent_type`, `config_dir`,
  `pty_id`); JS wrapper uses `camelCase` (`agentType`, `configDir`,
  `ptyId`) — Tauri 2 converts automatically (per CLAUDE.md). ✓
- `env_key_for_agent` returns `Option<&'static str>` (Rust) ↔
  `env_key_supported` returns `boolean` (TS) — same whitelist, distinct
  surface. ✓
- `switchSessionAccount(ptyId, newAccountId)` matches the
  `useSessionActions` test. ✓

**4. Ambiguity:** Whitelist behaviour is restated in three places
(Task 5 step 3, Task 18 smoke check, Self-review). FK SET NULL behaviour
is tested in Task 1 and asserted in Task 15 (warning copy). Switch flow
explicitly says "kill + create_session(replace_id)" — no other
interpretation.

Plan is internally consistent. No edits needed.

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-05-18-account-switching.md`. Two execution options:**

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

**Which approach?**
