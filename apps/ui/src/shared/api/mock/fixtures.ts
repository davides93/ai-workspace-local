import type {
  AgentProfile, AuditEntry, CatalogModel, ChatMessage, ConfigView, Fit, HookConfig, InferenceProfile, LocalModel, McpServer,
  PolicyRule, RuntimeCompat, RuntimeInfo, Session, SettingsSchema, SystemInfo, RemoteAgent, TeamDef, ToolInfo, WorkflowDef, WorkflowRun,
} from "../types";
import { RUN_ERROR_MESSAGE, SECRET_MASK } from "../types";

// Demo data for the interactive mockup. Everything here is sample content, clearly labelled
// in the UI as "demo"; the real values come from the Rust core (crates/aiw-*).

export const GB = 1024 ** 3;
export const now = () => new Date().toISOString();
export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
let idSeq = 100;
export const nid = (p: string) => `${p}_${(idSeq++).toString(36)}`;

export const llamaSchema: SettingsSchema = {
  type: "object",
  properties: {
    ctx_size: { type: "integer", title: "Finestra di contesto", default: 16384, minimum: 512, maximum: 262144, step: 512 },
    gpu_layers: { type: "integer", title: "Layer su GPU", default: 999, minimum: 0, maximum: 999, description: "999 = tutto su GPU" },
    threads: { type: "integer", title: "Thread CPU", default: 0, minimum: 0, maximum: 64, description: "0 = automatico" },
    flash_attn: { type: "boolean", title: "Flash attention", default: true },
    cache_type: { type: "string", title: "Quantizzazione KV cache", enum: ["f16", "q8_0", "q4_0"], default: "q8_0" },
    parallel: { type: "integer", title: "Slot paralleli", default: 2, minimum: 1, maximum: 16, advanced: true },
    extra_args: { type: "array", title: "Argomenti extra", items: { type: "string" }, advanced: true, placeholder: "--mlock" },
  },
};
export const openaiEndpointSchema: SettingsSchema = {
  type: "object",
  properties: {
    base_url: { type: "string", title: "Endpoint", default: "http://127.0.0.1:8000/v1" },
    api_key: { type: "string", title: "API key (opzionale)", secret: true },
    timeout_s: { type: "integer", title: "Timeout (s)", default: 600, minimum: 5, maximum: 3600, advanced: true },
  },
};
export const vllmSchema: SettingsSchema = {
  type: "object",
  properties: {
    model: { type: "string", title: "Modello HF", default: "Qwen/Qwen3.5-35B-A3B-AWQ" },
    gpu_memory_utilization: { type: "number", title: "Uso memoria GPU", default: 0.9, minimum: 0.1, maximum: 0.98, step: 0.01 },
    max_model_len: { type: "integer", title: "Max model len", default: 32768, minimum: 1024, maximum: 262144 },
    tensor_parallel_size: { type: "integer", title: "Tensor parallel", default: 1, minimum: 1, maximum: 8 },
    quantization: { type: "string", title: "Quantizzazione", enum: ["auto", "awq", "gptq", "fp8"], default: "auto" },
  },
};
export const systemSchema: SettingsSchema = {
  type: "object",
  properties: {
    temperature: { type: "number", title: "Temperatura", default: 0.7, minimum: 0, maximum: 2, step: 0.05 },
    guided_generation: { type: "boolean", title: "Output strutturato (guided generation)", default: true },
  },
};

function rt(p: Partial<RuntimeInfo> & Pick<RuntimeInfo, "id" | "name" | "kind" | "status">): RuntimeInfo {
  return {
    description: "",
    platforms: ["windows", "macos", "linux"],
    accelerators: ["cpu"],
    formats: ["gguf"],
    api: "openai",
    capabilities: ["chat"],
    loaded_models: [],
    settings_schema: openaiEndpointSchema,
    settings: {},
    ...p,
  };
}

export const runtimes: RuntimeInfo[] = [
  rt({ id: "llama-server", name: "llama.cpp server", vendor: "ggml-org", kind: "sidecar", status: "running", endpoint: "http://127.0.0.1:39281/v1",
    description: "Server OpenAI-compatibile di llama.cpp, supervisionato dall'app.", accelerators: ["cuda", "vulkan", "cpu"], capabilities: ["chat", "tools", "vision", "embeddings", "grammar"],
    loaded_models: ["qwen3.5-35b-a3b-q4_k_m"], settings_schema: llamaSchema, settings: { ctx_size: 32768, gpu_layers: 999, threads: 0, flash_attn: true, cache_type: "q8_0", parallel: 2, extra_args: [] },
    status_detail: "CUDA · RTX 4090 · 112 tok/s", install_hint: "winget install ggml.llamacpp", adapter_format: "gguf", homepage: "https://github.com/ggml-org/llama.cpp" }),
  rt({ id: "llama-embedded", api: "none", name: "llama.cpp (embedded)", vendor: "ggml-org", kind: "embedded", status: "available", platforms: ["windows", "macos", "linux", "android", "ios"],
    description: "llama.cpp collegato in-process via FFI: nessun processo esterno. Default su mobile.", accelerators: ["cuda", "metal", "vulkan", "opencl", "cpu"], capabilities: ["chat", "tools", "embeddings", "grammar"], settings_schema: llamaSchema, settings: { ctx_size: 8192, gpu_layers: 999 } }),
  rt({ id: "ollama", name: "Ollama", vendor: "Ollama", kind: "sidecar", status: "available", endpoint: "http://127.0.0.1:11434", api: "ollama",
    description: "Rilevato su questo PC. Gestione modelli via `ollama pull`.", accelerators: ["cuda", "rocm", "metal", "cpu"], capabilities: ["chat", "tools", "vision", "embeddings"], homepage: "https://ollama.com" }),
  rt({ id: "whisper-server", name: "whisper.cpp server", vendor: "ggml-org", kind: "sidecar", status: "running", api: "native", endpoint: "http://127.0.0.1:39290", formats: ["ggml"],
    description: "Trascrizione vocale locale (whisper-server) per la modalità voce.", accelerators: ["cuda", "metal", "vulkan", "cpu"], capabilities: ["stt"], loaded_models: ["ggml-large-v3-turbo-q5_0"],
    status_detail: "ggml-large-v3-turbo-q5_0", install_hint: "winget install ggml.whispercpp", homepage: "https://github.com/ggml-org/whisper.cpp" }),
  rt({ id: "piper", name: "Piper", vendor: "Rhasspy", kind: "sidecar", status: "available", api: "none", formats: ["piper"],
    description: "Voce neurale locale (TTS) per leggere le risposte: AIW avvia l'eseguibile per ogni frase, con una voce .onnx scaricata da Modelli.", accelerators: ["cpu"], capabilities: ["tts"],
    install_hint: "Scarica la release di Piper da https://github.com/rhasspy/piper/releases", homepage: "https://github.com/rhasspy/piper" }),
  rt({ id: "lmstudio", name: "LM Studio", vendor: "Element Labs", kind: "sidecar", status: "stopped", endpoint: "http://127.0.0.1:1234/v1",
    description: "Server locale di LM Studio (`lms server start`).", accelerators: ["cuda", "vulkan", "metal", "cpu"], formats: ["gguf", "mlx"], capabilities: ["chat", "tools", "vision", "embeddings"] }),
  rt({ id: "vllm", name: "vLLM", vendor: "vLLM project", kind: "sidecar", status: "installable", platforms: ["linux"],
    description: "Serving ad alto throughput (PagedAttention). Su Windows via WSL2.", accelerators: ["cuda", "rocm", "sycl"], formats: ["safetensors", "awq", "gptq", "fp8"], capabilities: ["chat", "tools", "vision", "embeddings", "grammar"],
    settings_schema: vllmSchema, settings: { gpu_memory_utilization: 0.9, max_model_len: 32768, tensor_parallel_size: 1, quantization: "auto" }, install_hint: "wsl -- pip install vllm", install_auto: true /* demo of the one-click install */ }),
  rt({ id: "sglang", name: "SGLang", vendor: "LMSYS", kind: "sidecar", status: "installable", platforms: ["linux"], accelerators: ["cuda", "rocm"], formats: ["safetensors"], description: "Runtime con RadixAttention, ottimo per agenti multi-turn.", install_hint: "pip install sglang[all]" }),
  rt({ id: "trtllm", name: "TensorRT-LLM", vendor: "NVIDIA", kind: "sidecar", status: "installable", platforms: ["linux", "windows"], accelerators: ["cuda"], formats: ["trt", "safetensors"], description: "`trtllm-serve` con engine compilati per GPU NVIDIA." }),
  rt({ id: "foundry-local", name: "Foundry Local", vendor: "Microsoft", kind: "sidecar", status: "available", platforms: ["windows", "macos"], accelerators: ["npu", "directml", "cuda", "cpu"], formats: ["onnx"],
    description: "Runtime LLM di Windows: sceglie in automatico CPU, GPU o NPU.", endpoint: "http://127.0.0.1:5273/v1" }),
  rt({ id: "windows-ml", name: "Windows ML / ORT GenAI", vendor: "Microsoft", kind: "embedded", status: "available", platforms: ["windows"], accelerators: ["directml", "npu", "cuda"], formats: ["onnx"], api: "native",
    description: "ONNX Runtime di sistema con execution provider scaricati per NPU e GPU." }),
  rt({ id: "lemonade", name: "Lemonade Server", vendor: "AMD", kind: "sidecar", status: "unsupported", platforms: ["windows", "linux"], accelerators: ["npu", "vulkan", "rocm"], formats: ["onnx", "gguf"],
    description: "NPU + iGPU Ryzen AI. Nessuna NPU AMD rilevata su questo PC." }),
  rt({ id: "mlx-lm", name: "MLX LM", vendor: "Apple ML Research", kind: "sidecar", status: "unsupported", platforms: ["macos"], accelerators: ["metal"], formats: ["mlx"], adapter_format: "mlx", description: "Solo Apple Silicon. Disponibile sul Mac accoppiato." }),
  rt({ id: "apple-fm", name: "Apple Foundation Models", vendor: "Apple", kind: "system", status: "unsupported", platforms: ["macos", "ios"], accelerators: ["npu", "metal"], formats: ["system"], api: "native",
    description: "Modello di sistema on-device (iOS/macOS 26+), con guided generation e tool.", settings_schema: systemSchema }),
  rt({ id: "gemini-nano", name: "Gemini Nano (ML Kit GenAI)", vendor: "Google", kind: "system", status: "unsupported", platforms: ["android"], accelerators: ["npu"], formats: ["system"], api: "native",
    description: "Prompt API su AICore. Solo con l'app in primo piano.", settings_schema: systemSchema }),
  rt({ id: "litert-lm", name: "LiteRT-LM", vendor: "Google AI Edge", kind: "embedded", status: "available", platforms: ["android", "ios", "windows", "macos", "linux"], accelerators: ["cpu", "opencl", "npu"], formats: ["litertlm"], api: "native",
    description: "Gemma 3n/4 on-device con MTP e function calling." }),
  rt({ id: "remote-macbook", name: "MacBook Pro M4 · mlx-lm", vendor: "Device accoppiato", kind: "remote", status: "running", platforms: ["macos"], accelerators: ["metal"], formats: ["mlx"],
    endpoint: "http://192.168.1.42:7777/v1", description: "Runtime condiviso da un altro device AIW in LAN.", loaded_models: ["gemma-4-27b-it-4bit"] }),
  rt({ id: "remote-openai", name: "Endpoint compatibile OpenAI", vendor: "Personalizzato", kind: "remote", status: "stopped", description: "Qualsiasi server OpenAI-compatibile in LAN (vLLM, LocalAI, KoboldCpp…)." }),
];

