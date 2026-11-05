// Steps of a workflow as the visual editor edits them. The shapes follow `WorkflowStep` in
// crates/aiw-core/src/api.rs; the engine (crates/aiw-workflow/src/engine.rs) gives them their meaning:
//  - `{{inputs.x}}`, `{{steps.id.output}}` and `{{now}}` are replaced in args, prompts, messages and conditions;
//  - a condition steers the steps named in `then` / `else`: those steps run only when chosen, and a step
//    that comes before its condition is always skipped;
//  - an approval that is refused ends the run as cancelled; with `timeout_s` it takes `default` (deny or allow) when nobody answers;
//  - `foreach` runs one inline step for every entry of a list (`{{item}}`, `{{item.field}}`, `{{index}}`, up to 8 at a time),
//    `parallel` runs several inline steps together; the inline steps are tool, agent or llm only and are edited as JSON;
//  - `workflow` starts another workflow with inputs and waits for it (3 levels at most);
//  - tool, agent, llm, foreach, parallel and workflow steps take `retry`, `retry_delay_s`, `timeout_s` and `on_error` (see `StepOptions`).

import type { StepOptions, WorkflowStep } from "../../../shared/api/types";

export type StepKind = WorkflowStep["kind"];

export const STEP_KINDS: { kind: StepKind; label: string; icon: string; hint: string }[] = [
  { kind: "tool", label: "Tool", icon: "tool", hint: "Chiama un tool con argomenti fissi, senza il modello." },
  { kind: "agent", label: "Agente", icon: "agent", hint: "Affida un compito a un agente, che può usare i suoi tool." },
  { kind: "llm", label: "LLM", icon: "sparkle", hint: "Una sola risposta del modello di un profilo, senza tool." },
  { kind: "foreach", label: "Per ogni voce", icon: "flow", hint: "Ripete un passo per ogni voce di un elenco, anche più voci alla volta." },
  { kind: "parallel", label: "In parallelo", icon: "flow", hint: "Esegue più passi insieme e aspetta che finiscano tutti." },
  { kind: "workflow", label: "Sotto-workflow", icon: "flow", hint: "Avvia un altro workflow con degli input e ne aspetta la fine." },
  { kind: "approval", label: "Conferma", icon: "shield", hint: "Si ferma e chiede il tuo consenso: se lo neghi il workflow finisce." },
  { kind: "condition", label: "Condizione", icon: "flow", hint: "Sceglie quali passi eseguire in base a un'espressione." },
];

export const stepMeta = (kind: StepKind) => STEP_KINDS.find((k) => k.kind === kind)!;

/** A step with its tool arguments as text, so a half-typed JSON is not lost while it is invalid. */
export type StepDraft =
  | { id: string; kind: "tool"; tool: string; args: string; opts?: StepOptions }
  | { id: string; kind: "agent"; agent: string; prompt: string; opts?: StepOptions }
  | { id: string; kind: "llm"; profile: string; prompt: string; opts?: StepOptions }
  /** `body` is the JSON of the inline step; `parallel` (1..8) how many entries run together, 1 when absent. */
  | { id: string; kind: "foreach"; items: string; body: string; parallel?: number; opts?: StepOptions }
  /** `steps` is the JSON array of the inline steps. */
  | { id: string; kind: "parallel"; steps: string; opts?: StepOptions }
  /** `inputs` is the JSON object of the inputs given to the workflow. */
  | { id: string; kind: "workflow"; workflow: string; inputs: string; opts?: StepOptions }
  /** `timeout_s`: seconds to wait for the answer; `default` is the answer taken when the time is up (deny when absent). */
  | { id: string; kind: "approval"; message: string; timeout_s?: number; default?: "allow" }
  | { id: string; kind: "condition"; if: string; then: string[]; else: string[] };

export interface StepDefaults {
  workflow?: string;
  agent?: string;
  profile?: string;
  tool?: string;
}

/** Step ids go into `{{steps.<id>.output}}`, so they are limited to letters, digits, `_` and `-`. */
export const STEP_ID = /^[A-Za-z0-9_-]+$/;

