---
name: Git Diff Viewer
description: View uncommitted working-tree diff of an active project or session worktree in a read-only unified (line-by-line) UI
targets:
  - ../src-tauri/src/diff_manager.rs
  - ../src-tauri/src/lib.rs
  - ../src/components/DiffViewer.tsx
  - ../src/components/DiffFileList.tsx
  - ../src/components/DiffFilePane.tsx
  - ../src/components/SummaryBar.tsx
  - ../src/hooks/useWorktreeDiff.ts
  - ../src/store/sessionStore.ts
  - ../src/lib/api.ts
  - ../src/App.tsx
---

# Git Diff Viewer

Read-only **unified (line-by-line)** diff of the current **working tree vs `HEAD`** for the active project or the worktree of a previously-selected session. Surfaced via a persistent "Diff" pseudo-session pinned to the right of the `SummaryBar`.

## Scope

In scope:
- Compute diff for a given `working_dir` (`git diff` + `git diff --staged` + untracked, unified model: working tree vs `HEAD`).
- Show results in a split-view UI (old | new), with a file list on the left.
- One "Diff" pseudo-session per active project, activated by tab click or `Cmd+0`.
- Rename detection with 50% similarity threshold.
- Size limits: 1 MB or 5000 lines per file → `TooLarge` stub; 500 files max per request.
- Binary files → `Binary` stub.
- Untracked files → shown as `Added` with empty `old_content`.
- Read-only. No stage / unstage / commit / discard actions.

Out of scope:
- Interactive staging, hunk-level operations, commit authoring.
- Diff against branches other than `HEAD` (no `vs main`, no branch picker).
- Side-by-side comparison with historic commits.
- Per-session Terminal ↔ Diff tabbing inside one session (the pseudo-session replaces per-session tabbing).
- Editing files from the diff viewer.

## Backend API

Single Tauri IPC command in `src-tauri/src/lib.rs`, backed by `diff_manager.rs`:

```rust
pub struct WorktreeDiff {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub head_sha: Option<String>,          // first 7 chars
    pub files: Vec<DiffFile>,
    pub truncated_files: u32,              // how many files hidden past the 500 cap
}

pub struct DiffFile {
    pub old_path: String,                  // "" when status == Added
    pub new_path: String,                  // "" when status == Deleted
    pub status: DiffStatus,
    pub additions: u32,                    // hunk line counts (`git diff` stats)
    pub deletions: u32,
    pub payload: DiffPayload,
}

pub enum DiffStatus { Added, Modified, Deleted, Renamed }

pub enum DiffPayload {
    Text { old_content: String, new_content: String },
    Binary,
    TooLarge { size_bytes: u64 },
}

#[tauri::command]
async fn get_worktree_diff(working_dir: String) -> Result<WorktreeDiff, String>;
```

Implementation uses `git2::Repository::discover` + `diff_index_to_workdir` with `DiffOptions::include_untracked(true).recurse_untracked_dirs(true)` and `DiffFindOptions::renames(true).rename_threshold(50)`. The command is `async` and wraps blocking git2 calls in `tauri::async_runtime::spawn_blocking`.

`[@test] ../src-tauri/src/diff_manager.rs` (inline `#[cfg(test)] mod tests`)

## Diff computation rules

- Base = `HEAD`. Target = working tree (includes staged + unstaged + untracked).
  `[@test] ../src-tauri/src/diff_manager.rs::test_modified_file`
- Each `DiffFile` reports per-file hunk line stats: `additions` (lines added, `+`) and `deletions` (lines removed, `−`), derived from `git2::Patch::line_stats()`. Binary and too-large files still carry whatever stats git2 produced (may be zero). The file list UI renders these alongside the path.
  `[@test] ../src-tauri/src/diff_manager.rs::test_modified_file`
- A directory that is not a git repo (no parent `.git`) returns `WorktreeDiff { is_repo: false, .. }` with `files` empty.
  `[@test] ../src-tauri/src/diff_manager.rs::test_non_git_dir`
- A clean working tree returns `is_repo: true`, empty `files`, `truncated_files: 0`.
  `[@test] ../src-tauri/src/diff_manager.rs::test_empty_diff`
