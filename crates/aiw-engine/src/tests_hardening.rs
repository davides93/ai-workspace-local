//! Pairing rate limit, partial persistence of interrupted runs and the delete/rename operations
//! the UI relies on.

use super::*;
use aiw_runtimes::{ChatDelta, ScriptedBackend};
use std::collections::VecDeque;

async fn engine(backend: Arc<dyn ChatBackend>) -> (Engine, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().to_path_buf()),
        workspace_root: Some(ws.path().to_path_buf()),
        mode: "embedded".into(),
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap();
    e.register_native_runtime("llama-embedded", backend);
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

async fn scripted(turns: Vec<Vec<ChatDelta>>) -> (Engine, tempfile::TempDir, tempfile::TempDir) {
    engine(Arc::new(ScriptedBackend::new(turns))).await
}

/// A backend whose turns may break half way.
struct Flaky(Mutex<VecDeque<Vec<Result<ChatDelta>>>>);

#[async_trait::async_trait]
impl ChatBackend for Flaky {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec!["flaky".into()])
    }
    async fn chat(&self, _req: aiw_runtimes::ChatRequest, _cancel: CancellationToken) -> Result<aiw_runtimes::DeltaStream> {
        use futures::StreamExt;
        let turn = self.0.lock().unwrap().pop_front().unwrap_or_default();
        Ok(futures::stream::iter(turn).boxed())
    }
}

async fn wait_finished(e: &Engine, run_id: &str) {
    for _ in 0..200 {
        if e.run_done(run_id) {
            tokio::time::sleep(Duration::from_millis(150)).await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("run {run_id} did not finish");
}

/// A code that is certainly not `code`.
fn wrong(code: &str) -> String {
    let first = if code.starts_with('0') { '1' } else { '0' };
    format!("{first}{}", &code[1..])
}

// ------------------------------------------------------------------ pairing

#[tokio::test]
async fn pairing_failures_lock_everyone_out_and_rotate_the_code() {
    let (e, _d, _w) = scripted(vec![]).await;
    let original = e.pairing_code();
    // Ten wrong codes from ten different addresses: the global limit does not care who asks.
    for i in 0..pairing::MAX_FAILURES {
        let from = format!("10.0.0.{i}");
        assert_eq!(e.pair_attempt(&wrong(&original), Some(&from)), PairOutcome::Invalid);
    }
    let fresh = e.pairing_code();
    assert_ne!(fresh, original, "the code is rotated at lockout");
    assert!(e.audit().iter().any(|a| a.action == "pair_lockout"), "the lockout is audited");

    // Now every attempt is refused with a retry hint, even with the right code, from anywhere.
    for code in [fresh.clone(), original.clone(), wrong(&fresh)] {
        match e.pair_attempt(&code, Some("172.16.0.77")) {
            PairOutcome::Locked { retry_after } => {
                assert!(retry_after > Duration::from_secs(0) && retry_after <= pairing::LOCKOUT_BASE, "{retry_after:?}")
            }
            other => panic!("expected a lockout, got {other:?}"),
        }
    }
    assert!(e.pair(&fresh).is_none(), "the plain API honours the lockout too");
    // Nothing was handed out meanwhile, and the code did not change during the lockout.
    assert_eq!(e.pairing_code(), fresh);
}

#[tokio::test]
async fn one_client_is_limited_before_the_global_limit() {
    let (e, _d, _w) = scripted(vec![]).await;
    let code = e.pairing_code();
    for _ in 0..pairing::MAX_FAILURES_PER_CLIENT {
        assert_eq!(e.pair_attempt(&wrong(&code), Some("192.168.1.9")), PairOutcome::Invalid);
    }
    assert!(matches!(e.pair_attempt(&code, Some("192.168.1.9")), PairOutcome::Locked { .. }));
    // Another device is not affected and pairs with the right code; the global counter is still low.
    assert_eq!(e.pair_attempt(&code, Some("192.168.1.10")), PairOutcome::Paired(e.token().to_string()));
    assert_ne!(e.pairing_code(), code, "a successful pairing consumes the code");
}

#[tokio::test]
async fn a_success_resets_the_counters() {
    let (e, _d, _w) = scripted(vec![]).await;
    for round in 0..3 {
        let code = e.pairing_code();
        // Four failures per round would lock the client out if they added up (limit: five).
        for _ in 0..pairing::MAX_FAILURES_PER_CLIENT - 1 {
            assert_eq!(e.pair_attempt(&wrong(&code), Some("192.168.1.9")), PairOutcome::Invalid, "round {round}");
        }
        assert!(matches!(e.pair_attempt(&code, Some("192.168.1.9")), PairOutcome::Paired(_)), "round {round}");
    }
}

// ------------------------------------------------------------------ interrupted runs

#[tokio::test]
async fn a_failed_run_keeps_the_messages_it_produced() {
    let flaky = Flaky(Mutex::new(
        vec![
            ScriptedBackend::tool_turn("c1", "list_dir", json!({"path": "."})).into_iter().map(Ok).collect(),
            vec![Ok(ChatDelta::Text("Ho visto la cartella e".into())), Err(Error::Runtime("il runtime si è chiuso".into()))],
        ]
        .into(),
    ));
    let (e, _d, ws) = engine(Arc::new(flaky)).await;
    std::fs::write(ws.path().join("a.txt"), "x").unwrap();
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "Cosa c'è qui?".into()).await.unwrap();
    wait_finished(&e, &run).await;

    let msgs = e.messages(&s.id).unwrap();
    let roles: Vec<&str> = msgs.iter().map(|m| m.role.as_str()).collect();
    // The error that ended the run closes the history as a `system` message.
    assert_eq!(roles, vec!["user", "assistant", "tool", "assistant", "system"], "{msgs:?}");
    assert!(msgs[2].content.contains("a.txt"), "the tool result is there");
    assert_eq!(msgs[3].content, "Ho visto la cartella e");
    assert_eq!(
        (msgs[4].name.as_deref(), msgs[4].content.as_str()),
        (Some(RUN_ERROR_MESSAGE), "Errore del runtime: il runtime si è chiuso")
    );
    assert_eq!(e.session(&s.id).unwrap().last_status.as_deref(), Some("failed"));
    let evs = e.run_events(&run);
    assert!(matches!(&evs.last().unwrap().event.event, AgEvent::RunError { code: Some(c), .. } if c == "runtime"));
    // It is shown, not told to the model: the next turn starts from the four real messages.
    assert_eq!(aiw_agent::to_msgs(&msgs).len(), 4);
}

/// A backend that cannot even start a turn.
struct Refusing;

#[async_trait::async_trait]
impl ChatBackend for Refusing {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }
    async fn chat(&self, _req: aiw_runtimes::ChatRequest, _cancel: CancellationToken) -> Result<aiw_runtimes::DeltaStream> {
        Err(Error::Unsupported("serve il plugin nativo dell'app".into()))
    }
}

