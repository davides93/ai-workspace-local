import type { ReactNode } from "react";
import { Chip, Field, Input, Select, Textarea } from "../../../shared/ui";
import type { AgentProfile, InferenceProfile, ToolInfo } from "../../../shared/api/types";
import type { Issue } from "../model/draft";
import { schemaParams, type StepDraft } from "../model/steps";
import { PlaceholderInsert } from "./PlaceholderInsert";

export interface StepCatalog {
  agents: AgentProfile[];
  profiles: InferenceProfile[];
  tools: ToolInfo[];
}

type Props<K extends StepDraft["kind"]> = {
  step: Extract<StepDraft, { kind: K }>;
  uid: number;
  /** Issues of this step only. */
  issues: Issue[];
  catalog: StepCatalog;
  /** Steps before this one and the workflow inputs, for the placeholder menu. */
  before: StepDraft[];
  inputKeys: string[];
  /** Every other step, for the condition targets. */
  others: StepDraft[];
  onChange: (next: StepDraft) => void;
};

/** `error` and `hint` props of a field from the issues of the step. */
function note(issues: Issue[], field: string): { error?: string; hint?: ReactNode } {
  const hit = issues.filter((i) => i.field === field);
  const err = hit.find((i) => i.level === "error");
  const warn = hit.filter((i) => i.level === "warn");
  return { error: err?.message, hint: warn.length ? <span className="row-warn">{warn.map((w) => w.message).join(" ")}</span> : undefined };
}

function PromptField({ id, label, value, rows = 4, placeholder, issues, field, before, inputKeys, onChange }: {
  id: string; label: string; value: string; rows?: number; placeholder?: string; issues: Issue[]; field: string; before: StepDraft[]; inputKeys: string[]; onChange: (v: string) => void;
}) {
  const n = note(issues, field);
  const placeholderWarn = note(issues, "placeholder");
  return (
    <Field label={label} htmlFor={id} error={n.error} hint={n.hint ?? placeholderWarn.hint}>
      <Textarea id={id} rows={rows} value={value} placeholder={placeholder} onChange={(e) => onChange(e.target.value)} aria-invalid={!!n.error} />
      <PlaceholderInsert targetId={id} value={value} onChange={onChange} before={before} inputKeys={inputKeys} />
    </Field>
  );
}

export function ToolFields({ step, uid, issues, catalog, before, inputKeys, onChange }: Props<"tool">) {
  const tool = catalog.tools.find((t) => t.name === step.tool);
  const params = tool ? schemaParams(tool.parameters) : [];
  const names = [...catalog.tools].sort((a, b) => a.name.localeCompare(b.name));
  const t = note(issues, "tool");
  const a = note(issues, "args");
  return (
    <>
      <Field label="Tool" htmlFor={`st-${uid}-tool`} error={t.error} hint={t.hint ?? tool?.description}>
        <Select id={`st-${uid}-tool`} value={step.tool} onChange={(e) => onChange({ ...step, tool: e.target.value })} aria-invalid={!!t.error}>
          {!step.tool && <option value="">Scegli un tool…</option>}
          {step.tool && !tool && <option value={step.tool}>{step.tool} (non nel catalogo)</option>}
          {names.map((x) => <option key={x.name} value={x.name}>{x.name}{x.enabled ? "" : " (disattivato)"}</option>)}
        </Select>
      </Field>
      <Field label="Argomenti (JSON)" htmlFor={`st-${uid}-args`} error={a.error} hint={a.hint ?? (params.length ? <>Parametri: {params.map((p) => <Chip key={p.name} title={p.required ? "Obbligatorio" : "Facoltativo"}>{p.name}{p.required ? "*" : ""}</Chip>)}</> : "Un oggetto JSON; puoi usare i segnaposto nei valori.")}>
        <Textarea id={`st-${uid}-args`} rows={4} className="mono" spellCheck={false} value={step.args} onChange={(e) => onChange({ ...step, args: e.target.value })} aria-invalid={!!a.error} />
        <PlaceholderInsert targetId={`st-${uid}-args`} value={step.args} onChange={(v) => onChange({ ...step, args: v })} before={before} inputKeys={inputKeys} quoted />
      </Field>
    </>
  );
}

