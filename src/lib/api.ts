import { invoke } from "@tauri-apps/api/core";
import type { AgentStatus, CreateSessionRequest, Task } from "./types";

export async function createSession(
  request: CreateSessionRequest
): Promise<number> {
  return invoke<number>("create_session", { request });
}

export async function writeToSession(
  id: number,
  data: number[]
): Promise<void> {
  return invoke<void>("write_to_session", { id, data });
}

export async function resizeSession(
  id: number,
  cols: number,
  rows: number
): Promise<void> {
  return invoke<void>("resize_session", { id, cols, rows });
}

export async function killSession(id: number): Promise<void> {
  return invoke<void>("kill_session", { id });
}

export async function detachSession(id: number): Promise<void> {
  return invoke<void>("detach_session", { id });
}

export async function attachSession(id: number): Promise<number[]> {
  return invoke<number[]>("attach_session", { id });
}

export async function getSessionCount(): Promise<number> {
  return invoke<number>("get_session_count");
}

export async function getSessionStatus(
  id: number,
  agentType?: string,
  workingDir?: string,
  agentSessionId?: string
): Promise<AgentStatus> {
  return invoke<AgentStatus>("get_session_status", {
    id,
    agentType,
    workingDir,
    agentSessionId,
  });
}

export interface SessionCostInfo {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  model: string;
  cost_usd: number;
  turns: number;
}

export async function getSessionCost(
  workingDir: string,
  agentSessionId?: string
): Promise<SessionCostInfo> {
  return invoke<SessionCostInfo>("get_session_cost", {
    workingDir,
    agentSessionId,
  });
}

export async function getAvailableAgents(): Promise<string[]> {
  return invoke<string[]>("get_available_agents");
}

export async function getDefaultShell(): Promise<string> {
  return invoke<string>("get_default_shell");
}

// --- Project persistence ---

export interface ProjectInfo {
  id: number;
  name: string;
  path: string;
}

export interface SessionInfo {
  id: number;
  pty_id: number | null;
  agent_type: string;
  task_name: string;
  working_dir: string;
  status: string;
  status_line: string;
  exit_code: number | null;
  launch_command: string;
  launch_args: string;
  started_at: string;
  agent_session_id: string | null;
  sort_order: number;
  worktree_path: string | null;
  base_commit: string | null;
  initial_prompt: string | null;
  task_id: number | null;
}

export async function addProject(
  name: string,
  path: string
): Promise<number> {
  return invoke<number>("add_project", { name, path });
}

export async function removeProject(path: string): Promise<void> {
  return invoke<void>("remove_project", { path });
}

export async function listProjects(): Promise<ProjectInfo[]> {
  return invoke<ProjectInfo[]>("list_projects");
}

export async function listSessions(
  projectPath: string
): Promise<SessionInfo[]> {
  return invoke<SessionInfo[]>("list_sessions", {
    projectPath,
  });
}

export async function deleteSession(ptyId: number): Promise<void> {
  return invoke<void>("delete_session", { ptyId });
}

export async function reorderSession(
  ptyId: number,
  newSortOrder: number
): Promise<void> {
  return invoke<void>("reorder_session", { ptyId, newSortOrder });
}

export async function reorderProject(
  path: string,
  newSortOrder: number
): Promise<void> {
  return invoke<void>("reorder_project", { path, newSortOrder });
}

export async function setSortOrder(
  ptyId: number,
  sortOrder: number
): Promise<void> {
  return invoke<void>("set_sort_order", { ptyId, sortOrder });
}

export async function notifySessionExit(ptyId: number): Promise<void> {
  return invoke<void>("notify_session_exit", { ptyId });
}

export async function saveScrollback(
  ptyId: number,
  data: string
): Promise<void> {
  return invoke<void>("save_scrollback", { ptyId, data });
}

export async function installClaudeHooks(): Promise<boolean> {
  return invoke<boolean>("install_claude_hooks");
}

export async function checkClaudeHooks(): Promise<boolean> {
  return invoke<boolean>("check_claude_hooks");
}

export async function getScrollback(
  ptyId: number
): Promise<string | null> {
  return invoke<string | null>("get_scrollback", { ptyId });
}

// --- Settings ---

export async function detectAgents(): Promise<Record<string, boolean>> {
  return invoke<Record<string, boolean>>("detect_agents");
}

