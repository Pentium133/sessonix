use crate::types::{AgentStatus, SessionStatus};
use super::{AgentAdapter, LaunchConfig};
use std::collections::HashMap;

pub struct GeminiAdapter;

impl AgentAdapter for GeminiAdapter {
    fn name(&self) -> &str {
        "Gemini CLI"
    }

    fn agent_type(&self) -> &str {
        "gemini"
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
        ("gemini".to_string(), args, env)
    }

    fn extract_status(&self, last_lines: &[String]) -> AgentStatus {
        for line in last_lines.iter().rev() {
            let stripped = super::strip_ansi(line);
            let trimmed = stripped.trim();

            if trimmed.is_empty() {
                continue;
            }

            if trimmed.contains("Thinking") || trimmed.contains("Generating") {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: "Thinking...".to_string(),
                };
            }

            if trimmed.contains("Editing") || trimmed.contains("Writing") {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: trimmed.to_string(),
                };
            }

            if trimmed.contains("Running") {
                return AgentStatus {
                    state: SessionStatus::Running,
                    status_line: trimmed.to_string(),
                };
            }

            if trimmed.starts_with(">") || trimmed.starts_with("$") {
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

    #[test]
    fn test_build_command() {
        let adapter = GeminiAdapter;
        let config = LaunchConfig {
            working_dir: "/tmp".to_string(),
            prompt: Some("Explain this code".to_string()),
            extra_args: vec![],
        };
        let (cmd, args, _) = adapter.build_command(&config);
        assert_eq!(cmd, "gemini");
        assert_eq!(args, vec!["Explain this code"]);
    }

    #[test]
    fn test_extract_status_thinking() {
        let adapter = GeminiAdapter;
        let lines = vec!["Generating response...".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert_eq!(status.status_line, "Thinking...");
    }

    #[test]
    fn test_extract_status_idle() {
        let adapter = GeminiAdapter;
        let lines = vec!["> ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_extract_status_error() {
        let adapter = GeminiAdapter;
        let lines = vec!["Error: API rate limit exceeded".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Error);
    }

    #[test]
    fn test_extract_status_editing() {
        let adapter = GeminiAdapter;
        let lines = vec!["Editing src/app.ts".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Editing"));
    }

    #[test]
    fn test_extract_status_running() {
        let adapter = GeminiAdapter;
        let lines = vec!["Running npm test".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Running);
        assert!(status.status_line.contains("Running"));
    }

    #[test]
    fn test_cost_command_none() {
        let adapter = GeminiAdapter;
        assert_eq!(adapter.cost_command(), None);
    }
}
