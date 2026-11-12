//! Background checks on a phone: the glue between the engine and the native schedulers of the plugin
//! (`tauri-plugin-aiw-native`: WorkManager on Android, BGTaskScheduler, local notifications and App Intents on iOS).
//!
//! The decisions live in the engine (`aiw_engine::background`: the time budget, the reasons, the summary, the model
//! step that is skipped or delegated when the phone has no model). This file only
//!
//! * hands the proactive settings to the native side (`aiw_background` command, action `configure`) so that it can
//!   (re)schedule its jobs, and relays the other actions (`status`, `request_notifications`, `check_now`);
//! * registers the running engine, so that a system wake-up in this process uses it;
//! * exports the entry point the native side calls when the system wakes the app: a C function on iOS and a JNI
//!   function on Android. Both take and return JSON text and call `aiw_engine::background::run_background_blocking`.
//!
//! Desktop builds only compile the `aiw_background` command, which answers "not supported".

use crate::Shell;
use serde_json::{json, Value};

/// What the native side needs to schedule its jobs and, when the app is not running, to open the data.
#[cfg(mobile)]
fn schedule_payload(app: &tauri::AppHandle<tauri::Wry>, engine: &aiw_engine::Engine) -> Value {
    use tauri::Manager;
    let cfg = engine.config().effective;
    let level = cfg.get("proactive.level").and_then(Value::as_str).unwrap_or("off");
    let data_dir = app.path().app_data_dir().ok();
    json!({
        "enabled": level != "off",
        "intervalMinutes": cfg.get("proactive.interval_minutes").and_then(Value::as_u64).unwrap_or(30),
        "briefingTime": cfg.get("proactive.briefing_time").and_then(Value::as_str).unwrap_or("08:00"),
        "quietHours": cfg.get("proactive.quiet_hours").and_then(Value::as_str).unwrap_or(""),
        "onlyOnPower": cfg.get("proactive.only_on_power").and_then(Value::as_bool).unwrap_or(true),
        "notifications": cfg.get("app.notifications").and_then(Value::as_bool).unwrap_or(true),
        // Where the native side opens the data when the system starts the process without the app.
        "dataDir": data_dir.as_ref().map(|d| d.to_string_lossy().to_string()),
        "workspaceRoot": data_dir.map(|d| crate::mobile_workspace(app, &d).to_string_lossy().to_string()),
    })
}

/// `aiw_background(action)`: `status`, `request_notifications`, `configure` or `check_now`. On a phone it goes to
/// the native plugin, which answers `{platform, supported, notifications, scheduled, detail?}`.
#[tauri::command]
pub async fn aiw_background(app: tauri::AppHandle<tauri::Wry>, state: tauri::State<'_, Shell>, action: String) -> Result<Value, String> {
    #[cfg(mobile)]
    {
        use tauri::Manager;
        let method = match action.as_str() {
            "status" => "backgroundState",
            "request_notifications" => "backgroundRequestNotifications",
            "configure" => "backgroundSchedule",
            "check_now" => "backgroundCheckNow",
            other => return Err(format!("azione sconosciuta: {other}")),
        };
        let payload = schedule_payload(&app, &state.engine);
        // The plugin call blocks until the native side answers: keep it off the async runtime.
        let handle = app.clone();
        return tauri::async_runtime::spawn_blocking(move || {
            handle.state::<tauri_plugin_aiw_native::NativeLlm<tauri::Wry>>().background_call(method, payload)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string());
    }
    #[cfg(not(mobile))]
    {
        let _ = (&app, &state, &action);
        Ok(json!({ "platform": "desktop", "supported": false, "notifications": "unsupported", "scheduled": false }))
    }
}

/// Called once the engine exists: a system wake-up uses it, and the native jobs are scheduled from the settings.
#[cfg(mobile)]
pub fn setup(app: &tauri::AppHandle<tauri::Wry>, engine: &aiw_engine::Engine) {
    use tauri::Manager;
    aiw_engine::background::register_running(engine);
    let (app, engine) = (app.clone(), engine.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let payload = schedule_payload(&app, &engine);
        if let Err(e) = app.state::<tauri_plugin_aiw_native::NativeLlm<tauri::Wry>>().background_call("backgroundSchedule", payload) {
            tracing::warn!("background schedule not applied: {e}");
        }
    });
}

/// iOS: called from Swift (`@_silgen_name("aiw_background_tick")`). `request` is JSON (`reason`, `budgetS`, `onPower`,
/// `dataDir`, `workspaceRoot`); the answer is JSON (a summary, or `{"error": …}`) and must be given back to
/// [`aiw_background_free`].
///
/// # Safety
/// `request` must be null or a valid NUL-terminated string.
#[cfg(target_os = "ios")]
#[no_mangle]
pub unsafe extern "C" fn aiw_background_tick(request: *const std::os::raw::c_char) -> *mut std::os::raw::c_char {
    let req = if request.is_null() { String::new() } else { std::ffi::CStr::from_ptr(request).to_string_lossy().into_owned() };
    let out = aiw_engine::background::run_background_blocking(&req);
    std::ffi::CString::new(out).unwrap_or_default().into_raw()
}

/// iOS: releases the string returned by [`aiw_background_tick`].
///
/// # Safety
/// `ptr` must come from [`aiw_background_tick`] and not be used afterwards.
#[cfg(target_os = "ios")]
#[no_mangle]
pub unsafe extern "C" fn aiw_background_free(ptr: *mut std::os::raw::c_char) {
    if !ptr.is_null() {
        drop(std::ffi::CString::from_raw(ptr));
    }
}

/// Android: called by `BackgroundBridge.nativeTick` (the WorkManager worker, on its own thread). Same JSON in and out.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_aiworkspace_nativellm_BackgroundBridge_nativeTick<'l>(
    mut env: jni::JNIEnv<'l>,
    _class: jni::objects::JClass<'l>,
    request: jni::objects::JString<'l>,
) -> jni::sys::jstring {
    let req: String = env.get_string(&request).map(String::from).unwrap_or_default();
    let out = aiw_engine::background::run_background_blocking(&req);
    env.new_string(out).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}
