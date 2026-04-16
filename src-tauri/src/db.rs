use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct ProjectRow {
    pub id: i64,
    pub name: String,
    pub path: String,
}

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
        let conn = self.conn.lock().unwrap();
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

        Ok(())
    }

    // --- Projects ---

    pub fn insert_project(&self, name: &str, path: &str) -> Result<i64, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO projects (name, path) VALUES (?1, ?2)",
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
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path FROM projects ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ProjectRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    #[allow(dead_code)]
    pub fn find_project_id_by_path(&self, path: &str) -> Result<Option<i64>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM projects WHERE path = ?1")?;
        let mut rows = stmt.query(params![path])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn delete_project(&self, path: &str) -> Result<(), rusqlite::Error> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        // Delete sessions by project_id (not working_dir) to avoid cross-project deletion
        tx.execute(
            "DELETE FROM sessions WHERE project_id = (SELECT id FROM projects WHERE path = ?1)",
            params![path],
        )?;
        tx.execute("DELETE FROM projects WHERE path = ?1", params![path])?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_session_by_pty_id(&self, pty_id: u32) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM sessions WHERE pty_id = ?1", params![pty_id])?;
        Ok(())
    }

    // --- Sessions ---

    pub fn insert_session(&self, s: &InsertSession<'_>) -> Result<i64, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let next_order: u32 = conn.query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM sessions WHERE project_id = ?1",
            params![s.project_id],
            |row| row.get::<_, u32>(0),
        )?;
        conn.execute(
            "INSERT INTO sessions (project_id, pty_id, agent_type, task_name, working_dir, launch_command, launch_args, agent_session_id, sort_order, worktree_path, base_commit, initial_prompt)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![s.project_id, s.pty_id, s.agent_type, s.task_name, s.working_dir, s.command, s.args, s.agent_session_id, next_order, s.worktree_path, s.base_commit, s.initial_prompt],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn reorder_session(&self, pty_id: u32, new_order: u32) -> Result<(), rusqlite::Error> {
        if new_order < 1 {
            return Err(rusqlite::Error::InvalidParameterName(
                "new_order must be >= 1".to_string(),
            ));
        }
        let mut conn = self.conn.lock().unwrap();
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

    #[allow(dead_code)]
    pub fn update_session_status(
        &self,
        pty_id: u32,
        status: &str,
        exit_code: Option<i32>,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
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

    #[allow(dead_code)]
    pub fn list_sessions_for_project(
        &self,
        project_id: i64,
    ) -> Result<Vec<SessionRow>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, pty_id, agent_type, task_name, working_dir,
                    status, status_line, exit_code, launch_command, launch_args,
                    started_at, ended_at, scrollback, agent_session_id, sort_order,
                    worktree_path, base_commit, initial_prompt
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Set sort_order directly without shifting other sessions.
    /// Used when replacing a session (relaunch) to inherit the old position.
    pub fn set_sort_order(&self, pty_id: u32, sort_order: u32) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET worktree_path = NULL, base_commit = NULL WHERE pty_id = ?1",
            params![pty_id],
        )?;
        Ok(())
    }

    pub fn save_scrollback(&self, pty_id: u32, data: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET scrollback = ?1 WHERE pty_id = ?2",
            params![data, pty_id],
        )?;
        Ok(())
    }

    pub fn get_scrollback(&self, pty_id: u32) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT scrollback FROM sessions WHERE pty_id = ?1",
            params![pty_id],
            |row| row.get(0),
        )
    }

    pub fn max_pty_id(&self) -> Result<u32, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.project_id, s.pty_id, s.agent_type, s.task_name, s.working_dir,
                    s.status, s.status_line, s.exit_code, s.launch_command, s.launch_args,
                    s.started_at, s.ended_at, s.scrollback, s.agent_session_id, s.sort_order,
                    s.worktree_path, s.base_commit, s.initial_prompt
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // --- Settings ---

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_all_settings(&self) -> Result<Vec<(String, String)>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key ASC")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn mark_all_running_as_exited(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
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
            worktree_path: None, base_commit: None, initial_prompt: None,
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 2, agent_type: "codex", task_name: "t2",
            working_dir: "/tmp/app", command: "codex", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None,
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
            worktree_path: None, base_commit: None, initial_prompt: None,
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
            worktree_path: None, base_commit: None, initial_prompt: None,
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 11, agent_type: "codex", task_name: "s2",
            working_dir: "/tmp/app", command: "codex", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None,
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
            worktree_path: None, base_commit: None, initial_prompt: None,
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 42, agent_type: "codex", task_name: "t",
            working_dir: "/tmp/app", command: "codex", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None,
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
            worktree_path: None, base_commit: None, initial_prompt: None,
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
            worktree_path: None, base_commit: None, initial_prompt: None,
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
            worktree_path: None, base_commit: None, initial_prompt: None,
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
            worktree_path: None, base_commit: None, initial_prompt: None,
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 11, agent_type: "claude",
            task_name: "s2", working_dir: "/tmp/sortapp",
            command: "claude", args: "[]", agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None,
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 12, agent_type: "shell",
            task_name: "s3", working_dir: "/tmp/sortapp",
            command: "bash", args: "[]", agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None,
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
                worktree_path: None, base_commit: None, initial_prompt: None,
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
            worktree_path: None, base_commit: None, initial_prompt: None,
        }).unwrap();
        db.insert_session(&InsertSession {
            project_id: pid, pty_id: 21, agent_type: "codex", task_name: "task2",
            working_dir: "/tmp/pathapp", command: "codex", args: "[]",
            agent_session_id: None,
            worktree_path: None, base_commit: None, initial_prompt: None,
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
                worktree_path: None, base_commit: None, initial_prompt: None,
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
}
