import type { ReactNode } from "react";
import { Chip, Field, Input, Select, Textarea } from "../../../shared/ui";
import type { AgentProfile, InferenceProfile, TeamDef, ToolInfo } from "../../../shared/api/types";
import type { Issue } from "../model/draft";
import { cleanOptions, DEFAULT_RETRY_DELAY_S, FOREACH_MAX_PARALLEL, OPTION_LIMITS, schemaParams, type StepDraft } from "../model/steps";
import type { StepOptions } from "../../../shared/api/types";
import { PlaceholderInsert } from "./PlaceholderInsert";

import { t as tr, tNode as trNode } from "../../../shared/i18n";
export interface StepCatalog {
  /** Workflows a sub-workflow step can start (not the one being edited). */
  workflows: { id: string; name: string }[];
  agents: AgentProfile[];
  teams: TeamDef[];
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
      <Field label={tr("workflows.tool")} htmlFor={`st-${uid}-tool`} error={t.error} hint={t.hint ?? tool?.description}>
        <Select id={`st-${uid}-tool`} value={step.tool} onChange={(e) => onChange({ ...step, tool: e.target.value })} aria-invalid={!!t.error}>
          {!step.tool && <option value="">{tr("workflows.scegli_un_tool")}</option>}
          {step.tool && !tool && <option value={step.tool}>{tr("workflows.non_nel_catalogo", { tool: step.tool })}</option>}
          {names.map((x) => <option key={x.name} value={x.name}>{x.name}{x.enabled ? "" : tr("workflows.disattivato")}</option>)}
        </Select>
      </Field>
      <Field label={tr("workflows.argomenti_json")} htmlFor={`st-${uid}-args`} error={a.error} hint={a.hint ?? (params.length ? <>Parametri: {params.map((p) => <Chip key={p.name} title={p.required ? "Obbligatorio" : "Facoltativo"}>{p.name}{p.required ? "*" : ""}</Chip>)}</> : tr("workflows.un_oggetto_json_puoi_usare"))}>
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
      <Field label={tr("workflows.agente")} htmlFor={`st-${uid}-agent`} error={n.error} hint={n.hint ?? catalog.agents.find((a) => a.id === step.agent)?.description}>
        <Select id={`st-${uid}-agent`} value={step.agent} onChange={(e) => onChange({ ...step, agent: e.target.value })} aria-invalid={!!n.error}>
          {!step.agent && <option value="">{tr("workflows.scegli_un_agente")}</option>}
          {step.agent && !known && <option value={step.agent}>{tr("workflows.non_esiste", { agent: step.agent })}</option>}
          {catalog.agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
        </Select>
      </Field>
      <PromptField id={`st-${uid}-prompt`} label={tr("workflows.compito")} value={step.prompt} placeholder={tr("workflows.descrivi_cosa_deve_fare_l")} issues={issues} field="prompt" before={before} inputKeys={inputKeys} onChange={(prompt) => onChange({ ...step, prompt })} />
    </>
  );
}

