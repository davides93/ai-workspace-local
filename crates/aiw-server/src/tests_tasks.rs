//! Background tasks over HTTP: start, observe, pause, instruct, resume, cancel, and what the answers look like.

use super::*;

async fn wait_status(app: &Router, id: &str, status: &str) -> Value {
    for _ in 0..400 {
        let (st, t) = dispatch(app.clone(), "GET", &format!("/api/tasks/{id}"), None).await;
        assert_eq!(st, 200, "{t}");
        if t["status"] == status {
            return t;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("the task did not become {status}");
}

fn quiet_now() -> String {
    let n = chrono::Local::now();
    format!("{}-{}", (n - chrono::Duration::hours(1)).format("%H:%M"), (n + chrono::Duration::hours(1)).format("%H:%M"))
}

#[tokio::test]
async fn a_task_is_started_listed_read_and_finishes_and_a_finished_one_refuses_every_control() {
    let (app, _e, _d, _w) = super::tests::app().await;
    assert_eq!(dispatch(app.clone(), "GET", "/api/tasks", None).await.1, json!([]));
    let (st, t) =
        dispatch(app.clone(), "POST", "/api/tasks", Some(json!({"instruction": "Elenca la cartella", "agent_id": "operator"}))).await;
    assert_eq!(st, 200, "{t}");
    let id = t["id"].as_str().unwrap().to_string();
    assert_eq!(
        (t["origin"].as_str(), t["status"].as_str(), t["title"].as_str()),
        (Some("user"), Some("running"), Some("Elenca la cartella"))
    );
    assert!(t["limits"]["max_seconds"].as_u64().unwrap() > 0, "a task always has limits: {t}");
    let done = wait_status(&app, &id, "succeeded").await;
    assert_eq!(done["segments"], 1);
    assert!(done["progress"]["turns"].as_u64().unwrap() >= 1);
    assert!(done["notes"].as_array().unwrap().len() >= 2, "{done}");
    // The session of the task holds what it did.
    let session = done["session_id"].as_str().unwrap();
    let (_, msgs) = dispatch(app.clone(), "GET", &format!("/api/sessions/{session}/messages"), None).await;
    assert!(msgs.as_array().unwrap().iter().any(|m| m["content"] == "Elenca la cartella"), "{msgs}");

    let (_, all) = dispatch(app.clone(), "GET", "/api/tasks", None).await;
    assert_eq!(all.as_array().unwrap().len(), 1);
    let (_, only) = dispatch(app.clone(), "GET", "/api/tasks?status=paused", None).await;
    assert_eq!(only, json!([]));
    let (_, only) = dispatch(app.clone(), "GET", "/api/tasks?status=succeeded", None).await;
    assert_eq!(only.as_array().unwrap().len(), 1);
    for (path, body) in [("pause", None), ("resume", None), ("cancel", None), ("instructions", Some(json!({"text": "ancora"})))] {
        let (st, v) = dispatch(app.clone(), "POST", &format!("/api/tasks/{id}/{path}"), body).await;
        assert_eq!(st, 409, "{path}: {v}");
    }
}

#[tokio::test]
async fn bad_requests_and_unknown_tasks_are_refused_like_the_neighbours() {
    let (app, _e, _d, _w) = super::tests::app().await;
    assert_eq!(dispatch(app.clone(), "GET", "/api/tasks?status=boh", None).await.0, 400);
    assert_eq!(dispatch(app.clone(), "GET", "/api/tasks/tsk_none", None).await.0, 404);
    for p in ["pause", "resume", "cancel"] {
        assert_eq!(dispatch(app.clone(), "POST", &format!("/api/tasks/tsk_none/{p}"), None).await.0, 404, "{p}");
    }
    assert_eq!(dispatch(app.clone(), "POST", "/api/tasks/tsk_none/instructions", Some(json!({"text": "x"}))).await.0, 404);
    assert_eq!(dispatch(app.clone(), "POST", "/api/tasks", Some(json!({"instruction": "   "}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "POST", "/api/tasks", Some(json!({"instruction": "x", "agent_id": "nessuno"}))).await.0, 404);
}

#[tokio::test]
async fn a_task_held_back_by_quiet_hours_says_why_takes_instructions_and_goes_on_when_forced() {
    let (app, e, _d, _w) = super::tests::app().await;
    e.set_config("global", "proactive.quiet_hours", json!(quiet_now())).unwrap();
    let (st, t) = dispatch(app.clone(), "POST", "/api/tasks", Some(json!({"instruction": "Riordina", "agent_id": "operator"}))).await;
    assert_eq!(st, 200, "{t}");
    let id = t["id"].as_str().unwrap().to_string();
    assert_eq!(
        (t["status"].as_str(), t["pause_reason"].as_str(), t["auto_resume"].as_bool()),
        (Some("paused"), Some("quiet_hours"), Some(true)),
        "{t}"
    );
    assert!(t["pause_text"].as_str().unwrap().contains("Orario di silenzio"));
    // While it waits it takes instructions (typed or dictated), refuses nonsense, and says what it holds.
    let (st, t) = dispatch(
        app.clone(),
        "POST",
        &format!("/api/tasks/{id}/instructions"),
        Some(json!({"text": "Tieni i nomi originali", "source": "voice"})),
    )
    .await;
    assert_eq!(st, 200, "{t}");
    assert_eq!((t["instructions"][0]["status"].as_str(), t["instructions"][0]["source"].as_str()), (Some("pending"), Some("voice")));
    assert_eq!(
        dispatch(app.clone(), "POST", &format!("/api/tasks/{id}/instructions"), Some(json!({"text": "x", "source": "telepatia"}))).await.0,
        400
    );
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/tasks/{id}/instructions"), Some(json!({"text": "  "}))).await.0, 400);
    // Resuming says how to go on anyway; forcing does.
    let (st, v) = dispatch(app.clone(), "POST", &format!("/api/tasks/{id}/resume"), None).await;
    assert_eq!(st, 409, "{v}");
    assert!(v["error"].as_str().unwrap().contains("Riprendi comunque"), "{v}");
    let (st, v) = dispatch(app.clone(), "POST", &format!("/api/tasks/{id}/resume"), Some(json!({"force": true}))).await;
    assert_eq!((st, v["status"].as_str()), (200, Some("running")), "{v}");
    let done = wait_status(&app, &id, "succeeded").await;
    assert_eq!(done["instructions"][0]["status"], "delivered");
}

#[tokio::test]
async fn a_paused_task_can_be_cancelled() {
    let (app, e, _d, _w) = super::tests::app().await;
    e.set_config("global", "proactive.quiet_hours", json!(quiet_now())).unwrap();
    let (_, t) = dispatch(app.clone(), "POST", "/api/tasks", Some(json!({"instruction": "Riordina", "agent_id": "operator"}))).await;
    let id = t["id"].as_str().unwrap().to_string();
    let (st, c) = dispatch(app.clone(), "POST", &format!("/api/tasks/{id}/cancel"), None).await;
    assert_eq!((st, c["status"].as_str()), (200, Some("cancelled")), "{c}");
    assert!(c["finished_at"].is_string());
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/tasks/{id}/resume"), Some(json!({"force": true}))).await.0, 409);
}
