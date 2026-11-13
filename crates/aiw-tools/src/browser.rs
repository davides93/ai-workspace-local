//! Browser control over the Chrome DevTools Protocol (pattern P18): launches a dedicated
//! Chrome/Edge/Chromium profile and exposes navigation, snapshots, clicks, typing, forms, tabs,
//! waits, downloads, the console and (on explicit grant) script evaluation.
//!
//! Snapshots are text with references so that text-only local models can browse; screenshots are
//! returned as images for vision models. Every request of every tab goes through the network
//! policy (`Fetch.requestPaused`, see `cdp.rs`).

mod ax;
mod cdp;
#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;

use crate::web::{html_to_text, proxy_applies, system_resolve, vet_url_with};
use crate::{arg_str, Risk, Tool, ToolContext, ToolOutput};
use aiw_core::{Error, Result};
use async_trait::async_trait;
pub(crate) use cdp::RefArg;
use cdp::{Cdp, SnapFmt, MAX_DOWNLOADS_PER_RUN, MAX_DOWNLOAD_BYTES, MAX_TABS};
use serde_json::{json, Value};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// The default snapshot ("elements"). Why this one and not the accessibility tree is the default:
/// it is one compact block (the controls numbered, then the visible text as running text) that a
/// small model reads in one go; it works the same on every platform (the phone WebViews have no
/// accessibility tree to ask for); and it was the format the tools were tried with. The
/// accessibility tree (`format: "ax"`) is better where the DOM lies about what a control is
/// (custom widgets, labels given by `aria-labelledby`, states of checkboxes and menus) and its
/// references stay valid across snapshots, but it costs one line per text node and needs the
/// model to cope with the roles of the ARIA vocabulary: the model picks it when the elements list
/// is not enough. Both are capped, and say «troncato» when they are.
pub(crate) const SNAPSHOT_JS: &str = r#"(() => {
  const out = []; let n = 0, more = false;
  document.querySelectorAll('[data-aiw-ref]').forEach(e => e.removeAttribute('data-aiw-ref'));
  const sel = 'a[href],button,input,select,textarea,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[role=option],[contenteditable=true],summary';
  for (const el of document.querySelectorAll(sel)) {
    const r = el.getBoundingClientRect(); const st = getComputedStyle(el);
    if (r.width < 2 || r.height < 2 || st.visibility === 'hidden' || st.display === 'none' || el.disabled) continue;
    if (n >= 150) { more = true; break; }
    n++; el.setAttribute('data-aiw-ref', String(n));
    const off = r.bottom < 0 || r.top > innerHeight ? ' (fuori vista)' : '';
    const tag = el.tagName.toLowerCase();
    const role = el.getAttribute('role') || (tag === 'a' ? 'link' : tag);
    const type = el.getAttribute('type');
    const label = (el.getAttribute('aria-label') || el.innerText || el.getAttribute('placeholder') || el.getAttribute('title') || el.getAttribute('name') || el.value || '').trim().replace(/\s+/g, ' ').slice(0, 80);
    let extra = '';
    if (tag === 'input' || tag === 'textarea') extra = ` value="${String(el.value || '').slice(0, 40)}"`;
    if (tag === 'a') extra = ` -> ${String(el.getAttribute('href')).slice(0, 90)}`;
    if (tag === 'select') extra = ` [${[...el.options].slice(0, 8).map(o => o.text.trim()).join(' | ')}]`;
    if (type === 'checkbox' || type === 'radio') extra += el.checked ? ' (selezionato)' : '';
    out.push(`[${n}] ${role}${type ? '[' + type + ']' : ''} "${label}"${extra}${off}`);
  }
  if (more) out.push('… elenco troncato: altri elementi non mostrati (scorri con browser_scroll, oppure usa browser_snapshot con format=ax)');
  const full = (document.body ? document.body.innerText : '').replace(/\n{3,}/g, '\n\n');
  const text = full.slice(0, 6000) + (full.length > 6000 ? '\n… [testo troncato: per leggerlo tutto usa browser_snapshot con format=text]' : '');
  const se = document.scrollingElement || document.documentElement;
  const total = Math.max(se.scrollHeight, 1), seen = Math.min(100, Math.max(1, Math.round((se.scrollTop + innerHeight) / total * 100)));
  const pos = se.scrollHeight <= innerHeight + 4 ? 'tutta la pagina è in vista' : `vista fino al ${seen}% della pagina (usa browser_scroll per vedere altro)`;
  return `Titolo: ${document.title}\nURL: ${location.href}\nPosizione: ${pos}\n\nElementi interattivi (usa il numero come ref):\n${out.join('\n')}\n\nTesto visibile:\n${text}`;
})()"#;

/// Network policy of the controlled browser: the context of the tool call that is running (it
/// changes from call to call) and the resolver used to vet names.
#[derive(Clone)]
pub(crate) struct Policy {
    pub(crate) ctx: Arc<Mutex<ToolContext>>,
    pub(crate) resolve: ResolveFn,
    /// Requests refused since the last navigation started: `(url, reason)`.
    pub(crate) blocked: Arc<Mutex<Vec<(String, String)>>>,
}

pub(crate) type ResolveFn = Arc<dyn Fn(String, u16) -> futures::future::BoxFuture<'static, std::io::Result<Vec<IpAddr>>> + Send + Sync>;

/// Longest a paused request waits for its verdict (DNS included) before it is failed.
const REQUEST_VERDICT_TIMEOUT: Duration = Duration::from_secs(12);

/// Verdict on one browser request. `data:`, `blob:` and `about:` never leave the browser and are
/// allowed; `ws(s):` is vetted like `http(s):`; every other scheme (file:, ftp:, chrome:, ...) is
/// refused. http(s) goes through the same policy as `web_fetch` (allowlist, private ranges, DNS
/// answers, proxy rules).
pub(crate) async fn request_verdict<R, F>(url: &str, ctx: &ToolContext, resolve: R) -> std::result::Result<(), String>
where
    R: Fn(String, u16) -> F,
    F: std::future::Future<Output = std::io::Result<Vec<IpAddr>>>,
{
    let scheme = url.split_once(':').map(|(s, _)| s.to_ascii_lowercase()).unwrap_or_default();
    let vetted = match scheme.as_str() {
        "data" | "blob" | "about" => return Ok(()),
        "http" | "https" => url.to_string(),
        "ws" => format!("http:{}", &url[3..]),
        "wss" => format!("https:{}", &url[4..]),
        other => return Err(format!("schema `{other}` non consentito")),
    };
    let via_proxy = reqwest::Url::parse(&vetted).is_ok_and(|u| proxy_applies(&u));
    vet_url_with(&vetted, ctx, resolve, via_proxy).await.map(|_| ()).map_err(|e| e.to_string())
}

/// The tools' own address checks (before and after a navigation): the same policy and resolver
/// as the per-request one.
pub(crate) async fn vet_with(url: &str, ctx: &ToolContext, resolve: &ResolveFn) -> Result<reqwest::Url> {
    let via_proxy = reqwest::Url::parse(url).is_ok_and(|u| proxy_applies(&u));
    let resolve = resolve.clone();
    Ok(vet_url_with(url, ctx, move |h, p| resolve(h, p), via_proxy).await?.url)
}

