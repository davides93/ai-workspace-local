use aiw_core::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// OpenAI-format chat message (the lingua franca of every adapter).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Msg {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<MsgContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OaiToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MsgContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OaiToolCall {
    pub id: String,
    #[serde(rename = "type", default = "function_kind")]
    pub kind: String,
    pub function: OaiFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OaiFunctionCall {
    pub name: String,
    pub arguments: String,
}

fn function_kind() -> String {
    "function".into()
}

impl Msg {
    pub fn system(s: impl Into<String>) -> Self {
        Self { role: "system".into(), content: Some(MsgContent::Text(s.into())), ..Default::default() }
    }
    pub fn user(s: impl Into<String>) -> Self {
        Self { role: "user".into(), content: Some(MsgContent::Text(s.into())), ..Default::default() }
    }
    pub fn assistant(s: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: Some(MsgContent::Text(s.into())), ..Default::default() }
    }
    pub fn tool(call_id: impl Into<String>, name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(MsgContent::Text(content.into())),
            tool_call_id: Some(call_id.into()),
            name: Some(name.into()),
            ..Default::default()
        }
    }
    /// Concatenated text of the message.
    pub fn text(&self) -> String {
        match &self.content {
            Some(MsgContent::Text(t)) => t.clone(),
            Some(MsgContent::Parts(p)) => p
                .iter()
                .filter_map(|x| if let ContentPart::Text { text } = x { Some(text.as_str()) } else { None })
                .collect::<Vec<_>>()
                .join("\n"),
            None => String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolSpec {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolSpec {
    pub fn function(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self { kind: "function".into(), function: FunctionSpec { name: name.into(), description: description.into(), parameters } }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Msg>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tokens_per_s: Option<f64>,
}

/// Normalised streaming delta produced by every backend.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatDelta {
    Text(String),
    Reasoning(String),
    ToolCall { index: usize, id: Option<String>, name: Option<String>, arguments: String },
    Usage(TokenUsage),
    Done { finish_reason: Option<String> },
}

pub type DeltaStream = BoxStream<'static, Result<ChatDelta>>;

/// One inference backend (an adapter instance bound to an endpoint or a native engine).
#[async_trait]
pub trait ChatBackend: Send + Sync {
    /// Model identifiers the backend can serve right now.
    async fn list_models(&self) -> Result<Vec<String>>;
    /// Streams a chat completion. Implementations must stop promptly when `cancel` fires.
    async fn chat(&self, req: ChatRequest, cancel: CancellationToken) -> Result<DeltaStream>;
    /// For in-process engines that depend on something installed separately: why they cannot
    /// run yet (shown in the runtime list).
    fn availability(&self) -> std::result::Result<(), String> {
        Ok(())
    }
}
