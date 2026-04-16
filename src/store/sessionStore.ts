import { create } from "zustand";
import {
  createSession,
  killSession,
  deleteSession,
  attachSession,
  detachSession,
  listProjects,
  listSessions,
  installClaudeHooks,
  checkClaudeHooks,
  reorderSession,
  setSortOrder,
} from "../lib/api";
import { writeToTerminal } from "../components/TerminalPane";
import { useProjectStore } from "./projectStore";
import type { AgentType, GitStatus, Session, SessionStatus } from "../lib/types";

interface SessionState {
  sessions: Session[];
  activeSessionId: number | null;
  loaded: boolean;

  // Init
  restore: () => Promise<void>;

  // CRUD
  addSession: (params: {
    command: string;
    args?: string[];
    working_dir: string;
    task_name?: string;
    agent_type?: AgentType;
    replaceId?: number;
    worktree_path?: string;
    base_commit?: string;
    prompt?: string;
    task_id?: number;
  }) => Promise<number>;
  removeSession: (id: number) => Promise<void>;
  removeSessions: (ids: number[]) => void; // bulk remove — for project removal cleanup
  switchSession: (targetId: number) => Promise<void>;
  updateSessionStatus: (id: number, status: SessionStatus, statusLine?: string) => void;
  batchUpdateSessionStatus: (updates: Array<{ id: number; status: SessionStatus; statusLine: string; gitStatus?: GitStatus | null }>) => void;
  handleExit: (sessionId: number) => void;
  clearSessionWorktree: (id: number) => void;
  reorderSessionOrder: (ptyId: number, newSortOrder: number) => Promise<void>;

  // Derived
  sessionsForProject: (projectPath: string) => Session[];
}

