import { useState } from "react";
import { useTaskStore } from "../store/taskStore";
import { showToast } from "./Toast";
import { slugify } from "../lib/slugify";

interface TaskCreateModalProps {
  projectPath: string;
  onClose: () => void;
}

function branchFromName(name: string): string {
  const slug = slugify(name);
  // If transliteration wiped everything out (e.g. pure CJK/emoji input),
  // fall back to a stable prefix. git_manager will dedup with -2/-3 suffixes.
  return `feat/${slug || "task"}`;
}

export default function TaskCreateModal({ projectPath, onClose }: TaskCreateModalProps) {
  const [name, setName] = useState("");
  const [branchName, setBranchName] = useState("");
  const [branchDirty, setBranchDirty] = useState(false);
  const [creating, setCreating] = useState(false);

  const trimmedName = name.trim();
  const autoBranch = trimmedName ? branchFromName(trimmedName) : "";
  const canCreate = trimmedName.length > 0 && !creating;

  function handleNameChange(v: string) {
    setName(v);
    // While the branch field hasn't been manually touched, keep it in sync
    // with a slugified (and transliterated) version of the task name.
    if (!branchDirty) {
      const trimmed = v.trim();
      setBranchName(trimmed ? branchFromName(trimmed) : "");
    }
  }

  function handleBranchChange(v: string) {
    setBranchName(v);
    // If the user clears the field, resume auto-fill from the name.
    setBranchDirty(v.trim().length > 0);
  }

  async function handleCreate() {
    if (!canCreate) return;
    const branch = branchName.trim() || autoBranch;
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
          onChange={(e) => handleNameChange(e.target.value)}
          onKeyDown={handleKeyDown}
          autoFocus
        />
        <input
          className="launcher-input"
          placeholder="feat/my-task"
          value={branchName}
          onChange={(e) => handleBranchChange(e.target.value)}
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
