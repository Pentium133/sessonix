import { useEffect, useState } from "react";
import { useUiStore } from "../store/uiStore";
import { useSettingsStore, FONT_FAMILIES } from "../store/settingsStore";
import { checkClaudeHooks, installClaudeHooks } from "../lib/api";
import { isPermissionGranted } from "@tauri-apps/plugin-notification";
import { requestNotificationPermission } from "../lib/notifications";
import { THEMES } from "../lib/themes";
import type { Theme } from "../lib/constants";

const TABS = ["General", "Terminal", "Agents", "Notifications"] as const;
type Tab = (typeof TABS)[number];

interface SettingsModalProps {
  onCheckForUpdates?: () => void;
}

export default function SettingsModal({ onCheckForUpdates }: SettingsModalProps) {
  const closeSettings = useUiStore((s) => s.closeSettings);
  const theme = useUiStore((s) => s.theme);
  const setTheme = useUiStore((s) => s.setTheme);
  const uiZoom = useUiStore((s) => s.uiZoom);
  const setUiZoom = useUiStore((s) => s.setUiZoom);
  const fontSize = useSettingsStore((s) => s.terminalFontSize);
  const fontFamily = useSettingsStore((s) => s.terminalFontFamily);
  const defaultAgent = useSettingsStore((s) => s.defaultAgent);
  const skipPerms = useSettingsStore((s) => s.claudeSkipPermissions);
  const notifyPermission = useSettingsStore((s) => s.notifyPermission);
  const notifyExit = useSettingsStore((s) => s.notifyExit);
  const notifyIdle = useSettingsStore((s) => s.notifyIdle);
  const setFontSize = useSettingsStore((s) => s.setTerminalFontSize);
  const setFontFamily = useSettingsStore((s) => s.setTerminalFontFamily);
  const setDefaultAgent = useSettingsStore((s) => s.setDefaultAgent);
  const setSkipPerms = useSettingsStore((s) => s.setClaudeSkipPermissions);
  const setNotifyPermission = useSettingsStore((s) => s.setNotifyPermission);
  const setNotifyExit = useSettingsStore((s) => s.setNotifyExit);
  const setNotifyIdle = useSettingsStore((s) => s.setNotifyIdle);

  const [tab, setTab] = useState<Tab>("General");
  const [hooksInstalled, setHooksInstalled] = useState<boolean | null>(null);
  const [hooksLoading, setHooksLoading] = useState(false);
  const [notifGranted, setNotifGranted] = useState<boolean | null>(null);

  useEffect(() => {
    checkClaudeHooks().then(setHooksInstalled).catch(() => setHooksInstalled(false));
    isPermissionGranted().then(setNotifGranted).catch(() => setNotifGranted(false));
  }, []);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeSettings();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [closeSettings]);

  const handleInstallHooks = async () => {
    setHooksLoading(true);
    try {
      await installClaudeHooks();
      setHooksInstalled(true);
    } catch {
      setHooksInstalled(false);
    }
    setHooksLoading(false);
  };

  const handleEnableNotifications = async () => {
    const granted = await requestNotificationPermission();
    setNotifGranted(granted);
  };

  const handleBackdrop = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) closeSettings();
  };

  return (
    <div className="settings-backdrop" onClick={handleBackdrop}>
      <div className="settings-modal" role="dialog" aria-modal="true" aria-label="Settings">
        <div className="settings-header">
          <span className="settings-title">Settings</span>
          <button className="settings-close-btn" onClick={closeSettings} title="Close">
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
              <line x1="2" y1="2" x2="12" y2="12" />
              <line x1="12" y1="2" x2="2" y2="12" />
            </svg>
          </button>
        </div>

        <div className="settings-tabs">
          {TABS.map((t) => (
            <button
              key={t}
              className={`settings-tab${tab === t ? " active" : ""}`}
              onClick={() => setTab(t)}
            >
              {t}
            </button>
          ))}
        </div>

        <div className="settings-body">

          {tab === "General" && (
            <>
              <div className="settings-section">
                <div className="settings-section-title">Appearance</div>
                <div className="settings-row">
                  <label className="settings-label">Theme</label>
                  <select
                    className="settings-select"
                    value={theme}
                    onChange={(e) => setTheme(e.target.value as Theme)}
                  >
                    <option value="system">System</option>
                    {THEMES.map((t) => (
                      <option key={t.id} value={t.id}>
                        {t.label}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="settings-row">
                  <label className="settings-label">UI zoom</label>
                  <div className="settings-control">
                    <input
                      type="range"
                      min={75}
                      max={150}
                      step={5}
                      value={Math.round(uiZoom * 100)}
                      onChange={(e) => setUiZoom(parseInt(e.target.value, 10) / 100)}
                      className="settings-slider"
                    />
                    <span className="settings-value">{Math.round(uiZoom * 100)}%</span>
                  </div>
                </div>
              </div>

              <div className="settings-section">
                <div className="settings-section-title">About</div>
                <div className="settings-row">
                  <label className="settings-label">Updates</label>
                  <button
                    className="settings-btn-small"
                    onClick={() => {
                      closeSettings();
                      onCheckForUpdates?.();
                    }}
                  >
                    Check for Updates
                  </button>
                </div>
              </div>
            </>
          )}

          {tab === "Terminal" && (
            <div className="settings-section">
              <div className="settings-section-title">Terminal</div>
              <div className="settings-row">
                <label className="settings-label">Font size</label>
                <div className="settings-control">
                  <input
                    type="range"
                    min={10}
                    max={24}
                    value={fontSize}
                    onChange={(e) => setFontSize(parseInt(e.target.value, 10))}
                    className="settings-slider"
                  />
                  <span className="settings-value">{fontSize}px</span>
                </div>
              </div>
              <div className="settings-row">
                <label className="settings-label">Font family</label>
                <select
                  className="settings-select"
                  value={fontFamily}
                  onChange={(e) => setFontFamily(e.target.value)}
                >
                  {FONT_FAMILIES.map((f) => (
                    <option key={f.value} value={f.value}>
                      {f.label}
                    </option>
                  ))}
                </select>
              </div>
            </div>
          )}

          {tab === "Agents" && (
            <>
              <div className="settings-section">
                <div className="settings-section-title">Sessions</div>
                <div className="settings-row">
                  <label className="settings-label">Default agent</label>
                  <select
                    className="settings-select"
                    value={defaultAgent}
                    onChange={(e) => setDefaultAgent(e.target.value)}
                  >
                    <option value="claude">Claude</option>
                    <option value="codex">Codex</option>
                    <option value="gemini">Gemini</option>
                    <option value="opencode">OpenCode</option>
                    <option value="shell">Shell</option>
                  </select>
                </div>
              </div>

              <div className="settings-section">
                <div className="settings-section-title">Claude</div>
                <div className="settings-row">
                  <label className="settings-label">Skip permissions</label>
                  <label className="settings-toggle">
                    <input
                      type="checkbox"
                      checked={skipPerms}
                      onChange={(e) => setSkipPerms(e.target.checked)}
                    />
                    <span className="settings-toggle-track" />
                  </label>
                </div>
                <div className="settings-row">
                  <label className="settings-label">Hooks</label>
                  <div className="settings-control">
                    <span className={`settings-hooks-status ${hooksInstalled ? "installed" : ""}`}>
                      {hooksInstalled === null ? "Checking..." : hooksInstalled ? "Installed ✓" : "Not installed"}
                    </span>
                    <button
                      className="settings-btn-small"
                      onClick={handleInstallHooks}
                      disabled={hooksLoading}
                    >
                      {hooksLoading ? "..." : hooksInstalled ? "Reinstall" : "Install"}
                    </button>
                  </div>
                </div>
              </div>
            </>
          )}

          {tab === "Notifications" && (
            <div className="settings-section">
              <div className="settings-section-title">Notifications</div>
              <div className="settings-row">
                <label className="settings-label">System permission</label>
                <div className="settings-control">
                  <span className={`settings-hooks-status ${notifGranted ? "installed" : ""}`}>
                    {notifGranted === null ? "Checking..." : notifGranted ? "Allowed ✓" : "Not allowed"}
                  </span>
                  {!notifGranted && notifGranted !== null && (
                    <button
                      className="settings-btn-small"
                      onClick={handleEnableNotifications}
                    >
                      Enable
                    </button>
                  )}
                </div>
              </div>
              <div className="settings-row">
                <label className="settings-label">Permission requests</label>
                <label className="settings-toggle">
                  <input
                    type="checkbox"
                    checked={notifyPermission}
                    onChange={(e) => setNotifyPermission(e.target.checked)}
                  />
                  <span className="settings-toggle-track" />
                </label>
              </div>
              <div className="settings-row">
                <label className="settings-label">Session exit</label>
                <label className="settings-toggle">
                  <input
                    type="checkbox"
                    checked={notifyExit}
                    onChange={(e) => setNotifyExit(e.target.checked)}
                  />
                  <span className="settings-toggle-track" />
                </label>
              </div>
              <div className="settings-row">
                <label className="settings-label">Agent becomes idle</label>
                <label className="settings-toggle">
                  <input
                    type="checkbox"
                    checked={notifyIdle}
                    onChange={(e) => setNotifyIdle(e.target.checked)}
                  />
                  <span className="settings-toggle-track" />
                </label>
              </div>
            </div>
          )}

        </div>
      </div>
    </div>
  );
}