/** `tool1`, `tool2`… the first free id for the kind. */
export function freshStepId(kind: StepKind, steps: readonly Pick<StepDraft, "id">[]): string {
  const used = new Set(steps.map((s) => s.id));
  for (let n = 1; ; n++) if (!used.has(`${kind}${n}`)) return `${kind}${n}`;
}

/** An inline step as the editable text of `foreach` and `parallel`. */
export const inlineJson = (v: unknown): string => JSON.stringify(v, null, 2);

export const FOREACH_MAX_PARALLEL = 8;
export const INLINE_KINDS = ["tool", "agent", "llm"];

/** The inline steps in the JSON of a `foreach` (`one`) or `parallel` (a list), or what is wrong with it. */
export function parseInline(text: string, many: boolean): { ok: true; steps: WorkflowStep[] } | { ok: false; error: string } {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    return { ok: false, error: "Non è un JSON valido." };
  }
  const list = many ? v : [v];
  if (!Array.isArray(list) || (many && list.length === 0)) return { ok: false, error: many ? "Serve un elenco con almeno un passo." : "Serve un oggetto JSON." };
  for (const s of list) {
    const o = s as { id?: unknown; kind?: unknown } | null;
    if (o === null || typeof o !== "object" || Array.isArray(o) || typeof o.id !== "string" || !o.id.trim()) return { ok: false, error: "Ogni passo ha bisogno di un «id»." };
    if (typeof o.kind !== "string" || !INLINE_KINDS.includes(o.kind)) return { ok: false, error: `Il passo «${o.id}» deve essere di tipo ${INLINE_KINDS.join(", ")} (niente annidamento, conferme o condizioni).` };
    if (!STEP_ID.test(o.id)) return { ok: false, error: `L'id «${o.id}» può contenere solo lettere, cifre, «_» e «-».` };
  }
  return { ok: true, steps: list as WorkflowStep[] };
}

/** Ids of the inline steps of a draft, when their JSON can be read. */
export function innerIds(s: StepDraft): string[] {
  if (s.kind !== "foreach" && s.kind !== "parallel") return [];
  const r = parseInline(s.kind === "foreach" ? s.body : s.steps, s.kind === "parallel");
  return r.ok ? r.steps.map((x) => x.id) : [];
}

export function newStep(kind: StepKind, steps: readonly StepDraft[], d: StepDefaults = {}): StepDraft {
  const id = freshStepId(kind, steps);
  switch (kind) {
    case "tool": return { id, kind, tool: d.tool ?? "", args: "{}" };
    case "agent": return { id, kind, agent: d.agent ?? "", prompt: "" };
    case "llm": return { id, kind, profile: d.profile ?? "", prompt: "" };
    case "foreach": return { id, kind, items: "", body: inlineJson({ id: `${id}-step`, kind: "llm", profile: d.profile ?? "", prompt: "{{item}}" }) };
    case "parallel": return { id, kind, steps: JSON.stringify([{ id: `${id}-a`, kind: "llm", profile: d.profile ?? "", prompt: "" }], null, 2) };
    case "workflow": return { id, kind, workflow: d.workflow ?? "", inputs: "{}" };
    case "approval": return { id, kind, message: "Procedo?" };
    case "condition": return { id, kind, if: "", then: [], else: [] };
  }
}

/** Same id, another kind: the fields of the new kind start empty; the options carry over between kinds that have them. */
export function changeKind(step: StepDraft, kind: StepKind, steps: readonly StepDraft[], d: StepDefaults = {}): StepDraft {
  if (step.kind === kind) return step;
  const fresh = newStep(kind, steps, d);
  const opts = hasOptions(step) && hasOptions(fresh) ? step.opts : undefined;
  return { ...fresh, id: step.id, ...(opts ? { opts } : {}) } as StepDraft;
}

// ---------------------------------------------------------------- retry / timeout / on_error

export const OPTION_LIMITS = { retry: 5, retryDelay: 300, timeout: 86_400 } as const;
export const DEFAULT_RETRY_DELAY_S = 2;

