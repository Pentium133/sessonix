# Close Project Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a project-level "close" action that kills all running PTY sessions of a project but keeps DB rows intact (mirrors app-exit semantics), so sessions can be relaunched/resumed afterwards.

**Architecture:** Frontend-only change. New `closeProject(path)` in `projectStore` calls existing `killSession` IPC for each running session in parallel, **does not** call `deleteSession`, then optimistically marks them `exited` via `sessionStore.handleExit`. UI adds a power-toggle button to the project header in `Sidebar.tsx` next to the existing Remove button, with the same inline confirm pattern (`Close → Close / Cancel`). No new Tauri commands.

**Tech Stack:** TypeScript, React 19, Zustand, Vitest + @testing-library/react, existing Tauri 2 IPC (`kill_session`).

**Spec:** `specs/close-project.spec.md`

---

## File Structure

- **Modify:** `src/store/projectStore.ts` — add `closeProject` action.
- **Modify:** `src/components/Sidebar.tsx` — add Close button, second confirm-state hook, mutual-exclusion with Remove confirm.
- **Modify:** `src/App.css` — add `.project-close-header-btn` and `.project-close-confirm-btn` styles.
- **Modify:** `src/__tests__/projectStore.test.ts` — `describe("closeProject")` block with the cases listed in the spec's Testing section.
- **Modify:** `src/__tests__/Sidebar.test.tsx` — `describe("Close project button")` block.

No new files. All work is contained to existing modules to match the project convention.

---

## Task 1: `closeProject` action in projectStore

**Files:**
- Modify: `src/store/projectStore.ts`
- Test: `src/__tests__/projectStore.test.ts`

- [ ] **Step 1.1: Add `deleteSession` to the api mock so it can be asserted-not-called**

In `src/__tests__/projectStore.test.ts`, replace the `vi.mock("../lib/api", ...)` block (currently lines 3-7) with:

```ts
vi.mock("../lib/api", () => ({
  addProject: vi.fn().mockResolvedValue(1),
  removeProject: vi.fn().mockResolvedValue(undefined),
  killSession: vi.fn().mockResolvedValue(undefined),
  deleteSession: vi.fn().mockResolvedValue(undefined),
  reorderProject: vi.fn().mockResolvedValue(undefined),
}));
```

(The added `reorderProject` mock matches the import already present in `projectStore.ts:7` and prevents an unrelated `undefined is not a function` crash if any future test path triggers `apiReorderProject`.)

- [ ] **Step 1.2: Write failing tests for `closeProject`**

Append a new `describe` block to `src/__tests__/projectStore.test.ts` (after the existing `describe("removeProject")` block, before `describe("addProject")`):