#[tokio::test]
async fn a_run_that_fails_at_once_leaves_its_error_in_the_history() {
    let (e, _d, _w) = engine(Arc::new(Refusing)).await;
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "Ciao".into()).await.unwrap();
    wait_finished(&e, &run).await;

    // What the chat showed live as RUN_ERROR is still there after a reload (it used to be lost:
    // only the user message was stored).
    let live = match &e.run_events(&run).last().unwrap().event.event {
        AgEvent::RunError { message, .. } => message.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(live, "Errore del runtime: serve il plugin nativo dell'app", "no English prefix in the banner");
    let msgs = e.messages(&s.id).unwrap();
    assert_eq!(
        msgs.iter().map(|m| (m.role.as_str(), m.content.as_str())).collect::<Vec<_>>(),
        [("user", "Ciao"), ("system", live.as_str())]
    );
    assert_eq!(msgs[1].name.as_deref(), Some(RUN_ERROR_MESSAGE));
    assert!(msgs[1].tool_calls.is_none() && msgs[1].tool_call_id.is_none() && msgs[1].reasoning.is_none());
    assert_eq!(e.session(&s.id).unwrap().last_status.as_deref(), Some("failed"));

    // A second failure adds a second marker after its own user message; the model never sees them.
    let run2 = e.start_run(&s.id, "Riprova".into()).await.unwrap();
    wait_finished(&e, &run2).await;
    let msgs = e.messages(&s.id).unwrap();
    assert_eq!(msgs.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(), ["user", "system", "user", "system"]);
    assert_eq!(aiw_agent::to_msgs(&msgs).len(), 2);
}

#[tokio::test]
async fn a_run_that_succeeds_stores_no_error_marker() {
    let (e, _d, _w) = scripted(vec![ScriptedBackend::text_turn("Ciao!")]).await;
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "Ciao".into()).await.unwrap();
    wait_finished(&e, &run).await;
    let msgs = e.messages(&s.id).unwrap();
    assert_eq!(msgs.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(), ["user", "assistant"]);
    assert!(msgs.iter().all(|m| m.name.as_deref() != Some(RUN_ERROR_MESSAGE)));
}