/** The steps that take `StepOptions`. */
export const hasOptions = (s: StepDraft): s is Extract<StepDraft, { kind: "tool" | "agent" | "llm" | "foreach" | "parallel" | "workflow" }> =>
  s.kind === "tool" || s.kind === "agent" || s.kind === "llm" || s.kind === "foreach" || s.kind === "parallel" || s.kind === "workflow";

/** Options without the defaults (what the backend does not serialize either); `undefined` when nothing is left. */
export function cleanOptions(o: StepOptions | undefined): StepOptions | undefined {
  if (!o) return undefined;
  const out: StepOptions = {};
  if (o.retry) out.retry = o.retry;
  if (o.retry_delay_s !== undefined && o.retry_delay_s !== DEFAULT_RETRY_DELAY_S) out.retry_delay_s = o.retry_delay_s;
  if (o.timeout_s) out.timeout_s = o.timeout_s;
  if (o.on_error === "continue") out.on_error = "continue";
  return Object.keys(out).length ? out : undefined;
}

/** The options of a wire-form step. */
export function optionsOf(s: StepOptions): StepOptions | undefined {
  return cleanOptions({ retry: s.retry, retry_delay_s: s.retry_delay_s, timeout_s: s.timeout_s, on_error: s.on_error });
}

/** Problem with the time limit of an approval, or null. */
export function approvalTimeoutProblem(t: number | undefined): string | null {
  if (t === undefined) return null;
  return Number.isInteger(t) && t >= 1 && t <= OPTION_LIMITS.timeout ? null : `Il tempo di attesa va da 1 a ${OPTION_LIMITS.timeout} secondi.`;
}

/** First problem with the option values, or null. */
export function optionsProblem(o: StepOptions | undefined): { field: string; message: string } | null {
  if (!o) return null;
  const bad = (n: number | undefined, min: number, max: number) => n !== undefined && (!Number.isInteger(n) || n < min || n > max);
  if (bad(o.retry, 0, OPTION_LIMITS.retry)) return { field: "retry", message: `I tentativi extra vanno da 0 a ${OPTION_LIMITS.retry}.` };
  if (bad(o.retry_delay_s, 0, OPTION_LIMITS.retryDelay)) return { field: "retry_delay_s", message: `L'attesa tra i tentativi va da 0 a ${OPTION_LIMITS.retryDelay} secondi.` };
  if (bad(o.timeout_s, 1, OPTION_LIMITS.timeout)) return { field: "timeout_s", message: `Il limite di tempo va da 1 a ${OPTION_LIMITS.timeout} secondi.` };
  return null;
}

/** Short text for the options, empty when there are none. */
export function optionsSummary(o: StepOptions | undefined): string {
  const c = cleanOptions(o);
  if (!c) return "";
  const parts: string[] = [];
  if (c.retry) parts.push(`${c.retry} ${c.retry === 1 ? "nuovo tentativo" : "nuovi tentativi"}`);
  if (c.timeout_s) parts.push(`limite ${c.timeout_s} s`);
  if (c.on_error === "continue") parts.push("prosegue se fallisce");
  return parts.join(" · ");
}

const PLACEHOLDER = /\{\{\s*([a-zA-Z0-9_.-]+)\s*\}\}/g;

/** Id of the step a placeholder path refers to (`steps.scan.output` → `scan`), or null. */
export function stepRef(path: string): string | null {
  if (!path.startsWith("steps.")) return null;
  return path.slice("steps.".length).replace(/(\.output)+$/, "");
}

export interface Placeholder {
  path: string;
  /** `inputs`, `steps`, `now` or something the engine leaves untouched. */
  root: string;
}

export function placeholders(text: string): Placeholder[] {
  return [...text.matchAll(PLACEHOLDER)].map((m) => ({ path: m[1], root: m[1].split(".")[0] }));
}

/** Every templated string of a step. */
export function stepTexts(s: StepDraft): string[] {
  switch (s.kind) {
    case "tool": return [s.args];
    case "agent":
    case "llm": return [s.prompt];
    case "foreach": return [s.items, s.body];
    case "parallel": return [s.steps];
    case "workflow": return [s.inputs];
    case "approval": return [s.message];
    case "condition": return [s.if];
  }
}

