import type { AgentProfile, CatalogModel, Fit, InferenceProfile, LocalModel, Recommendations, RuntimeInfo } from "../../../shared/api/types";
import { catalogForChoice, isAutoProfile } from "../../models/model/adviser";
import { applyModelToProfile, newProfileFor, pickRuntime } from "../../models/model/profile";

import { t, t as tr } from "../../../shared/i18n";
export type ChecklistId = "runtime" | "model" | "profile" | "chat";

export interface ChecklistStep {
  id: ChecklistId;
  label: string;
  done: boolean;
}

/** Runtime that can serve requests now or on demand (`running`, or detected and startable). */
export const isUsableRuntime = (r: RuntimeInfo): boolean => r.status === "running" || r.status === "available";

/** A runtime the "Avvia" action can start: detected, not running (a failed one may be retried). */
export const isStartableRuntime = (r: RuntimeInfo): boolean => r.status === "available" || r.status === "stopped" || r.status === "error";

/**
 * Fallback of the first-run recommendation, for a server without the model adviser: the largest
 * model that fits comfortably, not yet installed, downloadable.
 */
export function recommendedModel(catalog: CatalogModel[]): CatalogModel | undefined {
  const candidates = catalog.filter((c) => c.downloadable !== false && !c.installed && c.fit === "great");
  return candidates.sort((a, b) => b.size_bytes - a.size_bytes)[0] ?? catalog.find((c) => c.downloadable !== false && !c.installed && c.fit === "ok");
}

/** The default agent (config `agent.default`, else the first agent). */
export const defaultAgent = (agents: AgentProfile[], defaultAgentId?: unknown): AgentProfile | undefined =>
  agents.find((a) => a.id === defaultAgentId) ?? agents[0];

/** Profile of the default agent. An `auto:*` profile is not stored: it is not in the list, and the first profile stands in. */
export function defaultProfile(agents: AgentProfile[], profiles: InferenceProfile[], defaultAgentId?: unknown): InferenceProfile | undefined {
  const agent = defaultAgent(agents, defaultAgentId);
  return profiles.find((p) => p.id === agent?.inference_profile) ?? profiles[0];
}

/** The default agent follows the model adviser, so no profile has to be patched when a model arrives. */
export const followsAdviser = (agents: AgentProfile[], defaultAgentId?: unknown): boolean => isAutoProfile(defaultAgent(agents, defaultAgentId)?.inference_profile);

/** The four first-run steps, in the order they unlock each other: runtime → model → profile → first session. */
export function checklist(opts: { runtimes: RuntimeInfo[]; models: LocalModel[]; profiles: InferenceProfile[]; sessions: number }): ChecklistStep[] {
  return [
    { id: "runtime", label: t("home.un_runtime_disponibile"), done: opts.runtimes.some(isUsableRuntime) },
    { id: "model", label: t("home.un_modello_adatto_a_questo"), done: opts.models.length > 0 },
    { id: "profile", label: t("home.un_profilo_di_inferenza"), done: opts.profiles.length > 0 },
    { id: "chat", label: t("home.la_prima_sessione"), done: opts.sessions > 0 },
  ];
}

/** The checklist stays on the overview until every step is done. */
export const firstRunPending = (steps: ChecklistStep[]): boolean => steps.some((s) => !s.done);

/** First step still to do: the one the page points at. */
export const currentStep = (steps: ChecklistStep[]): ChecklistId | undefined => steps.find((s) => !s.done)?.id;

/** What the "Avvia runtime" shortcut does for the default profile's runtime. */
export type RuntimeShortcut =
  | { kind: "start"; runtime: RuntimeInfo; model?: string }
  | { kind: "running"; runtime: RuntimeInfo }
  | { kind: "open" };

export function runtimeShortcut(runtimes: RuntimeInfo[], profile: InferenceProfile | undefined): RuntimeShortcut {
  const rt = runtimes.find((r) => r.id === profile?.runtime_id);
  if (!rt) return { kind: "open" };
  if (rt.status === "running" || rt.status === "starting") return { kind: "running", runtime: rt };
  if (isStartableRuntime(rt)) return { kind: "start", runtime: rt, model: profile?.model || undefined };
  return { kind: "open" };
}

/** Share of a download that has arrived, 0..1. */
export const downloadFraction = (received: number, total: number): number => (total > 0 ? Math.min(1, received / total) : 0);

// ---------------------------------------------------------------- simple mode

