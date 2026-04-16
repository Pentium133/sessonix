import { useEffect } from "react";
import { getSessionStatus } from "../lib/api";
import { getGitStatus } from "../lib/git";
import { useSessionStore } from "../store/sessionStore";
import { useSettingsStore } from "../store/settingsStore";
import { sendSessionNotification, agentLabel } from "../lib/notifications";
import type { GitStatus, SessionStatus } from "../lib/types";

const POLL_INTERVAL_MS = 5_000;

/** Track previous status + status_line per session to detect transitions. */
const prevStatus = new Map<number, { status: SessionStatus; statusLine: string }>();

function gitStatusEqual(a: GitStatus | null | undefined, b: GitStatus | null | undefined): boolean {
  if (a == null && b == null) return true;
  if (a == null || b == null) return false;
  return (
    a.is_repo === b.is_repo &&
    a.branch === b.branch &&
    a.changed_files === b.changed_files &&
    a.modified === b.modified &&
    a.added === b.added &&
    a.deleted === b.deleted &&
    a.head_sha === b.head_sha &&
    a.is_worktree === b.is_worktree
  );
}

/**
 * Polls get_session_status for running sessions and batch-updates the store.
 * Uses a single set() call per tick to avoid N re-renders for N sessions.
 * Sends OS notifications on permission/idle transitions.
 */
export function useStatusPolling() {
  useEffect(() => {
    async function poll() {
      const { sessions, batchUpdateSessionStatus, loaded } = useSessionStore.getState();
      if (!loaded) return; // skip poll before restore() completes

      const alive = sessions.filter((s) => s.status !== "exited");
      if (alive.length === 0) return;

      // Clean up stale entries from prevStatus
      const aliveIds = new Set(alive.map((s) => s.id));
      for (const id of prevStatus.keys()) {
        if (!aliveIds.has(id)) prevStatus.delete(id);
      }

      // Deduplicate git status calls by effective dir (worktree_path or working_dir)
      const effectiveDir = (s: typeof alive[0]) => s.worktree_path ?? s.working_dir;
      const uniqueDirs = [...new Set(alive.map(effectiveDir))];
      const gitResults = await Promise.allSettled(
        uniqueDirs.map((dir) => getGitStatus(dir).catch(() => null))
      );
      const gitByDir = new Map<string, GitStatus | null>(
        uniqueDirs.map((dir, i) => {
          const r = gitResults[i];
          return [dir, r.status === "fulfilled" ? r.value : null];
        })
      );

      const results = await Promise.allSettled(
        alive.map(async (session) => {
          const result = await getSessionStatus(
            session.id,
            session.agent_type,
            session.working_dir,
            session.agentSessionId
          );
          return { session, result };
        })
      );

      const changed: Array<{ id: number; status: SessionStatus; statusLine: string; gitStatus?: GitStatus | null }> = [];
      for (const r of results) {
        if (r.status !== "fulfilled") continue;
        const { session, result } = r.value;
        const git = gitByDir.get(session.worktree_path ?? session.working_dir) ?? null;
        const statusChanged = result.state !== session.status || result.status_line !== session.status_line;
        const gitChanged = !gitStatusEqual(git, session.gitStatus);
        if (statusChanged || gitChanged) {
          changed.push({
            id: session.id,
            status: result.state,
            statusLine: result.status_line,
            ...(gitChanged ? { gitStatus: git } : {}),
          });
        }

        // Detect transitions for notifications
        const prev = prevStatus.get(session.id);
        const isPermission = result.status_line.toLowerCase().includes("permission");

        if (!prev) {
          // Seed with current state on first encounter to avoid spurious notifications at startup
          prevStatus.set(session.id, { status: result.state, statusLine: result.status_line });
        } else {
          const wasPermission = prev.statusLine.toLowerCase().includes("permission");
          const settings = useSettingsStore.getState();
          const label = agentLabel(session.agent_type);
          const name = session.task_name && session.task_name !== label ? session.task_name : "";

          // Permission transition: was not permission → now permission
          if (isPermission && !wasPermission && settings.notifyPermission) {
            sendSessionNotification(
              label,
              name ? `${name} — Waiting for permission` : "Waiting for permission",
              session.working_dir,
              session.id,
            );
          }

          // Idle transition: was running → now idle (and not permission)
          if (
            prev.status === "running" &&
            result.state === "idle" &&
            !isPermission &&
            settings.notifyIdle
          ) {
            const isShell = session.agent_type === "shell" || session.agent_type === "custom";
            const idleText = isShell ? "Command finished" : "Ready for input";
            sendSessionNotification(
              label,
              name ? `${name} — ${idleText}` : idleText,
              session.working_dir,
              session.id,
            );
          }

          prevStatus.set(session.id, { status: result.state, statusLine: result.status_line });
        }
      }

      batchUpdateSessionStatus(changed);
    }

    poll();
    const interval = setInterval(poll, POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, []);
}
