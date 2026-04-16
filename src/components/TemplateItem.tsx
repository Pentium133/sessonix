import type { TemplateInfo } from "../lib/api";

interface TemplateItemProps {
  template: TemplateInfo;
  onRun: () => void;
  onEdit: () => void;
  onDelete: () => void;
}

export default function TemplateItem({ template, onRun, onEdit, onDelete }: TemplateItemProps) {
  return (
    <div className="template-item">
      <div className="template-body">
        <div className="template-name">{template.name}</div>
        {template.initial_prompt && (
          <div className="template-prompt" title={template.initial_prompt}>
            {template.initial_prompt}
          </div>
        )}
      </div>
      <div className="template-actions">
        <button className="template-btn template-btn-run" onClick={onRun} title="Run">
          ▶
        </button>
        <button className="template-btn" onClick={onEdit} title="Edit">
          ✎
        </button>
        <button className="template-btn template-btn-delete" onClick={onDelete} title="Delete">
          ×
        </button>
      </div>
    </div>
  );
}
