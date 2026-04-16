---
name: clean-rust
description: |
  Use when writing or reviewing Rust code in src-tauri/. Enforces idiomatic Rust
  API guidelines: naming, error handling, conversions, common traits, ownership,
  and visibility. Applies to new functions, structs, enums, modules, and refactors.
  Triggers: "rust", "refactor rust", "clean up backend", editing *.rs files,
  adding new module in src-tauri/src/, designing public API.
allowed-tools:
  - Read
  - Edit
  - Grep
  - Bash
---

# Clean Rust Guidelines (Sessonix)

Source: [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/).
Applied to `src-tauri/` — a Tauri 2 backend with PTY, SQLite, git2.

## Naming (C-CASE)

| Item | Convention | Example |
|---|---|---|
| Types, traits, enum variants | `UpperCamelCase` | `SessionManager`, `AgentAdapter`, `PtySize` |
| Functions, methods, fields, modules, local vars | `snake_case` | `create_session`, `pty_manager` |
| Constants, statics | `SCREAMING_SNAKE_CASE` | `MAX_BUFFER_SIZE` |
| Type params | Single upper letter or `UpperCamelCase` | `T`, `Runtime` |
| Lifetimes | Short lowercase | `'a`, `'de` |
| Acronyms | Treat as one word | `PtyId` (not `PTYId`), `HttpClient` (not `HTTPClient`) |

Conversions: `as_` (cheap ref→ref), `to_` (expensive or owned), `into_` (consumes self).

## Error handling

- **Tauri commands** return `Result<T, String>` at the IPC boundary only.
- **Internally**, use a typed error enum (`thiserror` or manual).
- Error messages: **lowercase, no trailing punctuation**. Good: `"invalid path at index 3"`. Bad: `"Invalid path!"`.
- Impl `std::error::Error` with `source()` for chaining.
- Never return `()` from a fallible function — use a meaningful error type.
- **No `unwrap()` / `expect()` in production paths.** OK in tests and startup invariants documented in comments.

```rust
#[derive(Debug)]
pub enum PtyError {
    SpawnFailed(std::io::Error),
    InvalidCwd { path: PathBuf },
}

impl std::fmt::Display for PtyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed(e) => write!(f, "pty spawn failed: {e}"),
            Self::InvalidCwd { path } => write!(f, "invalid cwd: {}", path.display()),
        }
    }
}

impl std::error::Error for PtyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::SpawnFailed(e) = self { Some(e) } else { None }
    }
}
```

## Conversions

- **Impl `From`**, not `Into` (blanket impl gives you `Into` for free).
- Use `TryFrom` for fallible conversions.
- Prefer `AsRef<Path>` / `AsRef<str>` in function args for flexibility.

```rust
fn open_repo(path: impl AsRef<Path>) -> Result<Repository, git2::Error> {
    Repository::open(path.as_ref())
}
```

## Common traits (impl all that apply)

For public types, derive or impl: `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`, `Default` where sensible.
Also: `Send + Sync` for anything crossing threads (PTY reader, Tauri state).

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SessionId(pub u32);
```

## Newtype pattern (prevent ID confusion)

Sessonix has multiple numeric IDs (PTY ID, DB row ID). Wrap to prevent mix-ups:

```rust
pub struct PtyId(pub u32);
pub struct SessionRowId(pub i64);
// Compiler now prevents passing DB row where PTY ID is expected
```

## Ownership & borrowing

- Take `&str` not `&String`, `&[T]` not `&Vec<T>`, `&Path` not `&PathBuf` (or `AsRef`).
- Return owned values from constructors; borrowed from accessors.
- Prefer `Cow<str>` when sometimes-owned, sometimes-borrowed.
- Use `Arc<Mutex<T>>` for shared mutable state across threads (PTY reader, SessionManager).

## Visibility

- Default to private. Expose via `pub(crate)` for same-crate, `pub` only at module boundaries.
- Never make fields `pub` on state structs — use accessor methods or newtypes.
- `#[non_exhaustive]` on public enums/structs that may grow.

## Documentation

- `///` on every `pub` item.
- First line is a single sentence summary.
- Panics / Errors / Safety sections where relevant.
- Runnable examples for non-trivial APIs.

```rust
/// Spawns a PTY session and starts a reader thread.
///
/// # Errors
/// Returns `PtyError::InvalidCwd` if `cwd` is not an absolute existing directory.
pub fn spawn_session(&self, cmd: &str, cwd: &Path) -> Result<PtyId, PtyError> { ... }
```

## Async / blocking discipline

- Tauri `async fn` commands must wrap blocking ops (`git2`, `rusqlite`, heavy I/O)
  in `tauri::async_runtime::spawn_blocking`.
- Never `.await` inside `Mutex::lock()` scope (use `parking_lot::Mutex` for sync state).
- See `sessonix-ipc` skill for IPC-specific rules.

## Formatting & lints

```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy -- -D warnings
cd src-tauri && cargo test
```

Clippy lints to respect: `needless_collect`, `redundant_clone`, `manual_map`,
`use_self`, `match_same_arms`, `shadow_unrelated`.

## Checklist before commit

- [ ] `cargo fmt` applied
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo check` passes
- [ ] `cargo test` passes
- [ ] No `unwrap()` in production paths
- [ ] Error types impl `Error + Display + Debug`
- [ ] Public items documented with `///`
- [ ] Newtype used for distinct integer IDs
- [ ] Tauri async commands wrap blocking ops

## Files in this project

- `src-tauri/src/lib.rs` — IPC handlers
- `src-tauri/src/{session,pty,ring_buffer,db,git,hooks,jsonl}.rs` + `error.rs`, `types.rs`
- `src-tauri/src/adapters/` — `AgentAdapter` trait impls
