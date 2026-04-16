import { create } from "zustand";
import {
  createTemplate,
  listTemplates,
  deleteTemplate,
  updateTemplate,
} from "../lib/api";
import type { TemplateInfo } from "../lib/api";

interface TemplateState {
  templates: TemplateInfo[];
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

export const useTemplateStore = create<TemplateState>((set) => ({
  templates: [],
  loaded: false,

  load: async (projectPath) => {
    set({ templates: [], loaded: false });
    try {
      const templates = await listTemplates(projectPath);
      set({ templates, loaded: true });
    } catch {
      set({ templates: [], loaded: true });
    }
  },

  add: async (params) => {
    const id = await createTemplate(params);
    const entry: TemplateInfo = {
      id,
      name: params.name,
      project_path: params.project_path,
      agent: params.agent,
      initial_prompt: params.initial_prompt ?? null,
      skip_permissions: params.skip_permissions,
    };
    set((state) => ({ templates: [...state.templates, entry] }));
  },

  update: async (id, name, initialPrompt) => {
    await updateTemplate(id, name, initialPrompt);
    set((state) => ({
      templates: state.templates.map((t) =>
        t.id === id ? { ...t, name, initial_prompt: initialPrompt ?? null } : t
      ),
    }));
  },

  remove: async (id) => {
    await deleteTemplate(id);
    set((state) => ({ templates: state.templates.filter((t) => t.id !== id) }));
  },
}));
