//! Anthropic Messages-compatible gateway (`POST /v1/messages`, `/v1/messages/count_tokens`).
//! Any Anthropic client — including coding agents pointed at `ANTHROPIC_BASE_URL` — can run on
//! the workspace's local runtimes. Requests are translated to the OpenAI-style [`ChatRequest`]
//! used by every adapter; streamed deltas are re-encoded as Anthropic SSE events.

use crate::extract::Json;
use crate::{ApiError, AppState};
use aiw_core::Error;
use aiw_runtimes::{ChatDelta, ChatRequest, ContentPart, ImageUrl, Msg, MsgContent, OaiFunctionCall, OaiToolCall, ToolSpec};
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// Translates an Anthropic Messages request into a [`ChatRequest`].
pub fn to_chat_request(body: &Value, model: &str) -> Result<ChatRequest, Error> {
    let mut messages = Vec::new();
    let system = text_of(&body["system"]);
    if !system.is_empty() {
        messages.push(Msg::system(system));
    }
    let mut tool_names = std::collections::HashMap::new();
    for m in body["messages"].as_array().ok_or_else(|| Error::Invalid("`messages` mancante".into()))? {
        let role = m["role"].as_str().unwrap_or("user");
        let blocks: Vec<Value> = match &m["content"] {
            Value::String(s) => vec![json!({"type": "text", "text": s})],
            Value::Array(a) => a.clone(),
            _ => vec![],
        };
        if role == "assistant" {
            let text: Vec<&str> = blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect();
            let calls: Vec<OaiToolCall> = blocks
                .iter()
                .filter(|b| b["type"] == "tool_use")
                .map(|b| {
                    let id = b["id"].as_str().unwrap_or_default().to_string();
                    let name = b["name"].as_str().unwrap_or_default().to_string();
                    tool_names.insert(id.clone(), name.clone());
                    OaiToolCall { id, kind: "function".into(), function: OaiFunctionCall { name, arguments: b["input"].to_string() } }
                })
                .collect();
            messages.push(Msg {
                role: "assistant".into(),
                content: (!text.is_empty()).then(|| MsgContent::Text(text.join("\n"))),
                tool_calls: (!calls.is_empty()).then_some(calls),
                ..Default::default()
            });
            continue;
        }
        // user turn: tool results become `tool` messages, the rest a user message
        let mut parts = Vec::new();
        for b in &blocks {
            match b["type"].as_str() {
                Some("tool_result") => {
                    let id = b["tool_use_id"].as_str().unwrap_or_default();
                    let mut content = text_of(&b["content"]);
                    if b["is_error"].as_bool().unwrap_or(false) {
                        content = format!("ERRORE: {content}");
                    }
                    messages.push(Msg::tool(id, tool_names.get(id).cloned().unwrap_or_default(), content));
                }
                Some("text") => parts.push(ContentPart::Text { text: b["text"].as_str().unwrap_or_default().to_string() }),
                Some("image") => {
                    let src = &b["source"];
                    let url = match src["type"].as_str() {
                        Some("base64") => format!(
                            "data:{};base64,{}",
                            src["media_type"].as_str().unwrap_or("image/png"),
                            src["data"].as_str().unwrap_or_default()
                        ),
                        _ => src["url"].as_str().unwrap_or_default().to_string(),
                    };
                    parts.push(ContentPart::ImageUrl { image_url: ImageUrl { url } });
                }
                _ => {}
            }
        }
        if !parts.is_empty() {
            let only_text = parts.iter().all(|p| matches!(p, ContentPart::Text { .. }));
            let content = if only_text {
                MsgContent::Text(
                    parts
                        .iter()
                        .filter_map(|p| if let ContentPart::Text { text } = p { Some(text.as_str()) } else { None })
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            } else {
                MsgContent::Parts(parts)
            };
            messages.push(Msg { role: "user".into(), content: Some(content), ..Default::default() });
        }
    }
    let tools = body["tools"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|t| t["name"].is_string() && t.get("input_schema").is_some())
                .map(|t| {
                    ToolSpec::function(
                        t["name"].as_str().unwrap(),
                        t["description"].as_str().unwrap_or_default(),
                        t["input_schema"].clone(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ChatRequest {
        model: model.to_string(),
        messages,
        tools,
        temperature: body["temperature"].as_f64(),
        top_p: body["top_p"].as_f64(),
        max_tokens: body["max_tokens"].as_u64().map(|v| v as u32),
        stop: body["stop_sequences"]
            .as_array()
            .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
            .unwrap_or_default(),
    })
}

fn stop_reason(finish: Option<&str>, had_tools: bool) -> &'static str {
    match finish {
        _ if had_tools => "tool_use",
        Some("length") => "max_tokens",
        Some("stop_sequence") => "stop_sequence",
        _ => "end_turn",
    }
}

/// Incremental encoder from [`ChatDelta`] to Anthropic stream events (`(event, data)` pairs).
#[derive(Default)]
pub struct StreamEncoder {
    index: i64,
    open: Option<&'static str>,
    tool_blocks: std::collections::HashMap<usize, i64>,
    had_tools: bool,
    output_tokens: u64,
    pub input_tokens: u64,
    finish: Option<String>,
}

impl StreamEncoder {
    fn close(&mut self, out: &mut Vec<(String, Value)>) {
        if self.open.take().is_some() {
            out.push(("content_block_stop".into(), json!({"type": "content_block_stop", "index": self.index})));
        }
    }
    fn open_block(&mut self, kind: &'static str, block: Value, out: &mut Vec<(String, Value)>) {
        self.close(out);
        self.index += 1;
        self.open = Some(kind);
        out.push(("content_block_start".into(), json!({"type": "content_block_start", "index": self.index, "content_block": block})));
    }

    pub fn start(&mut self, id: &str, model: &str) -> Vec<(String, Value)> {
        self.index = -1;
        vec![(
            "message_start".into(),
            json!({"type": "message_start", "message": {"id": id, "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": self.input_tokens, "output_tokens": 0}}}),
        )]
    }

    pub fn push(&mut self, d: ChatDelta) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        match d {
            ChatDelta::Reasoning(r) => {
                if self.open != Some("thinking") {
                    self.open_block("thinking", json!({"type": "thinking", "thinking": ""}), &mut out);
                }
                out.push((
                    "content_block_delta".into(),
                    json!({"type": "content_block_delta", "index": self.index, "delta": {"type": "thinking_delta", "thinking": r}}),
                ));
            }
            ChatDelta::Text(t) => {
                if self.open != Some("text") {
                    self.open_block("text", json!({"type": "text", "text": ""}), &mut out);
                }
                out.push((
                    "content_block_delta".into(),
                    json!({"type": "content_block_delta", "index": self.index, "delta": {"type": "text_delta", "text": t}}),
                ));
            }
            ChatDelta::ToolCall { index, id, name, arguments } => {
                if !self.tool_blocks.contains_key(&index) {
                    let id = id.unwrap_or_else(|| format!("toolu_{}", aiw_core::ids::new_id("t")));
                    self.open_block(
                        "tool_use",
                        json!({"type": "tool_use", "id": id, "name": name.unwrap_or_default(), "input": {}}),
                        &mut out,
                    );
                    self.tool_blocks.insert(index, self.index);
                    self.had_tools = true;
                }
                if !arguments.is_empty() {
                    let block = self.tool_blocks[&index];
                    out.push(("content_block_delta".into(), json!({"type": "content_block_delta", "index": block, "delta": {"type": "input_json_delta", "partial_json": arguments}})));
                }
            }
            ChatDelta::Usage(u) => {
                self.input_tokens = u.prompt_tokens.max(self.input_tokens);
                self.output_tokens = u.completion_tokens;
            }
            ChatDelta::Done { finish_reason } => self.finish = finish_reason,
        }
        out
    }

    pub fn finish(&mut self) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        self.close(&mut out);
        out.push((
            "message_delta".into(),
            json!({"type": "message_delta", "delta": {"stop_reason": stop_reason(self.finish.as_deref(), self.had_tools), "stop_sequence": null}, "usage": {"output_tokens": self.output_tokens}}),
        ));
        out.push(("message_stop".into(), json!({"type": "message_stop"})));
        out
    }
}