```ts
  describe("closeProject", () => {
    // Static import of useSessionStore at the top of the file would create
    // a circular dependency with projectStore in this test file's module
    // graph. Use an async import inside each test instead.
    async function loadSessionStore() {
      const mod = await import("../store/sessionStore");
      return mod.useSessionStore;
    }

    function makeRunning(id: number, path: string) {
      return {
        id,
        command: "claude",
        args: [],
        working_dir: path,
        task_name: `s${id}`,
        agent_type: "claude" as const,
        status: "running" as const,
        status_line: "doing things",
        created_at: 0,
        sortOrder: id,
        gitStatus: null,
        worktree_path: null,
        base_commit: null,
        initial_prompt: null,
        task_id: null,
      };
    }

    function makeExited(id: number, path: string) {
      return { ...makeRunning(id, path), status: "exited" as const, status_line: "" };
    }

    it("kills every running session of the project, never deleteSession", async () => {
      const useSessionStore = await loadSessionStore();
      useSessionStore.setState({
        sessions: [
          makeRunning(1, "/proj/a"),
          makeRunning(2, "/proj/a"),
          makeExited(3, "/proj/a"),
          makeRunning(4, "/proj/b"),
        ],
        activeSessionId: 1,
        loaded: true,
      });
      useProjectStore.setState({
        projects: [
          { path: "/proj/a", name: "a", sessions: [1, 2, 3] },
          { path: "/proj/b", name: "b", sessions: [4] },
        ],
        activeProjectPath: "/proj/a",
      });

      await useProjectStore.getState().closeProject("/proj/a");

      expect(api.killSession).toHaveBeenCalledTimes(2);
      expect(api.killSession).toHaveBeenCalledWith(1);
      expect(api.killSession).toHaveBeenCalledWith(2);
      expect(api.killSession).not.toHaveBeenCalledWith(3); // already exited
      expect(api.killSession).not.toHaveBeenCalledWith(4); // other project
      expect(api.deleteSession).not.toHaveBeenCalled();
      expect(api.removeProject).not.toHaveBeenCalled();
    });

    it("optimistically marks running sessions as exited", async () => {
      const useSessionStore = await loadSessionStore();
      useSessionStore.setState({
        sessions: [makeRunning(1, "/proj/a"), makeRunning(2, "/proj/a")],
        activeSessionId: 1,
        loaded: true,
      });
      useProjectStore.setState({
        projects: [{ path: "/proj/a", name: "a", sessions: [1, 2] }],
        activeProjectPath: "/proj/a",
      });

      await useProjectStore.getState().closeProject("/proj/a");

      const { sessions } = useSessionStore.getState();
      expect(sessions).toHaveLength(2);
      expect(sessions.find((s) => s.id === 1)?.status).toBe("exited");
      expect(sessions.find((s) => s.id === 2)?.status).toBe("exited");
      expect(sessions.find((s) => s.id === 1)?.status_line).toBe("");
    });

    it("is a no-op when no running sessions", async () => {
      const useSessionStore = await loadSessionStore();
      useSessionStore.setState({
        sessions: [makeExited(1, "/proj/a"), makeExited(2, "/proj/a")],
        activeSessionId: null,
        loaded: true,
      });
      useProjectStore.setState({
        projects: [{ path: "/proj/a", name: "a", sessions: [1, 2] }],
        activeProjectPath: "/proj/a",
      });

      await useProjectStore.getState().closeProject("/proj/a");

      expect(api.killSession).not.toHaveBeenCalled();
    });

    it("does not touch other projects' sessions", async () => {
      const useSessionStore = await loadSessionStore();
      useSessionStore.setState({
        sessions: [makeRunning(1, "/proj/a"), makeRunning(2, "/proj/b")],
        activeSessionId: 1,
        loaded: true,
      });
      useProjectStore.setState({
        projects: [
          { path: "/proj/a", name: "a", sessions: [1] },
          { path: "/proj/b", name: "b", sessions: [2] },
        ],
        activeProjectPath: "/proj/a",
      });

      await useProjectStore.getState().closeProject("/proj/a");

      const sb = useSessionStore.getState().sessions.find((s) => s.id === 2)!;
      expect(sb.status).toBe("running");
    });

    it("preserves activeProjectPath, activeSessionId, sessions array length", async () => {
      const useSessionStore = await loadSessionStore();
      useSessionStore.setState({
        sessions: [makeRunning(1, "/proj/a"), makeRunning(2, "/proj/a")],
        activeSessionId: 1,
        loaded: true,
      });
      useProjectStore.setState({
        projects: [{ path: "/proj/a", name: "a", sessions: [1, 2] }],
        activeProjectPath: "/proj/a",
        lastActiveSession: { "/proj/a": 1 },
      });

      await useProjectStore.getState().closeProject("/proj/a");

      const sStore = useSessionStore.getState();
      const pStore = useProjectStore.getState();
      expect(sStore.sessions).toHaveLength(2);
      expect(sStore.activeSessionId).toBe(1);
      expect(pStore.activeProjectPath).toBe("/proj/a");
      expect(pStore.lastActiveSession["/proj/a"]).toBe(1);
      expect(pStore.projects[0].sessions).toEqual([1, 2]);
    });
  });
```

- [ ] **Step 1.3: Run the new tests, verify they fail**

```bash
npm run test -- --run projectStore
```

Expected: failures referencing `closeProject is not a function` (the action does not exist yet).

- [ ] **Step 1.4: Implement `closeProject` in projectStore**

Open `src/store/projectStore.ts`. Make these edits:

1. At the top of the file, add an import for `useSessionStore` (after the existing imports, line 9):

```ts
import { useSessionStore } from "./sessionStore";
```

2. In the `ProjectState` interface (after line 30, in the CRUD block), add the new method signature:

```ts
  closeProject: (path: string) => Promise<void>;
```

3. In the store body, after the `removeProject` implementation (after line 100, before `ensureProject`), add the implementation:

