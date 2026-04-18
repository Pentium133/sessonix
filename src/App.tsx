import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import ProjectRail from "./components/ProjectRail";
import Sidebar from "./components/Sidebar";
import SummaryBar from "./components/SummaryBar";
import TerminalPane from "./components/TerminalPane";
import DiffViewer from "./components/DiffViewer";
import StatusBar from "./components/StatusBar";
import SessionLauncher from "./components/SessionLauncher";
import SettingsModal from "./components/SettingsModal";
import WelcomeWizard from "./components/WelcomeWizard";
import ToastContainer, { showToast } from "./components/Toast";
import ErrorBoundary from "./components/ErrorBoundary";
import { usePtyOutput } from "./hooks/usePtyOutput";
import { useStatusPolling } from "./hooks/useStatusPolling";
import { useGlobalShortcuts } from "./hooks/useGlobalShortcuts";
import type { AgentType } from "./lib/types";
import { useSessionStore, DIFF_PSEUDO_ID } from "./store/sessionStore";
import { useProjectStore } from "./store/projectStore";
import { useTaskStore } from "./store/taskStore";
import { useUiStore, initUi } from "./store/uiStore";
import { SIDEBAR_MIN, SIDEBAR_MAX } from "./lib/constants";
import { getSetting, checkForUpdate } from "./lib/api";
import type { UpdateInfo } from "./lib/api";
import { useSettingsStore } from "./store/settingsStore";
import { initNotifications, setupNotificationClickHandler } from "./lib/notifications";
import UpdateModal from "./components/UpdateModal";
import { version } from "../package.json";

