---
name: Windows Support
description: Run Sessonix on Windows 10 (1809+) with Shell and Claude adapters, ConPTY, and Windows-aware path and env handling
targets:
  - ../src-tauri/Cargo.toml
  - ../src-tauri/src/adapters/shell.rs
  - ../src-tauri/src/adapters/mod.rs
  - ../src-tauri/src/pty_manager.rs
  - ../src-tauri/src/hooks.rs
---

# Windows Support

Make Sessonix launchable on Windows so that the **Shell** and **Claude** adapters operate end-to-end: a PTY session starts, output streams, status detection works, and the app shuts down cleanly. Other adapters (Codex, Gemini, Cursor, OpenCode) are **out of scope** and may fail gracefully.

## Scope

In scope:
- Shell adapter auto-detects a Windows shell at session create: `pwsh.exe` → `powershell.exe` → `%COMSPEC%` → literal `cmd.exe`.
- Shell prompt detector recognises `cmd.exe` (`C:\Users\Foo>`) and PowerShell (`PS C:\Users\Foo>`) idle prompts, without false positives on `>` appearing in ordinary command output.
- Claude adapter runs unchanged; the binary is located via `PATH` (which searches `.exe`/`PATHEXT` automatically).
- `working_dir` validation works for Windows paths (drive-letter absolute, mixed slashes) and avoids the `\\?\` UNC prefix that some child processes reject.
- PTY environment whitelist (`SAFE_ENV_VARS`) includes the Windows variables that console apps require (`USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, …).
- Claude-hooks installation is a no-op on Windows — `install_hooks()` returns `Ok(())` without writing files or touching `~/.claude/settings.json`.
- `cargo build` / `tauri build` succeed on `x86_64-pc-windows-msvc`; `"bundle.targets": "all"` produces NSIS + MSI artifacts (existing `icons/icon.ico` already covers Windows icons).

Out of scope:
- Codex / Gemini / Cursor / OpenCode adapter parity on Windows (may launch but status-parsing, session resume, and CLI-specific paths are not guaranteed).
- PowerShell / `.cmd` version of `hook-handler.sh`.
- User-configurable shell override (env var or UI setting).
- `aarch64-pc-windows-msvc` target — x86_64 only.
- Minimum-OS enforcement at runtime (ConPTY availability is assumed — Windows 10 1809+).
- Windows-specific CI runners; verification is local by the user.

## Platform detection strategy

All Windows-specific code paths live behind `#[cfg(windows)]`. Existing Unix code paths stay behind `#[cfg(unix)]` where they already are, or remain unconditional where behaviour is identical. No runtime `cfg!(target_os = "windows")` branching in hot paths — compile-time only.

## Shell adapter

### Shell resolution (`adapters/shell.rs`)

Extract a `resolve_shell()` helper that produces the command to spawn. The existing `build_command` delegates to it.

```rust
#[cfg(unix)]
fn resolve_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
}

#[cfg(windows)]
fn resolve_shell() -> String {
    if which::which("pwsh").is_ok() {
        return "pwsh.exe".to_string();
    }
    if which::which("powershell").is_ok() {
        return "powershell.exe".to_string();
    }
    std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
}
```

- On Unix, behaviour is unchanged: `$SHELL` is honoured, `/bin/bash` is the fallback.
  `[@test] ../src-tauri/src/adapters/shell.rs` (existing `mod tests`, `#[cfg(unix)]`)
- On Windows, resolution walks the priority list; the first resolvable name wins.
  `[@test] ../src-tauri/src/adapters/shell.rs` (`#[cfg(windows)] mod windows_tests::test_resolves_pwsh_when_available`)
- If `pwsh.exe` is not in `PATH` but `powershell.exe` is, the adapter picks `powershell.exe`.
  `[@test] ../src-tauri/src/adapters/shell.rs::windows_tests::test_falls_back_to_powershell`
- If neither is in `PATH`, `%COMSPEC%` is used; if `%COMSPEC%` is unset, the literal `"cmd.exe"` is returned.
  `[@test] ../src-tauri/src/adapters/shell.rs::windows_tests::test_falls_back_to_comspec`

Tests that depend on `which::which` inspecting `PATH` are written by manipulating a scoped `PATH` through `std::env::set_var` inside a `serial_test` guard, or by injecting a resolver trait — whichever the implementer picks when writing tests.

### Prompt detection (`adapters/shell.rs`)

