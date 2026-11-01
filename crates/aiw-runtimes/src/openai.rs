//! OpenAI-compatible adapter (`api: "openai"`): llama-server, vLLM, SGLang, mlx-lm, TensorRT-LLM,
//! Foundry Local, Lemonade, LM Studio, LocalAI, Ollama's /v1, paired AIW devices…

use crate::types::*;
use aiw_core::{Error, Result};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct OpenAiBackend {
    base: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

impl OpenAiBackend {
    /// `base` is the URL that ends with `/v1` (e.g. `http://127.0.0.1:8080/v1`).
    pub fn new(base: impl Into<String>, api_key: Option<String>, timeout: Duration) -> Self {
        let http = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).read_timeout(timeout).build().expect("http client");
        Self { base: base.into().trim_end_matches('/').to_string(), api_key, http }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(k) if !k.is_empty() => rb.bearer_auth(k),
            _ => rb,
        }
    }

    fn body(req: &ChatRequest) -> Value {
        let mut b = json!({
            "model": req.model,
            "messages": req.messages,
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        if !req.tools.is_empty() {
            b["tools"] = serde_json::to_value(&req.tools).unwrap_or(Value::Null);
            b["tool_choice"] = json!("auto");
            b["parallel_tool_calls"] = json!(true);
        }
        if let Some(t) = req.temperature {
            b["temperature"] = json!(t);
        }
        if let Some(t) = req.top_p {
            b["top_p"] = json!(t);
        }
        if let Some(m) = req.max_tokens {
            b["max_tokens"] = json!(m);
        }
        if !req.stop.is_empty() {
            b["stop"] = json!(req.stop);
        }
        b
    }
}

/// Converts one streamed `chat.completion.chunk` into deltas.
pub fn parse_chunk(v: &Value) -> Vec<ChatDelta> {
    let mut out = Vec::new();
    if let Some(choice) = v["choices"].get(0) {
        let d = &choice["delta"];
        for key in ["reasoning_content", "reasoning"] {
            if let Some(r) = d[key].as_str() {
                if !r.is_empty() {
                    out.push(ChatDelta::Reasoning(r.to_string()));
                }
            }
        }
        if let Some(c) = d["content"].as_str() {
            if !c.is_empty() {
                out.push(ChatDelta::Text(c.to_string()));
            }
        }
        if let Some(calls) = d["tool_calls"].as_array() {
            for (i, c) in calls.iter().enumerate() {
                out.push(ChatDelta::ToolCall {
                    index: c["index"].as_u64().map(|x| x as usize).unwrap_or(i),
                    id: c["id"].as_str().filter(|s| !s.is_empty()).map(String::from),
                    name: c["function"]["name"].as_str().filter(|s| !s.is_empty()).map(String::from),
                    arguments: c["function"]["arguments"].as_str().unwrap_or("").to_string(),
                });
            }
        }
        if let Some(fr) = choice["finish_reason"].as_str() {
            out.push(ChatDelta::Done { finish_reason: Some(fr.to_string()) });
        }
    }
    let usage = &v["usage"];
    let timings = &v["timings"]; // llama-server extension
    if usage.is_object() || timings.is_object() {
        out.push(ChatDelta::Usage(TokenUsage {
            prompt_tokens: usage["prompt_tokens"].as_u64().or_else(|| timings["prompt_n"].as_u64()).unwrap_or(0),
            completion_tokens: usage["completion_tokens"].as_u64().or_else(|| timings["predicted_n"].as_u64()).unwrap_or(0),
            tokens_per_s: timings["predicted_per_second"].as_f64(),
        }));
    }
    out
}

/// Incremental Server-Sent-Events line parser (handles frames split across network chunks).
#[derive(Default)]
pub struct SseParser {
    buf: String,
}

impl SseParser {
    /// Feeds bytes; returns complete `data:` payloads.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.push_str(&String::from_utf8_lossy(bytes));
        let mut out = Vec::new();
        while let Some(pos) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=pos).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(data) = line.strip_prefix("data:") {
                out.push(data.trim_start().to_string());
            }
        }
        out
    }
}

