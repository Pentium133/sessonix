import { useState, useEffect, useMemo } from "react";
import { useSessionStore } from "../store/sessionStore";
import { useProjectStore } from "../store/projectStore";
import { useQuickPromptStore } from "../store/quickPromptStore";
import { useTaskStore } from "../store/taskStore";
import { useUiStore } from "../store/uiStore";
import { useSessionActions } from "../hooks/useSessionActions";
import { useGitStatus } from "../hooks/useGitStatus";
import { useSessionDragAndDrop } from "../hooks/useSessionDragAndDrop";
import { writeToSession, getDefaultShell } from "../lib/api";
import { focusTerminal } from "../lib/terminalPool";
import { showToast } from "./Toast";
import SessionItem from "./SessionItem";
import QuickPromptItem from "./QuickPromptItem";
import QuickPromptSaveModal from "./QuickPromptSaveModal";
import TaskCreateModal from "./TaskCreateModal";
import TaskGroup from "./TaskGroup";
import WorktreeIcon from "./WorktreeIcon";
import type { Session, Task } from "../lib/types";
import type { QuickPromptInfo } from "../lib/api";

export default function Sidebar() {
  const projects = useProjectStore((s) => s.projects);
  const activeProjectPath = useProjectStore((s) => s.activeProjectPath);
  const sessions = useSessionStore((s) => s.sessions);
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const switchSession = useSessionStore((s) => s.switchSession);
  const sidebarCollapsed = useUiStore((s) => s.sidebarCollapsed);
  const width = useUiStore((s) => s.sidebarWidth);
  const toggleCollapse = useUiStore((s) => s.toggleCollapse);

  const [confirmRemoveProject, setConfirmRemoveProject] = useState(false);

  const { draggingId, dragOverId, draggingIdRef, handlersFor: dragHandlersFor } = useSessionDragAndDrop();

  const { handleRemoveSession, handleRelaunchSession, handleForkSession } = useSessionActions();

  const quickPrompts = useQuickPromptStore((s) => s.quickPrompts);
  const loadQuickPrompts = useQuickPromptStore((s) => s.load);
  const addQuickPrompt = useQuickPromptStore((s) => s.add);
  const updateQuickPrompt = useQuickPromptStore((s) => s.update);
  const removeQuickPrompt = useQuickPromptStore((s) => s.remove);

  const tasks = useTaskStore((s) => s.tasks);
  const destroyTask = useTaskStore((s) => s.destroy);

  const [quickPromptModal, setQuickPromptModal] = useState<{ open: boolean; editing?: QuickPromptInfo }>({ open: false });
  const [taskModalOpen, setTaskModalOpen] = useState(false);
  const [expandedTasks, setExpandedTasks] = useState<Record<number, boolean>>({});

  const projectGit = useGitStatus(activeProjectPath, {
    pollMs: 5_000,
    enabled: !sidebarCollapsed,
  });

  useEffect(() => {
    if (activeProjectPath) loadQuickPrompts(activeProjectPath);
  }, [activeProjectPath, loadQuickPrompts]);

  const activeProject = projects.find((p) => p.path === activeProjectPath);
  const projectSessions = sessions
    .filter((s) => s.working_dir === activeProjectPath)
    .sort((a, b) => a.sortOrder - b.sortOrder);

  // Split sessions: ungrouped (task_id === null) stay on top; grouped sessions
  // render inside their task group.
  const { ungroupedSessions, sessionsByTaskId } = useMemo(() => {
    const ungrouped: Session[] = [];
    const byTask = new Map<number, Session[]>();
    for (const s of projectSessions) {
      if (s.task_id == null) {
        ungrouped.push(s);
      } else {
        const list = byTask.get(s.task_id) ?? [];
        list.push(s);
        byTask.set(s.task_id, list);
      }
    }
    return { ungroupedSessions: ungrouped, sessionsByTaskId: byTask };
  }, [projectSessions]);

  const onNewSession = () => {
    if (activeProjectPath) {
      useUiStore.getState().openLauncher({ open: true, mode: "session", projectPath: activeProjectPath });
    }
  };

  const onNewTask = () => {
    if (!activeProjectPath || !projectGit?.is_repo) return;
    setTaskModalOpen(true);
  };

  const toggleTaskExpanded = (taskId: number) => {
    setExpandedTasks((prev) => ({
      ...prev,
      [taskId]: prev[taskId] === undefined ? false : !prev[taskId],
    }));
  };

  const isTaskExpanded = (taskId: number) =>
    expandedTasks[taskId] === undefined ? true : expandedTasks[taskId];

  const openLauncherForTask = (task: Task) => {
    if (!activeProjectPath) return;
    useUiStore.getState().openLauncher({
      open: true,
      mode: "session",
      projectPath: activeProjectPath,
      taskId: task.id,
    });
  };

  const launchShellInTask = async (task: Task) => {
    if (!activeProjectPath) return;
    try {
      const command = await getDefaultShell();
      await useSessionStore.getState().addSession({
        command,
        args: [],
        working_dir: activeProjectPath,
        task_name: "shell",
        agent_type: "shell",
        task_id: task.id,
      });
    } catch (e) {
      showToast(`Failed to launch shell: ${e}`, "error");
    }
  };

  // Confirmation is now inline in TaskGroup header (kill-confirm pattern);
  // this handler runs only after the user actually confirms.
  const deleteTaskConfirmed = async (task: Task) => {
    const taskSessions = sessionsByTaskId.get(task.id) ?? [];
    try {
      const worktreeWarning = await destroyTask(task.id);
      // Backend deletes sessions via cascade; mirror in sessionStore.
      if (taskSessions.length > 0) {
        useSessionStore.getState().removeSessions(taskSessions.map((s) => s.id));
      }
      if (worktreeWarning) {
        showToast(worktreeWarning, "error");
      } else {
        showToast(`Task "${task.name}" removed`, "info");
      }
    } catch (e) {
      showToast(`Failed to remove task: ${e}`, "error");
    }
  };

  const onRemoveProject = async () => {
    if (!activeProjectPath) return;
    if (!confirmRemoveProject) {
      setConfirmRemoveProject(true);
      return;
    }
    setConfirmRemoveProject(false);
    try {
      const removedSessionIds = await useProjectStore.getState().removeProject(activeProjectPath);
      if (removedSessionIds.length > 0) {
        useSessionStore.getState().removeSessions(removedSessionIds);
      }
    } catch (err) {
      showToast(String(err), "error");
    }
  };

  if (sidebarCollapsed) return null; // ProjectRail handles collapsed state

  if (!activeProject) {
    return (
      <aside className="sidebar" style={{ width: `${width}px` }}>
        <div className="sidebar-header">
          <button className="sidebar-collapse-btn" onClick={toggleCollapse} title="Collapse sidebar">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <path d="M8 2L4 6L8 10" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
            </svg>
          </button>
          <h2>Sessions</h2>
        </div>
        <div className="sidebar-content">
          <p className="sidebar-empty">
            Select a project or press <kbd>Cmd+Shift+K</kbd> to add one.
          </p>
        </div>
      </aside>
    );
  }

  return (
    <aside className="sidebar" style={{ width: `${width}px` }}>
      <div className="sidebar-header">
        <button className="sidebar-collapse-btn" onClick={toggleCollapse} title="Collapse sidebar">
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
            <path d="M8 2L4 6L8 10" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
          </svg>
        </button>
        <h2 className="sidebar-project-name" title={activeProject.path}>{activeProject.name}</h2>
        <div className="sidebar-header-actions">
          <button className="new-btn" onClick={onNewSession} title="New session">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
              <line x1="6" y1="1" x2="6" y2="11" />
              <line x1="1" y1="6" x2="11" y2="6" />
            </svg>
          </button>
          <button
            className="new-task-btn"
            onClick={onNewTask}
            disabled={!projectGit?.is_repo}
            title={projectGit?.is_repo ? "New task (creates worktree)" : "Project must be a git repository"}
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="4" cy="3.5" r="1.5" />
              <circle cx="12" cy="3.5" r="1.5" />
              <circle cx="4" cy="12.5" r="1.5" />
              <path d="M4 5v6" />
              <path d="M12 5c0 3-4 3.5-4 6.5" />
            </svg>
          </button>
          {confirmRemoveProject ? (
            <>
              <button
                className="project-remove-confirm-btn"
                onClick={onRemoveProject}
                title="Confirm remove"
              >
                Remove
              </button>
              <button
                className="project-remove-cancel-btn"
                onClick={() => setConfirmRemoveProject(false)}
                title="Cancel"
              >
                Cancel
              </button>
            </>
          ) : (
            <button
              className="project-remove-header-btn"
              onClick={onRemoveProject}
              title="Remove project"
            >
              <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
                <line x1="2" y1="2" x2="10" y2="10" />
                <line x1="10" y1="2" x2="2" y2="10" />
              </svg>
            </button>
          )}
        </div>
      </div>
      {projectGit?.is_repo && (
        <div className="sidebar-git-info">
          {projectGit.is_worktree ? (
            <WorktreeIcon className="session-wt-icon" title="Worktree" />
          ) : (
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="6" y1="3" x2="6" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/>
            </svg>
          )}
          <span className="sidebar-git-branch">{projectGit.branch ?? "detached"}</span>
          {projectGit.changed_files > 0 && (
            <span className="sidebar-git-stats">
              {projectGit.modified > 0 && <span className="sidebar-git-modified">{projectGit.modified}M</span>}
              {projectGit.added > 0 && <span className="sidebar-git-added">{projectGit.added}A</span>}
              {projectGit.deleted > 0 && <span className="sidebar-git-deleted">{projectGit.deleted}D</span>}
            </span>
          )}
        </div>
      )}
      <div className="sidebar-content">
        {projectSessions.length === 0 && tasks.length === 0 ? (
          <div className="session-empty">
            No sessions.{" "}
            <button className="inline-link" onClick={onNewSession}>
              Start one
            </button>
          </div>
        ) : (
          <>
            {ungroupedSessions.map((session) => (
              <SessionItem
                key={session.id}
                session={session}
                isActive={session.id === activeSessionId}
                isDragOver={dragOverId === session.id && draggingId !== session.id}
                projectBranch={projectGit?.branch ?? null}
                onSwitch={() => { if (!draggingIdRef.current) switchSession(session.id); }}
                onRelaunch={handleRelaunchSession}
                onRemove={handleRemoveSession}
                onFork={handleForkSession}
                {...dragHandlersFor(session)}
              />
            ))}

            {tasks.length > 0 && (
              <div className="sidebar-tasks-header">Tasks</div>
            )}
            {tasks.map((task) => (
              <TaskGroup
                key={task.id}
                task={task}
                sessions={sessionsByTaskId.get(task.id) ?? []}
                isExpanded={isTaskExpanded(task.id)}
                activeSessionId={activeSessionId}
                projectBranch={projectGit?.branch ?? null}
                onToggle={() => toggleTaskExpanded(task.id)}
                onAddAgent={() => openLauncherForTask(task)}
                onInstantShell={() => launchShellInTask(task)}
                onDelete={() => deleteTaskConfirmed(task)}
                onSwitchSession={(id) => switchSession(id)}
                onRelaunchSession={handleRelaunchSession}
                onRemoveSession={handleRemoveSession}
                onForkSession={handleForkSession}
              />
            ))}
          </>
        )}
      </div>
      <div className="sidebar-quick-prompts">
        <div className="sidebar-quick-prompts-header">
          <span>Quick Prompts</span>
          <button
            className="new-btn"
            onClick={() => setQuickPromptModal({ open: true })}
            title="New quick prompt"
          >
            <svg width="10" height="10" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
              <line x1="6" y1="1" x2="6" y2="11" />
              <line x1="1" y1="6" x2="11" y2="6" />
            </svg>
          </button>
        </div>
        {quickPrompts.length === 0 ? (
          <div className="sidebar-quick-prompts-empty">
            No quick prompts yet
          </div>
        ) : (
          quickPrompts.map((t) => (
            <QuickPromptItem
              key={t.id}
              quickPrompt={t}
              onRun={() => {
                const sid = activeSessionId;
                if (sid == null) {
                  showToast("No active session", "error");
                  return;
                }
                const text = t.initial_prompt ?? "";
                if (!text) {
                  showToast("Quick prompt is empty", "error");
                  return;
                }
                const encoder = new TextEncoder();
                writeToSession(sid, Array.from(encoder.encode(text + "\n")))
                  .then(() => focusTerminal(sid))
                  .catch((err) => showToast(String(err), "error"));
              }}
              onEdit={() => setQuickPromptModal({ open: true, editing: t })}
              onDelete={() => {
                removeQuickPrompt(t.id).catch((err) => showToast(String(err), "error"));
              }}
            />
          ))
        )}
      </div>
      {taskModalOpen && activeProjectPath && (
        <TaskCreateModal
          projectPath={activeProjectPath}
          onClose={() => setTaskModalOpen(false)}
        />
      )}
      {quickPromptModal.open && (
        <QuickPromptSaveModal
          title={quickPromptModal.editing ? "Edit Quick Prompt" : "New Quick Prompt"}
          initialName={quickPromptModal.editing?.name}
          initialPrompt={quickPromptModal.editing?.initial_prompt ?? undefined}
          onClose={() => setQuickPromptModal({ open: false })}
          onSave={async (name, prompt) => {
            if (!activeProjectPath) return;
            try {
              if (quickPromptModal.editing) {
                await updateQuickPrompt(quickPromptModal.editing.id, name, prompt);
                showToast(`Quick prompt "${name}" updated`, "success");
              } else {
                await addQuickPrompt({ name, project_path: activeProjectPath, agent: "", initial_prompt: prompt, skip_permissions: false });
                showToast(`Quick prompt "${name}" saved`, "success");
              }
              setQuickPromptModal({ open: false });
            } catch (err) {
              showToast(String(err), "error");
            }
          }}
        />
      )}
    </aside>
  );
}
