//! Telegram bridge: a self-hosted bot for remote monitoring and control of
//! Sessonix agent sessions.
//!
//! High-level design:
//! - A single worker thread drives everything: short-poll Telegram `getUpdates`
//!   for inbound messages, then sweep every opted-in session to detect
//!   `Idle` / `Permission` events and push notifications outbound.
//! - Sessions opt in per-row via the `sessions.telegram_enabled` column.
//! - State lives in `BridgeInner` behind a `Mutex`, shared between the worker
//!   and the IPC layer.
//! - Token and `owner_chat_id` persist in the generic `settings` key-value
//!   store (keys `telegram_bot_token`, `telegram_owner_chat_id`) — not in a
//!   dedicated table — to match existing patterns.

pub mod api;
pub mod events;
pub mod settings;

use crate::adapters::AdapterRegistry;
use crate::db::Db;
use crate::pty_manager::PtyManager;
use crate::telegram_bridge::api::{Message, TelegramApi};
use crate::telegram_bridge::events::{
    detect_events, format_exit, format_idle, format_permission, Notification, SessionEvent,
    SessionSnapshot,
};
use crate::types::SessionStatus;
use lru::LruCache;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Tick interval for the session sweep. Lower values ping faster but also
/// hammer the DB and Telegram API unnecessarily.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Long-poll timeout for `getUpdates`. The worker wakes at minimum every
/// `POLL_INTERVAL` so inbound is not the only wakeup signal.
const GET_UPDATES_TIMEOUT_SECS: u64 = 1;

/// Max mappings held in the reply-routing cache. Oldest entries fall out when
/// the user has left a lot of bot messages unanswered.
const PENDING_REPLIES_CAPACITY: usize = 256;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum BridgeStatus {
    /// No token configured.
    Disabled,
    /// Token set, worker is verifying or long-polling.
    Connecting,
    /// Worker is successfully polling.
    Polling,
    /// Token rejected or the connection is in persistent failure.
    Error { message: String },
}

struct BridgeInner {
    status: Mutex<BridgeStatus>,
    /// Current owner chat id. `None` until the first `/start`.
    owner_chat_id: Mutex<Option<i64>>,
    /// telegram_message_id we sent → pty_id the message referred to.
    /// Replies to messages no longer in this cache fall through to an
    /// explanation asking the user to open Sessonix.
    pending_replies: Mutex<LruCache<i64, u32>>,
    /// Monotonic offset for `getUpdates`.
    next_update_offset: Mutex<i64>,
    /// Per-session event-detection state.
    snapshots: Mutex<HashMap<u32, SessionSnapshot>>,
    /// Bumped by the main thread to ask the worker to exit (token reload,
    /// shutdown). The worker checks it at every iteration.
    shutdown: AtomicBool,
    /// Bumped on every token change so any stale worker that loses a race
    /// can tell it's been superseded. Cheap alternative to killing threads.
    generation: AtomicU64,
}

pub struct TelegramBridge {
    inner: Arc<BridgeInner>,
    db: Arc<Db>,
    pty: Arc<PtyManager>,
    adapters: Arc<AdapterRegistry>,
    /// Current worker handle. Dropped/replaced on token change.
    worker: Mutex<Option<WorkerHandle>>,
}

struct WorkerHandle {
    join: Option<JoinHandle<()>>,
    inner: Arc<BridgeInner>,
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            // Worker loops on POLL_INTERVAL + a 1s getUpdates — bounded wait.
            let _ = j.join();
        }
    }
}

impl TelegramBridge {
    pub fn new(db: Arc<Db>, pty: Arc<PtyManager>, adapters: Arc<AdapterRegistry>) -> Arc<Self> {
        let inner = Arc::new(BridgeInner {
            status: Mutex::new(BridgeStatus::Disabled),
            owner_chat_id: Mutex::new(settings::get_owner(&db).unwrap_or(None)),
            pending_replies: Mutex::new(LruCache::new(
                NonZeroUsize::new(PENDING_REPLIES_CAPACITY).unwrap(),
            )),
            next_update_offset: Mutex::new(0),
            snapshots: Mutex::new(HashMap::new()),
            shutdown: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        });
        let bridge = Arc::new(Self {
            inner,
            db,
            pty,
            adapters,
            worker: Mutex::new(None),
        });
        // Spin up a worker if a token is already configured from a previous run.
        if let Ok(Some(_)) = settings::get_token(&bridge.db) {
            bridge.start_worker();
        }
        bridge
    }

