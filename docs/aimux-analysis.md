# Aimux Analysis — Patterns Worth Adopting

Source: https://github.com/zanetworker/aimux
Go TUI (Bubble Tea) for managing AI coding agents. 102 commits, MIT, active.

## What aimux does that we don't (yet)

| Feature | Aimux | AICoder | Priority |
|---------|-------|---------|----------|
| Auto-discover running agents | `ps aux` scan every 2s | Manual launch only | HIGH |
| Conversation tracing | Parses JSONL turn-by-turn | Terminal output only | HIGH |
| Cost tracking | Per-model pricing, per-turn costs | Not implemented | MEDIUM |
| Turn annotations | GOOD/BAD/WASTE per turn | Not implemented | LOW |
| OTEL telemetry | Receives spans on :4318 | Not implemented | LOW |
| Subagent correlation | Process tree + OTEL attrs | Not implemented | LOW |
| K8s remote agents | Redis heartbeat + kubectl | Not implemented | FUTURE |

## 1. Agent Discovery via Process Scanning

Aimux discovers already-running agents by scanning `ps aux`:

```
Orchestrator.Discover()
  -> For each provider (Claude, Codex, Gemini):
     -> ScanProcesses()  // ps aux parsing
     -> discoverRecentSessions()  // 24h idle sessions from FS
  -> assignUniqueSuffixes()  // "myapp #1", "myapp #2"
```

**Key details:**
- Filters out: wrappers, Chrome helpers, shell subprocesses, tmux, grep, aimux itself
- Extracts flags: `--model`, `--permission-mode`, `--resume`, `--session-id`
- Classifies source: VSCode, SDK, CLI
- Subagent detection: walks parent PIDs up 5 levels, tags child agents

**What we could adopt:**
Add an "Attach" mode alongside "Launch". On startup (or via button), scan for orphan Claude/Codex/Gemini processes and offer to adopt them into the dashboard. Requires matching PID to working directory.

## 2. Session File Locations

Where agents store their conversation data:

| Agent | Path | Format |
|-------|------|--------|
| Claude | `~/.claude/projects/<dir-key>/*.jsonl` | JSONL (one entry per message/tool) |
| Codex | `~/.codex/sessions/*.jsonl` | JSONL |
| Gemini | `~/.gemini/tmp/*/chats/*.json` | JSON |

**Claude dir-key encoding:** `/Users/me/repo` becomes `-Users-me-repo` (replace `/` with `-`).

Decoding is lossy. Aimux tries: naive replace, github.com fix, segment walking with dot/hyphen joins.

## 3. Conversation Tracing (Claude JSONL)

Each Claude session has a JSONL file with entries:

```json
{"type": "user", "message": {...}, "timestamp": "..."}
{"type": "assistant", "message": {"content": [...], "usage": {...}}, "stop_reason": "end_turn"}
{"type": "system", "subtype": "error", "message": "context_window_exceeded"}
```

Aimux parses this into `Turn` objects:

```
Turn {
    Number, Timestamp, EndTime
    UserLines []string       // human input
    Actions   []ToolSpan     // tool calls (Read, Edit, Bash...)
    OutputLines []string     // assistant text
    TokensIn, TokensOut int64
    CostUSD   float64
    Model     string
}

ToolSpan {
    Name      string   // "Read", "Edit", "Bash"
    Snippet   string   // short summary of input
    Success   bool
    ErrorMsg  string
    ToolUseID string   // for matching tool_result
}
```

**Tool result matching:** When a `tool_result` entry with `tool_use_id` arrives, Aimux updates the corresponding ToolSpan's success status via a `pendingTools` map.

**What we could adopt:**
Build a trace viewer panel (alongside terminal). Parse Claude's JSONL in real-time to show: what tool was called, what files were read/edited, token usage per turn, cost. This is the single biggest UX upgrade possible.

## 4. Status Detection (Tail-Based)

Instead of parsing entire JSONL, reads last 8192 bytes:

```
Seek to fileSize - 8192
Skip first partial line
Walk backwards from last entry:
  - system error           -> StatusError
  - stop_reason=end_turn   -> StatusIdle
  - stop_reason=tool_use   -> StatusWaitingPermission
  - tool_result in content -> StatusActive
  - queue enqueue          -> StatusActive
```

O(1) status detection regardless of conversation length.

**What we could adopt:**
Replace our regex-on-terminal-output approach with JSONL tail parsing. More reliable, catches "waiting for permission" state which we currently miss.

## 5. Cost Tracking

Per-model pricing table (verified 2026-02-28):

