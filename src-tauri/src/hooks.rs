//! Claude Code hooks management.
//! Installs hook handlers into ~/.claude/settings.json and reads hook status files.

use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
const HOOK_COMMAND: &str = "bash ~/.sessonix/hook-handler.sh";

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
}

/// Read hook status for a given PTY session.
/// Staleness rules differ by status:
/// - "running" / "waiting_permission": stale after 30s (guard against missed Stop events)
/// - "idle" / "exited": valid for 5 minutes (a stopped session won't restart spontaneously)
pub fn read_hook_status(pty_id: u32) -> Option<HookStatus> {
    let path = hooks_dir().join(format!("{}.json", pty_id));
    let content = fs::read_to_string(&path).ok()?;
    let status: HookStatus = serde_json::from_str(&content).ok()?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let age = now - status.ts;

    let max_age = match status.status.as_str() {
        "idle" | "exited" => 300,  // 5 minutes: idle sessions stay idle until new prompt
        _ => 30,                   // 30s for running/waiting_permission
    };

    if age > max_age {
        return None;
    }

    Some(status)
}

/// Install Sessonix hooks into Claude's settings.json.
/// Preserves existing hooks from other tools.
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

    // Read existing settings or create new
    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .map_err(|e| format!("Failed to read settings.json: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings.json: {}", e))?
    } else {
        serde_json::json!({})
    };

    // Check if already installed
    if is_installed(&settings) {
        return Ok(false);
    }

    // Ensure hooks object exists
    if settings.get("hooks").is_none() {
        settings["hooks"] = serde_json::json!({});
    }

    let hooks = settings["hooks"].as_object_mut()
        .ok_or_else(|| "hooks is not an object".to_string())?;

    // Events to hook into
    let events = [
        ("SessionStart", true),
        ("UserPromptSubmit", true),
        ("Stop", true),
        ("PermissionRequest", true),
        ("SessionEnd", true),
        ("PreCompact", false), // sync for PreCompact
    ];

    for (event, is_async) in events {
        let hook_entry = if is_async {
            serde_json::json!({
                "type": "command",
                "command": HOOK_COMMAND,
                "async": true
            })
        } else {
            serde_json::json!({
                "type": "command",
                "command": HOOK_COMMAND
            })
        };

        let hook_config = serde_json::json!({
            "hooks": [hook_entry]
        });

        // Append to existing array or create new
        if let Some(arr) = hooks.get_mut(event).and_then(|v| v.as_array_mut()) {
            // Check if our hook is already there
            let already = arr.iter().any(|entry| {
                entry.get("hooks")
                    .and_then(|h| h.as_array())
                    .is_some_and(|hooks_arr| {
                        hooks_arr.iter().any(|h| {
                            h.get("command").and_then(|c| c.as_str()) == Some(HOOK_COMMAND)
                        })
                    })
            });
            if !already {
                arr.push(hook_config);
            }
        } else {
            hooks.insert(event.to_string(), serde_json::json!([hook_config]));
        }
    }

    // Also add Notification hook with matcher
    let notification_hook = serde_json::json!({
        "matcher": "permission_prompt|elicitation_dialog",
        "hooks": [{
            "type": "command",
            "command": HOOK_COMMAND,
            "async": true
        }]
    });

    if let Some(arr) = hooks.get_mut("Notification").and_then(|v| v.as_array_mut()) {
        let already = arr.iter().any(|entry| {
            entry.get("hooks")
                .and_then(|h| h.as_array())
                .is_some_and(|hooks_arr| {
                    hooks_arr.iter().any(|h| {
                        h.get("command").and_then(|c| c.as_str()) == Some(HOOK_COMMAND)
                    })
                })
        });
        if !already {
            arr.push(notification_hook);
        }
    } else {
        hooks.insert("Notification".to_string(), serde_json::json!([notification_hook]));
    }

    // Write back
    let output = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&settings_path, output)
        .map_err(|e| format!("Failed to write settings.json: {}", e))?;

    // Ensure hook handler script is installed
    install_handler_script()?;

    Ok(true)
}

/// Check if Sessonix hooks are already installed.
///
/// Windows: always `false` — `install_hooks` is a no-op there.
#[cfg(windows)]
pub fn check_installed() -> bool {
    false
}

#[cfg(unix)]
pub fn check_installed() -> bool {
    let settings_path = match claude_settings_path() {
        Some(p) => p,
        None => return false,
    };

    let content = match fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let settings: serde_json::Value = match serde_json::from_str(&content) {
        Ok(s) => s,
        Err(_) => return false,
    };

    is_installed(&settings)
}

#[cfg(unix)]
fn is_installed(settings: &serde_json::Value) -> bool {
    settings.get("hooks")
        .and_then(|h| h.as_object())
        .is_some_and(|hooks| {
            hooks.values().any(|arr| {
                arr.as_array().is_some_and(|entries| {
                    entries.iter().any(|entry| {
                        entry.get("hooks")
                            .and_then(|h| h.as_array())
                            .is_some_and(|hooks_arr| {
                                hooks_arr.iter().any(|h| {
                                    h.get("command").and_then(|c| c.as_str()) == Some(HOOK_COMMAND)
                                })
                            })
                    })
                })
            })
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
    let script = include_str!("../resources/hook-handler.sh");
    fs::write(&script_path, script)
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
