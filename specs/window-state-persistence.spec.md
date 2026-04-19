---
name: Window State Persistence
description: Persist main window position and size across launches with safe fallback when the saved monitor geometry no longer fits the current display configuration
targets:
  - ../src-tauri/src/window_state.rs
  - ../src-tauri/src/lib.rs
  - ../src-tauri/tauri.conf.json
---

# Window State Persistence

Save the Sessonix main window's position, size, and its host monitor on the fly, and restore them on the next launch. When the saved geometry can no longer be honoured (monitor disconnected, resolution shrunk), apply a soft-correction fallback that keeps the user's preferred size whenever possible and guarantees the window is always fully visible.

## Scope

In scope:
- Persist `x`, `y`, `width`, `height` plus a geometric fingerprint of the monitor the window was on.
- Restore state before the window is first shown on the next launch (no visible "jump").
- Debounced save (500 ms) on `Resized` / `Moved` plus a final blocking save on `CloseRequested`.
- Soft-correction fallback: clamp position/size to the matched monitor's bounds; if that monitor is gone, centre on the primary monitor while preserving (clamped) size.
- Atomic on-disk writes so a crash mid-save cannot corrupt the state file.

Out of scope:
- `maximized` / `fullscreen` flags — Sessonix does not currently use them.
- Monitor identification by `name()` — unreliable on macOS (often empty or generic).
- Multi-window support — only the single primary window defined in `tauri.conf.json`.
- Per-project or per-workspace window layouts.
- Migration from other window-state storage (none exists today).
- A UI surface for resetting saved state (user can delete the JSON file manually).

## Data model

### Storage location

File `window_state.json` lives in the same `app_dir` used for the SQLite database, which follows the existing dev/prod split in `lib.rs::run()`:

- Dev (`cfg!(debug_assertions)`): `<repo>/.dev-data/window_state.json` (worktree-local, isolated from production state)
- Prod: `<data_dir>/com.sessonix.app/window_state.json`
  - macOS: `~/Library/Application Support/com.sessonix.app/window_state.json`
  - Linux: `~/.local/share/com.sessonix.app/window_state.json`
  - Windows: `%APPDATA%\com.sessonix.app\window_state.json`

The module accepts `app_dir` as a parameter (same pattern as `db::Db::open`) instead of resolving the path internally, so dev/prod logic stays in one place.

### JSON schema

```json
{
  "version": 1,
  "x": 240,
  "y": 120,
  "width": 1440,
  "height": 900,
  "monitor": {
    "x": 0,
    "y": 0,
    "width": 2560,
    "height": 1440
  }
}
```

### Field semantics

- `version: u32` — schema version. Readers that encounter `version ≠ 1` treat the file as absent.
- `x, y: i32` — window's `outer_position` in the virtual desktop in **logical** pixels (top-left of the window frame). May be negative.
- `width, height: u32` — window's **inner** (content) size in logical pixels. Matches `tauri.conf.json` `width`/`height` and what `WebviewWindow::set_size()` consumes. Saving `outer_size` instead would cause the window to grow on every restart by the title-bar height.
- `monitor.x, y: i32` — host monitor's position in logical pixels.
- `monitor.width, height: u32` — host monitor's size in logical pixels (derived from `physical / scale_factor`).

### Rust types

```rust
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
struct WindowState {
    version: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    monitor: MonitorFingerprint,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
struct MonitorFingerprint {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}
```

## Module: `src-tauri/src/window_state.rs`

New module. Self-contained — no dependency on `session_manager`, `db`, or other modules.

### Public API

```rust
/// Read saved state (if any) and apply it to the window.
/// Safe to call before window.show(). Never panics; logs and returns on any error.
pub fn restore(window: &tauri::WebviewWindow, app_dir: &Path);

/// Install the on-window-event listener and start the debounced save worker.
/// Must be called once during setup. `app_dir` is moved into the worker thread.
pub fn install_auto_save(window: &tauri::WebviewWindow, app_dir: PathBuf);
```

### Internals (high level)

- `read_state(path) -> Option<WindowState>` — returns `None` if the file is missing, unparseable, or `version ≠ 1`.
- `write_state_atomic(path, &state) -> io::Result<()>` — writes to `window_state.json.tmp`, then renames to `window_state.json`.
- `compute_target_rect(saved, monitors, primary, min_size) -> Option<Rect>` — pure function containing the fallback algorithm. Tested directly.
- Debounce worker: a dedicated `std::thread` listening on `std::sync::mpsc` + a shared `parking_lot::Mutex<Option<WindowState>>` holding the latest snapshot.

## Save triggers

The window event listener handles three events:

- `WindowEvent::Resized(_)` → capture snapshot, send debounce signal.
- `WindowEvent::Moved(_)` → capture snapshot, send debounce signal.
- `WindowEvent::CloseRequested { .. }` → blocking `save_now()` before the window is destroyed.

### Debounce semantics

- Reset-on-event (not throttle). Timer resets on every new signal; the last snapshot always wins.
- Delay: **500 ms**.
- Work off the main thread — the event callback only captures the snapshot into the mutex and wakes the worker; no I/O in the hot path.

### Ignored states

Snapshots are not captured (and nothing is written) when:
- `window.is_minimized() == Ok(true)` — minimised windows report meaningless geometry on some platforms.
- `inner_size` is `0 × 0` or the position values overflow — defensive sanity check.

If `CloseRequested` fires and no valid snapshot has ever been captured, the existing file is left untouched.

## Restore algorithm

### Startup visibility

`tauri.conf.json` is updated so the main window is created **hidden** (`"visible": false`). After `restore()` applies size and position, the `setup` hook calls `window.show()`. This eliminates the visible jump when saved geometry differs from the `tauri.conf.json` defaults.

### Sequence

