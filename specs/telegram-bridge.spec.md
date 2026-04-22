---
name: Telegram Bridge
description: Remote monitoring and control of Sessonix agent sessions through a user-provided Telegram bot — get pinged on idle/permission-prompts/process-exit, and reply inline to inject prompts back into the PTY
targets:
  - ../src-tauri/src/telegram_bridge.rs
  - ../src-tauri/src/telegram_bridge/bot.rs
  - ../src-tauri/src/telegram_bridge/events.rs
  - ../src-tauri/src/telegram_bridge/settings.rs
  - ../src-tauri/src/adapters/mod.rs
  - ../src-tauri/src/adapters/claude.rs
  - ../src-tauri/src/adapters/codex.rs
  - ../src-tauri/src/adapters/gemini.rs
  - ../src-tauri/src/session_manager.rs
  - ../src-tauri/src/db.rs
  - ../src-tauri/src/lib.rs
  - ../src-tauri/Cargo.toml
  - ../src/components/TelegramSettings.tsx
  - ../src/components/Sidebar.tsx
  - ../src/store/sessionStore.ts
  - ../src/lib/api.ts
---

# Telegram Bridge

A **self-hosted** Telegram bot lets the user walk away from their machine and still
watch key events across running agent sessions, and send prompts back into a session
by replying to the bot's notification messages.

The problem: agents frequently stall waiting for a permission prompt (`y/n`), or
finish a long task and just sit there. Today the user must stay at the screen. With
this feature, Sessonix pings the user's Telegram on three events (idle, permission
prompt, process exit), and a reply-to-message flow routes any text the user sends
back into the session that triggered the notification.

## Scope

In scope:

- User registers their own bot (BotFather token pasted in Settings). Sessonix runs the
  long-polling loop itself while the app is open.
- Single owner: the first Telegram account to send `/start` after a fresh token is
  locked in as owner; no other `chat_id` can interact with the bot.
- Per-session opt-in toggle — each session tab has a Telegram switch, default OFF.
- Three notification triggers, each on opted-in sessions only:
  - Thinking → Idle transition (agent finished its turn).
  - Permission prompt detected in terminal output.
  - Process exit (clean or crash).
- Notification content: per-adapter "final agent response" parser with a generic
  tail-to-prompt fallback. Oversize responses (≥ 3500 chars) are truncated inline and
  attached in full as a `.md` file.
- Inline reply: a user reply to any bot message is treated as a prompt and written
  verbatim (+ `\n`) to the PTY of the session that triggered the replied-to message.
- Settings UI in the frontend: token field, owner `chat_id` display, reset-owner
  button, bot-status indicator (off / polling / error).
- Token persisted locally; agent output sent to Telegram's API only for sessions the
  user has explicitly opted in.

Bot commands (v1 set):

- `/start` — first sender becomes owner; subsequent attempts from other chats are silent.
- `/reset` — owner only; clears the claimed chat id so a new `/start` can rebind.
- `/sessions` (alias `/list`) — owner only; reply with a numbered list of opted-in
  running sessions plus their last-known status (idle / running / error).
- `/send <pty_id> <prompt>` — owner only; write the prompt text (+ `\n`) into the
  specified session's PTY. Target must be opted-in. Fallback route when the user
  wants to nudge a session the bot hasn't pinged about recently.
- `/help` — usage summary listing all commands.

Out of scope (v1):

- Shared/hosted bot with multi-user accounts — users must run their own.
- Multiple owners / multi-device whitelisting — one `chat_id` per token.
- Active-session `/select` mode or Telegram-topics-per-session routing.
- `/screenshot`, `/log`, `/watch` commands — v2 if users ask for them.
- Long-thinking-timeout and error-pattern detection triggers.
- Away-mode global toggle (per-session opt-in is the only control).
- Encrypted-at-rest token storage via OS keychain — v1 stores the token in the same
  `app_dir` as the SQLite DB with `0600` file permissions on Unix.
- Message delivery while Sessonix is not running — if the app is closed, no PTYs are
  alive anyway; we inherit the app lifecycle.
- Localisation of bot messages — English only.

## Architecture

### Two actors inside the Rust backend

- **`TelegramBot`** — owns the `teloxide` long-polling task. One task, one tokio
  runtime handle. Reads inbound updates, routes replies to the right session.
- **`EventBridge`** — subscribes to `SessionEvent`s emitted by `SessionManager` and
  forwards them as outbound messages to the `TelegramBot`. Maintains the map of
  "bot message_id → session_id" needed for reply-to-message routing.

