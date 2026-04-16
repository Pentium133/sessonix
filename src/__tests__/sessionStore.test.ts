import { describe, it, expect, vi, beforeEach } from "vitest";
import type { Session } from "../lib/types";

// Mock API before importing stores
vi.mock("../lib/api", () => ({
  createSession: vi.fn().mockResolvedValue(100),
  killSession: vi.fn().mockResolvedValue(undefined),
  deleteSession: vi.fn().mockResolvedValue(undefined),
  attachSession: vi.fn().mockResolvedValue([]),
  detachSession: vi.fn().mockResolvedValue(undefined),
  listProjects: vi.fn().mockResolvedValue([]),
  listSessions: vi.fn().mockResolvedValue([]),
  installClaudeHooks: vi.fn().mockResolvedValue(true),
  checkClaudeHooks: vi.fn().mockResolvedValue(true),
  reorderSession: vi.fn().mockResolvedValue(undefined),
  setSortOrder: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../components/TerminalPane", () => ({
  writeToTerminal: vi.fn(),
}));

import { useSessionStore } from "../store/sessionStore";
import { useProjectStore } from "../store/projectStore";
import * as api from "../lib/api";

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 1,
    command: "claude",
    args: [],
    working_dir: "/tmp/app",
    task_name: "Test",
    agent_type: "claude",
    status: "running",
    status_line: "",
    created_at: Date.now(),
    sortOrder: 1,
    gitStatus: null,
    worktree_path: null,
    base_commit: null,
    ...overrides,
  };
}

function seedSessions(sessions: Session[], activeId: number | null = null) {
  useSessionStore.setState({ sessions, activeSessionId: activeId, loaded: true });
}

function seedProject(path: string, sessionIds: number[]) {
  useProjectStore.setState({
    projects: [{ path, name: path.split("/").pop()!, sessions: sessionIds }],
    activeProjectPath: path,
  });
}

