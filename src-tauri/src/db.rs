use parking_lot::Mutex;
use rusqlite::{Connection, params};
use std::path::PathBuf;

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct ProjectRow {
    pub id: i64,
    pub name: String,
    pub path: String,
    /// Used for ordering and asserted in tests; the IPC layer projects rows
    /// in array order so this field is not currently serialized to the
    /// frontend.
    #[allow(dead_code)]
    pub sort_order: u32,
}

/// Full row of the `sessions` table. Some fields are selected for schema
/// completeness but not currently read by any caller; removing them would
/// reshape the struct every time a new consumer needs a previously-ignored
/// column.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SessionRow {
    pub id: i64,
    pub project_id: i64,
    pub pty_id: Option<u32>,
    pub agent_type: String,
    pub task_name: String,
    pub working_dir: String,
    pub status: String,
    pub status_line: String,
    pub exit_code: Option<i32>,
    pub launch_command: String,
    pub launch_args: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub scrollback: Option<String>,
    pub agent_session_id: Option<String>,
    pub sort_order: u32,
    pub worktree_path: Option<String>,
    pub base_commit: Option<String>,
    pub initial_prompt: Option<String>,
    pub task_id: Option<i64>,
}

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
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TaskRow {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub branch: Option<String>,
    pub worktree_path: Option<String>,
    pub base_commit: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct QuickPromptRow {
    pub id: i64,
    pub name: String,
    pub project_path: String,
    pub agent: String,
    pub initial_prompt: Option<String>,
    pub skip_permissions: bool,
    pub created_at: String,
}

impl Db {
    pub fn open(app_dir: &PathBuf) -> Result<Self, rusqlite::Error> {
        std::fs::create_dir_all(app_dir).ok();
        let db_path = app_dir.join("sessonix.db");
        let conn = Connection::open(db_path)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS projects (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                name        TEXT NOT NULL,
                path        TEXT NOT NULL UNIQUE,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id      INTEGER NOT NULL REFERENCES projects(id),
                pty_id          INTEGER,
                agent_type      TEXT NOT NULL DEFAULT 'custom',
                task_name       TEXT NOT NULL DEFAULT '',
                working_dir     TEXT NOT NULL,
                status          TEXT NOT NULL DEFAULT 'running',
                status_line     TEXT NOT NULL DEFAULT '',
                exit_code       INTEGER,
                launch_command  TEXT NOT NULL,
                launch_args     TEXT NOT NULL DEFAULT '[]',
                started_at      TEXT NOT NULL DEFAULT (datetime('now')),
                ended_at        TEXT,
                scrollback      TEXT
            );
            ",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );"
        )?;

        // Migration: add agent_session_id column
        let has_agent_col: bool = conn
            .prepare("SELECT agent_session_id FROM sessions LIMIT 0")
            .is_ok();
        if !has_agent_col {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN agent_session_id TEXT;",
            )?;
        }

        // Migration: add sort_order column
        let has_sort_col: bool = conn
            .prepare("SELECT sort_order FROM sessions LIMIT 0")
            .is_ok();
        if !has_sort_col {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;",
            )?;
            // Backfill existing rows: assign sort_order based on started_at within each project
            conn.execute_batch(
                "UPDATE sessions SET sort_order = (
                    SELECT COUNT(*) FROM sessions s2
                    WHERE s2.project_id = sessions.project_id
                    AND s2.started_at <= sessions.started_at
                    AND s2.id <= sessions.id
                );",
            )?;
        }

        // Migration: add sort_order column to projects.
        // Backfill in created_at, then id order so existing installs keep
        // their visible ordering. New projects get MAX+1 on insert.
        let has_proj_sort_col: bool = conn
            .prepare("SELECT sort_order FROM projects LIMIT 0")
            .is_ok();
        if !has_proj_sort_col {
            conn.execute_batch(
                "ALTER TABLE projects ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;",
            )?;
            conn.execute_batch(
                "UPDATE projects SET sort_order = (
                    SELECT COUNT(*) FROM projects p2
                    WHERE p2.created_at < projects.created_at
                       OR (p2.created_at = projects.created_at AND p2.id <= projects.id)
                );",
            )?;
        }

        // Migration: add worktree_path and base_commit columns (checked independently)
        let has_wt_col: bool = conn
            .prepare("SELECT worktree_path FROM sessions LIMIT 0")
            .is_ok();
        if !has_wt_col {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN worktree_path TEXT;",
            )?;
        }
        let has_bc_col: bool = conn
            .prepare("SELECT base_commit FROM sessions LIMIT 0")
            .is_ok();
        if !has_bc_col {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN base_commit TEXT;",
            )?;
        }

        // Migration: add initial_prompt column
        let has_prompt_col: bool = conn
            .prepare("SELECT initial_prompt FROM sessions LIMIT 0")
            .is_ok();
        if !has_prompt_col {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN initial_prompt TEXT;",
            )?;
        }

        // Migration: rename legacy `templates` table → `quick_prompts`.
        // Runs before the CREATE below so the empty new table doesn't block the rename.
        let has_legacy_templates: bool = conn
            .prepare("SELECT 1 FROM templates LIMIT 0")
            .is_ok();
        let has_quick_prompts: bool = conn
            .prepare("SELECT 1 FROM quick_prompts LIMIT 0")
            .is_ok();
        if has_legacy_templates && !has_quick_prompts {
            conn.execute_batch("ALTER TABLE templates RENAME TO quick_prompts;")?;
        } else if has_legacy_templates && has_quick_prompts {
            // Split-brain: both tables exist (e.g. user bounced between old/new
            // branches of a dogfooding worktree). Copy orphaned rows into the
            // live table and drop the legacy one. `INSERT OR IGNORE` skips
            // rows whose PK already landed in quick_prompts.
            conn.execute_batch(
                "INSERT OR IGNORE INTO quick_prompts
                   SELECT id, name, project_path, agent, initial_prompt, skip_permissions, created_at
                   FROM templates;
                 DROP TABLE templates;",
            )?;
        }

        // Quick prompts table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS quick_prompts (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                name            TEXT NOT NULL,
                project_path    TEXT NOT NULL,
                agent           TEXT NOT NULL DEFAULT '',
                initial_prompt  TEXT,
                skip_permissions INTEGER NOT NULL DEFAULT 0,
                created_at      TEXT NOT NULL DEFAULT (datetime('now'))
            );"
        )?;

        // Tasks table — worktree-scoped groups of sessions
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tasks (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id      INTEGER NOT NULL REFERENCES projects(id),
                name            TEXT NOT NULL,
                branch          TEXT,
                worktree_path   TEXT,
                base_commit     TEXT,
                created_at      TEXT NOT NULL DEFAULT (datetime('now'))
            );"
        )?;

        // Migration: add task_id column to sessions
        let has_task_id_col: bool = conn
            .prepare("SELECT task_id FROM sessions LIMIT 0")
            .is_ok();
        if !has_task_id_col {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN task_id INTEGER REFERENCES tasks(id);",
            )?;
        }

        // Indices for task-aware queries.
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_sessions_task_id ON sessions(task_id);
             CREATE INDEX IF NOT EXISTS idx_sessions_project_id ON sessions(project_id);
             CREATE INDEX IF NOT EXISTS idx_tasks_project_id ON tasks(project_id);",
        )?;

        Ok(())
    }

    // --- Projects ---

    pub fn insert_project(&self, name: &str, path: &str) -> Result<i64, rusqlite::Error> {
        let conn = self.conn.lock();
        // INSERT OR IGNORE skips on UNIQUE(path) conflict; sort_order comes from
        // a subquery so duplicates do not bump the counter.
        conn.execute(
            "INSERT OR IGNORE INTO projects (name, path, sort_order)
             SELECT ?1, ?2, COALESCE(MAX(sort_order), 0) + 1 FROM projects",
            params![name, path],
        )?;
        // Return existing or new id
        let id: i64 = conn.query_row(
            "SELECT id FROM projects WHERE path = ?1",
            params![path],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectRow>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, sort_order FROM projects ORDER BY sort_order ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ProjectRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    sort_order: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn reorder_project(&self, path: &str, new_order: u32) -> Result<(), rusqlite::Error> {
        if new_order < 1 {
            return Err(rusqlite::Error::InvalidParameterName(
                "new_order must be >= 1".to_string(),
            ));
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        let old_order: u32 = tx.query_row(
            "SELECT sort_order FROM projects WHERE path = ?1",
            params![path],
            |row| row.get(0),
        )?;

        if old_order == new_order {
            return Ok(());
        }

        // Clamp new_order to [1, count]
        let count: u32 = tx.query_row(
            "SELECT COUNT(*) FROM projects",
            [],
            |row| row.get(0),
        )?;
        let clamped = new_order.min(count).max(1);

        if old_order > clamped {
            // Moving up: shift the slice [clamped, old_order) down by 1
            tx.execute(
                "UPDATE projects SET sort_order = sort_order + 1
                 WHERE sort_order >= ?1 AND sort_order < ?2",
                params![clamped, old_order],
            )?;
        } else {
            // Moving down: shift the slice (old_order, clamped] up by 1
            tx.execute(
                "UPDATE projects SET sort_order = sort_order - 1
                 WHERE sort_order > ?1 AND sort_order <= ?2",
                params![old_order, clamped],
            )?;
        }
        tx.execute(
            "UPDATE projects SET sort_order = ?1 WHERE path = ?2",
            params![clamped, path],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn find_project_id_by_path(&self, path: &str) -> Result<Option<i64>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id FROM projects WHERE path = ?1")?;
        let mut rows = stmt.query(params![path])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn delete_project(&self, path: &str) -> Result<(), rusqlite::Error> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        // Delete sessions by project_id (not working_dir) to avoid cross-project deletion.
        // Sessions go first (they reference tasks via task_id and projects via project_id).
        tx.execute(
            "DELETE FROM sessions WHERE project_id = (SELECT id FROM projects WHERE path = ?1)",
            params![path],
        )?;
        // Tasks reference projects via project_id — remove before projects.
        tx.execute(
            "DELETE FROM tasks WHERE project_id = (SELECT id FROM projects WHERE path = ?1)",
            params![path],
        )?;
        tx.execute("DELETE FROM quick_prompts WHERE project_path = ?1", params![path])?;
        tx.execute("DELETE FROM projects WHERE path = ?1", params![path])?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_session_by_pty_id(&self, pty_id: u32) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM sessions WHERE pty_id = ?1", params![pty_id])?;
        Ok(())
    }

    // --- Sessions ---

    pub fn insert_session(&self, s: &InsertSession<'_>) -> Result<i64, rusqlite::Error> {
        let conn = self.conn.lock();
        let next_order: u32 = conn.query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM sessions WHERE project_id = ?1",
            params![s.project_id],
            |row| row.get::<_, u32>(0),
        )?;
        conn.execute(
            "INSERT INTO sessions (project_id, pty_id, agent_type, task_name, working_dir, launch_command, launch_args, agent_session_id, sort_order, worktree_path, base_commit, initial_prompt, task_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![s.project_id, s.pty_id, s.agent_type, s.task_name, s.working_dir, s.command, s.args, s.agent_session_id, next_order, s.worktree_path, s.base_commit, s.initial_prompt, s.task_id],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn reorder_session(&self, pty_id: u32, new_order: u32) -> Result<(), rusqlite::Error> {
        if new_order < 1 {
            return Err(rusqlite::Error::InvalidParameterName(
                "new_order must be >= 1".to_string(),
            ));
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        let (old_order, project_id): (u32, i64) = tx.query_row(
            "SELECT sort_order, project_id FROM sessions WHERE pty_id = ?1",
            params![pty_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        if old_order == new_order {
            return Ok(());
        }

        // Clamp new_order to actual session count for this project
        let session_count: u32 = tx.query_row(
            "SELECT COUNT(*) FROM sessions WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )?;
        let clamped = new_order.min(session_count).max(1);

        if old_order > clamped {
            tx.execute(
                "UPDATE sessions SET sort_order = sort_order + 1
                 WHERE project_id = ?1 AND sort_order >= ?2 AND sort_order < ?3",
                params![project_id, clamped, old_order],
            )?;
        } else {
            tx.execute(
                "UPDATE sessions SET sort_order = sort_order - 1
                 WHERE project_id = ?1 AND sort_order > ?2 AND sort_order <= ?3",
                params![project_id, old_order, clamped],
            )?;
        }
        tx.execute(
            "UPDATE sessions SET sort_order = ?1 WHERE pty_id = ?2",
            params![clamped, pty_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn update_session_status(
        &self,
        pty_id: u32,
        status: &str,
        exit_code: Option<i32>,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        if status == "exited" {
            conn.execute(
                "UPDATE sessions SET status = ?1, exit_code = ?2, ended_at = datetime('now') WHERE pty_id = ?3",
                params![status, exit_code, pty_id],
            )?;
        } else {
            conn.execute(
                "UPDATE sessions SET status = ?1 WHERE pty_id = ?2",
                params![status, pty_id],
            )?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn list_sessions_for_project(
        &self,
        project_id: i64,
    ) -> Result<Vec<SessionRow>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, pty_id, agent_type, task_name, working_dir,
                    status, status_line, exit_code, launch_command, launch_args,
                    started_at, ended_at, scrollback, agent_session_id, sort_order,
                    worktree_path, base_commit, initial_prompt, task_id
             FROM sessions WHERE project_id = ?1 ORDER BY sort_order ASC",
        )?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok(SessionRow {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    pty_id: row.get::<_, Option<u32>>(2)?,
                    agent_type: row.get(3)?,
                    task_name: row.get(4)?,
                    working_dir: row.get(5)?,
                    status: row.get(6)?,
                    status_line: row.get(7)?,
                    exit_code: row.get(8)?,
                    launch_command: row.get(9)?,
                    launch_args: row.get(10)?,
                    started_at: row.get(11)?,
                    ended_at: row.get(12)?,
                    scrollback: row.get(13)?,
                    agent_session_id: row.get(14)?,
                    sort_order: row.get::<_, u32>(15).unwrap_or(0),
                    worktree_path: row.get(16).unwrap_or(None),
                    base_commit: row.get(17).unwrap_or(None),
                    initial_prompt: row.get(18).unwrap_or(None),
                    task_id: row.get(19).unwrap_or(None),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Set sort_order directly without shifting other sessions.
    /// Used when replacing a session (relaunch) to inherit the old position.
    pub fn set_sort_order(&self, pty_id: u32, sort_order: u32) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET sort_order = ?1 WHERE pty_id = ?2",
            params![sort_order, pty_id],
        )?;
        Ok(())
    }

    pub fn update_agent_session_id(
        &self,
        pty_id: u32,
        agent_session_id: &str,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        let rows = conn.execute(
            "UPDATE sessions SET agent_session_id = ?1 WHERE pty_id = ?2",
            params![agent_session_id, pty_id],
        )?;
        if rows == 0 {
            log::warn!("update_agent_session_id: no session found for pty_id {}", pty_id);
        }
        Ok(())
    }

    pub fn clear_worktree_path(&self, pty_id: u32) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET worktree_path = NULL, base_commit = NULL WHERE pty_id = ?1",
            params![pty_id],
        )?;
        Ok(())
    }

    pub fn save_scrollback(&self, pty_id: u32, data: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET scrollback = ?1 WHERE pty_id = ?2",
            params![data, pty_id],
        )?;
        Ok(())
    }

    pub fn get_scrollback(&self, pty_id: u32) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT scrollback FROM sessions WHERE pty_id = ?1",
            params![pty_id],
            |row| row.get(0),
        )
    }

    pub fn max_pty_id(&self) -> Result<u32, rusqlite::Error> {
        let conn = self.conn.lock();
        let max: Option<u32> = conn.query_row(
            "SELECT MAX(pty_id) FROM sessions",
            [],
            |row| row.get(0),
        )?;
        Ok(max.unwrap_or(0))
    }

    /// List sessions for a project identified by path, using a single JOIN query.
    pub fn list_sessions_by_project_path(
        &self,
        project_path: &str,
    ) -> Result<Vec<SessionRow>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.project_id, s.pty_id, s.agent_type, s.task_name, s.working_dir,
                    s.status, s.status_line, s.exit_code, s.launch_command, s.launch_args,
                    s.started_at, s.ended_at, s.scrollback, s.agent_session_id, s.sort_order,
                    s.worktree_path, s.base_commit, s.initial_prompt, s.task_id
             FROM sessions s
             INNER JOIN projects p ON s.project_id = p.id
             WHERE p.path = ?1
             ORDER BY s.sort_order ASC",
        )?;
        let rows = stmt
            .query_map(params![project_path], |row| {
                Ok(SessionRow {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    pty_id: row.get::<_, Option<u32>>(2)?,
                    agent_type: row.get(3)?,
                    task_name: row.get(4)?,
                    working_dir: row.get(5)?,
                    status: row.get(6)?,
                    status_line: row.get(7)?,
                    exit_code: row.get(8)?,
                    launch_command: row.get(9)?,
                    launch_args: row.get(10)?,
                    started_at: row.get(11)?,
                    ended_at: row.get(12)?,
                    scrollback: row.get(13)?,
                    agent_session_id: row.get(14)?,
                    sort_order: row.get::<_, u32>(15).unwrap_or(0),
                    worktree_path: row.get(16).unwrap_or(None),
                    base_commit: row.get(17).unwrap_or(None),
                    initial_prompt: row.get(18).unwrap_or(None),
                    task_id: row.get(19).unwrap_or(None),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // --- Tasks ---

    /// Insert a new task. Returns the new row id.
    pub fn insert_task(
        &self,
        project_id: i64,
        name: &str,
        branch: Option<&str>,
        worktree_path: Option<&str>,
        base_commit: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO tasks (project_id, name, branch, worktree_path, base_commit)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![project_id, name, branch, worktree_path, base_commit],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// List all tasks for a project identified by path, ordered by created_at ASC.
    pub fn list_tasks_by_project_path(
        &self,
        project_path: &str,
    ) -> Result<Vec<TaskRow>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT t.id, t.project_id, t.name, t.branch, t.worktree_path, t.base_commit, t.created_at
             FROM tasks t
             INNER JOIN projects p ON t.project_id = p.id
             WHERE p.path = ?1
             ORDER BY t.created_at ASC",
        )?;
        let rows = stmt
            .query_map(params![project_path], |row| {
                Ok(TaskRow {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    name: row.get(2)?,
                    branch: row.get(3)?,
                    worktree_path: row.get(4)?,
                    base_commit: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Fetch a single task by id. Returns None if not found.
    pub fn get_task_by_id(&self, task_id: i64) -> Result<Option<TaskRow>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, name, branch, worktree_path, base_commit, created_at
             FROM tasks WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![task_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(TaskRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                name: row.get(2)?,
                branch: row.get(3)?,
                worktree_path: row.get(4)?,
                base_commit: row.get(5)?,
                created_at: row.get(6)?,
            })),
            None => Ok(None),
        }
    }

    /// List all sessions belonging to a task (any status). Needed for kill cascade on delete_task.
    pub fn list_sessions_by_task_id(
        &self,
        task_id: i64,
    ) -> Result<Vec<SessionRow>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, pty_id, agent_type, task_name, working_dir,
                    status, status_line, exit_code, launch_command, launch_args,
                    started_at, ended_at, scrollback, agent_session_id, sort_order,
                    worktree_path, base_commit, initial_prompt, task_id
             FROM sessions WHERE task_id = ?1 ORDER BY sort_order ASC",
        )?;
        let rows = stmt
            .query_map(params![task_id], |row| {
                Ok(SessionRow {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    pty_id: row.get::<_, Option<u32>>(2)?,
                    agent_type: row.get(3)?,
                    task_name: row.get(4)?,
                    working_dir: row.get(5)?,
                    status: row.get(6)?,
                    status_line: row.get(7)?,
                    exit_code: row.get(8)?,
                    launch_command: row.get(9)?,
                    launch_args: row.get(10)?,
                    started_at: row.get(11)?,
                    ended_at: row.get(12)?,
                    scrollback: row.get(13)?,
                    agent_session_id: row.get(14)?,
                    sort_order: row.get::<_, u32>(15).unwrap_or(0),
                    worktree_path: row.get(16).unwrap_or(None),
                    base_commit: row.get(17).unwrap_or(None),
                    initial_prompt: row.get(18).unwrap_or(None),
                    task_id: row.get(19).unwrap_or(None),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Delete a task and all its sessions in a single transaction.
    /// Note: PTY processes must be killed BEFORE calling this (see lib.rs::delete_task).
    pub fn delete_task(&self, task_id: i64) -> Result<(), rusqlite::Error> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM sessions WHERE task_id = ?1", params![task_id])?;
        tx.execute("DELETE FROM tasks WHERE id = ?1", params![task_id])?;
        tx.commit()?;
        Ok(())
    }

    // --- Quick prompts ---

    pub fn insert_quick_prompt(
        &self,
        name: &str,
        project_path: &str,
        agent: &str,
        initial_prompt: Option<&str>,
        skip_permissions: bool,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO quick_prompts (name, project_path, agent, initial_prompt, skip_permissions)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, project_path, agent, initial_prompt, skip_permissions as i32],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_quick_prompts(&self, project_path: &str) -> Result<Vec<QuickPromptRow>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, project_path, agent, initial_prompt, skip_permissions, created_at
             FROM quick_prompts WHERE project_path = ?1 ORDER BY name ASC",
        )?;
        let rows = stmt
            .query_map(params![project_path], |row| {
                Ok(QuickPromptRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    project_path: row.get(2)?,
                    agent: row.get(3)?,
                    initial_prompt: row.get(4)?,
                    skip_permissions: row.get::<_, i32>(5)? != 0,
                    created_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_quick_prompt(&self, id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM quick_prompts WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn update_quick_prompt(
        &self,
        id: i64,
        name: &str,
        initial_prompt: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE quick_prompts SET name = ?1, initial_prompt = ?2 WHERE id = ?3",
            params![name, initial_prompt, id],
        )?;
        Ok(())
    }

    // --- Settings ---

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_all_settings(&self) -> Result<Vec<(String, String)>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key ASC")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn mark_all_running_as_exited(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock();
        let count = conn.execute(
            "UPDATE sessions SET status = 'exited', ended_at = datetime('now') WHERE status = 'running'",
            [],
        )?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrations() {
        let db = Db::open_in_memory().unwrap();
        // Should not panic on second migrate
        db.migrate().unwrap();
    }

    #[test]
    fn test_project_crud() {
        let db = Db::open_in_memory().unwrap();
        let id = db.insert_project("myapp", "/home/user/myapp").unwrap();
        assert!(id > 0);

        let projects = db.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "myapp");
        assert_eq!(projects[0].path, "/home/user/myapp");

        // Duplicate insert returns same id
        let id2 = db.insert_project("myapp", "/home/user/myapp").unwrap();
        assert_eq!(id, id2);
        assert_eq!(db.list_projects().unwrap().len(), 1);

        db.delete_project("/home/user/myapp").unwrap();
        assert_eq!(db.list_projects().unwrap().len(), 0);
    }

    #[test]
    fn test_list_projects_preserves_insertion_order() {
        // Regression: new projects appear at the bottom on create, but used to jump
        // to the top after restart because list_projects ordered by created_at DESC.
        // After SPEC-001, ordering is driven by sort_order (assigned MAX+1 on insert).
        let db = Db::open_in_memory().unwrap();
        db.insert_project("alpha", "/tmp/alpha").unwrap();
        db.insert_project("beta", "/tmp/beta").unwrap();
        db.insert_project("gamma", "/tmp/gamma").unwrap();

        let projects = db.list_projects().unwrap();
        assert_eq!(projects.len(), 3);
        assert_eq!(projects[0].path, "/tmp/alpha");
        assert_eq!(projects[1].path, "/tmp/beta");
        assert_eq!(projects[2].path, "/tmp/gamma");
    }

    #[test]
    fn test_insert_project_assigns_next_sort_order() {
        let db = Db::open_in_memory().unwrap();
        db.insert_project("a", "/tmp/a").unwrap();
        db.insert_project("b", "/tmp/b").unwrap();
        db.insert_project("c", "/tmp/c").unwrap();

        let projects = db.list_projects().unwrap();
        assert_eq!(projects[0].sort_order, 1);
        assert_eq!(projects[1].sort_order, 2);
        assert_eq!(projects[2].sort_order, 3);
    }

    #[test]
    fn test_insert_duplicate_does_not_bump_sort_order() {
        let db = Db::open_in_memory().unwrap();
        db.insert_project("a", "/tmp/a").unwrap();
        db.insert_project("a", "/tmp/a").unwrap(); // duplicate
        db.insert_project("a", "/tmp/a").unwrap(); // duplicate

        let projects = db.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].sort_order, 1, "duplicate inserts must not advance sort_order");
    }

    #[test]
    fn test_reorder_project_move_up() {
        // [a=1, b=2, c=3] → move c to position 1 → [c=1, a=2, b=3]
        let db = Db::open_in_memory().unwrap();
        db.insert_project("a", "/tmp/a").unwrap();
        db.insert_project("b", "/tmp/b").unwrap();
        db.insert_project("c", "/tmp/c").unwrap();

        db.reorder_project("/tmp/c", 1).unwrap();

        let projects = db.list_projects().unwrap();
        assert_eq!(projects[0].path, "/tmp/c");
        assert_eq!(projects[0].sort_order, 1);
        assert_eq!(projects[1].path, "/tmp/a");
        assert_eq!(projects[1].sort_order, 2);
        assert_eq!(projects[2].path, "/tmp/b");
        assert_eq!(projects[2].sort_order, 3);
    }

    #[test]
    fn test_reorder_project_move_down() {
        // [a=1, b=2, c=3] → move a to position 3 → [b=1, c=2, a=3]
        let db = Db::open_in_memory().unwrap();
        db.insert_project("a", "/tmp/a").unwrap();
        db.insert_project("b", "/tmp/b").unwrap();
        db.insert_project("c", "/tmp/c").unwrap();

        db.reorder_project("/tmp/a", 3).unwrap();

        let projects = db.list_projects().unwrap();
        assert_eq!(projects[0].path, "/tmp/b");
        assert_eq!(projects[0].sort_order, 1);
        assert_eq!(projects[1].path, "/tmp/c");
        assert_eq!(projects[1].sort_order, 2);
        assert_eq!(projects[2].path, "/tmp/a");
        assert_eq!(projects[2].sort_order, 3);
    }

    #[test]
    fn test_reorder_project_clamps_above_count() {
        // [a=1, b=2] → move a to position 999 → [b=1, a=2]
        let db = Db::open_in_memory().unwrap();
        db.insert_project("a", "/tmp/a").unwrap();
        db.insert_project("b", "/tmp/b").unwrap();

        db.reorder_project("/tmp/a", 999).unwrap();

        let projects = db.list_projects().unwrap();
        assert_eq!(projects[0].path, "/tmp/b");
        assert_eq!(projects[1].path, "/tmp/a");
    }

    #[test]
    fn test_reorder_project_zero_errors() {
        let db = Db::open_in_memory().unwrap();
        db.insert_project("a", "/tmp/a").unwrap();
        let result = db.reorder_project("/tmp/a", 0);
        assert!(result.is_err(), "new_order < 1 must error");
    }

    #[test]
    fn test_reorder_project_same_position_noop() {
        let db = Db::open_in_memory().unwrap();
        db.insert_project("a", "/tmp/a").unwrap();
        db.insert_project("b", "/tmp/b").unwrap();

        db.reorder_project("/tmp/b", 2).unwrap(); // already at 2

        let projects = db.list_projects().unwrap();
        assert_eq!(projects[0].path, "/tmp/a");
        assert_eq!(projects[1].path, "/tmp/b");
    }

    #[test]
    fn test_reorder_project_unknown_path_errors() {
        // A typo or delete-during-drag race would call reorder_project with
        // a path that no longer exists. Verify it surfaces a clean Err
        // rather than silently no-op'ing or panicking.
        let db = Db::open_in_memory().unwrap();
        db.insert_project("a", "/tmp/a").unwrap();

        let result = db.reorder_project("/tmp/does-not-exist", 1);
        assert!(matches!(result, Err(rusqlite::Error::QueryReturnedNoRows)));

        // Existing project untouched.
        let projects = db.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].sort_order, 1);
    }

    #[test]
    fn test_migration_backfill_tiebreaker_uses_id() {
        // SQLite stores `created_at` with second precision via datetime('now').
        // Two projects inserted in the same second share a timestamp, so the
        // backfill UPDATE must fall back to `id` to break the tie. This test
        // forces the tie via raw UPDATE, resets sort_order to 0, re-runs the
        // backfill, and verifies the result is deterministic by id.
        let db = Db::open_in_memory().unwrap();
        db.insert_project("a", "/tmp/a").unwrap();
        db.insert_project("b", "/tmp/b").unwrap();
        db.insert_project("c", "/tmp/c").unwrap();

        // Force identical timestamps and clear sort_order so the backfill
        // is the only thing assigning order.
        {
            let conn = db.conn.lock();
            conn.execute(
                "UPDATE projects SET created_at = '2026-01-01 00:00:00', sort_order = 0",
                [],
            )
            .unwrap();
            conn.execute_batch(
                "UPDATE projects SET sort_order = (
                    SELECT COUNT(*) FROM projects p2
                    WHERE p2.created_at < projects.created_at
                       OR (p2.created_at = projects.created_at AND p2.id <= projects.id)
                );",
            )
            .unwrap();
        }

        // With all timestamps tied, ordering must be by id ASC.
        let projects = db.list_projects().unwrap();
        assert_eq!(projects.len(), 3);
        assert_eq!(projects[0].path, "/tmp/a");
        assert_eq!(projects[0].sort_order, 1);
        assert_eq!(projects[1].path, "/tmp/b");
        assert_eq!(projects[1].sort_order, 2);
        assert_eq!(projects[2].path, "/tmp/c");
        assert_eq!(projects[2].sort_order, 3);
    }

    #[test]
    fn test_session_crud() {
        let db = Db::open_in_memory().unwrap();
        let project_id = db.insert_project("myapp", "/tmp/myapp").unwrap();
        let session_id = db
            .insert_session(&InsertSession {
                project_id,
                pty_id: 1,
                agent_type: "claude",
                task_name: "Fix bug",
                working_dir: "/tmp/myapp",
                command: "claude",
                args: "[]",
                agent_session_id: None,
                worktree_path: None,
                base_commit: None,
                initial_prompt: None,
                task_id: None,
            })
            .unwrap();
        assert!(session_id > 0);

        let sessions = db.list_sessions_for_project(project_id).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].agent_type, "claude");
        assert_eq!(sessions[0].task_name, "Fix bug");
        assert_eq!(sessions[0].status, "running");

        db.update_session_status(1, "exited", Some(0)).unwrap();
        let sessions = db.list_sessions_for_project(project_id).unwrap();
        assert_eq!(sessions[0].status, "exited");
        assert_eq!(sessions[0].exit_code, Some(0));
        assert!(sessions[0].ended_at.is_some());
    }

    #[test]
    fn test_mark_all_running_as_exited() {
        let db = Db::open_in_memory().unwrap();
        let pid = db.insert_project("app", "/tmp/app").unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 1, agent_type: "claude", task_name: "t1",
            working_dir: "/tmp/app", command: "claude", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 2, agent_type: "codex", task_name: "t2",
            working_dir: "/tmp/app", command: "codex", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();

        let count = db.mark_all_running_as_exited().unwrap();
        assert_eq!(count, 2);

        let sessions = db.list_sessions_for_project(pid).unwrap();
        assert!(sessions.iter().all(|s| s.status == "exited"));
    }

    #[test]
    fn test_scrollback_save_load() {
        let db = Db::open_in_memory().unwrap();
        let pid = db.insert_project("app", "/tmp/app").unwrap();
        let _sid = db.insert_session(&InsertSession {
            project_id: pid, pty_id: 42, agent_type: "claude", task_name: "test",
            working_dir: "/tmp/app", command: "claude", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();

        // No scrollback initially
        assert_eq!(db.get_scrollback(42).unwrap(), None);

        // Save scrollback
        db.save_scrollback(42, "terminal state data here").unwrap();

        // Load it back
        let data = db.get_scrollback(42).unwrap();
        assert_eq!(data, Some("terminal state data here".to_string()));
    }

    #[test]
    fn test_delete_session_by_pty_id() {
        let db = Db::open_in_memory().unwrap();
        let pid = db.insert_project("app", "/tmp/app").unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 10, agent_type: "claude", task_name: "s1",
            working_dir: "/tmp/app", command: "claude", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 11, agent_type: "codex", task_name: "s2",
            working_dir: "/tmp/app", command: "codex", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();

        assert_eq!(db.list_sessions_for_project(pid).unwrap().len(), 2);

        db.delete_session_by_pty_id(10).unwrap();
        let sessions = db.list_sessions_for_project(pid).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].pty_id, Some(11));
    }

    #[test]
    fn test_max_pty_id() {
        let db = Db::open_in_memory().unwrap();

        // No sessions → max_pty_id returns 0
        assert_eq!(db.max_pty_id().unwrap(), 0);

        let pid = db.insert_project("app", "/tmp/app").unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 5, agent_type: "claude", task_name: "t",
            working_dir: "/tmp/app", command: "claude", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 42, agent_type: "codex", task_name: "t",
            working_dir: "/tmp/app", command: "codex", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();

        assert_eq!(db.max_pty_id().unwrap(), 42);
    }

    #[test]
    fn test_agent_session_id_stored() {
        let db = Db::open_in_memory().unwrap();
        let pid = db.insert_project("app", "/tmp/app").unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 1, agent_type: "claude", task_name: "t",
            working_dir: "/tmp/app", command: "claude", args: "[]",
            agent_session_id: Some("uuid-abc-123"),
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();

        let sessions = db.list_sessions_for_project(pid).unwrap();
        assert_eq!(sessions[0].agent_session_id, Some("uuid-abc-123".to_string()));
    }

    #[test]
    fn test_update_agent_session_id() {
        let db = Db::open_in_memory().unwrap();
        let pid = db.insert_project("app", "/tmp/app").unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 7, agent_type: "codex", task_name: "t",
            working_dir: "/tmp/app", command: "codex", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();

        // Initially no agent_session_id
        let sessions = db.list_sessions_for_project(pid).unwrap();
        assert_eq!(sessions[0].agent_session_id, None);

        // Update it
        db.update_agent_session_id(7, "thread-abc-123").unwrap();

        let sessions = db.list_sessions_for_project(pid).unwrap();
        assert_eq!(sessions[0].agent_session_id, Some("thread-abc-123".to_string()));
    }

    #[test]
    fn test_delete_project_cascades_sessions() {
        let db = Db::open_in_memory().unwrap();
        let pid = db.insert_project("app", "/tmp/app").unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 1, agent_type: "claude", task_name: "t",
            working_dir: "/tmp/app", command: "claude", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();

        db.delete_project("/tmp/app").unwrap();
        assert_eq!(db.list_projects().unwrap().len(), 0);
        assert_eq!(db.list_sessions_for_project(pid).unwrap().len(), 0);
    }

    #[test]
    fn test_sort_order_assigned_on_insert() {
        let db = Db::open_in_memory().unwrap();
        let pid = db.insert_project("app", "/tmp/sortapp").unwrap();

        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 10, agent_type: "shell",
            task_name: "s1", working_dir: "/tmp/sortapp",
            command: "bash", args: "[]", agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 11, agent_type: "claude",
            task_name: "s2", working_dir: "/tmp/sortapp",
            command: "claude", args: "[]", agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 12, agent_type: "shell",
            task_name: "s3", working_dir: "/tmp/sortapp",
            command: "bash", args: "[]", agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();

        let sessions = db.list_sessions_for_project(pid).unwrap();
        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].sort_order, 1);
        assert_eq!(sessions[1].sort_order, 2);
        assert_eq!(sessions[2].sort_order, 3);
    }

    #[test]
    fn test_list_sessions_ordered_by_sort_order() {
        let db = Db::open_in_memory().unwrap();
        let pid = db.insert_project("app", "/tmp/ordapp").unwrap();

        // Insert 3 sessions — they must come back in sort_order order
        for i in 1u32..=3 {
            db.insert_session(&InsertSession {
                project_id: pid, pty_id: i * 100, agent_type: "shell",
                task_name: &format!("session {i}"), working_dir: "/tmp/ordapp",
                command: "bash", args: "[]", agent_session_id: None,
                worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
            }).unwrap();
        }

        let sessions = db.list_sessions_for_project(pid).unwrap();
        assert_eq!(sessions[0].task_name, "session 1");
        assert_eq!(sessions[1].task_name, "session 2");
        assert_eq!(sessions[2].task_name, "session 3");
    }

    #[test]
    fn test_list_sessions_by_project_path() {
        let db = Db::open_in_memory().unwrap();
        let pid = db.insert_project("app", "/tmp/pathapp").unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 20, agent_type: "claude", task_name: "task1",
            working_dir: "/tmp/pathapp", command: "claude", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 21, agent_type: "codex", task_name: "task2",
            working_dir: "/tmp/pathapp", command: "codex", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();

        // Single JOIN query returns same results as two-query approach
        let sessions = db.list_sessions_by_project_path("/tmp/pathapp").unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].task_name, "task1");
        assert_eq!(sessions[1].task_name, "task2");

        // Unknown path returns empty vec (not an error)
        let empty = db.list_sessions_by_project_path("/tmp/noexist").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_settings_crud() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.get_setting("theme").unwrap(), None);
        db.set_setting("theme", "dark").unwrap();
        assert_eq!(db.get_setting("theme").unwrap(), Some("dark".to_string()));
        db.set_setting("theme", "light").unwrap();
        assert_eq!(db.get_setting("theme").unwrap(), Some("light".to_string()));
        db.set_setting("font_size", "14").unwrap();
        let all = db.get_all_settings().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].0, "font_size");
        assert_eq!(all[1].0, "theme");
    }

    #[test]
    fn test_reorder_session() {
        let db = Db::open_in_memory().unwrap();
        let pid = db.insert_project("app", "/tmp/reordapp").unwrap();

        // Insert 3 sessions: sort_order 1, 2, 3
        for i in 1u32..=3 {
            db.insert_session(&InsertSession {
                project_id: pid, pty_id: i * 10, agent_type: "shell",
                task_name: &format!("s{i}"), working_dir: "/tmp/reordapp",
                command: "bash", args: "[]", agent_session_id: None,
                worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
            }).unwrap();
        }

        // Move pty_id=30 (sort_order=3) to position 1
        db.reorder_session(30, 1).unwrap();

        let sessions = db.list_sessions_for_project(pid).unwrap();
        // s3 should now be first
        assert_eq!(sessions[0].task_name, "s3");
        assert_eq!(sessions[1].task_name, "s1");
        assert_eq!(sessions[2].task_name, "s2");
    }

    #[test]
    fn test_task_crud_and_cascade() {
        let db = Db::open_in_memory().unwrap();
        let pid = db.insert_project("app", "/tmp/taskapp").unwrap();

        // Insert two tasks in the same project
        let tid1 = db
            .insert_task(
                pid,
                "feature A",
                Some("feat/a"),
                Some("/tmp/taskapp/.sessonix-worktrees/feat-a"),
                Some("abc123"),
            )
            .unwrap();
        let tid2 = db
            .insert_task(pid, "feature B", Some("feat/b"), None, None)
            .unwrap();
        assert!(tid1 > 0 && tid2 > tid1);

        // list_tasks_by_project_path returns both, ordered by created_at ASC
        let tasks = db.list_tasks_by_project_path("/tmp/taskapp").unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name, "feature A");
        assert_eq!(tasks[0].branch.as_deref(), Some("feat/a"));
        assert_eq!(tasks[0].base_commit.as_deref(), Some("abc123"));
        assert_eq!(tasks[1].name, "feature B");
        assert!(tasks[1].worktree_path.is_none());

        // Unknown project returns empty
        assert!(db.list_tasks_by_project_path("/tmp/nope").unwrap().is_empty());

        // get_task_by_id
        let fetched = db.get_task_by_id(tid1).unwrap().unwrap();
        assert_eq!(fetched.id, tid1);
        assert_eq!(fetched.name, "feature A");
        assert!(db.get_task_by_id(99_999).unwrap().is_none());

        // Insert sessions, one bound to tid1, one free-floating
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 100, agent_type: "claude", task_name: "inside task",
            working_dir: "/tmp/taskapp", command: "claude", args: "[]",
            agent_session_id: None,
            worktree_path: Some("/tmp/taskapp/.sessonix-worktrees/feat-a"),
            base_commit: Some("abc123"),
            initial_prompt: None,
            task_id: Some(tid1),
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 101, agent_type: "shell", task_name: "ungrouped",
            working_dir: "/tmp/taskapp", command: "zsh", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None,
            task_id: None,
        }).unwrap();

        // list_sessions_by_task_id returns only the task-bound session
        let task_sessions = db.list_sessions_by_task_id(tid1).unwrap();
        assert_eq!(task_sessions.len(), 1);
        assert_eq!(task_sessions[0].pty_id, Some(100));
        assert_eq!(task_sessions[0].task_id, Some(tid1));

        // delete_task removes task + its sessions; ungrouped session survives
        db.delete_task(tid1).unwrap();
        assert!(db.get_task_by_id(tid1).unwrap().is_none());
        assert!(db.list_sessions_by_task_id(tid1).unwrap().is_empty());
        let remaining = db.list_sessions_for_project(pid).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].pty_id, Some(101));

        // Remaining task untouched
        let tasks_after = db.list_tasks_by_project_path("/tmp/taskapp").unwrap();
        assert_eq!(tasks_after.len(), 1);
        assert_eq!(tasks_after[0].id, tid2);
    }

    #[test]
    fn test_delete_project_cascades_tasks() {
        let db = Db::open_in_memory().unwrap();
        let pid = db.insert_project("app", "/tmp/cascadeapp").unwrap();
        let tid = db
            .insert_task(pid, "t", Some("b"), Some("/tmp/cascadeapp/.sessonix-worktrees/b"), None)
            .unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 50, agent_type: "claude", task_name: "t",
            working_dir: "/tmp/cascadeapp", command: "claude", args: "[]",
            agent_session_id: None,
            worktree_path: Some("/tmp/cascadeapp/.sessonix-worktrees/b"),
            base_commit: None, initial_prompt: None, task_id: Some(tid),
        }).unwrap();

        db.delete_project("/tmp/cascadeapp").unwrap();
        assert_eq!(db.list_projects().unwrap().len(), 0);
        assert!(db.list_tasks_by_project_path("/tmp/cascadeapp").unwrap().is_empty());
        assert!(db.list_sessions_by_task_id(tid).unwrap().is_empty());
    }

    #[test]
    fn test_migration_idempotent_task_id_column() {
        // Reopening / running migrate twice should not fail or duplicate the task_id column.
        let db = Db::open_in_memory().unwrap();
        db.migrate().unwrap();
        // Also after inserting rows
        let pid = db.insert_project("x", "/tmp/xx").unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 1, agent_type: "shell", task_name: "t",
            working_dir: "/tmp/xx", command: "zsh", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None, task_id: None,
        }).unwrap();
        db.migrate().unwrap();
        let sessions = db.list_sessions_for_project(pid).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].task_id, None);
    }

    #[test]
    fn test_quick_prompt_crud() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .insert_quick_prompt("Greet", "/tmp/p", "claude", Some("say hi"), true)
            .unwrap();
        assert!(id > 0);

        let rows = db.list_quick_prompts("/tmp/p").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Greet");
        assert_eq!(rows[0].agent, "claude");
        assert_eq!(rows[0].initial_prompt.as_deref(), Some("say hi"));
        assert!(rows[0].skip_permissions);

        // Filter by project_path
        db.insert_quick_prompt("Other", "/tmp/other", "", None, false).unwrap();
        assert_eq!(db.list_quick_prompts("/tmp/p").unwrap().len(), 1);

        db.delete_quick_prompt(id).unwrap();
        assert!(db.list_quick_prompts("/tmp/p").unwrap().is_empty());
    }

    #[test]
    fn test_update_quick_prompt_preserves_agent_and_skip_permissions() {
        // Regression guard: update_quick_prompt must not clobber `agent`
        // or `skip_permissions` (they are not editable from the UI).
        let db = Db::open_in_memory().unwrap();
        let id = db
            .insert_quick_prompt("orig", "/tmp/p", "codex", Some("body"), true)
            .unwrap();

        db.update_quick_prompt(id, "renamed", Some("new body")).unwrap();

        let rows = db.list_quick_prompts("/tmp/p").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "renamed");
        assert_eq!(rows[0].initial_prompt.as_deref(), Some("new body"));
        assert_eq!(rows[0].agent, "codex", "agent must be preserved on update");
        assert!(rows[0].skip_permissions, "skip_permissions must be preserved on update");
    }

    #[test]
    fn test_migration_renames_legacy_templates_table() {
        // Simulate a DB that only has the old `templates` table with data,
        // then run migrate() and verify the rows land in `quick_prompts`.
        let db = Db::open_in_memory().unwrap();
        {
            let conn = db.conn.lock();
            conn.execute_batch(
                "DROP TABLE quick_prompts;
                 CREATE TABLE templates (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL,
                    project_path TEXT NOT NULL,
                    agent TEXT NOT NULL DEFAULT '',
                    initial_prompt TEXT,
                    skip_permissions INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                 );
                 INSERT INTO templates (name, project_path, agent, initial_prompt, skip_permissions)
                    VALUES ('legacy', '/tmp/p', 'claude', 'hi', 1);",
            ).unwrap();
        }

        db.migrate().unwrap();

        // templates gone, quick_prompts carries the row.
        let rows = db.list_quick_prompts("/tmp/p").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "legacy");
        assert_eq!(rows[0].agent, "claude");
        assert_eq!(rows[0].initial_prompt.as_deref(), Some("hi"));
        assert!(rows[0].skip_permissions);

        let conn = db.conn.lock();
        let legacy_exists = conn.prepare("SELECT 1 FROM templates LIMIT 0").is_ok();
        assert!(!legacy_exists, "legacy `templates` table should be removed");
    }

    #[test]
    fn test_migration_split_brain_merges_and_drops_legacy() {
        // Simulate the dogfooding worktree case: both tables exist
        // (old branch created `templates` with data; new branch previously
        // created an empty `quick_prompts`). Migration must merge the
        // orphaned rows and drop the legacy table without data loss.
        let db = Db::open_in_memory().unwrap();
        {
            let conn = db.conn.lock();
            conn.execute_batch(
                "CREATE TABLE templates (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL,
                    project_path TEXT NOT NULL,
                    agent TEXT NOT NULL DEFAULT '',
                    initial_prompt TEXT,
                    skip_permissions INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                 );
                 -- Legacy row, id=10
                 INSERT INTO templates (id, name, project_path, agent, initial_prompt, skip_permissions)
                    VALUES (10, 'from-legacy', '/tmp/p', 'codex', 'old', 0);",
            ).unwrap();
        }
        // Insert a row into the current `quick_prompts` to simulate the new branch
        // having already created data; the legacy row must merge alongside.
        db.insert_quick_prompt("from-new", "/tmp/p", "claude", Some("fresh"), true).unwrap();

        db.migrate().unwrap();

        let mut rows = db.list_quick_prompts("/tmp/p").unwrap();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(rows.len(), 2, "legacy and new rows must coexist after merge");
        assert_eq!(rows[0].name, "from-legacy");
        assert_eq!(rows[0].agent, "codex");
        assert_eq!(rows[1].name, "from-new");
        assert_eq!(rows[1].agent, "claude");

        let conn = db.conn.lock();
        let legacy_exists = conn.prepare("SELECT 1 FROM templates LIMIT 0").is_ok();
        assert!(!legacy_exists, "legacy `templates` table should be dropped after merge");
    }
}
