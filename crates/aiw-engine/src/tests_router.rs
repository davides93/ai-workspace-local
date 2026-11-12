//! The runtime router: ranking per device class, exclusions with reasons, `auto` resolution, the
//! fallback path with scripted runtimes, and the memory of failures.

use super::router::*;
use super::tests::wait_finished;
use super::*;
use aiw_core::api::{Accelerator, CpuInfo, PeerHardware, RuntimeInfo};
use aiw_runtimes::{ChatDelta, ChatRequest, DeltaStream, RuntimeRegistry, ScriptedBackend};
use std::sync::Mutex;

fn accel(kind: &str, mem: Option<u64>) -> Accelerator {
    Accelerator { kind: kind.into(), name: kind.into(), vendor: None, memory_mb: mem, driver: None, details: None }
}

fn device(os: &str, arch: &str, ram_gb: u64, free_gb: u64, accels: Vec<Accelerator>) -> DeviceProfile {
    DeviceProfile {
        id: "d".into(),
        name: "d".into(),
        os: os.into(),
        os_version: String::new(),
        arch: arch.into(),
        form_factor: "desktop".into(),
        cpu: CpuInfo { brand: "cpu".into(), cores: 8, threads: 16, features: vec![] },
        memory_mb: ram_gb * 1024,
        available_memory_mb: free_gb * 1024,
        accelerators: [vec![accel("cpu", None)], accels].concat(),
        tier: String::new(),
    }
}

fn mac(ram_gb: u64) -> DeviceProfile {
    device("macos", "aarch64", ram_gb, ram_gb * 3 / 4, vec![accel("metal", None)])
}
fn nvidia(os: &str, vram_gb: u64) -> DeviceProfile {
    device(os, "x86_64", 32, 24, vec![accel("cuda", Some(vram_gb * 1024))])
}
fn cpu_only() -> DeviceProfile {
    device("linux", "x86_64", 16, 12, vec![])
}
fn phone() -> DeviceProfile {
    device("android", "aarch64", 8, 4, vec![])
}

/// The real manifests as the registry shows them on `os`/`arch`: every runtime this device can run
/// is "installable" except the ones named in `available` and `running`.
fn fleet(os: &str, arch: &str, available: &[&str], running: &[&str]) -> Vec<RuntimeInfo> {
    let dir = tempfile::tempdir().unwrap();
    let reg = RuntimeRegistry::new(os, None, vec![], dir.path().to_path_buf());
    reg.set_arch(arch);
    reg.list()
        .into_iter()
        .map(|mut r| {
            if r.status != "unsupported" {
                r.status = if running.contains(&r.id.as_str()) {
                    "running"
                } else if available.contains(&r.id.as_str()) {
                    "available"
                } else {
                    "installable"
                }
                .into();
            }
            r
        })
        .collect()
}

fn gguf(gb: f64) -> ModelSpec {
    ModelSpec {
        id: "m".into(),
        format: "gguf".into(),
        size_bytes: (gb * 1024.0 * 1024.0 * 1024.0) as u64,
        // 32 layers, 8 KV heads, 128 dim, f16: 128 KiB per token.
        kv_bytes_per_token: Some(128 * 1024),
        ..Default::default()
    }
}

fn route(model: &ModelSpec, device: &DeviceProfile, runtimes: &[RuntimeInfo], ctx: u32) -> aiw_core::api::RouteDecision {
    rank(&RouteInput {
        model,
        want_context: ctx,
        device,
        runtimes,
        peers: &[],
        allow_remote: false,
        skips: &[],
        embedded_accelerators: &["cpu", "metal"],
    })
}

fn ids(d: &aiw_core::api::RouteDecision) -> Vec<&str> {
    d.candidates.iter().map(|c| c.runtime_id.as_str()).collect()
}

fn why<'a>(d: &'a aiw_core::api::RouteDecision, id: &str) -> &'a str {
    &d.excluded.iter().find(|e| e.runtime_id == id).unwrap_or_else(|| panic!("{id} is not excluded: {:?}", d.excluded)).reason
}

const ALL_GGUF: &[&str] = &["llama-server", "ollama", "lmstudio", "mistralrs", "koboldcpp", "llamafile", "localai", "nexa", "lemonade"];