The Unix detector (prompt-char set `$`, `%`, `#`, `>`) stays as-is. A Windows detector is added and selected via `#[cfg(windows)]`.

- `cmd.exe` idle prompt: a line whose trimmed form matches `^[A-Za-z]:[\\/].*>\s?$` — e.g. `C:\Users\Foo>` or `C:\Users\Foo> `.
  `[@test] ../src-tauri/src/adapters/shell.rs::windows_tests::test_detect_cmd_prompt`
- PowerShell idle prompt: a line whose trimmed form matches `^PS [A-Za-z]:[\\/].*>\s?$` — e.g. `PS C:\Users\Foo>`.
  `[@test] ../src-tauri/src/adapters/shell.rs::windows_tests::test_detect_powershell_prompt`
- A bare `>` at the end of a line is **not** treated as an idle prompt on Windows (avoids false positives on `echo foo > file.txt` output, redirect noise, PS continuation `>>`, etc.).
  `[@test] ../src-tauri/src/adapters/shell.rs::windows_tests::test_lone_gt_is_running`
- Running state: a non-empty last line that does not match either regex → `SessionStatus::Running` with the trimmed line (truncated to 80 chars) as `status_line`. Matches existing Unix behaviour.
  `[@test] ../src-tauri/src/adapters/shell.rs::windows_tests::test_running_when_output_is_not_prompt`
- ANSI-wrapped Windows prompts are normalised via the existing `strip_ansi` helper before matching. PowerShell coloured prompts still detect as idle.
  `[@test] ../src-tauri/src/adapters/shell.rs::windows_tests::test_ansi_wrapped_powershell_prompt`

Regexes are compiled once with `once_cell::sync::Lazy<Regex>` (the `regex` crate is already a transitive dep; declare it explicitly in `Cargo.toml` if not direct).

## PTY manager (`pty_manager.rs`)

### `working_dir` canonicalisation

`std::fs::canonicalize` on Windows returns UNC paths prefixed with `\\?\`, which `CommandBuilder::cwd` forwards to `CreateProcessW` — some child processes (notably older tooling) refuse UNC paths or misbehave on them. Use the `dunce` crate, which strips the UNC prefix when the path has a standard drive-letter form and falls back to `std::fs::canonicalize` otherwise.

- Add `dunce = "1"` to `src-tauri/Cargo.toml`.
- Replace `wd.canonicalize()` with `dunce::canonicalize(wd)` in `PtyManager::create_session`.
- Error messages (`"Directory '{}' does not exist"`) are unchanged.

Behaviour:
- A valid Windows working dir `C:\Users\Foo\project` resolves without the `\\?\` prefix.
  `[@test] ../src-tauri/src/pty_manager.rs` (new `#[cfg(windows)]` test `test_canonicalize_strips_unc`)
- A non-existent directory returns `AppError::Pty("Directory '...' does not exist")` — same as Unix.
  `[@test] ../src-tauri/src/pty_manager.rs` (existing path-validation test extended under `#[cfg(windows)]`)
- Unix canonicalisation behaviour is unchanged (`dunce::canonicalize` is a thin passthrough on Unix).

### `SAFE_ENV_VARS` expansion

The existing whitelist is Unix-shaped. Add a Windows section gated by `#[cfg(windows)]`.

```rust
#[cfg(windows)]
const WINDOWS_SAFE_ENV_VARS: &[&str] = &[
    "USERPROFILE", "APPDATA", "LOCALAPPDATA",
    "PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMDATA",
    "SYSTEMROOT", "WINDIR", "SYSTEMDRIVE",
    "HOMEDRIVE", "HOMEPATH",
    "TEMP", "TMP",
    "PATHEXT", "COMSPEC",
    "USERNAME", "COMPUTERNAME",
];
```

- On Windows, the env filter accepts a key if it appears in **either** `SAFE_ENV_VARS` (existing cross-platform entries like `PATH`, `LANG`) **or** `WINDOWS_SAFE_ENV_VARS`. On Unix, only `SAFE_ENV_VARS` is consulted.
  `[@test] ../src-tauri/src/pty_manager.rs` (new `#[cfg(windows)]` test `test_windows_env_vars_forwarded`)
- Matching remains case-sensitive (`std::env::vars()` on Windows reports keys as upper-case, which matches the whitelist). No case-insensitive compare is added.
- `TERM=xterm-256color` is still set on Windows; ConPTY accepts it.