export const system: SystemInfo = {
  version: "0.5.0-beta.7",
  mode: "demo",
  data_dir: "C:\\Users\\demo\\.aiw",
  workspace_root: "C:\\Users\\demo\\Workspace",
  // The demo machine is a Windows PC: the shell is contained by the low-integrity token (no network or read isolation) and
  // MCP servers cannot be isolated.
  shell_sandbox: "windows-low-integrity",
  mcp_sandbox: false,
  uptime_s: 4920,
  pairing_code: "482 913",
  device: {
    id: "dev_studio",
    name: "Studio-PC",
    os: "windows",
    os_version: "Windows 11 25H2",
    arch: "x86_64",
    form_factor: "desktop",
    cpu: { brand: "AMD Ryzen 9 9950X", cores: 16, threads: 32, features: ["avx2", "avx512f", "avx512_bf16", "fma"] },
    memory_mb: 65536,
    available_memory_mb: 38912,
    accelerators: [
      { kind: "cuda", name: "NVIDIA GeForce RTX 4090", vendor: "NVIDIA", memory_mb: 24564, driver: "580.88 · CUDA 13.0" },
      { kind: "vulkan", name: "NVIDIA GeForce RTX 4090", vendor: "NVIDIA", memory_mb: 24564 },
      { kind: "directml", name: "DirectML 1.15", vendor: "Microsoft" },
      { kind: "cpu", name: "AMD Ryzen 9 9950X", vendor: "AMD", details: "AVX-512" },
    ],
    tier: "xl",
  },
  peers: [
    { id: "dev_mbp", name: "MacBook Pro M4 Max", os: "macos", address: "192.168.1.42", status: "online", shared_runtimes: ["mlx-lm", "apple-fm"],
      hardware: { arch: "aarch64", tier: "xl", memory_mb: 131072, vram_mb: 131072, accelerators: ["metal", "cpu"], unified_memory: true } },
    { id: "dev_iphone", name: "iPhone 17 Pro", os: "ios", address: "192.168.1.57", status: "online", shared_runtimes: ["apple-fm"] },
    { id: "dev_pixel", name: "Pixel 10", os: "android", address: "—", status: "offline", shared_runtimes: ["gemini-nano", "litert-lm"] },
  ],
};

type ModelSeed = Omit<LocalModel, "origin" | "removable" | "runtime_compat" | "fit" | "fit_detail">;

/** Compatibility of a model/variant with the runtimes of this (demo) device, available runtimes first. */
export function compat(ids: string[]): RuntimeCompat[] {
  const out: RuntimeCompat[] = [];
  for (const id of ids) {
    const r = runtimes.find((x) => x.id === id);
    if (r) out.push({ id: r.id, name: r.name, available: r.status !== "installable" && r.status !== "unsupported", reason: r.status === "installable" ? "da installare" : r.status === "unsupported" ? `solo ${r.platforms.join(" / ")}` : "" });
  }
  return out.sort((a, b) => Number(b.available) - Number(a.available));
}

/** Runtimes of the demo device that can open a given format. */
export const compatForFormat = (format: string): RuntimeCompat[] => compat(runtimes.filter((r) => r.formats.includes(format)).map((r) => r.id));

/** Fit of `size` bytes on the demo device (24 GB VRAM, 64 GB RAM): same rule as the real catalog (size x 1.1 + 700 MB). */
export function fitFor(size: number): { fit: Fit; detail: string } {
  const need = (size / 1024 ** 2) * 1.1 + 700;
  const vram = system.device.accelerators.find((a) => a.kind === "cuda")?.memory_mb ?? 0;
  const ram = system.device.memory_mb;
  if (need <= vram * 0.6) return { fit: "great", detail: "Tutti i layer su GPU con contesto ampio" };
  if (need <= vram * 0.95) return { fit: "ok", detail: "Sta in VRAM con contesto ridotto" };
  if (need <= vram + ram * 0.5) return { fit: "tight", detail: "Offload parziale su CPU: più lento" };
  return { fit: "no", detail: `Servono circa ${Math.round(need / 1024)} GB di memoria: usa un device remoto` };
}

