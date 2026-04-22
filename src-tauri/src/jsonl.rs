//! Claude JSONL session file parser.
//! Reads session files from ~/.claude/projects/<dir-key>/ to extract
//! status (from tail) and cost (from full scan of usage blocks).

use serde::Deserialize;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// Working directory to Claude dir-key:
/// - Unix  `/Users/serg/repo`    → `-Users-serg-repo`
/// - Win   `C:\Users\serg\repo`  → `C--Users-serg-repo`
///
/// Mirrors Claude CLI's `~/.claude/projects/` naming: `/`, `\`, and `:` are
/// all replaced with `-`. Paths differing only in separators collide
/// (e.g. `/a-b/c` vs `/a/b-c`) — a known Claude-scheme limitation.
fn dir_key(working_dir: &str) -> String {
    working_dir
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c })
        .collect()
}

/// Find the most recent .jsonl file for a given working directory.
pub fn find_session_file(working_dir: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let projects_dir = home.join(".claude").join("projects").join(dir_key(working_dir));

    if !projects_dir.is_dir() {
        return None;
    }

    // Find most recent .jsonl file by modification time
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    if let Ok(entries) = fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                if let Ok(meta) = path.metadata() {
                    if let Ok(modified) = meta.modified() {
                        if best.as_ref().is_none_or(|(_, t)| modified > *t) {
                            best = Some((path, modified));
                        }
                    }
                }
            }
        }
    }

    best.map(|(p, _)| p)
}

/// Find session file by specific session ID (uuid).
pub fn find_session_file_by_id(working_dir: &str, session_id: &str) -> Option<PathBuf> {
    // Validate session_id is a UUID to prevent path traversal attacks
    if uuid::Uuid::parse_str(session_id).is_err() {
        return None;
    }
    let home = dirs::home_dir()?;
    let projects_dir = home.join(".claude").join("projects").join(dir_key(working_dir));
    let path = projects_dir.join(format!("{}.jsonl", session_id));
    if path.exists() { Some(path) } else { None }
}

// --- Status Detection ---

#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeStatus {
    Active,             // processing tool results
    Idle,               // finished, waiting for input
    WaitingPermission,  // tool_use, waiting for approval
    Error(String),      // system error
    Unknown,
}

#[derive(Deserialize)]
struct JsonlEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    subtype: Option<String>,
    message: Option<MessageBody>,
}

