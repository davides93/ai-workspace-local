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
            }]);
        }
        let a = helper.availability()?;
        Ok(vec![NativeRuntime { id: "apple-fm".into(), available: a.available, detail: a.detail, models: vec![] }])
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
