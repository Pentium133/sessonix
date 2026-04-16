import { useState } from "react";
import { useTaskStore } from "../store/taskStore";
import { showToast } from "./Toast";

interface TaskCreateModalProps {
  projectPath: string;
  onClose: () => void;
}

function slugify(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export default function TaskCreateModal({ projectPath, onClose }: TaskCreateModalProps) {
  const [name, setName] = useState("");
  const [branchName, setBranchName] = useState("");
  const [creating, setCreating] = useState(false);

  const trimmedName = name.trim();
  const placeholderBranch = trimmedName ? `feat/${slugify(trimmedName)}` : "feat/my-task";
  const canCreate = trimmedName.length > 0 && !creating;

  async function handleCreate() {
    if (!canCreate) return;
    const branch = branchName.trim() || `feat/${slugify(trimmedName)}`;
    setCreating(true);
    try {
      await useTaskStore.getState().create(projectPath, trimmedName, branch);
      showToast("Task created", "info");
      onClose();
    } catch (e) {
      showToast(`Failed to create task: ${e}`, "error");
      setCreating(false);
    }
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter" && canCreate) {
      e.preventDefault();
      handleCreate();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  }

  return (
    <div className="launcher-overlay" onClick={onClose}>
      <div className="task-create-modal" onClick={(e) => e.stopPropagation()}>
        <div className="launcher-title">New Task</div>
        <div className="launcher-hint">
          Creates a git worktree. Sessions launched inside run in the task's working copy.
        </div>
        <input
          className="launcher-input"
          placeholder="Task name (e.g. fix auth flow)"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={handleKeyDown}
          autoFocus
        />
        <input
          className="launcher-input"
          placeholder={`Branch name (default: ${placeholderBranch})`}
          value={branchName}
          onChange={(e) => setBranchName(e.target.value)}
          onKeyDown={handleKeyDown}
          style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}
        />
        <div className="launcher-actions">
          <button className="launcher-back" onClick={onClose} disabled={creating}>
            Cancel
          </button>
          <button
            className="launcher-launch"
            onClick={handleCreate}
            disabled={!canCreate}
          >
            {creating ? "Creating..." : "Create Task"}
          </button>
        </div>
      </div>
    </div>
  );
}
