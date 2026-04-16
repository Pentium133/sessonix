# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is AICoder

Tauri 2 desktop app ("Agent Mission Control") for managing multiple AI coding agent sessions (Claude, Codex, Gemini, custom CLIs) in parallel via PTY terminals.

## Commands

```bash
# Development (full app with hot reload)
npm run tauri dev

# Frontend only (Vite dev server, port 1420)
npm run dev

# Type check
npm run typecheck        # or: npx tsc --noEmit

# Rust check/lint
cd src-tauri && cargo check
cd src-tauri && cargo clippy -- -D warnings

# Rust tests
cd src-tauri && cargo test

# Frontend tests (vitest, no tests written yet)
npm run test

# Production build
npm run tauri build
```

Note: On some systems `cargo` may not be in PATH for zsh. Use `~/.cargo/bin/cargo` as fallback.

## Architecture

**Two-process model:** Rust backend (Tauri) manages PTY sessions and SQLite. React frontend renders xterm.js terminals and communicates via Tauri IPC (`invoke`).

### Data flow: PTY output

```
Agent process → PTY slave → Reader thread (blocking, 1 per session)
  → ring_buffer.write(data)           // always, for detach recovery
  → if is_attached: emit("pty-output", {id, data})
      → frontend: usePtyOutput listener → writeToTerminal(id, bytes)
          → terminalPool.get(id).terminal.write(bytes)
```

### Data flow: Session switch

```
Switch from A to B:
  1. detachSession(A)  → is_attached=false, drain ring buffer (discard already-sent data)
  2. Hide terminal A wrapper (display:none)
  3. Show terminal B wrapper (display:block)
  4. attachSession(B)  → if was_detached: drain ring buffer → return missed data
  5. Write buffered data to terminal B
  6. FitAddon.fit() + focus
```

### Backend modules (src-tauri/src/)

- **lib.rs** — 17 Tauri IPC command handlers (the entire API surface)
- **session_manager.rs** — Coordinates PTY + DB. Generates UUID session IDs for Claude (`--session-id`). On startup, marks all "running" sessions as "exited" and initializes PTY ID counter from max DB value.
- **pty_manager.rs** — Spawns PTY processes, manages reader threads, attach/detach with `AtomicBool`. Validates working directory (absolute path, canonicalized, must exist).
- **ring_buffer.rs** — 1MB circular buffer per session. Drained on detach (discard) and on attach (return missed data).
- **db.rs** — SQLite with `projects` and `sessions` tables. Migrations add columns via ALTER TABLE. DB at `~/Library/Application Support/com.aicoder.app/aicoder.db` (macOS).
- **adapters/** — `AgentAdapter` trait with `build_command()` and `extract_status()`. Claude adapter strips ANSI and pattern-matches "Thinking...", "Reading", "Writing", prompt (`$`/`>`), errors.

### Frontend modules (src/)

- **App.tsx** — Layout shell: ProjectRail | SessionPanel | Content. Keyboard shortcuts: Cmd+Shift+K (add project), Cmd+Shift+T (new session in active project), Cmd+Shift+W (kill), Cmd+1-9 (switch within active project).
- **ProjectRail.tsx** — 48px left strip showing project letter icons. Clicking a project sets `activeProjectPath` and scopes all UI to that project.
- **Sidebar.tsx** — Session panel showing only sessions for `activeProjectPath`. Flat list (no project nesting). Collapsible via chevron.
- **TerminalPane.tsx** — Module-level `terminalPool` Map and `accessOrder` array (LRU, max 5 live xterm.js instances). Evicted terminals get scrollback saved. Wrappers are absolutely-positioned divs toggled via `display:none/block`.
- **SummaryBar.tsx** — Running session tabs, scoped to active project.
- **store/sessionStore.ts** — All session CRUD state. Restores from DB on startup with retry+backoff. `addSession` supports `replaceId` for in-place session replacement (relaunch preserves ordering).
- **store/projectStore.ts** — Project CRUD + `activeProjectPath`. The key state that scopes the entire UI.
- **hooks/usePtyOutput.ts** — Single global listener for `pty-output` and `pty-exit` Tauri events.
- **lib/api.ts** — Type-safe `invoke()` wrappers. Tauri 2 auto-converts Rust snake_case params to JS camelCase.

## Key conventions

- **Tauri IPC naming:** Rust commands are snake_case (`create_session`), JS calls use camelCase (`createSession`). Tauri 2 handles conversion automatically.
- **Session ID = PTY ID** (u32), not DB row ID. All frontend state keys on PTY ID.
- **Claude session management:** New sessions get `--session-id <uuid>`. Relaunch uses `--resume <uuid>` or `--continue` as fallback.
- **CSS:** Single `App.css` file, design tokens via CSS variables (`--bg`, `--surface`, `--accent`, `--claude`, `--codex`, `--gemini`). See DESIGN.md for full system.
- **Error handling:** Backend returns `Result<T, String>` from commands. Frontend shows errors via `showToast()`.
- **Tests:** Rust tests are inline `#[cfg(test)] mod tests` in each file. Frontend uses Vitest + jsdom + @testing-library/react.

## Design System
Always read DESIGN.md before making any visual or UI decisions.
All font choices, colors, spacing, and aesthetic direction are defined there.
Do not deviate without explicit user approval.
In QA mode, flag any code that doesn't match DESIGN.md.

## Skill routing

When the user's request matches an available skill, ALWAYS invoke it using the Skill
tool as your FIRST action. Do NOT answer directly, do NOT use other tools first.
The skill has specialized workflows that produce better results than ad-hoc answers.

Key routing rules:
- Product ideas, "is this worth building", brainstorming → invoke office-hours
- Bugs, errors, "why is this broken", 500 errors → invoke investigate
- Ship, deploy, push, create PR → invoke ship
- QA, test the site, find bugs → invoke qa
- Code review, check my diff → invoke review
- Update docs after shipping → invoke document-release
- Weekly retro → invoke retro
- Design system, brand → invoke design-consultation
- Visual audit, design polish → invoke design-review
- Architecture review → invoke plan-eng-review
- Save progress, checkpoint, resume → invoke checkpoint
- Code quality, health check → invoke health
