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

    fn extract_status(&self, _last_lines: &[String]) -> AgentStatus {
        // Stub — implemented in Phase 2
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
}
