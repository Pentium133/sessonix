//! Cursor CLI (`agent`) adapter.
//!
//! Status-line heuristics are conservative — the Cursor TUI's exact status
//! strings are undocumented as of 2026-04. Patterns will be refined based on
//! real output collected during TASK-003 (end-to-end verification).

use crate::types::{AgentStatus, SessionStatus};
use super::{AgentAdapter, LaunchConfig};
use std::collections::HashMap;

pub struct CursorAdapter;

impl AgentAdapter for CursorAdapter {
    fn name(&self) -> &str {
        "Cursor Agent"
    }

    fn agent_type(&self) -> &str {
        "cursor"
    }

    fn build_command(
        &self,
        config: &LaunchConfig,
    ) -> (String, Vec<String>, HashMap<String, String>) {
        let mut args = Vec::new();

        if let Some(ref prompt) = config.prompt {
            args.push(prompt.clone());
        }

        args.extend(config.extra_args.clone());

        let env = HashMap::new();
        ("agent".to_string(), args, env)
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

            if trimmed.contains("Reading") {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: trimmed.to_string(),
                };
            }

            if trimmed.contains("Writing")
                || trimmed.contains("Editing")
                || trimmed.contains("Applying")
            {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: trimmed.to_string(),
                };
            }

            if trimmed.starts_with('>') || trimmed.starts_with('$') {
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
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- Phase 1: identity + build_command ----------

    #[test]
    fn test_name_and_agent_type() {
        let adapter = CursorAdapter;
        assert_eq!(adapter.name(), "Cursor Agent");
        assert_eq!(adapter.agent_type(), "cursor");
    }

    #[test]
    fn test_build_command_no_prompt() {
        let adapter = CursorAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: None,
            extra_args: vec![],
        };
        let (cmd, args, env) = adapter.build_command(&config);
        assert_eq!(cmd, "agent");
        assert!(args.is_empty());
        assert!(env.is_empty());
    }

    #[test]
    fn test_build_command_with_prompt() {
        let adapter = CursorAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: Some("Refactor this module".to_string()),
            extra_args: vec![],
        };
        let (cmd, args, _) = adapter.build_command(&config);
        assert_eq!(cmd, "agent");
        assert_eq!(args, vec!["Refactor this module"]);
    }

    #[test]
    fn test_build_command_with_resume() {
        let adapter = CursorAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: None,
            extra_args: vec!["--resume".to_string(), "6ffd78e9-b552-49a7-9abf-2b00327c2764".to_string()],
        };
        let (cmd, args, _) = adapter.build_command(&config);
        assert_eq!(cmd, "agent");
        assert_eq!(args, vec!["--resume", "6ffd78e9-b552-49a7-9abf-2b00327c2764"]);
    }

    #[test]
    fn test_build_command_with_continue() {
        let adapter = CursorAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: None,
            extra_args: vec!["--continue".to_string()],
        };
        let (cmd, args, _) = adapter.build_command(&config);
        assert_eq!(cmd, "agent");
        assert_eq!(args, vec!["--continue"]);
    }

    #[test]
    fn test_build_command_with_prompt_and_extra_args() {
        let adapter = CursorAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: Some("Fix the bug".to_string()),
            extra_args: vec!["--model".to_string(), "gpt-5".to_string()],
        };
        let (cmd, args, _) = adapter.build_command(&config);
        assert_eq!(cmd, "agent");
        assert_eq!(args, vec!["Fix the bug", "--model", "gpt-5"]);
    }

    #[test]
    fn test_cost_command_none() {
        let adapter = CursorAdapter;
        assert_eq!(adapter.cost_command(), None);
    }

    // ---------- Phase 2: extract_status ----------

    #[test]
    fn test_extract_status_thinking() {
        let adapter = CursorAdapter;
        let lines = vec!["Thinking...".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert_eq!(status.status_line, "Thinking...");
    }

    #[test]
    fn test_extract_status_planning() {
        let adapter = CursorAdapter;
        let lines = vec!["Planning changes...".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert_eq!(status.status_line, "Thinking...");
    }

    #[test]
    fn test_extract_status_reading() {
        let adapter = CursorAdapter;
        let lines = vec!["Reading src/main.rs".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Reading"));
    }

    #[test]
    fn test_extract_status_writing() {
        let adapter = CursorAdapter;
        let lines = vec!["Writing app.ts".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Writing"));
    }

    #[test]
    fn test_extract_status_editing() {
        let adapter = CursorAdapter;
        let lines = vec!["Editing config.json".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Editing"));
    }

    #[test]
    fn test_extract_status_applying() {
        let adapter = CursorAdapter;
        let lines = vec!["Applying patch to handler.go".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Applying"));
    }

    #[test]
    fn test_extract_status_idle_angle_bracket() {
        let adapter = CursorAdapter;
        let lines = vec!["> ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
        assert_eq!(status.status_line, "Waiting for input");
    }

    #[test]
    fn test_extract_status_idle_dollar() {
        let adapter = CursorAdapter;
        let lines = vec!["$ ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_extract_status_error() {
        let adapter = CursorAdapter;
        let lines = vec!["Error: authentication failed".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Error);
        assert!(status.status_line.contains("authentication"));
    }

    #[test]
    fn test_extract_status_ansi_wrapped() {
        let adapter = CursorAdapter;
        let lines = vec!["\x1b[33mThinking...\x1b[0m".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert_eq!(status.status_line, "Thinking...");
    }

    #[test]
    fn test_extract_status_empty_lines_skipped() {
        let adapter = CursorAdapter;
        let lines = vec!["".to_string(), "  ".to_string(), "> ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_extract_status_empty_input() {
        let adapter = CursorAdapter;
        let lines: Vec<String> = vec![];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert_eq!(status.status_line, "");
    }

    #[test]
    fn test_extract_status_walks_reverse() {
        // Newest line is last — "> " wins over the older "Thinking..."
        let adapter = CursorAdapter;
        let lines = vec!["Thinking...".to_string(), "> ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }
}
