use crate::types::{AgentStatus, SessionStatus};
use super::{AgentAdapter, LaunchConfig};
use std::collections::HashMap;

pub struct ShellAdapter;

impl AgentAdapter for ShellAdapter {
    fn name(&self) -> &str {
        "Shell"
    }

    fn agent_type(&self) -> &str {
        "custom"
    }

    fn build_command(
        &self,
        config: &LaunchConfig,
    ) -> (String, Vec<String>, HashMap<String, String>) {
        let shell = resolve_shell();
        let args = config.extra_args.clone();
        let env = HashMap::new();
        // working_dir is handled upstream by pty_manager (cmd.cwd()).
        // prompt is intentionally ignored: shells don't accept an initial prompt via argv.
        let _ = (&config.working_dir, &config.prompt);
        (shell, args, env)
    }

    fn extract_status(&self, last_lines: &[String]) -> AgentStatus {
        for line in last_lines.iter().rev() {
            let stripped = super::strip_ansi(line);
            let trimmed = stripped.trim();

            if trimmed.is_empty() {
                continue;
            }

            if is_idle_prompt(trimmed) {
                return AgentStatus {
                    state: SessionStatus::Idle,
                    status_line: String::new(),
                };
            }

            // If the last non-empty line is not a prompt, something is running
            return AgentStatus {
                state: SessionStatus::Running,
                status_line: super::truncate(trimmed, 80),
            };
        }

        // No output yet
        AgentStatus {
            state: SessionStatus::Running,
            status_line: String::new(),
        }
    }

    fn cost_command(&self) -> Option<&str> {
        None
    }
}

// --- Shell resolution ---

#[cfg(unix)]
pub fn resolve_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
}

#[cfg(windows)]
pub fn resolve_shell() -> String {
    resolve_shell_windows(
        |name| which::which(name).is_ok(),
        std::env::var("COMSPEC").ok(),
    )
}

/// Pure resolver for the Windows shell. Takes an `is_on_path` probe and the
/// value of `%COMSPEC%` (if any) so the logic is testable on non-Windows hosts.
///
/// Priority: `pwsh.exe` → `powershell.exe` → `%COMSPEC%` → literal `cmd.exe`.
#[cfg(any(windows, test))]
fn resolve_shell_windows<F: Fn(&str) -> bool>(is_on_path: F, comspec: Option<String>) -> String {
    if is_on_path("pwsh") {
        return "pwsh.exe".to_string();
    }
    if is_on_path("powershell") {
        return "powershell.exe".to_string();
    }
    comspec.unwrap_or_else(|| "cmd.exe".to_string())
}

// --- Prompt detection ---

#[cfg(unix)]
fn is_idle_prompt(trimmed: &str) -> bool {
    is_unix_idle_prompt(trimmed)
}

#[cfg(windows)]
fn is_idle_prompt(trimmed: &str) -> bool {
    is_windows_idle_prompt(trimmed)
}

/// Unix prompt detector: line ends with `$`, `%`, `#`, or `>` (with optional trailing space).
/// Covers bash, zsh, root, and PS2/custom continuation prompts.
fn is_unix_idle_prompt(trimmed: &str) -> bool {
    if trimmed.is_empty() {
        return false;
    }
    let prompt_chars = ['$', '%', '#', '>'];
    trimmed.ends_with(|c: char| prompt_chars.contains(&c))
        || (trimmed.len() >= 2
            && trimmed.ends_with(' ')
            && trimmed[..trimmed.len() - 1]
                .ends_with(|c: char| prompt_chars.contains(&c)))
}