/// Answers a `Fetch.requestPaused` event: continue when the policy allows the URL, otherwise (or
/// when the verdict takes too long) fail it with `BlockedByClient`. The browser never waits forever.
fn handle_paused(params: Value, session: Option<String>, policy: Policy, tx: mpsc::UnboundedSender<Message>, next: Arc<AtomicU64>) {
    tokio::spawn(async move {
        let Some(request_id) = params["requestId"].as_str().map(String::from) else { return };
        let url = params["request"]["url"].as_str().unwrap_or_default().to_string();
        let ctx = policy.ctx.lock().unwrap().clone();
        let resolve = policy.resolve.clone();
        let verdict = tokio::time::timeout(REQUEST_VERDICT_TIMEOUT, request_verdict(&url, &ctx, move |h, p| resolve(h, p))).await;
        let (method, params) = match verdict {
            Ok(Ok(())) => ("Fetch.continueRequest", json!({"requestId": request_id})),
            Ok(Err(why)) => {
                tracing::warn!(%url, %why, "browser: request blocked by network policy");
                let mut b = policy.blocked.lock().unwrap();
                if b.len() < 50 {
                    b.push((url.clone(), why));
                }
                ("Fetch.failRequest", json!({"requestId": request_id, "errorReason": "BlockedByClient"}))
            }
            Err(_) => {
                tracing::warn!(%url, "browser: verdict timed out, request failed");
                ("Fetch.failRequest", json!({"requestId": request_id, "errorReason": "BlockedByClient"}))
            }
        };
        send_command(&tx, &next, session.as_deref(), method, params);
    });
}

/// Fire-and-forget CDP command (the reply carries an id nobody waits for and is dropped).
fn send_command(tx: &mpsc::UnboundedSender<Message>, next: &AtomicU64, session: Option<&str>, method: &str, params: Value) {
    let mut m = json!({"id": next.fetch_add(1, Ordering::SeqCst), "method": method, "params": params});
    if let Some(sid) = session {
        m["sessionId"] = json!(sid);
    }
    let _ = tx.send(Message::Text(m.to_string().into()));
}

/// Request patterns of `Fetch.enable`: no `resourceType`, so every type is paused (Document,
/// Stylesheet, Image, Media, Font, Script, XHR, Fetch, EventSource, WebSocket, Ping, Other...).
fn fetch_patterns() -> Value {
    json!({"patterns": [{"urlPattern": "*", "requestStage": "Request"}]})
}

struct Session {
    child: Option<tokio::process::Child>,
    cdp: Cdp,
}

/// Owns the controlled browser (one per engine). Lazily started on first use.
pub struct BrowserManager {
    profile_dir: PathBuf,
    headless: bool,
    session: tokio::sync::Mutex<Option<Session>>,
    pub(crate) policy: Policy,
    /// In-app WebView (iOS, Android): when set, the tools drive it instead of Chrome.
    webview: Mutex<Option<Arc<crate::webview::WebViewHost>>>,
    /// Extra Chrome command-line flags (tests only).
    extra_args: Vec<String>,
    /// A browser that is already running (tests: a fake CDP endpoint) instead of launching Chrome.
    #[cfg(test)]
    endpoint: Option<String>,
}

pub fn find_chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("AIW_CHROME") {
        return Some(PathBuf::from(p));
    }
    for n in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge", "msedge", "chrome"] {
        if let Ok(p) = which::which(n) {
            return Some(p);
        }
    }
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        "/opt/pw-browsers/chromium",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

impl BrowserManager {
    pub fn new(profile_dir: PathBuf) -> Self {
        let no_display =
            cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none();
        let headless = std::env::var("AIW_BROWSER_HEADLESS").map(|v| v == "1" || v == "true").unwrap_or(no_display);
        let policy = Policy {
            ctx: Arc::new(Mutex::new(ToolContext::new("browser", std::env::temp_dir()))),
            resolve: Arc::new(|h, p| Box::pin(system_resolve(h, p))),
            blocked: Default::default(),
        };
        Self {
            profile_dir,
            headless,
            session: tokio::sync::Mutex::new(None),
            policy,
            webview: Mutex::new(None),
            extra_args: vec![],
            #[cfg(test)]
            endpoint: None,
        }
    }

    /// Hands the tools an in-app WebView (mobile). Replaces Chrome for every later call.
    pub fn set_webview(&self, bridge: Arc<dyn crate::webview::WebViewBridge>) {
        *self.webview.lock().unwrap() = Some(Arc::new(crate::webview::WebViewHost::new(bridge)));
    }

    /// Opens the in-app browser panel on the page it has (the "browser" button of the chat).
    /// `Ok(false)` when this device has no in-app WebView (desktop drives Chrome instead).
    pub async fn show_webview(&self) -> Result<bool> {
        let Some(host) = self.webview() else { return Ok(false) };
        host.show(&self.policy).await?;
        Ok(true)
    }

    pub(crate) fn webview(&self) -> Option<Arc<crate::webview::WebViewHost>> {
        self.webview.lock().unwrap().clone()
    }

    pub fn available(&self) -> std::result::Result<(), String> {
        if self.webview().is_some() {
            return Ok(());
        }
        #[cfg(test)]
        if self.endpoint.is_some() {
            return Ok(());
        }
        if cfg!(any(target_os = "ios", target_os = "android")) {
            return Err("il browser controllato è disponibile su desktop".into());
        }
        find_chrome().map(|_| ()).ok_or_else(|| "nessun browser Chrome/Edge/Chromium trovato (imposta AIW_CHROME)".into())
    }

