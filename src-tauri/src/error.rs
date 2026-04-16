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
            AppError::Pty(msg) => write!(f, "pty: {msg}"),
            AppError::Db(msg) => write!(f, "db: {msg}"),
            AppError::SessionNotFound(id) => write!(f, "session {id} not found"),
            AppError::AdapterNotFound(name) => write!(f, "agent adapter '{name}' not found"),
            AppError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppError::Io(e) => Some(e),
            _ => None,
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
