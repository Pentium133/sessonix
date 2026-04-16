import { useState, useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { detectAgents, getSetting, setSetting, installClaudeHooks } from "../lib/api";
import { useProjectStore } from "../store/projectStore";
import { sendNotification } from "@tauri-apps/plugin-notification";

interface WelcomeWizardProps {
  onComplete: () => void;
}

const AGENT_COLORS: Record<string, string> = {
  claude: "var(--claude)",
  codex: "var(--codex)",
  gemini: "var(--gemini)",
  opencode: "var(--opencode)",
};

export default function WelcomeWizard({ onComplete }: WelcomeWizardProps) {
  const [step, setStep] = useState(1);
  const [agents, setAgents] = useState<Record<string, boolean>>({});
  const [agentsLoaded, setAgentsLoaded] = useState(false);
  const [installHooks, setInstallHooks] = useState(false);
  const [notifStatus, setNotifStatus] = useState<"unknown" | "granted" | "denied">("unknown");
  const [selectedFolder, setSelectedFolder] = useState<string | null>(null);

  const totalSteps = 4;
  const anyAgentFound = Object.values(agents).some(Boolean);

  // Detect agents and check notification permission when entering step 2
  useEffect(() => {
    if (step === 2 && !agentsLoaded) {
      detectAgents()
        .then((result) => {
          setAgents(result);
          setAgentsLoaded(true);
          if (result["claude"]) setInstallHooks(true);
        })
        .catch(() => setAgentsLoaded(true));
      getSetting("notification_registered")
        .then((v) => setNotifStatus(v === "true" ? "granted" : "denied"))
        .catch(() => {});
    }
  }, [step, agentsLoaded]);

  const handleEnableNotifications = async () => {
    try {
      // Send a test notification to register with macOS Notification Center
      sendNotification({
        title: "Sessonix",
        body: "Notifications enabled. You'll be alerted when agents need attention.",
      });
      await setSetting("notification_registered", "true");
      setNotifStatus("granted");
    } catch {
      setNotifStatus("denied");
    }
  };

  const handleNext = async () => {
    if (step === 2 && installHooks) {
      await installClaudeHooks().catch(() => {});
    }
    setStep((s) => s + 1);
  };

  const handleChooseFolder = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setSelectedFolder(selected);
    }
  };

  const handleStepThreeNext = async () => {
    if (selectedFolder) {
      await useProjectStore.getState().addProject(selectedFolder).catch(() => {});
    }
    setStep(4);
  };

  const handleFinish = async () => {
    await setSetting("welcome_completed", "true");
    onComplete();
  };

  const folderName = selectedFolder?.split("/").pop() ?? null;

  return (
    <div className="wizard-overlay">
      <div className="wizard-container">

        {/* Step 1: Welcome */}
        {step === 1 && (
          <div className="wizard-step">
            <div className="wizard-logo">A</div>
            <h1 className="wizard-heading">Sessonix</h1>
            <p className="wizard-subtitle">Agent Mission Control</p>
            <p className="wizard-body">
              Run Claude, Codex, Gemini, OpenCode and custom AI agents side by side.
              Each session gets its own terminal. Switch between them instantly.
            </p>
            <button className="wizard-btn-primary" onClick={() => setStep(2)}>
              Get Started
            </button>
          </div>
        )}

        {/* Step 2: Agent Detection */}
        {step === 2 && (
          <div className="wizard-step">
            <h2 className="wizard-heading">Agent Detection</h2>
            <p className="wizard-body">
              Sessonix works with any AI coding CLI. Here's what we found on your system:
            </p>
            {!agentsLoaded ? (
              <div className="wizard-loading">Detecting agents...</div>
            ) : (
              <>
                <div className="wizard-agents">
                  {["claude", "codex", "gemini", "opencode"].map((name) => (
                    <div key={name} className="wizard-agent-row">
                      <span
                        className="wizard-agent-dot"
                        style={{ background: AGENT_COLORS[name] }}
                      />
                      <span className="wizard-agent-name">{name}</span>
                      <span className={`wizard-agent-status ${agents[name] ? "found" : "missing"}`}>
                        {agents[name] ? "✓ found" : "✗ not found"}
                      </span>
                    </div>
                  ))}
                </div>

                {!anyAgentFound && (
                  <p className="wizard-warning">
                    No AI agents detected. You can still use Sessonix with custom shell commands.
                  </p>
                )}

                {agents["claude"] && (
                  <label className="wizard-checkbox-label">
                    <input
                      type="checkbox"
                      checked={installHooks}
                      onChange={(e) => setInstallHooks(e.target.checked)}
                    />
                    Install Claude hooks (real-time session status)
                  </label>
                )}

                <div className="wizard-notif-row">
                  <span className="wizard-notif-label">Notifications</span>
                  {notifStatus === "granted" ? (
                    <span className="wizard-notif-granted">✓ Enabled</span>
                  ) : (
                    <button
                      className="wizard-btn-secondary"
                      onClick={handleEnableNotifications}
                    >
                      Enable Notifications
                    </button>
                  )}
                </div>
              </>
            )}
            <button
              className="wizard-btn-primary"
              onClick={handleNext}
              disabled={!agentsLoaded}
            >
              {installHooks ? "Install Hooks & Next" : "Next"}
            </button>
          </div>
        )}

        {/* Step 3: Add Project */}
        {step === 3 && (
          <div className="wizard-step">
            <h2 className="wizard-heading">Add a Project</h2>
            <p className="wizard-body">
              Pick a project folder to get started.
              You can add more projects later with <kbd>Cmd+Shift+K</kbd>.
            </p>
            {folderName ? (
              <div className="wizard-folder-selected">
                <span className="wizard-folder-check">✓</span>
                <span className="wizard-folder-name">{folderName}</span>
                <button
                  className="wizard-btn-secondary wizard-folder-change"
                  onClick={handleChooseFolder}
                >
                  Change
                </button>
              </div>
            ) : (
              <button className="wizard-btn-primary" onClick={handleChooseFolder}>
                Choose Folder
              </button>
            )}
            <div className="wizard-step3-actions">
              {folderName && (
                <button className="wizard-btn-primary" onClick={handleStepThreeNext}>
                  Next
                </button>
              )}
              <button className="wizard-link" onClick={() => setStep(4)}>
                Skip for now
              </button>
            </div>
          </div>
        )}

        {/* Step 4: Ready */}
        {step === 4 && (
          <div className="wizard-step">
            <div className="wizard-ready-icon">🚀</div>
            <h2 className="wizard-heading">You're all set</h2>
            <div className="wizard-shortcuts">
              <div className="wizard-shortcut-row">
                <kbd>Cmd+Shift+T</kbd>
                <span>New session</span>
              </div>
              <div className="wizard-shortcut-row">
                <kbd>Cmd+Shift+K</kbd>
                <span>Add project</span>
              </div>
              <div className="wizard-shortcut-row">
                <kbd>Cmd+← →</kbd>
                <span>Prev / next session</span>
              </div>
              <div className="wizard-shortcut-row">
                <kbd>Cmd+↑ ↓</kbd>
                <span>Prev / next project</span>
              </div>
              <div className="wizard-shortcut-row">
                <kbd>Cmd+Shift+W</kbd>
                <span>Kill session</span>
              </div>
            </div>
            <button className="wizard-btn-primary" onClick={handleFinish}>
              Start Using Sessonix
            </button>
          </div>
        )}

        {/* Step indicator */}
        <div className="wizard-dots">
          {Array.from({ length: totalSteps }, (_, i) => (
            <span
              key={i}
              className={`wizard-dot${step === i + 1 ? " active" : ""}`}
            />
          ))}
        </div>

      </div>
    </div>
  );
}
