import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import SummaryBar from "../components/SummaryBar";
import { useSessionStore } from "../store/sessionStore";
import { useProjectStore } from "../store/projectStore";
import type { Session } from "../lib/types";

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 1,
    command: "claude",
    args: [],
    working_dir: "/tmp",
    task_name: "Test Session",
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

function setStore(sessions: Session[], activeSessionId: number | null = null) {
  useSessionStore.setState({ sessions, activeSessionId });
  // SummaryBar filters by active project — set it to match session working_dir
  useProjectStore.setState({ activeProjectPath: "/tmp" });
}

describe("SummaryBar", () => {
  beforeEach(() => {
    useSessionStore.setState({ sessions: [], activeSessionId: null, loaded: true });
  });

  it("renders nothing when there are no sessions", () => {
    setStore([]);
    const { container } = render(<SummaryBar />);
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when all sessions are exited", () => {
    setStore([
      makeSession({ id: 1, status: "exited" }),
      makeSession({ id: 2, status: "error" }),
    ]);
    const { container } = render(<SummaryBar />);
    expect(container.firstChild).toBeNull();
  });

  it("renders session task names as tab items", () => {
    setStore([
      makeSession({ id: 1, task_name: "Alpha" }),
      makeSession({ id: 2, task_name: "Beta", sortOrder: 2 }),
    ]);
    render(<SummaryBar />);
    expect(screen.getByText("Alpha")).toBeTruthy();
    expect(screen.getByText("Beta")).toBeTruthy();
  });

  it("renders idle sessions alongside running sessions", () => {
    setStore([
      makeSession({ id: 1, task_name: "Running Task", status: "running" }),
      makeSession({ id: 2, task_name: "Idle Task", status: "idle", sortOrder: 2 }),
    ]);
    render(<SummaryBar />);
    expect(screen.getByText("Running Task")).toBeTruthy();
    expect(screen.getByText("Idle Task")).toBeTruthy();
  });

  it("applies active class to the active session tab", () => {
    setStore([
      makeSession({ id: 1, task_name: "First" }),
      makeSession({ id: 2, task_name: "Second", sortOrder: 2 }),
    ], 1);
    const { container } = render(<SummaryBar />);
    const activeButtons = container.querySelectorAll(".summary-item.active");
    expect(activeButtons).toHaveLength(1);
    expect(activeButtons[0].textContent).toContain("First");
  });

  it("does not apply active class when activeSessionId is null", () => {
    setStore([makeSession({ id: 1, task_name: "Task" })]);
    const { container } = render(<SummaryBar />);
    expect(container.querySelectorAll(".summary-item.active")).toHaveLength(0);
  });

  it("does not apply active class to inactive tabs", () => {
    setStore([
      makeSession({ id: 1, task_name: "Active" }),
      makeSession({ id: 2, task_name: "Inactive", sortOrder: 2 }),
    ], 1);
    const { container } = render(<SummaryBar />);
    const inactiveButtons = container.querySelectorAll(
      ".summary-item:not(.active)"
    );
    expect(inactiveButtons).toHaveLength(1);
    expect(inactiveButtons[0].textContent).toContain("Inactive");
  });

  it("calls switchSession with the session id when a tab is clicked", () => {
    const switchSession = vi.fn();
    setStore([makeSession({ id: 42, task_name: "Click Me" })]);
    useSessionStore.setState({ switchSession });
    render(<SummaryBar />);
    fireEvent.click(screen.getByText("Click Me"));
    expect(switchSession).toHaveBeenCalledWith(42);
  });

  it("shows status_line text in the tab when present", () => {
    setStore([
      makeSession({ id: 1, task_name: "Worker", status_line: "Thinking..." }),
    ]);
    render(<SummaryBar />);
    expect(screen.getByText("Thinking...")).toBeTruthy();
  });

  it("does not render status element when status_line is empty", () => {
    setStore([makeSession({ id: 1, task_name: "Worker", status_line: "" })]);
    const { container } = render(<SummaryBar />);
    expect(container.querySelectorAll(".summary-status")).toHaveLength(0);
  });

  it("limits visible tabs to 5 and shows overflow count", () => {
    const sessions = Array.from({ length: 7 }, (_, i) =>
      makeSession({ id: i + 1, task_name: `Session ${i + 1}`, sortOrder: i + 1 })
    );
    setStore(sessions);
    render(<SummaryBar />);
    expect(screen.queryByText("Session 6")).toBeNull();
    expect(screen.queryByText("Session 7")).toBeNull();
    expect(screen.getByText("+2 more")).toBeTruthy();
  });

  it("does not show overflow indicator when 5 or fewer sessions", () => {
    const sessions = Array.from({ length: 5 }, (_, i) =>
      makeSession({ id: i + 1, task_name: `Session ${i + 1}`, sortOrder: i + 1 })
    );
    setStore(sessions);
    const { container } = render(<SummaryBar />);
    expect(container.querySelectorAll(".summary-overflow")).toHaveLength(0);
  });

  it("exited sessions are excluded from the visible tab count", () => {
    setStore([
      makeSession({ id: 1, task_name: "Active", status: "running" }),
      makeSession({ id: 2, task_name: "Dead", status: "exited", sortOrder: 2 }),
    ]);
    render(<SummaryBar />);
    expect(screen.getByText("Active")).toBeTruthy();
    expect(screen.queryByText("Dead")).toBeNull();
  });
});
