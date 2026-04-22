import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, fireEvent, cleanup } from "@testing-library/react";
import type { WorktreeDiff } from "../lib/api";

// Stub react-diff-viewer-continued so unit tests don't depend on its DOM output.
vi.mock("react-diff-viewer-continued", () => ({
  default: ({ oldValue, newValue }: { oldValue: string; newValue: string }) => (
    <div data-testid="rdv-mock">
      <pre data-testid="rdv-old">{oldValue}</pre>
      <pre data-testid="rdv-new">{newValue}</pre>
    </div>
  ),
  DiffMethod: { LINES: "LINES" },
}));

const getWorktreeDiffMock = vi.fn();
vi.mock("../lib/api", async () => {
  const actual = await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return {
    ...actual,
    getWorktreeDiff: (...args: unknown[]) => getWorktreeDiffMock(...args),
  };
});

import DiffViewer from "../components/DiffViewer";
import { useProjectStore } from "../store/projectStore";
import { useSessionStore } from "../store/sessionStore";

function resetStores() {
  useProjectStore.setState({
    projects: [{ path: "/tmp/app", name: "app", sessions: [] }],
    activeProjectPath: "/tmp/app",
    lastActiveSession: {},
  });
  useSessionStore.setState({ sessions: [], activeSessionId: null, loaded: true });
}

function diffWith(overrides: Partial<WorktreeDiff>): WorktreeDiff {
  return {
    isRepo: true,
    branch: "main",
    headSha: "abcdef0",
    files: [],
    truncatedFiles: 0,
    ...overrides,
  };
}

