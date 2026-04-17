import { useCallback, useRef, useState } from "react";
import { useSessionStore } from "../store/sessionStore";

interface SessionItemDragHandlers {
  onDragStart: (e: React.DragEvent) => void;
  onDragOver: (e: React.DragEvent) => void;
  onDragLeave: (e: React.DragEvent) => void;
  onDrop: (e: React.DragEvent) => void;
  onDragEnd: () => void;
}

interface UseSessionDragAndDrop {
  /** Session id currently being dragged, or null. */
  draggingId: number | null;
  /** Session id currently being hovered over (for drop-target styling). */
  dragOverId: number | null;
  /** Ref to the currently-dragging id (stable across renders for onMouseDown etc.). */
  draggingIdRef: React.MutableRefObject<number | null>;
  /** Handlers to spread onto each draggable session row. */
  handlersFor: (session: { id: number; sortOrder: number }) => SessionItemDragHandlers;
}

/**
 * Manages drag-and-drop state for reordering sessions in the sidebar.
 *
 * React's HTML5 drag API is awkward: dragOver fires continuously, dragLeave
 * fires for each child, and drop+dragEnd order varies across browsers. We
 * commit the reorder in onDragEnd using refs captured during the drag so we
 * don't depend on dragleave cleanup timing.
 */
export function useSessionDragAndDrop(): UseSessionDragAndDrop {
  const [draggingId, setDraggingId] = useState<number | null>(null);
  const [dragOverId, setDragOverId] = useState<number | null>(null);
  const draggingIdRef = useRef<number | null>(null);
  const dragOverRef = useRef<{ id: number; sortOrder: number } | null>(null);
  const dropAcceptedRef = useRef(false);

  // Stable across renders: closes only over refs and setters. Spreading the
  // returned object onto SessionItem wouldn't break React.memo if SessionItem
  // ever opts in — the inner arrow identities stay stable per `session.id`.
  const handlersFor: UseSessionDragAndDrop["handlersFor"] = useCallback((session) => ({
    onDragStart: (e) => {
      setDraggingId(session.id);
      draggingIdRef.current = session.id;
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", String(session.id));
    },
    onDragOver: (e) => {
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      if (session.id !== draggingIdRef.current) {
        setDragOverId(session.id);
        dragOverRef.current = { id: session.id, sortOrder: session.sortOrder };
      }
    },
    onDragLeave: (e) => {
      const related = e.relatedTarget as Node | null;
      if (!e.currentTarget.contains(related)) {
        setDragOverId(null);
      }
    },
    onDrop: (e) => {
      e.preventDefault();
      dropAcceptedRef.current = true;
    },
    onDragEnd: () => {
      const dragId = draggingIdRef.current;
      const over = dragOverRef.current;
      if (dropAcceptedRef.current && dragId !== null && over && dragId !== over.id) {
        useSessionStore.getState().reorderSessionOrder(dragId, over.sortOrder);
      }
      setDraggingId(null);
      draggingIdRef.current = null;
      setDragOverId(null);
      dragOverRef.current = null;
      dropAcceptedRef.current = false;
    },
  }), []);

  return { draggingId, dragOverId, draggingIdRef, handlersFor };
}
