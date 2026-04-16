import { useState, useRef, useEffect } from "react";
import { useSessionStore } from "../store/sessionStore";
import { useProjectStore } from "../store/projectStore";
import { useTemplateStore } from "../store/templateStore";
import { useUiStore } from "../store/uiStore";
import { useSessionActions } from "../hooks/useSessionActions";
import { getGitStatus } from "../lib/git";
import { writeToSession } from "../lib/api";
import { focusTerminal } from "./TerminalPane";
import { showToast } from "./Toast";
import SessionItem from "./SessionItem";
import TemplateItem from "./TemplateItem";
import TemplateSaveModal from "./TemplateSaveModal";
import WorktreeIcon from "./WorktreeIcon";
import type { GitStatus } from "../lib/types";
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
  const removeTemplate = useTemplateStore((s) => s.remove);

  const [templateModal, setTemplateModal] = useState<{ open: boolean; editing?: TemplateInfo }>({ open: false });

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

  const onNewSession = () => {
    if (activeProjectPath) {
      useUiStore.getState().openLauncher({ open: true, mode: "session", projectPath: activeProjectPath });
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
        {projectSessions.length === 0 ? (
          <div className="session-empty">
            No sessions.{" "}
            <button className="inline-link" onClick={onNewSession}>
              Start one
            </button>
          </div>
        ) : (
          projectSessions.map((session) => (
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
          ))
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
                if (!text) return;
                const encoder = new TextEncoder();
                writeToSession(sid, Array.from(encoder.encode(text)))
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
      <TemplateSaveModal
        isOpen={templateModal.open}
        onClose={() => setTemplateModal({ open: false })}
        initial={templateModal.editing ? { name: templateModal.editing.name, prompt: templateModal.editing.initial_prompt ?? "" } : undefined}
        onSave={(name, prompt) => {
          if (!activeProjectPath) return;
          if (templateModal.editing) {
            // Update existing: delete + recreate (simple approach)
            removeTemplate(templateModal.editing.id)
              .then(() => addTemplate({ name, project_path: activeProjectPath, agent: "", initial_prompt: prompt, skip_permissions: false }))
              .then(() => showToast(`Template "${name}" updated`, "success"))
              .catch((err) => showToast(String(err), "error"));
          } else {
            addTemplate({ name, project_path: activeProjectPath, agent: "", initial_prompt: prompt, skip_permissions: false })
              .then(() => showToast(`Template "${name}" saved`, "success"))
              .catch((err) => showToast(String(err), "error"));
          }
          setTemplateModal({ open: false });
        }}
      />
    </aside>
  );
}
