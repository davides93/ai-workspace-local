//! Lessons over HTTP: reflection on request, the list, approval and rejection.

use super::*;
use aiw_runtimes::ScriptedBackend;
use std::sync::Arc;

#[tokio::test]
async fn a_lesson_is_proposed_listed_edited_approved_or_rejected() {
    let (app, e, data, _w) = super::tests::app().await;
    let lesson = |kind: &str, title: &str| {
        ScriptedBackend::text_turn(
            &json!({"kind": kind, "title": title, "content": "Controlla sempre la cartella.", "reason": "serve"}).to_string(),
        )
    };
    e.register_native_runtime(
        "llama-embedded",
        Arc::new(ScriptedBackend::new(vec![
            ScriptedBackend::text_turn("Fatto"),
            lesson("skill", "Controlla la cartella"),
            lesson("memory", "Nota"),
        ])),
    );
    let (_, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "planner"}))).await;
    let sid = ses["id"].as_str().unwrap().to_string();
    let (_, run) = dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/runs"), Some(json!({"input": "Ciao"}))).await;
    for _ in 0..200 {
        if e.run_done(run["run_id"].as_str().unwrap()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(150)).await;

    let (st, a) = dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/reflect"), None).await;
    assert_eq!(st, 200, "{a}");
    assert_eq!((a["status"].as_str(), a["kind"].as_str(), a["session_id"].as_str()), (Some("pending"), Some("skill"), Some(sid.as_str())));
    let (st, b) = dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/reflect"), Some(json!({}))).await;
    assert_eq!(st, 200, "{b}");
    assert_eq!(dispatch(app.clone(), "POST", "/api/sessions/none/reflect", None).await.0, 404);

    let (_, all) = dispatch(app.clone(), "GET", "/api/lessons", None).await;
    assert_eq!(all.as_array().unwrap().len(), 2);
    let (_, pending) = dispatch(app.clone(), "GET", "/api/lessons?status=pending", None).await;
    assert_eq!(pending.as_array().unwrap().len(), 2);
    assert_eq!(dispatch(app.clone(), "GET", "/api/lessons?status=boh", None).await.0, 400);

    let id_a = a["id"].as_str().unwrap();
    let (st, done) = dispatch(
        app.clone(),
        "POST",
        &format!("/api/lessons/{id_a}/approve"),
        Some(json!({"title": "Controlla la cartella 2", "content": "Passi nuovi."})),
    )
    .await;
    assert_eq!(st, 200, "{done}");
    assert_eq!(
        (done["status"].as_str(), done["title"].as_str(), done["content"].as_str()),
        (Some("approved"), Some("Controlla la cartella 2"), Some("Passi nuovi."))
    );
    let skill = std::fs::read_to_string(data.path().join("skills/controlla-la-cartella-2/SKILL.md")).unwrap();
    assert!(skill.contains("Passi nuovi."), "{skill}");
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/lessons/{id_a}/approve"), None).await.0, 409);
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/lessons/{id_a}/reject"), None).await.0, 409);
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/lessons/{id_a}/approve"), Some(json!({"title": " "}))).await.0, 409);

    let id_b = b["id"].as_str().unwrap();
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/lessons/{id_b}/approve"), Some(json!({"title": " "}))).await.0, 400);
    let (st, rejected) = dispatch(app.clone(), "POST", &format!("/api/lessons/{id_b}/reject"), None).await;
    assert_eq!((st, rejected["status"].as_str()), (200, Some("rejected")));
    assert!(!data.path().join("AGENTS.md").exists());
    assert_eq!(dispatch(app.clone(), "POST", "/api/lessons/les_none/reject", None).await.0, 404);
    let (_, left) = dispatch(app, "GET", "/api/lessons?status=pending", None).await;
    assert!(left.as_array().unwrap().is_empty());
}
