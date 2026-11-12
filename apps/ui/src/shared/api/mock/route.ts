import type { RouteCandidate, RouteDecision, RouteExclusion } from "../types";
import { models, runtimes } from "./fixtures";

const MIN_CONTEXT = 2048;

/** Demo answer of `GET /api/route`: the demo runtimes that read the model's format, ready ones first. */
export function mockRoute(model: string, context?: number, remote?: boolean): RouteDecision {
  const wanted = context && context > 0 ? context : 8192;
  const local = models.find((m) => m.id === model);
  const candidates: RouteCandidate[] = [];
  const excluded: RouteExclusion[] = [];
  const sizeMb = (local?.size_bytes ?? 0) / (1 << 20);
  const need = Math.round(sizeMb * 1.1 + 700);
  for (const r of runtimes) {
    if (!r.capabilities.includes("chat") || r.api === "none") continue;
    const reads = r.loaded_models.includes(model) || (local ? r.formats.length === 0 || r.formats.includes(local.format) || r.formats.includes("gguf") && local.format === "gguf" : false);
    if (!reads) continue;
    if (r.kind === "remote" && !remote) {
      excluded.push({ runtime_id: r.id, runtime_name: r.name, reason: "è un altro dispositivo o un server remoto: il profilo non lo consente" });
    } else if (r.status === "running" || r.status === "available") {
      const gpu = r.accelerators.find((a) => a !== "cpu");
      candidates.push({
        rank: 0, runtime_id: r.id, runtime_name: r.name, accelerator: r.kind === "remote" ? "remoto" : gpu ?? "cpu", offload: gpu ? "all" : "none", gpu_layers: gpu ? -1 : 0,
        context: wanted, need_mb: need, kv_mb: Math.round(wanted / 64), kv_estimated: true, remote: r.kind === "remote", ready: r.status === "running", reason: "",
      });
    } else if (r.status === "installable") {
      excluded.push({ runtime_id: r.id, runtime_name: r.name, reason: `non installato${r.install_hint ? ` (${r.install_hint})` : ""}` });
    } else {
      excluded.push({ runtime_id: r.id, runtime_name: r.name, reason: r.status_detail || (r.status === "unsupported" ? "non supportato su questo sistema" : "non raggiungibile") });
    }
  }
  candidates.sort((a, b) => Number(a.remote) - Number(b.remote) || Number(b.ready) - Number(a.ready));
  candidates.forEach((c, i) => {
    c.rank = i + 1;
    const where = c.remote ? "su un altro dispositivo" : c.offload === "all" ? `tutto il modello su ${c.accelerator.toUpperCase()}` : "sulla CPU";
    c.reason = `${where}; ${c.ready ? "già in esecuzione" : "AIW lo avvia"}; contesto ${Math.max(c.context, MIN_CONTEXT)}${i > 0 ? " (alternativa)" : ""}`;
  });
  return { model, wanted_context: wanted, candidates, excluded, notes: ["La KV cache è una stima approssimata: il modello non dice la sua struttura."] };
}
