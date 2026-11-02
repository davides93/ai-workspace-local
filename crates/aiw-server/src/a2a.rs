//! A2A (Agent2Agent) 1.0, JSON-RPC binding: other agents (another AIW device, an orchestrator,
//! a cloud agent the user runs) discover this device's agents through the Agent Card at
//! `/.well-known/agent-card.json` and hand them tasks with `SendMessage` on `/a2a`.
//!
//! Each `contextId` maps to one local session, so a conversation continues where it left off.
//! Every action still goes through the local policy, approvals and audit. The 0.3 method names
//! (`message/send`, `tasks/get`, `tasks/cancel`) are accepted too, with 0.3-shaped results.

use crate::AppState;
use aiw_core::ids::new_id;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const PROTOCOL_VERSION: &str = "1.0";

struct TaskEntry {
    task: Value,
    session: Option<String>,
}

fn tasks() -> &'static Mutex<HashMap<String, TaskEntry>> {
    static T: OnceLock<Mutex<HashMap<String, TaskEntry>>> = OnceLock::new();
    T.get_or_init(Default::default)
}

fn contexts() -> &'static Mutex<HashMap<String, String>> {
    static C: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

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
        "capabilities": {"streaming": false, "pushNotifications": false},
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

fn finish(task_id: &str, result: aiw_core::Result<(String, String)>) {
    let mut all = tasks().lock().unwrap();
    let Some(entry) = all.get_mut(task_id) else { return };
    if entry.task["status"]["state"] == "TASK_STATE_CANCELED" {
        return;
    }
    let context = entry.task["contextId"].as_str().unwrap_or_default().to_string();
    match result {
        Ok((answer, session)) => {
            contexts().lock().unwrap().insert(context.clone(), session.clone());
            entry.session = Some(session);
            let agent_msg = json!({"messageId": new_id("msg"), "role": "ROLE_AGENT", "parts": [{"text": answer}], "contextId": context, "taskId": task_id});
            entry.task["artifacts"] = json!([{"artifactId": new_id("art"), "name": "risposta", "parts": [{"text": answer}]}]);
            if let Some(h) = entry.task["history"].as_array_mut() {
                h.push(agent_msg);
            }
            entry.task["status"] = json!({"state": "TASK_STATE_COMPLETED", "timestamp": now()});
        }
        Err(e) => {
            entry.task["status"] = json!({
                "state": "TASK_STATE_FAILED",
                "timestamp": now(),
                "message": {"messageId": new_id("msg"), "role": "ROLE_AGENT", "parts": [{"text": e.to_string()}], "contextId": context, "taskId": task_id}
            });
        }
    }
}

async fn send_message(s: &AppState, params: &Value, legacy_shape: bool, id: &Value) -> Value {
    let message = &params["message"];
    let text = text_of(message);
    if text.trim().is_empty() {
        return rpc_error(id, -32602, "il messaggio non contiene parti di testo");
    }
    let agent = params["metadata"]["agent"].as_str().or_else(|| message["metadata"]["agent"].as_str()).unwrap_or("operator").to_string();
    if s.engine.agent(&agent).is_err() {
        return rpc_error(id, -32602, format!("agente sconosciuto: {agent}"));
    }
    let context = message["contextId"].as_str().filter(|c| !c.is_empty()).map(String::from).unwrap_or_else(|| new_id("ctx"));
    let title = format!("A2A · {}", text.chars().take(40).collect::<String>());
    // One local session per context, created now so that the task can be cancelled at once.
    let known = contexts().lock().unwrap().get(&context).cloned();
    let session = match known {
        Some(sid) => sid,
        None => match s.engine.create_session(&agent, Some(title.clone())) {
            Ok(created) => {
                contexts().lock().unwrap().insert(context.clone(), created.id.clone());
                created.id
            }
            Err(e) => return rpc_error(id, -32603, e.to_string()),
        },
    };
    let task_id = new_id("task");
    let user_msg = json!({
        "messageId": message["messageId"].as_str().map(String::from).unwrap_or_else(|| new_id("msg")),
        "role": "ROLE_USER", "parts": [{"text": text}], "contextId": context, "taskId": task_id
    });
    let task = json!({"id": task_id, "contextId": context, "status": {"state": "TASK_STATE_WORKING", "timestamp": now()}, "artifacts": [], "history": [user_msg]});
    tasks().lock().unwrap().insert(task_id.clone(), TaskEntry { task, session: Some(session.clone()) });
    let blocking = params["configuration"]["blocking"].as_bool() != Some(false)
        && params["configuration"]["returnImmediately"].as_bool() != Some(true);
    let engine = s.engine.clone();
    let tid = task_id.clone();
    let work = async move {
        let r = engine.run_agent_task(&agent, &text, Some(&session), &title).await;
        finish(&tid, r);
    };
    if blocking {
        work.await;
    } else {
        tokio::spawn(work);
    }
    let task = tasks().lock().unwrap().get(&task_id).map(|e| e.task.clone()).unwrap_or(Value::Null);
    json!({"jsonrpc": "2.0", "id": id, "result": wrap(&task, legacy_shape)})
}

/// `POST /a2a` (JSON-RPC 2.0).
pub async fn handle(State(s): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let method = body["method"].as_str().unwrap_or_default();
    let params = &body["params"];
    let legacy_shape = method.contains('/');
    Json(match method {
        "SendMessage" | "message/send" => send_message(&s, params, legacy_shape, &id).await,
        "GetTask" | "tasks/get" => {
            let tid = params["id"].as_str().unwrap_or_default();
            match tasks().lock().unwrap().get(tid) {
                Some(e) => json!({"jsonrpc": "2.0", "id": id, "result": if legacy_shape { legacy(&e.task) } else { e.task.clone() }}),
                None => rpc_error(&id, -32001, format!("task non trovato: {tid}")),
            }
        }
        "CancelTask" | "tasks/cancel" => {
            let tid = params["id"].as_str().unwrap_or_default().to_string();
            let entry = tasks().lock().unwrap().get(&tid).map(|e| (e.task["status"]["state"].clone(), e.session.clone()));
            match entry {
                None => rpc_error(&id, -32001, format!("task non trovato: {tid}")),
                Some((state, _)) if state != "TASK_STATE_WORKING" => rpc_error(&id, -32002, "il task non è più annullabile"),
                Some((_, session)) => {
                    if let Some(run) = session.and_then(|sid| s.engine.active_run(&sid)) {
                        let _ = s.engine.cancel_run(&run);
                    }
                    let mut all = tasks().lock().unwrap();
                    let e = all.get_mut(&tid).expect("task present");
                    e.task["status"] = json!({"state": "TASK_STATE_CANCELED", "timestamp": now()});
                    json!({"jsonrpc": "2.0", "id": id, "result": if legacy_shape { legacy(&e.task) } else { e.task.clone() }})
                }
            }
        }
        "" => rpc_error(&id, -32600, "richiesta JSON-RPC non valida"),
        other => rpc_error(&id, -32601, format!("metodo non supportato: {other}")),
    })
}
