//! Event detection + notification formatting for the Telegram bridge.
//!
//! - `SessionSnapshot` captures enough state per session to detect rising-edge
//!   events (Idle transition, permission-prompt appearance).
//! - `format_*` functions turn a detected event into a `Notification` —
//!   either an inline text message, or a text preview plus a `.md` attachment
//!   when the agent's last response is too long for a single Telegram message.

use crate::adapters::{strip_ansi, AgentAdapter};
use crate::telegram_bridge::api::TELEGRAM_TEXT_LIMIT;
use crate::types::SessionStatus;

/// Leave headroom under Telegram's 4096-char limit so the header (≤ ~200 chars)
/// plus the preview body always fit inside a single message.
pub const INLINE_PREVIEW_LIMIT: usize = 3500;

/// Per-session state the poller carries across ticks. Captures just what we
/// need to decide "did something new happen since last time".
#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    pub last_status: SessionStatus,
    pub permission_pending: bool,
}

impl Default for SessionSnapshot {
    fn default() -> Self {
        Self {
            // Treat a fresh session as Running so the first Idle emits a
            // notification (matches the observed UX: agent finished → ping).
            last_status: SessionStatus::Running,
            permission_pending: false,
        }
    }
}

/// Events the poller can surface on a single tick. `Exit` is fired from the
/// existing `pty-exit` event path (not the sweep), so the variant appears
/// unused to dead-code analysis — suppressed with `allow` since it's part
/// of the public event taxonomy.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SessionEvent {
    /// Agent transitioned from Running → Idle. Carries the extracted response.
    Idle { response: String },
    /// Agent is waiting on a yes/no permission prompt (rising edge).
    Permission,
    /// PTY process exited. `code` is the OS exit code if reported.
    Exit { code: Option<i32> },
}

/// A ready-to-send Telegram message. `attachment` is populated when the
/// response exceeded [`INLINE_PREVIEW_LIMIT`] and the full body moves to a
/// `.md` attachment.
#[derive(Debug)]
pub struct Notification {
    pub text: String,
    pub attachment: Option<Attachment>,
}

#[derive(Debug)]
pub struct Attachment {
    pub filename: String,
    pub contents: Vec<u8>,
}

/// Inspect the latest scrollback against a previous snapshot and decide which
/// events (if any) to emit. Returns the updated snapshot alongside the events.
pub fn detect_events(
    prev: &SessionSnapshot,
    scrollback: &[String],
    adapter: &dyn AgentAdapter,
) -> (SessionSnapshot, Vec<SessionEvent>) {
    let mut events = Vec::new();

    let status = adapter.extract_status(scrollback);
    let permission = adapter.detect_permission_prompt(scrollback);

    // Rising edge: not-Idle → Idle. Error/Exited states don't count.
    let entered_idle =
        prev.last_status != SessionStatus::Idle && status.state == SessionStatus::Idle;

    let entered_permission = permission && !prev.permission_pending;

    if entered_idle {
        let response = adapter
            .extract_last_response(scrollback)
            .unwrap_or_else(|| fallback_tail_response(scrollback));
        events.push(SessionEvent::Idle { response });
    }

    if entered_permission {
        events.push(SessionEvent::Permission);
    }

    let next = SessionSnapshot {
        last_status: status.state,
        permission_pending: permission,
    };
    (next, events)
}

/// Generic "hand-rolled adapter" fallback: take lines in reverse order until
/// we hit a shell-prompt-looking line, then flip back to reading-order. ANSI
/// stripped, blank lines preserved as separators.
pub fn fallback_tail_response(last_lines: &[String]) -> String {
    let mut buf: Vec<String> = Vec::new();
    for line in last_lines.iter().rev() {
        let stripped = strip_ansi(line);
        let trimmed = stripped.trim();
        // Stop when we hit a line that looks like the agent's own prompt
        // appearing AFTER some content. Without this, we'd happily include
        // the prompt character as part of the response.
        if !buf.is_empty() && (trimmed.starts_with('$') || trimmed.starts_with('>')) {
            break;
        }
        buf.push(stripped);
    }
    buf.reverse();
    buf.join("\n").trim().to_string()
}

