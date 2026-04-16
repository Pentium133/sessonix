mod adapters;
mod db;
mod error;
mod git_manager;
mod hooks;
mod jsonl;
mod pty_manager;
mod ring_buffer;
mod session_manager;
mod types;

use adapters::AdapterRegistry;
use session_manager::{CreateSessionParams, SessionManager};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::{Emitter, Manager};

static FORCE_EXIT: AtomicBool = AtomicBool::new(false);
static LAST_UPDATE_CHECK: Mutex<Option<Instant>> = Mutex::new(None);

use types::CreateSessionRequest;

#[tauri::command]
fn create_session(
    state: tauri::State<'_, SessionManager>,
    app: tauri::AppHandle,
    request: CreateSessionRequest,
) -> Result<u32, String> {
    state
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
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn write_to_session(state: tauri::State<'_, SessionManager>, id: u32, data: Vec<u8>) -> Result<(), String> {
    let session = state.pty.get_session(id).map_err(|e| e.to_string())?;
    session.write_input(&data).map_err(|e| e.to_string())
}

#[tauri::command]
fn resize_session(
    state: tauri::State<'_, SessionManager>,
    id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let session = state.pty.get_session(id).map_err(|e| e.to_string())?;
    session.resize(cols, rows).map_err(|e| e.to_string())
}

#[tauri::command]
fn kill_session(state: tauri::State<'_, SessionManager>, id: u32) -> Result<(), String> {
    let session = state.pty.get_session(id).map_err(|e| e.to_string())?;
    session.kill().map_err(|e| e.to_string())
}

#[tauri::command]
fn detach_session(state: tauri::State<'_, SessionManager>, id: u32) -> Result<(), String> {
    let session = state.pty.get_session(id).map_err(|e| e.to_string())?;
    session.detach();
    Ok(())
}

#[tauri::command]
fn attach_session(state: tauri::State<'_, SessionManager>, id: u32) -> Result<Vec<u8>, String> {
    let session = state.pty.get_session(id).map_err(|e| e.to_string())?;
    Ok(session.attach_and_drain())
}

#[tauri::command]
fn get_session_count(state: tauri::State<'_, SessionManager>) -> Result<usize, String> {
    Ok(state.pty.session_count())
}

#[tauri::command]
fn get_session_status(
    state: tauri::State<'_, SessionManager>,
    adapters: tauri::State<'_, AdapterRegistry>,
    id: u32,
    agent_type: Option<String>,
    working_dir: Option<String>,
    agent_session_id: Option<String>,
) -> Result<types::AgentStatus, String> {
    let session = state.pty.get_session(id).map_err(|e| e.to_string())?;

    // For Claude: try hook-based status first (real-time), then JSONL, then terminal
    if agent_type.as_deref() == Some("claude") {
        // 1. Check hook status file (fastest, real-time from Claude events)
        if let Some(hook) = hooks::read_hook_status(id) {
            let (state_val, status_line) = match hook.status.as_str() {
                "running" => (types::SessionStatus::Running, "Working...".to_string()),
                "idle" => (types::SessionStatus::Idle, "Waiting for input".to_string()),
                "waiting_permission" => (types::SessionStatus::Idle, "Waiting for permission".to_string()),
                "exited" => (types::SessionStatus::Running, "Session ending...".to_string()),
                _ => (types::SessionStatus::Running, String::new()),
            };
            return Ok(types::AgentStatus { state: state_val, status_line });
        }

        // 2. Check JSONL tail — only when we have a session UUID.
        // Without a UUID (e.g. --continue sessions), find_session_file returns the most
        // recently modified file in the project dir, which may belong to a different session
        // that is actively running. Skip JSONL lookup in that case; fall to terminal detection.
        if let (Some(wd), Some(sid)) = (working_dir.as_ref(), agent_session_id.as_ref()) {
            let jsonl_path = jsonl::find_session_file_by_id(wd, sid);

            if let Some(ref path) = jsonl_path {
                let status = jsonl::detect_status(path);
                let (state_val, status_line) = match status {
                    jsonl::ClaudeStatus::Active => (types::SessionStatus::Running, "Working...".to_string()),
                    jsonl::ClaudeStatus::Idle => (types::SessionStatus::Idle, "Waiting for input".to_string()),
                    jsonl::ClaudeStatus::WaitingPermission => (types::SessionStatus::Idle, "Waiting for permission".to_string()),
                    jsonl::ClaudeStatus::Error(msg) => (types::SessionStatus::Error, msg),
                    jsonl::ClaudeStatus::Unknown => {
                        // Fall back to terminal-based detection
                        let lines: Vec<String> = session.snapshot_last_lines();
                        if let Some(adapter) = adapters.get("claude") {
                            let s = adapter.extract_status(&lines);
                            (s.state, s.status_line)
                        } else {
                            (types::SessionStatus::Running, String::new())
                        }
                    }
                };
                return Ok(types::AgentStatus { state: state_val, status_line });
            }
        }
    }

    // For shell/custom sessions: use kernel-level foreground process detection (tcgetpgrp).
    // `is_foreground_idle()` returns None only when shell_pid or process_group_leader() is
    // unavailable — theoretically unreachable on Unix (process_id always returns Some).
    let is_shell = matches!(agent_type.as_deref(), Some("shell" | "custom"));
    if is_shell {
        let lines: Vec<String> = session.snapshot_last_lines();

        return match session.is_foreground_idle() {
            Some(true) => Ok(types::AgentStatus {
                state: types::SessionStatus::Idle,
                status_line: String::new(),
            }),
            Some(false) => {
                // Compute status_line only when needed (Running path)
                let last_line = lines.last().map(|s| adapters::strip_ansi(s)).unwrap_or_default();
                Ok(types::AgentStatus {
                    state: types::SessionStatus::Running,
                    status_line: adapters::truncate(last_line.trim(), 80),
                })
            }
            None => {
                // Fallback to terminal heuristic if tcgetpgrp unavailable (Windows / edge cases)
                let at = agent_type.as_deref().unwrap_or("custom");
                if let Some(adapter) = adapters.get(at) {
                    Ok(adapter.extract_status(&lines))
                } else {
                    Ok(types::AgentStatus {
                        state: types::SessionStatus::Running,
                        status_line: String::new(),
                    })
                }
            }
        };
    }

    // Fallback: terminal-based status extraction for other agent types
    let lines: Vec<String> = session.snapshot_last_lines();
    if let Some(ref at) = agent_type {
        if let Some(adapter) = adapters.get(at) {
            return Ok(adapter.extract_status(&lines));
        }
    }

    Ok(types::AgentStatus {
        state: types::SessionStatus::Running,
        status_line: adapters::strip_ansi(lines.last().map(|s| s.as_str()).unwrap_or("")),
    })
}

#[derive(serde::Serialize)]
struct SessionCostInfo {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    model: String,
    cost_usd: f64,
    turns: u32,
}

#[tauri::command]
fn get_session_cost(
    working_dir: String,
    agent_session_id: Option<String>,
) -> Result<SessionCostInfo, String> {
    let jsonl_path = if let Some(ref sid) = agent_session_id {
        jsonl::find_session_file_by_id(&working_dir, sid)
    } else {
        jsonl::find_session_file(&working_dir)
    };

    let path = jsonl_path.ok_or_else(|| "No JSONL session file found".to_string())?;
    let cost = jsonl::compute_cost(&path);

    Ok(SessionCostInfo {
        input_tokens: cost.input_tokens,
        output_tokens: cost.output_tokens,
        cache_read_tokens: cost.cache_read_tokens,
        cache_write_tokens: cost.cache_write_tokens,
        model: cost.model,
        cost_usd: cost.cost_usd,
        turns: cost.turns,
    })
}

#[tauri::command]
fn get_available_agents(adapters: tauri::State<'_, AdapterRegistry>) -> Result<Vec<String>, String> {
    Ok(adapters.available_types().into_iter().map(String::from).collect())
}

// --- Project/Session persistence commands ---

#[tauri::command]
fn add_project(state: tauri::State<'_, SessionManager>, name: String, path: String) -> Result<i64, String> {
    state.add_project(&name, &path).map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_project(state: tauri::State<'_, SessionManager>, path: String) -> Result<(), String> {
    state.remove_project(&path).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct ProjectInfo {
    id: i64,
    name: String,
    path: String,
}

#[derive(serde::Serialize)]
struct SessionInfo {
    id: i64,
    pty_id: Option<u32>,
    agent_type: String,
    task_name: String,
    working_dir: String,
    status: String,
    status_line: String,
    exit_code: Option<i32>,
    launch_command: String,
    launch_args: String,
    started_at: String,
    agent_session_id: Option<String>,
    sort_order: u32,
    worktree_path: Option<String>,
    base_commit: Option<String>,
    initial_prompt: Option<String>,
    task_id: Option<i64>,
}

#[tauri::command]
fn list_projects(state: tauri::State<'_, SessionManager>) -> Result<Vec<ProjectInfo>, String> {
    let projects = state.list_projects().map_err(|e| e.to_string())?;
    Ok(projects
        .into_iter()
        .map(|p| ProjectInfo {
            id: p.id,
            name: p.name,
            path: p.path,
        })
        .collect())
}

#[tauri::command]
fn list_sessions(
    state: tauri::State<'_, SessionManager>,
    project_path: String,
) -> Result<Vec<SessionInfo>, String> {
    let sessions = state
        .db
        .list_sessions_by_project_path(&project_path)
        .map_err(|e| e.to_string())?;
    Ok(sessions
        .into_iter()
        .map(|s| SessionInfo {
            id: s.id,
            pty_id: s.pty_id,
            agent_type: s.agent_type,
            task_name: s.task_name,
            working_dir: s.working_dir,
            status: s.status,
            status_line: s.status_line,
            exit_code: s.exit_code,
            launch_command: s.launch_command,
            launch_args: s.launch_args,
            started_at: s.started_at,
            agent_session_id: s.agent_session_id,
            sort_order: s.sort_order,
            worktree_path: s.worktree_path,
            base_commit: s.base_commit,
            initial_prompt: s.initial_prompt,
            task_id: s.task_id,
        })
        .collect())
}

#[tauri::command]
fn save_scrollback(
    state: tauri::State<'_, SessionManager>,
    pty_id: u32,
    data: String,
) -> Result<(), String> {
    state
        .db
        .save_scrollback(pty_id, &data)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_session(state: tauri::State<'_, SessionManager>, pty_id: u32) -> Result<(), String> {
    state
        .db
        .delete_session_by_pty_id(pty_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reorder_session(
    state: tauri::State<'_, SessionManager>,
    pty_id: u32,
    new_sort_order: u32,
) -> Result<(), String> {
    state
        .db
        .reorder_session(pty_id, new_sort_order)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_sort_order(
    state: tauri::State<'_, SessionManager>,
    pty_id: u32,
    sort_order: u32,
) -> Result<(), String> {
    state.db.set_sort_order(pty_id, sort_order).map_err(|e| e.to_string())
}

#[tauri::command]
fn notify_session_exit(state: tauri::State<'_, SessionManager>, pty_id: u32) -> Result<(), String> {
    state.on_session_exit(pty_id);
    Ok(())
}

#[tauri::command]
fn get_scrollback(
    state: tauri::State<'_, SessionManager>,
    pty_id: u32,
) -> Result<Option<String>, String> {
    state
        .db
        .get_scrollback(pty_id)
        .map_err(|e| e.to_string())
}

// --- Templates ---

#[derive(serde::Serialize)]
struct TemplateInfo {
    id: i64,
    name: String,
    project_path: String,
    agent: String,
    initial_prompt: Option<String>,
    skip_permissions: bool,
}

#[derive(serde::Deserialize)]
struct CreateTemplateRequest {
    name: String,
    project_path: String,
    agent: String,
    initial_prompt: Option<String>,
    skip_permissions: bool,
}

#[tauri::command]
fn create_template(state: tauri::State<'_, SessionManager>, request: CreateTemplateRequest) -> Result<i64, String> {
    state.db.insert_template(
        &request.name,
        &request.project_path,
        &request.agent,
        request.initial_prompt.as_deref(),
        request.skip_permissions,
    ).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_templates(state: tauri::State<'_, SessionManager>, project_path: String) -> Result<Vec<TemplateInfo>, String> {
    let rows = state.db.list_templates(&project_path).map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|t| TemplateInfo {
        id: t.id,
        name: t.name,
        project_path: t.project_path,
        agent: t.agent,
        initial_prompt: t.initial_prompt,
        skip_permissions: t.skip_permissions,
    }).collect())
}

#[tauri::command]
fn delete_template(state: tauri::State<'_, SessionManager>, id: i64) -> Result<(), String> {
    state.db.delete_template(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn update_template(state: tauri::State<'_, SessionManager>, id: i64, name: String, initial_prompt: Option<String>) -> Result<(), String> {
    state.db.update_template(id, &name, "", initial_prompt.as_deref(), false).map_err(|e| e.to_string())
}

// --- Tasks (worktree-scoped session groups) ---

#[derive(serde::Deserialize)]
struct CreateTaskRequest {
    project_path: String,
    name: String,
    branch_name: String,
}

#[derive(serde::Serialize)]
struct TaskInfo {
    id: i64,
    project_id: i64,
    name: String,
    branch: Option<String>,
    worktree_path: Option<String>,
    base_commit: Option<String>,
    created_at: i64,
}

/// Parse SQLite `datetime('now')` output (`YYYY-MM-DD HH:MM:SS` in UTC) to Unix ms.
/// Returns 0 on parse failure or on fields outside valid calendar ranges.
/// Implemented inline to avoid pulling in chrono.
fn parse_iso_to_unix_ms(s: &str) -> i64 {
    if s.len() < 19 {
        return 0;
    }
    let parse = |start: usize, end: usize| -> Option<i64> {
        s.get(start..end).and_then(|v| v.parse::<i64>().ok())
    };
    let (year, month, day, hour, minute, second) = match (
        parse(0, 4),
        parse(5, 7),
        parse(8, 10),
        parse(11, 13),
        parse(14, 16),
        parse(17, 19),
    ) {
        (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(s)) => (y, mo, d, h, mi, s),
        _ => return 0,
    };

    // Calendar bounds. Hinnant's formula assumes valid inputs; out-of-range
    // values silently produce garbage without these checks.
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return 0;
    }

    // Howard Hinnant's days_from_civil: days since 1970-01-01 (UTC).
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    (days * 86400 + hour * 3600 + minute * 60 + second) * 1000
}

fn task_row_to_info(row: db::TaskRow) -> TaskInfo {
    TaskInfo {
        id: row.id,
        project_id: row.project_id,
        name: row.name,
        branch: row.branch,
        worktree_path: row.worktree_path,
        base_commit: row.base_commit,
        created_at: parse_iso_to_unix_ms(&row.created_at),
    }
}

/// Upper bound to prevent pathological inputs from allocating large buffers
/// downstream (sanitize_branch is char-by-char, git2 will reject at ~255, etc).
const TASK_FIELD_MAX_LEN: usize = 200;

/// Trim + length/empty validation for a task creation request.
/// Pure, side-effect free — extracted so it can be exercised in unit tests
/// without the full Tauri State + DB setup.
fn validate_task_fields(name: &str, branch_name: &str) -> Result<(String, String), String> {
    let name = name.trim().to_string();
    let branch_name = branch_name.trim().to_string();
    if name.is_empty() {
        return Err("Task name is required".into());
    }
    if branch_name.is_empty() {
        return Err("Branch name is required".into());
    }
    if name.len() > TASK_FIELD_MAX_LEN {
        return Err(format!("Task name must be ≤{} characters", TASK_FIELD_MAX_LEN));
    }
    if branch_name.len() > TASK_FIELD_MAX_LEN {
        return Err(format!("Branch name must be ≤{} characters", TASK_FIELD_MAX_LEN));
    }
    Ok((name, branch_name))
}

#[tauri::command]
async fn create_task(
    state: tauri::State<'_, SessionManager>,
    request: CreateTaskRequest,
) -> Result<TaskInfo, String> {
    // Early input validation — reject empty or absurdly long fields before
    // any filesystem or DB work.
    let (name, branch_name) = validate_task_fields(&request.name, &request.branch_name)?;

    // Project must already exist (added via add_project). Lookup id first.
    let project_id = state
        .db
        .find_project_id_by_path(&request.project_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Unknown project path: {}", request.project_path))?;

    // Offload blocking git2 + DB work so the Tauri dispatcher thread stays free.
    let db = state.db.clone();
    let project_path = request.project_path;

    tauri::async_runtime::spawn_blocking(move || -> Result<TaskInfo, String> {
        let wt = git_manager::create_worktree(&project_path, &branch_name)?;

        // On DB failure, compensate by removing the worktree we just created.
        let task_id = match db.insert_task(
            project_id,
            &name,
            Some(&wt.branch),
            Some(&wt.path),
            Some(&wt.base_commit),
        ) {
            Ok(id) => id,
            Err(e) => {
                let _ = git_manager::remove_worktree(&wt.path);
                return Err(format!("Failed to persist task: {}", e));
            }
        };

        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Ok(TaskInfo {
            id: task_id,
            project_id,
            name,
            branch: Some(wt.branch),
            worktree_path: Some(wt.path),
            base_commit: Some(wt.base_commit),
            created_at,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn list_tasks(
    state: tauri::State<'_, SessionManager>,
    project_path: String,
) -> Result<Vec<TaskInfo>, String> {
    let rows = state
        .db
        .list_tasks_by_project_path(&project_path)
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(task_row_to_info).collect())
}

#[derive(serde::Serialize)]
struct DeleteTaskResult {
    /// Populated when DB rows were deleted but the worktree on disk wasn't
    /// fully removed (e.g. dirty index, filesystem permission). The caller
    /// should surface this as a non-fatal warning — DB state is consistent.
    worktree_warning: Option<String>,
}

#[tauri::command]
async fn delete_task(
    state: tauri::State<'_, SessionManager>,
    task_id: i64,
) -> Result<DeleteTaskResult, String> {
    // 1. Look up task to learn its worktree_path.
    let task = state
        .db
        .get_task_by_id(task_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task {} not found", task_id))?;

    // 2. Kill all PTY sessions belonging to this task synchronously —
    //    these are cheap signal sends, not blocking IO. Must happen with
    //    access to `state.pty`, before we offload the rest.
    let sessions = state
        .db
        .list_sessions_by_task_id(task_id)
        .map_err(|e| e.to_string())?;
    for s in &sessions {
        if let Some(pid) = s.pty_id {
            if let Ok(session) = state.pty.get_session(pid) {
                let _ = session.kill();
            }
        }
    }

    // 3. Offload worktree removal + DB delete to the blocking pool.
    let db = state.db.clone();
    let worktree_path = task.worktree_path.clone();

    tauri::async_runtime::spawn_blocking(move || -> Result<DeleteTaskResult, String> {
        // Remove the worktree (if tracked). Best-effort for missing dirs, but
        // surface genuine failures so an orphaned worktree on disk is visible
        // to the user (otherwise silent disk leak).
        let mut worktree_warning: Option<String> = None;
        if let Some(ref path) = worktree_path {
            if let Err(e) = git_manager::remove_worktree(path) {
                log::warn!("delete_task {}: worktree cleanup failed at {}: {}", task_id, path, e);
                worktree_warning = Some(format!("Worktree at {} was not fully removed: {}", path, e));
            }
        }

        // Delete DB rows (sessions first, then task — inside a transaction).
        // Done even if worktree cleanup failed: otherwise the task row points
        // at a potentially-half-deleted worktree with no way to retry via UI.
        db.delete_task(task_id).map_err(|e| e.to_string())?;

        Ok(DeleteTaskResult { worktree_warning })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_setting(state: tauri::State<'_, SessionManager>, key: String) -> Result<Option<String>, String> {
    state.db.get_setting(&key).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_setting(state: tauri::State<'_, SessionManager>, key: String, value: String) -> Result<(), String> {
    state.db.set_setting(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_all_settings(state: tauri::State<'_, SessionManager>) -> Result<Vec<(String, String)>, String> {
    state.db.get_all_settings().map_err(|e| e.to_string())
}

#[tauri::command]
fn install_claude_hooks() -> Result<bool, String> {
    hooks::install_hooks()
}

#[tauri::command]
fn check_claude_hooks() -> Result<bool, String> {
    Ok(hooks::check_installed())
}

#[tauri::command]
fn get_running_session_count(sm: tauri::State<'_, SessionManager>) -> Result<u32, String> {
    Ok(sm.pty.running_count())
}

/// Check whether a given CLI binary is available in PATH.
fn is_in_path(name: &str) -> bool {
    let cmd = if cfg!(target_os = "windows") { "where" } else { "which" };
    std::process::Command::new(cmd)
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Detect which AI agent CLIs are installed (claude, codex, gemini).
/// Returns a map of agent name → found in PATH.
#[tauri::command]
async fn create_worktree(working_dir: String, branch_name: String) -> Result<git_manager::WorktreeInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_manager::create_worktree(&working_dir, &branch_name)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn remove_worktree(worktree_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_manager::remove_worktree(&worktree_path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn clear_worktree_path(state: tauri::State<'_, SessionManager>, pty_id: u32) -> Result<(), String> {
    state.db.clear_worktree_path(pty_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_git_status(working_dir: String) -> Result<git_manager::GitStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_manager::get_git_status(&working_dir)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn detect_agents() -> std::collections::HashMap<String, bool> {
    ["claude", "codex", "gemini"]
        .iter()
        .map(|&name| (name.to_string(), is_in_path(name)))
        .collect()
}

#[tauri::command]
fn force_exit(app: tauri::AppHandle) {
    FORCE_EXIT.store(true, Ordering::SeqCst);
    app.exit(0);
}

#[derive(serde::Serialize)]
struct UpdateInfo {
    version: String,
    html_url: String,
    current_version: String,
}

/// Compare two semver strings (e.g. "0.8.11" vs "0.9.0").
/// Returns true if `available` is strictly newer than `current`.
fn is_newer_version(current: &str, available: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.').filter_map(|p| p.parse().ok()).collect()
    };
    let cur = parse(current);
    let avail = parse(available);
    for i in 0..3 {
        let c = cur.get(i).copied().unwrap_or(0);
        let a = avail.get(i).copied().unwrap_or(0);
        if a > c { return true; }
        if a < c { return false; }
    }
    false
}

#[tauri::command]
async fn check_for_update(force: bool) -> Result<Option<UpdateInfo>, String> {
    // Rate limit: skip if checked less than 10 minutes ago (unless forced).
    // Single lock scope for atomic read+stamp to prevent concurrent duplicate calls.
    if !force {
        let mut guard = LAST_UPDATE_CHECK.lock();
        if let Some(last) = *guard {
            if last.elapsed().as_secs() < 600 {
                return Ok(None);
            }
        }
        // Stamp now to prevent concurrent calls while network request is in-flight
        *guard = Some(Instant::now());
    }

    let current_version = env!("CARGO_PKG_VERSION");

    let result = tauri::async_runtime::spawn_blocking(move || -> Result<(String, String), String> {
        let mut response = ureq::get("https://api.github.com/repos/Pentium133/sessonix/releases/latest")
            .header("User-Agent", &format!("Sessonix/{}", current_version))
            .header("Accept", "application/vnd.github.v3+json")
            .call()
            .map_err(|e| format!("Network error: {}", e))?;

        let body: serde_json::Value = response.body_mut().read_json()
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let tag_name = body["tag_name"]
            .as_str()
            .ok_or_else(|| "Missing tag_name in response".to_string())?;
        let html_url = body["html_url"]
            .as_str()
            .ok_or_else(|| "Missing html_url in response".to_string())?;

        // Validate URL points to GitHub (defense-in-depth against supply chain attacks)
        if !html_url.starts_with("https://github.com/") {
            return Err("Unexpected release URL domain".to_string());
        }

        // Strip 'v' prefix from tag
        let version = tag_name.strip_prefix('v').unwrap_or(tag_name);

        Ok((version.to_string(), html_url.to_string()))
    })
    .await
    .map_err(|e| e.to_string())?;

    // On network/parse error, clear the timestamp so next attempt can try again
    let (version, html_url) = match result {
        Ok(v) => v,
        Err(e) => {
            *LAST_UPDATE_CHECK.lock() = None;
            return Err(e);
        }
    };

    if is_newer_version(current_version, &version) {
        Ok(Some(UpdateInfo {
            version,
            html_url,
            current_version: current_version.to_string(),
        }))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod iso_parse_tests {
    use super::parse_iso_to_unix_ms;

    #[test]
    fn parses_sqlite_datetime_now_format() {
        // Known epoch: 1970-01-01 00:00:00 UTC = 0
        assert_eq!(parse_iso_to_unix_ms("1970-01-01 00:00:00"), 0);
        // 1970-01-01 00:00:01 = 1000 ms
        assert_eq!(parse_iso_to_unix_ms("1970-01-01 00:00:01"), 1000);
        // 2000-01-01 00:00:00 UTC = 946_684_800 seconds
        assert_eq!(parse_iso_to_unix_ms("2000-01-01 00:00:00"), 946_684_800_000);
        // 2026-04-16 12:00:00 UTC = 1_776_340_800 seconds
        assert_eq!(parse_iso_to_unix_ms("2026-04-16 12:00:00"), 1_776_340_800_000);
    }

    #[test]
    fn malformed_returns_zero() {
        assert_eq!(parse_iso_to_unix_ms(""), 0);
        assert_eq!(parse_iso_to_unix_ms("not a date"), 0);
        assert_eq!(parse_iso_to_unix_ms("2026-04-16"), 0); // Too short
    }

    #[test]
    fn out_of_range_fields_return_zero() {
        // Month 00 or 13+
        assert_eq!(parse_iso_to_unix_ms("2026-00-16 12:00:00"), 0);
        assert_eq!(parse_iso_to_unix_ms("2026-13-16 12:00:00"), 0);
        // Day 00 or 32+
        assert_eq!(parse_iso_to_unix_ms("2026-04-00 12:00:00"), 0);
        assert_eq!(parse_iso_to_unix_ms("2026-04-32 12:00:00"), 0);
        // Hour/minute/second overflow
        assert_eq!(parse_iso_to_unix_ms("2026-04-16 24:00:00"), 0);
        assert_eq!(parse_iso_to_unix_ms("2026-04-16 12:60:00"), 0);
        // Non-numeric garbage in any field
        assert_eq!(parse_iso_to_unix_ms("2026-AA-16 12:00:00"), 0);
    }
}

#[cfg(test)]
mod task_validation_tests {
    use super::{validate_task_fields, TASK_FIELD_MAX_LEN};

    #[test]
    fn accepts_trimmed_nonempty_fields() {
        let (n, b) = validate_task_fields("  fix auth  ", "  feat/auth  ").unwrap();
        assert_eq!(n, "fix auth");
        assert_eq!(b, "feat/auth");
    }

    #[test]
    fn rejects_empty_name() {
        assert!(validate_task_fields("", "feat/x").is_err());
        assert!(validate_task_fields("   ", "feat/x").is_err());
    }

    #[test]
    fn rejects_empty_branch() {
        assert!(validate_task_fields("name", "").is_err());
        assert!(validate_task_fields("name", "   ").is_err());
    }

    #[test]
    fn rejects_name_too_long() {
        let too_long = "a".repeat(TASK_FIELD_MAX_LEN + 1);
        assert!(validate_task_fields(&too_long, "feat/x").is_err());
    }

    #[test]
    fn rejects_branch_too_long() {
        let too_long = "x".repeat(TASK_FIELD_MAX_LEN + 1);
        assert!(validate_task_fields("name", &too_long).is_err());
    }

    #[test]
    fn accepts_fields_at_max_length() {
        let at_max = "b".repeat(TASK_FIELD_MAX_LEN);
        let (n, b) = validate_task_fields(&at_max, &at_max).unwrap();
        assert_eq!(n.len(), TASK_FIELD_MAX_LEN);
        assert_eq!(b.len(), TASK_FIELD_MAX_LEN);
    }
}

#[cfg(test)]
mod update_tests {
    use super::is_newer_version;

    #[test]
    fn newer_patch() {
        assert!(is_newer_version("0.8.11", "0.8.12"));
    }

    #[test]
    fn newer_minor() {
        assert!(is_newer_version("0.8.11", "0.9.0"));
    }

    #[test]
    fn newer_major() {
        assert!(is_newer_version("0.8.11", "1.0.0"));
    }

    #[test]
    fn equal_versions() {
        assert!(!is_newer_version("0.8.11", "0.8.11"));
    }

    #[test]
    fn older_version() {
        assert!(!is_newer_version("0.9.0", "0.8.11"));
    }

    #[test]
    fn short_version() {
        assert!(is_newer_version("1.0", "1.1.0"));
        assert!(!is_newer_version("1.1.0", "1.0"));
    }

    #[test]
    fn same_major_minor_different_patch() {
        assert!(is_newer_version("1.2.3", "1.2.4"));
        assert!(!is_newer_version("1.2.4", "1.2.3"));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.sessonix.app");

    let db = match db::Db::open(&app_dir) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            log::error!("Failed to open database: {}", e);
            // Retry with fresh DB file
            let db_path = app_dir.join("sessonix.db");
            if db_path.exists() {
                let _ = std::fs::remove_file(&db_path);
            }
            Arc::new(
                db::Db::open(&app_dir)
                    .expect("Failed to open database even after reset"),
            )
        }
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(SessionManager::new(db))
        .manage(AdapterRegistry::new())
        .setup(|app| {
            // Custom macOS app menu: replace default Quit with our own that emits confirm-exit
            use tauri::menu::*;

            let settings_item = MenuItemBuilder::with_id("app-settings", "Settings...")
                .accelerator("CmdOrCtrl+,")
                .build(app)?;

            let update_item = MenuItemBuilder::with_id("check-updates", "Check for Updates...")
                .build(app)?;

            let quit_item = MenuItemBuilder::with_id("app-quit", "Quit Sessonix")
                .accelerator("CmdOrCtrl+Q")
                .build(app)?;

            let app_submenu = SubmenuBuilder::new(app, "Sessonix")
                .item(&PredefinedMenuItem::about(app, Some("About Sessonix"), None)?)
                .separator()
                .item(&settings_item)
                .item(&update_item)
                .separator()
                .item(&PredefinedMenuItem::hide(app, None)?)
                .item(&PredefinedMenuItem::hide_others(app, None)?)
                .item(&PredefinedMenuItem::show_all(app, None)?)
                .separator()
                .item(&quit_item)
                .build()?;

            let edit_submenu = SubmenuBuilder::new(app, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;

            let menu = MenuBuilder::new(app)
                .item(&app_submenu)
                .item(&edit_submenu)
                .build()?;

            app.set_menu(menu)?;

            app.on_menu_event(move |app_handle, event| {
                if event.id() == settings_item.id() {
                    let _ = app_handle.emit("open-settings", ());
                    return;
                }
                if event.id() == update_item.id() {
                    let _ = app_handle.emit("check-for-updates", ());
                    return;
                }
                if event.id() == quit_item.id() {
                    if FORCE_EXIT.load(Ordering::SeqCst) {
                        app_handle.exit(0);
                        return;
                    }
                    let sm = app_handle.state::<SessionManager>();
                    if sm.pty.running_count() > 0 {
                        let _ = app_handle.emit("confirm-exit", ());
                    } else {
                        app_handle.exit(0);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            create_session,
            write_to_session,
            resize_session,
            kill_session,
            detach_session,
            attach_session,
            get_session_count,
            get_session_status,
            get_available_agents,
            get_session_cost,
            install_claude_hooks,
            check_claude_hooks,
            add_project,
            remove_project,
            list_projects,
            list_sessions,
            save_scrollback,
            get_scrollback,
            delete_session,
            notify_session_exit,
            reorder_session,
            set_sort_order,
            get_running_session_count,
            force_exit,
            get_setting,
            set_setting,
            get_all_settings,
            detect_agents,
            get_git_status,
            create_worktree,
            remove_worktree,
            clear_worktree_path,
            check_for_update,
            create_template,
            list_templates,
            delete_template,
            update_template,
            create_task,
            list_tasks,
            delete_task,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = &event {
            if FORCE_EXIT.load(Ordering::SeqCst) {
                return; // User confirmed, let it exit
            }
            let sm = app_handle.state::<SessionManager>();
            if sm.pty.running_count() > 0 {
                api.prevent_exit();
                let _ = app_handle.emit("confirm-exit", ());
            }
        }
    });
}
