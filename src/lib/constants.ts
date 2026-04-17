import type { ThemeId } from "./themes";

export type Theme = "system" | ThemeId;

export const AGENT_COLORS: Record<string, string> = {
  claude: "var(--claude)",
  codex: "var(--codex)",
  cursor: "var(--cursor)",
  gemini: "var(--gemini)",
  opencode: "var(--opencode)",
  shell: "var(--text-dim)",
  custom: "var(--text-dim)",
};

export const SIDEBAR_MIN = 180;
export const SIDEBAR_MAX = 500;
export const SIDEBAR_DEFAULT = 260;