```ts
  closeProject: async (path) => {
    const sessionStore = useSessionStore.getState();
    const running = sessionStore.sessions.filter(
      (s) => s.working_dir === path && s.status !== "exited"
    );
    if (running.length === 0) return;

    await Promise.all(
      running.map((s) =>
        killSession(s.id).catch((err) => {
          console.error(`[closeProject] killSession ${s.id} failed:`, err);
        })
      )
    );

    for (const s of running) {
      sessionStore.handleExit(s.id);
    }
  },
```

- [ ] **Step 1.5: Run the tests again, verify they pass**

```bash
npm run test -- --run projectStore
```

Expected: all `closeProject` tests pass; existing `projectStore` tests still pass.

- [ ] **Step 1.6: Run typecheck**

```bash
npm run typecheck
```

Expected: zero errors. The `useSessionStore` import is the only new symbol; no circular-import error should appear at compile time because `sessionStore` already imports from `projectStore`. (Zustand's `getState()` is called inside the closure, lazily, so the runtime cycle is broken — same pattern as the existing `addSession` in `sessionStore.ts:152`.)

- [ ] **Step 1.7: Commit**

```bash
git add src/store/projectStore.ts src/__tests__/projectStore.test.ts
git commit -m "$(cat <<'EOF'
feat(project): add closeProject action

Kill PTYs of all running sessions in a project without deleting their
DB rows. Mirrors app-exit semantics so sessions remain in the list and
can be relaunched/resumed.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Close button in Sidebar with confirmation

**Files:**
- Modify: `src/components/Sidebar.tsx`
- Test: `src/__tests__/Sidebar.test.tsx`

- [ ] **Step 2.1: Write failing tests for the Close button**

Append a new `describe` block to `src/__tests__/Sidebar.test.tsx` after the `describe("Fork button", ...)` block (i.e. just before the final `});` on line 395):

```ts
  describe("Close project button", () => {
    function setupWithRunning(runningCount: number) {
      const projects: Project[] = [
        { path: "/tmp/app", name: "app", sessions: Array.from({ length: runningCount + 1 }, (_, i) => i + 1) },
      ];
      const sessions: Session[] = Array.from({ length: runningCount + 1 }, (_, i) => ({
        id: i + 1,
        command: "claude",
        args: [],
        working_dir: "/tmp/app",
        task_name: `Session ${i + 1}`,
        agent_type: "claude" as const,
        status: i < runningCount ? "running" as const : "exited" as const,
        status_line: "",
        created_at: Date.now(),
        sortOrder: i + 1,
        gitStatus: null,
        worktree_path: null,
        base_commit: null,
        initial_prompt: null,
        task_id: null,
      }));
      setupStores(projects, sessions);
    }

    it("renders the close button when an active project exists", () => {
      setupWithRunning(1);
      const { container } = render(<Sidebar />);
      expect(container.querySelector(".project-close-header-btn")).toBeTruthy();
    });

    it("disables the button when there are no running sessions", () => {
      setupWithRunning(0); // 0 running + 1 exited
      const { container } = render(<Sidebar />);
      const btn = container.querySelector(".project-close-header-btn") as HTMLButtonElement | null;
      expect(btn).toBeTruthy();
      expect(btn!.disabled).toBe(true);
      expect(btn!.title).toBe("No running sessions");
    });

    it("first click shows Close / Cancel confirm row, hides original button", () => {
      setupWithRunning(2);
      const { container } = render(<Sidebar />);
      const btn = container.querySelector(".project-close-header-btn") as HTMLButtonElement;
      fireEvent.click(btn);
      expect(container.querySelector(".project-close-header-btn")).toBeNull();
      expect(container.querySelector(".project-close-confirm-btn")).toBeTruthy();
      expect(container.querySelector(".project-close-cancel-btn")).toBeTruthy();
    });

    it("Cancel restores the close button without calling closeProject", () => {
      const closeProject = vi.fn().mockResolvedValue(undefined);
      setupWithRunning(2);
      useProjectStore.setState({ closeProject });
      const { container } = render(<Sidebar />);
      fireEvent.click(container.querySelector(".project-close-header-btn")!);
      fireEvent.click(container.querySelector(".project-close-cancel-btn")!);
      expect(container.querySelector(".project-close-header-btn")).toBeTruthy();
      expect(container.querySelector(".project-close-confirm-btn")).toBeNull();
      expect(closeProject).not.toHaveBeenCalled();
    });

    it("confirm Close calls closeProject with the active project path", () => {
      const closeProject = vi.fn().mockResolvedValue(undefined);
      setupWithRunning(2);
      useProjectStore.setState({ closeProject });
      const { container } = render(<Sidebar />);
      fireEvent.click(container.querySelector(".project-close-header-btn")!);
      fireEvent.click(container.querySelector(".project-close-confirm-btn")!);
      expect(closeProject).toHaveBeenCalledWith("/tmp/app");
    });

    it("opening close confirm cancels an open remove confirm", () => {
      setupWithRunning(2);
      const { container } = render(<Sidebar />);
      // Open Remove confirm first
      fireEvent.click(container.querySelector(".project-remove-header-btn")!);
      expect(container.querySelector(".project-remove-confirm-btn")).toBeTruthy();
      // Open Close confirm — should hide Remove confirm
      fireEvent.click(container.querySelector(".project-close-header-btn")!);
      expect(container.querySelector(".project-close-confirm-btn")).toBeTruthy();
      expect(container.querySelector(".project-remove-confirm-btn")).toBeNull();
    });

    it("opening remove confirm cancels an open close confirm", () => {
      setupWithRunning(2);
      const { container } = render(<Sidebar />);
      fireEvent.click(container.querySelector(".project-close-header-btn")!);
      expect(container.querySelector(".project-close-confirm-btn")).toBeTruthy();
      fireEvent.click(container.querySelector(".project-remove-header-btn")!);
      expect(container.querySelector(".project-remove-confirm-btn")).toBeTruthy();
      expect(container.querySelector(".project-close-confirm-btn")).toBeNull();
    });
  });
