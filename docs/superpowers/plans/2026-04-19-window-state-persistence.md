# Window State Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist the main window's position and size across launches, with a safe fallback when the saved monitor is gone or has shrunk.

**Architecture:** All logic lives in a new self-contained module `src-tauri/src/window_state.rs`. A pure function `compute_target_rect` holds the fallback algorithm and is unit-tested. Tauri integration (`restore`, `install_auto_save`) is thin glue wired into the existing `setup` hook in `lib.rs`. State persists as atomic JSON writes to the app config dir. A background thread debounces save events at 500 ms.

**Tech Stack:** Rust (Tauri 2), `serde`/`serde_json` (already in deps), `parking_lot` (already in deps), `std::thread` + `std::sync::mpsc` for debounce, `tempfile` for tests (already in dev-deps).

**Spec:** `specs/window-state-persistence.spec.md`

**Work directory:** All commands assume CWD is the repo root (`.sessonix-worktrees/feat-autosave-size-and-position`).

---

## File structure

- **Create** `src-tauri/src/window_state.rs` — entire feature: types, JSON I/O, pure algorithm, Tauri glue, unit tests in `#[cfg(test)] mod tests`.
- **Modify** `src-tauri/src/lib.rs:1-11` — register the new module.
- **Modify** `src-tauri/src/lib.rs:1314` — add `window_state::restore(...)`, `window_state::install_auto_save(...)`, and `window.show()` just before `Ok(())`.
- **Modify** `src-tauri/tauri.conf.json:22-34` — add `"visible": false` to the main window entry.

---

## Task 1: Scaffold module with types and register in lib.rs

**Files:**
- Create: `src-tauri/src/window_state.rs`
- Modify: `src-tauri/src/lib.rs:1-11`

- [ ] **Step 1: Create `window_state.rs` with types and constants**

Write `src-tauri/src/window_state.rs`:

```rust
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
```

- [ ] **Step 2: Register the module in `lib.rs`**

Modify `src-tauri/src/lib.rs` lines 1-11. Change:

```rust
mod adapters;
mod db;
mod diff_manager;
mod error;
mod git_manager;
mod hooks;
mod jsonl;
mod pty_manager;
mod ring_buffer;
mod session_manager;
mod types;
```

to:

```rust
mod adapters;
mod db;
mod diff_manager;
mod error;
mod git_manager;
mod hooks;
mod jsonl;
mod pty_manager;
mod ring_buffer;
mod session_manager;
mod types;
mod window_state;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: compiles with no errors. Warnings about unused `window_state` items are fine at this stage.

- [ ] **Step 4: Run the placeholder test**

Run: `cd src-tauri && cargo test --lib window_state::tests::types_compile`
Expected: `test window_state::tests::types_compile ... ok`, 1 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/window_state.rs src-tauri/src/lib.rs
git commit -m "feat(window-state): scaffold module with types"
```

---

## Task 2: JSON read path (`read_state`)

**Files:**
- Modify: `src-tauri/src/window_state.rs`

- [ ] **Step 1: Write the failing tests**

At the top of the file (after existing `use serde::...`), add:

```rust
use std::fs;
use std::path::Path;
```

Replace the contents of the existing `#[cfg(test)] mod tests { ... }` block with:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib window_state::tests`
Expected: compile error — `read_state` is not defined.

- [ ] **Step 3: Implement `read_state`**

Insert this function right above the `#[cfg(test)] mod tests` block:

```rust
pub fn read_state(path: &Path) -> Option<WindowState> {
    let data = fs::read_to_string(path).ok()?;
    let state: WindowState = serde_json::from_str(&data).ok()?;
    if state.version != SCHEMA_VERSION {
        return None;
    }
    Some(state)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib window_state::tests`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/window_state.rs
