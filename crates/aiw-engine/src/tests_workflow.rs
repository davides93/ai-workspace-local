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

#[tokio::test]
async fn agents_running_together_in_a_workflow_get_their_own_sessions() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(options(data.path(), ws.path())).await.unwrap();
    e.register_native_runtime(
        "llama-embedded",
        Arc::new(ScriptedBackend::new(vec![ScriptedBackend::text_turn("risposta"), ScriptedBackend::text_turn("risposta")])),
    );
    e.save_profile(InferenceProfile {
        id: "default".into(),
        name: "Test".into(),
        description: None,
        runtime_id: "llama-embedded".into(),
        model: "scripted".into(),
        params: InferenceParams { context: Some(8192), ..Default::default() },
    })
    .unwrap();
    e.save_workflow(
        serde_json::from_value(json!({
            "id": "par", "name": "Par", "enabled": false, "trigger": {"kind": "manual"}, "inputs": {},
            "steps": [
                {"id": "both", "kind": "parallel", "steps": [
                    {"id": "a", "kind": "agent", "agent": "operator", "prompt": "primo"},
                    {"id": "b", "kind": "agent", "agent": "operator", "prompt": "secondo"}
                ]}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let first = e.run_workflow("par", Map::new(), "test").await.unwrap();
    wait_until("run finished", || e.workflow_runs().iter().any(|r| r.id == first.id && r.status != "running")).await;
    let run = e.workflow_runs().into_iter().find(|r| r.id == first.id).unwrap();
    assert_eq!(run.status, "succeeded", "{run:?}");
    assert_eq!(run.steps[0].output.as_deref(), Some(r#"{"a":"risposta","b":"risposta"}"#));
    assert!(e.sessions().unwrap().iter().filter(|s| s.title.starts_with("⚙ Par")).count() >= 2);
}

/// An engine whose scripted model answers `turns` in order, with two copies of an agent for a team.
async fn engine_with_team_members(turns: Vec<Vec<aiw_runtimes::ChatDelta>>) -> (Engine, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(options(data.path(), ws.path())).await.unwrap();
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
    for id in ["primo", "secondo"] {
        let mut a = e.agent("planner").unwrap();
        a.id = id.into();
        a.name = id.into();
        e.save_agent(a).unwrap();
    }
    e.save_team(TeamDef {
        id: "squadra".into(),
        name: "Squadra".into(),
        description: "Un team di prova".into(),
        strategy: "pipeline".into(),
        supervisor: None,
        members: ["primo", "secondo"].iter().map(|a| TeamMember { agent: a.to_string(), role: format!("ruolo di {a}") }).collect(),
        aggregator: None,
        budget: None,
        output_schema: None,
    })
    .unwrap();
    (e, data, ws)
}

#[tokio::test]
async fn a_team_step_runs_the_team_in_its_own_session_and_its_answer_is_the_output() {
    let (e, _d, _w) = engine_with_team_members(vec![ScriptedBackend::text_turn("uno"), ScriptedBackend::text_turn("due")]).await;
    e.save_workflow(serde_json::from_value(json!({
        "id": "notturno", "name": "Notturno", "enabled": false, "trigger": {"kind": "cron", "expr": "0 3 * * *"}, "inputs": {"tema": "AI"},
        "steps": [{"id": "squad", "kind": "team", "team": "squadra", "prompt": "Studia {{inputs.tema}}", "retry": 1}]
    }))
    .unwrap())
    .unwrap();
    let first = e.run_workflow("notturno", Map::new(), "test").await.unwrap();
    wait_until("run finished", || e.workflow_runs().iter().any(|r| r.id == first.id && r.status != "running")).await;
    let run = e.workflow_runs().into_iter().find(|r| r.id == first.id).unwrap();
    assert_eq!(run.status, "succeeded", "{run:?}");
    assert_eq!(run.steps[0].output.as_deref(), Some("due"), "the answer of the last member of the pipeline");
    let session = e.sessions().unwrap().into_iter().find(|s| s.title.starts_with("⚙ Notturno")).expect("the team session");
    assert_eq!(session.agent_id, "team:squadra");
    assert_eq!(e.messages(&session.id).unwrap().first().unwrap().content, "Studia AI");
}

#[tokio::test]
async fn a_team_step_of_a_missing_team_fails_the_run_and_a_used_team_cannot_be_deleted() {
    let (e, _d, _w) = engine_with_team_members(vec![]).await;
    let wf = |id: &str, team: &str| {
        serde_json::from_value::<WorkflowDef>(json!({
            "id": id, "name": id, "enabled": false, "trigger": {"kind": "manual"}, "inputs": {},
            "steps": [{"id": "squad", "kind": "parallel", "steps": [{"id": "in", "kind": "team", "team": team, "prompt": "x"}]}]
        }))
        .unwrap()
    };
    e.save_workflow(wf("ghost", "nessuno")).unwrap();
    let first = e.run_workflow("ghost", Map::new(), "test").await.unwrap();
    wait_until("run finished", || e.workflow_runs().iter().any(|r| r.id == first.id && r.status != "running")).await;
    let run = e.workflow_runs().into_iter().find(|r| r.id == first.id).unwrap();
    assert_eq!(run.status, "failed", "{run:?}");

    // A team named by a step, even an inline one, cannot be deleted from under the workflow.
    e.save_workflow(wf("usa", "squadra")).unwrap();
    let err = e.delete_team("squadra").unwrap_err();
    assert!(matches!(err, Error::Conflict(_)) && err.to_string().contains("usa"), "{err}");
    e.delete_workflow("usa").unwrap();
    e.delete_team("squadra").unwrap();
}

fn simple_workflow(id: &str, steps: serde_json::Value) -> WorkflowDef {
    serde_json::from_value(json!({"id": id, "name": id, "enabled": false, "trigger": {"kind": "manual"}, "inputs": {}, "steps": steps}))
        .unwrap()
}

#[tokio::test]
async fn a_subworkflow_is_a_normal_run_that_points_at_its_parent() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.txt"), "x").unwrap();
    let e = Engine::new(options(data.path(), ws.path())).await.unwrap();
    // The child is disabled: that only concerns its triggers.
    e.save_workflow(simple_workflow(
        "figlio",
        json!([{"id": "scan", "kind": "tool", "tool": "list_dir", "args": {"path": "{{inputs.dir}}"}}]),
    ))
    .unwrap();
    e.save_workflow(simple_workflow(
        "padre",
        json!([
            {"id": "sub", "kind": "workflow", "workflow": "figlio", "inputs": {"dir": "."}},
            {"id": "glob", "kind": "tool", "tool": "glob", "args": {"pattern": "*.txt", "note": "{{steps.sub.output}}"}}
        ]),
    ))
    .unwrap();
    let first = e.run_workflow("padre", Map::new(), "test").await.unwrap();
    wait_until("parent succeeded", || e.workflow_runs().iter().any(|r| r.id == first.id && r.status == "succeeded")).await;
    let runs = e.workflow_runs();
    let child = runs.iter().find(|r| r.workflow_id == "figlio").expect("child run listed");
    assert_eq!(child.parent_run_id.as_deref(), Some(first.id.as_str()));
    assert_eq!(child.status, "succeeded");
    let parent = runs.iter().find(|r| r.id == first.id).unwrap();
    assert!(parent.steps[0].output.as_deref().unwrap().contains("a.txt"));
    assert!(e.0.store.workflow_states().unwrap().is_empty());
}

#[tokio::test]
async fn a_restart_runs_the_parent_again_from_the_subworkflow_step_and_closes_the_old_child() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e1 = Engine::new(options(data.path(), ws.path())).await.unwrap();
    e1.save_workflow(simple_workflow("figlio", json!([{"id": "ok", "kind": "approval", "message": "Procedo?"}, {"id": "end", "kind": "tool", "tool": "list_dir", "args": {"path": "."}}]))).unwrap();
    e1.save_workflow(simple_workflow("padre", json!([{"id": "sub", "kind": "workflow", "workflow": "figlio"}]))).unwrap();
    let first = e1.run_workflow("padre", Map::new(), "test").await.unwrap();
    wait_until("child waiting", || !e1.approvals().is_empty()).await;
    let old_child = e1.workflow_runs().into_iter().find(|r| r.workflow_id == "figlio").unwrap();

    let e2 = Engine::new(options(data.path(), ws.path())).await.unwrap();
    // The parent asks again, through a new child.
    wait_until("new child waiting", || !e2.approvals().is_empty()).await;
    let runs = e2.workflow_runs();
    let old = runs.iter().find(|r| r.id == old_child.id).unwrap();
    assert_eq!(old.status, "cancelled");
    let new_child = runs.iter().find(|r| r.workflow_id == "figlio" && r.id != old_child.id).expect("a new child");
    assert_eq!(new_child.parent_run_id.as_deref(), Some(first.id.as_str()));
    let req = e2.approvals().remove(0);
    e2.respond_approval(&req.id, true, false).unwrap();
    wait_until("parent succeeded", || e2.workflow_runs().iter().any(|r| r.id == first.id && r.status == "succeeded")).await;
    assert!(e2.0.store.workflow_states().unwrap().is_empty());
}

#[tokio::test]
async fn rewind_restores_originals_deletes_created_files_and_refuses_a_running_run() {
    use aiw_tools::fs::tools;
    use aiw_tools::ToolContext;
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let engine = Engine::new(options(data.path(), ws.path())).await.unwrap();
    std::fs::write(ws.path().join("vecchio.md"), "originale").unwrap();

    let mut ctx = ToolContext::new("run_rw", engine.workspace_root());
    ctx.checkpoint_dir = Some(engine.0.dirs.checkpoints());
    let t = |n: &str| tools().into_iter().find(|t| t.name() == n).unwrap();
    t("write_file").call(json!({"path": "vecchio.md", "content": "cambiato"}), &ctx).await.unwrap();
    t("write_file").call(json!({"path": "nuovo.md", "content": "creato"}), &ctx).await.unwrap();

    assert!(matches!(engine.checkpoint_info("run_nessuno"), Err(Error::NotFound(_))));
    assert!(matches!(engine.checkpoint_info("../fuori"), Err(Error::Invalid(_))));
    let info = engine.checkpoint_info("run_rw").unwrap();
    assert_eq!(info.restore, vec!["vecchio.md"]);
    assert_eq!(info.delete, vec!["nuovo.md"]);

    // A run that is still going cannot be rewound.
    engine.0.runs.lock().unwrap().insert(
        "run_rw".into(),
        RunHandle { session_id: None, cancel: CancellationToken::new(), events: vec![], done: false, origin: None },
    );
    assert!(matches!(engine.rewind("run_rw"), Err(Error::Conflict(_))));
    assert_eq!(std::fs::read_to_string(ws.path().join("vecchio.md")).unwrap(), "cambiato");
    engine.0.runs.lock().unwrap().get_mut("run_rw").unwrap().done = true;

    let res = engine.rewind("run_rw").unwrap();
    assert_eq!(res.restored, vec!["vecchio.md"]);
    assert_eq!(res.deleted, vec!["nuovo.md"]);
    assert_eq!(std::fs::read_to_string(ws.path().join("vecchio.md")).unwrap(), "originale");
    assert!(!ws.path().join("nuovo.md").exists());
    assert!(engine.audit().iter().any(|a| a.action == "checkpoint.rewind" && a.run_id.as_deref() == Some("run_rw")));
    // Nothing left to rewind.
    assert!(matches!(engine.rewind("run_rw"), Err(Error::NotFound(_))));
}
