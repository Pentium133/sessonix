import { useEffect } from "react";
import { useSessionStore } from "../store/sessionStore";
import { useProjectStore } from "../store/projectStore";
import { useUiStore } from "../store/uiStore";

/**
 * Switch the active project and restore its last-used session (or the first one,
 * if none was remembered). Remembers the *current* active session before
 * leaving, so bouncing back-and-forth preserves per-project focus.
 */
function switchToProject(targetPath: string) {
  const pStore = useProjectStore.getState();
  const sStore = useSessionStore.getState();

  if (targetPath === pStore.activeProjectPath) return;

  if (pStore.activeProjectPath && sStore.activeSessionId != null) {
    pStore.setLastActiveSession(pStore.activeProjectPath, sStore.activeSessionId);
  }
  pStore.setActiveProjectPath(targetPath);

  const projectSessions = sStore.sessions
    .filter((s) => s.working_dir === targetPath)
    .sort((a, b) => a.sortOrder - b.sortOrder);
  if (projectSessions.length > 0) {
    const lastId = pStore.lastActiveSession[targetPath];
    const target = projectSessions.find((s) => s.id === lastId) ?? projectSessions[0];
    sStore.switchSession(target.id);
  }
}

export function useGlobalShortcuts() {
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      const sStore = useSessionStore.getState();
      const pStore = useProjectStore.getState();
      const ui = useUiStore.getState();

      // Cmd/Ctrl + , — toggle Settings
      if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        e.preventDefault();
        if (ui.settingsOpen) ui.closeSettings(); else ui.openSettings();
        return;
      }
      // Cmd+Shift+K — Add project
      if (e.metaKey && e.shiftKey && e.key === "K") {
        e.preventDefault();
        ui.openLauncher({ open: true, mode: "project" });
        return;
      }
      // Cmd+Shift+T — New session in active (or first) project
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
      // Cmd+Shift+W — Kill active session
      if (e.metaKey && e.shiftKey && e.key === "W") {
        e.preventDefault();
        if (sStore.activeSessionId !== null) {
          sStore.removeSession(sStore.activeSessionId);
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
      // Ctrl+1-9 — switch projects by index
      if (e.ctrlKey && !e.metaKey && !e.shiftKey && e.key >= "1" && e.key <= "9") {
        const idx = parseInt(e.key) - 1;
        if (idx < pStore.projects.length) {
          e.preventDefault();
          switchToProject(pStore.projects[idx].path);
        }
        return;
      }
      // Cmd+1-9 — switch sessions within active project by index
      if (e.metaKey && e.key >= "1" && e.key <= "9") {
        const activePath = pStore.activeProjectPath;
        const projectSessions = sStore.sessions
          .filter((s) => s.working_dir === activePath)
          .sort((a, b) => a.sortOrder - b.sortOrder);
        const idx = parseInt(e.key) - 1;
        if (idx < projectSessions.length) {
          e.preventDefault();
          sStore.switchSession(projectSessions[idx].id);
        }
        return;
      }
      // Cmd+Left/Right — prev/next session within active project
      if (e.metaKey && !e.shiftKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        const activePath = pStore.activeProjectPath;
        if (!activePath) return;
        const projectSessions = sStore.sessions
          .filter((s) => s.working_dir === activePath)
          .sort((a, b) => a.sortOrder - b.sortOrder);
        if (projectSessions.length < 2) return;
        const currentIdx = projectSessions.findIndex((s) => s.id === sStore.activeSessionId);
        const nextIdx = e.key === "ArrowRight"
          ? (currentIdx + 1) % projectSessions.length
          : (currentIdx - 1 + projectSessions.length) % projectSessions.length;
        e.preventDefault();
        sStore.switchSession(projectSessions[nextIdx].id);
        return;
      }
      // Cmd+Up/Down — prev/next project
      if (e.metaKey && !e.shiftKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
        if (pStore.projects.length < 2) return;
        const currentIdx = pStore.projects.findIndex((p) => p.path === pStore.activeProjectPath);
        const nextIdx = e.key === "ArrowDown"
          ? (currentIdx + 1) % pStore.projects.length
          : (currentIdx - 1 + pStore.projects.length) % pStore.projects.length;
        e.preventDefault();
        switchToProject(pStore.projects[nextIdx].path);
        return;
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);
}
