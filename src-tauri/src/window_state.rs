use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
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

pub fn write_state_atomic(path: &Path, state: &WindowState) -> io::Result<()> {
    let tmp_path = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&tmp_path, json)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
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

    #[test]
    fn atomic_write_creates_file_and_round_trips() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("window_state.json");
        let state = sample_state();

        write_state_atomic(&path, &state).expect("write ok");

        assert!(path.exists(), "final file must exist");
        let tmp_leftover = path.with_extension("json.tmp");
        assert!(!tmp_leftover.exists(), "tmp file must be renamed away");

        let reloaded = read_state(&path).expect("reread ok");
        assert_eq!(state, reloaded);
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("window_state.json");

        let first = sample_state();
        write_state_atomic(&path, &first).unwrap();

        let mut second = first.clone();
        second.width = 2000;
        second.height = 1200;
        write_state_atomic(&path, &second).unwrap();

        assert_eq!(read_state(&path), Some(second));
    }
}
