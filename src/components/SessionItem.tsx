import { useState, useEffect } from "react";
import type { Session } from "../lib/types";
import { removeWorktree, clearWorktreePath } from "../lib/git";
import { useSessionStore } from "../store/sessionStore";
import { showToast } from "./Toast";
import AgentIcon from "./AgentIcon";
import WorktreeIcon from "./WorktreeIcon";
import { AGENT_COLORS } from "../lib/constants";

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

function formatDuration(ms: number): string {
  const totalSec = Math.floor(ms / 1000);
  if (totalSec < 60) return `${totalSec}s`;
  const min = Math.floor(totalSec / 60);
  const sec = totalSec % 60;
  if (min < 60) return `${min}m ${sec}s`;
  const hr = Math.floor(min / 60);
  const remainMin = min % 60;
  return `${hr}h ${remainMin}m`;
}

function useDuration(createdAt: number, isAlive: boolean): string {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!isAlive) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [isAlive]);
  const elapsed = (isAlive ? now : Date.now()) - createdAt;
  return formatDuration(Math.max(0, elapsed));
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

  const isAlive = session.status !== "exited";
  const duration = useDuration(session.created_at, isAlive);
  const agentColor = AGENT_COLORS[session.agent_type] ?? AGENT_COLORS.custom;

  const handleKill = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (e.shiftKey) {
      onRemove(session.id);
      return;
    }
    setShowKillConfirm(true);
  };

  // Status indicator dot color
  const statusDotColor = session.status === "running" ? "var(--success)"
    : session.status === "idle" ? "var(--warning)"
    : session.status === "error" ? "var(--error)"
    : "var(--text-dim)";

  // Branch display for worktree sessions
  const showBranch = session.worktree_path && session.gitStatus?.branch && session.gitStatus.branch !== projectBranch;

  return (
    <div
      className={`session-card${isActive ? " active" : ""}${isDragOver ? " drag-over" : ""}`}
      onClick={onSwitch}
      draggable
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
      onDragEnd={onDragEnd}
      style={{ "--agent-color": agentColor } as React.CSSProperties}
    >
      <div className="card-badge" />
      <div className="card-body">
        <div className="card-row-top">
          <AgentIcon agentType={session.agent_type} size={14} />
          <span className="card-name">{session.task_name}</span>
          <span className="card-duration">{duration}</span>
          <span className="card-status-dot" style={{ background: statusDotColor }} />
        </div>
        <div className="card-row-bottom">
          {session.status_line ? (
            <span className="card-status-line" title={session.status_line}>{session.status_line}</span>
          ) : showBranch ? (
            <span className="card-branch">
              <WorktreeIcon className="session-wt-icon" size={10} />
              {session.gitStatus!.branch}
            </span>
          ) : (
            <span className="card-status-line card-status-dim">
              {isAlive ? session.agent_type : "Exited"}
            </span>
          )}
          <span className="card-actions">
            {session.status === "exited" ? (
              <>
                {session.worktree_path && (
                  <button
                    className="card-btn"
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
                  className="card-btn card-btn-relaunch"
                  onClick={(e) => { e.stopPropagation(); onRelaunch(session); }}
                >
                  ↻
                </button>
                <button
                  className="card-btn card-btn-delete"
                  onClick={(e) => { e.stopPropagation(); onRemove(session.id); }}
                  title="Delete session"
                >
                  ×
                </button>
              </>
            ) : (
              <>
                {session.agent_type === "claude" && (
                  showForkConfirm ? (
                    <span className="kill-confirm">
                      <button className="kill-confirm-btn" onClick={(e) => { e.stopPropagation(); onFork(session); setShowForkConfirm(false); }}>Fork?</button>
                      <button className="kill-cancel-btn" onClick={(e) => { e.stopPropagation(); setShowForkConfirm(false); }}>No</button>
                    </span>
                  ) : (
                    <button
                      className="card-btn card-btn-fork"
                      onClick={(e) => { e.stopPropagation(); setShowForkConfirm(true); }}
                      title="Fork session"
                    >
                      ⑂
                    </button>
                  )
                )}
                {showKillConfirm ? (
                  <span className="kill-confirm">
                    <button className="kill-confirm-btn" onClick={(e) => { e.stopPropagation(); onRemove(session.id); setShowKillConfirm(false); }}>Kill</button>
                    <button className="kill-cancel-btn" onClick={(e) => { e.stopPropagation(); setShowKillConfirm(false); }}>Cancel</button>
                  </span>
                ) : (
                  <button
                    className="card-btn card-btn-kill"
                    onClick={handleKill}
                    title="Shift+click to skip confirmation"
                  >
                    ■
                  </button>
                )}
              </>
            )}
          </span>
        </div>
      </div>
    </div>
  );
}
