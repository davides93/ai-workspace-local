import { useEffect, useState } from "react";
import { Icon, IconButton, Input, Select, StatusPill, RowControls } from "../../../shared/ui";
import type { Issue } from "../model/draft";
import { hasOptions, optionsSummary, STEP_ID, STEP_KINDS, stepMeta, stepSummary, type StepDraft, type StepKind } from "../model/steps";
import { AgentFields, ApprovalFields, ConditionFields, ForeachFields, LlmFields, OptionsFields, ParallelFields, TeamFields, ToolFields, WorkflowFields, type StepCatalog } from "./StepForms";

/** Id of a step: edited as text, applied on blur so that references only follow a valid, free id. */
function IdField({ uid, index, value, taken, onCommit }: { uid: number; index: number; value: string; taken: string[]; onCommit: (id: string) => void }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  const id = text.trim();
  const problem =
    id === value || id === "" && value === "" ? null
    : id === "" ? "L'identificativo non può essere vuoto."
    : !STEP_ID.test(id) ? "Solo lettere, cifre, «_» e «-»."
    : taken.includes(id) ? "Già usato da un altro passo."
    : null;
  return (
    <div className="step-id">
      <label htmlFor={`st-${uid}-id`} className="sr-only">Identificativo del passo {index + 1}</label>
      <Input
        id={`st-${uid}-id`} mono value={text} spellCheck={false} aria-invalid={!!problem} aria-describedby={problem ? `st-${uid}-id-err` : undefined}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => (problem || id === value ? setText(value) : onCommit(id))}
        onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
      />
      {problem && <span id={`st-${uid}-id-err`} className="field-error" role="alert">{problem}</span>}
    </div>
  );
}

/** One step of the workflow: kind, id, order controls and the form of its kind. */
export function StepCard({ uid, index, steps, catalog, inputKeys, issues, runStatus, collapsed, onToggle, onChange, onKind, onRename, onMove, onRemove }: {
  uid: number;
  index: number;
  steps: StepDraft[];
  catalog: StepCatalog;
  inputKeys: string[];
  /** Issues of this step. */
  issues: Issue[];
  /** Status of the step in the last run, when there is one. */
  runStatus?: string;
  collapsed: boolean;
  onToggle: () => void;
  onChange: (next: StepDraft) => void;
  onKind: (kind: StepKind) => void;
  onRename: (id: string) => void;
  onMove: (delta: -1 | 1) => void;
  onRemove: () => void;
}) {
  const step = steps[index];
  const meta = stepMeta(step.kind);
  const errors = issues.filter((i) => i.level === "error").length;
  const common = { uid, issues, catalog, before: steps.slice(0, index), inputKeys, others: steps.filter((_, i) => i !== index), onChange };
  const idIssue = issues.find((i) => i.field === "id");
  return (
    <li className="step-card" id={`step-${uid}`} data-kind={step.kind} data-invalid={errors > 0 || undefined}>
      <div className="step-rail" aria-hidden="true"><b>{index + 1}</b>{index < steps.length - 1 && <i />}</div>
      <div className="step-body">
        <div className="step-head">
          <IconButton aria-label={`${collapsed ? "Espandi" : "Comprimi"} il passo ${index + 1}`} aria-expanded={!collapsed} icon="down" size={16} className={collapsed ? "chev collapsed" : "chev"} onClick={onToggle} />
          <Select aria-label={`Tipo del passo ${index + 1}`} value={step.kind} className="step-kind" onChange={(e) => onKind(e.target.value as StepKind)}>
            {STEP_KINDS.map((k) => <option key={k.kind} value={k.kind}>{k.label}</option>)}
          </Select>
          <IdField uid={uid} index={index} value={step.id} taken={steps.filter((_, i) => i !== index).map((s) => s.id)} onCommit={onRename} />
          {runStatus && <span className="step-status"><StatusPill status={runStatus} /></span>}
          <RowControls index={index} count={steps.length} noun={`il passo ${index + 1} (${step.id})`} onMove={onMove} onRemove={onRemove} />
        </div>
        {idIssue && <p className={idIssue.level === "error" ? "field-error" : "row-warn"}>{idIssue.message}</p>}
        {collapsed ? (
          <p className="step-summary mono muted text-xs"><Icon name={meta.icon} size={13} /> {stepSummary(step)}{hasOptions(step) && optionsSummary(step.opts) ? ` (${optionsSummary(step.opts)})` : ""}</p>
        ) : (
          <div className="step-fields">
            {step.kind === "tool" && <ToolFields {...common} step={step} />}
            {step.kind === "agent" && <AgentFields {...common} step={step} />}
            {step.kind === "team" && <TeamFields {...common} step={step} />}
            {step.kind === "llm" && <LlmFields {...common} step={step} />}
            {step.kind === "foreach" && <ForeachFields {...common} step={step} />}
            {step.kind === "parallel" && <ParallelFields {...common} step={step} />}
            {step.kind === "workflow" && <WorkflowFields {...common} step={step} />}
            {step.kind === "approval" && <ApprovalFields {...common} step={step} />}
            {step.kind === "condition" && <ConditionFields {...common} step={step} />}
            {hasOptions(step) && <OptionsFields uid={uid} issues={issues} opts={step.opts} onChange={(opts) => onChange({ ...step, opts })} />}
          </div>
        )}
      </div>
    </li>
  );
}
