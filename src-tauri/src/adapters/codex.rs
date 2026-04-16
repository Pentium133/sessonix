use crate::types::{AgentStatus, SessionStatus};
use super::{AgentAdapter, LaunchConfig};
use std::collections::HashMap;

pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn name(&self) -> &str {
        "Codex CLI"
    }

    fn agent_type(&self) -> &str {
        "codex"
    }

    fn build_command(
        &self,
        config: &LaunchConfig,
    ) -> (String, Vec<String>, HashMap<String, String>) {
        let mut args = Vec::new();

        // Check if extra_args starts with a subcommand (resume, fork)
        let has_subcommand = config.extra_args.first()
            .is_some_and(|a| matches!(a.as_str(), "resume" | "fork"));

        if has_subcommand {
            // Pass subcommand and its args directly: codex resume <thread_id> [prompt]
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

        let env = HashMap::new();
        ("codex".to_string(), args, env)
    }

    fn extract_status(&self, last_lines: &[String]) -> AgentStatus {
        for line in last_lines.iter().rev() {
            let stripped = super::strip_ansi(line);
            let trimmed = stripped.trim();

            if trimmed.is_empty() {
                continue;
            }

            if trimmed.contains("Thinking") || trimmed.contains("Planning") {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: "Thinking...".to_string(),
                };
            }

            if trimmed.contains("Applying") || trimmed.contains("Writing") {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: trimmed.to_string(),
                };
            }

            if trimmed.contains("Reading") {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: trimmed.to_string(),
                };
            }

            if trimmed.starts_with(">") || trimmed.ends_with("$") {
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
        Some("/stats model")
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_command() {
        let adapter = CodexAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: Some("Fix the tests".to_string()),
            extra_args: vec![],
        };
        let (cmd, args, _) = adapter.build_command(&config);
        assert_eq!(cmd, "codex");
        assert_eq!(args, vec!["Fix the tests"]);
    }

    #[test]
    fn test_extract_status_thinking() {
        let adapter = CodexAdapter;
        let lines = vec!["Planning changes...".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert_eq!(status.status_line, "Thinking...");
    }

    #[test]
    fn test_extract_status_idle() {
        let adapter = CodexAdapter;
        let lines = vec!["> ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_extract_status_error() {
        let adapter = CodexAdapter;
        let lines = vec!["Error: file not found".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Error);
    }

    #[test]
    fn test_extract_status_applying() {
        let adapter = CodexAdapter;
        let lines = vec!["Applying changes to src/main.rs".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Applying"));
    }

    #[test]
    fn test_extract_status_reading() {
        let adapter = CodexAdapter;
        let lines = vec!["Reading package.json".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Reading"));
    }

    #[test]
    fn test_cost_command() {
        let adapter = CodexAdapter;
        assert_eq!(adapter.cost_command(), Some("/stats model"));
    }

    #[test]
    fn test_build_command_resume() {
        let adapter = CodexAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: None,
            extra_args: vec!["resume".to_string(), "019d8c75-bef1-7ba2-a5ab-71615e82c39f".to_string()],
        };
        let (cmd, args, _) = adapter.build_command(&config);
        assert_eq!(cmd, "codex");
        assert_eq!(args, vec!["resume", "019d8c75-bef1-7ba2-a5ab-71615e82c39f"]);
    }

    #[test]
    fn test_build_command_resume_with_prompt() {
        let adapter = CodexAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: Some("Continue the work".to_string()),
            extra_args: vec!["resume".to_string(), "thread-id-123".to_string()],
        };
        let (cmd, args, _) = adapter.build_command(&config);
        assert_eq!(cmd, "codex");
        assert_eq!(args, vec!["resume", "thread-id-123", "Continue the work"]);
    }

    #[test]
    fn test_build_command_fork() {
        let adapter = CodexAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: None,
            extra_args: vec!["fork".to_string(), "thread-id-456".to_string()],
        };
        let (cmd, args, _) = adapter.build_command(&config);
        assert_eq!(cmd, "codex");
        assert_eq!(args, vec!["fork", "thread-id-456"]);
    }

    #[test]
    fn test_build_command_resume_last() {
        let adapter = CodexAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: None,
            extra_args: vec!["resume".to_string(), "--last".to_string()],
        };
        let (cmd, args, _) = adapter.build_command(&config);
        assert_eq!(cmd, "codex");
        assert_eq!(args, vec!["resume", "--last"]);
    }
}