Both live under a new module tree `src-tauri/src/telegram_bridge/` with `mod.rs`
exposing a single `TelegramBridge` facade. The facade owns a `tokio::mpsc` channel
(`Sender<BridgeCommand>`) that the rest of the app uses. This keeps all Telegram
concerns behind one interface and lets `session_manager.rs` stay oblivious to the
presence of a bot.

### Data flow: outbound (session → Telegram)

```
PTY reader thread
  → SessionManager.on_output(session_id, bytes)
      → AgentAdapter.extract_status(scrollback)         // existing
      → AgentAdapter.detect_permission_prompt(scrollback) // NEW
      → emit SessionEvent::StatusChanged / PermissionPrompt / ProcessExited
  → EventBridge.handle(event)
      → if !is_opted_in(session_id): return
      → AgentAdapter.extract_last_response(scrollback) → text    // NEW
      → format header ("🟢 Claude · <project>/<session> — idle")
      → if text.len() > 3500: truncate + build .md attachment
      → TelegramBot.send({chat_id: owner, text, reply_markup: Force, attachment?})
      → remember (telegram_message_id → session_id) in BotState.pending_replies
```

### Data flow: inbound (Telegram → session)

```
teloxide long-polling loop
  → Update received
  → if not a message from owner chat_id: drop
  → if message is a reply to a known bot message_id:
      → resolve session_id from BotState.pending_replies
      → PtyManager.write_stdin(session_id, format!("{}\n", user_text).as_bytes())
      → ACK back to user (single "✓" reaction)
  → else if message is a bot command (/start, /status, /reset):
      → handle command
  → else: reply "Reply to a notification to send a prompt."
```

## Data model

### New SQLite table: `telegram_settings`

One row, `id = 1`. Rationale for a table over a JSON file: we already have a DB
connection everywhere we need this, migrations are in-place, and the per-session
opt-in bit lives more naturally beside the row for its session.

```sql
CREATE TABLE IF NOT EXISTS telegram_settings (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    bot_token    TEXT,                       -- null when unset
    owner_chat_id INTEGER,                   -- null until /start claims ownership
    created_at   INTEGER NOT NULL            -- unix seconds
);
```

Reset-owner clears `owner_chat_id` only; the token stays.
Reset-token clears both fields and stops the long-polling loop.

### New column on `sessions`

```sql
ALTER TABLE sessions ADD COLUMN telegram_enabled INTEGER NOT NULL DEFAULT 0;
```

The per-session opt-in bit. Read by `EventBridge` before forwarding any event.

### In-memory bot state

```rust
struct BotState {
    owner_chat_id: Option<i64>,
    // Maps Telegram message_id (of our outbound notification)
    // to the Sessonix session_id it referenced.
    // Bounded: 256 entries, LRU-evicted. Replies to evicted messages fall through
    // to a "can't find session, sorry" error.
    pending_replies: lru::LruCache<i32, u32>,
}
```

## Module: `src-tauri/src/telegram_bridge/`

### `mod.rs` — public facade

```rust
pub struct TelegramBridge {
    cmd_tx: tokio::sync::mpsc::Sender<BridgeCommand>,
}

pub enum BridgeCommand {
    /// Set or replace the bot token. Restarts the polling loop.
    SetToken(Option<String>),
    /// Reset owner chat_id; next /start wins again.
    ResetOwner,
    /// Forward a session event for potential delivery.
    SessionEvent(SessionEvent),
    /// Report current status (polling / stopped / error) to frontend.
    GetStatus(tokio::sync::oneshot::Sender<BridgeStatus>),
}

pub enum BridgeStatus {
    Disabled,          // no token
    Connecting,
    Polling,
    Error(String),
}

impl TelegramBridge {
    pub fn spawn(db: Arc<Db>, pty: Arc<PtyManager>) -> Self { ... }
    pub fn send(&self, cmd: BridgeCommand) -> Result<(), ()>;
}
```

### `bot.rs` — teloxide runtime

Owns the polling loop. Holds `BotState`. Exposes:

```rust
async fn run(
    token: String,
    db: Arc<Db>,
    pty: Arc<PtyManager>,
    event_rx: tokio::sync::mpsc::Receiver<OutboundMessage>,
) -> Result<()>;
```

The loop selects over (teloxide updates) × (event_rx outbound messages). On each
update it dispatches commands (`/start`, `/status`, `/reset`) or routes replies.

### `events.rs` — `EventBridge`

A thin adapter that subscribes to `SessionEvent`s (via a broadcast channel exposed
by `SessionManager`) and pushes `OutboundMessage`s into `bot.rs`. Contains the
formatter and the truncate-to-attachment logic.

### `settings.rs`

Persistence helpers around the `telegram_settings` table: `get()`, `set_token()`,
`set_owner()`, `clear_owner()`.

## `AgentAdapter` trait extensions

Two new methods on the existing trait:

```rust
/// Extract the last "meaningful" agent response from the terminal scrollback.
/// Used to populate Telegram idle notifications.
/// Return None if no parseable response was found — the caller falls back to
/// a generic "tail until prompt marker" extraction.
fn extract_last_response(&self, scrollback: &str) -> Option<String>;

/// Return true if the current terminal tail shows the agent waiting on a
/// yes/no permission prompt. Detected on the latest scrollback only.
fn detect_permission_prompt(&self, scrollback: &str) -> bool;
```

### Per-adapter implementations

- **`claude.rs`** — split on Claude Code turn markers; return the last assistant
  turn with ANSI stripped and tool-use blocks elided. Permission prompt detected
  by the Claude Code `Do you want to` / `y/n` signature.
- **`codex.rs`** — locate Codex's final response block; permission prompt pattern
  specific to Codex CLI.
- **`gemini.rs`** — same shape, adapter-specific markers.
- **`GenericAdapter`** (fallback for custom CLIs) — `extract_last_response` returns
  `None`, `detect_permission_prompt` returns `false`. The generic tail extraction
  in `events.rs` kicks in: last N lines up to (but not including) the trailing
  shell-prompt character.

## `SessionManager` changes

Add a `tokio::sync::broadcast::Sender<SessionEvent>` on `SessionManager` and emit:

```rust
pub enum SessionEvent {
    StatusChanged { session_id: u32, old: Status, new: Status },
    PermissionPrompt { session_id: u32 },
    ProcessExited { session_id: u32, code: Option<i32> },
}
```

Emission points (all already exist as no-op hooks in the reader thread):

- `StatusChanged` → fired from the same place that already calls
  `AgentAdapter::extract_status()` after every `ring_buffer.write`. Dedup: only fire
  when the new status differs from the last emitted status for that session.
- `PermissionPrompt` → called from the same hook if
  `AgentAdapter::detect_permission_prompt()` returns `true` *and* the previous tick
  returned `false` (rising edge).
- `ProcessExited` → from the existing `wait()` arm in `pty_manager.rs`; bubbled up
  through `session_manager` where it already marks the DB row as exited.

`TelegramBridge` subscribes to this broadcast once during `spawn()`.

## IPC surface (`lib.rs`)

Four new commands. All snake_case on the Rust side; frontend calls them as camelCase.

```rust
#[tauri::command]
async fn get_telegram_status(state: State<'_, AppState>) -> Result<TelegramStatusDto, String>;

#[tauri::command]
async fn set_telegram_token(token: Option<String>, state: State<'_, AppState>) -> Result<(), String>;

#[tauri::command]
async fn reset_telegram_owner(state: State<'_, AppState>) -> Result<(), String>;

#[tauri::command]
async fn set_session_telegram_enabled(
    session_id: u32,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String>;
```

`TelegramStatusDto` mirrors `BridgeStatus` plus `owner_chat_id: Option<i64>` and
`has_token: bool`. Used by `TelegramSettings.tsx` to render state.

## Frontend changes

### `src/components/TelegramSettings.tsx` — new component

A card inside the existing Settings surface. Fields:

- **Bot Token** (`<input type="password">`) with save/clear buttons. Validates
  non-empty before calling `setTelegramToken()`. Shows a help link explaining
  BotFather.
- **Owner** — shows `owner_chat_id` if claimed ("Registered to chat 123456789"), or
  instructions ("Open the bot in Telegram and send `/start`"). Reset button clears it.
- **Status** — a badge: `Disabled` (grey) / `Connecting` (amber) / `Polling` (green)
  / `Error: <message>` (red).

Status polls every 3 s via `getTelegramStatus()` while the settings view is open.

### `src/components/Sidebar.tsx` — per-session toggle