const modelSeeds: ModelSeed[] = [
  { id: "qwen3.5-35b-a3b-q4_k_m", name: "Qwen3.5 35B-A3B Instruct", format: "gguf", quant: "Q4_K_M", size_bytes: 21.2 * GB, params_b: 35, context_length: 262144, arch: "qwen3_5moe", capabilities: ["chat", "tools", "reasoning"], runtimes: ["llama-server", "llama-embedded", "ollama", "lmstudio"], source: "hf:Qwen/Qwen3.5-35B-A3B-GGUF", path: "models\\qwen3.5-35b-a3b-q4_k_m.gguf" },
  { id: "holo2-8b-q5_k_m", name: "Holo2 8B (computer-use)", format: "gguf", quant: "Q5_K_M", size_bytes: 5.8 * GB, params_b: 8, context_length: 65536, arch: "qwen3vl", capabilities: ["chat", "vision", "gui-grounding"], runtimes: ["llama-server", "lmstudio"], source: "hf:Hcompany/Holo2-8B-GGUF" },
  { id: "gemma-4-e4b-q4_k_m", name: "Gemma 4 E4B", format: "gguf", quant: "Q4_K_M", size_bytes: 3.1 * GB, params_b: 8, context_length: 131072, arch: "gemma4", capabilities: ["chat", "tools", "vision", "audio"], runtimes: ["llama-server", "llama-embedded", "ollama"], source: "hf:google/gemma-4-E4B-it-GGUF" },
  { id: "qwen3-embedding-0.6b-q8_0", name: "Qwen3 Embedding 0.6B", format: "gguf", quant: "Q8_0", size_bytes: 0.64 * GB, params_b: 0.6, context_length: 32768, arch: "qwen3", capabilities: ["embeddings"], runtimes: ["llama-server", "llama-embedded"] },
  { id: "ggml-large-v3-turbo-q5_0", name: "Whisper large-v3-turbo", format: "ggml", quant: "Q5_0", size_bytes: 0.53 * GB, arch: "whisper", capabilities: ["stt"], runtimes: ["whisper-server"], path: "models\\ggml-large-v3-turbo-q5_0.bin" },
  { id: "phi-4-mini-onnx-dml", name: "Phi-4 mini (ONNX DirectML)", format: "onnx", quant: "int4", size_bytes: 2.4 * GB, params_b: 3.8, context_length: 131072, capabilities: ["chat", "tools"], runtimes: ["foundry-local", "windows-ml"] },
]
export const models: LocalModel[] = [
  ...modelSeeds.map<LocalModel>((m) => ({ ...m, path: m.path ?? `${system.data_dir}\\models\\${m.id}.${m.format === "onnx" ? "onnx" : "gguf"}`, origin: "aiw", removable: true, runtime_compat: compat(m.runtimes), ...(() => { const f = fitFor(m.size_bytes); return { fit: f.fit, fit_detail: f.detail }; })() })),
  { id: "ollama:llama3.2:3b", name: "Llama 3.2 3B (Ollama)", format: "gguf", quant: "Q4_K_M", size_bytes: 1.9 * GB, params_b: 3, context_length: 131072, arch: "llama", capabilities: ["chat", "tools"], runtimes: ["llama-server", "ollama"], source: "ollama:library/llama3.2:3b", origin: "ollama", removable: false, runtime_compat: compat(["llama-server", "ollama"]), fit: "great", fit_detail: "Tutti i layer su GPU con contesto ampio" },
  { id: "lmstudio:bartowski/Mistral-7B-Instruct-v0.3-GGUF/Mistral-7B-Instruct-v0.3-Q5_K_M.gguf", name: "Mistral 7B Instruct v0.3 (LM Studio)", format: "gguf", quant: "Q5_K_M", size_bytes: 4.8 * GB, params_b: 7, context_length: 32768, arch: "llama", capabilities: ["chat", "tools"], runtimes: ["llama-server", "lmstudio"], source: "lmstudio:bartowski", origin: "lmstudio", removable: false, runtime_compat: compat(["llama-server", "lmstudio"]), fit: "great", fit_detail: "Tutti i layer su GPU con contesto ampio" },
  { id: "hf-cache:Qwen/Qwen3-8B-GGUF/Qwen3-8B-Q4_K_M.gguf", name: "Qwen3 8B (cache Hugging Face)", path: "C:\\Users\\demo\\.cache\\huggingface\\hub\\models--Qwen--Qwen3-8B-GGUF\\snapshots\\a1b2c3\\Qwen3-8B-Q4_K_M.gguf", format: "gguf", quant: "Q4_K_M", size_bytes: 5.0 * GB, params_b: 8, context_length: 40960, arch: "qwen3", capabilities: ["chat", "tools", "reasoning"], runtimes: ["llama-server", "llama-embedded", "ollama", "lmstudio"], source: "hf-cache:Qwen/Qwen3-8B-GGUF", origin: "hf-cache", removable: false, runtime_compat: compat(["llama-server", "llama-embedded", "ollama", "lmstudio"]), fit: "great", fit_detail: "Tutti i layer su GPU con contesto ampio" },
  { id: "qwen3-0.6b-q8_0", name: "Qwen3 0.6B", path: `${system.data_dir}\\models\\qwen3-0.6b-q8_0.gguf`, format: "gguf", quant: "Q8_0", size_bytes: 0.64 * GB, params_b: 0.6, context_length: 40960, arch: "qwen3", capabilities: ["chat"], runtimes: ["llama-server", "llama-embedded"], origin: "manual", removable: true, runtime_compat: compat(["llama-server", "llama-embedded"]), fit: "great", fit_detail: "Tutti i layer su GPU con contesto ampio" },
];

type CatalogSeed = Omit<CatalogModel, "runtime_compat" | "downloadable">;
const catalogSeeds: CatalogSeed[] = [
  { id: "qwen3.5-35b-a3b", name: "Qwen3.5 35B-A3B", family: "Qwen3.5", publisher: "Alibaba", description: "MoE con 3B attivi: veloce come un 3B, qualità vicina a un 30B denso. Ottimo per agenti e tool calling.", params_b: 35, active_params_b: 3, format: "gguf", quant: "Q4_K_M", size_bytes: 21.2 * GB, context_length: 262144, capabilities: ["chat", "tools", "reasoning"], license: "Apache-2.0", source: { kind: "hf", repo: "Qwen/Qwen3.5-35B-A3B-GGUF", file: "Qwen3.5-35B-A3B-Q4_K_M.gguf" }, min_memory_mb: 24000, tags: ["agent", "coding"], fit: "great", fit_detail: "24 GB VRAM: tutti i layer su GPU con contesto da 32K", installed: true },
  { id: "holo3-35b-a3b", name: "Holo3 35B-A3B", family: "Holo3", publisher: "H Company", description: "Tra i migliori open-weight su OSWorld per il computer-use.", params_b: 35, active_params_b: 3, format: "gguf", quant: "Q4_K_M", size_bytes: 21.6 * GB, context_length: 65536, capabilities: ["chat", "vision", "gui-grounding", "tools"], license: "Apache-2.0", source: { kind: "hf", repo: "Hcompany/Holo3-35B-A3B-GGUF", file: "Holo3-35B-A3B-Q4_K_M.gguf" }, min_memory_mb: 25000, tags: ["computer-use"], fit: "ok", fit_detail: "Sta in VRAM con contesto ridotto (16K)", installed: false },
  { id: "gemma-4-27b", name: "Gemma 4 27B", family: "Gemma 4", publisher: "Google", description: "Multimodale (testo, immagini, audio), 128K di contesto.", params_b: 27, format: "gguf", quant: "Q4_K_M", size_bytes: 16.5 * GB, context_length: 131072, capabilities: ["chat", "vision", "tools"], license: "Gemma", source: { kind: "hf", repo: "google/gemma-4-27b-it-GGUF", file: "gemma-4-27b-it-Q4_K_M.gguf" }, min_memory_mb: 19000, tags: ["general", "vision"], fit: "great", fit_detail: "Tutto su GPU", installed: false },
  { id: "llama-3.3-70b", name: "Llama 3.3 70B Instruct", family: "Llama 3.3", publisher: "Meta", description: "Denso 70B, forte nel ragionamento generale.", params_b: 70, format: "gguf", quant: "Q4_K_M", size_bytes: 42.5 * GB, context_length: 131072, capabilities: ["chat", "tools"], license: "Llama 3.3", source: { kind: "hf", repo: "bartowski/Llama-3.3-70B-Instruct-GGUF", file: "Llama-3.3-70B-Instruct-Q4_K_M.gguf" }, min_memory_mb: 46000, tags: ["general"], fit: "tight", fit_detail: "Offload parziale su CPU: circa 9 tok/s", installed: false },
  { id: "qwen3-coder-480b", name: "Qwen3 Coder 480B-A35B", family: "Qwen3", publisher: "Alibaba", description: "Agente di coding di frontiera, richiede un server multi-GPU.", params_b: 480, active_params_b: 35, format: "gguf", quant: "Q4_K_M", size_bytes: 290 * GB, context_length: 262144, capabilities: ["chat", "tools"], license: "Apache-2.0", source: { kind: "hf", repo: "Qwen/Qwen3-Coder-480B-A35B-Instruct-GGUF", file: "Q4_K_M" }, min_memory_mb: 300000, tags: ["coding"], fit: "no", fit_detail: "Servono circa 300 GB di memoria: usa un device remoto", installed: false },
  { id: "gemma-4-e2b-litert", name: "Gemma 4 E2B (LiteRT-LM)", family: "Gemma 4", publisher: "Google", description: "Per telefoni: NPU e GPU con LiteRT-LM.", params_b: 5, active_params_b: 2, format: "litertlm", quant: "int4", size_bytes: 1.4 * GB, context_length: 32768, capabilities: ["chat", "vision", "audio", "tools"], license: "Gemma", source: { kind: "hf", repo: "litert-community/gemma-4-E2B-it-litert-lm", file: "gemma-4-E2B-it.litertlm" }, min_memory_mb: 3000, tags: ["mobile"], fit: "great", fit_detail: "Pensato per mobile; qui gira su CPU", installed: false },
  { id: "qwen3-4b-mlx", name: "Qwen3 4B (MLX)", family: "Qwen3", publisher: "mlx-community", description: "Repository MLX a 4 bit per Apple Silicon: si scarica intero e gira solo con MLX su macOS.", params_b: 4, format: "mlx", quant: "4bit", size_bytes: 2.3 * GB, context_length: 40960, capabilities: ["chat", "tools"], license: "Apache-2.0", source: { kind: "hf", repo: "mlx-community/Qwen3-4B-4bit", file: "" }, min_memory_mb: 4000, tags: ["mlx", "apple-silicon"], fit: "no", fit_detail: "Richiede macOS su Apple Silicon: questo PC non è compatibile", installed: false },
  { id: "whisper-small-q8", name: "Whisper Small", family: "Whisper", publisher: "OpenAI (ggml: ggml-org)", description: "Buon equilibrio tra qualità e velocità in italiano su CPU.", params_b: 0.244, format: "ggml", quant: "Q8_0", size_bytes: 0.25 * GB, context_length: 448, capabilities: ["stt"], license: "MIT", source: { kind: "hf", repo: "ggerganov/whisper.cpp", file: "ggml-small-q8_0.bin" }, min_memory_mb: 900, tags: ["voce"], fit: "great", fit_detail: "Tutto su GPU", installed: false },
  { id: "piper-it-paola-medium", name: "Voce Paola (italiano)", family: "Piper", publisher: "Rhasspy (rhasspy/piper-voices)", description: "Voce neurale italiana per leggere le risposte (Piper). Dimensione indicativa.", params_b: 0, format: "piper", quant: "medium", size_bytes: 0.063 * GB, context_length: 0, capabilities: ["tts"], license: "vedi la scheda della voce", source: { kind: "hf", repo: "rhasspy/piper-voices", file: "it/it_IT/paola/medium/it_IT-paola-medium.onnx" }, min_memory_mb: 300, tags: ["voce"], fit: "great", fit_detail: "Gira sulla CPU", installed: false },
  { id: "piper-en-lessac-medium", name: "Voce Lessac (inglese)", family: "Piper", publisher: "Rhasspy (rhasspy/piper-voices)", description: "Voce neurale inglese per leggere le risposte (Piper). Dimensione indicativa.", params_b: 0, format: "piper", quant: "medium", size_bytes: 0.063 * GB, context_length: 0, capabilities: ["tts"], license: "vedi la scheda della voce", source: { kind: "hf", repo: "rhasspy/piper-voices", file: "en/en_US/lessac/medium/en_US-lessac-medium.onnx" }, min_memory_mb: 300, tags: ["voce"], fit: "great", fit_detail: "Gira sulla CPU", installed: false },
  { id: "apple-fm-system", name: "Apple Foundation Model (di sistema)", family: "Apple", publisher: "Apple", description: "Già presente su iPhone e Mac con Apple Intelligence: non serve scaricarlo.", params_b: 3, format: "system", quant: "2-bit QAT", size_bytes: 0, context_length: 4096, capabilities: ["chat", "tools", "guided-generation"], license: "Apple", source: { kind: "system", runtime: "apple-fm" }, min_memory_mb: 0, tags: ["system", "mobile"], fit: "no", fit_detail: "Solo su dispositivi Apple", installed: false },
]
export const catalog: CatalogModel[] = catalogSeeds.map<CatalogModel>((c) => ({ ...c, runtime_compat: compatForFormat(c.format), downloadable: c.source.kind !== "system" }));

