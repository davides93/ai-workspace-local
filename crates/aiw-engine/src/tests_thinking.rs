//! Reasoning of a run: the override of the run over the profile's, and nothing for a model that cannot reason.

use super::tests::{engine, wait_finished};
use super::*;
use aiw_runtimes::ScriptedBackend;

/// An engine whose scripted backend we can inspect; the model `scripted` can reason when `reasoning` is true.
async fn reasoning_engine(
    reasoning: bool,
    profile_mode: Option<ThinkingMode>,
) -> (Engine, ScriptedBackend, tempfile::TempDir, tempfile::TempDir) {
    let (e, d, w) = engine(vec![]).await;
    let backend = ScriptedBackend::new(vec![ScriptedBackend::text_turn("ok"), ScriptedBackend::text_turn("ok")]);
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    let caps = if reasoning { vec!["chat".to_string(), "reasoning".to_string()] } else { vec!["chat".to_string()] };
    *e.0.models.write().unwrap() =
        vec![LocalModel { id: "scripted".into(), name: "Scripted".into(), capabilities: caps, ..Default::default() }];
    let mut p = e.profile("default").unwrap();
    p.params.thinking = profile_mode;
    e.save_profile(p).unwrap();
    (e, backend, d, w)
}

async fn first_request_thinking(e: &Engine, backend: &ScriptedBackend, over: Option<ThinkingMode>) -> Option<(bool, bool, Option<u32>)> {
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run_opts(&s.id, "ciao".into(), None, over).await.unwrap();
    wait_finished(e, &run).await;
    let reqs = backend.requests.lock().unwrap();
    reqs[0].thinking.map(|t| (t.enabled, t.deep, t.budget))
}

#[tokio::test]
async fn the_profile_decides_when_the_run_has_no_override() {
    let (e, b, _d, _w) = reasoning_engine(true, Some(ThinkingMode::Auto)).await;
    assert_eq!(first_request_thinking(&e, &b, None).await, Some((true, false, Some(aiw_agent::thinking::AUTO_BUDGET))));
}

#[tokio::test]
async fn the_override_of_the_run_wins_over_the_profile() {
    let (e, b, _d, _w) = reasoning_engine(true, Some(ThinkingMode::Deep)).await;
    assert_eq!(first_request_thinking(&e, &b, Some(ThinkingMode::Off)).await, Some((false, false, None)));
}

#[tokio::test]
async fn an_override_works_on_a_profile_with_no_mode() {
    let (e, b, _d, _w) = reasoning_engine(true, None).await;
    assert_eq!(first_request_thinking(&e, &b, Some(ThinkingMode::Deep)).await, Some((true, true, Some(aiw_agent::thinking::DEEP_BUDGET))));
}

#[tokio::test]
async fn no_mode_anywhere_sends_nothing() {
    let (e, b, _d, _w) = reasoning_engine(true, None).await;
    assert_eq!(first_request_thinking(&e, &b, None).await, None);
}

#[tokio::test]
async fn a_model_without_the_reasoning_capability_gets_nothing_even_with_an_override() {
    let (e, b, _d, _w) = reasoning_engine(false, Some(ThinkingMode::Deep)).await;
    assert_eq!(first_request_thinking(&e, &b, Some(ThinkingMode::Deep)).await, None);
}
