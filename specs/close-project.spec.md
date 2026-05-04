---
name: Close Project
description: Close all running PTY sessions of a project without deleting them from the database, mirroring app-exit behavior so sessions can be relaunched/resumed afterwards
targets:
  - ../src/store/projectStore.ts
  - ../src/components/Sidebar.tsx
  - ../src/__tests__/projectStore.test.ts
  - ../src/__tests__/Sidebar.test.tsx
---

# Close Project

Add a project-level "close" action that terminates all running PTY processes for the project but **keeps** their database rows intact. Mirrors what already happens on app exit (PTYs die, DB rows persist, statuses become `exited` on next launch). Lets the user clear out a project's running agents without losing the ability to relaunch/resume them.

## Motivation

Today the project header in `Sidebar.tsx` only offers `Remove project` (X icon → inline `Remove / Cancel`), which calls `killSession + delete_session + remove_project` — destructive: the project, all its sessions, agent_session_ids, and DB rows are gone. There is no "close" affordance equivalent to quitting the app.

Users running 5+ agents per project want to "park" everything in a project (free up CPU, stop network calls) without throwing away the conversation thread IDs that allow `Relaunch` to resume against the same Claude/Codex/OpenCode session. This spec fills that gap.

## Scope

In scope:
- New `closeProject(path)` action in `projectStore`.
- New `Close` button in the project header in `Sidebar.tsx` next to the existing `Remove` button.
- Inline two-step confirmation (`Close` → `Close / Cancel`), matching the existing `Remove project` pattern.
- Power-toggle icon for the new button.
- `Close` is **disabled** when the project has zero `running` sessions.
- Optimistic local status update to `exited`, reconciled by the existing `pty-exit` listener.

Out of scope:
- Per-session "close" affordance (existing `Kill` on a single session keeps its current `kill + delete` semantics).
- Closing all projects at once / global "park" action.
- Any new backend Tauri command — `kill_session` already does the right thing (kills PTY, leaves DB row, sets status to `exited` via the `pty-exit` flow).
- Persistence of "closed" project state — there is nothing to persist; closed simply means "all sessions in this project happen to be `exited`".
- Changes to task / worktree lifecycle. Closing the project does **not** touch worktrees, tasks, or `agent_session_id`.
- Keyboard shortcut. The action is rare enough that the button is sufficient.

## Behaviour

### What "close" does

For every session belonging to the project (`session.working_dir === path`) whose `status !== "exited"`:

1. Call `killSession(id)` (existing IPC → `kill_session` → `child.kill()` in the PTY).
2. Optimistically set `session.status = "exited"` and `session.status_line = ""` in `sessionStore` via the existing `handleExit(id)` reducer. This avoids a stale `running` flicker between the `kill` resolving and the `pty-exit` event arriving.
3. The reader thread observes EOF, emits `pty-exit`, frontend re-runs `handleExit` (idempotent — no double effect).
4. Backend's `notify_session_exit` updates the DB row to `status = 'exited'`. The row, `agent_session_id`, `worktree_path`, `base_commit`, and `initial_prompt` are all preserved.

### What "close" does NOT do

- Does **not** call `deleteSession`. DB rows survive.
- Does **not** call `remove_project`. Project survives in `projects` table.
- Does **not** clear `activeProjectPath`, `activeSessionId`, or `lastActiveSession`. The user stays where they were; the visible terminal just becomes an `exited` session card with the existing `Relaunch` affordance.
- Does **not** remove sessions from the `sessions[]` array in `sessionStore`. Same list, statuses flipped.

### Idempotency and race conditions

- Calling `closeProject` when no sessions are running is a no-op (button is disabled in that state, but the action handles it safely if called programmatically).
- A `pty-exit` event arriving for a session whose status is already `exited` is a no-op via the existing `handleExit` reducer.
- Concurrent `closeProject` calls for the same project are safe: the second call simply finds no `running` sessions to kill.

## UI

### Project header in `Sidebar.tsx`

Current layout (from line ~219 in `Sidebar.tsx`):

```
[ project name + git info ]                  [ X / Remove project ]
```

After the change:

```
[ project name + git info ]    [ ⏻ / Close project ] [ X / Remove project ]
```

The `Close` button sits **left of** the existing `Remove` button. Both share the same inline-confirm pattern so they behave consistently.

### Confirmation states

State machine (one piece of UI state per button — they are independent):

| `confirmCloseProject` | Render                                              |
| --------------------- | --------------------------------------------------- |
| `false`               | One ⏻ button. Click → set to `true`.                |
| `true`                | Two buttons inline: `Close` (confirm) + `Cancel`.   |

- Confirming `Close` while `confirmRemoveProject` is also `true` (or vice-versa) cancels the other to keep the UI from showing two confirmation rows at once.
- `Cancel` resets `confirmCloseProject` to `false` without side effects.
- Switching projects in `ProjectRail` while a confirmation is open should reset both `confirmCloseProject` and `confirmRemoveProject` to `false` (already the case for `confirmRemoveProject` because `Sidebar` re-renders on `activeProject` change, but verify the new state follows the same lifetime).

### Disabled state

The button is `disabled` when:

```ts
sessions.filter(s => s.working_dir === activeProjectPath && s.status !== "exited").length === 0
```

When disabled:
- `title="No running sessions"` (instead of `"Close project"`).
- Visual style: dimmed (CSS class to be added — match the existing `disabled` button conventions in `App.css`).
- Click handler is a no-op (`disabled` attribute is sufficient; no JS guard needed).

