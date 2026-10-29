//! Claude Code hooks management.
//! Installs hook handlers into ~/.claude/settings.json and reads hook status files.

use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
const HOOK_COMMAND: &str = "bash ~/.sessonix/hook-handler.sh";
#[cfg(unix)]
const HANDLER_SCRIPT: &str = include_str!("../resources/hook-handler.sh");
#[cfg(unix)]
const OPENCODE_PLUGIN: &str = include_str!("../resources/opencode-status-plugin.js");

/// Hook status file written by hook-handler.sh. Fields are deserialized from
/// JSON produced by an external process; some are retained for future
/// consumers even though no code currently reads them.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct HookStatus {
    pub event: String,
    pub status: String,
    pub session_id: String,
    pub pty_id: u32,
    pub ts: i64,
    /// StopFailure error type (`rate_limit`, `overloaded`, ...).
    #[serde(default)]
    pub error: Option<String>,
}

/// How long a `running` status is trusted without a new hook event. Tool
/// events refresh it during a turn, so only a single tool call running longer
/// than this (or a missed event) lets it lapse.
const RUNNING_MAX_AGE_SECS: i64 = 600;

/// Read hook status for a given PTY session.
pub fn read_hook_status(pty_id: u32) -> Option<HookStatus> {
    let path = hooks_dir().join(format!("{}.json", pty_id));
    let content = fs::read_to_string(&path).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    parse_hook_status(&content, now)
}

/// Staleness: idle / exited / error / waiting_permission only change on the
/// next hook event (a permission prompt may wait for hours), so they never
/// expire. Anything else is treated as running and expires after
/// [`RUNNING_MAX_AGE_SECS`].
fn parse_hook_status(content: &str, now: i64) -> Option<HookStatus> {
    let status: HookStatus = serde_json::from_str(content).ok()?;
    let settled = matches!(
        status.status.as_str(),
        "idle" | "exited" | "error" | "waiting_permission"
    );
    if !settled && now - status.ts > RUNNING_MAX_AGE_SECS {
        return None;
    }
    Some(status)
}

/// Claude Code events Sessonix listens to, with whether the hook runs async.
/// PreCompact stays sync so the status is written before compaction starts.
#[cfg(unix)]
const HOOK_EVENTS: &[(&str, bool)] = &[
    ("SessionStart", true),
    ("UserPromptSubmit", true),
    ("PreToolUse", true),
    ("PostToolUse", true),
    ("PostToolUseFailure", true),
    ("PermissionDenied", true),
    ("SubagentStart", true),
    ("PreCompact", false),
    ("Stop", true),
    ("StopFailure", true),
    ("PermissionRequest", true),
    // No matcher: the handler filters by notification_type.
    ("Notification", true),
    ("SessionEnd", true),
];

/// Codex CLI events Sessonix listens to. Codex has no
/// Notification/StopFailure; `Interrupt` reports Esc.
#[cfg(unix)]
const CODEX_HOOK_EVENTS: &[(&str, bool)] = &[
    ("SessionStart", true),
    ("UserPromptSubmit", true),
    ("PreToolUse", true),
    ("PostToolUse", true),
    ("PermissionRequest", true),
    ("SubagentStart", true),
    ("PreCompact", true),
    ("PostCompact", true),
    ("Stop", true),
    ("Interrupt", true),
    // Codex always runs SessionEnd synchronously and warns if marked async.
    ("SessionEnd", false),
];

/// Installs or upgrades one agent's status hooks; `Ok(true)` when it changed
/// something.
pub type HookInstaller = fn() -> Result<bool, String>;

/// Gemini CLI events Sessonix listens to. Gemini runs hooks synchronously
/// (no async option documented).
#[cfg(unix)]
const GEMINI_HOOK_EVENTS: &[(&str, bool)] = &[
    ("SessionStart", false),
    ("BeforeAgent", false),
    ("BeforeTool", false),
    ("AfterTool", false),
    ("PreCompress", false),
    ("AfterAgent", false),
    // Filtered by notification_type (ToolPermission) in the handler.
    ("Notification", false),
    ("SessionEnd", false),
];

/// Cursor events Sessonix listens to. Never add a permission hook
/// (preToolUse, beforeShellExecution, beforeMCPExecution, beforeReadFile,
/// subagentStart): Cursor blocks the action when such a hook's output isn't a
/// valid permission decision, and answering "allow" would override the
/// user's permissions.
#[cfg(unix)]
const CURSOR_HOOK_EVENTS: &[&str] = &[
    "sessionStart",
    "beforeSubmitPrompt",
    "postToolUse",
    "postToolUseFailure",
    "afterShellExecution",
    "afterMCPExecution",
    "afterFileEdit",
    "preCompact",
    "stop",
    "sessionEnd",
];

/// Install Sessonix hooks into Claude's settings.json, adding any missing
/// events and upgrading entries written by older versions. Preserves hooks
/// from other tools. Always rewrites the handler script.
/// Returns whether settings.json changed.
///
/// Windows: no-op. The handler is a bash script and Windows Claude CLI has no
/// guaranteed bash available. Claude status detection falls back to JSONL
/// tailing, which is platform-independent.
#[cfg(windows)]
pub fn install_hooks() -> Result<bool, String> {
    Ok(false)
}

#[cfg(unix)]
pub fn install_hooks() -> Result<bool, String> {
    let settings_path = claude_settings_path()
        .ok_or_else(|| "Cannot find Claude config directory".to_string())?;
    let changed = install_into(&settings_path, HOOK_EVENTS, "claude")?;
    install_handler_script()?;
    Ok(changed)
}

/// Upgrade an existing install (settings or handler script out of date).
/// Never installs from scratch: that needs the user's consent.
/// Returns whether anything was upgraded.
#[cfg(windows)]
pub fn upgrade_if_installed() -> Result<bool, String> {
    Ok(false)
}

#[cfg(unix)]
pub fn upgrade_if_installed() -> Result<bool, String> {
    upgrade_existing(claude_settings_path(), HOOK_EVENTS, "claude", install_hooks)
}

/// Check if Sessonix hooks are installed and up to date.
///
/// Windows: always `false` — `install_hooks` is a no-op there.
#[cfg(windows)]
pub fn check_installed() -> bool {
    false
}

#[cfg(unix)]
pub fn check_installed() -> bool {
    claude_settings_path()
        .and_then(|p| read_json(&p))
        .is_some_and(|s| hooks_complete(&s, HOOK_EVENTS, "claude"))
        && handler_script_current()
}

/// Install Sessonix hooks into Codex's `hooks.json` (same merge rules as
/// Claude). Codex runs them only after the user trusts them via `/hooks`;
/// Sessonix never touches that trust state. Returns whether the file changed.
#[cfg(windows)]
pub fn install_codex_hooks() -> Result<bool, String> {
    Ok(false)
}

