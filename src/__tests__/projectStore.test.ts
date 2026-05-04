import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../lib/api", () => ({
  addProject: vi.fn().mockResolvedValue(1),
  removeProject: vi.fn().mockResolvedValue(undefined),
  killSession: vi.fn().mockResolvedValue(undefined),
  deleteSession: vi.fn().mockResolvedValue(undefined),
  reorderProject: vi.fn().mockResolvedValue(undefined),
}));

import { useProjectStore } from "../store/projectStore";
import * as api from "../lib/api";

describe("projectStore", () => {
  beforeEach(() => {
    useProjectStore.setState({ projects: [], activeProjectPath: null });
    vi.clearAllMocks();
  });

  describe("ensureProject", () => {
    it("adds project if not present", () => {
      useProjectStore.getState().ensureProject("/tmp/app");

      const projects = useProjectStore.getState().projects;
      expect(projects).toHaveLength(1);
      expect(projects[0].path).toBe("/tmp/app");
      expect(projects[0].name).toBe("app");
      expect(projects[0].sessions).toEqual([]);
    });

    it("is idempotent — does not duplicate", () => {
      useProjectStore.getState().ensureProject("/tmp/app");
      useProjectStore.getState().ensureProject("/tmp/app");

      expect(useProjectStore.getState().projects).toHaveLength(1);
    });
  });

  describe("addSessionToProject", () => {
    it("adds session ID to project", () => {
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
      });

      useProjectStore.getState().addSessionToProject("/tmp/app", 2);

      expect(useProjectStore.getState().projects[0].sessions).toEqual([1, 2]);
    });
  });

  describe("removeSessionFromProject", () => {
    it("removes session ID from all projects", () => {
      useProjectStore.setState({
        projects: [
          { path: "/tmp/a", name: "a", sessions: [1, 2] },
          { path: "/tmp/b", name: "b", sessions: [2, 3] },
        ],
      });

      useProjectStore.getState().removeSessionFromProject(2);

      const projects = useProjectStore.getState().projects;
      expect(projects[0].sessions).toEqual([1]);
      expect(projects[1].sessions).toEqual([3]);
    });
  });

  describe("replaceSessionInProject", () => {
    it("swaps old ID with new ID", () => {
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1, 2, 3] }],
      });

      useProjectStore.getState().replaceSessionInProject("/tmp/app", 2, 99);

      expect(useProjectStore.getState().projects[0].sessions).toEqual([1, 99, 3]);
    });

    it("does nothing if old ID not found", () => {
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1, 2] }],
      });

      useProjectStore.getState().replaceSessionInProject("/tmp/app", 999, 50);

      expect(useProjectStore.getState().projects[0].sessions).toEqual([1, 2]);
    });
  });

  describe("removeProject", () => {
    it("removes project and returns session IDs", async () => {
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1, 2] }],
        activeProjectPath: "/tmp/app",
      });

      const removedIds = await useProjectStore.getState().removeProject("/tmp/app");

      expect(removedIds).toEqual([1, 2]);
      expect(useProjectStore.getState().projects).toHaveLength(0);
      expect(useProjectStore.getState().activeProjectPath).toBeNull();
    });

    it("kills each session before removing project", async () => {
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [5, 6] }],
        activeProjectPath: "/tmp/app",
      });

      await useProjectStore.getState().removeProject("/tmp/app");

      expect(api.killSession).toHaveBeenCalledWith(5);
      expect(api.killSession).toHaveBeenCalledWith(6);
    });

    it("switches activeProjectPath to next project", async () => {
      useProjectStore.setState({
        projects: [
          { path: "/tmp/a", name: "a", sessions: [] },
          { path: "/tmp/b", name: "b", sessions: [] },
        ],
        activeProjectPath: "/tmp/a",
      });

      await useProjectStore.getState().removeProject("/tmp/a");

      expect(useProjectStore.getState().activeProjectPath).toBe("/tmp/b");
    });

    it("returns empty array for non-existent project", async () => {
      const result = await useProjectStore.getState().removeProject("/tmp/nope");
      expect(result).toEqual([]);
    });

    it("keeps activeProjectPath if a different project was removed", async () => {
      useProjectStore.setState({
        projects: [
          { path: "/tmp/a", name: "a", sessions: [] },
          { path: "/tmp/b", name: "b", sessions: [] },
        ],
        activeProjectPath: "/tmp/a",
      });

      await useProjectStore.getState().removeProject("/tmp/b");

      expect(useProjectStore.getState().activeProjectPath).toBe("/tmp/a");
    });
  });

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

  describe("addProject", () => {
    it("adds project and sets active", async () => {
      await useProjectStore.getState().addProject("/home/user/myapp");

      const { projects, activeProjectPath } = useProjectStore.getState();
      expect(projects).toHaveLength(1);
      expect(projects[0].name).toBe("myapp");
      expect(activeProjectPath).toBe("/home/user/myapp");
    });

    it("does not duplicate existing project", async () => {
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [] }],
      });

      await useProjectStore.getState().addProject("/tmp/app");

      expect(useProjectStore.getState().projects).toHaveLength(1);
    });
  });

  describe("setProjects", () => {
    it("bulk-sets projects (used by sessionStore.restore)", () => {
      useProjectStore.getState().setProjects([
        { path: "/tmp/a", name: "a", sessions: [1] },
        { path: "/tmp/b", name: "b", sessions: [2, 3] },
      ]);

      expect(useProjectStore.getState().projects).toHaveLength(2);
    });
  });
});