/**
 * What the single first-run panel of the simple interface offers (no step list):
 * - `chat`: a model is installed and the default profile can use it;
 * - `prepare`: nothing installed yet, download `model` (the one recommended for this device);
 * - `connect`: a model is installed but the default profile does not point at a usable one;
 * - `no-runtime`: nothing on this device can run a model;
 * - `no-model`: a runtime exists but the catalog has nothing that fits;
 * - `loading`: the catalog has not answered yet.
 */
export type SimpleFirstRun =
  | { kind: "chat" }
  | { kind: "prepare"; model: CatalogModel }
  | { kind: "connect"; model: LocalModel; runtimeId: string }
  | { kind: "no-runtime" }
  | { kind: "no-model" }
  | { kind: "loading" };

const FIT_RANK: Record<Fit | "", number> = { great: 3, ok: 2, tight: 1, no: -1, "": 0 };

/** Best installed chat model that a runtime of this device can open: fits well, has tools, then the largest. */
export function bestInstalledModel(models: LocalModel[]): { model: LocalModel; runtimeId: string } | undefined {
  const tools = (m: LocalModel) => (m.capabilities.includes("tools") ? 1 : 0);
  const ranked = models
    .filter((m) => (m.capabilities.length === 0 || m.capabilities.includes("chat")) && m.fit !== "no" && pickRuntime(m) !== null)
    .sort((a, b) => FIT_RANK[b.fit] - FIT_RANK[a.fit] || tools(b) - tools(a) || b.size_bytes - a.size_bytes || a.id.localeCompare(b.id));
  const model = ranked[0];
  const runtimeId = model ? pickRuntime(model) : null;
  return model && runtimeId ? { model, runtimeId } : undefined;
}

/** The profile points at a model that is on disk and at a runtime that can serve requests. */
export function isUsableProfile(profile: InferenceProfile | undefined, models: LocalModel[], runtimes: RuntimeInfo[]): boolean {
  if (!profile || !profile.model) return false;
  const rt = runtimes.find((r) => r.id === profile.runtime_id);
  return !!rt && isUsableRuntime(rt) && models.some((m) => m.id === profile.model);
}

/**
 * The model to offer for the first download: the adviser's `agent` proposal when the server has one,
 * else (older server, no endpoint) the largest that fits comfortably.
 */
export function firstRunModel(catalog: CatalogModel[], recommendation?: Recommendations | null): CatalogModel | undefined {
  return catalogForChoice(recommendation?.agent.download, catalog) ?? (recommendation ? undefined : recommendedModel(catalog));
}

export function simpleFirstRun(opts: {
  runtimes: RuntimeInfo[];
  models: LocalModel[];
  profile?: InferenceProfile;
  catalog?: CatalogModel[];
  /** The adviser's answer; `null` when the server cannot give one (the old rule is used), `undefined` when not asked. */
  recommendation?: Recommendations | null;
  /** The adviser is still being asked: wait instead of showing a guess. */
  recommendationPending?: boolean;
  /** The default agent's profile is `auto:*`: it follows the installed model by itself. */
  followsAdviser?: boolean;
}): SimpleFirstRun {
  const { runtimes, models, profile, catalog, recommendation } = opts;
  if (opts.recommendationPending) return { kind: "loading" };
  if (models.length > 0) {
    if (opts.followsAdviser && recommendation?.agent.installed) return { kind: "chat" };
    if (isUsableProfile(profile, models, runtimes)) return { kind: "chat" };
    const best = bestInstalledModel(models);
    return best ? { kind: "connect", ...best } : { kind: "no-runtime" };
  }
  if (!runtimes.some(isUsableRuntime)) return { kind: "no-runtime" };
  if (!catalog) return { kind: "loading" };
  const rec = firstRunModel(catalog, recommendation);
  if (!rec) return { kind: "no-model" };
  return pickRuntime(rec) === null ? { kind: "no-runtime" } : { kind: "prepare", model: rec };
}

/** The simple panel stays until a model is ready for chat and a first session exists. */
export const simpleFirstRunPending = (state: SimpleFirstRun, sessions: number): boolean => state.kind !== "chat" || sessions === 0;

/** Profile to save so the default agent talks to `model`: the existing one with the new model, or a new "default". */
export function profileForModel(profile: InferenceProfile | undefined, model: LocalModel, runtimeId: string): InferenceProfile {
  return profile ? applyModelToProfile(profile, model, runtimeId) : { ...newProfileFor(model, runtimeId, tr("home.predefinito"), []), id: "default" };
}
