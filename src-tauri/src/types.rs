use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Running,
    Idle,
    Error,
    Exited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub state: SessionStatus,
    pub status_line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: String,
    pub task_name: Option<String>,
    pub agent_type: Option<String>,
    pub worktree_path: Option<String>,
    pub base_commit: Option<String>,
    pub prompt: Option<String>,
    pub task_id: Option<i64>,
}
