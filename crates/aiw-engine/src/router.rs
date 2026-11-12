//! The automatic runtime router: which runtime should run a model on this device, with which
//! acceleration and which context, and what to try when it does not work.
//!
//! [`rank`] is pure: it takes the model ([`ModelSpec`]), the device and the live state of the
//! runtimes ([`RouteInput`]) and returns the ordered candidates, each with the reason for its rank,
//! plus the runtimes that were left out, each with the reason. [`Engine::route`] feeds it with the
//! real state; an inference profile with `runtime_id: "auto"` (and the `auto:*` profiles of the
//! adviser) is resolved through it when a run starts, and the next candidates are the fallbacks of
//! that run (see `Engine::routed_backend`).
//!
//! ## What a candidate needs
//! * the manifest handles the model's format (`formats`) on this OS and CPU (`status` is not
//!   `unsupported`) and chats (`capabilities` has `chat`);
//! * it is installed and reachable: `running`, or `available` (AIW starts it);
//! * it has not failed on this model a moment ago (see [`FailureMemory`]);
//! * the model fits: weights × 1.1 + 700 MB + the KV cache of the context, against the free memory
//!   of the device (`aiw_models::min_memory_mb`). When it does not fit, the context is halved, down
//!   to 2048, before the candidate is given up. A runtime that already serves the model has it in
//!   memory and is not asked for more.
//!
//! ## Order
//! 1. **Where it runs**, best first: the whole model on a native GPU API (Metal, CUDA, ROCm) 60;
//!    on a portable one (Vulkan, SYCL, OpenCL, DirectML) or an NPU 50; split between GPU and RAM 30;
//!    CPU 10. The accelerator must be in the manifest's `accelerators` *and* on the device (the
//!    in-process engine only counts the ones it was compiled with). A runtime on another device
//!    comes after every local one, and only when the profile allows it (`allow_remote`).
//! 2. **Ready first**: running (or built in) before one AIW has to start.
//! 3. **`route_priority`** of the manifest (lower first; +25 for a format that is not the first the
//!    manifest lists, so vLLM, made for safetensors, comes after llama-server for GGUF), then the
//!    order of the manifest.
//!
//! The result per device class, with the manifests as shipped (a runtime that is not installed is
//! excluded, so the table assumes everything is installed):
//!
//! | device | model | first choices |
//! |--------|-------|---------------|
//! | Apple Silicon | GGUF | `llama-embedded` (Metal, built in), `llama-server`, `ollama`, `lmstudio`… |
//! | Apple Silicon | MLX | `mlx-lm`, then `lmstudio`, `nexa`, `exo` (`mlx-swift` is not implemented yet) |
//! | NVIDIA, Linux or Windows | GGUF | `llama-server` (CUDA), `ollama`, `lmstudio`, `mistralrs`; `vllm` last (Linux) |
//! | NVIDIA, Linux | safetensors | `vllm`, `sglang`, `trtllm`, `mistralrs` |
//! | NVIDIA, Windows | ONNX | `ort-genai` (CUDA or DirectML), `foundry-local` |
//! | CPU only | GGUF | `llama-server`, `ollama`, `llamafile`…; everything on the CPU |
//! | phone | GGUF | `llama-embedded` (sidecars are not available) |
//! | system model | `system` | only the platform runtime (Apple Foundation Models…), then the smallest local GGUF |
//!
//! Limits: the acceleration of a candidate (`gpu_layers`) is reported, not forced: the runtime
//! starts with its own settings and only the context is applied. A candidate on a paired device is
//! checked against the memory that device reported when it was paired (total, not free); a plain
//! remote server reports nothing, and the check is skipped, with a note.

use aiw_core::api::{DeviceProfile, InferenceProfile, LocalModel, PeerDevice, RouteCandidate, RouteDecision, RouteExclusion, RuntimeInfo};
use aiw_core::{Error, Result};
use aiw_runtimes::ChatBackend;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Smallest context the router offers.
pub const MIN_CONTEXT: u32 = 2048;
/// Fallbacks a run may use after its first choice.
pub const MAX_FALLBACKS: usize = 2;
/// Added to the priority of a runtime for a format that is not the first of its manifest's `formats`.
pub const NON_NATIVE_PENALTY: u32 = 25;
/// How long a failed candidate is skipped.
pub const FAILURE_TTL: Duration = Duration::from_secs(10 * 60);

/// The model to run, as far as the router needs to know it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelSpec {
    pub id: String,
    /// `gguf`, `mlx`, `safetensors`, `onnx`…, `system` for a platform model; empty when unknown
    /// (a name served by a running server, like an Ollama tag).
    pub format: String,
    pub size_bytes: u64,
    pub quant: Option<String>,
    /// KV cache per token of context, when the model header gives the shape.
    pub kv_bytes_per_token: Option<u64>,
    /// Longest context the model supports.
    pub native_context: Option<u64>,
    /// The only runtime that serves a system model.
    pub system_runtime: Option<String>,
}

