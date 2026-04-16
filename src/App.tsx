import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import ProjectRail from "./components/ProjectRail";
import Sidebar from "./components/Sidebar";
import SummaryBar from "./components/SummaryBar";
import TerminalPane from "./components/TerminalPane";
import StatusBar from "./components/StatusBar";
import SessionLauncher from "./components/SessionLauncher";
import SettingsModal from "./components/SettingsModal";
import WelcomeWizard from "./components/WelcomeWizard";
import ToastContainer, { showToast } from "./components/Toast";
import ErrorBoundary from "./components/ErrorBoundary";
import { usePtyOutput } from "./hooks/usePtyOutput";
import { useStatusPolling } from "./hooks/useStatusPolling";
import type { AgentType } from "./lib/types";
import { useSessionStore } from "./store/sessionStore";
import { useProjectStore } from "./store/projectStore";
import { useTaskStore } from "./store/taskStore";
import { useUiStore } from "./store/uiStore";
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
    initNotifications();
    setupNotificationClickHandler();
    // Check for updates silently on startup
    triggerUpdateCheck(false);
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
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      const store = useSessionStore.getState();
      const pStore = useProjectStore.getState();
      const ui = useUiStore.getState();

      if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        e.preventDefault();
        const { settingsOpen, openSettings, closeSettings } = useUiStore.getState();
        if (settingsOpen) closeSettings(); else openSettings();
        return;
      }
      if (e.metaKey && e.shiftKey && e.key === "K") {
        e.preventDefault();
        ui.openLauncher({ open: true, mode: "project" });
        return;
      }
      if (e.metaKey && e.shiftKey && e.key === "T") {
        e.preventDefault();
        if (pStore.activeProjectPath) {
          ui.openLauncher({ open: true, mode: "session", projectPath: pStore.activeProjectPath });
        } else if (pStore.projects.length > 0) {
          ui.openLauncher({ open: true, mode: "session", projectPath: pStore.projects[0].path });
        } else {
          ui.openLauncher({ open: true, mode: "project" });
        }
        return;
      }
      if (e.metaKey && e.shiftKey && e.key === "W") {
        e.preventDefault();
        if (store.activeSessionId !== null) {
          store.removeSession(store.activeSessionId);
        }
        return;
      }
      // Zoom: Cmd+= / Cmd+- / Cmd+0
      if ((e.metaKey || e.ctrlKey) && (e.key === "=" || e.key === "+")) {
        e.preventDefault();
        ui.zoomIn();
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key === "-") {
        e.preventDefault();
        ui.zoomOut();
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key === "0") {
        e.preventDefault();
        ui.resetZoom();
        return;
      }
      // Ctrl+1-9: switch projects
      if (e.ctrlKey && !e.metaKey && !e.shiftKey && e.key >= "1" && e.key <= "9") {
        const idx = parseInt(e.key) - 1;
        if (idx < pStore.projects.length) {
          e.preventDefault();
          const targetPath = pStore.projects[idx].path;
          if (targetPath !== pStore.activeProjectPath) {
            // Remember current session before leaving
            if (pStore.activeProjectPath && store.activeSessionId != null) {
              pStore.setLastActiveSession(pStore.activeProjectPath, store.activeSessionId);
            }
            pStore.setActiveProjectPath(targetPath);
            // Restore last active session in target project
            const projectSessions = store.sessions
              .filter((s) => s.working_dir === targetPath)
              .sort((a, b) => a.sortOrder - b.sortOrder);
            if (projectSessions.length > 0) {
              const lastId = pStore.lastActiveSession[targetPath];
              const target = projectSessions.find((s) => s.id === lastId) ?? projectSessions[0];
              store.switchSession(target.id);
            }
          }
        }
        return;
      }
      // Cmd+1-9: switch sessions within active project
      if (e.metaKey && e.key >= "1" && e.key <= "9") {
        const activePath = pStore.activeProjectPath;
        const projectSessions = store.sessions
          .filter((s) => s.working_dir === activePath)
          .sort((a, b) => a.sortOrder - b.sortOrder);
        const idx = parseInt(e.key) - 1;
        if (idx < projectSessions.length) {
          e.preventDefault();
          store.switchSession(projectSessions[idx].id);
        }
        return;
      }
      // Cmd+Left/Right: prev/next session within active project
      if (e.metaKey && !e.shiftKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        const activePath = pStore.activeProjectPath;
        if (!activePath) return;
        const projectSessions = store.sessions
          .filter((s) => s.working_dir === activePath)
          .sort((a, b) => a.sortOrder - b.sortOrder);
        if (projectSessions.length < 2) return;
        const currentIdx = projectSessions.findIndex((s) => s.id === store.activeSessionId);
        const nextIdx = e.key === "ArrowRight"
          ? (currentIdx + 1) % projectSessions.length
          : (currentIdx - 1 + projectSessions.length) % projectSessions.length;
        e.preventDefault();
        store.switchSession(projectSessions[nextIdx].id);
        return;
      }
      // Cmd+Up/Down: prev/next project
      if (e.metaKey && !e.shiftKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
        if (pStore.projects.length < 2) return;
        const currentIdx = pStore.projects.findIndex((p) => p.path === pStore.activeProjectPath);
        const nextIdx = e.key === "ArrowDown"
          ? (currentIdx + 1) % pStore.projects.length
          : (currentIdx - 1 + pStore.projects.length) % pStore.projects.length;
        const targetPath = pStore.projects[nextIdx].path;
        e.preventDefault();
        // Remember current session before leaving
        if (pStore.activeProjectPath && store.activeSessionId != null) {
          pStore.setLastActiveSession(pStore.activeProjectPath, store.activeSessionId);
        }
        pStore.setActiveProjectPath(targetPath);
        // Restore last active session in target project
        const projectSessions = store.sessions
          .filter((s) => s.working_dir === targetPath)
          .sort((a, b) => a.sortOrder - b.sortOrder);
        if (projectSessions.length > 0) {
          const lastId = pStore.lastActiveSession[targetPath];
          const target = projectSessions.find((s) => s.id === lastId) ?? projectSessions[0];
          store.switchSession(target.id);
        }
        return;
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

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
                <kbd>Ctrl+1–9</kbd> switch projects
              </p>
            </div>
          ) : null}
          <ErrorBoundary>
            <TerminalPane
              activeSessionId={activeSessionId}
              sessionIds={sessionIds}
              isActiveSessionExited={isActiveSessionExited}
            />
          </ErrorBoundary>
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