#[test]
fn apple_silicon_prefers_the_built_in_engine_on_metal_then_the_servers() {
    let d = mac(16);
    let rts = fleet("macos", "aarch64", ALL_GGUF, &["llama-embedded"]);
    let r = route(&gguf(4.9), &d, &rts, 8192);
    assert_eq!(&ids(&r)[..3], ["llama-embedded", "llama-server", "ollama"], "{:?}", ids(&r));
    let first = &r.candidates[0];
    assert_eq!((first.accelerator.as_str(), first.offload.as_str(), first.context, first.rank), ("metal", "all", 8192, 1));
    assert!(first.ready && first.reason.contains("Metal") && first.reason.contains("integrato nell'app"), "{}", first.reason);
    assert!(
        r.candidates[1].reason.contains("AIW lo avvia") && r.candidates[1].reason.contains("alternativa"),
        "{}",
        r.candidates[1].reason
    );
    // Everything that reads GGUF is either ranked or excluded with a reason; the rest is only counted.
    assert!(!why(&r, "vllm").is_empty());
    assert!(r.notes.iter().any(|n| n.contains("non leggono il formato gguf")), "{:?}", r.notes);
}

#[test]
fn apple_silicon_runs_mlx_repositories_on_mlx_lm() {
    let mlx =
        ModelSpec { id: "q".into(), format: "mlx".into(), size_bytes: 2 << 30, kv_bytes_per_token: Some(100_000), ..Default::default() };
    let rts = fleet("macos", "aarch64", &["mlx-lm", "lmstudio"], &[]);
    let r = route(&mlx, &mac(16), &rts, 8192);
    assert_eq!(ids(&r)[..2], ["mlx-lm", "lmstudio"]);
    assert_eq!(why(&r, "mlx-swift"), "non ancora implementato in questa versione");
    // An Intel Mac has no MLX at all: the runtime is not even a candidate to exclude.
    let intel = fleet("macos", "x86_64", &["mlx-lm"], &[]);
    let r = route(&mlx, &device("macos", "x86_64", 16, 12, vec![accel("metal", Some(2048))]), &intel, 8192);
    assert!(!ids(&r).contains(&"mlx-lm"));
    assert!(why(&r, "mlx-lm").contains("non supportato"), "{}", why(&r, "mlx-lm"));
}

#[test]
fn nvidia_prefers_llama_server_on_cuda_and_puts_vllm_last_for_gguf() {
    for os in ["linux", "windows"] {
        let rts = fleet(os, "x86_64", ALL_GGUF, &[]);
        let r = route(&gguf(4.9), &nvidia(os, 12), &rts, 8192);
        assert_eq!(r.candidates[0].runtime_id, "llama-server", "{os}");
        assert_eq!((r.candidates[0].accelerator.as_str(), r.candidates[0].offload.as_str()), ("cuda", "all"), "{os}");
        assert_eq!(r.candidates[0].gpu_layers, Some(-1));
        // The built-in engine of this build has no CUDA: not ranked as a GPU engine (and not available here at all).
        assert!(why(&r, "llama-embedded").contains("plugin nativo"));
    }
    // Linux also has vLLM, with a lower priority for GGUF; safetensors is its home.
    let rts = fleet("linux", "x86_64", &["llama-server", "vllm", "sglang", "mistralrs", "trtllm"], &[]);
    let r = route(&gguf(4.9), &nvidia("linux", 12), &rts, 8192);
    assert_eq!(ids(&r), ["llama-server", "mistralrs", "vllm"]);
    let st = ModelSpec {
        id: "s".into(),
        format: "safetensors".into(),
        size_bytes: 6 << 30,
        kv_bytes_per_token: Some(131072),
        ..Default::default()
    };
    let r = route(&st, &nvidia("linux", 24), &rts, 8192);
    assert_eq!(ids(&r)[..2], ["vllm", "sglang"]);
    // Windows has neither vLLM nor SGLang.
    let r = route(&st, &nvidia("windows", 24), &fleet("windows", "x86_64", &["trtllm", "mistralrs"], &[]), 8192);
    assert!(!ids(&r).contains(&"vllm"));
}

#[test]
fn cpu_only_machines_run_everything_on_the_cpu() {
    let rts = fleet("linux", "x86_64", ALL_GGUF, &[]);
    let r = route(&gguf(2.5), &cpu_only(), &rts, 8192);
    assert_eq!(&ids(&r)[..3], ["llama-server", "ollama", "lmstudio"]);
    assert!(r.candidates.iter().all(|c| c.accelerator == "cpu" && c.offload == "none"), "{:?}", r.candidates);
    assert_eq!(r.candidates[0].gpu_layers, Some(0));
    // A runtime that needs a GPU says so.
    let r = route(
        &ModelSpec { id: "t".into(), format: "exl2".into(), size_bytes: 3 << 30, ..Default::default() },
        &cpu_only(),
        &fleet("linux", "x86_64", &["tabbyapi"], &[]),
        4096,
    );
    assert!(why(&r, "tabbyapi").contains("serve CUDA"), "{}", why(&r, "tabbyapi"));
}