    async fn launch(&self) -> Result<Session> {
        let tmp = self.profile_dir.join("downloads-tmp");
        let _ = std::fs::remove_dir_all(&tmp);
        #[cfg(test)]
        if let Some(ws) = &self.endpoint {
            let cdp = Cdp::connect(ws, self.policy.clone(), tmp).await?;
            cdp.init().await?;
            return Ok(Session { child: None, cdp });
        }
        let bin = find_chrome().ok_or_else(|| Error::Unsupported("nessun browser Chrome/Edge/Chromium trovato".into()))?;
        std::fs::create_dir_all(&self.profile_dir)?;
        let port_file = self.profile_dir.join("DevToolsActivePort");
        let _ = std::fs::remove_file(&port_file);
        let mut args = vec![
            "--remote-debugging-port=0".to_string(),
            format!("--user-data-dir={}", self.profile_dir.display()),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            "--window-size=1280,900".into(),
        ];
        if self.headless {
            args.push("--headless=new".into());
        }
        #[cfg(unix)]
        {
            // Chrome refuses its own sandbox when running as root (containers, CI).
            if std::env::var("USER").map(|u| u == "root").unwrap_or(false) || std::path::Path::new("/.dockerenv").exists() {
                args.push("--no-sandbox".into());
            }
        }
        args.extend(self.extra_args.iter().cloned());
        args.push("about:blank".into());
        let child = tokio::process::Command::new(&bin)
            .args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| Error::Runtime(format!("avvio browser: {e}")))?;
        let mut port = None;
        for _ in 0..150 {
            if let Ok(s) = std::fs::read_to_string(&port_file) {
                if let Some(p) = s.lines().next().and_then(|l| l.trim().parse::<u16>().ok()) {
                    port = Some(p);
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let port = port.ok_or_else(|| Error::Runtime("il browser non ha aperto la porta di debug".into()))?;
        let version: Value = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/json/version"))
            .send()
            .await
            .map_err(Error::other)?
            .json()
            .await
            .map_err(Error::other)?;
        // The connection is made to the browser itself, not to one page: that is what lets the
        // network policy cover the tabs the site opens (see `cdp.rs`).
        let ws = version["webSocketDebuggerUrl"]
            .as_str()
            .ok_or_else(|| Error::Runtime("il browser non ha indicato l'indirizzo di debug".into()))?;
        let cdp = Cdp::connect(ws, self.policy.clone(), tmp).await?;
        cdp.init().await?;
        Ok(Session { child: Some(child), cdp })
    }

    async fn with<T>(&self, ctx: &ToolContext, f: impl for<'a> FnOnce(&'a Cdp) -> futures::future::BoxFuture<'a, Result<T>>) -> Result<T> {
        let mut guard = self.session.lock().await;
        // The calls are serialised by the lock above, so the policy in force is always that of
        // the call that is running.
        *self.policy.ctx.lock().unwrap() = ctx.clone();
        if guard.is_none() {
            *guard = Some(self.launch().await?);
        }
        let cdp = &guard.as_ref().unwrap().cdp;
        let r = match cdp.ensure_tab().await {
            Ok(()) => f(cdp).await,
            Err(e) => Err(e),
        };
        if let Err(Error::Runtime(m)) = &r {
            if m.contains("chiusa") || m.contains("chiuso") {
                *guard = None; // relaunch next time
            }
        }
        r
    }

    /// Test constructor: custom name resolver and extra Chrome flags.
    #[cfg(test)]
    fn for_test(profile_dir: PathBuf, resolve: ResolveFn, extra_args: Vec<String>) -> Self {
        let mut m = Self::new(profile_dir);
        m.policy.resolve = resolve;
        m.extra_args = extra_args;
        m
    }

    /// Test constructor: talks to an already running (fake) browser endpoint.
    #[cfg(test)]
    fn for_endpoint(profile_dir: PathBuf, ws_url: String, resolve: ResolveFn) -> Self {
        let mut m = Self::new(profile_dir);
        m.policy.resolve = resolve;
        m.endpoint = Some(ws_url);
        m
    }

    /// Closes the controlled browser (kill switch).
    pub async fn close(&self) -> bool {
        if let Some(w) = self.webview() {
            w.close().await;
            return true;
        }
        let mut guard = self.session.lock().await;
        if let Some(mut s) = guard.take() {
            let _ = s.cdp.call_root("Browser.close", json!({})).await;
            s.cdp.forget_downloads_tmp();
            if let Some(mut c) = s.child.take() {
                let _ = c.kill().await;
            }
            return true;
        }
        false
    }
}

pub fn tools(mgr: Arc<BrowserManager>) -> Vec<Arc<dyn Tool>> {
    use Op::*;
    [
        Navigate, Snapshot, Click, Type, Scroll, Back, Press, Wait, Screenshot, Tabs, TabNew, TabSelect, TabClose, Select, Hover, Upload,
        Downloads, Eval, Console,
    ]
    .into_iter()
    .map(|op| Arc::new(BrowserTool { mgr: mgr.clone(), op }) as Arc<dyn Tool>)
    .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Op {
    Navigate,
    Snapshot,
    Click,
    Type,
    Screenshot,
    Scroll,
    Back,
    Press,
    Wait,
    Tabs,
    TabNew,
    TabSelect,
    TabClose,
    Select,
    Hover,
    Upload,
    Downloads,
    Eval,
    Console,
}

impl Op {
    /// Whether the in-app WebView of a phone can do it (it has one page, no console feed, no
    /// downloads and cannot be handed a file).
    pub(crate) fn on_webview(self) -> bool {
        !matches!(self, Op::Tabs | Op::TabNew | Op::TabSelect | Op::TabClose | Op::Upload | Op::Downloads | Op::Console)
    }
}

pub(crate) const NOT_ON_DEVICE: &str = "Non disponibile su questo dispositivo: il browser integrato ha una sola pagina, senza schede, console, download e caricamento di file.";

// ---------------------------------------------------------------------- shared scripts

/// Keys `browser_press` accepts (anything else is refused before it reaches the page).
const PRESS_KEYS: &[&str] =
    &["Enter", "Escape", "Tab", "ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight", "Backspace", "PageDown", "PageUp", "Home", "End"];

/// Bodies of `function(el) { ... }`, shared by the Chrome and the in-app WebView branches. They
/// only ever get typed text as a JSON string literal, never as code.
pub(crate) const CLICK_BODY: &str = "if (!el) return 'missing'; el.scrollIntoView({block:'center'}); el.click(); return 'ok';";

pub(crate) fn type_body(text: &str, submit: bool) -> String {
    format!(
        "if (!el) return 'missing'; el.focus();\
         const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;\
         const setter = Object.getOwnPropertyDescriptor(proto, 'value'); if (setter && 'value' in el) setter.set.call(el, {text}); else el.textContent = {text};\
         el.dispatchEvent(new Event('input', {{bubbles:true}})); el.dispatchEvent(new Event('change', {{bubbles:true}}));\
         if ({submit}) {{ if (el.form && el.form.requestSubmit) el.form.requestSubmit(); else el.dispatchEvent(new KeyboardEvent('keydown', {{key:'Enter', bubbles:true}})); }}\
         return 'ok';",
        text = serde_json::to_string(text).unwrap()
    )
}

/// Chooses an option of a `<select>`: by value, by text, then by part of the text.
pub(crate) fn select_body(option: &str) -> String {
    format!(
        "if (!el) return 'missing'; if (el.tagName !== 'SELECT') return 'notselect';\
         const want = {want}; const low = want.toLowerCase(); const opts = [...el.options];\
         const o = opts.find(x => x.value === want) || opts.find(x => x.text.trim() === want) || opts.find(x => x.text.trim().toLowerCase() === low) || opts.find(x => x.text.toLowerCase().includes(low));\
         if (!o) return 'noopt:' + opts.slice(0, 20).map(x => x.text.trim()).join(' | ');\
         if (o.disabled) return 'disabled:' + o.text.trim();\
         el.value = o.value; o.selected = true;\
         el.dispatchEvent(new Event('input', {{bubbles:true}})); el.dispatchEvent(new Event('change', {{bubbles:true}}));\
         return 'ok:' + o.text.trim();",
        want = serde_json::to_string(option).unwrap()
    )
}

/// Chrome: where to move the pointer (the real mouse event is sent with `Input.dispatchMouseEvent`).
const HOVER_POS_BODY: &str = "if (!el) return 'missing'; el.scrollIntoView({block:'center'}); const r = el.getBoundingClientRect(); return {x: r.left + r.width / 2, y: r.top + r.height / 2};";

/// WebView: no pointer to move, so the events a hover produces are sent to the element (CSS
/// `:hover` is not triggered).
pub(crate) const HOVER_EVENTS_BODY: &str = "if (!el) return 'missing'; el.scrollIntoView({block:'center'}); for (const t of ['pointerover', 'mouseover', 'mouseenter', 'mousemove']) el.dispatchEvent(new MouseEvent(t, {bubbles: t !== 'mouseenter', cancelable: true})); return 'ok';";

/// A body run on the element with the numeric `ref` of the elements snapshot.
pub(crate) fn num_ref_js(n: u64, body: &str) -> String {
    format!("(function(el) {{ {body} }})(document.querySelector('[data-aiw-ref=\"{n}\"]'))")
}

/// Body and settle time (ms) of the actions that only act on the page (scroll, back, press):
/// `function(el)` with `el` the element of the `ref`, if there is one.
pub(crate) fn page_action_body(op: Op, args: &Value) -> Result<Option<(String, u64)>> {
    let has_ref = args.get("ref").is_some_and(|v| !v.is_null());
    Ok(match op {
        Op::Scroll => {
            let dir = args.get("direction").and_then(|v| v.as_str()).unwrap_or("down");
            if !["up", "down", "top", "bottom"].contains(&dir) {
                return Err(Error::Invalid("`direction` deve essere up, down, top o bottom".into()));
            }
            let amount = args.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.8).clamp(0.1, 2.0);
            Some((
                format!(
                    "{}if (el) {{ el.scrollIntoView({{block:'center'}}); return 'ok'; }}\
                     const sc = document.scrollingElement || document.documentElement; const before = sc.scrollTop;\
                     if ('{dir}' === 'top') sc.scrollTo(0, 0); else if ('{dir}' === 'bottom') sc.scrollTo(0, sc.scrollHeight);\
                     else sc.scrollBy(0, ('{dir}' === 'up' ? -1 : 1) * Math.round(innerHeight * {amount}));\
                     return sc.scrollTop === before ? 'edge' : 'ok';",
                    if has_ref { "if (!el) return 'missing'; " } else { "" }
                ),
                400,
            ))
        }
        Op::Back => Some(("if (history.length <= 1) return 'none'; history.back(); return 'ok';".into(), 1200)),
        Op::Press => {
            let key = arg_str(args, "key")?;
            if !PRESS_KEYS.contains(&key) {
                return Err(Error::Invalid(format!("tasto non consentito: {key} (consentiti: {})", PRESS_KEYS.join(", "))));
            }
            Some((
                format!(
                    "{} el.focus && el.focus();\
                     const k = '{key}'; const init = {{key: k, bubbles: true, cancelable: true}};\
                     const go = el.dispatchEvent(new KeyboardEvent('keydown', init));\
                     if (k === 'Enter' && go) {{ const tag = el.tagName; if (tag === 'BUTTON' || tag === 'A' || el.getAttribute('role') === 'button') el.click(); else if (el.form && el.form.requestSubmit) el.form.requestSubmit(); }}\
                     if (k === 'PageDown') scrollBy(0, innerHeight * 0.8); if (k === 'PageUp') scrollBy(0, -innerHeight * 0.8);\
                     if (k === 'Home') scrollTo(0, 0); if (k === 'End') scrollTo(0, document.body.scrollHeight);\
                     el.dispatchEvent(new KeyboardEvent('keyup', init)); return 'ok';",
                    if has_ref { "if (!el) return 'missing';" } else { "el = document.activeElement || document.body;" }
                ),
                if key == "Enter" { 1200 } else { 300 },
            ))
        }
        _ => None,
    })
}

/// Page script and settle time of the same actions for the WebView (numeric refs only).
pub(crate) fn page_action(op: Op, args: &Value) -> Result<Option<(String, u64)>> {
    let Some((body, settle)) = page_action_body(op, args)? else { return Ok(None) };
    let js = match args.get("ref").filter(|v| !v.is_null()) {
        Some(_) => num_ref_js(ref_arg(args)?, &body),
        None => format!("(function(el) {{ {body} }})(null)"),
    };
    Ok(Some((js, settle)))
}

/// What the model reads after an action that only acts on the page.
pub(crate) fn page_action_note(op: Op, result: &str) -> String {
    match (op, result) {
        (Op::Scroll, "edge") => "Fine della pagina: non c'è altro da scorrere in quella direzione.".into(),
        (Op::Scroll, _) => "Scorso.".into(),
        (Op::Back, "none") => "Non c'è una pagina precedente.".into(),
        (Op::Back, _) => "Tornato alla pagina precedente.".into(),
        (Op::Press, _) => "Tasto premuto.".into(),
        _ => String::new(),
    }
}

/// The `ref` argument: the number of the elements snapshot or an `eN` of the accessibility one.
pub(crate) fn parse_ref(args: &Value) -> Result<RefArg> {
    args.get("ref").and_then(RefArg::parse).ok_or_else(|| {
        Error::Invalid("argomento `ref` mancante o non valido (il numero dello snapshot, o eN per l'albero di accessibilità)".into())
    })
}

/// The `ref` as a number, for the in-app WebView (which has no accessibility tree).
pub(crate) fn ref_arg(args: &Value) -> Result<u64> {
    match parse_ref(args)? {
        RefArg::Num(n) => Ok(n),
        RefArg::Ax(_) => {
            Err(Error::Invalid("su questo dispositivo i ref sono i numeri dello snapshot (l'albero di accessibilità non c'è)".into()))
        }
    }
}

/// A plain string schema (some providers refuse type arrays); a number is accepted all the same.
fn ref_schema() -> Value {
    json!({"type": "string", "description": "numero [n] dello snapshot (es. \"3\"), oppure [eN] dell'albero di accessibilità (es. \"e5\")"})
}

// ---------------------------------------------------------------------- snapshot formats

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Format {
    Elements,
    Ax,
    Text,
}

pub(crate) fn snapshot_format(args: &Value) -> Result<Format> {
    match args.get("format").and_then(|v| v.as_str()).unwrap_or("elements") {
        "elements" | "" => Ok(Format::Elements),
        "ax" => Ok(Format::Ax),
        "text" => Ok(Format::Text),
        other => Err(Error::Invalid(format!("`format` non valido: {other} (usa elements, ax o text)"))),
    }
}

pub(crate) fn text_offset(args: &Value) -> usize {
    args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize
}

/// Script of `format: "text"`: title, address and the HTML of the page (capped), as one JSON string.
pub(crate) const TEXT_JS: &str = "(() => JSON.stringify({t: document.title, u: location.href, h: (document.documentElement ? document.documentElement.outerHTML : '').slice(0, 3000000)}))()";

/// Characters of readable text returned by one `format: "text"` call.
const TEXT_PAGE: usize = 12_000;

/// Readable text of a page (the output of `TEXT_JS`, through the same converter as `web_fetch`),
/// from character `offset`, with a «troncato» note telling how to go on.
pub(crate) fn text_snapshot(raw: &str, offset: usize) -> String {
    let v: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
    let html = v["h"].as_str().unwrap_or_default();
    let (found, text) = html_to_text(html);
    let title = v["t"].as_str().filter(|t| !t.is_empty()).map(String::from).or(found).unwrap_or_default();
    let total = text.chars().count();
    let start = offset.min(total);
    let slice: String = text.chars().skip(start).take(TEXT_PAGE).collect();
    let end = start + slice.chars().count();
    let note = if end < total {
        format!(
            "Testo da {start} a {end} di {total} caratteri, troncato: per continuare usa browser_snapshot con format=text e offset={end}."
        )
    } else {
        format!("Testo da {start} a {end} di {total} caratteri (fine della pagina).")
    };
    format!("Titolo: {title}\nURL: {}\n{note}\n\n{slice}", v["u"].as_str().unwrap_or_default())
}

// ---------------------------------------------------------------------- waiting

/// What `browser_wait` waits for. Every condition given must hold at the same time.
pub(crate) struct WaitSpec {
    pub timeout_ms: u64,
    pub text: Option<String>,
    pub text_gone: Option<String>,
    pub selector: Option<String>,
    pub element: Option<RefArg>,
    pub navigation: bool,
    pub network_idle: bool,
}

/// Longest `browser_wait` waits, whatever it is asked.
pub(crate) const WAIT_MAX_MS: u64 = 30_000;

impl WaitSpec {
    pub(crate) fn parse(args: &Value) -> Result<Self> {
        let s = |k: &str| args.get(k).and_then(|v| v.as_str()).filter(|t| !t.is_empty()).map(String::from);
        let flag = |k: &str| args.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
        let element = match args.get("ref").filter(|v| !v.is_null()) {
            Some(v) => Some(RefArg::parse(v).ok_or_else(|| Error::Invalid("`ref` non valido: il numero dello snapshot o eN".into()))?),
            None => None,
        };
        let mut w = WaitSpec {
            timeout_ms: 0,
            text: s("text"),
            text_gone: s("text_gone"),
            selector: s("selector"),
            element,
            navigation: flag("navigation"),
            network_idle: flag("network_idle"),
        };
        let default = if w.has_condition() { 10_000 } else { 1500 };
        w.timeout_ms = args.get("ms").and_then(|v| v.as_u64()).unwrap_or(default).clamp(100, WAIT_MAX_MS);
        Ok(w)
    }