    pub fn status(&self) -> BridgeStatus {
        self.inner.status.lock().clone()
    }

    pub fn owner_chat_id(&self) -> Option<i64> {
        *self.inner.owner_chat_id.lock()
    }

    pub fn has_token(&self) -> bool {
        settings::get_token(&self.db)
            .ok()
            .flatten()
            .is_some()
    }

    /// Store a new token (or clear it when `None`). Restarts the worker.
    pub fn set_token(&self, token: Option<String>) -> Result<(), String> {
        let trimmed = token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        settings::set_token(&self.db, trimmed.as_deref()).map_err(|e| e.to_string())?;
        // Token change invalidates the previous owner assignment — the new
        // token belongs to a different bot, so first /start claims ownership
        // again. This matches the "reset bot" mental model.
        settings::clear_owner(&self.db).map_err(|e| e.to_string())?;
        *self.inner.owner_chat_id.lock() = None;
        self.inner.pending_replies.lock().clear();
        *self.inner.next_update_offset.lock() = 0;
        self.inner.snapshots.lock().clear();
        self.inner.generation.fetch_add(1, Ordering::SeqCst);

        // Drop and replace the worker. Drop signals shutdown and joins.
        {
            let mut guard = self.worker.lock();
            *guard = None;
        }
        if trimmed.is_some() {
            self.start_worker();
        } else {
            *self.inner.status.lock() = BridgeStatus::Disabled;
        }
        Ok(())
    }

    /// Clear the owner so the next `/start` captures a new chat.
    pub fn reset_owner(&self) -> Result<(), String> {
        settings::clear_owner(&self.db).map_err(|e| e.to_string())?;
        *self.inner.owner_chat_id.lock() = None;
        self.inner.pending_replies.lock().clear();
        Ok(())
    }

    /// Signal the PTY-exit path that a session with `telegram_enabled` just
    /// died. Fires a one-shot exit notification. Safe to call for sessions
    /// that aren't opted in — it drops silently.
    pub fn notify_exit(&self, pty_id: u32, code: Option<i32>) {
        // Look up opt-in + labels BEFORE the PTY row flips to exited.
        let Ok(enabled) = self.db.get_session_telegram_enabled(pty_id) else {
            return;
        };
        if !enabled {
            return;
        }
        let Some(label) = self.session_label(pty_id) else {
            return;
        };
        let Some(agent) = self.session_agent_type(pty_id) else {
            return;
        };
        let Some(chat_id) = *self.inner.owner_chat_id.lock() else {
            return;
        };
        let Ok(Some(token)) = settings::get_token(&self.db) else {
            return;
        };
        let api = TelegramApi::new(token);
        let notif = format_exit(&label, &agent, code);
        if let Err(e) = send_notification(&api, chat_id, &notif, None, &self.inner, pty_id) {
            log::warn!("tg notify_exit: {e}");
        }
    }

    // -- Internals -----------------------------------------------------------

    fn start_worker(&self) {
        *self.inner.status.lock() = BridgeStatus::Connecting;
        self.inner.shutdown.store(false, Ordering::SeqCst);

        let inner = self.inner.clone();
        let db = self.db.clone();
        let pty = self.pty.clone();
        let adapters = self.adapters.clone();
        let generation = self.inner.generation.load(Ordering::SeqCst);

        let handle = thread::spawn(move || worker_loop(inner, db, pty, adapters, generation));

        let mut guard = self.worker.lock();
        *guard = Some(WorkerHandle {
            join: Some(handle),
            inner: self.inner.clone(),
        });
    }

    fn session_label(&self, pty_id: u32) -> Option<String> {
        // Scan all projects to find the session's task_name. Cheap because
        // the set is small; avoids a dedicated SQL helper.
        let projects = self.db.list_projects().ok()?;
        for p in projects {
            let sessions = self.db.list_sessions_by_project_path(&p.path).ok()?;
            for s in sessions {
                if s.pty_id == Some(pty_id) {
                    return Some(format!("{}/{}", p.name, s.task_name));
                }
            }
        }
        None
    }

