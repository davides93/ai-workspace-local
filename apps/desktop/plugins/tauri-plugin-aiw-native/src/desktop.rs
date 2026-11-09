use crate::helper::Helper;
use crate::{GenerateArgs, NativeEvent, NativeGenerate, NativeRuntime};
use aiw_core::{Error, Result};
use serde::de::DeserializeOwned;
use tauri::plugin::PluginApi;
#[cfg(aiw_apple_fm_helper)]
use tauri::Manager;
use tauri::{AppHandle, Runtime};
use tokio_util::sync::CancellationToken;

/// The Swift helper built by `build.rs` (macOS, Apple Silicon, SDK 26+), embedded in the app.
#[cfg(aiw_apple_fm_helper)]
const EMBEDDED_HELPER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/aiw-apple-fm"));

pub fn init<R: Runtime, C: DeserializeOwned>(app: &AppHandle<R>, _api: PluginApi<R, C>) -> tauri::Result<NativeLlm<R>> {
    Ok(NativeLlm { _app: app.clone(), helper: locate_helper(app) })
}

/// Where the helper is, if this build has one.
fn locate_helper<R: Runtime>(app: &AppHandle<R>) -> Option<Helper> {
    // Development aid: point at another helper binary. Not honoured in release builds, where an
    // environment variable must not choose what the app executes.
    #[cfg(debug_assertions)]
    if let Some(p) = std::env::var_os("AIW_NATIVE_HELPER") {
        return Some(Helper::new(p));
    }
    #[cfg(aiw_apple_fm_helper)]
    {
        let dir = app.path().app_data_dir().ok()?.join("bin");
        match crate::helper::extract_embedded(EMBEDDED_HELPER, &dir) {
            Ok(p) => return Some(Helper::new(p)),
            Err(e) => eprintln!("aiw-native: cannot extract the Apple Foundation Models helper: {e}"),
        }
    }
    let _ = app;
    None
}

/// Desktop: native system models. macOS 26+ on Apple Silicon has Apple Foundation Models, reached
/// through the `aiw-apple-fm` helper process; Windows AI APIs (Phi Silica) are still on the
/// roadmap. Everything else on desktop goes through sidecars and embedded engines.
pub struct NativeLlm<R: Runtime> {
    _app: AppHandle<R>,
    helper: Option<Helper>,
}

impl<R: Runtime> NativeLlm<R> {
    pub fn availability(&self) -> Result<Vec<NativeRuntime>> {
        let Some(helper) = &self.helper else { return Ok(vec![]) };
        // The helper is built for macOS 26: on an older system it cannot even start.
        #[cfg(target_os = "macos")]
        if crate::helper::macos_major().is_some_and(|v| v < 26) {
            return Ok(vec![NativeRuntime {
                id: "apple-fm".into(),
                available: false,
                detail: Some("Richiede macOS 26 o successivo".into()),
                models: vec![],
                context_tokens: None,
            }]);
        }
        let a = helper.availability()?;
        Ok(vec![NativeRuntime {
            id: "apple-fm".into(),
            available: a.available,
            detail: a.detail,
            models: vec![],
            context_tokens: a.context_tokens,
        }])
    }
}

impl<R: Runtime> NativeGenerate for NativeLlm<R> {
    fn generate(&self, args: GenerateArgs, cancel: CancellationToken, on_event: Box<dyn Fn(NativeEvent) + Send + Sync>) -> Result<()> {
        match (&self.helper, args.runtime.as_str()) {
            (Some(h), "apple-fm") => h.generate(&args, &cancel, &*on_event),
            _ => Err(Error::Unsupported("nessun modello di sistema nativo su questa piattaforma desktop".into())),
        }
    }
}

/// Tests of the embedded Swift helper itself (macOS, Apple Silicon, where build.rs built it).
#[cfg(all(test, aiw_apple_fm_helper))]
mod helper_tests {
    use super::*;
    use crate::{NativeCall, NativeTool, NativeTurn};
    use std::sync::{Arc, Mutex};

    fn helper() -> (tempfile::TempDir, Helper) {
        let dir = tempfile::tempdir().unwrap();
        let path = crate::helper::extract_embedded(EMBEDDED_HELPER, dir.path()).unwrap();
        (dir, Helper::new(path))
    }

