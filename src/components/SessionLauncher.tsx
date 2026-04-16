import { useState, useEffect, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { AgentType } from "../lib/types";
import { useSettingsStore } from "../store/settingsStore";
import { useTaskStore } from "../store/taskStore";
import { getGitStatus, createWorktree } from "../lib/git";
import { slugify } from "../lib/slugify";
import { showToast } from "./Toast";
import AgentIcon from "./AgentIcon";
import WorktreeIcon from "./WorktreeIcon";
import type { LauncherPrefill } from "../store/uiStore";
type ClaudeSessionMode = "new" | "continue" | "resume";
type CodexSessionMode = "new" | "resume" | "last";
type OpenCodeSessionMode = "new" | "resume" | "last";

const AGENTS: { type: AgentType; label: string; command: string; color: string }[] = [
  { type: "shell",    label: "shell",    command: "zsh",      color: "var(--text-dim)" },
  { type: "claude",   label: "claude",   command: "claude",   color: "var(--claude)"   },
  { type: "gemini",   label: "gemini",   command: "gemini",   color: "var(--gemini)"   },
  { type: "codex",    label: "codex",    command: "codex",    color: "var(--codex)"    },
  { type: "opencode", label: "opencode", command: "opencode", color: "var(--opencode)" },
  { type: "custom",   label: "+",        command: "",         color: "var(--accent)"   },
];

function buildArgs(
  agentType: AgentType,
  claudeMode: ClaudeSessionMode,
  codexMode: CodexSessionMode,
  opencodeMode: OpenCodeSessionMode,
  skipPerms: boolean,
  resumeSessionId: string,
  prompt: string
): string[] {
  if (agentType === "claude") {
    const args: string[] = [];
    if (skipPerms) args.push("--dangerously-skip-permissions");
    if (claudeMode === "continue") args.push("--continue");
    if (claudeMode === "resume" && resumeSessionId.trim()) {
      args.push("--resume", resumeSessionId.trim());
    }
    // Positional arg = interactive session with initial prompt (not -p which exits after response)
    if (prompt && claudeMode === "new") {
      args.push(prompt);
    }
    // "new" → session_manager.rs generates --session-id <uuid>
    return args;
  }
  if (agentType === "codex") {
    const args: string[] = [];
    if (codexMode === "resume" && resumeSessionId.trim()) {
      args.push("resume", resumeSessionId.trim());
    } else if (codexMode === "last") {
      args.push("resume", "--last");
    }
    // For new Codex sessions, prompt is passed as positional arg
    if (prompt && codexMode === "new") {
      args.push(prompt);
    }
    // "new" → session_manager.rs polls Codex SQLite to capture thread ID
    return args;
  }
  if (agentType === "opencode") {
    const args: string[] = ["run", "--quiet"];
    if (opencodeMode === "resume" && resumeSessionId.trim()) {
      args.push("--session", resumeSessionId.trim());
    } else if (opencodeMode === "last") {
      args.push("--continue");
    }
    // For new OpenCode sessions, prompt is passed as positional arg
    if (prompt && opencodeMode === "new") {
      args.push(prompt);
    }
    // "new" → session_manager.rs polls OpenCode SQLite to capture session id
    return args;
  }
  if (agentType === "gemini") {
    const args: string[] = [];
    if (prompt) {
      args.push(prompt);
    }
    return args;
  }
  // shell/custom: prompt handled via backend stdin write, not args
  return [];
}

interface AddProjectProps {
  mode: "project";
  isOpen: boolean;
  onClose: () => void;
  onAddProject: (path: string) => void;
}

interface NewSessionProps {
  mode: "session";
  isOpen: boolean;
  onClose: () => void;
  projectPath: string;
  prefill?: LauncherPrefill;
  taskId?: number;
  onLaunch: (params: {
    command: string;
    args: string[];
    working_dir: string;
    task_name: string;
    agent_type: AgentType;
    worktree_path?: string;
    base_commit?: string;
    prompt?: string;
    task_id?: number;
  }) => void;
}

type SessionLauncherProps = AddProjectProps | NewSessionProps;

export default function SessionLauncher(props: SessionLauncherProps) {
  const { isOpen, onClose } = props;

  const [selectedAgent, setSelectedAgent] = useState(AGENTS[1]); // claude default
  const [taskName, setTaskName] = useState("");
  const [customCommand, setCustomCommand] = useState("");
  const [claudeSessionMode, setClaudeSessionMode] = useState<ClaudeSessionMode>("new");
  const [codexSessionMode, setCodexSessionMode] = useState<CodexSessionMode>("new");
  const [opencodeSessionMode, setOpencodeSessionMode] = useState<OpenCodeSessionMode>("new");
  const [skipPermissions, setSkipPermissions] = useState(false);
  const [resumeSessionId, setResumeSessionId] = useState("");
  const [prompt, setPrompt] = useState("");
  const [extraArgs, setExtraArgs] = useState("");
  const [useWorktree, setUseWorktree] = useState(false);
  const [worktreeBranch, setWorktreeBranch] = useState("");
  const [isGitRepo, setIsGitRepo] = useState(false);
  const [gitChangedFiles, setGitChangedFiles] = useState(0);
  const [worktreeCreating, setWorktreeCreating] = useState(false);
  const taskNameRef = useRef<HTMLInputElement>(null);
  const folderPickerTriggered = useRef(false);
  const skipPermsAutoSet = useRef(false);

  const prefill = props.mode === "session" ? props.prefill : undefined;
  const taskId = props.mode === "session" ? props.taskId : undefined;
  const tasks = useTaskStore((s) => s.tasks);
  const launchingInTask = taskId != null ? tasks.find((t) => t.id === taskId) : undefined;
  const hideWorktreeSection = launchingInTask != null;

  // Reset state when opening
  useEffect(() => {
    if (isOpen) {
      const defaultType = useSettingsStore.getState().defaultAgent;
      const agentType = prefill?.agent || defaultType;
      setSelectedAgent(AGENTS.find((a) => a.type === agentType) ?? AGENTS[1]);
      setTaskName(prefill?.taskName ?? "");
      setCustomCommand("");
      setClaudeSessionMode("new");
      setCodexSessionMode("new");
      setOpencodeSessionMode("new");
      setSkipPermissions(prefill?.skipPermissions ?? useSettingsStore.getState().claudeSkipPermissions);
      skipPermsAutoSet.current = false;
      setResumeSessionId("");
      setPrompt(prefill?.prompt ?? "");
      setExtraArgs("");
      setUseWorktree(false);
      setWorktreeBranch("");
      setIsGitRepo(false);
      setGitChangedFiles(0);
      setWorktreeCreating(false);
    }
  }, [isOpen]);

  // Detect if project is a git repo when opening in session mode
  const projectPath = props.mode === "session" ? props.projectPath : "";
  useEffect(() => {
    if (isOpen && projectPath) {
      getGitStatus(projectPath).then((s) => {
        setIsGitRepo(s.is_repo);
        setGitChangedFiles(s.changed_files);
      }).catch(() => setIsGitRepo(false));
    }
  }, [isOpen, projectPath]);

  // Auto-focus task name field for session mode
  useEffect(() => {
    if (isOpen && props.mode === "session") {
      setTimeout(() => taskNameRef.current?.focus(), 50);
    }
  }, [isOpen, props.mode]);

  // Escape to close
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape" && isOpen) onClose();
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  // Project mode: open native folder picker
  useEffect(() => {
    if (!isOpen || props.mode !== "project") return;
    if (folderPickerTriggered.current) return;
    folderPickerTriggered.current = true;
    (async () => {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select project folder",
      });
      if (typeof selected === "string") props.onAddProject(selected);
      onClose();
      folderPickerTriggered.current = false;
    })();
  }, [isOpen, props, onClose]);

  if (!isOpen) return null;
  if (props.mode === "project") return null;

  const projectName = props.projectPath.split("/").pop() ?? "";

  const launchingRef = useRef(false);
  const handleLaunch = async () => {
    if (launchingRef.current) return;
    launchingRef.current = true;

    const command =
      selectedAgent.type === "custom"
        ? customCommand.trim()
        : selectedAgent.command;
    if (!command) { launchingRef.current = false; return; }

    const name = taskName.trim() || `${selectedAgent.label} session`;
    const trimmedPrompt = prompt.trim();
    const args = buildArgs(selectedAgent.type, claudeSessionMode, codexSessionMode, opencodeSessionMode, skipPermissions, resumeSessionId, trimmedPrompt);

    // Append custom extra arguments (split by spaces, respecting quotes)
    if (extraArgs.trim()) {
      const parsed = extraArgs.trim().match(/(?:[^\s"]+|"[^"]*")+/g) ?? [];
      args.push(...parsed.map(a => a.replace(/^"|"$/g, "")));
    }

    let worktreePath: string | undefined;
    let baseCommit: string | undefined;

    if (useWorktree && isGitRepo) {
      const branch = worktreeBranch.trim() || `sessonix/${slugify(name) || "session"}`;
      setWorktreeCreating(true);
      try {
        const info = await createWorktree(props.projectPath, branch);
        worktreePath = info.path;
        baseCommit = info.base_commit;
      } catch (err) {
        setWorktreeCreating(false);
        launchingRef.current = false;
        showToast(`Worktree creation failed: ${err}`, "error");
        return;
      }
      setWorktreeCreating(false);
    }

    props.onLaunch({
      command,
      args,
      working_dir: props.projectPath,
      task_name: name,
      agent_type: selectedAgent.type,
      worktree_path: worktreePath,
      base_commit: baseCommit,
      prompt: trimmedPrompt || undefined,
      task_id: taskId,
    });
    launchingRef.current = false;
    onClose();
  };

  const isLaunchDisabled =
    (selectedAgent.type === "custom" && !customCommand.trim()) ||
    (selectedAgent.type === "claude" && claudeSessionMode === "resume" && !resumeSessionId.trim()) ||
    (selectedAgent.type === "codex" && codexSessionMode === "resume" && !resumeSessionId.trim()) ||
    (selectedAgent.type === "opencode" && opencodeSessionMode === "resume" && !resumeSessionId.trim());

  return (
    <div className="launcher-overlay" onClick={onClose}>
      <div className="launcher-modal" onClick={(e) => e.stopPropagation()}>
        <div className="launcher-title">
          New Session
          <span className="launcher-project-badge">{projectName}</span>
        </div>

        {launchingInTask && (
          <div className="launcher-task-badge">
            <WorktreeIcon className="session-wt-icon" />
            <span>In task: <strong>{launchingInTask.name}</strong></span>
            {launchingInTask.branch && (
              <span className="launcher-task-branch">{launchingInTask.branch}</span>
            )}
          </div>
        )}

        {/* Agent pills */}
        <div className="launcher-label">Agent</div>
        <div className="launcher-pills">
          {AGENTS.map((agent) => (
            <button
              key={agent.type}
              className={`launcher-pill ${selectedAgent.type === agent.type ? "active" : ""}`}
              style={
                selectedAgent.type === agent.type
                  ? ({ "--pill-accent": agent.color } as React.CSSProperties)
                  : undefined
              }
              onClick={() => setSelectedAgent(agent)}
            >
              <AgentIcon agentType={agent.type} size={16} />
              <span>{agent.label}</span>
            </button>
          ))}
        </div>

        {/* Custom command input */}
        {selectedAgent.type === "custom" && (
          <input
            className="launcher-input"
            placeholder="Command (e.g. aider, cursor)"
            value={customCommand}
            onChange={(e) => setCustomCommand(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") handleLaunch(); }}
            autoFocus
          />
        )}

        {/* Session name */}
        <input
          ref={taskNameRef}
          className="launcher-input"
          placeholder="Session name (optional)"
          value={taskName}
          onChange={(e) => setTaskName(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") handleLaunch(); }}
        />

        {/* Initial prompt */}
        {selectedAgent.type !== "custom" && (
          <textarea
            className="launcher-input launcher-prompt"
            placeholder={selectedAgent.type === "shell" ? "Initial command (optional)" : "Enter a task for the agent... (optional)"}
            value={prompt}
            onChange={(e) => {
              setPrompt(e.target.value);
              if (selectedAgent.type === "claude") {
                if (e.target.value.trim()) {
                  // Auto-check skip permissions when prompt is entered
                  if (!skipPermissions) {
                    setSkipPermissions(true);
                    skipPermsAutoSet.current = true;
                  }
                } else if (skipPermsAutoSet.current) {
                  // Auto-uncheck only if we auto-set it (user didn't toggle manually)
                  setSkipPermissions(useSettingsStore.getState().claudeSkipPermissions);
                  skipPermsAutoSet.current = false;
                }
              }
            }}
            rows={3}
          />
        )}

        {/* Claude-specific options */}
        {selectedAgent.type === "claude" && (
          <div className="launcher-claude-options">
            <div className="launcher-section-divider">Claude Options</div>
            <div className="launcher-option-row">
              <span className="launcher-option-label">Session</span>
              <div className="launcher-radio-group">
                {(["new", "continue", "resume"] as ClaudeSessionMode[]).map((mode) => (
                  <label key={mode} className="launcher-radio">
                    <input
                      type="radio"
                      name="claude-session-mode"
                      checked={claudeSessionMode === mode}
                      onChange={() => setClaudeSessionMode(mode)}
                    />
                    {mode.charAt(0).toUpperCase() + mode.slice(1)}
                  </label>
                ))}
              </div>
            </div>
            {claudeSessionMode === "resume" && (
              <input
                className="launcher-input"
                placeholder="Session ID (uuid)"
                value={resumeSessionId}
                onChange={(e) => setResumeSessionId(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") handleLaunch(); }}
                style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}
              />
            )}
            <label className="launcher-checkbox-label">
              <input
                type="checkbox"
                checked={skipPermissions}
                onChange={(e) => { setSkipPermissions(e.target.checked); skipPermsAutoSet.current = false; }}
              />
              Skip permissions
            </label>
          </div>
        )}

        {/* Codex-specific options */}
        {selectedAgent.type === "codex" && (
          <div className="launcher-claude-options">
            <div className="launcher-section-divider">Codex Options</div>
            <div className="launcher-option-row">
              <span className="launcher-option-label">Session</span>
              <div className="launcher-radio-group">
                {(["new", "last", "resume"] as CodexSessionMode[]).map((mode) => (
                  <label key={mode} className="launcher-radio">
                    <input
                      type="radio"
                      name="codex-session-mode"
                      checked={codexSessionMode === mode}
                      onChange={() => { setCodexSessionMode(mode); setResumeSessionId(""); }}
                    />
                    {mode === "last" ? "Last" : mode.charAt(0).toUpperCase() + mode.slice(1)}
                  </label>
                ))}
              </div>
            </div>
            {codexSessionMode === "resume" && (
              <input
                className="launcher-input"
                placeholder="Thread ID (uuid)"
                value={resumeSessionId}
                onChange={(e) => setResumeSessionId(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") handleLaunch(); }}
                style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}
              />
            )}
          </div>
        )}

        {/* OpenCode-specific options */}
        {selectedAgent.type === "opencode" && (
          <div className="launcher-claude-options">
            <div className="launcher-section-divider">OpenCode Options</div>
            <div className="launcher-option-row">
              <span className="launcher-option-label">Session</span>
              <div className="launcher-radio-group">
                {(["new", "last", "resume"] as OpenCodeSessionMode[]).map((mode) => (
                  <label key={mode} className="launcher-radio">
                    <input
                      type="radio"
                      name="opencode-session-mode"
                      checked={opencodeSessionMode === mode}
                      onChange={() => { setOpencodeSessionMode(mode); setResumeSessionId(""); }}
                    />
                    {mode === "last" ? "Last" : mode.charAt(0).toUpperCase() + mode.slice(1)}
                  </label>
                ))}
              </div>
            </div>
            {opencodeSessionMode === "resume" && (
              <input
                className="launcher-input"
                placeholder="Session ID (ses_xxx)"
                value={resumeSessionId}
                onChange={(e) => setResumeSessionId(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") handleLaunch(); }}
                style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}
              />
            )}
          </div>
        )}

        {/* Extra arguments */}
        <input
          className="launcher-input"
          placeholder="Extra args (e.g. --model sonnet --verbose)"
          value={extraArgs}
          onChange={(e) => setExtraArgs(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") handleLaunch(); }}
        />

        {/* Worktree isolation — hidden when launching inside an existing task */}
        {isGitRepo && !hideWorktreeSection && (
          <div className="launcher-worktree-section">
            <label className="launcher-checkbox-label">
              <input
                type="checkbox"
                checked={useWorktree}
                onChange={(e) => setUseWorktree(e.target.checked)}
              />
              <WorktreeIcon className="session-wt-icon" />
              Run in isolated worktree
            </label>
            {useWorktree && (
              <>
                <input
                  className="launcher-input"
                  placeholder={`Branch name (default: sessonix/${slugify(taskName) || "session"})`}
                  value={worktreeBranch}
                  onChange={(e) => setWorktreeBranch(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter") handleLaunch(); }}
                  style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}
                />
                {gitChangedFiles > 0 && (
                  <div className="launcher-worktree-warning">
                    Note: {gitChangedFiles} uncommitted change{gitChangedFiles !== 1 ? "s" : ""} will stay in main. Worktree uses committed state.
                  </div>
                )}
              </>
            )}
          </div>
        )}

        <div className="launcher-actions">
          <button className="launcher-back" onClick={onClose}>
            Cancel
          </button>
          <button
            className="launcher-launch"
            onClick={handleLaunch}
            disabled={isLaunchDisabled || worktreeCreating}
          >
            {worktreeCreating ? "Creating worktree..." : useWorktree ? "Launch in Worktree" : "Launch"}
          </button>
        </div>
      </div>
    </div>
  );
}
