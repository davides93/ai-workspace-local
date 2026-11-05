//! Export of the trajectories: shape of a record, masking of secrets, exclusions and filters.

use super::tests::{engine, wait_finished};
use super::*;
use crate::traces::{to_jsonl, Redactor, TraceFilter, EXCLUDED, REDACTED};
use aiw_runtimes::ScriptedBackend;

/// Runs one session: the agent asks to write a file, the user refuses, the agent answers.
async fn rejected_write_session(e: &Engine) -> (String, String) {
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "Crea una nota".into()).await.unwrap();
    for _ in 0..200 {
        if let Some(a) = e.approvals().first() {
            e.respond_approval(&a.id.clone(), false, false).unwrap();
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    wait_finished(e, &run).await;
    (s.id, run)
}

#[tokio::test]
async fn a_session_becomes_a_record_with_tool_calls_and_the_refused_approval() {
    let turns = vec![
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "nota.md", "content": "ciao"})),
        ScriptedBackend::text_turn("Non posso scrivere: la chiave è sk-abcdefghijklmnop1234567890"),
    ];
    let (e, _d, _w) = engine(turns).await;
    let (sid, run) = rejected_write_session(&e).await;

    let recs = e.export_traces(&TraceFilter::default()).unwrap();
    assert_eq!(recs.len(), 1);
    let r = &recs[0];
    assert_eq!((r.session_id.as_str(), r.agent_id.as_str()), (sid.as_str(), "operator"));
    assert_eq!(r.last_status.as_deref(), Some("succeeded"));
    let roles: Vec<&str> = r.messages.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, vec!["user", "assistant", "tool", "assistant"]);
    let call = &r.messages[1]["tool_calls"][0];
    assert_eq!(
        (call["id"].as_str(), call["type"].as_str(), call["function"]["name"].as_str()),
        (Some("c1"), Some("function"), Some("write_file"))
    );
    assert_eq!(r.messages[2]["tool_call_id"], "c1");
    assert_eq!(r.runs.len(), 1);
    assert_eq!((r.runs[0].run_id.as_str(), r.runs[0].status.as_str()), (run.as_str(), "succeeded"));
    assert_eq!(r.signals.len(), 1);
    assert_eq!(
        (r.signals[0].kind.as_str(), r.signals[0].tool.as_deref(), r.signals[0].decision.as_str()),
        ("approval", Some("write_file"), "rejected")
    );
    // The key the agent quoted is masked.
    let last = r.messages[3]["content"].as_str().unwrap();
    assert!(last.contains(REDACTED) && !last.contains("sk-abcdefgh"), "{last}");
    // Without masking it stays as it was.
    let raw = e.export_traces(&TraceFilter { redact: false, ..Default::default() }).unwrap();
    assert!(raw[0].messages[3]["content"].as_str().unwrap().contains("sk-abcdefghijklmnop1234567890"));
    // One line per session.
    assert_eq!(to_jsonl(&recs).unwrap().lines().count(), 1);
}

#[tokio::test]
async fn internal_notes_are_counted_not_exported() {
    let (e, _d, _w) = engine(vec![]).await;
    let s = e.create_session("operator", None).unwrap();
    let msg = |role: &str, content: &str, name: Option<&str>| ChatMessage {
        id: new_id("msg"),
        role: role.into(),
        content: content.into(),
        reasoning: None,
        tool_calls: None,
        tool_call_id: None,
        name: name.map(String::from),
        created_at: now_rfc3339(),
    };
    e.0.store
        .append_messages(
            &s.id,
            &[
                msg("user", "ciao", None),
                msg("user", "[Controllo automatico] …", Some("aiw.self_check")),
                msg("user", "formato", Some("aiw.format_check")),
                msg("system", "errore del run", Some(RUN_ERROR_MESSAGE)),
                msg("user", "bloccato", Some(BLOCKED_PROMPT_MESSAGE)),
                msg("assistant", "salve", None),
            ],
        )
        .unwrap();
    let r = &e.export_traces(&TraceFilter::default()).unwrap()[0];
    assert_eq!(r.notes, 4);
    assert_eq!(r.messages.iter().map(|m| m["content"].as_str().unwrap()).collect::<Vec<_>>(), vec!["ciao", "salve"]);
}

