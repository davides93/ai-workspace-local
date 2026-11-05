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