git commit -m "feat(window-state): add read_state with version and parse safety"
```

---

## Task 3: Atomic write (`write_state_atomic`)

**Files:**
- Modify: `src-tauri/src/window_state.rs`

- [ ] **Step 1: Write the failing test**

Inside `mod tests { ... }`, append:

```rust
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
```

Also add `use std::io;` to the imports at the top of the file (next to the existing `use std::fs;` and `use std::path::Path;`).

- [ ] **Step 2: Run tests to verify failure**

Run: `cd src-tauri && cargo test --lib window_state::tests::atomic_write`
Expected: compile error — `write_state_atomic` is not defined.

- [ ] **Step 3: Implement `write_state_atomic`**

Add this function right below `read_state`:

```rust
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
```

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test --lib window_state::tests`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/window_state.rs
git commit -m "feat(window-state): add atomic write with tmp-file rename"
```

---

## Task 4: Pure fallback algorithm (`compute_target_rect`)

This task covers all four cases (A/B/C/D) from the spec plus their edge variants.

**Files:**
- Modify: `src-tauri/src/window_state.rs`

- [ ] **Step 1: Write all failing tests**

Inside `mod tests { ... }`, append (adjusting `monitor` field where needed to match saved state):

```rust
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
        // Saved state was on a 2560x1440 monitor at (1200, 800) with size 1440x900
        let saved_monitor_fp = MonitorFingerprint { x: 0, y: 0, width: 2560, height: 1440 };
        let saved = WindowState {
            version: SCHEMA_VERSION,
            x: 1200, y: 800, width: 1440, height: 900,
            monitor: saved_monitor_fp,
        };
        // System no longer has the 2560x1440 monitor; only a 1440x900 one exists.
        let current = mrect(0, 0, 1440, 900);
        // No fingerprint match → Case C (centred on primary, clamped).
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
        // Saved x far to the right, size fits.
        let saved = saved_on(m, 5000, 0, 1280, 800);
        let got = compute_target_rect(&saved, &[m], Some(&m), MIN_SIZE);
        // Max x is 1920 - 1280 = 640.
        assert_eq!(got, Some(Rect { x: 640, y: 0, width: 1280, height: 800 }));
    }

    #[test]
    fn case_b_negative_monitor_origin_clamp_works() {
        // Secondary monitor to the left of the primary: origin at -1920.
        let m = mrect(-1920, 0, 1920, 1080);
        let saved = saved_on(m, -5000, 0, 1280, 800);
        let got = compute_target_rect(&saved, &[m], Some(&m), MIN_SIZE);
        // Expected clamp: x ≥ -1920. Returned x should be -1920.
        assert_eq!(got, Some(Rect { x: -1920, y: 0, width: 1280, height: 800 }));
    }

    #[test]
    fn case_b_tiny_monitor_smaller_than_min_size() {
        // A 500x400 screen is smaller than the 900x600 minimum.
        let m = mrect(0, 0, 500, 400);
        let saved = saved_on(m, 0, 0, 1280, 800);
        let got = compute_target_rect(&saved, &[m], Some(&m), MIN_SIZE);
        // Spec corner case: fill the monitor rather than exceed it.
        assert_eq!(got, Some(Rect { x: 0, y: 0, width: 500, height: 400 }));
    }

    // Case C: saved monitor gone, centre on primary
    #[test]
    fn case_c_monitor_gone_center_on_primary() {
        // Saved on a monitor that no longer exists (unique fingerprint).
        let saved = WindowState {
            version: SCHEMA_VERSION,
            x: 100, y: 100, width: 1440, height: 900,
            monitor: MonitorFingerprint { x: 0, y: 0, width: 3840, height: 2160 },
        };
        let primary = mrect(0, 0, 1920, 1080);
        let got = compute_target_rect(&saved, &[primary], Some(&primary), MIN_SIZE);
        // Size clamped to primary (1440x900 fits, no change). Centred:
        // x = 0 + (1920 - 1440) / 2 = 240; y = 0 + (1080 - 900) / 2 = 90.
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
        // 1280x800 fits. Centred: x = (1920 - 1280) / 2 = 320, y = (1080 - 800) / 2 = 140.
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
        // Non-empty monitor list but no fingerprint match AND no primary → Case C cannot proceed.
        let saved = WindowState {
            version: SCHEMA_VERSION,
            x: 0, y: 0, width: 1280, height: 800,
            monitor: MonitorFingerprint { x: 0, y: 0, width: 3840, height: 2160 },
        };
        let other = mrect(0, 0, 1920, 1080);
        let got = compute_target_rect(&saved, &[other], None, MIN_SIZE);
        assert_eq!(got, None);
    }
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cd src-tauri && cargo test --lib window_state::tests`
Expected: compile error — `compute_target_rect` is not defined.

- [ ] **Step 3: Implement the clamp helpers and `compute_target_rect`**

Add these functions right below `write_state_atomic`:

```rust
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
```

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test --lib window_state::tests`
Expected: 17 passed (7 from before + 10 new compute_target_rect tests).

- [ ] **Step 5: Run clippy**

