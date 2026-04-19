use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use log::{info, warn};
use tauri::{LogicalPosition, LogicalSize, Manager, WebviewWindow};

pub const SCHEMA_VERSION: u32 = 1;
pub const FILE_NAME: &str = "window_state.json";
pub const MIN_WIDTH: u32 = 900;
pub const MIN_HEIGHT: u32 = 600;
#[allow(dead_code)]
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

#[allow(dead_code)]
pub fn write_state_atomic(path: &Path, state: &WindowState) -> io::Result<()> {
    let tmp_path = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(state)
        .map_err(io::Error::other)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&tmp_path, json)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn clamp_u32(value: u32, lo: u32, hi: u32) -> u32 {
    // If hi < lo (monitor smaller than min_size), prefer hi so the window
    // still fits on-screen. Spec: "tiny monitor" corner case in Case B.
    if hi < lo {
        return hi;
    }
    if value < lo {
        lo
    } else if value > hi {
        hi
    } else {
        value
    }
}

fn clamp_i32(value: i32, lo: i32, hi: i32) -> i32 {
    if hi < lo {
        return lo;
    }
    if value < lo {
        lo
    } else if value > hi {
        hi
    } else {
        value
    }
}

pub fn compute_target_rect(
    saved: &WindowState,
    monitors: &[MonitorRect],
    primary: Option<&MonitorRect>,
    min_size: (u32, u32),
) -> Option<Rect> {
    if monitors.is_empty() {
        return None;
    }

    let matched = monitors.iter().find(|m| {
        m.x == saved.monitor.x
            && m.y == saved.monitor.y
            && m.width == saved.monitor.width
            && m.height == saved.monitor.height
    });

    if let Some(m) = matched {
        let width = clamp_u32(saved.width, min_size.0, m.width);
        let height = clamp_u32(saved.height, min_size.1, m.height);
        let max_x = m.x + (m.width - width) as i32;
        let max_y = m.y + (m.height - height) as i32;
        let x = clamp_i32(saved.x, m.x, max_x);
        let y = clamp_i32(saved.y, m.y, max_y);
        return Some(Rect { x, y, width, height });
    }

    // Case C: matched monitor gone — centre on primary.
    let p = primary?;
    let width = clamp_u32(saved.width, min_size.0, p.width);
    let height = clamp_u32(saved.height, min_size.1, p.height);
    let x = p.x + ((p.width - width) / 2) as i32;
    let y = p.y + ((p.height - height) / 2) as i32;
    Some(Rect { x, y, width, height })
}

fn state_path(window: &WebviewWindow) -> Option<PathBuf> {
    window
        .app_handle()
        .path()
        .app_config_dir()
        .ok()
        .map(|d| d.join(FILE_NAME))
}

fn to_monitor_rect(monitor: &tauri::Monitor) -> MonitorRect {
    let sf = monitor.scale_factor();
    let pos = monitor.position();
    let size = monitor.size();
    MonitorRect {
        x: (pos.x as f64 / sf).round() as i32,
        y: (pos.y as f64 / sf).round() as i32,
        width: (size.width as f64 / sf).round() as u32,
        height: (size.height as f64 / sf).round() as u32,
    }
}