    /// Schemas, native history and context fitting, without the model.
    #[test]
    fn swift_selftest_passes() {
        let (_dir, h) = helper();
        let out = std::process::Command::new(h.path()).arg("selftest").output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && stdout.contains("\"ok\":true"), "{stdout} {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Needs Apple Intelligence on this Mac: `cargo test -p tauri-plugin-aiw-native -- --ignored`.
    #[test]
    #[ignore = "needs Apple Intelligence"]
    fn model_calls_a_tool_and_then_uses_its_result() {
        let (_dir, h) = helper();
        if !h.availability().map(|a| a.available).unwrap_or(false) {
            eprintln!("Apple Intelligence not available: skipped");
            return;
        }
        let tool = NativeTool {
            name: "list_dir".into(),
            description: "Elenca i file di una cartella".into(),
            parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
        };
        let run = |args: GenerateArgs| {
            let events = Arc::new(Mutex::new(Vec::new()));
            let sink = events.clone();
            h.generate(&args, &CancellationToken::new(), &move |e| sink.lock().unwrap().push(e)).unwrap();
            let ev = events.lock().unwrap().clone();
            ev
        };
        let first = run(GenerateArgs {
            system: "Sei un assistente operativo. Usa gli strumenti quando servono.".into(),
            prompt: "Elenca i file nella cartella corrente".into(),
            tools: vec![tool.clone()],
            transcript: Some(vec![]),
            ..Default::default()
        });
        let call = first.iter().find_map(|e| if let NativeEvent::ToolCall { calls } = e { calls.first().cloned() } else { None });
        let call = call.unwrap_or_else(|| panic!("no tool call: {first:?}"));
        assert_eq!(call.name, "list_dir");

        let second = run(GenerateArgs {
            system: "Sei un assistente operativo. Usa gli strumenti quando servono.".into(),
            prompt: "Usa i risultati degli strumenti per rispondere.".into(),
            tools: vec![tool],
            transcript: Some(vec![
                NativeTurn::User { text: "Elenca i file nella cartella corrente".into() },
                NativeTurn::Assistant {
                    text: String::new(),
                    tool_calls: vec![NativeCall { id: "c1".into(), name: call.name.clone(), arguments: call.arguments.clone() }],
                },
                NativeTurn::Tool { id: "c1".into(), name: call.name, text: "alfa.txt\nbeta.md".into() },
            ]),
            ..Default::default()
        });
        let text: String =
            second.iter().filter_map(|e| if let NativeEvent::Text { delta } = e { Some(delta.as_str()) } else { None }).collect();
        assert!(text.contains("alfa") && text.contains("beta"), "{second:?}");
    }

    /// The native backend over the real helper: a tool call comes out as a `ToolCall` delta and
    /// the next request, carrying the result, is answered from it.
    #[tokio::test]
    #[ignore = "needs Apple Intelligence"]
    async fn native_backend_round_trips_a_tool_call_with_the_real_model() {
        use aiw_runtimes::{ChatBackend, ChatDelta, ChatRequest, Msg, OaiFunctionCall, OaiToolCall, ToolSpec};
        use futures::StreamExt;
        struct Direct(Helper);
        impl NativeGenerate for Direct {
            fn generate(
                &self,
                args: GenerateArgs,
                cancel: CancellationToken,
                on_event: Box<dyn Fn(NativeEvent) + Send + Sync>,
            ) -> Result<()> {
                self.0.generate(&args, &cancel, &*on_event)
            }
        }
        let (_dir, h) = helper();
        if !h.availability().map(|a| a.available).unwrap_or(false) {
            return;
        }
        let backend = crate::NativeBackend::new("apple-fm", Arc::new(Direct(h)));
        let tools = vec![
            ToolSpec::function(
                "list_dir",
                "Elenca i file di una cartella",
                serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
            ),
            ToolSpec::function(
                "browser_navigate",
                "Apre un indirizzo web nel browser",
                serde_json::json!({"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]}),
            ),
        ];
        let mut messages = vec![Msg::system("Sei un assistente operativo."), Msg::user("Apri il sito https://example.com")];
        let ask = |messages: Vec<Msg>| {
            let (backend, tools) = (&backend, tools.clone());
            async move {
                let mut s = backend.chat(ChatRequest { messages, tools, ..Default::default() }, CancellationToken::new()).await.unwrap();
                let mut out = Vec::new();
                while let Some(d) = s.next().await {
                    let d = d.unwrap();
                    let end = matches!(d, ChatDelta::Done { .. });
                    out.push(d);
                    if end {
                        break;
                    }
                }
                out
            }
        };
        let first = ask(messages.clone()).await;
        let (id, name, args) = first
            .iter()
            .find_map(|d| {
                if let ChatDelta::ToolCall { id, name, arguments, .. } = d {
                    Some((id.clone(), name.clone().unwrap(), arguments.clone()))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("no tool call: {first:?}"));
        assert_eq!(name, "browser_navigate");
        assert!(args.contains("example.com"), "{args}");
        assert!(matches!(first.last(), Some(ChatDelta::Done { finish_reason: Some(r) }) if r == "tool_calls"));

        let id = id.unwrap();
        messages.push(Msg {
            role: "assistant".into(),
            tool_calls: Some(vec![OaiToolCall {
                id: id.clone(),
                kind: "function".into(),
                function: OaiFunctionCall { name: name.clone(), arguments: args },
            }]),
            ..Default::default()
        });
        messages.push(Msg::tool(id, name, "Pagina aperta. Titolo: Example Domain"));
        let second = ask(messages).await;
        let text: String = second.iter().filter_map(|d| if let ChatDelta::Text(t) = d { Some(t.as_str()) } else { None }).collect();
        assert!(text.to_lowercase().contains("example"), "{text}");
        assert!(matches!(second.last(), Some(ChatDelta::Done { finish_reason: Some(r) }) if r == "stop"));
    }
}