impl ModelSpec {
    pub fn from_local(m: &LocalModel, kv_bytes_per_token: Option<u64>) -> Self {
        Self {
            id: m.id.clone(),
            format: m.format.clone(),
            size_bytes: m.size_bytes,
            quant: m.quant.clone(),
            kv_bytes_per_token,
            native_context: m.context_length,
            system_runtime: (m.format == "system").then(|| m.runtimes.first().cloned()).flatten(),
        }
    }
}

/// A paired device, with what it told about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct RemotePeer {
    pub name: String,
    pub memory_mb: u64,
    pub vram_mb: u64,
    pub unified: bool,
    /// Memory is known (it was reported when the device was paired).
    pub known: bool,
    pub models: Vec<String>,
}

impl RemotePeer {
    pub fn from_device(p: &PeerDevice) -> Self {
        let hw = p.hardware.as_ref();
        Self {
            name: p.name.clone(),
            memory_mb: hw.map(|h| h.memory_mb).unwrap_or(0),
            vram_mb: hw.map(|h| h.vram_mb).unwrap_or(0),
            unified: hw.is_some_and(|h| h.unified_memory),
            known: hw.is_some(),
            models: p.shared_runtimes.clone(),
        }
    }
    fn budget_mb(&self) -> u64 {
        if self.unified {
            aiw_hw::unified_budget_mb(self.memory_mb)
        } else {
            self.vram_mb.max(self.memory_mb / 2)
        }
    }
}

/// How a candidate failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// Did not start, or died while starting.
    Start,
    /// Started but never became healthy.
    Health,
    /// Out of memory.
    Memory,
    /// The binary or the model file is missing.
    Missing,
    /// The runtime cannot read the model (format, architecture).
    Format,
    /// Anything else (a refused request…): never a reason to change runtime.
    Other,
}

impl FailureKind {
    /// Reads the kind from the text of an error.
    pub fn classify(error: &str) -> Self {
        let e = error.to_lowercase();
        let has = |needles: &[&str]| needles.iter().any(|n| e.contains(n));
        if has(&[
            "out of memory",
            "outofmemory",
            "oom",
            "cudamalloc",
            "failed to allocate",
            "not enough memory",
            "insufficient memory",
            "memoria insufficiente",
            "memoria esaurita",
            "cannot allocate",
        ]) {
            Self::Memory
        } else if has(&[
            "unknown model architecture",
            "unsupported model",
            "invalid magic",
            "failed to load model",
            "formato non supportato",
            "unsupported format",
        ]) {
            Self::Format
        } else if has(&[
            "impossibile avviare",
            "no such file",
            "non è stato trovato",
            "not found",
            "serve il plugin",
            "non è installato",
            "non ha una ricetta",
        ]) {
            Self::Missing
        } else if has(&["non è pronto", "ready", "health"]) {
            Self::Health
        } else if has(&["si è chiuso durante l", "non è partito", "exited", "spawn"]) {
            Self::Start
        } else {
            Self::Other
        }
    }

    /// Whether the next candidate should be tried after this failure.
    pub fn is_load_error(self) -> bool {
        !matches!(self, Self::Other)
    }
}

/// A candidate that failed recently.
#[derive(Debug, Clone, PartialEq)]
pub struct Skip {
    pub runtime_id: String,
    pub kind: FailureKind,
    /// Context it failed with (memory failures).
    pub context: Option<u32>,
    pub detail: String,
}

/// Failures of the last minutes, in memory only: a runtime that failed to start a model is not
/// tried again for it until the failure expires. Memory failures do not exclude the runtime: they
/// cap its context below the one that failed.
pub struct FailureMemory {
    ttl: Duration,
    seen: Mutex<HashMap<(String, String), (Instant, Skip)>>,
}

impl FailureMemory {
    pub fn new(ttl: Duration) -> Self {
        Self { ttl, seen: Mutex::new(HashMap::new()) }
    }

    pub fn record(&self, runtime_id: &str, model: &str, kind: FailureKind, context: Option<u32>, detail: &str) {
        self.record_at(Instant::now(), runtime_id, model, kind, context, detail);
    }

    pub fn record_at(&self, now: Instant, runtime_id: &str, model: &str, kind: FailureKind, context: Option<u32>, detail: &str) {
        let skip = Skip { runtime_id: runtime_id.into(), kind, context, detail: detail.chars().take(200).collect() };
        self.seen.lock().unwrap().insert((runtime_id.into(), model.into()), (now, skip));
    }

    /// What is still remembered about `model`.
    pub fn skips(&self, model: &str) -> Vec<Skip> {
        self.skips_at(Instant::now(), model)
    }

    pub fn skips_at(&self, now: Instant, model: &str) -> Vec<Skip> {
        let mut seen = self.seen.lock().unwrap();
        seen.retain(|_, (at, _)| now.saturating_duration_since(*at) < self.ttl);
        let mut v: Vec<Skip> = seen.iter().filter(|((_, m), _)| m == model).map(|(_, (_, s))| s.clone()).collect();
        v.sort_by(|a, b| a.runtime_id.cmp(&b.runtime_id));
        v
    }

    pub fn clear(&self) {
        self.seen.lock().unwrap().clear();
    }
}

impl Default for FailureMemory {
    fn default() -> Self {
        Self::new(FAILURE_TTL)
    }
}

