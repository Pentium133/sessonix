use crate::db::Db;
use crate::error::AppError;
use crate::pty_manager::PtyManager;
use rusqlite::Connection;
use std::sync::Arc;
use tauri::AppHandle;

pub struct SessionManager {
    pub pty: Arc<PtyManager>,
    pub db: Arc<Db>,
}

/// Read the most recent thread ID from Codex's local SQLite database,
/// created at or after `not_before_secs` (Unix timestamp).
///
/// Codex stores threads in `~/.codex/state_5.sqlite` table `threads`.
/// The `not_before_secs` guard prevents capturing a stale thread from a
/// previous session in the same working directory.
///
/// Thread IDs are UUIDs (max 128 hex chars); we cap at 256 to guard against
/// a crafted database.
pub fn read_codex_thread_id(working_dir: &str, not_before_secs: i64) -> Option<String> {
    let home = dirs::home_dir()?;
    let db_path = home.join(".codex").join("state_5.sqlite");
    if !db_path.exists() {
        return None;
    }
    // Open read-only to avoid interfering with Codex
    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ).ok()?;
    let id: String = conn.query_row(
        "SELECT id FROM threads WHERE cwd = ?1 AND created_at >= ?2 ORDER BY created_at DESC LIMIT 1",
        rusqlite::params![working_dir, not_before_secs],
        |row| row.get(0),
    ).ok()?;
    // Guard against a crafted database with an absurdly large ID string
    if id.len() > 256 {
        return None;
    }
    Some(id)
}

/// Default location of OpenCode's SQLite database.
///
/// OpenCode uses XDG paths on all platforms (confirmed macOS 1.4.3),
/// so `$HOME/.local/share/opencode/opencode.db` — not
/// `~/Library/Application Support`.
pub(crate) fn opencode_db_path() -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".local").join("share").join("opencode").join("opencode.db"))
}

/// Open OpenCode's DB read-only with NO_MUTEX (single-thread use) and no
/// TOCTOU pre-check — the only failure mode of interest is "couldn't open",
/// which `.ok()?` reports as `None`. Callers retry on `None` returns.
pub(crate) fn open_opencode_db(path: &std::path::Path) -> Option<Connection> {
    Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()
}

/// Query `conn` for the most recent OpenCode session ID matching `working_dir`
/// and created at or after `not_before_millis` (milliseconds, matching
/// `session.time_created`).
///
/// Guards: rejects IDs >256 chars, without the `ses_` prefix, or containing
/// anything other than `[a-zA-Z0-9_]` — defends against log-injection and
/// downstream rendering of ANSI/control characters from a crafted DB.
pub(crate) fn query_opencode_session_id(
    conn: &Connection,
    working_dir: &str,
    not_before_millis: i64,
) -> Option<String> {
    let id: String = conn
        .query_row(
            "SELECT id FROM session \
             WHERE directory = ?1 AND time_created >= ?2 \
             ORDER BY time_created DESC LIMIT 1",
            rusqlite::params![working_dir, not_before_millis],
            |row| row.get(0),
        )
        .ok()?;
    if id.len() > 256 || !id.starts_with("ses_") {
        return None;
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(id)
}

/// Path-parametrised convenience wrapper used by tests. Production polling
/// uses `open_opencode_db` + `query_opencode_session_id` directly to reuse a
/// single connection across ticks.
#[cfg(test)]
pub(crate) fn read_opencode_session_id_from_path(
    db_path: &std::path::Path,
    working_dir: &str,
    not_before_millis: i64,
) -> Option<String> {
    let conn = open_opencode_db(db_path)?;
    query_opencode_session_id(&conn, working_dir, not_before_millis)
}

/// Pull the `ses_xxx` OpenCode session ID out of resume-style args.
///
/// Expects args shaped like `["--session", "ses_...", ...]` (the flag may
/// appear anywhere in the list). Returns `None` for `--continue` — in that
/// case the ID is only known after polling, and callers should not
/// prematurely store a stale value.
///
/// Guards: the candidate ID must start with `ses_` — anything else is
/// treated as absent so polling takes over.
pub(crate) fn extract_opencode_resume_id(args: &[String]) -> Option<String> {
    let idx = args.iter().position(|a| a == "--session")?;
    let candidate = args.get(idx + 1)?;
    if candidate.starts_with("ses_") {
        Some(candidate.clone())
    } else {
        None
    }
}

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
}