Run: `cd src-tauri && cargo clippy -- -D warnings`
Expected: no warnings. If clippy flags dead code on internal helpers, add `#[allow(dead_code)]` temporarily — it will be used in Task 5.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/window_state.rs
git commit -m "feat(window-state): add compute_target_rect fallback algorithm"
```

---

## Task 5: Tauri integration — `restore` + hide-on-start

**Files:**
- Modify: `src-tauri/src/window_state.rs`
- Modify: `src-tauri/tauri.conf.json:23-33`
- Modify: `src-tauri/src/lib.rs:1314`

- [ ] **Step 1: Make the main window hidden on startup**

Edit `src-tauri/tauri.conf.json`. Change the `windows` array entry from:

```json
{
  "title": "Sessonix",
  "width": 1280,
  "height": 800,
  "minWidth": 900,
  "minHeight": 600,
  "decorations": true,
  "resizable": true,
  "dragDropEnabled": false
}
```

to:

```json
{
  "title": "Sessonix",
  "width": 1280,
  "height": 800,
  "minWidth": 900,
  "minHeight": 600,
  "decorations": true,
  "resizable": true,
  "dragDropEnabled": false,
  "visible": false
}
```

- [ ] **Step 2: Add Tauri imports to `window_state.rs`**

Append to the existing imports at the top of `src-tauri/src/window_state.rs`:

```rust
use std::path::PathBuf;
use log::{info, warn};
use tauri::{LogicalPosition, LogicalSize, Manager, WebviewWindow};
```

- [ ] **Step 3: Add `state_path` helper and monitor conversion**

Insert these helpers at the bottom of `window_state.rs`, above the `#[cfg(test)] mod tests` block:

```rust
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
```

- [ ] **Step 4: Implement `restore`**

Append to `window_state.rs` (above the test module):

```rust
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
```

- [ ] **Step 5: Wire `restore` + `show` into `setup`**

Edit `src-tauri/src/lib.rs`. The current `setup` callback ends at line ~1314 with `Ok(())`. Insert two lines immediately before that `Ok(())`:

```rust
            if let Some(window) = app.get_webview_window("main") {
                window_state::restore(&window);
                let _ = window.show();
            }

            Ok(())
        })
```

- [ ] **Step 6: Compile and run clippy**

Run: `cd src-tauri && cargo check && cargo clippy -- -D warnings`
Expected: no errors or warnings.

- [ ] **Step 7: Re-run existing tests**

Run: `cd src-tauri && cargo test --lib window_state::tests`
Expected: 17 passed (Tauri glue is not unit-tested — no new tests here).

- [ ] **Step 8: Manual smoke test**

Run: `npm run tauri dev`

Verify:
1. App launches; window appears at default 1280×800 on the primary monitor (since no saved state exists yet).
2. Check `ls ~/Library/Application\ Support/com.sessonix.desktop/`: `window_state.json` should **not** exist yet (Task 6 wires the writer).
3. Close the window via the red traffic-light button; no panic in the terminal.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/window_state.rs src-tauri/src/lib.rs src-tauri/tauri.conf.json
git commit -m "feat(window-state): restore saved geometry on startup"
```

---

## Task 6: Auto-save worker and event listener

**Files:**
- Modify: `src-tauri/src/window_state.rs`
- Modify: `src-tauri/src/lib.rs:1314`

- [ ] **Step 1: Add threading imports**

Append to the imports at the top of `window_state.rs`:

```rust
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use parking_lot::Mutex;
use tauri::WindowEvent;
```

- [ ] **Step 2: Implement `capture_snapshot`**

Append above the test module:

```rust
fn capture_snapshot(window: &WebviewWindow) -> Option<WindowState> {
    if window.is_minimized().unwrap_or(false) {
        return None;
    }
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    if size.width == 0 || size.height == 0 {
        return None;
    }
    let monitor = window.current_monitor().ok().flatten()?;
    let sf = monitor.scale_factor();
    let monitor_pos = monitor.position();
    let monitor_size = monitor.size();

    Some(WindowState {
        version: SCHEMA_VERSION,
        x: (position.x as f64 / sf).round() as i32,
        y: (position.y as f64 / sf).round() as i32,
        width: (size.width as f64 / sf).round() as u32,
        height: (size.height as f64 / sf).round() as u32,
        monitor: MonitorFingerprint {
            x: (monitor_pos.x as f64 / sf).round() as i32,
            y: (monitor_pos.y as f64 / sf).round() as i32,
            width: (monitor_size.width as f64 / sf).round() as u32,
            height: (monitor_size.height as f64 / sf).round() as u32,
        },
    })
}
```

- [ ] **Step 3: Implement the debounce worker**

Append:

```rust
fn debounce_worker(
    rx: mpsc::Receiver<()>,
    snapshot: Arc<Mutex<Option<WindowState>>>,
    path: PathBuf,
) {
    loop {
        // Block until the first signal arrives.
        if rx.recv().is_err() {
            return;
        }
        // Reset-on-event: keep draining until DEBOUNCE_MS of silence.
        loop {
            match rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
                Ok(_) => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        let state_to_write = snapshot.lock().clone();
        if let Some(state) = state_to_write {
            if let Err(e) = write_state_atomic(&path, &state) {
                warn!("window_state: debounced write failed: {e}");
            }
        }
    }
}
```

- [ ] **Step 4: Implement `install_auto_save`**

Append:

```rust
pub fn install_auto_save(window: &WebviewWindow) {
    let Some(path) = state_path(window) else {
        warn!("window_state: cannot install auto-save without config dir");
        return;
    };

    let snapshot: Arc<Mutex<Option<WindowState>>> = Arc::new(Mutex::new(None));
    let (tx, rx) = mpsc::channel::<()>();

    let worker_snapshot = Arc::clone(&snapshot);
    let worker_path = path.clone();
    thread::spawn(move || debounce_worker(rx, worker_snapshot, worker_path));

    let listener_snapshot = Arc::clone(&snapshot);
    let listener_path = path;
    let window_for_events = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(_) | WindowEvent::Moved(_) => {
            if let Some(state) = capture_snapshot(&window_for_events) {
                *listener_snapshot.lock() = Some(state);
                let _ = tx.send(());
            }
        }
        WindowEvent::CloseRequested { .. } => {
            if let Some(state) = capture_snapshot(&window_for_events) {
                if let Err(e) = write_state_atomic(&listener_path, &state) {
                    warn!("window_state: close-time write failed: {e}");
                }
            }
        }
        _ => {}
    });
}
```

- [ ] **Step 5: Call `install_auto_save` in `setup`**

In `src-tauri/src/lib.rs`, extend the block added in Task 5. Replace:

```rust
            if let Some(window) = app.get_webview_window("main") {
                window_state::restore(&window);
                let _ = window.show();
            }
