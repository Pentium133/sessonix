import { create } from "zustand";
import {
  createQuickPrompt,
  listQuickPrompts,
  deleteQuickPrompt,
  updateQuickPrompt,
} from "../lib/api";
import type { QuickPromptInfo } from "../lib/api";

interface QuickPromptState {
  quickPrompts: QuickPromptInfo[];
  loaded: boolean;

  load: (projectPath: string) => Promise<void>;
  add: (params: {
    name: string;
    project_path: string;
    agent: string;
    initial_prompt?: string;
    skip_permissions: boolean;
  }) => Promise<void>;
  update: (id: number, name: string, initialPrompt?: string) => Promise<void>;
  remove: (id: number) => Promise<void>;
}

export const useQuickPromptStore = create<QuickPromptState>((set) => ({
  quickPrompts: [],
  loaded: false,

  load: async (projectPath) => {
    set({ quickPrompts: [], loaded: false });
    try {
      const quickPrompts = await listQuickPrompts(projectPath);
      set({ quickPrompts, loaded: true });
    } catch {
      set({ quickPrompts: [], loaded: true });
    }
  },

  add: async (params) => {
    const id = await createQuickPrompt(params);
    const entry: QuickPromptInfo = {
      id,
      name: params.name,
      project_path: params.project_path,
      agent: params.agent,
      initial_prompt: params.initial_prompt ?? null,
      skip_permissions: params.skip_permissions,
    };
    set((state) => ({ quickPrompts: [...state.quickPrompts, entry] }));
  },

  update: async (id, name, initialPrompt) => {
    await updateQuickPrompt(id, name, initialPrompt);
    set((state) => ({
      quickPrompts: state.quickPrompts.map((t) =>
        t.id === id ? { ...t, name, initial_prompt: initialPrompt ?? null } : t
      ),
    }));
  },

  remove: async (id) => {
    await deleteQuickPrompt(id);
    set((state) => ({ quickPrompts: state.quickPrompts.filter((t) => t.id !== id) }));
  },
}));
