//! OpenAI-compatible gateway: lets any app (editors, Claude Code via a proxy, Codex `--oss`,
//! OpenCode, CopilotKit…) use the local runtimes through `http://<device>:7777/v1`.

use crate::extract::Json;
use crate::{ApiError, AppState};
use aiw_runtimes::{ChatDelta, ChatRequest, Msg, ToolSpec};
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

pub async fn models(State(s): State<AppState>) -> Json<Value> {
    let data: Vec<Value> =
        s.engine.gateway_models().into_iter().map(|id| json!({"id": id, "object": "model", "owned_by": "ai-workspace-local"})).collect();
    Json(json!({"object": "list", "data": data}))
}

#[derive(Deserialize)]
pub struct CompletionBody {
    #[serde(default)]
    model: String,
    messages: Vec<Msg>,
    #[serde(default)]
    tools: Vec<ToolSpec>,
    #[serde(default)]
    stream: bool,
    temperature: Option<f64>,
    top_p: Option<f64>,
    max_tokens: Option<u32>,
    max_completion_tokens: Option<u32>,
    #[serde(default)]
    stop: Option<Value>,
}

fn chunk(id: &str, model: &str, delta: Value, finish: Option<&str>) -> Value {
    json!({"id": id, "object": "chat.completion.chunk", "created": chrono::Utc::now().timestamp(), "model": model, "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]})
}

pub async fn chat_completions(State(s): State<AppState>, Json(b): Json<CompletionBody>) -> Result<Response, ApiError> {
    let (backend, model) = s.engine.gateway_backend(&b.model).await?;
    let stop = match b.stop {
        Some(Value::String(x)) => vec![x],
        Some(Value::Array(a)) => a.into_iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        _ => vec![],
    };
    let req = ChatRequest {
        model: model.clone(),
        messages: b.messages,
        tools: b.tools,
        temperature: b.temperature,
        top_p: b.top_p,
        max_tokens: b.max_tokens.or(b.max_completion_tokens),
        stop,
        thinking: None,
    };
    let mut stream = backend.chat(req, CancellationToken::new()).await?;
    let id = format!("chatcmpl-{}", aiw_core::ids::new_id("gw"));
    let shown_model = if b.model.is_empty() { model } else { b.model };
    if b.stream {
        let (id2, m2) = (id.clone(), shown_model.clone());
        let mut started = false;
        let events = stream
            .map(move |d| {
                let mut out = Vec::new();
                if !started {
                    started = true;
                    out.push(chunk(&id2, &m2, json!({"role": "assistant", "content": ""}), None));
                }
                match d {
                    Ok(ChatDelta::Text(t)) => out.push(chunk(&id2, &m2, json!({"content": t}), None)),
                    Ok(ChatDelta::Reasoning(r)) => out.push(chunk(&id2, &m2, json!({"reasoning_content": r}), None)),
                    Ok(ChatDelta::ToolCall { index, id, name, arguments }) => {
                        let mut f = json!({"arguments": arguments});
                        if let Some(n) = name {
                            f["name"] = json!(n);
                        }
                        let mut tc = json!({"index": index, "type": "function", "function": f});
                        if let Some(i) = id {
                            tc["id"] = json!(i);
                        }
                        out.push(chunk(&id2, &m2, json!({"tool_calls": [tc]}), None));
                    }
                    Ok(ChatDelta::Usage(u)) => {
                        let mut c = chunk(&id2, &m2, json!({}), None);
                        c["choices"] = json!([]);
                        c["usage"] = json!({"prompt_tokens": u.prompt_tokens, "completion_tokens": u.completion_tokens, "total_tokens": u.prompt_tokens + u.completion_tokens});
                        out.push(c);
                    }
                    Ok(ChatDelta::Done { finish_reason }) => out.push(chunk(&id2, &m2, json!({}), Some(finish_reason.as_deref().unwrap_or("stop")))),
                    Err(e) => out.push(json!({"error": {"message": e.to_string()}})),
                }
                futures::stream::iter(out.into_iter().map(|v| Ok::<_, std::convert::Infallible>(Event::default().data(v.to_string()))))
            })
            .flatten()
            .chain(futures::stream::once(async { Ok(Event::default().data("[DONE]")) }));
        return Ok(Sse::new(events).keep_alive(KeepAlive::default()).into_response());
    }
    // Non-streaming: accumulate.
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls: BTreeMap<usize, (String, String, String)> = BTreeMap::new();
    let mut usage = json!(null);
    let mut finish = "stop".to_string();
    while let Some(d) = stream.next().await {
        match d? {
            ChatDelta::Text(t) => text.push_str(&t),
            ChatDelta::Reasoning(r) => reasoning.push_str(&r),
            ChatDelta::ToolCall { index, id, name, arguments } => {
                let e = calls.entry(index).or_default();
                if let Some(i) = id {
                    e.0 = i;
                }
                if let Some(n) = name {
                    e.1.push_str(&n);
                }
                e.2.push_str(&arguments);
            }
            ChatDelta::Usage(u) => {
                usage = json!({"prompt_tokens": u.prompt_tokens, "completion_tokens": u.completion_tokens, "total_tokens": u.prompt_tokens + u.completion_tokens})
            }
            ChatDelta::Done { finish_reason } => finish = finish_reason.unwrap_or(finish),
        }
    }
    let mut message = json!({"role": "assistant", "content": text});
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    if !calls.is_empty() {
        message["tool_calls"] = json!(calls.into_values().enumerate().map(|(i, (id, name, args))| json!({"id": if id.is_empty() { format!("call_{i}") } else { id }, "type": "function", "function": {"name": name, "arguments": args}})).collect::<Vec<_>>());
    }
    Ok(Json(json!({"id": id, "object": "chat.completion", "created": chrono::Utc::now().timestamp(), "model": shown_model, "choices": [{"index": 0, "message": message, "finish_reason": finish}], "usage": usage})).into_response())
}