    pub(crate) fn has_condition(&self) -> bool {
        self.text.is_some()
            || self.text_gone.is_some()
            || self.selector.is_some()
            || self.element.is_some()
            || self.navigation
            || self.network_idle
    }

    /// The conditions that only look at the page, as one script answering 'yes', 'no' (strings
    /// only: the in-app WebViews hand back strings) or 'bad' (a selector that is not valid).
    /// `None` when there are none. Only text and selectors go in, as JSON string literals.
    pub(crate) fn page_js(&self) -> Option<String> {
        let mut conds: Vec<String> = vec![];
        let lit = |t: &str| serde_json::to_string(t).unwrap();
        if let Some(t) = &self.text {
            conds.push(format!("txt.includes({})", lit(t)));
        }
        if let Some(t) = &self.text_gone {
            conds.push(format!("!txt.includes({})", lit(t)));
        }
        let mut selectors: Vec<String> = vec![];
        if let Some(s) = &self.selector {
            selectors.push(s.clone());
        }
        if let Some(RefArg::Num(n)) = &self.element {
            selectors.push(format!("[data-aiw-ref=\"{n}\"]"));
        }
        for s in &selectors {
            conds.push(format!("vis(document.querySelector({}))", lit(s)));
        }
        if conds.is_empty() {
            return None;
        }
        Some(format!(
            "(() => {{ try {{ const txt = document.body ? document.body.innerText : ''; \
             const vis = e => {{ if (!e) return false; const r = e.getBoundingClientRect(); const st = getComputedStyle(e); return r.width > 0 && r.height > 0 && st.visibility !== 'hidden' && st.display !== 'none'; }}; \
             return ({}) ? 'yes' : 'no'; }} catch (e) {{ return 'bad'; }} }})()",
            conds.join(" && ")
        ))
    }

