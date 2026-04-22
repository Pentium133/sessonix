import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent, screen } from "@testing-library/react";

vi.mock("../components/Toast", () => ({
  showToast: vi.fn(),
}));

vi.mock("../lib/git", () => ({
  removeWorktree: vi.fn().mockResolvedValue(undefined),
  clearWorktreePath: vi.fn().mockResolvedValue(undefined),
}));

import TaskGroup from "../components/TaskGroup";
import type { Session, Task } from "../lib/types";

function makeTask(overrides: Partial<Task> = {}): Task {
  return {
    id: 10,
    projectId: 1,
    name: "Fix auth",
    branch: "feat/fix-auth",
    worktreePath: "/repo/.sessonix-worktrees/feat-fix-auth",
    baseCommit: "abc",
    createdAt: 0,
    ...overrides,
  };
}

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 1,
    command: "claude",
    args: [],
    working_dir: "/repo",
    task_name: "S1",
    agent_type: "claude",
    status: "running",
    status_line: "",
    created_at: Date.now(),
    sortOrder: 1,
    gitStatus: null,
    worktree_path: null,
    base_commit: null,
    initial_prompt: null,
    task_id: 10,
    telegramEnabled: false,
    ...overrides,
  };
}

function defaultHandlers() {
  return {
    onToggle: vi.fn(),
    onAddAgent: vi.fn(),
    onInstantShell: vi.fn(),
    onDelete: vi.fn(),
    onSwitchSession: vi.fn(),
    onRelaunchSession: vi.fn(),
    onRemoveSession: vi.fn(),
    onForkSession: vi.fn(),
  };
}

describe("TaskGroup", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders task name and branch badge", () => {
    const h = defaultHandlers();
    render(
      <TaskGroup
        task={makeTask()}
        sessions={[]}
        isExpanded={false}
        activeSessionId={null}
        projectBranch="main"
        {...h}
      />
    );
    expect(screen.getByText("Fix auth")).toBeTruthy();
    expect(screen.getByText("feat/fix-auth")).toBeTruthy();
  });

  it("calls onToggle when header is clicked", () => {
    const h = defaultHandlers();
    const { container } = render(
      <TaskGroup
        task={makeTask()}
        sessions={[]}
        isExpanded={false}
        activeSessionId={null}
        projectBranch="main"
        {...h}
      />
    );
    fireEvent.click(container.querySelector(".task-group-header")!);
    expect(h.onToggle).toHaveBeenCalledTimes(1);
  });

  it("shows empty state when expanded with no sessions", () => {
    const h = defaultHandlers();
    render(
      <TaskGroup
        task={makeTask()}
        sessions={[]}
        isExpanded={true}
        activeSessionId={null}
        projectBranch="main"
        {...h}
      />
    );
    expect(screen.getByText("No sessions yet")).toBeTruthy();
  });

  it("does not render body when collapsed", () => {
    const h = defaultHandlers();
    const { container } = render(
      <TaskGroup
        task={makeTask()}
        sessions={[makeSession()]}
        isExpanded={false}
        activeSessionId={null}
        projectBranch="main"
        {...h}
      />
    );
    expect(container.querySelector(".task-group-body")).toBeNull();
  });

  it("renders session count when sessions exist", () => {
    const h = defaultHandlers();
    render(
      <TaskGroup
        task={makeTask()}
        sessions={[makeSession({ id: 1 }), makeSession({ id: 2, status: "exited" })]}
        isExpanded={false}
        activeSessionId={null}
        projectBranch="main"
        {...h}
      />
    );
    expect(screen.getByText("2")).toBeTruthy();
  });

  it("delete button shows 'Kill N + remove' label when running sessions exist", () => {
    const h = defaultHandlers();
    const { container } = render(
      <TaskGroup
        task={makeTask()}
        sessions={[
          makeSession({ id: 1, status: "running" }),
          makeSession({ id: 2, status: "running" }),
        ]}
        isExpanded={false}
        activeSessionId={null}
        projectBranch="main"
        {...h}
      />
    );
    fireEvent.click(container.querySelector(".task-group-btn-danger")!);
    expect(screen.getByText("Kill 2 + remove")).toBeTruthy();
  });

  it("delete button shows 'Remove' when no running sessions", () => {
    const h = defaultHandlers();
    const { container } = render(
      <TaskGroup
        task={makeTask()}
        sessions={[]}
        isExpanded={false}
        activeSessionId={null}
        projectBranch="main"
        {...h}
      />
    );
    fireEvent.click(container.querySelector(".task-group-btn-danger")!);
    expect(screen.getByText("Remove")).toBeTruthy();
  });

  it("calls onDelete after confirm, not on cancel", () => {
    const h = defaultHandlers();
    const { container } = render(
      <TaskGroup
        task={makeTask()}
        sessions={[]}
        isExpanded={false}
        activeSessionId={null}
        projectBranch="main"
        {...h}
      />
    );
    fireEvent.click(container.querySelector(".task-group-btn-danger")!);
    fireEvent.click(screen.getByText("Cancel"));
    expect(h.onDelete).not.toHaveBeenCalled();

    fireEvent.click(container.querySelector(".task-group-btn-danger")!);
    fireEvent.click(screen.getByText("Remove"));
    expect(h.onDelete).toHaveBeenCalledTimes(1);
  });

  it("action buttons trigger correct handlers", () => {
    const h = defaultHandlers();
    const { container } = render(
      <TaskGroup
        task={makeTask()}
        sessions={[]}
        isExpanded={false}
        activeSessionId={null}
        projectBranch="main"
        {...h}
      />
    );
    const actionBtns = container.querySelectorAll(".task-group-actions .task-group-btn");
    // 3 buttons: add agent (+), instant shell (>), delete (×)
    expect(actionBtns.length).toBe(3);
    fireEvent.click(actionBtns[0]);
    expect(h.onAddAgent).toHaveBeenCalledTimes(1);
    fireEvent.click(actionBtns[1]);
    expect(h.onInstantShell).toHaveBeenCalledTimes(1);
    // onToggle should not fire from action button clicks (stopPropagation)
    expect(h.onToggle).not.toHaveBeenCalled();
  });

  it("clicking action buttons does not toggle expansion", () => {
    const h = defaultHandlers();
    const { container } = render(
      <TaskGroup
        task={makeTask()}
        sessions={[]}
        isExpanded={false}
        activeSessionId={null}
        projectBranch="main"
        {...h}
      />
    );
    fireEvent.click(container.querySelectorAll(".task-group-actions .task-group-btn")[0]);
    expect(h.onToggle).not.toHaveBeenCalled();
  });
});
