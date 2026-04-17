import { useState, useEffect, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { AgentType } from "../lib/types";
import { useSettingsStore } from "../store/settingsStore";
import { useTaskStore } from "../store/taskStore";
import { createWorktree } from "../lib/git";
import { slugify } from "../lib/slugify";
import { showToast } from "./Toast";
import AgentIcon from "./AgentIcon";
import WorktreeIcon from "./WorktreeIcon";
import AgentSessionOptions from "./AgentSessionOptions";
import type { LauncherPrefill } from "../store/uiStore";
import { useEscapeKey } from "../hooks/useEscapeKey";
import { useGitStatus } from "../hooks/useGitStatus";
import {
  AGENT_OPTIONS,
  AGENT_PRESETS,
  buildAgentArgs,
  parseExtraArgs,
  type SessionMode,
} from "../lib/agentLauncher";

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

const CLAUDE_PRESET = AGENT_PRESETS.find((a) => a.type === "claude")!;

export default function SessionLauncher(props: SessionLauncherProps) {
  const { isOpen, onClose } = props;

  const [selectedAgent, setSelectedAgent] = useState(CLAUDE_PRESET);
  const [taskName, setTaskName] = useState("");
  const [customCommand, setCustomCommand] = useState("");
  // Single session-mode field shared across agents. Reset on agent switch; each
  // agent only reacts to modes it supports (see AGENT_OPTIONS).
  const [sessionMode, setSessionMode] = useState<SessionMode>("new");
  const [skipPermissions, setSkipPermissions] = useState(false);
  const [resumeSessionId, setResumeSessionId] = useState("");
  const [prompt, setPrompt] = useState("");
  const [extraArgs, setExtraArgs] = useState("");
  const [useWorktree, setUseWorktree] = useState(false);
  const [worktreeBranch, setWorktreeBranch] = useState("");
  const [worktreeCreating, setWorktreeCreating] = useState(false);
  const taskNameRef = useRef<HTMLInputElement>(null);
  const folderPickerTriggered = useRef(false);
  const skipPermsAutoSet = useRef(false);
  const launchingRef = useRef(false);

  const prefill = props.mode === "session" ? props.prefill : undefined;
  const taskId = props.mode === "session" ? props.taskId : undefined;
  const tasks = useTaskStore((s) => s.tasks);
  const launchingInTask = taskId != null ? tasks.find((t) => t.id === taskId) : undefined;
  const hideWorktreeSection = launchingInTask != null;

  // Reset state when opening. Depend on prefill's primitives (not the object
  // ref) so a parent re-opening the launcher with new prefill while isOpen is
  // already true still re-seeds the form.
  const prefillAgent = prefill?.agent;
  const prefillTaskName = prefill?.taskName;
  const prefillSkipPermissions = prefill?.skipPermissions;
  const prefillPrompt = prefill?.prompt;
  useEffect(() => {
    if (isOpen) {
      const defaultType = useSettingsStore.getState().defaultAgent;
      const agentType = prefillAgent || defaultType;
      setSelectedAgent(AGENT_PRESETS.find((a) => a.type === agentType) ?? CLAUDE_PRESET);
      setTaskName(prefillTaskName ?? "");
      setCustomCommand("");
      setSessionMode("new");
      setSkipPermissions(prefillSkipPermissions ?? useSettingsStore.getState().claudeSkipPermissions);
      skipPermsAutoSet.current = false;
      setResumeSessionId("");
      setPrompt(prefillPrompt ?? "");
      setExtraArgs("");
      setUseWorktree(false);
      setWorktreeBranch("");
      setWorktreeCreating(false);
    }
  }, [isOpen, prefillAgent, prefillTaskName, prefillSkipPermissions, prefillPrompt]);

  // Detect if project is a git repo when opening in session mode
  const projectPath = props.mode === "session" ? props.projectPath : "";
  const gitStatus = useGitStatus(projectPath || null, { enabled: isOpen });
  const isGitRepo = gitStatus?.is_repo ?? false;
  const gitChangedFiles = gitStatus?.changed_files ?? 0;

  // Auto-focus task name field for session mode
  useEffect(() => {
    if (isOpen && props.mode === "session") {
      setTimeout(() => taskNameRef.current?.focus(), 50);
    }
  }, [isOpen, props.mode]);

  // Escape to close
  useEscapeKey(onClose, isOpen);

  // Project mode: open native folder picker
  const propsMode = props.mode;
  const onAddProject = props.mode === "project" ? props.onAddProject : undefined;
  useEffect(() => {
    if (!isOpen || propsMode !== "project") return;
    if (folderPickerTriggered.current) return;
    folderPickerTriggered.current = true;
    (async () => {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select project folder",
      });
      if (typeof selected === "string") onAddProject?.(selected);
      onClose();
      folderPickerTriggered.current = false;
    })();
  }, [isOpen, propsMode, onAddProject, onClose]);

  if (!isOpen) return null;
  if (props.mode === "project") return null;

  const projectName = props.projectPath.split("/").pop() ?? "";
  const agentOptionsSpec = AGENT_OPTIONS[selectedAgent.type];

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
    const args = buildAgentArgs({
      agentType: selectedAgent.type,
      mode: sessionMode,
      skipPerms: skipPermissions,
      resumeSessionId: resumeSessionId.trim(),
      prompt: trimmedPrompt,
    });
    args.push(...parseExtraArgs(extraArgs));

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
    (agentOptionsSpec?.modes.includes("resume") && sessionMode === "resume" && !resumeSessionId.trim());

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
          {AGENT_PRESETS.map((agent) => (
            <button
              key={agent.type}
              className={`launcher-pill ${selectedAgent.type === agent.type ? "active" : ""}`}
              style={
                selectedAgent.type === agent.type
                  ? ({ "--pill-accent": agent.color } as React.CSSProperties)
                  : undefined
              }
              onClick={() => {
                if (agent.type !== selectedAgent.type) {
                  setSelectedAgent(agent);
                  // Session mode and resume-ID are shared across Claude/Codex/OpenCode;
                  // reset on agent switch so a Codex thread ID doesn't leak into an
                  // OpenCode session ID field, and so non-new modes don't persist.
                  setSessionMode("new");
                  setResumeSessionId("");
                }
              }}
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

        {agentOptionsSpec && (
          <AgentSessionOptions
            spec={agentOptionsSpec}
            mode={sessionMode}
            onModeChange={(m) => { setSessionMode(m); setResumeSessionId(""); }}
            resumeSessionId={resumeSessionId}
            onResumeSessionIdChange={setResumeSessionId}
            onSubmit={handleLaunch}
          >
            {selectedAgent.type === "claude" && (
              <label className="launcher-checkbox-label">
                <input
                  type="checkbox"
                  checked={skipPermissions}
                  onChange={(e) => { setSkipPermissions(e.target.checked); skipPermsAutoSet.current = false; }}
                />
                Skip permissions
              </label>
            )}
          </AgentSessionOptions>
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
