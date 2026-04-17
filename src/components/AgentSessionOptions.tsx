import type { AgentOptionsSpec, SessionMode } from "../lib/agentLauncher";
import { labelForMode } from "../lib/agentLauncher";

interface Props {
  spec: AgentOptionsSpec;
  mode: SessionMode;
  onModeChange: (mode: SessionMode) => void;
  resumeSessionId: string;
  onResumeSessionIdChange: (value: string) => void;
  onSubmit: () => void;
  /** Optional extra controls rendered below the radio row (e.g. Claude's "Skip permissions"). */
  children?: React.ReactNode;
}

/**
 * Renders the "<Agent> Options" block — title, session-mode radio group, and
 * the resume-ID input (only when mode === "resume"). Shared by Claude, Codex
 * and OpenCode; differences come from the AgentOptionsSpec and children.
 */
export default function AgentSessionOptions({
  spec,
  mode,
  onModeChange,
  resumeSessionId,
  onResumeSessionIdChange,
  onSubmit,
  children,
}: Props) {
  return (
    <div className="launcher-claude-options">
      <div className="launcher-section-divider">{spec.title}</div>
      <div className="launcher-option-row">
        <span className="launcher-option-label">Session</span>
        <div className="launcher-radio-group">
          {spec.modes.map((m) => (
            <label key={m} className="launcher-radio">
              <input
                type="radio"
                name={spec.radioName}
                checked={mode === m}
                onChange={() => onModeChange(m)}
              />
              {labelForMode(m, spec.modeLabels)}
            </label>
          ))}
        </div>
      </div>
      {mode === "resume" && (
        <input
          className="launcher-input"
          placeholder={spec.resumePlaceholder}
          value={resumeSessionId}
          onChange={(e) => onResumeSessionIdChange(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") onSubmit(); }}
          style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}
        />
      )}
      {children}
    </div>
  );
}