function mapTexts(s: StepDraft, fn: (t: string) => string): StepDraft {
  switch (s.kind) {
    case "tool": return { ...s, args: fn(s.args) };
    case "agent": return { ...s, prompt: fn(s.prompt) };
    case "llm": return { ...s, prompt: fn(s.prompt) };
    case "foreach": return { ...s, items: fn(s.items), body: fn(s.body) };
    case "parallel": return { ...s, steps: fn(s.steps) };
    case "workflow": return { ...s, inputs: fn(s.inputs) };
    case "approval": return { ...s, message: fn(s.message) };
    case "condition": return { ...s, if: fn(s.if) };
  }
}

/** Renames a step: `then`/`else` lists and `{{steps.old.output}}` placeholders of every step follow. Length is kept. */
export function renameReferences(steps: readonly StepDraft[], from: string, to: string): StepDraft[] {
  const rewrite = (text: string) =>
    text.replace(PLACEHOLDER, (m, path: string) => (stepRef(path) === from ? `{{steps.${to}${/\.output$/.test(path) ? ".output" : ""}}}` : m));
  return steps.map((s) => {
    const t = mapTexts(s, rewrite);
    return t.kind === "condition" ? { ...t, then: t.then.map((x) => (x === from ? to : x)), else: t.else.map((x) => (x === from ? to : x)) } : t;
  });
}

/** Drops a step id from every `then`/`else` list (before the step itself is removed). Length is kept. */
export function dropReferences(steps: readonly StepDraft[], id: string): StepDraft[] {
  return steps.map((s) => (s.kind === "condition" ? { ...s, then: s.then.filter((x) => x !== id), else: s.else.filter((x) => x !== id) } : s));
}

/** Steps a condition sends control to, in the order of the list. */
export function gatedIds(steps: readonly StepDraft[]): Set<string> {
  const out = new Set<string>();
  for (const s of steps) if (s.kind === "condition") for (const id of [...s.then, ...s.else]) out.add(id);
  return out;
}

/** One line that says what the step does, for the collapsed card. */
export function stepSummary(s: StepDraft): string {
  switch (s.kind) {
    case "tool": return `${s.tool || "tool da scegliere"} ${s.args.replace(/\s+/g, " ").trim()}`.trim();
    case "agent": return `${s.agent || "agente da scegliere"}: ${s.prompt}`;
    case "llm": return `${s.profile || "profilo da scegliere"}: ${s.prompt}`;
    case "foreach": return `per ogni voce di ${s.items.replace(/\s+/g, " ").trim() || "…"}${s.parallel && s.parallel > 1 ? ` (${s.parallel} alla volta)` : ""}`;
    case "parallel": {
      const ids = innerIds(s);
      return ids.length ? `in parallelo: ${ids.join(", ")}` : "in parallelo (passi da correggere)";
    }
    case "workflow": return `${s.workflow || "workflow da scegliere"} ${s.inputs.replace(/\s+/g, " ").trim()}`.trim();
    case "approval": return s.timeout_s ? `${s.message} (dopo ${s.timeout_s} s: ${s.default === "allow" ? "approva" : "rifiuta"})` : s.message;
    case "condition": return `se ${s.if || "…"} → ${s.then.join(", ") || "nessun passo"}${s.else.length ? ` altrimenti ${s.else.join(", ")}` : ""}`;
  }
}

/** Parameter names of a tool from its JSON Schema (`properties`, with `required` marked); empty when unknown. */
export function schemaParams(schema: unknown): { name: string; required: boolean }[] {
  if (schema === null || typeof schema !== "object") return [];
  const s = schema as { properties?: unknown; required?: unknown };
  if (s.properties === null || typeof s.properties !== "object") return [];
  const required = new Set(Array.isArray(s.required) ? s.required.filter((r): r is string => typeof r === "string") : []);
  return Object.keys(s.properties).map((name) => ({ name, required: required.has(name) }));
}