```

- [ ] **Step 2.2: Run the new tests, verify they fail**

```bash
npm run test -- --run Sidebar
```

Expected: failures because `.project-close-header-btn` does not exist in the rendered DOM yet.

- [ ] **Step 2.3: Implement Close button in Sidebar**

Open `src/components/Sidebar.tsx`.

**Edit A — add new state next to `confirmRemoveProject` (after line 32):**

```tsx
  const [confirmCloseProject, setConfirmCloseProject] = useState(false);
```

**Edit B — add `runningCount` derivation right after `projectSessions` is computed (after line 63):**

```tsx
  const runningCount = projectSessions.filter((s) => s.status !== "exited").length;
```

**Edit C — add `onCloseProject` handler. Place it directly after `onRemoveProject` (which ends at line 165):**

```tsx
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

**Edit D — make `onRemoveProject` cancel any open close-confirm.** Modify the existing handler (around line 150) so the first-click branch resets `confirmCloseProject`:

Original:
```tsx
    if (!confirmRemoveProject) {
      setConfirmRemoveProject(true);
      return;
    }
```

Replace with:
```tsx
    if (!confirmRemoveProject) {
      setConfirmRemoveProject(true);
      setConfirmCloseProject(false); // mutual exclusion
      return;
    }
```

**Edit E — render the Close button block in the project header.** In the JSX (around lines 219-247), replace the existing `{confirmRemoveProject ? (…) : (…)}` block with the following block that handles **both** confirms (only one can be open at a time, mutual exclusion is enforced by the handlers):

Original (lines 219-247):
```tsx
          {confirmRemoveProject ? (
            <>
              <button
                className="project-remove-confirm-btn"
                onClick={onRemoveProject}
                title="Confirm remove"
              >
                Remove
              </button>
              <button
                className="project-remove-cancel-btn"
                onClick={() => setConfirmRemoveProject(false)}
                title="Cancel"
              >
                Cancel
              </button>
            </>
          ) : (
            <button
              className="project-remove-header-btn"
              onClick={onRemoveProject}
              title="Remove project"
            >
              <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
                <line x1="2" y1="2" x2="10" y2="10" />
                <line x1="10" y1="2" x2="2" y2="10" />
              </svg>
            </button>
          )}
```

