# Agent Deck Analysis — Patterns Worth Adopting

Source: https://github.com/asheshgoplani/agent-deck
Go TUI (BubbleTea + Lipgloss), tmux-based. Active, feature-rich.

## Key Architectural Difference

Agent Deck does NOT spawn PTY processes. It uses **tmux as the PTY container**.
All agents run in tmux sessions. Agent Deck is just a TUI dashboard that:
- Creates tmux sessions (`tmux new-session -d -s agentdeck_<uuid>`)
- Sends commands via `tmux send-keys`
- Reads output via `tmux capture-pane`
- Detects status by pattern-matching captured output

This means crash resilience for free: if Agent Deck dies, agents keep running in tmux.
Our approach (embedded PTY) gives tighter integration but loses sessions on crash.

## What agent-deck does that we don't (yet)

| Feature | Agent Deck | AICoder | Priority |
|---------|-----------|---------|----------|
| Claude hooks integration | Injects hooks into settings.json | Not implemented | **HIGH** |
| MCP management per session | Toggle MCPs via TUI | Not implemented | HIGH |
| Session forking | Fork with context inheritance | Not implemented | MEDIUM |
| Undo delete (Ctrl+Z) | Last 5 deletes recoverable | Not implemented | MEDIUM |
| Git worktree per agent | Auto-create worktrees | Not implemented | MEDIUM |
| Cost budgets | Daily/weekly/monthly limits | Just tracking, no limits | LOW |
| Docker sandbox | Run agents in containers | Not implemented | LOW |
| Conductor orchestration | Agent monitors other agents | Not implemented | FUTURE |
| Multi-profile | work/personal/client isolation | Not implemented | FUTURE |
| Global search | Full-text search across Claude conversations | Not implemented | MEDIUM |

## 1. Claude Code Hooks (most impactful pattern)

Agent Deck injects hooks into `~/.claude/settings.json` to receive structured events:

```json
{
  "hooks": {
    "SessionStart": [{ "command": "agent-deck hook-handler ..." }],
    "UserPromptSubmit": [{ "command": "agent-deck hook-handler ..." }],
    "Stop": [{ "command": "agent-deck hook-handler ..." }],
    "PermissionRequest": [{ "command": "agent-deck hook-handler ..." }],
    "Notification": [{ "command": "agent-deck hook-handler ..." }],
    "SessionEnd": [{ "command": "agent-deck hook-handler ..." }],
    "PreCompact": [{ "command": "agent-deck hook-handler ..." }]
  }
}
```

**7 hook events:**
- `SessionStart` — session began
- `UserPromptSubmit` — user sent a message
- `Stop` — agent finished a turn
- `PermissionRequest` — waiting for tool approval
- `Notification` — agent wants to notify user
- `SessionEnd` — session ended
- `PreCompact` — context window at 80%, about to compact

**Why this matters for us:**
Instead of polling JSONL tail every 5s or regex on terminal lines, Claude TELLS us
its status in real-time. Zero latency. Structured data. No ANSI parsing.

**What we could adopt:**
- Register a Tauri command as a hook handler
- Or: write a small binary that sends events via Tauri IPC/events
- Hook payload includes sessionID, so we can route to correct session
- PreCompact hook could trigger auto-save scrollback

## 2. Status Detection Patterns (terminal-based)

When hooks aren't available, Agent Deck uses sophisticated pattern matching on
`tmux capture-pane` output (last 50 lines):

**Claude Code indicators:**
```
BUSY:
  - "ctrl+c to interrupt" or "esc to interrupt"
  - Braille spinners: ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏
  - Asterisk spinners: ✳✽✶✢
  - Unicode ellipsis "…" + "tokens"

WAITING (permission prompts):
  - "No, and tell Claude what to do differently"
  - "│ Do you want..." (box-drawing chars)
  - "❯ Yes/No/Allow" (selection indicators)

IDLE:
  - ">" prompt at end (with --dangerously-skip-permissions)
```

**Comparison with our current approach:**
- We check: "Thinking...", "Reading file", "Writing", "$"/">" prompts, "error"
- Agent Deck checks: spinner characters, box-drawing permission dialogs, ctrl+c hint
- Their detection is MORE reliable because they check for actual UI elements, not text

**What we could adopt:**
Add spinner detection and permission dialog detection to our Claude adapter.
Especially the "ctrl+c to interrupt" / "esc to interrupt" pattern — it's the most
reliable indicator that Claude is actively working.

