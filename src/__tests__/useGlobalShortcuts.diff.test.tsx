import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { fireEvent } from "@testing-library/react";

vi.mock("../lib/api", () => ({
  createSession: vi.fn(),
  killSession: vi.fn(),
  deleteSession: vi.fn(),
  attachSession: vi.fn().mockResolvedValue([]),
  detachSession: vi.fn(),
  listProjects: vi.fn().mockResolvedValue([]),
  listSessions: vi.fn().mockResolvedValue([]),
  installClaudeHooks: vi.fn(),
  checkClaudeHooks: vi.fn(),
  reorderSession: vi.fn(),
  setSortOrder: vi.fn(),
}));

vi.mock("../lib/terminalPool", () => ({
  writeToTerminal: vi.fn(),
}));

import { useGlobalShortcuts } from "../hooks/useGlobalShortcuts";
import { useSessionStore, DIFF_PSEUDO_ID } from "../store/sessionStore";
import { useProjectStore } from "../store/projectStore";
import { useUiStore } from "../store/uiStore";

describe("Cmd+0 shortcut", () => {
  beforeEach(() => {
    useSessionStore.setState({ sessions: [], activeSessionId: null, loaded: true });
    useProjectStore.setState({
      projects: [{ path: "/tmp/app", name: "app", sessions: [] }],
      activeProjectPath: "/tmp/app",
      lastActiveSession: {},
    });
  });

  it("Cmd+0 with active project → switches to Diff pseudo-session", () => {
    const switchSession = vi.fn();
    useSessionStore.setState({ switchSession });
    renderHook(() => useGlobalShortcuts());

    fireEvent.keyDown(window, { key: "0", metaKey: true });
    expect(switchSession).toHaveBeenCalledWith(DIFF_PSEUDO_ID);
  });

  it("Cmd+0 with no active project → no-op", () => {
    useProjectStore.setState({ activeProjectPath: null });
    const switchSession = vi.fn();
    useSessionStore.setState({ switchSession });
    renderHook(() => useGlobalShortcuts());

    fireEvent.keyDown(window, { key: "0", metaKey: true });
    expect(switchSession).not.toHaveBeenCalled();
  });

  it("Cmd+Shift+0 still resets zoom", () => {
    const resetZoom = vi.fn();
    const switchSession = vi.fn();
    useSessionStore.setState({ switchSession });
    useUiStore.setState({ resetZoom });
    renderHook(() => useGlobalShortcuts());

    fireEvent.keyDown(window, { key: "0", metaKey: true, shiftKey: true });
    expect(resetZoom).toHaveBeenCalled();
    expect(switchSession).not.toHaveBeenCalled();
  });
});