export const profiles: InferenceProfile[] = [
  { id: "balanced", name: "Bilanciato (GPU)", description: "Qwen3.5 35B-A3B su llama.cpp CUDA", runtime_id: "llama-server", model: "qwen3.5-35b-a3b-q4_k_m", params: { temperature: 0.6, top_p: 0.95, max_tokens: 4096, context: 32768 } },
  { id: "fast-local", name: "Veloce", description: "Gemma 4 E4B, latenza minima", runtime_id: "llama-server", model: "gemma-4-e4b-q4_k_m", params: { temperature: 0.4, max_tokens: 2048, context: 16384 } },
  { id: "vision-cu", name: "Computer-use", description: "Holo2 8B, grounding GUI", runtime_id: "llama-server", model: "holo2-8b-q5_k_m", params: { temperature: 0.2, max_tokens: 1024, context: 16384 } },
  { id: "mac-mlx", name: "Mac remoto (MLX)", description: "Gemma 4 27B sul MacBook in LAN", runtime_id: "remote-macbook", model: "gemma-4-27b-it-4bit", params: { temperature: 0.7 } },
];

export const agents: AgentProfile[] = [
  { id: "operator", name: "Operatore", description: "Task operativi su file, cartelle, documenti e app.", inference_profile: "balanced", system_prompt: "Sei un operatore preciso. Pianifica, chiedi conferma prima delle azioni distruttive e riporta cosa hai fatto.", tools: ["read_file", "write_file", "edit_file", "list_dir", "glob", "grep", "shell", "web_fetch", "web_search", "todo", "agent", "kb_search"], mode: "ask", sandbox: "workspace-write", max_turns: 30 },
  { id: "coder", name: "Coder", description: "Coding agent sul workspace corrente.", inference_profile: "balanced", system_prompt: "Sei un ingegnere software. Leggi prima di modificare e verifica con i test.", tools: ["*"], mode: "auto-edit", sandbox: "workspace-write", max_turns: 60 },
  { id: "browser", name: "Navigatore", description: "Ricerca e azioni nel browser via CDP.", inference_profile: "fast-local", system_prompt: "Usi il browser per raccogliere informazioni e completare form.", tools: ["browser_navigate", "browser_snapshot", "browser_click", "browser_type", "browser_scroll", "browser_back", "browser_press", "browser_wait", "browser_screenshot", "browser_tabs", "browser_tab_new", "browser_tab_select", "browser_select", "browser_hover", "browser_downloads", "browser_console", "web_fetch", "web_search", "todo"], mode: "ask", sandbox: "read-only", max_turns: 40 },
  { id: "desktop", name: "Pilota desktop", description: "Computer-use: vede lo schermo e usa mouse e tastiera.", inference_profile: "vision-cu", system_prompt: "Controlli il desktop tramite screenshot e azioni. Chiedi conferma prima di inviare, pagare o cancellare.", tools: ["computer_screenshot", "computer_click", "computer_type", "computer_key", "computer_scroll", "todo"], mode: "ask", sandbox: "read-only", max_turns: 50 },
  { id: "teacher", name: "Insegnante", description: "Impara da ciò che gli racconti: salva, corregge e dimentica i ricordi.", inference_profile: "fast-local", system_prompt: "Sei l'Insegnante: aiuti l'utente a insegnare all'assistente ciò che deve ricordare.", tools: ["memory_save", "memory_search", "memory_update", "memory_forget", "render_ui", "todo"], mode: "auto", sandbox: "read-only", max_turns: 12 },
  { id: "planner", name: "Pianificatore", description: "Solo lettura: analizza e propone piani.", inference_profile: "mac-mlx", system_prompt: "Analizzi senza modificare nulla e proponi piani numerati.", tools: ["read_file", "list_dir", "glob", "grep", "web_fetch", "web_search", "todo", "kb_search"], mode: "plan", sandbox: "read-only", max_turns: 20 },
];

/** Teams of the demo: a supervisor with two members, and a chain. */
/** Remote agents (A2A). The token of a stored one is kept here and masked by the mock API. */
export const remoteAgents: RemoteAgent[] = [
  { id: "studio", name: "PC dello studio", url: "http://192.168.1.20:7777", token: "demo-token", skill: "operator", description: "Il portatile dello studio, con i documenti del cliente." },
];

export const teams: TeamDef[] = [
  {
    id: "revisione", name: "Revisione del codice", description: "Il Coder implementa, il Pianificatore rivede: il supervisore li coordina.",
    strategy: "supervisor", supervisor: "operator",
    members: [{ agent: "coder", role: "Scrive e verifica le modifiche" }, { agent: "planner", role: "Rivede il piano e i rischi, senza modificare" }],
    budget: { max_turns: 40 },
  },
  {
    id: "ricerca", name: "Ricerca e sintesi", description: "Il Navigatore raccoglie, il Pianificatore riassume.",
    strategy: "pipeline", members: [{ agent: "browser", role: "Raccoglie le fonti" }, { agent: "planner", role: "Riassume in un piano" }],
  },
];

const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();

export const sessions: Session[] = [
  { id: "ses_pdf", title: "Riordino PDF in Downloads", agent_id: "operator", created_at: ago(95), updated_at: ago(4), last_status: "succeeded" },
  { id: "ses_cv", title: "Confronto offerte fornitori", agent_id: "browser", created_at: ago(300), updated_at: ago(75), last_status: "succeeded" },
  { id: "ses_ui", title: "Prenotazione sala riunioni", agent_id: "operator", created_at: ago(1500), updated_at: ago(190), last_status: "succeeded" },
  {
    id: "ses_bug", title: "Fix test parser GGUF", agent_id: "coder", created_at: ago(3000), updated_at: ago(1450), last_status: "failed",
    worktree: { path: "~/.aiw/worktrees/ai-workspace-local-7c1e9a20", root: "~/.aiw/worktrees/ai-workspace-local-7c1e9a20", branch: "aiw/7c1e9a20", base: "9c39159", repo: "~/code/ai-workspace-local" },
  },
];

