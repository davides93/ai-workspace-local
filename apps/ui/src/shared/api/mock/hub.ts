import type {
  Fit, ModelProviderInfo, ModelSearchParams, ModelVariant, RemoteFile, RemoteModel, RemoteModelDetail, RuntimeCompat, SearchPage,
} from "../types";
import { GB, compat, fitFor } from "./fixtures";

// Demo content of the model hub (docs/architecture/model-hub.md): providers, a corpus of plausible
// search results and the variants of each repository. The shapes are the real ones; the data is sample data.

export const providers: ModelProviderInfo[] = [
  { id: "catalog", name: "Consigliati", description: "Catalogo curato: modelli provati su questo tipo di device, con la compatibilità già calcolata.", homepage: "", capabilities: ["search"], formats: ["gguf", "litertlm", "onnx", "mlx", "system"], auth: "none", auth_configured: false, endpoint: "" },
  { id: "hf", name: "Hugging Face", description: "Milioni di modelli: GGUF, MLX, ONNX e safetensors. Il token serve solo per i repository gated.", homepage: "https://huggingface.co", capabilities: ["search", "files", "auth", "multi_file"], formats: ["gguf", "mlx", "onnx", "safetensors"], auth: "optional", auth_configured: false, endpoint: "https://huggingface.co" },
  { id: "modelscope", name: "ModelScope", description: "Hub cinese di modelli, utile come mirror dove Hugging Face è lento o non raggiungibile.", homepage: "https://www.modelscope.cn", capabilities: ["search", "files", "auth", "multi_file"], formats: ["gguf", "safetensors"], auth: "optional", auth_configured: false, endpoint: "https://www.modelscope.cn" },
  { id: "ollama", name: "Registry Ollama", description: "I modelli della libreria Ollama scaricati direttamente dal registro, senza bisogno del demone Ollama.", homepage: "https://ollama.com/library", capabilities: ["search", "files", "multi_file"], formats: ["gguf"], auth: "none", auth_configured: false, endpoint: "https://registry.ollama.ai" },
  { id: "url", name: "URL diretto", description: "Un file qualsiasi raggiungibile via HTTP(S), con ripresa del download e verifica sha256 opzionale.", homepage: "", capabilities: ["files"], formats: ["gguf", "onnx", "safetensors"], auth: "none", auth_configured: false, endpoint: "" },
];

interface Seed {
  provider: "hf" | "modelscope" | "ollama";
  id: string;
  publisher: string;
  description: string;
  formats: string[];
  tags: string[];
  capabilities: string[];
  downloads: number;
  likes: number;
  days: number;
  license: string;
  gated?: boolean;
  params_b: number;
}

