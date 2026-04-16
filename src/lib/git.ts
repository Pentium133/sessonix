import { invoke } from "@tauri-apps/api/core";
import type { GitStatus } from "./types";

export interface WorktreeInfo {
  path: string;
  branch: string;
  base_commit: string;
}

export async function getGitStatus(workingDir: string): Promise<GitStatus> {
  return invoke<GitStatus>("get_git_status", { workingDir });
}

export async function createWorktree(workingDir: string, branchName: string): Promise<WorktreeInfo> {
  return invoke<WorktreeInfo>("create_worktree", { workingDir, branchName });
}

export async function removeWorktree(worktreePath: string): Promise<void> {
  return invoke<void>("remove_worktree", { worktreePath });
}

export async function clearWorktreePath(ptyId: number): Promise<void> {
  return invoke<void>("clear_worktree_path", { ptyId });
}
