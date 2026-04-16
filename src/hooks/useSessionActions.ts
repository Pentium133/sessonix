import { useCallback } from "react";
import { useSessionStore } from "../store/sessionStore";
import { useSettingsStore } from "../store/settingsStore";
import { showToast } from "../components/Toast";
import { getGitStatus } from "../lib/git";
import type { Session } from "../lib/types";

/** Build resume/relaunch args for a session based on agent type. */
function buildResumeArgs(session: Session): string[] {
  const prompt = session.initial_prompt?.trim() || "";

  if (session.agent_type === "claude" && session.agentSessionId) {
    const skipPerms = session.args.includes("--dangerously-skip-permissions")
      || useSettingsStore.getState().claudeSkipPermissions;
    const flags = skipPerms ? ["--dangerously-skip-permissions"] : [];
    // Claude resume: no -p flag (it's incompatible with --resume)
    return [...flags, "--resume", session.agentSessionId];
  }
  if (session.agent_type === "claude") {
    const skipPerms = session.args.includes("--dangerously-skip-permissions")
      || useSettingsStore.getState().claudeSkipPermissions;
    const flags = skipPerms ? ["--dangerously-skip-permissions"] : [];
    return [...flags, "--continue"];
  }
  if (session.agent_type === "codex" && session.agentSessionId) {
    return ["resume", session.agentSessionId];
  }
  if (session.agent_type === "codex") {
    return ["resume", "--last"];
  }
  if (session.agent_type === "opencode" && session.agentSessionId) {
    return ["run", "--quiet", "--session", session.agentSessionId];
  }
  if (session.agent_type === "opencode") {
    return ["run", "--quiet", "--continue"];
  }
  // Gemini: prompt as positional arg (same as initial launch)
  if (session.agent_type === "gemini" && prompt) {
    return [prompt];
  }
  return session.args;
}

/**
 * Higher-level session actions that involve UI side effects (toasts, confirmation).
 * Keeps stores pure data+API; UI concerns live here.
 */
export function useSessionActions() {
  const handleRemoveSession = useCallback((id: number) => {
    const { sessions, removeSession, addSession } = useSessionStore.getState();
    const session = sessions.find((s) => s.id === id);
    if (!session) {
      removeSession(id);
      return;
    }

    // Running sessions: kill immediately
    if (session.status !== "exited") {
      removeSession(id);
      return;
    }

    // Exited sessions: remove + show relaunch toast
    removeSession(id);
    showToast(
      `"${session.task_name}" removed`,
      "info",
      {
        label: "Relaunch",
        onClick: () => {
          addSession({
            command: session.command,
            args: buildResumeArgs(session),
            working_dir: session.working_dir,
            task_name: session.task_name,
            agent_type: session.agent_type,
            prompt: session.initial_prompt ?? undefined,
          }).catch(() => {});
        },
      }
    );
  }, []);

  const handleRelaunchSession = useCallback(async (session: Session) => {
    const { addSession } = useSessionStore.getState();
    try {
      // Check if worktree still exists for worktree sessions.
      // working_dir must always be the original project path (for project grouping).
      // worktree_path is where the PTY actually runs.
      let worktreePath: string | undefined;
      let baseCommit: string | undefined;

      if (session.worktree_path) {
        const status = await getGitStatus(session.worktree_path).catch(() => null);
        if (status?.is_repo) {
          worktreePath = session.worktree_path;
          baseCommit = session.base_commit ?? undefined;
        }
        // If worktree was deleted, launch in original project dir (no worktree)
      }

      await addSession({
        command: session.command,
        args: buildResumeArgs(session),
        working_dir: session.working_dir,
        task_name: session.task_name,
        agent_type: session.agent_type,
        replaceId: session.id,
        worktree_path: worktreePath,
        base_commit: baseCommit,
        prompt: session.initial_prompt ?? undefined,
        task_id: session.task_id ?? undefined,
      });
    } catch (err) {
      showToast(String(err), "error");
    }
  }, []);

  const handleForkSession = useCallback(async (session: Session) => {
    const { addSession } = useSessionStore.getState();
    try {
      // For Codex, fork requires a known thread ID — without it we can't fork.
      if (session.agent_type === "codex" && !session.agentSessionId) {
        showToast("Cannot fork: thread ID not yet captured for this session", "error");
        return;
      }

      // OpenCode CLI has no fork subcommand. Two processes resuming the same
      // session ID would race on the SQLite state, so block the action.
      if (session.agent_type === "opencode") {
        showToast("OpenCode does not support forking sessions", "error");
        return;
      }

      // For Codex with thread ID, use "fork" subcommand instead of "resume"
      let args: string[];
      if (session.agent_type === "codex" && session.agentSessionId) {
        args = ["fork", session.agentSessionId];
      } else {
        args = buildResumeArgs(session);
      }

      await addSession({
        command: session.command,
        args,
        working_dir: session.working_dir,
        task_name: `${session.task_name} (fork)`,
        agent_type: session.agent_type,
        worktree_path: session.worktree_path ?? undefined,
        base_commit: session.base_commit ?? undefined,
        task_id: session.task_id ?? undefined,
      });
      showToast("Session forked", "success");
    } catch (err) {
      showToast(String(err), "error");
    }
  }, []);

  return { handleRemoveSession, handleRelaunchSession, handleForkSession };
}