    /// «Il testo «x» è comparso», for each condition.
    pub(crate) fn done_phrases(&self) -> Vec<String> {
        let mut v = vec![];
        if let Some(t) = &self.text {
            v.push(format!("il testo «{t}» è comparso"));
        }
        if let Some(t) = &self.text_gone {
            v.push(format!("il testo «{t}» è scomparso"));
        }
        if let Some(s) = &self.selector {
            v.push(format!("«{s}» è presente"));
        }
        if let Some(r) = &self.element {
            v.push(format!("[{}] è visibile", r.label()));
        }
        if self.navigation {
            v.push("la pagina ha navigato".into());
        }
        if self.network_idle {
            v.push("la rete è inattiva".into());
        }
        v
    }

    pub(crate) fn failed_phrases(&self) -> Vec<String> {
        let mut v = vec![];
        if let Some(t) = &self.text {
            v.push(format!("il testo «{t}» non è comparso"));
        }
        if let Some(t) = &self.text_gone {
            v.push(format!("il testo «{t}» è ancora presente"));
        }
        if let Some(s) = &self.selector {
            v.push(format!("«{s}» non è presente"));
        }
        if let Some(r) = &self.element {
            v.push(format!("[{}] non è visibile", r.label()));
        }
        if self.navigation {
            v.push("nessuna navigazione".into());
        }
        if self.network_idle {
            v.push("la rete è ancora attiva".into());
        }
        v
    }

    pub(crate) fn ok_note(&self) -> String {
        format!("Atteso: {}.", self.done_phrases().join(", "))
    }