const seeds: Seed[] = [
  { provider: "hf", id: "unsloth/Qwen3.5-35B-A3B-GGUF", publisher: "unsloth", description: "Qwen3.5 35B-A3B (MoE, 3B attivi) quantizzato con Unsloth Dynamic: ottimo per agenti e tool calling.", formats: ["gguf"], tags: ["qwen3_5_moe", "agent", "coding"], capabilities: ["chat", "tools", "reasoning"], downloads: 412_380, likes: 1_204, days: 3, license: "apache-2.0", params_b: 35 },
  { provider: "hf", id: "bartowski/Llama-3.3-70B-Instruct-GGUF", publisher: "bartowski", description: "Llama 3.3 70B Instruct in tutte le quantizzazioni imatrix.", formats: ["gguf"], tags: ["llama", "general"], capabilities: ["chat", "tools"], downloads: 1_820_400, likes: 2_310, days: 40, license: "llama3.3", params_b: 70 },
  { provider: "hf", id: "google/gemma-4-27b-it-GGUF", publisher: "google", description: "Gemma 4 27B multimodale (testo e immagini) con proiettore mmproj.", formats: ["gguf"], tags: ["gemma4", "vision"], capabilities: ["chat", "vision", "tools"], downloads: 289_150, likes: 980, days: 12, license: "gemma", params_b: 27 },
  { provider: "hf", id: "Qwen/Qwen3-4B-GGUF", publisher: "Qwen", description: "Qwen3 4B ufficiale: piccolo, veloce, con modalità di ragionamento.", formats: ["gguf"], tags: ["qwen3", "small"], capabilities: ["chat", "tools", "reasoning"], downloads: 960_700, likes: 1_540, days: 70, license: "apache-2.0", params_b: 4 },
  { provider: "hf", id: "Qwen/Qwen3-Embedding-0.6B-GGUF", publisher: "Qwen", description: "Modello di embedding multilingue per ricerca semantica e RAG.", formats: ["gguf"], tags: ["embedding"], capabilities: ["embeddings"], downloads: 540_200, likes: 610, days: 55, license: "apache-2.0", params_b: 0.6 },
  { provider: "hf", id: "Hcompany/Holo2-8B-GGUF", publisher: "Hcompany", description: "Holo2 8B: grounding della GUI per il computer-use.", formats: ["gguf"], tags: ["computer-use", "vision"], capabilities: ["chat", "vision", "gui-grounding"], downloads: 48_900, likes: 210, days: 20, license: "apache-2.0", params_b: 8 },
  { provider: "hf", id: "mistralai/Mistral-7B-Instruct-v0.3-GGUF", publisher: "mistralai", description: "Mistral 7B Instruct v0.3, function calling nativo.", formats: ["gguf"], tags: ["mistral", "general"], capabilities: ["chat", "tools"], downloads: 710_300, likes: 880, days: 160, license: "apache-2.0", params_b: 7 },
  { provider: "hf", id: "deepseek-ai/DeepSeek-R1-Distill-Qwen-14B-GGUF", publisher: "deepseek-ai", description: "Distillazione di R1 su Qwen 14B: ragionamento passo-passo.", formats: ["gguf"], tags: ["reasoning"], capabilities: ["chat", "reasoning"], downloads: 355_800, likes: 760, days: 95, license: "mit", params_b: 14 },
  { provider: "hf", id: "mlx-community/Qwen3-4B-4bit", publisher: "mlx-community", description: "Qwen3 4B convertito per MLX (4 bit). Solo Apple Silicon.", formats: ["mlx"], tags: ["mlx", "apple-silicon"], capabilities: ["chat", "tools"], downloads: 188_400, likes: 240, days: 30, license: "apache-2.0", params_b: 4 },
  { provider: "hf", id: "mlx-community/gemma-4-27b-it-4bit", publisher: "mlx-community", description: "Gemma 4 27B per MLX, con encoder visivo. Solo Apple Silicon.", formats: ["mlx"], tags: ["mlx", "vision"], capabilities: ["chat", "vision"], downloads: 41_200, likes: 120, days: 9, license: "gemma", params_b: 27 },
  { provider: "hf", id: "microsoft/Phi-4-mini-instruct-onnx", publisher: "microsoft", description: "Phi-4 mini ottimizzato per ONNX Runtime GenAI (CPU, DirectML, NPU).", formats: ["onnx"], tags: ["onnx", "directml"], capabilities: ["chat", "tools"], downloads: 129_000, likes: 330, days: 85, license: "mit", params_b: 3.8 },
  { provider: "hf", id: "meta-llama/Llama-3.2-3B-Instruct", publisher: "meta-llama", description: "Llama 3.2 3B Instruct, pesi originali. Repository gated: serve un token Hugging Face.", formats: ["safetensors"], tags: ["llama", "gated"], capabilities: ["chat", "tools"], downloads: 3_100_000, likes: 1_900, days: 140, license: "llama3.2", gated: true, params_b: 3 },
  { provider: "hf", id: "ggml-org/gpt-oss-20b-GGUF", publisher: "ggml-org", description: "gpt-oss 20B (MoE) in MXFP4: ragionamento e tool calling con licenza permissiva.", formats: ["gguf"], tags: ["agent", "reasoning"], capabilities: ["chat", "tools", "reasoning"], downloads: 690_400, likes: 1_100, days: 18, license: "apache-2.0", params_b: 20 },
  { provider: "hf", id: "unsloth/gemma-3n-E4B-it-GGUF", publisher: "unsloth", description: "Gemma 3n E4B a basso consumo, con proiettore mmproj per le immagini.", formats: ["gguf"], tags: ["gemma3n", "vision", "small"], capabilities: ["chat", "vision"], downloads: 330_200, likes: 460, days: 75, license: "gemma", params_b: 8 },
  { provider: "hf", id: "bartowski/Qwen2.5-Coder-7B-Instruct-GGUF", publisher: "bartowski", description: "Qwen2.5 Coder 7B Instruct: completamento e revisione di codice.", formats: ["gguf"], tags: ["coding"], capabilities: ["chat", "tools"], downloads: 262_100, likes: 540, days: 210, license: "apache-2.0", params_b: 7 },
  { provider: "modelscope", id: "Qwen/Qwen3-8B-GGUF", publisher: "Qwen", description: "Qwen3 8B GGUF dal mirror ModelScope.", formats: ["gguf"], tags: ["qwen3"], capabilities: ["chat", "tools", "reasoning"], downloads: 220_100, likes: 410, days: 45, license: "apache-2.0", params_b: 8 },
  { provider: "modelscope", id: "unsloth/gemma-4-E4B-it-GGUF", publisher: "unsloth", description: "Gemma 4 E4B (testo, immagini, audio) quantizzato.", formats: ["gguf"], tags: ["gemma4", "small"], capabilities: ["chat", "vision", "tools"], downloads: 98_600, likes: 205, days: 14, license: "gemma", params_b: 8 },
  { provider: "modelscope", id: "LLM-Research/Meta-Llama-3.1-8B-Instruct-GGUF", publisher: "LLM-Research", description: "Llama 3.1 8B Instruct in formato GGUF.", formats: ["gguf"], tags: ["llama"], capabilities: ["chat", "tools"], downloads: 172_000, likes: 300, days: 110, license: "llama3.1", params_b: 8 },
  { provider: "modelscope", id: "AI-ModelScope/bge-m3-gguf", publisher: "AI-ModelScope", description: "BGE-M3: embedding densi e sparsi, 100+ lingue.", formats: ["gguf"], tags: ["embedding"], capabilities: ["embeddings"], downloads: 64_300, likes: 150, days: 70, license: "mit", params_b: 0.57 },
  { provider: "ollama", id: "qwen3:4b", publisher: "library", description: "Qwen3 4B dalla libreria Ollama.", formats: ["gguf"], tags: ["qwen3", "small"], capabilities: ["chat", "tools", "reasoning"], downloads: 5_400_000, likes: 0, days: 25, license: "apache-2.0", params_b: 4 },
  { provider: "ollama", id: "llama3.2:3b", publisher: "library", description: "Llama 3.2 3B Instruct.", formats: ["gguf"], tags: ["llama", "small"], capabilities: ["chat", "tools"], downloads: 22_000_000, likes: 0, days: 120, license: "llama3.2", params_b: 3 },
  { provider: "ollama", id: "gemma3:12b", publisher: "library", description: "Gemma 3 12B multimodale.", formats: ["gguf"], tags: ["gemma3", "vision"], capabilities: ["chat", "vision"], downloads: 9_800_000, likes: 0, days: 60, license: "gemma", params_b: 12 },
  { provider: "ollama", id: "phi4-mini", publisher: "library", description: "Phi-4 mini 3.8B di Microsoft.", formats: ["gguf"], tags: ["phi", "small"], capabilities: ["chat", "tools"], downloads: 1_300_000, likes: 0, days: 80, license: "mit", params_b: 3.8 },
  { provider: "ollama", id: "nomic-embed-text", publisher: "library", description: "Embedding di testo ad alte prestazioni con contesto lungo.", formats: ["gguf"], tags: ["embedding"], capabilities: ["embeddings"], downloads: 31_000_000, likes: 0, days: 200, license: "apache-2.0", params_b: 0.14 },
  { provider: "ollama", id: "llava:7b", publisher: "library", description: "LLaVA 7B: chat con immagini.", formats: ["gguf"], tags: ["llava", "vision"], capabilities: ["chat", "vision"], downloads: 4_200_000, likes: 0, days: 300, license: "apache-2.0", params_b: 7 },
];

