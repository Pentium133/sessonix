use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;
pub const FILE_NAME: &str = "window_state.json";
pub const MIN_WIDTH: u32 = 900;
pub const MIN_HEIGHT: u32 = 600;
pub const DEBOUNCE_MS: u64 = 500;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct WindowState {
    pub version: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub monitor: MonitorFingerprint,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct MonitorFingerprint {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn read_state(path: &Path) -> Option<WindowState> {
    let data = fs::read_to_string(path).ok()?;
    let state: WindowState = serde_json::from_str(&data).ok()?;
    if state.version != SCHEMA_VERSION {
        return None;
    }
    Some(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn sample_state() -> WindowState {
        WindowState {
            version: SCHEMA_VERSION,
            x: 240,
            y: 120,
            width: 1440,
            height: 900,
            monitor: MonitorFingerprint { x: 0, y: 0, width: 2560, height: 1440 },
        }
    }

    #[test]
    fn round_trip_serialization() {
        let s = sample_state();
        let json = serde_json::to_string(&s).expect("serialize");
        let parsed: WindowState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(s, parsed);
    }

    #[test]
    fn read_state_returns_none_when_file_missing() {
        let path = Path::new("/nonexistent/does-not-exist.json");
        assert!(read_state(path).is_none());
    }

    #[test]
    fn read_state_returns_none_on_version_mismatch() {
        let tmp = NamedTempFile::new().unwrap();
        let json = r#"{"version":2,"x":0,"y":0,"width":100,"height":100,"monitor":{"x":0,"y":0,"width":100,"height":100}}"#;
        fs::write(tmp.path(), json).unwrap();
        assert!(read_state(tmp.path()).is_none());
    }

    #[test]
    fn read_state_returns_none_on_malformed_json() {
        let tmp = NamedTempFile::new().unwrap();
        fs::write(tmp.path(), "{not json").unwrap();
        assert!(read_state(tmp.path()).is_none());
    }

    #[test]
    fn read_state_returns_parsed_when_valid() {
        let tmp = NamedTempFile::new().unwrap();
        let state = sample_state();
        let json = serde_json::to_string(&state).unwrap();
        fs::write(tmp.path(), json).unwrap();
        assert_eq!(read_state(tmp.path()), Some(state));
    }
}