    pub(crate) fn timeout_note(&self) -> String {
        format!("Non soddisfatto entro {} ms: {}.", self.timeout_ms, self.failed_phrases().join(", "))
    }
}

/// Polls `eval` until the page script answers 'yes' (`Ok(true)`), `ms` pass (`Ok(false)`) or it
/// answers 'bad' (an invalid selector).
pub(crate) async fn wait_page<F, Fut>(eval: F, js: &str, ms: u64) -> Result<bool>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<Value>>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
    loop {
        match eval(js.to_string()).await {
            Ok(v) if v.as_str() == Some("yes") => return Ok(true),
            Ok(v) if v.as_str() == Some("bad") => return Err(Error::Invalid("selettore CSS non valido".into())),
            _ => {}
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(false);
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

// ---------------------------------------------------------------------- results

const EVAL_CAP: usize = 20_000;

/// A script result as the model reads it: JSON, capped with a «troncato» note.
pub(crate) fn cap_result(s: &str) -> String {
    let n = s.chars().count();
    if n <= EVAL_CAP {
        return s.to_string();
    }
    format!("{}\n… [risultato troncato: {n} caratteri in tutto, mostrati {EVAL_CAP}]", s.chars().take(EVAL_CAP).collect::<String>())
}

/// Script of `browser_eval` for the in-app WebView (strings only come back from there).
pub(crate) fn eval_webview_js(expr: &str) -> String {
    // The expression is the model's, on purpose: this tool runs code in the page.
    format!("(() => {{ try {{ const v = ({expr}\n); const s = JSON.stringify(v); return s === undefined ? 'undefined' : s; }} catch (e) {{ return 'ERRORE: ' + e; }} }})()")
}

fn with_notes(cdp: &Cdp, text: String, extra: Vec<String>) -> String {
    let mut notes = cdp.take_notes();
    notes.extend(extra);
    if notes.is_empty() {
        return text;
    }
    format!("{text}\n\nAvvisi:\n{}", notes.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n"))
}

fn tabs_list(cdp: &Cdp) -> String {
    let tabs = cdp.tabs();
    let mut out = format!("Schede aperte ({} su {MAX_TABS} al massimo):", tabs.len());
    for t in tabs {
        let title = if t.title.is_empty() { "(senza titolo)" } else { t.title.as_str() };
        out.push_str(&format!("\n[{}] «{}» {}{}", t.index, title, t.url, if t.active { " (attiva)" } else { "" }));
        if t.by_site {
            out.push_str(" (aperta dal sito)");
        }
    }
    out
}

/// The snapshot of the active tab in the format that tab last used (elements or accessibility).
async fn default_snapshot(cdp: &Cdp) -> Result<String> {
    let body = match cdp.fmt() {
        SnapFmt::Ax => cdp.ax_snapshot().await?,
        SnapFmt::Elements => cdp.eval(SNAPSHOT_JS).await?.as_str().unwrap_or_default().to_string(),
    };
    Ok(format!("{}{body}", cdp.tab_header()))
}

/// The end of an action: let the page settle, report downloads, dialogs and new tabs, then the snapshot.
async fn tail(cdp: &Cdp, head: &str, settle_ms: u64, downloads_before: usize) -> Result<ToolOutput> {
    if settle_ms > 0 {
        tokio::time::sleep(Duration::from_millis(settle_ms)).await;
    }
    let dl =
        if cdp.downloads_len() > downloads_before { cdp.downloads_since(downloads_before, Duration::from_secs(20)).await } else { vec![] };
    let snap = default_snapshot(cdp).await?;
    let head = std::iter::once(head.to_string()).chain(dl).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
    let head = if head.is_empty() { String::new() } else { format!("{head}\n\n") };
    Ok(ToolOutput::text(with_notes(cdp, format!("{head}{snap}"), vec![])))
}

/// Opens `url` in the active tab (already vetted by the caller) and returns what the model reads.
async fn do_navigate(
    cdp: &Cdp,
    url: &str,
    ctx: &ToolContext,
    resolve: &ResolveFn,
    blocked: &Arc<Mutex<Vec<(String, String)>>>,
) -> Result<ToolOutput> {
    let (_, loads0) = cdp.tab_loads();
    let dl0 = cdp.downloads_len();
    blocked.lock().unwrap().clear();
    let r = cdp.call("Page.navigate", json!({"url": url})).await?;
    if let Some(e) = r.get("errorText").and_then(|e| e.as_str()) {
        // The request policy refused the page itself or one of its redirects.
        let refused = blocked.lock().unwrap().last().cloned();
        if let Some((u, why)) = refused {
            let _ = cdp.call("Page.navigate", json!({"url": "about:blank"})).await;
            return Ok(ToolOutput::error(format!("Navigazione annullata: {u} non consentito ({why})")));
        }
        if e.contains("ERR_ABORTED") {
            // A link to a file is not a page: the browser turns it into a download.
            tokio::time::sleep(Duration::from_millis(600)).await;
            if cdp.downloads_len() > dl0 {
                let lines = cdp.downloads_since(dl0, Duration::from_secs(30)).await;
                let failed = lines.iter().all(|l| l.starts_with("Download non riuscito"));
                let text = with_notes(cdp, format!("{}\n(la pagina aperta non è cambiata)", lines.join("\n")), vec![]);
                return Ok(if failed { ToolOutput::error(text) } else { ToolOutput::text(text) });
            }
        }
        return Ok(ToolOutput::error(format!("Navigazione fallita: {e}")));
    }
    let _ = tokio::time::timeout(Duration::from_secs(20), async {
        while cdp.tab_loads().1 <= loads0 {
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await;
    // The browser follows redirects on its own, so the page that actually loaded gets the same
    // network policy as the requested address: when it is not allowed (a public page bouncing to
    // localhost or the LAN) it is dropped before anything of it reaches the model.
    let landed = cdp.eval("location.href").await.ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    // (about:blank and browser error pages are not network targets.)
    if landed.starts_with("http://") || landed.starts_with("https://") {
        if let Err(Error::Denied(why)) = vet_with(&landed, ctx, resolve).await {
            let _ = cdp.call("Page.navigate", json!({"url": "about:blank"})).await;
            return Ok(ToolOutput::error(format!("Navigazione annullata: la pagina ha reindirizzato a {landed}, non consentito ({why})")));
        }
    }
    cdp.set_fmt(SnapFmt::Elements);
    let snap = default_snapshot(cdp).await?;
    Ok(ToolOutput::text(with_notes(cdp, snap, vec![])))
}

/// `browser_wait` on Chrome.
async fn do_wait(cdp: &Cdp, w: &WaitSpec) -> Result<ToolOutput> {
    if !w.has_condition() {
        tokio::time::sleep(Duration::from_millis(w.timeout_ms)).await;
        return tail(cdp, &format!("Atteso {} ms.", w.timeout_ms), 0, cdp.downloads_len()).await;
    }
    let js = w.page_js();
    let (navs0, loads0) = cdp.tab_loads();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(w.timeout_ms);
    let mut idle_since: Option<tokio::time::Instant> = None;
    let satisfied = loop {
        let now = tokio::time::Instant::now();
        let mut ok = true;
        if let Some(js) = &js {
            match cdp.eval(js).await {
                Ok(v) if v.as_str() == Some("yes") => {}
                Ok(v) if v.as_str() == Some("bad") => return Ok(ToolOutput::error("Il selettore CSS non è valido.")),
                _ => ok = false,
            }
        }
        if let (true, Some(RefArg::Ax(_))) = (ok, &w.element) {
            let body = "if (!el) return 'no'; const r = el.getBoundingClientRect(); const st = getComputedStyle(el); return r.width > 0 && r.height > 0 && st.visibility !== 'hidden' && st.display !== 'none' ? 'yes' : 'no';";
            ok = cdp.on_ref(w.element.as_ref().unwrap(), body).await.map(|v| v.as_str() == Some("yes")).unwrap_or(false);
        }
        if w.navigation {
            let (n, l) = cdp.tab_loads();
            ok &= n > navs0 && l > loads0;
        }
        if w.network_idle {
            if cdp.inflight() == 0 {
                let since = *idle_since.get_or_insert(now);
                ok &= now.duration_since(since) >= Duration::from_millis(500);
            } else {
                idle_since = None;
                ok = false;
            }
        }
        if ok {
            break true;
        }
        if now >= deadline {
            break false;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    let note = if satisfied { w.ok_note() } else { w.timeout_note() };
    tail(cdp, &note, 0, cdp.downloads_len()).await
}

struct BrowserTool {
    mgr: Arc<BrowserManager>,
    op: Op,
}

/// Everything `run_op` needs besides the connection.
struct OpEnv {
    args: Value,
    ctx: ToolContext,
    /// The address to open, already vetted (navigate and tab_new).
    url: Option<String>,
    resolve: ResolveFn,
    blocked: Arc<Mutex<Vec<(String, String)>>>,
}

async fn run_op(cdp: &Cdp, op: Op, env: OpEnv) -> Result<ToolOutput> {
    let OpEnv { args, ctx, url, resolve, blocked } = env;
    match op {
        Op::Navigate => do_navigate(cdp, &url.unwrap_or_default(), &ctx, &resolve, &blocked).await,
        Op::Snapshot => {
            let text = match snapshot_format(&args)? {
                Format::Elements => {
                    cdp.set_fmt(SnapFmt::Elements);
                    default_snapshot(cdp).await?
                }
                Format::Ax => format!("{}{}", cdp.tab_header(), cdp.ax_snapshot().await?),
                Format::Text => {
                    let raw = cdp.eval(TEXT_JS).await?;
                    format!("{}{}", cdp.tab_header(), text_snapshot(raw.as_str().unwrap_or_default(), text_offset(&args)))
                }
            };
            Ok(ToolOutput::text(with_notes(cdp, text, vec![])))
        }
        Op::Click => {
            let r = parse_ref(&args)?;
            let dl0 = cdp.downloads_len();
            if cdp.on_ref(&r, CLICK_BODY).await?.as_str() != Some("ok") {
                return Ok(ToolOutput::error(format!("Elemento [{}] non trovato: rifai browser_snapshot", r.label())));
            }
            tail(cdp, &format!("Cliccato [{}].", r.label()), 900, dl0).await
        }
        Op::Type => {
            let r = parse_ref(&args)?;
            let text = arg_str(&args, "text")?.to_string();
            let submit = args.get("submit").and_then(|v| v.as_bool()).unwrap_or(false);
            let dl0 = cdp.downloads_len();
            if cdp.on_ref(&r, &type_body(&text, submit)).await?.as_str() != Some("ok") {
                return Ok(ToolOutput::error(format!("Campo [{}] non trovato: rifai browser_snapshot", r.label())));
            }
            tail(cdp, &format!("Scritto in [{}].", r.label()), if submit { 1500 } else { 200 }, dl0).await
        }
        Op::Scroll | Op::Back | Op::Press => {
            let Some((body, settle)) = page_action_body(op, &args)? else { unreachable!() };
            let r = match args.get("ref").filter(|v| !v.is_null()) {
                Some(_) => Some(parse_ref(&args)?),
                None => None,
            };
            let dl0 = cdp.downloads_len();
            let res = cdp.run_body(r.as_ref(), &body).await?;
            let res = res.as_str().unwrap_or_default().to_string();
            if res == "missing" {
                return Ok(ToolOutput::error("Elemento non trovato: rifai browser_snapshot"));
            }
            tail(cdp, &page_action_note(op, &res), settle, dl0).await
        }
        Op::Wait => do_wait(cdp, &WaitSpec::parse(&args)?).await,
        Op::Screenshot => {
            let r = cdp.call("Page.captureScreenshot", json!({"format": "png"})).await?;
            let data = r["data"].as_str().unwrap_or_default();
            Ok(ToolOutput {
                content: with_notes(cdp, "Screenshot della pagina allegato.".into(), vec![]),
                images: vec![format!("data:image/png;base64,{data}")],
                is_error: false,
            })
        }
        Op::Tabs => {
            cdp.refresh_tabs().await;
            Ok(ToolOutput::text(with_notes(cdp, tabs_list(cdp), vec![])))
        }
        Op::TabNew => {
            if cdp.tab_count() >= MAX_TABS {
                return Ok(ToolOutput::error(format!(
                    "Ci sono già {MAX_TABS} schede aperte (il massimo): chiudine una con browser_tab_close."
                )));
            }
            cdp.new_tab().await?;
            let i = cdp.active_index().unwrap_or(0);
            match url {
                Some(u) => {
                    let out = do_navigate(cdp, &u, &ctx, &resolve, &blocked).await?;
                    Ok(ToolOutput { content: format!("Nuova scheda [{i}].\n\n{}", out.content), ..out })
                }
                None => {
                    Ok(ToolOutput::text(format!("Nuova scheda [{i}] vuota, ora attiva: usa browser_navigate per aprirci un indirizzo.")))
                }
            }
        }
        Op::TabSelect => {
            let i = args
                .get("index")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| Error::Invalid("argomento `index` (numero della scheda) mancante".into()))? as usize;
            cdp.select_tab(i).await?;
            tail(cdp, &format!("Scheda [{i}] attiva."), 0, cdp.downloads_len()).await
        }
        Op::TabClose => {
            let i = match args.get("index").and_then(|v| v.as_u64()) {
                Some(i) => i as usize,
                None => cdp.active_index().ok_or_else(|| Error::Runtime("nessuna scheda attiva".into()))?,
            };
            cdp.close_tab(i).await?;
            cdp.refresh_tabs().await;
            Ok(ToolOutput::text(with_notes(cdp, format!("Scheda [{i}] chiusa.\n{}", tabs_list(cdp)), vec![])))
        }
        Op::Select => {
            let r = parse_ref(&args)?;
            let option = arg_str(&args, "option")?.to_string();
            let dl0 = cdp.downloads_len();
            let res = cdp.on_ref(&r, &select_body(&option)).await?;
            let res = res.as_str().unwrap_or_default().to_string();
            if let Some(err) = select_error(&r, &option, &res) {
                return Ok(ToolOutput::error(err));
            }
            tail(cdp, &format!("Scelto «{}» in [{}].", res.trim_start_matches("ok:"), r.label()), 300, dl0).await
        }
        Op::Hover => {
            let r = parse_ref(&args)?;
            let pos = cdp.on_ref(&r, HOVER_POS_BODY).await?;
            let (Some(x), Some(y)) = (pos["x"].as_f64(), pos["y"].as_f64()) else {
                return Ok(ToolOutput::error(format!("Elemento [{}] non trovato: rifai browser_snapshot", r.label())));
            };
            let dl0 = cdp.downloads_len();
            cdp.call("Input.dispatchMouseEvent", json!({"type": "mouseMoved", "x": x, "y": y})).await?;
            tail(cdp, &format!("Puntatore sopra [{}].", r.label()), 400, dl0).await
        }
        Op::Upload => {
            let r = parse_ref(&args)?;
            let path = arg_str(&args, "path")?;
            // Reading rules of the sandbox: the workspace and the folders the run may read
            // (skills, the attachments of the session), never anything else on the disk.
            let abs = ctx.resolve(path, false)?;
            let meta = std::fs::metadata(&abs).map_err(|_| Error::NotFound(format!("file non trovato: {}", abs.display())))?;
            if !meta.is_file() {
                return Ok(ToolOutput::error(format!("{} non è un file.", abs.display())));
            }
            if meta.len() > MAX_DOWNLOAD_BYTES {
                return Ok(ToolOutput::error(format!("Il file è troppo grande da caricare (oltre {} MB).", MAX_DOWNLOAD_BYTES >> 20)));
            }
            let check = cdp
                .on_ref(&r, "if (!el) return 'missing'; return el.tagName === 'INPUT' && el.type === 'file' ? 'ok' : 'notfile';")
                .await?;
            match check.as_str() {
                Some("ok") => {}
                Some("notfile") => {
                    return Ok(ToolOutput::error(format!(
                        "L'elemento [{}] non è un campo di caricamento file (input type=file).",
                        r.label()
                    )))
                }
                _ => return Ok(ToolOutput::error(format!("Elemento [{}] non trovato: rifai browser_snapshot", r.label()))),
            }
            let Some(obj) = cdp.object_for_ref(&r).await? else {
                return Ok(ToolOutput::error(format!("Elemento [{}] non trovato: rifai browser_snapshot", r.label())));
            };
            cdp.call("DOM.setFileInputFiles", json!({"files": [abs.to_string_lossy()], "objectId": obj})).await?;
            tail(
                cdp,
                &format!(
                    "File «{}» scelto nel campo [{}].",
                    abs.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                    r.label()
                ),
                300,
                cdp.downloads_len(),
            )
            .await
        }
        Op::Downloads => {
            let all = cdp.all_downloads();
            let body = if all.is_empty() {
                "Nessun download in questa sessione.".to_string()
            } else {
                format!("Download della sessione ({}):\n{}", all.len(), all.join("\n"))
            };
            Ok(ToolOutput::text(with_notes(
                cdp,
                format!("{body}\nLimiti: {MAX_DOWNLOADS_PER_RUN} per run, {} MB ciascuno.", MAX_DOWNLOAD_BYTES >> 20),
                vec![],
            )))
        }
        Op::Eval => {
            let expr = arg_str(&args, "expression")?;
            let r = cdp
                .call("Runtime.evaluate", json!({"expression": expr, "returnByValue": true, "awaitPromise": true, "timeout": 10_000}))
                .await?;
            if let Some(ex) = r.get("exceptionDetails") {
                let msg = ex["exception"]["description"].as_str().or(ex["text"].as_str()).unwrap_or("errore sconosciuto");
                return Ok(ToolOutput::error(with_notes(
                    cdp,
                    format!("Errore JavaScript: {}", msg.lines().next().unwrap_or_default()),
                    vec![],
                )));
            }
            let out = match r["result"].get("value") {
                Some(v) => cap_result(&v.to_string()),
                None => r["result"]["description"]
                    .as_str()
                    .map(|d| format!("(valore non serializzabile: {d})"))
                    .unwrap_or_else(|| "undefined".into()),
            };
            Ok(ToolOutput::text(with_notes(cdp, out, vec![])))
        }
        Op::Console => {
            let clear = args.get("clear").and_then(|v| v.as_bool()).unwrap_or(false);
            let lines = cdp.console(clear);
            let body = if lines.is_empty() {
                "Nessun errore o avviso recente nella console della scheda.".to_string()
            } else {
                format!(
                    "Console della scheda, ultimi errori e avvisi ({}):\n{}",
                    lines.len(),
                    lines.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n")
                )
            };
            Ok(ToolOutput::text(with_notes(cdp, body, vec![])))
        }
    }
}

/// The message for a failed `browser_select`, `None` when it worked.
pub(crate) fn select_error(r: &RefArg, option: &str, res: &str) -> Option<String> {
    let label = r.label();
    match res {
        "missing" => Some(format!("Elemento [{label}] non trovato: rifai browser_snapshot")),
        "notselect" => Some(format!("L'elemento [{label}] non è un menu a tendina (select): per caselle e opzioni usa browser_click.")),
        _ if res.starts_with("noopt:") => {
            Some(format!("Nessuna opzione «{option}» in [{label}]. Opzioni: {}", res.trim_start_matches("noopt:")))
        }
        _ if res.starts_with("disabled:") => Some(format!("L'opzione «{}» è disabilitata.", res.trim_start_matches("disabled:"))),
        _ if res.starts_with("ok:") => None,
        _ => Some(format!("Scelta non riuscita in [{label}].")),
    }
}

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        match self.op {
            Op::Navigate => "browser_navigate",
            Op::Snapshot => "browser_snapshot",
            Op::Click => "browser_click",
            Op::Type => "browser_type",
            Op::Screenshot => "browser_screenshot",
            Op::Scroll => "browser_scroll",
            Op::Back => "browser_back",
            Op::Press => "browser_press",
            Op::Wait => "browser_wait",
            Op::Tabs => "browser_tabs",
            Op::TabNew => "browser_tab_new",
            Op::TabSelect => "browser_tab_select",
            Op::TabClose => "browser_tab_close",
            Op::Select => "browser_select",
            Op::Hover => "browser_hover",
            Op::Upload => "browser_upload",
            Op::Downloads => "browser_downloads",
            Op::Eval => "browser_eval",
            Op::Console => "browser_console",
        }
    }
    fn description(&self) -> &str {
        match self.op {
            Op::Navigate => "Apre un URL nel browser controllato e restituisce lo snapshot della pagina.",
            Op::Snapshot => "Snapshot della pagina. Di serie: elementi interattivi numerati [n] e testo visibile. Con format=ax: albero di accessibilità con ref [eN] stabili (ruolo, nome, valore, stato), utile se i controlli sono ambigui. Con format=text: testo leggibile di articoli lunghi (offset per continuare). Gli elenchi lunghi sono troncati.",
            Op::Click => "Clicca l'elemento `ref` dell'ultimo snapshot (numero [n] o [eN]); vale anche per caselle e opzioni radio. Se il clic avvia un download lo riporta; se il sito apre una scheda la segnala.",
            Op::Type => "Scrive `text` nel campo `ref`; con submit=true invia il modulo.",
            Op::Screenshot => "Screenshot della pagina (immagine, per modelli con visione).",
            Op::Scroll => "Scorre la pagina (up, down, top, bottom) o porta in vista l'elemento `ref`; restituisce il nuovo snapshot.",
            Op::Back => "Torna alla pagina precedente della cronologia e restituisce lo snapshot.",
            Op::Press => "Preme un tasto (Enter, Escape, Tab, frecce, PageDown…) sull'elemento `ref` o su quello attivo.",
            Op::Wait => "Aspetta che la pagina cambi, poi restituisce lo snapshot. Senza condizioni attende `ms` millisecondi. Condizioni (tutte quelle indicate devono valere): `text` compare, `text_gone` sparisce, `selector` (CSS) o `ref` visibile, `navigation` (una navigazione completata), `network_idle` (nessuna richiesta in corso). `ms` è il tempo massimo (di serie 10000, al massimo 30000).",
            Op::Tabs => "Elenca le schede aperte del browser: numero, titolo, indirizzo e quale è attiva (comprese quelle aperte dai siti).",
            Op::TabNew => "Apre una nuova scheda, vuota o su `url` (controllato come in browser_navigate), e la rende attiva. Al massimo 8 schede.",
            Op::TabSelect => "Rende attiva la scheda numero `index` (vedi browser_tabs) e ne restituisce lo snapshot.",
            Op::TabClose => "Chiude la scheda numero `index` (di serie quella attiva).",
            Op::Select => "Sceglie un'opzione di un menu a tendina (select) `ref`, per valore o per testo (`option`).",
            Op::Hover => "Porta il puntatore sull'elemento `ref` (menu che si aprono al passaggio, descrizioni) e restituisce lo snapshot.",
            Op::Upload => "Sceglie un file da caricare nel campo `ref` (input di tipo file). `path` deve stare nel workspace o negli allegati della sessione: il contenuto va al sito.",
            Op::Downloads => "Elenca i download della sessione (nome, stato, percorso nel workspace). I download vanno in `downloads/<run>/`, solo con il permesso di scrittura; al massimo 10 per run, 200 MB ciascuno.",
            Op::Eval => "Esegue codice JavaScript nella pagina aperta (`expression`) e restituisce il risultato come JSON (troncato se lungo). Attenzione: è codice eseguito nella pagina, con i suoi cookie e la sua sessione; serve un permesso esplicito.",
            Op::Console => "Errori e avvisi recenti della console della scheda attiva (anche richieste bloccate o fallite); `clear=true` li svuota.",
        }
    }
    fn parameters(&self) -> Value {
        match self.op {
            Op::Navigate => json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}),
            Op::Snapshot => json!({"type":"object","properties":{
                "format":{"type":"string","enum":["elements","ax","text"],"description":"elements (di serie), ax (albero di accessibilità) o text (testo leggibile)"},
                "offset":{"type":"integer","description":"con format=text: carattere da cui riprendere"}}}),
            Op::Screenshot | Op::Back | Op::Tabs | Op::Downloads => json!({"type":"object","properties":{}}),
            Op::Click | Op::Hover => json!({"type":"object","properties":{"ref":ref_schema()},"required":["ref"]}),
            Op::Type => {
                json!({"type":"object","properties":{"ref":ref_schema(),"text":{"type":"string"},"submit":{"type":"boolean"}},"required":["ref","text"]})
            }
            Op::Scroll => {
                json!({"type":"object","properties":{"direction":{"type":"string","enum":["up","down","top","bottom"]},"amount":{"type":"number","description":"frazione di schermata (0.1-2, default 0.8)"},"ref":ref_schema()}})
            }
            Op::Press => {
                json!({"type":"object","properties":{"key":{"type":"string","enum":PRESS_KEYS},"ref":ref_schema()},"required":["key"]})
            }
            Op::Wait => json!({"type":"object","properties":{
                "ms":{"type":"integer","description":"millisecondi: attesa fissa, o tempo massimo se ci sono condizioni (max 30000)"},
                "text":{"type":"string"},"text_gone":{"type":"string"},"selector":{"type":"string","description":"selettore CSS"},
                "ref":ref_schema(),"navigation":{"type":"boolean"},"network_idle":{"type":"boolean"}}}),
            Op::TabNew => json!({"type":"object","properties":{"url":{"type":"string"}}}),
            Op::TabSelect => {
                json!({"type":"object","properties":{"index":{"type":"integer","description":"numero della scheda in browser_tabs"}},"required":["index"]})
            }
            Op::TabClose => json!({"type":"object","properties":{"index":{"type":"integer"}}}),
            Op::Select => {
                json!({"type":"object","properties":{"ref":ref_schema(),"option":{"type":"string","description":"valore o testo dell'opzione"}},"required":["ref","option"]})
            }
            Op::Upload => {
                json!({"type":"object","properties":{"ref":ref_schema(),"path":{"type":"string","description":"file nel workspace o negli allegati"}},"required":["ref","path"]})
            }
            Op::Eval => {
                json!({"type":"object","properties":{"expression":{"type":"string","description":"espressione JavaScript"}},"required":["expression"]})
            }
            Op::Console => json!({"type":"object","properties":{"clear":{"type":"boolean"}}}),
        }
    }
    fn risk(&self) -> Risk {
        match self.op {
            Op::Navigate | Op::TabNew => Risk::Network,
            Op::Snapshot | Op::Screenshot | Op::Scroll | Op::Wait | Op::Tabs | Op::TabSelect | Op::Downloads | Op::Console => Risk::Read,
            // Upload hands a local file to a site: a person confirms it like any other action on a page.
            Op::Click | Op::Type | Op::Back | Op::Press | Op::Select | Op::Hover | Op::Upload | Op::TabClose => Risk::Computer,
            Op::Eval => Risk::Exec,
        }
    }
    /// Running code in the page is not covered by a `*` in an agent's tool list.
    fn explicit_grant(&self) -> bool {
        self.op == Op::Eval
    }
    fn subject(&self, args: &Value) -> Option<String> {
        let keys: &[&str] = if self.op == Op::Eval { &["expression"] } else { &["path", "url"] };
        keys.iter().find_map(|k| args.get(*k).and_then(|v| v.as_str()).map(String::from))
    }
    fn available(&self) -> std::result::Result<(), String> {
        if self.mgr.webview().is_some() && !self.op.on_webview() {
            return Err("non disponibile su questo dispositivo (il browser integrato ha una sola pagina)".into());
        }
        self.mgr.available()
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        if let Some(host) = self.mgr.webview() {
            if !self.op.on_webview() {
                return Ok(ToolOutput::error(NOT_ON_DEVICE));
            }
            return crate::webview::call(&self.mgr, host, self.op, args, ctx).await;
        }
        // The address is vetted before the browser is involved (and before it is launched).
        let resolve = self.mgr.policy.resolve.clone();
        let url = match (self.op, args.get("url").and_then(|v| v.as_str()).filter(|u| !u.is_empty())) {
            (Op::Navigate, _) => Some(vet_with(arg_str(&args, "url")?, ctx, &resolve).await?.to_string()),
            (Op::TabNew, Some(u)) => Some(vet_with(u, ctx, &resolve).await?.to_string()),
            _ => None,
        };
        let env = OpEnv { args, ctx: ctx.clone(), url, resolve, blocked: self.mgr.policy.blocked.clone() };
        let op = self.op;
        // A stopped run ends the call, wherever it is waiting.
        tokio::select! {
            r = self.mgr.with(ctx, move |cdp| Box::pin(run_op(cdp, op, env))) => r,
            _ = ctx.cancel.cancelled() => Ok(ToolOutput::error("Annullato: il run è stato fermato.")),
        }
    }
}