#[cfg(unix)]
pub fn install_codex_hooks() -> Result<bool, String> {
    let path = codex_hooks_path().ok_or_else(|| "Cannot find Codex config directory".to_string())?;
    let changed = install_into(&path, CODEX_HOOK_EVENTS, "codex")?;
    install_handler_script()?;
    Ok(changed)
}

/// Upgrade Sessonix Codex hooks if present and outdated.
#[cfg(windows)]
pub fn upgrade_codex_if_installed() -> Result<bool, String> {
    Ok(false)
}

#[cfg(unix)]
pub fn upgrade_codex_if_installed() -> Result<bool, String> {
    upgrade_existing(codex_hooks_path(), CODEX_HOOK_EVENTS, "codex", install_codex_hooks)
}

/// Install Sessonix hooks into Gemini CLI's `~/.gemini/settings.json`.
#[cfg(windows)]
pub fn install_gemini_hooks() -> Result<bool, String> {
    Ok(false)
}

#[cfg(unix)]
pub fn install_gemini_hooks() -> Result<bool, String> {
    let path = gemini_settings_path().ok_or_else(|| "Cannot find home directory".to_string())?;
    let changed = install_into(&path, GEMINI_HOOK_EVENTS, "gemini")?;
    install_handler_script()?;
    Ok(changed)
}

#[cfg(windows)]
pub fn upgrade_gemini_if_installed() -> Result<bool, String> {
    Ok(false)
}

#[cfg(unix)]
pub fn upgrade_gemini_if_installed() -> Result<bool, String> {
    upgrade_existing(gemini_settings_path(), GEMINI_HOOK_EVENTS, "gemini", install_gemini_hooks)
}

/// Install Sessonix hooks into Cursor's `~/.cursor/hooks.json` (flat entries).
#[cfg(windows)]
pub fn install_cursor_hooks() -> Result<bool, String> {
    Ok(false)
}

#[cfg(unix)]
pub fn install_cursor_hooks() -> Result<bool, String> {
    let path = cursor_hooks_path().ok_or_else(|| "Cannot find home directory".to_string())?;
    let changed = install_json(&path, |hooks| merge_flat_hooks(hooks, CURSOR_HOOK_EVENTS, "cursor"))?;
    install_handler_script()?;
    Ok(changed)
}

#[cfg(windows)]
pub fn upgrade_cursor_if_installed() -> Result<bool, String> {
    Ok(false)
}

#[cfg(unix)]
pub fn upgrade_cursor_if_installed() -> Result<bool, String> {
    let Some(hooks) = cursor_hooks_path().and_then(|p| read_json(&p)) else {
        return Ok(false);
    };
    let installed = hooks["hooks"].as_object().is_some_and(|events| {
        events.values().any(|e| e.as_array().is_some_and(|e| e.iter().any(is_our_command)))
    });
    if !installed || (flat_hooks_complete(&hooks, CURSOR_HOOK_EVENTS, "cursor") && handler_script_current()) {
        return Ok(false);
    }
    install_cursor_hooks().map(|_| true)
}

/// Install the Sessonix status plugin into OpenCode's global plugin dir.
#[cfg(windows)]
pub fn install_opencode_plugin() -> Result<bool, String> {
    Ok(false)
}

#[cfg(unix)]
pub fn install_opencode_plugin() -> Result<bool, String> {
    let path = opencode_plugin_path().ok_or_else(|| "Cannot find home directory".to_string())?;
    if !path.exists() {
        // The plugin loads in every OpenCode run; an unsupported module shape
        // would be a load error each time, so only install where it's known
        // to work.
        let version = std::process::Command::new("opencode")
            .arg("--version")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        if !opencode_supports_object_plugin(&version) {
            return Err(format!(
                "OpenCode {} too old for the status plugin (needs {}.{}.{}+)",
                version.trim(),
                OPENCODE_MIN_VERSION.0,
                OPENCODE_MIN_VERSION.1,
                OPENCODE_MIN_VERSION.2
            ));
        }
    }
    install_file(&path, OPENCODE_PLUGIN)
}

/// First OpenCode version that loads object plugin entrypoints (V1 `server()`
/// on a default-export object); older loaders call every export as a function.
#[cfg(unix)]
const OPENCODE_MIN_VERSION: (u32, u32, u32) = (1, 18, 29);

/// Whether `opencode --version` output names a version that supports the
/// plugin's module shape. Unknown output → false.
#[cfg(unix)]
fn opencode_supports_object_plugin(version_output: &str) -> bool {
    let Some(version) = version_output
        .split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))
    else {
        return false;
    };
    let mut parts = version.trim_start_matches('v').split('.').map(|p| {
        p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok()
    });
    match (parts.next().flatten(), parts.next().flatten(), parts.next().flatten()) {
        (Some(a), Some(b), Some(c)) => (a, b, c) >= OPENCODE_MIN_VERSION,
        _ => false,
    }
}

/// Update the plugin if a previous version is installed.
#[cfg(windows)]
pub fn upgrade_opencode_if_installed() -> Result<bool, String> {
    Ok(false)
}

#[cfg(unix)]
pub fn upgrade_opencode_if_installed() -> Result<bool, String> {
    match opencode_plugin_path() {
        Some(path) if path.exists() => install_file(&path, OPENCODE_PLUGIN),
        _ => Ok(false),
    }
}

/// Re-run `install` when `path` already holds Sessonix hooks but they (or the
/// handler script) are out of date.
#[cfg(unix)]
fn upgrade_existing(
    path: Option<PathBuf>,
    events: &[(&str, bool)],
    agent: &str,
    install: fn() -> Result<bool, String>,
) -> Result<bool, String> {
    let Some(settings) = path.and_then(|p| read_json(&p)) else {
        return Ok(false);
    };
    if !is_installed(&settings) || (hooks_complete(&settings, events, agent) && handler_script_current()) {
        return Ok(false);
    }
    install().map(|_| true)
}

/// Merge Sessonix hooks for `events` into the JSON file at `path` (created,
/// with its directory, if missing). Returns whether the file changed.
#[cfg(unix)]
fn install_into(path: &std::path::Path, events: &[(&str, bool)], agent: &str) -> Result<bool, String> {
    install_json(path, |settings| merge_hooks(settings, events, agent))
}

