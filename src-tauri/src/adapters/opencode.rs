use crate::types::{AgentStatus, SessionStatus};
use super::{AgentAdapter, LaunchConfig};
use std::collections::HashMap;

pub struct OpenCodeAdapter;

impl AgentAdapter for OpenCodeAdapter {
    fn name(&self) -> &str {
        "OpenCode"
    }

    fn agent_type(&self) -> &str {
        "opencode"
    }

    fn build_command(
        &self,
        config: &LaunchConfig,
    ) -> (String, Vec<String>, HashMap<String, String>) {
        // Always start with the `run --quiet` subcommand for headless, PTY-friendly
        // output (no Bubble Tea TUI, no spinner).
        let mut args = vec!["run".to_string(), "--quiet".to_string()];

        // If extra_args starts with a resume flag (`--session` or `--continue`),
        // splice it in before the prompt so it reaches `opencode run` in the
        // expected order. Otherwise prompt comes first, then extra_args.
        let has_resume_flag = config
            .extra_args
            .first()
            .is_some_and(|a| matches!(a.as_str(), "--session" | "--continue"));

        if has_resume_flag {
            args.extend(config.extra_args.clone());
            if let Some(ref prompt) = config.prompt {
                args.push(prompt.clone());
            }
        } else {
            if let Some(ref prompt) = config.prompt {
                args.push(prompt.clone());
            }
            args.extend(config.extra_args.clone());
        }

        ("opencode".to_string(), args, HashMap::new())
    }

    fn extract_status(&self, last_lines: &[String]) -> AgentStatus {
        // Walk lines in reverse to find the most recent status indicator.
        //
        // OpenCode `run --quiet` emits tool-call markers with Unicode prefixes:
        //   →  read  (U+2192)
        //   ←  edit  (U+2190)
        //   $  bash  (ASCII; at START of line — distinct from EOL shell prompt)
        //   ✱  grep  (U+2731)
        for line in last_lines.iter().rev() {
            let stripped = super::strip_ansi(line);
            let trimmed = stripped.trim();

            if trimmed.is_empty() {
                continue;
            }

            // Error takes priority — catch it before idle heuristics so an
            // "Error: ..." line isn't masked by a trailing prompt character.
            if trimmed.contains("error") || trimmed.contains("Error") {
                return AgentStatus {
                    state: SessionStatus::Error,
                    status_line: super::truncate(trimmed, 80),
                };
            }

            // Idle: prompt character at END of line (shell-style, awaiting input).
            // Must be checked BEFORE the bash-tool pattern (`$ cmd` at start).
            if trimmed.ends_with("$") || trimmed.ends_with(">") {
                return AgentStatus {
                    state: SessionStatus::Idle,
                    status_line: "Waiting for input".to_string(),
                };
            }

            // Tool-call markers at START of line.
            if trimmed.starts_with('→') {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: "Reading".to_string(),
                };
            }

            if trimmed.starts_with('←') {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: "Editing".to_string(),
                };
            }