/// Build an Idle-event notification. Oversize responses collapse to a short
/// inline preview plus a markdown attachment with the full body.
pub fn format_idle(session_label: &str, agent_type: &str, response: &str) -> Notification {
    let header = format!("🟢 {} · {} — idle", display_agent(agent_type), session_label);
    let response_trimmed = response.trim();

    if response_trimmed.chars().count() <= INLINE_PREVIEW_LIMIT {
        // One-message case.
        let text = if response_trimmed.is_empty() {
            header
        } else {
            format!("{header}\n\n{response_trimmed}")
        };
        return Notification {
            text: clamp_to_telegram_limit(&text),
            attachment: None,
        };
    }

    // Oversize: inline preview (first N chars) + full body as attachment.
    let preview: String = response_trimmed.chars().take(INLINE_PREVIEW_LIMIT).collect();
    let total = response_trimmed.chars().count();
    let text = format!(
        "{header}\n\n{preview}\n\n… ({}/{} chars — full response attached)",
        INLINE_PREVIEW_LIMIT, total
    );
    Notification {
        text: clamp_to_telegram_limit(&text),
        attachment: Some(Attachment {
            filename: "response.md".to_string(),
            contents: response_trimmed.as_bytes().to_vec(),
        }),
    }
}

pub fn format_permission(session_label: &str, agent_type: &str) -> Notification {
    let text = format!(
        "🟡 {} · {} — waiting for permission\n\nReply y / yes to allow, n / no to deny.",
        display_agent(agent_type),
        session_label
    );
    Notification {
        text,
        attachment: None,
    }
}

pub fn format_exit(session_label: &str, agent_type: &str, code: Option<i32>) -> Notification {
    let suffix = match code {
        Some(0) => "exited cleanly".to_string(),
        Some(c) => format!("exited with code {c}"),
        None => "exited".to_string(),
    };
    Notification {
        text: format!("⚪ {} · {} — {suffix}", display_agent(agent_type), session_label),
        attachment: None,
    }
}

fn display_agent(agent_type: &str) -> &'static str {
    match agent_type {
        "claude" => "Claude",
        "codex" => "Codex",
        "cursor" => "Cursor",
        "gemini" => "Gemini",
        "opencode" => "OpenCode",
        "shell" | "custom" => "Shell",
        _ => "Agent",
    }
}

