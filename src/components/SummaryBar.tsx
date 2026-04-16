import { useSessionStore } from "../store/sessionStore";
import { useProjectStore } from "../store/projectStore";
import AgentIcon from "./AgentIcon";

export default function SummaryBar() {
  const sessions = useSessionStore((s) => s.sessions);
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const switchSession = useSessionStore((s) => s.switchSession);
  const activeProjectPath = useProjectStore((s) => s.activeProjectPath);

  const running = sessions
    .filter(
      (s) =>
        s.working_dir === activeProjectPath &&
        (s.status === "running" || s.status === "idle")
    )
    .sort((a, b) => a.sortOrder - b.sortOrder);

  if (running.length === 0) return null;

  const MAX_VISIBLE = 5;
  const visible = running.slice(0, MAX_VISIBLE);
  const overflow = running.length - MAX_VISIBLE;

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
    </div>
  );
}
