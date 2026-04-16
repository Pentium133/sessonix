import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
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
}));

vi.mock("../components/TerminalPane", () => ({
  writeToTerminal: vi.fn(),
}));

vi.mock("../lib/git", () => ({
  getGitStatus: vi.fn().mockResolvedValue({ is_repo: true, branch: "main", changed_files: 0, modified: 0, added: 0, deleted: 0, head_sha: null, is_worktree: true }),
  createWorktree: vi.fn(),
  removeWorktree: vi.fn(),
}));

// Mock Toast
const mockShowToast = vi.fn();
vi.mock("../components/Toast", () => ({
  showToast: (...args: unknown[]) => mockShowToast(...args),
}));

import { useSessionStore } from "../store/sessionStore";
import { useProjectStore } from "../store/projectStore";
import { useSessionActions } from "../hooks/useSessionActions";
import * as api from "../lib/api";

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 1,
    command: "claude",
    args: ["--dangerously-skip-permissions"],
    working_dir: "/tmp/app",
    task_name: "Test",
    agent_type: "claude",
    status: "running",
    status_line: "",
    created_at: Date.now(),
    sortOrder: 1,
    agentSessionId: "uuid-123",
    gitStatus: null,
    worktree_path: null,
    base_commit: null,
    initial_prompt: null,
    task_id: null,
    ...overrides,
  };
}