```

with:

```rust
            if let Some(window) = app.get_webview_window("main") {
                window_state::restore(&window);
                window_state::install_auto_save(&window);
                let _ = window.show();
            }
```

- [ ] **Step 6: Compile and clippy**

Run: `cd src-tauri && cargo check && cargo clippy -- -D warnings`
Expected: no errors or warnings.

- [ ] **Step 7: Re-run tests**

Run: `cd src-tauri && cargo test --lib window_state::tests`
Expected: 17 passed.

- [ ] **Step 8: Manual verification**

Run: `npm run tauri dev`

Perform and observe:
1. **Debounced save on resize**: resize the window, wait ~1 s, check the config dir:
   - macOS: `ls -la ~/Library/Application\ Support/com.sessonix.desktop/ | grep window_state`
   - `window_state.json` exists; `window_state.json.tmp` does NOT.
   - `cat` the file — JSON with `"version": 1` and the new dimensions.
2. **Debounced save on move**: drag the window, wait ~1 s, `cat` again — `x`/`y` updated.
3. **Save on close**: move the window, immediately quit (red button). Restart with `npm run tauri dev`. Window appears at the saved position and size.
4. **Case B fallback**: in System Settings, reduce the primary display's resolution below the saved window size; restart `tauri dev`; window opens clamped inside the new resolution (check logs for `case` keyword via `log::info!`).
5. **Case C fallback**: on a multi-monitor setup, move the window to an external display, quit, disconnect the external display, restart; window opens centred on the primary monitor at the saved size (or clamped).
6. **Crash tolerance**: start `tauri dev`, resize continuously, `kill -9 $(pgrep -f sessonix)`. Restart. The window reopens at the last debounced frame, not corrupted.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/window_state.rs src-tauri/src/lib.rs
git commit -m "feat(window-state): add debounced auto-save and close-time flush"
```

---

## Task 7: Full verification gate

**Files:** none.

- [ ] **Step 1: Run the full Rust test suite**

Run: `cd src-tauri && cargo test`
Expected: all existing tests + 17 new `window_state::tests` tests pass.

- [ ] **Step 2: Run clippy one final time**

Run: `cd src-tauri && cargo clippy -- -D warnings`
Expected: no warnings.

- [ ] **Step 3: Run the frontend typecheck**

Run: `npm run typecheck`
Expected: no TypeScript errors. (Frontend is untouched; this is a sanity check.)

- [ ] **Step 4: Confirm all manual verifications from Task 6 Step 8 passed**

If any step failed, stop and investigate root-cause rather than patch around.

- [ ] **Step 5: Summarize changes in the PR description**

Collect the commit range:

```bash
git log --oneline main..HEAD
```

Paste into the PR body along with a short note on Cases A/B/C/D and how to test them.
