//! Tauri 2 shell for every platform (ADR 0001). The engine runs in-process; the UI talks to it
//! through `aiw_request`, dispatched by the same axum router used by `aiw serve` (ADR 0002), and
//! receives AG-UI run events as `aiw://run-event` Tauri events.

use aiw_engine::{Engine, EngineOptions};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{Emitter, Manager, RunEvent};

#[cfg(all(feature = "mas", feature = "computer-use"))]
compile_error!("`mas` (App Sandbox) cannot be combined with `computer-use`: the sandbox forbids synthetic input and screen control");

struct Shell {
    engine: Engine,
    router: aiw_server::AxumRouter,
}

#[tauri::command]
async fn aiw_request(state: tauri::State<'_, Shell>, method: String, path: String, body: Option<Value>) -> Result<Value, String> {
    let (status, body) = aiw_server::dispatch(state.router.clone(), &method, &path, body).await;
    Ok(json!({ "status": status, "body": body }))
}

#[tauri::command]
fn aiw_run_events(state: tauri::State<'_, Shell>, run_id: String) -> Vec<Value> {
    state.engine.run_events(&run_id).into_iter().map(|e| json!({ "seq": e.seq, "event": e.event })).collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .try_init();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_aiw_native::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            // Mac App Store build: App Sandbox, no external programs (see the `mas` feature).
            let sandboxed = cfg!(feature = "mas");
            // Desktop: agents work in ~/AIW-Workspace by default (changeable in Settings); mobile and the
            // sandboxed Mac App Store build: inside the app's own container.
            let workspace_root = if cfg!(mobile) || sandboxed {
                Some(app.path().document_dir().unwrap_or_else(|_| data_dir.join("workspace")).join("AIW-Workspace"))
            } else {
                None
            };
            let engine = tauri::async_runtime::block_on(Engine::new(EngineOptions {
                data_dir: Some(data_dir),
                workspace_root,
                mode: "embedded".into(),
                sandboxed,
                ..Default::default()
            }))
            .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;

            // Platform-native models (Apple Foundation Models, Gemini Nano, LiteRT-LM) become runtimes.
            let native = app.state::<tauri_plugin_aiw_native::NativeLlm<tauri::Wry>>();
            match native.availability() {
                Ok(list) => {
                    for rt in list.iter().filter(|r| !r.available) {
                        tracing::info!("native runtime {} not available: {}", rt.id, rt.detail.as_deref().unwrap_or("-"));
                    }
                    for rt in list.into_iter().filter(|r| r.available) {
                        let bridge: Arc<dyn tauri_plugin_aiw_native::NativeGenerate> = Arc::new(NativeHandle(app.handle().clone()));
                        let backend = Arc::new(tauri_plugin_aiw_native::NativeBackend::new(rt.id.clone(), bridge));
                        if rt.id == "apple-fm" {
                            // Not ready / refused / failing: continue on a downloaded local model.
                            engine.register_system_runtime(&rt.id, backend);
                        } else {
                            engine.register_native_runtime(&rt.id, backend);
                        }
                    }
                }
                Err(e) => tracing::warn!("native runtimes unavailable: {e}"),
            }

            // Mobile: the browser tools drive the app's own WebView (a visible panel).
            #[cfg(mobile)]
            engine.browser().set_webview(Arc::new(WebViewHandle(app.handle().clone())));

            // Forward the run-event bus to the webview.
            let handle = app.handle().clone();
            let mut rx = engine.subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(m) => {
                            let _ = handle.emit("aiw://run-event", &m);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => tracing::warn!("UI lagged {n} events"),
                        Err(_) => break,
                    }
                }
            });

            // Optional LAN/browser access (Settings → "Accesso dalla LAN").
            let cfg = engine.config().effective;
            if cfg.get("server.lan").and_then(|v| v.as_bool()).unwrap_or(false) {
                let port = cfg.get("server.port").and_then(|v| v.as_u64()).unwrap_or(7777) as u16;
                let e2 = engine.clone();
                let ui = app.path().resource_dir().ok().map(|d| d.join("ui"));
                tauri::async_runtime::spawn(async move {
                    if let Err(err) = aiw_server::serve(e2, port, true, ui).await {
                        tracing::warn!("LAN server not started: {err}");
                    }
                });
            }

            let router = aiw_server::router(engine.clone(), None);
            app.manage(Shell { engine, router });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![aiw_request, aiw_run_events])
        .build(tauri::generate_context!())
        .expect("error while building AI Workspace Local");

    app.run(|handle, event| {
        if let RunEvent::ExitRequested { .. } = event {
            // Stop sidecars (llama-server, vLLM…) started by the app.
            if let Some(shell) = handle.try_state::<Shell>() {
                let engine = shell.engine.clone();
                tauri::async_runtime::block_on(async move {
                    engine.kill().await;
                });
            }
        }
    });
}

/// Adapter between the browser tools and the plugin's in-app WebView.
#[cfg(mobile)]
struct WebViewHandle(tauri::AppHandle<tauri::Wry>);

#[cfg(mobile)]
impl WebViewHandle {
    fn call(&self, method: &str, payload: serde_json::Value) -> aiw_core::Result<serde_json::Value> {
        self.0.state::<tauri_plugin_aiw_native::NativeLlm<tauri::Wry>>().browser_call(method, payload)
    }
}

#[cfg(mobile)]
impl aiw_tools::webview::WebViewBridge for WebViewHandle {
    fn configure(&self, proxy_port: u16) -> aiw_core::Result<()> {
        self.call("browserConfigure", serde_json::json!({ "proxyPort": proxy_port })).map(|_| ())
    }
    fn navigate(&self, url: &str) -> aiw_core::Result<String> {
        let v = self.call("browserNavigate", serde_json::json!({ "url": url }))?;
        Ok(v["url"].as_str().unwrap_or(url).to_string())
    }
    fn eval(&self, js: &str) -> aiw_core::Result<serde_json::Value> {
        let v = self.call("browserEval", serde_json::json!({ "js": js }))?;
        Ok(v["value"].clone())
    }
    fn screenshot(&self) -> aiw_core::Result<String> {
        let v = self.call("browserScreenshot", serde_json::json!({}))?;
        Ok(v["dataUrl"].as_str().unwrap_or_default().to_string())
    }
    fn close(&self) {
        let _ = self.call("browserClose", serde_json::json!({}));
    }
}

/// Adapter so the engine can call the plugin without knowing the Tauri runtime type.
struct NativeHandle(tauri::AppHandle<tauri::Wry>);

impl tauri_plugin_aiw_native::NativeGenerate for NativeHandle {
    fn generate(
        &self,
        args: tauri_plugin_aiw_native::GenerateArgs,
        cancel: tokio_util::sync::CancellationToken,
        on_event: Box<dyn Fn(tauri_plugin_aiw_native::NativeEvent) + Send + Sync>,
    ) -> aiw_core::Result<()> {
        let native = self.0.state::<tauri_plugin_aiw_native::NativeLlm<tauri::Wry>>();
        tauri_plugin_aiw_native::NativeGenerate::generate(native.inner(), args, cancel, on_event)
    }
}
