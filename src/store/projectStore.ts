import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { Project } from "../lib/types";
import {
  addProject as apiAddProject,
  removeProject as apiRemoveProject,
  reorderProject as apiReorderProject,
  killSession,
} from "../lib/api";

function projectName(path: string): string {
  const segments = path.replace(/\/+$/, "").split("/");
  return segments[segments.length - 1] || path;
}

interface ProjectState {
  projects: Project[];
  activeProjectPath: string | null;
  /** Remembers the last active session per project so switching back restores it */
  lastActiveSession: Record<string, number>;

  // Bulk init (called by sessionStore.restore)
  setProjects: (projects: Project[]) => void;
  setActiveProjectPath: (path: string | null) => void;
  setLastActiveSession: (path: string, sessionId: number) => void;

  // CRUD
  addProject: (path: string) => Promise<void>;
  removeProject: (path: string) => Promise<number[]>; // returns removed session IDs for caller cleanup
  ensureProject: (path: string) => void;
  addSessionToProject: (path: string, sessionId: number) => void;
  removeSessionFromProject: (sessionId: number) => void;
  replaceSessionInProject: (path: string, oldId: number, newId: number) => void;

  // Reorder: optimistic local move, then persist via reorder_project IPC.
  reorderProjects: (fromPath: string, toPath: string) => void;
}

export const useProjectStore = create<ProjectState>()(
  persist(
    (set, get) => ({
  projects: [],
  activeProjectPath: null,
  lastActiveSession: {},

  setProjects: (projects) => set({ projects }),

  setActiveProjectPath: (path) => set({ activeProjectPath: path }),

  setLastActiveSession: (path, sessionId) =>
    set((state) => ({
      lastActiveSession: { ...state.lastActiveSession, [path]: sessionId },
    })),

  addProject: async (path) => {
    const name = projectName(path);
    await apiAddProject(name, path).catch(console.error);
    set((state) => {
      if (state.projects.some((p) => p.path === path)) {
        // Project already exists — just switch to it
        return { activeProjectPath: path };
      }
      return {
        projects: [...state.projects, { path, name, sessions: [] }],
        activeProjectPath: path,
      };
    });
  },

  removeProject: async (path) => {
    const { projects, activeProjectPath } = get();
    const project = projects.find((p) => p.path === path);
    if (!project) return [];

    for (const sid of project.sessions) {
      await killSession(sid).catch(console.error);
    }
    await apiRemoveProject(path).catch(console.error);

    const remaining = projects.filter((p) => p.path !== path);
    set({
      projects: remaining,
      activeProjectPath:
        activeProjectPath === path
          ? (remaining.length > 0 ? remaining[0].path : null)
          : activeProjectPath,
    });

    // Return removed session IDs so caller can clean up sessionStore
    return project.sessions;
  },

  ensureProject: (path) => {
    set((state) => {
      if (state.projects.some((p) => p.path === path)) return state;
      const name = projectName(path);
      return { projects: [...state.projects, { path, name, sessions: [] }] };
    });
  },

  addSessionToProject: (path, sessionId) => {
    set((state) => ({
      projects: state.projects.map((p) =>
        p.path === path ? { ...p, sessions: [...p.sessions, sessionId] } : p
      ),
    }));
  },

  removeSessionFromProject: (sessionId) => {
    set((state) => ({
      projects: state.projects.map((p) => ({
        ...p,
        sessions: p.sessions.filter((sid) => sid !== sessionId),
      })),
    }));
  },

  replaceSessionInProject: (path, oldId, newId) => {
    set((state) => ({
      projects: state.projects.map((p) =>
        p.path === path
          ? { ...p, sessions: p.sessions.map((sid) => (sid === oldId ? newId : sid)) }
          : p
      ),
    }));
  },

  reorderProjects: (fromPath, toPath) => {
    if (fromPath === toPath) return;
    let backendOrder = -1;
    set((state) => {
      const from = state.projects.findIndex((p) => p.path === fromPath);
      const to = state.projects.findIndex((p) => p.path === toPath);
      if (from === -1 || to === -1) return state;
      const next = state.projects.slice();
      const [moved] = next.splice(from, 1);
      next.splice(to, 0, moved);
      backendOrder = to + 1; // 1-based for SQL sort_order
      return { projects: next };
    });
    if (backendOrder > 0) {
      apiReorderProject(fromPath, backendOrder).catch((e) => {
        console.error("[reorderProject] backend failed:", e);
      });
    }
  },
    }),
    {
      name: "sessonix-projects",
      // Projects list comes from DB via sessionStore.restore — don't persist it.
      // Only persist user's navigation state.
      partialize: (state) => ({
        activeProjectPath: state.activeProjectPath,
        lastActiveSession: state.lastActiveSession,
      }),
    }
  )
);
