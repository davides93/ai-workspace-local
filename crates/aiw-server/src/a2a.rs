//! A2A (Agent2Agent) 1.0, JSON-RPC binding: other agents (another AIW device, an orchestrator,
//! a cloud agent the user runs) discover this device's agents through the Agent Card at
//! `/.well-known/agent-card.json` and hand them tasks with `SendMessage` on `/a2a`.
//!
//! Each `contextId` maps to one local session, so a conversation continues where it left off.
//! Tasks live in SQLite (`a2a_tasks`) and expire after 24 hours.
//! Streaming: `SendStreamingMessage` (0.3: `message/stream`) and `SubscribeToTask` (`tasks/resubscribe`)
//! answer with an SSE stream of `TaskStatusUpdateEvent` / `TaskArtifactUpdateEvent` built from the run's
//! event bus (text as artifact chunks, tools as status messages). A client that does not stream polls `GetTask`.
//! Every action still goes through the local policy, approvals and audit. The 0.3 method names
//! (`message/send`, `tasks/get`, `tasks/cancel`) are accepted too, with 0.3-shaped results.

use crate::extract::Json;
use crate::AppState;
use aiw_core::ids::new_id;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::time::Duration;

pub const PROTOCOL_VERSION: &str = "1.0";

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// `GET /.well-known/agent-card.json`
pub async fn agent_card(State(s): State<AppState>, headers: HeaderMap) -> Json<Value> {
    let host = headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or("127.0.0.1:7777");
    let base = format!("http://{host}");
    let d = s.engine.device();
    let skills: Vec<Value> = s
        .engine
        .agents()
        .unwrap_or_default()
        .into_iter()
        .map(|a| json!({"id": a.id, "name": a.name, "description": a.description, "tags": ["agente", a.mode]}))
        .collect();
    Json(json!({
        "name": format!("AI Workspace · {}", d.name),
        "description": "Agenti locali di AI Workspace che lavorano con i modelli di questo dispositivo: file, shell in sandbox, browser e workflow, con le conferme dell'utente. Scegli l'agente con metadata.agent (default: operator).",
        "supportedInterfaces": [{"url": format!("{base}/a2a"), "protocolBinding": "JSONRPC", "protocolVersion": PROTOCOL_VERSION}],
        "provider": {"organization": "AI Workspace Local", "url": "https://github.com/davides93/ai-workspace-local"},
        "version": aiw_core::VERSION,
        "capabilities": {"streaming": true, "pushNotifications": false},
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain"],
        "skills": skills,
        "securitySchemes": {"pairing": {"httpAuthSecurityScheme": {"scheme": "Bearer", "description": "Token ottenuto con il codice di pairing (POST /api/pair); non serve da questo dispositivo"}}},
        "securityRequirements": [{"schemes": {"pairing": {"list": []}}}]
    }))
}

