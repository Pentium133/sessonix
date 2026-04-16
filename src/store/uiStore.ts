import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { Theme } from "../lib/constants";
import { SIDEBAR_MIN, SIDEBAR_MAX, SIDEBAR_DEFAULT } from "../lib/constants";
import { THEME_IDS, getThemeById, resolveSystemTheme } from "../lib/themes";
import type { ThemeId } from "../lib/themes";
import { getSetting, setSetting } from "../lib/api";

const ZOOM_MIN = 0.75;
const ZOOM_MAX = 1.5;
const ZOOM_STEP = 0.05;

function applyZoom(zoom: number) {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  root.style.setProperty("zoom", String(zoom));
  root.style.setProperty("--ui-zoom-inverse", String(1 / zoom));
  // Compensate: zoom on <html> scales the box, so shrink dimensions
  // so that zoomed box fits exactly in the viewport.
  root.style.height = `${100 / zoom}vh`;
  root.style.width = `${100 / zoom}vw`;
  root.style.overflow = "hidden";
}

let zoomTimer: ReturnType<typeof setTimeout> | undefined;
function debouncedSaveZoom(zoom: number) {
  clearTimeout(zoomTimer);
  zoomTimer = setTimeout(() => {
    void setSetting("ui_zoom", String(zoom));
  }, 300);
}
type LauncherState =
  | { open: false }
  | { open: true; mode: "project" }
  | { open: true; mode: "session"; projectPath: string };

interface UiState {
  sidebarWidth: number;
  sidebarCollapsed: boolean;
  theme: Theme;
  uiZoom: number;
  launcher: LauncherState;
  settingsOpen: boolean;

  setSidebarWidth: (width: number) => void;
  toggleCollapse: () => void;
  setCollapsed: (collapsed: boolean) => void;
  setTheme: (theme: Theme) => void;
  cycleTheme: () => void;
  setUiZoom: (zoom: number) => void;
  zoomIn: () => void;
  zoomOut: () => void;
  resetZoom: () => void;
  openLauncher: (state: LauncherState) => void;
  closeLauncher: () => void;
  openSettings: () => void;
  closeSettings: () => void;
}

function applyTheme(theme: Theme) {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  const resolvedId: ThemeId = theme === "system" ? resolveSystemTheme() : theme;
  const def = getThemeById(resolvedId);
  if (!def) return;

  for (const [key, value] of Object.entries(def.cssVars)) {
    root.style.setProperty(key, value);
  }

  root.setAttribute("data-theme", resolvedId);
  root.style.setProperty("color-scheme", def.type);
}

// Cycle order: system → all theme ids → system
const CYCLE_ORDER: Theme[] = ["system", ...THEME_IDS];
const VALID_THEMES: ReadonlySet<string> = new Set(CYCLE_ORDER);

function isValidTheme(value: string): value is Theme {
  return VALID_THEMES.has(value);
}

let sidebarWidthTimer: ReturnType<typeof setTimeout> | undefined;
function debouncedSaveSidebarWidth(width: number) {
  clearTimeout(sidebarWidthTimer);
  sidebarWidthTimer = setTimeout(() => {
    void setSetting("sidebar_width", String(width));
  }, 300);
}

