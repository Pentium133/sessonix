import { useState, useEffect, useRef } from "react";

interface TemplateSaveModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSave: (name: string, prompt: string) => void;
  initial?: { name: string; prompt: string };
}

export default function TemplateSaveModal({ isOpen, onClose, onSave, initial }: TemplateSaveModalProps) {
  const [name, setName] = useState("");
  const [prompt, setPrompt] = useState("");
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (isOpen) {
      setName(initial?.name ?? "");
      setPrompt(initial?.prompt ?? "");
      setTimeout(() => nameRef.current?.focus(), 50);
    }
  }, [isOpen, initial]);

  useEffect(() => {
    if (!isOpen) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  const canSave = name.trim() && prompt.trim();

  return (
    <div className="launcher-overlay" onClick={onClose}>
      <div className="template-modal" onClick={(e) => e.stopPropagation()}>
        <div className="launcher-title">
          {initial ? "Edit Template" : "New Template"}
        </div>
        <input
          ref={nameRef}
          className="launcher-input"
          placeholder="Template name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && canSave) onSave(name.trim(), prompt.trim()); }}
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
