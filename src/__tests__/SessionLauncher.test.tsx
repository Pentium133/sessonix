import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import SessionLauncher from "../components/SessionLauncher";
import { useTaskStore } from "../store/taskStore";
import type { Task } from "../lib/types";

describe("SessionLauncher", () => {
  const defaultProps = {
    mode: "session" as const,
    isOpen: true,
    onClose: vi.fn(),
    projectPath: "/home/user/myapp",
    onLaunch: vi.fn(),
  };

  it("renders when open", () => {
    render(<SessionLauncher {...defaultProps} />);
    expect(screen.getByText("New Session")).toBeTruthy();
  });

  it("shows project name in badge", () => {
    render(<SessionLauncher {...defaultProps} />);
    expect(screen.getByText("myapp")).toBeTruthy();
  });

  it("does not render when closed", () => {
    render(<SessionLauncher {...defaultProps} isOpen={false} />);
    expect(screen.queryByText("New Session")).toBeNull();
  });

  it("shows agent pills", () => {
    const { container } = render(<SessionLauncher {...defaultProps} />);
    expect(screen.getByText("shell")).toBeTruthy();
    expect(screen.getByText("claude")).toBeTruthy();
    expect(screen.getByText("gemini")).toBeTruthy();
    expect(screen.getByText("codex")).toBeTruthy();
    expect(screen.getByText("opencode")).toBeTruthy();
    expect(screen.getByText("+")).toBeTruthy();
    expect(container.querySelectorAll(".launcher-pill .agent-icon")).toHaveLength(6);
  });

  it("defaults to claude agent", () => {
    render(<SessionLauncher {...defaultProps} />);
    // Claude options should be visible by default
    expect(screen.getByText("Claude Options")).toBeTruthy();
  });

  it("hides Claude options when shell selected", () => {
    render(<SessionLauncher {...defaultProps} />);
    fireEvent.click(screen.getByText("shell"));
    expect(screen.queryByText("Claude Options")).toBeNull();
  });

  it("shows custom command input when + selected", () => {
    render(<SessionLauncher {...defaultProps} />);
    fireEvent.click(screen.getByText("+"));
    expect(screen.getByPlaceholderText("Command (e.g. aider, cursor)")).toBeTruthy();
  });

  it("disables Launch when custom agent has no command", () => {
    render(<SessionLauncher {...defaultProps} />);
    fireEvent.click(screen.getByText("+"));
    const launchBtn = screen.getByText("Launch");
    expect((launchBtn as HTMLButtonElement).disabled).toBe(true);
  });

  it("calls onLaunch with claude defaults", () => {
    const onLaunch = vi.fn();
    render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
    fireEvent.click(screen.getByText("Launch"));
    expect(onLaunch).toHaveBeenCalledWith({
      command: "claude",
      args: [],
      working_dir: "/home/user/myapp",
      task_name: "claude session",
      agent_type: "claude",
    });
  });

  it("calls onLaunch with custom task name", () => {
    const onLaunch = vi.fn();
    render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
    const input = screen.getByPlaceholderText("Session name (optional)");
    fireEvent.change(input, { target: { value: "Fix auth bug" } });
    fireEvent.click(screen.getByText("Launch"));
    expect(onLaunch).toHaveBeenCalledWith(
      expect.objectContaining({ task_name: "Fix auth bug" })
    );
  });

  it("calls onLaunch with shell agent", () => {
    const onLaunch = vi.fn();
    render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
    fireEvent.click(screen.getByText("shell"));
    fireEvent.click(screen.getByText("Launch"));
    expect(onLaunch).toHaveBeenCalledWith(
      expect.objectContaining({ command: "zsh", agent_type: "shell" })
    );
  });

  it("passes --dangerously-skip-permissions flag when skip permissions checked", () => {
    const onLaunch = vi.fn();
    render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
    const checkbox = screen.getByLabelText("Skip permissions");
    fireEvent.click(checkbox);
    fireEvent.click(screen.getByText("Launch"));
    expect(onLaunch).toHaveBeenCalledWith(
      expect.objectContaining({ args: ["--dangerously-skip-permissions"] })
    );
  });

  it("passes --continue for continue mode", () => {
    const onLaunch = vi.fn();
    render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
    fireEvent.click(screen.getByText("Continue"));
    fireEvent.click(screen.getByText("Launch"));
    expect(onLaunch).toHaveBeenCalledWith(
      expect.objectContaining({ args: ["--continue"] })
    );
  });

  it("passes --resume with session ID for resume mode", () => {
    const onLaunch = vi.fn();
    render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
    fireEvent.click(screen.getByText("Resume"));
    const idInput = screen.getByPlaceholderText("Session ID (uuid)");
    fireEvent.change(idInput, { target: { value: "abc-123" } });
    fireEvent.click(screen.getByText("Launch"));
    expect(onLaunch).toHaveBeenCalledWith(
      expect.objectContaining({ args: ["--resume", "abc-123"] })
    );
  });

  it("calls onClose on Cancel", () => {
    const onClose = vi.fn();
    render(<SessionLauncher {...defaultProps} onClose={onClose} />);
    fireEvent.click(screen.getByText("Cancel"));
    expect(onClose).toHaveBeenCalled();
  });

  it("calls onClose on Escape", () => {
    const onClose = vi.fn();
    render(<SessionLauncher {...defaultProps} onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("launches on Enter in task name field", () => {
    const onLaunch = vi.fn();
    render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
    const input = screen.getByPlaceholderText("Session name (optional)");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onLaunch).toHaveBeenCalled();
  });

  it("shows Codex Options when codex selected", () => {
    render(<SessionLauncher {...defaultProps} />);
    fireEvent.click(screen.getByText("codex"));
    expect(screen.getByText("Codex Options")).toBeTruthy();
    expect(screen.queryByText("Claude Options")).toBeNull();
  });

  it("launches codex with resume --last for Last mode", () => {
    const onLaunch = vi.fn();
    render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
    fireEvent.click(screen.getByText("codex"));
    fireEvent.click(screen.getByText("Last"));
    fireEvent.click(screen.getByText("Launch"));
    expect(onLaunch).toHaveBeenCalledWith(
      expect.objectContaining({
        command: "codex",
        args: ["resume", "--last"],
        agent_type: "codex",
      })
    );
  });

  it("launches codex with resume <id> for Resume mode", () => {
    const onLaunch = vi.fn();
    render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
    fireEvent.click(screen.getByText("codex"));
    // There are multiple "Resume" elements (Claude radio + Codex radio),
    // click the one inside Codex Options
    const codexResume = screen.getAllByText("Resume").find((el) =>
      el.closest(".launcher-claude-options")?.textContent?.includes("Codex")
    );
    fireEvent.click(codexResume!);
    const idInput = screen.getByPlaceholderText("Thread ID (uuid)");
    fireEvent.change(idInput, { target: { value: "thr-abc-123" } });
    fireEvent.click(screen.getByText("Launch"));
    expect(onLaunch).toHaveBeenCalledWith(
      expect.objectContaining({
        command: "codex",
        args: ["resume", "thr-abc-123"],
        agent_type: "codex",
      })
    );
  });

  it("launches codex with no args for New mode", () => {
    const onLaunch = vi.fn();
    render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
    fireEvent.click(screen.getByText("codex"));
    fireEvent.click(screen.getByText("Launch"));
    expect(onLaunch).toHaveBeenCalledWith(
      expect.objectContaining({
        command: "codex",
        args: [],
        agent_type: "codex",
      })
    );
  });

  describe("OpenCode", () => {
    it("shows OpenCode in agent pills", () => {
      render(<SessionLauncher {...defaultProps} />);
      expect(screen.getByText("opencode")).toBeTruthy();
    });

    it("shows OpenCode Options when opencode selected", () => {
      render(<SessionLauncher {...defaultProps} />);
      fireEvent.click(screen.getByText("opencode"));
      expect(screen.getByText("OpenCode Options")).toBeTruthy();
      expect(screen.queryByText("Claude Options")).toBeNull();
    });

    it("launches opencode with empty args for New mode (no prompt, TUI)", () => {
      const onLaunch = vi.fn();
      render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
      fireEvent.click(screen.getByText("opencode"));
      fireEvent.click(screen.getByText("Launch"));
      expect(onLaunch).toHaveBeenCalledWith(
        expect.objectContaining({
          command: "opencode",
          args: [],
          agent_type: "opencode",
        })
      );
    });

    it("passes prompt via --prompt flag for New mode", () => {
      const onLaunch = vi.fn();
      render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
      fireEvent.click(screen.getByText("opencode"));
      const promptInput = screen.getByPlaceholderText(/Enter a task for the agent/);
      fireEvent.change(promptInput, { target: { value: "what is 2+2?" } });
      fireEvent.click(screen.getByText("Launch"));
      expect(onLaunch).toHaveBeenCalledWith(
        expect.objectContaining({
          command: "opencode",
          args: ["--prompt", "what is 2+2?"],
          agent_type: "opencode",
        })
      );
    });

    it("launches opencode with --continue for Last mode", () => {
      const onLaunch = vi.fn();
      render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
      fireEvent.click(screen.getByText("opencode"));
      const opencodeLast = screen.getAllByText("Last").find((el) =>
        el.closest(".launcher-claude-options")?.textContent?.includes("OpenCode")
      );
      fireEvent.click(opencodeLast!);
      fireEvent.click(screen.getByText("Launch"));
      expect(onLaunch).toHaveBeenCalledWith(
        expect.objectContaining({
          command: "opencode",
          args: ["--continue"],
          agent_type: "opencode",
        })
      );
    });

    it("launches opencode with --session <id> for Resume mode", () => {
      const onLaunch = vi.fn();
      render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);
      fireEvent.click(screen.getByText("opencode"));
      const opencodeResume = screen.getAllByText("Resume").find((el) =>
        el.closest(".launcher-claude-options")?.textContent?.includes("OpenCode")
      );
      fireEvent.click(opencodeResume!);
      const idInput = screen.getByPlaceholderText("Session ID (ses_xxx)");
      fireEvent.change(idInput, { target: { value: "ses_abc123" } });
      fireEvent.click(screen.getByText("Launch"));
      expect(onLaunch).toHaveBeenCalledWith(
        expect.objectContaining({
          command: "opencode",
          args: ["--session", "ses_abc123"],
          agent_type: "opencode",
        })
      );
    });

    it("disables Launch for OpenCode Resume mode without ID", () => {
      render(<SessionLauncher {...defaultProps} />);
      fireEvent.click(screen.getByText("opencode"));
      const opencodeResume = screen.getAllByText("Resume").find((el) =>
        el.closest(".launcher-claude-options")?.textContent?.includes("OpenCode")
      );
      fireEvent.click(opencodeResume!);
      const launchBtn = screen.getByText("Launch");
      expect((launchBtn as HTMLButtonElement).disabled).toBe(true);
    });

    it("clears resumeSessionId when switching from OpenCode to Codex pill", () => {
      const onLaunch = vi.fn();
      render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} />);

      // Enter OpenCode Resume mode with a session ID
      fireEvent.click(screen.getByText("opencode"));
      const opencodeResume = screen.getAllByText("Resume").find((el) =>
        el.closest(".launcher-claude-options")?.textContent?.includes("OpenCode")
      );
      fireEvent.click(opencodeResume!);
      fireEvent.change(screen.getByPlaceholderText("Session ID (ses_xxx)"), {
        target: { value: "ses_leak" },
      });

      // Switch to Codex — resumeSessionId state is shared, must be cleared
      fireEvent.click(screen.getByText("codex"));
      fireEvent.click(screen.getByText("Launch"));

      // Codex default is "new" mode → args should be empty, not carry ses_leak
      expect(onLaunch).toHaveBeenCalledWith(
        expect.objectContaining({ command: "codex", args: [] })
      );
    });
  });

  describe("Launch inside task", () => {
    const task: Task = {
      id: 42,
      projectId: 1,
      name: "Fix auth",
      branch: "feat/fix-auth",
      worktreePath: "/repo/.sessonix-worktrees/feat-fix-auth",
      baseCommit: "abc",
      createdAt: 0,
    };

    beforeEach(() => {
      useTaskStore.setState({ tasks: [task], loaded: true });
    });

    it("shows 'In task' badge when taskId prop resolves to a task", () => {
      render(<SessionLauncher {...defaultProps} taskId={42} />);
      expect(screen.getByText(/In task:/)).toBeTruthy();
      expect(screen.getByText("Fix auth")).toBeTruthy();
      expect(screen.getByText("feat/fix-auth")).toBeTruthy();
    });

    it("hides worktree toggle when taskId is set", () => {
      render(<SessionLauncher {...defaultProps} taskId={42} />);
      expect(screen.queryByText(/Run in isolated worktree/)).toBeNull();
    });

    it("passes task_id through to onLaunch", () => {
      const onLaunch = vi.fn();
      render(<SessionLauncher {...defaultProps} onLaunch={onLaunch} taskId={42} />);
      fireEvent.click(screen.getByText("Launch"));
      expect(onLaunch).toHaveBeenCalledWith(
        expect.objectContaining({ task_id: 42 })
      );
    });
  });
});
