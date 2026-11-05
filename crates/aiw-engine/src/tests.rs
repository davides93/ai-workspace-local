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
        ..Default::default()
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

fn team_def(strategy: &str, supervisor: Option<&str>, members: &[&str]) -> TeamDef {
    TeamDef {
        id: "squadra".into(),
        name: "Squadra".into(),
        description: "Un team di prova".into(),
        strategy: strategy.into(),
        supervisor: supervisor.map(String::from),
        members: members.iter().map(|a| TeamMember { agent: a.to_string(), role: format!("ruolo di {a}") }).collect(),
        aggregator: None,
        budget: None,
        output_schema: None,
    }
}

fn copy_agent(e: &Engine, from: &str, id: &str) {
    let mut a = e.agent(from).unwrap();
    a.id = id.into();
    a.name = id.into();
    e.save_agent(a).unwrap();
}

#[tokio::test]
async fn a_team_session_runs_the_supervisor_with_only_the_members_as_subagents() {
    let call = |index: usize, id: &str, agent: &str| aiw_runtimes::ChatDelta::ToolCall {
        index,
        id: Some(id.into()),
        name: Some("agent".into()),
        arguments: json!({"agent": agent, "task": format!("compito per {agent}")}).to_string(),
    };
    let backend = ScriptedBackend::new(vec![
        vec![
            call(0, "a", "membro-a"),
            call(1, "b", "membro-b"),
            aiw_runtimes::ChatDelta::Done { finish_reason: Some("tool_calls".into()) },
        ],
        ScriptedBackend::text_turn("risposta del membro"),
        ScriptedBackend::text_turn("risposta del membro"),
        ScriptedBackend::text_turn("Sintesi finale"),
    ]);
    let (e, _d, _w) = engine(vec![]).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    for id in ["membro-a", "membro-b", "capo"] {
        copy_agent(&e, "planner", id);
    }
    e.save_team(team_def("supervisor", Some("capo"), &["membro-a", "membro-b"])).unwrap();

    let s = e.create_session("team:squadra", None).unwrap();
    assert_eq!(s.agent_id, "team:squadra");
    let run = e.start_run(&s.id, "Fate il lavoro".into()).await.unwrap();
    wait_finished(&e, &run).await;
    let msgs = e.messages(&s.id).unwrap();
    assert_eq!(msgs.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(), ["user", "assistant", "tool", "tool", "assistant"]);
    assert_eq!(msgs.last().unwrap().content, "Sintesi finale");
    assert!(msgs[2].content.contains("risposta del membro"), "{}", msgs[2].content);

    // The supervisor saw the team section and only the members as sub-agents.
    let reqs = backend.requests.lock().unwrap();
    let first = &reqs[0];
    let system = first.messages[0].text();
    assert!(system.contains("# Team «Squadra»") && system.contains("- membro-a: ruolo di membro-a"), "{system}");
    let spec = first.tools.iter().find(|t| t.function.name == "agent").unwrap();
    assert_eq!(spec.function.parameters["properties"]["agent"]["enum"], json!(["membro-a", "membro-b"]));
    let evs = e.run_events(&run);
    assert_eq!(evs.iter().filter(|x| matches!(x.event.event, AgEvent::SubagentStarted { .. })).count(), 2);
}

#[tokio::test]
async fn parallel_and_pipeline_teams_run_without_a_supervisor_model() {
    let (e, _d, _w) = engine(vec![ScriptedBackend::text_turn("uno"), ScriptedBackend::text_turn("due")]).await;
    for id in ["primo", "secondo"] {
        copy_agent(&e, "planner", id);
    }
    e.save_team(team_def("pipeline", None, &["primo", "secondo"])).unwrap();
    let s = e.create_session("team:squadra", None).unwrap();
    let run = e.start_run(&s.id, "Procedi".into()).await.unwrap();
    wait_finished(&e, &run).await;
    let msgs = e.messages(&s.id).unwrap();
    assert_eq!(msgs.iter().map(|m| (m.role.as_str(), m.content.as_str())).collect::<Vec<_>>(), [("user", "Procedi"), ("assistant", "due")]);
    let types: Vec<String> =
        e.run_events(&run).iter().map(|x| serde_json::to_value(&x.event.event).unwrap()["type"].as_str().unwrap().to_string()).collect();
    assert_eq!(types.first().unwrap(), "RUN_STARTED");
    assert_eq!(types.last().unwrap(), "RUN_FINISHED");
    assert_eq!(types.iter().filter(|t| *t == "SUBAGENT_FINISHED").count(), 2);
    assert_eq!(e.sessions().unwrap()[0].last_status.as_deref(), Some("succeeded"));
}

