//! Model roles at engine level: the agent setting (serde, validation), the plan a run gets, the
//! collapse to one model and the labels in the run events.

use super::tests::{engine, wait_finished};
use super::*;
use aiw_runtimes::ScriptedBackend;

fn profile(id: &str, runtime: &str, model: &str) -> InferenceProfile {
    InferenceProfile {
        id: id.into(),
        name: id.into(),
        description: None,
        runtime_id: runtime.into(),
        model: model.into(),
        params: InferenceParams { context: Some(8192), ..Default::default() },
    }
}

fn custom_events(e: &Engine, run: &str, name: &str) -> Vec<Value> {
    e.run_events(run)
        .iter()
        .filter_map(|s| match serde_json::to_value(&s.event.event).unwrap() {
            v if v["type"] == "CUSTOM" && v["name"] == name => Some(v["value"].clone()),
            _ => None,
        })
        .collect()
}

async fn with_roles(roles: Option<AgentRoles>) -> (Engine, ScriptedBackend, ScriptedBackend, tempfile::TempDir, tempfile::TempDir) {
    let (e, d, w) = engine(vec![]).await;
    let base = ScriptedBackend::new(vec![ScriptedBackend::text_turn("dal principale")]);
    let other = ScriptedBackend::new(vec![ScriptedBackend::text_turn("dal secondo")]);
    e.register_native_runtime("llama-embedded", Arc::new(base.clone()));
    e.register_native_runtime("ort-genai", Arc::new(other.clone()));
    e.save_profile(profile("grande", "ort-genai", "grande")).unwrap();
    e.save_profile(profile("stesso-motore", "llama-embedded", "altro")).unwrap();
    let mut a = e.agent("operator").unwrap();
    a.id = "ruoli".into();
    a.roles = roles;
    e.save_agent(a).unwrap();
    (e, base, other, d, w)
}

async fn run(e: &Engine) -> String {
    let s = e.create_session("ruoli", None).unwrap();
    let run = e.start_run(&s.id, "ciao".into()).await.unwrap();
    wait_finished(e, &run).await;
    run
}

#[test]
fn the_roles_setting_has_a_compatible_json_shape() {
    // Old JSON (no `roles`) stays valid and writes nothing back.
    let old = json!({"id": "a", "name": "A", "inference_profile": "default", "tools": ["*"], "mode": "ask", "sandbox": "read-only", "max_turns": 5});
    let a: AgentProfile = serde_json::from_value(old).unwrap();
    assert_eq!(a.roles, None);
    assert!(serde_json::to_value(&a).unwrap().get("roles").is_none());
    // "auto" and an object.
    let mut v = serde_json::to_value(&a).unwrap();
    v["roles"] = json!("auto");
    assert_eq!(serde_json::from_value::<AgentProfile>(v.clone()).unwrap().roles, Some(AgentRoles::Auto(AutoRoles::Auto)));
    v["roles"] = json!({"plan": "auto:reasoning", "summarize": "piccolo"});
    let b: AgentProfile = serde_json::from_value(v).unwrap();
    assert_eq!(
        b.roles,
        Some(AgentRoles::Custom(RoleProfiles {
            plan: Some("auto:reasoning".into()),
            summarize: Some("piccolo".into()),
            ..Default::default()
        }))
    );
    assert_eq!(serde_json::to_value(&b).unwrap()["roles"], json!({"plan": "auto:reasoning", "summarize": "piccolo"}));
    assert_eq!(serde_json::to_value(AgentRoles::Auto(AutoRoles::Auto)).unwrap(), json!("auto"));
}

#[tokio::test]
async fn a_role_must_name_an_existing_profile_or_an_auto_one() {
    let (e, ..) = with_roles(None).await;
    let mut a = e.agent("ruoli").unwrap();
    a.roles = Some(AgentRoles::Custom(RoleProfiles { plan: Some("non-esiste".into()), ..Default::default() }));
    let msg = match e.save_agent(a.clone()) {
        Err(Error::Invalid(m)) => m,
        other => panic!("{other:?}"),
    };
    assert!(msg.contains("non-esiste") && msg.contains("plan"), "{msg}");
    a.roles =
        Some(AgentRoles::Custom(RoleProfiles { plan: Some("auto:reasoning".into()), act: Some("grande".into()), ..Default::default() }));
    e.save_agent(a).unwrap();
}

#[tokio::test]
async fn an_agent_without_roles_runs_as_before() {
    let (e, base, other, _d, _w) = with_roles(None).await;
    let r = run(&e).await;
    assert_eq!(base.requests.lock().unwrap().len(), 1);
    assert_eq!(other.requests.lock().unwrap().len(), 0);
    for name in ["aiw.roles", "aiw.model", "aiw.check"] {
        assert!(custom_events(&e, &r, name).is_empty());
    }
}

#[tokio::test]
async fn the_plan_role_runs_on_its_own_runtime_and_the_events_say_so() {
    let roles = AgentRoles::Custom(RoleProfiles { plan: Some("grande".into()), ..Default::default() });
    let (e, base, other, _d, _w) = with_roles(Some(roles)).await;
    let r = run(&e).await;
    // The first call is the plan call: the other runtime answered, the agent's own one was not called.
    assert_eq!((other.requests.lock().unwrap().len(), base.requests.lock().unwrap().len()), (1, 0));
    let announced = custom_events(&e, &r, "aiw.roles");
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0]["plan"]["model"], "grande");
    assert_eq!(announced[0]["single"], false);
    let m = custom_events(&e, &r, "aiw.model");
    assert_eq!(
        (m[0]["role"].as_str(), m[0]["model"].as_str(), m[0]["runtime"].as_str()),
        (Some("plan"), Some("grande"), Some("ort-genai"))
    );
}

#[tokio::test]
async fn two_models_on_one_single_model_runtime_collapse_and_the_run_says_why() {
    let roles = AgentRoles::Custom(RoleProfiles { plan: Some("stesso-motore".into()), ..Default::default() });
    let (e, base, other, _d, _w) = with_roles(Some(roles)).await;
    let r = run(&e).await;
    assert_eq!((base.requests.lock().unwrap().len(), other.requests.lock().unwrap().len()), (1, 0));
    assert_eq!(base.requests.lock().unwrap()[0].model, "scripted", "the agent's own model, not the second one");
    let announced = &custom_events(&e, &r, "aiw.roles")[0];
    assert_eq!(announced["single"], true);
    assert!(announced["notes"][0].as_str().unwrap().contains("un solo modello"), "{announced}");
}

#[tokio::test]
async fn a_deleted_role_profile_does_not_stop_the_run() {
    let roles = AgentRoles::Custom(RoleProfiles { act: Some("grande".into()), ..Default::default() });
    let (e, base, _other, _d, _w) = with_roles(Some(roles)).await;
    e.0.store.delete_doc("profile", "grande").unwrap();
    let r = run(&e).await;
    assert_eq!(base.requests.lock().unwrap().len(), 1);
    assert!(custom_events(&e, &r, "aiw.roles")[0]["notes"][0].as_str().unwrap().contains("non c'è più"));
}

#[tokio::test]
async fn which_runtimes_hold_one_model() {
    let (e, ..) = engine(vec![]).await;
    let one = |id: &str| e.0.runtimes.holds_one_model(id);
    assert!(one("llama-server"), "its launch recipe names the model");
    assert!(one("llama-embedded"), "in-process engine");
    assert!(!one("ollama"), "loads models on demand");
    assert!(!one("remote-openai") && !one("aiw-peer"), "remote endpoints");
}