#[test]
fn a_phone_has_only_the_built_in_engine() {
    let rts = fleet("android", "aarch64", &[], &["llama-embedded"]);
    let r = route(&gguf(1.0), &phone(), &rts, 4096);
    assert_eq!(ids(&r), ["llama-embedded"]);
    assert_eq!(r.candidates[0].accelerator, "cpu");
    // The sidecars do not exist there: they are not offered, and say why when listed.
    assert!(r.excluded.iter().all(|e| e.runtime_id != "llama-server") || why(&r, "llama-server").contains("non supportato"));
}

#[test]
fn system_models_are_served_by_their_platform_runtime_only() {
    let sys =
        ModelSpec { id: "apple-fm-system".into(), format: "system".into(), system_runtime: Some("apple-fm".into()), ..Default::default() };
    let rts = fleet("macos", "aarch64", ALL_GGUF, &["apple-fm", "llama-embedded"]);
    let r = route(&sys, &mac(16), &rts, 4096);
    assert_eq!(ids(&r), ["apple-fm"]);
    assert_eq!(r.candidates[0].need_mb, 0);
    assert!(r.notes.is_empty(), "no KV estimate for a system model: {:?}", r.notes);
    // Not available (no native plugin registered): excluded with the reason.
    let r = route(&sys, &mac(16), &fleet("macos", "aarch64", &[], &[]), 4096);
    assert!(r.candidates.is_empty());
    assert!(why(&r, "apple-fm").contains("plugin nativo"));
}

#[test]
fn exclusions_say_why() {
    let rts = fleet("linux", "x86_64", &["llama-server"], &[]);
    let r = route(&gguf(4.9), &cpu_only(), &rts, 8192);
    assert!(why(&r, "ollama").starts_with("non installato"), "{}", why(&r, "ollama"));
    assert!(why(&r, "llama-embedded").contains("plugin nativo"));
    let mut rts = rts;
    rts.iter_mut().find(|x| x.id == "ollama").unwrap().status = "stopped".into();
    rts.iter_mut().find(|x| x.id == "lmstudio").unwrap().status = "error".into();
    rts.iter_mut().find(|x| x.id == "lmstudio").unwrap().status_detail = Some("si è chiuso".into());
    let r = route(&gguf(4.9), &cpu_only(), &rts, 8192);
    assert_eq!(why(&r, "ollama"), "non raggiungibile");
    assert_eq!(why(&r, "lmstudio"), "si è chiuso");
    // Remote runtimes need the profile's consent.
    assert!(why(&r, "aiw-peer").contains("il profilo non lo consente"));
}

#[test]
fn memory_decides_the_context_the_offload_and_the_exclusion() {
    let rts = fleet("linux", "x86_64", &["llama-server"], &[]);
    // 8 GB free, a 4.9 GB model: weights ~6.1 GB + KV. 32768 tokens of KV is 4 GB: halved until it fits.
    let d = device("linux", "x86_64", 16, 8, vec![]);
    let r = route(&gguf(4.9), &d, &rts, 32768);
    let c = &r.candidates[0];
    assert!(c.context < 32768 && c.context >= 2048, "{}", c.context);
    assert!(c.need_mb <= 8 * 1024, "{}", c.need_mb);
    assert!(c.reason.contains("contesto ridotto da 32768"), "{}", c.reason);
    assert!(!c.kv_estimated);
    // Too big even at 2048: excluded, with the numbers.
    let tiny = device("linux", "x86_64", 4, 3, vec![]);
    let r = route(&gguf(8.0), &tiny, &rts, 8192);
    assert!(r.candidates.is_empty());
    assert!(why(&r, "llama-server").starts_with("memoria insufficiente: servono ~"), "{}", why(&r, "llama-server"));
    // GPU too small for the whole model: split between GPU and RAM.
    let r = route(&gguf(10.0), &nvidia("linux", 8), &rts, 4096);
    assert_eq!((r.candidates[0].offload.as_str(), r.candidates[0].gpu_layers), ("partial", None));
    // A GPU that holds it all is better than a context at full size on the CPU.
    let r = route(&gguf(4.0), &nvidia("linux", 8), &rts, 8192);
    assert_eq!(r.candidates[0].offload, "all");
    // KV unknown: estimated and said so.
    let mut unknown = gguf(4.0);
    unknown.kv_bytes_per_token = None;
    let r = route(&unknown, &nvidia("linux", 8), &rts, 8192);
    assert!(r.candidates[0].kv_estimated && r.notes.iter().any(|n| n.contains("stima approssimata")));
}

