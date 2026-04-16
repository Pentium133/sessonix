use crate::types::{AgentStatus, SessionStatus};
use super::{AgentAdapter, LaunchConfig};
use std::collections::HashMap;

pub struct ClaudeAdapter;

impl AgentAdapter for ClaudeAdapter {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn agent_type(&self) -> &str {
        "claude"
    }

    fn build_command(
        &self,
        config: &LaunchConfig,
    ) -> (String, Vec<String>, HashMap<String, String>) {
        let mut args = Vec::new();

        // If prompt provided, pass it with --dangerously-skip-permissions
        if let Some(ref prompt) = config.prompt {
            args.push("--dangerously-skip-permissions".to_string());
            args.push("-p".to_string());
            args.push(prompt.clone());
        }

        args.extend(config.extra_args.clone());

        let env = HashMap::new();
        ("claude".to_string(), args, env)
    }

    fn extract_status(&self, last_lines: &[String]) -> AgentStatus {
        // Walk lines in reverse to find the most recent status indicator
        for line in last_lines.iter().rev() {
            let stripped = super::strip_ansi(line);
            let trimmed = stripped.trim();

            if trimmed.is_empty() {
                continue;
            }

            // Claude Code patterns
            if trimmed.contains("Thinking...") || trimmed.contains("thinking") {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: "Thinking...".to_string(),
                };
            }

            if trimmed.contains("Reading") && trimmed.contains("file") {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: trimmed.to_string(),
                };
            }

            if trimmed.contains("Writing") || trimmed.contains("Editing") {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: trimmed.to_string(),
                };
            }

            if trimmed.contains("Running") && trimmed.contains("command") {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: trimmed.to_string(),
                };
            }

            if trimmed.starts_with("$") || trimmed.starts_with(">") {
                return AgentStatus {
                    state: SessionStatus::Idle,
                    status_line: "Waiting for input".to_string(),
                };
            }

            if trimmed.contains("error") || trimmed.contains("Error") {
                return AgentStatus {
                    state: SessionStatus::Error,
                    status_line: super::truncate(trimmed, 80),
                };
            }
        }

        AgentStatus {
            state: SessionStatus::Running,
            status_line: String::new(),
        }
    }

    fn cost_command(&self) -> Option<&str> {
        Some("/cost")
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_command_no_prompt() {
        let adapter = ClaudeAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: None,
            extra_args: vec![],
        };
        let (cmd, args, _env) = adapter.build_command(&config);
        assert_eq!(cmd, "claude");
        assert!(args.is_empty());
    }

    #[test]
    fn test_build_command_with_prompt() {
        let adapter = ClaudeAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: Some("Fix the bug".to_string()),
            extra_args: vec![],
        };
        let (cmd, args, _env) = adapter.build_command(&config);
        assert_eq!(cmd, "claude");
        assert_eq!(args, vec!["--dangerously-skip-permissions", "-p", "Fix the bug"]);
    }

    #[test]
    fn test_extract_status_thinking() {
        let adapter = ClaudeAdapter;
        let lines = vec!["Thinking...".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert_eq!(status.status_line, "Thinking...");
    }

    #[test]
    fn test_extract_status_idle() {
        let adapter = ClaudeAdapter;
        let lines = vec!["$ ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_extract_status_error() {
        let adapter = ClaudeAdapter;
        let lines = vec!["Error: connection refused".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Error);
    }

    #[test]
    fn test_extract_status_ansi_wrapped() {
        let adapter = ClaudeAdapter;
        let lines = vec!["\x1b[33mThinking...\x1b[0m".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert_eq!(status.status_line, "Thinking...");
    }

    #[test]
    fn test_extract_status_writing() {
        let adapter = ClaudeAdapter;
        let lines = vec!["Writing src/main.rs".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Writing"));
    }

    #[test]
    fn test_extract_status_reading_file() {
        let adapter = ClaudeAdapter;
        let lines = vec!["Reading 3 files...".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Reading"));
    }

    #[test]
    fn test_extract_status_empty_lines_skipped() {
        let adapter = ClaudeAdapter;
        let lines = vec!["".to_string(), "  ".to_string(), "$ ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_extract_status_running_command() {
        let adapter = ClaudeAdapter;
        let lines = vec!["Running command: npm test".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Running"));
    }

    #[test]
    fn test_extract_status_empty_input() {
        let adapter = ClaudeAdapter;
        let lines: Vec<String> = vec![];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert_eq!(status.status_line, "");
    }

    #[test]
    fn test_build_command_with_extra_args() {
        let adapter = ClaudeAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: None,
            extra_args: vec!["--session-id".to_string(), "abc-123".to_string()],
        };
        let (cmd, args, _) = adapter.build_command(&config);
        assert_eq!(cmd, "claude");
        assert_eq!(args, vec!["--session-id", "abc-123"]);
    }

    #[test]
    fn test_cost_command() {
        let adapter = ClaudeAdapter;
        assert_eq!(adapter.cost_command(), Some("/cost"));
    }

}
