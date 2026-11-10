import type { AdviserTask, CatalogModel, LocalModel, ModelChoice, Recommendations, TaskAdvice } from "../types";
import { catalog, models } from "./fixtures";

/** What a task needs from a model (the demo has no per-task scores: capabilities decide). */
const NEEDS: Record<AdviserTask, string[]> = {
  chat: ["chat"],
  agent: ["chat", "tools"],
  reasoning: ["chat", "tools", "reasoning"],
  vision: ["chat", "vision"],
  embeddings: ["embeddings"],
};

const REASON: Record<AdviserTask, string> = {
  chat: "Risponde veloce e va bene per le conversazioni di ogni giorno.",
  agent: "Sa usare bene gli strumenti e lascia memoria per ricordare la conversazione.",
  reasoning: "Ragiona prima di rispondere: adatto ai compiti difficili.",
  vision: "Capisce le immagini e le schermate.",
  embeddings: "Piccolo e veloce: serve a cercare tra i tuoi documenti.",
};

const suited = (caps: string[], task: AdviserTask) => NEEDS[task].every((c) => caps.includes(c));
const runs = (fit: string) => fit === "great" || fit === "ok";

function choice(task: AdviserTask, c: { id: string; name: string; fit: ModelChoice["fit"]; size_bytes: number; context: number; runtime: string; catalog_id?: string; caps: string[] }): ModelChoice {
  const thinking = task === "reasoning" && c.caps.includes("reasoning") ? ("deep" as const) : task === "chat" && c.caps.includes("reasoning") ? ("off" as const) : undefined;
  return { model: c.id, name: c.name, runtime_id: c.runtime, context: c.context, thinking, fit: c.fit, size_bytes: c.size_bytes, catalog_id: c.catalog_id, reason: REASON[task] };
}

function advice(task: AdviserTask, local: LocalModel[], cat: CatalogModel[]): TaskAdvice {
  const inst = local.filter((m) => m.format === "gguf" && suited(m.capabilities, task) && runs(m.fit)).sort((a, b) => b.size_bytes - a.size_bytes)[0];
  const down = cat.filter((c) => c.downloadable && !c.installed && suited(c.capabilities, task) && runs(c.fit) && c.runtime_compat.some((r) => r.available)).sort((a, b) => b.size_bytes - a.size_bytes)[0];
  return {
    installed: inst ? choice(task, { id: inst.id, name: inst.name, fit: inst.fit as ModelChoice["fit"], size_bytes: inst.size_bytes, context: Math.min(inst.context_length ?? 8192, 16384), runtime: "llama-server", caps: inst.capabilities }) : null,
    download: !inst && down ? choice(task, { id: down.id, name: down.name, fit: down.fit as ModelChoice["fit"], size_bytes: down.size_bytes, context: Math.min(down.context_length, 16384), runtime: "llama-server", catalog_id: down.id, caps: down.capabilities }) : null,
  };
}

/** Demo answer of `GET /api/recommend`, from the demo models and catalog. */
export function mockRecommend(): Recommendations {
  const a = (t: AdviserTask) => advice(t, models, catalog);
  return { tier: "xl", chat: a("chat"), agent: a("agent"), reasoning: a("reasoning"), vision: a("vision"), embeddings: a("embeddings") };
}
