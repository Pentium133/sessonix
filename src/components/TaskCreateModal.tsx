import { useEffect, useMemo, useState } from "react";
import { useTaskStore } from "../store/taskStore";
import { showToast } from "./Toast";
import { slugify } from "../lib/slugify";
import { listBranches, type BranchListItem } from "../lib/api";

interface TaskCreateModalProps {
  projectPath: string;
  onClose: () => void;
}

/** Sentinel value for the "create a brand new branch" dropdown option. */
const CREATE_NEW = "__create_new__";

function branchFromName(name: string): string {
  const slug = slugify(name);
  // If transliteration wiped everything out (e.g. pure CJK/emoji input),
  // fall back to a stable prefix. git_manager will dedup with -2/-3 suffixes.
  return `feat/${slug || "task"}`;
}

/** Short human-readable state for a branch entry in the dropdown. */
function branchState(b: BranchListItem): "new" | "attach" | "task" | "main" {
  if (b.is_main) return "main";
  if (b.task_id != null) return "task";
  if (b.worktree_path) return "attach";
  return "new";
}

function stateLabel(state: ReturnType<typeof branchState>): string {
  switch (state) {
    case "new":
      return "creates worktree";
    case "attach":
      return "attaches existing worktree";
    case "task":
      return "task already exists";
    case "main":
      return "main checkout (not selectable)";
  }
}

export default function TaskCreateModal({ projectPath, onClose }: TaskCreateModalProps) {
  const [name, setName] = useState("");
  const [branchName, setBranchName] = useState("");
  const [branchDirty, setBranchDirty] = useState(false);
  const [creating, setCreating] = useState(false);

  const [branches, setBranches] = useState<BranchListItem[]>([]);
  // Initial render shows an enabled select with just the "(create new)"
  // sentinel — the useEffect below flips this to `true` before the request
  // and back to `false` on resolution. Keeping the initial value `false`
  // avoids a momentary disabled select on mount.
  const [branchesLoading, setBranchesLoading] = useState(false);
  const [selectedSource, setSelectedSource] = useState<string>(CREATE_NEW);

  useEffect(() => {
    let cancelled = false;
    setBranchesLoading(true);
    listBranches(projectPath)
      .then((result) => {
        if (!cancelled) {
          setBranches(result);
          setBranchesLoading(false);
        }
      })
      .catch((e) => {
        if (!cancelled) {
          setBranchesLoading(false);
          // Non-fatal: user can still use the "create new" flow.
          console.warn("[TaskCreateModal] listBranches failed:", e);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [projectPath]);

  const selectedBranch = useMemo(
    () =>
      selectedSource === CREATE_NEW
        ? null
        : branches.find((b) => b.name === selectedSource) ?? null,
    [branches, selectedSource]
  );

  const selectedState = selectedBranch ? branchState(selectedBranch) : "new";
  const trimmedName = name.trim();
  const autoBranch = trimmedName ? branchFromName(trimmedName) : "";

  const canCreate =
    !creating &&
    trimmedName.length > 0 &&
    selectedState !== "task" &&
    selectedState !== "main";

  function handleNameChange(v: string) {
    setName(v);
    // Only keep the branch input auto-synced in "create new" mode.
    if (selectedSource === CREATE_NEW && !branchDirty) {
      const trimmed = v.trim();
      setBranchName(trimmed ? branchFromName(trimmed) : "");
    }
  }

  function handleBranchChange(v: string) {
    setBranchName(v);
    setBranchDirty(v.trim().length > 0);
  }

  function handleSourceChange(v: string) {
    setSelectedSource(v);
    // Suggest a task name based on the picked branch, but only while the user
    // hasn't typed their own.
    if (v !== CREATE_NEW && trimmedName.length === 0) {
      setName(v);
    }
  }

  async function handleCreate() {
    if (!canCreate) return;
    setCreating(true);
    try {
      if (selectedSource === CREATE_NEW) {
        const branch = branchName.trim() || autoBranch;
        await useTaskStore.getState().create(projectPath, trimmedName, branch);
      } else {
        await useTaskStore
          .getState()
          .create(projectPath, trimmedName, "", selectedSource);
      }
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

  // Display order, likely picks first: new → attach → task-taken → main.
  // `main` sorts last (and is also rendered disabled) since it can never be
  // selected for a worktree.
  const sortedBranches = useMemo(() => {
    const order = { new: 0, attach: 1, task: 2, main: 3 } as const;
    return [...branches].sort((a, b) => {
      const delta = order[branchState(a)] - order[branchState(b)];
      return delta !== 0 ? delta : a.name.localeCompare(b.name);
    });
  }, [branches]);

  return (
    <div className="launcher-overlay" onClick={onClose}>
      <div className="task-create-modal" onClick={(e) => e.stopPropagation()}>
        <div className="launcher-title">New Task</div>
        <div className="launcher-hint">
          Creates a git worktree. Sessions launched inside run in the task's working copy.
        </div>

        <select
          className="launcher-select"
          value={selectedSource}
          onChange={(e) => handleSourceChange(e.target.value)}
          disabled={branchesLoading}
          data-testid="task-source-select"
        >
          <option value={CREATE_NEW}>(create new branch)</option>
          {sortedBranches.map((b) => {
            const st = branchState(b);
            return (
              <option
                key={b.name}
                value={b.name}
                disabled={st === "main"}
              >
                {b.name} — {stateLabel(st)}
              </option>
            );
          })}
        </select>

        <input
          className="launcher-input"
          placeholder="Task name (e.g. fix auth flow)"
          value={name}
          onChange={(e) => handleNameChange(e.target.value)}
          onKeyDown={handleKeyDown}
          autoFocus
        />

        {selectedSource === CREATE_NEW && (
          <input
            className="launcher-input"
            placeholder="feat/my-task"
            value={branchName}
            onChange={(e) => handleBranchChange(e.target.value)}
            onKeyDown={handleKeyDown}
            style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}
          />
        )}

        {selectedBranch && (
          <div
            className="launcher-hint"
            data-testid="source-branch-hint"
            style={{ marginTop: -4 }}
          >
            {selectedState === "attach" && selectedBranch.worktree_path && (
              <>Will attach to existing worktree at <code>{selectedBranch.worktree_path}</code>.</>
            )}
            {selectedState === "new" && (
              <>Will create a new worktree on top of <code>{selectedBranch.name}</code>.</>
            )}
            {selectedState === "task" && (
              <>A task already uses this branch — pick another source.</>
            )}
          </div>
        )}

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
