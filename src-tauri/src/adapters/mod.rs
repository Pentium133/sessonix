pub mod claude;
pub mod codex;
pub mod gemini;
pub mod shell;

use crate::types::AgentStatus;
use std::collections::HashMap;

/// Strip ANSI escape sequences from a string.
///
/// Handles:
/// - CSI sequences: `ESC [` ... final byte (alpha or `~`)
/// - OSC sequences: `ESC ]` ... BEL (`\x07`) or ST (`ESC \`)
/// - All other `ESC X` two-byte sequences: both chars are dropped
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.next() {
                Some('[') => {
                    // CSI: consume parameter/intermediate bytes until final byte
                    for c2 in chars.by_ref() {
                        if c2.is_ascii_alphabetic() || c2 == '~' {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC: consume until BEL or ST (ESC \)
                    let mut prev_esc = false;
                    for c2 in chars.by_ref() {
                        if c2 == '\x07' {
                            break;
                        }
                        if prev_esc && c2 == '\\' {
                            break;
                        }
                        prev_esc = c2 == '\x1b';
                    }
                }
                // Any other ESC X: drop both the ESC and the following char (already consumed)
                Some(_) | None => {}
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Truncate a string to `max` characters, appending "..." if truncated.
/// Uses char-based iteration to avoid panicking on multi-byte UTF-8.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{truncated}...")
    }
}

/// Launch parameters used by `AgentAdapter::build_command`.
///
/// Production sessions are currently spawned with commands built on the
/// frontend, so this struct is exercised only from adapter tests — but the
/// trait is the eventual home for command construction on the Rust side.
#[allow(dead_code)]
pub struct LaunchConfig {
    pub working_dir: String,
    pub prompt: Option<String>,
    pub extra_args: Vec<String>,
}

/// Agent adapter provides agent-specific command building and status extraction.
///
/// `extract_status` is the only method invoked by production code today;
/// the remaining methods define the contract each adapter must satisfy and
/// are exercised by unit tests, so the trait is marked `allow(dead_code)`.
#[allow(dead_code)]
pub trait AgentAdapter: Send + Sync {
    fn name(&self) -> &str;
    fn agent_type(&self) -> &str;
    fn build_command(&self, config: &LaunchConfig) -> (String, Vec<String>, HashMap<String, String>);
    fn extract_status(&self, last_lines: &[String]) -> AgentStatus;
    fn cost_command(&self) -> Option<&str>;
}

pub struct AdapterRegistry {
    adapters: HashMap<String, Box<dyn AgentAdapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            adapters: HashMap::new(),
        };
        registry.register(Box::new(claude::ClaudeAdapter));
        registry.register(Box::new(codex::CodexAdapter));
        registry.register(Box::new(gemini::GeminiAdapter));
        registry.register(Box::new(shell::ShellAdapter));
        // "shell" is a frontend alias for the same adapter as "custom"
        registry.adapters.insert("shell".to_string(), Box::new(shell::ShellAdapter));
        registry
    }

    pub fn register(&mut self, adapter: Box<dyn AgentAdapter>) {
        self.adapters
            .insert(adapter.agent_type().to_string(), adapter);
    }

    pub fn get(&self, agent_type: &str) -> Option<&dyn AgentAdapter> {
        self.adapters.get(agent_type).map(|a| a.as_ref())
    }

    pub fn available_types(&self) -> Vec<&str> {
        self.adapters.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_claude() {
        let registry = AdapterRegistry::new();
        assert!(registry.get("claude").is_some());
        assert_eq!(registry.get("claude").unwrap().name(), "Claude Code");
    }

    #[test]
    fn test_registry_unknown_returns_none() {
        let registry = AdapterRegistry::new();
        assert!(registry.get("unknown").is_none());
    }

    #[test]
    fn test_registry_has_all_adapters() {
        let registry = AdapterRegistry::new();
        assert!(registry.get("claude").is_some());
        assert!(registry.get("codex").is_some());
        assert!(registry.get("gemini").is_some());
        assert!(registry.get("custom").is_some());
        assert!(registry.get("shell").is_some());
    }

    #[test]
    fn test_registry_available_types() {
        let registry = AdapterRegistry::new();
        let types = registry.available_types();
        assert_eq!(types.len(), 5);
        assert!(types.contains(&"claude"));
        assert!(types.contains(&"codex"));
        assert!(types.contains(&"gemini"));
        assert!(types.contains(&"custom"));
        assert!(types.contains(&"shell"));
    }

    #[test]
    fn test_adapter_names() {
        let registry = AdapterRegistry::new();
        assert_eq!(registry.get("claude").unwrap().name(), "Claude Code");
        assert_eq!(registry.get("codex").unwrap().name(), "Codex CLI");
        assert_eq!(registry.get("gemini").unwrap().name(), "Gemini CLI");
        assert_eq!(registry.get("custom").unwrap().name(), "Shell");
        assert_eq!(registry.get("shell").unwrap().name(), "Shell");
    }

    #[test]
    fn test_strip_ansi_basic() {
        assert_eq!(strip_ansi("\x1b[31mhello\x1b[0m"), "hello");
        assert_eq!(strip_ansi("no escapes"), "no escapes");
        assert_eq!(strip_ansi("\x1b[1;34mblue\x1b[0m"), "blue");
    }

    #[test]
    fn test_strip_ansi_256_color() {
        assert_eq!(strip_ansi("\x1b[38;5;242mtext\x1b[0m"), "text");
    }

    #[test]
    fn test_strip_ansi_osc_bel_terminated() {
        // OSC 8 hyperlink terminated by BEL: \x1b]8;;url\x07text\x1b]8;;\x07
        let input = "\x1b]8;;https://example.com\x07click here\x1b]8;;\x07";
        assert_eq!(strip_ansi(input), "click here");
    }

    #[test]
    fn test_strip_ansi_osc_st_terminated() {
        // OSC terminated by ST (ESC \)
        let input = "\x1b]0;window title\x1b\\visible text";
        assert_eq!(strip_ansi(input), "visible text");
    }

    #[test]
    fn test_strip_ansi_osc_title() {
        // Common terminal title sequence
        let input = "\x1b]2;My Terminal\x07hello";
        assert_eq!(strip_ansi(input), "hello");
    }

    #[test]
    fn test_strip_ansi_unknown_esc_sequence() {
        // ESC followed by non-[ non-] char: both ESC and the char are dropped
        let input = "\x1b=normal text";
        assert_eq!(strip_ansi(input), "normal text");
    }

    #[test]
    fn test_strip_ansi_mixed_sequences() {
        // CSI color + OSC hyperlink + plain text
        let input = "\x1b[32m\x1b]8;;http://x.com\x07link\x1b]8;;\x07\x1b[0m end";
        assert_eq!(strip_ansi(input), "link end");
    }

    #[test]
    fn test_truncate_long() {
        let long = "a".repeat(100);
        let result = truncate(&long, 20);
        assert_eq!(result.chars().count(), 20);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("short", 20), "short");
    }

    #[test]
    fn test_truncate_multibyte() {
        let s = "Привет мир, это длинная строка на русском языке";
        let result = truncate(s, 20);
        assert!(result.chars().count() <= 20);
        assert!(result.ends_with("..."));
    }
}
