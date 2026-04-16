import { useSessionStore } from "../store/sessionStore";
import { useProjectStore } from "../store/projectStore";
import { useUiStore } from "../store/uiStore";
import { version } from "../../package.json";
import { getThemeById, resolveSystemTheme } from "../lib/themes";

export default function StatusBar() {
  const sessions = useSessionStore((s) => s.sessions);
  const projects = useProjectStore((s) => s.projects);
  const theme = useUiStore((s) => s.theme);
  const cycleTheme = useUiStore((s) => s.cycleTheme);

  const running = sessions.filter((s) => s.status === "running").length;
  const total = sessions.length;

  const resolvedId = theme === "system" ? resolveSystemTheme() : theme;
  const themeDef = getThemeById(resolvedId);
  const isDark = themeDef?.type !== "light";
  const themeLabel = theme === "system" ? `System (${themeDef?.label ?? ""})` : themeDef?.label ?? theme;

  return (
    <div className="statusbar">
      <span>
        {projects.length} project{projects.length !== 1 ? "s" : ""} &middot;{" "}
        {running} running / {total} total
      </span>

      <span className="statusbar-right">
        <button
          className="theme-toggle-sm"
          onClick={cycleTheme}
          title={`Theme: ${themeLabel}`}
        >
          {isDark ? (
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>
            </svg>
          ) : (
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/>
            </svg>
          )}
        </button>
        v{version}
      </span>
    </div>
  );
}
