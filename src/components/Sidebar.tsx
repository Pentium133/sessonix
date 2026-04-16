import { useState, useRef, useEffect, useMemo } from "react";
import { useSessionStore } from "../store/sessionStore";
import { useProjectStore } from "../store/projectStore";
import { useTemplateStore } from "../store/templateStore";
import { useTaskStore } from "../store/taskStore";
import { useUiStore } from "../store/uiStore";
import { useSessionActions } from "../hooks/useSessionActions";
import { getGitStatus } from "../lib/git";
import { writeToSession } from "../lib/api";
import { focusTerminal } from "./TerminalPane";
import { showToast } from "./Toast";
import SessionItem from "./SessionItem";
import TemplateItem from "./TemplateItem";
import TemplateSaveModal from "./TemplateSaveModal";
import TaskCreateModal from "./TaskCreateModal";
import TaskGroup from "./TaskGroup";
import WorktreeIcon from "./WorktreeIcon";
import type { GitStatus, Session, Task } from "../lib/types";
import type { TemplateInfo } from "../lib/api";

export default function Sidebar() {
  const projects = useProjectStore((s) => s.projects);
  const activeProjectPath = useProjectStore((s) => s.activeProjectPath);
  const sessions = useSessionStore((s) => s.sessions);
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const switchSession = useSessionStore((s) => s.switchSession);
  const reorderSessionOrder = useSessionStore((s) => s.reorderSessionOrder);
  const sidebarCollapsed = useUiStore((s) => s.sidebarCollapsed);
  const width = useUiStore((s) => s.sidebarWidth);
  const toggleCollapse = useUiStore((s) => s.toggleCollapse);

  const [confirmRemoveProject, setConfirmRemoveProject] = useState(false);

  // Drag state for reordering sessions
  const [draggingId, setDraggingId] = useState<number | null>(null);
  const [dragOverId, setDragOverId] = useState<number | null>(null);
  const draggingIdRef = useRef<number | null>(null);
  const dragOverRef = useRef<{ id: number; sortOrder: number } | null>(null);
  const dropAcceptedRef = useRef(false);

  const { handleRemoveSession, handleRelaunchSession, handleForkSession } = useSessionActions();

  const templates = useTemplateStore((s) => s.templates);
  const loadTemplates = useTemplateStore((s) => s.load);
  const addTemplate = useTemplateStore((s) => s.add);
  const updateTemplate = useTemplateStore((s) => s.update);
  const removeTemplate = useTemplateStore((s) => s.remove);

  const tasks = useTaskStore((s) => s.tasks);
  const destroyTask = useTaskStore((s) => s.destroy);

  const [templateModal, setTemplateModal] = useState<{ open: boolean; editing?: TemplateInfo }>({ open: false });
  const [taskModalOpen, setTaskModalOpen] = useState(false);
  const [expandedTasks, setExpandedTasks] = useState<Record<number, boolean>>({});

  const [projectGit, setProjectGit] = useState<GitStatus | null>(null);

  useEffect(() => {
    if (!activeProjectPath) { setProjectGit(null); return; }
    const fetch = () => {
      getGitStatus(activeProjectPath).then(setProjectGit).catch(() => setProjectGit(null));
    };
    fetch();
    const interval = setInterval(fetch, 5_000);
    return () => clearInterval(interval);
  }, [activeProjectPath]);

  useEffect(() => {
    if (activeProjectPath) loadTemplates(activeProjectPath);
  }, [activeProjectPath, loadTemplates]);

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
      await useSessionStore.getState().addSession({
        command: "zsh",
        args: [],
        working_dir: activeProjectPath,
        task_name: `${task.name} shell`,
        agent_type: "shell",
        task_id: task.id,
      });
    } catch (e) {
      showToast(`Failed to launch shell: ${e}`, "error");
    }
  };

  const deleteTaskWithConfirm = async (task: Task) => {
    const taskSessions = sessionsByTaskId.get(task.id) ?? [];
    const runningCount = taskSessions.filter((s) => s.status !== "exited").length;
    const msg = runningCount > 0
      ? `Kill ${runningCount} running session${runningCount > 1 ? "s" : ""} and remove "${task.name}" (worktree will be deleted)?`
      : `Remove task "${task.name}" and its worktree?`;
    if (!window.confirm(msg)) return;
    try {
      await destroyTask(task.id);
      // Backend deletes sessions via cascade; mirror in sessionStore.
      if (taskSessions.length > 0) {
        useSessionStore.getState().removeSessions(taskSessions.map((s) => s.id));
      }
      showToast(`Task "${task.name}" removed`, "info");
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
            className="new-btn new-task-btn"
            onClick={onNewTask}
            disabled={!projectGit?.is_repo}
            title={projectGit?.is_repo ? "New task (creates worktree)" : "Project must be a git repository"}
          >
            <WorktreeIcon className="session-wt-icon" />
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
                onDragStart={(e) => {
                  setDraggingId(session.id);
                  draggingIdRef.current = session.id;
                  e.dataTransfer.effectAllowed = "move";
                  e.dataTransfer.setData("text/plain", String(session.id));
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  e.dataTransfer.dropEffect = "move";
                  if (session.id !== draggingIdRef.current) {
                    setDragOverId(session.id);
                    dragOverRef.current = { id: session.id, sortOrder: session.sortOrder };
                  }
                }}
                onDragLeave={(e) => {
                  const related = e.relatedTarget as Node | null;
                  if (!e.currentTarget.contains(related)) {
                    setDragOverId(null);
                  }
                }}
                onDrop={(e) => {
                  e.preventDefault();
                  dropAcceptedRef.current = true;
                }}
                onDragEnd={() => {
                  const dragId = draggingIdRef.current;
                  const over = dragOverRef.current;
                  if (dropAcceptedRef.current && dragId !== null && over && dragId !== over.id) {
                    reorderSessionOrder(dragId, over.sortOrder);
                  }
                  setDraggingId(null);
                  draggingIdRef.current = null;
                  setDragOverId(null);
                  dragOverRef.current = null;
                  dropAcceptedRef.current = false;
                }}
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
                onDelete={() => deleteTaskWithConfirm(task)}
                onSwitchSession={(id) => switchSession(id)}
                onRelaunchSession={handleRelaunchSession}
                onRemoveSession={handleRemoveSession}
                onForkSession={handleForkSession}
              />
            ))}
          </>
        )}
      </div>
      <div className="sidebar-templates">
        <div className="sidebar-templates-header">
          <span>Templates</span>
          <button
            className="new-btn"
            onClick={() => setTemplateModal({ open: true })}
            title="New template"
          >
            <svg width="10" height="10" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
              <line x1="6" y1="1" x2="6" y2="11" />
              <line x1="1" y1="6" x2="11" y2="6" />
            </svg>
          </button>
        </div>
        {templates.length === 0 ? (
          <div className="sidebar-templates-empty">
            No templates yet
          </div>
        ) : (
          templates.map((t) => (
            <TemplateItem
              key={t.id}
              template={t}
              onRun={() => {
                const sid = activeSessionId;
                if (sid == null) {
                  showToast("No active session", "error");
                  return;
                }
                const text = t.initial_prompt ?? "";
                if (!text) {
                  showToast("Template has no prompt", "error");
                  return;
                }
                const encoder = new TextEncoder();
                writeToSession(sid, Array.from(encoder.encode(text + "\n")))
                  .then(() => focusTerminal(sid))
                  .catch((err) => showToast(String(err), "error"));
              }}
              onEdit={() => setTemplateModal({ open: true, editing: t })}
              onDelete={() => {
                removeTemplate(t.id).catch((err) => showToast(String(err), "error"));
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
      {templateModal.open && (
        <TemplateSaveModal
          title={templateModal.editing ? "Edit Template" : "New Template"}
          initialName={templateModal.editing?.name}
          initialPrompt={templateModal.editing?.initial_prompt ?? undefined}
          onClose={() => setTemplateModal({ open: false })}
          onSave={async (name, prompt) => {
            if (!activeProjectPath) return;
            try {
              if (templateModal.editing) {
                await updateTemplate(templateModal.editing.id, name, prompt);
                showToast(`Template "${name}" updated`, "success");
              } else {
                await addTemplate({ name, project_path: activeProjectPath, agent: "", initial_prompt: prompt, skip_permissions: false });
                showToast(`Template "${name}" saved`, "success");
              }
              setTemplateModal({ open: false });
            } catch (err) {
              showToast(String(err), "error");
            }
          }}
        />
      )}
    </aside>
  );
}