/// Read the JSON object at `path` (or `{}` if missing), apply `merge`, and
/// write it back atomically if it changed (directory created as needed).
#[cfg(unix)]
fn install_json(
    path: &std::path::Path,
    merge: impl FnOnce(&mut serde_json::Value) -> Result<bool, String>,
) -> Result<bool, String> {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let (mut settings, read_at): (serde_json::Value, _) = if path.exists() {
        let read_at = fs::metadata(path).and_then(|m| m.modified()).ok();
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", name, e))?;
        let value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", name, e))?;
        (value, read_at)
    } else {
        (serde_json::json!({}), None)
    };
    if !settings.is_object() {
        return Err(format!("{} is not an object", name));
    }

    let changed = merge(&mut settings)?;
    if changed {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("Failed to create {}: {}", dir.display(), e))?;
        }
        let output = serde_json::to_string_pretty(&settings)
            .map_err(|e| format!("Failed to serialize {}: {}", name, e))?;
        write_settings(path, &output, read_at)?;
    }
    Ok(changed)
}

#[cfg(unix)]
fn read_json(path: &std::path::Path) -> Option<serde_json::Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

#[cfg(unix)]
fn handler_script_current() -> bool {
    fs::read_to_string(sessonix_dir().join("hook-handler.sh"))
        .is_ok_and(|on_disk| on_disk == HANDLER_SCRIPT)
}

/// Replace settings.json atomically (temp file + rename in the target's
/// directory, following a symlink to its target). If `read_at` is given and
/// the file was modified after it — e.g. Claude Code wrote it meanwhile —
/// nothing is written and an error is returned, so no change is lost.
#[cfg(unix)]
fn write_settings(
    path: &std::path::Path,
    content: &str,
    read_at: Option<std::time::SystemTime>,
) -> Result<(), String> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let tmp = target.with_extension(format!("json.sessonix-{}.tmp", std::process::id()));
    fs::write(&tmp, content).map_err(|e| format!("Failed to write settings.json: {}", e))?;
    if let Some(read_at) = read_at {
        let now = fs::metadata(&target).and_then(|m| m.modified()).ok();
        if now != Some(read_at) {
            let _ = fs::remove_file(&tmp);
            return Err("settings.json changed while updating hooks; try again".to_string());
        }
    }
    fs::rename(&tmp, &target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("Failed to write settings.json: {}", e)
    })
}

/// Hook command for `agent`. The handler ignores events whose agent differs
/// from the PTY's `SESSONIX_AGENT`, so e.g. a `codex exec` run as a tool
/// inside a Claude session can't rewrite that session's status.
#[cfg(unix)]
fn agent_command(agent: &str) -> String {
    format!("{} {}", HOOK_COMMAND, agent)
}

/// Our handler, with or without an agent argument (older installs had none).
#[cfg(unix)]
fn is_our_command(hook: &serde_json::Value) -> bool {
    hook.get("command").and_then(|c| c.as_str()).is_some_and(|c| {
        c == HOOK_COMMAND || c.strip_prefix(HOOK_COMMAND).is_some_and(|rest| rest.starts_with(' '))
    })
}

/// A hook-config entry (`{"matcher"?, "hooks": [...]}`) that runs our command.
#[cfg(unix)]
fn entry_is_ours(entry: &serde_json::Value) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .is_some_and(|hooks| hooks.iter().any(is_our_command))
}

/// Our handler entry is exactly what this version installs for the event.
#[cfg(unix)]
fn our_hook_current(hook: &serde_json::Value, agent: &str, is_async: bool) -> bool {
    hook.get("command").and_then(|c| c.as_str()) == Some(agent_command(agent).as_str())
        && hook.get("async").and_then(|a| a.as_bool()).unwrap_or(false) == is_async
}

/// Add a Sessonix entry for every event in `events` that lacks one and
/// drop the matcher older versions set on the Notification entry. Returns
/// whether `settings` changed. Refuses (leaving `settings` untouched) when
/// `hooks` or an event's value has an unexpected shape rather than replace
/// the user's data.
#[cfg(unix)]
fn merge_hooks(
    settings: &mut serde_json::Value,
    events: &[(&str, bool)],
    agent: &str,
) -> Result<bool, String> {
    match settings.get("hooks") {
        None => {}
        Some(h) if h.is_object() => {}
        Some(_) => return Err("settings.json: \"hooks\" is not an object".to_string()),
    }
    if let Some(hooks) = settings.get("hooks").and_then(|h| h.as_object()) {
        for (event, _) in events {
            if hooks.get(*event).is_some_and(|v| !v.is_array()) {
                return Err(format!("settings.json: hooks.{} is not an array", event));
            }
        }
    }

    let mut changed = false;
    if settings.get("hooks").is_none() {
        settings["hooks"] = serde_json::json!({});
        changed = true;
    }
    let hooks = settings["hooks"].as_object_mut().expect("hooks is an object");

    for &(event, is_async) in events {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .expect("validated above");

        let mut found = false;
        for entry in entries.iter_mut().filter(|e| entry_is_ours(e)) {
            found = true;
            let Some(obj) = entry.as_object_mut() else { continue };
            changed |= obj.remove("matcher").is_some();
            // Bring entries from older versions to the current command/async.
            let Some(hooks) = obj.get_mut("hooks").and_then(|h| h.as_array_mut()) else { continue };
            for hook in hooks.iter_mut().filter(|h| is_our_command(h)) {
                if our_hook_current(hook, agent, is_async) {
                    continue;
                }
                let Some(h) = hook.as_object_mut() else { continue };
                h.insert("command".into(), serde_json::json!(agent_command(agent)));
                if is_async {
                    h.insert("async".into(), serde_json::json!(true));
                } else {
                    h.remove("async");
                }
                changed = true;
            }
        }
        if !found {
            let mut hook = serde_json::json!({ "type": "command", "command": agent_command(agent) });
            if is_async {
                hook["async"] = serde_json::json!(true);
            }
            entries.push(serde_json::json!({ "hooks": [hook] }));
            changed = true;
        }
    }
    Ok(changed)
}