#[async_trait]
impl ChatBackend for OpenAiBackend {
    async fn list_models(&self) -> Result<Vec<String>> {
        let res = self
            .auth(self.http.get(format!("{}/models", self.base)))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| Error::Runtime(e.to_string()))?;
        if !res.status().is_success() {
            return Err(Error::Runtime(format!("GET /models → {}", res.status())));
        }
        let v: Value = res.json().await.map_err(|e| Error::Runtime(e.to_string()))?;
        Ok(v["data"].as_array().map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect()).unwrap_or_default())
    }

    async fn chat(&self, req: ChatRequest, cancel: CancellationToken) -> Result<DeltaStream> {
        let res = tokio::select! {
            r = self.auth(self.http.post(format!("{}/chat/completions", self.base))).json(&Self::body(&req)).send() => r.map_err(|e| Error::Runtime(format!("runtime non raggiungibile ({}): {e}", self.base)))?,
            _ = cancel.cancelled() => return Err(Error::Cancelled),
        };
        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(Error::Runtime(format!("{status}: {}", text.chars().take(500).collect::<String>())));
        }
        let bytes = res.bytes_stream();
        let stream = futures::stream::unfold(
            (bytes, SseParser::default(), VecDeque::<ChatDelta>::new(), cancel, false),
            |(mut bytes, mut parser, mut pending, cancel, mut finished)| async move {
                loop {
                    if let Some(d) = pending.pop_front() {
                        return Some((Ok(d), (bytes, parser, pending, cancel, finished)));
                    }
                    if finished {
                        return None;
                    }
                    let next = tokio::select! {
                        n = bytes.next() => n,
                        _ = cancel.cancelled() => return Some((Err(Error::Cancelled), (bytes, parser, pending, cancel, true))),
                    };
                    match next {
                        None => {
                            finished = true;
                        }
                        Some(Err(e)) => return Some((Err(Error::Runtime(e.to_string())), (bytes, parser, pending, cancel, true))),
                        Some(Ok(chunk)) => {
                            for data in parser.feed(&chunk) {
                                if data == "[DONE]" {
                                    finished = true;
                                    break;
                                }
                                match serde_json::from_str::<Value>(&data) {
                                    Ok(v) => {
                                        if let Some(err) = v.get("error") {
                                            return Some((Err(Error::Runtime(err.to_string())), (bytes, parser, pending, cancel, true)));
                                        }
                                        pending.extend(parse_chunk(&v));
                                    }
                                    Err(e) => tracing::debug!("unparseable SSE payload: {e}"),
                                }
                            }
                        }
                    }
                }
            },
        );
        Ok(stream.boxed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_parser_handles_split_frames() {
        let mut p = SseParser::default();
        assert!(p.feed(b"data: {\"a\":").is_empty());
        let out = p.feed(b"1}\n\ndata: [DONE]\n\n");
        assert_eq!(out, vec!["{\"a\":1}".to_string(), "[DONE]".to_string()]);
    }

    #[test]
    fn parses_text_reasoning_tool_calls_and_timings() {
        let v = json!({"choices":[{"delta":{"reasoning_content":"hmm","content":"Ciao","tool_calls":[{"index":0,"id":"c1","function":{"name":"read_file","arguments":"{\"pa"}}]},"finish_reason":null}]});
        let d = parse_chunk(&v);
        assert_eq!(d[0], ChatDelta::Reasoning("hmm".into()));
        assert_eq!(d[1], ChatDelta::Text("Ciao".into()));
        assert_eq!(
            d[2],
            ChatDelta::ToolCall { index: 0, id: Some("c1".into()), name: Some("read_file".into()), arguments: "{\"pa".into() }
        );
        let end = parse_chunk(
            &json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}],"timings":{"prompt_n":12,"predicted_n":30,"predicted_per_second":42.5}}),
        );
        assert_eq!(end[0], ChatDelta::Done { finish_reason: Some("tool_calls".into()) });
        assert_eq!(end[1], ChatDelta::Usage(TokenUsage { prompt_tokens: 12, completion_tokens: 30, tokens_per_s: Some(42.5) }));
    }
}