pub struct RouteInput<'a> {
    pub model: &'a ModelSpec,
    /// Context window wanted.
    pub want_context: u32,
    pub device: &'a DeviceProfile,
    pub runtimes: &'a [RuntimeInfo],
    pub peers: &'a [RemotePeer],
    pub allow_remote: bool,
    pub skips: &'a [Skip],
    /// Accelerators the in-process engine was compiled with.
    pub embedded_accelerators: &'a [&'a str],
}

/// Rough KV cache per token of context when the header does not say: about 26 KB per GB of weights
/// (a 4.9 GB Llama 3 8B needs 128 KB per token).
const KV_BYTES_PER_GB: u64 = 26_000;

fn kv_mb(spec: &ModelSpec, ctx: u32) -> (u64, bool) {
    match spec.kv_bytes_per_token {
        Some(b) => ((b * ctx as u64) >> 20, false),
        None => (((spec.size_bytes / (1 << 30)).max(1) * KV_BYTES_PER_GB * ctx as u64) >> 20, true),
    }
}

fn need_mb(spec: &ModelSpec, ctx: u32) -> (u64, u64, bool) {
    let (kv, est) = kv_mb(spec, ctx);
    (aiw_models::min_memory_mb(spec.size_bytes, kv), kv, est)
}

fn gb(mb: u64) -> String {
    format!("{:.1} GB", mb as f64 / 1024.0)
}

/// Descending contexts to try: the wanted one, then halves down to [`MIN_CONTEXT`].
fn context_ladder(want: u32, floor: u32) -> Vec<u32> {
    let mut v = vec![want.max(MIN_CONTEXT)];
    while let Some(&last) = v.last() {
        let next = last / 2;
        if next < floor.max(MIN_CONTEXT) {
            break;
        }
        v.push(next);
    }
    v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// Whole model on a native GPU API.
    NativeGpu,
    /// Whole model on a portable GPU API or an NPU.
    PortableGpu,
    /// GPU and RAM.
    Split,
    Cpu,
}

impl Place {
    fn score(self) -> u32 {
        match self {
            Place::NativeGpu => 60,
            Place::PortableGpu => 50,
            Place::Split => 30,
            Place::Cpu => 10,
        }
    }
}

fn is_native_gpu(kind: &str) -> bool {
    matches!(kind, "metal" | "cuda" | "rocm")
}

fn is_gpu_like(kind: &str) -> bool {
    is_native_gpu(kind) || matches!(kind, "vulkan" | "sycl" | "opencl" | "directml" | "webgpu" | "npu" | "coreml" | "qnn")
}

fn accel_label(kind: &str) -> String {
    match kind {
        "metal" => "Metal".into(),
        "cuda" => "CUDA".into(),
        "rocm" => "ROCm".into(),
        "vulkan" => "Vulkan".into(),
        "sycl" => "SYCL".into(),
        "opencl" => "OpenCL".into(),
        "directml" => "DirectML".into(),
        "npu" => "NPU".into(),
        "cpu" => "CPU".into(),
        other => other.to_string(),
    }
}

struct Placement {
    place: Place,
    accelerator: String,
    context: u32,
    need: u64,
    kv: u64,
    kv_estimated: bool,
    gpu_layers: Option<i32>,
}

/// Where a model of `spec` fits on `device` with the accelerators `accels` (those of the runtime
/// that the device has, best first), at the largest context ≤ `want`.
fn place(
    spec: &ModelSpec,
    device: &DeviceProfile,
    accels: &[String],
    cpu_ok: bool,
    want: u32,
    cap: Option<u32>,
) -> std::result::Result<Placement, String> {
    let ram_free = if device.available_memory_mb > 0 { device.available_memory_mb } else { device.memory_mb };
    let want = want.min(cap.unwrap_or(u32::MAX)).max(MIN_CONTEXT);
    let vram = device.vram_mb();
    let unified = device.unified_memory();
    let gpus: Vec<&String> = accels.iter().filter(|a| is_gpu_like(a)).collect();
    let mk = |place, accel: &str, ctx, need, kv, est, layers| Placement {
        place,
        accelerator: accel.into(),
        context: ctx,
        need,
        kv,
        kv_estimated: est,
        gpu_layers: layers,
    };
    // Whole model on the accelerator, keeping at least 4096 tokens of context.
    if let Some(g) = gpus.first() {
        let cap_full = if unified { aiw_hw::model_budget_mb(device).min(ram_free) } else { vram };
        let place = if is_native_gpu(g) { Place::NativeGpu } else { Place::PortableGpu };
        for ctx in context_ladder(want, want.min(4096)) {
            let (need, kv, est) = need_mb(spec, ctx);
            if cap_full > 0 && need <= cap_full {
                return Ok(mk(place, g, ctx, need, kv, est, Some(-1)));
            }
        }
        // Split between GPU and RAM: the memory the GPU has plus 60 % of the free RAM.
        let split_cap = if unified { cap_full } else { vram + ram_free * 6 / 10 };
        for ctx in context_ladder(want, MIN_CONTEXT) {
            let (need, kv, est) = need_mb(spec, ctx);
            if need <= split_cap && !unified {
                return Ok(mk(Place::Split, g, ctx, need, kv, est, None));
            }
        }
    }
    if cpu_ok {
        let cpu_cap = if unified { aiw_hw::model_budget_mb(device).min(ram_free) } else { ram_free };
        for ctx in context_ladder(want, MIN_CONTEXT) {
            let (need, kv, est) = need_mb(spec, ctx);
            if need <= cpu_cap {
                return Ok(mk(Place::Cpu, "cpu", ctx, need, kv, est, Some(0)));
            }
        }
    }
    let (need, _, _) = need_mb(spec, MIN_CONTEXT);
    let have = if gpus.is_empty() || !unified && vram == 0 {
        ram_free
    } else if unified {
        aiw_hw::model_budget_mb(device).min(ram_free)
    } else {
        vram + ram_free * 6 / 10
    };
    Err(format!("memoria insufficiente: servono ~{} anche con contesto {MIN_CONTEXT}, ne restano ~{}", gb(need), gb(have)))
}