export const useUiStore = create<UiState>()(
  persist(
    (set, get) => ({
      sidebarWidth: SIDEBAR_DEFAULT,
      sidebarCollapsed: false,
      theme: "system" as Theme,
      uiZoom: 1.0,
      launcher: { open: false } as LauncherState,
      settingsOpen: false,

      setSidebarWidth: (width) => {
        const clamped = Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, width));
        set({ sidebarWidth: clamped });
        debouncedSaveSidebarWidth(clamped);
      },

      toggleCollapse: () => {
        const next = !get().sidebarCollapsed;
        set({ sidebarCollapsed: next });
        void setSetting("sidebar_collapsed", next ? "true" : "false");
      },

      setCollapsed: (collapsed) => {
        set({ sidebarCollapsed: collapsed });
        void setSetting("sidebar_collapsed", collapsed ? "true" : "false");
      },

      setTheme: (theme: Theme) => {
        applyTheme(theme);
        set({ theme });
        void setSetting("theme", theme);
      },

      cycleTheme: () => {
        const { theme } = get();
        const idx = CYCLE_ORDER.indexOf(theme);
        const safeIdx = idx === -1 ? 0 : idx;
        const next = CYCLE_ORDER[(safeIdx + 1) % CYCLE_ORDER.length];
        applyTheme(next);
        set({ theme: next });
        void setSetting("theme", next);
      },

      setUiZoom: (zoom) => {
        const clamped = Math.round(Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, zoom)) * 20) / 20;
        applyZoom(clamped);
        set({ uiZoom: clamped });
        debouncedSaveZoom(clamped);
      },

      zoomIn: () => {
        const { uiZoom, setUiZoom } = get();
        setUiZoom(uiZoom + ZOOM_STEP);
      },

      zoomOut: () => {
        const { uiZoom, setUiZoom } = get();
        setUiZoom(uiZoom - ZOOM_STEP);
      },

      resetZoom: () => {
        get().setUiZoom(1.0);
      },

      openLauncher: (state) => set({ launcher: state }),

      closeLauncher: () => set({ launcher: { open: false } }),

      openSettings: () => set({ settingsOpen: true }),
      closeSettings: () => set({ settingsOpen: false }),
    }),
    {
      name: "sessonix-ui",
      partialize: (state) => ({
        sidebarWidth: state.sidebarWidth,
        sidebarCollapsed: state.sidebarCollapsed,
        theme: state.theme,
        uiZoom: state.uiZoom,
      }),
      merge: (persisted, current) => {
        const p = persisted as Partial<UiState> | undefined;
        let theme: Theme = p?.theme ?? "system";

        // Migrate old "dark"/"light" values from pre-themes era
        if (theme === ("dark" as string)) theme = "sessonix-dark";
        if (theme === ("light" as string)) theme = "sessonix-light";

        // Validate persisted theme (e.g. downgrade scenario)
        if (!isValidTheme(theme as string)) theme = "system";

        const rawZoom = p?.uiZoom;
        const uiZoom = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX,
          typeof rawZoom === "number" && !isNaN(rawZoom) ? rawZoom : 1.0
        ));

        const merged = { ...current, ...p, theme, uiZoom } as UiState;
        applyTheme(merged.theme);
        applyZoom(merged.uiZoom);
        return merged;
      },
    }
  )
);

// Restore UI settings from SQLite (authoritative source, overrides localStorage)
void Promise.all([
  getSetting("theme"),
  getSetting("sidebar_width"),
  getSetting("sidebar_collapsed"),
  getSetting("ui_zoom"),
]).then(([dbTheme, dbWidth, dbCollapsed, dbZoom]) => {
  const state = useUiStore.getState();

  if (dbTheme && isValidTheme(dbTheme) && dbTheme !== state.theme) {
    state.setTheme(dbTheme as Theme);
  }

  const updates: Partial<Pick<UiState, "sidebarWidth" | "sidebarCollapsed">> = {};
  if (dbWidth) {
    const w = parseInt(dbWidth, 10);
    if (!isNaN(w) && w !== state.sidebarWidth) {
      updates.sidebarWidth = Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, w));
    }
  }
  if (dbCollapsed !== null) {
    const collapsed = dbCollapsed === "true";
    if (collapsed !== state.sidebarCollapsed) {
      updates.sidebarCollapsed = collapsed;
    }
  }
  if (Object.keys(updates).length > 0) {
    useUiStore.setState(updates);
  }

  if (dbZoom) {
    const z = parseFloat(dbZoom);
    if (!isNaN(z) && z !== state.uiZoom) {
      state.setUiZoom(z);
    }
  }
});

// Listen for OS theme changes — update CSS vars when preference is "system"
if (typeof window !== "undefined") {
  window
    .matchMedia("(prefers-color-scheme: light)")
    .addEventListener("change", () => {
      const { theme } = useUiStore.getState();
      if (theme === "system") {
        applyTheme("system");
      }
    });
}
