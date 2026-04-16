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
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
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

            // Shell prompt detection: line ends with common prompt characters
            // Covers bash ($), zsh (%), root (#), and PS2/custom (>)
            // Allow optional trailing space after the prompt char
            let prompt_chars = ['$', '%', '#', '>'];
            let ends_with_prompt = trimmed.ends_with(|c: char| prompt_chars.contains(&c))
                || (trimmed.len() >= 2
                    && trimmed.ends_with(' ')
                    && trimmed[..trimmed.len() - 1]
                        .ends_with(|c: char| prompt_chars.contains(&c)));

            if ends_with_prompt {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_bash_prompt() {
        let adapter = ShellAdapter;
        let lines = vec!["user@host:~/project$ ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_detect_bash_prompt_no_trailing_space() {
        let adapter = ShellAdapter;
        let lines = vec!["user@host:~/project$".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_detect_zsh_prompt() {
        let adapter = ShellAdapter;
        let lines = vec!["user@host ~/project % ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_detect_root_prompt() {
        let adapter = ShellAdapter;
        let lines = vec!["root@host:/# ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

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
    fn test_detect_idle_after_command() {
        let adapter = ShellAdapter;
        let lines = vec![
            "PASS src/test.ts".to_string(),
            "user@host:~$ ".to_string(),
        ];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_empty_lines_skipped() {
        let adapter = ShellAdapter;
        let lines = vec![
            "user@host:~$ ".to_string(),
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

    #[test]
    fn test_ansi_wrapped_prompt() {
        let adapter = ShellAdapter;
        let lines = vec!["\x1b[32muser@host\x1b[0m:\x1b[34m~\x1b[0m$ ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_simple_dollar_prompt() {
        let adapter = ShellAdapter;
        let lines = vec!["$ ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }

    #[test]
    fn test_ps2_continuation_prompt() {
        let adapter = ShellAdapter;
        let lines = vec!["> ".to_string()];
        let status = adapter.extract_status(&lines);
        assert_eq!(status.state, SessionStatus::Idle);
    }
}