    fn session_agent_type(&self, pty_id: u32) -> Option<String> {
        let projects = self.db.list_projects().ok()?;
        for p in projects {
            let sessions = self.db.list_sessions_by_project_path(&p.path).ok()?;
            for s in sessions {
                if s.pty_id == Some(pty_id) {
                    return Some(s.agent_type);
                }
            }
        }
        None
    }
}

/// Send a prepared notification and remember (message_id → pty_id) in the
/// reply cache. Returns the Telegram message id on success.
fn send_notification(
    api: &TelegramApi,
    chat_id: i64,
    notif: &Notification,
    reply_to: Option<i64>,
    inner: &BridgeInner,
    pty_id: u32,
) -> Result<i64, String> {
    let message_id = if let Some(att) = &notif.attachment {
        api.send_document(
            chat_id,
            &att.filename,
            &att.contents,
            Some(&notif.text),
            reply_to,
        )?
    } else {
        api.send_message(chat_id, &notif.text, reply_to)?
    };
    inner.pending_replies.lock().put(message_id, pty_id);
    Ok(message_id)
}

fn worker_loop(
    inner: Arc<BridgeInner>,
    db: Arc<Db>,
    pty: Arc<PtyManager>,
    adapters: Arc<AdapterRegistry>,
    generation: u64,
) {
    let token = match settings::get_token(&db) {
        Ok(Some(t)) => t,
        _ => {
            *inner.status.lock() = BridgeStatus::Disabled;
            return;
        }
    };
    let api = TelegramApi::new(token);

    // One-shot sanity check up front.
    match api.get_me() {
        Ok(_) => *inner.status.lock() = BridgeStatus::Polling,
        Err(e) => {
            *inner.status.lock() = BridgeStatus::Error {
                message: format!("token rejected: {e}"),
            };
            // Don't exit — user may fix the token and we re-read on next iteration.
        }
    }

    while !inner.shutdown.load(Ordering::SeqCst) {
        if inner.generation.load(Ordering::SeqCst) != generation {
            break;
        }

        // --- Inbound: poll Telegram updates (short poll, non-blocking-ish).
        let offset = *inner.next_update_offset.lock();
        match api.get_updates(offset, GET_UPDATES_TIMEOUT_SECS) {
            Ok(updates) => {
                if !updates.is_empty() {
                    *inner.status.lock() = BridgeStatus::Polling;
                }
                for u in updates {
                    let advance = u.update_id + 1;
                    *inner.next_update_offset.lock() = advance.max(offset);
                    if let Some(msg) = u.message {
                        handle_inbound(&api, &inner, &db, &pty, msg);
                    }
                }
            }
            Err(e) => {
                *inner.status.lock() = BridgeStatus::Error { message: e };
                // Back off once and continue; transient network errors are expected.
                thread::sleep(POLL_INTERVAL);
                continue;
            }
        }

        // --- Outbound: sweep opted-in sessions for new events.
        sweep_sessions(&api, &inner, &db, &pty, &adapters);

        // Breathing room between cycles. We already waited up to
        // GET_UPDATES_TIMEOUT_SECS on the long-poll, so keep this short.
        thread::sleep(POLL_INTERVAL);
    }

    // Exiting: don't clobber status that a set_token() follow-up already set.
    if matches!(*inner.status.lock(), BridgeStatus::Polling | BridgeStatus::Connecting) {
        *inner.status.lock() = BridgeStatus::Disabled;
    }
}

