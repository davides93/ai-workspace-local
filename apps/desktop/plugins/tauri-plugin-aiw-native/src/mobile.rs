use crate::{GenerateArgs, NativeEvent, NativeGenerate, NativeRuntime};
use aiw_core::{Error, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::plugin::{PluginApi, PluginHandle};
use tauri::{AppHandle, Runtime};

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
    pub fn availability(&self) -> Result<Vec<NativeRuntime>> {
        self.0.run_mobile_plugin::<AvailabilityResponse>("availability", ()).map(|r| r.runtimes).map_err(|e| Error::Runtime(e.to_string()))
    }
}

impl<R: Runtime> NativeGenerate for NativeLlm<R> {
    fn generate(&self, args: GenerateArgs, on_event: Box<dyn Fn(NativeEvent) + Send + Sync>) -> Result<()> {
        let channel = Channel::new(move |body: InvokeResponseBody| {
            let value: serde_json::Value = match body {
                InvokeResponseBody::Json(s) => serde_json::from_str(&s).unwrap_or(serde_json::Value::Null),
                InvokeResponseBody::Raw(b) => serde_json::from_slice(&b).unwrap_or(serde_json::Value::Null),
            };
            if let Ok(ev) = serde_json::from_value::<NativeEvent>(value) {
                on_event(ev);
            }
            Ok(())
        });
        self.0
            .run_mobile_plugin::<serde_json::Value>("generate", GeneratePayload { args, on_event: channel })
            .map(|_| ())
            .map_err(|e| Error::Runtime(e.to_string()))
    }
}