const iso = (daysAgo: number) => new Date(Date.now() - daysAgo * 86_400_000).toISOString();

function toRemote(s: Seed): RemoteModel {
  const name = s.id.includes("/") ? s.id.split("/")[1] : s.id;
  return {
    provider: s.provider, id: s.id, name, publisher: s.publisher, description: s.description, formats: s.formats, tags: s.tags,
    capabilities: s.capabilities, downloads: s.downloads, likes: s.provider === "ollama" ? undefined : s.likes, updated_at: iso(s.days),
    license: s.license, gated: !!s.gated, params_b: s.params_b,
    url: s.provider === "hf" ? `https://huggingface.co/${s.id}` : s.provider === "modelscope" ? `https://www.modelscope.cn/models/${s.id}` : `https://ollama.com/library/${s.id.split(":")[0]}`,
  };
}

export const corpus: RemoteModel[] = seeds.map(toRemote);

const norm = (s: string) => s.toLowerCase();

/** Search over the demo corpus: text, format filter, sort and cursor pagination (`cursor` = offset). */
export function searchCorpus(items: RemoteModel[], p: ModelSearchParams): SearchPage {
  // Demo of an unreachable provider (HTTP 502): search for "offline".
  if (norm(p.q).trim() === "offline") {
    throw Object.assign(new Error("Provider irraggiungibile: impossibile contattare il servizio (timeout di connessione)"), { status: 502 });
  }
  const words = norm(p.q).split(/\s+/).filter(Boolean);
  let list = items.filter((m) => {
    if (p.format && !m.formats.includes(p.format)) return false;
    const hay = norm(`${m.id} ${m.name} ${m.publisher} ${m.tags.join(" ")} ${m.capabilities.join(" ")} ${m.description ?? ""}`);
    return words.every((w) => hay.includes(w));
  });
  const sort = p.sort ?? "downloads";
  list = [...list].sort((a, b) =>
    sort === "likes" ? (b.likes ?? 0) - (a.likes ?? 0)
    : sort === "updated" ? (b.updated_at ?? "").localeCompare(a.updated_at ?? "")
    : (b.downloads ?? 0) - (a.downloads ?? 0));
  const limit = Math.max(1, Math.min(50, p.limit ?? 12));
  const offset = p.cursor ? Math.max(0, parseInt(p.cursor, 10) || 0) : 0;
  const page = list.slice(offset, offset + limit);
  return { items: page, next_cursor: offset + limit < list.length ? String(offset + limit) : undefined };
}

