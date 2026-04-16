use crate::db::Db;
use crate::error::AppError;
use crate::pty_manager::PtyManager;
use std::sync::Arc;
use tauri::AppHandle;

pub struct SessionManager {
    pub pty: PtyManager,
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
            pty: PtyManager::new(start_id),
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

        let mut args: Vec<String> = params.args.to_vec();
        if let Some(ref sid) = agent_session_id {
            args.push("--session-id".to_string());
            args.push(sid.clone());
        }

        // Record timestamp immediately before spawning so polling can exclude
        // pre-existing threads in the same working directory.
        let launch_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // If task_id is set, look up the task's worktree_path and use it for both
        // the PTY cwd and the session's denormalized worktree_path. This keeps task
        // as source of truth for worktree lifecycle while preserving existing
        // session-level worktree reads (WorktreeIcon, git polling).
        let (task_worktree_path, task_base_commit) = if let Some(tid) = params.task_id {
            match self.db.get_task_by_id(tid) {
                Ok(Some(task)) => (task.worktree_path, task.base_commit),
                _ => (None, None),
            }
        } else {
            (None, None)
        };

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

    pub fn list_sessions_for_project(
        &self,
        project_path: &str,
    ) -> Result<Vec<crate::db::SessionRow>, AppError> {
        self.db
            .list_sessions_by_project_path(project_path)
            .map_err(|e| AppError::Db(e.to_string()))
    }
}
