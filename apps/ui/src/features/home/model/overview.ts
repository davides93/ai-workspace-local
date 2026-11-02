import type { AgentProfile, CatalogModel, InferenceProfile, LocalModel, RuntimeInfo } from "../../../shared/api/types";

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

/** Best model to download first: the largest that fits comfortably, not yet installed, downloadable. */
export function recommendedModel(catalog: CatalogModel[]): CatalogModel | undefined {
  const candidates = catalog.filter((c) => c.downloadable !== false && !c.installed && c.fit === "great");
  return candidates.sort((a, b) => b.size_bytes - a.size_bytes)[0] ?? catalog.find((c) => c.downloadable !== false && !c.installed && c.fit === "ok");
}

/** Profile of the default agent (config `agent.default`, else the first agent). */
export function defaultProfile(agents: AgentProfile[], profiles: InferenceProfile[], defaultAgentId?: unknown): InferenceProfile | undefined {
  const agent = agents.find((a) => a.id === defaultAgentId) ?? agents[0];
  return profiles.find((p) => p.id === agent?.inference_profile) ?? profiles[0];
}

/** The four first-run steps, in the order they unlock each other: runtime → model → profile → first session. */
export function checklist(opts: { runtimes: RuntimeInfo[]; models: LocalModel[]; profiles: InferenceProfile[]; sessions: number }): ChecklistStep[] {
  return [
    { id: "runtime", label: "Un runtime disponibile", done: opts.runtimes.some(isUsableRuntime) },
    { id: "model", label: "Un modello adatto a questo dispositivo", done: opts.models.length > 0 },
    { id: "profile", label: "Un profilo di inferenza", done: opts.profiles.length > 0 },
    { id: "chat", label: "La prima sessione", done: opts.sessions > 0 },
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
