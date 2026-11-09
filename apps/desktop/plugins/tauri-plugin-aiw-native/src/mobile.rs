use crate::{GenerateArgs, NativeEvent, NativeGenerate, NativeRuntime};
use aiw_core::{Error, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::plugin::{PluginApi, PluginHandle};
use tauri::{AppHandle, Runtime};
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_aiw_native);

pub fn init<R: Runtime, C: DeserializeOwned>(_app: &AppHandle<R>, api: PluginApi<R, C>) -> tauri::Result<NativeLlm<R>> {
    // `tauri::Error: From<PluginInvokeError>` exists only on Android, so convert explicitly.
    #[cfg(target_os = "android")]
    let handle = api.register_android_plugin("dev.aiworkspace.nativellm", "NativeLlmPlugin");
    #[cfg(target_os = "ios")]
    let handle = api.register_ios_plugin(init_plugin_aiw_native);
    let handle = handle.map_err(|e| std::io::Error::other(format!("plugin nativo: {e}")))?;
    Ok(NativeLlm(handle))
}

pub struct NativeLlm<R: Runtime>(PluginHandle<R>);

#[derive(Deserialize)]
struct AvailabilityResponse {
    runtimes: Vec<NativeRuntime>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeneratePayload {
    #[serde(flatten)]
    args: GenerateArgs,
    on_event: Channel<serde_json::Value>,
}

impl<R: Runtime> NativeLlm<R> {
    /// Calls a command of the in-app browser (`browserConfigure`, `browserNavigate`, `browserEval`,
    /// `browserScreenshot`, `browserClose`) and returns its JSON answer. Blocks.
    pub fn browser_call(&self, method: &str, payload: serde_json::Value) -> Result<serde_json::Value> {
        self.0.run_mobile_plugin::<serde_json::Value>(method, payload).map_err(|e| Error::Runtime(e.to_string()))
    }

    /// Speech to text with the system recognizer (iOS: SFSpeechRecognizer). The audio travels as a
    /// WAV file in the temp folder; blocks until the recognizer is done.
    pub fn transcribe(&self, wav: &[u8], language: Option<&str>) -> Result<String> {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "aiw-stt-{}-{}.wav",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::write(&path, wav).map_err(|e| Error::Runtime(format!("audio temporaneo: {e}")))?;
        let r = self
            .0
            .run_mobile_plugin::<serde_json::Value>(
                "transcribe",
                serde_json::json!({ "path": path.to_string_lossy(), "language": language }),
            )
            .map_err(|e| Error::Runtime(e.to_string()));
        let _ = std::fs::remove_file(&path);
        Ok(r?["text"].as_str().unwrap_or_default().trim().to_string())
    }

    pub fn availability(&self) -> Result<Vec<NativeRuntime>> {
        self.0.run_mobile_plugin::<AvailabilityResponse>("availability", ()).map(|r| r.runtimes).map_err(|e| Error::Runtime(e.to_string()))
    }
}

impl<R: Runtime> NativeGenerate for NativeLlm<R> {
    fn generate(&self, args: GenerateArgs, _cancel: CancellationToken, on_event: Box<dyn Fn(NativeEvent) + Send + Sync>) -> Result<()> {
        // Tauri keeps every mobile-plugin `Channel` (and so its closure) in a global map forever.
        // The callback owns the sender of the run's delta stream: if it stayed alive here, the
        // stream would never end and the run would stay "in esecuzione" after the answer. It is
        // therefore held in a slot that is emptied as soon as the native call returns.
        type Slot = std::sync::Arc<std::sync::Mutex<Option<Box<dyn Fn(NativeEvent) + Send + Sync>>>>;
        let slot: Slot = std::sync::Arc::new(std::sync::Mutex::new(Some(on_event)));
        let channel_slot = slot.clone();
        let channel = Channel::new(move |body: InvokeResponseBody| {
            let value: serde_json::Value = match body {
                InvokeResponseBody::Json(s) => serde_json::from_str(&s).unwrap_or(serde_json::Value::Null),
                InvokeResponseBody::Raw(b) => serde_json::from_slice(&b).unwrap_or(serde_json::Value::Null),
            };
            if let Ok(ev) = serde_json::from_value::<NativeEvent>(value) {
                if let Some(cb) = channel_slot.lock().unwrap().as_ref() {
                    cb(ev);
                }
            }
            Ok(())
        });
        let result = self
            .0
            .run_mobile_plugin::<serde_json::Value>("generate", GeneratePayload { args, on_event: channel })
            .map(|_| ())
            .map_err(|e| Error::Runtime(e.to_string()));
        slot.lock().unwrap().take();
        result
    }
}
