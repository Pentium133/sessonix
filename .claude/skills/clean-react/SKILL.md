---
name: clean-react
description: |
  Use when writing or reviewing React components and hooks in src/. Enforces
  React 19 Rules of React, Rules of Hooks, effect discipline, memoization
  strategy, and Sessonix-specific Zustand patterns.
  Triggers: "react", "component", "hook", "useEffect", "refactor frontend",
  editing *.tsx files, adding new component in src/components/ or hook in src/hooks/.
allowed-tools:
  - Read
  - Edit
  - Grep
  - Bash
---

# Clean React Guidelines (Sessonix)

Source: [React Docs](https://react.dev/reference/rules). React 19 + Zustand 5.

## The 3 Rules of React (non-negotiable)

1. **Components and Hooks must be pure.** Same inputs → same output. No mutation of props, state, or module-level vars during render.
2. **React calls components.** Never call a component function like a regular function. Never call hooks from regular functions.
3. **Rules of Hooks:** call at top level only. Never inside conditions, loops, early returns, event handlers, `useMemo`/`useEffect` bodies, or class components.

```jsx
// ❌ Bad — conditional hook
function Bad({ cond }) {
  if (cond) {
    const theme = useContext(ThemeContext);
  }
}

// ✅ Good — hook at top, use value conditionally
function Good({ cond }) {
  const theme = useContext(ThemeContext);
  if (cond) { /* use theme */ }
}
```

## useEffect discipline

Rule of thumb: **if you can compute it during render or in an event handler, don't use `useEffect`.**

Valid uses:
- Subscribing to external systems (Tauri events, WebSocket, `window` events)
- Setting up/tearing down non-React resources
- Running imperative code that must happen after commit (focus, scroll)

Invalid uses:
- Transforming state → derived value. Use a variable in render or `useMemo`.
- Handling events. Put logic in the event handler.
- "Synchronizing" two pieces of state. Lift state up or derive.

### Required patterns

**Always clean up subscriptions:**
```tsx
useEffect(() => {
  const unlisten = await listen('pty-output', handler);
  return () => unlisten(); // cleanup
}, []);
```

**Guard against stale responses in fetches:**
```tsx
useEffect(() => {
  let cancelled = false;
  async function load() {
    const data = await invoke('get_sessions');
    if (!cancelled) setSessions(data);
  }
  load();
  return () => { cancelled = true; };
}, [projectPath]);
```

**Include every reactive value in deps.** If lint complains, fix the effect, don't suppress the warning.

## Keys in lists

- Stable, unique, from data. Never use array index unless list never reorders/filters.
- In Sessonix: use PTY `id` for sessions, project `path` for projects.

```tsx
{sessions.map(s => <SessionItem key={s.id} session={s} />)}
```

## Memoization — when to use it

Default: **don't memoize.** Reach for `useMemo`/`useCallback`/`memo` only when:

1. Profiler shows measurable re-render cost, AND
2. Child is wrapped in `memo()` and breaks on unstable refs, OR
3. Value is an expensive computation (filter/sort of large list), OR
4. Value is a dep of another hook and must be stable.

Premature memoization adds noise and bugs (stale deps).

```tsx
// ✅ justified — filtering 1000+ items, recomputed only when inputs change
const filtered = useMemo(
  () => sessions.filter(s => s.projectPath === activeProjectPath),
  [sessions, activeProjectPath],
);
```

## State — lift, derive, or store

Decision tree:
1. Used by one component → `useState`.
2. Used by siblings → lift to common parent.
3. Used across unrelated trees → Zustand store in `src/store/`.
4. Derived from other state → compute in render (no state needed).

Sessonix Zustand pattern (see `sessionStore.ts`):

```ts
import { create } from 'zustand';

interface SessionStore {
  sessions: Session[];
  addSession: (s: Session, replaceId?: number) => void;
}

export const useSessionStore = create<SessionStore>()((set, get) => ({
  sessions: [],
  addSession: (s, replaceId) => set(state => ({
    sessions: replaceId
      ? state.sessions.map(x => x.id === replaceId ? s : x)
      : [...state.sessions, s],
  })),
}));
```

Use with selector to avoid over-subscription:
```tsx
const sessions = useSessionStore(s => s.sessions);
const addSession = useSessionStore(s => s.addSession);
```

## Composition over configuration

Prefer children / render props over boolean flag props.

```tsx
// ❌ Flag explosion
<Modal showClose showTitle title="Settings" closable />

// ✅ Composition
<Modal>
  <Modal.Header>Settings</Modal.Header>
  <Modal.Body>...</Modal.Body>
</Modal>
```

## Component file conventions (Sessonix)

- One component per file, PascalCase filename matching export.
- Hooks in `src/hooks/`, prefixed `use*`.
- Pure helpers in `src/lib/` (no React imports).
- Stores in `src/store/`, suffixed `Store.ts`.
- Tests in `src/__tests__/` using Vitest + `@testing-library/react`.

## Event handlers vs effects

Rule: **user-initiated → handler. External-sync → effect.**

```tsx
// ❌ Sending analytics in effect
useEffect(() => { analytics.track('clicked'); }, [clickCount]);

// ✅ In the handler
function handleClick() {
  setClickCount(c => c + 1);
  analytics.track('clicked');
}
```

## TypeScript

- `interface` for object shapes, `type` for unions/intersections.
- No `any`. Use `unknown` + narrowing.
- Prop types co-located with component, exported only if reused.
- `ReactNode` for children, `ReactElement` only when required.

## Checklist before commit

- [ ] `npm run typecheck` passes
- [ ] `npm run test` passes (Vitest)
- [ ] No `useEffect` that could be computed in render
- [ ] Every effect has cleanup if it subscribes
- [ ] Every effect dep array is complete (no eslint-disable)
- [ ] Keys are stable and unique
- [ ] No memoization without a measured reason
- [ ] Zustand selectors are used (not full store destructure)
- [ ] Event handlers handle events, effects sync external systems

## Files in this project

- `src/components/*.tsx` — 17 components (PascalCase)
- `src/hooks/*.ts` — `usePtyOutput`, `useSessionActions`, `useStatusPolling`
- `src/store/*Store.ts` — 6 Zustand stores
- `src/lib/*.ts` — pure helpers (api, git, slugify, themes, updater)