fn rpc_error(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

/// 1.0 objects → 0.3 shape (kind discriminators, lowercase states and roles).
fn legacy(task: &Value) -> Value {
    let mut t = task.clone();
    t["kind"] = json!("task");
    if let Some(state) = t["status"]["state"].as_str() {
        let s = state.trim_start_matches("TASK_STATE_").to_lowercase().replace('_', "-");
        t["status"]["state"] = json!(if s == "canceled" { "canceled".to_string() } else { s });
    }
    let fix_msg = |m: &mut Value| {
        m["kind"] = json!("message");
        if let Some(r) = m["role"].as_str() {
            m["role"] = json!(if r == "ROLE_AGENT" { "agent" } else { "user" });
        }
        if let Some(parts) = m["parts"].as_array_mut() {
            for p in parts {
                p["kind"] = json!("text");
            }
        }
    };
    if let Some(h) = t["history"].as_array_mut() {
        h.iter_mut().for_each(fix_msg);
    }
    if let Some(a) = t["artifacts"].as_array_mut() {
        for art in a {
            if let Some(parts) = art["parts"].as_array_mut() {
                for p in parts {
                    p["kind"] = json!("text");
                }
            }
        }
    }
    t
}

fn wrap(task: &Value, legacy_shape: bool) -> Value {
    if legacy_shape {
        legacy(task)
    } else {
        json!({"task": task})
    }
}

fn text_of(message: &Value) -> String {
    message["parts"]
        .as_array()
        .map(|parts| parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

fn finish(s: &AppState, task_id: &str, art_id: Option<&str>, result: aiw_core::Result<(String, String)>) {
    let art_id = art_id.map(String::from).unwrap_or_else(|| new_id("art"));
    let _ = s.engine.a2a_update_task(task_id, |task, session| {
        if task["status"]["state"] == "TASK_STATE_CANCELED" {
            return;
        }
        let context = task["contextId"].as_str().unwrap_or_default().to_string();
        match result {
            Ok((answer, sid)) => {
                *session = Some(sid);
                let agent_msg = json!({"messageId": new_id("msg"), "role": "ROLE_AGENT", "parts": [{"text": answer}], "contextId": context, "taskId": task_id});
                task["artifacts"] = json!([{"artifactId": art_id, "name": "risposta", "parts": [{"text": answer}]}]);
                if let Some(h) = task["history"].as_array_mut() {
                    h.push(agent_msg);
                }
                task["status"] = json!({"state": "TASK_STATE_COMPLETED", "timestamp": now()});
            }
            Err(e) => {
                task["status"] = json!({
                    "state": "TASK_STATE_FAILED",
                    "timestamp": now(),
                    "message": {"messageId": new_id("msg"), "role": "ROLE_AGENT", "parts": [{"text": e.to_string()}], "contextId": context, "taskId": task_id}
                });
            }
        }
    });
}

/// A task that was accepted: stored, with its local session, ready to run.
struct Prepared {
    task_id: String,
    session: String,
    agent: String,
    text: String,
    title: String,
}

fn prepare(s: &AppState, params: &Value, id: &Value) -> Result<Prepared, Value> {
    let message = &params["message"];
    let text = text_of(message);
    if text.trim().is_empty() {
        return Err(rpc_error(id, -32602, "il messaggio non contiene parti di testo"));
    }
    let agent = params["metadata"]["agent"].as_str().or_else(|| message["metadata"]["agent"].as_str()).unwrap_or("operator").to_string();
    if s.engine.agent(&agent).is_err() {
        return Err(rpc_error(id, -32602, format!("agente sconosciuto: {agent}")));
    }
    let context = message["contextId"].as_str().filter(|c| !c.is_empty()).map(String::from).unwrap_or_else(|| new_id("ctx"));
    let title = format!("A2A · {}", text.chars().take(40).collect::<String>());
    // One local session per context, created now so that the task can be cancelled at once.
    let known = s.engine.a2a_context_session(&context).ok().flatten();
    let session = match known {
        Some(sid) => sid,
        None => match s.engine.create_session(&agent, Some(title.clone())) {
            Ok(created) => created.id,
            Err(e) => return Err(rpc_error(id, -32603, e.to_string())),
        },
    };
    let task_id = new_id("task");
    let user_msg = json!({
        "messageId": message["messageId"].as_str().map(String::from).unwrap_or_else(|| new_id("msg")),
        "role": "ROLE_USER", "parts": [{"text": text}], "contextId": context, "taskId": task_id
    });
    let task = json!({"id": task_id, "contextId": context, "status": {"state": "TASK_STATE_WORKING", "timestamp": now()}, "artifacts": [], "history": [user_msg]});
    if let Err(e) = s.engine.a2a_put_task(&task, Some(&session)) {
        return Err(rpc_error(id, -32603, e.to_string()));
    }
    Ok(Prepared { task_id, session, agent, text, title })
}

async fn send_message(s: &AppState, params: &Value, legacy_shape: bool, id: &Value) -> Value {
    let p = match prepare(s, params, id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let blocking = params["configuration"]["blocking"].as_bool() != Some(false)
        && params["configuration"]["returnImmediately"].as_bool() != Some(true);
    let state = s.clone();
    let (tid, agent, text, session, title) = (p.task_id.clone(), p.agent, p.text, p.session, p.title);
    let work = async move {
        let r = state.engine.run_agent_task(&agent, &text, Some(&session), &title).await;
        finish(&state, &tid, None, r);
    };
    if blocking {
        work.await;
    } else {
        tokio::spawn(work);
    }
    let task = s.engine.a2a_task(&p.task_id).ok().flatten().map(|(t, _)| t).unwrap_or(Value::Null);
    json!({"jsonrpc": "2.0", "id": id, "result": wrap(&task, legacy_shape)})
}

// ------------------------------------------------------------------ streaming

fn is_terminal(state: &str) -> bool {
    matches!(state, "TASK_STATE_COMPLETED" | "TASK_STATE_FAILED" | "TASK_STATE_CANCELED" | "TASK_STATE_REJECTED")
}

fn legacy_state(state: &str) -> String {
    let s = state.trim_start_matches("TASK_STATE_").to_lowercase().replace('_', "-");
    if s == "canceled" {
        "canceled".into()
    } else {
        s
    }
}

/// One JSON-RPC response of the stream. 1.0 wraps the payload (`task`, `statusUpdate`, `artifactUpdate`),
/// the 0.3 shape is the payload with a `kind`.
fn stream_item(id: &Value, key: &str, mut payload: Value, legacy_shape: bool) -> Value {
    let result = if legacy_shape {
        payload["kind"] = json!(match key {
            "statusUpdate" => "status-update",
            "artifactUpdate" => "artifact-update",
            _ => "task",
        });
        payload
    } else {
        json!({ key: payload })
    };
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn status_update(task_id: &str, context: &str, state: &str, text: Option<&str>, legacy_shape: bool) -> Value {
    let mut status = json!({"state": if legacy_shape { json!(legacy_state(state)) } else { json!(state) }, "timestamp": now()});
    if let Some(t) = text {
        let part = if legacy_shape { json!({"kind": "text", "text": t}) } else { json!({"text": t}) };
        status["message"] = json!({"messageId": new_id("msg"), "role": if legacy_shape { "agent" } else { "ROLE_AGENT" }, "parts": [part], "contextId": context, "taskId": task_id});
    }
    let mut v = json!({"taskId": task_id, "contextId": context, "status": status});
    if legacy_shape {
        v["final"] = json!(is_terminal(state));
    }
    v
}

fn artifact_update(task_id: &str, context: &str, art_id: &str, text: &str, append: bool, last: bool, legacy_shape: bool) -> Value {
    let part = if legacy_shape { json!({"kind": "text", "text": text}) } else { json!({"text": text}) };
    json!({"taskId": task_id, "contextId": context, "artifact": {"artifactId": art_id, "name": "risposta", "parts": [part]}, "append": append, "lastChunk": last})
}

/// Sends the stream of a task: the task as it is now, then updates built from the run's events until the
/// task ends. `watch` is the session whose run to follow; the end is read from the stored task, so the
/// stream closes also when the run was started by somebody else (`SubscribeToTask`).
fn stream_task(
    s: AppState,
    mut bus: tokio::sync::broadcast::Receiver<aiw_engine::RunEventMsg>,
    id: Value,
    task_id: String,
    session: String,
    art_id: String,
    legacy_shape: bool,
) -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    tokio::spawn(async move {
        let send = |v: Value| {
            let tx = tx.clone();
            async move { tx.send(Ok(Event::default().data(v.to_string()))).await.is_ok() }
        };
        let Some((task, _)) = s.engine.a2a_task(&task_id).ok().flatten() else { return };
        let context = task["contextId"].as_str().unwrap_or_default().to_string();
        let first = if legacy_shape { legacy(&task) } else { task.clone() };
        if !send(stream_item(&id, "task", first, legacy_shape)).await {
            return;
        }
        let mut run_id = s.engine.active_run(&session);
        let mut streamed = false;
        let mut tick = tokio::time::interval(Duration::from_millis(400));
        loop {
            let current = s.engine.a2a_task(&task_id).ok().flatten().map(|(t, _)| t);
            let state = current.as_ref().and_then(|t| t["status"]["state"].as_str()).unwrap_or("TASK_STATE_FAILED").to_string();
            if is_terminal(&state) {
                // Whatever was not streamed goes out now, then the final status.
                if let Some(t) = current.as_ref() {
                    let answer: String = t["artifacts"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|a| a["parts"][0]["text"].as_str())
                        .unwrap_or_default()
                        .to_string();
                    if state == "TASK_STATE_COMPLETED" && !streamed && !answer.is_empty() {
                        let ev = artifact_update(&task_id, &context, &art_id, &answer, false, true, legacy_shape);
                        if !send(stream_item(&id, "artifactUpdate", ev, legacy_shape)).await {
                            return;
                        }
                    } else if streamed && state == "TASK_STATE_COMPLETED" {
                        let ev = artifact_update(&task_id, &context, &art_id, "", true, true, legacy_shape);
                        if !send(stream_item(&id, "artifactUpdate", ev, legacy_shape)).await {
                            return;
                        }
                    }
                    let msg = t["status"]["message"]["parts"][0]["text"].as_str().map(String::from);
                    let ev = status_update(&task_id, &context, &state, msg.as_deref(), legacy_shape);
                    let _ = send(stream_item(&id, "statusUpdate", ev, legacy_shape)).await;
                }
                return;
            }
            tokio::select! {
                m = bus.recv() => {
                    let Ok(m) = m else { if matches!(m, Err(tokio::sync::broadcast::error::RecvError::Closed)) { return } else { continue } };
                    if run_id.is_none() {
                        if let aiw_core::events::AgEvent::RunStarted { thread_id, run_id: r } = &m.event.event {
                            if *thread_id == session {
                                run_id = Some(r.clone());
                            }
                        }
                    }
                    if run_id.as_deref() != Some(m.run_id.as_str()) {
                        continue;
                    }
                    use aiw_core::events::AgEvent;
                    let item = match &m.event.event {
                        AgEvent::TextMessageContent { delta, .. } if !delta.is_empty() => {
                            let ev = artifact_update(&task_id, &context, &art_id, delta, streamed, false, legacy_shape);
                            streamed = true;
                            Some(stream_item(&id, "artifactUpdate", ev, legacy_shape))
                        }
                        AgEvent::ToolCallStart { tool_call_name, .. } => {
                            let text = format!("Strumento in uso: {tool_call_name}");
                            Some(stream_item(&id, "statusUpdate", status_update(&task_id, &context, "TASK_STATE_WORKING", Some(&text), legacy_shape), legacy_shape))
                        }
                        _ => None,
                    };
                    if let Some(item) = item {
                        if !send(item).await {
                            return;
                        }
                    }
                }
                _ = tick.tick() => {
                    if run_id.is_none() {
                        run_id = s.engine.active_run(&session);
                    }
                }
            }
        }
    });
    Sse::new(tokio_stream::wrappers::ReceiverStream::new(rx)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))).into_response()
}

async fn send_streaming(s: &AppState, params: &Value, legacy_shape: bool, id: &Value) -> Response {
    let p = match prepare(s, params, id) {
        Ok(p) => p,
        Err(e) => return Json(e).into_response(),
    };
    let art_id = new_id("art");
    let state = s.clone();
    let (tid, agent, text, session, title, art) = (p.task_id.clone(), p.agent, p.text, p.session.clone(), p.title, art_id.clone());
    // The stream subscribes to the event bus before the run starts (see `stream_task`).
    let response = stream_task(s.clone(), s.engine.subscribe(), id.clone(), p.task_id, p.session, art_id, legacy_shape);
    tokio::spawn(async move {
        let r = state.engine.run_agent_task(&agent, &text, Some(&session), &title).await;
        finish(&state, &tid, Some(&art), r);
    });
    response
}

fn subscribe(s: &AppState, params: &Value, legacy_shape: bool, id: &Value) -> Response {
    let tid = params["id"].as_str().unwrap_or_default().to_string();
    match s.engine.a2a_task(&tid) {
        Ok(Some((task, session))) => {
            if is_terminal(task["status"]["state"].as_str().unwrap_or_default()) {
                return Json(rpc_error(id, -32004, "il task è già terminato: leggilo con GetTask")).into_response();
            }
            stream_task(s.clone(), s.engine.subscribe(), id.clone(), tid, session.unwrap_or_default(), new_id("art"), legacy_shape)
        }
        Ok(None) => Json(rpc_error(id, -32001, format!("task non trovato: {tid}"))).into_response(),
        Err(e) => Json(rpc_error(id, -32603, e.to_string())).into_response(),
    }
}

/// `POST /a2a` (JSON-RPC 2.0).
pub async fn handle(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let method = body["method"].as_str().unwrap_or_default();
    let params = &body["params"];
    let legacy_shape = method.contains('/');
    match method {
        "SendStreamingMessage" | "message/stream" => return send_streaming(&s, params, legacy_shape, &id).await,
        "SubscribeToTask" | "tasks/resubscribe" => return subscribe(&s, params, legacy_shape, &id),
        _ => {}
    }
    Json(match method {
        "SendMessage" | "message/send" => send_message(&s, params, legacy_shape, &id).await,
        "GetTask" | "tasks/get" => {
            let tid = params["id"].as_str().unwrap_or_default();
            match s.engine.a2a_task(tid) {
                Ok(Some((task, _))) => json!({"jsonrpc": "2.0", "id": id, "result": if legacy_shape { legacy(&task) } else { task }}),
                Ok(None) => rpc_error(&id, -32001, format!("task non trovato: {tid}")),
                Err(e) => rpc_error(&id, -32603, e.to_string()),
            }
        }
        "CancelTask" | "tasks/cancel" => {
            let tid = params["id"].as_str().unwrap_or_default().to_string();
            // The state change is atomic with respect to the end of the run: whichever comes first wins.
            let changed = s.engine.a2a_update_task(&tid, |task, session| {
                if task["status"]["state"] != "TASK_STATE_WORKING" {
                    return Err(());
                }
                task["status"] = json!({"state": "TASK_STATE_CANCELED", "timestamp": now()});
                Ok((task.clone(), session.clone()))
            });
            match changed {
                Err(e) => rpc_error(&id, -32603, e.to_string()),
                Ok(None) => rpc_error(&id, -32001, format!("task non trovato: {tid}")),
                Ok(Some(Err(()))) => rpc_error(&id, -32002, "il task non è più annullabile"),
                Ok(Some(Ok((task, session)))) => {
                    if let Some(run) = session.and_then(|sid| s.engine.active_run(&sid)) {
                        let _ = s.engine.cancel_run(&run);
                    }
                    json!({"jsonrpc": "2.0", "id": id, "result": if legacy_shape { legacy(&task) } else { task }})
                }
            }
        }
        "" => rpc_error(&id, -32600, "richiesta JSON-RPC non valida"),
        other => rpc_error(&id, -32601, format!("metodo non supportato: {other}")),
    })
    .into_response()
}