enum Eval {
    /// Not relevant to this model (another format, no chat): not listed.
    Irrelevant,
    Out(String),
    /// The candidate, its placement score and its priority (lower is better).
    In(Box<RouteCandidate>, u32, u32),
}

fn reason_for(r: &RuntimeInfo, p: &Placement, ready: bool, resident: bool, want: u32) -> String {
    let where_ = match p.place {
        Place::NativeGpu | Place::PortableGpu => format!("tutto il modello su {}", accel_label(&p.accelerator)),
        Place::Split => format!("diviso tra {} e RAM", accel_label(&p.accelerator)),
        Place::Cpu => "sulla CPU".to_string(),
    };
    let state = if resident {
        "ha già il modello in memoria"
    } else if ready && r.kind == "embedded" {
        "integrato nell'app"
    } else if ready {
        "già in esecuzione"
    } else {
        "AIW lo avvia"
    };
    let ctx = if p.context < want {
        format!("contesto ridotto da {want} a {} per la memoria (KV cache ~{} MB)", p.context, p.kv)
    } else {
        format!("contesto {}", p.context)
    };
    format!("{where_}; {state}; {ctx}")
}

fn evaluate(r: &RuntimeInfo, input: &RouteInput) -> Eval {
    let spec = input.model;
    let want = input.want_context;
    if r.api == "none" || !r.capabilities.iter().any(|c| c == "chat") {
        return Eval::Irrelevant;
    }
    let remote = r.kind == "remote";
    let serves_by_name = r.status == "running" && !spec.id.is_empty() && r.loaded_models.iter().any(|m| m == &spec.id);
    // Which runtimes are relevant at all.
    if let Some(sys) = &spec.system_runtime {
        if &r.id != sys {
            return Eval::Irrelevant;
        }
    } else if remote {
        if !input.allow_remote {
            return Eval::Out("è un altro dispositivo o un server remoto: il profilo non lo consente".into());
        }
    } else if spec.format.is_empty() || spec.format == "system" {
        if !serves_by_name {
            return Eval::Irrelevant;
        }
    } else if !r.formats.iter().any(|f| f == &spec.format) && !serves_by_name {
        return Eval::Irrelevant;
    }
    if crate::models::MANIFEST_ONLY.contains(&r.id.as_str()) {
        return Eval::Out("non ancora implementato in questa versione".into());
    }
    match r.status.as_str() {
        "running" | "available" | "starting" => {}
        "unsupported" => {
            return Eval::Out(
                r.status_detail.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| "non supportato su questo sistema".into()),
            )
        }
        "installable" if matches!(r.kind.as_str(), "embedded" | "system") => return Eval::Out("serve il plugin nativo dell'app".into()),
        "installable" => {
            return Eval::Out(match r.install_hint.as_deref().filter(|h| !h.is_empty()) {
                Some(h) => format!("non installato ({h})"),
                None => "non installato".into(),
            })
        }
        "stopped" => {
            return Eval::Out(r.status_detail.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| "non raggiungibile".into()));
        }
        "error" => return Eval::Out(r.status_detail.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| "in errore".into())),
        other => return Eval::Out(other.to_string()),
    }
    let mut cap = None;
    if let Some(skip) = input.skips.iter().find(|s| s.runtime_id == r.id) {
        match (skip.kind, skip.context) {
            (FailureKind::Memory, Some(c)) if c / 2 >= MIN_CONTEXT => cap = Some(c / 2),
            _ => return Eval::Out(format!("ha già fallito da poco con questo modello: {}", skip.detail)),
        }
    }
    let ready = r.status == "running";
    // A runtime whose manifest lists the format after others is not at home with it (vLLM and GGUF).
    let priority =
        r.route_priority + if r.formats.iter().position(|f| f == &spec.format).is_some_and(|i| i > 0) { NON_NATIVE_PENALTY } else { 0 };
    if remote {
        return evaluate_remote(r, input, ready);
    }
    if spec.system_runtime.is_some() {
        let c = RouteCandidate {
            rank: 0,
            runtime_id: r.id.clone(),
            runtime_name: r.name.clone(),
            model: String::new(),
            accelerator: r.accelerators.first().cloned().unwrap_or_else(|| "npu".into()),
            offload: "all".into(),
            gpu_layers: None,
            context: want,
            need_mb: 0,
            kv_mb: 0,
            kv_estimated: false,
            remote: false,
            ready,
            reason: "modello del sistema: nessuna memoria da caricare; ".to_string()
                + if ready { "integrato nell'app" } else { "disponibile" },
        };
        return Eval::In(Box::new(c), Place::NativeGpu.score() + 5, r.route_priority);
    }
    // Accelerators this runtime can use here.
    let mut manifest_accels: Vec<String> = r.accelerators.clone();
    if r.id == "llama-embedded" {
        manifest_accels.retain(|a| input.embedded_accelerators.contains(&a.as_str()));
    }
    let device_kinds: Vec<&str> = input.device.accelerators.iter().map(|a| a.kind.as_str()).collect();
    let mut usable: Vec<String> =
        manifest_accels.iter().filter(|a| a.as_str() != "cpu" && device_kinds.contains(&a.as_str())).cloned().collect();
    usable.sort_by_key(|a| std::cmp::Reverse(if is_native_gpu(a) { 2 } else { 1 }));
    let cpu_ok = r.accelerators.is_empty() || manifest_accels.iter().any(|a| a == "cpu");
    if usable.is_empty() && !cpu_ok {
        let want_kinds = r.accelerators.iter().map(|a| accel_label(a)).collect::<Vec<_>>().join(", ");
        let have = device_kinds.iter().filter(|k| **k != "cpu").map(|k| accel_label(k)).collect::<Vec<_>>();
        return Eval::Out(format!(
            "serve {want_kinds}, che qui {}",
            if have.is_empty() { "non c'è".to_string() } else { format!("non è tra quelli presenti ({})", have.join(", ")) }
        ));
    }
    if serves_by_name {
        // Already loaded: the memory is spent, there is nothing more to ask of the device.
        let (need, kv, est) = need_mb(spec, want);
        let accel = usable.first().cloned().unwrap_or_else(|| "cpu".into());
        let place = if let Some(a) = usable.first() {
            if is_native_gpu(a) {
                Place::NativeGpu
            } else {
                Place::PortableGpu
            }
        } else {
            Place::Cpu
        };
        let p = Placement { place, accelerator: accel.clone(), context: want, need, kv, kv_estimated: est, gpu_layers: None };
        let c = candidate(r, &p, ready, true, want);
        return Eval::In(Box::new(c), place.score(), priority);
    }
    match place(spec, input.device, &usable, cpu_ok, want, cap) {
        Ok(p) => {
            let place = p.place;
            let c = candidate(r, &p, ready, false, want);
            Eval::In(Box::new(c), place.score(), priority)
        }
        Err(e) => Eval::Out(e),
    }
}

