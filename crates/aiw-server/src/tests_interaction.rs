//! The questions and the requests for a secret of the agent over the real router: the four
//! endpoints, who may use them, what the bodies may hold, and that a secret value is nowhere in
//! what the API says. (The AG-UI entry point has its tests in `agui.rs`.)

use super::tests_security::{fixture, req, send, LAN};
use super::*;
use aiw_runtimes::ScriptedBackend;
use std::sync::Arc;

const GOOD_TOKEN: &str = "FAKE_HF_TOKEN_hf_Z4x8Qm2Lp7Vn3Bk9";
const WRONG_TOKEN: &str = "FAKE_WRONG_TOKEN_r5t6y7u8i9o0";

fn models() -> Value {
    json!({
        "kind": "choice", "title": "Quale modello?", "message": "Ho trovato tre varianti.",
        "options": [{"id": "q4", "label": "Q4_K_M", "description": "Consigliata"}, {"id": "q8", "label": "Q8_0"}]
    })
}

/// The `Authorization` header of a paired device (not the owner).
fn device(e: &Engine) -> String {
    format!("Bearer {}", e.create_api_token("Telefono", "device", None, None, None).unwrap().token)
}

/// A request from the LAN as a paired device would send it.
async fn as_device(app: &Router, auth: &str, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let (st, _, v) = send(app, req(method, uri, Some(LAN), &[("host", "192.168.1.5:7777"), ("authorization", auth)], body)).await;
    (st, v)
}

/// A request with a body that is not JSON the server could read (or is not JSON at all).
async fn raw_post(app: &Router, uri: &str, body: &str) -> (u16, String) {
    use tower::ServiceExt;
    let r = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = app.clone().oneshot(r).await.unwrap();
    let status = res.status().as_u16();
    (status, String::from_utf8_lossy(&axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap()).into_owned())
}

async fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    for _ in 0..300 {
        if let Some(v) = f() {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timeout: {what}");
}

/// Starts a run of the Assistant in a new session, as the app does; returns the session and the run.
async fn start(app: &Router, text: &str) -> (String, String) {
    let (st, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "assistant"}))).await;
    assert_eq!(st, 200, "{ses}");
    let sid = ses["id"].as_str().unwrap().to_string();
    let (st, run) = dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/runs"), Some(json!({"input": text}))).await;
    assert_eq!(st, 200, "{run}");
    (sid, run["run_id"].as_str().unwrap().to_string())
}

async fn done(e: &Engine, run: &str) {
    wait_for("the run to end", || e.run_done(run).then_some(())).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
}

async fn listed(app: &Router, path: &str) -> Vec<Value> {
    let (st, v) = dispatch(app.clone(), "GET", path, None).await;
    assert_eq!(st, 200, "{v}");
    v.as_array().cloned().unwrap_or_else(|| panic!("a list: {v}"))
}

