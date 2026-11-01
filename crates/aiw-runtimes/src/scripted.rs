//! Deterministic backend used by tests and by the offline demo agent.

use crate::types::*;
use aiw_core::Result;
use async_trait::async_trait;
use futures::StreamExt;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct ScriptedBackend {
    turns: Arc<Mutex<VecDeque<Vec<ChatDelta>>>>,
    /// Every request received, for assertions.
    pub requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl ScriptedBackend {
    pub fn new(turns: Vec<Vec<ChatDelta>>) -> Self {
        Self { turns: Arc::new(Mutex::new(turns.into())), requests: Default::default() }
    }
    /// Helper: a turn that calls one tool.
    pub fn tool_turn(id: &str, name: &str, args: serde_json::Value) -> Vec<ChatDelta> {
        vec![
            ChatDelta::ToolCall { index: 0, id: Some(id.into()), name: Some(name.into()), arguments: String::new() },
            ChatDelta::ToolCall { index: 0, id: None, name: None, arguments: args.to_string() },
            ChatDelta::Done { finish_reason: Some("tool_calls".into()) },
        ]
    }
    /// Helper: a final text turn.
    pub fn text_turn(text: &str) -> Vec<ChatDelta> {
        vec![
            ChatDelta::Text(text.into()),
            ChatDelta::Usage(TokenUsage { prompt_tokens: 10, completion_tokens: 5, tokens_per_s: Some(100.0) }),
            ChatDelta::Done { finish_reason: Some("stop".into()) },
        ]
    }
}

#[async_trait]
impl ChatBackend for ScriptedBackend {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec!["scripted".into()])
    }
    async fn chat(&self, req: ChatRequest, _cancel: CancellationToken) -> Result<DeltaStream> {
        self.requests.lock().unwrap().push(req);
        let turn = self.turns.lock().unwrap().pop_front().unwrap_or_else(|| Self::text_turn("Fatto."));
        Ok(futures::stream::iter(turn.into_iter().map(Ok)).boxed())
    }
}