describe("useSessionActions", () => {
  beforeEach(() => {
    useSessionStore.setState({ sessions: [], activeSessionId: null, loaded: true });
    useProjectStore.setState({ projects: [], activeProjectPath: null });
    vi.clearAllMocks();
  });

  describe("handleRelaunchSession", () => {
    it("deletes old session from DB before creating new one", async () => {
      const session = makeSession({ id: 1, status: "exited" });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(50);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleRelaunchSession(session);
      });

      // Old session must be killed + deleted from DB before creating new one
      // (prevents ghost duplicates after app restart)
      expect(api.killSession).toHaveBeenCalledWith(1);
      expect(api.deleteSession).toHaveBeenCalledWith(1);
      expect(api.createSession).toHaveBeenCalled();
    });

    it("preserves --dangerously-skip-permissions for Claude", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "claude",
        args: ["--dangerously-skip-permissions"],
        agentSessionId: "uuid-abc",
        status: "exited",
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(51);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleRelaunchSession(session);
      });

      const call = vi.mocked(api.createSession).mock.calls[0][0];
      expect(call.args).toContain("--dangerously-skip-permissions");
      expect(call.args).toContain("--resume");
      expect(call.args).toContain("uuid-abc");
    });

    it("uses --continue for Claude without agentSessionId", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "claude",
        args: [],
        agentSessionId: undefined,
        status: "exited",
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(52);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleRelaunchSession(session);
      });

      const call = vi.mocked(api.createSession).mock.calls[0][0];
      expect(call.args).toContain("--continue");
      expect(call.args).not.toContain("--resume");
    });

    it("uses resume --last for Codex without agentSessionId", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "codex",
        command: "codex",
        args: ["--model", "o4-mini"],
        agentSessionId: undefined,
        status: "exited",
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(53);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleRelaunchSession(session);
      });

      const call = vi.mocked(api.createSession).mock.calls[0][0];
      expect(call.args).toEqual(["resume", "--last"]);
    });

    it("uses resume <id> for Codex with agentSessionId", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "codex",
        command: "codex",
        args: [],
        agentSessionId: "thread-xyz-789",
        status: "exited",
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(54);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleRelaunchSession(session);
      });

      const call = vi.mocked(api.createSession).mock.calls[0][0];
      expect(call.args).toEqual(["resume", "thread-xyz-789"]);
    });

    it("passes original args for non-Claude/Codex agents", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "gemini",
        command: "gemini",
        args: ["--model", "gemini-pro"],
        agentSessionId: undefined,
        status: "exited",
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(55);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleRelaunchSession(session);
      });

      const call = vi.mocked(api.createSession).mock.calls[0][0];
      expect(call.args).toEqual(["--model", "gemini-pro"]);
    });

    it("uses run --quiet --session <id> for OpenCode with agentSessionId", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "opencode",
        command: "opencode",
        args: ["run", "--quiet"],
        agentSessionId: "ses_abc123",
        status: "exited",
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(56);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleRelaunchSession(session);
      });

      const call = vi.mocked(api.createSession).mock.calls[0][0];
      expect(call.args).toEqual(["run", "--quiet", "--session", "ses_abc123"]);
    });

    it("uses run --quiet --continue for OpenCode without agentSessionId", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "opencode",
        command: "opencode",
        args: ["run", "--quiet"],
        agentSessionId: undefined,
        status: "exited",
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(57);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleRelaunchSession(session);
      });

      const call = vi.mocked(api.createSession).mock.calls[0][0];
      expect(call.args).toEqual(["run", "--quiet", "--continue"]);
    });

    it("drops prompt on OpenCode relaunch (context already on backend)", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "opencode",
        command: "opencode",
        args: ["run", "--quiet", "fix the bug"],
        agentSessionId: "ses_xyz",
        initial_prompt: "fix the bug",
        status: "exited",
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(58);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleRelaunchSession(session);
      });

      const call = vi.mocked(api.createSession).mock.calls[0][0];
      expect(call.args).toEqual(["run", "--quiet", "--session", "ses_xyz"]);
      expect(call.args).not.toContain("fix the bug");
    });

    it("threads worktree_path through OpenCode relaunch when repo still exists", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "opencode",
        command: "opencode",
        args: ["run", "--quiet"],
        agentSessionId: "ses_wt",
        worktree_path: "/tmp/app/.sessonix-worktrees/feat-x",
        base_commit: "abc123",
        status: "exited",
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(59);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleRelaunchSession(session);
      });

      const call = vi.mocked(api.createSession).mock.calls[0][0];
      expect(call.args).toEqual(["run", "--quiet", "--session", "ses_wt"]);
      expect(call.worktree_path).toBe("/tmp/app/.sessonix-worktrees/feat-x");
      expect(call.base_commit).toBe("abc123");
      expect(call.working_dir).toBe("/tmp/app"); // project root, not worktree
    });
  });

  describe("handleForkSession", () => {
    it("uses --resume for Claude with agentSessionId", async () => {
      const session = makeSession({
        agent_type: "claude",
        agentSessionId: "uuid-fork",
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(60);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleForkSession(session);
      });

      const call = vi.mocked(api.createSession).mock.calls[0][0];
      expect(call.args).toContain("--resume");
      expect(call.args).toContain("uuid-fork");
      expect(call.task_name).toBe("Test (fork)");
    });

    it("uses fork subcommand for Codex with agentSessionId", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "codex",
        command: "codex",
        args: [],
        agentSessionId: "thread-fork-123",
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(61);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleForkSession(session);
      });

      const call = vi.mocked(api.createSession).mock.calls[0][0];
      expect(call.args).toEqual(["fork", "thread-fork-123"]);
    });

    it("shows error toast for Codex fork without agentSessionId", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "codex",
        command: "codex",
        args: ["--verbose"],
        agentSessionId: undefined,
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleForkSession(session);
      });

      // Should not create a session — just show error
      expect(api.createSession).not.toHaveBeenCalled();
      expect(mockShowToast).toHaveBeenCalledWith(
        expect.stringContaining("thread ID not yet captured"),
        "error"
      );
    });

    it("passes original args for non-Claude/Codex agents", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "gemini",
        command: "gemini",
        args: ["--verbose"],
        agentSessionId: undefined,
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(63);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleForkSession(session);
      });

      const call = vi.mocked(api.createSession).mock.calls[0][0];
      expect(call.args).toEqual(["--verbose"]);
      expect(call.args).not.toContain("--continue");
    });

    it("does NOT set replaceId (fork is a new session)", async () => {
      const session = makeSession();
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(62);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleForkSession(session);
      });

      // After fork, both sessions should exist
      const sessions = useSessionStore.getState().sessions;
      expect(sessions).toHaveLength(2);
    });

    it("blocks fork for OpenCode (CLI has no fork subcommand)", async () => {
      const session = makeSession({
        id: 1,
        agent_type: "opencode",
        command: "opencode",
        args: ["run", "--quiet"],
        agentSessionId: "ses_abc",
      });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleForkSession(session);
      });

      expect(api.createSession).not.toHaveBeenCalled();
      expect(mockShowToast).toHaveBeenCalledWith(
        expect.stringContaining("OpenCode does not support forking"),
        "error"
      );
    });

    it("shows success toast on fork", async () => {
      const session = makeSession();
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });
      useProjectStore.setState({
        projects: [{ path: "/tmp/app", name: "app", sessions: [1] }],
        activeProjectPath: "/tmp/app",
      });
      vi.mocked(api.createSession).mockResolvedValue(63);

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        await result.current.handleForkSession(session);
      });

      expect(mockShowToast).toHaveBeenCalledWith("Session forked", "success");
    });
  });

  describe("handleRemoveSession", () => {
    it("shows relaunch toast for exited sessions", async () => {
      const session = makeSession({ id: 1, status: "exited" });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        result.current.handleRemoveSession(1);
      });

      expect(mockShowToast).toHaveBeenCalledWith(
        '"Test" removed',
        "info",
        expect.objectContaining({ label: "Relaunch" })
      );
    });

    it("does not show toast for running sessions", async () => {
      const session = makeSession({ id: 1, status: "running" });
      useSessionStore.setState({ sessions: [session], activeSessionId: 1 });

      const { result } = renderHook(() => useSessionActions());

      await act(async () => {
        result.current.handleRemoveSession(1);
      });

      expect(mockShowToast).not.toHaveBeenCalled();
    });
  });
});
