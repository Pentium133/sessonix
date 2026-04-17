import type { AgentType } from "./types";

export type SessionMode = "new" | "continue" | "resume" | "last";

export interface AgentPreset {
  type: AgentType;
  label: string;
  command: string;
  color: string;
}

export const AGENT_PRESETS: AgentPreset[] = [
  { type: "shell",    label: "shell",    command: "zsh",      color: "var(--text-dim)" },
  { type: "claude",   label: "claude",   command: "claude",   color: "var(--claude)"   },
  { type: "gemini",   label: "gemini",   command: "gemini",   color: "var(--gemini)"   },
  { type: "codex",    label: "codex",    command: "codex",    color: "var(--codex)"    },
  { type: "opencode", label: "opencode", command: "opencode", color: "var(--opencode)" },
  { type: "custom",   label: "+",        command: "",         color: "var(--accent)"   },
];

/** Describes which session modes an agent supports and how to render the resume input. */
export interface AgentOptionsSpec {
  /** Title rendered above the radio group (e.g. "Claude Options"). */
  title: string;
  /** Radio name attribute, unique per agent so grouped controls don't collide. */
  radioName: string;
  /** Available modes in the order they render. */
  modes: SessionMode[];
  /** Visible label for each mode (defaults to capitalised mode name). */
  modeLabels?: Partial<Record<SessionMode, string>>;
  /** Placeholder shown in the resume-ID input for this agent's "resume" mode. */
  resumePlaceholder: string;
}

export const AGENT_OPTIONS: Partial<Record<AgentType, AgentOptionsSpec>> = {
  claude: {
    title: "Claude Options",
    radioName: "claude-session-mode",
    modes: ["new", "continue", "resume"],
    resumePlaceholder: "Session ID (uuid)",
  },
  codex: {
    title: "Codex Options",
    radioName: "codex-session-mode",
    modes: ["new", "last", "resume"],
    modeLabels: { last: "Last" },
    resumePlaceholder: "Thread ID (uuid)",
  },
  opencode: {
    title: "OpenCode Options",
    radioName: "opencode-session-mode",
    modes: ["new", "last", "resume"],
    modeLabels: { last: "Last" },
    resumePlaceholder: "Session ID (ses_xxx)",
  },
};

export function labelForMode(mode: SessionMode, labels?: Partial<Record<SessionMode, string>>): string {
  return labels?.[mode] ?? mode.charAt(0).toUpperCase() + mode.slice(1);
}

export interface BuildArgsOptions {
  agentType: AgentType;
  mode: SessionMode;
  skipPerms: boolean;
  resumeSessionId: string;
  prompt: string;
}

type Builder = (opts: BuildArgsOptions) => string[];

const builders: Partial<Record<AgentType, Builder>> = {
  claude: ({ mode, skipPerms, resumeSessionId, prompt }) => {
    const args: string[] = [];
    if (skipPerms) args.push("--dangerously-skip-permissions");
    if (mode === "continue") args.push("--continue");
    if (mode === "resume" && resumeSessionId) args.push("--resume", resumeSessionId);
    // Positional arg = interactive session with initial prompt (not -p which exits after response)
    if (prompt && mode === "new") args.push(prompt);
    // "new" → session_manager.rs generates --session-id <uuid>
    return args;
  },

  codex: ({ mode, resumeSessionId, prompt }) => {
    const args: string[] = [];
    if (mode === "resume" && resumeSessionId) args.push("resume", resumeSessionId);
    else if (mode === "last") args.push("resume", "--last");
    // For new Codex sessions, prompt is passed as positional arg
    if (prompt && mode === "new") args.push(prompt);
    // "new" → session_manager.rs polls Codex SQLite to capture thread ID
    return args;
  },

  opencode: ({ mode, resumeSessionId, prompt }) => {
    // Bare `opencode` launches the interactive TUI (the default command).
    // `opencode run` is batch-only — exits after one message, so unusable
    // for a persistent PTY session. Prompt goes through `--prompt` since a
    // positional arg would be interpreted as the `project` path.
    const args: string[] = [];
    if (mode === "resume" && resumeSessionId) args.push("--session", resumeSessionId);
    else if (mode === "last") args.push("--continue");
    if (prompt && mode === "new") args.push("--prompt", prompt);
    // "new" → session_manager.rs polls OpenCode SQLite to capture session id
    return args;
  },

  gemini: ({ prompt }) => (prompt ? [prompt] : []),
};

/** Build CLI args for launching an agent. shell/custom pass prompt via stdin, not args. */
export function buildAgentArgs(opts: BuildArgsOptions): string[] {
  const builder = builders[opts.agentType];
  return builder ? builder(opts) : [];
}

/** Parse extra args from a user-supplied string, respecting double quotes. */
export function parseExtraArgs(input: string): string[] {
  const trimmed = input.trim();
  if (!trimmed) return [];
  const parsed = trimmed.match(/(?:[^\s"]+|"[^"]*")+/g) ?? [];
  return parsed.map((a) => a.replace(/^"|"$/g, ""));
}