export function AgentFields({ step, uid, issues, catalog, before, inputKeys, onChange }: Props<"agent">) {
  const known = catalog.agents.some((a) => a.id === step.agent);
  const n = note(issues, "agent");
  return (
    <>
      <Field label="Agente" htmlFor={`st-${uid}-agent`} error={n.error} hint={n.hint ?? catalog.agents.find((a) => a.id === step.agent)?.description}>
        <Select id={`st-${uid}-agent`} value={step.agent} onChange={(e) => onChange({ ...step, agent: e.target.value })} aria-invalid={!!n.error}>
          {!step.agent && <option value="">Scegli un agente…</option>}
          {step.agent && !known && <option value={step.agent}>{step.agent} (non esiste)</option>}
          {catalog.agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
        </Select>
      </Field>
      <PromptField id={`st-${uid}-prompt`} label="Compito" value={step.prompt} placeholder="Descrivi cosa deve fare l'agente. Puoi citare i risultati dei passi precedenti." issues={issues} field="prompt" before={before} inputKeys={inputKeys} onChange={(prompt) => onChange({ ...step, prompt })} />
    </>
  );
}

export function LlmFields({ step, uid, issues, catalog, before, inputKeys, onChange }: Props<"llm">) {
  const known = catalog.profiles.some((p) => p.id === step.profile);
  const n = note(issues, "profile");
  return (
    <>
      <Field label="Profilo di inferenza" htmlFor={`st-${uid}-profile`} error={n.error} hint={n.hint}>
        <Select id={`st-${uid}-profile`} value={step.profile} onChange={(e) => onChange({ ...step, profile: e.target.value })} aria-invalid={!!n.error}>
          {!step.profile && <option value="">Scegli un profilo…</option>}
          {step.profile && !known && <option value={step.profile}>{step.profile} (non esiste)</option>}
          {catalog.profiles.map((p) => <option key={p.id} value={p.id}>{p.name} — {p.model || "nessun modello"}</option>)}
        </Select>
      </Field>
      <PromptField id={`st-${uid}-prompt`} label="Prompt" value={step.prompt} placeholder="Una sola richiesta al modello, senza tool." issues={issues} field="prompt" before={before} inputKeys={inputKeys} onChange={(prompt) => onChange({ ...step, prompt })} />
    </>
  );
}

export function ApprovalFields({ step, uid, issues, before, inputKeys, onChange }: Props<"approval">) {
  return (
    <PromptField id={`st-${uid}-message`} label="Domanda per te" value={step.message} rows={2} placeholder="Sposto i file come proposto?" issues={issues} field="message" before={before} inputKeys={inputKeys} onChange={(message) => onChange({ ...step, message })} />
  );
}

function Targets({ legend, value, others, onToggle }: { legend: string; value: string[]; others: StepDraft[]; onToggle: (id: string, on: boolean) => void }) {
  return (
    <fieldset className="targets">
      <legend className="field-label">{legend}</legend>
      {others.length === 0 ? <span className="faint text-sm">Aggiungi altri passi per sceglierli qui.</span> : (
        <div className="chips">
          {others.map((o) => (
            <button key={o.id} type="button" className={`chip chip-btn ${value.includes(o.id) ? "hot" : ""}`} aria-pressed={value.includes(o.id)} onClick={() => onToggle(o.id, !value.includes(o.id))}>
              {o.id}
            </button>
          ))}
        </div>
      )}
    </fieldset>
  );
}

export function ConditionFields({ step, uid, issues, before, inputKeys, others, onChange }: Props<"condition">) {
  const n = note(issues, "if");
  const t = issues.filter((i) => i.field === "then");
  const toggle = (which: "then" | "else", id: string, on: boolean) => {
    const other = which === "then" ? "else" : "then";
    onChange({ ...step, [which]: on ? [...step[which], id] : step[which].filter((x) => x !== id), [other]: step[other].filter((x) => x !== id) });
  };
  return (
    <>
      <Field
        label="Espressione" htmlFor={`st-${uid}-if`} error={n.error}
        hint={n.hint ?? <>Forme ammesse: <code>A contains B</code>, <code>A == B</code>, <code>A != B</code>, <code>not A</code>, oppure un valore (vuoto, «0», «no» e «false» sono falsi). I segnaposto si sostituiscono prima.</>}
      >
        <Input id={`st-${uid}-if`} mono value={step.if} onChange={(e) => onChange({ ...step, if: e.target.value })} placeholder="{{steps.scan.output}} contains pdf" aria-invalid={!!n.error} spellCheck={false} />
        <PlaceholderInsert targetId={`st-${uid}-if`} value={step.if} onChange={(v) => onChange({ ...step, if: v })} before={before} inputKeys={inputKeys} />
      </Field>
      <div className="targets-grid">
        <Targets legend="Se vero, esegui" value={step.then} others={others} onToggle={(id, on) => toggle("then", id, on)} />
        <Targets legend="Altrimenti, esegui" value={step.else} others={others} onToggle={(id, on) => toggle("else", id, on)} />
      </div>
      <p className="faint text-xs">I passi non scelti vengono saltati. Un passo scelto deve venire dopo la condizione, altrimenti non parte mai.</p>
      {t.map((i) => <p key={i.message} className={i.level === "error" ? "field-error" : "row-warn"}>{i.message}</p>)}
    </>
  );
}
