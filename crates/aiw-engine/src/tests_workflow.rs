//! Durable workflows: state saved at every step and resumed after a restart.

use super::*;
use aiw_runtimes::ScriptedBackend;
use serde_json::json;

fn options(data: &std::path::Path, ws: &std::path::Path) -> EngineOptions {
    EngineOptions {
        data_dir: Some(data.to_path_buf()),
        workspace_root: Some(ws.to_path_buf()),
        mode: "embedded".into(),
        offline: true,
        ..Default::default()
    }
}

async fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    for _ in 0..300 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timeout: {what}");
}

fn approval_workflow() -> WorkflowDef {
    serde_json::from_value(json!({
        "id": "durable", "name": "Durable", "enabled": false, "trigger": {"kind": "manual"}, "inputs": {},
        "steps": [
            {"id": "scan", "kind": "tool", "tool": "list_dir", "args": {"path": "."}},
            {"id": "ok", "kind": "approval", "message": "Procedo?"},
            {"id": "after", "kind": "tool", "tool": "glob", "args": {"pattern": "*.txt"}}
        ]
    }))
    .unwrap()
}

#[tokio::test]
async fn every_step_is_saved_and_a_waiting_approval_survives_a_restart() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.txt"), "x").unwrap();

    // First process: the workflow stops at the approval.
    let e1 = Engine::new(options(data.path(), ws.path())).await.unwrap();
    e1.save_workflow(approval_workflow()).unwrap();
    let first = e1.run_workflow("durable", Map::new(), "test").await.unwrap();
    wait_until("approval requested", || !e1.approvals().is_empty()).await;
    let saved = e1.0.store.workflow_states().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].run.id, first.id);
    assert_eq!(saved[0].next, 1, "the approval is the step to run again");
    assert_eq!(e1.0.store.workflow_run(&first.id).unwrap().unwrap().status, "waiting_approval", "the run is saved at every change");

    // "Restart": a second engine on the same data folder resumes the run and asks again.
    let e2 = Engine::new(options(data.path(), ws.path())).await.unwrap();
    wait_until("approval asked again", || !e2.approvals().is_empty()).await;
    assert!(e2.audit().iter().any(|a| a.action == "workflow.resume" && a.run_id.as_deref() == Some(first.id.as_str())));
    let req = e2.approvals().remove(0);
    assert_eq!(req.run_id, first.id);
    e2.respond_approval(&req.id, true, false).unwrap();
    wait_until("run succeeded", || e2.workflow_runs().iter().any(|r| r.id == first.id && r.status == "succeeded")).await;
    let run = e2.workflow_runs().into_iter().find(|r| r.id == first.id).unwrap();
    assert!(run.steps.iter().all(|s| s.status == "done"), "{run:?}");
    assert!(run.steps[2].output.as_deref().unwrap_or_default().contains("a.txt"));
    assert!(e2.0.store.workflow_states().unwrap().is_empty(), "the state goes away with the run");
}

#[tokio::test]
async fn a_run_of_a_deleted_workflow_is_marked_failed_and_a_finished_one_is_left_alone() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e1 = Engine::new(options(data.path(), ws.path())).await.unwrap();
    e1.save_workflow(approval_workflow()).unwrap();
    let first = e1.run_workflow("durable", Map::new(), "test").await.unwrap();
    wait_until("approval requested", || !e1.approvals().is_empty()).await;
    // A stale state of a run that already finished.
    let mut done = e1.0.store.workflow_states().unwrap().remove(0);
    done.run.id = "wfr_done".into();
    done.run.status = "running".into();
    let mut finished = done.run.clone();
    finished.status = "succeeded".into();
    e1.0.store.save_workflow_run(&finished).unwrap();
    e1.0.store.save_workflow_state(&done).unwrap();
    e1.delete_workflow("durable").unwrap();

    let e2 = Engine::new(options(data.path(), ws.path())).await.unwrap();
    let run = e2.workflow_runs().into_iter().find(|r| r.id == first.id).unwrap();
    assert_eq!(run.status, "failed");
    assert!(run.steps[1].error.as_deref().unwrap().contains("non esiste più"), "{run:?}");
    assert!(e2.0.store.workflow_states().unwrap().is_empty());
    assert!(e2.approvals().is_empty());
}