export const MOCK_DIFF = `diff --git a/crates/aiw-models/src/gguf.rs b/crates/aiw-models/src/gguf.rs
--- a/crates/aiw-models/src/gguf.rs
+++ b/crates/aiw-models/src/gguf.rs
@@ -88,7 +88,8 @@ fn read_header(r: &mut impl Read) -> Result<Header> {
     let version = read_u32(r)?;
-    let tensors = read_u32(r)? as u64;
-    let kv = read_u32(r)? as u64;
+    // GGUF v2+ stores the counts as u64.
+    let tensors = read_u64(r)?;
+    let kv = read_u64(r)?;
     Ok(Header { version, tensors, kv })
 }
diff --git a/crates/aiw-models/tests/header.rs b/crates/aiw-models/tests/header.rs
new file mode 100644
--- /dev/null
+++ b/crates/aiw-models/tests/header.rs
@@ -0,0 +1,6 @@
+#[test]
+fn reads_v3_header() {
+    let h = aiw_models::gguf::read_header(&mut &V3[..]).unwrap();
+    assert_eq!(h.version, 3);
+}
`;

export const appliedWorktrees = new Set<string>();

/** Runs whose files were put back (the demo checkpoint of the coder session is then empty). */
export const rewoundRuns = new Set<string>();

/**
 * A tiny PNG (256x160) standing for the screenshot of a web page: the scripted browser run sends it as the
 * `aiw.image` event of `browser_screenshot` (`value.data`: base64 without the `data:` prefix).
 */
export const DEMO_SCREENSHOT_PNG = "iVBORw0KGgoAAAANSUhEUgAAAQAAAACgCAIAAABseyVrAAACXUlEQVR42u3bQUoCURzA4TlHCz2C+7yBqEgXcNUqWrXoDhIELWclRItu0cpo0Q0ExRuU7m1hSAwtFId57/m+x7cTQt/4o970n2KxmkO2CluAAEAAzdpa1p8lAEsAcQTwNhpU7F9avlxW7F8aP19XCMASwBEBlNPyzPhaC0AAlgAEYAlAAJYAHIItt0EFYAnAJbeyDgDMAoEAIGAAF602ZEsACAAEAAIAAYAAQACQdwDfr90gXBgEwEEmTw8nSvdnCgABCEAAAhCAAAQgAHAbFAQAAgABgABAACAAEAD8BrDefEG2BIAAQAAgABAACAAEAAIAAYAAQACQWwBGpprh6ygAASAAASAAASAAASAAASAAASAAASAAAeA/wSAAEAAIAAQAAoDaAyinJdROAAhAAAhAAAjAIRgEAAIAAYBDMI6qAkAAAkAAAkAADsEgABAACAAEAAIAAYAAQAAgABAACAAEAEcHsFjNITkCQAACQAACQAD1BNC76keomQ2N//3H+d5C7ZsABCAAAQhAAAIQgAAcgnEIFgACEAACEAACMAyHYTgQAAgABAACAAH85+buNrhQmxX/Z+w8DmsR874JQAACEIAABCAAAQjAIRgEAAIAAYAAQAAgADg5gI/PGSTHbwAQAAgABAACgNSnQUPNh8b/ieLcT+PQAhCAAAQgAAEIQAAeiAF3gUAAIAAQAAgABAAeiAEPxIA/gUAAIAAQAJgGDTQfahrUNKgABCAAAQhAAAIQgAdiwF0gEAAIAM4xgPf7IjkuLQIAAUCGAXgcBIdgEAAIAAQAAgABwM4PimibFSc3vIQAAAAASUVORK5CYII=";

export const BOOKING_UI = {
  surfaceId: "prenotazione",
  components: [
    { id: "root", component: "Card", child: "col" },
    { id: "col", component: "Column", children: ["titolo", "sala", "quando", "persone", "note", "proiettore", "azioni"] },
    { id: "titolo", component: "Text", text: "Prenota una sala", variant: "h3" },
    { id: "sala", component: "ChoicePicker", label: "Sala", options: [{ label: "Galilei (8 posti)", value: "galilei" }, { label: "Volta (4 posti)", value: "volta" }, { label: "Fermi (12 posti)", value: "fermi" }], value: { path: "/sala" }, displayStyle: "chips" },
    { id: "quando", component: "DateTimeInput", label: "Quando", value: { path: "/quando" }, enableDate: true, enableTime: true },
    { id: "persone", component: "Slider", label: "Partecipanti", min: 1, max: 12, value: { path: "/persone" } },
    { id: "note", component: "TextField", label: "Ordine del giorno", value: { path: "/note" }, variant: "longText" },
    { id: "proiettore", component: "CheckBox", label: "Serve il proiettore", value: { path: "/proiettore" } },
    { id: "azioni", component: "Row", justify: "end", children: ["annulla", "conferma"] },
    { id: "annulla", component: "Button", label: "Annulla", variant: "borderless", action: { event: { name: "annulla" } } },
    { id: "conferma", component: "Button", label: "Prenota", variant: "primary", action: { event: { name: "prenota", context: { sala: { path: "/sala" }, quando: { path: "/quando" }, persone: { path: "/persone" }, proiettore: { path: "/proiettore" } } } },
      checks: [{ condition: { call: "required", args: { value: { path: "/quando" } } }, message: "Scegli data e ora" }] },
  ],
  data: { sala: ["galilei"], quando: "2026-11-05T10:00", persone: 6, note: "", proiettore: true },
};

export const sessionMessages: Record<string, ChatMessage[]> = {
  ses_ui: [
    { id: "u1", role: "user", content: "Prenotami una sala per la riunione di giovedì con il team.", created_at: now() },
    { id: "u2", role: "assistant", content: "", tool_calls: [{ id: "c_ui", name: "render_ui", arguments: JSON.stringify(BOOKING_UI) }], created_at: now() },
    { id: "u3", role: "tool", tool_call_id: "c_ui", name: "render_ui", content: "Interfaccia `prenotazione` mostrata all'utente (12 componenti).", created_at: now() },
    { id: "u4", role: "assistant", content: "Ho preparato il modulo: controlla sala, orario e partecipanti, poi premi **Prenota**.", created_at: now() },
  ],
  ses_pdf: [
    { id: "h1", role: "user", content: "Quanti PDF ci sono in Downloads e quanto pesano in totale?", created_at: now() },
    { id: "h2", role: "assistant", content: "", reasoning: "Basta elencare i .pdf della cartella e sommare le dimensioni.", tool_calls: [{ id: "c_h1", name: "list_dir", arguments: JSON.stringify({ path: "~/Downloads", pattern: "*.pdf" }) }], created_at: now() },
    { id: "h3", role: "tool", tool_call_id: "c_h1", name: "list_dir", content: "7 file · 19.2 MB totali (il più grande: progetto-casa-2026.pdf, 12.8 MB)", created_at: now() },
    { id: "h4", role: "assistant", content: "In `Downloads` ci sono **7 PDF** per **19,2 MB** in totale. Il più pesante è `progetto-casa-2026.pdf` (12,8 MB). Vuoi che li ordini per anno?", created_at: now() },
  ],
  ses_cv: [
    { id: "m1", role: "user", content: "Apri i tre preventivi nella cartella Fornitori e fammi una tabella con prezzo, tempi e penali.", created_at: now() },
    { id: "m2", role: "assistant", content: "Ho letto i tre PDF. **Rossi Srl** costa meno (12.400 €) ma ha tempi più lunghi (45 gg). **Nordtech** è il più rapido (20 gg) e prevede penali dello 0,5% al giorno.\n\n| Fornitore | Prezzo | Tempi | Penali |\n|---|---|---|---|\n| Rossi Srl | 12.400 € | 45 gg | nessuna |\n| Nordtech | 14.950 € | 20 gg | 0,5%/giorno |\n| Alfa Impianti | 13.700 € | 30 gg | 0,2%/giorno |", created_at: now() },
  ],
  // A run that died with the runtime: the history ends with the persisted error (system message `aiw.run_error`).
  ses_bug: [
    { id: "m3", role: "user", content: "Il test `gguf::tests::reads_header` fallisce, sistemalo.", created_at: now() },
    { id: "m4", role: "assistant", content: "Eseguo il test per vedere l'errore.", tool_calls: [{ id: "c_bug", name: "shell", arguments: JSON.stringify({ command: "cargo test -p aiw-models gguf::tests::reads_header" }) }], created_at: now() },
    { id: "m5", role: "tool", tool_call_id: "c_bug", name: "shell", content: "ERRORE: exit 101\nthread 'gguf::tests::reads_header' panicked at crates/aiw-models/src/gguf.rs:88:\nassertion `left == right` failed: magic GGUF non riconosciuto", created_at: now() },
    { id: "m6", role: "system", name: RUN_ERROR_MESSAGE, content: "Errore del runtime: il runtime llama-server si è chiuso durante la richiesta (memoria video esaurita, codice 137). Riavvialo dalla sezione Runtime e riprova.", created_at: now() },
  ],
};

