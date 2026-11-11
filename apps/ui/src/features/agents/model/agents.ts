// Agent profiles: identity of the seeded agents, ids, defaults, normalization and the checks made
// before saving. Pure, so it can be tested without a DOM.

import type { AgentProfile, BudgetLimits } from "../../../shared/api/types";
import { slugify as slug, uniqueSlug } from "../../../shared/lib/slug";
import { isAutoProfile } from "../../models/model/adviser";
import { normalizeRoles } from "./roles";
import { ALL, normalizeTools } from "./tools";

/** Seeded on first run by crates/aiw-engine/src/defaults.rs `agents()`; the backend refuses to delete them (409). */
export const BUILTIN_AGENT_IDS: readonly string[] = ["operator", "coder", "browser", "desktop", "planner", "teacher"];

export const isBuiltinAgent = (id: string): boolean => BUILTIN_AGENT_IDS.includes(id);

/** URL segment of an agent that does not exist yet (`#/agents/new`); never used as a real id. */
export const NEW_AGENT = "new";

export const MAX_TURNS_LIMIT = 500;
/** Beyond this many characters the prompt starts to eat a small context window. */
export const LONG_PROMPT_CHARS = 6000;

/** `Revisore di codice` → `revisore-di-codice`. */
export const slugify = (name: string, fallback = "agente"): string => slug(name, fallback);

/** `base`, or `base-2`, `base-3`… when taken; never the reserved `new`. */
export const uniqueId = (base: string, taken: readonly string[]): string => uniqueSlug(base, taken, [NEW_AGENT]);

/** A new agent starts read-only and asks before anything else: the user opens it up as needed. */
export function blankAgent(inferenceProfile: string): AgentProfile {
  return {
    id: "",
    name: "",
    description: "",
    inference_profile: inferenceProfile,
    system_prompt: "",
    tools: ["read_file", "list_dir", "glob", "grep", "todo"],
    mode: "ask",
    sandbox: "workspace-write",
    max_turns: 30,
  };
}

/** `Copia di Coder`, then `Copia di Coder (2)`… */
export function copyName(name: string, existing: readonly string[]): string {
  const base = `Copia di ${name}`;
  const taken = new Set(existing.map((n) => n.toLowerCase()));
  if (!taken.has(base.toLowerCase())) return base;
  for (let n = 2; ; n++) if (!taken.has(`${base} (${n})`.toLowerCase())) return `${base} (${n})`;
}

export function duplicateAgent(src: AgentProfile, existing: readonly AgentProfile[]): AgentProfile {
  return { ...structuredClone(src), id: "", name: copyName(src.name, existing.map((a) => a.name)) };
}

export const BUDGET_KEYS = ["max_tokens", "max_turns", "max_seconds"] as const satisfies readonly (keyof BudgetLimits)[];