impl SessionManager {
    pub fn new(db: Arc<Db>) -> Self {
        if let Err(e) = db.mark_all_running_as_exited() {
            log::warn!("Failed to mark stale sessions: {}", e);
        }

        // Start PTY IDs after the highest existing ID to prevent collisions
        let start_id = db.max_pty_id().unwrap_or(0) + 1;

        Self {
            pty: Arc::new(PtyManager::new(start_id)),
            db,
        }
    }

    pub fn add_project(&self, name: &str, path: &str) -> Result<i64, AppError> {
        self.db
            .insert_project(name, path)
            .map_err(|e| AppError::Db(e.to_string()))
    }

    pub fn remove_project(&self, path: &str) -> Result<(), AppError> {
        self.db
            .delete_project(path)
            .map_err(|e| AppError::Db(e.to_string()))
    }

    pub fn list_projects(&self) -> Result<Vec<crate::db::ProjectRow>, AppError> {
        self.db
            .list_projects()
            .map_err(|e| AppError::Db(e.to_string()))
    }

    pub fn create_session(&self, params: CreateSessionParams<'_>) -> Result<u32, AppError> {
        let dir_name = params
            .working_dir
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(params.working_dir);
        let project_id = self
            .db
            .insert_project(dir_name, params.working_dir)
            .map_err(|e| AppError::Db(e.to_string()))?;

        // For Claude: generate a stable session ID for new sessions only.
        // Skip if --resume (relaunch with existing ID) or --continue (resume last session).
        let is_claude = params.agent_type == "claude";
        let is_codex = params.agent_type == "codex";
        let is_opencode = params.agent_type == "opencode";
        let is_resume = params.args.iter().any(|a| a == "--resume");
        let is_continue = params.args.iter().any(|a| a == "--continue");
        let agent_session_id = if is_claude && !is_resume && !is_continue {
            Some(uuid::Uuid::new_v4().to_string())
        } else {
            None
        };

        // For Codex resume: the command is "codex" and first arg is "resume" subcommand.
        // Extract the thread ID from args for storage.
        // "resume --last" is NOT a real thread ID — store None so the fallback
        // path picks up the correct ID after polling.
        let is_codex_resume = is_codex && params.args.first().map(|a| a.as_str()) == Some("resume");
        let codex_resume_id = if is_codex_resume {
            params.args.get(1)
                .filter(|id| *id != "--last")
                .cloned()
        } else {
            None
        };

        // OpenCode resume: args carry `--session <ses_id>` (specific) or
        // `--continue` (last session in cwd). For `--continue` the ID is
        // unknown until the agent registers it, so polling fills it in.
        let opencode_resume_id = if is_opencode {
            extract_opencode_resume_id(params.args)
        } else {
            None
        };

        let mut args: Vec<String> = params.args.to_vec();
        if let Some(ref sid) = agent_session_id {
            args.push("--session-id".to_string());
            args.push(sid.clone());
        }

        // Record timestamp immediately before spawning so polling can exclude
        // pre-existing threads in the same working directory.
        // Codex stores created_at in seconds; OpenCode stores time_created in ms.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let launch_ts = i64::try_from(now.as_secs()).unwrap_or(i64::MAX);
        let launch_ts_ms = i64::try_from(now.as_millis()).unwrap_or(i64::MAX);

        // If task_id is set, look up the task's worktree_path and use it for both
        // the PTY cwd and the session's denormalized worktree_path. This keeps task
        // as source of truth for worktree lifecycle while preserving existing
        // session-level worktree reads (WorktreeIcon, git polling).
        let (task_worktree_path, task_base_commit) = self.resolve_task_worktree(params.task_id);

        // Use (in priority): task worktree → explicit params.worktree_path → working_dir.
        // working_dir is always the project root (for project grouping in DB).
        let effective_worktree_path = task_worktree_path.as_deref().or(params.worktree_path);
        let effective_base_commit = task_base_commit.as_deref().or(params.base_commit);
        let pty_cwd = effective_worktree_path.unwrap_or(params.working_dir);
        let pty_id = self.pty.create_session(
            params.command,
            &args,
            pty_cwd,
            params.cols,
            params.rows,
            params.app_handle,
        )?;

        // For Claude resume, extract the session ID from args (--resume <id>)
        let stored_session_id = if is_resume {
            params.args.iter()
                .position(|a| a == "--resume")
                .and_then(|i| params.args.get(i + 1))
                .map(|s| s.to_string())
        } else if let Some(ref cid) = codex_resume_id {
            Some(cid.clone())
        } else if let Some(ref oid) = opencode_resume_id {
            Some(oid.clone())
        } else {
            agent_session_id
        };

        let args_json =
            serde_json::to_string(params.args).unwrap_or_else(|_| "[]".to_string());
        self.db
            .insert_session(&crate::db::InsertSession {
                project_id,
                pty_id,
                agent_type: params.agent_type,
                task_name: params.task_name,
                working_dir: params.working_dir,
                command: params.command,
                args: &args_json,
                agent_session_id: stored_session_id.as_deref(),
                worktree_path: effective_worktree_path,
                base_commit: effective_base_commit,
                initial_prompt: params.prompt,
                task_id: params.task_id,
            })
            .map_err(|e| AppError::Db(e.to_string()))?;

        // For new Codex sessions (not resume): poll Codex's SQLite to capture the thread ID.
        // Codex assigns the thread ID server-side after launch, so we retry with a backoff.
        // The launch_ts guard ensures we only accept threads created AFTER this session
        // started, preventing false matches from previous sessions in the same cwd.
        if is_codex && !is_codex_resume {
            let db = self.db.clone();
            let wd = params.working_dir.to_string();
            std::thread::spawn(move || {
                for _ in 0..10 {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    if let Some(thread_id) = read_codex_thread_id(&wd, launch_ts) {
                        if let Err(e) = db.update_agent_session_id(pty_id, &thread_id) {
                            log::warn!("Failed to save Codex thread ID: {}", e);
                        } else {
                            log::info!("Captured Codex thread ID: {} for pty {}", thread_id, pty_id);
                        }
                        return;
                    }
                }
                log::warn!("Could not capture Codex thread ID for pty {} after 5s", pty_id);
            });
        }

        // For OpenCode sessions where the ID isn't yet known (new session, or
        // `--continue` which defers ID selection to the CLI): poll OpenCode's
        // SQLite to pick up the `ses_xxx` assigned after launch. Same backoff
        // as Codex. Polling runs against the session's actual cwd (task worktree
        // when applicable), not the project root — that's what OpenCode stores
        // in `session.directory`.
        //
        // The DB connection is opened once and reused across the 10 polling
        // ticks: each tick is one `query_row`, not a full open/close cycle.
        // If the DB doesn't exist yet (OpenCode hasn't written it), we retry
        // opening on subsequent ticks until either success or timeout.
        if is_opencode && opencode_resume_id.is_none() {
            let db = self.db.clone();
            let wd = pty_cwd.to_string();
            std::thread::spawn(move || {
                let Some(db_path) = opencode_db_path() else {
                    log::warn!("Could not locate OpenCode DB for pty {}", pty_id);
                    return;
                };
                let mut conn: Option<Connection> = None;
                for _ in 0..10 {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    if conn.is_none() {
                        conn = open_opencode_db(&db_path);
                    }
                    let Some(c) = conn.as_ref() else { continue };
                    if let Some(session_id) = query_opencode_session_id(c, &wd, launch_ts_ms) {
                        if let Err(e) = db.update_agent_session_id(pty_id, &session_id) {
                            log::warn!("failed to save opencode session id: {}", e);
                        } else {
                            log::info!(
                                "captured opencode session id for pty {}",
                                pty_id
                            );
                        }
                        return;
                    }
                }
                log::warn!("could not capture opencode session id for pty {} after 5s", pty_id);
            });
        }

        // Cursor: no local session store to poll (unlike Codex/OpenCode which
        // expose SQLite DBs). Relaunch relies on either `--continue` (last
        // session in cwd, handled by the `agent` CLI itself) or a manual UUID
        // via `--resume <id>`, captured in `stored_session_id` above.

        // Shell/custom: write prompt to stdin after delay so the shell has time to init.
        // Claude/Codex/Gemini: prompt passed as CLI positional arg, not stdin.
        let needs_stdin_prompt = matches!(params.agent_type, "shell" | "custom");
        if needs_stdin_prompt {
            if let Some(prompt) = params.prompt {
                let prompt = format!("{}\n", prompt);
                if let Ok(session) = self.pty.get_session(pty_id) {
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(2000));
                        if let Err(e) = session.write_input(prompt.as_bytes()) {
                            log::warn!("Failed to write prompt to shell stdin: {}", e);
                        }
                    });
                }
            }
        }

        Ok(pty_id)
    }

    pub fn on_session_exit(&self, pty_id: u32) {
        if let Err(e) = self.db.update_session_status(pty_id, "exited", None) {
            log::warn!("Failed to update session {} status: {}", pty_id, e);
        }
    }

    /// Resolve a task's worktree path and base commit by id. Returns
    /// (None, None) when task_id is None or the task row is missing.
    /// Extracted so it can be exercised without spawning a PTY.
    pub(crate) fn resolve_task_worktree(
        &self,
        task_id: Option<i64>,
    ) -> (Option<String>, Option<String>) {
        let Some(tid) = task_id else { return (None, None); };
        match self.db.get_task_by_id(tid) {
            Ok(Some(task)) => (task.worktree_path, task.base_commit),
            _ => (None, None),
        }
    }
}