#[tokio::test]
async fn a_session_of_a_missing_team_is_refused_or_conflicts() {
    let (e, _d, _w) = engine(vec![]).await;
    assert!(matches!(e.create_session("team:nessuno", None), Err(Error::NotFound(_))));
    copy_agent(&e, "planner", "primo");
    e.save_team(team_def("pipeline", None, &["primo"])).unwrap();
    let s = e.create_session("team:squadra", None).unwrap();
    e.delete_team("squadra").unwrap();
    // The session still exists, its team is gone: a conflict the user can fix, not a 404.
    assert!(matches!(e.start_run(&s.id, "ciao".into()).await, Err(Error::Conflict(_))));
}

fn handoff_turn(to: &str, summary: &str) -> Vec<aiw_runtimes::ChatDelta> {
    ScriptedBackend::tool_turn("h", "handoff", json!({"agent": to, "summary": summary}))
}

/// Waits until the session has no run going and its last message satisfies `done`.
async fn wait_messages(e: &Engine, session_id: &str, done: impl Fn(&[ChatMessage]) -> bool) -> Vec<ChatMessage> {
    for _ in 0..200 {
        let msgs = e.messages(session_id).unwrap();
        let idle = e.active_run(session_id).is_none() && e.session(session_id).unwrap().last_status.as_deref() != Some("running");
        if idle && done(&msgs) {
            return msgs;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the conversation did not settle: {:?}", e.messages(session_id).unwrap().iter().map(|m| &m.content).collect::<Vec<_>>());
}

#[tokio::test]
async fn a_handoff_moves_the_session_to_the_other_agent_and_starts_its_run() {
    let backend = ScriptedBackend::new(vec![handoff_turn("secondo", "Serve una revisione"), ScriptedBackend::text_turn("Revisione fatta")]);
    let (e, _d, _w) = engine(vec![]).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    copy_agent(&e, "planner", "primo");
    copy_agent(&e, "planner", "secondo");
    let s = e.create_session("primo", None).unwrap();
    let run = e.start_run(&s.id, "Aiutami".into()).await.unwrap();
    let msgs = wait_messages(&e, &s.id, |m| m.last().is_some_and(|x| x.content == "Revisione fatta")).await;

    assert_eq!(e.session(&s.id).unwrap().agent_id, "secondo");
    let shown: Vec<(&str, &str)> = msgs.iter().map(|m| (m.role.as_str(), m.content.as_str())).collect();
    assert_eq!(shown[0], ("user", "Aiutami"));
    assert_eq!(shown[3], ("assistant", "Passo la conversazione a secondo: Serve una revisione"));
    assert_eq!((shown[4].0, shown[4].1), ("user", "[Passaggio da primo] Serve una revisione"));
    assert_eq!(msgs[4].name.as_deref(), Some(HANDOFF_MESSAGE));
    assert_eq!(shown[5], ("assistant", "Revisione fatta"));
    // The first run announced it and ended normally.
    let evs = e.run_events(&run);
    assert!(evs.iter().any(|x| matches!(&x.event.event, AgEvent::Custom { name, value } if name == "aiw.handoff" && value["to"] == "secondo" && value["from"] == "primo")));
    assert!(matches!(evs.last().unwrap().event.event, AgEvent::RunFinished { .. }));
    // The second run uses the new agent's profile and sees the summary, not the first system prompt.
    let reqs = backend.requests.lock().unwrap();
    assert!(reqs[1].messages.last().unwrap().text().starts_with("[Passaggio da primo]"));
}

#[tokio::test]
async fn a_handoff_to_a_more_permissive_agent_is_refused_and_the_run_goes_on() {
    let backend = ScriptedBackend::new(vec![handoff_turn("libero", "Passa a te"), ScriptedBackend::text_turn("Resto io")]);
    let (e, _d, _w) = engine(vec![]).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    copy_agent(&e, "planner", "cauto");
    copy_agent(&e, "planner", "libero");
    let mut cauto = e.agent("cauto").unwrap();
    cauto.mode = "ask".into();
    e.save_agent(cauto).unwrap();
    let mut libero = e.agent("libero").unwrap();
    libero.mode = "yolo".into();
    e.save_agent(libero).unwrap();
    let s = e.create_session("cauto", None).unwrap();
    e.start_run(&s.id, "Aiutami".into()).await.unwrap();
    let msgs = wait_messages(&e, &s.id, |m| m.iter().any(|x| x.content == "Resto io")).await;
    assert_eq!(e.session(&s.id).unwrap().agent_id, "cauto");
    let tool = msgs.iter().find(|m| m.role == "tool").unwrap();
    assert!(tool.content.contains("Passaggio rifiutato") && tool.content.contains("più permissiva"), "{}", tool.content);
}

#[tokio::test]
async fn handoffs_stop_after_three_per_user_message() {
    let turns = (0..5).map(|i| handoff_turn(if i % 2 == 0 { "secondo" } else { "primo" }, "Tocca a te")).collect();
    let (e, _d, _w) = engine(turns).await;
    copy_agent(&e, "planner", "primo");
    copy_agent(&e, "planner", "secondo");
    let s = e.create_session("primo", None).unwrap();
    e.start_run(&s.id, "Aiutami".into()).await.unwrap();
    let msgs =
        wait_messages(&e, &s.id, |m| m.last().is_some_and(|x| x.name.as_deref() == Some(HANDOFF_MESSAGE) && x.role == "system")).await;
    assert!(msgs.last().unwrap().content.contains("Limite di 3 passaggi"), "{}", msgs.last().unwrap().content);
    // The user's run plus three handoffs: four handoff requests were answered, the fourth refused.
    assert_eq!(msgs.iter().filter(|m| m.role == "user" && m.name.as_deref() == Some(HANDOFF_MESSAGE)).count(), 3);
    assert_eq!(e.session(&s.id).unwrap().agent_id, "secondo", "primo → secondo → primo → secondo, the fourth is refused");
}

#[tokio::test]
async fn pipeline_members_share_the_team_state_and_the_root_run_streams_it() {
    let backend = ScriptedBackend::new(vec![
        ScriptedBackend::tool_turn("w", "team_state", json!({"op": "set", "key": "esito", "value": "trovato in /srv"})),
        ScriptedBackend::text_turn("ho scritto"),
        ScriptedBackend::tool_turn("r", "team_state", json!({"op": "get", "key": "esito"})),
        ScriptedBackend::text_turn("ho letto"),
    ]);
    let (e, _d, _w) = engine(vec![]).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    for id in ["primo", "secondo"] {
        copy_agent(&e, "planner", id);
        // The members list their tools by name: the team still gives them the shared state.
        let mut a = e.agent(id).unwrap();
        a.tools = vec!["read_file".into()];
        e.save_agent(a).unwrap();
    }
    e.save_team(team_def("pipeline", None, &["primo", "secondo"])).unwrap();
    let s = e.create_session("team:squadra", None).unwrap();
    let run = e.start_run(&s.id, "Procedi".into()).await.unwrap();
    wait_finished(&e, &run).await;

    // The second member's tool result carries what the first one wrote.
    let reqs = backend.requests.lock().unwrap();
    let result = reqs[3].messages.iter().rev().find(|m| m.role == "tool").unwrap().text();
    assert_eq!(result, "\"trovato in /srv\"");
    // The root run got a snapshot with the shared object (the members' own runs did not).
    let snapshots: Vec<serde_json::Value> = e
        .run_events(&run)
        .iter()
        .filter_map(|x| match &x.event.event {
            AgEvent::StateSnapshot { snapshot } => snapshot.shared.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(snapshots, vec![json!({"esito": "trovato in /srv"})]);
}

#[tokio::test]
async fn team_state_is_not_offered_outside_teams() {
    let backend = ScriptedBackend::new(vec![ScriptedBackend::text_turn("ok")]);
    let (e, _d, _w) = engine(vec![]).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    let s = e.create_session("planner", None).unwrap();
    let run = e.start_run(&s.id, "ciao".into()).await.unwrap();
    wait_finished(&e, &run).await;
    assert!(!backend.requests.lock().unwrap()[0].tools.iter().any(|t| t.function.name == "team_state"));
}
