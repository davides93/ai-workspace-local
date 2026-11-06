//! Prompt optimization over HTTP: started in the background, polled, applied by the user.

use super::*;
use aiw_runtimes::ScriptedBackend;
use std::sync::Arc;

const SUITE: &str =
    "id: api\nname: Suite API\ntasks:\n  - id: t\n    prompt: crea note.md\n    graders: [{ kind: file_exists, path: note.md }]\n";

#[tokio::test]
async fn an_optimization_runs_in_the_background_and_is_applied_only_on_request() {
    let (app, e, _d, _w) = super::tests::app().await;
    e.register_native_runtime(
        "llama-embedded",
        Arc::new(ScriptedBackend::new(vec![
            ScriptedBackend::text_turn("Niente"),
            ScriptedBackend::text_turn(
                &json!({"variants": [{"prompt": "Crea sempre i file richiesti.", "rationale": "più esplicito"}]}).to_string(),
            ),
            ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "note.md", "content": "ciao"})),
            ScriptedBackend::text_turn("Fatto"),
        ])),
    );
    dispatch(app.clone(), "PUT", "/api/evals/api", Some(json!({"text": SUITE}))).await;

    // Refused: unknown suite or agent, a bad number of variants, no agent at all.
    let go = |body: Value| dispatch(app.clone(), "POST", "/api/evals/api/optimize", Some(body));
    assert_eq!(dispatch(app.clone(), "POST", "/api/evals/none/optimize", Some(json!({"agent": "operator"}))).await.0, 404);
    assert_eq!(go(json!({"agent": "fantasma"})).await.0, 404);
    assert_eq!(go(json!({"agent": "operator", "variants": 9})).await.0, 400);
    assert_eq!(go(json!({})).await.0, 400);

    let (st, started) = go(json!({"agent": "operator", "variants": 1})).await;
    assert_eq!(st, 200, "{started}");
    assert_eq!(
        (started["status"].as_str(), started["suite_id"].as_str(), started["agent_id"].as_str()),
        (Some("running"), Some("api"), Some("operator"))
    );
    let id = started["id"].as_str().unwrap().to_string();
    let mut last = Value::Null;
    for _ in 0..300 {
        let (st, r) = dispatch(app.clone(), "GET", &format!("/api/prompt-optimizations/{id}"), None).await;
        assert_eq!(st, 200);
        last = r;
        if last["status"] != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(last["status"], "done", "{last}");
    assert_eq!((last["best"].as_u64(), last["variants"][0]["pass_rate"].as_f64()), (Some(0), Some(1.0)));
    assert!(last["variants"][0]["diff"].as_str().unwrap().contains("+Crea sempre i file richiesti."));
    let (_, list) = dispatch(app.clone(), "GET", "/api/prompt-optimizations", None).await;
    assert_eq!(list[0]["id"], id.as_str());
    assert_eq!(dispatch(app.clone(), "GET", "/api/prompt-optimizations/opt_none", None).await.0, 404);

    // Nothing changed in the agent until the user applies it.
    let prompt_of = |agents: &Value| agents.as_array().unwrap().iter().find(|a| a["id"] == "operator").unwrap()["system_prompt"].clone();
    let (_, before) = dispatch(app.clone(), "GET", "/api/agents", None).await;
    assert_ne!(prompt_of(&before), json!("Crea sempre i file richiesti."));
    let apply = format!("/api/prompt-optimizations/{id}/apply");
    assert_eq!(dispatch(app.clone(), "POST", &apply, Some(json!({"variant": 4}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "POST", &apply, Some(json!({}))).await.0, 400);
    let (st, applied) = dispatch(app.clone(), "POST", &apply, Some(json!({"variant": 0}))).await;
    assert_eq!((st, applied["status"].as_str(), applied["applied"].as_u64()), (200, Some("applied"), Some(0)));
    let (_, after) = dispatch(app.clone(), "GET", "/api/agents", None).await;
    assert_eq!(prompt_of(&after), json!("Crea sempre i file richiesti."));
    assert_eq!(dispatch(app.clone(), "POST", &apply, Some(json!({"variant": 0}))).await.0, 409);
}
