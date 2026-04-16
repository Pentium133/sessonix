import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

const createTaskMock = vi.fn();

vi.mock("../lib/api", () => ({
  createTask: (...args: unknown[]) => createTaskMock(...args),
  listTasks: vi.fn().mockResolvedValue([]),
  deleteTask: vi.fn().mockResolvedValue({ worktree_warning: null }),
}));

vi.mock("../components/Toast", () => ({
  showToast: vi.fn(),
}));

import TaskCreateModal from "../components/TaskCreateModal";
import { useTaskStore } from "../store/taskStore";
import { showToast } from "../components/Toast";

describe("TaskCreateModal", () => {
  beforeEach(() => {
    createTaskMock.mockReset();
    vi.mocked(showToast).mockClear();
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

  it("auto-fills branch field live as name is typed", () => {
    render(<TaskCreateModal projectPath="/repo" onClose={vi.fn()} />);
    const nameInput = screen.getByPlaceholderText(/Task name/i) as HTMLInputElement;
    const branchInput = screen.getByPlaceholderText("feat/my-task") as HTMLInputElement;

    fireEvent.change(nameInput, { target: { value: "Fix OAuth flow" } });
    expect(branchInput.value).toBe("feat/fix-oauth-flow");

    fireEvent.change(nameInput, { target: { value: "Add WebSocket support" } });
    expect(branchInput.value).toBe("feat/add-websocket-support");
  });

  it("stops auto-filling branch once user edits it manually", () => {
    render(<TaskCreateModal projectPath="/repo" onClose={vi.fn()} />);
    const nameInput = screen.getByPlaceholderText(/Task name/i) as HTMLInputElement;
    const branchInput = screen.getByPlaceholderText("feat/my-task") as HTMLInputElement;

    fireEvent.change(nameInput, { target: { value: "Fix bug" } });
    expect(branchInput.value).toBe("feat/fix-bug");

    // User customizes branch
    fireEvent.change(branchInput, { target: { value: "bugfix/custom-name" } });
    // Typing in name field shouldn't override anymore
    fireEvent.change(nameInput, { target: { value: "Fix another thing" } });
    expect(branchInput.value).toBe("bugfix/custom-name");
  });

  it("transliterates non-Latin task names when auto-filling branch", () => {
    render(<TaskCreateModal projectPath="/repo" onClose={vi.fn()} />);
    const nameInput = screen.getByPlaceholderText(/Task name/i) as HTMLInputElement;
    const branchInput = screen.getByPlaceholderText("feat/my-task") as HTMLInputElement;

    fireEvent.change(nameInput, { target: { value: "Исправить нотификации" } });
    expect(branchInput.value).toBe("feat/ispravit-notifikatsii");

    fireEvent.change(nameInput, { target: { value: "Café résumé" } });
    expect(branchInput.value).toBe("feat/cafe-resume");
  });

  it("falls back to feat/task when name has no representable chars", () => {
    render(<TaskCreateModal projectPath="/repo" onClose={vi.fn()} />);
    const nameInput = screen.getByPlaceholderText(/Task name/i) as HTMLInputElement;
    const branchInput = screen.getByPlaceholderText("feat/my-task") as HTMLInputElement;

    fireEvent.change(nameInput, { target: { value: "🚀🎉" } });
    expect(branchInput.value).toBe("feat/task");
  });

  it("resumes auto-fill after user clears the branch field", () => {
    render(<TaskCreateModal projectPath="/repo" onClose={vi.fn()} />);
    const nameInput = screen.getByPlaceholderText(/Task name/i) as HTMLInputElement;
    const branchInput = screen.getByPlaceholderText("feat/my-task") as HTMLInputElement;

    fireEvent.change(nameInput, { target: { value: "First" } });
    fireEvent.change(branchInput, { target: { value: "custom" } });
    // Clear branch
    fireEvent.change(branchInput, { target: { value: "" } });
    // Auto-fill resumes
    fireEvent.change(nameInput, { target: { value: "Second task" } });
    expect(branchInput.value).toBe("feat/second-task");
  });

  it("shows error toast and keeps modal open on createTask rejection", async () => {
    createTaskMock.mockRejectedValueOnce(new Error("git failed"));
    const onClose = vi.fn();

    render(<TaskCreateModal projectPath="/repo" onClose={onClose} />);
    fireEvent.change(screen.getByPlaceholderText(/Task name/i), {
      target: { value: "Broken task" },
    });
    fireEvent.click(screen.getByText("Create Task"));

    await waitFor(() => expect(showToast).toHaveBeenCalled());
    expect(vi.mocked(showToast).mock.calls[0][0]).toMatch(/Failed to create task/);
    expect(vi.mocked(showToast).mock.calls[0][1]).toBe("error");
    expect(onClose).not.toHaveBeenCalled();
    // Button should be re-enabled after failure so user can retry
    const btn = screen.getByText("Create Task") as HTMLButtonElement;
    expect(btn.disabled).toBe(false);
  });
});
