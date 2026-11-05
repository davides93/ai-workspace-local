import type { ReactNode } from "react";
import { Chip, Field, Input, Select, Textarea } from "../../../shared/ui";
import type { AgentProfile, InferenceProfile, ToolInfo } from "../../../shared/api/types";
import type { Issue } from "../model/draft";
import { cleanOptions, DEFAULT_RETRY_DELAY_S, OPTION_LIMITS, schemaParams, type StepDraft } from "../model/steps";
import type { StepOptions } from "../../../shared/api/types";
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
  const t = note(issues, "timeout_s");
  return (
    <>
      <PromptField id={`st-${uid}-message`} label="Domanda per te" value={step.message} rows={2} placeholder="Sposto i file come proposto?" issues={issues} field="message" before={before} inputKeys={inputKeys} onChange={(message) => onChange({ ...step, message })} />
      <div className="step-options-grid">
        <Field label="Attendi al massimo (s)" htmlFor={`st-${uid}-wait`} error={t.error} hint="Vuoto = aspetta senza limite">
          <Input
            id={`st-${uid}-wait`} type="number" min={1} max={OPTION_LIMITS.timeout} value={step.timeout_s ?? ""}
            onChange={(e) => onChange({ ...step, timeout_s: e.target.value.trim() === "" ? undefined : Number(e.target.value) })} aria-invalid={!!t.error}
          />
        </Field>
        {step.timeout_s !== undefined && (
          <Field label="Se nessuno risponde" htmlFor={`st-${uid}-default`}>
            <Select id={`st-${uid}-default`} value={step.default ?? "deny"} onChange={(e) => onChange({ ...step, default: e.target.value === "allow" ? "allow" : undefined })}>
              <option value="deny">Rifiuta (il workflow finisce)</option>
              <option value="allow">Approva e prosegui</option>
            </Select>
          </Field>
        )}
      </div>
    </>
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

/** Retry, time limit and what to do when the step fails; closed unless something is set. */
export function OptionsFields({ opts, uid, issues, onChange }: { opts?: StepOptions; uid: number; issues: Issue[]; onChange: (next?: StepOptions) => void }) {
  const o = opts ?? {};
  const set = (patch: StepOptions) => onChange({ ...o, ...patch });
  const num = (v: string): number | undefined => (v.trim() === "" ? undefined : Number(v));
  const err = (field: string) => note(issues, field).error;
  return (
    <details className="step-options" open={!!cleanOptions(opts) || undefined}>
      <summary className="muted text-sm">Tentativi, limite di tempo ed errori</summary>
      <div className="step-options-grid">
        <Field label="Nuovi tentativi" htmlFor={`st-${uid}-retry`} error={err("retry")} hint={`0–${OPTION_LIMITS.retry}`}>
          <Input id={`st-${uid}-retry`} type="number" min={0} max={OPTION_LIMITS.retry} value={o.retry ?? ""} placeholder="0" onChange={(e) => set({ retry: num(e.target.value) })} aria-invalid={!!err("retry")} />
        </Field>
        <Field label="Attesa tra i tentativi (s)" htmlFor={`st-${uid}-delay`} error={err("retry_delay_s")} hint={`0–${OPTION_LIMITS.retryDelay}`}>
          <Input id={`st-${uid}-delay`} type="number" min={0} max={OPTION_LIMITS.retryDelay} value={o.retry_delay_s ?? ""} placeholder={String(DEFAULT_RETRY_DELAY_S)} onChange={(e) => set({ retry_delay_s: num(e.target.value) })} aria-invalid={!!err("retry_delay_s")} />
        </Field>
        <Field label="Limite di tempo (s)" htmlFor={`st-${uid}-timeout`} error={err("timeout_s")} hint="Per ogni tentativo; vuoto = nessun limite">
          <Input id={`st-${uid}-timeout`} type="number" min={1} max={OPTION_LIMITS.timeout} value={o.timeout_s ?? ""} onChange={(e) => set({ timeout_s: num(e.target.value) })} aria-invalid={!!err("timeout_s")} />
        </Field>
        <Field label="Se il passo fallisce" htmlFor={`st-${uid}-onerror`} hint="Con «prosegui» l'errore è in {{steps.<id>.error}}">
          <Select id={`st-${uid}-onerror`} value={o.on_error ?? "fail"} onChange={(e) => set({ on_error: e.target.value === "continue" ? "continue" : "fail" })}>
            <option value="fail">Ferma il workflow</option>
            <option value="continue">Prosegui con i passi successivi</option>
          </Select>
        </Field>
      </div>
    </details>
  );
}