export const tools: ToolInfo[] = [
  ["read_file", "Legge un file di testo nel workspace", "read"], ["list_dir", "Elenca una cartella", "read"], ["glob", "Trova file per pattern", "read"], ["grep", "Cerca testo con regex", "read"],
  ["write_file", "Crea o sovrascrive un file", "write"], ["edit_file", "Sostituzione esatta in un file", "write"], ["shell", "Esegue un comando nella sandbox", "exec"],
  ["web_fetch", "Scarica una pagina e la converte in testo", "network"], ["web_search", "Cerca sul web e restituisce titolo, indirizzo e anteprima dei risultati", "network"], ["browser_navigate", "Apre un URL nel browser controllato", "network"], ["browser_snapshot", "Snapshot della pagina: elementi numerati, albero di accessibilità (format=ax) o testo leggibile (format=text)", "read"],
  ["browser_click", "Clicca un elemento dello snapshot (anche caselle e opzioni radio)", "computer"], ["browser_type", "Scrive in un campo", "computer"], ["browser_screenshot", "Screenshot della pagina", "read"],
  ["browser_scroll", "Scorre la pagina o porta in vista un elemento", "read"], ["browser_back", "Torna alla pagina precedente", "computer"], ["browser_press", "Preme un tasto sull'elemento attivo o su un ref", "computer"],
  ["browser_wait", "Aspetta un testo, un selettore, una navigazione o la rete inattiva (massimo 30 secondi)", "read"],
  ["browser_tabs", "Elenca le schede aperte del browser", "read"], ["browser_tab_new", "Apre una nuova scheda, vuota o su un indirizzo", "network"], ["browser_tab_select", "Rende attiva una scheda", "read"], ["browser_tab_close", "Chiude una scheda", "computer"],
  ["browser_select", "Sceglie un'opzione di un menu a tendina", "computer"], ["browser_hover", "Porta il puntatore su un elemento", "computer"],
  ["browser_upload", "Sceglie un file del workspace da caricare in un campo di caricamento", "computer"],
  ["browser_downloads", "Elenca i download della sessione", "read"], ["browser_console", "Errori e avvisi recenti della console della pagina", "read"],
  ["browser_eval", "Esegue codice JavaScript nella pagina aperta (serve un permesso esplicito)", "exec"],
  ["computer_screenshot", "Screenshot del desktop", "computer"], ["computer_click", "Clic alle coordinate", "computer"], ["computer_type", "Digita testo", "computer"], ["computer_key", "Combinazione di tasti", "computer"], ["computer_scroll", "Scroll", "computer"],
].map(([name, description, risk]) => ({ name, description, risk: risk as ToolInfo["risk"], source: "builtin" as const, enabled: true, available: true, parameters: {} }));
/** The read-only calendar and mail tools: listed always, available only while their switch (`connectors.*.enabled`) is on. */
export const CONNECTOR_TOOLS: { name: string; description: string; switch: "connectors.calendar.enabled" | "connectors.mail.enabled" }[] = [
  { name: "calendar_today", description: "Elenca gli eventi di oggi dai calendari scelti dall'utente. Sola lettura.", switch: "connectors.calendar.enabled" },
  { name: "calendar_upcoming", description: "Elenca gli eventi dei prossimi giorni dai calendari scelti dall'utente. Sola lettura.", switch: "connectors.calendar.enabled" },
  { name: "mail_recent", description: "Elenca la posta recente dalle cartelle locali scelte dall'utente. Sola lettura.", switch: "connectors.mail.enabled" },
  { name: "mail_search", description: "Cerca nella posta locale scelta dall'utente. Sola lettura.", switch: "connectors.mail.enabled" },
];
tools.push(
  { name: "mcp__github__list_issues", description: "Elenca le issue di un repository", source: "mcp", server: "github", risk: "network", enabled: true, available: true, parameters: {} },
  { name: "mcp__filesystem__search_files", description: "Ricerca file (server MCP filesystem)", source: "mcp", server: "filesystem", risk: "read", enabled: false, available: true, parameters: {} },
);
/**
 * The tools of the agent loop, as `GET /api/tools` lists them after the built-in and MCP ones: `source: "internal"`,
 * always on, risk `read`, no switch (crates/aiw-agent/src/internal.rs).
 */
export const INTERNAL_TOOL_NAMES = ["todo", "agent", "skill", "render_ui", "code", "handoff", "team_state", "memory_save", "memory_search", "memory_update", "memory_forget", "kb_search"] as const;
tools.push(
  ...([
    ["todo", "Pubblica e aggiorna il piano di lavoro dell'agente: elenco di passi con il loro stato."],
    ["agent", "Delega un sotto-compito a un altro agente con contesto separato e ne restituisce la risposta finale."],
    ["skill", "Carica le istruzioni complete di una skill elencata nel prompt di sistema."],
    ["render_ui", "Mostra all'utente un'interfaccia interattiva (A2UI): moduli, scelte, riepiloghi, tabelle di card."],
    ["code", "Esegue uno script Rhai che orchestra più strumenti in un solo passo (cicli, filtri, conteggi)."],
    ["handoff", "Passa la conversazione a un altro agente: il run termina e il nuovo agente prosegue con il tuo riassunto."],
    ["team_state", "Legge e scrive lo stato condiviso tra gli agenti di un team (solo nei run di team)."],
    ["memory_save", "Salva un ricordo a lungo termine: subito se l'utente lo ha chiesto, come proposta da confermare se dedotto."],
    ["memory_search", "Cerca tra i ricordi dell'utente per parole e per significato."],
    ["memory_update", "Corregge un ricordo esistente."],
    ["memory_forget", "Dimentica un ricordo (l'utente può ripristinarlo)."],
    ["kb_search", "Cerca nei documenti dell'utente (raccolte di file) per parole e per significato."],
  ] as const).map(([name, description]): ToolInfo => ({ name, description, source: "internal", risk: "read", enabled: true, available: true, parameters: {} })),
);

/**
 * MCP servers as the engine stores them, secrets included. The transport masks the values whose name looks like a
 * secret when it lists them (`mcpServers()`), as `GET /api/mcp/servers` does.
 */
export const mcp: McpServer[] = [
  { id: "mcp_fs", name: "filesystem", transport: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "~/Workspace"], env: { LOG_LEVEL: "info" }, enabled: true, status: "connected", tools: ["search_files", "read_text_file", "directory_tree"], capabilities: { tools: true, resources: true, prompts: false } },
  { id: "mcp_gh", name: "github", transport: "http", url: "http://127.0.0.1:8931/mcp", headers: { Authorization: "Bearer ghp_demo_0000" }, enabled: true, status: "connected", tools: ["list_issues", "create_issue"], capabilities: { tools: true, resources: false, prompts: true }, signed_in: true, sampling: false },
  { id: "mcp_pw", name: "playwright", transport: "stdio", command: "npx", args: ["@playwright/mcp@latest"], enabled: false, status: "disconnected", tools: [] },
  { id: "mcp_db", name: "postgres", transport: "stdio", command: "uvx", args: ["mcp-server-postgres", "--dsn", "postgresql://localhost/app"], env: { PGPASSWORD: "demo" }, enabled: true, status: "unreachable", tools: [], error: "spawn uvx ENOENT: comando non trovato nel PATH" },
  { id: "mcp_notes", name: "appunti", transport: "http", url: "https://mcp.example.test/notes", enabled: true, status: "auth_required", tools: [], error: "il server MCP appunti richiede l'accesso: premi «Accedi» in Integrazioni" },
];