## 3. MCP Socket Pooling

Agent Deck shares MCP processes across sessions via Unix sockets:

```
~/.agent-deck/sockets/mcp-<name>.sock
```

- Multiple Claude sessions connect to ONE MCP process
- 85-90% memory savings (each MCP can use 100MB+)
- JSON-RPC proxy rewrites request IDs to route responses correctly
- Auto-reconnect on crash (3s recovery)
- Two scopes: LOCAL (per-session) or GLOBAL (all sessions)

**What we could adopt:**
When users run multiple Claude sessions with MCPs, each session spawns its own
MCP processes. We could add a shared MCP pool in the Tauri backend.

## 4. Session Forking

Agent Deck can "fork" a Claude session:
- Creates a new session that inherits the full conversation context
- User can explore alternatives without losing the original thread
- Uses Claude's `--resume` with the original session ID

**What we could adopt:**
Add a "Fork" button next to "Relaunch". Fork would create a new session with
`--continue` from the current session's agentSessionId, preserving the original
session as-is.

## 5. Undo Stack for Destructive Operations

```go
undoStack []deletedSessionEntry  // Chrome-style, last 5
```

Ctrl+Z recovers last deleted session. Reduces anxiety about accidental deletion.

**What we could adopt:**
Instead of immediate delete, keep sessions in a "recently deleted" buffer.
Show a toast "Session deleted. Undo?" with 5s timeout. Simple, high-value UX.

## 6. Round-Robin Status Updates

Instead of polling ALL sessions every tick:
- Update batches of 5-10 sessions per 2s tick
- Rotate through all sessions
- Active/focused session updated more frequently

**What we could adopt:**
Our `useStatusPolling` polls all alive sessions sequentially every 5s.
Should prioritize active session (poll every 2s) and background sessions less
frequently (every 10-15s).

## 7. Global Search Across Claude Conversations

Agent Deck indexes Claude transcripts from `~/.claude/transcripts/` and provides
fuzzy full-text search across all sessions via `G` key.

**What we could adopt:**
Add a Cmd+Shift+F search that reads JSONL files from `~/.claude/projects/`,
extracts user/assistant text content, and provides fuzzy search results.
Each result links to its session. Would be very useful for "where did I discuss X?"

## 8. Responsive Layout Breakpoints

```
< 50 cols:  single column (list only)
50-79 cols: stacked (list above preview)
80+ cols:   dual (side-by-side)
```

**What we could adopt:**
We already auto-collapse sidebar at <900px. Could go further:
- <700px: hide SummaryBar, icon-only sidebar
- <500px: full-screen terminal with tab bar

## 9. Configuration System

Agent Deck uses `~/.agent-deck/config.toml`:
```toml
[claude]
config_dir = "~/.claude"
allow_dangerous_mode = false

[costs.budgets]
daily_limit = 50.00

[tools.custom-agent]
name = "My Agent"
command = "my-cli"
```

**What we could adopt:**
We have no settings UI or config file. Should add `~/.aicoder/config.toml` for:
- Custom agent definitions
- Default agent on launch
- Cost budget limits
- Keyboard shortcut remapping
- Theme preferences

## 10. PreCompact Hook — Auto-Actions at Context Limit

Agent Deck uses the `PreCompact` hook (fires at ~80% context window) to:
- Auto-send `/clear` if configured
- Log cost snapshot before compaction
- Trigger notification to user

**What we could adopt:**
Register a PreCompact hook that:
- Shows a toast notification "Claude session X is running low on context"
- Saves cost snapshot
- Optionally auto-triggers session fork before context is lost

## Priority Roadmap (from Agent Deck patterns)

### Phase 2a: Claude Hooks Integration (highest value, lowest effort)
- Inject hooks into `~/.claude/settings.json`
- Receive structured events for real-time status
- Replace polling-based status detection
- Handle PermissionRequest → show badge "Waiting for permission"

### Phase 2b: Undo Delete + Toast Improvements
- Soft-delete with 5s undo window
- Toast shows "Undo" button

### Phase 2c: Session Forking
- "Fork" button on running Claude sessions
- New session with `--continue` from current agentSessionId
- Original session preserved

### Phase 2d: Global Search
- Index JSONL files across all projects
- Cmd+Shift+F opens search overlay
- Fuzzy match on user/assistant text
- Click result → switch to that session

### Phase 2e: Settings/Config
- `~/.aicoder/config.toml`
- Custom agents
- Cost budgets
- Keyboard shortcuts
