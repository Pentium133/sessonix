import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
  onAction,
} from "@tauri-apps/plugin-notification";
import { showToast } from "../components/Toast";
import { useUiStore } from "../store/uiStore";
import { useProjectStore } from "../store/projectStore";
import { useSessionStore } from "../store/sessionStore";
import { getSetting, setSetting } from "./api";
import type { AgentType } from "./types";

const AGENT_LABELS: Record<AgentType, string> = {
  claude: "Claude",
  codex: "Codex",
  gemini: "Gemini",
  opencode: "OpenCode",
  shell: "Shell",
  custom: "Custom",
};

let permissionReady = false;
let hintShown = false;

/** Initialize notifications on app start.
 *  Requests permission and sends a one-time welcome notification
 *  to register the app with macOS Notification Center.
 *  Non-sandboxed macOS apps only appear in System Settings → Notifications
 *  after sending their first notification. */
export async function initNotifications(): Promise<void> {
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      const result = await requestPermission();
      granted = result === "granted";
    }
    permissionReady = granted;

    // Send a one-time notification to register with OS notification center.
    // Skip during onboarding — wizard handles it via "Enable Notifications" button.
    if (permissionReady) {
      const [alreadySent, welcomeDone] = await Promise.all([
        getSetting("notification_registered"),
        getSetting("welcome_completed"),
      ]);
      if (!alreadySent && welcomeDone === "true") {
        sendNotification({
          title: "Sessonix",
          body: "Notifications enabled. You'll be alerted when agents need attention.",
        });
        await setSetting("notification_registered", "true");
      }
    }
  } catch {
    permissionReady = false;
  }
}

/** Explicitly request notification permission from OS. Call from user action (button click). */
export async function requestNotificationPermission(): Promise<boolean> {
  try {
    if (await isPermissionGranted()) {
      permissionReady = true;
      return true;
    }
    const result = await requestPermission();
    permissionReady = result === "granted";
    return permissionReady;
  } catch {
    permissionReady = false;
    return false;
  }
}

/**
 * Send an OS notification for a session event.
 * Suppressed when:
 *  - Window is focused AND session belongs to the active project (user can see it)
 * Fires when:
 *  - Window is not focused (user is in another app)
 *  - Window is focused but session is in a different project (user can't see it)
 *
 * Embeds sessionId and projectPath in `extra` so the click handler
 * can navigate directly to the triggering session.
 */
export function sendSessionNotification(
  title: string,
  body: string,
  sessionWorkingDir?: string,
  sessionId?: number,
): void {
  if (document.hasFocus()) {
    // Window focused — only notify if session is from a different project
    const activeProject = useProjectStore.getState().activeProjectPath;
    if (sessionWorkingDir && activeProject && sessionWorkingDir === activeProject) {
      return; // User is looking at this project, no need
    }
  }

  if (!permissionReady) {
    if (!hintShown) {
      hintShown = true;
      showToast(
        "Enable notifications to get alerts when agents need attention",
        "info",
        { label: "Settings", onClick: () => useUiStore.getState().openSettings() },
      );
    }
    return;
  }

  try {
    sendNotification({
      title,
      body,
      extra: sessionId != null ? { sessionId: String(sessionId) } : undefined,
    });
  } catch {
    // Silently ignore — notification is non-critical
  }
}

let clickHandlerRegistered = false;

/** Listen for notification clicks and navigate to the session that triggered it. */
export function setupNotificationClickHandler(): void {
  if (clickHandlerRegistered) return;
  clickHandlerRegistered = true;

  onAction((notification) => {
    const raw = notification.extra?.sessionId;
    const sessionId = typeof raw === "number" ? raw : parseInt(String(raw ?? ""), 10);
    if (isNaN(sessionId)) return;

    const sessionStore = useSessionStore.getState();
    const projectStore = useProjectStore.getState();

    const session = sessionStore.sessions.find((s) => s.id === sessionId);
    if (!session) return; // session was deleted while notification was pending

    // Switch project if needed (use canonical working_dir from session, not stale notification payload)
    const targetProject = session.working_dir;
    if (targetProject && targetProject !== projectStore.activeProjectPath) {
      if (projectStore.activeProjectPath && sessionStore.activeSessionId != null) {
        projectStore.setLastActiveSession(projectStore.activeProjectPath, sessionStore.activeSessionId);
      }
      projectStore.setActiveProjectPath(targetProject);
    }

    sessionStore.switchSession(sessionId).catch(console.error);
  }).catch(console.error);
}

/** Format agent type to display label. */
export function agentLabel(agentType: AgentType): string {
  return AGENT_LABELS[agentType] ?? agentType;
}