#[cfg(test)]
mod task_worktree_resolution_tests {
    use super::*;
    use crate::db::Db;
    use std::sync::Arc;

    fn fresh_db() -> Arc<Db> {
        // :memory: instances isolate each test.
        Arc::new(Db::open_in_memory().expect("open in-memory db"))
    }

    fn make_mgr() -> SessionManager {
        SessionManager::new(fresh_db())
    }

    #[test]
    fn returns_none_when_task_id_is_none() {
        let mgr = make_mgr();
        let (wt, bc) = mgr.resolve_task_worktree(None);
        assert!(wt.is_none());
        assert!(bc.is_none());
    }

    #[test]
    fn returns_none_when_task_id_missing_in_db() {
        let mgr = make_mgr();
        let (wt, bc) = mgr.resolve_task_worktree(Some(9999));
        assert!(wt.is_none());
        assert!(bc.is_none());
    }

    #[test]
    fn returns_task_worktree_path_and_base_commit() {
        let mgr = make_mgr();
        let project_id = mgr.db.insert_project("p", "/tmp/p").unwrap();
        let task_id = mgr
            .db
            .insert_task(
                project_id,
                "t1",
                Some("feat/x"),
                Some("/tmp/p/.sessonix-worktrees/feat-x"),
                Some("abc123"),
            )
            .unwrap();

        let (wt, bc) = mgr.resolve_task_worktree(Some(task_id));
        assert_eq!(wt.as_deref(), Some("/tmp/p/.sessonix-worktrees/feat-x"));
        assert_eq!(bc.as_deref(), Some("abc123"));
    }