#[test]
fn a_runtime_that_already_serves_the_model_is_not_asked_for_more_memory() {
    let mut rts = fleet("linux", "x86_64", &[], &["ollama"]);
    rts.iter_mut().find(|x| x.id == "ollama").unwrap().loaded_models = vec!["m".into()];
    // The model would not fit in the 3 GB that are free, but it is already loaded.
    let r = route(&gguf(8.0), &device("linux", "x86_64", 16, 3, vec![]), &rts, 8192);
    assert_eq!(ids(&r), ["ollama"]);
    assert!(r.candidates[0].reason.contains("ha già il modello in memoria"));
    // A name nobody has on disk (an Ollama tag) is routed only to the server that lists it.
    let tag = ModelSpec { id: "qwen3:4b".into(), ..Default::default() };
    let mut rts = fleet("linux", "x86_64", &["llama-server"], &["ollama"]);
    rts.iter_mut().find(|x| x.id == "ollama").unwrap().loaded_models = vec!["qwen3:4b".into()];
    assert_eq!(ids(&route(&tag, &cpu_only(), &rts, 8192)), ["ollama"]);
}

#[test]
fn remote_candidates_come_last_and_only_when_allowed() {
    let rts = fleet("linux", "x86_64", &["llama-server"], &["aiw-peer", "remote-openai"]);
    let peers = [
        RemotePeer { name: "Mac studio".into(), memory_mb: 96 * 1024, vram_mb: 0, unified: true, known: true, models: vec![] },
        RemotePeer { name: "Telefono".into(), memory_mb: 0, vram_mb: 0, unified: false, known: false, models: vec!["altro".into()] },
    ];
    let spec = gguf(40.0);
    let cpu = cpu_only();
    let input = |allow| RouteInput {
        model: &spec,
        want_context: 8192,
        device: &cpu,
        runtimes: &rts,
        peers: &peers,
        allow_remote: allow,
        skips: &[],
        embedded_accelerators: &["cpu"],
    };
    let off = rank(&input(false));
    assert!(off.candidates.is_empty(), "40 GB does not fit this device: {:?}", off.candidates);
    let on = rank(&input(true));
    assert_eq!(ids(&on), ["aiw-peer", "remote-openai"]);
    assert!(on.candidates.iter().all(|c| c.remote));
    assert!(on.candidates[0].reason.contains("Mac studio") && on.candidates[0].reason.contains("KV cache"), "{}", on.candidates[0].reason);
    assert!(on.candidates[1].reason.contains("nessun dato sulla sua memoria"));
    // A local candidate always precedes the remote ones.
    let small = gguf(2.0);
    let r = rank(&RouteInput { model: &small, ..input(true) });
    assert_eq!(ids(&r)[0], "llama-server");
    // A device that is too small for the model is not offered; no data = no estimate, said so.
    let weak = [RemotePeer { name: "Piccolo".into(), memory_mb: 8 * 1024, vram_mb: 0, unified: false, known: true, models: vec![] }];
    let r = rank(&RouteInput { peers: &weak, ..input(true) });
    assert!(why(&r, "aiw-peer").contains("nessun dispositivo accoppiato ha memoria"), "{}", why(&r, "aiw-peer"));
    let unknown = [RemotePeer { name: "Ignoto".into(), memory_mb: 0, vram_mb: 0, unified: false, known: false, models: vec![] }];
    let r = rank(&RouteInput { peers: &unknown, ..input(true) });
    assert!(r.candidates.iter().find(|c| c.runtime_id == "aiw-peer").unwrap().reason.contains("memoria del dispositivo non nota"));
    assert!(
        RemotePeer::from_device(&aiw_core::api::PeerDevice {
            id: "p".into(),
            name: "p".into(),
            os: "macos".into(),
            address: String::new(),
            status: "online".into(),
            shared_runtimes: vec![],
            hardware: Some(PeerHardware {
                arch: "aarch64".into(),
                tier: "l".into(),
                memory_mb: 32768,
                vram_mb: 0,
                accelerators: vec![],
                unified_memory: true
            }),
        })
        .known
    );
}

#[test]
fn failed_candidates_are_skipped_and_memory_failures_cap_the_context() {
    let rts = fleet("linux", "x86_64", &["llama-server", "ollama"], &[]);
    let skip = |kind, ctx| Skip { runtime_id: "llama-server".into(), kind, context: ctx, detail: "boom".into() };
    let with = |skips: &[Skip]| {
        rank(&RouteInput {
            model: &gguf(2.0),
            want_context: 8192,
            device: &cpu_only(),
            runtimes: &rts,
            peers: &[],
            allow_remote: false,
            skips,
            embedded_accelerators: &["cpu"],
        })
    };
    let r = with(&[skip(FailureKind::Start, None)]);
    assert_eq!(ids(&r), ["ollama"]);
    assert_eq!(why(&r, "llama-server"), "ha già fallito da poco con questo modello: boom");
    let r = with(&[skip(FailureKind::Memory, Some(8192))]);
    assert_eq!(r.candidates[0].runtime_id, "llama-server");
    assert_eq!(r.candidates[0].context, 4096, "below the context that ran out of memory");
    // Failed at the smallest context already: nothing left to reduce.
    assert_eq!(ids(&with(&[skip(FailureKind::Memory, Some(2048))])), ["ollama"]);
}