Executed synchronously **before** `window.show()`:

1. Read `window_state.json`. If missing, invalid, or `version ≠ 1` → return; leave `tauri.conf.json` defaults.
2. Enumerate monitors via `window.available_monitors()` and fetch `window.primary_monitor()`. If the list is empty → return.
3. Convert each `tauri::Monitor` to a logical `MonitorRect` using `physical / scale_factor`.
4. Call `compute_target_rect(saved, monitors, primary, (900, 600))`.
5. On `Some(rect)` → `window.set_size(LogicalSize { rect.w, rect.h })` then `window.set_position(LogicalPosition { rect.x, rect.y })`.
6. On `None` → do nothing; `tauri.conf.json` defaults apply.

### Fallback cases (core of this spec)

Given saved state `S` and the current list of monitors:

- **Case A — matched monitor, window fits entirely:**
  - Match: some monitor's logical rect equals `S.monitor` exactly (all four fields).
  - Fit: `S.x ≥ M.x` and `S.y ≥ M.y` and `S.x + S.w ≤ M.x + M.w` and `S.y + S.h ≤ M.y + M.h`.
  - Result: unchanged `(S.x, S.y, S.w, S.h)`.

- **Case B — matched monitor, window partially/fully outside:**
  - `width = clamp(S.w, min_w, M.w)` where `min_w = 900`.
  - `height = clamp(S.h, min_h, M.h)` where `min_h = 600`.
  - `x = clamp(S.x, M.x, M.x + M.w - width)`.
  - `y = clamp(S.y, M.y, M.y + M.h - height)`.
  - Corner case: if `min > monitor` (tiny monitor), use `width = M.w` (and `height = M.h`); the window will fill the monitor rather than hang off its edge.

- **Case C — no matched monitor:**
  - Let `P` = primary monitor's rect.
  - `width = clamp(S.w, min_w, P.w)`.
  - `height = clamp(S.h, min_h, P.h)`.
  - Centre: `x = P.x + (P.w - width) / 2`, `y = P.y + (P.h - height) / 2`.

- **Case D — no monitors available:** return `None` (defaults apply).

### Clamp definition

```rust
fn clamp<T: Ord>(value: T, lo: T, hi: T) -> T {
    if hi < lo { lo } else if value < lo { lo } else if value > hi { hi } else { value }
}
```

The `hi < lo` guard matters for the "tiny monitor" corner case (Case B), where `M.w < min_w`.

### Matching rule

Monitor match is **exact** on all four logical fields (`x`, `y`, `width`, `height`). Resolutions don't drift by a pixel — either the configuration is the same or it isn't. Introducing a tolerance would only mask bugs.

### Logging

Every restore run emits one `log::info!` line stating which case (A/B/C/D) fired, with the input and output rects. This is the primary diagnostic when users report "my window moved".

## Wiring in `src-tauri/src/lib.rs`

Inside the existing Tauri `setup`:

```rust
.setup(|app| {
    let window = app.get_webview_window("main").unwrap();
    window_state::restore(&window);
    window_state::install_auto_save(&window);
    let _ = window.show();
    // ... existing setup ...
    Ok(())
})
```

`tauri.conf.json` gains `"visible": false` on the main window entry so `show()` here is the first moment the user sees the window — with the correct geometry already applied. Exact line placement is a planning concern, not a spec concern.

## Testing

### Pure-logic tests — `#[cfg(test)] mod tests` in `window_state.rs`

`[@test] ../src-tauri/src/window_state.rs`

Required cases for `compute_target_rect`:

- Case A — saved 1440×900 @ (240, 120) inside a 2560×1440 monitor → unchanged.
- Case B — saved 1440×900 @ (1200, 800) in a 2560×1440 monitor that shrank to 1440×900 → clamped to fit.
- Case B — saved width > monitor width → `width = monitor_w`, `x = monitor_x`.
- Case B — saved state on a monitor with negative `x` (secondary left of primary) → clamp handles negatives correctly.
- Case B — tiny monitor (`500×400`) where `min_size` exceeds monitor → returns `width = M.w`, `height = M.h`.
- Case C — saved monitor fingerprint not in the current list → centred on primary, size clamped to primary.
- Case C — saved size smaller than primary → centred, size unchanged.
- Case D — empty monitor list → `None`.

### Serialization tests

`[@test] ../src-tauri/src/window_state.rs`

- Round-trip `WindowState` → JSON → `WindowState` equals input.
- File with `version = 2` parses as `None` (forward-compat guard).
- Malformed JSON parses as `None`.

### Atomic-write test

`[@test] ../src-tauri/src/window_state.rs`

- Using `tempfile::TempDir`, call `write_state_atomic` and verify (a) no `.tmp` file remains after success, (b) the final file content round-trips.

### Not covered automatically — manual verification via `npm run tauri dev`

The Tauri window API, event listener, and debounce thread are verified by hand:

1. Resize/move the window → `window_state.json` updates within ~500 ms.
2. Close via red button → file matches final geometry.
3. `kill -9` the process mid-resize → file contains the last debounced frame, not corrupted.
4. Change display resolution in System Settings to something smaller than the saved window, relaunch → window opens clamped inside the monitor (Case B).
5. Unplug external monitor the window was on, relaunch → window opens centred on primary (Case C).

## Non-functional notes

- **No new Cargo dependencies** — `serde_json`, `parking_lot`, `std::thread`, and `tauri::path` are all already in the project.
- **Startup cost** — one small synchronous file read (<1 KB) plus monitor enumeration. No measurable delay.
- **Runtime cost** — during active resize, the event handler captures a struct and wakes a thread; one JSON write per burst, size ~200 bytes.
- **Error policy** — every I/O or parsing failure is logged at `warn` level and falls through to defaults. Never panic. Never block startup.