### PATH lookup

`which::which(command)` on Windows already honours `PATHEXT`, so `claude` resolves to `claude.cmd` / `claude.exe`. No code change required; this requirement exists to prevent regressions.

- Passing `"claude"` as a command on Windows resolves successfully when any `PATHEXT` match exists on `PATH`.
  `[@test] manual — verified via `npm run tauri dev` on Windows`

## Claude adapter

No code changes expected; the adapter builds the command from a plain binary name, and `pty_manager` handles PATH resolution and env forwarding.

- `--session-id <uuid>` and `--resume <uuid>` argv construction is byte-identical across platforms.
- JSONL path `~/.claude/projects/<dir-key>/` uses `dirs::home_dir()` which on Windows returns `%USERPROFILE%` (`C:\Users\<name>`); Claude CLI writes there, so reads succeed.
- `dir_key(working_dir)` in `jsonl.rs` replaces `/` with `-`. Windows working dirs contain `\`, not `/`, so the current key would mismatch. Fix: when running on Windows, `dir_key` replaces both `/` **and** `\` with `-`, and additionally replaces `:` (drive-letter colon) with `-` to mirror Claude CLI's own scheme on Windows.
  `[@test] ../src-tauri/src/jsonl.rs` (new `#[cfg(windows)]` test `test_dir_key_windows_path`)

> Verification of Claude CLI's actual on-Windows folder-naming scheme is an **implementation-time TODO**: if the observed scheme differs from the above, update this section before landing.

## Hooks (`hooks.rs`)

`install_hooks` currently writes a bash script to `~/.sessonix/hook-handler.sh` and patches `~/.claude/settings.json`. On Windows bash is absent by default; the hooks would never fire.

- On Windows, `install_hooks()` returns `Ok(())` immediately without creating directories, writing scripts, or modifying `~/.claude/settings.json`.
  `[@test] ../src-tauri/src/hooks.rs` (new `#[cfg(windows)]` test `test_install_hooks_is_noop_on_windows`)
- Claude status detection degrades gracefully — session state is derived from the JSONL tail (already supported in `jsonl.rs`), which does not require the hook.
- On Unix, behaviour is unchanged: scripts are written, permissions are set, settings are patched.

## Build & packaging

- `src-tauri/tauri.conf.json` already has `"bundle.targets": "all"` and `icons/icon.ico` — no change.
- `cargo check` + `cargo clippy -- -D warnings` pass on Windows.
- `cargo test` runs green on Windows; the new `#[cfg(windows)] mod windows_tests` blocks execute.
- `npm run tauri build` on Windows produces NSIS + MSI artifacts; artifact paths are not spec-locked (Tauri defaults).

## Cross-platform CI

No new CI targets added. Existing macOS CI continues to run the Unix test suite. Windows verification is manual: the user runs `npm run tauri dev` and `cargo test` locally after the branch lands.

## Non-functional requirements

- Performance: identical to Unix. No extra syscalls per PTY read/write.
- Security: env whitelist remains strict-match; API-key-shaped variables (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, etc.) are still **not** forwarded.
- Regression surface: all Windows-only code is behind `#[cfg(windows)]` — macOS and Linux builds compile and behave byte-identically to pre-change.
- The `dunce` crate (MIT/Apache-2.0) is the only new Rust dependency.

## Build sequence

1. Add `dunce = "1"` to `src-tauri/Cargo.toml`; `cd src-tauri && cargo check`.
2. Refactor `shell.rs` to extract `resolve_shell()` + platform-split prompt detector; add `#[cfg(windows)] mod windows_tests`.
3. Update `pty_manager.rs`: `dunce::canonicalize`, add `WINDOWS_SAFE_ENV_VARS`, update env-forwarding loop, add `#[cfg(windows)]` tests.
4. Update `jsonl.rs` `dir_key` for Windows-path slugging; add `#[cfg(windows)]` test.
5. Update `hooks.rs::install_hooks` with a `#[cfg(windows)]` early return; add test.
6. `cd src-tauri && cargo check` + `cargo clippy -- -D warnings` + `cargo test` — all green on macOS (Unix-gated tests run; Windows-gated tests compile only if cross-targeted).
7. User runs on Windows: `npm install` → `npm run tauri dev` → smoke-test Shell and Claude sessions → `cargo test` from `src-tauri\` → `npm run tauri build` for MSI/NSIS.