- Untracked files are included as `Added` with `old_content = ""` and `new_content` = full file contents.
  `[@test] ../src-tauri/src/diff_manager.rs::test_added_untracked`
- Deleted files set `new_path = ""`, `old_content` = pre-delete contents, `new_content = ""`.
  `[@test] ../src-tauri/src/diff_manager.rs::test_deleted_file`
- File renames with ≥50% content similarity are reported as a single `Renamed` entry with `old_path != new_path`.
  `[@test] ../src-tauri/src/diff_manager.rs::test_rename_detected`
- Moves with <50% similarity fall back to a `Deleted` entry + an `Added` entry.
  `[@test] ../src-tauri/src/diff_manager.rs::test_rename_below_threshold`

## File size and count limits

- A file whose byte size (old or new, whichever exists) exceeds **1 048 576 bytes** OR whose line count exceeds **5000** returns `DiffPayload::TooLarge { size_bytes }` — no text content is read.
  `[@test] ../src-tauri/src/diff_manager.rs::test_large_file`
- Files that git2 classifies as binary return `DiffPayload::Binary`. No content is returned.
  `[@test] ../src-tauri/src/diff_manager.rs::test_binary_file`
- If the total changed-files count exceeds **500**, only the first 500 (in git2 delta order) are returned; the remainder is counted into `truncated_files`.
  `[@test] ../src-tauri/src/diff_manager.rs::test_500_file_limit`

## UI: pseudo-session integration

```ts
export const DIFF_PSEUDO_ID = 0;
```

`sessionStore` extensions:

```ts
interface SessionStore {
  // … existing
  lastFocusedSessionIdByProject: Record<string, number>;
  switchSession: (id: number) => void;
}
```

- Real PTY ids are always ≥ 1, so `0` is reserved for the Diff pseudo-session.
  `[@test] ../src/__tests__/sessionStore.diff-pseudo.test.ts::test_reserved_id`
- When `switchSession(id)` is called with a real session id inside an active project, the store records `lastFocusedSessionIdByProject[activeProjectPath] = id` **before** updating `activeSessionId`.
  `[@test] ../src/__tests__/sessionStore.diff-pseudo.test.ts::test_last_focused_tracked`
- When `switchSession(DIFF_PSEUDO_ID)` is called, `activeSessionId` becomes `0`; nothing is written to `lastFocusedSessionIdByProject`.
  `[@test] ../src/__tests__/sessionStore.diff-pseudo.test.ts::test_switch_to_diff`

## UI: SummaryBar — Diff button

`SummaryBar` renders running session tabs on the left and a persistent "Diff" button pinned to the right, visually separated by a left border and auto margin.

```tsx
<div className="summary-bar">
  {/* …session buttons… */}
  <button
    className={`summary-item summary-diff-btn ${activeSessionId === DIFF_PSEUDO_ID ? "active" : ""}`}
    onClick={() => switchSession(DIFF_PSEUDO_ID)}
    aria-label="Show diff"
  >
    <DiffIcon size={16} />
    <span>Diff</span>
  </button>
</div>
```

- The Diff button is always rendered whenever `SummaryBar` is rendered — even when `running.length === 0`. The current `if (running.length === 0) return null;` short-circuit is removed.
  `[@test] ../src/__tests__/SummaryBar.diff-button.test.tsx::test_visible_with_zero_sessions`
- The Diff button is visually separated from session tabs via `border-left: 1px solid var(--border); margin-left: auto; padding-left: 12px;`.
  `[@test] ../src/__tests__/SummaryBar.diff-button.test.tsx::test_divider_rendered`
- Clicking the Diff button calls `switchSession(DIFF_PSEUDO_ID)` and sets its `.active` class when `activeSessionId === 0`.
  `[@test] ../src/__tests__/SummaryBar.diff-button.test.tsx::test_click_activates`

## UI: DiffViewer component

`DiffViewer` is mounted in the content area whenever `activeSessionId === DIFF_PSEUDO_ID`. Unlike `TerminalPane` (which uses a pool and `display:none` to stay alive), `DiffViewer` is conditionally rendered: switching away unmounts it, switching back remounts it and triggers a fresh fetch. It derives its `workingDir` as:

