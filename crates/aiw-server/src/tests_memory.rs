//! Long-term memory over HTTP: list and search, create, edit, forget and restore, approve and
//! reject proposals, delete.

use super::*;

#[tokio::test]
async fn memories_are_created_searched_edited_forgotten_restored_and_deleted() {
    let (app, e, _d, _w) = super::tests::app().await;
    let (st, list) = dispatch(app.clone(), "GET", "/api/memories", None).await;
    assert_eq!((st, list.as_array().map(|l| l.len())), (200, Some(0)));

    let (st, a) = dispatch(
        app.clone(),
        "POST",
        "/api/memories",
        Some(json!({"text": "Preferisco il caffè amaro", "kind": "preference", "pinned": true})),
    )
    .await;
    assert_eq!(st, 200, "{a}");
    assert_eq!(
        (a["status"].as_str(), a["kind"].as_str(), a["scope"].as_str(), a["pinned"].as_bool()),
        (Some("active"), Some("preference"), Some("all"), Some(true))
    );
    let id = a["id"].as_str().unwrap().to_string();
    let (_, b) = dispatch(app.clone(), "POST", "/api/memories", Some(json!({"text": "Ho due gatti"}))).await;
    assert_eq!(b["kind"], "fact", "the default kind");

    // Errors follow the neighbours: 400 for a bad body, 404 for an unknown id, 409 for a wrong state.
    assert_eq!(dispatch(app.clone(), "POST", "/api/memories", Some(json!({"text": " "}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "POST", "/api/memories", Some(json!({"text": "x", "kind": "boh"}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "GET", "/api/memories?status=boh", None).await.0, 400);
    assert_eq!(dispatch(app.clone(), "GET", "/api/memories?kind=boh", None).await.0, 400);
    assert_eq!(dispatch(app.clone(), "PATCH", "/api/memories/mem_none", Some(json!({"text": "x"}))).await.0, 404);
    assert_eq!(dispatch(app.clone(), "POST", "/api/memories/mem_none/forget", None).await.0, 404);

    // List filters and search (ranked, with a score).
    let (_, by_kind) = dispatch(app.clone(), "GET", "/api/memories?kind=preference", None).await;
    assert_eq!(by_kind.as_array().unwrap().len(), 1);
    let (_, found) = dispatch(app.clone(), "GET", "/api/memories?q=gatt", None).await;
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["text"], "Ho due gatti");
    assert!(found[0]["score"].as_f64().unwrap() > 0.0);

    // Edit.
    let (st, u) =
        dispatch(app.clone(), "PATCH", &format!("/api/memories/{id}"), Some(json!({"text": "Preferisco il tè", "pinned": false}))).await;
    assert_eq!((st, u["text"].as_str(), u["pinned"].as_bool()), (200, Some("Preferisco il tè"), Some(false)));
    assert_eq!(dispatch(app.clone(), "PATCH", &format!("/api/memories/{id}"), Some(json!({"kind": "boh"}))).await.0, 400);

    // Forget (soft), list, restore.
    let (st, f) = dispatch(app.clone(), "POST", &format!("/api/memories/{id}/forget"), None).await;
    assert_eq!((st, f["status"].as_str()), (200, Some("forgotten")));
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/memories/{id}/forget"), None).await.0, 409);
    let (_, left) = dispatch(app.clone(), "GET", "/api/memories", None).await;
    assert_eq!(left.as_array().unwrap().len(), 1);
    let (_, gone) = dispatch(app.clone(), "GET", "/api/memories?status=forgotten", None).await;
    assert_eq!(gone[0]["id"], id.as_str());
    let (_, all) = dispatch(app.clone(), "GET", "/api/memories?status=all", None).await;
    assert_eq!(all.as_array().unwrap().len(), 2);
    let (st, r) = dispatch(app.clone(), "POST", &format!("/api/memories/{id}/restore"), None).await;
    assert_eq!((st, r["status"].as_str()), (200, Some("active")));
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/memories/{id}/restore"), None).await.0, 409);

    // Delete for good; delete-all needs the confirmation.
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/memories/{id}"), None).await.0, 200);
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/memories/{id}"), None).await.0, 404);
    assert_eq!(dispatch(app.clone(), "DELETE", "/api/memories", None).await.0, 400);
    assert_eq!(e.memories(None, None).unwrap().len(), 1);
    let (st, d) = dispatch(app.clone(), "DELETE", "/api/memories?confirm=true", None).await;
    assert_eq!((st, d["deleted"].as_u64()), (200, Some(1)));
    assert!(e.memories(Some("all"), None).unwrap().is_empty());
}

#[tokio::test]
async fn proposed_memories_are_approved_or_rejected_over_http() {
    let (app, e, _d, _w) = super::tests::app().await;
    // What the agent only guessed arrives as a proposal.
    for t in ["Forse ama il jazz", "Forse ama il rock"] {
        e.memory_tool("ses", "operator", "memory_save", &json!({"text": t, "kind": "preference", "explicit": false})).await.unwrap();
    }
    let by_text = |t: &str| e.memories(Some("proposed"), None).unwrap().into_iter().find(|m| m.text == t).unwrap();
    let (p, q) = (by_text("Forse ama il jazz"), by_text("Forse ama il rock"));
    let (_, proposed) = dispatch(app.clone(), "GET", "/api/memories?status=proposed", None).await;
    assert_eq!(proposed.as_array().unwrap().len(), 2);
    let (st, ok) = dispatch(app.clone(), "POST", &format!("/api/memories/{}/approve", p.id), None).await;
    assert_eq!((st, ok["status"].as_str()), (200, Some("active")));
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/memories/{}/approve", p.id), None).await.0, 409);
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/memories/{}/reject", p.id), None).await.0, 409);
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/memories/{}/reject", q.id), None).await.0, 200);
    assert_eq!(dispatch(app.clone(), "POST", &format!("/api/memories/{}/reject", q.id), None).await.0, 404);
    let (_, left) = dispatch(app, "GET", "/api/memories?status=proposed", None).await;
    assert!(left.as_array().unwrap().is_empty());
}
