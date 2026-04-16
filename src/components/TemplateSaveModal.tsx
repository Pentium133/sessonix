import { useState } from "react";

interface TemplateSaveModalProps {
  title: string;
  initialName?: string;
  initialPrompt?: string;
  onClose: () => void;
  onSave: (name: string, prompt: string) => void;
}

export default function TemplateSaveModal({ title, initialName, initialPrompt, onClose, onSave }: TemplateSaveModalProps) {
  const [name, setName] = useState(initialName ?? "");
  const [prompt, setPrompt] = useState(initialPrompt ?? "");

  const canSave = name.trim() && prompt.trim();

  return (
    <div className="launcher-overlay" onClick={onClose}>
      <div className="template-modal" onClick={(e) => e.stopPropagation()}>
        <div className="launcher-title">{title}</div>
        <input
          className="launcher-input"
          placeholder="Template name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          autoFocus
        />
        <textarea
          className="launcher-input launcher-prompt"
          placeholder="Prompt text"
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          rows={10}
        />
        <div className="launcher-actions">
          <button className="launcher-back" onClick={onClose}>Cancel</button>
          <button
            className="launcher-launch"
            disabled={!canSave}
            onClick={() => onSave(name.trim(), prompt.trim())}
          >
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