// ---------------------------------------------------------------- variants

const BYTES_PER_PARAM: Record<string, number> = { Q4_K_M: 0.58, Q5_K_M: 0.69, Q8_0: 1.06, F16: 2.0 };
const GGUF_RUNTIMES = ["llama-server", "llama-embedded", "ollama", "lmstudio"];
export const fakeSha = (seed: string) => {
  let h = 0x811c9dc5;
  const out: string[] = [];
  for (let r = 0; r < 8; r++) {
    for (const ch of `${seed}${r}`) h = Math.imul(h ^ ch.charCodeAt(0), 0x01000193) >>> 0;
    out.push(h.toString(16).padStart(8, "0"));
  }
  return out.join("");
};

/** Splits the weights into shards of at most 40 GB, like `*-00001-of-00003.gguf` repositories do. */
function shards(base: string, quant: string, total: number): RemoteFile[] {
  const n = Math.max(1, Math.ceil(total / (40 * GB)));
  return Array.from({ length: n }, (_, i) => ({
    path: n === 1 ? `${base}-${quant}.gguf` : `${base}-${quant}-${String(i + 1).padStart(5, "0")}-of-${String(n).padStart(5, "0")}.gguf`,
    size_bytes: Math.round(total / n),
    sha256: fakeSha(`${base}${quant}${i}`),
    role: "weights" as const,
  }));
}

function pickRecommended(variants: ModelVariant[]): ModelVariant | undefined {
  const rank: Fit[] = ["great", "ok"];
  for (const fit of rank) {
    const c = variants.filter((v) => v.fit === fit);
    if (c.length === 0) continue;
    return c.find((v) => v.quant === "Q4_K_M") ?? c.reduce((a, b) => (b.size_bytes > a.size_bytes ? b : a));
  }
  return undefined;
}