#[tokio::test]
async fn a_cancelled_run_keeps_its_messages_and_the_next_turn_sees_them() {
    let (e, _d, ws) = scripted(vec![
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "nota.md", "content": "# Nota\n"})),
        ScriptedBackend::text_turn("Risposta del secondo turno"),
    ])
    .await;
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "Crea una nota".into()).await.unwrap();
    // The run waits for the user's approval of the write; the user stops it instead.
    for _ in 0..200 {
        if !e.approvals().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(e.approvals().len(), 1);
    e.cancel_run(&run).unwrap();
    wait_finished(&e, &run).await;

    assert!(!ws.path().join("nota.md").exists());
    assert_eq!(e.session(&s.id).unwrap().last_status.as_deref(), Some("cancelled"));
    let msgs = e.messages(&s.id).unwrap();
    let roles: Vec<&str> = msgs.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, vec!["user", "assistant", "tool", "system"], "{msgs:?}");
    assert_eq!(msgs[1].tool_calls.as_ref().unwrap()[0].name, "write_file");
    assert!(msgs[2].content.contains("Negato"), "{}", msgs[2].content);
    assert_eq!(msgs[2].tool_call_id.as_deref(), Some("c1"));
    // The stop is recorded like the error row the chat showed.
    assert_eq!((msgs[3].name.as_deref(), msgs[3].content.as_str()), (Some(RUN_ERROR_MESSAGE), "Run annullato"));

    // The follow-up turn starts from that history.
    let run2 = e.start_run(&s.id, "Va bene, lascia stare".into()).await.unwrap();
    wait_finished(&e, &run2).await;
    let msgs = e.messages(&s.id).unwrap();
    assert_eq!(msgs.last().unwrap().content, "Risposta del secondo turno");
    assert_eq!(e.session(&s.id).unwrap().last_status.as_deref(), Some("succeeded"));
}

/// A tool that returns a screenshot, like `browser_screenshot`.
struct FakeShot;

