import { useState } from "react";
import type { Session } from "../lib/types";
import { removeWorktree, clearWorktreePath } from "../lib/git";
import { useSessionStore } from "../store/sessionStore";
import { showToast } from "./Toast";
import AgentIcon from "./AgentIcon";
import WorktreeIcon from "./WorktreeIcon";

const STATUS_BADGES: Record<string, { color: string; label: string }> = {
  running: { color: "var(--success)", label: "Running" },
  idle: { color: "var(--warning)", label: "Idle" },
  error: { color: "var(--error)", label: "Error" },
  exited: { color: "var(--text-dim)", label: "Exited" },
};

interface SessionItemProps {
  session: Session;
  isActive: boolean;
  isDragOver: boolean;
  projectBranch: string | null;
  onSwitch: () => void;
  onRelaunch: (session: Session) => void;
  onRemove: (id: number) => void;
  onFork: (session: Session) => void;
  onDragStart: (e: React.DragEvent) => void;
  onDragOver: (e: React.DragEvent) => void;
  onDragLeave: (e: React.DragEvent) => void;
  onDrop: (e: React.DragEvent) => void;
  onDragEnd: () => void;
}

export default function SessionItem({
  session,
  isActive,
  isDragOver,
  projectBranch,
  onSwitch,
  onRelaunch,
  onRemove,
  onFork,
  onDragStart,
  onDragOver,
  onDragLeave,
  onDrop,
  onDragEnd,
}: SessionItemProps) {
  const [showKillConfirm, setShowKillConfirm] = useState(false);
  const [showForkConfirm, setShowForkConfirm] = useState(false);

  const badge = STATUS_BADGES[session.status] ?? STATUS_BADGES.running;

  const handleKill = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (e.shiftKey) {
      onRemove(session.id);
      return;
    }
    setShowKillConfirm(true);
  };

  return (
    <div
      className={`session-item${isActive ? " active" : ""}${isDragOver ? " drag-over" : ""}`}
      onClick={onSwitch}
      draggable
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
      onDragEnd={onDragEnd}
    >
      <div className="session-item-header">
        <AgentIcon agentType={session.agent_type} />
        <span className="session-name">{session.task_name}</span>
        <span className="status-badge" style={{ color: badge.color }}>
          {badge.label}
        </span>
      </div>
      <div className="session-item-meta">
        <span className="session-agent-label">
          {session.worktree_path && session.gitStatus?.branch && session.gitStatus.branch !== projectBranch ? (
            <>
              <WorktreeIcon className="session-wt-icon" />
              <span className="session-branch" title={session.gitStatus.branch}>
                {session.gitStatus.branch}
              </span>
            </>
          ) : (
            session.agent_type
          )}
        </span>
        {session.status === "exited" ? (
          <span className="session-actions">
            {session.worktree_path && (
              <button
                className="wt-remove-btn"
                onClick={(e) => {
                  e.stopPropagation();
                  removeWorktree(session.worktree_path!).then(() => {
                    clearWorktreePath(session.id).catch(console.error);
                    useSessionStore.getState().clearSessionWorktree(session.id);
                    showToast("Worktree removed", "info");
                  }).catch((err) => showToast(`Failed: ${err}`, "error"));
                }}
                title="Remove worktree and branch"
              >
                <WorktreeIcon size={10} />&times;
              </button>
            )}
            <button
              className="relaunch-btn"
              onClick={(e) => { e.stopPropagation(); onRelaunch(session); }}
            >
              Relaunch
            </button>
            <button
              className="delete-btn"
              onClick={(e) => { e.stopPropagation(); onRemove(session.id); }}
              title="Delete session"
            >
              &times;
            </button>
          </span>
        ) : (
          <>
            {session.agent_type === "claude" && (
              showForkConfirm ? (
                <span className="kill-confirm">
                  <button
                    className="kill-confirm-btn"
                    onClick={(e) => { e.stopPropagation(); onFork(session); setShowForkConfirm(false); }}
                  >
                    Fork?
                  </button>
                  <button
                    className="kill-cancel-btn"
                    onClick={(e) => { e.stopPropagation(); setShowForkConfirm(false); }}
                  >
                    No
                  </button>
                </span>
              ) : (
                <button
                  className="fork-btn"
                  onClick={(e) => { e.stopPropagation(); setShowForkConfirm(true); }}
                  title="Fork session (explore alternative approach)"
                >
                  Fork
                </button>
              )
            )}
            {showKillConfirm ? (
              <span className="kill-confirm">
                <button
                  className="kill-confirm-btn"
                  onClick={(e) => { e.stopPropagation(); onRemove(session.id); setShowKillConfirm(false); }}
                >
                  Kill
                </button>
                <button
                  className="kill-cancel-btn"
                  onClick={(e) => { e.stopPropagation(); setShowKillConfirm(false); }}
                >
                  Cancel
                </button>
              </span>
            ) : (
              <button
                className="kill-btn"
                onClick={handleKill}
                title="Shift+click to skip confirmation"
              >
                Kill
              </button>
            )}
          </>
        )}
      </div>
    </div>
  );
}