describe("DiffViewer", () => {
  beforeEach(() => {
    resetStores();
    getWorktreeDiffMock.mockReset();
  });

  afterEach(() => cleanup());

  it("renders 'Not a git repository' when isRepo=false", async () => {
    getWorktreeDiffMock.mockResolvedValue(diffWith({ isRepo: false }));
    render(<DiffViewer />);
    await waitFor(() => expect(screen.getByText("Not a git repository.")).toBeTruthy());
  });

  it("renders 'No changes' with branch and SHA when files is empty", async () => {
    getWorktreeDiffMock.mockResolvedValue(
      diffWith({ isRepo: true, branch: "feat/x", headSha: "1234567", files: [] })
    );
    render(<DiffViewer />);
    await waitFor(() => expect(screen.getByText("No changes")).toBeTruthy());
    expect(screen.getByText(/feat\/x.*1234567/)).toBeTruthy();
  });

  it("renders error state with Retry when fetch rejects", async () => {
    getWorktreeDiffMock.mockRejectedValueOnce("boom").mockResolvedValueOnce(diffWith({}));
    render(<DiffViewer />);
    await waitFor(() => expect(screen.getByText("Error: boom")).toBeTruthy());
    const retry = screen.getByRole("button", { name: /retry/i });
    fireEvent.click(retry);
    await waitFor(() => expect(screen.getByText("No changes")).toBeTruthy());
    expect(getWorktreeDiffMock).toHaveBeenCalledTimes(2);
  });

  it("auto-selects the first file and renders its diff", async () => {
    getWorktreeDiffMock.mockResolvedValue(
      diffWith({
        files: [
          {
            oldPath: "a.txt",
            newPath: "a.txt",
            status: "modified",
            additions: 1,
            deletions: 1,
            payload: { kind: "text", oldContent: "old\n", newContent: "new\n" },
          },
          {
            oldPath: "",
            newPath: "b.txt",
            status: "added",
            additions: 2,
            deletions: 0,
            payload: { kind: "text", oldContent: "", newContent: "x\ny\n" },
          },
        ],
      })
    );
    render(<DiffViewer />);
    await waitFor(() => expect(screen.getByText("a.txt")).toBeTruthy());
    // First file's content should be in the mocked diff viewer.
    expect(screen.getByTestId("rdv-old").textContent).toBe("old\n");
    expect(screen.getByTestId("rdv-new").textContent).toBe("new\n");
  });

  it("switches selected file on list click", async () => {
    getWorktreeDiffMock.mockResolvedValue(
      diffWith({
        files: [
          {
            oldPath: "a.txt",
            newPath: "a.txt",
            status: "modified",
            additions: 1,
            deletions: 1,
            payload: { kind: "text", oldContent: "old\n", newContent: "new\n" },
          },
          {
            oldPath: "",
            newPath: "b.txt",
            status: "added",
            additions: 2,
            deletions: 0,
            payload: { kind: "text", oldContent: "", newContent: "xx\nyy\n" },
          },
        ],
      })
    );
    render(<DiffViewer />);
    await waitFor(() => expect(screen.getByText("b.txt")).toBeTruthy());
    fireEvent.click(screen.getByRole("option", { name: /b\.txt/ }));
    expect(screen.getByTestId("rdv-new").textContent).toBe("xx\nyy\n");
  });

  it("renders Binary stub for binary files", async () => {
    getWorktreeDiffMock.mockResolvedValue(
      diffWith({
        files: [
          {
            oldPath: "",
            newPath: "img.png",
            status: "added",
            additions: 0,
            deletions: 0,
            payload: { kind: "binary" },
          },
        ],
      })
    );
    render(<DiffViewer />);
    await waitFor(() =>
      expect(screen.getByText(/Binary file — contents not shown/)).toBeTruthy()
    );
  });

  it("renders TooLarge stub with size", async () => {
    getWorktreeDiffMock.mockResolvedValue(
      diffWith({
        files: [
          {
            oldPath: "",
            newPath: "huge.txt",
            status: "added",
            additions: 0,
            deletions: 0,
            payload: { kind: "tooLarge", sizeBytes: 2_500_000 },
          },
        ],
      })
    );
    render(<DiffViewer />);
    await waitFor(() =>
      expect(screen.getByText(/File too large .*2\.4 MB.*not displayed/)).toBeTruthy()
    );
  });

  it("renders the truncation banner when truncatedFiles > 0", async () => {
    getWorktreeDiffMock.mockResolvedValue(
      diffWith({
        truncatedFiles: 23,
        files: [
          {
            oldPath: "",
            newPath: "a.txt",
            status: "added",
            additions: 1,
            deletions: 0,
            payload: { kind: "text", oldContent: "", newContent: "x\n" },
          },
        ],
      })
    );
    render(<DiffViewer />);
    await waitFor(() => expect(screen.getByText(/23 more files hidden/)).toBeTruthy());
  });

  it("resolves workingDir to last-focused session's worktree when present", async () => {
    useProjectStore.setState({
      projects: [{ path: "/tmp/app", name: "app", sessions: [7] }],
      activeProjectPath: "/tmp/app",
      lastActiveSession: { "/tmp/app": 7 },
    });
    useSessionStore.setState({
      sessions: [
        {
          id: 7,
          command: "claude",
          args: [],
          working_dir: "/tmp/app",
          task_name: "t",
          agent_type: "claude",
          status: "running",
          status_line: "",
          created_at: 0,
          sortOrder: 1,
          gitStatus: null,
          worktree_path: "/tmp/app/.sessonix-worktrees/t",
          base_commit: null,
          initial_prompt: null,
          task_id: null,
    telegramEnabled: false,
        },
      ],
      activeSessionId: null,
      loaded: true,
    });
    getWorktreeDiffMock.mockResolvedValue(diffWith({ files: [] }));
    render(<DiffViewer />);
    await waitFor(() => expect(getWorktreeDiffMock).toHaveBeenCalled());
    expect(getWorktreeDiffMock).toHaveBeenCalledWith("/tmp/app/.sessonix-worktrees/t");
  });

  it("falls back to activeProjectPath when no last-focused session", async () => {
    getWorktreeDiffMock.mockResolvedValue(diffWith({ files: [] }));
    render(<DiffViewer />);
    await waitFor(() => expect(getWorktreeDiffMock).toHaveBeenCalled());
    expect(getWorktreeDiffMock).toHaveBeenCalledWith("/tmp/app");
  });
});
