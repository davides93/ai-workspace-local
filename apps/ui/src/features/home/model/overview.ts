import type { AgentProfile, CatalogModel, InferenceProfile, LocalModel, RuntimeInfo } from "../../../shared/api/types";

export interface ChecklistStep {
  id: "runtime" | "model" | "profile" | "chat";
  label: string;
  done: boolean;
}

/** Runtime that can serve requests now or on demand (`running`, or detected and startable). */
export const isUsableRuntime = (r: RuntimeInfo): boolean => r.status === "running" || r.status === "available";

/** The first-run checklist is shown until there is a local model and a usable runtime. */
export function needsFirstRun(models: LocalModel[], runtimes: RuntimeInfo[]): boolean {
  return models.length === 0 || !runtimes.some(isUsableRuntime);
}

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

export function checklist(opts: { runtimes: RuntimeInfo[]; models: LocalModel[]; profiles: InferenceProfile[]; sessions: number }): ChecklistStep[] {
  return [
    { id: "runtime", label: "Un runtime pronto", done: opts.runtimes.some(isUsableRuntime) },
    { id: "model", label: "Un modello sul disco", done: opts.models.length > 0 },
    { id: "profile", label: "Un profilo di inferenza", done: opts.profiles.length > 0 },
    { id: "chat", label: "La prima conversazione", done: opts.sessions > 0 },
  ];
}