fn candidate(r: &RuntimeInfo, p: &Placement, ready: bool, resident: bool, want: u32) -> RouteCandidate {
    let offload = match p.place {
        Place::NativeGpu | Place::PortableGpu => "all",
        Place::Split => "partial",
        Place::Cpu => "none",
    };
    // Only runtimes with a `gpu_layers` setting take it.
    let has_layers = r.settings_schema.properties.contains_key("gpu_layers");
    RouteCandidate {
        rank: 0,
        runtime_id: r.id.clone(),
        runtime_name: r.name.clone(),
        model: String::new(),
        accelerator: p.accelerator.clone(),
        offload: offload.into(),
        gpu_layers: if has_layers { p.gpu_layers } else { None },
        context: p.context,
        need_mb: p.need,
        kv_mb: p.kv,
        kv_estimated: p.kv_estimated,
        remote: false,
        ready,
        reason: reason_for(r, p, ready, resident, want),
    }
}

fn evaluate_remote(r: &RuntimeInfo, input: &RouteInput, ready: bool) -> Eval {
    let spec = input.model;
    let want = input.want_context;
    let (need, kv, est) = need_mb(spec, want);
    let reason;
    let mut checked = false;
    if r.id == "aiw-peer" {
        let peers: Vec<&RemotePeer> =
            input.peers.iter().filter(|p| p.models.is_empty() || p.models.iter().any(|m| m == &spec.id)).collect();
        if input.peers.is_empty() {
            return Eval::Out("nessun dispositivo accoppiato".into());
        }
        if peers.is_empty() {
            return Eval::Out("nessun dispositivo accoppiato offre questo modello".into());
        }
        let known: Vec<&&RemotePeer> = peers.iter().filter(|p| p.known).collect();
        if known.is_empty() {
            reason = "dispositivo accoppiato; memoria del dispositivo non nota: stima non fatta".into();
        } else if let Some(p) = known.iter().find(|p| p.budget_mb() >= need) {
            checked = true;
            reason = format!("dispositivo accoppiato «{}»; servono ~{} (KV cache ~{} MB), ne ha ~{} (non è memoria libera: è il totale riferito all'accoppiamento)", p.name, gb(need), kv, gb(p.budget_mb()));
        } else {
            let best = known.iter().map(|p| p.budget_mb()).max().unwrap_or(0);
            return Eval::Out(format!(
                "nessun dispositivo accoppiato ha memoria per il modello: servono ~{}, il maggiore ne ha ~{}",
                gb(need),
                gb(best)
            ));
        }
    } else {
        reason = "server remoto; nessun dato sulla sua memoria: stima non fatta".into();
    }
    let c = RouteCandidate {
        rank: 0,
        runtime_id: r.id.clone(),
        runtime_name: r.name.clone(),
        model: String::new(),
        accelerator: "remoto".into(),
        offload: "all".into(),
        gpu_layers: None,
        context: want,
        need_mb: need,
        kv_mb: kv,
        kv_estimated: est,
        remote: true,
        ready,
        reason,
    };
    // A device whose memory was checked comes before a server about which nothing is known.
    Eval::In(Box::new(c), u32::from(checked), r.route_priority)
}

