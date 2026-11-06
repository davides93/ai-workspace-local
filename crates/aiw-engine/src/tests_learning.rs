//! Reflection: lessons proposed after a thumbs down, and what approving them writes.

use super::tests::{engine, wait_finished};
use super::*;
use aiw_runtimes::ScriptedBackend;

fn lesson_json(kind: &str, title: &str, content: &str) -> Vec<aiw_runtimes::ChatDelta> {
    ScriptedBackend::text_turn(&json!({"kind": kind, "title": title, "content": content, "reason": "è andata male"}).to_string())
}

/// A finished session with one answer; returns the session and the id of the assistant message.
async fn answered_session(e: &Engine) -> (String, String) {
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "Riassumi il file".into()).await.unwrap();
    wait_finished(e, &run).await;
    let msg = e.messages(&s.id).unwrap().into_iter().find(|m| m.role == "assistant").unwrap();
    (s.id, msg.id)
}

async fn wait_lessons(e: &Engine, n: usize) -> Vec<Lesson> {
    for _ in 0..200 {
        let l = e.lessons(None).unwrap();
        if l.len() >= n {
            return l;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("la lezione non è arrivata");
}

fn pending(l: &Lesson, id: &str, kind: &str, title: &str, content: &str) -> Lesson {
    Lesson {
        id: id.into(),
        kind: kind.into(),
        title: title.into(),
        content: content.into(),
        status: "pending".into(),
        decided_at: None,
        ..l.clone()
    }
}

#[tokio::test]
async fn a_thumbs_down_creates_a_pending_lesson_and_writes_nothing() {
    let backend = ScriptedBackend::new(vec![
        ScriptedBackend::text_turn("Ecco il riassunto"),
        lesson_json("memory", "Chiedi il formato", "Prima di riassumere chiedi il formato desiderato."),
    ]);
    let (e, data, ws) = engine(vec![]).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    let (sid, mid) = answered_session(&e).await;
    e.set_feedback(&sid, &mid, -1, Some("troppo lungo".into())).unwrap();

    let lessons = wait_lessons(&e, 1).await;
    let l = &lessons[0];
    assert_eq!(
        (l.status.as_str(), l.kind.as_str(), l.session_id.as_str(), l.agent_id.as_str()),
        ("pending", "memory", sid.as_str(), "operator")
    );
    assert!(l.run_id.is_some() && l.decided_at.is_none());
    // The model saw the conversation and the note of the thumbs down.
    let asked = backend.requests.lock().unwrap().last().unwrap().messages.last().unwrap().text();
    assert!(asked.contains("Riassumi il file") && asked.contains("troppo lungo"), "{asked}");
    // Nothing on disk before the approval.
    assert!(!data.path().join("AGENTS.md").exists() && !ws.path().join("AGENTS.md").exists());
    assert!(e.skills().is_empty());

    // Only one lesson waits per session: a second thumbs down does not ask again.
    e.set_feedback(&sid, &mid, -1, None).unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(e.lessons(None).unwrap().len(), 1);
    assert_eq!(e.lessons(Some("pending")).unwrap().len(), 1);
    assert!(e.lessons(Some("approved")).unwrap().is_empty());
    assert!(matches!(e.lessons(Some("boh")), Err(Error::Invalid(_))));
}

#[tokio::test]
async fn a_thumbs_up_or_the_setting_off_asks_for_nothing() {
    let backend = ScriptedBackend::new(vec![ScriptedBackend::text_turn("Ecco"), lesson_json("memory", "T", "C")]);
    let (e, _d, _w) = engine(vec![]).await;
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    let (sid, mid) = answered_session(&e).await;
    e.set_feedback(&sid, &mid, 1, None).unwrap();
    e.set_config("global", "learning.reflect", json!("off")).unwrap();
    e.set_feedback(&sid, &mid, -1, None).unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(e.lessons(None).unwrap().is_empty());
    assert_eq!(backend.requests.lock().unwrap().len(), 1, "only the run asked the model");
}

#[tokio::test]
async fn a_run_stopped_by_its_budget_reflects_only_with_on_failure() {
    let script = || {
        ScriptedBackend::new(vec![
            ScriptedBackend::tool_turn("c1", "list_dir", json!({"path": "."})),
            ScriptedBackend::text_turn("mai raggiunto"),
            lesson_json("skill", "Budget corto", "Spezza il lavoro in passi piccoli."),
        ])
    };
    let (e, _d, _w) = engine(vec![]).await;
    e.set_config("global", "agents.budget.max_turns", json!(1)).unwrap();

    // The default (on_feedback) does not react to a run that ends badly.
    e.register_native_runtime("llama-embedded", Arc::new(script()));
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "Elenca la cartella".into()).await.unwrap();
    wait_finished(&e, &run).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(e.lessons(None).unwrap().is_empty());

    e.set_config("global", "learning.reflect", json!("on_failure")).unwrap();
    e.register_native_runtime("llama-embedded", Arc::new(script()));
    let s = e.create_session("operator", None).unwrap();
    let run = e.start_run(&s.id, "Elenca la cartella".into()).await.unwrap();
    wait_finished(&e, &run).await;
    let lessons = wait_lessons(&e, 1).await;
    assert_eq!((lessons[0].kind.as_str(), lessons[0].run_id.as_deref()), ("skill", Some(run.as_str())));
}

#[tokio::test]
async fn approving_a_skill_writes_it_and_the_next_run_finds_it() {
    let (e, data, ws) = engine(vec![]).await;
    let (sid, _) = answered_session(&e).await;
    e.register_native_runtime(
        "llama-embedded",
        Arc::new(ScriptedBackend::new(vec![lesson_json(
            "skill",
            "Rilascio sicuro",
            "# Rilascio sicuro\n\n1. Esegui i test\n2. Crea il tag",
        )])),
    );
    let l = e.reflect(&sid, None).await.unwrap();
    assert_eq!(l.status, "pending");
    assert!(e.skills().is_empty());

    let edited = e.approve_lesson(&l.id, Some("Rilascio sicuro v2".into()), None).unwrap();
    assert_eq!((edited.status.as_str(), edited.title.as_str()), ("approved", "Rilascio sicuro v2"));
    assert!(edited.decided_at.is_some());
    let text = std::fs::read_to_string(data.path().join("skills/rilascio-sicuro-v2/SKILL.md")).unwrap();
    assert!(text.starts_with("---\nname: rilascio-sicuro-v2\ndescription: Rilascio sicuro\n---"), "{text}");
    assert!(text.contains("2. Crea il tag"));
    let skills = e.skills();
    assert_eq!(skills.len(), 1);
    assert_eq!((skills[0].name.as_str(), skills[0].description.as_str()), ("rilascio-sicuro-v2", "Rilascio sicuro"));
    // The workspace is repository content: nothing goes there.
    assert!(std::fs::read_dir(ws.path()).unwrap().next().is_none());
    assert!(e.audit().iter().any(|a| a.action == "lesson.approve" && a.actor == "user"));

    // Deciding twice is a conflict, and a second skill with the same name gets a suffix.
    assert!(matches!(e.approve_lesson(&l.id, None, None), Err(Error::Conflict(_))));
    assert!(matches!(e.reject_lesson(&l.id), Err(Error::Conflict(_))));
    let again = pending(&l, "les_due", "skill", "Rilascio sicuro v2", "altro");
    e.0.store.put_doc("lesson", &again.id, &again).unwrap();
    e.approve_lesson("les_due", None, None).unwrap();
    assert!(data.path().join("skills/rilascio-sicuro-v2-2/SKILL.md").is_file());
    assert_eq!(e.skills().len(), 2);
}

#[tokio::test]
async fn approving_a_memory_adds_it_under_the_heading_of_the_global_agents_md() {
    let (e, data, ws) = engine(vec![]).await;
    std::fs::write(data.path().join("AGENTS.md"), "# Le mie regole\n\nParla italiano.\n\n## Altro\nresta qui\n").unwrap();
    let (sid, _) = answered_session(&e).await;
    e.register_native_runtime(
        "llama-embedded",
        Arc::new(ScriptedBackend::new(vec![
            lesson_json("memory", "Usa i test", "Esegui sempre i test prima di chiudere."),
            lesson_json("memory", "Sii breve", "Risposte brevi."),
        ])),
    );
    let first = e.reflect(&sid, None).await.unwrap();
    let second = e.reflect(&sid, None).await.unwrap();
    e.approve_lesson(&first.id, None, Some("Esegui i test, sempre.".into())).unwrap();
    e.approve_lesson(&second.id, None, None).unwrap();

    let text = std::fs::read_to_string(data.path().join("AGENTS.md")).unwrap();
    assert!(text.starts_with("# Le mie regole\n\nParla italiano.\n\n## Altro\nresta qui\n"), "{text}");
    assert_eq!(text.matches("## Lezioni apprese").count(), 1, "{text}");
    let (a, b) = (text.find("### Usa i test").unwrap(), text.find("### Sii breve").unwrap());
    assert!(text.find("## Lezioni apprese").unwrap() < a && a < b, "{text}");
    assert!(text.contains("Esegui i test, sempre.") && !text.contains("prima di chiudere"));
    // A later run sees it in its system prompt (the global instructions).
    assert!(e.agents_md().contains("### Sii breve"));
    assert!(!ws.path().join("AGENTS.md").exists());

    // The heading is in the middle of the file: new entries stay in its section.
    std::fs::write(data.path().join("AGENTS.md"), "## Lezioni apprese\n\n### Vecchia (2025-01-01)\nx\n\n## Altro\nresta\n").unwrap();
    let l = pending(&first, "les_x", "memory", "Nuova", "y");
    e.0.store.put_doc("lesson", &l.id, &l).unwrap();
    e.approve_lesson("les_x", None, None).unwrap();
    let text = std::fs::read_to_string(data.path().join("AGENTS.md")).unwrap();
    assert!(text.find("### Nuova").unwrap() < text.find("## Altro").unwrap() && text.ends_with("## Altro\nresta\n"), "{text}");
}

#[tokio::test]
async fn a_rejected_lesson_writes_nothing() {
    let (e, data, _w) = engine(vec![]).await;
    let (sid, _) = answered_session(&e).await;
    e.register_native_runtime("llama-embedded", Arc::new(ScriptedBackend::new(vec![lesson_json("skill", "Scarta", "contenuto")])));
    let l = e.reflect(&sid, None).await.unwrap();
    let r = e.reject_lesson(&l.id).unwrap();
    assert_eq!(r.status, "rejected");
    assert!(e.audit().iter().any(|a| a.action == "lesson.reject"));
    assert!(e.skills().is_empty() && !data.path().join("AGENTS.md").exists());
    assert!(matches!(e.approve_lesson(&l.id, None, None), Err(Error::Conflict(_))));
    assert!(matches!(e.approve_lesson("les_none", None, None), Err(Error::NotFound(_))));
}

#[tokio::test]
async fn an_invalid_answer_is_corrected_once_and_then_it_is_an_error_without_a_lesson() {
    // First answer is not JSON, the correction is: a lesson.
    let (e, _d, _w) = engine(vec![]).await;
    let (sid, _) = answered_session(&e).await;
    e.register_native_runtime(
        "llama-embedded",
        Arc::new(ScriptedBackend::new(vec![ScriptedBackend::text_turn("non so"), lesson_json("memory", "Titolo", "Contenuto")])),
    );
    assert_eq!(e.reflect(&sid, None).await.unwrap().title, "Titolo");

    // Wrong shape twice (unknown kind, then an empty title): one retry only, then a clear error.
    let bad_kind = ScriptedBackend::text_turn(r#"{"kind": "altro", "title": "t", "content": "c", "reason": "r"}"#);
    let empty = ScriptedBackend::text_turn(r#"{"kind": "memory", "title": " ", "content": "c", "reason": "r"}"#);
    let spare = lesson_json("memory", "Non deve arrivare", "x");
    let backend = ScriptedBackend::new(vec![bad_kind, empty, spare]);
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    let err = e.reflect(&sid, None).await.unwrap_err();
    assert!(matches!(err, Error::Invalid(_)) && err.to_string().contains("lezione valida"), "{err}");
    assert_eq!(backend.requests.lock().unwrap().len(), 2);
    let retry = backend.requests.lock().unwrap()[1].messages.last().unwrap().text();
    assert!(retry.contains("Correzione") && retry.contains("valore non consentito"), "{retry}");
    assert_eq!(e.lessons(None).unwrap().len(), 1, "no lesson from the failed attempt");
    assert!(matches!(e.reflect("ses_none", None).await, Err(Error::NotFound(_))));
}