#[test]
fn failures_are_forgotten_after_the_ttl() {
    let mem = FailureMemory::new(Duration::from_secs(60));
    let t0 = Instant::now();
    mem.record_at(t0, "llama-server", "m", FailureKind::Start, None, "boom");
    mem.record_at(t0 + Duration::from_secs(30), "ollama", "m", FailureKind::Memory, Some(8192), "oom");
    mem.record_at(t0, "ollama", "other", FailureKind::Start, None, "x");
    let at = |s| mem.skips_at(t0 + Duration::from_secs(s), "m").into_iter().map(|k| k.runtime_id).collect::<Vec<_>>();
    assert_eq!(at(10), ["llama-server", "ollama"]);
    assert_eq!(at(59), ["llama-server", "ollama"]);
    assert_eq!(at(61), ["ollama"], "the first one expired, the second is 31 seconds old");
    assert!(at(95).is_empty());
    // The same candidate failing again renews the entry instead of piling up.
    mem.record_at(t0 + Duration::from_secs(100), "ollama", "m", FailureKind::Start, None, "again");
    assert_eq!(mem.skips_at(t0 + Duration::from_secs(110), "m").len(), 1);
    mem.clear();
    assert!(mem.skips_at(t0, "m").is_empty());
}

#[test]
fn errors_are_classified_by_what_they_say() {
    use FailureKind::*;
    for (text, kind) in [
        ("CUDA error: out of memory", Memory),
        ("failed to allocate buffer", Memory),
        ("llama-server non è pronto dopo 180 s", Health),
        ("llama-server si è chiuso durante l’avvio (exit status: 1)", Start),
        ("impossibile avviare /usr/bin/llama-server: No such file or directory", Missing),
        ("unknown model architecture: 'foo'", Format),
        ("HTTP 400: prompt troppo lungo", Other),
    ] {
        assert_eq!(FailureKind::classify(text), kind, "{text}");
    }
    assert!(Memory.is_load_error() && !Other.is_load_error());
}

// ----------------------------------------------------------------------------- the start chain

fn plan(runtimes: &[&str]) -> RoutePlan {
    let d = aiw_core::api::RouteDecision {
        model: "m".into(),
        wanted_context: 8192,
        candidates: runtimes
            .iter()
            .enumerate()
            .map(|(i, r)| aiw_core::api::RouteCandidate {
                rank: i as u32 + 1,
                runtime_id: r.to_string(),
                runtime_name: r.to_string(),
                model: String::new(),
                accelerator: "cpu".into(),
                offload: "none".into(),
                gpu_layers: None,
                context: 8192,
                need_mb: 1000,
                kv_mb: 100,
                kv_estimated: false,
                remote: false,
                ready: true,
                reason: format!("motivo {r}"),
            })
            .collect(),
        excluded: vec![],
        notes: vec![],
    };
    RoutePlan::from_decision(d, "m", vec![]).unwrap()
}

type Script = Arc<Mutex<Vec<(String, u32)>>>;