/// Ranks the runtimes for a model. See the module documentation for the order.
pub fn rank(input: &RouteInput) -> RouteDecision {
    let mut found: Vec<(RouteCandidate, u32, u32, usize)> = Vec::new();
    let mut excluded = Vec::new();
    for (i, r) in input.runtimes.iter().enumerate() {
        match evaluate(r, input) {
            Eval::Irrelevant => {}
            Eval::Out(reason) => excluded.push(RouteExclusion { runtime_id: r.id.clone(), runtime_name: r.name.clone(), reason }),
            Eval::In(c, score, priority) => found.push((*c, score, priority, i)),
        }
    }
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.ready.cmp(&a.0.ready)).then_with(|| a.2.cmp(&b.2)).then_with(|| a.3.cmp(&b.3)));
    let candidates: Vec<RouteCandidate> = found
        .into_iter()
        .enumerate()
        .map(|(i, (mut c, ..))| {
            c.rank = i as u32 + 1;
            if i > 0 {
                c.reason = format!("{} (alternativa)", c.reason);
            }
            c
        })
        .collect();
    let mut notes = Vec::new();
    let spec = input.model;
    if spec.kv_bytes_per_token.is_none() && spec.system_runtime.is_none() {
        notes.push("La KV cache è una stima approssimata: il modello non dice la sua struttura.".to_string());
    }
    let hidden = input
        .runtimes
        .iter()
        .filter(|r| r.api != "none" && r.capabilities.iter().any(|c| c == "chat") && r.kind != "remote")
        .filter(|r| !spec.format.is_empty() && spec.system_runtime.is_none() && !r.formats.iter().any(|f| f == &spec.format))
        .count();
    if hidden > 0 {
        notes.push(format!("Altri {hidden} runtime non leggono il formato {}.", spec.format));
    }
    if candidates.iter().any(|c: &RouteCandidate| c.runtime_id == "remote-openai") {
        notes.push("Per un server remoto non ci sono dati sulla memoria: la stima non è fatta.".to_string());
    }
    RouteDecision { model: spec.id.clone(), wanted_context: input.want_context, candidates, excluded, notes }
}

// ------------------------------------------------------------------------------------- engine

/// The choice made when a run starts, and what to try if it does not work.
#[derive(Debug, Clone)]
pub struct RoutePlan {
    pub decision: RouteDecision,
    /// Up to `1 + MAX_FALLBACKS` steps, in order: runtime, model and context.
    pub steps: Vec<RouteStep>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteStep {
    pub runtime_id: String,
    pub runtime_name: String,
    pub model: String,
    pub context: u32,
    pub remote: bool,
}

impl RoutePlan {
    pub fn from_decision(decision: RouteDecision, model: &str, extra: Vec<RouteStep>) -> Option<Self> {
        let mut steps: Vec<RouteStep> = decision
            .candidates
            .iter()
            .map(|c| RouteStep {
                runtime_id: c.runtime_id.clone(),
                runtime_name: c.runtime_name.clone(),
                model: if c.model.is_empty() { model.to_string() } else { c.model.clone() },
                context: c.context,
                remote: c.remote,
            })
            .collect();
        steps.extend(extra);
        steps.truncate(1 + MAX_FALLBACKS);
        (!steps.is_empty()).then_some(Self { decision, steps })
    }
}

/// The wording of a switch, for the run note: «llama-server non è partito: uso il motore interno».
pub fn fallback_note(from: &str, kind: FailureKind, to: &str) -> String {
    let what = match kind {
        FailureKind::Start => "non è partito",
        FailureKind::Health => "non ha risposto in tempo",
        FailureKind::Memory => "non ha abbastanza memoria",
        FailureKind::Missing => "non è disponibile",
        FailureKind::Format => "non legge questo modello",
        FailureKind::Other => "non ha funzionato",
    };
    format!("{from} {what}: uso {to}")
}

impl crate::Engine {
    /// The model as the router needs it, from the models known now.
    pub(crate) fn model_spec(&self, id: &str) -> ModelSpec {
        let local = self.live_local_models();
        match local.iter().find(|m| m.id == id) {
            Some(m) => {
                let kv = if m.format == "gguf" {
                    m.path
                        .as_deref()
                        .and_then(|p| aiw_models::gguf_info_cached(std::path::Path::new(p)))
                        .and_then(|i| i.kv_cache_bytes(1024))
                        .map(|b| b / 1024)
                } else {
                    None
                };
                ModelSpec::from_local(m, kv)
            }
            None => ModelSpec { id: id.into(), ..Default::default() },
        }
    }

