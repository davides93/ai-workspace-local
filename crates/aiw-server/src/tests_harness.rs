//! The simple harness from end to end: one run of the Assistant over `POST /agui/assistant`, read as the
//! SSE stream a client would read, that goes through a question, a request for a secret and the
//! confirmation of a management tool in the order a chat does. Each piece has tests of its own
//! (`agui.rs`, `tests_interaction.rs`, `tests_assistant.rs`, and the tools' files in the engine); what is
//! checked here is the whole path in one run, and that a paired device cannot take part in the two
//! steps that are the owner's.

use super::tests_security::{fixture, req, send, LAN};
use super::*;
use aiw_runtimes::ScriptedBackend;
use axum::response::Response;
use http_body_util::BodyExt;
use std::sync::Arc;

/// The token the fake Hugging Face accepts: recognisable, never a real one.
const FAKE_TOKEN: &str = "FAKE_HF_TOKEN_hf_Harness_R3m8Xq5Tn2Wv7Yc4";

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

/// The request of an AG-UI client for the Assistant: `props` is its `forwardedProps`.
fn input(thread: &str, props: Value) -> Value {
    json!({
        "threadId": thread, "runId": "run-client-1",
        "messages": [{"id": "m1", "role": "user", "content": "Metti un tema scuro e collega Hugging Face"}],
        "tools": [], "context": [], "state": {}, "forwardedProps": props
    })
}

/// What a client that can show questions and take secrets says about itself.
fn interactive() -> Value {
    json!({"aiw": {"interactive": ["questions", "secrets"]}})
}

/// The owner's AG-UI request: this computer, no network peer.
async fn post_agui(app: &Router, body: Value) -> Response {
    tower::ServiceExt::oneshot(app.clone(), req("POST", "/agui/assistant", None, &[], Some(body))).await.unwrap()
}

/// The frames of an SSE text that are complete (a `data:` line counts once its line ends).
fn frames_of(text: &str) -> Vec<Value> {
    let complete = text.rfind('\n').map_or("", |i| &text[..i]);
    complete
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(|d| serde_json::from_str(d.trim()).unwrap_or_else(|e| panic!("data line is not JSON ({e}): {d}")))
        .collect()
}

/// The `value` of the custom events with this name, in order.
fn customs(evs: &[Value], name: &str) -> Vec<Value> {
    evs.iter().filter(|v| v["type"] == "CUSTOM" && v["name"] == name).map(|v| v["value"].clone()).collect()
}

/// An SSE body read as it comes (the stream stays open while the run waits), with a deadline.
struct Live {
    body: axum::body::Body,
    text: String,
}

impl Live {
    async fn more(&mut self) -> bool {
        match self.body.frame().await {
            Some(frame) => {
                if let Ok(data) = frame.expect("a frame").into_data() {
                    self.text.push_str(&String::from_utf8_lossy(&data));
                }
                true
            }
            None => false,
        }
    }

    /// Reads until a complete custom event of this name has come, and returns its value.
    async fn until_custom(&mut self, name: &str) -> Value {
        let read = async {
            loop {
                if let Some(v) = customs(&frames_of(&self.text), name).pop() {
                    return v;
                }
                assert!(self.more().await, "the stream ended before {name}: {}", self.text);
            }
        };
        match tokio::time::timeout(Duration::from_secs(10), read).await {
            Ok(v) => v,
            Err(_) => panic!("no {name} in: {}", self.text),
        }
    }

    /// Reads to the end of the stream, which closes with the run.
    async fn to_the_end(&mut self) -> Vec<Value> {
        let read = async { while self.more().await {} };
        tokio::time::timeout(Duration::from_secs(10), read).await.unwrap_or_else(|_| panic!("the stream does not end: {}", self.text));
        frames_of(&format!("{}\n", self.text))
    }
}

