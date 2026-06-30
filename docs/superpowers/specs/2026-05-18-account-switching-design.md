---
name: Account Switching for Claude and Codex
description: Per-session account selection and hot-swap via CLAUDE_CONFIG_DIR / CODEX_HOME
status: draft
date: 2026-05-18
---

# Account Switching for Claude and Codex

## Summary

Allow a user to run multiple isolated Claude Code and Codex CLI accounts side
by side, and switch the account of any session — including a live one — via a
badge in the terminal header.

The mechanism is the canonical one supported by both CLIs:
`CLAUDE_CONFIG_DIR` for Claude Code and `CODEX_HOME` for Codex. Each value
points to an isolated directory containing credentials, history, settings,
plugins, and hooks. We inject these env vars at PTY spawn time.

Hot-swap on a live session means **kill the current process and relaunch it
under the new account in the same UI slot**. Conversation context is not
preserved (Claude/Codex history lives inside the configDir we're swapping).
The terminal is cleared on relaunch; the old scrollback is not carried over.

## Goals

- Register multiple accounts per agent type, each backed by an isolated
  configDir on disk.
- Choose the account for a session at creation time, with a sensible default
  (last used per agent type).
- Switch the account of a live session in two clicks (badge → entry → confirm).
- Auto-detect existing configDirs (`~/.claude*`, `~/.codex*`) so users with
  preexisting setups don't have to type paths.
- Keep credentials out of AICoder's process and DB. We only store paths and
  names.

## Non-goals

- We do not run `claude /login` for the user. Login is a manual step in the
  user's own terminal; AICoder shows clear instructions.
- We do not preserve conversation context across an account swap. The
  Claude/Codex on-disk session store is configDir-scoped; cross-account
  resume is unreliable and depends on undocumented internals.
- We do not preserve terminal scrollback across a swap.
- We do not support per-project default accounts in this iteration.
  `last_used per agent_type` is global.
- We do not support Gemini, OpenCode, Cursor, Shell. The data model is
  agent-type-keyed so Gemini can be added later by extending
  `env_key_for_agent`.

## Mechanism research

Both Claude Code and Codex support full account isolation via a single env
variable pointing to the config directory:

- **Claude Code**: `CLAUDE_CONFIG_DIR` (default `~/.claude`). On macOS
  credentials live in Keychain, scoped by configDir. On Linux/Windows
  credentials live in `<configDir>/.credentials.json`. Sessions, settings,
  plugins, hooks — all under configDir.
  ([env-vars docs](https://code.claude.com/docs/en/env-vars))
- **Codex CLI**: `CODEX_HOME` (default `~/.codex`). Same idea, includes
  `state_5.sqlite` which AICoder already reads to capture thread IDs
  (`session_manager.rs:read_codex_thread_id`).

These env vars must be set at process spawn time. They cannot be changed for
a running child process — hence the "kill + relaunch in same UI slot" model.

## Architecture (approach A from brainstorming)

### Data model

New table `accounts`:

```sql
CREATE TABLE accounts (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  agent_type   TEXT NOT NULL,
  name         TEXT NOT NULL,
  config_dir   TEXT NOT NULL,
  sort_order   INTEGER NOT NULL DEFAULT 0,
  created_at   DATETIME DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(agent_type, name),
  UNIQUE(agent_type, config_dir)
);
```

Migration on existing `sessions`:

```sql
ALTER TABLE sessions ADD COLUMN account_id INTEGER
  REFERENCES accounts(id) ON DELETE SET NULL;
```

`NULL` means "system default" — Claude/Codex use their own `~/.claude` /
`~/.codex` without any env override. All preexisting sessions are NULL after
migration; new sessions without an account choice are also NULL.

Last-used preference uses the existing `settings` key-value table:

- `last_account_claude` → `<account_id>`
- `last_account_codex`  → `<account_id>`

Server-side constant maps agent type to its env var:

```rust
fn env_key_for_agent(agent_type: &str) -> Option<&'static str> {
    match agent_type {
        "claude" => Some("CLAUDE_CONFIG_DIR"),
        "codex"  => Some("CODEX_HOME"),
        _        => None,
    }
}
```

### Backend: env injection through PTY

`LaunchConfig` gets an `env: HashMap<String, String>` field.

`PtyManager::create_session` gets an `extra_env: HashMap<String, String>`
parameter. Inside the spawn flow, after the existing whitelist loop:

```rust
for (key, value) in std::env::vars() {
    if is_safe_env_key(&key) { cmd.env(key, value); }
}
cmd.env("TERM", "xterm-256color");

// Inject per-session env AFTER whitelist — overrides on key collision.
for (key, value) in extra_env {
    cmd.env(key, value);
}
```

The whitelist is **not** changed. `CLAUDE_CONFIG_DIR` and `CODEX_HOME` remain
absent from it, so they cannot leak from the parent shell into a session that
didn't explicitly request an account. This preserves the existing security
invariant (API-key-shaped variables stay out of children).

`CreateSessionParams` gets `account_id: Option<i64>`. Before spawn, the
session manager resolves it to an env injection:

```rust
let extra_env = match params.account_id {
    Some(aid) => {
        let acc = self.db.get_account_by_id(aid)?
            .ok_or(AppError::AccountNotFound(aid))?;
        let key = env_key_for_agent(&acc.agent_type)
            .ok_or(AppError::InvalidAgentForAccount(acc.agent_type.clone()))?;
        let canonical = dunce::canonicalize(&acc.config_dir)
            .map_err(|_| AppError::AccountConfigDirMissing(acc.config_dir.clone()))?;
        HashMap::from([(key.to_string(), canonical.to_string_lossy().into_owned())])
    }
    None => HashMap::new(),
};
```

`account_id` is stored on the resulting `sessions` row.

### IPC commands

| Command | Params | Returns |
|---|---|---|
| `list_accounts` | — | `Vec<AccountInfo>` |
| `add_account` | `agent_type`, `name`, `config_dir` | `account_id` |
| `delete_account` | `id` | — |
| `rename_account` | `id`, `name` | — |
| `detect_accounts` | — | `Vec<DetectedAccount>` |
| `set_session_account` | `pty_id`, `account_id?` | — (DB only, no relaunch) |

`create_session` gets an optional `account_id` field in
`CreateSessionRequest`. `None` = system default. Backwards-compatible with
older frontends.

`detect_accounts` scans `$HOME` for `~/.claude*` and `~/.codex*` (shallow,
depth 1). A directory only qualifies if it contains a known marker file
(Claude: `.credentials.json` OR `settings.json` OR `projects/` subdir; Codex:
`auth.json` OR `state_5.sqlite`). Already-registered configDirs are
excluded.

### Frontend

**`accountStore`** (Zustand, mirrors `projectStore` style):

```ts
interface AccountState {
  accounts: AccountInfo[];
  lastUsedByAgent: Record<string, number>;
  loadAccounts(): Promise<void>;
  addAccount(agentType, name, configDir): Promise<void>;
  deleteAccount(id): Promise<void>;
  renameAccount(id, name): Promise<void>;
  detectAndOfferImport(): Promise<DetectedAccount[]>;
  forAgent(agentType): AccountInfo[];
  resolveDefault(agentType): number | null;  // last-used → first → null
  rememberLastUsed(agentType, id): Promise<void>;
}
```

**UI components:**

- `AccountBadge` — rendered in `TerminalPane` header, only for
  `agent_type ∈ {"claude", "codex"}`. Shows `Claude · work` or `Claude ·
  default` (muted). Click opens a popover listing the agent's accounts plus
  `+ Add account…` and `⚙ Manage…`.
- `AccountSwitchDialog` — confirm modal. Explains: "Restarts the process,
  conversation context will be lost."
- `AccountsManager` — modal (also reachable from Settings). Two sections
  (Claude / Codex), per-account: name + truncated configDir + rename +
  delete. "Detect accounts" button populates a checkbox list of suggestions.
  "Add manually" form: agent dropdown, name, configDir picker (Tauri
  `dialog::open_folder`). Inline help: "To create credentials, run
  `CLAUDE_CONFIG_DIR=~/.claude-work claude` in your terminal and complete
  `/login`. Then add the account here."

**New-session dialog:** if accounts exist for the chosen agent type, show an
`Account` dropdown defaulted via `resolveDefault`. If no accounts exist for
that type, the dropdown is hidden — behaviour matches today's app.

### Switch flow on a live session

In `useSessionActions.switchSessionAccount(ptyId, newAccountId)`:

1. Read the session's `command`, `launch_args` (deserialize JSON),
   `working_dir`, `worktree_path`, `task_id` from the store.
2. Show `AccountSwitchDialog`. Cancel = exit.
3. On confirm:
   1. Strip resume-related args. For Claude: drop `--resume <id>` (pair),
      `--continue`, and `--session-id <id>`. For Codex: drop `resume <id>`
      subcommand pair and `--last`.
   2. `killSession(ptyId)`.
   3. `createSession({ command, args: cleaned, working_dir, account_id:
      newAccountId, replace_id: ptyId, ... })`.
      The existing `replaceId` plumbing in `sessionStore.addSession`
      preserves UI ordering and the visual slot.
   4. `accountStore.rememberLastUsed(agent_type, newAccountId)`.

The badge updates automatically because it reads
`sessions[ptyId].account_id`.

## Edge cases

1. **Delete an account that's used by a live session** — FK SET NULL flips
   the badge to "default", running process keeps its env. Confirm dialog
   shows live-session count if > 0.
2. **configDir vanishes off disk between sessions** — `dunce::canonicalize`
   fails at spawn; surface as toast. In Manage Accounts, missing paths
   render red (one-shot existence check on store load).
3. **Duplicate `(agent_type, config_dir)`** — UNIQUE constraint rejects;
   frontend shows "An account for this configDir already exists: '<name>'".
4. **Relative path on add** — backend resolves against `home_dir` then
   canonicalizes before insert. DB always stores absolute paths.
5. **Tilde expansion** — `~/foo` is expanded backend-side before
   canonicalize. Tauri does not expand tildes.
6. **Stale `last_used` pointing at deleted account** — `resolveDefault`
   validates against current accounts list, falls back to first then null.
   Cleared on next `rememberLastUsed`.
7. **Auto-detect false positives** — directories without a known marker file
   are skipped (`~/.claude-old-backup` doesn't appear).
8. **Switch invoked on unsupported agent** — badge isn't rendered for
   shell/cursor/gemini/opencode; action is unreachable from UI.
9. **Switch to current account** — current entry is marked `●` and
   non-interactive in the popover.
10. **Pre-migration sessions on first launch after upgrade** — all get
    `account_id = NULL`; behave as default.
11. **`--resume`/`--session-id` left over in args** — explicitly stripped
    before relaunch (see flow above). A fresh `--session-id <uuid>` is
    generated by the existing path in `session_manager.rs:195`.
12. **Parent shell has `CLAUDE_CONFIG_DIR` exported** — does not leak
    (whitelist exclusion preserved). A new no-account session uses
    `~/.claude`.
13. **Codex resume + account swap** — `codex resume <thread_id>` references
    an ID in the old configDir's SQLite. Stripped from args alongside
    `--resume` for Claude.

## Testing

### Rust unit tests

- **`db.rs`**: insert returns id; UNIQUE rejections on `(agent_type, name)`
  and `(agent_type, config_dir)`; `list_accounts_by_agent` filters and sorts;
  delete sets `sessions.account_id = NULL` (FK behaviour); update name;
  migration adds `account_id` column to pre-migration schemas (open
  `:memory:`, apply pre-migration DDL, run migration, assert).
- **`pty_manager.rs`**: `extra_env` is merged after whitelist; collisions
  resolve to `extra_env`; empty `extra_env` is a no-op (regression).
- **`session_manager.rs`**: account_id injects the correct env key per agent
  type; shell/cursor returns `InvalidAgentForAccount`; missing
  account_id → `AccountNotFound`; missing configDir →
  `AccountConfigDirMissing`; no account_id → empty extra_env (back-compat).
- **`adapters/mod.rs`**: `env_key_for_agent` for claude/codex/unknown.
- **`accounts/detect.rs`** (new module): skips dirs without marker files;
  excludes already-registered; handles missing `$HOME` gracefully.
- **Tilde + canonicalize helpers**: tilde expansion; non-tilde passthrough;
  relative path canonicalization at add time.

### Frontend tests (Vitest + RTL)

- **`accountStore`**: load populates state; `resolveDefault` last-used →
  first → null; `forAgent` filters and sorts; `rememberLastUsed` persists.
- **`AccountBadge`**: hidden for unsupported agents; "default" label when
  null or stale; account name when resolved; popover lists only matching
  agent.
- **`switchSessionAccount`**: cleans Claude resume args; cleans Codex resume
  subcommand; orders `killSession` → `createSession(replaceId)`; cancel
  short-circuits; `rememberLastUsed` called on success.
- **`AccountsManager`**: invalid configDir surfaces IPC error; delete with
  active sessions warns with count; detect populates suggestions, check adds
  to store.

### Manual smoke checklist (pre-merge)

- [ ] `CLAUDE_CONFIG_DIR=~/.claude-test1 claude` → `/login` → add in AICoder
  → start session → `/status` shows test1 account.
- [ ] Switch account on a live session via popover → same UI slot, new
  process under the new account.
- [ ] Delete an active account → badge becomes "default", running process
  keeps working.
- [ ] Open AICoder with `CLAUDE_CONFIG_DIR=foo` exported in the parent
  shell → no-account session uses `~/.claude`, not `foo`.
- [ ] Codex account add + swap + relaunch.
- [ ] Delete a configDir via Finder → next session under that account shows
  a clear error.

### Out of scope for automated tests

- Real OAuth login flows.
- Visual regression of the badge.
- Windows path/tilde handling — single-iteration follow-up.

## Files touched (forward-looking; informational, not normative)

Backend:

- `src-tauri/src/db.rs` — schema migration, account CRUD methods,
  `account_id` on sessions.
- `src-tauri/src/pty_manager.rs` — `extra_env` parameter on
  `create_session`; merge after whitelist.
- `src-tauri/src/session_manager.rs` — `account_id` on
  `CreateSessionParams`; resolve to env injection.
- `src-tauri/src/adapters/mod.rs` — `env: HashMap<…>` on `LaunchConfig`;
  `env_key_for_agent` constant.
- `src-tauri/src/accounts/detect.rs` — new module for configDir scanning.
- `src-tauri/src/lib.rs` — six new IPC commands; `account_id` on
  `CreateSessionRequest`.
- `src-tauri/src/error.rs` — three new error variants.

Frontend:

- `src/lib/api.ts` — new wrappers + types; `account_id` on existing types.
- `src/store/accountStore.ts` — new store.
- `src/components/AccountBadge.tsx` — new.
- `src/components/AccountsManager.tsx` — new modal.
- `src/components/AccountSwitchDialog.tsx` — new confirm dialog.
- `src/components/TerminalPane.tsx` — render `AccountBadge` in header.
- `src/components/NewSessionDialog.tsx` — account dropdown.
- `src/hooks/useSessionActions.ts` — `switchSessionAccount` action.
- `src/components/Settings*.tsx` — entry point for Manage Accounts.

## Open questions

None remaining from brainstorming. Implementation plan is the next artifact
(produced by the writing-plans skill from this spec).