    #[test]
    fn returns_none_fields_when_task_has_no_worktree() {
        let mgr = make_mgr();
        let project_id = mgr.db.insert_project("p", "/tmp/p").unwrap();
        let task_id = mgr
            .db
            .insert_task(project_id, "t1", None, None, None)
            .unwrap();

        let (wt, bc) = mgr.resolve_task_worktree(Some(task_id));
        assert!(wt.is_none());
        assert!(bc.is_none());
    }
}

#[cfg(test)]
mod opencode_session_id_tests {
    use super::*;
    use rusqlite::Connection;
    use std::path::PathBuf;

    /// Build a temp path + create a throwaway on-disk SQLite with OpenCode's
    /// real `session` schema (subset we query), populated with `rows`.
    ///
    /// Returns `(path, _guard)` — the guard deletes the file on drop.
    fn fixture_db(rows: &[(&str, &str, i64)]) -> (PathBuf, TempFile) {
        let path = std::env::temp_dir().join(format!(
            "opencode-test-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let conn = Connection::open(&path).expect("open fixture db");
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                directory TEXT NOT NULL,
                time_created INTEGER NOT NULL
            );",
        )
        .expect("create table");
        for (id, dir, ts) in rows {
            conn.execute(
                "INSERT INTO session (id, directory, time_created) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, dir, ts],
            )
            .expect("insert fixture row");
        }
        drop(conn);
        (path.clone(), TempFile(path))
    }

    struct TempFile(PathBuf);
    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn returns_none_when_db_missing() {
        let path = PathBuf::from("/nonexistent/opencode-does-not-exist.db");
        let id = read_opencode_session_id_from_path(&path, "/tmp", 0);
        assert!(id.is_none());
    }

    #[test]
    fn returns_most_recent_matching_session() {
        let (path, _guard) = fixture_db(&[
            ("ses_oldMatching", "/tmp/proj", 1_000),
            ("ses_newMatching", "/tmp/proj", 2_000),
            ("ses_otherDir", "/tmp/other", 3_000),
        ]);
        let id = read_opencode_session_id_from_path(&path, "/tmp/proj", 0);
        assert_eq!(id.as_deref(), Some("ses_newMatching"));
    }

    #[test]
    fn respects_timestamp_guard() {
        let (path, _guard) = fixture_db(&[
            ("ses_stale", "/tmp/proj", 500),
            ("ses_fresh", "/tmp/proj", 1_500),
        ]);
        // Cutoff between the two — only "fresh" qualifies.
        let id = read_opencode_session_id_from_path(&path, "/tmp/proj", 1_000);
        assert_eq!(id.as_deref(), Some("ses_fresh"));
    }

