import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import SessionLauncher from "../components/SessionLauncher";

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
    expect(screen.getByText("+")).toBeTruthy();
    expect(container.querySelectorAll(".launcher-pill .agent-icon")).toHaveLength(5);
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
});
