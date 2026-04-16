import { useState } from "react";
import type { Session, Task } from "../lib/types";
import SessionItem from "./SessionItem";

interface TaskGroupProps {
  task: Task;
  sessions: Session[];
  isExpanded: boolean;
  activeSessionId: number | null;
  projectBranch: string | null;
  onToggle: () => void;
  onAddAgent: () => void;
  onInstantShell: () => void;
  onDelete: () => void;
  onSwitchSession: (id: number) => void;
  onRelaunchSession: (session: Session) => void;
  onRemoveSession: (id: number) => void;
  onForkSession: (session: Session) => void;
}

export default function TaskGroup({
  task,
  sessions,
  isExpanded,
  activeSessionId,
  projectBranch,
  onToggle,
  onAddAgent,
  onInstantShell,
  onDelete,
  onSwitchSession,
  onRelaunchSession,
  onRemoveSession,
  onForkSession,
}: TaskGroupProps) {
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const runningCount = sessions.filter((s) => s.status !== "exited").length;

  return (
    <div className="task-group">
      <div className="task-group-header" onClick={onToggle}>
        <svg
          className={`task-group-chevron${isExpanded ? " expanded" : ""}`}
          width="10"
          height="10"
          viewBox="0 0 12 12"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M4 2L8 6L4 10" />
        </svg>
        <span className="task-group-name" title={task.name}>{task.name}</span>
        {task.branch && (
          <span className="task-branch-badge" title={task.branch}>{task.branch}</span>
        )}
        {sessions.length > 0 && (
          <span className="task-group-count" title={`${runningCount} running / ${sessions.length} total`}>
            {sessions.length}
          </span>
        )}
        {showDeleteConfirm ? (
          <span className="kill-confirm" onClick={(e) => e.stopPropagation()}>
            <button
              className="kill-confirm-btn"
              onClick={() => {
                setShowDeleteConfirm(false);
                onDelete();
              }}
              title={runningCount > 0
                ? `Kill ${runningCount} running session${runningCount > 1 ? "s" : ""} and remove worktree`
                : "Remove task and worktree"}
            >
              {runningCount > 0 ? `Kill ${runningCount} + remove` : "Remove"}
            </button>
            <button
              className="kill-cancel-btn"
              onClick={() => setShowDeleteConfirm(false)}
            >
              Cancel
            </button>
          </span>
        ) : (
          <div className="task-group-actions" onClick={(e) => e.stopPropagation()}>
            <button
              className="task-group-btn"
              onClick={onAddAgent}
              title="New agent session in this task"
            >
              <svg width="10" height="10" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                <line x1="6" y1="1" x2="6" y2="11" />
                <line x1="1" y1="6" x2="11" y2="6" />
              </svg>
            </button>
            <button
              className="task-group-btn"
              onClick={onInstantShell}
              title="Open shell in this task"
            >
              <svg width="10" height="10" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M2 3L5 6L2 9" />
                <line x1="6" y1="9" x2="10" y2="9" />
              </svg>
            </button>
            <button
              className="task-group-btn task-group-btn-danger"
              onClick={() => setShowDeleteConfirm(true)}
              title="Delete task and its worktree"
            >
              <svg width="10" height="10" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
                <line x1="2" y1="2" x2="10" y2="10" />
                <line x1="10" y1="2" x2="2" y2="10" />
              </svg>
            </button>
          </div>
        )}
      </div>
      {isExpanded && (
        <div className="task-group-body">
          {sessions.length === 0 ? (
            <div className="task-group-empty">No sessions yet</div>
          ) : (
            // Drag-and-drop for sessions inside a task group is intentionally
            // disabled: cross-task reordering would need a DB column + semantics
            // for moving a session between tasks (out of scope for MVP).
            // `draggable={false}` on the card removes the misleading ghost hint.
            sessions.map((s) => (
              <SessionItem
                key={s.id}
                session={s}
                isActive={s.id === activeSessionId}
                isDragOver={false}
                projectBranch={projectBranch}
                draggable={false}
                onSwitch={() => onSwitchSession(s.id)}
                onRelaunch={onRelaunchSession}
                onRemove={onRemoveSession}
                onFork={onForkSession}
                onDragStart={() => {}}
                onDragOver={() => {}}
                onDragLeave={() => {}}
                onDrop={() => {}}
                onDragEnd={() => {}}
              />
            ))
          )}
        </div>
      )}
    </div>
  );
}
