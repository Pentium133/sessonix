use crate::error::AppError;
use crate::ring_buffer::RingBuffer;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use tauri::{AppHandle, Emitter};

const RING_BUFFER_SIZE: usize = 1024 * 1024; // 1MB per session

#[allow(dead_code)]
pub struct PtySession {
    pub id: u32,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    _reader_handle: Mutex<Option<JoinHandle<()>>>,
    pub is_attached: Arc<AtomicBool>,
    pub ring_buffer: Arc<Mutex<RingBuffer>>,
    pub last_lines: Arc<Mutex<VecDeque<String>>>,
    /// PID of the spawned shell/agent process (used for foreground process detection)
    pub shell_pid: Option<u32>,
}

impl PtySession {
    pub fn write_input(&self, data: &[u8]) -> Result<(), AppError> {
        let mut writer = self.writer.lock().unwrap();
        writer
            .write_all(data)
            .map_err(|e| AppError::Pty(format!("write failed: {}", e)))?;
        writer
            .flush()
            .map_err(|e| AppError::Pty(format!("flush failed: {}", e)))?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), AppError> {
        self.master
            .lock()
            .unwrap()
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| AppError::Pty(format!("resize failed: {}", e)))?;
        Ok(())
    }

    pub fn kill(&self) -> Result<(), AppError> {
        let mut child = self.child.lock().unwrap();
        child
            .kill()
            .map_err(|e| AppError::Pty(format!("kill failed: {}", e)))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn try_wait(&self) -> Option<u32> {
        let mut child = self.child.lock().unwrap();
        child.try_wait().ok().flatten().map(|s| {
            if s.success() {
                0
            } else {
                s.exit_code()
            }
        })
    }

    /// Check if the shell/agent is idle by comparing the PTY's foreground process group
    /// with the shell's own PID. Uses kernel-level `tcgetpgrp()` via portable-pty.
    /// Returns `Some(true)` if idle, `Some(false)` if a command is running, `None` if unknown.
    #[cfg(unix)]
    pub fn is_foreground_idle(&self) -> Option<bool> {
        let shell_pid = self.shell_pid?;
        let fg_pgid = self.master.lock().unwrap().process_group_leader()?;
        // `pid_t` is i32; guard against unexpected negative values before widening to u32.
        if fg_pgid <= 0 {
            return None;
        }
        Some(fg_pgid as u32 == shell_pid)
    }

    /// Windows PTYs do not expose Unix process group APIs via portable-pty,
    /// so callers should fall back to terminal-output heuristics.
    #[cfg(not(unix))]
    pub fn is_foreground_idle(&self) -> Option<bool> {
        None
    }
}

pub struct PtyManager {
    sessions: Arc<Mutex<HashMap<u32, Arc<PtySession>>>>,
    next_id: AtomicU32,
}

