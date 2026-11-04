//! MCP server (streamable HTTP, JSON responses) at `/mcp`: other agents — editors, CLI coding
//! agents, other AIW devices — can use this workspace's tools and delegate tasks to its local
//! agents. Every call goes through the workspace policy, the approval inbox and the audit trail.

use crate::extract::Json;
use crate::AppState;
use aiw_core::Error;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const AGENT_TOOL: &str = "aiw_agent_run";

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn rpc_error(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn tools_list(s: &AppState) -> Value {
    let mut tools: Vec<Value> = s
        .engine
        .tools()
        .into_iter()
        .filter(|t| t.enabled && t.available && t.source == "builtin")
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.parameters,
                "annotations": {"readOnlyHint": t.risk == "read", "destructiveHint": matches!(t.risk.as_str(), "write" | "exec" | "computer"), "openWorldHint": t.risk == "network"}
            })
        })
        .collect();
    let agents: Vec<Value> = s.engine.agents().unwrap_or_default().into_iter().map(|a| json!(a.id)).collect();
    tools.push(json!({
        "name": AGENT_TOOL,
        "description": "Delega un compito a un agente locale di AI Workspace (modelli on-device) e restituisce la sua risposta finale.",
        "inputSchema": {"type": "object", "properties": {"agent": {"type": "string", "enum": agents}, "task": {"type": "string"}}, "required": ["agent", "task"]},
        "annotations": {"readOnlyHint": false, "destructiveHint": true}
    }));
    json!({ "tools": tools })
}

async fn tools_call(s: &AppState, params: &Value, client: &str) -> Result<Value, Error> {
    let name = params["name"].as_str().ok_or_else(|| Error::Invalid("`name` mancante".into()))?;
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let run_id = aiw_core::ids::new_id("mcp");
    if name == AGENT_TOOL {
        let agent = args["agent"].as_str().unwrap_or("operator");
        let task = args["task"].as_str().ok_or_else(|| Error::Invalid("`task` mancante".into()))?;
        let (text, session) = s.engine.run_agent_task(agent, task, None, &format!("MCP · {client}")).await?;
        return Ok(json!({"content": [{"type": "text", "text": text}], "structuredContent": {"session_id": session}, "isError": false}));
    }
    let out = s.engine.execute_tool(&run_id, "agent", &format!("Client MCP {client}"), name, args).await?;
    let mut content = vec![json!({"type": "text", "text": out.content})];
    for img in out.images {
        if let Some((meta, data)) = img.split_once(";base64,") {
            content.push(json!({"type": "image", "mimeType": meta.trim_start_matches("data:"), "data": data}));
        }
    }
    Ok(json!({"content": content, "isError": out.is_error}))
}

/// Handles one JSON-RPC message (or a batch).
pub async fn handle(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let client = headers.get("user-agent").and_then(|h| h.to_str().ok()).unwrap_or("client").chars().take(40).collect::<String>();
    let msgs: Vec<Value> = if let Value::Array(a) = &body { a.clone() } else { vec![body.clone()] };
    let mut replies = Vec::new();
    for m in msgs {
        let id = m.get("id").cloned();
        let method = m["method"].as_str().unwrap_or_default();
        let Some(id) = id else {
            continue; // notification (e.g. notifications/initialized): nothing to answer
        };
        let reply = match method {
            "initialize" => rpc_result(
                &id,
                json!({
                    "protocolVersion": m["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL_VERSION),
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "ai-workspace-local", "title": "AI Workspace Local", "version": aiw_core::VERSION},
                    "instructions": "Tool del workspace locale (file, shell in sandbox, web, browser) e agenti on-device. Le azioni con effetti collaterali possono richiedere la conferma dell'utente nell'app."
                }),
            ),
            "ping" => rpc_result(&id, json!({})),
            "tools/list" => rpc_result(&id, tools_list(&s)),
            "tools/call" => match tools_call(&s, &m["params"], &client).await {
                Ok(r) => rpc_result(&id, r),
                // Tool-level failures are results with isError, so the calling model can react.
                Err(e @ (Error::Denied(_) | Error::Runtime(_) | Error::Unsupported(_) | Error::Invalid(_))) => {
                    rpc_result(&id, json!({"content": [{"type": "text", "text": e.to_string()}], "isError": true}))
                }
                Err(e @ Error::NotFound(_)) => rpc_error(&id, -32602, e.to_string()),
                Err(e) => rpc_error(&id, -32603, e.to_string()),
            },
            "resources/list" => rpc_result(&id, json!({"resources": []})),
            "prompts/list" => rpc_result(&id, json!({"prompts": []})),
            other => rpc_error(&id, -32601, format!("metodo non supportato: {other}")),
        };
        replies.push(reply);
    }
    if replies.is_empty() {
        return StatusCode::ACCEPTED.into_response();
    }
    let body = if matches!(body, Value::Array(_)) { Value::Array(replies) } else { replies.remove(0) };
    ([("mcp-session-id", "aiw")], Json(body)).into_response()
}

/// Streamable HTTP clients may open a GET stream for server-initiated messages; we have none.
pub async fn get_stream() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}