export async function getSetting(key: string): Promise<string | null> {
  return invoke<string | null>("get_setting", { key });
}

export async function setSetting(key: string, value: string): Promise<void> {
  return invoke<void>("set_setting", { key, value });
}

export async function getAllSettings(): Promise<[string, string][]> {
  return invoke<[string, string][]>("get_all_settings");
}

// --- Quick prompts ---

export interface QuickPromptInfo {
  id: number;
  name: string;
  project_path: string;
  agent: string;
  initial_prompt: string | null;
  skip_permissions: boolean;
}

export async function createQuickPrompt(request: {
  name: string;
  project_path: string;
  agent: string;
  initial_prompt?: string;
  skip_permissions: boolean;
}): Promise<number> {
  return invoke<number>("create_quick_prompt", { request });
}

export async function listQuickPrompts(projectPath: string): Promise<QuickPromptInfo[]> {
  return invoke<QuickPromptInfo[]>("list_quick_prompts", { projectPath });
}

export async function deleteQuickPrompt(id: number): Promise<void> {
  return invoke<void>("delete_quick_prompt", { id });
}

export async function updateQuickPrompt(id: number, name: string, initialPrompt?: string): Promise<void> {
  return invoke<void>("update_quick_prompt", { id, name, initialPrompt });
}

// --- Tasks (worktree-scoped session groups) ---

interface TaskInfo {
  id: number;
  project_id: number;
  name: string;
  branch: string | null;
  worktree_path: string | null;
  base_commit: string | null;
  created_at: number; // Unix ms (backend converts from SQLite datetime)
}

function mapTaskInfo(info: TaskInfo): Task {
  return {
    id: info.id,
    projectId: info.project_id,
    name: info.name,
    branch: info.branch,
    worktreePath: info.worktree_path,
    baseCommit: info.base_commit,
    createdAt: info.created_at,
  };
}

export async function createTask(request: {
  project_path: string;
  name: string;
  branch_name: string;
  /** When set, attach the task to this existing branch instead of creating a new one. */
  source_branch?: string;
}): Promise<Task> {
  const info = await invoke<TaskInfo>("create_task", { request });
  return mapTaskInfo(info);
}

export async function listTasks(projectPath: string): Promise<Task[]> {
  const infos = await invoke<TaskInfo[]>("list_tasks", { projectPath });
  return infos.map(mapTaskInfo);
}

export interface BranchListItem {
  name: string;
  /** Absolute path of the worktree where this branch is currently checked out. */
  worktree_path: string | null;
  /** True when this branch is the HEAD of the project root workdir — git
   *  won't double-check-out, so it can't back a Task. */
  is_project_head: boolean;
  /** Set when an existing Task row already owns `worktree_path`. */
  task_id: number | null;
}

export async function listBranches(workingDir: string): Promise<BranchListItem[]> {
  return invoke<BranchListItem[]>("list_branches", { workingDir });
}

export interface DeleteTaskResult {
  /// Non-fatal warning: DB rows removed, but worktree cleanup failed (e.g.
  /// dirty index, permission error). Surface to user but don't treat as error.
  worktree_warning: string | null;
}

export async function deleteTask(taskId: number): Promise<DeleteTaskResult> {
  return invoke<DeleteTaskResult>("delete_task", { taskId });
}

// --- Worktree diff ---

export type DiffStatus = "added" | "modified" | "deleted" | "renamed";

export type DiffPayload =
  | { kind: "text"; oldContent: string; newContent: string }
  | { kind: "binary" }
  | { kind: "tooLarge"; sizeBytes: number };

export interface DiffFile {
  oldPath: string;
  newPath: string;
  status: DiffStatus;
  additions: number;
  deletions: number;
  payload: DiffPayload;
}

export interface WorktreeDiff {
  isRepo: boolean;
  branch: string | null;
  headSha: string | null;
  files: DiffFile[];
  truncatedFiles: number;
}

export async function getWorktreeDiff(workingDir: string): Promise<WorktreeDiff> {
  return invoke<WorktreeDiff>("get_worktree_diff", { workingDir });
}

// --- Updates ---

export interface UpdateInfo {
  version: string;
  html_url: string;
  current_version: string;
}

export async function checkForUpdate(force: boolean): Promise<UpdateInfo | null> {
  return invoke<UpdateInfo | null>("check_for_update", { force });
}