| Model | Input $/1M | Output $/1M | Cache Read | Cache Write |
|-------|-----------|------------|------------|-------------|
| claude-opus-4-6 | $15.00 | $75.00 | $1.50 | $18.75 |
| claude-sonnet-4-5 | $3.00 | $15.00 | $0.30 | $3.75 |
| claude-haiku-3-5 | $0.80 | $4.00 | $0.08 | $1.00 |
| o3 (Codex) | $2.00 | $8.00 | - | - |
| gemini-2.5-pro | $1.25 | $10.00 | - | - |
| gemini-2.5-flash | $0.15 | $0.60 | - | - |

Model name normalization strips suffixes: `claude-sonnet-4-5[1m]@20250929` -> `claude-sonnet-4-5`.

Cost per turn = (tokensIn / 1M) * inputPrice + (tokensOut / 1M) * outputPrice + cache components.

Data source: `usage` blocks in JSONL assistant messages.

## 6. Annotations & Export

Per-turn annotation sidecar file: `~/.aimux/evaluations/<session-id>.jsonl`

```json
{"turn": 3, "label": "good", "note": "correct refactor", "timestamp": "..."}
{"turn": 7, "label": "waste", "note": "hallucinated file path", "timestamp": "..."}
```

Session-level metadata sidecar: `<session>.meta.json`
```json
{"annotation": "achieved", "tags": ["refactoring"], "title": "LLM-generated title"}
```

Export to JSONL or OTEL/MLflow for evaluation datasets.

## 7. OTEL Telemetry Integration

Aimux runs a local OTEL receiver on port 4318. When launching agents, injects env vars:

```
CLAUDE_CODE_ENABLE_TELEMETRY=1
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
OTEL_LOG_USER_PROMPTS=1
OTEL_LOG_TOOL_DETAILS=1
```

This gives real-time span data with tool calls, durations, and subagent identity.

**What we could adopt:**
When launching Claude, inject OTEL env vars. Run a lightweight receiver in the Tauri backend. This provides structured data without parsing terminal output at all.

## 8. Provider Interface Design

Clean abstraction with 11 core methods + optional traits:

```go
type Provider interface {
    Name() string
    Discover() ([]agent.Agent, error)
    ResumeCommand(a agent.Agent) *exec.Cmd
    CanEmbed() bool                          // can run in embedded PTY?
    FindSessionFile(a agent.Agent) string
    RecentDirs(max int) []RecentDir
    SpawnCommand(dir, model, mode string) *exec.Cmd
    SpawnArgs() SpawnArgs                    // models + modes available
    ParseTrace(filePath string) ([]trace.Turn, error)
    Kill(a agent.Agent) error
}

// Optional capabilities (composition)
type Messenger interface { SendMessage(agentID, text string) error }
type TaskLister interface { ListTasks() / GetTaskResult() }
type InfraProvider interface { Status() / CheckHealth() / SpawnSession() }
```

**What we could adopt:**
Our `AgentAdapter` trait only has `build_command()` and `extract_status()`. We should expand it with: `discover()`, `find_session_file()`, `parse_trace()`, `recent_dirs()`, `spawn_args()` (returns available models/modes for UI).

## 9. Architecture Patterns Worth Copying

**A. Parallel provider discovery with timeouts**
Run all providers concurrently. Individual provider timeout (1s for K8s). Don't drop slow results (prevents UI flicker).

**B. Atomic file operations for annotations**
Append-only JSONL (O_APPEND, kernel atomicity). Temp file + rename for deletions. No locking needed.

**C. Process tree correlation**
Walk parent PIDs up 5 levels to detect subagents. Prevents duplicate entries when Claude spawns Task agents.

**D. Multi-strategy session matching**
Match running process to session file via: (1) session ID flag, (2) process start time vs JSONL first timestamp, (3) directory + recency fallback.

## Priority Roadmap

### Phase 2a: Trace Viewer (highest value)
- Parse Claude JSONL from `~/.claude/projects/`
- Show turns, tools, tokens, cost in a side panel
- Real-time updates as session runs

### Phase 2b: Agent Discovery
- Scan `ps aux` for orphan agents
- "Attach" button to adopt running processes
- Show agents started outside AICoder

### Phase 2c: Cost Dashboard
- Implement pricing table
- Parse `usage` from JSONL
- Show per-session and per-project costs in StatusBar

### Phase 2d: OTEL Integration
- Inject telemetry env vars on launch
- Run lightweight OTLP receiver in Tauri backend
- Replace terminal regex with structured spans
