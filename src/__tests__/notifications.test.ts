import { describe, it, expect, vi, beforeAll, beforeEach } from "vitest";
import { listen } from "@tauri-apps/api/event";
import { sendNotification } from "@tauri-apps/plugin-notification";
import { sendSessionNotificationNative } from "../lib/api";
import {
  initNotifications,
  sendSessionNotification,
  setupNotificationClickHandler,
} from "../lib/notifications";

const { sessionState, projectState } = vi.hoisted(() => ({
  sessionState: {
    sessions: [] as { id: number; working_dir: string }[],
    activeSessionId: null as number | null,
    switchSession: vi.fn(() => Promise.resolve()),
  },
  projectState: {
    activeProjectPath: null as string | null,
    setActiveProjectPath: vi.fn(),
    setLastActiveSession: vi.fn(),
  },
}));

vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(() => Promise.resolve(true)),
  requestPermission: vi.fn(),
  sendNotification: vi.fn(),
}));

vi.mock("../lib/api", () => ({
  getSetting: vi.fn(() => Promise.resolve("true")),
  setSetting: vi.fn(),
  sendSessionNotificationNative: vi.fn(() => Promise.resolve()),
}));

vi.mock("../components/Toast", () => ({ showToast: vi.fn() }));
vi.mock("../store/uiStore", () => ({ useUiStore: { getState: () => ({}) } }));
vi.mock("../store/sessionStore", () => ({ useSessionStore: { getState: () => sessionState } }));
vi.mock("../store/projectStore", () => ({ useProjectStore: { getState: () => projectState } }));

type ClickHandler = (event: { payload: { sessionId: number } }) => void;

function clickHandler(): ClickHandler {
  const call = vi.mocked(listen).mock.calls.find(([name]) => name === "notification-clicked");
  if (!call) throw new Error("notification-clicked listener not registered");
  return call[1] as unknown as ClickHandler;
}

beforeAll(async () => {
  await initNotifications();
  setupNotificationClickHandler();
});

beforeEach(() => {
  vi.mocked(sendNotification).mockClear();
  vi.mocked(sendSessionNotificationNative).mockClear();
  sessionState.switchSession.mockClear();
  projectState.setActiveProjectPath.mockClear();
  projectState.setLastActiveSession.mockClear();
  sessionState.sessions = [
    { id: 1, working_dir: "/proj/a" },
    { id: 2, working_dir: "/proj/b" },
  ];
  sessionState.activeSessionId = 1;
  projectState.activeProjectPath = "/proj/a";
  vi.spyOn(document, "hasFocus").mockReturnValue(false);
});

describe("sendSessionNotification", () => {
  it("sends through the backend command when a sessionId is given", () => {
    sendSessionNotification("Claude", "Ready for input", "/proj/b", 2);
    expect(sendSessionNotificationNative).toHaveBeenCalledWith("Claude", "Ready for input", 2);
    expect(sendNotification).not.toHaveBeenCalled();
  });

  it("swallows backend rejection", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.mocked(sendSessionNotificationNative).mockRejectedValueOnce("boom");
    expect(() => sendSessionNotification("Claude", "x", "/proj/b", 2)).not.toThrow();
    await Promise.resolve();
    expect(error).toHaveBeenCalled();
    error.mockRestore();
  });

  it("falls back to the plugin without a sessionId", () => {
    sendSessionNotification("Sessonix", "hello");
    expect(sendNotification).toHaveBeenCalledWith({ title: "Sessonix", body: "hello" });
    expect(sendSessionNotificationNative).not.toHaveBeenCalled();
  });
});

describe("setupNotificationClickHandler", () => {
  it("registers the listener only once", () => {
    setupNotificationClickHandler();
    const calls = vi.mocked(listen).mock.calls.filter(([name]) => name === "notification-clicked");
    expect(calls).toHaveLength(1);
  });

  it("switches project and session for a session in another project", () => {
    clickHandler()({ payload: { sessionId: 2 } });
    expect(projectState.setLastActiveSession).toHaveBeenCalledWith("/proj/a", 1);
    expect(projectState.setActiveProjectPath).toHaveBeenCalledWith("/proj/b");
    expect(sessionState.switchSession).toHaveBeenCalledWith(2);
  });

  it("only switches session within the active project", () => {
    sessionState.activeSessionId = 2;
    sessionState.sessions.push({ id: 3, working_dir: "/proj/a" });
    clickHandler()({ payload: { sessionId: 3 } });
    expect(projectState.setActiveProjectPath).not.toHaveBeenCalled();
    expect(sessionState.switchSession).toHaveBeenCalledWith(3);
  });

  it("ignores unknown sessions", () => {
    clickHandler()({ payload: { sessionId: 99 } });
    expect(projectState.setActiveProjectPath).not.toHaveBeenCalled();
    expect(sessionState.switchSession).not.toHaveBeenCalled();
  });
});