async fn open_one(app: &Router, path: &str) -> Value {
    for _ in 0..300 {
        if let Some(first) = listed(app, path).await.into_iter().next() {
            return first;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("nothing in {path}");
}

fn tool_result(e: &Engine, session: &str) -> Value {
    let m = e.messages(session).unwrap().into_iter().rev().find(|m| m.role == "tool").expect("a tool result");
    serde_json::from_str(&m.content).unwrap_or_else(|_| panic!("JSON: {}", m.content))
}

/// A local Hugging Face that accepts one token and answers 401 to any other.
async fn fake_hf(good: &'static str) -> String {
    use axum::http::HeaderMap;
    let app = axum::Router::new().route(
        "/api/whoami-v2",
        axum::routing::get(move |h: HeaderMap| async move {
            let given = h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
            if given == format!("Bearer {good}") {
                (StatusCode::OK, axum::Json(json!({"name": "tester"})))
            } else {
                (StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "Invalid credentials"})))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://127.0.0.1:{port}")
}

#[tokio::test]
async fn a_question_is_listed_answered_declined_and_checked_over_the_api() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let turns = vec![
        ScriptedBackend::tool_turn("a1", "ask_user", models()),
        ScriptedBackend::text_turn("Scarico la Q4."),
        ScriptedBackend::tool_turn("a2", "ask_user", models()),
        ScriptedBackend::text_turn("Scarico la Q8."),
        ScriptedBackend::tool_turn("a3", "ask_user", models()),
        ScriptedBackend::text_turn("Non scarico nulla."),
    ];
    e.register_native_runtime("llama-embedded", Arc::new(ScriptedBackend::new(turns)));
    let phone = device(&e);

    // First run: the question shows up with the contract's shape, a paired device sees it and answers it.
    let (sid, run) = start(&app, "Scarica un modello").await;
    let q = open_one(&app, "/api/questions").await;
    let id = q["id"].as_str().unwrap().to_string();
    assert_eq!(
        (q["run_id"].as_str(), q["kind"].as_str(), q["multiple"].as_bool(), q["allow_other"].as_bool()),
        (Some(run.as_str()), Some("choice"), Some(false), Some(false))
    );
    assert_eq!(
        (&q["options"][1]["description"], &q["confirm_label"], &q["deny_label"], &q["fields"]),
        (&Value::Null, &Value::Null, &Value::Null, &json!([]))
    );
    assert!(q["expires_at"].as_str().is_some_and(|t| t.ends_with('Z')));
    let (st, listing) = as_device(&app, &phone, "GET", "/api/questions", None).await;
    assert_eq!((st, listing.as_array().map(Vec::len)), (StatusCode::OK, Some(1)));
    let uri = format!("/api/questions/{id}");
    // A wrong answer is a 400 with the reason, and the question is still there.
    let bad = json!({"action": "answer", "selected": ["zz"], "other": null, "confirmed": null, "values": null});
    let (st, v) = as_device(&app, &phone, "POST", &uri, Some(bad)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().contains("opzione che non esiste"), "{v}");
    for body in [
        json!({"action": "boh"}),
        json!({"action": "answer"}),
        json!({"action": "answer", "selected": ["q4", "q8"]}),
        // A body of the wrong type is a 400 like the other wrong answers (not axum's 422), and the question stays open.
        json!({"action": "answer", "selected": "q4"}),
        json!({"action": "answer", "confirmed": "sì"}),
        json!({"action": "answer", "values": ["x"]}),
        json!({"selected": ["q4"]}),
        json!({}),
        json!(["q4"]),
    ] {
        assert_eq!(dispatch(app.clone(), "POST", &uri, Some(body.clone())).await.0, 400, "{body}");
    }
    let (st, text) = raw_post(&app, &uri, "{\"action\":\"answer\",\"selected\":[\"q4\"").await;
    assert_eq!(st, 400, "{text}");
    assert_eq!(listed(&app, "/api/questions").await.len(), 1);
    assert!(!e.run_done(&run), "the run is still waiting");
    // The right answer, with the four fields, from the device.
    let good = json!({"action": "answer", "selected": ["q4"], "other": null, "confirmed": null, "values": null});
    let (st, v) = as_device(&app, &phone, "POST", &uri, Some(good.clone())).await;
    assert_eq!((st, v), (StatusCode::OK, json!({})));
    done(&e, &run).await;
    assert_eq!(tool_result(&e, &sid), json!({"outcome": "answered", "selected": ["q4"], "other": null, "confirmed": null, "values": null}));
    assert!(listed(&app, "/api/questions").await.is_empty());
    // Closed: a 404, with the same body or any other.
    assert_eq!(dispatch(app.clone(), "POST", &uri, Some(good)).await.0, 404);
    assert_eq!(dispatch(app.clone(), "POST", "/api/questions/ask_non_esiste", Some(json!({"action": "decline"}))).await.0, 404);

    // Second run: a body with only what matters (the missing fields are null).
    let (sid, run) = start(&app, "Un altro").await;
    let id = open_one(&app, "/api/questions").await["id"].as_str().unwrap().to_string();
    let (st, v) =
        dispatch(app.clone(), "POST", &format!("/api/questions/{id}"), Some(json!({"action": "answer", "selected": ["q8"]}))).await;
    assert_eq!((st, v), (200, json!({})));
    done(&e, &run).await;
    assert_eq!(tool_result(&e, &sid)["selected"], json!(["q8"]));

    // Third run: «Non ora».
    let (sid, run) = start(&app, "E un terzo").await;
    let id = open_one(&app, "/api/questions").await["id"].as_str().unwrap().to_string();
    let (st, v) = dispatch(app.clone(), "POST", &format!("/api/questions/{id}"), Some(json!({"action": "decline"}))).await;
    assert_eq!((st, v), (200, json!({})));
    done(&e, &run).await;
    assert_eq!(tool_result(&e, &sid), json!({"outcome": "declined"}));
}

#[tokio::test]
async fn a_secret_is_given_over_the_api_by_the_owner_only_and_the_value_is_never_said_back() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    e.set_provider_auth("hf", None, Some(fake_hf(GOOD_TOKEN).await)).await.unwrap();
    let args = json!({"target": "provider_token:hf", "label": "Token di Hugging Face", "reason": "Per i modelli riservati"});
    let turns = vec![
        ScriptedBackend::tool_turn("s1", "aiw_secret_request", args.clone()),
        ScriptedBackend::text_turn("Il token è salvato."),
        ScriptedBackend::tool_turn("s2", "aiw_secret_request", args),
        ScriptedBackend::text_turn("Va bene, niente token."),
    ];
    let backend = ScriptedBackend::new(turns);
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    let phone = device(&e);
    // Everything the API said while the secret was around; the value must be in none of it.
    let mut said: Vec<String> = vec![];

    let (sid, run) = start(&app, "Aggiungi il token di Hugging Face").await;
    let r = open_one(&app, "/api/secret-requests").await;
    said.push(r.to_string());
    let id = r["id"].as_str().unwrap().to_string();
    assert_eq!(
        (r["run_id"].as_str(), r["target"].as_str(), r["label"].as_str(), r["destination"].as_str()),
        (Some(run.as_str()), Some("provider_token:hf"), Some("Token di Hugging Face"), Some("Hugging Face (127.0.0.1)"))
    );
    assert_eq!(r["reason"], "Per i modelli riservati");
    // A device sees that there is a request, and cannot answer it: 403, and it stays open.
    let (st, v) = as_device(&app, &phone, "GET", "/api/secret-requests", None).await;
    assert_eq!((st, v.as_array().map(Vec::len)), (StatusCode::OK, Some(1)));
    let uri = format!("/api/secret-requests/{id}");
    let provide = |value: &str| json!({"action": "provide", "value": value});
    let (st, v) = as_device(&app, &phone, "POST", &uri, Some(provide(GOOD_TOKEN))).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    said.push(v.to_string());
    // The owner's token from the LAN without TLS is a device's, too.
    let owner = format!("Bearer {}", e.token());
    let (st, v) = as_device(&app, &owner, "POST", &uri, Some(provide(GOOD_TOKEN))).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    said.push(v.to_string());
    assert_eq!(listed(&app, "/api/secret-requests").await.len(), 1);
    assert!(!hf_configured(&e), "nothing was stored by a refused call");

    // Bodies that are not right: a fixed sentence, never an echo of what was typed.
    for body in [
        json!({"action": "provide", "value": 123456789}),
        json!({"action": "provide", "value": ["a", GOOD_TOKEN]}),
        json!({"action": "provide", "value": {"x": GOOD_TOKEN}}),
        json!({"value": GOOD_TOKEN}),
        json!({"action": 7, "value": GOOD_TOKEN}),
        json!({"action": "provide"}),
        json!({"action": "boh", "value": GOOD_TOKEN}),
        json!([GOOD_TOKEN]),
    ] {
        let (st, v) = dispatch(app.clone(), "POST", &uri, Some(body.clone())).await;
        assert_eq!(st, 400, "{body} -> {v}");
        said.push(v.to_string());
    }
    for text in [
        format!("{{\"action\":\"provide\",\"value\":\"{GOOD_TOKEN}"),
        format!("action=provide&value={GOOD_TOKEN}"),
        format!("{{\"action\":\"provide\" \"value\":\"{GOOD_TOKEN}\"}}"),
    ] {
        let (st, v) = raw_post(&app, &uri, &text).await;
        assert!(st == 400 || st == 415 || st == 422, "{st} {v}");
        said.push(v);
    }
    assert_eq!(listed(&app, "/api/secret-requests").await.len(), 1, "no wrong body closed the request");

    // The service refuses a value: 401, the words say nothing of it, the request is still open.
    let (st, v) = dispatch(app.clone(), "POST", &uri, Some(provide(WRONG_TOKEN))).await;
    assert_eq!(st, 401, "{v}");
    assert!(v["error"].as_str().unwrap().contains("non è valido"), "{v}");
    said.push(v.to_string());
    assert_eq!(listed(&app, "/api/secret-requests").await.len(), 1);
    // The right one: stored, and the answer only says so.
    let (st, v) = dispatch(app.clone(), "POST", &uri, Some(provide(GOOD_TOKEN))).await;
    assert_eq!((st, &v), (200, &json!({"stored": true})));
    said.push(v.to_string());
    done(&e, &run).await;
    assert_eq!(tool_result(&e, &sid), json!({"outcome": "stored", "target": "provider_token:hf"}));
    assert!(listed(&app, "/api/secret-requests").await.is_empty());
    assert_eq!(dispatch(app.clone(), "POST", &uri, Some(provide(GOOD_TOKEN))).await.0, 404, "closed");
    assert!(hf_configured(&e), "the token is stored (and the fake Hugging Face accepted it)");

    // What the run and the history hold, as the API shows it.
    let (_, events) = dispatch(app.clone(), "GET", &format!("/api/runs/{run}/events.json"), None).await;
    let (_, messages) = dispatch(app.clone(), "GET", &format!("/api/sessions/{sid}/messages"), None).await;
    let (_, audit) = dispatch(app.clone(), "GET", "/api/audit", None).await;
    let (_, config) = dispatch(app.clone(), "GET", "/api/models/providers", None).await;
    assert!(audit.to_string().contains("secret.set") && audit.to_string().contains("provider_token:hf"));
    said.extend([
        events.to_string(),
        messages.to_string(),
        audit.to_string(),
        config.to_string(),
        serde_json::to_string(&*backend.requests.lock().unwrap()).unwrap(),
    ]);
    for (i, text) in said.iter().enumerate() {
        assert!(!text.contains(GOOD_TOKEN) && !text.contains(WRONG_TOKEN), "a value was said back (piece {i}): {text}");
    }
    assert!(said.iter().any(|t| t.contains("secret_request")), "the events were looked at");

    // «Non ora» closes it with a plain 200.
    let (_, run) = start(&app, "Riproviamo").await;
    let id = open_one(&app, "/api/secret-requests").await["id"].as_str().unwrap().to_string();
    let (st, v) = dispatch(app.clone(), "POST", &format!("/api/secret-requests/{id}"), Some(json!({"action": "decline"}))).await;
    assert_eq!((st, v), (200, json!({})));
    done(&e, &run).await;
}

#[tokio::test]
async fn the_four_routes_ask_for_the_scopes_the_contract_gives_them() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    use crate::scopes::{required_scope, Required};
    use aiw_engine::tokens::Scope;
    // The table: questions and the listing of the requests are for a device, the value of a secret needs full access.
    for (method, path, full) in [
        ("GET", "/api/questions", false),
        ("POST", "/api/questions/ask_1", false),
        ("GET", "/api/secret-requests", false),
        ("POST", "/api/secret-requests/sec_1", true),
    ] {
        let want = Required::Scope(if full { Scope::Full } else { Scope::Device });
        assert_eq!(required_scope(method, path), Some(want), "{method} {path}");
    }
    // And over the real router: who gets through (the answer is a 404 for a request that does not exist: the door was open).
    let dev = device(&e);
    let gw = format!("Bearer {}", e.create_api_token("App", "gateway", None, None, None).unwrap().token);
    let decline = || Some(json!({"action": "decline"}));
    let rows: [(&str, &str, Option<Value>, StatusCode, StatusCode); 4] = [
        // method, path, body, as a device, as a gateway app
        ("GET", "/api/questions", None, StatusCode::OK, StatusCode::FORBIDDEN),
        ("POST", "/api/questions/ask_x", decline(), StatusCode::NOT_FOUND, StatusCode::FORBIDDEN),
        ("GET", "/api/secret-requests", None, StatusCode::OK, StatusCode::FORBIDDEN),
        ("POST", "/api/secret-requests/sec_x", decline(), StatusCode::FORBIDDEN, StatusCode::FORBIDDEN),
    ];
    for (method, path, body, as_device_gets, as_gateway_gets) in rows {
        assert_eq!(as_device_(&app, &dev, method, path, body.clone()).await, as_device_gets, "device: {method} {path}");
        assert_eq!(as_device_(&app, &gw, method, path, body.clone()).await, as_gateway_gets, "gateway: {method} {path}");
    }
    // The app on this computer (the owner) gets through to the 404 of a request that is not there.
    let (st, _) = dispatch(app.clone(), "POST", "/api/secret-requests/sec_x", decline()).await;
    assert_eq!(st, 404);
}

fn hf_configured(e: &Engine) -> bool {
    e.model_providers().into_iter().find(|p| p.id == "hf").unwrap().auth_configured
}

async fn as_device_(app: &Router, auth: &str, method: &str, uri: &str, body: Option<Value>) -> StatusCode {
    as_device(app, auth, method, uri, body).await.0
}
