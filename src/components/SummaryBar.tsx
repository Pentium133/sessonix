import { useSessionStore, DIFF_PSEUDO_ID } from "../store/sessionStore";
import { useProjectStore } from "../store/projectStore";
import AgentIcon from "./AgentIcon";

export default function SummaryBar() {
  const sessions = useSessionStore((s) => s.sessions);
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const switchSession = useSessionStore((s) => s.switchSession);
  const activeProjectPath = useProjectStore((s) => s.activeProjectPath);

  // Show every non-exited session. Transient statuses like `error` used to
  // drop the tab out of the bar until the next poll tick repaired it, which
  // looked like the tab vanishing mid-work.
  const running = sessions
    .filter((s) => s.working_dir === activeProjectPath && s.status !== "exited")
    .sort((a, b) => a.sortOrder - b.sortOrder);

  const MAX_VISIBLE = 5;
  const visible = running.slice(0, MAX_VISIBLE);
  const overflow = running.length - MAX_VISIBLE;
  const diffActive = activeSessionId === DIFF_PSEUDO_ID;

  if (!activeProjectPath) return null;

  return (
    <div className="summary-bar">
      {visible.map((session) => {
        const isActive = session.id === activeSessionId;

        return (
          <button
            key={session.id}
            className={`summary-item ${isActive ? "active" : ""}`}
            onClick={() => switchSession(session.id)}
          >
            <AgentIcon agentType={session.agent_type} size={18} />
            <span className="summary-name">{session.task_name}</span>
            {session.status_line && (
              <span className="summary-status">{session.status_line}</span>
            )}
          </button>
        );
      })}
      {overflow > 0 && (
        <span className="summary-overflow">+{overflow} more</span>
      )}
      <button
        type="button"
        className={`summary-item summary-diff-btn ${diffActive ? "active" : ""}`}
        onClick={() => switchSession(DIFF_PSEUDO_ID)}
        aria-label="Show diff"
        title="Show diff (Cmd+0)"
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
          <path
            d="M6 3v10M10 3v10M3 6h6M7 10h6"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
          />
        </svg>
        <span className="summary-name">Diff</span>
      </button>
    </div>
  );
}