#[async_trait::async_trait]
impl aiw_tools::Tool for FakeShot {
    fn name(&self) -> &str {
        "fake_screenshot"
    }
    fn description(&self) -> &str {
        "screenshot finto"
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn risk(&self) -> aiw_tools::Risk {
        aiw_tools::Risk::Read
    }
    async fn call(&self, _args: Value, _ctx: &ToolContext) -> Result<aiw_tools::ToolOutput> {
        Ok(aiw_tools::ToolOutput {
            content: "Screenshot allegato.".into(),
            images: vec!["data:image/png;base64,iVBORw0KGgo=".into()],
            is_error: false,
        })
    }
}

#[tokio::test]
async fn tool_images_are_stored_with_the_run_events_for_history_replay() {
    let (e, _d, _w) =
        scripted(vec![ScriptedBackend::tool_turn("c1", "fake_screenshot", json!({})), ScriptedBackend::text_turn("Vedo")]).await;
    e.tool_registry().register(Arc::new(FakeShot));
    let s = e.create_session("coder", None).unwrap();
    let run = e.start_run(&s.id, "Fai uno screenshot".into()).await.unwrap();
    wait_finished(&e, &run).await;

    let image_at = |evs: &[aiw_core::events::SeqEvent]| {
        evs.iter().position(|ev| matches!(&ev.event.event, AgEvent::Custom { name, .. } if name == "aiw.image"))
    };
    let live = e.run_events(&run);
    let i = image_at(&live).expect("aiw.image was emitted");
    assert!(
        matches!(&live[i - 1].event.event, AgEvent::ToolCallResult { tool_call_id, .. } if tool_call_id == "c1"),
        "right after the result"
    );
    let AgEvent::Custom { value, .. } = &live[i].event.event else { unreachable!() };
    assert_eq!(*value, json!({"toolCallId": "c1", "mime": "image/png", "data": "iVBORw0KGgo="}));
    // What a client that connects after a restart replays comes from the database.
    let stored = e.0.store.events(&run).unwrap();
    assert_eq!(image_at(&stored), Some(i));
    assert_eq!(stored, live);
}

// ------------------------------------------------------------------ delete and rename

#[tokio::test]
async fn built_in_agents_cannot_be_deleted_custom_ones_can() {
    let (e, _d, _w) = scripted(vec![]).await;
    for id in ["operator", "coder", "browser", "desktop", "planner"] {
        let err = e.delete_agent(id).unwrap_err();
        assert!(matches!(err, Error::Conflict(ref m) if m.contains("integrato")), "{id}: {err}");
        assert_eq!(err.status(), 409);
        assert!(e.agent(id).is_ok());
    }
    assert!(matches!(e.delete_agent("non-esiste"), Err(Error::NotFound(_))));

    let mut mine = e.agent("planner").unwrap();
    mine.id = "revisore".into();
    mine.name = "Revisore".into();
    e.save_agent(mine).unwrap();
    e.delete_agent("revisore").unwrap();
    assert!(matches!(e.agent("revisore"), Err(Error::NotFound(_))));
    assert!(e.audit().iter().any(|a| a.action == "agent.delete" && a.detail.contains("Revisore")));
}

#[tokio::test]
async fn an_agent_used_by_a_workflow_is_not_deleted() {
    let (e, _d, _w) = scripted(vec![]).await;
    let mut mine = e.agent("planner").unwrap();
    mine.id = "revisore".into();
    mine.name = "Revisore".into();
    e.save_agent(mine).unwrap();
    e.save_workflow(
        serde_json::from_value(json!({
            "id": "wf", "name": "Revisione", "enabled": false, "trigger": {"kind": "manual"}, "inputs": {},
            "steps": [{"id": "r", "kind": "agent", "agent": "revisore", "prompt": "rivedi"}]
        }))
        .unwrap(),
    )
    .unwrap();
    let err = e.delete_agent("revisore").unwrap_err();
    assert!(matches!(err, Error::Conflict(ref m) if m.contains("Revisione")), "{err}");
    e.delete_workflow("wf").unwrap();
    e.delete_agent("revisore").unwrap();
}

#[tokio::test]
async fn a_profile_in_use_is_not_deleted() {
    let (e, _d, _w) = scripted(vec![]).await;
    // `default` is the profile of every seeded agent.
    let err = e.delete_profile("default").unwrap_err();
    assert!(matches!(err, Error::Conflict(ref m) if m.contains("Operatore")), "{err}");
    assert_eq!(err.status(), 409);
    assert!(e.profile("default").is_ok());
    assert!(matches!(e.delete_profile("non-esiste"), Err(Error::NotFound(_))));

    let mut spare = e.profile("default").unwrap();
    spare.id = "scorta".into();
    spare.name = "Scorta".into();
    e.save_profile(spare).unwrap();
    // A workflow step that uses it keeps it alive too.
    e.save_workflow(
        serde_json::from_value(json!({
            "id": "wf", "name": "Riassunto", "enabled": false, "trigger": {"kind": "manual"}, "inputs": {},
            "steps": [{"id": "s", "kind": "llm", "profile": "scorta", "prompt": "riassumi"}]
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(e.delete_profile("scorta"), Err(Error::Conflict(ref m)) if m.contains("Riassunto")));
    e.delete_workflow("wf").unwrap();
    e.delete_profile("scorta").unwrap();
    assert!(matches!(e.profile("scorta"), Err(Error::NotFound(_))));
    assert!(e.audit().iter().any(|a| a.action == "profile.delete"));

    // Once an agent points elsewhere the old profile can go.
    let mut other = e.profile("default").unwrap();
    other.id = "altro".into();
    e.save_profile(other).unwrap();
    for mut a in e.agents().unwrap() {
        a.inference_profile = "altro".into();
        e.save_agent(a).unwrap();
    }
    e.delete_profile("default").unwrap();
}

#[tokio::test]
async fn sessions_are_renamed_without_becoming_recent() {
    let (e, _d, _w) = scripted(vec![]).await;
    let s = e.create_session("operator", Some("Vecchio".into())).unwrap();
    tokio::time::sleep(Duration::from_millis(15)).await;
    let renamed = e.rename_session(&s.id, "  Riordino\n  della   cartella  ").unwrap();
    assert_eq!(renamed.title, "Riordino della cartella");
    assert_eq!(renamed.updated_at, s.updated_at, "renaming is not activity");
    assert_eq!(e.session(&s.id).unwrap().title, "Riordino della cartella");
    assert!(matches!(e.rename_session(&s.id, "   \n "), Err(Error::Invalid(_))));
    assert!(matches!(e.rename_session(&s.id, &"x".repeat(MAX_TITLE_CHARS + 1)), Err(Error::Invalid(_))));
    assert!(e.rename_session(&s.id, &"x".repeat(MAX_TITLE_CHARS)).is_ok());
    assert!(matches!(e.rename_session("ses_assente", "Titolo"), Err(Error::NotFound(_))));
}
