//! Loop-level tests of the model roles: which backend gets which call, the `check` pass, and
//! that a run without roles is untouched.

use crate::roles::{RoleModel, RolePlan, CHECK_NAME};
use crate::runner::{AgentRunner, RunHost, RunInput};
use crate::tests_support::{input, setup, TestHost};
use crate::RunBudget;
use aiw_core::api::{InferenceParams, InferenceProfile};
use aiw_runtimes::ScriptedBackend;
use serde_json::json;
use std::sync::Arc;

fn prof(model: &str) -> InferenceProfile {
    InferenceProfile {
        id: model.into(),
        name: model.into(),
        description: None,
        runtime_id: "scripted".into(),
        model: model.into(),
        params: InferenceParams { context: Some(8192), ..Default::default() },
    }
}

fn role(model: &str) -> Option<RoleModel> {
    Some(RoleModel { profile: prof(model), can_reason: true })
}

fn plan(check: bool) -> RolePlan {
    RolePlan {
        plan: role("big"),
        act: role("fast"),
        check: check.then(|| RoleModel { profile: prof("rev"), can_reason: true }),
        summarize: None,
        base_can_reason: true,
        check_enabled: check,
        mode: "custom".into(),
        notes: vec![],
    }
}

fn give(host: &TestHost, model: &str, turns: Vec<Vec<aiw_runtimes::ChatDelta>>) -> ScriptedBackend {
    let b = ScriptedBackend::new(turns);
    host.by_model.lock().unwrap().insert(model.into(), b.clone());
    b
}

fn write_call() -> Vec<aiw_runtimes::ChatDelta> {
    ScriptedBackend::tool_turn("w1", "write_file", json!({"path": "out.txt", "content": "ciao"}))
}

fn custom_events(host: &TestHost, name: &str) -> Vec<serde_json::Value> {
    host.events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match serde_json::to_value(e).unwrap() {
            v if v["type"] == "CUSTOM" && v["name"] == name => Some(v["value"].clone()),
            _ => None,
        })
        .collect()
}

fn with_roles(dir: &std::path::Path, plan: RolePlan) -> RunInput {
    let mut i = input(dir, "auto");
    i.roles = Some(plan);
    i
}

#[tokio::test]
async fn plan_gets_the_first_call_and_act_the_calls_after_successful_tools() {
    let (host, dir) = setup(vec![], true);
    let big = give(&host, "big", vec![ScriptedBackend::tool_turn("c1", "list_dir", json!({"path": "."}))]);
    let fast = give(&host, "fast", vec![ScriptedBackend::text_turn("Fatto.")]);
    let out = AgentRunner::run(with_roles(dir.path(), plan(false)), host.clone()).await.unwrap();
    assert_eq!(out.final_text, "Fatto.");
    let (b, f) = (big.requests.lock().unwrap(), fast.requests.lock().unwrap());
    assert_eq!((b.len(), f.len()), (1, 1));
    assert_eq!((b[0].model.as_str(), f[0].model.as_str()), ("big", "fast"));
    // Plan thinks, act does not.
    assert!(b[0].thinking.unwrap().enabled);
    assert!(!f[0].thinking.unwrap().enabled);
    // The history is shared: the act model sees the plan model's tool call and its result.
    assert!(f[0].messages.iter().any(|m| m.role == "tool") && f[0].messages.iter().any(|m| m.tool_calls.is_some()));
    // The inspector knows who handled each call.
    let models = custom_events(&host, "aiw.model");
    let seen: Vec<(&str, &str)> = models.iter().map(|v| (v["role"].as_str().unwrap(), v["model"].as_str().unwrap())).collect();
    assert_eq!(seen, vec![("plan", "big"), ("act", "fast")]);
    assert_eq!(custom_events(&host, "aiw.roles").len(), 1);
}

#[tokio::test]
async fn the_call_after_a_failed_tool_goes_back_to_the_plan_model() {
    let (host, dir) = setup(vec![], true);
    let big = give(
        &host,
        "big",
        vec![ScriptedBackend::tool_turn("c1", "read_file", json!({"path": "manca.txt"})), ScriptedBackend::text_turn("Fatto.")],
    );
    let fast = give(&host, "fast", vec![]);
    AgentRunner::run(with_roles(dir.path(), plan(false)), host.clone()).await.unwrap();
    // call 1 (first) and call 2 (after the failed read) are plan calls; the self-check note after the final text is a failure too.
    assert!(big.requests.lock().unwrap().len() >= 2);
    assert_eq!(fast.requests.lock().unwrap().len(), 0);
}

#[tokio::test]
async fn check_runs_once_after_acting_tools_and_a_failure_continues_the_run() {
    let (host, dir) = setup(vec![], true);
    let big = give(&host, "big", vec![write_call(), ScriptedBackend::text_turn("Corretto.")]);
    let fast = give(&host, "fast", vec![ScriptedBackend::text_turn("Fatto.")]);
    let rev = give(
        &host,
        "rev",
        vec![ScriptedBackend::text_turn("FAIL: il file non contiene il testo richiesto"), ScriptedBackend::text_turn("FAIL: ancora no")],
    );
    let mut i = with_roles(dir.path(), plan(true));
    i.budget = Arc::new(RunBudget::unlimited());
    let budget = i.budget.clone();
    let out = AgentRunner::run(i, host.clone()).await.unwrap();
    // One review only, however many times the run finishes; the correction came from the plan model.
    assert_eq!(rev.requests.lock().unwrap().len(), 1);
    assert_eq!(out.final_text, "Corretto.");
    assert_eq!((big.requests.lock().unwrap().len(), fast.requests.lock().unwrap().len()), (2, 1));
    let note = out.new_messages.iter().find(|m| m.name.as_deref() == Some(CHECK_NAME)).expect("feedback stored");
    assert!(note.content.contains("il file non contiene il testo richiesto"));
    let checks = custom_events(&host, "aiw.check");
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0]["passed"], false);
    // The review is a model call like any other: plan, act, check, plan.
    assert_eq!(budget.used_turns(), 4);
    // The reviewer sees the request and the action, not the whole conversation.
    let r = &rev.requests.lock().unwrap()[0];
    assert_eq!(r.messages.len(), 2);
    assert!(r.messages[1].text().contains("write_file"));
}

