//! Bridges platform-native, on-device models into the AIW runtime registry.
//!
//! * iOS / macOS (26+): Apple Foundation Models (`apple-fm`)
//! * Android: Gemini Nano through ML Kit GenAI Prompt API (`gemini-nano`) and LiteRT-LM (`litert-lm`)
//!
//! The shell asks [`NativeLlm::availability`] at startup and registers one [`NativeBackend`] per
//! available runtime with `Engine::register_native_runtime`. Apple Foundation Models has native
//! tool calling and a native conversation history (`native_tools`); the other runtimes use the
//! text protocol (`<tool_call>{…}</tool_call>`), parsed by the agent loop like any other model.

use aiw_core::{Error, Result};
use aiw_runtimes::{ChatBackend, ChatDelta, ChatRequest, DeltaStream, MsgContent};
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};
use tokio_util::sync::CancellationToken;

#[cfg(desktop)]
mod desktop;
pub mod helper;
#[cfg(all(target_os = "macos", feature = "macos-browser"))]
mod macos_browser;
#[cfg(mobile)]
mod mobile;
pub mod native_tools;

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
    /// Context window of the model, in tokens, when the platform side knows it.
    #[serde(default)]
    pub context_tokens: Option<u32>,
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
    /// Tools the model may call (Apple Foundation Models). It never runs them: the calls come
    /// back as [`NativeEvent::ToolCall`] and the agent loop runs them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<NativeTool>,
    /// The conversation before `prompt`, as a native history. `None`: the model only sees `prompt`
    /// (the flattened text of the conversation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Vec<NativeTurn>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeTool {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments.
    pub parameters: serde_json::Value,
}

/// A tool call: made by the model, or made earlier in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeCall {
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// JSON text of the arguments.
    pub arguments: String,
}

/// One message of the conversation, in the shape the Swift side turns into a `Transcript` entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum NativeTurn {
    User {
        text: String,
    },
    Assistant {
        text: String,
        #[serde(default, rename = "toolCalls", skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<NativeCall>,
    },
    Tool {
        id: String,
        name: String,
        text: String,
    },
}

/// Events streamed back by the native side.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum NativeEvent {
    Text {
        delta: String,
    },
    /// The model decided to call tools (all of one step, together). Followed by `Done`.
    ToolCall {
        calls: Vec<NativeCall>,
    },
    Done,
    Error {
        message: String,
    },
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

/// Runtimes with native tool calling and a native history (the Swift side builds a `Transcript`).
fn has_native_tools(runtime: &str) -> bool {
    runtime == "apple-fm"
}

/// What the native-tools path learns about itself while the app runs.
#[derive(Default)]
struct NativeToolsState {
    /// Consecutive requests where the native path failed and the text path worked.
    failures: AtomicUsize,
    /// Set after repeated failures of that kind: this session sticks to the text protocol.
    off: AtomicBool,
}

/// [`ChatBackend`] backed by a platform-native model.
pub struct NativeBackend {
    runtime: String,
    native: Arc<dyn NativeGenerate>,
    context: Option<u32>,
    state: Arc<NativeToolsState>,
}

impl NativeBackend {
    pub fn new(runtime: impl Into<String>, native: Arc<dyn NativeGenerate>) -> Self {
        Self { runtime: runtime.into(), native, context: None, state: Arc::default() }
    }

    /// The context window of the model, in tokens: the agent sizes its prompts (and compacts the
    /// conversation) by it, and the native side fits the history to it.
    pub fn with_context(mut self, context: Option<u32>) -> Self {
        self.context = context.filter(|c| *c > 0);
        self
    }

    fn base_args(&self, req: &ChatRequest) -> GenerateArgs {
        GenerateArgs {
            runtime: self.runtime.clone(),
            model: Some(req.model.clone()).filter(|m| !m.is_empty()),
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            context_tokens: self.context,
            ..Default::default()
        }
    }

    /// Text protocol: the conversation flattened into one prompt, tools described in the system.
    fn text_args(&self, req: &ChatRequest) -> GenerateArgs {
        let (system, prompt) = flatten(req);
        GenerateArgs { system, prompt, ..self.base_args(req) }
    }