/// An environment whose runtimes behave as `behaviour` says: `Ok(text)` serves it, `Err(start)` fails to
/// start, `Load(text)` starts and fails on the first request, `Late` produces a token and then fails.
#[derive(Clone)]
enum Behaviour {
    Serves(&'static str),
    FailsToStart(&'static str),
    FailsOnFirstRequest(&'static str),
    FailsAfterAToken,
    /// Out of memory above this context, serves below.
    MemoryAbove(u32),
}

struct Failing(String);
#[async_trait::async_trait]
impl ChatBackend for Failing {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }
    async fn chat(&self, _: ChatRequest, _: CancellationToken) -> Result<DeltaStream> {
        Err(Error::Runtime(self.0.clone()))
    }
}
struct Breaks;
#[async_trait::async_trait]
impl ChatBackend for Breaks {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }
    async fn chat(&self, _: ChatRequest, _: CancellationToken) -> Result<DeltaStream> {
        use futures::StreamExt;
        Ok(futures::stream::iter(vec![Ok(ChatDelta::Text("inizio".into())), Err(Error::Runtime("out of memory".into()))]).boxed())
    }
}

fn scripted(behaviours: Vec<(&'static str, Behaviour)>) -> (Arc<RouteEnv>, Script, Arc<Mutex<Vec<String>>>) {
    let starts: Script = Default::default();
    let notes: Arc<Mutex<Vec<String>>> = Default::default();
    let (s2, n2) = (starts.clone(), notes.clone());
    let env = RouteEnv {
        starter: Box::new(move |step, ctx| {
            s2.lock().unwrap().push((step.runtime_id.clone(), ctx));
            let b = behaviours.iter().find(|(id, _)| *id == step.runtime_id).map(|(_, b)| b.clone()).unwrap();
            Box::pin(async move {
                match b {
                    Behaviour::Serves(t) => Ok(Arc::new(ScriptedBackend::new(vec![ScriptedBackend::text_turn(t)])) as Arc<dyn ChatBackend>),
                    Behaviour::FailsToStart(e) => Err(Error::Runtime(e.into())),
                    Behaviour::FailsOnFirstRequest(e) => Ok(Arc::new(Failing(e.into())) as Arc<dyn ChatBackend>),
                    Behaviour::FailsAfterAToken => Ok(Arc::new(Breaks) as Arc<dyn ChatBackend>),
                    Behaviour::MemoryAbove(limit) if ctx > limit => Err(Error::Runtime("CUDA error: out of memory".into())),
                    Behaviour::MemoryAbove(_) => {
                        Ok(Arc::new(ScriptedBackend::new(vec![ScriptedBackend::text_turn(&format!("ctx {ctx}"))])) as Arc<dyn ChatBackend>)
                    }
                }
            })
        }),
        memory: Arc::new(FailureMemory::default()),
        notify: Box::new(move |to, _from, note| n2.lock().unwrap().push(format!("{}: {note}", to.runtime_id))),
    };
    (Arc::new(env), starts, notes)
}

async fn said(b: &Arc<dyn ChatBackend>) -> Result<String> {
    use futures::StreamExt;
    let mut s = b.chat(ChatRequest { model: "m".into(), ..Default::default() }, CancellationToken::new()).await?;
    let mut out = String::new();
    while let Some(d) = s.next().await {
        if let ChatDelta::Text(t) = d? {
            out.push_str(&t);
        }
    }
    Ok(out)
}

#[tokio::test]
async fn the_first_runtime_that_works_is_used_without_a_word() {
    let (env, starts, notes) = scripted(vec![("a", Behaviour::Serves("da a")), ("b", Behaviour::Serves("da b"))]);
    let b = build_routed(&plan(&["a", "b"]), env).await.unwrap();
    assert_eq!(said(&b).await.unwrap(), "da a");
    assert_eq!(starts.lock().unwrap().len(), 1);
    assert!(notes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_runtime_that_does_not_start_hands_over_to_the_next_with_a_note() {
    let (env, _starts, notes) = scripted(vec![
        ("llama-server", Behaviour::FailsToStart("llama-server si è chiuso durante l’avvio (exit status: 1)")),
        ("llama-embedded", Behaviour::Serves("motore interno")),
    ]);
    let mem = env.memory.clone();
    let b = build_routed(&plan(&["llama-server", "llama-embedded"]), env).await.unwrap();
    assert_eq!(said(&b).await.unwrap(), "motore interno");
    assert_eq!(notes.lock().unwrap().as_slice(), ["llama-embedded: llama-server non è partito: uso llama-embedded"]);
    // Remembered: the next resolution of the same model leaves it out.
    let skips = mem.skips("m");
    assert_eq!(skips.len(), 1);
    assert_eq!((skips[0].runtime_id.as_str(), skips[0].kind), ("llama-server", FailureKind::Start));
}

#[tokio::test]
async fn a_failed_health_check_is_a_start_failure_too() {
    let (env, _s, notes) = scripted(vec![("a", Behaviour::FailsToStart("A non è pronto dopo 180 s")), ("b", Behaviour::Serves("b"))]);
    let b = build_routed(&plan(&["a", "b"]), env).await.unwrap();
    assert_eq!(said(&b).await.unwrap(), "b");
    assert!(notes.lock().unwrap()[0].contains("a non ha risposto in tempo: uso b"), "{:?}", notes.lock().unwrap());
}

#[tokio::test]
async fn a_load_error_on_the_first_request_moves_on() {
    let (env, starts, notes) =
        scripted(vec![("a", Behaviour::FailsOnFirstRequest("unknown model architecture: 'x'")), ("b", Behaviour::Serves("da b"))]);
    let b = build_routed(&plan(&["a", "b"]), env).await.unwrap();
    assert_eq!(said(&b).await.unwrap(), "da b");
    assert_eq!(starts.lock().unwrap().iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    assert!(notes.lock().unwrap()[0].contains("a non legge questo modello: uso b"));
    // An error that is not a load error (a refused request) is not a reason to change runtime.
    let (env, starts, _n) = env_one("a", Behaviour::FailsOnFirstRequest("HTTP 400: richiesta non valida"));
    let b = build_routed(&plan(&["a", "b"]), env).await.unwrap();
    assert!(said(&b).await.unwrap_err().to_string().contains("richiesta non valida"));
    assert_eq!(starts.lock().unwrap().len(), 1);
}

fn env_one(first: &'static str, b: Behaviour) -> (Arc<RouteEnv>, Script, Arc<Mutex<Vec<String>>>) {
    scripted(vec![(first, b), ("b", Behaviour::Serves("da b"))])
}

#[tokio::test]
async fn memory_failures_reduce_the_context_before_the_next_runtime_is_tried() {
    let (env, starts, notes) = scripted(vec![("a", Behaviour::MemoryAbove(2048 * 2)), ("b", Behaviour::Serves("da b"))]);
    let b = build_routed(&plan(&["a", "b"]), env).await.unwrap();
    // 8192 fails, the retry at 4096 works.
    assert_eq!(said(&b).await.unwrap(), "ctx 4096");
    assert_eq!(starts.lock().unwrap().as_slice(), [("a".to_string(), 8192), ("a".to_string(), 4096)]);
    assert!(notes.lock().unwrap()[0].contains("non ha abbastanza memoria con il contesto richiesto: uso contesto 4096"));
    // Too little even at half: the next runtime.
    let (env, starts, notes) = scripted(vec![("a", Behaviour::MemoryAbove(1024)), ("b", Behaviour::Serves("da b"))]);
    let b = build_routed(&plan(&["a", "b"]), env).await.unwrap();
    assert_eq!(said(&b).await.unwrap(), "da b");
    assert_eq!(starts.lock().unwrap().len(), 3);
    assert!(notes.lock().unwrap()[0].contains("a non ha abbastanza memoria: uso b"));
}

#[tokio::test]
async fn there_is_no_fallback_once_the_reply_has_started() {
    let (env, starts, notes) = scripted(vec![("a", Behaviour::FailsAfterAToken), ("b", Behaviour::Serves("da b"))]);
    let b = build_routed(&plan(&["a", "b"]), env).await.unwrap();
    let err = said(&b).await.unwrap_err();
    assert!(err.to_string().contains("out of memory"), "{err}");
    assert_eq!(starts.lock().unwrap().len(), 1, "b was never started");
    assert!(notes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn at_most_two_fallbacks_and_then_the_error_of_the_last_one() {
    let (env, starts, _n) = scripted(vec![
        ("a", Behaviour::FailsToStart("a non è partito")),
        ("b", Behaviour::FailsToStart("b non è partito")),
        ("c", Behaviour::FailsToStart("c non è partito")),
        ("d", Behaviour::Serves("da d")),
    ]);
    // The plan itself holds the first choice and two fallbacks: `d` is not part of it.
    let p = plan(&["a", "b", "c", "d"]);
    assert_eq!(p.steps.len(), 1 + MAX_FALLBACKS);
    let err = build_routed(&p, env).await.err().unwrap();
    assert!(err.to_string().contains("c non è partito"), "{err}");
    assert_eq!(starts.lock().unwrap().iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    // A start failure uses up one of the two fallbacks: a then fails on the first request, b serves.
    let (env, _s, _n) = scripted(vec![
        ("a", Behaviour::FailsToStart("a non è partito")),
        ("b", Behaviour::FailsOnFirstRequest("failed to load model")),
        ("c", Behaviour::Serves("da c")),
    ]);
    let b = build_routed(&plan(&["a", "b", "c"]), env).await.unwrap();
    assert_eq!(said(&b).await.unwrap(), "da c");
}

// --------------------------------------------------------------------------------- the engine

fn local(id: &str, gb: f64) -> LocalModel {
    serde_json::from_value(json!({
        "id": id, "name": id, "path": format!("/models/{id}.gguf"), "format": "gguf",
        "size_bytes": (gb * 1024.0 * 1024.0 * 1024.0) as u64, "context_length": 40960,
        "capabilities": ["chat", "tools"], "runtimes": ["llama-embedded"], "params_b": 4.0,
    }))
    .unwrap()
}

async fn engine_on(device: DeviceProfile) -> (Engine, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().to_path_buf()),
        workspace_root: Some(ws.path().to_path_buf()),
        mode: "embedded".into(),
        device: Some(device),
        extra_bin_dirs: vec![],
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap();
    (e, data, ws)
}

fn auto_profile(model: &str) -> InferenceProfile {
    InferenceProfile {
        id: "auto-test".into(),
        name: "Automatico".into(),
        description: None,
        runtime_id: "auto".into(),
        model: model.into(),
        params: InferenceParams { context: Some(8192), ..Default::default() },
    }
}

#[tokio::test]
async fn an_auto_profile_is_saved_and_resolved_when_used() {
    let (e, _d, _w) = engine_on(cpu_only()).await;
    e.register_native_runtime("llama-embedded", Arc::new(ScriptedBackend::new(vec![])));
    *e.0.models.write().unwrap() = vec![local("qwen", 2.5)];
    e.save_profile(auto_profile("qwen")).unwrap();
    // Stored as `auto`, resolved on use.
    assert_eq!(e.profiles().unwrap().iter().find(|p| p.id == "auto-test").unwrap().runtime_id, "auto");
    let (p, plan) = e.profile_routed("auto-test").unwrap();
    assert_eq!((p.runtime_id.as_str(), p.model.as_str()), ("llama-embedded", "qwen"));
    assert!(plan.unwrap().decision.candidates[0].reason.contains("sulla CPU"));
    assert_eq!(e.profile("auto-test").unwrap().runtime_id, "llama-embedded");
    // Explicit profiles are untouched.
    assert!(e.profile_routed("default").unwrap().1.is_none());
    // Needs a model; adapters need a real runtime; and a model no runtime can run is a clear conflict.
    assert!(e.save_profile(InferenceProfile { model: String::new(), ..auto_profile("") }).is_err());
    let mut with_adapter = auto_profile("qwen");
    with_adapter.params.adapters = vec![aiw_core::api::AdapterRef { path: "/x.gguf".into(), scale: None }];
    assert!(e.save_profile(with_adapter).is_err());
    *e.0.models.write().unwrap() = vec![local("huge", 200.0)];
    e.save_profile(auto_profile("huge")).unwrap();
    let err = e.profile("auto-test").unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "{err}");
    // The API view of the same decision.
    let d = e.route("qwen", Some(4096), false);
    assert_eq!((d.model.as_str(), d.wanted_context), ("qwen", 4096));
}

#[tokio::test]
async fn a_run_on_auto_announces_the_choice_and_falls_back_with_a_note() {
    let (e, _d, _w) = engine_on(cpu_only()).await;
    // The first choice cannot load the model; the second one answers.
    e.register_native_runtime("llama-embedded", Arc::new(Failing("failed to load model: out of memory".into())));
    e.register_native_runtime("koboldcpp", Arc::new(ScriptedBackend::new(vec![ScriptedBackend::text_turn("risposta di kobold")])));
    *e.0.models.write().unwrap() = vec![local("qwen", 2.5)];
    e.save_profile(auto_profile("qwen")).unwrap();
    let base = e.agent("operator").unwrap();
    e.save_agent(AgentProfile { id: "auto-agent".into(), inference_profile: "auto-test".into(), ..base }).unwrap();
    let s = e.create_session("auto-agent", None).unwrap();
    let run = e.start_run(&s.id, "ciao".into()).await.unwrap();
    wait_finished(&e, &run).await;
    let events: Vec<serde_json::Value> = e
        .run_events(&run)
        .iter()
        .filter_map(|ev| match &ev.event.event {
            AgEvent::Custom { name, value } if name == RUNTIME_EVENT => Some(value.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!((events[0]["requested"].as_str(), events[0]["runtime"].as_str()), (Some("auto"), Some("llama-embedded")));
    assert!(events[0]["reason"].as_str().unwrap().contains("sulla CPU"));
    assert!(events[0]["alternatives"].as_array().unwrap().iter().any(|a| a["runtime"] == "koboldcpp"));
    assert_eq!(events[1]["runtime"], "koboldcpp");
    let note = events[1]["fallback"]["note"].as_str().unwrap();
    assert!(note.contains("non ha abbastanza memoria: uso") && note.contains("KoboldCpp"), "{note}");
    let answer = e.messages(&s.id).unwrap().into_iter().rev().find(|m| m.role == "assistant").unwrap().content;
    assert!(answer.contains("risposta di kobold"), "{answer}");
    // The failure is remembered: the next route leaves the first runtime out, with the reason.
    let again = e.route("qwen", Some(8192), false);
    assert!(
        again.excluded.iter().any(|x| x.runtime_id == "llama-embedded" && x.reason.contains("già fallito"))
            || again.candidates[0].runtime_id == "llama-embedded" && again.candidates[0].context < 8192
    );
}
