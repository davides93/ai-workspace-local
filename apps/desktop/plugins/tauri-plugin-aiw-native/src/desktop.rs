use crate::{GenerateArgs, NativeEvent, NativeGenerate, NativeRuntime};
use aiw_core::{Error, Result};
use serde::de::DeserializeOwned;
use tauri::plugin::PluginApi;
use tauri::{AppHandle, Runtime};

pub fn init<R: Runtime, C: DeserializeOwned>(app: &AppHandle<R>, _api: PluginApi<R, C>) -> tauri::Result<NativeLlm<R>> {
    Ok(NativeLlm(app.clone()))
}

/// Desktop: native system models (Windows AI APIs / Phi Silica, macOS Foundation Models through
/// a Swift helper) are on the roadmap; desktop inference goes through sidecars and embedded engines.
pub struct NativeLlm<R: Runtime>(#[allow(dead_code)] AppHandle<R>);

impl<R: Runtime> NativeLlm<R> {
    pub fn availability(&self) -> Result<Vec<NativeRuntime>> {
        Ok(vec![])
    }
}

impl<R: Runtime> NativeGenerate for NativeLlm<R> {
    fn generate(&self, _args: GenerateArgs, _on_event: Box<dyn Fn(NativeEvent) + Send + Sync>) -> Result<()> {
        Err(Error::Unsupported("nessun modello di sistema nativo su questa piattaforma desktop".into()))
    }
}