    /// Native tools and history, when the runtime has them and they have not been given up on.
    fn native_args(&self, req: &ChatRequest) -> Option<GenerateArgs> {
        if !has_native_tools(&self.runtime) || self.state.off.load(Ordering::Relaxed) {
            return None;
        }
        let plan = native_tools::plan(req, native_tools::TOOL_BUDGET_TOKENS);
        Some(GenerateArgs {
            system: plan.system,
            prompt: plan.prompt,
            tools: plan.tools,
            transcript: Some(plan.transcript),
            ..self.base_args(req)
        })
    }
}

/// How one run of the native side ended.
enum Outcome {
    /// Finished (or cancelled): everything has been forwarded.
    Done,
    /// Failed before showing anything and held its error back, so that another way can be tried.
    Failed(Error),
    /// Failed after showing something: the error has been forwarded.
    Reported,
}

/// Runs one generation, forwarding its events as deltas. With `retryable`, a failure before any
/// output is not forwarded but returned.
fn run_native(
    native: &dyn NativeGenerate,
    args: GenerateArgs,
    stop: &CancellationToken,
    tx: &tokio::sync::mpsc::UnboundedSender<Result<ChatDelta>>,
    retryable: bool,
) -> Outcome {
    let shown = Arc::new(AtomicBool::new(false));
    let called = Arc::new(AtomicBool::new(false));
    let reported = Arc::new(AtomicBool::new(false));
    let held: Arc<Mutex<Option<Error>>> = Arc::default();
    let (cb_tx, cb_shown, cb_called, cb_held, cb_reported) = (tx.clone(), shown.clone(), called.clone(), held.clone(), reported.clone());
    let r = native.generate(
        args,
        stop.clone(),
        Box::new(move |ev| match ev {
            NativeEvent::Text { delta } => {
                cb_shown.store(true, Ordering::SeqCst);
                let _ = cb_tx.send(Ok(ChatDelta::Text(delta)));
            }
            NativeEvent::ToolCall { calls } => {
                cb_shown.store(true, Ordering::SeqCst);
                cb_called.store(true, Ordering::SeqCst);
                for (index, c) in calls.into_iter().enumerate() {
                    let id = Some(c.id).filter(|i| !i.is_empty());
                    let _ = cb_tx.send(Ok(ChatDelta::ToolCall { index, id, name: Some(c.name), arguments: c.arguments }));
                }
            }
            NativeEvent::Done => {
                let reason = if cb_called.load(Ordering::SeqCst) { "tool_calls" } else { "stop" };
                let _ = cb_tx.send(Ok(ChatDelta::Done { finish_reason: Some(reason.into()) }));
            }
            NativeEvent::Error { message } => {
                let e = Error::Runtime(message);
                if retryable && !cb_shown.load(Ordering::SeqCst) {
                    *cb_held.lock().unwrap() = Some(e);
                } else {
                    cb_reported.store(true, Ordering::SeqCst);
                    let _ = cb_tx.send(Err(e));
                }
            }
        }),
    );
    let held = held.lock().unwrap().take();
    let can_retry = retryable && !shown.load(Ordering::SeqCst) && !stop.is_cancelled();
    match (r, held) {
        (Ok(()), None) if reported.load(Ordering::SeqCst) => Outcome::Reported,
        (Ok(()), None) => Outcome::Done,
        (Ok(()), Some(e)) if can_retry => Outcome::Failed(e),
        (Err(e), _) if can_retry => Outcome::Failed(e),
        (Ok(()), Some(_)) => Outcome::Reported,
        (Err(e), _) => {
            let _ = tx.send(Err(e));
            Outcome::Reported
        }
    }
}

