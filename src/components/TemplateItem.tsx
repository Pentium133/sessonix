import AgentIcon from "./AgentIcon";
import { AGENT_COLORS } from "../lib/constants";
import type { TemplateInfo } from "../lib/api";
import type { AgentType } from "../lib/types";

interface TemplateItemProps {
  template: TemplateInfo;
  onRun: () => void;
  onEdit: () => void;
  onDelete: () => void;
}

export default function TemplateItem({ template, onRun, onEdit, onDelete }: TemplateItemProps) {
  const agentColor = AGENT_COLORS[template.agent] ?? AGENT_COLORS.custom;

  return (
    <div
      className="template-item"
      style={{ "--agent-color": agentColor } as React.CSSProperties}
    >
      <div className="template-badge" />
      <div className="template-body">
        <div className="template-row-top">
          <AgentIcon agentType={template.agent as AgentType} size={12} />
          <span className="template-name">{template.name}</span>
        </div>
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
