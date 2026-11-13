//! OpenAI-compatible gateway: lets any app (editors, Claude Code via a proxy, Codex `--oss`,
//! OpenCode, CopilotKit…) use the local runtimes through `http://<device>:7777/v1`.

use crate::extract::Json;
use crate::{ApiError, AppState};
use aiw_runtimes::{ChatDelta, ChatRequest, Msg, ToolSpec};
use axum::extract::State;
use axum::http::StatusCode;
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

/// Most texts in one embeddings request, and most characters across all of them.
const MAX_EMBED_INPUTS: usize = 256;
const MAX_EMBED_CHARS: usize = 1_000_000;

#[derive(Deserialize)]
pub struct EmbeddingsBody {
    #[serde(default)]
    model: String,
    input: Value,
    /// `float` (default) or `base64` (little-endian f32, what the OpenAI Python client asks for).
    #[serde(default)]
    encoding_format: Option<String>,
}

fn invalid(msg: &str) -> ApiError {
    ApiError(aiw_core::Error::Invalid(msg.into()))
}

/// The texts of an `input`: a string or a list of strings.
fn embedding_inputs(input: Value) -> Result<Vec<String>, ApiError> {
    let texts: Vec<String> = match input {
        Value::String(s) => vec![s],
        Value::Array(items) => items
            .into_iter()
            .map(|v| match v {
                Value::String(s) => Ok(s),
                _ => Err(invalid("`input` deve essere un testo o una lista di testi (i token già calcolati non sono supportati)")),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(invalid("`input` deve essere un testo o una lista di testi")),
    };
    if texts.is_empty() || texts.iter().any(|t| t.is_empty()) {
        return Err(invalid("`input` non può essere vuoto"));
    }
    if texts.len() > MAX_EMBED_INPUTS {
        return Err(invalid(&format!("al massimo {MAX_EMBED_INPUTS} testi per richiesta")));
    }
    if texts.iter().map(|t| t.chars().count()).sum::<usize>() > MAX_EMBED_CHARS {
        return Err(invalid("i testi sono troppo lunghi: al massimo un milione di caratteri per richiesta"));
    }
    Ok(texts)
}

fn f32_base64(v: &[f32]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>())
}

/// `POST /v1/embeddings` (OpenAI-compatible). Uses the same embedding model as the memory and the
/// knowledge base (the adviser's choice on a runtime that is already running); a profile or an
/// installed embedding model can be named in `model`. It is answered by this device only: like the
/// chat completions it is never forwarded to a paired device.
pub async fn embeddings(State(s): State<AppState>, Json(b): Json<EmbeddingsBody>) -> Result<Response, ApiError> {
    let base64_out = match b.encoding_format.as_deref() {
        None | Some("float") => false,
        Some("base64") => true,
        Some(_) => return Err(invalid("`encoding_format` può essere float o base64")),
    };
    let texts = embedding_inputs(b.input)?;
    let Some((model, vectors)) = s.engine.embed_texts(&b.model, &texts).await? else {
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Nessun modello di embedding è pronto: scarica un modello di embedding in Modelli e tienilo acceso, poi riprova."})),
        )
            .into_response());
    };
    let data: Vec<Value> = vectors
        .iter()
        .enumerate()
        .map(|(i, v)| json!({"object": "embedding", "index": i, "embedding": if base64_out { json!(f32_base64(v)) } else { json!(v) }}))
        .collect();
    // The runtimes do not report token counts for embeddings: an estimate (about 4 characters a token).
    let tokens: usize = texts.iter().map(|t| t.chars().count().div_ceil(4).max(1)).sum();
    let shown = if b.model.is_empty() { model } else { b.model };
    Ok(Json(json!({"object": "list", "data": data, "model": shown, "usage": {"prompt_tokens": tokens, "total_tokens": tokens}}))
        .into_response())
}