### Icon

Standard power-toggle glyph: a circle with a vertical line on top.

```svg
<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
  <path d="M18.36 6.64a9 9 0 1 1-12.73 0" />
  <line x1="12" y1="2" x2="12" y2="12" />
</svg>
```

Sizing matches the existing `project-remove-header-btn` (12×12).

### CSS

Two new classes added to `App.css` mirroring the existing remove pair:

- `.project-close-header-btn` — same baseline style as `.project-remove-header-btn`, neutral colour.
- `.project-close-confirm-btn` — same shape as `.project-remove-confirm-btn`, accent colour (`var(--accent)` rather than the destructive red used for Remove).
- Cancel button reuses `.project-remove-cancel-btn` style (or rename to a shared `.project-action-cancel-btn` if both use it — implementation-time choice).

## Implementation

### `src/store/projectStore.ts`

Add the new method to the `ProjectState` interface and the store body:

```ts
interface ProjectState {
  // ... existing fields ...
  closeProject: (path: string) => Promise<void>;
}
```

Implementation:

```ts
closeProject: async (path) => {
  const sessionStore = useSessionStore.getState();
  const running = sessionStore.sessions.filter(
    (s) => s.working_dir === path && s.status !== "exited"
  );
  if (running.length === 0) return;

  // Kill PTYs in parallel; do NOT call deleteSession.
  await Promise.all(
    running.map((s) =>
      killSession(s.id).catch((err) => {
        console.error(`[closeProject] killSession ${s.id} failed:`, err);
      })
    )
  );

  // Optimistic status update — pty-exit will re-run handleExit harmlessly.
  for (const s of running) {
    sessionStore.handleExit(s.id);
  }
},
```

Notes:
- `useSessionStore` import is added at the top of `projectStore.ts` (file already cross-references `sessionStore` via `replaceSessionInProject` callers, but the import is new — verify no circular-import pitfall during plan step).
- `killSession` failures are logged but do not abort the loop. A single failed kill should not leave the rest running.

### `src/components/Sidebar.tsx`

Changes:

1. Add state: `const [confirmCloseProject, setConfirmCloseProject] = useState(false);`.
2. Add handler `onCloseProject`:
   ```ts
   const onCloseProject = async () => {
     if (!activeProjectPath) return;
     if (!confirmCloseProject) {
       setConfirmCloseProject(true);
       setConfirmRemoveProject(false); // mutual exclusion
       return;
     }
     setConfirmCloseProject(false);
     try {
       await useProjectStore.getState().closeProject(activeProjectPath);
     } catch (err) {
       showToast(String(err), "error");
     }
   };
   ```
3. In the project header JSX, **before** the existing remove block, render the close block. Both blocks use the same confirm-row pattern, but only one row is visible at a time (because clicking one resets the other).
4. Compute `runningCount` for the active project and pass `disabled={runningCount === 0}` to the close button (when not in confirm state).
5. When the active project changes, reset `confirmCloseProject` (mirror the existing reset logic for `confirmRemoveProject`).

### Backend

No changes. Existing surface used:
- `kill_session(id)` — kills PTY, no DB delete.
- `pty-exit` event → `notify_session_exit` → DB `status = 'exited'`.

## Testing

`[@test] ../src/__tests__/projectStore.test.ts`

Add a new `describe("closeProject")` block:

- **Calls `killSession` for every running session, never `deleteSession`.**
  Seed `sessionStore` with three sessions for `/proj/a` (two `running`, one `exited`) and one session for `/proj/b` (`running`). Call `closeProject("/proj/a")`. Assert: `api.killSession` called twice with the two `running` ids, never with the `exited` id, never with the `/proj/b` id. `api.deleteSession` never called.
- **Optimistically marks running sessions as `exited`.**
  After the call, `sessionStore.sessions` for `/proj/a` all have `status === "exited"`. Sessions for `/proj/b` are untouched.
- **No-op when zero running sessions.**
  Project with only `exited` sessions → `api.killSession` not called, no state change.
- **Does not remove sessions from state.**
  `sessionStore.sessions.length` is unchanged.
- **Does not touch `activeProjectPath` / `activeSessionId`.**
  Snapshot before/after equality.
- **`api.removeProject` never called.**
  Distinguishes `closeProject` from `removeProject`.

`[@test] ../src/__tests__/Sidebar.test.tsx`

- **Renders the Close button when an active project exists.**
- **Click flow without running sessions:** the button is rendered with `disabled` and `title="No running sessions"`; clicking it does not enter confirm state.
- **Click flow with running sessions:**
  - First click → `Close` and `Cancel` buttons appear; the original Close button is hidden.
  - Click `Cancel` → confirm row disappears, no API call.
  - Click `Close` (confirm) → `closeProject` is invoked with the active project path, confirm row disappears.
- **Mutual exclusion:** opening the close-confirm row while remove-confirm is open hides the remove row, and vice versa.

## Non-functional notes

- **No new dependencies.** Reuses existing `kill_session` IPC, existing `handleExit` reducer, existing event flow.
- **Performance.** `Promise.all(killSession)` parallelises the kills; for typical N ≤ 10 the whole action completes in well under 100 ms.
- **Risk.** Low. `kill_session` is already widely used (`removeSession`, `removeProject`, replace-session in `addSession`). The only new behaviour is "do not call `deleteSession` after". DB integrity is unchanged because `delete_session` was the destructive part.
