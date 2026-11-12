//! Desktop-only behaviour of the shell (Windows, macOS, Linux): the app stays alive in the tray,
//! closing the window hides it, a second launch focuses the first, optional start at login and
//! native notifications. Compiled only for desktop targets (`cfg(desktop)` in `lib.rs`).
//!
//! The decisions that can be tested without a window (what deserves a notification) live in
//! `aiw_engine::notices`; this file only wires them to Tauri.
//!
//! Settings (machine-level, see `aiw-core/src/config.rs`): `app.close_to_tray` (default on),
//! `app.start_at_login` (default off), `app.notifications` (default on).

use crate::Shell;
use aiw_engine::notices::{Notice, NoticeTracker};
use aiw_engine::Engine;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Window, WindowEvent, Wry};
use tauri_plugin_notification::NotificationExt;

/// Flag given to the app by the login item: start hidden in the tray.
pub const MINIMIZED_ARG: &str = "--minimized";

const MAIN: &str = "main";
const TRAY_ID: &str = "main";
/// Marker file (in the data folder): the person was told once that the app keeps running.
const TRAY_NOTICE_MARKER: &str = "tray-notice-shown";

/// Present when the tray icon exists. Hiding the window is only safe then: without a tray (a
/// Linux desktop with no indicator support) a hidden window could never be brought back.
pub struct TrayReady;

fn flag(engine: &Engine, key: &str, default: bool) -> bool {
    engine.config().effective.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

/// Brings the main window back (also from the tray and from a second launch).
pub fn show_main(app: &AppHandle<Wry>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The tray menu's "Nuova chat": a session with the default agent, opened in the window.
fn new_chat(app: &AppHandle<Wry>) {
    show_main(app);
    let Some(shell) = app.try_state::<Shell>() else { return };
    let agent = shell.engine.config().effective.get("agent.default").and_then(|v| v.as_str()).unwrap_or("operator").to_string();
    match shell.engine.create_session(&agent, None) {
        Ok(session) => {
            if let (Some(w), Ok(id)) = (app.get_webview_window(MAIN), serde_json::to_string(&session.id)) {
                // The UI is a hash router: `#/chat/<session>` opens the conversation.
                let _ = w.eval(format!("window.location.hash = '#/chat/' + encodeURIComponent({id});"));
            }
        }
        Err(e) => tracing::warn!("new chat from the tray failed: {e}"),
    }
}

fn setup_tray(app: &AppHandle<Wry>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Apri AI Workspace", true, None::<&str>)?;
    let chat = MenuItem::with_id(app, "new_chat", "Nuova chat", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Esci", true, None::<&str>)?;
    // No "pause the agents" entry: the engine has no pause yet.
    let menu = Menu::with_items(app, &[&open, &chat, &sep, &quit])?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("AI Workspace Local")
        .menu(&menu)
        // A click opens the window; the menu is on the right click (Linux always shows the menu).
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "new_chat" => new_chat(app),
            // `exit` runs the `ExitRequested` handler of `lib.rs`, which stops the sidecars.
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn notify(app: &AppHandle<Wry>, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("notification not shown: {e}");
    }
}

/// True when the person is looking at the window: a notification would only repeat it.
fn window_in_front(app: &AppHandle<Wry>) -> bool {
    app.get_webview_window(MAIN).is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
}

fn deliver(app: &AppHandle<Wry>, engine: &Engine, n: &Notice) {
    if flag(engine, "app.notifications", true) && !window_in_front(app) {
        notify(app, &n.title, &n.body);
    }
}

/// Notifications for approvals waiting and runs that ended (the run-event bus) and for downloads
/// that finished (polled: they publish no event), and the start-at-login setting kept in step.
fn spawn_background(app: &AppHandle<Wry>, engine: &Engine) {
    let mut tracker = NoticeTracker::new();
    let mut rx = engine.subscribe();
    let (handle, e) = (app.clone(), engine.clone());
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3));
        let mut wanted_login: Option<bool> = None;
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Ok(m) => {
                        if let Some(n) = tracker.on_event(&m) {
                            deliver(&handle, &e, &n);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                },
                _ = tick.tick() => {
                    for n in tracker.on_downloads(&e.downloads()) {
                        deliver(&handle, &e, &n);
                    }
                    // The assistant's new proposals, held back during quiet hours (uncompiled: Tauri is not built here).
                    if let Ok(open) = e.proposals(Some("open"), None) {
                        for n in tracker.on_proposals(&open, e.proactive_quiet_now()) {
                            deliver(&handle, &e, &n);
                        }
                    }
                    // Only act when the setting changes (and once at start): a login item the person
                    // added or removed by hand in the system settings is not fought over.
                    let want = flag(&e, "app.start_at_login", false);
                    if wanted_login != Some(want) {
                        wanted_login = Some(want);
                        apply_start_at_login(&handle, want);
                    }
                }
            }
        }
    });
}

/// Registers or removes the login item. The Mac App Store build cannot (App Sandbox).
#[cfg(not(feature = "mas"))]
fn apply_start_at_login(app: &AppHandle<Wry>, want: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    match launcher.is_enabled() {
        Ok(now) if now == want => {}
        Ok(_) => {
            if let Err(e) = if want { launcher.enable() } else { launcher.disable() } {
                tracing::warn!("start at login not changed: {e}");
            }
        }
        Err(e) => tracing::warn!("start at login: state unknown: {e}"),
    }
}

#[cfg(feature = "mas")]
fn apply_start_at_login(_app: &AppHandle<Wry>, want: bool) {
    if want {
        tracing::info!("start at login is not available in the Mac App Store build (App Sandbox)");
    }
}

/// Called from `setup`: the tray, the notifications and, when launched by the login item, a hidden window.
pub fn setup(app: &tauri::App<Wry>, engine: &Engine) {
    let handle = app.handle();
    match setup_tray(handle) {
        Ok(()) => {
            app.manage(TrayReady);
            if std::env::args().any(|a| a == MINIMIZED_ARG) {
                if let Some(w) = app.get_webview_window(MAIN) {
                    let _ = w.hide();
                }
            }
        }
        Err(e) => tracing::warn!("tray icon not created: {e}; closing the window quits the app"),
    }
    spawn_background(handle, engine);
}

/// Closing the main window hides it to the tray instead of quitting, when the setting is on.
pub fn on_window_event(window: &Window<Wry>, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else { return };
    if window.label() != MAIN {
        return;
    }
    let app = window.app_handle();
    let (Some(shell), Some(_)) = (app.try_state::<Shell>(), app.try_state::<TrayReady>()) else { return };
    if !flag(&shell.engine, "app.close_to_tray", true) {
        return;
    }
    api.prevent_close();
    let _ = window.hide();
    tell_once(app);
}

/// The first time the window is hidden: say that the app keeps running and how to quit it. Shown
/// even with the notifications setting off: it is the one message that explains where the app went.
fn tell_once(app: &AppHandle<Wry>) {
    let Ok(dir) = app.path().app_data_dir() else { return };
    let marker = dir.join(TRAY_NOTICE_MARKER);
    if marker.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::write(&marker, b"1").is_ok() {
        notify(
            app,
            "AI Workspace resta attivo",
            "L'app continua a lavorare accanto all'orologio. Per chiuderla davvero usa «Esci» dal menu della sua icona.",
        );
    }
}
