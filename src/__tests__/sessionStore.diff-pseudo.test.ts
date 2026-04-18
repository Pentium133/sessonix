import { describe, it, expect, vi, beforeEach } from "vitest";
import type { Session } from "../lib/types";

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

vi.mock("../lib/terminalPool", () => ({
  writeToTerminal: vi.fn(),
}));

import { useSessionStore, DIFF_PSEUDO_ID } from "../store/sessionStore";
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
    initial_prompt: null,
    task_id: null,
    ...overrides,
  };
}

describe("sessionStore — Diff pseudo-session", () => {
  beforeEach(() => {
    useSessionStore.setState({ sessions: [], activeSessionId: null, loaded: false });
    useProjectStore.setState({ projects: [], activeProjectPath: null, lastActiveSession: {} });
    vi.clearAllMocks();
  });

  it("reserves id 0 for the pseudo-session", () => {
    expect(DIFF_PSEUDO_ID).toBe(0);
  });

  it("switchSession(DIFF_PSEUDO_ID) sets activeSessionId to 0 without writing lastActiveSession", async () => {
    useSessionStore.setState({
      sessions: [makeSession({ id: 5, working_dir: "/tmp/app" })],
      activeSessionId: 5,
      loaded: true,
    });
    useProjectStore.setState({
      projects: [{ path: "/tmp/app", name: "app", sessions: [5] }],
      activeProjectPath: "/tmp/app",
      lastActiveSession: {},
    });

    await useSessionStore.getState().switchSession(DIFF_PSEUDO_ID);

    expect(useSessionStore.getState().activeSessionId).toBe(DIFF_PSEUDO_ID);
    expect(useProjectStore.getState().lastActiveSession).toEqual({});
    expect(api.attachSession).not.toHaveBeenCalled();
    expect(api.detachSession).toHaveBeenCalledWith(5);
  });

  it("switchSession to a real session after diff records lastActiveSession", async () => {
    useSessionStore.setState({
      sessions: [
        makeSession({ id: 7, working_dir: "/tmp/app" }),
      ],
      activeSessionId: DIFF_PSEUDO_ID,
      loaded: true,
    });
    useProjectStore.setState({
      projects: [{ path: "/tmp/app", name: "app", sessions: [7] }],
      activeProjectPath: "/tmp/app",
      lastActiveSession: {},
    });

    await useSessionStore.getState().switchSession(7);

    expect(useSessionStore.getState().activeSessionId).toBe(7);
    expect(useProjectStore.getState().lastActiveSession).toEqual({ "/tmp/app": 7 });
    // There was no real session to detach (active was DIFF_PSEUDO_ID=0 → not found).
    expect(api.detachSession).not.toHaveBeenCalled();
  });

  it("switchSession(DIFF_PSEUDO_ID) is a no-op when already active", async () => {
    useSessionStore.setState({
      sessions: [],
      activeSessionId: DIFF_PSEUDO_ID,
      loaded: true,
    });

    await useSessionStore.getState().switchSession(DIFF_PSEUDO_ID);

    expect(api.detachSession).not.toHaveBeenCalled();
    expect(api.attachSession).not.toHaveBeenCalled();
  });
});