/// Defensive cap: even after preview truncation, pathological inputs can push
/// the composed message above Telegram's limit. Clamp on chars, not bytes, so
/// multibyte scripts don't split mid-codepoint.
fn clamp_to_telegram_limit(text: &str) -> String {
    if text.chars().count() <= TELEGRAM_TEXT_LIMIT {
        text.to_string()
    } else {
        text.chars().take(TELEGRAM_TEXT_LIMIT - 3).collect::<String>() + "..."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubAdapter {
        status: SessionStatus,
        permission: bool,
        response: Option<String>,
    }

    impl AgentAdapter for StubAdapter {
        fn name(&self) -> &str {
            "stub"
        }
        fn agent_type(&self) -> &str {
            "stub"
        }
        fn build_command(
            &self,
            _c: &crate::adapters::LaunchConfig,
        ) -> (String, Vec<String>, std::collections::HashMap<String, String>) {
            ("".into(), vec![], Default::default())
        }
        fn extract_status(&self, _: &[String]) -> crate::types::AgentStatus {
            crate::types::AgentStatus {
                state: self.status,
                status_line: String::new(),
            }
        }
        fn cost_command(&self) -> Option<&str> {
            None
        }
        fn extract_last_response(&self, _: &[String]) -> Option<String> {
            self.response.clone()
        }
        fn detect_permission_prompt(&self, _: &[String]) -> bool {
            self.permission
        }
    }

    #[test]
    fn idle_transition_emits_one_event() {
        let prev = SessionSnapshot::default(); // Running
        let adapter = StubAdapter {
            status: SessionStatus::Idle,
            permission: false,
            response: Some("done".to_string()),
        };
        let (_, events) = detect_events(&prev, &[], &adapter);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SessionEvent::Idle { .. }));
    }

    #[test]
    fn idle_stays_idle_no_event() {
        let prev = SessionSnapshot {
            last_status: SessionStatus::Idle,
            permission_pending: false,
        };
        let adapter = StubAdapter {
            status: SessionStatus::Idle,
            permission: false,
            response: None,
        };
        let (_, events) = detect_events(&prev, &[], &adapter);
        assert!(events.is_empty());
    }

    #[test]
    fn permission_rising_edge_emits_event() {
        let prev = SessionSnapshot::default();
        let adapter = StubAdapter {
            status: SessionStatus::Running,
            permission: true,
            response: None,
        };
        let (snap, events) = detect_events(&prev, &[], &adapter);
        assert!(events.iter().any(|e| matches!(e, SessionEvent::Permission)));
        assert!(snap.permission_pending);
    }

    #[test]
    fn permission_stays_pending_no_event() {
        let prev = SessionSnapshot {
            last_status: SessionStatus::Running,
            permission_pending: true,
        };
        let adapter = StubAdapter {
            status: SessionStatus::Running,
            permission: true,
            response: None,
        };
        let (_, events) = detect_events(&prev, &[], &adapter);
        assert!(!events.iter().any(|e| matches!(e, SessionEvent::Permission)));
    }

    #[test]
    fn idle_uses_adapter_response() {
        let prev = SessionSnapshot::default();
        let adapter = StubAdapter {
            status: SessionStatus::Idle,
            permission: false,
            response: Some("hello".to_string()),
        };
        let (_, events) = detect_events(&prev, &[], &adapter);
        match &events[0] {
            SessionEvent::Idle { response } => assert_eq!(response, "hello"),
            _ => panic!("expected Idle"),
        }
    }

    #[test]
    fn idle_falls_back_to_tail_when_adapter_returns_none() {
        let prev = SessionSnapshot::default();
        let adapter = StubAdapter {
            status: SessionStatus::Idle,
            permission: false,
            response: None,
        };
        let scrollback = vec![
            "$ build".to_string(),
            "compiling".to_string(),
            "done".to_string(),
        ];
        let (_, events) = detect_events(&prev, &scrollback, &adapter);
        match &events[0] {
            SessionEvent::Idle { response } => {
                // Fallback stops at the `$ ...` prompt and keeps what came after.
                assert!(response.contains("compiling"));
                assert!(response.contains("done"));
                assert!(!response.contains("$ build"));
            }
            _ => panic!("expected Idle"),
        }
    }

    #[test]
    fn short_response_single_message_no_attachment() {
        let notif = format_idle("main/feat", "claude", "small response");
        assert!(notif.attachment.is_none());
        assert!(notif.text.contains("main/feat"));
        assert!(notif.text.contains("small response"));
    }

    #[test]
    fn long_response_produces_attachment_and_preview() {
        let long: String = "x".repeat(8000);
        let notif = format_idle("main", "claude", &long);
        assert!(notif.attachment.is_some());
        assert!(notif.text.chars().count() <= TELEGRAM_TEXT_LIMIT);
        let attach = notif.attachment.unwrap();
        assert_eq!(attach.filename, "response.md");
        assert_eq!(attach.contents.len(), 8000);
    }

    #[test]
    fn permission_message_shape() {
        let notif = format_permission("main", "claude");
        assert!(notif.text.contains("waiting for permission"));
        assert!(notif.text.contains("y / yes"));
        assert!(notif.attachment.is_none());
    }

    #[test]
    fn exit_message_reports_code() {
        let ok = format_exit("main", "claude", Some(0));
        assert!(ok.text.contains("exited cleanly"));
        let err = format_exit("main", "claude", Some(137));
        assert!(err.text.contains("code 137"));
        let unknown = format_exit("main", "claude", None);
        assert!(unknown.text.contains("exited"));
    }

    #[test]
    fn clamp_cuts_multibyte_safely() {
        // Build a string that exceeds the limit in chars.
        let big: String = "Привет ".repeat(800); // 7 chars × 800 = 5600 chars
        let clamped = clamp_to_telegram_limit(&big);
        assert!(clamped.chars().count() <= TELEGRAM_TEXT_LIMIT);
        // And no UTF-8 split.
        assert!(clamped.is_char_boundary(clamped.len()));
    }

    #[test]
    fn fallback_tail_stops_at_prompt() {
        let lines = vec![
            "$ old command".to_string(),
            "hello".to_string(),
            "world".to_string(),
        ];
        let r = fallback_tail_response(&lines);
        assert!(r.contains("hello"));
        assert!(r.contains("world"));
        assert!(!r.contains("old command"));
    }

    #[test]
    fn fallback_tail_strips_ansi() {
        let lines = vec!["\x1b[32mgreen\x1b[0m text".to_string()];
        let r = fallback_tail_response(&lines);
        assert_eq!(r, "green text");
    }

    #[test]
    fn fallback_tail_empty_input() {
        let r = fallback_tail_response(&[]);
        assert_eq!(r, "");
    }
}
