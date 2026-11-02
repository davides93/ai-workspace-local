import { Field, Icon, Input, Pill } from "../../../shared/ui";
import { APPROVAL_MODES, SANDBOX_MODES } from "../../../shared/lib/policyModes";
import type { PillTone } from "../../../shared/ui";
import type { PolicyMode, SandboxMode, AgentProfile } from "../../../shared/api/types";
import { MAX_TURNS_LIMIT, type AgentIssue } from "../model/agents";

interface Choice<T extends string> {
  id: T;
  label: string;
  summary: string;
  detail?: string;
  tone: PillTone;
}

/** Native radio buttons dressed as cards: one choice, each explained. */
function ChoiceCards<T extends string>({ legend, name, value, options, onChange }: { legend: string; name: string; value: T; options: Choice<T>[]; onChange: (id: T) => void }) {
  return (
    <fieldset className="choice-cards">
      <legend className="field-label">{legend}</legend>
      <div className="choice-grid">
        {options.map((o) => (
          <label key={o.id} className="choice-card" data-tone={o.tone} data-checked={value === o.id ? "true" : undefined}>
            <input type="radio" name={name} value={o.id} checked={value === o.id} onChange={() => onChange(o.id)} />
            <span className="choice-head">
              <Pill tone={o.tone}>{o.label}</Pill>
              <code className="faint">{o.id}</code>
            </span>
            <span className="choice-summary">{o.summary}</span>
            {value === o.id && o.detail && <span className="choice-detail text-xs muted">{o.detail}</span>}
          </label>
        ))}
      </div>
    </fieldset>
  );
}

/** Approval mode, sandbox and the turn limit: what the agent may do on its own. */
export function SecuritySection({ agent, issues, onChange }: { agent: AgentProfile; issues: AgentIssue[]; onChange: (patch: Partial<AgentProfile>) => void }) {
  const turnsError = issues.find((i) => i.field === "max_turns" && i.level === "error")?.message;
  const warnings = issues.filter((i) => (i.field === "mode" || i.field === "sandbox") && i.level === "warn");
  return (
    <div className="form-grid">
      <ChoiceCards<PolicyMode>
        legend="Modalità di approvazione" name="ag-mode" value={agent.mode} onChange={(mode) => onChange({ mode })}
        options={APPROVAL_MODES.map((m) => ({ id: m.id, label: m.label, summary: m.summary, detail: m.detail, tone: m.tone }))}
      />
      <ChoiceCards<SandboxMode>
        legend="Sandbox dei comandi" name="ag-sandbox" value={agent.sandbox} onChange={(sandbox) => onChange({ sandbox })}
        options={SANDBOX_MODES.map((m) => ({ id: m.id, label: m.label, summary: m.summary, tone: m.tone }))}
      />
      {warnings.map((w) => <p key={w.field} className="tool-warn text-sm" role="note"><Icon name="alert" size={14} /> {w.message}</p>)}
      <Field label="Turni massimi" htmlFor="ag-turns" error={turnsError} hint={`Dopo questo numero di giri tra agente e modello il run si ferma (1–${MAX_TURNS_LIMIT}).`} className="field-narrow">
        <Input
          id="ag-turns" className="num" type="number" min={1} max={MAX_TURNS_LIMIT} step={1} inputMode="numeric"
          value={Number.isNaN(agent.max_turns) ? "" : agent.max_turns} aria-invalid={!!turnsError}
          onChange={(e) => onChange({ max_turns: e.target.value === "" ? Number.NaN : Number(e.target.value) })}
        />
      </Field>
    </div>
  );
}
