//! `auto:<task>` inference profiles: resolved by the model adviser when a run starts.

use super::tests::wait_finished;
use super::*;
use aiw_runtimes::ScriptedBackend;

// Model ranking must not depend on the runner's RAM or GPU memory.
async fn engine(turns: Vec<Vec<aiw_runtimes::ChatDelta>>) -> (Engine, tempfile::TempDir, tempfile::TempDir) {
    let (e, data, ws) = super::tests::engine(turns).await;
    let mut device = e.device();
    device.memory_mb = 64 * 1024;
    device.available_memory_mb = 48 * 1024;
    device.accelerators.retain(|a| a.kind == "cpu");
    e.set_device(device);
    (e, data, ws)
}

fn gguf(id: &str, file: &str, size_gb: f64, caps: &[&str]) -> LocalModel {
    serde_json::from_value(json!({
        "id": id, "name": id, "path": format!("/models/{file}"), "format": "gguf",
        "size_bytes": (size_gb * 1024.0 * 1024.0 * 1024.0) as u64, "context_length": 40960,
        "capabilities": caps, "runtimes": ["llama-embedded"], "params_b": 4.0,
    }))
    .unwrap()
}

#[tokio::test]
async fn a_fresh_install_seeds_agents_on_the_adviser_and_keeps_the_default_profile() {
    let (e, _d, _w) = engine(vec![]).await;
    let by = |id: &str| e.agent(id).unwrap().inference_profile;
    assert_eq!(by("operator"), "auto:agent");
    assert_eq!(by("coder"), "auto:agent");
    assert_eq!(by("planner"), "auto:reasoning");
    // The stored `default` profile is still there and listed; `auto:*` is not a stored profile.
    assert!(e.profiles().unwrap().iter().any(|p| p.id == "default"));
    assert!(!e.profiles().unwrap().iter().any(|p| p.id.starts_with("auto:")));
}

#[tokio::test]
async fn auto_resolves_to_the_installed_model_and_follows_it() {
    let (e, _d, _w) = engine(vec![]).await;
    // Nothing the adviser can use: the default profile (a model picked by hand) keeps working.
    assert_eq!(e.profile("auto:agent").unwrap().id, "default");
    *e.0.models.write().unwrap() = vec![gguf("qwen3-4b-q4_k_m", "Qwen3-4B-Q4_K_M.gguf", 2.5, &["chat", "tools", "reasoning"])];
    let p = e.profile("auto:agent").unwrap();
    assert_eq!((p.id.as_str(), p.model.as_str(), p.runtime_id.as_str()), ("auto:agent", "qwen3-4b-q4_k_m", "llama-embedded"));
    assert!(p.params.context.unwrap() >= 2048);
    assert_eq!(e.profile("auto:reasoning").unwrap().params.thinking, Some(ThinkingMode::Deep));
    // A better model appears: the same id now points at it, no profile was edited.
    *e.0.models.write().unwrap() = vec![
        gguf("qwen3-4b-q4_k_m", "Qwen3-4B-Q4_K_M.gguf", 2.5, &["chat", "tools", "reasoning"]),
        gguf("qwen3-8b-q4_k_m", "Qwen3-8B-Q4_K_M.gguf", 5.0, &["chat", "tools", "reasoning"]),
    ];
    assert_eq!(e.profile("auto:agent").unwrap().model, "qwen3-8b-q4_k_m");
    // The recommendation rescans the disk (nothing installed in a test): downloads only.
    let rec = e.recommend();
    assert!(!rec.tier.is_empty() && rec.agent.installed.is_none() && rec.agent.download.is_some());
}

#[tokio::test]
async fn nothing_installed_is_a_clear_conflict_and_a_run_does_not_start() {
    let (e, _d, _w) = engine(vec![]).await;
    e.delete_profile("default").unwrap();
    let err = e.profile("auto:agent").unwrap_err();
    assert!(matches!(err, Error::Conflict(ref m) if m.contains("scaricane uno da Modelli")), "{err}");
    assert_eq!(err.status(), 409);
    // Vision has no fallback: an agent that needs images says so.
    assert!(matches!(e.profile("auto:vision"), Err(Error::Conflict(ref m)) if m.contains("immagini")));
    let s = e.create_session("operator", None).unwrap();
    let err = e.start_run(&s.id, "ciao".into()).await.unwrap_err();
    assert_eq!(err.status(), 409, "{err}");
    assert!(err.to_string().contains("Modelli"));
}

#[tokio::test]
async fn auto_profiles_are_reserved_and_cannot_be_stored_or_deleted() {
    let (e, _d, _w) = engine(vec![]).await;
    let mut p = e.profile("default").unwrap();
    p.id = "auto:agent".into();
    assert!(matches!(e.save_profile(p), Err(Error::Invalid(_))));
    assert!(matches!(e.delete_profile("auto:agent"), Err(Error::Invalid(_))));
    // An agent may name one before anything is installed; a made-up task is refused.
    let mut a = e.agent("operator").unwrap();
    a.inference_profile = "auto:chat".into();
    e.save_agent(a.clone()).unwrap();
    a.inference_profile = "auto:nope".into();
    assert!(matches!(e.save_agent(a), Err(Error::Invalid(_))));
}

#[tokio::test]
async fn a_run_on_an_auto_agent_works_end_to_end() {
    let (e, _d, _w) = engine(vec![ScriptedBackend::text_turn("fatto")]).await;
    let s = e.create_session("planner", None).unwrap();
    let run = e.start_run(&s.id, "ciao".into()).await.unwrap();
    wait_finished(&e, &run).await;
    let msgs = e.messages(&s.id).unwrap();
    assert_eq!(msgs.last().map(|m| m.content.as_str()), Some("fatto"));
}

#[tokio::test]
async fn the_gateway_accepts_an_auto_model_name() {
    let (e, _d, _w) = engine(vec![]).await;
    *e.0.models.write().unwrap() = vec![gguf("qwen3-4b-q4_k_m", "Qwen3-4B-Q4_K_M.gguf", 2.5, &["chat", "tools"])];
    let (_backend, model) = e.gateway_backend("auto:chat").await.unwrap();
    assert_eq!(model, "qwen3-4b-q4_k_m");
}