/// The name of each event of a stream as the order check reads it: `RUN_STARTED`, `tool:<name>` for the call of a
/// tool, `result:<name>` for its result, the name of a custom event, the type of any other.
fn names_of(evs: &[Value]) -> Vec<String> {
    let mut tool_of: std::collections::HashMap<String, String> = Default::default();
    for v in evs.iter().filter(|v| v["type"] == "TOOL_CALL_START") {
        tool_of.insert(v["toolCallId"].as_str().unwrap().into(), v["toolCallName"].as_str().unwrap().into());
    }
    evs.iter()
        .map(|v| match v["type"].as_str().unwrap() {
            "CUSTOM" => v["name"].as_str().unwrap().to_string(),
            "TOOL_CALL_START" => format!("tool:{}", v["toolCallName"].as_str().unwrap()),
            "TOOL_CALL_RESULT" => format!("result:{}", tool_of[v["toolCallId"].as_str().unwrap()]),
            other => other.to_string(),
        })
        .collect()
}

/// The result a tool gave the model, from the `TOOL_CALL_RESULT` of the stream.
fn tool_result(evs: &[Value], tool: &str) -> String {
    let call = evs.iter().find(|v| v["type"] == "TOOL_CALL_START" && v["toolCallName"] == tool);
    let id = call.unwrap_or_else(|| panic!("no call of {tool}"))["toolCallId"].clone();
    let result = evs.iter().find(|v| v["type"] == "TOOL_CALL_RESULT" && v["toolCallId"] == id);
    result.unwrap_or_else(|| panic!("no result of {tool}"))["content"].as_str().unwrap().to_string()
}

/// The `Authorization` header of a paired device (not the owner).
fn device(e: &Engine) -> String {
    format!("Bearer {}", e.create_api_token("Telefono", "device", None, None, None).unwrap().token)
}

/// A request from the LAN as a paired device would send it (no TLS).
async fn as_device(app: &Router, auth: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let (st, _, v) = send(app, req("POST", uri, Some(LAN), &[("host", "192.168.1.5:7777"), ("authorization", auth)], Some(body))).await;
    (st, v)
}

fn theme(e: &Engine) -> Option<Value> {
    e.config().effective.get("ui.theme").cloned()
}