#[derive(Deserialize)]
struct MessageBody {
    /// Deserialized but not read — kept so serde accepts JSONL lines where
    /// this field is present without triggering deserialization churn.
    #[allow(dead_code)]
    role: Option<String>,
    stop_reason: Option<String>,
    content: Option<serde_json::Value>,
    usage: Option<UsageBlock>,
    model: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct UsageBlock {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
}

/// Read tail of JSONL file and determine Claude's current status.
pub fn detect_status(path: &PathBuf) -> ClaudeStatus {
    let (tail, truncated) = match read_tail(path, 8192) {
        Some(t) => t,
        None => return ClaudeStatus::Unknown,
    };

    let entries = parse_tail_entries(&tail, truncated);

    // Walk backwards
    for entry in entries.iter().rev() {
        let entry_type = entry.entry_type.as_deref().unwrap_or("");

        // System error
        if entry_type == "system"
            && entry.subtype.as_deref() == Some("error")
        {
            let msg = entry.message.as_ref()
                .and_then(|m| m.content.as_ref())
                .and_then(|c| c.as_str())
                .unwrap_or("unknown error")
                .to_string();
            return ClaudeStatus::Error(msg);
        }

        // Assistant message
        if entry_type == "assistant" {
            if let Some(ref msg) = entry.message {
                match msg.stop_reason.as_deref() {
                    Some("end_turn") => return ClaudeStatus::Idle,
                    Some("tool_use") => return ClaudeStatus::WaitingPermission,
                    _ => {}
                }
            }
        }

        // User message with tool_result → agent is active (processing)
        if entry_type == "user" {
            if let Some(ref msg) = entry.message {
                if has_tool_result(msg) {
                    return ClaudeStatus::Active;
                }
            }
        }
    }

    ClaudeStatus::Unknown
}

/// Extract the text of the most recent assistant turn that ended with
/// `stop_reason == "end_turn"`. Returns `None` if no such turn has been
/// written yet. Used to populate Telegram Idle notifications with a
/// meaningful response body instead of a screen-repaint dump.
pub fn extract_last_assistant_text(path: &PathBuf) -> Option<String> {
    let (tail, truncated) = read_tail(path, 16384)?;
    let entries = parse_tail_entries(&tail, truncated);
    for entry in entries.iter().rev() {
        if entry.entry_type.as_deref() != Some("assistant") {
            continue;
        }
        let Some(msg) = entry.message.as_ref() else {
            continue;
        };
        if msg.stop_reason.as_deref() != Some("end_turn") {
            continue;
        }
        let text = message_text(msg);
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    None
}

/// Concatenate the `text` fields of all text-typed content blocks in an
/// assistant message. Ignores tool_use / thinking blocks because those aren't
/// part of the conversational reply.
fn message_text(msg: &MessageBody) -> String {
    let Some(content) = msg.content.as_ref() else {
        return String::new();
    };
    let Some(arr) = content.as_array() else {
        // Older transcripts may store content as a plain string.
        return content.as_str().unwrap_or("").to_string();
    };
    let mut out = String::new();
    for block in arr {
        if block.get("type").and_then(|t| t.as_str()) != Some("text") {
            continue;
        }
        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(t);
        }
    }
    out
}

fn has_tool_result(msg: &MessageBody) -> bool {
    if let Some(ref content) = msg.content {
        if let Some(arr) = content.as_array() {
            return arr.iter().any(|block| {
                block.get("type").and_then(|t| t.as_str()) == Some("tool_result")
            });
        }
    }
    false
}

// --- Cost Calculation ---

#[derive(Debug, Clone, Default)]
pub struct SessionCost {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub model: String,
    pub cost_usd: f64,
    pub turns: u32,
}

struct ModelPricing {
    input: f64,   // per million tokens
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

fn get_pricing(model: &str) -> ModelPricing {
    // Normalize: strip [context] suffix and @date
    let normalized = model
        .split('[').next().unwrap_or(model)
        .split('@').next().unwrap_or(model)
        .trim();

    match normalized {
        s if s.contains("opus") => ModelPricing {
            input: 15.0, output: 75.0, cache_read: 1.50, cache_write: 18.75,
        },
        s if s.contains("haiku") => ModelPricing {
            input: 0.80, output: 4.0, cache_read: 0.08, cache_write: 1.0,
        },
        // sonnet is default for Claude
        _ => ModelPricing {
            input: 3.0, output: 15.0, cache_read: 0.30, cache_write: 3.75,
        },
    }
}

/// Cached cost results keyed by path → (file_size, result).
use parking_lot::Mutex;
use std::collections::HashMap;

static COST_CACHE: std::sync::LazyLock<Mutex<HashMap<PathBuf, (u64, SessionCost)>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Parse JSONL file and compute total cost.
/// Caches result by file size — returns cached value if file hasn't grown.
pub fn compute_cost(path: &PathBuf) -> SessionCost {
    // Check cache: if file size unchanged, return cached result
    let file_size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    {
        let cache = COST_CACHE.lock();
        if let Some((cached_size, ref cached_cost)) = cache.get(path) {
            if *cached_size == file_size {
                return cached_cost.clone();
            }
        }
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return SessionCost::default(),
    };

    let mut result = SessionCost::default();

    for line in content.lines() {
        if line.is_empty() { continue; }

        let entry: JsonlEntry = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.entry_type.as_deref() != Some("assistant") {
            continue;
        }

        if let Some(ref msg) = entry.message {
            // Track model
            if let Some(ref model) = msg.model {
                if result.model.is_empty() || !model.is_empty() {
                    result.model = model.clone();
                }
            }

            // Accumulate tokens
            if let Some(ref usage) = msg.usage {
                result.input_tokens += usage.input_tokens.unwrap_or(0);
                result.output_tokens += usage.output_tokens.unwrap_or(0);
                result.cache_read_tokens += usage.cache_read_input_tokens.unwrap_or(0);
                result.cache_write_tokens += usage.cache_creation_input_tokens.unwrap_or(0);
            }

            // Count turns (assistant with stop_reason=end_turn)
            if msg.stop_reason.as_deref() == Some("end_turn") {
                result.turns += 1;
            }
        }
    }

    // Calculate cost
    let pricing = get_pricing(&result.model);
    result.cost_usd =
        (result.input_tokens as f64 / 1_000_000.0) * pricing.input
        + (result.output_tokens as f64 / 1_000_000.0) * pricing.output
        + (result.cache_read_tokens as f64 / 1_000_000.0) * pricing.cache_read
        + (result.cache_write_tokens as f64 / 1_000_000.0) * pricing.cache_write;

    // Cache the result (bounded to 32 entries; evict smallest file_size when full)
    {
        let mut cache = COST_CACHE.lock();
        if cache.len() >= 32 {
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, (sz, _))| *sz)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest_key);
            }
        }
        cache.insert(path.clone(), (file_size, result.clone()));
    }

    result
}

