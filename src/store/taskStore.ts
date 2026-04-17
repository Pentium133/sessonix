import { create } from "zustand";
import type { Task } from "../lib/types";
import {
  createTask as apiCreate,
  listTasks,
  deleteTask as apiDelete,
} from "../lib/api";

interface TaskState {
  tasks: Task[];
  loaded: boolean;

  load: (projectPath: string) => Promise<void>;
  create: (
    projectPath: string,
    name: string,
    branchName: string,
    sourceBranch?: string
  ) => Promise<Task>;
  /// Returns a non-null string when the task was removed from DB but the
  /// worktree cleanup failed (caller should surface as a warning toast).
  destroy: (taskId: number) => Promise<string | null>;
}

export const useTaskStore = create<TaskState>((set) => ({
  tasks: [],
  loaded: false,

  load: async (projectPath) => {
    // Reset before fetch — prevents stale flash when switching projects.
    // Mirrors quickPromptStore pattern.
    set({ tasks: [], loaded: false });
    try {
      const tasks = await listTasks(projectPath);
      set({ tasks, loaded: true });
    } catch (e) {
      console.error("[taskStore] load failed:", e);
      set({ tasks: [], loaded: true });
    }
  },

  create: async (projectPath, name, branchName, sourceBranch) => {
    // Omit `source_branch` entirely when not set — keeps the payload identical
    // to the pre-dropdown shape so tests and the backend both see the legacy
    // 3-field request for new-branch creation.
    const task = await apiCreate(
      sourceBranch
        ? {
            project_path: projectPath,
            name,
            branch_name: branchName,
            source_branch: sourceBranch,
          }
        : {
            project_path: projectPath,
            name,
            branch_name: branchName,
          }
    );
    set((s) => ({ tasks: [...s.tasks, task] }));
    return task;
  },

  destroy: async (taskId) => {
    const result = await apiDelete(taskId);
    set((s) => ({ tasks: s.tasks.filter((t) => t.id !== taskId) }));
    return result.worktree_warning;
  },
}));
