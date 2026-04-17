import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { writeToTerminal } from "../lib/terminalPool";
import { notifySessionExit } from "../lib/api";
import { getGitStatus, removeWorktree, clearWorktreePath } from "../lib/git";
import { useSessionStore } from "../store/sessionStore";
import { useSettingsStore } from "../store/settingsStore";
import { sendSessionNotification, agentLabel } from "../lib/notifications";
import { showToast } from "../components/Toast";
import type { PtyOutputPayload, PtyExitPayload } from "../lib/types";

export function usePtyOutput() {
  useEffect(() => {
    const unlistenOutput = listen<PtyOutputPayload>("pty-output", (event) => {
      const { id, data } = event.payload;
      const bytes = new Uint8Array(data);
      writeToTerminal(id, bytes);
    });

    const unlistenExit = listen<PtyExitPayload>("pty-exit", (event) => {
      const id = event.payload.id;
      const session = useSessionStore.getState().sessions.find((s) => s.id === id);
      notifySessionExit(id).catch(console.error);
      useSessionStore.getState().handleExit(id);

      // Send OS notification for session exit (guard against duplicate pty-exit events)
      if (session && session.status !== "exited" && useSettingsStore.getState().notifyExit) {
        const label = agentLabel(session.agent_type);
        const name = session.task_name && session.task_name !== label ? session.task_name : "";
        sendSessionNotification(
          label,
          name ? `${name} — Session ended` : "Session ended",
          session.working_dir,
          session.id,
        );
      }

      // Worktree cleanup
      if (session && session.worktree_path) {
        const wtPath = session.worktree_path;
        const sessionId = session.id;
        const clearWt = () => {
          clearWorktreePath(sessionId).catch(console.error);
          useSessionStore.getState().clearSessionWorktree(sessionId);
        };
        getGitStatus(wtPath).then((status) => {
          if (!status.is_repo || status.changed_files === 0) {
            // No changes — auto-cleanup
            removeWorktree(wtPath).then(() => {
              clearWt();
              showToast("Worktree cleaned up", "info");
            }).catch(console.error);
          } else {
            // Has changes — show toast with actions
            showToast(
              `Worktree has ${status.changed_files} changed file${status.changed_files !== 1 ? "s" : ""}`,
              "info",
              {
                label: "Remove",
                onClick: () => {
                  removeWorktree(wtPath).then(() => {
                    clearWt();
                    showToast("Worktree removed", "info");
                  }).catch((err) => showToast(`Failed: ${err}`, "error"));
                },
              },
            );
          }
        }).catch(() => {
          // Can't check git status (directory already gone) — clear DB reference
          clearWt();
        });
      }
    });

    return () => {
      unlistenOutput.then((fn) => fn());
      unlistenExit.then((fn) => fn());
    };
  }, []); // truly stable — no deps, reads store directly
}
