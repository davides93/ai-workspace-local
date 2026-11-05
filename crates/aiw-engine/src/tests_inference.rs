//! Engine-level checks of the inference wiring: registration of the in-process engine, the
//! default profile of a device, and the profile context reaching the runtime.

use super::*;
use aiw_agent::RunHost;
use aiw_core::api::{Accelerator, CpuInfo};
use aiw_runtimes::{ChatRequest, DeltaStream};

fn phone(os: &str) -> DeviceProfile {
    DeviceProfile {
        id: "dev_phone".into(),
        name: "phone".into(),
        os: os.into(),
        os_version: String::new(),
        arch: "aarch64".into(),
        form_factor: "phone".into(),
        cpu: CpuInfo { brand: "SoC".into(), cores: 8, threads: 8, features: vec![] },
        memory_mb: 8 * 1024,
        available_memory_mb: 4 * 1024,
        accelerators: vec![Accelerator {
            kind: "cpu".into(),
            name: "SoC".into(),
            vendor: None,
            memory_mb: None,
            driver: None,
            details: None,
        }],
        tier: "s".into(),
    }
}

async fn engine_for(device: Option<DeviceProfile>) -> (Engine, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().to_path_buf()),
        workspace_root: Some(ws.path().to_path_buf()),
        mode: "embedded".into(),
        device,
        extra_bin_dirs: vec![],
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap();
    (e, data, ws)
}

/// Backend that answers with the context window it was made for.
struct CtxProbe(u32);

#[async_trait::async_trait]
impl ChatBackend for CtxProbe {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![format!("ctx={}", self.0)])
    }
    async fn chat(&self, _req: ChatRequest, _cancel: CancellationToken) -> Result<DeltaStream> {
        Err(Error::Unsupported("test".into()))
    }
    fn with_context(&self, ctx: u32) -> Option<Arc<dyn ChatBackend>> {
        Some(Arc::new(CtxProbe(ctx)))
    }
}

/// `llama-embedded` is registered exactly when the engine's `llama-embedded` feature is on. The
/// GPU features (`llama-metal|cuda|vulkan`) imply it, so a build with only one of them used to
/// link llama.cpp without ever registering the runtime. Run with
/// `cargo test -p aiw-engine --features llama-embedded` to see the registered case.
#[tokio::test]
async fn embedded_runtime_is_registered_when_the_feature_is_on() {
    let (e, _d, _w) = engine_for(None).await;
    let info = e.runtimes().into_iter().find(|r| r.id == "llama-embedded").unwrap();
    if cfg!(feature = "llama-embedded") {
        assert_eq!(info.status, "running");
        assert_eq!(info.status_detail.as_deref(), Some("motore in-process"));
    } else {
        assert_ne!(info.status, "running");
    }
}

/// Guard for the feature wiring that `cargo test` cannot exercise without compiling llama.cpp:
/// every GPU feature of this crate must pull in `llama-embedded`, because the registration of the
/// in-process runtime sits behind that feature (see `Cargo.toml`). Verify the resolved graph with
/// `cargo tree -e features -p aiw-engine --features llama-metal -i aiw-engine`.
#[test]
fn gpu_features_imply_the_embedded_engine() {
    let manifest = include_str!("../Cargo.toml");
    for feature in ["llama-metal", "llama-cuda", "llama-vulkan"] {
        let line = manifest
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{feature} =")))
            .unwrap_or_else(|| panic!("feature {feature} is not declared"));
        assert!(line.contains("\"llama-embedded\""), "{feature} must imply llama-embedded, found: {line}");
        assert!(line.contains(&format!("aiw-runtimes/{feature}")), "{feature} must enable the runtime backend: {line}");
    }
}

#[tokio::test]
async fn phones_seed_a_profile_on_the_in_process_engine() {
    for os in ["android", "ios"] {
        let (e, _d, _w) = engine_for(Some(phone(os))).await;
        let p = e.profile("default").unwrap();
        // No sidecar exists there, so the default can only be the embedded engine; it is the one
        // the shell registers (feature `llama-embedded`, implied by `llama-metal` on iOS).
        assert_eq!(p.runtime_id, "llama-embedded", "{os}");
        assert_eq!(e.runtimes().into_iter().find(|r| r.id == "llama-server").unwrap().status, "unsupported", "{os}");
    }
}

#[tokio::test]
async fn the_profile_context_reaches_the_runtime() {
    let (e, _d, _w) = engine_for(None).await;
    e.register_native_runtime("llama-embedded", Arc::new(CtxProbe(8192)));
    let profile = InferenceProfile {
        id: "ctx".into(),
        name: "ctx".into(),
        description: None,
        runtime_id: "llama-embedded".into(),
        model: "m".into(),
        params: InferenceParams { context: Some(6144), ..Default::default() },
    };
    e.save_profile(profile.clone()).unwrap();
    // The agent runner's backend, workflow `complete`, and the OpenAI gateway all ask the registry
    // with the profile's context.
    let host = EngineHost::new(e.clone(), None, None);
    assert_eq!(host.backend(&profile).await.unwrap().list_models().await.unwrap(), vec!["ctx=6144"]);
    let (gw, model) = e.gateway_backend("ctx").await.unwrap();
    assert_eq!((gw.list_models().await.unwrap(), model.as_str()), (vec!["ctx=6144".to_string()], "m"));
    // A profile without a context leaves the engine's own setting alone.
    let mut plain = profile.clone();
    plain.params.context = None;
    assert_eq!(host.backend(&plain).await.unwrap().list_models().await.unwrap(), vec!["ctx=8192"]);
}
