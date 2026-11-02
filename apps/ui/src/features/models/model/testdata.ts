// Builders for the unit tests of this folder (not imported by the app).
import type { CatalogModel, DownloadProgress, InferenceProfile, LocalModel, ModelVariant, RuntimeCompat } from "../../../shared/api/types";

export const rt = (id: string, available = true, reason = ""): RuntimeCompat => ({ id, name: id.toUpperCase(), available, reason });

export function local(over: Partial<LocalModel> = {}): LocalModel {
  return {
    id: "m1", name: "Modello 1", format: "gguf", size_bytes: 1_000, capabilities: ["chat"], runtimes: ["llama-server"], origin: "aiw", removable: true,
    runtime_compat: [rt("llama-server")], fit: "great", fit_detail: "", ...over,
  };
}

export function variant(over: Partial<ModelVariant> = {}): ModelVariant {
  return {
    id: "Q4_K_M", label: "Q4_K_M", format: "gguf", quant: "Q4_K_M", files: [{ path: "a.gguf", size_bytes: 100, role: "weights" }], size_bytes: 100,
    fit: "great", fit_detail: "", recommended: false, runtimes: [rt("llama-server")], installed: false, ...over,
  };
}

export function job(over: Partial<DownloadProgress> = {}): DownloadProgress {
  return {
    id: "dl1", model_id: "hf:Qwen/Qwen3-4B-GGUF", file: "a.gguf", received: 0, total: 1_000, status: "running", provider: "hf", model_ref: "Qwen/Qwen3-4B-GGUF",
    variant: "Q4_K_M", files_total: 1, files_done: 0, speed_bps: 0, dest: "", ...over,
  };
}

export function catalogEntry(over: Partial<CatalogModel> = {}): CatalogModel {
  return {
    id: "qwen3-4b", name: "Qwen3 4B", family: "Qwen3", publisher: "Alibaba", description: "", params_b: 4, format: "gguf", quant: "Q4_K_M", size_bytes: 2_500_000_000,
    context_length: 40960, capabilities: ["chat"], license: "Apache-2.0", source: { kind: "hf", repo: "Qwen/Qwen3-4B-GGUF", file: "Qwen3-4B-Q4_K_M.gguf" },
    min_memory_mb: 4000, tags: [], fit: "great", fit_detail: "", installed: false, runtime_compat: [], downloadable: true, ...over,
  };
}

export function profile(over: Partial<InferenceProfile> = {}): InferenceProfile {
  return { id: "p1", name: "Profilo 1", runtime_id: "llama-server", model: "m1", params: { temperature: 0.5, context: 8192 }, ...over };
}
