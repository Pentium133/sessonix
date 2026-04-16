export type AgentType = "claude" | "codex" | "gemini" | "opencode" | "shell" | "custom";

export type SessionStatus = "running" | "idle" | "error" | "exited";

export interface AgentStatus {
  state: SessionStatus;
  status_line: string;
}

export interface CreateSessionRequest {
  command: string;
  args: string[];
  working_dir: string;
  task_name?: string;
  agent_type?: AgentType;
  worktree_path?: string;
  base_commit?: string;
  prompt?: string;
  task_id?: number;
}

export interface Task {
  id: number;
  projectId: number;
  name: string;
  branch: string | null;
  worktreePath: string | null;
  baseCommit: string | null;
  createdAt: number;
}

export interface Project {
  path: string;
  name: string;
  sessions: number[]; // session IDs
}

export interface Session {
  id: number;
  command: string;
  args: string[];
  working_dir: string;
  task_name: string;
  agent_type: AgentType;
  status: SessionStatus;
  status_line: string;
  created_at: number;
  dbId?: number;
  agentSessionId?: string; // Claude session ID for --resume
  sortOrder: number;
  gitStatus: GitStatus | null;
  worktree_path: string | null;
  base_commit: string | null;
  initial_prompt: string | null;
  task_id: number | null;
}

export interface GitStatus {
  is_repo: boolean;
  branch: string | null;
  changed_files: number;
  modified: number;
  added: number;
  deleted: number;
  head_sha: string | null;
  is_worktree: boolean;
}

export interface PtyOutputPayload {
  id: number;
  data: number[];
}

export interface PtyExitPayload {
  id: number;
}
