use std::fmt;

#[derive(Debug)]
pub enum AppError {
    Pty(String),
    Db(String),
    SessionNotFound(u32),
    AdapterNotFound(String),
    Io(std::io::Error),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Pty(msg) => write!(f, "PTY error: {}", msg),
            AppError::Db(msg) => write!(f, "Database error: {}", msg),
            AppError::SessionNotFound(id) => write!(f, "Session {} not found", id),
            AppError::AdapterNotFound(name) => write!(f, "Agent adapter '{}' not found", name),
            AppError::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e)
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Db(e.to_string())
    }
}

impl From<AppError> for tauri::ipc::InvokeError {
    fn from(e: AppError) -> Self {
        tauri::ipc::InvokeError::from(e.to_string())
    }
}