            if trimmed.starts_with("$ ") {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: "Running command".to_string(),
                };
            }

            if trimmed.starts_with('✱') {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: "Searching".to_string(),
                };
            }
        }

        AgentStatus {
            state: SessionStatus::Running,
            status_line: String::new(),
        }
    }

    fn cost_command(&self) -> Option<&str> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_command_new_session_with_prompt() {
        let adapter = OpenCodeAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: Some("fix the auth bug".to_string()),
            extra_args: vec![],
        };
        let (cmd, args, env) = adapter.build_command(&config);
        assert_eq!(cmd, "opencode");
        assert_eq!(args, vec!["run", "--quiet", "fix the auth bug"]);
        assert!(env.is_empty());
    }

    #[test]
    fn test_build_command_new_session_no_prompt() {
        let adapter = OpenCodeAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: None,
            extra_args: vec![],
        };
        let (cmd, args, _env) = adapter.build_command(&config);
        assert_eq!(cmd, "opencode");
        assert_eq!(args, vec!["run", "--quiet"]);
    }

    #[test]
    fn test_build_command_resume_by_session_id() {
        let adapter = OpenCodeAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: Some("continue the work".to_string()),
            extra_args: vec![
                "--session".to_string(),
                "ses_3cf7dd8d4ffeUPfENpVxfFojZ2".to_string(),
            ],
        };
        let (cmd, args, _env) = adapter.build_command(&config);
        assert_eq!(cmd, "opencode");
        assert_eq!(
            args,
            vec![
                "run",
                "--quiet",
                "--session",
                "ses_3cf7dd8d4ffeUPfENpVxfFojZ2",
                "continue the work",
            ]
        );
    }

    #[test]
    fn test_build_command_continue() {
        let adapter = OpenCodeAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: Some("next task".to_string()),
            extra_args: vec!["--continue".to_string()],
        };
        let (cmd, args, _env) = adapter.build_command(&config);
        assert_eq!(cmd, "opencode");
        assert_eq!(args, vec!["run", "--quiet", "--continue", "next task"]);
    }

    #[test]
    fn test_build_command_passes_extra_args() {
        let adapter = OpenCodeAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: Some("refactor".to_string()),
            extra_args: vec![
                "--model".to_string(),
                "anthropic/claude-sonnet-4-5".to_string(),
                "-d".to_string(),
            ],
        };
        let (cmd, args, _env) = adapter.build_command(&config);
        assert_eq!(cmd, "opencode");
        assert_eq!(
            args,
            vec![
                "run",
                "--quiet",
                "refactor",
                "--model",
                "anthropic/claude-sonnet-4-5",
                "-d",
            ]
        );
    }

    #[test]
    fn test_build_command_command_is_opencode() {
        let adapter = OpenCodeAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: None,
            extra_args: vec![],
        };
        let (cmd, _args, env) = adapter.build_command(&config);
        assert_eq!(cmd, "opencode");
        assert!(env.is_empty());
    }

    #[test]
    fn test_name_and_agent_type() {
        let adapter = OpenCodeAdapter;
        assert_eq!(adapter.name(), "OpenCode");
        assert_eq!(adapter.agent_type(), "opencode");
        assert_eq!(adapter.cost_command(), None);
    }

    // ─── extract_status ──────────────────────────────────────────────

    #[test]
    fn test_extract_status_reading() {
        let adapter = OpenCodeAdapter;
        let lines = vec!["→ src/main.rs".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(
            status.status_line.contains("Reading"),
            "expected status_line to mention 'Reading', got {:?}",
            status.status_line
        );
    }

    #[test]
    fn test_extract_status_editing() {
        let adapter = OpenCodeAdapter;
        let lines = vec!["← src/main.rs".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(
            status.status_line.contains("Editing"),
            "expected status_line to mention 'Editing', got {:?}",
            status.status_line
        );
    }

    #[test]
    fn test_extract_status_running_command() {
        let adapter = OpenCodeAdapter;
        let lines = vec!["$ cargo check".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(
            status.status_line.contains("Running command"),
            "expected status_line to mention 'Running command', got {:?}",
            status.status_line
        );
    }

    #[test]
    fn test_extract_status_searching() {
        let adapter = OpenCodeAdapter;
        let lines = vec!["✱ pattern (3 matches)".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(
            status.status_line.contains("Searching"),
            "expected status_line to mention 'Searching', got {:?}",
            status.status_line
        );
    }

    #[test]
    fn test_extract_status_idle_shell_prompt() {
        let adapter = OpenCodeAdapter;
        // `> ` at end of line indicates an idle shell prompt awaiting input,
        // as opposed to a `>` at the start which is a quoted-block marker.
        let lines = vec!["> ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_extract_status_idle_dollar_prompt() {
        let adapter = OpenCodeAdapter;
        // Bare `$ ` shell-style prompt at EOL — idle.
        let lines = vec!["user@host $ ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_extract_status_error() {
        let adapter = OpenCodeAdapter;
        let lines = vec!["Error: API rate limit exceeded".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Error);
        assert!(!status.status_line.is_empty());
    }

    #[test]
    fn test_extract_status_strips_ansi() {
        let adapter = OpenCodeAdapter;
        // ANSI-wrapped "→ file.rs" — must still be detected as Reading.
        let lines = vec!["\x1b[32m→ src/lib.rs\x1b[0m".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Reading"));
    }

    #[test]
    fn test_extract_status_default_running() {
        let adapter = OpenCodeAdapter;
        let lines = vec!["some unrecognized output".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert_eq!(status.status_line, "");
    }

    #[test]
    fn test_extract_status_empty_input() {
        let adapter = OpenCodeAdapter;
        let lines: Vec<String> = vec![];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert_eq!(status.status_line, "");
    }

    #[test]
    fn test_extract_status_skips_empty_lines() {
        let adapter = OpenCodeAdapter;
        let lines = vec![
            "→ file.rs".to_string(),
            "".to_string(),
            "   ".to_string(),
        ];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Reading"));
    }

    #[test]
    fn test_extract_status_truncates_long_error() {
        let adapter = OpenCodeAdapter;
        let long_msg = format!("Error: {}", "x".repeat(200));
        let lines = vec![long_msg];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Error);
        assert!(
            status.status_line.chars().count() <= 80,
            "expected status_line ≤80 chars, got {}",
            status.status_line.chars().count()
        );
    }
}