function App() {
  // Store subscriptions — narrow selectors to avoid re-rendering on unrelated changes
  const sessions = useSessionStore((s) => s.sessions);
  const sessionIds = useMemo(() => sessions.map((x) => x.id), [sessions]);
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const isActiveSessionExited = useSessionStore(
    (s) => s.sessions.find((x) => x.id === s.activeSessionId)?.status === "exited"
  );
  const loaded = useSessionStore((s) => s.loaded);
  const sidebarWidth = useUiStore((s) => s.sidebarWidth);
  const sidebarCollapsed = useUiStore((s) => s.sidebarCollapsed);
  const closeLauncher = useUiStore((s) => s.closeLauncher);
  const launcher = useUiStore((s) => s.launcher);
  const settingsOpen = useUiStore((s) => s.settingsOpen);
  const activeProjectPath = useProjectStore((s) => s.activeProjectPath);
  const activeProjectName = useProjectStore((s) => {
    const p = s.projects.find((p) => p.path === s.activeProjectPath);
    return p?.name ?? null;
  });

  // Load task groups whenever the active project changes.
  useEffect(() => {
    if (activeProjectPath) {
      useTaskStore.getState().load(activeProjectPath);
    }
  }, [activeProjectPath]);

  // Update window title with version and active project
  useEffect(() => {
    const title = activeProjectName
      ? `Sessonix v${version} — ${activeProjectName}`
      : `Sessonix v${version}`;
    getCurrentWindow().setTitle(title);
  }, [activeProjectName]);

  const [isResizing, setIsResizing] = useState(false);
  const sidebarWidthRef = useRef(sidebarWidth);
  sidebarWidthRef.current = sidebarWidth;

  // null = loading, false = show wizard, true = skip wizard
  const [welcomeCompleted, setWelcomeCompleted] = useState<boolean | null>(null);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);

  const triggerUpdateCheck = useCallback((force: boolean) => {
    checkForUpdate(force).then((info) => {
      if (info) {
        setUpdateInfo(info);
      } else if (force) {
        showToast("You're on the latest version", "info");
      }
    }).catch(() => {
      if (force) showToast("Could not check for updates", "error");
    });
  }, []);

  // Init: check welcome flag, load settings, restore sessions, request notification permission
  useEffect(() => {
    getSetting("welcome_completed").then((v) => {
      setWelcomeCompleted(v === "true");
    });
    useSettingsStore.getState().load();
    useSessionStore.getState().restore();
    const cleanupUi = initUi();
    initNotifications();
    setupNotificationClickHandler();
    // Check for updates silently on startup
    triggerUpdateCheck(false);
    return cleanupUi;
  }, [triggerUpdateCheck]);

  // Side effect hooks (no args needed — they read from stores)
  usePtyOutput();
  useStatusPolling();

  // Auto-collapse sidebar below 900px
  useEffect(() => {
    const mq = window.matchMedia("(max-width: 900px)");
    if (mq.matches) useUiStore.getState().setCollapsed(true);
    const handler = (e: MediaQueryListEvent) => {
      if (e.matches) useUiStore.getState().setCollapsed(true);
    };
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);

  // Sidebar resize via drag
  const handleResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsResizing(true);

    const startX = e.clientX;
    const startWidth = sidebarWidthRef.current;

    const onMouseMove = (ev: MouseEvent) => {
      const newWidth = Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, startWidth + (ev.clientX - startX)));
      useUiStore.getState().setSidebarWidth(newWidth);
    };

    const onMouseUp = () => {
      setIsResizing(false);
      document.removeEventListener("mousemove", onMouseMove);
      document.removeEventListener("mouseup", onMouseUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };

    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    document.addEventListener("mousemove", onMouseMove);
    document.addEventListener("mouseup", onMouseUp);
  }, []);

  const handleResizeDoubleClick = useCallback(() => {
    useUiStore.getState().toggleCollapse();
  }, []);

  // Keyboard shortcuts
  useGlobalShortcuts();

  // Close confirmation when running sessions exist
  useEffect(() => {
    const showConfirm = async () => {
      const { sessions: currentSessions } = useSessionStore.getState();
      const running = currentSessions.filter(
        (s) => s.status === "running" || s.status === "idle"
      );
      if (running.length === 0) {
        await invoke("force_exit");
        return;
      }
      const ok = await confirm(
        `${running.length} session${running.length > 1 ? "s" : ""} still running. Quit anyway?`,
        { title: "Sessonix", kind: "warning" }
      );
      if (ok) {
        await invoke("force_exit");
      }
    };

    const unlistenClose = getCurrentWindow().onCloseRequested(async (event) => {
      event.preventDefault();
      await showConfirm();
    });

    const unlistenQuit = listen("confirm-exit", () => {
      showConfirm();
    });

    const unlistenSettings = listen("open-settings", () => {
      useUiStore.getState().openSettings();
    });

    const unlistenUpdates = listen("check-for-updates", () => {
      triggerUpdateCheck(true);
    });

    return () => {
      unlistenClose.then((fn) => fn());
      unlistenQuit.then((fn) => fn());
      unlistenSettings.then((fn) => fn());
      unlistenUpdates.then((fn) => fn());
    };
  }, []);

  const handleLaunchSession = useCallback(
    async (params: {
      command: string;
      args: string[];
      working_dir: string;
      task_name: string;
      agent_type: AgentType;
      worktree_path?: string;
      base_commit?: string;
      prompt?: string;
      task_id?: number;
    }) => {
      try {
        await useSessionStore.getState().addSession({
          command: params.command,
          args: params.args,
          working_dir: params.working_dir,
          task_name: params.task_name,
          agent_type: params.agent_type,
          worktree_path: params.worktree_path,
          base_commit: params.base_commit,
          prompt: params.prompt,
          task_id: params.task_id,
        });
      } catch (err) {
        showToast(String(err), "error");
      }
    },
    []
  );

  const handleAddProject = useCallback((path: string) => {
    useProjectStore.getState().addProject(path);
  }, []);

  if (!loaded) return null;

  // Show wizard on first launch (null = still loading, render nothing)
  if (welcomeCompleted === false) {
    return (
      <>
        <WelcomeWizard onComplete={() => setWelcomeCompleted(true)} />
        <ToastContainer />
      </>
    );
  }
  if (welcomeCompleted === null) return null;

  return (
    <div className="app-layout">
      <div className={`main${isResizing ? " sidebar-resizing" : ""}`}>
        <ErrorBoundary>
          <ProjectRail />
        </ErrorBoundary>
        {!sidebarCollapsed && (
          <>
            <ErrorBoundary>
              <Sidebar />
            </ErrorBoundary>
            <div
              className="sidebar-resize-handle"
              onMouseDown={handleResizeStart}
              onDoubleClick={handleResizeDoubleClick}
            />
          </>
        )}
        <div className="content">
          <SummaryBar />
          {activeSessionId === null ? (
            <div className="welcome">
              <h1>Sessonix</h1>
              <p className="welcome-subtitle">Agent Mission Control</p>
              <p className="welcome-hint">
                <kbd>Cmd+Shift+K</kbd> add project
                &nbsp;&middot;&nbsp;
                <kbd>Cmd+Shift+T</kbd> new session
                &nbsp;&middot;&nbsp;
                <kbd>Cmd+0</kbd> diff
                &nbsp;&middot;&nbsp;
                <kbd>Ctrl+1–9</kbd> switch projects
              </p>
            </div>
          ) : null}
          {activeSessionId === DIFF_PSEUDO_ID ? (
            <ErrorBoundary>
              <DiffViewer />
            </ErrorBoundary>
          ) : (
            <ErrorBoundary>
              <TerminalPane
                activeSessionId={activeSessionId}
                sessionIds={sessionIds}
                isActiveSessionExited={isActiveSessionExited}
              />
            </ErrorBoundary>
          )}
        </div>
      </div>
      <StatusBar />
      <ToastContainer />

      {launcher.open && launcher.mode === "project" && (
        <SessionLauncher
          mode="project"
          isOpen={true}
          onClose={closeLauncher}
          onAddProject={handleAddProject}
        />
      )}
      {launcher.open && launcher.mode === "session" && (
        <SessionLauncher
          mode="session"
          isOpen={true}
          onClose={closeLauncher}
          projectPath={launcher.projectPath}
          prefill={launcher.prefill}
          taskId={launcher.taskId}
          onLaunch={handleLaunchSession}
        />
      )}
      {settingsOpen && <SettingsModal onCheckForUpdates={() => triggerUpdateCheck(true)} />}
      {updateInfo && <UpdateModal update={updateInfo} onClose={() => setUpdateInfo(null)} />}
    </div>
  );
}

export default App;