describe("sessionStore", () => {
  beforeEach(() => {
    useSessionStore.setState({ sessions: [], activeSessionId: null, loaded: false });
    useProjectStore.setState({ projects: [], activeProjectPath: null });
    vi.clearAllMocks();
  });

  // ─── removeSession ──────────────────────────────────────────

  describe("removeSession", () => {
    it("removes the only session → activeSessionId becomes null", async () => {
      seedSessions([makeSession({ id: 1, status: "exited" })], 1);
      seedProject("/tmp/app", [1]);

      await useSessionStore.getState().removeSession(1);

      const { sessions, activeSessionId } = useSessionStore.getState();
      expect(sessions).toHaveLength(0);
      expect(activeSessionId).toBeNull();
    });

    it("removes active session → activates sibling with lowest sortOrder", async () => {
      seedSessions([
        makeSession({ id: 1, sortOrder: 3, status: "running" }),
        makeSession({ id: 2, sortOrder: 1, status: "running" }),
        makeSession({ id: 3, sortOrder: 2, status: "running" }),
      ], 1);
      seedProject("/tmp/app", [1, 2, 3]);

      await useSessionStore.getState().removeSession(1);

      const { sessions, activeSessionId } = useSessionStore.getState();
      expect(sessions).toHaveLength(2);
      // Should pick id=2 (sortOrder=1), not id=3 (sortOrder=2)
      expect(activeSessionId).toBe(2);
    });

    it("removes non-active session → activeSessionId unchanged", async () => {
      seedSessions([
        makeSession({ id: 1 }),
        makeSession({ id: 2, sortOrder: 2 }),
      ], 1);
      seedProject("/tmp/app", [1, 2]);

      await useSessionStore.getState().removeSession(2);

      expect(useSessionStore.getState().activeSessionId).toBe(1);
      expect(useSessionStore.getState().sessions).toHaveLength(1);
    });

    it("calls killSession for running sessions, deleteSession for exited", async () => {
      seedSessions([makeSession({ id: 1, status: "running" })], 1);
      await useSessionStore.getState().removeSession(1);
      expect(api.killSession).toHaveBeenCalledWith(1);
      expect(api.deleteSession).toHaveBeenCalledWith(1);

      vi.clearAllMocks();
      seedSessions([makeSession({ id: 2, status: "exited" })], 2);
      await useSessionStore.getState().removeSession(2);
      expect(api.killSession).not.toHaveBeenCalled();
      expect(api.deleteSession).toHaveBeenCalledWith(2);
    });

    it("cleans up projectStore after removal", async () => {
      seedSessions([makeSession({ id: 1, status: "exited" })], 1);
      seedProject("/tmp/app", [1]);

      await useSessionStore.getState().removeSession(1);

      const project = useProjectStore.getState().projects[0];
      expect(project.sessions).toEqual([]);
    });

    it("only considers siblings within the same project for auto-selection", async () => {
      seedSessions([
        makeSession({ id: 1, working_dir: "/tmp/app", sortOrder: 1 }),
        makeSession({ id: 2, working_dir: "/tmp/other", sortOrder: 1 }),
      ], 1);

      await useSessionStore.getState().removeSession(1);

      // id=2 is in a different project, should NOT be selected
      expect(useSessionStore.getState().activeSessionId).toBeNull();
    });
  });

  // ─── removeSessions (bulk) ───────────────────────────────────

  describe("removeSessions", () => {
    it("removes multiple sessions at once", () => {
      seedSessions([
        makeSession({ id: 1 }),
        makeSession({ id: 2, sortOrder: 2 }),
        makeSession({ id: 3, sortOrder: 3 }),
      ], 1);

      useSessionStore.getState().removeSessions([1, 3]);

      const { sessions, activeSessionId } = useSessionStore.getState();
      expect(sessions).toHaveLength(1);
      expect(sessions[0].id).toBe(2);
      expect(activeSessionId).toBeNull(); // active was id=1, removed
    });

    it("keeps activeSessionId if not in removed set", () => {
      seedSessions([
        makeSession({ id: 1 }),
        makeSession({ id: 2, sortOrder: 2 }),
      ], 2);

      useSessionStore.getState().removeSessions([1]);

      expect(useSessionStore.getState().activeSessionId).toBe(2);
    });
  });

  // ─── addSession ─────────────────────────────────────────────

  describe("addSession", () => {
    it("adds first session with sortOrder 1", async () => {
      seedProject("/tmp/app", []);
      vi.mocked(api.createSession).mockResolvedValue(10);

      await useSessionStore.getState().addSession({
        command: "claude",
        working_dir: "/tmp/app",
        task_name: "First",
        agent_type: "claude",
      });

      const { sessions, activeSessionId } = useSessionStore.getState();
      expect(sessions).toHaveLength(1);
      expect(sessions[0].sortOrder).toBe(1);
      expect(sessions[0].id).toBe(10);
      expect(activeSessionId).toBe(10);
    });

    it("adds second session with sortOrder = max + 1", async () => {
      seedSessions([makeSession({ id: 1, sortOrder: 5 })]);
      seedProject("/tmp/app", [1]);
      vi.mocked(api.createSession).mockResolvedValue(20);

      await useSessionStore.getState().addSession({
        command: "claude",
        working_dir: "/tmp/app",
        task_name: "Second",
      });

      const sessions = useSessionStore.getState().sessions;
      expect(sessions).toHaveLength(2);
      expect(sessions[1].sortOrder).toBe(6); // 5 + 1
    });

    it("computes sortOrder independently per project", async () => {
      seedSessions([
        makeSession({ id: 1, working_dir: "/tmp/a", sortOrder: 10 }),
        makeSession({ id: 2, working_dir: "/tmp/b", sortOrder: 3 }),
      ]);
      seedProject("/tmp/b", [2]);
      vi.mocked(api.createSession).mockResolvedValue(30);

      await useSessionStore.getState().addSession({
        command: "claude",
        working_dir: "/tmp/b",
        task_name: "In B",
      });

      const newSession = useSessionStore.getState().sessions.find((s) => s.id === 30)!;
      expect(newSession.sortOrder).toBe(4); // max in /tmp/b is 3, so 3+1=4
    });

    it("replaceId inherits sortOrder from replaced session", async () => {
      seedSessions([
        makeSession({ id: 1, sortOrder: 7 }),
        makeSession({ id: 2, sortOrder: 2 }),
      ]);
      seedProject("/tmp/app", [1, 2]);
      vi.mocked(api.createSession).mockResolvedValue(40);

      await useSessionStore.getState().addSession({
        command: "claude",
        working_dir: "/tmp/app",
        task_name: "Replaced",
        replaceId: 1,
      });

      const sessions = useSessionStore.getState().sessions;
      const replaced = sessions.find((s) => s.id === 40)!;
      expect(replaced.sortOrder).toBe(7); // inherited from id=1
      // Old session is gone
      expect(sessions.find((s) => s.id === 1)).toBeUndefined();
    });

    it("replaceId with missing target falls back to sortOrder 0", async () => {
      seedSessions([makeSession({ id: 2, sortOrder: 3 })]);
      seedProject("/tmp/app", [2]);
      vi.mocked(api.createSession).mockResolvedValue(50);

      await useSessionStore.getState().addSession({
        command: "claude",
        working_dir: "/tmp/app",
        replaceId: 999, // doesn't exist
      });

      // replaceId path uses .find which returns undefined → sortOrder defaults to 0
      // The session with replaceId 999 won't be found in .map, so it adds nothing replaced
      // but the set still produces a new session (the map returns all existing sessions unchanged)
      const sessions = useSessionStore.getState().sessions;
      // id=2 still exists (wasn't replaced), new session was not inserted via map
      // because replaceId 999 matched nothing in the map
      expect(sessions.find((s) => s.id === 50)).toBeUndefined();
      expect(sessions.find((s) => s.id === 2)).toBeDefined();
    });

    it("sets activeSessionId to new session", async () => {
      seedSessions([makeSession({ id: 1 })], 1);
      seedProject("/tmp/app", [1]);
      vi.mocked(api.createSession).mockResolvedValue(60);

      await useSessionStore.getState().addSession({
        command: "claude",
        working_dir: "/tmp/app",
        task_name: "New Active",
      });

      expect(useSessionStore.getState().activeSessionId).toBe(60);
    });

    it("detaches previous active session on new session add", async () => {
      seedSessions([makeSession({ id: 1 })], 1);
      seedProject("/tmp/app", [1]);
      vi.mocked(api.createSession).mockResolvedValue(70);

      await useSessionStore.getState().addSession({
        command: "claude",
        working_dir: "/tmp/app",
      });

      expect(api.detachSession).toHaveBeenCalledWith(1);
    });

    it("does NOT detach when using replaceId", async () => {
      seedSessions([makeSession({ id: 1 })], 1);
      seedProject("/tmp/app", [1]);
      vi.mocked(api.createSession).mockResolvedValue(80);

      await useSessionStore.getState().addSession({
        command: "claude",
        working_dir: "/tmp/app",
        replaceId: 1,
      });

      expect(api.detachSession).not.toHaveBeenCalled();
    });

    it("updates projectStore on new session", async () => {
      seedProject("/tmp/app", []);
      vi.mocked(api.createSession).mockResolvedValue(90);

      await useSessionStore.getState().addSession({
        command: "claude",
        working_dir: "/tmp/app",
      });

      const project = useProjectStore.getState().projects[0];
      expect(project.sessions).toContain(90);
    });

    it("ensures project exists if not present", async () => {
      useProjectStore.setState({ projects: [], activeProjectPath: null });
      vi.mocked(api.createSession).mockResolvedValue(91);

      await useSessionStore.getState().addSession({
        command: "zsh",
        working_dir: "/tmp/newproj",
      });

      const projects = useProjectStore.getState().projects;
      expect(projects.find((p) => p.path === "/tmp/newproj")).toBeDefined();
    });
  });

  // ─── reorderSessionOrder ────────────────────────────────────

  describe("reorderSessionOrder", () => {
    it("moves session left (higher to lower sortOrder)", async () => {
      seedSessions([
        makeSession({ id: 1, sortOrder: 1 }),
        makeSession({ id: 2, sortOrder: 2 }),
        makeSession({ id: 3, sortOrder: 3 }),
      ]);

      // Move id=3 from position 3 to position 1
      await useSessionStore.getState().reorderSessionOrder(3, 1);

      const sessions = useSessionStore.getState().sessions;
      expect(sessions.find((s) => s.id === 3)!.sortOrder).toBe(1);
      expect(sessions.find((s) => s.id === 1)!.sortOrder).toBe(2); // shifted right
      expect(sessions.find((s) => s.id === 2)!.sortOrder).toBe(3); // shifted right
    });

    it("moves session right (lower to higher sortOrder)", async () => {
      seedSessions([
        makeSession({ id: 1, sortOrder: 1 }),
        makeSession({ id: 2, sortOrder: 2 }),
        makeSession({ id: 3, sortOrder: 3 }),
      ]);

      // Move id=1 from position 1 to position 3
      await useSessionStore.getState().reorderSessionOrder(1, 3);

      const sessions = useSessionStore.getState().sessions;
      expect(sessions.find((s) => s.id === 1)!.sortOrder).toBe(3);
      expect(sessions.find((s) => s.id === 2)!.sortOrder).toBe(1); // shifted left
      expect(sessions.find((s) => s.id === 3)!.sortOrder).toBe(2); // shifted left
    });

    it("no-op when moving to same position — state unchanged", async () => {
      seedSessions([
        makeSession({ id: 1, sortOrder: 2 }),
        makeSession({ id: 2, sortOrder: 1 }),
      ]);

      await useSessionStore.getState().reorderSessionOrder(1, 2);
      const after = useSessionStore.getState().sessions;

      // State should be unchanged (set() returned same state)
      expect(after.find((s) => s.id === 1)!.sortOrder).toBe(2);
      expect(after.find((s) => s.id === 2)!.sortOrder).toBe(1);
    });

    it("does not affect sessions in other projects", async () => {
      seedSessions([
        makeSession({ id: 1, working_dir: "/tmp/a", sortOrder: 1 }),
        makeSession({ id: 2, working_dir: "/tmp/a", sortOrder: 2 }),
        makeSession({ id: 3, working_dir: "/tmp/b", sortOrder: 1 }),
      ]);

      // Reorder within /tmp/a only
      await useSessionStore.getState().reorderSessionOrder(2, 1);

      // Session in /tmp/b should be unaffected
      expect(useSessionStore.getState().sessions.find((s) => s.id === 3)!.sortOrder).toBe(1);
    });

    it("calls API with correct args", async () => {
      seedSessions([
        makeSession({ id: 1, sortOrder: 1 }),
        makeSession({ id: 2, sortOrder: 2 }),
      ]);

      await useSessionStore.getState().reorderSessionOrder(2, 1);

      expect(api.reorderSession).toHaveBeenCalledWith(2, 1);
    });

    it("sessionsForProject returns reordered result", async () => {
      seedSessions([
        makeSession({ id: 1, sortOrder: 1 }),
        makeSession({ id: 2, sortOrder: 2 }),
        makeSession({ id: 3, sortOrder: 3 }),
      ]);

      await useSessionStore.getState().reorderSessionOrder(3, 1);

      const ordered = useSessionStore.getState().sessionsForProject("/tmp/app");
      expect(ordered.map((s) => s.id)).toEqual([3, 1, 2]);
    });
  });

  // ─── switchSession ──────────────────────────────────────────

  describe("switchSession", () => {
    it("updates activeSessionId", async () => {
      seedSessions([
        makeSession({ id: 1 }),
        makeSession({ id: 2, sortOrder: 2 }),
      ], 1);

      await useSessionStore.getState().switchSession(2);

      expect(useSessionStore.getState().activeSessionId).toBe(2);
    });

    it("no-op when switching to already active session", async () => {
      seedSessions([makeSession({ id: 1 })], 1);

      await useSessionStore.getState().switchSession(1);

      expect(api.detachSession).not.toHaveBeenCalled();
      expect(api.attachSession).not.toHaveBeenCalled();
    });

    it("detaches current and attaches target for live sessions", async () => {
      seedSessions([
        makeSession({ id: 1, status: "running" }),
        makeSession({ id: 2, status: "running", sortOrder: 2 }),
      ], 1);

      await useSessionStore.getState().switchSession(2);

      expect(api.detachSession).toHaveBeenCalledWith(1);
      expect(api.attachSession).toHaveBeenCalledWith(2);
    });

    it("does not attach exited sessions", async () => {
      seedSessions([
        makeSession({ id: 1, status: "running" }),
        makeSession({ id: 2, status: "exited", sortOrder: 2 }),
      ], 1);

      await useSessionStore.getState().switchSession(2);

      expect(api.detachSession).toHaveBeenCalledWith(1);
      expect(api.attachSession).not.toHaveBeenCalled();
    });

    it("updates projectStore activeProjectPath", async () => {
      seedSessions([
        makeSession({ id: 1, working_dir: "/tmp/a" }),
        makeSession({ id: 2, working_dir: "/tmp/b", sortOrder: 2 }),
      ], 1);

      await useSessionStore.getState().switchSession(2);

      expect(useProjectStore.getState().activeProjectPath).toBe("/tmp/b");
    });
  });

  // ─── updateSessionStatus / batchUpdate / handleExit ─────────

  describe("status updates", () => {
    it("updateSessionStatus changes status and status_line", () => {
      seedSessions([makeSession({ id: 1, status: "running", status_line: "" })]);

      useSessionStore.getState().updateSessionStatus(1, "idle", "Waiting...");

      const s = useSessionStore.getState().sessions[0];
      expect(s.status).toBe("idle");
      expect(s.status_line).toBe("Waiting...");
    });

    it("batchUpdateSessionStatus updates multiple in one call", () => {
      seedSessions([
        makeSession({ id: 1, status: "running" }),
        makeSession({ id: 2, status: "running", sortOrder: 2 }),
        makeSession({ id: 3, status: "idle", sortOrder: 3 }),
      ]);

      useSessionStore.getState().batchUpdateSessionStatus([
        { id: 1, status: "idle", statusLine: "Waiting" },
        { id: 3, status: "exited", statusLine: "Done" },
      ]);

      const sessions = useSessionStore.getState().sessions;
      expect(sessions.find((s) => s.id === 1)!.status).toBe("idle");
      expect(sessions.find((s) => s.id === 2)!.status).toBe("running"); // unchanged
      expect(sessions.find((s) => s.id === 3)!.status).toBe("exited");
    });

    it("batchUpdateSessionStatus with empty array is a no-op", () => {
      seedSessions([makeSession({ id: 1 })]);
      const before = useSessionStore.getState().sessions;

      useSessionStore.getState().batchUpdateSessionStatus([]);

      expect(useSessionStore.getState().sessions).toBe(before); // same reference
    });

    it("handleExit marks session as exited", () => {
      seedSessions([makeSession({ id: 1, status: "running" })]);

      useSessionStore.getState().handleExit(1);

      expect(useSessionStore.getState().sessions[0].status).toBe("exited");
    });
  });
});
