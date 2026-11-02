import type { Transport } from "./transport";
import type {
  AgEvent, AgentProfile, ApprovalRequest, AuditEntry, CatalogModel, ChatMessage, ConfigView,
  DownloadProgress, InferenceProfile, LocalModel, McpServer, RunState, RuntimeInfo, Session, SettingsSchema, WorktreeStatus,
  SystemInfo, ToolInfo, WorkflowDef, WorkflowRun,
} from "./types";

// Demo data for the interactive mockup. Everything here is sample content, clearly labelled
// in the UI as "demo"; the real values come from the Rust core (crates/aiw-*).

const GB = 1024 ** 3;
const now = () => new Date().toISOString();
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
let idSeq = 100;
const nid = (p: string) => `${p}_${(idSeq++).toString(36)}`;

const llamaSchema: SettingsSchema = {
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
const openaiEndpointSchema: SettingsSchema = {
  type: "object",
  properties: {
    base_url: { type: "string", title: "Endpoint", default: "http://127.0.0.1:8000/v1" },
    api_key: { type: "string", title: "API key (opzionale)", secret: true },
    timeout_s: { type: "integer", title: "Timeout (s)", default: 600, minimum: 5, maximum: 3600, advanced: true },
  },
};
const vllmSchema: SettingsSchema = {
  type: "object",
  properties: {
    model: { type: "string", title: "Modello HF", default: "Qwen/Qwen3.5-35B-A3B-AWQ" },
    gpu_memory_utilization: { type: "number", title: "Uso memoria GPU", default: 0.9, minimum: 0.1, maximum: 0.98, step: 0.01 },
    max_model_len: { type: "integer", title: "Max model len", default: 32768, minimum: 1024, maximum: 262144 },
    tensor_parallel_size: { type: "integer", title: "Tensor parallel", default: 1, minimum: 1, maximum: 8 },
    quantization: { type: "string", title: "Quantizzazione", enum: ["auto", "awq", "gptq", "fp8"], default: "auto" },
  },
};
const systemSchema: SettingsSchema = {
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

const runtimes: RuntimeInfo[] = [
  rt({ id: "llama-server", name: "llama.cpp server", vendor: "ggml-org", kind: "sidecar", status: "running", endpoint: "http://127.0.0.1:39281/v1",
    description: "Server OpenAI-compatibile di llama.cpp, supervisionato dall'app.", accelerators: ["cuda", "vulkan", "cpu"], capabilities: ["chat", "tools", "vision", "embeddings", "grammar"],
    loaded_models: ["qwen3.5-35b-a3b-q4_k_m"], settings_schema: llamaSchema, settings: { ctx_size: 32768, gpu_layers: 999, threads: 0, flash_attn: true, cache_type: "q8_0", parallel: 2, extra_args: [] },
    status_detail: "CUDA · RTX 4090 · 112 tok/s", install_hint: "winget install ggml.llamacpp", homepage: "https://github.com/ggml-org/llama.cpp" }),
  rt({ id: "llama-embedded", name: "llama.cpp (embedded)", vendor: "ggml-org", kind: "embedded", status: "available", platforms: ["windows", "macos", "linux", "android", "ios"],
    description: "llama.cpp collegato in-process via FFI: nessun processo esterno. Default su mobile.", accelerators: ["cuda", "metal", "vulkan", "opencl", "cpu"], capabilities: ["chat", "tools", "embeddings", "grammar"], settings_schema: llamaSchema, settings: { ctx_size: 8192, gpu_layers: 999 } }),
  rt({ id: "ollama", name: "Ollama", vendor: "Ollama", kind: "sidecar", status: "available", endpoint: "http://127.0.0.1:11434", api: "ollama",
    description: "Rilevato su questo PC. Gestione modelli via `ollama pull`.", accelerators: ["cuda", "rocm", "metal", "cpu"], capabilities: ["chat", "tools", "vision", "embeddings"], homepage: "https://ollama.com" }),
  rt({ id: "lmstudio", name: "LM Studio", vendor: "Element Labs", kind: "sidecar", status: "stopped", endpoint: "http://127.0.0.1:1234/v1",
    description: "Server locale di LM Studio (`lms server start`).", accelerators: ["cuda", "vulkan", "metal", "cpu"], formats: ["gguf", "mlx"], capabilities: ["chat", "tools", "vision", "embeddings"] }),
  rt({ id: "vllm", name: "vLLM", vendor: "vLLM project", kind: "sidecar", status: "installable", platforms: ["linux"],
    description: "Serving ad alto throughput (PagedAttention). Su Windows via WSL2.", accelerators: ["cuda", "rocm", "sycl"], formats: ["safetensors", "awq", "gptq", "fp8"], capabilities: ["chat", "tools", "vision", "embeddings", "grammar"],
    settings_schema: vllmSchema, settings: { gpu_memory_utilization: 0.9, max_model_len: 32768, tensor_parallel_size: 1, quantization: "auto" }, install_hint: "wsl -- pip install vllm" }),
  rt({ id: "sglang", name: "SGLang", vendor: "LMSYS", kind: "sidecar", status: "installable", platforms: ["linux"], accelerators: ["cuda", "rocm"], formats: ["safetensors"], description: "Runtime con RadixAttention, ottimo per agenti multi-turn.", install_hint: "pip install sglang[all]" }),
  rt({ id: "trtllm", name: "TensorRT-LLM", vendor: "NVIDIA", kind: "sidecar", status: "installable", platforms: ["linux", "windows"], accelerators: ["cuda"], formats: ["trt", "safetensors"], description: "`trtllm-serve` con engine compilati per GPU NVIDIA." }),
  rt({ id: "foundry-local", name: "Foundry Local", vendor: "Microsoft", kind: "sidecar", status: "available", platforms: ["windows", "macos"], accelerators: ["npu", "directml", "cuda", "cpu"], formats: ["onnx"],
    description: "Runtime LLM di Windows: sceglie in automatico CPU, GPU o NPU.", endpoint: "http://127.0.0.1:5273/v1" }),
  rt({ id: "windows-ml", name: "Windows ML / ORT GenAI", vendor: "Microsoft", kind: "embedded", status: "available", platforms: ["windows"], accelerators: ["directml", "npu", "cuda"], formats: ["onnx"], api: "native",
    description: "ONNX Runtime di sistema con execution provider scaricati per NPU e GPU." }),
  rt({ id: "lemonade", name: "Lemonade Server", vendor: "AMD", kind: "sidecar", status: "unsupported", platforms: ["windows", "linux"], accelerators: ["npu", "vulkan", "rocm"], formats: ["onnx", "gguf"],
    description: "NPU + iGPU Ryzen AI. Nessuna NPU AMD rilevata su questo PC." }),
  rt({ id: "mlx-lm", name: "MLX LM", vendor: "Apple ML Research", kind: "sidecar", status: "unsupported", platforms: ["macos"], accelerators: ["metal"], formats: ["mlx"], description: "Solo Apple Silicon. Disponibile sul Mac accoppiato." }),
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

const system: SystemInfo = {
  version: "0.1.0",
  mode: "demo",
  data_dir: "C:\\Users\\demo\\.aiw",
  workspace_root: "C:\\Users\\demo\\Workspace",
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
    { id: "dev_mbp", name: "MacBook Pro M4 Max", os: "macos", address: "192.168.1.42", status: "online", shared_runtimes: ["mlx-lm", "apple-fm"] },
    { id: "dev_iphone", name: "iPhone 17 Pro", os: "ios", address: "192.168.1.57", status: "online", shared_runtimes: ["apple-fm"] },
    { id: "dev_pixel", name: "Pixel 10", os: "android", address: "—", status: "offline", shared_runtimes: ["gemini-nano", "litert-lm"] },
  ],
};

const models: LocalModel[] = [
  { id: "qwen3.5-35b-a3b-q4_k_m", name: "Qwen3.5 35B-A3B Instruct", format: "gguf", quant: "Q4_K_M", size_bytes: 21.2 * GB, params_b: 35, context_length: 262144, arch: "qwen3_5moe", capabilities: ["chat", "tools", "reasoning"], runtimes: ["llama-server", "llama-embedded", "ollama", "lmstudio"], source: "hf:Qwen/Qwen3.5-35B-A3B-GGUF", path: "models\\qwen3.5-35b-a3b-q4_k_m.gguf" },
  { id: "holo2-8b-q5_k_m", name: "Holo2 8B (computer-use)", format: "gguf", quant: "Q5_K_M", size_bytes: 5.8 * GB, params_b: 8, context_length: 65536, arch: "qwen3vl", capabilities: ["chat", "vision", "gui-grounding"], runtimes: ["llama-server", "lmstudio"], source: "hf:Hcompany/Holo2-8B-GGUF" },
  { id: "gemma-4-e4b-q4_k_m", name: "Gemma 4 E4B", format: "gguf", quant: "Q4_K_M", size_bytes: 3.1 * GB, params_b: 8, context_length: 131072, arch: "gemma4", capabilities: ["chat", "tools", "vision", "audio"], runtimes: ["llama-server", "llama-embedded", "ollama"], source: "hf:google/gemma-4-E4B-it-GGUF" },
  { id: "qwen3-embedding-0.6b-q8_0", name: "Qwen3 Embedding 0.6B", format: "gguf", quant: "Q8_0", size_bytes: 0.64 * GB, params_b: 0.6, context_length: 32768, arch: "qwen3", capabilities: ["embeddings"], runtimes: ["llama-server", "llama-embedded"] },
  { id: "phi-4-mini-onnx-dml", name: "Phi-4 mini (ONNX DirectML)", format: "onnx", quant: "int4", size_bytes: 2.4 * GB, params_b: 3.8, context_length: 131072, capabilities: ["chat", "tools"], runtimes: ["foundry-local", "windows-ml"] },
];

const catalog: CatalogModel[] = [
  { id: "qwen3.5-35b-a3b", name: "Qwen3.5 35B-A3B", family: "Qwen3.5", publisher: "Alibaba", description: "MoE con 3B attivi: veloce come un 3B, qualità vicina a un 30B denso. Ottimo per agenti e tool calling.", params_b: 35, active_params_b: 3, format: "gguf", quant: "Q4_K_M", size_bytes: 21.2 * GB, context_length: 262144, capabilities: ["chat", "tools", "reasoning"], license: "Apache-2.0", source: { kind: "hf", repo: "Qwen/Qwen3.5-35B-A3B-GGUF", file: "Qwen3.5-35B-A3B-Q4_K_M.gguf" }, min_memory_mb: 24000, tags: ["agent", "coding"], fit: "great", fit_detail: "24 GB VRAM: tutti i layer su GPU con contesto da 32K", installed: true },
  { id: "holo3-35b-a3b", name: "Holo3 35B-A3B", family: "Holo3", publisher: "H Company", description: "Tra i migliori open-weight su OSWorld per il computer-use.", params_b: 35, active_params_b: 3, format: "gguf", quant: "Q4_K_M", size_bytes: 21.6 * GB, context_length: 65536, capabilities: ["chat", "vision", "gui-grounding", "tools"], license: "Apache-2.0", source: { kind: "hf", repo: "Hcompany/Holo3-35B-A3B-GGUF", file: "Holo3-35B-A3B-Q4_K_M.gguf" }, min_memory_mb: 25000, tags: ["computer-use"], fit: "ok", fit_detail: "Sta in VRAM con contesto ridotto (16K)", installed: false },
  { id: "gemma-4-27b", name: "Gemma 4 27B", family: "Gemma 4", publisher: "Google", description: "Multimodale (testo, immagini, audio), 128K di contesto.", params_b: 27, format: "gguf", quant: "Q4_K_M", size_bytes: 16.5 * GB, context_length: 131072, capabilities: ["chat", "vision", "tools"], license: "Gemma", source: { kind: "hf", repo: "google/gemma-4-27b-it-GGUF", file: "gemma-4-27b-it-Q4_K_M.gguf" }, min_memory_mb: 19000, tags: ["general", "vision"], fit: "great", fit_detail: "Tutto su GPU", installed: false },
  { id: "llama-3.3-70b", name: "Llama 3.3 70B Instruct", family: "Llama 3.3", publisher: "Meta", description: "Denso 70B, forte nel ragionamento generale.", params_b: 70, format: "gguf", quant: "Q4_K_M", size_bytes: 42.5 * GB, context_length: 131072, capabilities: ["chat", "tools"], license: "Llama 3.3", source: { kind: "hf", repo: "bartowski/Llama-3.3-70B-Instruct-GGUF", file: "Llama-3.3-70B-Instruct-Q4_K_M.gguf" }, min_memory_mb: 46000, tags: ["general"], fit: "tight", fit_detail: "Offload parziale su CPU: circa 9 tok/s", installed: false },
  { id: "qwen3-coder-480b", name: "Qwen3 Coder 480B-A35B", family: "Qwen3", publisher: "Alibaba", description: "Agente di coding di frontiera, richiede un server multi-GPU.", params_b: 480, active_params_b: 35, format: "gguf", quant: "Q4_K_M", size_bytes: 290 * GB, context_length: 262144, capabilities: ["chat", "tools"], license: "Apache-2.0", source: { kind: "hf", repo: "Qwen/Qwen3-Coder-480B-A35B-Instruct-GGUF", file: "Q4_K_M" }, min_memory_mb: 300000, tags: ["coding"], fit: "no", fit_detail: "Servono circa 300 GB di memoria: usa un device remoto", installed: false },
  { id: "gemma-4-e2b-litert", name: "Gemma 4 E2B (LiteRT-LM)", family: "Gemma 4", publisher: "Google", description: "Per telefoni: NPU e GPU con LiteRT-LM.", params_b: 5, active_params_b: 2, format: "litertlm", quant: "int4", size_bytes: 1.4 * GB, context_length: 32768, capabilities: ["chat", "vision", "audio", "tools"], license: "Gemma", source: { kind: "hf", repo: "litert-community/gemma-4-E2B-it-litert-lm", file: "gemma-4-E2B-it.litertlm" }, min_memory_mb: 3000, tags: ["mobile"], fit: "great", fit_detail: "Pensato per mobile; qui gira su CPU", installed: false },
  { id: "apple-fm-system", name: "Apple Foundation Model (di sistema)", family: "Apple", publisher: "Apple", description: "Già presente su iPhone e Mac con Apple Intelligence: non serve scaricarlo.", params_b: 3, format: "system", quant: "2-bit QAT", size_bytes: 0, context_length: 4096, capabilities: ["chat", "tools", "guided-generation"], license: "Apple", source: { kind: "system", runtime: "apple-fm" }, min_memory_mb: 0, tags: ["system", "mobile"], fit: "no", fit_detail: "Solo su dispositivi Apple", installed: false },
];

const profiles: InferenceProfile[] = [
  { id: "balanced", name: "Bilanciato (GPU)", description: "Qwen3.5 35B-A3B su llama.cpp CUDA", runtime_id: "llama-server", model: "qwen3.5-35b-a3b-q4_k_m", params: { temperature: 0.6, top_p: 0.95, max_tokens: 4096, context: 32768 } },
  { id: "fast-local", name: "Veloce", description: "Gemma 4 E4B, latenza minima", runtime_id: "llama-server", model: "gemma-4-e4b-q4_k_m", params: { temperature: 0.4, max_tokens: 2048, context: 16384 } },
  { id: "vision-cu", name: "Computer-use", description: "Holo2 8B, grounding GUI", runtime_id: "llama-server", model: "holo2-8b-q5_k_m", params: { temperature: 0.2, max_tokens: 1024, context: 16384 } },
  { id: "mac-mlx", name: "Mac remoto (MLX)", description: "Gemma 4 27B sul MacBook in LAN", runtime_id: "remote-macbook", model: "gemma-4-27b-it-4bit", params: { temperature: 0.7 } },
];

const agents: AgentProfile[] = [
  { id: "operator", name: "Operatore", description: "Task operativi su file, cartelle, documenti e app.", inference_profile: "balanced", system_prompt: "Sei un operatore preciso. Pianifica, chiedi conferma prima delle azioni distruttive e riporta cosa hai fatto.", tools: ["read_file", "write_file", "edit_file", "list_dir", "glob", "grep", "shell", "web_fetch", "todo", "agent"], mode: "ask", sandbox: "workspace-write", max_turns: 30 },
  { id: "coder", name: "Coder", description: "Coding agent sul workspace corrente.", inference_profile: "balanced", system_prompt: "Sei un ingegnere software. Leggi prima di modificare e verifica con i test.", tools: ["*"], mode: "auto-edit", sandbox: "workspace-write", max_turns: 60 },
  { id: "browser", name: "Navigatore", description: "Ricerca e azioni nel browser via CDP.", inference_profile: "fast-local", system_prompt: "Usi il browser per raccogliere informazioni e completare form.", tools: ["browser_navigate", "browser_snapshot", "browser_click", "browser_type", "browser_screenshot", "web_fetch", "todo"], mode: "ask", sandbox: "read-only", max_turns: 40 },
  { id: "desktop", name: "Pilota desktop", description: "Computer-use: vede lo schermo e usa mouse e tastiera.", inference_profile: "vision-cu", system_prompt: "Controlli il desktop tramite screenshot e azioni. Chiedi conferma prima di inviare, pagare o cancellare.", tools: ["computer_screenshot", "computer_click", "computer_type", "computer_key", "computer_scroll", "todo"], mode: "ask", sandbox: "read-only", max_turns: 50 },
  { id: "planner", name: "Pianificatore", description: "Solo lettura: analizza e propone piani.", inference_profile: "mac-mlx", system_prompt: "Analizzi senza modificare nulla e proponi piani numerati.", tools: ["read_file", "list_dir", "glob", "grep", "web_fetch", "todo"], mode: "plan", sandbox: "read-only", max_turns: 20 },
];

const sessions: Session[] = [
  { id: "ses_pdf", title: "Riordino PDF in Downloads", agent_id: "operator", created_at: now(), updated_at: now(), last_status: "succeeded" },
  { id: "ses_cv", title: "Confronto offerte fornitori", agent_id: "browser", created_at: now(), updated_at: now(), last_status: "succeeded" },
  { id: "ses_ui", title: "Prenotazione sala riunioni", agent_id: "operator", created_at: now(), updated_at: now(), last_status: "succeeded" },
  {
    id: "ses_bug", title: "Fix test parser GGUF", agent_id: "coder", created_at: now(), updated_at: now(), last_status: "succeeded",
    worktree: { path: "~/.aiw/worktrees/ai-workspace-local-7c1e9a20", root: "~/.aiw/worktrees/ai-workspace-local-7c1e9a20", branch: "aiw/7c1e9a20", base: "9c39159", repo: "~/code/ai-workspace-local" },
  },
];

const MOCK_DIFF = `diff --git a/crates/aiw-models/src/gguf.rs b/crates/aiw-models/src/gguf.rs
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

const appliedWorktrees = new Set<string>();

const BOOKING_UI = {
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

const sessionMessages: Record<string, ChatMessage[]> = {
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
  ses_bug: [
    { id: "m3", role: "user", content: "Il test `gguf::tests::reads_header` fallisce, sistemalo.", created_at: now() },
    { id: "m4", role: "assistant", content: "Il run si è fermato: il runtime è stato arrestato dal kill switch durante `cargo test`.", created_at: now() },
  ],
};

const tools: ToolInfo[] = [
  ["read_file", "Legge un file di testo nel workspace", "read"], ["list_dir", "Elenca una cartella", "read"], ["glob", "Trova file per pattern", "read"], ["grep", "Cerca testo con regex", "read"],
  ["write_file", "Crea o sovrascrive un file", "write"], ["edit_file", "Sostituzione esatta in un file", "write"], ["shell", "Esegue un comando nella sandbox", "exec"],
  ["web_fetch", "Scarica una pagina e la converte in testo", "network"], ["browser_navigate", "Apre un URL nel browser controllato", "network"], ["browser_snapshot", "Snapshot testuale della pagina", "read"],
  ["browser_click", "Clic su un elemento", "computer"], ["browser_type", "Scrive in un campo", "computer"], ["browser_screenshot", "Screenshot della pagina", "read"],
  ["computer_screenshot", "Screenshot del desktop", "computer"], ["computer_click", "Clic alle coordinate", "computer"], ["computer_type", "Digita testo", "computer"], ["computer_key", "Combinazione di tasti", "computer"], ["computer_scroll", "Scroll", "computer"],
  ["todo", "Aggiorna la lista dei passi", "read"], ["agent", "Delega a un subagente", "read"], ["skill", "Carica una skill", "read"],
].map(([name, description, risk]) => ({ name, description, risk: risk as ToolInfo["risk"], source: "builtin" as const, enabled: true, available: true, parameters: {} }));
tools.push(
  { name: "mcp__github__list_issues", description: "Elenca le issue di un repository", source: "mcp", server: "github", risk: "network", enabled: true, available: true, parameters: {} },
  { name: "mcp__filesystem__search_files", description: "Ricerca file (server MCP filesystem)", source: "mcp", server: "filesystem", risk: "read", enabled: false, available: true, parameters: {} },
);

const mcp: McpServer[] = [
  { id: "mcp_fs", name: "filesystem", transport: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "~/Workspace"], enabled: true, status: "connected", tools: ["search_files", "read_text_file", "directory_tree"] },
  { id: "mcp_gh", name: "github", transport: "http", url: "http://127.0.0.1:8931/mcp", enabled: true, status: "connected", tools: ["list_issues", "create_issue"] },
  { id: "mcp_pw", name: "playwright", transport: "stdio", command: "npx", args: ["@playwright/mcp@latest"], enabled: false, status: "disconnected", tools: [] },
];

const workflows: WorkflowDef[] = [
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
];

const workflowRuns: WorkflowRun[] = [
  { id: "wfr_1", workflow_id: "inbox-digest", status: "succeeded", started_at: new Date(Date.now() - 86400e3).toISOString(), finished_at: new Date(Date.now() - 86300e3).toISOString(),
    steps: [{ id: "scan", status: "done", output: "14 file" }, { id: "triage", status: "done", output: "4 categorie" }, { id: "confirm", status: "done", output: "approvato" }, { id: "act", status: "done", output: "14 file spostati" }] },
];

const audit: AuditEntry[] = [
  { id: "a1", ts: now(), run_id: "run_prev", actor: "agent", action: "tool", tool: "shell", decision: "approved", detail: "mkdir -p 2024 2025 && mv *.pdf …" },
  { id: "a2", ts: now(), run_id: "run_prev", actor: "agent", action: "tool", tool: "read_file", decision: "allow", detail: "fatture/2025-03.pdf" },
  { id: "a3", ts: now(), actor: "user", action: "runtime.start", detail: "llama-server · qwen3.5-35b-a3b-q4_k_m" },
  { id: "a4", ts: now(), actor: "agent", action: "tool", tool: "write_file", decision: "deny", detail: "C:\\Windows\\hosts fuori dal workspace" },
  { id: "a5", ts: now(), actor: "system", action: "kill", detail: "Kill switch: 1 run e 1 sidecar fermati" },
];

const configSchema: SettingsSchema = {
  type: "object",
  properties: {
    "ui.theme": { type: "string", title: "Tema", enum: ["system", "light", "dark"], default: "system" },
    "ui.language": { type: "string", title: "Lingua", enum: ["it", "en"], default: "it" },
    "agent.default": { type: "string", title: "Agente predefinito", default: "operator" },
    "policy.mode": { type: "string", title: "Modalità di approvazione", enum: ["plan", "ask", "auto-edit", "auto", "yolo"], default: "ask" },
    "sandbox.mode": { type: "string", title: "Sandbox", enum: ["read-only", "workspace-write", "off"], default: "workspace-write" },
    "context.compact_at": { type: "number", title: "Compatta il contesto al", default: 0.8, minimum: 0.3, maximum: 0.95, step: 0.05 },
    "server.lan": { type: "boolean", title: "Accesso dalla LAN (richiede pairing)", default: false },
    "server.port": { type: "integer", title: "Porta API", default: 7777, minimum: 1024, maximum: 65535 },
    "models.dir": { type: "string", title: "Cartella modelli", default: "~/.aiw/models" },
    "telemetry.otlp": { type: "string", title: "Endpoint OTLP (vuoto = disattivato)", default: "" },
  },
};
const configLayers: ConfigView["layers"] = [
  { name: "defaults", values: Object.fromEntries(Object.entries(configSchema.properties).map(([k, v]) => [k, v.default])) },
  { name: "global", path: "~/.aiw/config.json", values: { "ui.theme": "system", "models.dir": "D:\\Modelli", "server.lan": true } },
  { name: "device", path: "~/.aiw/devices/Studio-PC.json", values: { "context.compact_at": 0.85 } },
  { name: "workspace", path: "Workspace/.aiw/config.json", values: { "policy.mode": "auto-edit" } },
];

function effective(): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const l of configLayers) Object.assign(out, l.values);
  return out;
}

export class MockTransport implements Transport {
  readonly kind = "mock" as const;
  private runListeners = new Map<string, Set<(e: AgEvent) => void>>();
  private runEvents = new Map<string, AgEvent[]>();
  private pendingApprovals = new Map<string, { req: ApprovalRequest; resolve: (allow: boolean) => void }>();
  private downloadsState: DownloadProgress[] = [];

  constructor(public readonly notice?: string) {}

  async system() { return structuredClone(system); }
  async kill() {
    for (const [, p] of this.pendingApprovals) p.resolve(false);
    audit.unshift({ id: nid("a"), ts: now(), actor: "user", action: "kill", detail: "Kill switch premuto dalla UI" });
  }

  async runtimes() { return structuredClone(runtimes); }
  async runtimeAction(id: string, action: "start" | "stop" | "refresh") {
    const r = runtimes.find((x) => x.id === id);
    if (!r) throw new Error(`runtime ${id} non trovato`);
    if (action === "start") {
      if (r.status === "unsupported" || r.status === "installable") throw new Error(`${r.name}: ${r.status === "unsupported" ? "non supportato su questo device" : "non installato"}`);
      r.status = "starting";
      await sleep(900);
      r.status = "running";
      r.status_detail = r.accelerators[0]?.toUpperCase() + " · pronto";
    } else if (action === "stop") {
      r.status = "stopped";
      r.status_detail = undefined;
      r.loaded_models = [];
    }
    audit.unshift({ id: nid("a"), ts: now(), actor: "user", action: `runtime.${action}`, detail: r.name });
    return structuredClone(r);
  }
  async updateRuntimeSettings(id: string, settings: Record<string, unknown>) {
    const r = runtimes.find((x) => x.id === id)!;
    r.settings = { ...r.settings, ...settings };
    return structuredClone(r);
  }

  async models() { return structuredClone(models); }
  async catalog() { return structuredClone(catalog); }
  async downloads() { return structuredClone(this.downloadsState); }
  async pullModel(catalogId: string) {
    const c = catalog.find((x) => x.id === catalogId)!;
    const d: DownloadProgress = { id: nid("dl"), model_id: c.id, file: c.source.kind === "hf" ? c.source.file : c.id, received: 0, total: c.size_bytes || 1, status: "running" };
    this.downloadsState.unshift(d);
    const tick = setInterval(() => {
      d.received = Math.min(d.total, d.received + d.total / 18);
      if (d.received >= d.total) {
        d.status = "done";
        c.installed = true;
        models.push({ id: `${c.id}-${c.quant.toLowerCase()}`, name: c.name, format: c.format, quant: c.quant, size_bytes: c.size_bytes, params_b: c.params_b, context_length: c.context_length, capabilities: c.capabilities, runtimes: ["llama-server"], source: c.source.kind === "hf" ? `hf:${c.source.repo}` : undefined });
        clearInterval(tick);
      }
    }, 400);
    return structuredClone(d);
  }

  async profiles() { return structuredClone(profiles); }
  async saveProfile(p: InferenceProfile) {
    const i = profiles.findIndex((x) => x.id === p.id);
    if (i >= 0) profiles[i] = p; else profiles.push(p);
    return p;
  }
  async agents() { return structuredClone(agents); }
  async saveAgent(a: AgentProfile) {
    const i = agents.findIndex((x) => x.id === a.id);
    if (i >= 0) agents[i] = a; else agents.push(a);
    return a;
  }

  async sessions() { return structuredClone(sessions); }
  async createSession(input: { agent_id: string; title?: string; isolation?: "none" | "worktree" }) {
    const id = nid("ses");
    const short = Math.random().toString(16).slice(2, 10);
    const s: Session = {
      id, title: input.title ?? "Nuova sessione", agent_id: input.agent_id, created_at: now(), updated_at: now(),
      worktree: input.isolation === "worktree"
        ? { path: `~/.aiw/worktrees/progetto-${short}`, root: `~/.aiw/worktrees/progetto-${short}`, branch: `aiw/${short}`, base: "9c39159", repo: "~/progetto" }
        : undefined,
    };
    sessions.unshift(s);
    return s;
  }
  async worktreeStatus(sessionId: string): Promise<WorktreeStatus> {
    const s = sessions.find((x) => x.id === sessionId);
    if (!s?.worktree) throw new Error("Questa sessione non usa un worktree.");
    const changed = sessionId === "ses_bug" && !appliedWorktrees.has(sessionId);
    return {
      ...s.worktree,
      files: changed ? [{ status: "M", path: "crates/aiw-models/src/gguf.rs" }, { status: "A", path: "crates/aiw-models/tests/header.rs" }] : [],
      diff_stat: changed ? " crates/aiw-models/src/gguf.rs   | 5 +++--\n crates/aiw-models/tests/header.rs | 6 ++++++\n 2 files changed, 9 insertions(+), 2 deletions(-)\n" : "",
      diff: changed ? MOCK_DIFF : "",
      truncated: false,
    };
  }
  async worktreeApply(sessionId: string) {
    const st = await this.worktreeStatus(sessionId);
    if (st.files.length === 0) throw new Error("Nessuna modifica da applicare.");
    appliedWorktrees.add(sessionId);
    return this.worktreeStatus(sessionId);
  }
  async worktreeDiscard(sessionId: string) {
    const s = sessions.find((x) => x.id === sessionId);
    if (s) s.worktree = undefined;
  }
  async deleteSession(id: string) {
    const i = sessions.findIndex((s) => s.id === id);
    if (i >= 0) sessions.splice(i, 1);
  }
  async messages(sessionId: string) { return structuredClone(sessionMessages[sessionId] ?? []); }

  async activeRun() { return null; }
  async runtimeLogs(id: string) {
    return id === "llama-server"
      ? ["main: server is listening on http://127.0.0.1:39281", "srv  load_model: loaded qwen3.5-35b-a3b-q4_k_m (CUDA0: 21.4 GiB)", "slot launch_slot_: id 0 | task 12 | processing task", "slot print_timing: eval time = 1912 ms / 214 tokens (112.0 tokens per second)"]
      : [];
  }
  async approvals() { return [...this.pendingApprovals.values()].map((p) => p.req); }
  async respondApproval(id: string, decision: "allow" | "deny") {
    const p = this.pendingApprovals.get(id);
    if (!p) return;
    this.pendingApprovals.delete(id);
    audit.unshift({ id: nid("a"), ts: now(), run_id: p.req.run_id, actor: "user", action: "approval", tool: p.req.tool, decision: decision === "allow" ? "approved" : "rejected", detail: JSON.stringify(p.req.args) });
    p.resolve(decision === "allow");
  }

  subscribeRun(runId: string, onEvent: (e: AgEvent) => void) {
    (this.runEvents.get(runId) ?? []).forEach(onEvent);
    const set = this.runListeners.get(runId) ?? new Set();
    set.add(onEvent);
    this.runListeners.set(runId, set);
    return () => set.delete(onEvent);
  }
  async cancelRun(runId: string) {
    this.emit(runId, { type: "RUN_ERROR", message: "Run annullato dall'utente", code: "cancelled" });
  }

  private emit(runId: string, e: AgEvent) {
    const ev = { ...e, timestamp: Date.now() } as AgEvent;
    const list = this.runEvents.get(runId) ?? [];
    list.push(ev);
    this.runEvents.set(runId, list);
    this.runListeners.get(runId)?.forEach((fn) => fn(ev));
  }

  async startRun(sessionId: string, input: string, kind?: "a2ui.action") {
    const runId = nid("run");
    const msgs = sessionMessages[sessionId] ?? (sessionMessages[sessionId] = []);
    msgs.push({ id: nid("m"), role: "user", content: input, name: kind, created_at: now() });
    const s = sessions.find((x) => x.id === sessionId);
    if (s && s.title === "Nuova sessione" && !kind) s.title = input.slice(0, 48);
    if (kind === "a2ui.action") void this.actionReply(runId, sessionId, input);
    else void this.scriptedRun(runId, sessionId, input);
    return { run_id: runId };
  }

  private async actionReply(runId: string, threadId: string, input: string) {
    await sleep(150);
    this.emit(runId, { type: "RUN_STARTED", threadId, runId });
    let ctx: Record<string, unknown> = {};
    let name = "";
    try {
      const v = JSON.parse(input.replace(/^\[Azione A2UI\]\s*/, "")) as { action: { name: string; context: Record<string, unknown> } };
      ctx = v.action.context ?? {};
      name = v.action.name;
    } catch { /* demo */ }
    const m = nid("m");
    this.emit(runId, { type: "TEXT_MESSAGE_START", messageId: m, role: "assistant" });
    const sala = Array.isArray(ctx.sala) ? String(ctx.sala[0] ?? "") : String(ctx.sala ?? "");
    await this.streamText(runId, m, name === "prenota"
      ? `Fatto: sala **${sala.charAt(0).toUpperCase() + sala.slice(1)}** prenotata per ${String(ctx.quando ?? "").replace("T", " alle ")}, ${String(ctx.persone ?? "")} persone${ctx.proiettore ? ", con proiettore" : ""}. Ho aggiunto l'evento al calendario locale.`
      : "Va bene, non prenoto nulla.", "text");
    this.emit(runId, { type: "TEXT_MESSAGE_END", messageId: m });
    this.emit(runId, { type: "RUN_FINISHED", threadId, runId });
  }

  private async streamText(runId: string, messageId: string, text: string, kind: "text" | "reasoning") {
    const words = text.split(/(\s+)/);
    for (let i = 0; i < words.length; i += 3) {
      const delta = words.slice(i, i + 3).join("");
      this.emit(runId, kind === "text" ? { type: "TEXT_MESSAGE_CONTENT", messageId, delta } : { type: "REASONING_MESSAGE_CONTENT", messageId, delta });
      await sleep(kind === "text" ? 28 : 18);
    }
  }

  private async toolCall(runId: string, name: string, args: unknown, result: string, approval?: { reason: string; risk: ApprovalRequest["risk"] }) {
    const toolCallId = nid("call");
    this.emit(runId, { type: "TOOL_CALL_START", toolCallId, toolCallName: name });
    this.emit(runId, { type: "TOOL_CALL_ARGS", toolCallId, delta: JSON.stringify(args) });
    this.emit(runId, { type: "TOOL_CALL_END", toolCallId });
    if (approval) {
      const id = nid("apr");
      const req: ApprovalRequest = { id, run_id: runId, tool: name, args, reason: approval.reason, risk: approval.risk };
      const allowed = await new Promise<boolean>((resolve) => {
        this.pendingApprovals.set(id, { req, resolve });
        this.emit(runId, { type: "CUSTOM", name: "aiw.approval_request", value: req });
      });
      this.emit(runId, { type: "CUSTOM", name: "aiw.approval_resolved", value: { id, allowed } });
      if (!allowed) {
        this.emit(runId, { type: "TOOL_CALL_RESULT", messageId: nid("m"), toolCallId, content: "Negato dall'utente." });
        return false;
      }
    } else {
      await sleep(350);
    }
    this.emit(runId, { type: "TOOL_CALL_RESULT", messageId: nid("m"), toolCallId, content: result });
    audit.unshift({ id: nid("a"), ts: now(), run_id: runId, actor: "agent", action: "tool", tool: name, decision: approval ? "approved" : "allow", detail: JSON.stringify(args).slice(0, 120) });
    return true;
  }

  private async scriptedRun(runId: string, sessionId: string, input: string) {
    await sleep(150);
    const threadId = sessionId;
    this.emit(runId, { type: "RUN_STARTED", threadId, runId });
    type Todo = NonNullable<RunState["todos"]>[number];
    const todos: Todo[] = [
      { content: "Elencare i PDF in Downloads", status: "in_progress" },
      { content: "Creare le cartelle per anno e spostare i file", status: "pending" },
      { content: "Scrivere l'indice INDEX.md", status: "pending" },
    ];
    const state = () => ({ todos: structuredClone(todos), runtime: "llama-server", model: "qwen3.5-35b-a3b-q4_k_m", usage: { prompt_tokens: 1840 + Math.round(Math.random() * 400), completion_tokens: 220, tokens_per_s: 108 + Math.round(Math.random() * 10), context_used: 6100, context_max: 32768 } });
    this.emit(runId, { type: "STATE_SNAPSHOT", snapshot: state() });

    const r1 = nid("r");
    this.emit(runId, { type: "REASONING_START", messageId: r1 });
    await this.streamText(runId, r1, `L'utente chiede: "${input.slice(0, 80)}". Prima elenco i PDF, poi li raggruppo per anno dalla data nel nome o di modifica, chiedo conferma per lo spostamento e infine scrivo l'indice.`, "reasoning");
    this.emit(runId, { type: "REASONING_END", messageId: r1 });

    const m1 = nid("m");
    this.emit(runId, { type: "TEXT_MESSAGE_START", messageId: m1, role: "assistant" });
    await this.streamText(runId, m1, "Inizio guardando cosa c'è nella cartella Downloads.", "text");
    this.emit(runId, { type: "TEXT_MESSAGE_END", messageId: m1 });

    await this.toolCall(runId, "list_dir", { path: "~/Downloads", pattern: "*.pdf" },
      "fattura-2024-11-acme.pdf  212 KB  2024-11-03\nfattura-2025-02-acme.pdf  198 KB  2025-02-07\nestratto-conto-2025-06.pdf  1.1 MB  2025-07-01\ncontratto-affitto-2024.pdf  640 KB  2024-01-15\nmanuale-router.pdf  4.2 MB  2025-09-22\nbiglietto-treno-2026-03.pdf  88 KB  2026-03-14\nprogetto-casa-2026.pdf  12.8 MB  2026-05-30");
    todos[0].status = "completed";
    todos[1].status = "in_progress";
    this.emit(runId, { type: "STATE_SNAPSHOT", snapshot: state() });

    const m2 = nid("m");
    this.emit(runId, { type: "TEXT_MESSAGE_START", messageId: m2, role: "assistant" });
    await this.streamText(runId, m2, "Ho trovato 7 PDF tra il 2024 e il 2026. Creo le cartelle `2024`, `2025` e `2026` e ci sposto i file. Per questo comando serve la tua conferma.", "text");
    this.emit(runId, { type: "TEXT_MESSAGE_END", messageId: m2 });

    const ok = await this.toolCall(runId, "shell", { command: "mkdir 2024 2025 2026 && move *2024*.pdf 2024 && move *2025*.pdf 2025 && move *2026*.pdf 2026", cwd: "~/Downloads" },
      "Cartelle create: 3 · File spostati: 6 · Ignorato: manuale-router.pdf (anno non presente nel nome)",
      { reason: "Il comando sposta file nella cartella Downloads.", risk: "medium" });
    if (!ok) {
      const m = nid("m");
      this.emit(runId, { type: "TEXT_MESSAGE_START", messageId: m, role: "assistant" });
      await this.streamText(runId, m, "Ok, non sposto nulla. Posso solo preparare l'indice senza modificare la cartella, se vuoi.", "text");
      this.emit(runId, { type: "TEXT_MESSAGE_END", messageId: m });
      this.emit(runId, { type: "RUN_FINISHED", threadId, runId });
      this.finish(sessionId, "Ok, non sposto nulla.");
      return;
    }
    todos[1].status = "completed";
    todos[2].status = "in_progress";
    this.emit(runId, { type: "STATE_SNAPSHOT", snapshot: state() });

    const sub = nid("sub");
    this.emit(runId, { type: "SUBAGENT_STARTED", subagentId: sub, name: "Lettore di metadati (Veloce)" });
    await sleep(700);
    this.emit(runId, { type: "SUBAGENT_FINISHED", subagentId: sub, result: "manuale-router.pdf: data documento 2025-09-22 → 2025" });

    await this.toolCall(runId, "write_file", { path: "~/Downloads/INDEX.md", content: "# Indice PDF\n\n## 2024\n- fattura-2024-11-acme.pdf\n- contratto-affitto-2024.pdf\n…" },
      "Scritto INDEX.md (24 righe)");
    todos[2].status = "completed";
    this.emit(runId, { type: "STATE_SNAPSHOT", snapshot: state() });

    const m3 = nid("m");
    const final = "Fatto. I PDF sono ordinati per anno e l'indice è in `Downloads/INDEX.md`.\n\n| Anno | File |\n|---|---|\n| 2024 | 2 |\n| 2025 | 3 (incluso il manuale, datato dai metadati) |\n| 2026 | 2 |\n\nPosso trasformarlo in un workflow che lo rifà ogni lunedì?";
    this.emit(runId, { type: "TEXT_MESSAGE_START", messageId: m3, role: "assistant" });
    await this.streamText(runId, m3, final, "text");
    this.emit(runId, { type: "TEXT_MESSAGE_END", messageId: m3 });
    this.emit(runId, { type: "RUN_FINISHED", threadId, runId });
    this.finish(sessionId, final);
  }

  private finish(sessionId: string, final: string) {
    (sessionMessages[sessionId] ??= []).push({ id: nid("m"), role: "assistant", content: final, created_at: now() });
    const s = sessions.find((x) => x.id === sessionId);
    if (s) {
      s.updated_at = now();
      s.last_status = "succeeded";
    }
  }

  async workflows() { return structuredClone(workflows); }
  async saveWorkflow(w: WorkflowDef) {
    const i = workflows.findIndex((x) => x.id === w.id);
    if (i >= 0) workflows[i] = w; else workflows.push(w);
    return w;
  }
  async workflowRuns() { return structuredClone(workflowRuns); }
  async runWorkflow(id: string) {
    const w = workflows.find((x) => x.id === id)!;
    const run: WorkflowRun = { id: nid("wfr"), workflow_id: id, status: "running", started_at: now(), steps: w.steps.map((s) => ({ id: s.id, status: "pending" })) };
    workflowRuns.unshift(run);
    void (async () => {
      for (const st of run.steps) {
        st.status = "running";
        await sleep(900);
        st.status = "done";
        st.output = "ok";
      }
      run.status = "succeeded";
      run.finished_at = now();
    })();
    return structuredClone(run);
  }

  async tools() { return structuredClone(tools); }
  async setToolEnabled(name: string, enabled: boolean) {
    const t = tools.find((x) => x.name === name);
    if (t) t.enabled = enabled;
  }
  async mcpServers() { return structuredClone(mcp); }
  async saveMcpServer(s: Partial<McpServer> & { name: string; transport: "stdio" | "http" }) {
    const srv: McpServer = { id: nid("mcp"), enabled: true, status: "disconnected", tools: [], ...s } as McpServer;
    mcp.push(srv);
    return srv;
  }
  async removeMcpServer(id: string) {
    const i = mcp.findIndex((m) => m.id === id);
    if (i >= 0) mcp.splice(i, 1);
  }

  async config() { return { layers: structuredClone(configLayers), effective: effective(), schema: configSchema }; }
  async setConfig(layer: "global" | "device" | "workspace", key: string, value: unknown) {
    const l = configLayers.find((x) => x.name === layer)!;
    l.values[key] = value;
    return this.config();
  }
  async audit() { return structuredClone(audit); }
}
