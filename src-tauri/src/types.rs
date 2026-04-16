use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Claude,
    Codex,
    Gemini,
    Custom,
}

impl AgentType {
    pub fn as_str(&self) -> &str {
        match self {
            AgentType::Claude => "claude",
            AgentType::Codex => "codex",
            AgentType::Gemini => "gemini",
            AgentType::Custom => "custom",
        }
    }
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

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
pub struct SessionInfo {
    pub id: u32,
    pub project_name: String,
    pub agent_type: String,
    pub task_name: String,
    pub working_dir: String,
    pub status: SessionStatus,
    pub status_line: String,
    pub exit_code: Option<i32>,
    pub pid: Option<u32>,
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