```
target = lastFocusedSessionIdByProject[activeProjectPath]
  ? sessions.find(s => s.id === that).working_dir
  : activeProjectPath
```

- When no session has been focused yet in the active project, `DiffViewer` targets `activeProjectPath`.
  `[@test] ../src/__tests__/DiffViewer.target-resolution.test.tsx::test_defaults_to_project_root`
- When a session has previously been focused, `DiffViewer` targets that session's `working_dir`.
  `[@test] ../src/__tests__/DiffViewer.target-resolution.test.tsx::test_uses_last_focused`

### Layout

Two-column flex layout:
- Left column: `DiffFileList`, fixed 280 px width, scrollable, shows status icon + path + `+N / −M` counters per file.
- Right column: `DiffFilePane`, fills remaining width, renders `react-diff-viewer-continued` in `splitView: false` (unified, line-by-line) mode for the currently-selected file.

### States

- **Loading** — fetch inflight AND elapsed time > 500 ms → skeleton placeholder (grey bars matching list/pane geometry).
  `[@test] ../src/__tests__/DiffViewer.skeleton.test.tsx::test_skeleton_after_500ms`
- **Not a git repo** — `is_repo === false` → centered message `"Not a git repository"`.
  `[@test] ../src/__tests__/DiffViewer.empty-state.test.tsx::test_non_repo_state`
- **No changes** — `is_repo === true && files.length === 0` → centered `"No changes"` + branch name + 7-char SHA.
  `[@test] ../src/__tests__/DiffViewer.empty-state.test.tsx::test_no_changes_state`
- **IPC error** — rejection from `get_worktree_diff` → centered `"Error: <msg>"` + a `Retry` button that re-invokes the fetch.
  `[@test] ../src/__tests__/DiffViewer.error-state.test.tsx::test_error_with_retry`
- **Truncation banner** — `truncated_files > 0` → sticky banner above the file list: `"N more files hidden — reduce the scope of your changes"`.
  `[@test] ../src/__tests__/DiffViewer.truncation-banner.test.tsx::test_banner_when_truncated`

### File selection

- Selecting a file in `DiffFileList` sets the rendered file in `DiffFilePane`. Selection state lives in component state, not the global store.
  `[@test] ../src/__tests__/DiffViewer.file-list.test.tsx::test_click_selects_file`
- When the file list is non-empty and nothing is selected (initial render), the first file is auto-selected.
  `[@test] ../src/__tests__/DiffViewer.file-list.test.tsx::test_auto_select_first`
- Selecting a `Binary` file renders `"Binary file — contents not shown"` in the pane.
  `[@test] ../src/__tests__/DiffViewer.file-list.test.tsx::test_binary_pane`
- Selecting a `TooLarge` file renders `"File too large (N KB) — not displayed"` with the size formatted via `Intl.NumberFormat`.
  `[@test] ../src/__tests__/DiffViewer.file-list.test.tsx::test_too_large_pane`

### Refresh

- A `Refresh` button in the pane header re-invokes `get_worktree_diff` with the same `working_dir`.
  `[@test] ../src/__tests__/DiffViewer.refresh.test.tsx::test_refresh_refetches`
- The fetch runs exactly once on mount of `DiffViewer`; it does **not** auto-refresh on a timer or on filesystem events.
  `[@test] ../src/__tests__/DiffViewer.refresh.test.tsx::test_no_auto_refresh`

## Keyboard shortcut

`Cmd+0` (Mac) / `Ctrl+0` (others) in `App.tsx` global keydown handler invokes `switchSession(DIFF_PSEUDO_ID)`. Preserves `Cmd+1..9` for real sessions.

- Pressing `Cmd+0` with an active project switches to the Diff pseudo-session.
  `[@test] ../src/__tests__/App.keyboard.test.tsx::test_cmd_0_switches_to_diff`
- Pressing `Cmd+0` with no active project is a no-op.
  `[@test] ../src/__tests__/App.keyboard.test.tsx::test_cmd_0_noop_without_project`

## Persistence of terminal state across Diff switches

