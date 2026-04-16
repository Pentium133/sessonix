import { create } from "zustand";
import { getSetting, setSetting } from "../lib/api";

interface SettingsState {
  terminalFontSize: number;
  terminalFontFamily: string;
  defaultAgent: string;
  claudeSkipPermissions: boolean;
  notifyPermission: boolean;
  notifyExit: boolean;
  notifyIdle: boolean;
  loaded: boolean;

  load: () => Promise<void>;
  setTerminalFontSize: (size: number) => void;
  setTerminalFontFamily: (family: string) => void;
  setDefaultAgent: (agent: string) => void;
  setClaudeSkipPermissions: (skip: boolean) => void;
  setNotifyPermission: (v: boolean) => void;
  setNotifyExit: (v: boolean) => void;
  setNotifyIdle: (v: boolean) => void;
}

// Nerd Font variants appended as fallback so terminal glyphs (powerline, icons) render correctly
const NERD_FALLBACK = "'JetBrains Mono Nerd Font', 'JetBrainsMono Nerd Font', 'MesloLGS NF', 'Hack Nerd Font', 'FiraCode Nerd Font'";

const DEFAULTS = {
  terminalFontSize: 13,
  terminalFontFamily: `'JetBrains Mono', ${NERD_FALLBACK}, monospace`,
  defaultAgent: "claude",
  claudeSkipPermissions: false,
  notifyPermission: true,
  notifyExit: true,
  notifyIdle: true,
};

export const FONT_FAMILIES = [
  { label: "JetBrains Mono", value: `'JetBrains Mono', ${NERD_FALLBACK}, monospace` },
  { label: "Fira Code", value: `'Fira Code', ${NERD_FALLBACK}, monospace` },
  { label: "Menlo", value: `Menlo, ${NERD_FALLBACK}, Monaco, monospace` },
  { label: "Monaco", value: `Monaco, ${NERD_FALLBACK}, Menlo, monospace` },
  { label: "Cascadia Code", value: `'Cascadia Code', ${NERD_FALLBACK}, monospace` },
  { label: "SF Mono", value: `'SF Mono', ${NERD_FALLBACK}, Menlo, monospace` },
  { label: "System mono", value: `${NERD_FALLBACK}, monospace` },
];

export const useSettingsStore = create<SettingsState>()((set) => ({
  ...DEFAULTS,
  loaded: false,

  load: async () => {
    const [fontSize, fontFamily, agent, skipPerms, nPerm, nExit, nIdle] = await Promise.all([
      getSetting("terminal_font_size"),
      getSetting("terminal_font_family"),
      getSetting("default_agent"),
      getSetting("claude_skip_permissions"),
      getSetting("notification_permission"),
      getSetting("notification_exit"),
      getSetting("notification_idle"),
    ]);
    set({
      terminalFontSize: fontSize ? parseInt(fontSize, 10) || DEFAULTS.terminalFontSize : DEFAULTS.terminalFontSize,
      terminalFontFamily: fontFamily || DEFAULTS.terminalFontFamily,
      defaultAgent: agent || DEFAULTS.defaultAgent,
      claudeSkipPermissions: skipPerms === "true",
      notifyPermission: nPerm !== null ? nPerm !== "false" : DEFAULTS.notifyPermission,
      notifyExit: nExit !== null ? nExit !== "false" : DEFAULTS.notifyExit,
      notifyIdle: nIdle !== null ? nIdle !== "false" : DEFAULTS.notifyIdle,
      loaded: true,
    });
  },

  setTerminalFontSize: (size) => {
    const clamped = Math.max(10, Math.min(24, size));
    set({ terminalFontSize: clamped });
    setSetting("terminal_font_size", String(clamped)).catch(console.error);
  },

  setTerminalFontFamily: (family) => {
    set({ terminalFontFamily: family });
    setSetting("terminal_font_family", family).catch(console.error);
  },

  setDefaultAgent: (agent) => {
    set({ defaultAgent: agent });
    setSetting("default_agent", agent).catch(console.error);
  },

  setClaudeSkipPermissions: (skip) => {
    set({ claudeSkipPermissions: skip });
    setSetting("claude_skip_permissions", skip ? "true" : "false").catch(console.error);
  },

  setNotifyPermission: (v) => {
    set({ notifyPermission: v });
    setSetting("notification_permission", v ? "true" : "false").catch(console.error);
  },

  setNotifyExit: (v) => {
    set({ notifyExit: v });
    setSetting("notification_exit", v ? "true" : "false").catch(console.error);
  },

  setNotifyIdle: (v) => {
    set({ notifyIdle: v });
    setSetting("notification_idle", v ? "true" : "false").catch(console.error);
  },
}));
