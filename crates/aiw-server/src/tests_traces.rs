//! Export of the trajectories over HTTP.

use super::tests_errors::{fixture, raw};
use super::*;
use axum::http::{header, Method};

#[tokio::test]
async fn traces_export_streams_one_json_object_per_session() {
    let (app, e, _d) = fixture(None).await;
    let a = e.create_session("operator", Some("prima".into())).unwrap();
    let b = e.create_session("planner", Some("seconda".into())).unwrap();

    let (st, headers, body) = raw(&app, Method::GET, "/api/traces/export", None, "").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/x-ndjson");
    let rows: Vec<Value> = body.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|r| r["session_id"] == a.id) && rows.iter().any(|r| r["session_id"] == b.id));
    assert!(rows[0]["messages"].is_array() && rows[0]["feedback"].is_array());

    let (_, _, body) = raw(&app, Method::GET, "/api/traces/export?agent=planner&limit=5", None, "").await;
    let rows: Vec<Value> = body.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["session_id"], b.id);

    let (st, _, body) = raw(&app, Method::GET, "/api/traces/export?since=ieri", None, "").await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    let (st, _, _) = raw(&app, Method::GET, "/api/traces/export?limit=molti", None, "").await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _, _) = raw(&app, Method::GET, "/api/traces/export?exclude_path=%5B", None, "").await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn feedback_routes_rate_assistant_messages() {
    let (app, e, _d, _w) = super::tests::app().await;
    let (_, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "planner"}))).await;
    let sid = ses["id"].as_str().unwrap().to_string();
    let (_, run) = dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/runs"), Some(json!({"input": "Cosa c'è qui?"}))).await;
    let rid = run["run_id"].as_str().unwrap().to_string();
    for _ in 0..200 {
        if e.run_done(&rid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    let (_, msgs) = dispatch(app.clone(), "GET", &format!("/api/sessions/{sid}/messages"), None).await;
    let answer = msgs.as_array().unwrap().iter().rev().find(|m| m["role"] == "assistant").unwrap()["id"].as_str().unwrap().to_string();
    let user = msgs[0]["id"].as_str().unwrap().to_string();
    let url = |m: &str| format!("/api/sessions/{sid}/messages/{m}/feedback");

    let (st, f) = dispatch(app.clone(), "PUT", &url(&answer), Some(json!({"rating": -1, "note": "sbagliata"}))).await;
    assert_eq!(st, 200, "{f}");
    assert_eq!((f["rating"].as_i64(), f["note"].as_str(), f["run_id"].as_str()), (Some(-1), Some("sbagliata"), Some(rid.as_str())));
    let (st, _) = dispatch(app.clone(), "PUT", &url(&answer), Some(json!({"rating": 1}))).await;
    assert_eq!(st, 200);
    let (_, list) = dispatch(app.clone(), "GET", &format!("/api/sessions/{sid}/feedback"), None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["rating"], 1);
    assert!(list[0].get("note").is_none(), "a rating without note carries none");

    // Refusals: a user message (400), a bad rating (400), unknown message or session (404).
    assert_eq!(dispatch(app.clone(), "PUT", &url(&user), Some(json!({"rating": 1}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "PUT", &url(&answer), Some(json!({"rating": 5}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "PUT", &url("msg_none"), Some(json!({"rating": 1}))).await.0, 404);
    assert_eq!(dispatch(app.clone(), "GET", "/api/sessions/none/feedback", None).await.0, 404);
    assert_eq!(dispatch(app.clone(), "DELETE", "/api/sessions/none/messages/x/feedback", None).await.0, 404);

    let (_, _, body) = raw(&app, Method::GET, "/api/traces/export?rated=up", None, "").await;
    assert_eq!(body.lines().count(), 1);
    let (_, _, body) = raw(&app, Method::GET, "/api/traces/export?rated=down", None, "").await;
    assert_eq!(body.lines().count(), 0);

    assert_eq!(dispatch(app.clone(), "DELETE", &url(&answer), None).await.0, 200);
    let (_, list) = dispatch(app, "GET", &format!("/api/sessions/{sid}/feedback"), None).await;
    assert!(list.as_array().unwrap().is_empty());
}
