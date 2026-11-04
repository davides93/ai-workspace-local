use super::*;
use aiw_runtimes::ScriptedBackend;

async fn engine(turns: Vec<Vec<aiw_runtimes::ChatDelta>>) -> (Engine, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().to_path_buf()),
        workspace_root: Some(ws.path().to_path_buf()),
        mode: "embedded".into(),
        device: None,
        extra_bin_dirs: vec![],
        offline: true,
    })
    .await
    .unwrap();
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
    (e, data, ws)
}

async fn wait_finished(e: &Engine, run_id: &str) {
    for _ in 0..200 {
        if e.run_done(run_id) {
            tokio::time::sleep(Duration::from_millis(100)).await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("run {run_id} did not finish");
}

#[tokio::test]
async fn seeds_defaults_and_lists_everything() {
    let (e, _d, _w) = engine(vec![]).await;
    assert!(e.agents().unwrap().iter().any(|a| a.id == "operator"));
    assert!(e.runtimes().len() >= 25);
    assert!(e.catalog().iter().any(|c| c.id == "qwen3-0.6b" && !c.fit.is_empty()));
    assert!(e.tools().iter().any(|t| t.name == "shell"));
    assert!(!e.workflows().unwrap().is_empty());
    let sys = e.system(true);
    assert_eq!(sys.pairing_code.as_ref().map(|c| c.len()), Some(6));
    assert!(e.pair("000000x").is_none());
    let code = e.pairing_code();
    assert_eq!(e.pair(&code).as_deref(), Some(e.token()));
}

#[tokio::test]
async fn full_run_with_approval_persists_messages_and_audit() {
    let turns = vec![
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "nota.md", "content": "# Nota\n"})),
        ScriptedBackend::text_turn("Ho creato nota.md"),
    ];
    let (e, _d, ws) = engine(turns).await;
    let s = e.create_session("operator", None).unwrap();
    let mut bus = e.subscribe();
    let run = e.start_run(&s.id, "Crea una nota".into()).await.unwrap();
    // wait for the approval request, then approve it
    let mut approved = false;
    for _ in 0..200 {
        if let Some(a) = e.approvals().first() {
            assert_eq!(a.tool, "write_file");
            e.respond_approval(&a.id.clone(), true, true).unwrap();
            approved = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(approved, "approval was requested");
    wait_finished(&e, &run).await;
    assert_eq!(std::fs::read_to_string(ws.path().join("nota.md")).unwrap(), "# Nota\n");
    let msgs = e.messages(&s.id).unwrap();
    assert_eq!(msgs.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(), vec!["user", "assistant", "tool", "assistant"]);
    let sess = e.sessions().unwrap().into_iter().find(|x| x.id == s.id).unwrap();
    assert_eq!(sess.title, "Crea una nota");
    assert_eq!(sess.last_status.as_deref(), Some("succeeded"));
    assert!(e.audit().iter().any(|a| a.decision.as_deref() == Some("approved")));
    // events were broadcast and are replayable
    let mut got_finish = false;
    while let Ok(m) = bus.try_recv() {
        if m.run_id == run && matches!(m.event.event, AgEvent::RunFinished { .. }) {
            got_finish = true;
        }
    }
    assert!(got_finish);
    assert!(e.run_events(&run).len() > 5);
}

#[tokio::test]
async fn kill_switch_cancels_pending_approvals() {
    let (e, _d, ws) = engine(vec![ScriptedBackend::tool_turn("c1", "shell", json!({"command": "touch boom"}))]).await;
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "fai qualcosa".into()).await.unwrap();
    for _ in 0..200 {
        if !e.approvals().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let r = e.kill().await;
    assert_eq!(r["approvals"], 1);
    wait_finished(&e, &run).await;
    assert!(!ws.path().join("boom").exists());
}

#[tokio::test]
async fn workflow_runs_tool_and_condition_steps() {
    let (e, _d, ws) = engine(vec![]).await;
    std::fs::write(ws.path().join("a.pdf"), "x").unwrap();
    e.save_workflow(
        serde_json::from_value(json!({
            "id": "t", "name": "Test", "enabled": false, "trigger": {"kind": "manual"}, "inputs": {},
            "steps": [
                {"id": "scan", "kind": "tool", "tool": "list_dir", "args": {"path": "."}},
                {"id": "c", "kind": "condition", "if": "{{steps.scan.output}} contains pdf", "then": ["mk"]},
                {"id": "mk", "kind": "tool", "tool": "glob", "args": {"pattern": "*.pdf"}}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let first = e.run_workflow("t", Map::new(), "user").await.unwrap();
    for _ in 0..200 {
        if e.workflow_runs().iter().any(|r| r.id == first.id && r.status == "succeeded") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let run = e.workflow_runs().into_iter().find(|r| r.id == first.id).unwrap();
    assert_eq!(run.status, "succeeded", "{run:?}");
    assert!(run.steps[2].output.as_deref().unwrap_or_default().contains("a.pdf"));
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

#[tokio::test]
async fn isolated_session_writes_in_its_worktree_until_applied() {
    let turns = vec![
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "nota.md", "content": "# Dal worktree\n"})),
        ScriptedBackend::text_turn("Fatto"),
    ];
    let (e, _d, ws) = engine(turns).await;
    git(ws.path(), &["init", "-q"]);
    git(ws.path(), &["config", "core.autocrlf", "false"]);
    std::fs::write(ws.path().join("README.md"), "x\n").unwrap();
    git(ws.path(), &["add", "."]);
    git(ws.path(), &["commit", "-q", "-m", "init"]);

    let s = e.create_session_with("operator", None, Some("worktree")).unwrap();
    let wt = s.worktree.clone().expect("worktree");
    assert!(wt.branch.starts_with("aiw/"));
    assert_eq!(e.session_root(&s.id), std::path::PathBuf::from(&wt.root));
    let run = e.start_run(&s.id, "Scrivi una nota".into()).await.unwrap();
    for _ in 0..200 {
        if let Some(a) = e.approvals().first() {
            e.respond_approval(&a.id.clone(), true, false).unwrap();
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    wait_finished(&e, &run).await;

    // Written in the worktree only.
    assert!(!ws.path().join("nota.md").exists());
    let st = e.worktree_status(&s.id).unwrap();
    assert_eq!(st.files.iter().map(|f| (f.status.as_str(), f.path.as_str())).collect::<Vec<_>>(), vec![("A", "nota.md")]);
    assert!(e.sessions().unwrap().iter().any(|x| x.id == s.id && x.worktree.is_some()));

    e.worktree_apply(&s.id).unwrap();
    assert_eq!(std::fs::read_to_string(ws.path().join("nota.md")).unwrap(), "# Dal worktree\n");
    e.worktree_discard(&s.id).unwrap();
    assert!(!std::path::Path::new(&wt.path).exists());
    assert_eq!(e.session_root(&s.id), e.workspace_root());
    assert!(e.audit().iter().any(|a| a.action == "worktree_apply"));
    assert!(e.create_session_with("operator", None, Some("altro")).is_err());
}
