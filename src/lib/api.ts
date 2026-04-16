import { invoke } from "@tauri-apps/api/core";
import type { AgentStatus, CreateSessionRequest } from "./types";

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

// --- Templates ---

export interface TemplateInfo {
  id: number;
  name: string;
  project_path: string;
  agent: string;
  initial_prompt: string | null;
  skip_permissions: boolean;
}

export async function createTemplate(request: {
  name: string;
  project_path: string;
  agent: string;
  initial_prompt?: string;
  skip_permissions: boolean;
}): Promise<number> {
  return invoke<number>("create_template", { request });
}

export async function listTemplates(projectPath: string): Promise<TemplateInfo[]> {
  return invoke<TemplateInfo[]>("list_templates", { projectPath });
}

export async function deleteTemplate(id: number): Promise<void> {
  return invoke<void>("delete_template", { id });
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
