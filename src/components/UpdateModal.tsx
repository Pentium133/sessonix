import { useEffect, useState } from "react";
import type { UpdateInfo } from "../lib/api";
import { openReleasePage } from "../lib/updater";

interface UpdateModalProps {
  update: UpdateInfo;
  onClose: () => void;
}

export default function UpdateModal({ update, onClose }: UpdateModalProps) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  const handleBackdrop = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) onClose();
  };

  const handleDownload = () => {
    void openReleasePage(update.html_url);
    onClose();
  };

  return (
    <div className="settings-backdrop" onClick={handleBackdrop}>
      <div className="update-modal" role="dialog" aria-modal="true" aria-label="Update available">
        <div className="update-icon">
          <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 16V4" />
            <path d="M8 12l4 4 4-4" />
            <path d="M20 21H4" />
          </svg>
        </div>
        <div className="update-title">Update Available</div>
        <div className="update-body">
          Version <strong>v{update.version}</strong> is available.
          You&#39;re on <strong>v{update.current_version}</strong>.
        </div>
        <div className="update-brew-hint">
          <code>brew update && brew upgrade sessonix</code>
          <button
            className="update-copy-btn"
            onClick={() => {
              void navigator.clipboard.writeText("brew update && brew upgrade sessonix");
              setCopied(true);
              setTimeout(() => setCopied(false), 2000);
            }}
            title="Copy to clipboard"
          >
            {copied ? (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M20 6L9 17l-5-5" />
              </svg>
            ) : (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="9" y="9" width="13" height="13" rx="2" />
                <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
              </svg>
            )}
          </button>
        </div>
        <div className="update-actions">
          <button className="update-btn secondary" onClick={onClose}>
            Later
          </button>
          <button className="update-btn primary" onClick={handleDownload}>
            Download
          </button>
        </div>
      </div>
    </div>
  );
}