function ggufVariants(m: RemoteModel, installed: Set<string>): ModelVariant[] {
  const params = (m.params_b ?? 4) * 1e9;
  const vision = m.capabilities.includes("vision");
  const base = m.name.replace(/-GGUF$/i, "");
  const quants = (m.capabilities.includes("embeddings") ? ["Q8_0", "F16"] : ["Q4_K_M", "Q5_K_M", "Q8_0", "F16"]);
  const mmproj: RemoteFile | null = vision ? { path: `mmproj-${base}-F16.gguf`, size_bytes: Math.round(0.9 * GB), sha256: fakeSha(`${base}mmproj`), role: "mmproj" } : null;
  const variants = quants.map<ModelVariant>((q) => {
    const weights = shards(base, q, params * BYTES_PER_PARAM[q] + 0.05 * GB);
    const files = mmproj ? [...weights, mmproj] : weights;
    const size = files.reduce((s, f) => s + f.size_bytes, 0);
    const f = fitFor(size);
    return {
      id: q, label: `${q}${mmproj ? " + mmproj" : ""}`, format: "gguf", quant: q, files, size_bytes: size, fit: f.fit, fit_detail: f.detail, recommended: false,
      runtimes: compat(GGUF_RUNTIMES), installed: installed.has(`${m.provider}/${m.id}/${q}`),
    };
  });
  const rec = pickRecommended(variants);
  if (rec) rec.recommended = true;
  return variants;
}

function mlxVariant(m: RemoteModel, installed: Set<string>): ModelVariant {
  const size = Math.round((m.params_b ?? 4) * 1e9 * 0.55 + 0.1 * GB);
  const n = Math.max(1, Math.ceil(size / (4 * GB)));
  const files: RemoteFile[] = [
    { path: "config.json", size_bytes: 1_400, role: "config" },
    { path: "tokenizer.json", size_bytes: 11_400_000, sha256: fakeSha(`${m.id}tok`), role: "tokenizer" },
    ...Array.from({ length: n }, (_, i) => ({ path: `model-${String(i + 1).padStart(5, "0")}-of-${String(n).padStart(5, "0")}.safetensors`, size_bytes: Math.round(size / n), sha256: fakeSha(`${m.id}${i}`), role: "weights" as const })),
  ];
  const reason: RuntimeCompat[] = [
    { id: "mlx-lm", name: "MLX LM", available: false, reason: "solo macOS su Apple Silicon" },
    { id: "lmstudio", name: "LM Studio", available: false, reason: "il motore MLX richiede macOS" },
    { id: "mlx-swift", name: "MLX Swift", available: false, reason: "non ancora implementato" },
  ];
  return {
    id: "repo", label: "Repository MLX", format: "mlx", quant: "4bit", files, size_bytes: files.reduce((s, f) => s + f.size_bytes, 0), fit: "no",
    fit_detail: "Richiede macOS su Apple Silicon: questo PC non è compatibile (puoi usarlo su un Mac accoppiato).", recommended: false, runtimes: reason,
    installed: installed.has(`${m.provider}/${m.id}/repo`),
  };
}

function onnxVariant(m: RemoteModel, installed: Set<string>): ModelVariant {
  const size = Math.round((m.params_b ?? 3.8) * 1e9 * 0.62);
  const files: RemoteFile[] = [
    { path: "cpu_and_mobile/cpu-int4-rtn-block-32/model.onnx", size_bytes: 52_000_000, sha256: fakeSha(`${m.id}onnx`), role: "weights" },
    { path: "cpu_and_mobile/cpu-int4-rtn-block-32/model.onnx.data", size_bytes: size, sha256: fakeSha(`${m.id}data`), role: "weights" },
    { path: "cpu_and_mobile/cpu-int4-rtn-block-32/genai_config.json", size_bytes: 2_100, role: "config" },
    { path: "cpu_and_mobile/cpu-int4-rtn-block-32/tokenizer.json", size_bytes: 15_500_000, role: "tokenizer" },
  ];
  const total = files.reduce((s, f) => s + f.size_bytes, 0);
  const f = fitFor(total);
  return {
    id: "cpu-int4", label: "CPU · int4", format: "onnx", quant: "int4", files, size_bytes: total, fit: f.fit, fit_detail: f.detail, recommended: f.fit === "great",
    runtimes: compat(["foundry-local", "windows-ml"]), installed: installed.has(`${m.provider}/${m.id}/cpu-int4`),
  };
}