`TerminalPane` stays mounted for the entire lifetime of the content area and uses `display:none` to hide itself while the Diff pseudo-session is active. It is **never conditionally unmounted** based on `activeSessionId`, because unmounting the pane destroys the container div that xterm instances in `terminalPool` were `terminal.open()`-ed against — subsequent remount leaves those instances orphaned (no visible DOM, empty terminal on return from Diff). This applies uniformly to every agent type (shell, Claude, Codex, Gemini, custom): the pool is agent-agnostic, the fix protects all of them.

- `TerminalPane` is rendered unconditionally as a sibling of `DiffViewer` in `App.tsx`; only its `display` CSS property toggles based on whether `activeSessionId === DIFF_PSEUDO_ID`. `DiffViewer` itself may still be conditionally rendered — it holds no expensive state.
  `[@test] ../src/__tests__/App.diff-switch.test.tsx::test_terminal_pane_always_mounted`
- Switching from any real session to the Diff pseudo-session and back restores the terminal's on-screen contents (scrollback, cursor position, PTY output buffered via `ring_buffer` during detach) identically to switching between two real sessions.
  `[@test] ../src/__tests__/App.diff-switch.test.tsx::test_terminal_persists_across_diff_switch`

## Session launch updates last-focused tracking

When `sessionStore.addSession` creates a new session (fresh launch or relaunch with `replaceId`) and promotes it to `activeSessionId`, it must also write `lastActiveSession[working_dir] = newId` on `projectStore` — mirroring what `switchSession` already does for existing sessions. Without this write, the Diff pseudo-session cannot resolve the worktree of a freshly-launched task session: `resolveWorkingDir` in `DiffViewer` falls back to `activeProjectPath` (the main checkout) and renders a diff of the wrong branch.

- After `addSession` resolves, `useProjectStore.getState().lastActiveSession[working_dir]` equals the new session id.
  `[@test] ../src/__tests__/sessionStore.addSession.test.ts::test_addSession_writes_lastActive`
- Switching to Diff immediately after launching a worktree-backed session targets `session.worktree_path`, not the project root.
  `[@test] ../src/__tests__/DiffViewer.target-resolution.test.tsx::test_targets_worktree_after_launch`

## Error handling

- All `git2` errors surface as `Result::Err(String)` from the IPC command and are rendered in the `DiffViewer` error state (no toast).
- Invalid `working_dir` (does not exist / not a directory) returns `Err("working dir not found: <path>")`.
  `[@test] ../src-tauri/src/diff_manager.rs::test_invalid_working_dir`

## Non-functional requirements

- Blocking git2 operations run in `tauri::async_runtime::spawn_blocking` to keep the Tauri IPC runtime responsive.
- `DiffViewer` does not poll — no interval timers, no `useStatusPolling` integration.
- Styling follows `DESIGN.md` tokens (`--bg`, `--surface`, `--border`, `--accent`, status colors reused from sidebar git badges: `--sidebar-git-added`, `--sidebar-git-modified`, `--sidebar-git-deleted`).
- New npm dependency: `react-diff-viewer-continued` (MIT).

## Build sequence

1. `diff_manager.rs` with inline `#[cfg(test)]` tests; `cargo test -p aicoder` passes.
2. Register `get_worktree_diff` in `lib.rs` `generate_handler!` list; add typed wrapper `getWorktreeDiff` in `src/lib/api.ts`.
3. Extend `sessionStore` with `DIFF_PSEUDO_ID`, `lastFocusedSessionIdByProject`, updated `switchSession`.
4. `useWorktreeDiff(workingDir)` hook — fetch + loading/error state + `refresh()`.
5. `npm i react-diff-viewer-continued`.
6. `DiffFileList`, `DiffFilePane`, `DiffViewer` components + styles in `App.css`.
7. `SummaryBar` — remove the `running.length === 0` short-circuit, add the pinned Diff button + divider.
8. `App.tsx` — render `<DiffViewer />` when `activeSessionId === DIFF_PSEUDO_ID`; wire `Cmd+0` handler.
9. Vitest suites for each `[@test]` link above.
10. Manual QA: `npm run tauri dev` — verify Diff button visible, Cmd+0 works, diff renders for project root and for a selected session's worktree, empty/loading/error states, untracked file appears as Added, large file shows stub.