// --- Helpers ---

/// Returns (content, was_truncated)
fn read_tail(path: &PathBuf, tail_bytes: u64) -> Option<(String, bool)> {
    let mut file = fs::File::open(path).ok()?;
    let file_size = file.metadata().ok()?.len();

    if file_size == 0 {
        return None;
    }

    let truncated = file_size > tail_bytes;
    let start = if truncated { file_size - tail_bytes } else { 0 };
    file.seek(SeekFrom::Start(start)).ok()?;

    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    Some((buf, truncated))
}

fn parse_tail_entries(tail: &str, truncated: bool) -> Vec<JsonlEntry> {
    let mut entries = Vec::new();
    let mut lines = tail.lines();

    // Skip first line only if we seeked into the middle (may be partial JSON)
    if truncated {
        lines.next();
    }

    for line in lines {
        if line.is_empty() { continue; }
        if let Ok(entry) = serde_json::from_str::<JsonlEntry>(line) {
            entries.push(entry);
        }
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_dir_key() {
        assert_eq!(dir_key("/Users/serg/Developer/js/aicoder"), "-Users-serg-Developer-js-aicoder");
        assert_eq!(dir_key("/tmp/app"), "-tmp-app");
    }

    #[test]
    fn test_dir_key_windows_path() {
        assert_eq!(dir_key(r"C:\Users\serg\repo"), "C--Users-serg-repo");
        assert_eq!(dir_key(r"D:\work\aicoder"), "D--work-aicoder");
    }

    #[test]
    fn test_dir_key_windows_mixed_separators() {
        // Some tooling normalises to forward slashes mid-path.
        assert_eq!(dir_key(r"C:\Users/serg\repo"), "C--Users-serg-repo");
    }

    #[test]
    fn test_find_session_file_by_id_rejects_traversal() {
        // Path traversal attempts must return None without touching the filesystem
        assert!(find_session_file_by_id("/tmp/app", "../../etc/passwd").is_none());
        assert!(find_session_file_by_id("/tmp/app", "../secret").is_none());
        assert!(find_session_file_by_id("/tmp/app", "not-a-uuid").is_none());
        // A real UUID-shaped input should not be rejected (it may return None if file absent)
        // but it must not panic and must be treated as valid format
        let valid_uuid = "550e8400-e29b-41d4-a716-446655440000";
        // find_session_file_by_id returns None when file doesn't exist — that's fine
        let _ = find_session_file_by_id("/tmp/nonexistent", valid_uuid);
    }

    #[test]
    fn test_detect_status_idle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"Done"}}],"usage":{{"input_tokens":100,"output_tokens":50}}}}}}"#).unwrap();

        assert_eq!(detect_status(&path), ClaudeStatus::Idle);
    }

    #[test]
    fn test_extract_last_assistant_text_basic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"hi"}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"Hello world"}}]}}}}"#).unwrap();
        assert_eq!(
            extract_last_assistant_text(&path).as_deref(),
            Some("Hello world")
        );
    }

    #[test]
    fn test_extract_last_assistant_text_skips_tool_use_and_thinking() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"thinking","text":"pondering"}},{{"type":"text","text":"First reply"}},{{"type":"tool_use","id":"t","name":"Edit"}},{{"type":"text","text":"Second reply"}}]}}}}"#
        )
        .unwrap();
        let out = extract_last_assistant_text(&path).unwrap();
        assert!(out.contains("First reply"));
        assert!(out.contains("Second reply"));
        assert!(!out.contains("pondering"));
    }

    #[test]
    fn test_extract_last_assistant_text_walks_to_end_turn() {
        // Intermediate tool_use turns must be skipped — we want the most recent
        // turn that actually closed with end_turn.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"old answer"}}]}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"next"}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t","name":"Edit"}}]}}}}"#).unwrap();
        // No newer end_turn yet → we should still return the old one
        assert_eq!(
            extract_last_assistant_text(&path).as_deref(),
            Some("old answer")
        );
    }

    #[test]
    fn test_extract_last_assistant_text_returns_none_when_no_end_turn() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t","name":"Edit"}}]}}}}"#).unwrap();
        assert_eq!(extract_last_assistant_text(&path), None);
    }

    #[test]
    fn test_detect_status_waiting_permission() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t1","name":"Edit"}}],"usage":{{"input_tokens":100,"output_tokens":50}}}}}}"#).unwrap();

        assert_eq!(detect_status(&path), ClaudeStatus::WaitingPermission);
    }

    #[test]
    fn test_detect_status_active() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"tool_use","content":[],"usage":{{"input_tokens":100,"output_tokens":50}}}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":"ok"}}]}}}}"#).unwrap();

        assert_eq!(detect_status(&path), ClaudeStatus::Active);
    }

    #[test]
    fn test_detect_status_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"system","subtype":"error","message":{{"content":"context_window_exceeded"}}}}"#).unwrap();

        match detect_status(&path) {
            ClaudeStatus::Error(msg) => assert!(msg.contains("context_window")),
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_detect_status_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        fs::File::create(&path).unwrap();

        assert_eq!(detect_status(&path), ClaudeStatus::Unknown);
    }

    #[test]
    fn test_compute_cost() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let mut f = fs::File::create(&path).unwrap();

        // Two assistant turns
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"end_turn","model":"claude-sonnet-4-5","content":[],"usage":{{"input_tokens":1000,"output_tokens":500,"cache_read_input_tokens":200,"cache_creation_input_tokens":100}}}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"next question"}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"end_turn","model":"claude-sonnet-4-5","content":[],"usage":{{"input_tokens":2000,"output_tokens":800,"cache_read_input_tokens":300,"cache_creation_input_tokens":0}}}}}}"#).unwrap();

        let cost = compute_cost(&path);
        assert_eq!(cost.input_tokens, 3000);
        assert_eq!(cost.output_tokens, 1300);
        assert_eq!(cost.cache_read_tokens, 500);
        assert_eq!(cost.cache_write_tokens, 100);
        assert_eq!(cost.turns, 2);
        assert!(cost.model.contains("sonnet"));
        assert!(cost.cost_usd > 0.0);
    }

    #[test]
    fn test_compute_cost_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        fs::File::create(&path).unwrap();

        let cost = compute_cost(&path);
        assert_eq!(cost.cost_usd, 0.0);
        assert_eq!(cost.turns, 0);
    }

    #[test]
    fn test_get_pricing_opus() {
        let p = get_pricing("claude-opus-4-6");
        assert_eq!(p.input, 15.0);
    }

    #[test]
    fn test_get_pricing_normalized() {
        let p = get_pricing("claude-sonnet-4-5[1m]@20250929");
        assert_eq!(p.input, 3.0);
    }

    #[test]
    fn test_get_pricing_alias() {
        let p = get_pricing("opus");
        assert_eq!(p.input, 15.0);
    }
}
