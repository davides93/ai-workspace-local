// The editable form of a workflow (`PlainDraft`), its conversion from and to `WorkflowDef`, and the checks
// made before saving. Mirrors `aiw_workflow::validate` (crates/aiw-workflow/src/lib.rs) and adds the
// mistakes the engine would only reveal at run time.

import { keyed, unkeyed, type Keyed } from "../../../shared/lib/keyedList";
import type { WorkflowDef, WorkflowStep, WorkflowTrigger } from "../../../shared/api/types";
import { parseCron } from "./cron";
import { approvalTimeoutProblem, cleanOptions, FOREACH_MAX_PARALLEL, hasOptions, inlineJson, innerIds, optionsOf, optionsProblem, parseInline, isKnownTail, placeholders, STEP_ID, stepRef, stepTail, stepTexts, type StepDraft } from "./steps";
import { joinInterval, MIN_INTERVAL_S, splitInterval, type IntervalUnit } from "./triggers";

import { t as tr } from "../../../shared/i18n";
export type TriggerDraft =
  | { kind: "manual" }
  | { kind: "interval"; amount: number; unit: IntervalUnit }
  | { kind: "cron"; expr: string }
  | { kind: "file"; path: string }
  /** The secret is never edited here: the backend generates it and `defFromPlain` carries the saved one. */
  | { kind: "webhook" };

/** A default input of the workflow; `json` keeps numbers, booleans and objects as they were. */
export interface InputRow {
  key: string;
  value: string;
  json: boolean;
}

export interface PlainDraft {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  trigger: TriggerDraft;
  inputs: InputRow[];
  steps: StepDraft[];
}

const pretty = (v: unknown) => JSON.stringify(v, null, 2);

export function triggerToDraft(t: WorkflowTrigger): TriggerDraft {
  switch (t.kind) {
    case "manual": return { kind: "manual" };
    case "interval": return { kind: "interval", ...splitInterval(t.seconds) };
    case "cron": return { kind: "cron", expr: t.expr };
    case "file": return { kind: "file", path: t.path };
    case "webhook": return { kind: "webhook" };
  }
}

export function triggerFromDraft(t: TriggerDraft, secret = ""): WorkflowTrigger {
  switch (t.kind) {
    case "manual": return { kind: "manual" };
    case "interval": return { kind: "interval", seconds: joinInterval(t.amount, t.unit) };
    case "cron": return { kind: "cron", expr: t.expr.trim() };
    case "file": return { kind: "file", path: t.path.trim() };
    case "webhook": return secret ? { kind: "webhook", secret } : { kind: "webhook" };
  }
}

function stepToDraft(s: WorkflowStep): StepDraft {
  const opts = s.kind === "tool" || s.kind === "agent" || s.kind === "team" || s.kind === "llm" || s.kind === "foreach" || s.kind === "parallel" || s.kind === "workflow" ? optionsOf(s) : undefined;
  const o = opts ? { opts } : {};
  switch (s.kind) {
    case "tool": return { id: s.id, kind: "tool", tool: s.tool, args: pretty(s.args ?? {}), ...o };
    case "agent": return { id: s.id, kind: "agent", agent: s.agent, prompt: s.prompt, ...o };
    case "team": return { id: s.id, kind: "team", team: s.team, prompt: s.prompt, ...o };
    case "llm": return { id: s.id, kind: "llm", profile: s.profile, prompt: s.prompt, ...o };
    case "foreach": return { id: s.id, kind: "foreach", items: s.items, body: inlineJson(s.do), ...(s.parallel !== undefined && s.parallel !== 1 ? { parallel: s.parallel } : {}), ...o };
    case "parallel": return { id: s.id, kind: "parallel", steps: inlineJson(s.steps), ...o };
    case "workflow": return { id: s.id, kind: "workflow", workflow: s.workflow, inputs: pretty(s.inputs ?? {}), ...o };
    case "approval": return { id: s.id, kind: "approval", message: s.message, ...(s.timeout_s ? { timeout_s: s.timeout_s } : {}), ...(s.timeout_s && s.default === "allow" ? { default: "allow" as const } : {}) };
    case "condition": return { id: s.id, kind: "condition", if: s.if, then: [...s.then], else: [...(s.else ?? [])] };
  }
}