#[async_trait]
impl ChatBackend for NativeBackend {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![self.runtime.clone()])
    }

    fn served_context(&self) -> Option<u32> {
        self.context
    }

    async fn chat(&self, req: ChatRequest, cancel: CancellationToken) -> Result<DeltaStream> {
        let native_args = self.native_args(&req);
        let text_args = self.text_args(&req);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<ChatDelta>>();
        let native = self.native.clone();
        let state = self.state.clone();
        let stop = cancel.clone();
        // Native calls block until generation completes; tokens arrive through the callback.
        std::thread::spawn(move || {
            let Some(args) = native_args else {
                run_native(&*native, text_args, &stop, &tx, false);
                return;
            };
            match run_native(&*native, args, &stop, &tx, true) {
                Outcome::Done => state.failures.store(0, Ordering::Relaxed),
                Outcome::Reported => {}
                // The native path failed without showing anything: the text protocol is tried
                // before giving up. Twice in a row, and it is not tried first any more.
                Outcome::Failed(first) => {
                    eprintln!("aiw-native: native tools failed ({first}); retrying with the text protocol");
                    // Counted before the retry runs: its `Done` reaches the caller while this thread
                    // is still going, and the next request must already see the verdict.
                    let strikes = state.failures.fetch_add(1, Ordering::Relaxed) + 1;
                    if strikes >= 2 {
                        state.off.store(true, Ordering::Relaxed);
                    }
                    let worked = matches!(run_native(&*native, text_args, &stop, &tx, false), Outcome::Done);
                    // If the text protocol failed too, nothing says the native path is to blame.
                    if !worked || stop.is_cancelled() {
                        state.failures.fetch_sub(1, Ordering::Relaxed);
                        if strikes >= 2 {
                            state.off.store(false, Ordering::Relaxed);
                        }
                    }
                }
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
        // A runtime without native tools gets the conversation flattened into the prompt.
        let b = NativeBackend::new("gemini-nano", Arc::new(Echo));
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

    /// A native side that records what it was asked and answers with `script(args)`.
    type Script = Box<dyn Fn(&GenerateArgs) -> Vec<NativeEvent> + Send + Sync>;

    struct Scripted {
        seen: Mutex<Vec<GenerateArgs>>,
        script: Script,
    }

    impl Scripted {
        fn new(script: impl Fn(&GenerateArgs) -> Vec<NativeEvent> + Send + Sync + 'static) -> Arc<Self> {
            Arc::new(Self { seen: Mutex::default(), script: Box::new(script) })
        }
    }

    impl NativeGenerate for Scripted {
        fn generate(&self, args: GenerateArgs, _cancel: CancellationToken, on_event: Box<dyn Fn(NativeEvent) + Send + Sync>) -> Result<()> {
            let events = (self.script)(&args);
            self.seen.lock().unwrap().push(args);
            events.into_iter().for_each(on_event);
            Ok(())
        }
    }

    async fn drain(b: &NativeBackend, req: ChatRequest) -> Vec<Result<ChatDelta>> {
        let mut s = b.chat(req, CancellationToken::new()).await.unwrap();
        let mut out = Vec::new();
        while let Some(d) = s.next().await {
            let end = matches!(d, Ok(ChatDelta::Done { .. }) | Err(_));
            out.push(d);
            if end {
                break;
            }
        }
        out
    }

    fn agent_request() -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            messages: vec![Msg::system("Sei utile."), Msg::user("Apri il sito https://example.com")],
            tools: vec![ToolSpec::function(
                "browser_navigate",
                "Apre un indirizzo web",
                serde_json::json!({"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]}),
            )],
            temperature: Some(0.2),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn apple_fm_gets_native_tools_and_history_not_the_text_protocol() {
        let native = Scripted::new(|_| vec![NativeEvent::Text { delta: "ok".into() }, NativeEvent::Done]);
        let b = NativeBackend::new("apple-fm", native.clone()).with_context(Some(4096));
        assert_eq!(b.served_context(), Some(4096));
        drain(&b, agent_request()).await;
        let seen = native.seen.lock().unwrap();
        let a = &seen[0];
        assert_eq!(a.tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["browser_navigate"]);
        assert_eq!(a.prompt, "Apri il sito https://example.com");
        assert_eq!(a.transcript, Some(vec![]));
        assert!(!a.system.contains("<tool_call>") && a.system.starts_with("Sei utile."));
        assert_eq!((a.context_tokens, a.temperature), (Some(4096), Some(0.2)));
    }

    #[tokio::test]
    async fn native_tool_calls_become_tool_call_deltas() {
        let native = Scripted::new(|_| {
            vec![
                NativeEvent::ToolCall {
                    calls: vec![
                        NativeCall {
                            id: "c1".into(),
                            name: "browser_navigate".into(),
                            arguments: "{\"url\":\"https://example.com\"}".into(),
                        },
                        NativeCall { id: String::new(), name: "list_dir".into(), arguments: "{}".into() },
                    ],
                },
                NativeEvent::Done,
            ]
        });
        let b = NativeBackend::new("apple-fm", native);
        let out: Vec<ChatDelta> = drain(&b, agent_request()).await.into_iter().map(Result::unwrap).collect();
        assert_eq!(
            out,
            vec![
                ChatDelta::ToolCall {
                    index: 0,
                    id: Some("c1".into()),
                    name: Some("browser_navigate".into()),
                    arguments: "{\"url\":\"https://example.com\"}".into()
                },
                ChatDelta::ToolCall { index: 1, id: None, name: Some("list_dir".into()), arguments: "{}".into() },
                ChatDelta::Done { finish_reason: Some("tool_calls".into()) },
            ]
        );
    }

    #[tokio::test]
    async fn native_failure_before_any_output_retries_with_the_text_protocol_and_then_sticks_to_it() {
        // The native path is broken; the text protocol works.
        let native = Scripted::new(|a| {
            if a.transcript.is_some() {
                vec![NativeEvent::Error { message: "schema rifiutato".into() }]
            } else {
                vec![NativeEvent::Text { delta: "dal testo".into() }, NativeEvent::Done]
            }
        });
        let b = NativeBackend::new("apple-fm", native.clone());
        for _ in 0..2 {
            let out = drain(&b, agent_request()).await;
            assert!(matches!(&out[0], Ok(ChatDelta::Text(t)) if t == "dal testo"), "{out:?}");
        }
        assert_eq!(native.seen.lock().unwrap().len(), 4, "two requests, each tried natively and then as text");
        // From now on the text protocol goes first.
        drain(&b, agent_request()).await;
        let seen = native.seen.lock().unwrap();
        assert_eq!(seen.len(), 5);
        assert!(seen[4].transcript.is_none() && seen[4].tools.is_empty() && seen[4].system.contains("<tool_call>"));
    }

    #[tokio::test]
    async fn a_failure_after_output_is_reported_not_retried() {
        let native =
            Scripted::new(|_| vec![NativeEvent::Text { delta: "Vado".into() }, NativeEvent::Error { message: "guardrail".into() }]);
        let b = NativeBackend::new("apple-fm", native.clone());
        let out = drain(&b, agent_request()).await;
        assert!(matches!(&out[0], Ok(ChatDelta::Text(t)) if t == "Vado"));
        assert!(matches!(&out[1], Err(e) if e.to_string().contains("guardrail")));
        assert_eq!(native.seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn an_error_that_both_paths_hit_is_reported_once() {
        let native = Scripted::new(|_| vec![NativeEvent::Error { message: "assets non disponibili".into() }]);
        let b = NativeBackend::new("apple-fm", native.clone());
        let out = drain(&b, agent_request()).await;
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], Err(e) if e.to_string().contains("assets")));
        // No latch, however often: neither path worked, so nothing says the native one is to blame.
        for _ in 0..3 {
            drain(&b, agent_request()).await;
        }
        assert!(!b.state.off.load(Ordering::Relaxed));
    }

    #[test]
    fn native_events_and_turns_have_the_wire_format_the_swift_side_reads() {
        let ev: NativeEvent = serde_json::from_str(r#"{"type":"toolcall","calls":[{"id":"c","name":"glob","arguments":"{}"}]}"#).unwrap();
        assert!(matches!(ev, NativeEvent::ToolCall { calls } if calls[0].name == "glob"));
        let t = serde_json::to_value(NativeTurn::Assistant {
            text: String::new(),
            tool_calls: vec![NativeCall { id: "c".into(), name: "glob".into(), arguments: "{}".into() }],
        })
        .unwrap();
        assert_eq!(t, serde_json::json!({"role": "assistant", "text": "", "toolCalls": [{"id": "c", "name": "glob", "arguments": "{}"}]}));
        let t = serde_json::to_value(NativeTurn::Tool { id: "c".into(), name: "glob".into(), text: "a".into() }).unwrap();
        assert_eq!(t, serde_json::json!({"role": "tool", "id": "c", "name": "glob", "text": "a"}));
    }
}