#[tokio::test]
async fn approval_step_takes_its_default_when_time_runs_out() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(options(data.path(), ws.path())).await.unwrap();
    e.save_workflow(
        serde_json::from_value(json!({
            "id": "late", "name": "Late", "enabled": false, "trigger": {"kind": "manual"}, "inputs": {},
            "steps": [
                {"id": "ok", "kind": "approval", "message": "Procedo?", "timeout_s": 1, "default": "allow"},
                {"id": "after", "kind": "tool", "tool": "list_dir", "args": {"path": "."}}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let first = e.run_workflow("late", Map::new(), "test").await.unwrap();
    wait_until("run succeeded", || e.workflow_runs().iter().any(|r| r.id == first.id && r.status == "succeeded")).await;
    let run = e.workflow_runs().into_iter().find(|r| r.id == first.id).unwrap();
    assert_eq!(run.steps[0].output.as_deref(), Some("scaduto: approvato"));
    assert!(e.approvals().is_empty(), "the expired request is withdrawn");
    assert!(e
        .audit()
        .iter()
        .any(|a| a.action == "approval" && a.decision.as_deref() == Some("timeout") && a.run_id.as_deref() == Some(first.id.as_str())));

    // Default deny: the run is cancelled.
    let mut wf = e.workflows().unwrap().into_iter().find(|w| w.id == "late").unwrap();
    if let WorkflowStep::Approval { on_timeout, .. } = &mut wf.steps[0] {
        *on_timeout = ApprovalDefault::Deny;
    }
    e.save_workflow(wf).unwrap();
    let second = e.run_workflow("late", Map::new(), "test").await.unwrap();
    wait_until("run cancelled", || e.workflow_runs().iter().any(|r| r.id == second.id && r.status == "cancelled")).await;
    let run = e.workflow_runs().into_iter().find(|r| r.id == second.id).unwrap();
    assert_eq!(run.steps[0].error.as_deref(), Some("scaduto: rifiutato"));
}

#[tokio::test]
async fn tool_approvals_expire_with_the_setting() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(options(data.path(), ws.path())).await.unwrap();
    // Off by default: the request stays pending.
    assert_eq!(e.approval_timeout(), None);
    e.set_config("global", "approvals.timeout_s", json!(1)).unwrap();
    assert_eq!(e.approval_timeout(), Some(Duration::from_secs(1)));
    let err = e.execute_tool("run_t", "workflow", "Test", "write_file", json!({"path": "nota.md", "content": "x"})).await.unwrap_err();
    assert!(err.to_string().contains("tempo limite"), "{err}");
    assert!(!ws.path().join("nota.md").exists());
    assert!(e.approvals().is_empty());
    assert!(e.audit().iter().any(|a| a.decision.as_deref() == Some("timeout")));
}

#[tokio::test]
async fn an_agent_whose_approval_expires_gets_an_explanation() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(options(data.path(), ws.path())).await.unwrap();
    let turns =
        vec![ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "nota.md", "content": "x"})), ScriptedBackend::text_turn("Ok")];
    e.register_native_runtime("llama-embedded", Arc::new(ScriptedBackend::new(turns)));
    e.save_profile(InferenceProfile {
        id: "default".into(),
        name: "Test".into(),
        description: None,
        runtime_id: "llama-embedded".into(),
        model: "scripted".into(),
        params: InferenceParams { context: Some(8192), ..Default::default() },
    })
    .unwrap();
    e.set_config("global", "approvals.timeout_s", json!(1)).unwrap();
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "Crea una nota".into()).await.unwrap();
    wait_until("run done", || e.run_done(&run)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let tool_msg = e.messages(&s.id).unwrap().into_iter().find(|m| m.role == "tool").expect("tool result");
    assert!(tool_msg.content.contains("tempo limite") && tool_msg.content.contains("approvals.timeout_s"), "{}", tool_msg.content);
    assert!(!ws.path().join("nota.md").exists());
    assert!(e.approvals().is_empty());
    assert!(e.audit().iter().any(|a| a.decision.as_deref() == Some("timeout")));
}
