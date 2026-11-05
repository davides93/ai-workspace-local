//! Bridges platform-native, on-device models into the AIW runtime registry.
//!
//! * iOS / macOS (26+): Apple Foundation Models (`apple-fm`)
//! * Android: Gemini Nano through ML Kit GenAI Prompt API (`gemini-nano`) and LiteRT-LM (`litert-lm`)
//!
//! The shell asks [`NativeLlm::availability`] at startup and registers one [`NativeBackend`] per
//! available runtime with `Engine::register_native_runtime`. Tool use goes through the text
//! protocol (`<tool_call>{…}</tool_call>`), parsed by the agent loop like any other model.

use aiw_core::{Error, Result};
use aiw_runtimes::{ChatBackend, ChatDelta, ChatRequest, DeltaStream, MsgContent};
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};
use tokio_util::sync::CancellationToken;

#[cfg(desktop)]
mod desktop;
pub mod helper;
#[cfg(mobile)]
mod mobile;

#[cfg(desktop)]
pub use desktop::NativeLlm;
#[cfg(mobile)]
pub use mobile::NativeLlm;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeRuntime {
    /// Runtime id in the AIW catalog (`apple-fm`, `gemini-nano`, `litert-lm`).
    pub id: String,
    pub available: bool,
    #[serde(default)]
    pub detail: Option<String>,
    /// Model files the runtime can load (LiteRT-LM), empty for system models.
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateArgs {
    pub runtime: String,
    pub model: Option<String>,
    pub system: String,
    pub prompt: String,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

/// Events streamed back by the native side.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum NativeEvent {
    Text { delta: String },
    Done,
    Error { message: String },
}

/// Platform side of the bridge (implemented by `mobile` and `desktop`).
pub trait NativeGenerate: Send + Sync + 'static {
    /// Runs one generation, blocking until it ends. `cancel` fires when the caller is no longer
    /// interested: the implementation must stop generating (it keeps the model busy otherwise).
    fn generate(&self, args: GenerateArgs, cancel: CancellationToken, on_event: Box<dyn Fn(NativeEvent) + Send + Sync>) -> Result<()>;
}

