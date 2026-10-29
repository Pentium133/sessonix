//! Claude Code hooks management.
//! Installs hook handlers into ~/.claude/settings.json and reads hook status files.

use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
const HOOK_COMMAND: &str = "bash ~/.sessonix/hook-handler.sh";
#[cfg(unix)]
const HANDLER_SCRIPT: &str = include_str!("../resources/hook-handler.sh");

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

    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .map_err(|e| format!("Failed to read settings.json: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings.json: {}", e))?
    } else {
        serde_json::json!({})
    };
    if !settings.is_object() {
        return Err("settings.json is not an object".to_string());
    }

    let changed = merge_hooks(&mut settings);
    if changed {
        let output = serde_json::to_string_pretty(&settings)
            .map_err(|e| format!("Failed to serialize settings: {}", e))?;
        fs::write(&settings_path, output)
            .map_err(|e| format!("Failed to write settings.json: {}", e))?;
    }

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
    let Some(settings) = read_claude_settings() else {
        return Ok(false);
    };
    if !is_installed(&settings) || (hooks_complete(&settings) && handler_script_current()) {
        return Ok(false);
    }
    install_hooks().map(|_| true)
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
    read_claude_settings().is_some_and(|s| hooks_complete(&s)) && handler_script_current()
}

#[cfg(unix)]
fn read_claude_settings() -> Option<serde_json::Value> {
    let content = fs::read_to_string(claude_settings_path()?).ok()?;
    serde_json::from_str(&content).ok()
}

#[cfg(unix)]
fn handler_script_current() -> bool {
    fs::read_to_string(sessonix_dir().join("hook-handler.sh"))
        .is_ok_and(|on_disk| on_disk == HANDLER_SCRIPT)
}

/// A hook-config entry (`{"matcher"?, "hooks": [...]}`) that runs our command.
#[cfg(unix)]
fn entry_is_ours(entry: &serde_json::Value) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .is_some_and(|hooks| {
            hooks
                .iter()
                .any(|h| h.get("command").and_then(|c| c.as_str()) == Some(HOOK_COMMAND))
        })
}

/// Add a Sessonix entry for every event in [`HOOK_EVENTS`] that lacks one and
/// drop the matcher older versions set on the Notification entry. Returns
/// whether `settings` changed.
#[cfg(unix)]
fn merge_hooks(settings: &mut serde_json::Value) -> bool {
    let mut changed = false;
    if !settings.get("hooks").is_some_and(|h| h.is_object()) {
        settings["hooks"] = serde_json::json!({});
        changed = true;
    }
    let hooks = settings["hooks"].as_object_mut().expect("hooks is an object");

    for &(event, is_async) in HOOK_EVENTS {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]));
        if !entries.is_array() {
            *entries = serde_json::json!([]);
        }
        let entries = entries.as_array_mut().expect("array");

        let mut found = false;
        for entry in entries.iter_mut().filter(|e| entry_is_ours(e)) {
            found = true;
            if let Some(obj) = entry.as_object_mut() {
                changed |= obj.remove("matcher").is_some();
            }
        }
        if !found {
            let mut hook = serde_json::json!({ "type": "command", "command": HOOK_COMMAND });
            if is_async {
                hook["async"] = serde_json::json!(true);
            }
            entries.push(serde_json::json!({ "hooks": [hook] }));
            changed = true;
        }
    }
    changed
}

/// Every event in [`HOOK_EVENTS`] has a Sessonix entry, none with a matcher.
#[cfg(unix)]
fn hooks_complete(settings: &serde_json::Value) -> bool {
    HOOK_EVENTS.iter().all(|(event, _)| {
        settings["hooks"][*event].as_array().is_some_and(|entries| {
            let ours: Vec<_> = entries.iter().filter(|e| entry_is_ours(e)).collect();
            !ours.is_empty() && ours.iter().all(|e| e.get("matcher").is_none())
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

/// Install the hook handler script to ~/.sessonix/
#[cfg(unix)]
fn install_handler_script() -> Result<(), String> {
    let dir = sessonix_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create ~/.sessonix: {}", e))?;

    let script_path = dir.join("hook-handler.sh");

    // Always overwrite with latest version
    fs::write(&script_path, HANDLER_SCRIPT)
        .map_err(|e| format!("Failed to write hook-handler.sh: {}", e))?;

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
        assert!(merge_hooks(&mut settings));
        for (event, _) in HOOK_EVENTS {
            assert_eq!(sessonix_entries(&settings, event).len(), 1, "{event}");
        }
        assert!(hooks_complete(&settings));
    }

    #[test]
    fn upgrade_old_install() {
        let mut settings = old_install();
        assert!(!hooks_complete(&settings));
        assert!(merge_hooks(&mut settings));
        for (event, _) in HOOK_EVENTS {
            assert_eq!(sessonix_entries(&settings, event).len(), 1, "{event}");
        }
        let notif = sessonix_entries(&settings, "Notification")[0];
        assert!(notif.get("matcher").is_none());
        assert_eq!(settings["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
        assert_eq!(settings["model"], "opus");
        assert_eq!(sessonix_entries(&settings, "PreCompact")[0]["hooks"][0].get("async"), None);
        assert_eq!(sessonix_entries(&settings, "PreToolUse")[0]["hooks"][0]["async"], true);
        assert!(hooks_complete(&settings));
    }

    #[test]
    fn merge_is_idempotent() {
        let mut settings = old_install();
        merge_hooks(&mut settings);
        let snapshot = settings.clone();
        assert!(!merge_hooks(&mut settings));
        assert_eq!(settings, snapshot);
    }

    #[test]
    fn hooks_complete_false_when_event_missing() {
        let mut settings = serde_json::json!({});
        merge_hooks(&mut settings);
        settings["hooks"].as_object_mut().unwrap().remove("StopFailure");
        assert!(!hooks_complete(&settings));
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