pub fn estimate_tokens(body: &Value) -> u64 {
    (body.to_string().len() as u64 / 4).max(1)
}

pub async fn messages(State(s): State<AppState>, Json(body): Json<Value>) -> Result<Response, ApiError> {
    let requested = body["model"].as_str().unwrap_or_default().to_string();
    let (backend, model) = s.engine.gateway_backend(&requested).await?;
    let req = to_chat_request(&body, &model)?;
    let mut stream = backend.chat(req, CancellationToken::new()).await?;
    let id = format!("msg_{}", aiw_core::ids::new_id("aiw"));
    let shown = if requested.is_empty() { model } else { requested };
    if body["stream"].as_bool().unwrap_or(false) {
        let mut enc = StreamEncoder { input_tokens: estimate_tokens(&body["messages"]), ..Default::default() };
        let first = enc.start(&id, &shown);
        let events = async_stream_events(first, stream, enc);
        return Ok(Sse::new(events).keep_alive(KeepAlive::default()).into_response());
    }
    let mut content: Vec<Value> = Vec::new();
    let mut tool_args: std::collections::BTreeMap<usize, (String, String, String)> = Default::default();
    let (mut usage_in, mut usage_out, mut finish) = (estimate_tokens(&body["messages"]), 0, None);
    let mut text = String::new();
    let mut thinking = String::new();
    while let Some(d) = stream.next().await {
        match d? {
            ChatDelta::Text(t) => text.push_str(&t),
            ChatDelta::Reasoning(r) => thinking.push_str(&r),
            ChatDelta::ToolCall { index, id, name, arguments } => {
                let e = tool_args.entry(index).or_default();
                if let Some(i) = id {
                    e.0 = i;
                }
                if let Some(n) = name {
                    e.1.push_str(&n);
                }
                e.2.push_str(&arguments);
            }
            ChatDelta::Usage(u) => {
                usage_in = u.prompt_tokens.max(usage_in);
                usage_out = u.completion_tokens;
            }
            ChatDelta::Done { finish_reason } => finish = finish_reason,
        }
    }
    if !thinking.is_empty() {
        content.push(json!({"type": "thinking", "thinking": thinking, "signature": ""}));
    }
    if !text.is_empty() {
        content.push(json!({"type": "text", "text": text}));
    }
    let had_tools = !tool_args.is_empty();
    for (i, (tid, name, args)) in tool_args.into_values().enumerate() {
        let input: Value = serde_json::from_str(if args.trim().is_empty() { "{}" } else { &args }).unwrap_or(json!({}));
        content
            .push(json!({"type": "tool_use", "id": if tid.is_empty() { format!("toolu_{i}") } else { tid }, "name": name, "input": input}));
    }
    Ok(Json(json!({
        "id": id, "type": "message", "role": "assistant", "model": shown, "content": content,
        "stop_reason": stop_reason(finish.as_deref(), had_tools), "stop_sequence": null,
        "usage": {"input_tokens": usage_in, "output_tokens": usage_out}
    }))
    .into_response())
}

