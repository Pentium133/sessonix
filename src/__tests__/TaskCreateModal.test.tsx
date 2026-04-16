import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

const createTaskMock = vi.fn();

vi.mock("../lib/api", () => ({
  createTask: (...args: unknown[]) => createTaskMock(...args),
  listTasks: vi.fn().mockResolvedValue([]),
  deleteTask: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../components/Toast", () => ({
  showToast: vi.fn(),
}));

import TaskCreateModal from "../components/TaskCreateModal";
import { useTaskStore } from "../store/taskStore";

describe("TaskCreateModal", () => {
  beforeEach(() => {
    createTaskMock.mockReset();
    useTaskStore.setState({ tasks: [], loaded: true });
  });

  it("creates a task with explicit branch on Enter", async () => {
    createTaskMock.mockResolvedValueOnce({
      id: 1, projectId: 1, name: "Fix auth", branch: "feat/fix-auth",
      worktreePath: "/repo/.sessonix-worktrees/feat-fix-auth",
      baseCommit: "abc123", createdAt: 0,
    });
    const onClose = vi.fn();

    render(<TaskCreateModal projectPath="/repo" onClose={onClose} />);

    const nameInput = screen.getByPlaceholderText(/Task name/i);
    fireEvent.change(nameInput, { target: { value: "Fix auth" } });
    fireEvent.keyDown(nameInput, { key: "Enter" });

    await waitFor(() =>
      expect(createTaskMock).toHaveBeenCalledWith({
        project_path: "/repo",
        name: "Fix auth",
        branch_name: "feat/fix-auth",
      })
    );
    expect(onClose).toHaveBeenCalled();
    expect(useTaskStore.getState().tasks).toHaveLength(1);
    expect(useTaskStore.getState().tasks[0].name).toBe("Fix auth");
  });

  it("falls back to slugified branch when branch field is empty", async () => {
    createTaskMock.mockResolvedValueOnce({
      id: 2, projectId: 1, name: "Weird NAME!!", branch: "feat/weird-name",
      worktreePath: null, baseCommit: null, createdAt: 0,
    });
    const onClose = vi.fn();

    render(<TaskCreateModal projectPath="/repo" onClose={onClose} />);

    fireEvent.change(screen.getByPlaceholderText(/Task name/i), {
      target: { value: "Weird NAME!!" },
    });
    fireEvent.click(screen.getByText("Create Task"));

    await waitFor(() =>
      expect(createTaskMock).toHaveBeenCalledWith({
        project_path: "/repo",
        name: "Weird NAME!!",
        branch_name: "feat/weird-name",
      })
    );
  });

  it("closes on Escape without calling createTask", () => {
    const onClose = vi.fn();
    render(<TaskCreateModal projectPath="/repo" onClose={onClose} />);
    fireEvent.keyDown(screen.getByPlaceholderText(/Task name/i), { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
    expect(createTaskMock).not.toHaveBeenCalled();
  });

  it("keeps submit disabled when name is empty", () => {
    const onClose = vi.fn();
    render(<TaskCreateModal projectPath="/repo" onClose={onClose} />);
    const btn = screen.getByText("Create Task") as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
  });
});