export function TeamFields({ step, uid, issues, catalog, before, inputKeys, onChange }: Props<"team">) {
  const known = catalog.teams.some((t) => t.id === step.team);
  const n = note(issues, "team");
  return (
    <>
      <Field label={tr("workflows.team")} htmlFor={`st-${uid}-team`} error={n.error} hint={n.hint ?? catalog.teams.find((t) => t.id === step.team)?.description}>
        <Select id={`st-${uid}-team`} value={step.team} onChange={(e) => onChange({ ...step, team: e.target.value })} aria-invalid={!!n.error}>
          {!step.team && <option value="">{tr("workflows.scegli_un_team")}</option>}
          {step.team && !known && <option value={step.team}>{tr("workflows.non_esiste_2", { team: step.team })}</option>}
          {catalog.teams.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
        </Select>
      </Field>
      <PromptField id={`st-${uid}-prompt`} label={tr("workflows.compito")} value={step.prompt} placeholder={tr("workflows.descrivi_il_lavoro_del_team")} issues={issues} field="prompt" before={before} inputKeys={inputKeys} onChange={(prompt) => onChange({ ...step, prompt })} />
    </>
  );
}

export function LlmFields({ step, uid, issues, catalog, before, inputKeys, onChange }: Props<"llm">) {
  const known = catalog.profiles.some((p) => p.id === step.profile);
  const n = note(issues, "profile");
  return (
    <>
      <Field label={tr("workflows.profilo_di_inferenza")} htmlFor={`st-${uid}-profile`} error={n.error} hint={n.hint}>
        <Select id={`st-${uid}-profile`} value={step.profile} onChange={(e) => onChange({ ...step, profile: e.target.value })} aria-invalid={!!n.error}>
          {!step.profile && <option value="">{tr("workflows.scegli_un_profilo")}</option>}
          {step.profile && !known && <option value={step.profile}>{tr("workflows.non_esiste_3", { profile: step.profile })}</option>}
          {catalog.profiles.map((p) => <option key={p.id} value={p.id}>{p.name} — {p.model || tr("workflows.nessun_modello")}</option>)}
        </Select>
      </Field>
      <PromptField id={`st-${uid}-prompt`} label={tr("workflows.prompt")} value={step.prompt} placeholder={tr("workflows.una_sola_richiesta_al_modello")} issues={issues} field="prompt" before={before} inputKeys={inputKeys} onChange={(prompt) => onChange({ ...step, prompt })} />
    </>
  );
}

/** The text of an inline step (or list of steps) as JSON, with the check of what it says. */
function InlineJson({ id, label, value, hint, rows, issues, field, onChange }: { id: string; label: string; value: string; hint: ReactNode; rows: number; issues: Issue[]; field: string; onChange: (v: string) => void }) {
  const n = note(issues, field);
  return (
    <Field label={label} htmlFor={id} error={n.error} hint={n.hint ?? hint}>
      <Textarea id={id} rows={rows} className="mono" spellCheck={false} value={value} onChange={(e) => onChange(e.target.value)} aria-invalid={!!n.error} />
    </Field>
  );
}

export function ForeachFields({ step, uid, issues, before, inputKeys, onChange }: Props<"foreach">) {
  const p = note(issues, "parallel");
  return (
    <>
      <PromptField id={`st-${uid}-items`} label={tr("workflows.elenco_da_scorrere")} value={step.items} rows={2} placeholder="{{steps.scan.output}}" issues={issues} field="items" before={before} inputKeys={inputKeys} onChange={(items) => onChange({ ...step, items })} />
      <p className="faint text-xs">{tr("workflows.un_array_json_anche_in")}</p>
      <Field label={tr("workflows.voci_alla_volta")} htmlFor={`st-${uid}-parallel`} error={p.error} hint={`1–${FOREACH_MAX_PARALLEL}`}>
        <Input id={`st-${uid}-parallel`} type="number" min={1} max={FOREACH_MAX_PARALLEL} value={step.parallel ?? ""} placeholder="1" onChange={(e) => onChange({ ...step, parallel: e.target.value.trim() === "" ? undefined : Number(e.target.value) })} aria-invalid={!!p.error} />
      </Field>
      <InlineJson
        id={`st-${uid}-body`} label={tr("workflows.passo_da_ripetere_json")} value={step.body} rows={7} issues={issues} field="body" onChange={(body) => onChange({ ...step, body })}
        hint={<>Un passo <code>tool</code>, <code>agent</code> o <code>llm</code>. Usa <code>{"{{item}}"}</code>, <code>{"{{item.campo}}"}</code> e <code>{"{{index}}"}</code>.</>}
      />
    </>
  );
}

export function ParallelFields({ step, uid, issues, onChange }: Props<"parallel">) {
  return (
    <InlineJson
      id={`st-${uid}-steps`} label={tr("workflows.passi_da_eseguire_insieme_json")} value={step.steps} rows={10} issues={issues} field="steps" onChange={(steps) => onChange({ ...step, steps })}
      hint={<>Un elenco di passi <code>tool</code>, <code>agent</code> o <code>llm</code>. L'output di ognuno è anche <code>{"{{steps.<id>.output}}"}</code>.</>}
    />
  );
}

export function WorkflowFields({ step, uid, issues, catalog, before, inputKeys, onChange }: Props<"workflow">) {
  const known = catalog.workflows.some((w) => w.id === step.workflow);
  const n = note(issues, "workflow");
  const i = note(issues, "inputs");
  return (
    <>
      <Field label={tr("workflows.workflow_da_avviare")} htmlFor={`st-${uid}-workflow`} error={n.error} hint={n.hint ?? tr("workflows.anche_un_workflow_disattivato_l")}>
        <Select id={`st-${uid}-workflow`} value={step.workflow} onChange={(e) => onChange({ ...step, workflow: e.target.value })} aria-invalid={!!n.error}>
          {!step.workflow && <option value="">{tr("workflows.scegli_un_workflow")}</option>}
          {step.workflow && !known && <option value={step.workflow}>{tr("workflows.non_esiste_4", { workflow: step.workflow })}</option>}
          {catalog.workflows.map((w) => <option key={w.id} value={w.id}>{w.name}</option>)}
        </Select>
      </Field>
      <Field label={tr("workflows.input_json")} htmlFor={`st-${uid}-inputs`} error={i.error} hint={i.hint ?? tr("workflows.un_oggetto_json_i_valori")}>
        <Textarea id={`st-${uid}-inputs`} rows={4} className="mono" spellCheck={false} value={step.inputs} onChange={(e) => onChange({ ...step, inputs: e.target.value })} aria-invalid={!!i.error} />
        <PlaceholderInsert targetId={`st-${uid}-inputs`} value={step.inputs} onChange={(v) => onChange({ ...step, inputs: v })} before={before} inputKeys={inputKeys} quoted />
      </Field>
    </>
  );
}

export function ApprovalFields({ step, uid, issues, before, inputKeys, onChange }: Props<"approval">) {
  const t = note(issues, "timeout_s");
  return (
    <>
      <PromptField id={`st-${uid}-message`} label={tr("workflows.domanda_per_te")} value={step.message} rows={2} placeholder={tr("workflows.sposto_i_file_come_proposto")} issues={issues} field="message" before={before} inputKeys={inputKeys} onChange={(message) => onChange({ ...step, message })} />
      <div className="step-options-grid">
        <Field label={tr("workflows.attendi_al_massimo_s")} htmlFor={`st-${uid}-wait`} error={t.error} hint={tr("workflows.vuoto_aspetta_senza_limite")}>
          <Input
            id={`st-${uid}-wait`} type="number" min={1} max={OPTION_LIMITS.timeout} value={step.timeout_s ?? ""}
            onChange={(e) => onChange({ ...step, timeout_s: e.target.value.trim() === "" ? undefined : Number(e.target.value) })} aria-invalid={!!t.error}
          />
        </Field>
        {step.timeout_s !== undefined && (
          <Field label={tr("workflows.se_nessuno_risponde")} htmlFor={`st-${uid}-default`}>
            <Select id={`st-${uid}-default`} value={step.default ?? "deny"} onChange={(e) => onChange({ ...step, default: e.target.value === "allow" ? "allow" : undefined })}>
              <option value="deny">{tr("workflows.rifiuta_il_workflow_finisce")}</option>
              <option value="allow">{tr("workflows.approva_e_prosegui")}</option>
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
      {others.length === 0 ? <span className="faint text-sm">{tr("workflows.aggiungi_altri_passi_per_sceglierli")}</span> : (
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
        label={tr("workflows.espressione")} htmlFor={`st-${uid}-if`} error={n.error}
        hint={n.hint ?? <>{trNode("workflows.forme_ammesse_oppure_un_valore", { code: <code>A contains B</code>, code2: <code>A == B</code>, code3: <code>A != B</code>, code4: <code>not A</code> })}</>}
      >
        <Input id={`st-${uid}-if`} mono value={step.if} onChange={(e) => onChange({ ...step, if: e.target.value })} placeholder="{{steps.scan.output}} contains pdf" aria-invalid={!!n.error} spellCheck={false} />
        <PlaceholderInsert targetId={`st-${uid}-if`} value={step.if} onChange={(v) => onChange({ ...step, if: v })} before={before} inputKeys={inputKeys} />
      </Field>
      <div className="targets-grid">
        <Targets legend={tr("workflows.se_vero_esegui")} value={step.then} others={others} onToggle={(id, on) => toggle("then", id, on)} />
        <Targets legend={tr("workflows.altrimenti_esegui")} value={step.else} others={others} onToggle={(id, on) => toggle("else", id, on)} />
      </div>
      <p className="faint text-xs">{tr("workflows.i_passi_non_scelti_vengono")}</p>
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
      <summary className="muted text-sm">{tr("workflows.tentativi_limite_di_tempo_ed")}</summary>
      <div className="step-options-grid">
        <Field label={tr("workflows.nuovi_tentativi")} htmlFor={`st-${uid}-retry`} error={err("retry")} hint={`0–${OPTION_LIMITS.retry}`}>
          <Input id={`st-${uid}-retry`} type="number" min={0} max={OPTION_LIMITS.retry} value={o.retry ?? ""} placeholder="0" onChange={(e) => set({ retry: num(e.target.value) })} aria-invalid={!!err("retry")} />
        </Field>
        <Field label={tr("workflows.attesa_tra_i_tentativi_s")} htmlFor={`st-${uid}-delay`} error={err("retry_delay_s")} hint={`0–${OPTION_LIMITS.retryDelay}`}>
          <Input id={`st-${uid}-delay`} type="number" min={0} max={OPTION_LIMITS.retryDelay} value={o.retry_delay_s ?? ""} placeholder={String(DEFAULT_RETRY_DELAY_S)} onChange={(e) => set({ retry_delay_s: num(e.target.value) })} aria-invalid={!!err("retry_delay_s")} />
        </Field>
        <Field label={tr("workflows.limite_di_tempo_s")} htmlFor={`st-${uid}-timeout`} error={err("timeout_s")} hint={tr("workflows.per_ogni_tentativo_vuoto_nessun")}>
          <Input id={`st-${uid}-timeout`} type="number" min={1} max={OPTION_LIMITS.timeout} value={o.timeout_s ?? ""} onChange={(e) => set({ timeout_s: num(e.target.value) })} aria-invalid={!!err("timeout_s")} />
        </Field>
        <Field label={tr("workflows.se_il_passo_fallisce")} htmlFor={`st-${uid}-onerror`} hint="Con «prosegui» l'errore è in {{steps.<id>.error}}">
          <Select id={`st-${uid}-onerror`} value={o.on_error ?? "fail"} onChange={(e) => set({ on_error: e.target.value === "continue" ? "continue" : "fail" })}>
            <option value="fail">{tr("workflows.ferma_il_workflow")}</option>
            <option value="continue">{tr("workflows.prosegui_con_i_passi_successivi")}</option>
          </Select>
        </Field>
      </div>
    </details>
  );
}