#[tokio::test]
async fn one_run_of_the_assistant_over_agui_asks_takes_a_secret_and_waits_for_the_owner() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    e.set_provider_auth("hf", None, Some(fake_hf(FAKE_TOKEN).await)).await.unwrap();
    let ask = json!({
        "kind": "choice", "title": "Che tema?", "message": "Quale tema vuoi?",
        "options": [{"id": "dark", "label": "Scuro", "description": "Consigliato"}, {"id": "light", "label": "Chiaro"}]
    });
    let secret = json!({"target": "provider_token:hf", "label": "Token di Hugging Face", "reason": "Per i modelli riservati"});
    let change = json!({"key": "ui.theme", "value": "dark"});
    let backend = ScriptedBackend::new(vec![
        ScriptedBackend::tool_turn("t1", "ask_user", ask),
        ScriptedBackend::tool_turn("t2", "aiw_secret_request", secret),
        ScriptedBackend::tool_turn("t3", "aiw_setting_set", change.clone()),
        ScriptedBackend::text_turn("Fatto: tema scuro e token salvato."),
    ]);
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    let phone = device(&e);
    let before = theme(&e);
    assert_ne!(before, Some(json!("dark")));

    // The owner's client, that declares what it can show.
    let res = post_agui(&app, input("thr-harness", interactive())).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers().get("content-type").unwrap().to_str().unwrap().starts_with("text/event-stream"));
    let (sid, rid) = (
        res.headers().get("x-aiw-session-id").unwrap().to_str().unwrap().to_string(),
        res.headers().get("x-aiw-run-id").unwrap().to_str().unwrap().to_string(),
    );
    // Held until the end: dropping the body would cancel the run.
    let mut live = Live { body: res.into_body(), text: String::new() };

    // 1. A question comes as an event; the run waits; the answer is a request of its own.
    let q = live.until_custom("aiw.question").await;
    assert_eq!((q["kind"].as_str(), q["title"].as_str(), q["run_id"].as_str()), (Some("choice"), Some("Che tema?"), Some(rid.as_str())));
    assert_eq!(e.questions().len(), 1);
    assert!(!e.run_done(&rid), "the run waits for the person");
    let qid = q["id"].as_str().unwrap().to_string();
    let reply = json!({"action": "answer", "selected": ["dark"], "other": null, "confirmed": null, "values": null});
    let (st, v) = dispatch(app.clone(), "POST", &format!("/api/questions/{qid}"), Some(reply)).await;
    assert_eq!((st, v), (200, json!({})));

    // 2. A request for a secret: an event without a value; a device cannot give one, and nothing is stored.
    let s = live.until_custom("aiw.secret_request").await;
    let mut keys: Vec<&str> = s.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["destination", "expires_at", "id", "label", "reason", "run_id", "target"]);
    assert_eq!((s["target"].as_str(), s["run_id"].as_str()), (Some("provider_token:hf"), Some(rid.as_str())));
    assert_eq!(s["destination"], "Hugging Face (127.0.0.1)");
    let sid_secret = s["id"].as_str().unwrap().to_string();
    let uri = format!("/api/secret-requests/{sid_secret}");
    let provide = json!({"action": "provide", "value": FAKE_TOKEN});
    let (st, v) = as_device(&app, &phone, &uri, provide.clone()).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(!v.to_string().contains(FAKE_TOKEN));
    // The owner's token from the LAN without TLS is a device's, too.
    let owner_lan = format!("Bearer {}", e.token());
    assert_eq!(as_device(&app, &owner_lan, &uri, provide.clone()).await.0, StatusCode::FORBIDDEN);
    assert_eq!(e.secret_requests().len(), 1, "the request is still open");
    assert!(!e.model_providers().into_iter().find(|p| p.id == "hf").unwrap().auth_configured, "nothing was stored by a refused call");
    assert!(!e.run_done(&rid));
    // The owner (this computer) gives it.
    let (st, v) = dispatch(app.clone(), "POST", &uri, Some(provide)).await;
    assert_eq!((st, v), (200, json!({"stored": true})));
    assert!(e.model_providers().into_iter().find(|p| p.id == "hf").unwrap().auth_configured, "the fake provider accepted and stored it");

    // 3. A management tool that changes something: the run stops on a confirmation, and the change waits for the owner.
    let a = live.until_custom("aiw.approval_request").await;
    assert_eq!(
        (a["tool"].as_str(), a["run_id"].as_str(), a["risk"].as_str()),
        (Some("aiw_setting_set"), Some(rid.as_str()), Some("medium"))
    );
    assert_eq!(a["args"], change);
    let aid = a["id"].as_str().unwrap().to_string();
    let uri = format!("/api/approvals/{aid}");
    // A paired device may answer the confirmations of ordinary tools, not this one: 403, and it stays open.
    let allow = json!({"decision": "allow", "remember": true});
    let (st, v) = as_device(&app, &phone, &uri, allow.clone()).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v["error"].as_str().unwrap().contains("proprietario"), "{v}");
    assert_eq!(as_device(&app, &owner_lan, &uri, allow.clone()).await.0, StatusCode::FORBIDDEN);
    assert_eq!(e.approvals().len(), 1, "the confirmation is still open");
    assert_eq!(theme(&e), before, "nothing was changed by a refused confirmation");
    assert!(!e.run_done(&rid));
    // The owner decides.
    let (st, v) = dispatch(app.clone(), "POST", &uri, Some(allow)).await;
    assert_eq!((st, v), (200, json!({})));

    // 4. The rest of the stream ends with the run.
    let evs = live.to_the_end().await;
    let all = live.text.clone();
    let names = names_of(&evs);
    assert_eq!(names.first().map(String::as_str), Some("RUN_STARTED"));
    assert_eq!(names.last().map(String::as_str), Some("RUN_FINISHED"));
    for ev in [&evs[0], evs.last().unwrap()] {
        assert_eq!((ev["threadId"].as_str(), ev["runId"].as_str()), (Some("thr-harness"), Some("run-client-1")), "{ev}");
    }
    // In this order, each once: the question and its answer, the secret and its outcome, the change and its confirmation.
    let order = [
        "tool:ask_user",
        "aiw.question",
        "aiw.question_resolved",
        "result:ask_user",
        "tool:aiw_secret_request",
        "aiw.secret_request",
        "aiw.secret_resolved",
        "result:aiw_secret_request",
        "tool:aiw_setting_set",
        "aiw.approval_request",
        "aiw.approval_resolved",
        "result:aiw_setting_set",
        "TEXT_MESSAGE_CONTENT",
        "RUN_FINISHED",
    ];
    let mut at = 0;
    for want in order {
        let found = names[at..].iter().position(|n| n == want).unwrap_or_else(|| panic!("{want} is not after position {at} in {names:?}"));
        at += found + 1;
    }
    for once in ["aiw.question", "aiw.secret_request", "aiw.approval_request", "aiw.secret_resolved"] {
        assert_eq!(names.iter().filter(|n| *n == once).count(), 1, "{once}");
    }
    assert_eq!(
        customs(&evs, "aiw.question_resolved"),
        [json!({"id": qid, "outcome": "answered", "answer": {"selected": ["dark"], "other": null, "confirmed": null, "values": null}})]
    );
    assert_eq!(customs(&evs, "aiw.secret_resolved"), [json!({"id": sid_secret, "outcome": "stored"})]);
    assert_eq!(customs(&evs, "aiw.approval_resolved"), [json!({"id": aid, "allowed": true})]);
    let answered: Value = serde_json::from_str(&tool_result(&evs, "ask_user")).unwrap();
    assert_eq!((answered["outcome"].as_str(), &answered["selected"]), (Some("answered"), &json!(["dark"])));
    assert_eq!(tool_result(&evs, "aiw_secret_request"), json!({"outcome": "stored", "target": "provider_token:hf"}).to_string());
    assert!(tool_result(&evs, "aiw_setting_set").contains("dark"), "{}", tool_result(&evs, "aiw_setting_set"));
    let text: String = evs.iter().filter(|v| v["type"] == "TEXT_MESSAGE_CONTENT").map(|v| v["delta"].as_str().unwrap()).collect();
    assert_eq!(text, "Fatto: tema scuro e token salvato.");

    // The change was made once the owner confirmed, and nothing is left waiting.
    assert!(e.run_done(&rid));
    assert_eq!(theme(&e), Some(json!("dark")));
    assert!(e.questions().is_empty() && e.secret_requests().is_empty() && e.approvals().is_empty());

    // The secret value is nowhere: not in the stream, the log of the run, the saved messages, the audit, the
    // prompts the model was sent, the list of providers.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let (_, log) = dispatch(app.clone(), "GET", &format!("/api/runs/{rid}/events.json"), None).await;
    let (_, messages) = dispatch(app.clone(), "GET", &format!("/api/sessions/{sid}/messages"), None).await;
    let (_, audit) = dispatch(app.clone(), "GET", "/api/audit", None).await;
    let (_, providers) = dispatch(app.clone(), "GET", "/api/models/providers", None).await;
    let prompts = serde_json::to_string(&*backend.requests.lock().unwrap()).unwrap();
    assert_eq!(backend.requests.lock().unwrap().len(), 4, "four model calls, one per turn of the script");
    let audit_rows = audit.as_array().expect("a list of audit rows");
    assert!(
        audit_rows.iter().any(|r| r["action"] == "secret.set" && r["detail"] == "provider_token:hf"),
        "the secret left one row, with its target"
    );
    assert!(
        audit_rows.iter().any(|r| r["tool"] == "aiw_setting_set" && r["decision"] == "approved"),
        "the confirmation of the change is in the audit"
    );
    let said = [
        ("the SSE stream", all),
        ("the events of the run", log.to_string()),
        ("the saved messages", messages.to_string()),
        ("the audit", audit.to_string()),
        ("the providers", providers.to_string()),
        ("the prompts of the model", prompts),
    ];
    for (what, text) in &said {
        assert!(!text.contains(FAKE_TOKEN), "the value of the secret is in {what}");
    }
    assert!(said.iter().all(|(_, t)| !t.is_empty()) && said[1].1.contains("aiw.secret_request"), "the pieces were really looked at");
    // The session holds what the chat shows: the three tool calls and the answer, in the order they happened.
    let roles: Vec<String> = e.messages(&sid).unwrap().into_iter().map(|m| m.role).collect();
    assert_eq!(roles.iter().filter(|r| *r == "tool").count(), 3, "{roles:?}");
    assert_eq!(roles.first().map(String::as_str), Some("user"));
    assert_eq!(roles.last().map(String::as_str), Some("assistant"));
}