Replace with:
```tsx
          {confirmCloseProject ? (
            <>
              <button
                className="project-close-confirm-btn"
                onClick={onCloseProject}
                title="Confirm close"
              >
                Close
              </button>
              <button
                className="project-close-cancel-btn"
                onClick={() => setConfirmCloseProject(false)}
                title="Cancel"
              >
                Cancel
              </button>
            </>
          ) : confirmRemoveProject ? (
            <>
              <button
                className="project-remove-confirm-btn"
                onClick={onRemoveProject}
                title="Confirm remove"
              >
                Remove
              </button>
              <button
                className="project-remove-cancel-btn"
                onClick={() => setConfirmRemoveProject(false)}
                title="Cancel"
              >
                Cancel
              </button>
            </>
          ) : (
            <>
              <button
                className="project-close-header-btn"
                onClick={onCloseProject}
                disabled={runningCount === 0}
                title={runningCount === 0 ? "No running sessions" : "Close project (kill all running sessions, keep history)"}
              >
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M18.36 6.64a9 9 0 1 1-12.73 0" />
                  <line x1="12" y1="2" x2="12" y2="12" />
                </svg>
              </button>
              <button
                className="project-remove-header-btn"
                onClick={onRemoveProject}
                title="Remove project"
              >
                <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
                  <line x1="2" y1="2" x2="10" y2="10" />
                  <line x1="10" y1="2" x2="2" y2="10" />
                </svg>
              </button>
            </>
          )}
```

**Edit F — reset both confirm states when the active project changes.** Add this `useEffect` near the other `useEffect`s in the component body (after the existing `useEffect` at line 56-58):

```tsx
  useEffect(() => {
    setConfirmCloseProject(false);
    setConfirmRemoveProject(false);
  }, [activeProjectPath]);
```

- [ ] **Step 2.4: Run the tests again, verify they pass**

```bash
npm run test -- --run Sidebar
```

Expected: all new `Close project button` tests pass. Existing `Sidebar` tests still pass (the project-remove DOM is unchanged when neither confirm is open).

- [ ] **Step 2.5: Run typecheck**

```bash
npm run typecheck
```

Expected: zero errors.

- [ ] **Step 2.6: Commit**

```bash
git add src/components/Sidebar.tsx src/__tests__/Sidebar.test.tsx
git commit -m "$(cat <<'EOF'
feat(sidebar): add Close project button

Power-toggle button next to the existing Remove button. Inline
Close/Cancel confirm pattern, mutually exclusive with the Remove
confirm row. Disabled when the project has zero running sessions.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: CSS for the Close button

**Files:**
- Modify: `src/App.css`

- [ ] **Step 3.1: Add the new CSS classes**

Open `src/App.css`. Locate the existing `.project-remove-header-btn` block (line 355). Append the following blocks immediately after `.project-remove-cancel-btn:hover { … }` (line 403):

```css
.project-close-header-btn {
  background: none;
  border: none;
  color: var(--text-dim);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: var(--radius);
  opacity: 0;
  transition: opacity 0.15s, color 0.15s, background 0.15s;
}

.sidebar-header:hover .project-close-header-btn {
  opacity: 1;
}

.project-close-header-btn:hover:not(:disabled) {
  color: var(--accent);
  background: var(--hover);
}

.project-close-header-btn:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.sidebar-header:hover .project-close-header-btn:disabled {
  opacity: 0.3;
}

.project-close-confirm-btn {
  background: var(--accent);
  border: none;
  color: var(--bg);
  font-size: 10px;
  padding: 3px 8px;
  border-radius: 4px;
  cursor: pointer;
  font-weight: 600;
}

.project-close-cancel-btn {
  background: none;
  border: 1px solid var(--border);
  color: var(--text-dim);
  font-size: 10px;
  padding: 3px 8px;
  border-radius: 4px;
  cursor: pointer;
}

.project-close-cancel-btn:hover {
  color: var(--text);
  border-color: var(--text-dim);
}
```

Two notes:
- The `--accent` token is already used by other action surfaces (see `DESIGN.md` and existing accent usages in `App.css`); reusing it keeps the Close button visually distinct from the destructive red of Remove.
- The `disabled` state explicitly overrides the parent `:hover` rule that would otherwise still bump `opacity` to 1, keeping the button visibly inactive even when the user hovers the header.

- [ ] **Step 3.2: Verify nothing broke**

```bash
npm run typecheck && npm run test -- --run
```

Expected: typecheck clean; full test suite green.

- [ ] **Step 3.3: Commit**

```bash
git add src/App.css
git commit -m "$(cat <<'EOF'
style(sidebar): add Close project button styles