fn handle_inbound(
    api: &TelegramApi,
    inner: &BridgeInner,
    db: &Db,
    pty: &PtyManager,
    msg: Message,
) {
    let Some(user) = msg.from.as_ref() else {
        return;
    };
    let chat_id = msg.chat.id;
    let text = msg.text.clone().unwrap_or_default();

    // Commands: /start, /reset.
    if text.starts_with("/start") {
        // First writer wins; subsequent /start from non-owner is silent.
        let mut owner = inner.owner_chat_id.lock();
        match *owner {
            None => {
                *owner = Some(user.id);
                drop(owner);
                let _ = settings::set_owner(db, user.id);
                let _ = api.send_message(
                    chat_id,
                    "✅ Sessonix bot linked. You'll receive notifications from opted-in sessions. Reply to any notification to send a prompt back.",
                    Some(msg.message_id),
                );
            }
            Some(existing) if existing == user.id => {
                drop(owner);
                let _ = api.send_message(chat_id, "Already linked.", Some(msg.message_id));
            }
            Some(_) => {
                // Don't leak that another account owns the bot.
            }
        }
        return;
    }

    // Owner-only beyond this point.
    let owner = *inner.owner_chat_id.lock();
    if owner != Some(user.id) {
        return;
    }

    if text.starts_with("/reset") {
        inner.pending_replies.lock().clear();
        *inner.owner_chat_id.lock() = None;
        let _ = settings::clear_owner(db);
        let _ = api.send_message(
            chat_id,
            "Owner cleared. Send /start from a chat to re-link.",
            Some(msg.message_id),
        );
        return;
    }

    if text.starts_with("/help") {
        let _ = api.send_message(chat_id, HELP_TEXT, Some(msg.message_id));
        return;
    }

    if text.starts_with("/sessions") || text.starts_with("/list") {
        handle_list_sessions(api, inner, db, chat_id, msg.message_id);
        return;
    }

    if text.starts_with("/send") {
        handle_send_command(api, inner, db, pty, chat_id, &msg, text.as_str());
        return;
    }

    // Reply to a previous bot message → route the text into that session's PTY.
    if let Some(reply_to) = msg.reply_to_message.as_ref() {
        let Some(pty_id) = inner.pending_replies.lock().get(&reply_to.message_id).copied() else {
            let _ = api.send_message(
                chat_id,
                "Can't find the session for this reply — it may be too old. Open Sessonix to continue.",
                Some(msg.message_id),
            );
            return;
        };
        let Ok(session) = pty.get_session(pty_id) else {
            let _ = api.send_message(
                chat_id,
                "Session is no longer running.",
                Some(msg.message_id),
            );
            return;
        };
        // Terminal "Enter" is \r (0x0D), not \n — raw-mode TUIs (Claude,
        // Codex) listen for carriage return and ignore LF.
        let body = format!("{text}\r");
        match session.write_input(body.as_bytes()) {
            Ok(()) => {
                let _ = api.set_reaction(chat_id, msg.message_id, "👍");
            }
            Err(e) => {
                let _ = api.send_message(
                    chat_id,
                    &format!("Failed to deliver to session: {e}"),
                    Some(msg.message_id),
                );
            }
        }
        return;
    }

    // Top-level message from owner that isn't a command or reply: explain.
    let _ = api.send_message(
        chat_id,
        "Reply to a notification, or use /sessions + /send <id> <text> to target a session. /help for all commands.",
        Some(msg.message_id),
    );
}

const HELP_TEXT: &str = "Sessonix bot commands:\n\
/sessions — list opted-in sessions and their status\n\
/send <id> <text> — send a prompt to session #id\n\
/reset — clear owner; next /start rebinds the bot\n\
/help — this message\n\n\
You can also reply to any notification and your text goes into the same session.";

fn handle_list_sessions(
    api: &TelegramApi,
    inner: &BridgeInner,
    db: &Db,
    chat_id: i64,
    reply_to: i64,
) {
    let sessions = match db.list_telegram_enabled_sessions() {
        Ok(s) => s,
        Err(e) => {
            let _ = api.send_message(chat_id, &format!("DB error: {e}"), Some(reply_to));
            return;
        }
    };
    if sessions.is_empty() {
        let _ = api.send_message(
            chat_id,
            "No sessions opted in. Toggle the ✈ icon on any session card in Sessonix.",
            Some(reply_to),
        );
        return;
    }

    let snaps = inner.snapshots.lock();
    let mut body = String::from("Active sessions:\n");
    for (pty_id, agent_type, task_name, working_dir) in &sessions {
        let project = std::path::Path::new(working_dir)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(working_dir.as_str());
        let status_label = snaps
            .get(pty_id)
            .map(|s| match s.last_status {
                SessionStatus::Idle => "idle",
                SessionStatus::Running => "running",
                SessionStatus::Error => "error",
                SessionStatus::Exited => "exited",
            })
            .unwrap_or("pending");
        body.push_str(&format!(
            "\n#{pty_id}  {project}/{task_name}  ({agent_type}, {status_label})"
        ));
    }
    drop(snaps);
    body.push_str("\n\nSend with `/send <id> <text>`.");
    let _ = api.send_message(chat_id, &body, Some(reply_to));
}

