use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_compile() {
        let _ = WindowState {
            version: SCHEMA_VERSION,
            x: 0,
            y: 0,
            width: 1280,
            height: 800,
            monitor: MonitorFingerprint { x: 0, y: 0, width: 1920, height: 1080 },
        };
    }
}