/// Cursor-style `hooks.json`: `{"version": 1, "hooks": {event: [{"command", ...}]}}`
/// with hook objects directly in each event's array. Same rules as
/// [`merge_hooks`]: add missing, update our outdated commands, keep others,
/// refuse malformed shapes.
#[cfg(unix)]
fn merge_flat_hooks(
    settings: &mut serde_json::Value,
    events: &[&str],
    agent: &str,
) -> Result<bool, String> {
    match settings.get("hooks") {
        None => {}
        Some(h) if h.is_object() => {}
        Some(_) => return Err("hooks.json: \"hooks\" is not an object".to_string()),
    }
    if let Some(hooks) = settings.get("hooks").and_then(|h| h.as_object()) {
        for event in events {
            if hooks.get(*event).is_some_and(|v| !v.is_array()) {
                return Err(format!("hooks.json: hooks.{} is not an array", event));
            }
        }
    }

    let mut changed = false;
    if settings.get("version").is_none() {
        settings["version"] = serde_json::json!(1);
        changed = true;
    }
    if settings.get("hooks").is_none() {
        settings["hooks"] = serde_json::json!({});
        changed = true;
    }
    let hooks = settings["hooks"].as_object_mut().expect("hooks is an object");
    let command = agent_command(agent);
    for event in events {
        let entries = hooks
            .entry(*event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .expect("validated above");
        let mut found = false;
        for entry in entries.iter_mut().filter(|e| is_our_command(e)) {
            found = true;
            if entry["command"] != command.as_str() {
                entry["command"] = serde_json::json!(command);
                changed = true;
            }
        }
        if !found {
            entries.push(serde_json::json!({ "command": command }));
            changed = true;
        }
    }
    Ok(changed)
}

#[cfg(unix)]
fn flat_hooks_complete(settings: &serde_json::Value, events: &[&str], agent: &str) -> bool {
    let command = agent_command(agent);
    events.iter().all(|event| {
        settings["hooks"][*event].as_array().is_some_and(|entries| {
            let ours: Vec<_> = entries.iter().filter(|e| is_our_command(e)).collect();
            !ours.is_empty() && ours.iter().all(|e| e["command"] == command.as_str())
        })
    })
}

/// Every event in `events` has a Sessonix entry, none with a matcher.
#[cfg(unix)]
fn hooks_complete(settings: &serde_json::Value, events: &[(&str, bool)], agent: &str) -> bool {
    events.iter().all(|&(event, is_async)| {
        settings["hooks"][event].as_array().is_some_and(|entries| {
            let ours: Vec<_> = entries.iter().filter(|e| entry_is_ours(e)).collect();
            !ours.is_empty()
                && ours.iter().all(|e| {
                    e.get("matcher").is_none()
                        && e["hooks"].as_array().is_some_and(|hooks| {
                            hooks
                                .iter()
                                .filter(|h| is_our_command(h))
                                .all(|h| our_hook_current(h, agent, is_async))
                        })
                })
        })
    })
}

/// Any Sessonix hook present (the user installed hooks at some point).
#[cfg(unix)]
fn is_installed(settings: &serde_json::Value) -> bool {
    settings
        .get("hooks")
        .and_then(|h| h.as_object())
        .is_some_and(|hooks| {
            hooks
                .values()
                .any(|arr| arr.as_array().is_some_and(|e| e.iter().any(entry_is_ours)))
        })
}

/// Replace the file only when its content differs, via temp file + rename:
/// a hook of another session may be executing the script right now, and bash
/// reads scripts incrementally. Returns whether it was written.
#[cfg(unix)]
fn write_script_if_changed(path: &std::path::Path, content: &str) -> Result<bool, String> {
    if fs::read_to_string(path).is_ok_and(|current| current == content) {
        return Ok(false);
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!("{}.{}.tmp", name, std::process::id()));
    fs::write(&tmp, content).map_err(|e| format!("Failed to write {}: {}", name, e))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("Failed to write {}: {}", name, e)
    })?;
    Ok(true)
}

/// [`write_script_if_changed`], creating the parent directory first.
#[cfg(unix)]
fn install_file(path: &std::path::Path, content: &str) -> Result<bool, String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("Failed to create {}: {}", dir.display(), e))?;
    }
    write_script_if_changed(path, content)
}

/// Install the hook handler script to ~/.sessonix/
#[cfg(unix)]
fn install_handler_script() -> Result<(), String> {
    let dir = sessonix_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create ~/.sessonix: {}", e))?;

    let script_path = dir.join("hook-handler.sh");

    write_script_if_changed(&script_path, HANDLER_SCRIPT)?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&script_path, perms)
            .map_err(|e| format!("Failed to chmod: {}", e))?;
    }

    // Ensure hooks output directory exists
    fs::create_dir_all(hooks_dir())
        .map_err(|e| format!("Failed to create hooks dir: {}", e))?;

    Ok(())
}

#[cfg(unix)]
fn gemini_settings_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".gemini").join("settings.json"))
}

#[cfg(unix)]
fn cursor_hooks_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".cursor").join("hooks.json"))
}

/// `$XDG_CONFIG_HOME/opencode/plugins/sessonix-status.js`, defaulting to `~/.config`.
#[cfg(unix)]
fn opencode_plugin_path() -> Option<PathBuf> {
    let config = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => dirs::home_dir()?.join(".config"),
    };
    Some(config.join("opencode").join("plugins").join("sessonix-status.js"))
}

/// `$CODEX_HOME/hooks.json`, defaulting to `~/.codex/hooks.json`.
#[cfg(unix)]
fn codex_hooks_path() -> Option<PathBuf> {
    let home = match std::env::var_os("CODEX_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => dirs::home_dir()?.join(".codex"),
    };
    Some(home.join("hooks.json"))
}

#[cfg(unix)]
fn claude_settings_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".claude").join("settings.json"))
}

fn sessonix_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".sessonix")
}

