import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

// Mock API before importing stores — invoke returns undefined by default,
// which breaks async load() calls in quickPromptStore/taskStore on render.
vi.mock("../lib/api", () => ({
  writeToSession: vi.fn().mockResolvedValue(undefined),
  createSession: vi.fn().mockResolvedValue(100),
  killSession: vi.fn().mockResolvedValue(undefined),
  deleteSession: vi.fn().mockResolvedValue(undefined),
  attachSession: vi.fn().mockResolvedValue([]),
  detachSession: vi.fn().mockResolvedValue(undefined),
  listProjects: vi.fn().mockResolvedValue([]),
  listSessions: vi.fn().mockResolvedValue([]),
  listQuickPrompts: vi.fn().mockResolvedValue([]),
  createQuickPrompt: vi.fn().mockResolvedValue(1),
  deleteQuickPrompt: vi.fn().mockResolvedValue(undefined),
  updateQuickPrompt: vi.fn().mockResolvedValue(undefined),
  listTasks: vi.fn().mockResolvedValue([]),
  createTask: vi.fn().mockResolvedValue({
    id: 1, projectId: 1, name: "t", branch: null, worktreePath: null, baseCommit: null, createdAt: 0,
  }),
  deleteTask: vi.fn().mockResolvedValue({ worktree_warning: null }),
  installClaudeHooks: vi.fn().mockResolvedValue(true),
  checkClaudeHooks: vi.fn().mockResolvedValue(true),
  reorderSession: vi.fn().mockResolvedValue(undefined),
  setSortOrder: vi.fn().mockResolvedValue(undefined),
  addProject: vi.fn().mockResolvedValue(1),
  removeProject: vi.fn().mockResolvedValue(undefined),
  getSetting: vi.fn().mockResolvedValue(null),
  setSetting: vi.fn().mockResolvedValue(undefined),
  getAllSettings: vi.fn().mockResolvedValue([]),
  checkForUpdate: vi.fn().mockResolvedValue(null),
  detectAgents: vi.fn().mockResolvedValue({}),
}));

import Sidebar from "../components/Sidebar";
import { useSessionStore } from "../store/sessionStore";
import { useProjectStore } from "../store/projectStore";
import { useUiStore } from "../store/uiStore";
import { useTaskStore } from "../store/taskStore";
import type { Project, Session, Task } from "../lib/types";
import * as git from "../lib/git";

const mockHandleRemoveSession = vi.fn();
const mockHandleRelaunchSession = vi.fn();
const mockHandleForkSession = vi.fn();

vi.mock("../hooks/useSessionActions", () => ({
  useSessionActions: () => ({
    handleRemoveSession: mockHandleRemoveSession,
    handleRelaunchSession: mockHandleRelaunchSession,
    handleForkSession: mockHandleForkSession,
  }),
}));

vi.mock("../components/Toast", () => ({
  showToast: vi.fn(),
}));

vi.mock("../lib/git", async () => {
  const actual = await vi.importActual<typeof import("../lib/git")>("../lib/git");
  return {
    ...actual,
    getGitStatus: vi.fn().mockResolvedValue({
      is_repo: false,
      branch: null,
      changed_files: 0,
      modified: 0,
      added: 0,
      deleted: 0,
      head_sha: null,
      is_worktree: false,
    }),
  };
});

function makeSessions(count: number, projectPath: string): Session[] {
  return Array.from({ length: count }, (_, i) => ({
    id: i + 1,
    command: "claude",
    args: [],
    working_dir: projectPath,
    task_name: `Session ${i + 1}`,
    agent_type: "claude" as const,
    status: i === 0 ? "running" as const : "exited" as const,
    status_line: "",
    created_at: Date.now(),
    sortOrder: i + 1,
    gitStatus: null,
    worktree_path: null,
    base_commit: null,
    initial_prompt: null,
    task_id: null,
    telegramEnabled: false,
  }));
}

function setupStores(
  projects: Project[],
  sessions: Session[],
  activeSessionId: number | null = null,
  collapsed = false,
  tasks: Task[] = [],
) {
  useProjectStore.setState({ projects, activeProjectPath: projects[0]?.path ?? null });
  useSessionStore.setState({
    sessions,
    activeSessionId,
    loaded: true,
  });
  useUiStore.setState({ sidebarCollapsed: collapsed, sidebarWidth: 260 });
  useTaskStore.setState({ tasks, loaded: true });
}

function makeTask(overrides: Partial<Task> = {}): Task {
  return {
    id: 1,
    projectId: 1,
    name: "Task A",
    branch: "feat/a",
    worktreePath: "/tmp/app/.sessonix-worktrees/feat-a",
    baseCommit: "abc",
    createdAt: 0,
    ...overrides,
  };
}