#[tokio::test]
async fn a_passing_check_ends_the_run_and_read_only_runs_are_not_checked() {
    // acted + PASS
    let (host, dir) = setup(vec![], true);
    give(&host, "big", vec![write_call()]);
    give(&host, "fast", vec![ScriptedBackend::text_turn("Fatto.")]);
    let rev = give(&host, "rev", vec![ScriptedBackend::text_turn("PASS: il file c'è")]);
    let out = AgentRunner::run(with_roles(dir.path(), plan(true)), host.clone()).await.unwrap();
    assert_eq!(out.final_text, "Fatto.");
    assert_eq!(rev.requests.lock().unwrap().len(), 1);
    assert_eq!(custom_events(&host, "aiw.check")[0]["passed"], true);

    // only reads: no check
    let (host, dir) = setup(vec![], true);
    give(&host, "big", vec![ScriptedBackend::tool_turn("c1", "list_dir", json!({"path": "."}))]);
    give(&host, "fast", vec![ScriptedBackend::text_turn("Vuota.")]);
    let rev = give(&host, "rev", vec![]);
    AgentRunner::run(with_roles(dir.path(), plan(true)), host.clone()).await.unwrap();
    assert_eq!(rev.requests.lock().unwrap().len(), 0);
    assert!(custom_events(&host, "aiw.check").is_empty());
}

#[tokio::test]
async fn a_failed_acting_tool_does_not_trigger_the_check_and_a_garbled_verdict_passes() {
    let (host, dir) = setup(vec![], true);
    give(&host, "big", vec![write_call()]);
    give(&host, "fast", vec![ScriptedBackend::text_turn("Fatto.")]);
    let rev = give(&host, "rev", vec![ScriptedBackend::text_turn("boh, forse")]);
    let out = AgentRunner::run(with_roles(dir.path(), plan(true)), host.clone()).await.unwrap();
    assert_eq!(out.final_text, "Fatto.");
    assert_eq!(rev.requests.lock().unwrap().len(), 1);

    // the write is refused (plan mode): nothing acted, nothing to check
    let (host, dir) = setup(vec![], false);
    give(&host, "big", vec![write_call()]);
    give(&host, "fast", vec![ScriptedBackend::text_turn("Non ho potuto.")]);
    let rev = give(&host, "rev", vec![]);
    let mut i = with_roles(dir.path(), plan(true));
    i.policy.mode = "ask".into();
    AgentRunner::run(i, host.clone()).await.unwrap();
    assert_eq!(rev.requests.lock().unwrap().len(), 0);
}

#[tokio::test]
async fn a_role_that_cannot_start_falls_back_to_the_agents_own_model() {
    let (host, dir) = setup(vec![ScriptedBackend::text_turn("Ciao.")], true);
    host.unavailable.lock().unwrap().insert("big".into());
    let out = AgentRunner::run(with_roles(dir.path(), plan(false)), host.clone()).await.unwrap();
    assert_eq!(out.final_text, "Ciao.");
    // the default backend of the host (the agent's own) answered
    assert_eq!(host.backend.requests.lock().unwrap().len(), 1);
    assert_eq!(host.backend.requests.lock().unwrap()[0].model, "scripted");
}

#[tokio::test]
async fn compaction_uses_the_summarize_model() {
    let (host, dir) = setup(vec![], true);
    let tiny = give(&host, "tiny", vec![ScriptedBackend::text_turn("Riassunto breve.")]);
    give(&host, "big", vec![ScriptedBackend::text_turn("Ok.")]);
    let mut p = plan(false);
    p.summarize = role("tiny");
    let mut i = with_roles(dir.path(), p);
    i.compact_at = 0.0001;
    i.history = (0..8).map(|n| aiw_runtimes::Msg::user(format!("messaggio numero {n} con un po' di testo per fare volume"))).collect();
    AgentRunner::run(i, host.clone()).await.unwrap();
    let t = tiny.requests.lock().unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].model, "tiny");
    assert!(t[0].messages[0].text().contains("Riassumi"));
}

#[tokio::test]
async fn without_roles_nothing_changes() {
    let (host, dir) = setup(vec![write_call_turn(), ScriptedBackend::text_turn("Fatto.")], true);
    let out = AgentRunner::run(input(dir.path(), "auto"), host.clone()).await.unwrap();
    assert_eq!(out.final_text, "Fatto.");
    let reqs = host.backend.requests.lock().unwrap();
    // exactly the two calls of the loop, no review, the agent's own model, no thinking flag
    assert_eq!(reqs.len(), 2);
    assert!(reqs.iter().all(|r| r.model == "scripted" && r.thinking.is_none()));
    for name in ["aiw.model", "aiw.roles", "aiw.check"] {
        assert!(custom_events(&host, name).is_empty(), "{name} must not appear without roles");
    }
    assert!(out.new_messages.iter().all(|m| m.name.as_deref() != Some(CHECK_NAME)));
}

fn write_call_turn() -> Vec<aiw_runtimes::ChatDelta> {
    write_call()
}

#[allow(dead_code)]
fn _assert_host_is_run_host(h: &TestHost) -> &dyn RunHost {
    h
}