#[test]
fn redactor_masks_known_formats_and_stored_secrets() {
    let r = Redactor::new(vec!["hunter2-secret".into(), "abc".into()]);
    for secret in [
        "sk-ant-api03-abcdefghijklmnopqrstuv",
        "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
        "github_pat_11ABCDEFG0abcdefghijklmnop_qrstuvwxyz",
        "AKIAIOSFODNN7EXAMPLE",
        "xoxb-123456789012-abcdefghij",
    ] {
        let out = r.apply(&format!("token {secret} fine"));
        assert_eq!(out, format!("token {REDACTED} fine"), "{secret}");
    }
    assert_eq!(r.apply("Authorization: Bearer abc.def-123456"), format!("Authorization: Bearer {REDACTED}"));
    assert_eq!(r.apply("-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----\ndopo"), format!("{REDACTED}\ndopo"));
    assert_eq!(r.apply("la password è hunter2-secret"), format!("la password è {REDACTED}"));
    // Short stored values are not masked (they would eat ordinary words), plain text is untouched.
    assert_eq!(r.apply("abc e sk-corto"), "abc e sk-corto");
}

#[tokio::test]
async fn mcp_secret_values_are_masked_in_exports() {
    let turns = vec![ScriptedBackend::text_turn("Il token è tok_9f8e7d6c5b4a e basta")];
    let (e, _d, _w) = engine(turns).await;
    e.save_mcp_server(
        serde_json::from_value(json!({
            "name": "remoto", "transport": "http", "url": "http://127.0.0.1:9/mcp", "enabled": false,
            "headers": {"X-Api-Key": "tok_9f8e7d6c5b4a"}
        }))
        .unwrap(),
    )
    .await
    .unwrap();
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "dimmi il token".into()).await.unwrap();
    wait_finished(&e, &run).await;
    let r = &e.export_traces(&TraceFilter::default()).unwrap()[0];
    assert_eq!(r.messages[1]["content"], format!("Il token è {REDACTED} e basta"));
}

#[tokio::test]
async fn tool_messages_citing_excluded_paths_lose_their_content() {
    let turns = vec![
        ScriptedBackend::tool_turn("c1", "read_file", json!({"path": "segreti/chiavi.txt"})),
        ScriptedBackend::tool_turn("c2", "list_dir", json!({"path": "altro"})),
        ScriptedBackend::text_turn("Fatto"),
    ];
    let (e, _d, ws) = engine(turns).await;
    std::fs::create_dir(ws.path().join("segreti")).unwrap();
    std::fs::create_dir(ws.path().join("altro")).unwrap();
    std::fs::write(ws.path().join("altro/x.txt"), "x").unwrap();
    std::fs::write(ws.path().join("segreti/chiavi.txt"), "contenuto riservato").unwrap();
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "leggi".into()).await.unwrap();
    wait_finished(&e, &run).await;
    let f = TraceFilter { exclude_paths: vec!["segreti/**".into()], ..Default::default() };
    let r = &e.export_traces(&f).unwrap()[0];
    let tools: Vec<&Value> = r.messages.iter().filter(|m| m["role"] == "tool").collect();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["content"], EXCLUDED);
    assert_ne!(tools[1]["content"], EXCLUDED, "the listing of `altro` does not cite the excluded path");
    let args = r.messages[1]["tool_calls"][0]["function"]["arguments"].as_str().unwrap();
    assert_eq!(args, EXCLUDED);
    assert!(!to_jsonl(std::slice::from_ref(r)).unwrap().contains("contenuto riservato"));
    // A bad glob is a user error.
    assert!(matches!(e.export_traces(&TraceFilter { exclude_paths: vec!["[".into()], ..Default::default() }), Err(Error::Invalid(_))));
}

#[tokio::test]
async fn filters_select_sessions_by_agent_status_date_and_limit() {
    let (e, _d, _w) = engine(vec![]).await;
    let a = e.create_session("operator", Some("a".into())).unwrap();
    let b = e.create_session("planner", Some("b".into())).unwrap();
    let mut old = e.create_session("operator", Some("vecchia".into())).unwrap();
    old.updated_at = "2020-03-01T10:00:00.000Z".into();
    old.last_status = Some("failed".into());
    e.0.store.upsert_session(&old).unwrap();
    let ids = |f: TraceFilter| e.export_traces(&f).unwrap().into_iter().map(|r| r.session_id).collect::<Vec<_>>();

    assert_eq!(ids(TraceFilter::default()).len(), 3);
    assert_eq!(ids(TraceFilter { agent: Some("planner".into()), ..Default::default() }), vec![b.id.clone()]);
    assert_eq!(ids(TraceFilter { status: Some("failed".into()), ..Default::default() }), vec![old.id.clone()]);
    // A bare date covers the whole day, at both ends.
    assert_eq!(
        ids(TraceFilter { since: Some("2020-03-01".into()), until: Some("2020-03-01".into()), ..Default::default() }),
        vec![old.id.clone()]
    );
    assert_eq!(ids(TraceFilter { until: Some("2020-02-29".into()), ..Default::default() }), Vec::<String>::new());
    let recent = ids(TraceFilter { since: Some("2021-01-01T00:00:00Z".into()), ..Default::default() });
    assert_eq!(recent.len(), 2);
    assert!(recent.contains(&a.id) && recent.contains(&b.id));
    assert_eq!(ids(TraceFilter { limit: Some(1), ..Default::default() }).len(), 1);
    assert!(matches!(e.export_traces(&TraceFilter { since: Some("ieri".into()), ..Default::default() }), Err(Error::Invalid(_))));
}