describe("Sidebar (SessionPanel)", () => {
  beforeEach(() => {
    setupStores([], []);
    vi.clearAllMocks();
    vi.mocked(git.getGitStatus).mockResolvedValue({
      is_repo: false,
      branch: null,
      changed_files: 0,
      modified: 0,
      added: 0,
      deleted: 0,
      head_sha: null,
      is_worktree: false,
    });
  });

  it("shows empty state when no active project", () => {
    render(<Sidebar />);
    expect(screen.getByText(/Select a project/)).toBeTruthy();
  });

  it("returns null when sidebar is collapsed", () => {
    setupStores([], [], null, true);
    const { container } = render(<Sidebar />);
    expect(container.querySelector(".sidebar")).toBeNull();
  });

  it("renders project name as header", () => {
    const projects: Project[] = [
      { path: "/home/user/app", name: "app", sessions: [1] },
    ];
    const sessions = makeSessions(1, "/home/user/app");
    setupStores(projects, sessions);
    render(<Sidebar />);
    expect(screen.getByText("app")).toBeTruthy();
    expect(screen.getByText("Session 1")).toBeTruthy();
  });

  it("shows only sessions for active project", () => {
    const projects: Project[] = [
      { path: "/tmp/app", name: "app", sessions: [1] },
      { path: "/tmp/other", name: "other", sessions: [2] },
    ];
    const sessions: Session[] = [
      ...makeSessions(1, "/tmp/app"),
      {
        id: 2, command: "claude", args: [], working_dir: "/tmp/other",
        task_name: "Other Session", agent_type: "claude", status: "running",
        status_line: "", created_at: Date.now(), sortOrder: 1, gitStatus: null, worktree_path: null, base_commit: null, initial_prompt: null, task_id: null,
    telegramEnabled: false,
      },
    ];
    setupStores(projects, sessions);
    render(<Sidebar />);
    expect(screen.getByText("Session 1")).toBeTruthy();
    expect(screen.queryByText("Other Session")).toBeNull();
  });

  it("calls switchSession on click", () => {
    const switchSession = vi.fn();
    const projects: Project[] = [
      { path: "/tmp/app", name: "app", sessions: [1] },
    ];
    const sessions = makeSessions(1, "/tmp/app");
    setupStores(projects, sessions);
    useSessionStore.setState({ switchSession });
    render(<Sidebar />);
    fireEvent.click(screen.getByText("Session 1"));
    expect(switchSession).toHaveBeenCalledWith(1);
  });

  it("shows the worktree tree icon in git header for worktree projects", async () => {
    const projects: Project[] = [
      { path: "/tmp/app", name: "app", sessions: [1] },
    ];
    const sessions = makeSessions(1, "/tmp/app");
    setupStores(projects, sessions);
    vi.mocked(git.getGitStatus).mockResolvedValue({
      is_repo: true,
      branch: "feature/tree-icon",
      changed_files: 0,
      modified: 0,
      added: 0,
      deleted: 0,
      head_sha: "abc123",
      is_worktree: true,
    });

    const { container } = render(<Sidebar />);

    expect(await screen.findByText("feature/tree-icon")).toBeTruthy();
    expect(container.querySelector(".sidebar-git-info .session-wt-icon")).toBeTruthy();
  });

  it("highlights active session", () => {
    const projects: Project[] = [
      { path: "/tmp/app", name: "app", sessions: [1, 2] },
    ];
    const sessions = makeSessions(2, "/tmp/app");
    setupStores(projects, sessions, 1);
    const { container } = render(<Sidebar />);
    const activeItem = container.querySelector(".session-card.active");
    expect(activeItem).toBeTruthy();
    expect(activeItem?.textContent).toContain("Session 1");
  });

  describe("Task grouping", () => {
    const projects: Project[] = [
      { path: "/tmp/app", name: "app", sessions: [1, 2] },
    ];

    it("renders TaskGroup when project has tasks", () => {
      const sessions = makeSessions(1, "/tmp/app");
      setupStores(projects, sessions, null, false, [makeTask()]);
      const { container } = render(<Sidebar />);
      expect(container.querySelector(".task-group")).toBeTruthy();
      expect(screen.getByText("Task A")).toBeTruthy();
      expect(screen.getByText("Tasks")).toBeTruthy();
    });

    it("renders a grouped session inside its TaskGroup body", () => {
      const sessions: Session[] = [
        {
          ...makeSessions(1, "/tmp/app")[0],
          id: 1,
          task_name: "Grouped S",
          task_id: 1,
        },
      ];
      setupStores(projects, sessions, null, false, [makeTask()]);
      const { container } = render(<Sidebar />);
      const body = container.querySelector(".task-group-body");
      expect(body).toBeTruthy();
      expect(body?.textContent).toContain("Grouped S");
    });

    it("renders ungrouped sessions outside task groups", () => {
      const sessions: Session[] = [
        { ...makeSessions(1, "/tmp/app")[0], id: 1, task_name: "Loose S", task_id: null },
      ];
      setupStores(projects, sessions, null, false, [makeTask()]);
      const { container } = render(<Sidebar />);
      const body = container.querySelector(".task-group-body");
      // task has no sessions → body renders "No sessions yet" placeholder
      expect(body?.textContent).toContain("No sessions yet");
      // Ungrouped session should render outside, directly under sessions-list
      const looseCard = screen
        .getByText("Loose S")
        .closest(".session-card");
      expect(looseCard?.parentElement?.classList.contains("task-group-body")).toBe(false);
    });

    it("shows empty TaskGroup state when task has no sessions", () => {
      setupStores(projects, [], null, false, [makeTask()]);
      render(<Sidebar />);
      expect(screen.getByText("No sessions yet")).toBeTruthy();
    });

    it("does not render 'Tasks' header when no tasks exist", () => {
      const sessions = makeSessions(1, "/tmp/app");
      setupStores(projects, sessions, null, false, []);
      render(<Sidebar />);
      expect(screen.queryByText("Tasks")).toBeNull();
    });
  });

  describe("Kill confirmation", () => {
    function renderRunningSession() {
      const projects: Project[] = [
        { path: "/tmp/app", name: "app", sessions: [1] },
      ];
      const sessions = makeSessions(1, "/tmp/app"); // session 1 = running
      setupStores(projects, sessions);
      return render(<Sidebar />);
    }

    it("shows confirm/cancel on Kill click", () => {
      const { container } = renderRunningSession();
      expect(container.querySelector(".card-btn-kill")).toBeTruthy();
      expect(container.querySelector(".kill-confirm-btn")).toBeNull();

      fireEvent.click(container.querySelector(".card-btn-kill")!);

      expect(container.querySelector(".kill-confirm-btn")).toBeTruthy();
      expect(container.querySelector(".kill-cancel-btn")).toBeTruthy();
      expect(container.querySelector(".card-btn-kill")).toBeNull();
    });

    it("restores Kill button on Cancel", () => {
      const { container } = renderRunningSession();
      fireEvent.click(container.querySelector(".card-btn-kill")!);
      fireEvent.click(container.querySelector(".kill-cancel-btn")!);

      expect(container.querySelector(".card-btn-kill")).toBeTruthy();
      expect(container.querySelector(".kill-confirm-btn")).toBeNull();
    });

    it("calls handleRemoveSession on confirm Kill", () => {
      const { container } = renderRunningSession();
      fireEvent.click(container.querySelector(".card-btn-kill")!);
      fireEvent.click(container.querySelector(".kill-confirm-btn")!);

      expect(mockHandleRemoveSession).toHaveBeenCalledWith(1);
    });

    it("Shift+click Kill bypasses confirmation", () => {
      const { container } = renderRunningSession();
      fireEvent.click(container.querySelector(".card-btn-kill")!, { shiftKey: true });

      expect(mockHandleRemoveSession).toHaveBeenCalledWith(1);
      expect(container.querySelector(".kill-confirm-btn")).toBeNull();
    });
  });

  describe("Fork button", () => {
    it("shows Fork for running claude sessions", () => {
      const projects: Project[] = [
        { path: "/tmp/app", name: "app", sessions: [1] },
      ];
      const sessions = makeSessions(1, "/tmp/app"); // claude, running
      setupStores(projects, sessions);
      const { container } = render(<Sidebar />);

      expect(container.querySelector(".card-btn-fork")).toBeTruthy();
    });

    it("does not show Fork for non-claude sessions", () => {
      const projects: Project[] = [
        { path: "/tmp/app", name: "app", sessions: [1] },
      ];
      const sessions: Session[] = [{
        id: 1,
        command: "codex",
        args: [],
        working_dir: "/tmp/app",
        task_name: "Codex Session",
        agent_type: "codex",
        status: "running",
        status_line: "",
        created_at: Date.now(),
        sortOrder: 1,
        gitStatus: null,
        worktree_path: null,
        base_commit: null,
        initial_prompt: null,
        task_id: null,
    telegramEnabled: false,
      }];
      setupStores(projects, sessions);
      const { container } = render(<Sidebar />);

      expect(container.querySelector(".card-btn-fork")).toBeNull();
    });

    it("calls handleForkSession on Fork confirm", () => {
      const projects: Project[] = [
        { path: "/tmp/app", name: "app", sessions: [1] },
      ];
      const sessions = makeSessions(1, "/tmp/app");
      setupStores(projects, sessions);
      const { container } = render(<Sidebar />);

      fireEvent.click(container.querySelector(".card-btn-fork")!);
      expect(container.querySelector(".kill-confirm-btn")).toBeTruthy();

      fireEvent.click(container.querySelector(".kill-confirm-btn")!);
      expect(mockHandleForkSession).toHaveBeenCalledWith(sessions[0]);
    });
  });
});
