//! The proactive assistant over HTTP: status, a manual tick, and the proposal lifecycle.

use super::*;

#[tokio::test]
async fn it_is_off_by_default_and_a_tick_says_why() {
    let (app, _e, _d, _w) = super::tests::app().await;
    let (st, s) = dispatch(app.clone(), "GET", "/api/proactive/status", None).await;
    assert_eq!((st, s["level"].as_str(), s["open_proposals"].as_u64()), (200, Some("off"), Some(0)), "{s}");
    let (st, t) = dispatch(app.clone(), "POST", "/api/proactive/tick", None).await;
    assert_eq!((st, t["outcome"].as_str()), (200, Some("skipped")), "{t}");
    assert!(t["reason"].as_str().unwrap().to_lowercase().contains("spento"), "{t}");
    assert_eq!(dispatch(app.clone(), "GET", "/api/proposals", None).await.1, json!([]));
}

#[tokio::test]
async fn a_due_reminder_becomes_a_proposal_that_can_be_dismissed_and_not_twice() {
    let (app, e, _d, _w) = super::tests::app().await;
    e.set_config("global", "proactive.level", json!("propose")).unwrap();
    e.set_config("global", "proactive.briefing_time", json!("23:59")).unwrap();
    e.set_config("global", "proactive.quiet_hours", json!("")).unwrap();
    e.set_config("global", "proactive.only_on_power", json!(false)).unwrap();
    let (st, m) = dispatch(
        app.clone(),
        "POST",
        "/api/memories",
        Some(json!({"text": "Chiamare il dentista", "kind": "reminder", "due_at": "2020-01-01T09:00:00+00:00"})),
    )
    .await;
    assert_eq!(st, 200, "{m}");
    let (st, t) = dispatch(app.clone(), "POST", "/api/proactive/tick", None).await;
    assert_eq!((st, t["outcome"].as_str(), t["model_called"].as_bool()), (200, Some("proposed"), Some(false)), "{t}");
    let (_, list) = dispatch(app.clone(), "GET", "/api/proposals?status=open&kind=proposal", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
    let id = list[0]["id"].as_str().unwrap().to_string();
    assert!(list[0]["title"].as_str().unwrap().contains("dentista"));
    let (st, one) = dispatch(app.clone(), "GET", &format!("/api/proposals/{id}"), None).await;
    assert_eq!((st, one["status"].as_str()), (200, Some("open")));
    let (_, s) = dispatch(app.clone(), "GET", "/api/proactive/status", None).await;
    assert_eq!(s["open_proposals"], 1);

    let (st, d) = dispatch(app.clone(), "POST", &format!("/api/proposals/{id}/dismiss"), None).await;
    assert_eq!((st, d["status"].as_str()), (200, Some("dismissed")), "{d}");
    // A decided proposal cannot be decided again; the same reminder is not proposed again.
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/proposals/{id}/dismiss"), None).await.0, 409);
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/proposals/{id}/approve"), None).await.0, 409);
    dispatch(app.clone(), "POST", "/api/proactive/tick", None).await;
    let (_, all) = dispatch(app.clone(), "GET", "/api/proposals?kind=proposal", None).await;
    assert_eq!(all.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn bad_filters_and_unknown_ids_are_refused_like_the_neighbours() {
    let (app, _e, _d, _w) = super::tests::app().await;
    assert_eq!(dispatch(app.clone(), "GET", "/api/proposals?status=boh", None).await.0, 400);
    assert_eq!(dispatch(app.clone(), "GET", "/api/proposals?kind=boh", None).await.0, 400);
    assert_eq!(dispatch(app.clone(), "GET", "/api/proposals/prp_none", None).await.0, 404);
    assert_eq!(dispatch(app.clone(), "POST", "/api/proposals/prp_none/approve", None).await.0, 404);
    assert_eq!(dispatch(app.clone(), "POST", "/api/proposals/prp_none/dismiss", None).await.0, 404);
}

#[tokio::test]
async fn approving_starts_a_run_in_a_new_session_and_links_it() {
    let (app, e, _d, _w) = super::tests::app().await;
    e.set_config("global", "proactive.level", json!("propose")).unwrap();
    e.set_config("global", "proactive.briefing_time", json!("23:59")).unwrap();
    e.set_config("global", "proactive.quiet_hours", json!("")).unwrap();
    e.set_config("global", "proactive.only_on_power", json!(false)).unwrap();
    dispatch(
        app.clone(),
        "POST",
        "/api/memories",
        Some(json!({"text": "Riordinare Scaricati", "kind": "reminder", "due_at": "2020-01-01T09:00:00+00:00"})),
    )
    .await;
    dispatch(app.clone(), "POST", "/api/proactive/tick", None).await;
    let (_, list) = dispatch(app.clone(), "GET", "/api/proposals?status=open", None).await;
    let id = list[0]["id"].as_str().unwrap().to_string();
    let (st, a) = dispatch(
        app.clone(),
        "POST",
        &format!("/api/proposals/{id}/approve"),
        Some(json!({"instruction": "Riordina la cartella Scaricati"})),
    )
    .await;
    assert_eq!(st, 200, "{a}");
    assert_eq!(a["status"], "approved");
    assert!(a["run_id"].as_str().is_some() && a["session_id"].as_str().is_some(), "{a}");
}

#[tokio::test]
async fn the_background_tick_answers_with_a_small_summary_and_honours_the_gates() {
    let (app, e, _d, _w) = super::tests::app().await;
    // Off: skipped, with the reason, and no body needed.
    let (st, s) = dispatch(app.clone(), "POST", "/api/proactive/background-tick", None).await;
    assert_eq!((st, s["outcome"].as_str(), s["reason"].as_str()), (200, Some("skipped"), Some("interval")), "{s}");
    assert!(s["skipped_reason"].as_str().unwrap().to_lowercase().contains("spento"));
    assert_eq!(s["new_proposals"], 0);

    e.set_config("global", "proactive.level", json!("propose")).unwrap();
    e.set_config("global", "proactive.briefing_time", json!("23:59")).unwrap();
    e.set_config("global", "proactive.quiet_hours", json!("")).unwrap();
    e.set_config("global", "proactive.only_on_power", json!(false)).unwrap();
    let (st, m) = dispatch(
        app.clone(),
        "POST",
        "/api/memories",
        Some(json!({"text": "Chiamare il dentista", "kind": "reminder", "due_at": "2020-01-01T09:00:00+00:00"})),
    )
    .await;
    assert_eq!(st, 200, "{m}");
    let (st, s) =
        dispatch(app.clone(), "POST", "/api/proactive/background-tick", Some(json!({"reason": "shortcut", "budget_s": 20}))).await;
    assert_eq!(
        (st, s["outcome"].as_str(), s["new_proposals"].as_u64(), s["reason"].as_str()),
        (200, Some("proposed"), Some(1), Some("shortcut")),
        "{s}"
    );
    assert_eq!(s["notification"]["title"], "Una proposta per te", "{s}");
    assert!(s["notification"]["body"].as_str().unwrap().contains("dentista"));
    assert_eq!(s["timed_out"], false);
    // The same check again has nothing new to say.
    let (_, again) = dispatch(app.clone(), "POST", "/api/proactive/background-tick", Some(json!({"reason": "interval"}))).await;
    assert_eq!((again["outcome"].as_str(), again["new_proposals"].as_u64()), (Some("nothing"), Some(0)), "{again}");
    assert!(again.get("notification").is_none());
    // The ordinary tick and the status see the same state.
    let (_, st) = dispatch(app.clone(), "GET", "/api/proactive/status", None).await;
    assert_eq!(st["open_proposals"], 1);
}