/** The budget without empty limits (NaN, undefined); `undefined` when none is left. Values are kept as typed so that the checks can flag them. */
export function cleanBudget(b: BudgetLimits | undefined): BudgetLimits | undefined {
  const out: BudgetLimits = {};
  for (const k of BUDGET_KEYS) {
    const v = b?.[k];
    if (typeof v === "number" && Number.isFinite(v)) out[k] = v;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

/** Form values trimmed and clamped the way the backend will keep them. */
export function normalizeAgent(a: AgentProfile): AgentProfile {
  const turns = Number.isFinite(a.max_turns) ? Math.round(a.max_turns) : 30;
  const { budget: _budget, candidate_profiles: _candidates, roles: _roles, ...rest } = a;
  const budget = cleanBudget(a.budget);
  const candidates = [...new Set(a.candidate_profiles ?? [])];
  const roles = normalizeRoles(a.roles);
  return {
    ...rest,
    ...(budget ? { budget } : {}),
    ...(candidates.length > 0 ? { candidate_profiles: candidates } : {}),
    ...(roles ? { roles } : {}),
    name: a.name.trim(),
    description: a.description.trim(),
    system_prompt: a.system_prompt,
    tools: normalizeTools(a.tools),
    max_turns: Math.min(MAX_TURNS_LIMIT, Math.max(1, turns)),
  };
}

/** The id a new agent is saved under: from its name, unique among `existing`. */
export function idForNew(name: string, existing: readonly AgentProfile[]): string {
  return uniqueId(slugify(name), existing.map((a) => a.id));
}

/**
 * Reads the text of the "response schema" field: empty means no schema; anything else must be a JSON object
 * (the backend refuses other shapes and unknown `type` names with a 400 of its own).
 */
export function parseSchemaText(text: string): { value?: Record<string, unknown>; error?: string } {
  if (!text.trim()) return {};
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch (e) {
    return { error: `JSON non valido: ${(e as Error).message}` };
  }
  if (typeof v !== "object" || v === null || Array.isArray(v)) return { error: "Lo schema deve essere un oggetto JSON, per esempio {\"type\": \"object\"}." };
  return { value: v as Record<string, unknown> };
}

/** The text shown in the field for a stored schema. */
export const schemaToText = (schema: unknown): string => (schema === undefined || schema === null ? "" : JSON.stringify(schema, null, 2));

export type AgentField = "name" | "inference_profile" | "max_turns" | "system_prompt" | "tools" | "mode" | "sandbox" | "budget";

export interface AgentIssue {
  field: AgentField;
  level: "error" | "warn";
  message: string;
}

/** What the checks need to know about the rest of the workspace. */
export interface AgentContext {
  profileIds: readonly string[];
  otherNames: readonly string[];
  /** The list of profiles was loaded. Default: it is not empty (an empty list may just not have arrived). */
  profilesLoaded?: boolean;
  /** Names of the tool catalogue (internal tools included); undefined while it is not loaded, so nothing is flagged. */
  knownTools?: readonly string[];
}

/** Names in `tools` the backend would refuse: not `*`, not an MCP tool (`mcp__…`, the server may be offline), not in the catalogue. */
export function unknownTools(tools: readonly string[], known: readonly string[]): string[] {
  const names = new Set(known);
  return tools.filter((t) => t !== ALL && !t.startsWith("mcp__") && !names.has(t));
}

/**
 * Checks of the agent form. The errors are the rules of `save_agent` (crates/aiw-engine/src/lib.rs): at least one
 * turn, an inference profile that exists, only tools the catalogue knows. The backend answers 400 with the same
 * words if something slips through; the warnings are advice.
 */
export function validateAgent(a: AgentProfile, ctx: AgentContext): AgentIssue[] {
  const issues: AgentIssue[] = [];
  const name = a.name.trim();
  if (!name) issues.push({ field: "name", level: "error", message: "Dai un nome all'agente." });
  else if (ctx.otherNames.some((n) => n.trim().toLowerCase() === name.toLowerCase())) {
    issues.push({ field: "name", level: "warn", message: "Esiste già un altro agente con questo nome." });
  }
  const profilesLoaded = ctx.profilesLoaded ?? ctx.profileIds.length > 0;
  if (!a.inference_profile) issues.push({ field: "inference_profile", level: "error", message: "Scegli un profilo di inferenza." });
  // `auto:<task>` is not a stored profile: the adviser resolves it when a run starts.
  else if (profilesLoaded && !isAutoProfile(a.inference_profile) && !ctx.profileIds.includes(a.inference_profile)) {
    issues.push({ field: "inference_profile", level: "error", message: "Il profilo scelto non esiste più: sceglierne un altro." });
  }
  if (!Number.isFinite(a.max_turns) || a.max_turns < 1 || a.max_turns > MAX_TURNS_LIMIT) {
    issues.push({ field: "max_turns", level: "error", message: `I turni massimi vanno da 1 a ${MAX_TURNS_LIMIT}.` });
  }
  for (const k of BUDGET_KEYS) {
    const v = a.budget?.[k];
    if (v !== undefined && Number.isFinite(v) && (!Number.isInteger(v) || v < 1)) {
      issues.push({ field: "budget", level: "error", message: "I limiti del budget sono numeri interi da 1 in su: lascia vuoto il campo per nessun limite." });
      break;
    }
  }
  const unknown = ctx.knownTools ? unknownTools(a.tools, ctx.knownTools) : [];
  if (unknown.length > 0) {
    issues.push({ field: "tools", level: "error", message: `Strumenti sconosciuti: ${unknown.join(", ")} (usa «*» oppure i nomi elencati in Tool). Toglili dall'elenco per salvare.` });
  }
  if (a.system_prompt.length > LONG_PROMPT_CHARS) {
    issues.push({ field: "system_prompt", level: "warn", message: "Un prompt molto lungo riduce lo spazio di contesto disponibile per la conversazione." });
  }
  if (a.tools.length === 0) {
    issues.push({ field: "tools", level: "warn", message: "Senza tool l'agente può solo rispondere a parole." });
  }
  if (a.mode === "yolo") issues.push({ field: "mode", level: "warn", message: "Senza freni: l'agente non chiede mai conferma." });
  if (a.sandbox === "off") issues.push({ field: "sandbox", level: "warn", message: "Senza contenimento i comandi agiscono con i tuoi privilegi, anche fuori dal workspace." });
  return issues;
}

export const hasErrors = (issues: readonly AgentIssue[]): boolean => issues.some((i) => i.level === "error");

/** Rough token count of a prompt (the backend's own estimate: three characters per token). */
export const estimateTokens = (text: string): number => Math.ceil(text.length / 3);

/** Agents whose inference profile is `profileId`. */
export const agentsUsingProfile = (agents: readonly AgentProfile[], profileId: string): AgentProfile[] => agents.filter((a) => a.inference_profile === profileId);