export const workflows: WorkflowDef[] = [
  { id: "ordine-ricevuto", name: "Ordine dal negozio online", description: "Il negozio chiama il webhook a ogni ordine: l'agente prepara la bozza della mail di conferma e chiede l'ok.", enabled: true,
    trigger: { kind: "webhook", secret: "3f9a61c0d2b84e57a1c9e0f4b7d26a53" }, inputs: {},
    steps: [
      { id: "bozza", kind: "agent", agent: "operator", prompt: "Scrivi in bozze/ una mail di conferma per l'ordine {{inputs.order_id}} di {{inputs.customer}}." },
      { id: "ok", kind: "approval", message: "Invio la conferma a {{inputs.customer}}?" },
    ] },
  { id: "inbox-digest", name: "Digest mattutino dei download", description: "Ogni giorno feriale alle 8 classifica i nuovi file e propone dove spostarli.", enabled: true, trigger: { kind: "cron", expr: "0 8 * * 1-5" }, inputs: { folder: "~/Downloads" },
    steps: [
      { id: "scan", kind: "tool", tool: "list_dir", args: { path: "{{inputs.folder}}" } },
      { id: "triage", kind: "agent", agent: "planner", prompt: "Classifica questi file e proponi una cartella per ciascuno:\n{{steps.scan.output}}" },
      { id: "confirm", kind: "approval", message: "Sposto i file come proposto?" },
      { id: "act", kind: "agent", agent: "operator", prompt: "Esegui il piano approvato:\n{{steps.triage.output}}" },
    ] },
  { id: "price-watch", name: "Monitor prezzi", description: "Ogni 6 ore controlla 3 pagine prodotto e avvisa se il prezzo scende.", enabled: false, trigger: { kind: "interval", seconds: 21600 }, inputs: {},
    steps: [
      { id: "fetch", kind: "agent", agent: "browser", prompt: "Leggi i prezzi correnti delle pagine in prezzi.md" },
      { id: "check", kind: "condition", if: "{{steps.fetch.output}} contains 'ribasso'", then: ["notify"] },
      { id: "notify", kind: "llm", profile: "fast-local", prompt: "Scrivi una notifica breve: {{steps.fetch.output}}" },
    ] },
  { id: "repo-health", name: "Salute repository", description: "Su richiesta: test, lint e riassunto dei problemi.", enabled: true, trigger: { kind: "manual" }, inputs: { path: "~/Workspace/ai-workspace-local" },
    steps: [
      { id: "test", kind: "tool", tool: "shell", args: { command: "cargo test --workspace", cwd: "{{inputs.path}}" } },
      { id: "report", kind: "agent", agent: "coder", prompt: "Riassumi l'esito e proponi correzioni:\n{{steps.test.output}}" },
    ] },
  { id: "rassegna-settimanale", name: "Rassegna settimanale", description: "Ogni lunedì alle 7 il team di ricerca raccoglie le fonti e scrive la sintesi.", enabled: false, trigger: { kind: "cron", expr: "0 7 * * 1" }, inputs: { tema: "agenti AI locali" },
    steps: [
      { id: "ricerca", kind: "team", team: "ricerca", prompt: "Raccogli le novità della settimana su {{inputs.tema}} e scrivi la sintesi.", retry: 1, timeout_s: 1800 },
    ] },
];

export const workflowRuns: WorkflowRun[] = [
  { id: "wfr_1", workflow_id: "inbox-digest", status: "succeeded", started_at: new Date(Date.now() - 86400e3).toISOString(), finished_at: new Date(Date.now() - 86300e3).toISOString(),
    steps: [{ id: "scan", status: "done", output: "14 file" }, { id: "triage", status: "done", output: "4 categorie" }, { id: "confirm", status: "done", output: "approvato" }, { id: "act", status: "done", output: "14 file spostati" }] },
];

export const audit: AuditEntry[] = [
  { id: "a1", ts: now(), run_id: "run_prev", actor: "agent", action: "tool", tool: "shell", decision: "approved", detail: "mkdir -p 2024 2025 && mv *.pdf …" },
  { id: "a2", ts: now(), run_id: "run_prev", actor: "agent", action: "tool", tool: "read_file", decision: "allow", detail: "fatture/2025-03.pdf" },
  { id: "a3", ts: now(), actor: "user", action: "runtime.start", detail: "llama-server · qwen3.5-35b-a3b-q4_k_m" },
  { id: "a4", ts: now(), actor: "agent", action: "tool", tool: "write_file", decision: "deny", detail: "C:\\Windows\\hosts fuori dal workspace" },
  { id: "a5", ts: now(), actor: "system", action: "kill", detail: "Kill switch: 1 run e 1 sidecar fermati" },
];

/** `sandbox.deny_read` by default (crates/aiw-core/src/config.rs `DEFAULT_DENY_READ`). */
export const DEFAULT_DENY_READ: string[] = [
  "~/.ssh", "~/.gnupg", "~/.aws", "~/.azure", "~/.config/gcloud", "~/.config/gh", "~/.kube", "~/.docker/config.json", "~/.netrc", "~/.npmrc", "~/.pypirc", "~/.git-credentials",
  "~/.mozilla", "~/.config/google-chrome", "~/.config/chromium", "~/.config/BraveSoftware", "~/.config/microsoft-edge", "~/snap/firefox/common/.mozilla", "~/snap/chromium/common/chromium",
  "~/Library/Application Support/Google/Chrome", "~/Library/Application Support/Firefox", "~/Library/Application Support/BraveSoftware", "~/Library/Application Support/Microsoft Edge", "~/Library/Safari",
  "~/Library/Keychains", "~/.local/share/keyrings",
];

