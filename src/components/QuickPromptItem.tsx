import type { QuickPromptInfo } from "../lib/api";

interface QuickPromptItemProps {
  quickPrompt: QuickPromptInfo;
  onRun: () => void;
  onEdit: () => void;
  onDelete: () => void;
}

export default function QuickPromptItem({ quickPrompt, onRun, onEdit, onDelete }: QuickPromptItemProps) {
  return (
    <div className="quick-prompt-item">
      <div className="quick-prompt-body">
        <div className="quick-prompt-name">{quickPrompt.name}</div>
        {quickPrompt.initial_prompt && (
          <div className="quick-prompt-prompt" title={quickPrompt.initial_prompt}>
            {quickPrompt.initial_prompt}
          </div>
        )}
      </div>
      <div className="quick-prompt-actions">
        <button className="quick-prompt-btn quick-prompt-btn-run" onClick={onRun} title="Run">
          ▶
        </button>
        <button className="quick-prompt-btn" onClick={onEdit} title="Edit">
          ✎
        </button>
        <button className="quick-prompt-btn quick-prompt-btn-delete" onClick={onDelete} title="Delete">
          ×
        </button>
      </div>
    </div>
  );
}