fn handle_send_command(
    api: &TelegramApi,
    _inner: &BridgeInner,
    db: &Db,
    pty: &PtyManager,
    chat_id: i64,
    msg: &Message,
    full_text: &str,
) {
    const USAGE: &str =
        "Usage: /send <session_id> <text>\nRun /sessions to see active IDs.";

    // Drop the leading "/send" (and a possible "@botname" suffix in groups).
    let after = full_text
        .split_once(char::is_whitespace)
        .map(|(_, rest)| rest)
        .unwrap_or("")
        .trim_start();
    let Some((id_token, rest)) = after.split_once(char::is_whitespace) else {
        let _ = api.send_message(chat_id, USAGE, Some(msg.message_id));
        return;
    };
    let prompt = rest.trim();
    if prompt.is_empty() {
        let _ = api.send_message(chat_id, USAGE, Some(msg.message_id));
        return;
    }
    let Ok(pty_id) = id_token.trim_start_matches('#').parse::<u32>() else {
        let _ = api.send_message(
            chat_id,
            &format!("'{id_token}' is not a valid session id."),
            Some(msg.message_id),
        );
        return;
    };

    // Safety: refuse to write to a session the user didn't explicitly opt in.
    // Without this, the command could reach any PTY id by guessing.
    let opted_in = db.get_session_telegram_enabled(pty_id).unwrap_or(false);
    if !opted_in {
        let _ = api.send_message(
            chat_id,
            &format!(
                "Session #{pty_id} is not opted in. Enable the ✈ toggle on it in Sessonix first."
            ),
            Some(msg.message_id),
        );
        return;
    }

    let Ok(session) = pty.get_session(pty_id) else {
        let _ = api.send_message(
            chat_id,
            &format!("Session #{pty_id} is not running."),
            Some(msg.message_id),
        );
        return;
    };
    // Terminal Enter is \r (0x0D). See reply-to-message handler above.
    let payload = format!("{prompt}\r");
    match session.write_input(payload.as_bytes()) {
        Ok(()) => {
            let _ = api.set_reaction(chat_id, msg.message_id, "👍");
        }
        Err(e) => {
            let _ = api.send_message(
                chat_id,
                &format!("Failed to deliver: {e}"),
                Some(msg.message_id),
            );
        }
    }
}

fn sweep_sessions(
    api: &TelegramApi,
    inner: &BridgeInner,
    db: &Db,
    pty: &PtyManager,
    adapters: &AdapterRegistry,
) {
    let Some(chat_id) = *inner.owner_chat_id.lock() else {
        return; // No owner yet, nothing to send to.
    };

    let sessions = match db.list_telegram_enabled_sessions() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("tg sweep: list sessions failed: {e}");
            return;
        }
    };

    // Drop snapshots for sessions that are no longer opted-in (or ended).
    let live_ids: std::collections::HashSet<u32> = sessions.iter().map(|(id, _, _, _)| *id).collect();
    inner.snapshots.lock().retain(|k, _| live_ids.contains(k));

    for (pty_id, agent_type, task_name, _working_dir) in sessions {
        let Ok(session) = pty.get_session(pty_id) else {
            continue;
        };
        let scrollback = session.snapshot_last_lines();
        let Some(adapter) = adapters.get(&agent_type) else {
            continue;
        };

        let prev = inner
            .snapshots
            .lock()
            .get(&pty_id)
            .cloned()
            .unwrap_or_default();

        let (next, events) = detect_events(&prev, &scrollback, adapter);
        inner.snapshots.lock().insert(pty_id, next);

        if events.is_empty() {
            continue;
        }

        // Include the project name in the label when available, matching the
        // format used for exit notifications.
        let label = session_label(db, pty_id).unwrap_or_else(|| task_name.clone());

        for evt in events {
            let notif = match evt {
                SessionEvent::Idle { response } => format_idle(&label, &agent_type, &response),
                SessionEvent::Permission => format_permission(&label, &agent_type),
                SessionEvent::Exit { code } => format_exit(&label, &agent_type, code),
            };
            if let Err(e) = send_notification(api, chat_id, &notif, None, inner, pty_id) {
                log::warn!("tg send: {e}");
            }
        }
    }
}

fn session_label(db: &Db, pty_id: u32) -> Option<String> {
    let projects = db.list_projects().ok()?;
    for p in projects {
        let sessions = db.list_sessions_by_project_path(&p.path).ok()?;
        for s in sessions {
            if s.pty_id == Some(pty_id) {
                return Some(format!("{}/{}", p.name, s.task_name));
            }
        }
    }
    None
}