fn hooks_dir() -> PathBuf {
    sessonix_dir().join("hooks")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Run the bundled handler as installed for `agent` inside a PTY spawned
    /// for `env_agent`, over an optional existing status file.
    fn run_handler_as(
        agent: &str,
        env_agent: &str,
        existing: Option<serde_json::Value>,
        input: serde_json::Value,
    ) -> Option<serde_json::Value> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".sessonix/hooks");
        if let Some(existing) = existing {
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("42.json"), existing.to_string()).unwrap();
        }
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/hook-handler.sh");
        let mut child = Command::new("bash")
            .arg(script)
            .arg(agent)
            .env("HOME", home.path())
            .env("SESSONIX_PTY_ID", "42")
            .env("SESSONIX_AGENT", env_agent)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
        let content = fs::read_to_string(dir.join("42.json")).ok()?;
        Some(serde_json::from_str(&content).unwrap())
    }

    /// Run the handler for `agent`; `pty` = Some((pty_id, env_agent)) when
    /// inside a Sessonix PTY. Returns (stdout, status file if written).
    fn run_handler_io(
        agent: &str,
        pty: Option<(&str, &str)>,
        existing: Option<serde_json::Value>,
        input: serde_json::Value,
    ) -> (String, Option<serde_json::Value>) {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".sessonix/hooks");
        if let Some(existing) = existing {
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("42.json"), existing.to_string()).unwrap();
        }
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/hook-handler.sh");
        let mut cmd = Command::new("bash");
        cmd.arg(script).arg(agent).env("HOME", home.path())
            .env_remove("SESSONIX_PTY_ID").env_remove("SESSONIX_AGENT");
        if let Some((id, env_agent)) = pty {
            cmd.env("SESSONIX_PTY_ID", id).env("SESSONIX_AGENT", env_agent);
        }
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        let file = fs::read_to_string(dir.join("42.json")).ok().map(|c| serde_json::from_str(&c).unwrap());
        (String::from_utf8(out.stdout).unwrap(), file)
    }

    fn ev(name: &str) -> serde_json::Value {
        serde_json::json!({"hook_event_name": name, "session_id": "s"})
    }

    #[test]
    fn gemini_maps_events() {
        for (event, want) in [
            ("BeforeAgent", "running"),
            ("BeforeTool", "running"),
            ("AfterTool", "running"),
            ("PreCompress", "running"),
            ("AfterAgent", "idle"),
            ("SessionStart", "idle"),
            ("SessionEnd", "exited"),
        ] {
            let (_, file) = run_handler_io("gemini", Some(("42", "gemini")), None, ev(event));
            assert_eq!(file.unwrap()["status"], want, "{event}");
        }
        let (_, file) = run_handler_io(
            "gemini",
            Some(("42", "gemini")),
            None,
            serde_json::json!({"hook_event_name": "Notification", "session_id": "s", "notification_type": "ToolPermission"}),
        );
        assert_eq!(file.unwrap()["status"], "waiting_permission");
    }

    #[test]
    fn gemini_prints_empty_json() {
        let (out, _) = run_handler_io("gemini", Some(("42", "gemini")), None, ev("BeforeTool"));
        assert_eq!(out.trim(), "{}");
        let (out, _) = run_handler_io("gemini", Some(("42", "gemini")), None, ev("BeforeModel"));
        assert_eq!(out.trim(), "{}");
    }

    #[test]
    fn gemini_outside_sessonix_prints_json() {
        let (out, file) = run_handler_io("gemini", None, None, ev("BeforeTool"));
        assert_eq!(out.trim(), "{}");
        assert!(file.is_none());
        let (out, file) = run_handler_io("gemini", Some(("42", "claude")), None, ev("BeforeTool"));
        assert_eq!(out.trim(), "{}");
        assert!(file.is_none());
    }

    #[test]
    fn gemini_late_tool_after_after_agent_keeps_idle() {
        let (_, file) = run_handler_io(
            "gemini",
            Some(("42", "gemini")),
            Some(status_now("AfterAgent", "idle")),
            ev("AfterTool"),
        );
        assert_eq!(file.unwrap()["status"], "idle");
    }

    #[test]
    fn cursor_maps_events() {
        for (event, want) in [
            ("beforeSubmitPrompt", "running"),
            ("postToolUse", "running"),
            ("postToolUseFailure", "running"),
            ("afterShellExecution", "running"),
            ("afterMCPExecution", "running"),
            ("afterFileEdit", "running"),
            ("preCompact", "running"),
            ("sessionStart", "idle"),
            ("sessionEnd", "exited"),
        ] {
            let (_, file) = run_handler_io("cursor", Some(("42", "cursor")), None, ev(event));
            assert_eq!(file.unwrap()["status"], want, "{event}");
        }
    }

    #[test]
    fn cursor_stop_status() {
        for (status, want) in [("completed", "idle"), ("aborted", "idle"), ("error", "error")] {
            let (_, file) = run_handler_io(
                "cursor",
                Some(("42", "cursor")),
                None,
                serde_json::json!({"hook_event_name": "stop", "status": status}),
            );
            let file = file.unwrap();
            assert_eq!(file["status"], want, "{status}");
            if want == "error" {
                assert_eq!(file["error"], "agent error");
            }
        }
    }

    #[test]
    fn cursor_outputs() {
        let (out, _) = run_handler_io("cursor", Some(("42", "cursor")), None, ev("beforeSubmitPrompt"));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap(), serde_json::json!({"continue": true}));
        let (out, _) = run_handler_io("cursor", Some(("42", "cursor")), None, ev("postToolUse"));
        assert_eq!(out.trim(), "{}");
    }

    #[test]
    fn cursor_outside_sessonix_prints_json() {
        let (out, file) = run_handler_io("cursor", None, None, ev("beforeSubmitPrompt"));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap(), serde_json::json!({"continue": true}));
        assert!(file.is_none());
    }

    /// Run the handler with a PATH that has no python3 (bash builtins only).
    fn run_handler_without_python(agent: &str, input: &str) -> String {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let home = tempfile::tempdir().unwrap();
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/hook-handler.sh");
        let mut child = Command::new("/bin/bash")
            .arg(script)
            .arg(agent)
            .env("HOME", home.path())
            .env("PATH", "/nonexistent")
            .env("SESSONIX_PTY_ID", "42")
            .env("SESSONIX_AGENT", agent)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    }

    #[test]
    fn gemini_responds_without_python() {
        assert_eq!(run_handler_without_python("gemini", r#"{"hook_event_name":"BeforeTool"}"#).trim(), "{}");
    }

    #[test]
    fn cursor_responds_without_python() {
        let out = run_handler_without_python("cursor", r#"{"hook_event_name": "beforeSubmitPrompt", "prompt": "x"}"#);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap(), serde_json::json!({"continue": true}));
        assert_eq!(run_handler_without_python("cursor", r#"{"hook_event_name":"stop"}"#).trim(), "{}");
    }

    #[test]
    fn gemini_non_object_input_still_responds() {
        let (out, file) = run_handler_io("gemini", Some(("42", "gemini")), None, serde_json::json!([1]));
        assert_eq!(out.trim(), "{}");
        assert!(file.is_none());
    }

    #[test]
    fn opencode_version_gate() {
        assert!(opencode_supports_object_plugin("1.18.29"));
        assert!(opencode_supports_object_plugin("opencode 1.19.0"));
        assert!(opencode_supports_object_plugin("2.0.0\n"));
        assert!(!opencode_supports_object_plugin("1.18.28"));
        assert!(!opencode_supports_object_plugin("0.15.3"));
        assert!(!opencode_supports_object_plugin("garbage"));
    }

    #[test]
    fn claude_prints_nothing() {
        let (out, _) = run_handler_io("claude", Some(("42", "claude")), None, ev("Stop"));
        assert_eq!(out, "");
    }

    #[test]
    fn gemini_merge_into_empty_is_sync() {
        let mut settings = serde_json::json!({"general": {"enableNotifications": true}});
        assert!(merge_hooks(&mut settings, GEMINI_HOOK_EVENTS, "gemini").unwrap());
        for (event, _) in GEMINI_HOOK_EVENTS {
            let ours = sessonix_entries(&settings, event);
            assert_eq!(ours.len(), 1, "{event}");
            assert_eq!(ours[0]["hooks"][0]["command"], "bash ~/.sessonix/hook-handler.sh gemini");
            assert_eq!(ours[0]["hooks"][0].get("async"), None);
        }
        assert_eq!(settings["general"]["enableNotifications"], true);
        assert!(hooks_complete(&settings, GEMINI_HOOK_EVENTS, "gemini"));
    }

    #[test]
    fn cursor_never_registers_permission_hooks() {
        for forbidden in ["preToolUse", "beforeShellExecution", "beforeMCPExecution", "beforeReadFile", "subagentStart"] {
            assert!(!CURSOR_HOOK_EVENTS.contains(&forbidden), "{forbidden}");
        }
    }

    #[test]
    fn cursor_merge_into_empty() {
        let mut hooks = serde_json::json!({});
        assert!(merge_flat_hooks(&mut hooks, CURSOR_HOOK_EVENTS, "cursor").unwrap());
        assert_eq!(hooks["version"], 1);
        for event in CURSOR_HOOK_EVENTS {
            let entries = hooks["hooks"][*event].as_array().unwrap();
            assert_eq!(entries, &vec![serde_json::json!({"command": "bash ~/.sessonix/hook-handler.sh cursor"})], "{event}");
        }
        assert!(flat_hooks_complete(&hooks, CURSOR_HOOK_EVENTS, "cursor"));
        assert!(!merge_flat_hooks(&mut hooks, CURSOR_HOOK_EVENTS, "cursor").unwrap());
    }

    #[test]
    fn cursor_merge_keeps_user_hooks() {
        let mut hooks = serde_json::json!({
            "version": 1,
            "hooks": {"stop": [{"command": "./audit.sh", "loop_limit": 10}], "preToolUse": [{"command": "./guard.sh"}]}
        });
        merge_flat_hooks(&mut hooks, CURSOR_HOOK_EVENTS, "cursor").unwrap();
        let stop = hooks["hooks"]["stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0], serde_json::json!({"command": "./audit.sh", "loop_limit": 10}));
        assert_eq!(hooks["hooks"]["preToolUse"], serde_json::json!([{"command": "./guard.sh"}]));
    }

    #[test]
    fn cursor_merge_rejects_malformed() {
        let mut hooks = serde_json::json!({"hooks": {"stop": "nope"}});
        assert!(merge_flat_hooks(&mut hooks, CURSOR_HOOK_EVENTS, "cursor").is_err());
        assert_eq!(hooks["hooks"]["stop"], "nope");
    }

    #[test]
    fn opencode_plugin_installed_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opencode/plugins/sessonix-status.js");
        assert!(install_file(&path, OPENCODE_PLUGIN).unwrap());
        assert!(!install_file(&path, OPENCODE_PLUGIN).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), OPENCODE_PLUGIN);
    }

    #[test]
    fn handler_ignores_event_from_other_agent() {
        let out = run_handler_as(
            "codex",
            "claude",
            None,
            serde_json::json!({"hook_event_name": "Stop", "session_id": "s"}),
        );
        assert_eq!(out, None);
    }

    #[test]
    fn handler_claude_session_start_resets_running() {
        let out = run_handler_as(
            "claude",
            "claude",
            Some(status_now("PreToolUse", "running")),
            serde_json::json!({"hook_event_name": "SessionStart", "session_id": "s", "source": "clear"}),
        )
        .unwrap();
        assert_eq!(out["status"], "idle");
    }

    #[test]
    fn handler_codex_session_start_does_not_override_running() {
        let out = run_handler_as(
            "codex",
            "codex",
            Some(status_now("PreToolUse", "running")),
            serde_json::json!({"hook_event_name": "SessionStart", "session_id": "s", "source": "startup"}),
        )
        .unwrap();
        assert_eq!(out["status"], "running");
    }

    #[test]
    fn commands_carry_agent_name() {
        let mut claude = serde_json::json!({});
        merge_hooks(&mut claude, HOOK_EVENTS, "claude").unwrap();
        assert_eq!(sessonix_entries(&claude, "Stop")[0]["hooks"][0]["command"], "bash ~/.sessonix/hook-handler.sh claude");
        let mut codex = serde_json::json!({});
        merge_hooks(&mut codex, CODEX_HOOK_EVENTS, "codex").unwrap();
        assert_eq!(sessonix_entries(&codex, "Stop")[0]["hooks"][0]["command"], "bash ~/.sessonix/hook-handler.sh codex");
    }

    #[test]
    fn codex_session_end_is_sync() {
        let mut codex = serde_json::json!({});
        merge_hooks(&mut codex, CODEX_HOOK_EVENTS, "codex").unwrap();
        assert_eq!(sessonix_entries(&codex, "SessionEnd")[0]["hooks"][0].get("async"), None);
    }

    #[test]
    fn script_write_skipped_when_current() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hook-handler.sh");
        assert!(write_script_if_changed(&path, "a").unwrap());
        assert!(!write_script_if_changed(&path, "a").unwrap());
        assert!(write_script_if_changed(&path, "b").unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "b");
    }

    /// Run the bundled handler with `input` on stdin; return the status file it wrote.
    fn run_handler(input: serde_json::Value) -> Option<serde_json::Value> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let home = tempfile::tempdir().unwrap();
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/hook-handler.sh");
        let mut child = Command::new("bash")
            .arg(script)
            .env("HOME", home.path())
            .env("SESSONIX_PTY_ID", "42")
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
        let content = fs::read_to_string(home.path().join(".sessonix/hooks/42.json")).ok()?;
        Some(serde_json::from_str(&content).unwrap())
    }

    fn handler_status(input: serde_json::Value) -> Option<String> {
        run_handler(input).map(|v| v["status"].as_str().unwrap().to_string())
    }

    #[test]
    fn handler_maps_tool_event_to_running() {
        let s = handler_status(serde_json::json!({"hook_event_name": "PreToolUse", "session_id": "s"}));
        assert_eq!(s.as_deref(), Some("running"));
    }

    #[test]
    fn handler_maps_stop_failure_to_error() {
        let out = run_handler(serde_json::json!({
            "hook_event_name": "StopFailure", "session_id": "s", "error": "rate_limit"
        }))
        .unwrap();
        assert_eq!(out["status"], "error");
        assert_eq!(out["error"], "rate_limit");
    }

    #[test]
    fn handler_maps_permission_notification() {
        let s = handler_status(serde_json::json!({
            "hook_event_name": "Notification", "session_id": "s", "notification_type": "permission_prompt"
        }));
        assert_eq!(s.as_deref(), Some("waiting_permission"));
    }

    #[test]
    fn handler_maps_idle_notification() {
        let s = handler_status(serde_json::json!({
            "hook_event_name": "Notification", "session_id": "s", "notification_type": "idle_prompt"
        }));
        assert_eq!(s.as_deref(), Some("idle"));
    }

    #[test]
    fn handler_ignores_other_notifications() {
        let s = handler_status(serde_json::json!({
            "hook_event_name": "Notification", "session_id": "s", "notification_type": "auth_success"
        }));
        assert_eq!(s, None);
    }

    #[test]
    fn handler_maps_stop_to_idle() {
        let s = handler_status(serde_json::json!({"hook_event_name": "Stop", "session_id": "s"}));
        assert_eq!(s.as_deref(), Some("idle"));
    }

    fn status_json(status: &str, ts: i64) -> String {
        serde_json::json!({
            "event": "x", "status": status, "session_id": "s", "pty_id": 1, "ts": ts
        })
        .to_string()
    }

    #[test]
    fn running_fresh_within_10_min() {
        assert!(parse_hook_status(&status_json("running", 1000), 1599).is_some());
    }

    #[test]
    fn running_stale_after_10_min() {
        assert!(parse_hook_status(&status_json("running", 1000), 1601).is_none());
    }

    #[test]
    fn idle_never_stale() {
        assert!(parse_hook_status(&status_json("idle", 0), 86_400).is_some());
    }

    #[test]
    fn waiting_permission_never_stale() {
        assert!(parse_hook_status(&status_json("waiting_permission", 0), 86_400).is_some());
    }

    #[test]
    fn error_never_stale() {
        let s = parse_hook_status(
            r#"{"event":"StopFailure","status":"error","session_id":"s","pty_id":1,"ts":0,"error":"rate_limit"}"#,
            86_400,
        )
        .unwrap();
        assert_eq!(s.error.as_deref(), Some("rate_limit"));
    }

    #[test]
    fn unknown_status_uses_running_age() {
        assert!(parse_hook_status(&status_json("whatever", 1000), 1601).is_none());
    }

    fn ours(is_async: bool) -> serde_json::Value {
        if is_async {
            serde_json::json!({"type": "command", "command": HOOK_COMMAND, "async": true})
        } else {
            serde_json::json!({"type": "command", "command": HOOK_COMMAND})
        }
    }

    /// Layout written by the previous installer, plus another tool's hook.
    fn old_install() -> serde_json::Value {
        serde_json::json!({
            "model": "opus",
            "hooks": {
                "SessionStart": [
                    {"hooks": [ours(true)]},
                    {"hooks": [{"type": "command", "command": "python3 obsidian/load_vault.py"}]}
                ],
                "UserPromptSubmit": [{"hooks": [ours(true)]}],
                "Stop": [{"hooks": [ours(true)]}],
                "PermissionRequest": [{"hooks": [ours(true)]}],
                "SessionEnd": [{"hooks": [ours(true)]}],
                "PreCompact": [{"hooks": [ours(false)]}],
                "Notification": [{"matcher": "permission_prompt|elicitation_dialog", "hooks": [ours(true)]}]
            }
        })
    }

    fn sessonix_entries<'a>(settings: &'a serde_json::Value, event: &str) -> Vec<&'a serde_json::Value> {
        settings["hooks"][event]
            .as_array()
            .map(|a| a.iter().filter(|e| entry_is_ours(e)).collect())
            .unwrap_or_default()
    }

    #[test]
    fn merge_into_empty_adds_all_events() {
        let mut settings = serde_json::json!({});
        assert!(merge_hooks(&mut settings, HOOK_EVENTS, "claude").unwrap());
        for (event, _) in HOOK_EVENTS {
            assert_eq!(sessonix_entries(&settings, event).len(), 1, "{event}");
        }
        assert!(hooks_complete(&settings, HOOK_EVENTS, "claude"));
    }

    #[test]
    fn upgrade_old_install() {
        let mut settings = old_install();
        assert!(!hooks_complete(&settings, HOOK_EVENTS, "claude"));
        assert!(merge_hooks(&mut settings, HOOK_EVENTS, "claude").unwrap());
        for (event, _) in HOOK_EVENTS {
            assert_eq!(sessonix_entries(&settings, event).len(), 1, "{event}");
        }
        let notif = sessonix_entries(&settings, "Notification")[0];
        assert!(notif.get("matcher").is_none());
        assert_eq!(settings["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
        assert_eq!(settings["model"], "opus");
        assert_eq!(sessonix_entries(&settings, "PreCompact")[0]["hooks"][0].get("async"), None);
        assert_eq!(sessonix_entries(&settings, "PreToolUse")[0]["hooks"][0]["async"], true);
        assert!(hooks_complete(&settings, HOOK_EVENTS, "claude"));
    }

    #[test]
    fn merge_is_idempotent() {
        let mut settings = old_install();
        merge_hooks(&mut settings, HOOK_EVENTS, "claude").unwrap();
        let snapshot = settings.clone();
        assert!(!merge_hooks(&mut settings, HOOK_EVENTS, "claude").unwrap());
        assert_eq!(settings, snapshot);
    }

    #[test]
    fn hooks_complete_false_when_event_missing() {
        let mut settings = serde_json::json!({});
        merge_hooks(&mut settings, HOOK_EVENTS, "claude").unwrap();
        settings["hooks"].as_object_mut().unwrap().remove("StopFailure");
        assert!(!hooks_complete(&settings, HOOK_EVENTS, "claude"));
    }

    /// Run the handler with a pre-existing status file in a fresh HOME.
    fn run_handler_over(existing: serde_json::Value, input: serde_json::Value) -> serde_json::Value {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".sessonix/hooks");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("42.json"), existing.to_string()).unwrap();
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/hook-handler.sh");
        let mut child = Command::new("bash")
            .arg(script)
            .env("HOME", home.path())
            .env("SESSONIX_PTY_ID", "42")
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
        serde_json::from_str(&fs::read_to_string(dir.join("42.json")).unwrap()).unwrap()
    }

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }

    #[test]
    fn handler_session_start_after_compact_is_running() {
        let s = handler_status(serde_json::json!({
            "hook_event_name": "SessionStart", "session_id": "s", "source": "compact"
        }));
        assert_eq!(s.as_deref(), Some("running"));
    }

    #[test]
    fn handler_late_tool_event_does_not_override_fresh_stop() {
        let existing = serde_json::json!({
            "event": "Stop", "status": "idle", "session_id": "s", "pty_id": 42,
            "ts": now_ms() / 1000, "ts_ms": now_ms()
        });
        let out = run_handler_over(existing, serde_json::json!({"hook_event_name": "PostToolUse", "session_id": "s"}));
        assert_eq!(out["status"], "idle");
    }

    #[test]
    fn handler_late_pre_tool_does_not_override_fresh_permission() {
        let existing = serde_json::json!({
            "event": "PermissionRequest", "status": "waiting_permission", "session_id": "s",
            "pty_id": 42, "ts": now_ms() / 1000, "ts_ms": now_ms()
        });
        let out = run_handler_over(existing, serde_json::json!({"hook_event_name": "PreToolUse", "session_id": "s"}));
        assert_eq!(out["status"], "waiting_permission");
    }

    #[test]
    fn handler_tool_event_overrides_old_stop() {
        let old = now_ms() - 10_000;
        let existing = serde_json::json!({
            "event": "Stop", "status": "idle", "session_id": "s", "pty_id": 42,
            "ts": old / 1000, "ts_ms": old
        });
        let out = run_handler_over(existing, serde_json::json!({"hook_event_name": "PreToolUse", "session_id": "s"}));
        assert_eq!(out["status"], "running");
    }

    #[test]
    fn handler_parallel_writes_leave_valid_json() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let home = tempfile::tempdir().unwrap();
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/hook-handler.sh");
        let children: Vec<_> = (0..24)
            .map(|i| {
                let event = if i % 2 == 0 { "PostToolUse" } else { "PostToolUseFailure" };
                let mut c = Command::new("bash")
                    .arg(script)
                    .env("HOME", home.path())
                    .env("SESSONIX_PTY_ID", "42")
                    .stdin(Stdio::piped())
                    .spawn()
                    .unwrap();
                let input = serde_json::json!({"hook_event_name": event, "session_id": "s".repeat(i + 1)});
                c.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
                c
            })
            .collect();
        for mut c in children {
            c.wait().unwrap();
        }
        let content = fs::read_to_string(home.path().join(".sessonix/hooks/42.json")).unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&content).is_ok(), "{content}");
    }

    #[test]
    fn merge_preserves_key_order() {
        let mut settings: serde_json::Value =
            serde_json::from_str(r#"{"zeta": 1, "alpha": 2, "hooks": {"Stop": []}}"#).unwrap();
        merge_hooks(&mut settings, HOOK_EVENTS, "claude").unwrap();
        let out = serde_json::to_string(&settings).unwrap();
        assert!(out.find("zeta").unwrap() < out.find("alpha").unwrap(), "{out}");
    }

    #[test]
    fn merge_rejects_non_object_hooks() {
        let mut settings = serde_json::json!({"hooks": ["user", "data"]});
        assert!(merge_hooks(&mut settings, HOOK_EVENTS, "claude").is_err());
        assert_eq!(settings, serde_json::json!({"hooks": ["user", "data"]}));
    }

    #[test]
    fn merge_rejects_non_array_event() {
        let mut settings = serde_json::json!({"hooks": {"Stop": {"weird": true}}});
        assert!(merge_hooks(&mut settings, HOOK_EVENTS, "claude").is_err());
        assert_eq!(settings["hooks"]["Stop"], serde_json::json!({"weird": true}));
    }

    #[test]
    fn write_atomic_keeps_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.json");
        let link = dir.path().join("settings.json");
        fs::write(&target, "{}").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        write_settings(&link, "{\"a\":1}", None).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"a\":1}");
    }

    #[test]
    fn write_refuses_when_file_changed_since_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{}").unwrap();
        let read_at = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&path, "{\"changed\":true}").unwrap();
        assert!(write_settings(&path, "{}", Some(read_at)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"changed\":true}");
    }

    fn status_now(event: &str, status: &str) -> serde_json::Value {
        serde_json::json!({
            "event": event, "status": status, "session_id": "s", "pty_id": 42,
            "ts": now_ms() / 1000, "ts_ms": now_ms()
        })
    }

    #[test]
    fn handler_maps_interrupt_to_idle() {
        let s = handler_status(serde_json::json!({"hook_event_name": "Interrupt", "session_id": "s"}));
        assert_eq!(s.as_deref(), Some("idle"));
    }

    #[test]
    fn handler_maps_post_compact_to_running() {
        let s = handler_status(serde_json::json!({"hook_event_name": "PostCompact", "session_id": "s"}));
        assert_eq!(s.as_deref(), Some("running"));
    }

    #[test]
    fn handler_session_start_sets_idle_when_not_running() {
        let out = run_handler_over(
            status_now("Stop", "idle"),
            serde_json::json!({"hook_event_name": "SessionStart", "session_id": "s", "source": "startup"}),
        );
        assert_eq!(out["status"], "idle");
        assert_eq!(out["event"], "SessionStart");
    }

    #[test]
    fn handler_late_tool_event_does_not_override_fresh_interrupt() {
        let out = run_handler_over(
            status_now("Interrupt", "idle"),
            serde_json::json!({"hook_event_name": "PostToolUse", "session_id": "s"}),
        );
        assert_eq!(out["status"], "idle");
    }

    #[test]
    fn codex_merge_into_empty() {
        let mut settings = serde_json::json!({});
        assert!(merge_hooks(&mut settings, CODEX_HOOK_EVENTS, "codex").unwrap());
        for (event, _) in CODEX_HOOK_EVENTS {
            let ours = sessonix_entries(&settings, event);
            assert_eq!(ours.len(), 1, "{event}");
        }
        assert!(hooks_complete(&settings, CODEX_HOOK_EVENTS, "codex"));
        assert!(settings["hooks"].get("Notification").is_none());
    }

    #[test]
    fn codex_merge_keeps_user_hooks() {
        let mut settings = serde_json::json!({
            "description": "mine",
            "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]}
        });
        merge_hooks(&mut settings, CODEX_HOOK_EVENTS, "codex").unwrap();
        assert_eq!(settings["description"], "mine");
        let stop = settings["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "say done");
    }

    #[test]
    fn install_into_creates_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/hooks.json");
        assert!(install_into(&path, CODEX_HOOK_EVENTS, "codex").unwrap());
        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(hooks_complete(&written, CODEX_HOOK_EVENTS, "codex"));
    }

    #[test]
    fn install_into_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.json");
        install_into(&path, CODEX_HOOK_EVENTS, "codex").unwrap();
        let first = fs::read_to_string(&path).unwrap();
        assert!(!install_into(&path, CODEX_HOOK_EVENTS, "codex").unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), first);
    }

    #[test]
    fn test_is_installed_empty() {
        let settings = serde_json::json!({});
        assert!(!is_installed(&settings));
    }

    #[test]
    fn test_is_installed_with_hook() {
        let settings = serde_json::json!({
            "hooks": {
                "Stop": [{
                    "hooks": [{
                        "type": "command",
                        "command": HOOK_COMMAND,
                        "async": true
                    }]
                }]
            }
        });
        assert!(is_installed(&settings));
    }

    #[test]
    fn test_is_installed_other_hooks_only() {
        let settings = serde_json::json!({
            "hooks": {
                "Stop": [{
                    "hooks": [{
                        "type": "command",
                        "command": "agent-deck hook-handler",
                        "async": true
                    }]
                }]
            }
        });
        assert!(!is_installed(&settings));
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn test_install_hooks_is_noop_on_windows() {
        // Must not touch ~/.claude/settings.json or write any script.
        assert_eq!(install_hooks().unwrap(), false);
        assert!(!check_installed());
    }
}