fn async_stream_events(
    first: Vec<(String, Value)>,
    stream: aiw_runtimes::DeltaStream,
    enc: StreamEncoder,
) -> impl futures::Stream<Item = Result<Event, std::convert::Infallible>> + Send {
    let to_ev = |(name, data): (String, Value)| Ok(Event::default().event(name).data(data.to_string()));
    let head = futures::stream::iter(first.into_iter().map(to_ev));
    let body = futures::stream::unfold((stream, enc, false), move |(mut stream, mut enc, done)| async move {
        if done {
            return None;
        }
        match stream.next().await {
            Some(Ok(d)) => Some((enc.push(d), (stream, enc, false))),
            Some(Err(e)) => {
                let mut v = vec![("error".to_string(), json!({"type": "error", "error": {"type": "api_error", "message": e.to_string()}}))];
                v.extend(enc.finish());
                Some((v, (stream, enc, true)))
            }
            None => Some((enc.finish(), (stream, enc, true))),
        }
    })
    .flat_map(move |v| futures::stream::iter(v.into_iter().map(to_ev)));
    head.chain(body)
}

pub async fn count_tokens(Json(body): Json<Value>) -> Json<Value> {
    Json(json!({"input_tokens": estimate_tokens(&json!([body["system"], body["messages"], body["tools"]]))}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_tool_use_and_results() {
        let body = json!({
            "model": "claude-x", "max_tokens": 100, "system": [{"type": "text", "text": "Sei utile"}],
            "tools": [{"name": "list_dir", "description": "Elenca", "input_schema": {"type": "object"}}],
            "messages": [
                {"role": "user", "content": "Cosa c'è?"},
                {"role": "assistant", "content": [{"type": "text", "text": "Guardo"}, {"type": "tool_use", "id": "toolu_1", "name": "list_dir", "input": {"path": "."}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": [{"type": "text", "text": "a.txt"}]}, {"type": "text", "text": "e quindi?"}]}
            ]
        });
        let r = to_chat_request(&body, "local").unwrap();
        let roles: Vec<&str> = r.messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["system", "user", "assistant", "tool", "user"]);
        assert_eq!(r.messages[2].tool_calls.as_ref().unwrap()[0].function.arguments, "{\"path\":\".\"}");
        assert_eq!(r.messages[3].name.as_deref(), Some("list_dir"));
        assert_eq!(r.tools[0].function.name, "list_dir");
        assert_eq!(r.max_tokens, Some(100));
    }

    #[test]
    fn encodes_stream_in_anthropic_order() {
        let mut e = StreamEncoder::default();
        let mut evs = e.start("msg_1", "m");
        for d in [
            ChatDelta::Reasoning("penso".into()),
            ChatDelta::Text("Ciao".into()),
            ChatDelta::ToolCall { index: 0, id: Some("c1".into()), name: Some("read_file".into()), arguments: "{\"pa".into() },
            ChatDelta::ToolCall { index: 0, id: None, name: None, arguments: "th\":\"x\"}".into() },
            ChatDelta::Done { finish_reason: Some("tool_calls".into()) },
        ] {
            evs.extend(e.push(d));
        }
        evs.extend(e.finish());
        let names: Vec<&str> = evs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop"
            ]
        );
        assert_eq!(evs[7].1["content_block"]["type"], "tool_use");
        assert_eq!(evs[7].1["index"], 2);
        assert_eq!(evs[11].1["delta"]["stop_reason"], "tool_use");
    }
}
