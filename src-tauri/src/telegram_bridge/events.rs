//! Event detection + notification formatting for the Telegram bridge.
//!
//! - `SessionSnapshot` captures enough state per session to detect rising-edge
//!   events (Idle transition, permission-prompt appearance).
//! - `format_*` functions turn a detected event into a `Notification` —
//!   either an inline text message, or a text preview plus a `.md` attachment
//!   when the agent's last response is too long for a single Telegram message.

use crate::adapters::{strip_ansi, AgentAdapter};
use crate::telegram_bridge::api::{TELEGRAM_CAPTION_LIMIT, TELEGRAM_TEXT_LIMIT};
use crate::telegram_bridge::markdown::{escape_html, to_telegram_html};
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
/// `.md` attachment. `html` flags whether `text` contains Telegram HTML
/// markup that requires `parse_mode=HTML` on send.
#[derive(Debug)]
pub struct Notification {
    pub text: String,
    pub attachment: Option<Attachment>,
    pub html: bool,
}

#[derive(Debug)]
pub struct Attachment {
    pub filename: String,
    pub contents: Vec<u8>,
}

/// Inspect the latest scrollback against a previous snapshot and decide which
/// events (if any) to emit. Returns the updated snapshot alongside the events.
///
/// `current_status` and `permission` are supplied by the caller so multi-layer
/// detectors (hooks, JSONL, adapter) can override the default adapter-only
/// path. The scrollback + adapter are still used to extract the response body
/// that rides along with an `Idle` event.
///
/// When the agent is in a permission-wait state, `Idle` is suppressed: the
/// user needs to see the Permission notification, not a less-informative
/// "session idle" that rings for the same transition.
pub fn detect_events(
    prev: &SessionSnapshot,
    current_status: SessionStatus,
    permission: bool,
    scrollback: &[String],
    adapter: &dyn AgentAdapter,
) -> (SessionSnapshot, Vec<SessionEvent>) {
    let mut events = Vec::new();

    // Rising edge: not-Idle → Idle. Error/Exited states don't count.
    let entered_idle =
        prev.last_status != SessionStatus::Idle && current_status == SessionStatus::Idle;

    let entered_permission = permission && !prev.permission_pending;

    if entered_permission {
        events.push(SessionEvent::Permission);
    } else if entered_idle {
        let response = adapter
            .extract_last_response(scrollback)
            .unwrap_or_else(|| fallback_tail_response(scrollback));
        events.push(SessionEvent::Idle { response });
    }

    let next = SessionSnapshot {
        last_status: current_status,
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
///
/// `response` is the raw markdown the agent produced (e.g. Claude's assistant
/// text). It's converted to Telegram HTML so code blocks / bold / headers
/// render natively; without this, users saw literal ` ``` ` fences and `##`
/// markers in Telegram.
pub fn format_idle(session_label: &str, agent_type: &str, response: &str) -> Notification {
    let header = format!(
        "🟢 <b>{}</b> · <code>{}</code> — idle",
        escape_html(display_agent(agent_type)),
        escape_html(session_label)
    );
    let response_trimmed = response.trim();

    // Preview-vs-attachment split is decided on the RAW markdown length: HTML
    // tags don't count as "content", so we truncate sanely even when a reply
    // has lots of code-fence markup.
    let raw_chars = response_trimmed.chars().count();
    if raw_chars == 0 {
        return Notification {
            text: header,
            attachment: None,
            html: true,
        };
    }

    if raw_chars <= INLINE_PREVIEW_LIMIT {
        let body_html = to_telegram_html(response_trimmed);
        let composed = format!("{header}\n\n{body_html}");
        // HTML expansion may push a near-limit raw body over 4096 chars.
        // When that happens, fall back to the attachment path so the reader
        // still sees the whole thing.
        if composed.chars().count() <= TELEGRAM_TEXT_LIMIT {
            return Notification {
                text: composed,
                attachment: None,
                html: true,
            };
        }
    }

    // Oversize: header-only caption + full markdown body as an attachment.
    // Telegram captions are capped at 1024 chars — strictly smaller than the
    // message limit — so we deliberately drop the inline HTML preview here
    // and let the `response.md` body carry the full response.
    let caption = format!(
        "{header}\n\n<i>full response attached ({raw_chars} chars)</i>"
    );
    Notification {
        text: clamp_html_to_caption_limit(&caption),
        attachment: Some(Attachment {
            filename: "response.md".to_string(),
            contents: response_trimmed.as_bytes().to_vec(),
        }),
        html: true,
    }
}

pub fn format_permission(session_label: &str, agent_type: &str) -> Notification {
    let text = format!(
        "🟡 <b>{}</b> · <code>{}</code> — waiting for permission\n\nReply <b>y</b> / <b>yes</b> to allow, <b>n</b> / <b>no</b> to deny.",
        escape_html(display_agent(agent_type)),
        escape_html(session_label)
    );
    Notification {
        text,
        attachment: None,
        html: true,
    }
}

pub fn format_exit(session_label: &str, agent_type: &str, code: Option<i32>) -> Notification {
    let suffix = match code {
        Some(0) => "exited cleanly".to_string(),
        Some(c) => format!("exited with code {c}"),
        None => "exited".to_string(),
    };
    Notification {
        text: format!(
            "⚪ <b>{}</b> · <code>{}</code> — {suffix}",
            escape_html(display_agent(agent_type)),
            escape_html(session_label)
        ),
        attachment: None,
        html: true,
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

/// Clamp the `sendDocument` caption to Telegram's 1024-char limit. Only
/// pathological label lengths should trip this; we still clamp defensively
/// so the whole notification isn't rejected with `caption is too long`.
/// Chars, not bytes, so multibyte scripts don't split mid-codepoint.
fn clamp_html_to_caption_limit(text: &str) -> String {
    if text.chars().count() <= TELEGRAM_CAPTION_LIMIT {
        text.to_string()
    } else {
        text.chars()
            .take(TELEGRAM_CAPTION_LIMIT - 3)
            .collect::<String>()
            + "..."
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
        let (_, events) = detect_events(&prev, SessionStatus::Idle, false, &[], &adapter);
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
        let (_, events) = detect_events(&prev, SessionStatus::Idle, false, &[], &adapter);
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
        let (snap, events) = detect_events(&prev, SessionStatus::Running, true, &[], &adapter);
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
        let (_, events) = detect_events(&prev, SessionStatus::Running, true, &[], &adapter);
        assert!(!events.iter().any(|e| matches!(e, SessionEvent::Permission)));
    }

    #[test]
    fn permission_suppresses_idle() {
        // Agent both went idle AND is waiting on permission — only Permission fires.
        let prev = SessionSnapshot::default();
        let adapter = StubAdapter {
            status: SessionStatus::Idle,
            permission: true,
            response: Some("hello".to_string()),
        };
        let (_, events) = detect_events(&prev, SessionStatus::Idle, true, &[], &adapter);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SessionEvent::Permission));
    }

    #[test]
    fn idle_uses_adapter_response() {
        let prev = SessionSnapshot::default();
        let adapter = StubAdapter {
            status: SessionStatus::Idle,
            permission: false,
            response: Some("hello".to_string()),
        };
        let (_, events) = detect_events(&prev, SessionStatus::Idle, false, &[], &adapter);
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
        let (_, events) = detect_events(&prev, SessionStatus::Idle, false, &scrollback, &adapter);
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
        assert!(notif.html);
        assert!(notif.text.contains("main/feat"));
        assert!(notif.text.contains("small response"));
    }

    #[test]
    fn idle_converts_markdown_to_html() {
        let notif = format_idle("feat", "claude", "Use **bold** and `code`.");
        assert!(notif.text.contains("<b>bold</b>"));
        assert!(notif.text.contains("<code>code</code>"));
        assert!(notif.html);
    }

    #[test]
    fn idle_renders_fenced_code_block() {
        let md = "Here:\n```rust\nfn x() {}\n```";
        let notif = format_idle("feat", "claude", md);
        assert!(notif.text.contains("<pre><code class=\"language-rust\">"));
        assert!(notif.text.contains("fn x()"));
    }

    #[test]
    fn long_response_produces_attachment_and_preview() {
        let long: String = "x".repeat(8000);
        let notif = format_idle("main", "claude", &long);
        assert!(notif.attachment.is_some());
        // Caption fits the 1024-char sendDocument limit, not the 4096-char
        // message limit — otherwise Telegram rejects the whole notification.
        assert!(
            notif.text.chars().count() <= TELEGRAM_CAPTION_LIMIT,
            "caption was {} chars, expected <= {}",
            notif.text.chars().count(),
            TELEGRAM_CAPTION_LIMIT
        );
        // Inline preview body must NOT ride along — the file carries it.
        assert!(!notif.text.contains(&"x".repeat(100)));
        let attach = notif.attachment.unwrap();
        assert_eq!(attach.filename, "response.md");
        assert_eq!(attach.contents.len(), 8000);
    }

    #[test]
    fn attachment_caption_survives_oversize_label() {
        // Pathological label that would blow past 1024 after HTML escaping —
        // clamp should keep the caption valid rather than trip the API.
        let label: String = "a".repeat(2000);
        let long: String = "x".repeat(8000);
        let notif = format_idle(&label, "claude", &long);
        assert!(notif.text.chars().count() <= TELEGRAM_CAPTION_LIMIT);
    }

    #[test]
    fn permission_message_shape() {
        let notif = format_permission("main", "claude");
        assert!(notif.text.contains("waiting for permission"));
        assert!(notif.text.contains("<b>y</b>"));
        assert!(notif.attachment.is_none());
        assert!(notif.html);
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
        // Build a caption that exceeds the 1024-char limit in chars.
        let big: String = "Привет ".repeat(200); // 7 chars × 200 = 1400 chars
        let clamped = clamp_html_to_caption_limit(&big);
        assert!(clamped.chars().count() <= TELEGRAM_CAPTION_LIMIT);
        // And no UTF-8 split.
        assert!(clamped.is_char_boundary(clamped.len()));
    }

    #[test]
    fn labels_with_html_metachars_are_escaped() {
        // A task name with < or & must not break HTML parsing downstream.
        let notif = format_idle("proj/<bad>&task", "claude", "ok");
        assert!(notif.text.contains("&lt;bad&gt;&amp;task"));
        assert!(!notif.text.contains("<bad>"));
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
