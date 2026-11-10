//! The native bits of Ollama (`api: "ollama"`), layered on the OpenAI adapter.
//!
//! Chat keeps going through Ollama's `/v1/chat/completions` ([`OpenAiBackend`]). What `/v1` does
//! not give is handled here:
//! - the list of **installed** models, from `GET /api/tags` (the models a profile can select),
//!   with `/v1/models` only as a fallback for servers that do not answer it;
//! - launch-time knobs, which live in the manifest (`OLLAMA_CONTEXT_LENGTH`, `OLLAMA_KEEP_ALIVE`
//!   exported by `ollama serve`) and in [`crate::supervisor::plan_launch`].
//!
//! `/v1` ignores `num_ctx` and `keep_alive` in the request body, which is why they are applied
//! when AIW starts the server. An Ollama that is already running keeps its own configuration.

use crate::openai::OpenAiBackend;
use crate::types::*;
use aiw_core::{Error, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Server root for the native API, from the OpenAI base (`http://127.0.0.1:11434/v1` →
/// `http://127.0.0.1:11434`).
pub fn native_root(openai_base: &str) -> String {
    let b = openai_base.trim_end_matches('/');
    b.strip_suffix("/v1").unwrap_or(b).to_string()
}

/// Model names from the body of `GET /api/tags`:
/// `{"models":[{"name":"qwen3:4b","model":"qwen3:4b","size":…,"details":{…}}]}`.
pub fn parse_tags(v: &Value) -> Vec<String> {
    v["models"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["name"].as_str().or_else(|| m["model"].as_str()))
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Installed models of the Ollama server behind `openai_base`; `None` when `/api/tags` does not
/// answer with a success status.
pub async fn list_installed(http: &reqwest::Client, openai_base: &str, api_key: Option<&str>, timeout: Duration) -> Option<Vec<String>> {
    let mut rb = http.get(format!("{}/api/tags", native_root(openai_base))).timeout(timeout);
    if let Some(k) = api_key.filter(|k| !k.is_empty()) {
        rb = rb.bearer_auth(k);
    }
    let r = rb.send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    let v: Value = r.json().await.ok()?;
    Some(parse_tags(&v))
}

/// Chat through Ollama's OpenAI-compatible endpoint, model listing through `/api/tags`.
#[derive(Clone)]
pub struct OllamaBackend {
    inner: OpenAiBackend,
    http: reqwest::Client,
    api_key: Option<String>,
}

impl OllamaBackend {
    /// `base` is the OpenAI base of the server (ends with `/v1`).
    pub fn new(base: impl Into<String>, api_key: Option<String>, timeout: Duration) -> Self {
        let http = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).build().expect("http client");
        Self { inner: OpenAiBackend::new(base, api_key.clone(), timeout), http, api_key }
    }

    /// Sets how requests carry the reasoning switch (the manifest says `reasoning-effort`).
    pub fn with_thinking(mut self, thinking: crate::thinking::ThinkingControl) -> Self {
        self.inner = self.inner.with_thinking(thinking);
        self
    }

    pub fn base(&self) -> &str {
        self.inner.base()
    }
}

#[async_trait]
impl ChatBackend for OllamaBackend {
    async fn list_models(&self) -> Result<Vec<String>> {
        match list_installed(&self.http, self.inner.base(), self.api_key.as_deref(), Duration::from_secs(5)).await {
            Some(models) => Ok(models),
            None => self.inner.list_models().await.map_err(|e| match e {
                Error::Runtime(m) => Error::Runtime(format!("Ollama: {m}")),
                other => other,
            }),
        }
    }

    async fn chat(&self, req: ChatRequest, cancel: CancellationToken) -> Result<DeltaStream> {
        self.inner.chat(req, cancel).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Json, Router};
    use serde_json::json;

    /// Reproduces the real shape of Ollama's `GET /api/tags`, and a `/v1/models` that lists a
    /// single model, to prove the installed list does not come from there.
    async fn fake_ollama(tags: bool) -> String {
        let mut app = Router::new()
            .route("/api/version", get(|| async { Json(json!({"version": "0.12.3"})) }))
            .route("/v1/models", get(|| async { Json(json!({"object": "list", "data": [{"id": "qwen3:4b", "object": "model"}]})) }));
        if tags {
            app = app.route(
                "/api/tags",
                get(|| async {
                    Json(json!({"models": [
                        {"name": "qwen3:4b", "model": "qwen3:4b", "modified_at": "2025-10-02T10:11:12.000+02:00", "size": 2497293931u64,
                         "digest": "359d7dd4bcda", "details": {"parent_model": "", "format": "gguf", "family": "qwen3", "families": ["qwen3"], "parameter_size": "4.0B", "quantization_level": "Q4_K_M"}},
                        {"name": "llama3.2:3b", "model": "llama3.2:3b", "modified_at": "2025-09-01T08:00:00Z", "size": 2019393189u64,
                         "digest": "a80c4f17acd5", "details": {"format": "gguf", "family": "llama", "parameter_size": "3.2B", "quantization_level": "Q4_K_M"}},
                        {"name": "nomic-embed-text:latest", "model": "nomic-embed-text:latest", "size": 274302450u64, "digest": "0a109f422b47", "details": {"format": "gguf", "family": "nomic-bert"}}
                    ]}))
                }),
            );
        }
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://127.0.0.1:{port}/v1")
    }

    #[test]
    fn native_root_strips_the_openai_suffix() {
        assert_eq!(native_root("http://127.0.0.1:11434/v1"), "http://127.0.0.1:11434");
        assert_eq!(native_root("http://127.0.0.1:11434/v1/"), "http://127.0.0.1:11434");
        assert_eq!(native_root("http://host:9/ollama/v1"), "http://host:9/ollama");
        assert_eq!(native_root("http://127.0.0.1:11434"), "http://127.0.0.1:11434");
    }

    #[test]
    fn parses_tags_with_name_or_model() {
        assert_eq!(parse_tags(&json!({"models": [{"name": "a:1"}, {"model": "b:2"}, {"size": 3}]})), vec!["a:1", "b:2"]);
        assert!(parse_tags(&json!({})).is_empty());
        assert!(parse_tags(&json!({"models": []})).is_empty());
    }

    #[tokio::test]
    async fn lists_installed_models_from_api_tags() {
        let base = fake_ollama(true).await;
        let http = reqwest::Client::new();
        let names = list_installed(&http, &base, None, Duration::from_secs(2)).await.unwrap();
        assert_eq!(names, vec!["qwen3:4b", "llama3.2:3b", "nomic-embed-text:latest"]);
        // The backend reports the same installed list, not the single model of /v1/models.
        let backend = OllamaBackend::new(base, None, Duration::from_secs(5));
        assert_eq!(backend.list_models().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn falls_back_to_v1_models_when_tags_is_missing() {
        let base = fake_ollama(false).await;
        let http = reqwest::Client::new();
        assert!(list_installed(&http, &base, None, Duration::from_secs(2)).await.is_none());
        let backend = OllamaBackend::new(base, None, Duration::from_secs(5));
        assert_eq!(backend.list_models().await.unwrap(), vec!["qwen3:4b"]);
    }

    #[tokio::test]
    async fn unreachable_server_is_none() {
        let http = reqwest::Client::new();
        assert!(list_installed(&http, "http://127.0.0.1:9/v1", None, Duration::from_millis(300)).await.is_none());
    }
}