Each session row grows a small Telegram icon. Click toggles
`set_session_telegram_enabled`. Visual states: filled (enabled) / outlined (disabled)
/ hidden (no token configured globally — the toggle won't render until a token is set).

### `src/store/sessionStore.ts`

Add `telegramEnabled: boolean` on the session type and a `toggleTelegram(id)` action
that calls the IPC and updates local state optimistically with rollback on error.

### `src/lib/api.ts`

Four new typed wrappers: `getTelegramStatus`, `setTelegramToken`, `resetTelegramOwner`,
`setSessionTelegramEnabled`.

## Security notes

- Token storage: `telegram_settings.bot_token` is plaintext in SQLite. Acceptable
  because (a) the DB already lives under `~/Library/Application Support` / `%APPDATA%`
  with user-only permissions, (b) the token only controls a bot the user created,
  (c) moving it to OS keychain (`keyring` crate) is tracked as a follow-up. The spec
  for encryption-at-rest belongs to a future revision.
- Owner enforcement: every inbound update's `from.id` is checked against
  `owner_chat_id` before any session interaction. Non-owner messages are silently
  dropped (no "unauthorized" reply, to not leak the bot's purpose to scanners).
- Opt-in gating: an event for a session where `telegram_enabled = 0` is dropped
  before any network call. No accidental leaks from sessions the user didn't bridge.
- No secret scrubbing: terminal output may include tokens the agent printed. The
  user's opt-in is their consent for the session's output to flow to Telegram. A
  warning is shown in the per-session toggle tooltip.
- The bot never runs when the app is not running — no persistent server, no leaked
  notifications after `window.close()`.

## Dependencies

`src-tauri/Cargo.toml` gains:

- `teloxide = "0.13"` with default features — long-polling, command parsing,
  reply-markup helpers.
- `lru = "0.12"` for the bounded `pending_replies` cache.

No new frontend dependencies.

## Testing

### Pure-logic Rust tests

`[@test] ../src-tauri/src/telegram_bridge/events.rs`

- Formatter produces expected header for each event variant.
- Truncation kicks in at ≥ 3500 chars and preserves both an inline preview and a
  full-text attachment body.
- Short messages (< 3500 chars) produce no attachment.
- Opt-in gate: `handle(event)` is a no-op when the session's `telegram_enabled`
  is `false`.

### Adapter tests

`[@test] ../src-tauri/src/adapters/claude.rs`
`[@test] ../src-tauri/src/adapters/codex.rs`
`[@test] ../src-tauri/src/adapters/gemini.rs`

- `extract_last_response` on a realistic scrollback returns only the final
  assistant turn with ANSI stripped and tool-use blocks elided.
- `extract_last_response` on scrollback with no assistant turn yet returns `None`.
- `detect_permission_prompt` is `true` exactly when the agent-specific y/n signature
  is present at the tail.
- Rising-edge semantics: two consecutive scrollbacks that both contain a permission
  prompt (because the tail didn't advance) do not both match — callers dedup by
  comparing to last result.

### BotState tests

`[@test] ../src-tauri/src/telegram_bridge/bot.rs`

- `pending_replies` LRU eviction at 256 entries drops the oldest first.
- `pending_replies` lookup by Telegram `message_id` round-trips to the correct
  `session_id`.
- Non-owner `chat_id` message is dropped (no PTY write attempted).

### DB migration tests

`[@test] ../src-tauri/src/db.rs`

- Opening an existing DB without the `telegram_settings` table creates it on demand.
- Existing `sessions` rows gain `telegram_enabled = 0` after migration.

### Not covered automatically — manual verification via `npm run tauri dev`

The teloxide long-polling loop, actual network delivery, and Telegram UI chrome
require a real bot. Manual checklist:

1. Paste a fresh BotFather token in Settings → status goes `Connecting` → `Polling`
   within 2 s; no errors.
2. Send `/start` from the user's personal chat → `Owner` field populates; a second
   `/start` from a different chat is silently ignored.
3. Open a Claude session, opt in via the Sidebar toggle, run a prompt → receive one
   message on idle, containing the agent's final response.
4. Reply to that message in Telegram with `tell me more` → Sessonix injects the text
   into the PTY and Claude continues; a ✓ reaction appears on the user's reply.
5. Trigger a Claude "Do you want to allow this edit?" prompt → receive a Permission
   message; reply `y` → agent proceeds.
6. Kill the agent process externally → receive a Process-exit message with exit code.
7. Flip the per-session toggle OFF mid-run → subsequent events for that session do
   not reach Telegram; other opted-in sessions are unaffected.
8. Paste an 8 KB agent response → Telegram message shows the first ~3500 chars and
   a `response.md` attachment with the full text.
9. Reset owner → a new `/start` claims ownership.
10. Clear the token → status returns to `Disabled`; no further polling.

## Non-functional notes

- **Memory** — one LRU cache of 256 `(i32, u32)` pairs per running bridge
  (~4 KB). Negligible.
- **CPU** — per-event work is a string scan in the adapter plus one HTTPS request
  per outbound message. No hot-path overhead for non-opted-in sessions (gated
  before any work).
- **Network** — idle app: one long-polling GET every ~50 s (teloxide default).
  Active: proportional to events fired (rare).
- **Failure policy** — network errors in the polling loop are logged at `warn` and
  retried with exponential backoff (teloxide default). Token-invalid errors
  surface as `BridgeStatus::Error` visible in Settings. Never panic, never block
  the main Tauri thread.
- **App restart** — `TelegramBridge::spawn()` is called once from `setup`. On
  startup, if a token is present in `telegram_settings`, polling starts immediately;
  `owner_chat_id` persists across restarts.