    #[test]
    fn filters_by_directory() {
        let (path, _guard) = fixture_db(&[
            ("ses_wrongDir", "/tmp/other", 2_000),
        ]);
        let id = read_opencode_session_id_from_path(&path, "/tmp/proj", 0);
        assert!(id.is_none());
    }

    #[test]
    fn rejects_oversized_id() {
        let huge = format!("ses_{}", "x".repeat(300));
        let (path, _guard) = fixture_db(&[(&huge, "/tmp/proj", 1_000)]);
        let id = read_opencode_session_id_from_path(&path, "/tmp/proj", 0);
        assert!(id.is_none(), "expected None for id longer than 256 chars");
    }

    #[test]
    fn rejects_non_ses_prefix() {
        let (path, _guard) = fixture_db(&[
            ("corrupted_not_ses", "/tmp/proj", 1_000),
        ]);
        let id = read_opencode_session_id_from_path(&path, "/tmp/proj", 0);
        assert!(id.is_none(), "expected None for ID without ses_ prefix");
    }

    #[test]
    fn rejects_control_chars_in_id() {
        // Security guard: even with a valid ses_ prefix and length, an ID
        // with embedded control chars (ANSI, newlines) must not leak through
        // to logs or the frontend.
        let poisoned = "ses_\x1b[31mpwn\n";
        let (path, _guard) = fixture_db(&[(poisoned, "/tmp/proj", 1_000)]);
        let id = read_opencode_session_id_from_path(&path, "/tmp/proj", 0);
        assert!(id.is_none(), "expected None for ID with control characters");
    }

    #[test]
    fn returns_none_on_corrupt_schema() {
        // OpenCode DB exists but doesn't have the expected `session` table.
        // This is the most realistic failure mode — a version mismatch or
        // a user deleting table rows. Must degrade gracefully.
        let path = std::env::temp_dir().join(format!(
            "opencode-corrupt-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE unrelated (x INTEGER);")
            .unwrap();
        drop(conn);
        let _guard = TempFile(path.clone());
        let id = read_opencode_session_id_from_path(&path, "/tmp/proj", 0);
        assert!(id.is_none(), "expected None when `session` table missing");
    }

    #[test]
    fn default_path_points_into_xdg_share() {
        // Sanity: default path ends with the expected suffix on any platform.
        let p = opencode_db_path().expect("home dir available in test env");
        let s = p.to_string_lossy();
        assert!(
            s.ends_with(".local/share/opencode/opencode.db"),
            "unexpected default opencode db path: {s}"
        );
    }
}

#[cfg(test)]
mod extract_opencode_resume_id_tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn finds_session_id_after_session_flag() {
        let a = args(&["--session", "ses_abc", "--prompt", "continue the task"]);
        assert_eq!(
            extract_opencode_resume_id(&a).as_deref(),
            Some("ses_abc"),
        );
    }

    #[test]
    fn continue_only_returns_none() {
        // --continue defers the ID until polling — do not premature-store.
        let a = args(&["--continue"]);
        assert!(extract_opencode_resume_id(&a).is_none());
    }

    #[test]
    fn new_session_returns_none() {
        let a = args(&["--prompt", "fix bug"]);
        assert!(extract_opencode_resume_id(&a).is_none());
    }

    #[test]
    fn rejects_non_ses_value_after_session_flag() {
        // --session followed by something that isn't a valid ses_ prefix
        // shouldn't leak into the stored_session_id.
        let a = args(&["--session", "bogus-value"]);
        assert!(extract_opencode_resume_id(&a).is_none());
    }

    #[test]
    fn session_flag_at_end_with_no_value_returns_none() {
        let a = args(&["--session"]);
        assert!(extract_opencode_resume_id(&a).is_none());
    }

    #[test]
    fn finds_session_id_in_fork_args() {
        // Fork path from useSessionActions.handleForkSession:
        // ["--fork", "--session", "ses_fork_abc"]. Keyword search must still
        // locate the ID even with --fork ahead of --session.
        let a = args(&["--fork", "--session", "ses_fork_abc"]);
        assert_eq!(
            extract_opencode_resume_id(&a).as_deref(),
            Some("ses_fork_abc"),
        );
    }

    #[test]
    fn ignores_short_session_flag() {
        // Backend contract: we only parse --session (long form). If `-s ses_x`
        // ever reaches the adapter (e.g. via Extra Args), we deliberately do
        // NOT treat it as a resume ID — document that here.
        let a = args(&["-s", "ses_short"]);
        assert!(extract_opencode_resume_id(&a).is_none());
    }
}