Match the Remove button structure but use --accent (non-destructive)
for the confirm state, and add a dimmed disabled state that survives
the parent header :hover opacity bump.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Manual smoke verification

**No file changes — this task validates that the wired behaviour matches the spec end-to-end.**

- [ ] **Step 4.1: Launch the dev app**

```bash
npm run tauri dev
```

Wait for the window to open.

- [ ] **Step 4.2: Set up a project with running sessions**

In the running app:
1. Open or add a project (Cmd+Shift+K).
2. Launch 2 sessions in that project (Cmd+Shift+T → pick any agent or `shell`). Confirm both show as `running` in the Sidebar.

- [ ] **Step 4.3: Verify the Close button is present and enabled**

Hover the project header. The power-toggle (⏻) button is visible to the **left** of the X (Remove) button. Tooltip says "Close project (kill all running sessions, keep history)".

- [ ] **Step 4.4: Verify Cancel does nothing**

Click ⏻. Confirm row appears with `Close / Cancel`. Click `Cancel`. The ⏻ button reappears, both sessions are still `running`.

- [ ] **Step 4.5: Verify Close kills PTYs but keeps DB rows**

Click ⏻ → click `Close`. Within ~100 ms:
- Both session cards remain in the list.
- Status indicators flip to `exited` (greyed out).
- A `Relaunch` action becomes available on each card.
- The active terminal stops accepting input (PTY is dead).

- [ ] **Step 4.6: Verify Close is now disabled**

Hover the project header. The ⏻ button is dimmed (`disabled`). Tooltip says "No running sessions". Clicking does nothing (no confirm row appears).

- [ ] **Step 4.7: Verify Relaunch resumes against the same agent thread**

Click `Relaunch` on one of the `exited` sessions. A new PTY spawns; for Claude/Codex/OpenCode the agent should pick up the same thread (same `agent_session_id` carried through). Confirm by typing a follow-up that references prior context.

- [ ] **Step 4.8: Verify mutual exclusion in UI**

With at least one `running` session:
1. Click X (Remove). The Remove/Cancel confirm row appears.
2. Without confirming, click ⏻. Now the Close/Cancel confirm row replaces the Remove/Cancel one (only one row visible at a time).
3. Click `Cancel`. Plain header buttons return.

- [ ] **Step 4.9: Verify app restart preserves closed sessions**

While at least one session is in `exited` state from Step 4.5:
1. Close and reopen the app (Cmd+Q → relaunch).
2. The exited session is still in the project, still has its `task_name`, and `Relaunch` still works.

- [ ] **Step 4.10: If everything passes, no commit needed**

This task creates no files. If anything failed, debug, fix, commit the fix, and re-run from Step 4.1.

---

## Self-Review

**Spec coverage check:**
- "kill PTYs of all running sessions, do not call deleteSession" → Task 1 (Steps 1.2, 1.4) and verified in Step 4.5.
- "optimistic local status update via handleExit" → Step 1.4 implementation, Step 1.2 test.
- "no new backend command" → backend untouched in plan.
- "Close button left of Remove, power-toggle icon" → Step 2.3 Edit E.
- "inline two-step confirmation" → Step 2.3 Edit C and Edit E, tested in 2.1.
- "disabled when 0 running sessions" → Step 2.3 Edit B + Edit E, tested in 2.1.
- "mutual exclusion of close/remove confirms" → Step 2.3 Edits C, D; tested in 2.1.
- "reset confirm states on active project change" → Step 2.3 Edit F.
- "no changes to activeProjectPath / activeSessionId / lastActiveSession" → Step 1.2 final test asserts this.
- "agent_session_id and worktree_path preserved across close/relaunch" → manual Step 4.7.
- CSS classes (`.project-close-header-btn`, `.project-close-confirm-btn`, plus disabled state) → Task 3.

**Placeholder scan:** no TBD/TODO/"add appropriate". Every code edit shows the actual code. Every test step shows the test body.

**Type/name consistency:** `closeProject` is the same name in store interface (Step 1.4 #2), implementation (#3), Sidebar handler (Step 2.3 Edit C), and test mocks (Step 2.1). `confirmCloseProject` consistent across Edits A/C/D/E/F. CSS class names referenced in tests (Step 2.1) match those rendered in JSX (Step 2.3 Edit E) and defined in CSS (Step 3.1).