impl PtyManager {
    pub fn new(start_id: u32) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU32::new(start_id),
        }
    }

    pub fn create_session(
        &self,
        command: &str,
        args: &[String],
        working_dir: &str,
        cols: u16,
        rows: u16,
        app_handle: AppHandle,
    ) -> Result<u32, AppError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| AppError::Pty(format!("openpty failed: {}", e)))?;

        // Check if command exists in PATH
        // (fix-path-env::fix() in main.rs ensures we have the full shell PATH)
        if which::which(command).is_err() {
            return Err(AppError::Pty(format!(
                "'{}' not found in PATH. Is it installed?",
                command
            )));
        }

        // Validate working directory: must be absolute, no traversal, and exist
        let wd = std::path::Path::new(working_dir);
        if !wd.is_absolute() {
            return Err(AppError::Pty(format!(
                "Working directory must be an absolute path: '{}'",
                working_dir
            )));
        }
        // Canonicalize to resolve symlinks and reject .. components
        let canonical = wd
            .canonicalize()
            .map_err(|_| AppError::Pty(format!("Directory '{}' does not exist", working_dir)))?;
        if !canonical.is_dir() {
            return Err(AppError::Pty(format!(
                "Directory '{}' does not exist",
                working_dir
            )));
        }

        let mut cmd = CommandBuilder::new(command);
        cmd.args(args);
        cmd.cwd(&canonical);

        // Whitelist safe env vars — don't leak API keys or tokens to child processes
        const SAFE_ENV_VARS: &[&str] = &[
            "PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "LC_ALL",
            "LC_CTYPE", "LC_MESSAGES", "LC_TERMINAL", "TMPDIR", "XDG_DATA_HOME",
            "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR",
            "EDITOR", "VISUAL", "PAGER", "LESS", "COLORTERM", "TERM_PROGRAM",
            // SSH agent forwarding — explicit list, not prefix match
            "SSH_AUTH_SOCK", "SSH_AGENT_PID", "SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY",
        ];
        for (key, value) in std::env::vars() {
            if SAFE_ENV_VARS.contains(&key.as_str()) {
                cmd.env(key, value);
            }
        }
        cmd.env("TERM", "xterm-256color");

        // Allocate ID atomically before spawning, so SESSONIX_PTY_ID matches the session ID
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        cmd.env("SESSONIX_PTY_ID", id.to_string());

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| AppError::Pty(format!("Failed to start '{}': {}", command, e)))?;

        let shell_pid = child.process_id();

        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| AppError::Pty(format!("clone reader failed: {}", e)))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| AppError::Pty(format!("take writer failed: {}", e)))?;
        let is_attached = Arc::new(AtomicBool::new(true));
        let ring_buffer = Arc::new(Mutex::new(RingBuffer::new(RING_BUFFER_SIZE)));
        let last_lines: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));

        // Spawn blocking reader thread
        let rb = ring_buffer.clone();
        let attached = is_attached.clone();
        let ll = last_lines.clone();
        let session_id = id;

        let reader_handle = thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut partial_line = String::new();

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = &buf[..n];

                        // Always write to ring buffer
                        rb.lock().unwrap().write(data);

                        // Update last_lines for status extraction
                        let text = String::from_utf8_lossy(data);
                        partial_line.push_str(&text);

                        // Cap partial_line to prevent unbounded growth from
                        // programs that use \r without \n (spinners, progress bars).
                        const PARTIAL_LINE_CAP: usize = 4096;
                        if partial_line.len() > PARTIAL_LINE_CAP {
                            partial_line.clear();
                        }

                        // Split on both \n and \r so carriage-return-only redraws
                        // are processed rather than accumulated.
                        if partial_line.contains('\n') || partial_line.contains('\r') {
                            let mut lines = ll.lock().unwrap();
                            for line in partial_line.split(['\n', '\r']) {
                                if !line.is_empty() {
                                    lines.push_back(line.to_string());
                                    if lines.len() > 50 {
                                        lines.pop_front();
                                    }
                                }
                            }
                            partial_line.clear();
                        }

                        // Emit to frontend if attached
                        if attached.load(Ordering::Relaxed) {
                            let payload = serde_json::json!({
                                "id": session_id,
                                "data": data.to_vec(),
                            });
                            let _ = app_handle.emit("pty-output", payload);
                        }
                    }
                    Err(_) => break,
                }
            }

            // EOF: session exited
            let _ = app_handle.emit(
                "pty-exit",
                serde_json::json!({ "id": session_id }),
            );
        });

        let session = Arc::new(PtySession {
            id,
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            child: Mutex::new(child),
            _reader_handle: Mutex::new(Some(reader_handle)),
            is_attached,
            ring_buffer,
            last_lines,
            shell_pid,
        });

        self.sessions.lock().unwrap().insert(id, session);
        Ok(id)
    }

    pub fn get_session(&self, id: u32) -> Result<Arc<PtySession>, AppError> {
        self.sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or(AppError::SessionNotFound(id))
    }

    #[allow(dead_code)]
    pub fn remove_session(&self, id: u32) -> Option<Arc<PtySession>> {
        self.sessions.lock().unwrap().remove(&id)
    }

    #[allow(dead_code)]
    pub fn session_ids(&self) -> Vec<u32> {
        self.sessions.lock().unwrap().keys().cloned().collect()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    pub fn running_count(&self) -> u32 {
        self.sessions.lock().unwrap().len() as u32
    }
}

impl Drop for PtyManager {
    fn drop(&mut self) {
        let sessions = self.sessions.lock().unwrap();
        for (_, session) in sessions.iter() {
            let _ = session.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_roundtrip() {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();

        let mut cmd = CommandBuilder::new("echo");
        cmd.arg("hello_pty");

        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut output = String::new();
        let mut buf = [0u8; 256];

        // Read with timeout-like approach
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    output.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if output.contains("hello_pty") {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        assert!(output.contains("hello_pty"));
        let _ = child.wait();
    }

    #[test]
    fn test_pty_write_read() {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();

        let cmd = CommandBuilder::new("cat");
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();

        // Write to cat
        writer.write_all(b"test_input\n").unwrap();
        writer.flush().unwrap();

        let mut output = String::new();
        let mut buf = [0u8; 256];

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    output.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if output.contains("test_input") {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        assert!(output.contains("test_input"));
        drop(writer);
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn test_pty_resize() {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();

        // Resize should not error
        let result = pair.master.resize(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        });
        assert!(result.is_ok());
    }
}