/** Parses the argument text of a tool step; `null` when it is not a JSON object. */
export function parseArgs(text: string): Record<string, unknown> | null {
  if (text.trim() === "") return {};
  try {
    const v: unknown = JSON.parse(text);
    return v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

function stepFromDraft(s: StepDraft): WorkflowStep {
  const o = hasOptions(s) ? cleanOptions(s.opts) ?? {} : {};
  switch (s.kind) {
    case "tool": return { id: s.id, kind: "tool", tool: s.tool, args: parseArgs(s.args) ?? {}, ...o };
    case "agent": return { id: s.id, kind: "agent", agent: s.agent, prompt: s.prompt, ...o };
    case "team": return { id: s.id, kind: "team", team: s.team, prompt: s.prompt, ...o };
    case "llm": return { id: s.id, kind: "llm", profile: s.profile, prompt: s.prompt, ...o };
    case "foreach": {
      const inner = parseInline(s.body, false);
      return { id: s.id, kind: "foreach", items: s.items, do: inner.ok ? inner.steps[0] : { id: "", kind: "llm", profile: "", prompt: "" }, ...(s.parallel !== undefined && s.parallel !== 1 ? { parallel: s.parallel } : {}), ...o };
    }
    case "workflow": {
      const inputs = parseArgs(s.inputs) ?? {};
      return { id: s.id, kind: "workflow", workflow: s.workflow, ...(Object.keys(inputs).length ? { inputs } : {}), ...o };
    }
    case "parallel": {
      const inner = parseInline(s.steps, true);
      return { id: s.id, kind: "parallel", steps: inner.ok ? inner.steps : [], ...o };
    }
    case "approval": return { id: s.id, kind: "approval", message: s.message, ...(s.timeout_s ? { timeout_s: s.timeout_s } : {}), ...(s.timeout_s && s.default === "allow" ? { default: "allow" as const } : {}) };
    case "condition": return s.else.length ? { id: s.id, kind: "condition", if: s.if, then: s.then, else: s.else } : { id: s.id, kind: "condition", if: s.if, then: s.then };
  }
}

export function plainFromDef(def: WorkflowDef): PlainDraft {
  return {
    id: def.id,
    name: def.name,
    description: def.description ?? "",
    enabled: def.enabled,
    trigger: triggerToDraft(def.trigger),
    inputs: Object.entries(def.inputs ?? {}).map(([key, v]) => (typeof v === "string" ? { key, value: v, json: false } : { key, value: JSON.stringify(v), json: true })),
    steps: def.steps.map(stepToDraft),
  };
}

/** Back to the wire form: trimmed, empty description left out, inputs with a blank key dropped. `secret` is the saved webhook secret. */
export function defFromPlain(d: PlainDraft, secret = ""): WorkflowDef {
  const inputs: Record<string, unknown> = {};
  for (const r of d.inputs) {
    const key = r.key.trim();
    if (!key) continue;
    if (!r.json) inputs[key] = r.value;
    else {
      try {
        inputs[key] = JSON.parse(r.value);
      } catch {
        inputs[key] = r.value;
      }
    }
  }
  const description = d.description.trim();
  return {
    id: d.id,
    name: d.name.trim(),
    ...(description ? { description } : {}),
    enabled: d.enabled,
    trigger: triggerFromDraft(d.trigger, secret),
    inputs,
    steps: d.steps.map(stepFromDraft),
  };
}

export function blankWorkflow(id: string, d: { agent?: string } = {}): PlainDraft {
  return {
    id, name: "", description: "", enabled: false, trigger: { kind: "manual" }, inputs: [],
    steps: [{ id: "step1", kind: "agent", agent: d.agent ?? "", prompt: "" }],
  };
}

/** Another workflow's draft as the starting point of a copy. */
export function copyOf(src: PlainDraft, id: string, existingNames: readonly string[]): PlainDraft {
  const base = `Copia di ${src.name}`;
  const taken = new Set(existingNames.map((n) => n.toLowerCase()));
  let name = base;
  for (let n = 2; taken.has(name.toLowerCase()); n++) name = `${base} (${n})`;
  return { ...structuredClone(src), id, name, enabled: false };
}

// ---------------------------------------------------------------- rows with stable ids

/** The draft with a stable uid per input row and per step, so a row keeps its fields and focus when moved. */
export type KeyedDraft = Omit<PlainDraft, "inputs" | "steps"> & { inputs: Keyed<InputRow>[]; steps: Keyed<StepDraft>[] };

export const keyDraft = (d: PlainDraft): KeyedDraft => ({ ...d, inputs: keyed(d.inputs), steps: keyed(d.steps) });
export const unkeyDraft = (d: KeyedDraft): PlainDraft => ({ ...d, inputs: unkeyed(d.inputs), steps: unkeyed(d.steps) });

// ---------------------------------------------------------------- JSON view

const MASK = "••••••••";

/** JSON shown in the preview and the "Avanzato" tab: the webhook secret is masked. */
export function previewJson(d: PlainDraft): string {
  const def = defFromPlain(d, "x");
  const shown = def.trigger.kind === "webhook" ? { ...def, trigger: { kind: "webhook" as const, secret: MASK } } : def;
  return JSON.stringify(shown, null, 2);
}

const KINDS = ["tool", "agent", "llm", "foreach", "parallel", "workflow", "approval", "condition"];
const TRIGGERS = ["manual", "interval", "cron", "file", "webhook"];
const isObj = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === "object" && !Array.isArray(v);
const isStr = (v: unknown): v is string => typeof v === "string";
const isStrList = (v: unknown): v is string[] => Array.isArray(v) && v.every(isStr);

/** Reads the JSON of the "Avanzato" tab into a draft; the id of the workflow cannot change. */
export function parseWorkflowJson(text: string, id: string): { ok: true; draft: PlainDraft } | { ok: false; error: string } {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch (e) {
    return { ok: false, error: `JSON non valido: ${e instanceof Error ? e.message : "errore di sintassi"}` };
  }
  if (!isObj(v)) return { ok: false, error: "La definizione deve essere un oggetto JSON." };
  if (v.id !== id) return { ok: false, error: `L'id del workflow non si cambia da qui: deve restare «${id}».` };
  if (!isStr(v.name)) return { ok: false, error: "Manca il nome («name»)." };
  const t = v.trigger;
  if (!isObj(t) || !isStr(t.kind) || !TRIGGERS.includes(t.kind)) return { ok: false, error: `Il trigger deve avere «kind» tra: ${TRIGGERS.join(", ")}.` };
  if (t.kind === "interval" && typeof t.seconds !== "number") return { ok: false, error: "Il trigger «interval» richiede «seconds» (numero)." };
  if (t.kind === "cron" && !isStr(t.expr)) return { ok: false, error: "Il trigger «cron» richiede «expr» (testo)." };
  if (t.kind === "file" && !isStr(t.path)) return { ok: false, error: "Il trigger «file» richiede «path» (testo)." };
  if (!Array.isArray(v.steps)) return { ok: false, error: "«steps» deve essere un elenco." };
  for (const [i, s] of v.steps.entries()) {
    const at = `Passo ${i + 1}`;
    if (!isObj(s) || !isStr(s.id) || !isStr(s.kind) || !KINDS.includes(s.kind)) return { ok: false, error: `${at}: servono «id» e «kind» tra: ${KINDS.join(", ")}.` };
    const need = (k: string, ok: boolean) => (ok ? null : `${at} (${s.kind}): campo «${k}» mancante o non valido.`);
    const err =
      s.kind === "tool" ? need("tool", isStr(s.tool)) ?? need("args", s.args === undefined || isObj(s.args))
      : s.kind === "agent" ? need("agent", isStr(s.agent)) ?? need("prompt", isStr(s.prompt))
      : s.kind === "llm" ? need("profile", isStr(s.profile)) ?? need("prompt", isStr(s.prompt))
      : s.kind === "foreach" ? need("items", isStr(s.items)) ?? need("do", isObj(s.do))
      : s.kind === "parallel" ? need("steps", Array.isArray(s.steps))
      : s.kind === "workflow" ? need("workflow", isStr(s.workflow)) ?? need("inputs", s.inputs === undefined || isObj(s.inputs))
      : s.kind === "approval" ? need("message", isStr(s.message))
      : need("if", isStr(s.if)) ?? need("then", isStrList(s.then)) ?? need("else", s.else === undefined || isStrList(s.else));
    if (err) return { ok: false, error: err };
  }
  if (v.inputs !== undefined && !isObj(v.inputs)) return { ok: false, error: "«inputs» deve essere un oggetto." };
  const def = { ...v, description: isStr(v.description) ? v.description : undefined, enabled: v.enabled === true, inputs: v.inputs ?? {} } as unknown as WorkflowDef;
  return { ok: true, draft: plainFromDef(def) };
}

// ---------------------------------------------------------------- validation

export interface Issue {
  scope: "meta" | "trigger" | "inputs" | "steps" | "step";
  /** Index of the step for `scope: "step"`. */
  step?: number;
  field?: string;
  level: "error" | "warn";
  message: string;
}

export interface ValidationContext {
  agents: readonly string[];
  /** Ids of the teams a `team` step can run; empty or absent = not known. */
  teams?: readonly string[];
  profiles: readonly string[];
  tools: readonly string[];
  /** Ids of the workflows a `workflow` step can start; empty or absent = not known. */
  workflows?: readonly string[];
  /** Id of the workflow being edited: it cannot start itself. */
  selfId?: string;
}

export function validateDraft(d: PlainDraft, ctx: ValidationContext): Issue[] {
  const out: Issue[] = [];
  const add = (i: Issue) => out.push(i);
  if (!d.name.trim()) add({ scope: "meta", field: "name", level: "error", message: tr("workflows.dai_un_nome_al_workflow") });

  // Trigger
  const t = d.trigger;
  if (t.kind === "cron") {
    const r = parseCron(t.expr);
    if (!r.ok) add({ scope: "trigger", field: "expr", level: "error", message: r.error });
  } else if (t.kind === "interval") {
    if (!Number.isFinite(t.amount) || t.amount <= 0) add({ scope: "trigger", field: "amount", level: "error", message: tr("workflows.indica_un_intervallo_maggiore_di") });
    else if (joinInterval(t.amount, t.unit) < MIN_INTERVAL_S) add({ scope: "trigger", field: "amount", level: "warn", message: tr("workflows.il_servizio_non_esegue_piu", { MIN_INTERVAL_S, MIN_INTERVAL_S2: MIN_INTERVAL_S }) });
  } else if (t.kind === "file" && !t.path.trim()) {
    add({ scope: "trigger", field: "path", level: "error", message: tr("workflows.indica_il_file_o_la") });
  }

  // Inputs
  const keys = new Map<string, number>();
  d.inputs.forEach((r, i) => {
    const key = r.key.trim();
    if (!key) return;
    if (!/^[A-Za-z0-9_-]+$/.test(key)) add({ scope: "inputs", step: i, level: "error", message: tr("workflows.usa_solo_lettere_cifre_e", { key }) });
    if (keys.has(key)) add({ scope: "inputs", step: i, level: "error", message: tr("workflows.l_input_e_ripetuto", { key }) });
    keys.set(key, i);
    if (r.json) {
      try {
        JSON.parse(r.value);
      } catch {
        add({ scope: "inputs", step: i, level: "error", message: tr("workflows.il_valore_di_non_e", { key }) });
      }
    }
  });

  // Steps
  if (d.steps.length === 0) add({ scope: "steps", level: "warn", message: tr("workflows.il_workflow_non_ha_passi") });
  const index = new Map<string, number>();
  d.steps.forEach((s, i) => {
    for (const inner of innerIds(s)) {
      if (index.has(inner)) add({ scope: "step", step: i, field: s.kind === "foreach" ? "body" : "steps", level: "error", message: tr("workflows.identificativo_duplicato_e_gia_usato", { inner, p: index.get(inner)! + 1 }) });
      else index.set(inner, i);
    }
    if (!s.id.trim()) add({ scope: "step", step: i, field: "id", level: "error", message: tr("workflows.il_passo_ha_bisogno_di") });
    else if (!STEP_ID.test(s.id)) add({ scope: "step", step: i, field: "id", level: "error", message: tr("workflows.l_identificativo_puo_contenere_solo") });
    else if (index.has(s.id)) add({ scope: "step", step: i, field: "id", level: "error", message: tr("workflows.identificativo_duplicato_e_gia_usato_2", { id: s.id, p: index.get(s.id)! + 1 }) });
    if (!index.has(s.id)) index.set(s.id, i);
  });
  d.steps.forEach((s, i) => {
    const e = (field: string, message: string) => add({ scope: "step", step: i, field, level: "error", message });
    const w = (field: string, message: string) => add({ scope: "step", step: i, field, level: "warn", message });
    switch (s.kind) {
      case "tool":
        if (!s.tool.trim()) e("tool", "Scegli il tool da chiamare.");
        else if (ctx.tools.length > 0 && !ctx.tools.includes(s.tool)) w("tool", `Il tool «${s.tool}» non è nel catalogo: il passo fallirà se non esiste.`);
        if (parseArgs(s.args) === null) e("args", "Gli argomenti devono essere un oggetto JSON, per esempio {\"path\": \"~/Downloads\"}.");
        break;
      case "agent":
        if (!s.agent.trim()) e("agent", "Scegli l'agente.");
        else if (ctx.agents.length > 0 && !ctx.agents.includes(s.agent)) w("agent", `L'agente «${s.agent}» non esiste: il passo fallirà.`);
        if (!s.prompt.trim()) e("prompt", "Scrivi il compito per l'agente.");
        break;
      case "team":
        if (!s.team.trim()) e("team", "Scegli il team.");
        else if (ctx.teams && ctx.teams.length > 0 && !ctx.teams.includes(s.team)) w("team", `Il team «${s.team}» non esiste: il passo fallirà.`);
        if (!s.prompt.trim()) e("prompt", "Scrivi il compito per il team.");
        break;
      case "llm":
        if (!s.profile.trim()) e("profile", "Scegli il profilo di inferenza.");
        else if (ctx.profiles.length > 0 && !ctx.profiles.includes(s.profile)) w("profile", `Il profilo «${s.profile}» non esiste: il passo fallirà.`);
        if (!s.prompt.trim()) e("prompt", "Scrivi il prompt.");
        break;
      case "foreach": {
        if (!s.items.trim()) e("items", "Indica l'elenco da scorrere: un array JSON o un testo con una voce per riga.");
        if (s.parallel !== undefined && (!Number.isInteger(s.parallel) || s.parallel < 1 || s.parallel > FOREACH_MAX_PARALLEL)) e("parallel", `Le voci in parallelo vanno da 1 a ${FOREACH_MAX_PARALLEL}.`);
        const r = parseInline(s.body, false);
        if (!r.ok) e("body", r.error);
        else if (r.steps[0].kind === "agent" && ctx.agents.length > 0 && !ctx.agents.includes(r.steps[0].agent)) w("body", `L'agente «${r.steps[0].agent}» non esiste: il passo fallirà.`);
        else if (r.steps[0].kind === "team" && ctx.teams && ctx.teams.length > 0 && !ctx.teams.includes(r.steps[0].team)) w("body", `Il team «${r.steps[0].team}» non esiste: il passo fallirà.`);
        break;
      }
      case "parallel": {
        const r = parseInline(s.steps, true);
        if (!r.ok) e("steps", r.error);
        break;
      }
      case "workflow":
        if (!s.workflow.trim()) e("workflow", "Scegli il workflow da avviare.");
        else if (ctx.selfId !== undefined && s.workflow === ctx.selfId) e("workflow", "Un workflow non può avviare se stesso.");
        else if (ctx.workflows && ctx.workflows.length > 0 && !ctx.workflows.includes(s.workflow)) w("workflow", `Il workflow «${s.workflow}» non esiste: il passo fallirà.`);
        if (parseArgs(s.inputs) === null) e("inputs", "Gli input devono essere un oggetto JSON, per esempio {\"cartella\": \"{{inputs.folder}}\"}.");
        break;
      case "approval":
        if (!s.message.trim()) e("message", "Scrivi la domanda da porre.");
        {
          const p = approvalTimeoutProblem(s.timeout_s);
          if (p) e("timeout_s", p);
        }
        break;
      case "condition":
        if (!s.if.trim()) e("if", "Scrivi l'espressione da valutare.");
        for (const id of [...s.then, ...s.else]) {
          const at = index.get(id);
          if (at === undefined) e("then", `La condizione punta a un passo inesistente: «${id}».`);
          else if (at <= i) w("then", `Il passo «${id}» viene prima della condizione: sarà sempre saltato.`);
        }
        if (s.then.length === 0 && s.else.length === 0) w("then", "Nessun passo scelto: la condizione non cambia nulla.");
        break;
    }
    if (hasOptions(s)) {
      const p = optionsProblem(s.opts);
      if (p) e(p.field, p.message);
    }
    // Placeholders: what the engine would leave empty at run time.
    for (const text of stepTexts(s)) {
      for (const p of placeholders(text)) {
        if (p.root === "steps") {
          const ref = stepRef(p.path);
          const at = ref === null ? undefined : index.get(ref);
          if (at === undefined) w("placeholder", `{{${p.path}}} non corrisponde a nessun passo.`);
          else if (at >= i) w("placeholder", `{{${p.path}}} legge un passo che non è ancora stato eseguito a questo punto.`);
          else if (!isKnownTail(stepTail(p.path))) w("placeholder", `{{${p.path}}} non è un segnaposto noto: dopo l'id del passo usa output, error o json.`);
        } else if (p.root === "inputs") {
          const key = p.path.slice("inputs.".length);
          if (d.trigger.kind !== "webhook" && key !== "payload" && !keys.has(key)) w("placeholder", `L'input «${key}» non è definito: resterà vuoto.`);
        } else if (s.kind === "foreach" && (p.root === "item" || p.path === "index")) {
          // Bound by the loop.
        } else if (p.path !== "now") {
          w("placeholder", `{{${p.path}}} non è un segnaposto noto: resterà com'è.`);
        }
      }
    }
  });
  return out;
}

export const hasErrors = (issues: readonly Issue[]): boolean => issues.some((i) => i.level === "error");