#[tokio::test]
async fn without_a_declaration_the_agent_asks_in_text_and_cannot_take_a_secret_but_a_change_still_needs_the_owner() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    e.set_provider_auth("hf", None, Some(fake_hf(FAKE_TOKEN).await)).await.unwrap();
    let ask = json!({"kind": "confirm", "message": "Metto il tema scuro?"});
    let secret = json!({"target": "provider_token:hf", "label": "Token di Hugging Face"});
    let backend = ScriptedBackend::new(vec![
        ScriptedBackend::tool_turn("t1", "ask_user", ask),
        ScriptedBackend::tool_turn("t2", "aiw_secret_request", secret),
        ScriptedBackend::tool_turn("t3", "aiw_setting_set", json!({"key": "ui.theme", "value": "dark"})),
        ScriptedBackend::text_turn("Non ho cambiato nulla."),
    ]);
    e.register_native_runtime("llama-embedded", Arc::new(backend));
    let phone = device(&e);
    let before = theme(&e);

    // The client says nothing about what it can show (`forwardedProps` is empty).
    let res = post_agui(&app, input("thr-bare", json!({}))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let rid = res.headers().get("x-aiw-run-id").unwrap().to_str().unwrap().to_string();
    let mut live = Live { body: res.into_body(), text: String::new() };

    // The question and the secret come back at once as `unsupported`, with nothing to answer; the run goes on to the change.
    let a = live.until_custom("aiw.approval_request").await;
    let so_far = frames_of(&live.text);
    assert_eq!(
        serde_json::from_str::<Value>(&tool_result(&so_far, "ask_user")).unwrap(),
        json!({"outcome": "unsupported", "hint": "Fai la domanda in testo e aspetta il prossimo messaggio."})
    );
    let hint = serde_json::from_str::<Value>(&tool_result(&so_far, "aiw_secret_request")).unwrap();
    assert_eq!(hint["outcome"], "unsupported");
    assert!(hint["hint"].as_str().unwrap().contains("dall'app su questo computer"), "{hint}");
    for none in ["aiw.question", "aiw.question_resolved", "aiw.secret_request", "aiw.secret_resolved"] {
        assert!(customs(&so_far, none).is_empty(), "{none}");
    }
    assert!(e.questions().is_empty() && e.secret_requests().is_empty());

    // A change is confirmed whatever the client declared, and only by the owner: a device that denies is refused too.
    assert_eq!(a["tool"], "aiw_setting_set");
    let uri = format!("/api/approvals/{}", a["id"].as_str().unwrap());
    let deny = json!({"decision": "deny", "remember": false});
    let (st, v) = as_device(&app, &phone, &uri, deny.clone()).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v["error"].as_str().unwrap().contains("proprietario"), "{v}");
    assert_eq!(e.approvals().len(), 1);
    assert!(!e.run_done(&rid), "the run is still waiting for the owner");
    // The owner says no: the run goes on, and the setting stays as it was.
    let (st, v) = dispatch(app.clone(), "POST", &uri, Some(deny)).await;
    assert_eq!((st, v), (200, json!({})));
    let evs = live.to_the_end().await;
    assert_eq!(evs.last().unwrap()["type"], "RUN_FINISHED");
    assert_eq!(customs(&evs, "aiw.approval_resolved"), [json!({"id": a["id"], "allowed": false})]);
    assert_eq!(theme(&e), before, "a refused change changes nothing");
    assert!(!e.model_providers().into_iter().find(|p| p.id == "hf").unwrap().auth_configured, "no secret was taken");
}