pub fn restore(window: &WebviewWindow) {
    let Some(path) = state_path(window) else {
        warn!("window_state: cannot resolve config dir, skipping restore");
        return;
    };
    let Some(saved) = read_state(&path) else {
        info!("window_state: no saved state, using defaults");
        return;
    };
    let monitors = match window.available_monitors() {
        Ok(list) => list,
        Err(e) => {
            warn!("window_state: failed to enumerate monitors: {e}");
            return;
        }
    };
    let primary = window.primary_monitor().ok().flatten();

    let monitor_rects: Vec<MonitorRect> = monitors.iter().map(to_monitor_rect).collect();
    let primary_rect = primary.as_ref().map(to_monitor_rect);

    let Some(rect) = compute_target_rect(
        &saved,
        &monitor_rects,
        primary_rect.as_ref(),
        (MIN_WIDTH, MIN_HEIGHT),
    ) else {
        warn!("window_state: no monitors available, using defaults");
        return;
    };

    info!(
        "window_state: restore saved={:?} → applied={:?}",
        (saved.x, saved.y, saved.width, saved.height),
        (rect.x, rect.y, rect.width, rect.height)
    );
    if let Err(e) = window.set_size(LogicalSize::new(rect.width, rect.height)) {
        warn!("window_state: set_size failed: {e}");
    }
    if let Err(e) = window.set_position(LogicalPosition::new(rect.x, rect.y)) {
        warn!("window_state: set_position failed: {e}");
    }
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

    fn mrect(x: i32, y: i32, w: u32, h: u32) -> MonitorRect {
        MonitorRect { x, y, width: w, height: h }
    }

    fn saved_on(m: MonitorRect, x: i32, y: i32, w: u32, h: u32) -> WindowState {
        WindowState {
            version: SCHEMA_VERSION,
            x,
            y,
            width: w,
            height: h,
            monitor: MonitorFingerprint { x: m.x, y: m.y, width: m.width, height: m.height },
        }
    }

    const MIN_SIZE: (u32, u32) = (MIN_WIDTH, MIN_HEIGHT);

    // Case A
    #[test]
    fn case_a_window_fits_entirely_unchanged() {
        let m = mrect(0, 0, 2560, 1440);
        let saved = saved_on(m, 240, 120, 1440, 900);
        let got = compute_target_rect(&saved, &[m], Some(&m), MIN_SIZE);
        assert_eq!(got, Some(Rect { x: 240, y: 120, width: 1440, height: 900 }));
    }

    // Case B: monitor matched by fingerprint but current monitor list has different size
    #[test]
    fn case_b_saved_monitor_was_bigger_window_clamped() {
        let saved_monitor_fp = MonitorFingerprint { x: 0, y: 0, width: 2560, height: 1440 };
        let saved = WindowState {
            version: SCHEMA_VERSION,
            x: 1200, y: 800, width: 1440, height: 900,
            monitor: saved_monitor_fp,
        };
        let current = mrect(0, 0, 1440, 900);
        let got = compute_target_rect(&saved, &[current], Some(&current), MIN_SIZE);
        assert_eq!(got, Some(Rect { x: 0, y: 0, width: 1440, height: 900 }));
    }

    #[test]
    fn case_b_same_monitor_window_wider_than_screen() {
        let m = mrect(0, 0, 1440, 900);
        let saved = saved_on(m, 100, 50, 2000, 800);
        let got = compute_target_rect(&saved, &[m], Some(&m), MIN_SIZE);
        assert_eq!(got, Some(Rect { x: 0, y: 50, width: 1440, height: 800 }));
    }

    #[test]
    fn case_b_window_off_to_the_right() {
        let m = mrect(0, 0, 1920, 1080);
        let saved = saved_on(m, 5000, 0, 1280, 800);
        let got = compute_target_rect(&saved, &[m], Some(&m), MIN_SIZE);
        assert_eq!(got, Some(Rect { x: 640, y: 0, width: 1280, height: 800 }));
    }

    #[test]
    fn case_b_negative_monitor_origin_clamp_works() {
        let m = mrect(-1920, 0, 1920, 1080);
        let saved = saved_on(m, -5000, 0, 1280, 800);
        let got = compute_target_rect(&saved, &[m], Some(&m), MIN_SIZE);
        assert_eq!(got, Some(Rect { x: -1920, y: 0, width: 1280, height: 800 }));
    }

    #[test]
    fn case_b_tiny_monitor_smaller_than_min_size() {
        let m = mrect(0, 0, 500, 400);
        let saved = saved_on(m, 0, 0, 1280, 800);
        let got = compute_target_rect(&saved, &[m], Some(&m), MIN_SIZE);
        assert_eq!(got, Some(Rect { x: 0, y: 0, width: 500, height: 400 }));
    }

    // Case C: saved monitor gone, centre on primary
    #[test]
    fn case_c_monitor_gone_center_on_primary() {
        let saved = WindowState {
            version: SCHEMA_VERSION,
            x: 100, y: 100, width: 1440, height: 900,
            monitor: MonitorFingerprint { x: 0, y: 0, width: 3840, height: 2160 },
        };
        let primary = mrect(0, 0, 1920, 1080);
        let got = compute_target_rect(&saved, &[primary], Some(&primary), MIN_SIZE);
        assert_eq!(got, Some(Rect { x: 240, y: 90, width: 1440, height: 900 }));
    }

    #[test]
    fn case_c_saved_size_preserved_when_fits_primary() {
        let saved = WindowState {
            version: SCHEMA_VERSION,
            x: 99999, y: 99999, width: 1280, height: 800,
            monitor: MonitorFingerprint { x: -9999, y: -9999, width: 1000, height: 1000 },
        };
        let primary = mrect(0, 0, 1920, 1080);
        let got = compute_target_rect(&saved, &[primary], Some(&primary), MIN_SIZE);
        assert_eq!(got, Some(Rect { x: 320, y: 140, width: 1280, height: 800 }));
    }

    // Case D
    #[test]
    fn case_d_empty_monitor_list_returns_none() {
        let saved = sample_state();
        let got = compute_target_rect(&saved, &[], None, MIN_SIZE);
        assert_eq!(got, None);
    }

    #[test]
    fn case_d_no_primary_and_no_match_returns_none() {
        let saved = WindowState {
            version: SCHEMA_VERSION,
            x: 0, y: 0, width: 1280, height: 800,
            monitor: MonitorFingerprint { x: 0, y: 0, width: 3840, height: 2160 },
        };
        let other = mrect(0, 0, 1920, 1080);
        let got = compute_target_rect(&saved, &[other], None, MIN_SIZE);
        assert_eq!(got, None);
    }
}
