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
        // Bare `opencode` launches the interactive TUI (the `[default]` command).
        // `opencode run` is batch-only and exits after one message — unusable
        // for our PTY-attached session. Prompt must be passed via `--prompt`,
        // because a positional arg is interpreted as the `project` path.
        //
        // Resume flags (`--session` / `--continue` / `--fork`) come first so
        // flag+value stays contiguous. Only long-form flags are matched — the
        // short forms (`-s` / `-c`) are intentionally NOT detected here:
        // `extract_opencode_resume_id` also only parses `--session`, and the
        // frontend only ever emits long forms. Matching short forms here
        // would create drift (e.g. `-s ses_x` in Extra Args would route as
        // resume but never get stored as the session ID).
        let mut args: Vec<String> = Vec::new();

        let has_resume_flag = config
            .extra_args
            .iter()
            .any(|a| matches!(a.as_str(), "--session" | "--continue" | "--fork"));

        if has_resume_flag {
            args.extend(config.extra_args.clone());
        }
        if let Some(ref prompt) = config.prompt {
            args.push("--prompt".to_string());
            args.push(prompt.clone());
        }
        if !has_resume_flag {
            args.extend(config.extra_args.clone());
        }

        ("opencode".to_string(), args, HashMap::new())
    }

    fn extract_status(&self, last_lines: &[String]) -> AgentStatus {
        // Walk lines in reverse to find the most recent status indicator.
        //
        // OpenCode emits tool-call markers with Unicode prefixes:
        //   →  read  (U+2192)
        //   ←  edit  (U+2190)
        //   $  bash  (ASCII; at START of line — distinct from EOL shell prompt)
        //   ✱  grep  (U+2731)
        //
        // Order: tool markers (specific prefixes) → error (substring) → idle
        // (suffix). This prevents a filename-containing "error" like
        // `"→ error.log"` from being mis-classified as an Error state.
        for line in last_lines.iter().rev() {
            let stripped = super::strip_ansi(line);
            let trimmed = stripped.trim();

            if trimmed.is_empty() {
                continue;
            }

            // 1. Tool-call markers at START of line — most specific.
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

            // 2. Error (substring) — only fires on lines without a tool prefix.
            if trimmed.contains("error") || trimmed.contains("Error") {
                return AgentStatus {
                    state: SessionStatus::Error,
                    status_line: super::truncate(trimmed, 80),
                };
            }

            // 3. Idle: prompt character at END of line.
            if trimmed.ends_with('$') || trimmed.ends_with('>') {
                return AgentStatus {
                    state: SessionStatus::Idle,
                    status_line: "Waiting for input".to_string(),
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
        assert_eq!(args, vec!["--prompt", "fix the auth bug"]);
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
        assert!(args.is_empty());
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
                "--session",
                "ses_3cf7dd8d4ffeUPfENpVxfFojZ2",
                "--prompt",
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
        assert_eq!(args, vec!["--continue", "--prompt", "next task"]);
    }

    #[test]
    fn test_build_command_resume_flag_not_first() {
        // Regression: frontend may pass extra flags ahead of --session, e.g.
        // `opencode --model foo --session ses_x --prompt "..."`. The adapter
        // must still group extra_args before prompt so flag+value stays
        // together (`--session` must be followed by its value).
        let adapter = OpenCodeAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: Some("continue".to_string()),
            extra_args: vec![
                "--model".to_string(),
                "foo".to_string(),
                "--session".to_string(),
                "ses_xyz".to_string(),
            ],
        };
        let (_, args, _) = adapter.build_command(&config);
        assert_eq!(
            args,
            vec![
                "--model",
                "foo",
                "--session",
                "ses_xyz",
                "--prompt",
                "continue",
            ]
        );
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
                "--prompt",
                "refactor",
                "--model",
                "anthropic/claude-sonnet-4-5",
                "-d",
            ]
        );
    }

    #[test]
    fn test_build_command_short_flags_do_not_trigger_resume_path() {
        // Regression: short forms `-s` and `-c` must NOT activate has_resume_flag.
        // extract_opencode_resume_id only parses `--session`, so matching short
        // forms here would let `-s ses_x` in Extra Args silently skip ID capture.
        // Also a future `-c <value>` (e.g. custom "config" shorthand) in a NEW
        // session must not swallow the `--prompt` that belongs to that session.
        let adapter = OpenCodeAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: Some("hi".to_string()),
            extra_args: vec!["-c".to_string(), "some-value".to_string()],
        };
        let (_, args, _) = adapter.build_command(&config);
        // Expected: prompt FIRST (new-session path), extra_args AFTER.
        assert_eq!(args, vec!["--prompt", "hi", "-c", "some-value"]);
    }

    #[test]
    fn test_build_command_fork_flag_triggers_resume_path() {
        // When UI forks (`--fork --session <id>`), extra_args carry the --fork.
        // The adapter must group them before the prompt (even an empty one).
        let adapter = OpenCodeAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: None,
            extra_args: vec![
                "--fork".to_string(),
                "--session".to_string(),
                "ses_fork_xyz".to_string(),
            ],
        };
        let (_, args, _) = adapter.build_command(&config);
        assert_eq!(args, vec!["--fork", "--session", "ses_fork_xyz"]);
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
    fn test_extract_status_reading_file_named_error() {
        // Regression for M3: a filename with "error" in its name must still be
        // classified as Reading, not Error — tool-prefix check wins.
        let adapter = OpenCodeAdapter;
        let lines = vec!["→ src/error.log".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(
            status.status_line.contains("Reading"),
            "expected Reading, got {:?}",
            status.status_line
        );
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