    /// The ranked runtimes for `model` on this device (`GET /api/route`).
    pub fn route(&self, model: &str, context: Option<u32>, allow_remote: bool) -> RouteDecision {
        let spec = self.model_spec(model);
        self.route_spec(&spec, context, allow_remote)
    }

    pub(crate) fn route_spec(&self, spec: &ModelSpec, context: Option<u32>, allow_remote: bool) -> RouteDecision {
        let want =
            context.filter(|c| *c > 0).unwrap_or(8192).min(spec.native_context.map(|c| c.min(u32::MAX as u64) as u32).unwrap_or(u32::MAX));
        let peers: Vec<RemotePeer> = self.peers().iter().map(RemotePeer::from_device).collect();
        let skips = self.0.route_failures.skips(&spec.id);
        let compiled = aiw_runtimes::embedded_accelerators();
        rank(&RouteInput {
            model: spec,
            want_context: want,
            device: &self.device(),
            runtimes: &self.0.runtimes.list(),
            peers: &peers,
            allow_remote,
            skips: &skips,
            embedded_accelerators: &compiled,
        })
    }

    /// Resolves a profile whose runtime is `auto`. Other profiles come back as they are, with no plan.
    pub(crate) fn resolve_runtime(&self, mut p: InferenceProfile) -> Result<(InferenceProfile, Option<RoutePlan>)> {
        if p.runtime_id != "auto" {
            return Ok((p, None));
        }
        if p.model.is_empty() {
            return Err(Error::Invalid(format!(
                "Il profilo \"{}\" ha il runtime automatico ma non un modello: sceglilo in Agenti → Profili.",
                p.name
            )));
        }
        let spec = self.model_spec(&p.model);
        let decision = self.route_spec(&spec, p.params.context, p.params.allow_remote);
        let mut extra = Vec::new();
        // A system model that fails continues on the smallest local model.
        if spec.system_runtime.is_some() {
            if let Some(local) = self.local_fallback_model() {
                let alt = self.route(&local, p.params.context, false);
                extra.extend(alt.candidates.iter().take(1).map(|c| RouteStep {
                    runtime_id: c.runtime_id.clone(),
                    runtime_name: c.runtime_name.clone(),
                    model: local.clone(),
                    context: c.context,
                    remote: false,
                }));
            }
        }
        let Some(plan) = RoutePlan::from_decision(decision.clone(), &p.model, extra) else {
            let why = decision.excluded.iter().take(4).map(|e| format!("{}: {}", e.runtime_name, e.reason)).collect::<Vec<_>>().join("; ");
            return Err(Error::Conflict(format!(
                "Nessun runtime può eseguire «{}» su questo dispositivo{}",
                p.model,
                if why.is_empty() { ".".to_string() } else { format!(": {why}.") }
            )));
        };
        let first = &plan.steps[0];
        p.runtime_id = first.runtime_id.clone();
        p.model = first.model.clone();
        p.params.context = Some(first.context);
        Ok((p, Some(plan)))
    }
}

/// Run event that says which runtime the router chose (and, later, when it moved on).
pub const RUNTIME_EVENT: &str = "aiw.runtime";

/// Payload of [`RUNTIME_EVENT`]. `switch`: the note of a fallback (`from`, `to`, `note`).
pub fn runtime_event(plan: &RoutePlan, switch: Option<(&RouteStep, &str, &str)>) -> serde_json::Value {
    use serde_json::json;
    let (step, note, from) = match switch {
        Some((to, note, from)) => (to, Some(note), Some(from)),
        None => (&plan.steps[0], None, None),
    };
    let reason = plan.decision.candidates.iter().find(|c| c.runtime_id == step.runtime_id).map(|c| c.reason.clone()).unwrap_or_default();
    json!({
        "requested": "auto",
        "runtime": step.runtime_id,
        "name": step.runtime_name,
        "model": step.model,
        "context": step.context,
        "reason": reason,
        "alternatives": plan.decision.candidates.iter().filter(|c| c.runtime_id != step.runtime_id)
            .map(|c| json!({"runtime": c.runtime_id, "name": c.runtime_name, "reason": c.reason})).collect::<Vec<_>>(),
        "excluded": plan.decision.excluded.iter().map(|e| json!({"runtime": e.runtime_id, "name": e.runtime_name, "reason": e.reason})).collect::<Vec<_>>(),
        "fallback": note.map(|n| json!({"from": from, "note": n})),
    })
}

type Starter = Box<dyn Fn(&RouteStep, u32) -> futures::future::BoxFuture<'static, Result<Arc<dyn ChatBackend>>> + Send + Sync>;
type Notifier = Box<dyn Fn(&RouteStep, &str, &str) + Send + Sync>;

/// What the start of a plan needs from the outside, so that it can be scripted in tests.
pub(crate) struct RouteEnv {
    /// Starts `runtime` for `model` with the context given (the runtime registry, in the engine).
    pub starter: Starter,
    pub memory: Arc<FailureMemory>,
    /// Tells the person (run event): the step now used, who failed, and the note.
    pub notify: Notifier,
}

/// Starts the first step of `plan`, from `from` on, that works. A step that fails is remembered (so
/// that it is skipped for a while); a memory failure first retries the same runtime with half the
/// context. Returns the index of the step that started, and its backend.
pub(crate) async fn start_chain(
    plan: &RoutePlan,
    from: usize,
    mut failed: Option<(String, FailureKind)>,
    env: &RouteEnv,
) -> Result<(usize, Arc<dyn ChatBackend>)> {
    let mut last = Error::Runtime("nessun runtime disponibile".into());
    for (i, step) in plan.steps.iter().enumerate().skip(from) {
        let mut ctx = step.context;
        let mut ctx_retry = false;
        loop {
            match (env.starter)(step, ctx).await {
                Ok(b) => {
                    if let Some((name, kind)) = failed.take() {
                        (env.notify)(step, &name, &fallback_note(&name, kind, &step.runtime_name));
                    } else if ctx_retry {
                        let note = format!("{} non ha abbastanza memoria con il contesto richiesto: uso contesto {ctx}", step.runtime_name);
                        (env.notify)(step, &step.runtime_name, &note);
                    }
                    return Ok((i, b));
                }
                Err(e) => {
                    let text = e.to_string();
                    let kind = FailureKind::classify(&text);
                    tracing::warn!("runtime {} non è utilizzabile per {}: {text}", step.runtime_id, step.model);
                    env.memory.record(&step.runtime_id, &step.model, kind, Some(ctx), &text);
                    if kind == FailureKind::Memory && !ctx_retry && ctx / 2 >= MIN_CONTEXT {
                        ctx_retry = true;
                        ctx /= 2;
                        continue;
                    }
                    if matches!(e, Error::Cancelled | Error::Denied(_)) {
                        // Not a problem of the runtime: do not hide it behind another one.
                        return Err(e);
                    }
                    failed = Some((step.runtime_name.clone(), if kind == FailureKind::Other { FailureKind::Start } else { kind }));
                    last = e;
                    break;
                }
            }
        }
    }
    Err(last)
}

/// The backend of a run whose runtime was chosen by the router: the first candidate that starts,
/// and, if it fails on the first request with a load error, the next ones (at most [`MAX_FALLBACKS`]
/// in all, never once the reply has started, and never a remote device unless the profile allows
/// it: the plan has none then).
pub(crate) async fn build_routed(plan: &RoutePlan, env: Arc<RouteEnv>) -> Result<Arc<dyn ChatBackend>> {
    let (index, backend) = start_chain(plan, 0, None, &env).await?;
    if index + 1 >= plan.steps.len() {
        return Ok(backend);
    }
    let cursor = Arc::new(AtomicUsize::new(index));
    let plan = Arc::new(plan.clone());
    let left = MAX_FALLBACKS.saturating_sub(index);
    let resolver: aiw_runtimes::FallbackResolver = {
        let (plan, env) = (plan.clone(), env.clone());
        Arc::new(move |attempt| {
            let (plan, env, cursor) = (plan.clone(), env.clone(), cursor.clone());
            Box::pin(async move {
                let kind = FailureKind::classify(&attempt.error);
                if !kind.is_load_error() {
                    return None;
                }
                let current = cursor.load(Ordering::SeqCst);
                let step = plan.steps.get(current)?;
                env.memory.record(&step.runtime_id, &step.model, kind, Some(step.context), &attempt.error);
                let (i, backend) = start_chain(&plan, current + 1, Some((step.runtime_name.clone(), kind)), &env).await.ok()?;
                cursor.store(i, Ordering::SeqCst);
                Some(aiw_runtimes::Fallback { backend, model: plan.steps[i].model.clone(), prefix: None })
            })
        })
    };
    Ok(Arc::new(aiw_runtimes::FallbackBackend::new(backend, resolver).with_max_fallbacks(left)))
}

impl crate::Engine {
    /// The backend of a run on `auto`, with the real runtime registry (see [`build_routed`]).
    pub(crate) async fn routed_backend(
        &self,
        plan: &RoutePlan,
        params: &aiw_core::api::InferenceParams,
        run_id: &str,
    ) -> Result<Arc<dyn ChatBackend>> {
        let adapters = params.adapters.clone();
        let engine = self.clone();
        let starter = {
            let engine = engine.clone();
            Box::new(move |step: &RouteStep, ctx: u32| -> futures::future::BoxFuture<'static, Result<Arc<dyn ChatBackend>>> {
                let (engine, step, adapters) = (engine.clone(), step.clone(), adapters.clone());
                Box::pin(async move {
                    let model = (!step.model.is_empty()).then_some(step.model.as_str());
                    engine.0.runtimes.backend_for(&step.runtime_id, model, Some(ctx), &adapters).await
                })
            })
        };
        let (plan2, rid) = (plan.clone(), run_id.to_string());
        let notify = Box::new(move |to: &RouteStep, from: &str, note: &str| {
            engine.emit(
                &rid,
                aiw_core::events::AgEvent::Custom { name: RUNTIME_EVENT.into(), value: runtime_event(&plan2, Some((to, note, from))) },
            );
        });
        build_routed(plan, Arc::new(RouteEnv { starter, memory: self.0.route_failures.clone(), notify })).await
    }
}