export const useSessionStore = create<SessionState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  loaded: false,

  restore: async () => {
    const projectStore = useProjectStore.getState();

    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        const dbProjects = await listProjects();

        const sessionResults = await Promise.all(
          dbProjects.map((p) =>
            listSessions(p.path).then(
              (sessions) => ({ path: p, sessions, ok: true as const }),
              () => ({ path: p, sessions: [] as never[], ok: false as const })
            )
          )
        );

        const allSessions: Session[] = [];
        const projectList = [];

        for (const result of sessionResults) {
          const p = result.path;
          const dbSessions = result.ok ? result.sessions : [];
          const sessionIds: number[] = [];

          for (const s of dbSessions) {
            if (!s) continue;
            const sessionId = s.pty_id ?? s.id;
            if (!sessionId || sessionId <= 0) continue;
            sessionIds.push(sessionId);
            let parsedArgs: string[] = [];
            try {
              const raw: unknown = JSON.parse(s.launch_args || "[]");
              if (Array.isArray(raw) && raw.every((a): a is string => typeof a === "string")) {
                parsedArgs = raw;
              }
            } catch { /* ignore */ }
            allSessions.push({
              id: sessionId,
              command: s.launch_command,
              args: parsedArgs,
              working_dir: s.working_dir,
              task_name: s.task_name,
              agent_type: (s.agent_type as AgentType) ?? "custom",
              status: s.status as SessionStatus,
              status_line: s.status_line,
              created_at: new Date(s.started_at).getTime(),
              dbId: s.id,
              agentSessionId: s.agent_session_id ?? undefined,
              sortOrder: s.sort_order ?? 0,
              gitStatus: null,
              worktree_path: s.worktree_path ?? null,
              base_commit: s.base_commit ?? null,
              initial_prompt: s.initial_prompt ?? null,
              task_id: s.task_id ?? null,
            });
          }

          projectList.push({
            path: p.path,
            name: p.name,
            sessions: sessionIds,
          });
        }

        // Atomic: populate both stores. Honor a persisted activeProjectPath
        // (from localStorage via projectStore.persist) when it still matches
        // an existing project — otherwise fall back to the first.
        projectStore.setProjects(projectList);
        if (dbProjects.length > 0) {
          const persistedPath = projectStore.activeProjectPath;
          const stillExists = persistedPath && dbProjects.some((p) => p.path === persistedPath);
          if (!stillExists) {
            projectStore.setActiveProjectPath(dbProjects[0].path);
          }
        }
        set({ sessions: allSessions, loaded: true });
        return;
      } catch (e) {
        console.warn(`[restore] attempt ${attempt + 1} failed:`, e);
        if (attempt < 2) {
          await new Promise((r) => setTimeout(r, 300 * (attempt + 1)));
        }
      }
    }
    console.error("[restore] all attempts failed");
    set({ loaded: true });
  },

  addSession: async (params) => {
    const { activeSessionId } = get();
    const projectStore = useProjectStore.getState();

    // Detach current session (prevents duplicate output on switch back)
    if (activeSessionId !== null && params.replaceId == null) {
      await detachSession(activeSessionId).catch(console.error);
    }

    // Ensure project exists
    projectStore.ensureProject(params.working_dir);

    // Auto-install Claude hooks
    if (params.agent_type === "claude") {
      try {
        const installed = await checkClaudeHooks();
        if (!installed) await installClaudeHooks();
      } catch { /* non-fatal */ }
    }

    // When replacing (relaunch): kill old PTY + delete from DB before creating new
    if (params.replaceId != null) {
      await killSession(params.replaceId).catch(console.error);
      await deleteSession(params.replaceId).catch(console.error);
    }

    const id = await createSession({
      command: params.command,
      args: params.args ?? [],
      working_dir: params.working_dir,
      task_name: params.task_name,
      agent_type: params.agent_type,
      worktree_path: params.worktree_path,
      base_commit: params.base_commit,
      prompt: params.prompt,
      task_id: params.task_id,
    });

    if (params.replaceId != null) {
      const oldSession = get().sessions.find((s) => s.id === params.replaceId);
      const replacedOrder = oldSession?.sortOrder ?? 0;
      // Preserve task_id from replaced session if caller didn't supply one
      const effectiveTaskId = params.task_id ?? oldSession?.task_id ?? null;
      set((state) => {
        const session: Session = {
          id,
          command: params.command,
          args: params.args ?? [],
          working_dir: params.working_dir,
          task_name: params.task_name ?? params.command,
          agent_type: params.agent_type ?? "custom",
          status: "running",
          status_line: "",
          created_at: Date.now(),
          sortOrder: replacedOrder,
          gitStatus: null,
          worktree_path: params.worktree_path ?? null,
          base_commit: params.base_commit ?? null,
          initial_prompt: params.prompt ?? null,
          task_id: effectiveTaskId,
        };
        return {
          sessions: state.sessions.map((s) => (s.id === params.replaceId ? session : s)),
          activeSessionId: id,
        };
      });
      // Persist the original sort_order in DB so it survives app restart.
      // Use setSortOrder (direct UPDATE) instead of reorderSession (which shifts neighbors).
      if (replacedOrder > 0) {
        setSortOrder(id, replacedOrder).catch(console.error);
      }
      projectStore.replaceSessionInProject(params.working_dir, params.replaceId!, id);
    } else {
      set((state) => {
        const maxOrder = state.sessions
          .filter((s) => s.working_dir === params.working_dir)
          .reduce((max, s) => Math.max(max, s.sortOrder), 0);
        const session: Session = {
          id,
          command: params.command,
          args: params.args ?? [],
          working_dir: params.working_dir,
          task_name: params.task_name ?? params.command,
          agent_type: params.agent_type ?? "custom",
          status: "running",
          status_line: "",
          created_at: Date.now(),
          sortOrder: maxOrder + 1,
          gitStatus: null,
          worktree_path: params.worktree_path ?? null,
          base_commit: params.base_commit ?? null,
          initial_prompt: params.prompt ?? null,
          task_id: params.task_id ?? null,
        };
        return {
          sessions: [...state.sessions, session],
          activeSessionId: id,
        };
      });
      projectStore.addSessionToProject(params.working_dir, id);
    }

    projectStore.setActiveProjectPath(params.working_dir);
    return id;
  },

  removeSession: async (id) => {
    const { sessions } = get();
    const session = sessions.find((s) => s.id === id);

    if (session?.status === "exited") {
      await deleteSession(id).catch(console.error);
    } else {
      await killSession(id).catch(console.error);
      await deleteSession(id).catch(console.error);
    }

    // Compute sibling selection inside set() to use the latest state snapshot
    // (avoid stale reads from before the async kill/delete calls above)
    set((state) => {
      const removedSession = state.sessions.find((s) => s.id === id);
      let nextActiveId = state.activeSessionId;
      if (state.activeSessionId === id) {
        const siblings = state.sessions
          .filter((s) => s.working_dir === removedSession?.working_dir && s.id !== id)
          .sort((a, b) => a.sortOrder - b.sortOrder);
        nextActiveId = siblings.length > 0 ? siblings[0].id : null;
      }
      return {
        sessions: state.sessions.filter((s) => s.id !== id),
        activeSessionId: nextActiveId,
      };
    });
    useProjectStore.getState().removeSessionFromProject(id);
  },

  removeSessions: (ids) => {
    const idSet = new Set(ids);
    set((state) => ({
      sessions: state.sessions.filter((s) => !idSet.has(s.id)),
      activeSessionId: idSet.has(state.activeSessionId ?? -1) ? null : state.activeSessionId,
    }));
  },

  switchSession: async (targetId) => {
    const { activeSessionId, sessions } = get();
    if (targetId === activeSessionId) return;

    const targetSession = sessions.find((s) => s.id === targetId);
    const isTargetAlive = targetSession && targetSession.status !== "exited";

    // Detach current
    if (activeSessionId !== null) {
      const currentSession = sessions.find((s) => s.id === activeSessionId);
      if (currentSession && currentSession.status !== "exited") {
        await detachSession(activeSessionId).catch(console.error);
      }
    }

    // Attach target
    if (isTargetAlive) {
      const buffered = await attachSession(targetId);
      if (buffered.length > 0) {
        writeToTerminal(targetId, new Uint8Array(buffered));
      }
    }

    // Update active state
    set({ activeSessionId: targetId });
    if (targetSession) {
      const projectStore = useProjectStore.getState();
      projectStore.setActiveProjectPath(targetSession.working_dir);
      projectStore.setLastActiveSession(targetSession.working_dir, targetId);
    }
  },

  updateSessionStatus: (id, status, statusLine) => {
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.id === id
          ? { ...s, status, ...(statusLine !== undefined ? { status_line: statusLine } : {}) }
          : s
      ),
    }));
  },

  batchUpdateSessionStatus: (updates) => {
    if (updates.length === 0) return;
    const updateMap = new Map(updates.map((u) => [u.id, u]));
    set((state) => ({
      sessions: state.sessions.map((s) => {
        const u = updateMap.get(s.id);
        if (!u) return s;
        const next: Session = { ...s, status: u.status, status_line: u.statusLine };
        if (u.gitStatus !== undefined) next.gitStatus = u.gitStatus;
        return next;
      }),
    }));
  },

  handleExit: (sessionId) => {
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.id === sessionId ? { ...s, status: "exited" as const, status_line: "" } : s
      ),
    }));
  },

  clearSessionWorktree: (id) => {
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.id === id ? { ...s, worktree_path: null, base_commit: null } : s
      ),
    }));
  },

  reorderSessionOrder: async (ptyId, newSortOrder) => {
    set((state) => {
      const target = state.sessions.find((s) => s.id === ptyId);
      if (!target) return state;
      const oldOrder = target.sortOrder;
      if (oldOrder === newSortOrder) return state;
      return {
        sessions: state.sessions.map((s) => {
          if (s.working_dir !== target.working_dir) return s;
          if (s.id === ptyId) return { ...s, sortOrder: newSortOrder };
          if (oldOrder > newSortOrder && s.sortOrder >= newSortOrder && s.sortOrder < oldOrder) {
            return { ...s, sortOrder: s.sortOrder + 1 };
          }
          if (oldOrder < newSortOrder && s.sortOrder > oldOrder && s.sortOrder <= newSortOrder) {
            return { ...s, sortOrder: s.sortOrder - 1 };
          }
          return s;
        }),
      };
    });
    await reorderSession(ptyId, newSortOrder).catch(console.error);
  },

  sessionsForProject: (projectPath) => {
    return get()
      .sessions.filter((s) => s.working_dir === projectPath)
      .sort((a, b) => a.sortOrder - b.sortOrder);
  },
}));
