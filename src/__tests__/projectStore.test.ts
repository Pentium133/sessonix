import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../lib/api", () => ({
  addProject: vi.fn().mockResolvedValue(1),
  removeProject: vi.fn().mockResolvedValue(undefined),
  killSession: vi.fn().mockResolvedValue(undefined),
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
