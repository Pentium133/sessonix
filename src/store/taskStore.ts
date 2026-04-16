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
  add: (task: Task) => void;
  remove: (taskId: number) => void;
  create: (
    projectPath: string,
    name: string,
    branchName: string
  ) => Promise<Task>;
  destroy: (taskId: number) => Promise<void>;

  tasksForProject: () => Task[];
}

export const useTaskStore = create<TaskState>((set, get) => ({
  tasks: [],
  loaded: false,

  load: async (projectPath) => {
    // Reset before fetch — prevents stale flash when switching projects.
    // Mirrors templateStore pattern.
    set({ tasks: [], loaded: false });
    try {
      const tasks = await listTasks(projectPath);
      set({ tasks, loaded: true });
    } catch (e) {
      console.error("[taskStore] load failed:", e);
      set({ tasks: [], loaded: true });
    }
  },

  add: (task) => set((s) => ({ tasks: [...s.tasks, task] })),

  remove: (taskId) =>
    set((s) => ({ tasks: s.tasks.filter((t) => t.id !== taskId) })),

  create: async (projectPath, name, branchName) => {
    const task = await apiCreate({
      project_path: projectPath,
      name,
      branch_name: branchName,
    });
    set((s) => ({ tasks: [...s.tasks, task] }));
    return task;
  },

  destroy: async (taskId) => {
    await apiDelete(taskId);
    set((s) => ({ tasks: s.tasks.filter((t) => t.id !== taskId) }));
  },

  tasksForProject: () => get().tasks,
}));
