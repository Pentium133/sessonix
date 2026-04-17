import { useState } from "react";
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  useSensor,
  useSensors,
  closestCenter,
  type DragEndEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { useProjectStore } from "../store/projectStore";
import { useSessionStore } from "../store/sessionStore";
import { useUiStore } from "../store/uiStore";
import type { Project } from "../lib/types";

export default function ProjectRail() {
  const projects = useProjectStore((s) => s.projects);
  const activeProjectPath = useProjectStore((s) => s.activeProjectPath);
  const setActiveProjectPath = useProjectStore((s) => s.setActiveProjectPath);
  const reorderProjects = useProjectStore((s) => s.reorderProjects);
  const sessions = useSessionStore((s) => s.sessions);
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const switchSession = useSessionStore((s) => s.switchSession);
  const sidebarCollapsed = useUiStore((s) => s.sidebarCollapsed);
  const toggleCollapse = useUiStore((s) => s.toggleCollapse);
  const openSettings = useUiStore((s) => s.openSettings);

  const [draggingPath, setDraggingPath] = useState<string | null>(null);

  // 5px distance avoids hijacking clicks. Click still switches project.
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } })
  );

  const onAddProject = () => {
    useUiStore.getState().openLauncher({ open: true, mode: "project" });
  };

  const handleProjectClick = (projectPath: string) => {
    const currentProject = useProjectStore.getState().activeProjectPath;
    if (currentProject && activeSessionId != null) {
      useProjectStore.getState().setLastActiveSession(currentProject, activeSessionId);
    }

    setActiveProjectPath(projectPath);

    const projectSessions = sessions
      .filter((s) => s.working_dir === projectPath)
      .sort((a, b) => a.sortOrder - b.sortOrder);
    const alreadyInProject = projectSessions.some((s) => s.id === activeSessionId);
    if (!alreadyInProject && projectSessions.length > 0) {
      const lastId = useProjectStore.getState().lastActiveSession[projectPath];
      const target = projectSessions.find((s) => s.id === lastId) ?? projectSessions[0];
      switchSession(target.id);
    }
  };

  const handleDragStart = (e: DragStartEvent) => {
    setDraggingPath(String(e.active.id));
  };

  const handleDragEnd = (e: DragEndEvent) => {
    setDraggingPath(null);
    const { active, over } = e;
    if (!over || active.id === over.id) return;
    reorderProjects(String(active.id), String(over.id));
  };

  const draggingProject = draggingPath
    ? projects.find((p) => p.path === draggingPath)
    : null;

  return (
    <div className="project-rail">
      {sidebarCollapsed && (
        <button
          className="rail-expand-btn"
          onClick={toggleCollapse}
          title="Expand sidebar"
        >
          <svg width="14" height="14" viewBox="0 0 12 12" fill="none">
            <path d="M4 2L8 6L4 10" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
          </svg>
        </button>
      )}

      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
        onDragCancel={() => setDraggingPath(null)}
      >
        <SortableContext
          items={projects.map((p) => p.path)}
          strategy={verticalListSortingStrategy}
        >
          {projects.map((project) => {
            const isActive = project.path === activeProjectPath;
            const runningCount = sessions.filter(
              (s) => s.working_dir === project.path && s.status === "running"
            ).length;

            return (
              <SortableProjectButton
                key={project.path}
                project={project}
                isActive={isActive}
                hasRunning={runningCount > 0}
                onClick={() => handleProjectClick(project.path)}
              />
            );
          })}
        </SortableContext>

        <DragOverlay>
          {draggingProject ? (
            <div className="rail-project-btn dragging-overlay">
              <span className="rail-project-letter">
                {draggingProject.name[0]?.toUpperCase() ?? "?"}
              </span>
            </div>
          ) : null}
        </DragOverlay>
      </DndContext>

      <button
        className="rail-add-btn"
        onClick={onAddProject}
        title="Add project"
      >
        <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
          <line x1="7" y1="2" x2="7" y2="12" />
          <line x1="2" y1="7" x2="12" y2="7" />
        </svg>
      </button>
      <button
        className="rail-settings-btn"
        onClick={openSettings}
        title="Settings"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/>
          <circle cx="12" cy="12" r="3"/>
        </svg>
      </button>
    </div>
  );
}

interface SortableProjectButtonProps {
  project: Project;
  isActive: boolean;
  hasRunning: boolean;
  onClick: () => void;
}

function SortableProjectButton({
  project,
  isActive,
  hasRunning,
  onClick,
}: SortableProjectButtonProps) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: project.path });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  return (
    <button
      ref={setNodeRef}
      style={style}
      className={`rail-project-btn${isActive ? " active" : ""}${isDragging ? " dragging" : ""}`}
      onClick={onClick}
      title={project.name}
      {...attributes}
      {...listeners}
    >
      <span className="rail-project-letter">
        {project.name[0]?.toUpperCase() ?? "?"}
      </span>
      {hasRunning && <span className="rail-badge" />}
    </button>
  );
}
