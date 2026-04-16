# TODOS

## Backend adapter refactor: unify command building

**What:** Move CLI arg construction from frontend `buildArgs()` (SessionLauncher.tsx:20) to backend `adapter.build_command()` (adapters/mod.rs:75).

**Why:** Currently args are built in two places: frontend builds session mode + skip_permissions args, backend adapters have `build_command()` that is never called in production (only tests). This dual-location logic is confusing and will get worse as more launch options are added (prompt, templates, worktrees).

**Pros:** Single source of truth for CLI arg construction. Adapters become the real authority on how to launch each agent. Frontend sends structured data (agent_type, session_mode, prompt, skip_permissions), backend builds everything.

**Cons:** Requires changing the IPC contract. Frontend stops sending `args: string[]` and starts sending structured launch params. All adapters need to handle session_mode, resume IDs, etc. Migration path for existing code.

**Context:** Discovered during /plan-eng-review on 2026-04-16. Codex cold read flagged this as "the core assumption is false — adapters don't control launch." The current plan (SPEC-010 Part A) adds prompt to frontend `buildArgs()` to follow existing patterns. This TODO is the cleanup after that ships.

**Depends on:** SPEC-010 Part A (initial prompt) must ship first using the current frontend path. This refactor is a follow-up.