/// Flattens an OpenAI-style conversation into (system, prompt) for single-session native APIs.
pub fn flatten(req: &ChatRequest) -> (String, String) {
    let mut system = String::new();
    let mut prompt = String::new();
    for m in &req.messages {
        let text = match &m.content {
            Some(MsgContent::Text(t)) => t.clone(),
            Some(MsgContent::Parts(_)) => m.text(),
            None => String::new(),
        };
        match m.role.as_str() {
            "system" => system.push_str(&text),
            "user" => prompt.push_str(&format!("Utente: {text}\n")),
            "assistant" => {
                let calls = m
                    .tool_calls
                    .as_ref()
                    .map(|c| {
                        c.iter()
                            .map(|t| {
                                format!(
                                    "<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>",
                                    t.function.name, t.function.arguments
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                prompt.push_str(&format!("Assistente: {text}{calls}\n"));
            }
            "tool" => prompt.push_str(&format!("Risultato dello strumento {}: {text}\n", m.name.clone().unwrap_or_default())),
            _ => {}
        }
    }
    if !req.tools.is_empty() {
        let list: Vec<String> = req
            .tools
            .iter()
            .map(|t| format!("- {}: {} (argomenti: {})", t.function.name, t.function.description, t.function.parameters))
            .collect();
        system.push_str(&format!(
            "\n\nPer usare uno strumento rispondi SOLO con <tool_call>{{\"name\": \"nome\", \"arguments\": {{...}}}}</tool_call>.\nStrumenti:\n{}",
            list.join("\n")
        ));
    }
    prompt.push_str("Assistente:");
    (system, prompt)
}

/// [`ChatBackend`] backed by a platform-native model.
pub struct NativeBackend {
    runtime: String,
    native: Arc<dyn NativeGenerate>,
}

impl NativeBackend {
    pub fn new(runtime: impl Into<String>, native: Arc<dyn NativeGenerate>) -> Self {
        Self { runtime: runtime.into(), native }
    }
}

#[async_trait]
impl ChatBackend for NativeBackend {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![self.runtime.clone()])
    }

    async fn chat(&self, req: ChatRequest, cancel: CancellationToken) -> Result<DeltaStream> {
        let (system, prompt) = flatten(&req);
        let args = GenerateArgs {
            runtime: self.runtime.clone(),
            model: Some(req.model.clone()).filter(|m| !m.is_empty()),
            system,
            prompt,
            temperature: req.temperature,
            max_tokens: req.max_tokens,
        };
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<ChatDelta>>();
        let native = self.native.clone();
        let tx2 = tx.clone();
        let stop = cancel.clone();
        // Native calls block until generation completes; tokens arrive through the callback.
        std::thread::spawn(move || {
            let cb_tx = tx2.clone();
            let r = native.generate(
                args,
                stop,
                Box::new(move |ev| {
                    let _ = match ev {
                        NativeEvent::Text { delta } => cb_tx.send(Ok(ChatDelta::Text(delta))),
                        NativeEvent::Done => cb_tx.send(Ok(ChatDelta::Done { finish_reason: Some("stop".into()) })),
                        NativeEvent::Error { message } => cb_tx.send(Err(Error::Runtime(message))),
                    };
                }),
            );
            if let Err(e) = r {
                let _ = tx2.send(Err(e));
            }
        });
        let stream = tokio_stream_from(rx).take_until(async move { cancel.cancelled().await });
        Ok(stream.boxed())
    }
}

fn tokio_stream_from<T: Send + 'static>(mut rx: tokio::sync::mpsc::UnboundedReceiver<T>) -> impl futures::Stream<Item = T> + Send {
    futures::stream::poll_fn(move |cx| rx.poll_recv(cx))
}

/// Registers the plugin. Call `app.state::<NativeLlm<R>>()` afterwards.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("aiw-native")
        .setup(|app, api| {
            #[cfg(mobile)]
            let native = mobile::init(app, api)?;
            #[cfg(desktop)]
            let native = desktop::init(app, api)?;
            app.manage(native);
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiw_runtimes::{Msg, ToolSpec};

    struct Echo;
    impl NativeGenerate for Echo {
        fn generate(&self, args: GenerateArgs, _cancel: CancellationToken, on_event: Box<dyn Fn(NativeEvent) + Send + Sync>) -> Result<()> {
            for w in args.prompt.split_whitespace().take(3) {
                on_event(NativeEvent::Text { delta: format!("{w} ") });
            }
            on_event(NativeEvent::Done);
            Ok(())
        }
    }

    #[test]
    fn flattens_conversation_and_tools() {
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![Msg::system("Sei utile."), Msg::user("ciao"), Msg::tool("c1", "list_dir", "a.txt")],
            tools: vec![ToolSpec::function("list_dir", "Elenca", serde_json::json!({"type": "object"}))],
            ..Default::default()
        };
        let (system, prompt) = flatten(&req);
        assert!(system.starts_with("Sei utile.") && system.contains("<tool_call>") && system.contains("list_dir"));
        assert!(
            prompt.contains("Utente: ciao")
                && prompt.contains("Risultato dello strumento list_dir: a.txt")
                && prompt.ends_with("Assistente:")
        );
    }

    #[tokio::test]
    async fn streams_native_tokens() {
        let b = NativeBackend::new("apple-fm", Arc::new(Echo));
        let mut s = b
            .chat(
                ChatRequest { model: String::new(), messages: vec![Msg::user("uno due tre quattro")], ..Default::default() },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let mut text = String::new();
        while let Some(d) = s.next().await {
            match d.unwrap() {
                ChatDelta::Text(t) => text.push_str(&t),
                ChatDelta::Done { .. } => break,
                _ => {}
            }
        }
        assert!(text.starts_with("Utente: uno due"));
    }
}