function safetensorsVariant(m: RemoteModel, installed: Set<string>): ModelVariant {
  const size = Math.round((m.params_b ?? 3) * 1e9 * 2);
  const n = Math.max(1, Math.ceil(size / (5 * GB)));
  const files: RemoteFile[] = [
    { path: "config.json", size_bytes: 900, role: "config" },
    { path: "tokenizer.json", size_bytes: 17_200_000, role: "tokenizer" },
    ...Array.from({ length: n }, (_, i) => ({ path: `model-${String(i + 1).padStart(5, "0")}-of-${String(n).padStart(5, "0")}.safetensors`, size_bytes: Math.round(size / n), sha256: fakeSha(`${m.id}${i}`), role: "weights" as const })),
  ];
  const total = files.reduce((s, f) => s + f.size_bytes, 0);
  const f = fitFor(total);
  return {
    id: "repo", label: "Repository completo (bf16)", format: "safetensors", quant: "bf16", files, size_bytes: total, fit: f.fit,
    fit_detail: m.gated ? `${f.detail}. Repository gated: serve un token Hugging Face.` : f.detail, recommended: false, runtimes: compat(["vllm", "sglang"]),
    installed: installed.has(`${m.provider}/${m.id}/repo`),
  };
}

function ollamaVariants(m: RemoteModel, installed: Set<string>): ModelVariant[] {
  const params = (m.params_b ?? 4) * 1e9;
  const vision = m.capabilities.includes("vision");
  const [name, tag] = m.id.split(":");
  const tags: [string, string, number][] = [[tag ?? "latest", "Q4_K_M", 0.6], ["q8_0", "Q8_0", 1.08]];
  const variants = tags.map<ModelVariant>(([t, quant, bpp], i) => {
    const files: RemoteFile[] = [{ path: "model.gguf", size_bytes: Math.round(params * bpp), sha256: fakeSha(`${m.id}${t}`), role: "weights" }];
    if (vision) files.push({ path: "mmproj.gguf", size_bytes: Math.round(0.6 * GB), sha256: fakeSha(`${m.id}proj`), role: "mmproj" });
    const size = files.reduce((s, f) => s + f.size_bytes, 0);
    const f = fitFor(size);
    return {
      id: i === 0 ? (tag ?? "latest") : t, label: `${name}:${i === 0 ? (tag ?? "latest") : t} (${quant})`, format: "gguf", quant, files, size_bytes: size, fit: f.fit, fit_detail: f.detail,
      recommended: false, runtimes: compat(GGUF_RUNTIMES), installed: installed.has(`${m.provider}/${m.id}/${i === 0 ? (tag ?? "latest") : t}`),
    };
  });
  const rec = pickRecommended(variants);
  if (rec) rec.recommended = true;
  return variants;
}

/** Detail of a remote model; an unknown `owner/name` is accepted too (exact repository names always work). */
export function buildDetail(items: RemoteModel[], provider: string, model: string, installed: Set<string>, revision?: string): RemoteModelDetail {
  let m = items.find((x) => x.provider === provider && x.id === model);
  if (!m) {
    if (provider !== "hf" && provider !== "modelscope" && provider !== "ollama") throw new Error(`Modello ${model} non trovato su ${provider}`);
    const guessGguf = /gguf/i.test(model) || provider === "ollama";
    m = {
      provider, id: model, name: model.includes("/") ? model.split("/")[1] : model, publisher: model.includes("/") ? model.split("/")[0] : "library",
      formats: [guessGguf ? "gguf" : "safetensors"], tags: [], capabilities: ["chat"], gated: false, params_b: 7, url: "",
    };
  }
  let variants: ModelVariant[];
  if (m.formats.includes("mlx")) variants = [mlxVariant(m, installed)];
  else if (m.formats.includes("onnx")) variants = [onnxVariant(m, installed)];
  else if (m.formats.includes("safetensors") && !m.formats.includes("gguf")) variants = [safetensorsVariant(m, installed)];
  else if (provider === "ollama") variants = ollamaVariants(m, installed);
  else variants = ggufVariants(m, installed);
  return { model: m, revision: revision ?? (provider === "modelscope" ? "master" : provider === "ollama" ? "latest" : "main"), variants };
}