/// Windows prompt detector. Matches:
/// - `cmd.exe`: `C:\Users\Foo>` or `C:\Users\Foo> ` (drive-letter path + `>`)
/// - PowerShell: `PS C:\Users\Foo>` or `PS C:\Users\Foo> `
///
/// A bare trailing `>` is NOT treated as idle on Windows: redirect noise
/// (`echo foo > file`), PowerShell continuation (`>>`), and HTML-like output
/// would otherwise produce false positives. Unix-style `$ % #` are not matched;
/// those only make sense in WSL / Git Bash, which are out of scope.
#[cfg(any(windows, test))]
fn is_windows_idle_prompt(trimmed: &str) -> bool {
    use regex::Regex;
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // (PS )? drive + (\ or /) + anything + > + optional trailing space
        Regex::new(r"^(PS )?[A-Za-z]:[\\/].*>\s?$").unwrap()
    });
    re.is_match(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- Unix prompt detection (existing behaviour) ----------

    #[test]
    fn test_detect_bash_prompt() {
        assert!(is_unix_idle_prompt("user@host:~/project$ "));
    }

    #[test]
    fn test_detect_bash_prompt_no_trailing_space() {
        assert!(is_unix_idle_prompt("user@host:~/project$"));
    }

    #[test]
    fn test_detect_zsh_prompt() {
        assert!(is_unix_idle_prompt("user@host ~/project % "));
    }

    #[test]
    fn test_detect_root_prompt() {
        assert!(is_unix_idle_prompt("root@host:/# "));
    }

    #[test]
    fn test_simple_dollar_prompt() {
        assert!(is_unix_idle_prompt("$ "));
    }

    #[test]
    fn test_ps2_continuation_prompt() {
        assert!(is_unix_idle_prompt("> "));
    }

    #[test]
    fn test_unix_non_prompt_is_not_idle() {
        assert!(!is_unix_idle_prompt("PASS src/test.ts"));
        assert!(!is_unix_idle_prompt("  "));  // whitespace collapses to empty after trim, but we also early-out
    }

    // ---------- Windows shell resolution (pure) ----------

    #[test]
    fn test_resolve_shell_windows_picks_pwsh_when_available() {
        let got = resolve_shell_windows(|n| n == "pwsh", Some("C:\\Windows\\system32\\cmd.exe".into()));
        assert_eq!(got, "pwsh.exe");
    }

    #[test]
    fn test_resolve_shell_windows_falls_back_to_powershell() {
        let got = resolve_shell_windows(
            |n| n == "powershell",
            Some("C:\\Windows\\system32\\cmd.exe".into()),
        );
        assert_eq!(got, "powershell.exe");
    }

    #[test]
    fn test_resolve_shell_windows_uses_comspec_when_neither_on_path() {
        let got = resolve_shell_windows(|_| false, Some("C:\\Windows\\system32\\cmd.exe".into()));
        assert_eq!(got, "C:\\Windows\\system32\\cmd.exe");
    }

    #[test]
    fn test_resolve_shell_windows_defaults_to_cmd_exe_when_no_comspec() {
        let got = resolve_shell_windows(|_| false, None);
        assert_eq!(got, "cmd.exe");
    }

    #[test]
    fn test_resolve_shell_windows_prefers_pwsh_over_powershell() {
        let got = resolve_shell_windows(|n| n == "pwsh" || n == "powershell", None);
        assert_eq!(got, "pwsh.exe");
    }

    // ---------- Windows prompt detection (pure) ----------

    #[test]
    fn test_detect_cmd_prompt() {
        assert!(is_windows_idle_prompt("C:\\Users\\Foo>"));
        assert!(is_windows_idle_prompt("C:\\Users\\Foo> "));
        assert!(is_windows_idle_prompt("D:\\projects\\aicoder>"));
    }

    #[test]
    fn test_detect_powershell_prompt() {
        assert!(is_windows_idle_prompt("PS C:\\Users\\Foo>"));
        assert!(is_windows_idle_prompt("PS C:\\Users\\Foo> "));
        assert!(is_windows_idle_prompt("PS D:\\work>"));
    }

    #[test]
    fn test_detect_cmd_prompt_with_forward_slashes() {
        // Some shells normalise to forward slashes
        assert!(is_windows_idle_prompt("C:/Users/Foo>"));
    }

    #[test]
    fn test_windows_lone_gt_is_not_idle() {
        // PS continuation, redirect noise, HTML-like lines
        assert!(!is_windows_idle_prompt(">"));
        assert!(!is_windows_idle_prompt("> "));
        assert!(!is_windows_idle_prompt(">>"));
        assert!(!is_windows_idle_prompt("<div>"));
        assert!(!is_windows_idle_prompt("echo foo > file.txt"));
    }

    #[test]
    fn test_windows_non_path_gt_is_not_idle() {
        // Unix-style prompts must not accidentally match Windows detector
        assert!(!is_windows_idle_prompt("user@host:~/project$"));
        assert!(!is_windows_idle_prompt("root@host:/#"));
    }

    #[test]
    fn test_windows_ansi_wrapped_prompt() {
        // The dispatcher calls strip_ansi before is_idle_prompt, so the regex
        // sees a stripped string. Verify the post-strip form matches.
        let raw = "\x1b[32mPS C:\\Users\\Foo\x1b[0m>";
        let stripped = super::super::strip_ansi(raw);
        assert!(is_windows_idle_prompt(stripped.trim()));
    }

    // ---------- End-to-end extract_status via dispatch ----------

    #[test]
    fn test_detect_running_command() {
        let adapter = ShellAdapter;
        let lines = vec![
            "user@host:~$ ".to_string(),
            "npm test".to_string(),
            "PASS src/test.ts".to_string(),
        ];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("PASS"));
    }

    #[test]
    fn test_empty_lines_skipped() {
        let adapter = ShellAdapter;
        // On Unix host, the Unix prompt detector handles this.
        #[cfg(unix)]
        let lines = vec![
            "user@host:~$ ".to_string(),
            "".to_string(),
            "  ".to_string(),
        ];
        #[cfg(windows)]
        let lines = vec![
            "C:\\Users\\Foo>".to_string(),
            "".to_string(),
            "  ".to_string(),
        ];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_empty_input() {
        let adapter = ShellAdapter;
        let lines: Vec<String> = vec![];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
    }
}