#[tokio::test]
async fn feedback_is_validated_upserted_and_exported() {
    let turns = vec![ScriptedBackend::text_turn("Ecco la risposta")];
    let (e, _d, _w) = engine(turns).await;
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "ciao".into()).await.unwrap();
    wait_finished(&e, &run).await;
    let msgs = e.messages(&s.id).unwrap();
    let (user, answer) = (&msgs[0], &msgs[1]);
    // The streamed message and the stored one are the same message: the UI rates what it showed live.
    assert!(e
        .run_events(&run)
        .iter()
        .any(|ev| matches!(&ev.event.event, AgEvent::TextMessageStart { message_id, .. } if *message_id == answer.id)));

    // Only assistant messages can be rated, with 1 or -1 and a note of at most 2000 characters.
    assert!(matches!(e.set_feedback(&s.id, &user.id, 1, None), Err(Error::Invalid(_))));
    assert!(matches!(e.set_feedback(&s.id, "msg_none", 1, None), Err(Error::NotFound(_))));
    assert!(matches!(e.set_feedback("ses_none", &answer.id, 1, None), Err(Error::NotFound(_))));
    assert!(matches!(e.set_feedback(&s.id, &answer.id, 2, None), Err(Error::Invalid(_))));
    assert!(matches!(e.set_feedback(&s.id, &answer.id, -1, Some("x".repeat(2001))), Err(Error::Invalid(_))));
    assert!(e.session_feedback(&s.id).unwrap().is_empty());

    let up = e.set_feedback(&s.id, &answer.id, 1, None).unwrap();
    assert_eq!((up.rating, up.run_id.as_deref()), (1, Some(run.as_str())), "the run that wrote the answer");
    // Rating again replaces: one feedback per message, same id, blank note dropped.
    let down = e.set_feedback(&s.id, &answer.id, -1, Some("  troppo lunga  ".into())).unwrap();
    assert_eq!((down.id.as_str(), down.rating, down.note.as_deref()), (up.id.as_str(), -1, Some("troppo lunga")));
    assert_eq!(e.session_feedback(&s.id).unwrap(), vec![down]);
    assert!(e.set_feedback(&s.id, &answer.id, -1, Some("   ".into())).unwrap().note.is_none());
    e.set_feedback(&s.id, &answer.id, -1, Some("troppo lunga, chiave sk-abcdefghijklmnop1234567890".into())).unwrap();

    // Exported with the record (the note is masked like the messages) and selectable by rating.
    let rated = |r: &str| e.export_traces(&TraceFilter { rated: Some(r.into()), ..Default::default() }).unwrap().len();
    let rec = &e.export_traces(&TraceFilter::default()).unwrap()[0];
    assert_eq!(rec.feedback.len(), 1);
    assert_eq!((rec.feedback[0].message_id.as_str(), rec.feedback[0].rating), (answer.id.as_str(), -1));
    assert_eq!(rec.feedback[0].note.as_deref(), Some(format!("troppo lunga, chiave {REDACTED}").as_str()));
    assert_eq!((rated("down"), rated("any"), rated("up")), (1, 1, 0));
    assert!(matches!(e.export_traces(&TraceFilter { rated: Some("boh".into()), ..Default::default() }), Err(Error::Invalid(_))));

    e.delete_feedback(&s.id, &answer.id).unwrap();
    e.delete_feedback(&s.id, &answer.id).unwrap();
    assert!(e.session_feedback(&s.id).unwrap().is_empty());
    assert_eq!(rated("any"), 0);

    // Deleting the session takes its feedback along.
    e.set_feedback(&s.id, &answer.id, 1, None).unwrap();
    e.delete_session(&s.id).unwrap();
    assert!(e.0.store.session_feedback(&s.id).unwrap().is_empty());
}