export const configSchema: SettingsSchema = {
  type: "object",
  properties: {
    "ui.mode": { type: "string", title: "Modalità dell'interfaccia", description: "Semplice mostra solo le sezioni di uso quotidiano; Sviluppatore le mostra tutte", enum: ["simple", "developer"], default: "simple" },
    "ui.theme": { type: "string", title: "Tema", enum: ["system", "light", "dark"], default: "system" },
    "ui.language": { type: "string", title: "Lingua", enum: ["it", "en"], default: "it" },
    "agent.default": { type: "string", title: "Agente predefinito", default: "operator" },
    "approvals.timeout_s": { type: "integer", title: "Tempo massimo per rispondere a una richiesta di approvazione (secondi)", description: "0 = nessun limite. Allo scadere la richiesta dell'agente viene rifiutata", default: 0, minimum: 0, maximum: 86400 },
    "policy.mode": { type: "string", title: "Modalità di approvazione", enum: ["plan", "ask", "auto-edit", "auto", "yolo"], default: "ask" },
    "sandbox.mode": { type: "string", title: "Sandbox", enum: ["read-only", "workspace-write", "off"], default: "workspace-write" },
    "sandbox.shell_network": { type: "string", title: "Rete della shell", description: "off = i comandi della shell non hanno rete (per installare pacchetti o scaricare file serve «on»); on = rete aperta. Vale per Linux e macOS: su Windows la sandbox non può chiudere la rete. Un repository può solo chiuderla, mai aprirla", enum: ["off", "on"], default: "off" },
    "sandbox.shell_network_notice": { type: "boolean", title: "Avviso sulla rete della shell da mostrare", description: "Messo a true dalla migrazione per le installazioni esistenti (la rete è rimasta accesa); l'interfaccia lo mostra una volta e lo spegne", default: false },
    "sandbox.deny_read": { type: "array", title: "Cartelle e file nascosti ai comandi", description: "Percorsi (~ è la tua cartella personale) che i comandi della shell e i server MCP isolati non possono né leggere né scrivere, e che i tool di file rifiutano. Le cartelle dati di AIW sono nascoste sempre. Togli una voce se un flusso di lavoro la richiede (per esempio ~/.ssh per git via ssh). Su Windows non ha effetto", items: { type: "string" }, default: DEFAULT_DENY_READ },
    "sandbox.env_allow": { type: "array", title: "Variabili d'ambiente che i comandi possono ereditare", description: "I comandi della shell non ereditano le variabili che sembrano segreti (*_TOKEN, *_KEY, *_SECRET, *PASSWORD*, AIW_*, credenziali cloud). Qui elenchi i nomi (o un prefisso che finisce con *) da lasciare passare", items: { type: "string" }, default: [] },
    "agents.max_parallel_subagents": { type: "integer", title: "Subagenti in parallelo", default: 3, minimum: 1, maximum: 8 },
    "agents.budget.max_tokens": { type: "integer", title: "Budget di token per run (0 = nessun limite)", default: 0, minimum: 0 },
    "agents.budget.max_turns": { type: "integer", title: "Budget di turni per run (0 = nessun limite)", default: 0, minimum: 0 },
    "agents.budget.max_seconds": { type: "integer", title: "Tempo massimo per run, in secondi (0 = nessun limite)", default: 0, minimum: 0 },
    "context.compact_at": { type: "number", title: "Compatta il contesto al", default: 0.8, minimum: 0.3, maximum: 0.95, step: 0.05 },
    "network.allow_domains": { type: "array", title: "Domini consentiti a web/browser (vuoto = tutti i pubblici)", items: { type: "string" }, default: [] },
    "server.lan": { type: "boolean", title: "Accesso dalla LAN (richiede pairing)", default: false },
    "server.port": { type: "integer", title: "Porta API", default: 7777, minimum: 1024, maximum: 65535 },
    "models.dir": { type: "string", title: "Cartella modelli", default: "~/.aiw/models" },
    "workspace.root": { type: "string", title: "Cartella di lavoro degli agenti", default: "" },
    "telemetry.otlp": { type: "string", title: "Endpoint OTLP (vuoto = disattivato)", default: "" },
    "update.channel": { type: "string", title: "Canale di aggiornamento", description: "auto segue la versione installata (una beta resta sulle beta); stable = produzione; beta e alpha ricevono anche le versioni più stabili", enum: ["auto", "stable", "beta", "alpha"], default: "auto" },
    "update.auto_check": { type: "boolean", title: "Cerca aggiornamenti da solo", description: "All'avvio e ogni 6 ore, dalle release del repository pubblico su GitHub", default: true },
    "update.auto_download": { type: "boolean", title: "Scarica da solo gli aggiornamenti", description: "L'installer viene scaricato e verificato (SHA-256) in anticipo; l'installazione parte solo quando la confermi", default: false },
    "memory.enabled": { type: "boolean", title: "Memoria: l'assistente ricorda ciò che gli insegni", description: "Le cose che gli chiedi di ricordare restano sul tuo dispositivo e vengono ritrovate nelle conversazioni future. Spenta, l'assistente non usa né salva ricordi", default: true },
    "learning.reflect": { type: "string", title: "Riflessione dell'agente", description: "Quando l'agente propone una lezione da approvare: off = mai; on_feedback = dopo un pollice giù; on_failure = anche dopo un run fallito o fermato dal budget", enum: ["off", "on_feedback", "on_failure"], default: "on_feedback" },
    "learning.profile": { type: "string", title: "Profilo per riflessione e ottimizzazione del prompt", description: "Profilo di inferenza che scrive le lezioni e le varianti del prompt; vuoto = quello dell'agente", default: "" },
    "proactive.level": {"type": "string", "title": "Assistente proattivo", "description": "Spento per ora: lo accendi tu dopo averlo provato. Con «Propone» l'assistente controlla ogni tanto cosa c'è di nuovo e ti lascia nella casella Proposte delle idee pronte; non fa mai nulla da solo, e ogni proposta parte solo se la approvi. Con «Propone e legge» può anche consultare il web, sempre in sola lettura. Un pulsante di emergenza lo ferma subito", "enum": ["off", "propose", "act_readonly"], "default": "off"},
    "proactive.interval_minutes": {"type": "integer", "title": "Controlla ogni (minuti)", "description": "Ogni quanto l'assistente guarda cosa c'è di nuovo mentre l'app è aperta", "default": 30, "minimum": 5, "maximum": 1440},
    "proactive.briefing_time": {"type": "string", "title": "Riepilogo del mattino alle", "description": "Ora del giorno (HH:MM) in cui prepara il riepilogo: promemoria, proposte aperte, ieri e cartelle osservate", "default": "08:00"},
    "proactive.quiet_hours": {"type": "string", "title": "Orario di silenzio", "description": "Fascia (HH:MM-HH:MM, anche a cavallo della mezzanotte, ad esempio 22:00-07:00) in cui non controlla e non ti avvisa. Vuoto = nessuna fascia", "default": ""},
    "proactive.daily_budget_minutes": {"type": "integer", "title": "Tempo massimo al giorno (minuti)", "description": "Quanto tempo di calcolo al giorno può usare l'assistente proattivo. Finito il tempo, aspetta domani", "default": 15, "minimum": 0, "maximum": 1440},
    "proactive.only_on_power": {"type": "boolean", "title": "Solo con il computer collegato alla corrente", "description": "Dove il sistema dice se sei a batteria; dove non lo dice, questa scelta non ha effetto", "default": true},
    "proactive.agent": {"type": "string", "title": "Assistente che prepara le proposte", "default": "operator"},
    "proactive.watch_folders": {"type": "array", "title": "Cartelle da osservare", "description": "I file nuovi in queste cartelle finiscono tra le cose da guardare", "items": {"type": "string"}, "default": []},
    "connectors.calendar.enabled": { type: "boolean", title: "Calendario: l'assistente può leggere i tuoi eventi", description: "Sola lettura: legge i file .ics e i calendari che scegli, e i dati restano sul dispositivo", default: false },
    "connectors.calendar.sources": { type: "array", title: "Calendari da leggere", description: "File .ics, cartelle con file .ics o indirizzi di sottoscrizione (https:// o webcal://)", items: { type: "string" }, default: [] },
    "connectors.mail.enabled": { type: "boolean", title: "Posta: l'assistente può leggere la posta salvata sul computer", description: "Sola lettura: non invia, non cancella e non segna nulla; legge solo ciò che scegli, e i dati restano sul dispositivo", default: false },
    "connectors.mail.sources": { type: "array", title: "Posta da leggere", description: "Cartelle Maildir, file .mbox o .eml", items: { type: "string" }, default: [] },
    "search.provider": { type: "string", title: "Motore della ricerca web", description: "Con «Automatico» usa SearXNG se hai indicato un indirizzo, altrimenti Brave se hai inserito una chiave, altrimenti DuckDuckGo. Il testo cercato viene inviato al motore scelto. «Spenta» toglie lo strumento di ricerca agli agenti", enum: ["auto", "off", "searxng", "brave", "duckduckgo"], default: "auto" },
    "search.searxng_url": { type: "string", title: "Indirizzo di SearXNG", description: "Istanza di SearXNG, anche su questo computer o sulla rete di casa (per esempio http://localhost:8080). Deve consentire il formato json", default: "", placeholder: "http://localhost:8080" },
    "search.brave_api_key": { type: "string", title: "Chiave di Brave Search", description: "Chiave dell'API di Brave Search; resta su questo dispositivo e non viene mai mostrata per intero", default: "", secret: true },
    "voice.language": { type: "string", title: "Lingua della voce", description: "Codice ISO 639-1 (it, en…) o auto per riconoscerla", default: "auto" },
    "voice.stt_runtime": { type: "string", title: "Motore di trascrizione", description: "auto: whisper.cpp, poi un server locale che trascrive, poi un dispositivo accoppiato", enum: ["auto", "whisper-server", "localai", "nexa", "aiw-peer", "remote-openai"], default: "auto" },
    "voice.stt_model": { type: "string", title: "Modello Whisper", description: "Vuoto = il più grande installato che sta in memoria", default: "" },
    "voice.tts_voice": { type: "string", title: "Voce del sistema per leggere le risposte", description: "Nome di una voce locale del sistema; vuoto = la prima per la lingua", default: "" },
    "voice.tts.engine": { type: "string", title: "Motore della voce", description: "auto: Piper se è installato con una voce nella lingua della risposta, poi un server compatibile OpenAI se hai indicato l'indirizzo, altrimenti le voci del sistema. «Sistema» usa sempre le voci del sistema operativo", enum: ["auto", "system", "piper", "openai"], default: "auto" },
    "voice.tts.voice": { type: "object", title: "Voce neurale per lingua", description: "Per ogni lingua (it, en…) la voce di Piper o del server; la lingua senza voce scelta usa la prima disponibile", default: {} },
    "voice.tts.speed": { type: "number", title: "Velocità della voce", description: "1 = normale, 0.5 = metà, 2 = doppia (solo voci neurali)", default: 1.0, minimum: 0.5, maximum: 2.0, step: 0.1 },
    "voice.tts.server_url": { type: "string", title: "Server della voce (compatibile OpenAI)", description: "Per esempio Kokoro-FastAPI su http://localhost:8880 o LocalAI: AIW chiama /v1/audio/speech. Il testo da leggere viene inviato a quel server", default: "", placeholder: "http://localhost:8880" },
    "voice.tts.server_model": { type: "string", title: "Modello del server della voce", description: "Nome del modello da chiedere al server (kokoro, tts-1…)", default: "kokoro" },
  },
};
export const configLayers: ConfigView["layers"] = [
  { name: "defaults", values: Object.fromEntries(Object.entries(configSchema.properties).map(([k, v]) => [k, v.default])) },
  { name: "global", path: "~/.aiw/config.json", values: { "ui.theme": "system", "sandbox.shell_network": "on", "sandbox.shell_network_notice": true, "models.dir": "D:\\Modelli", "server.lan": true, "proactive.level": "propose", "network.allow_domains": ["docs.rs", "crates.io"] } },
  { name: "device", path: "~/.aiw/devices/Studio-PC.json", values: { "context.compact_at": 0.85 } },
  { name: "workspace", path: "Workspace/.aiw/config.json", values: { "policy.mode": "auto-edit" } },
];

/** Replaces the value of every non-empty secret setting with the mask, as the API does. */
export function maskConfigSecrets(values: Record<string, unknown>): Record<string, unknown> {
  const out = { ...values };
  for (const [key, field] of Object.entries(configSchema.properties)) {
    if (field.secret && typeof out[key] === "string" && out[key] !== "") out[key] = SECRET_MASK;
  }
  return out;
}

export function effective(): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const l of configLayers) Object.assign(out, l.values);
  return out;
}


/** Rules of the demo policy (GET/PUT /api/policy). */
export const policyRules: PolicyRule[] = [
  { tool: "read_file", pattern: null, action: "allow" },
  { tool: "shell", pattern: "git status*", action: "allow" },
  { tool: "shell", pattern: "rm *", action: "deny" },
  { tool: "write_file", pattern: "~/Workspace/**", action: "ask" },
  { tool: "mcp__github__*", pattern: null, action: "ask" },
];

/** Hooks of the demo (GET/PUT /api/hooks). */
export const hookConfigs: HookConfig[] = [
  { event: "PreToolUse", matcher: "shell", command: "./.aiw/hooks/check-shell.sh", timeout_s: 10 },
  { event: "Stop", matcher: null, command: "notify-send 'AI Workspace' 'Run terminato'", timeout_s: 5 },
];
