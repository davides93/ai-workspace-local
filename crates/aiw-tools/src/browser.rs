//! Browser control over the Chrome DevTools Protocol (pattern P18): launches a dedicated
//! Chrome/Edge/Chromium profile, then exposes navigate / snapshot / click / type / screenshot.
//! Snapshots are text with numbered references so that text-only local models can browse;
//! screenshots are returned as images for vision models.

use crate::web::{proxy_applies, system_resolve, vet_url_with};
use crate::{arg_str, Risk, Tool, ToolContext, ToolOutput};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

pub(crate) const SNAPSHOT_JS: &str = r#"(() => {
  const out = []; let n = 0;
  document.querySelectorAll('[data-aiw-ref]').forEach(e => e.removeAttribute('data-aiw-ref'));
  const sel = 'a[href],button,input,select,textarea,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[role=option],[contenteditable=true],summary';
  for (const el of document.querySelectorAll(sel)) {
    const r = el.getBoundingClientRect(); const st = getComputedStyle(el);
    if (r.width < 2 || r.height < 2 || st.visibility === 'hidden' || st.display === 'none' || el.disabled) continue;
    if (n >= 150) break;
    n++; el.setAttribute('data-aiw-ref', String(n));
    const tag = el.tagName.toLowerCase();
    const role = el.getAttribute('role') || (tag === 'a' ? 'link' : tag);
    const type = el.getAttribute('type');
    const label = (el.getAttribute('aria-label') || el.innerText || el.getAttribute('placeholder') || el.getAttribute('title') || el.getAttribute('name') || el.value || '').trim().replace(/\s+/g, ' ').slice(0, 80);
    let extra = '';
    if (tag === 'input' || tag === 'textarea') extra = ` value="${String(el.value || '').slice(0, 40)}"`;
    if (tag === 'a') extra = ` -> ${String(el.getAttribute('href')).slice(0, 90)}`;
    if (tag === 'select') extra = ` [${[...el.options].slice(0, 8).map(o => o.text.trim()).join(' | ')}]`;
    if (type === 'checkbox' || type === 'radio') extra += el.checked ? ' (selezionato)' : '';
    out.push(`[${n}] ${role}${type ? '[' + type + ']' : ''} "${label}"${extra}`);
  }
  const text = (document.body ? document.body.innerText : '').replace(/\n{3,}/g, '\n\n').slice(0, 6000);
  return `Titolo: ${document.title}\nURL: ${location.href}\n\nElementi interattivi (usa il numero come ref):\n${out.join('\n')}\n\nTesto visibile:\n${text}`;
})()"#;

struct Cdp {
    tx: mpsc::UnboundedSender<Message>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    events: broadcast::Sender<Value>,
    next: Arc<AtomicU64>,
}

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

impl Cdp {
    async fn connect(ws_url: &str, policy: Policy) -> Result<Self> {
        let (ws, _) = tokio_tungstenite::connect_async(ws_url).await.map_err(|e| Error::Runtime(format!("CDP: {e}")))?;
        let (mut sink, mut stream) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>> = Default::default();
        let (events, _) = broadcast::channel(256);
        tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                if sink.send(m).await.is_err() {
                    break;
                }
            }
        });
        let p2 = pending.clone();
        let ev2 = events.clone();
        let next = Arc::new(AtomicU64::new(1));
        let (tx2, next2) = (tx.clone(), next.clone());
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Ok(text) = msg.into_text() else { continue };
                let Ok(v) = serde_json::from_str::<Value>(text.as_ref()) else { continue };
                if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                    if let Some(s) = p2.lock().unwrap().remove(&id) {
                        let _ = s.send(v);
                    }
                } else {
                    // Network policy is enforced here, in the reader itself and not through the
                    // lossy broadcast channel, so a paused request is never missed. Child targets
                    // (cross-origin iframes, workers) attach paused: they get the same Fetch
                    // interception before they run anything.
                    let session = v["sessionId"].as_str().map(String::from);
                    match v["method"].as_str() {
                        Some("Fetch.requestPaused") => {
                            handle_paused(v["params"].clone(), session, policy.clone(), tx2.clone(), next2.clone());
                            continue;
                        }
                        Some("Target.attachedToTarget") => {
                            if let Some(sid) = v["params"]["sessionId"].as_str() {
                                let s = Some(sid);
                                send_command(&tx2, &next2, s, "Fetch.enable", fetch_patterns());
                                send_command(
                                    &tx2,
                                    &next2,
                                    s,
                                    "Target.setAutoAttach",
                                    json!({"autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true}),
                                );
                                send_command(&tx2, &next2, s, "Runtime.runIfWaitingForDebugger", json!({}));
                            }
                            continue;
                        }
                        _ => {}
                    }
                    let _ = ev2.send(v);
                }
            }
        });
        Ok(Self { tx, pending, events, next })
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (s, r) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, s);
        let msg = json!({"id": id, "method": method, "params": params}).to_string();
        self.tx.send(Message::Text(msg.into())).map_err(|_| Error::Runtime("connessione al browser chiusa".into()))?;
        let v = tokio::time::timeout(Duration::from_secs(30), r)
            .await
            .map_err(|_| Error::Runtime(format!("CDP {method}: tempo scaduto")))?
            .map_err(|_| Error::Runtime("browser chiuso".into()))?;
        if let Some(e) = v.get("error") {
            return Err(Error::Runtime(format!("CDP {method}: {e}")));
        }
        Ok(v["result"].clone())
    }

    /// Runs one of *our own* fixed scripts in the controlled page via `Runtime.evaluate`.
    /// Only two kinds of values are ever interpolated into them: the numeric `ref` (u64) and
    /// typed text encoded with `serde_json::to_string`, i.e. always as a string literal, never
    /// as code. Page content cannot reach this function.
    async fn eval(&self, expr: &str) -> Result<Value> {
        let r = self.call("Runtime.evaluate", json!({"expression": expr, "returnByValue": true, "awaitPromise": true})).await?;
        if let Some(ex) = r.get("exceptionDetails") {
            return Err(Error::Runtime(format!("errore JavaScript: {}", ex["exception"]["description"].as_str().unwrap_or("sconosciuto"))));
        }
        Ok(r["result"]["value"].clone())
    }
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
        Self { profile_dir, headless, session: tokio::sync::Mutex::new(None), policy, webview: Mutex::new(None), extra_args: vec![] }
    }

    /// Hands the tools an in-app WebView (mobile). Replaces Chrome for every later call.
    pub fn set_webview(&self, bridge: Arc<dyn crate::webview::WebViewBridge>) {
        *self.webview.lock().unwrap() = Some(Arc::new(crate::webview::WebViewHost::new(bridge)));
    }

    pub(crate) fn webview(&self) -> Option<Arc<crate::webview::WebViewHost>> {
        self.webview.lock().unwrap().clone()
    }

    pub fn available(&self) -> std::result::Result<(), String> {
        if self.webview().is_some() {
            return Ok(());
        }
        if cfg!(any(target_os = "ios", target_os = "android")) {
            return Err("il browser controllato è disponibile su desktop".into());
        }
        find_chrome().map(|_| ()).ok_or_else(|| "nessun browser Chrome/Edge/Chromium trovato (imposta AIW_CHROME)".into())
    }

    async fn launch(&self) -> Result<Session> {
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
        let http = reqwest::Client::new();
        let list: Value = http
            .get(format!("http://127.0.0.1:{port}/json/list"))
            .send()
            .await
            .map_err(Error::other)?
            .json()
            .await
            .map_err(Error::other)?;
        let page = list.as_array().and_then(|a| a.iter().find(|t| t["type"] == "page").cloned());
        let ws = match page {
            Some(p) => p["webSocketDebuggerUrl"].as_str().unwrap_or_default().to_string(),
            None => {
                let t: Value = http
                    .put(format!("http://127.0.0.1:{port}/json/new?about:blank"))
                    .send()
                    .await
                    .map_err(Error::other)?
                    .json()
                    .await
                    .map_err(Error::other)?;
                t["webSocketDebuggerUrl"].as_str().unwrap_or_default().to_string()
            }
        };
        let cdp = Cdp::connect(&ws, self.policy.clone()).await?;
        // Network policy per request: every request of the page (and of the targets that attach
        // to it) is paused until `handle_paused` has vetted its URL.
        cdp.call("Target.setAutoAttach", json!({"autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true})).await?;
        cdp.call("Fetch.enable", fetch_patterns()).await?;
        cdp.call("Page.enable", json!({})).await?;
        cdp.call("Runtime.enable", json!({})).await?;
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
        let r = f(&guard.as_ref().unwrap().cdp).await;
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

    /// Closes the controlled browser (kill switch).
    pub async fn close(&self) -> bool {
        if let Some(w) = self.webview() {
            w.close().await;
            return true;
        }
        let mut guard = self.session.lock().await;
        if let Some(mut s) = guard.take() {
            let _ = s.cdp.call("Browser.close", json!({})).await;
            if let Some(mut c) = s.child.take() {
                let _ = c.kill().await;
            }
            return true;
        }
        false
    }
}

pub fn tools(mgr: Arc<BrowserManager>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(BrowserTool { mgr: mgr.clone(), op: Op::Navigate }),
        Arc::new(BrowserTool { mgr: mgr.clone(), op: Op::Snapshot }),
        Arc::new(BrowserTool { mgr: mgr.clone(), op: Op::Click }),
        Arc::new(BrowserTool { mgr: mgr.clone(), op: Op::Type }),
        Arc::new(BrowserTool { mgr, op: Op::Screenshot }),
    ]
}

#[derive(Clone, Copy)]
pub(crate) enum Op {
    Navigate,
    Snapshot,
    Click,
    Type,
    Screenshot,
}

struct BrowserTool {
    mgr: Arc<BrowserManager>,
    op: Op,
}

pub(crate) fn ref_arg(args: &Value) -> Result<u64> {
    args.get("ref")
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim_matches(['[', ']']).parse().ok())))
        .ok_or_else(|| Error::Invalid("argomento `ref` (numero dello snapshot) mancante".into()))
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
        }
    }
    fn description(&self) -> &str {
        match self.op {
            Op::Navigate => "Apre un URL nel browser controllato e restituisce lo snapshot della pagina.",
            Op::Snapshot => "Snapshot testuale della pagina: elementi interattivi numerati [n] e testo visibile.",
            Op::Click => "Clicca l'elemento con il numero `ref` dell'ultimo snapshot.",
            Op::Type => "Scrive `text` nel campo `ref`; con submit=true invia il modulo.",
            Op::Screenshot => "Screenshot della pagina (immagine, per modelli con visione).",
        }
    }
    fn parameters(&self) -> Value {
        match self.op {
            Op::Navigate => json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}),
            Op::Snapshot | Op::Screenshot => json!({"type":"object","properties":{}}),
            Op::Click => json!({"type":"object","properties":{"ref":{"type":"integer"}},"required":["ref"]}),
            Op::Type => {
                json!({"type":"object","properties":{"ref":{"type":"integer"},"text":{"type":"string"},"submit":{"type":"boolean"}},"required":["ref","text"]})
            }
        }
    }
    fn risk(&self) -> Risk {
        match self.op {
            Op::Navigate => Risk::Network,
            Op::Snapshot | Op::Screenshot => Risk::Read,
            Op::Click | Op::Type => Risk::Computer,
        }
    }
    fn available(&self) -> std::result::Result<(), String> {
        self.mgr.available()
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        if let Some(host) = self.mgr.webview() {
            return crate::webview::call(&self.mgr, host, self.op, args, ctx).await;
        }
        match self.op {
            Op::Navigate => {
                let resolve = self.mgr.policy.resolve.clone();
                let url = vet_with(arg_str(&args, "url")?, ctx, &resolve).await?.to_string();
                let policy = ctx.clone();
                let blocked = self.mgr.policy.blocked.clone();
                self.mgr
                    .with(ctx, |cdp| {
                        Box::pin(async move {
                            let mut ev = cdp.events.subscribe();
                            blocked.lock().unwrap().clear();
                            let r = cdp.call("Page.navigate", json!({"url": url})).await?;
                            if let Some(e) = r.get("errorText").and_then(|e| e.as_str()) {
                                // The request policy refused the page itself or one of its redirects.
                                let refused = blocked.lock().unwrap().last().cloned();
                                if let Some((u, why)) = refused {
                                    let _ = cdp.call("Page.navigate", json!({"url": "about:blank"})).await;
                                    return Ok(ToolOutput::error(format!("Navigazione annullata: {u} non consentito ({why})")));
                                }
                                return Ok(ToolOutput::error(format!("Navigazione fallita: {e}")));
                            }
                            let _ = tokio::time::timeout(Duration::from_secs(20), async {
                                while let Ok(v) = ev.recv().await {
                                    if v["method"] == "Page.loadEventFired" {
                                        break;
                                    }
                                }
                            })
                            .await;
                            // The browser follows redirects on its own, so the page that actually
                            // loaded gets the same network policy as the requested address: when it
                            // is not allowed (a public page bouncing to localhost or the LAN) it is
                            // dropped before anything of it reaches the model.
                            let landed =
                                cdp.eval("location.href").await.ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
                            // (about:blank and browser error pages are not network targets.)
                            if landed.starts_with("http://") || landed.starts_with("https://") {
                                if let Err(Error::Denied(why)) = vet_with(&landed, &policy, &resolve).await {
                                    let _ = cdp.call("Page.navigate", json!({"url": "about:blank"})).await;
                                    return Ok(ToolOutput::error(format!(
                                        "Navigazione annullata: la pagina ha reindirizzato a {landed}, non consentito ({why})"
                                    )));
                                }
                            }
                            let snap = cdp.eval(SNAPSHOT_JS).await?;
                            Ok(ToolOutput::text(snap.as_str().unwrap_or_default().to_string()))
                        })
                    })
                    .await
            }
            Op::Snapshot => {
                self.mgr
                    .with(ctx, |cdp| {
                        Box::pin(async move { Ok(ToolOutput::text(cdp.eval(SNAPSHOT_JS).await?.as_str().unwrap_or_default().to_string())) })
                    })
                    .await
            }
            Op::Click => {
                let r = ref_arg(&args)?;
                self.mgr
                    .with(ctx, |cdp| {
                        Box::pin(async move {
                            let js = format!("(() => {{ const el = document.querySelector('[data-aiw-ref=\"{r}\"]'); if (!el) return 'missing'; el.scrollIntoView({{block:'center'}}); el.click(); return 'ok'; }})()");
                            if cdp.eval(&js).await?.as_str() != Some("ok") {
                                return Ok(ToolOutput::error(format!("Elemento [{r}] non trovato: rifai browser_snapshot")));
                            }
                            tokio::time::sleep(Duration::from_millis(900)).await;
                            let snap = cdp.eval(SNAPSHOT_JS).await?;
                            Ok(ToolOutput::text(format!("Cliccato [{r}].\n\n{}", snap.as_str().unwrap_or_default())))
                        })
                    })
                    .await
            }
            Op::Type => {
                let r = ref_arg(&args)?;
                let text = arg_str(&args, "text")?.to_string();
                let submit = args.get("submit").and_then(|v| v.as_bool()).unwrap_or(false);
                self.mgr
                    .with(ctx, |cdp| {
                        Box::pin(async move {
                            let js = format!(
                                "(() => {{ const el = document.querySelector('[data-aiw-ref=\"{r}\"]'); if (!el) return 'missing'; el.focus();\
                                 const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;\
                                 const setter = Object.getOwnPropertyDescriptor(proto, 'value'); if (setter && 'value' in el) setter.set.call(el, {text}); else el.textContent = {text};\
                                 el.dispatchEvent(new Event('input', {{bubbles:true}})); el.dispatchEvent(new Event('change', {{bubbles:true}}));\
                                 if ({submit}) {{ if (el.form && el.form.requestSubmit) el.form.requestSubmit(); else el.dispatchEvent(new KeyboardEvent('keydown', {{key:'Enter', bubbles:true}})); }}\
                                 return 'ok'; }})()",
                                text = serde_json::to_string(&text).unwrap()
                            );
                            if cdp.eval(&js).await?.as_str() != Some("ok") {
                                return Ok(ToolOutput::error(format!("Campo [{r}] non trovato: rifai browser_snapshot")));
                            }
                            tokio::time::sleep(Duration::from_millis(if submit { 1500 } else { 200 })).await;
                            let snap = cdp.eval(SNAPSHOT_JS).await?;
                            Ok(ToolOutput::text(format!("Scritto in [{r}].\n\n{}", snap.as_str().unwrap_or_default())))
                        })
                    })
                    .await
            }
            Op::Screenshot => {
                self.mgr
                    .with(ctx, |cdp| {
                        Box::pin(async move {
                            let r = cdp.call("Page.captureScreenshot", json!({"format": "png"})).await?;
                            let data = r["data"].as_str().unwrap_or_default();
                            Ok(ToolOutput {
                                content: "Screenshot della pagina allegato.".into(),
                                images: vec![format!("data:image/png;base64,{data}")],
                                is_error: false,
                            })
                        })
                    })
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ToolContext {
        ToolContext::new("r", std::env::temp_dir())
    }

    async fn verdict(url: &str, ctx: &ToolContext) -> std::result::Result<(), String> {
        // No name resolves: only literals and allowlisted/short-circuited cases are decided here.
        request_verdict(url, ctx, |_: String, _: u16| async { Err::<Vec<IpAddr>, _>(std::io::Error::other("no dns")) }).await
    }

    #[tokio::test]
    async fn request_verdicts() {
        let c = ctx();
        for u in ["data:text/html,<b>x</b>", "blob:http://a.example/uuid", "about:blank", "DATA:,x"] {
            assert!(verdict(u, &c).await.is_ok(), "{u}");
        }
        for u in [
            "http://127.0.0.1:9/",
            "https://[::1]/",
            "http://169.254.169.254/latest/meta-data/",
            "http://192.168.1.1/",
            "http://localhost/",
            "ws://127.0.0.1:9222/devtools",
            "wss://10.0.0.1/",
            "file:///etc/passwd",
            "ftp://example.org/",
            "chrome://settings",
            "javascript:alert(1)",
            "",
        ] {
            assert!(verdict(u, &c).await.is_err(), "{u} must be blocked");
        }
        assert!(verdict("http://8.8.8.8/", &c).await.is_ok());
        assert!(verdict("wss://8.8.8.8/socket", &c).await.is_ok());
        // A name that resolves to a private address is blocked, one that resolves to a public address passes.
        let answer =
            |ip: &'static str| move |_: String, _: u16| async move { Ok::<_, std::io::Error>(vec![ip.parse::<IpAddr>().unwrap()]) };
        assert!(request_verdict("https://rebind.example/x", &c, answer("127.0.0.1")).await.is_err());
        assert!(request_verdict("https://rebind.example/x", &c, answer("10.0.0.5")).await.is_err());
        assert!(request_verdict("https://ok.example/x", &c, answer("93.184.216.34")).await.is_ok());
        // The allowlist applies to sub-requests too.
        let mut allow = ctx();
        allow.allow_domains = vec!["ok.example".into()];
        assert!(request_verdict("https://cdn.ok.example/a.js", &allow, answer("93.184.216.34")).await.is_ok());
        let r = request_verdict("https://tracker.example/p.gif", &allow, answer("93.184.216.34")).await;
        assert!(r.unwrap_err().contains("domini consentiti"));
        // Allowing the private network lifts the address checks (but data: etc. stay as they are).
        let mut lan = ctx();
        lan.allow_private_network = true;
        assert!(verdict("http://192.168.1.1/", &lan).await.is_ok());
        assert!(verdict("file:///etc/passwd", &lan).await.is_err());
    }

    #[test]
    fn fetch_covers_every_resource_type() {
        let p = fetch_patterns();
        let pats = p["patterns"].as_array().unwrap();
        assert!(pats.iter().any(|x| x["urlPattern"] == "*" && x.get("resourceType").is_none()));
    }

    /// Spawns a one-shot-per-connection HTTP server; `handler(path)` is the raw response.
    async fn serve(handler: impl Fn(&str) -> String + Send + Sync + 'static) -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (h, handler) = (hits.clone(), Arc::new(handler));
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let (h, handler) = (h.clone(), handler.clone());
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    h.fetch_add(1, Ordering::SeqCst);
                    let _ = s.write_all(handler(&path).as_bytes()).await;
                });
            }
        });
        (port, hits)
    }

    fn html(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// Per-request policy in a real Chrome: a page on a "public" origin (`public.test`, mapped to
    /// the local server by Chrome's host resolver rules; our resolver says it is public) tries
    /// sub-requests, a delayed JS redirect, a link click and an iframe towards loopback and towards
    /// a name that resolves to a private address. None of them may reach the target.
    /// `cargo test -p aiw-tools browser -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn subrequests_and_late_redirects_are_blocked() {
        if find_chrome().is_none() {
            return;
        }
        let (secret_port, secret_hits) = serve(|_| html("<html><title>SEGRETO</title><body>SEGRETO</body></html>")).await;
        let (port, _) = serve(move |p| match p {
            "/ok" => html("fine"),
            "/fetch" => html(&format!(
                "<html><title>Pubblica</title><body><div id=r>attesa</div><iframe src='http://127.0.0.1:{secret_port}/frame'></iframe><script>
                 const t = (u) => fetch(u, {{mode:'no-cors'}}).then(() => 'raggiunto', () => 'bloccato');
                 Promise.all([t('http://127.0.0.1:{secret_port}/x'), t('http://[::1]:{secret_port}/x'), t('http://rebind.test:' + location.port + '/x'), t('/ok')]).then(r => {{
                   const x = new XMLHttpRequest(); x.open('GET', 'http://127.0.0.1:{secret_port}/xhr');
                   x.onloadend = () => {{ document.getElementById('r').textContent = 'esito ' + r.join(',') + ' xhr=' + (x.status === 0 ? 'bloccato' : 'raggiunto'); }};
                   x.send();
                 }});
                 </script></body></html>"
            )),
            "/delayed" => html(&format!(
                "<html><title>Ritardo</title><body>pagina pubblica<script>setTimeout(() => {{ location.href = 'http://127.0.0.1:{secret_port}/late'; }}, 400);</script></body></html>"
            )),
            "/link" => html(&format!("<html><title>Link</title><body><a href='http://127.0.0.1:{secret_port}/click'>vai</a></body></html>")),
            _ => html("<html><title>Radice</title><body>radice</body></html>"),
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("AIW_BROWSER_HEADLESS", "1");
        let resolve: ResolveFn = Arc::new(|host, _| {
            Box::pin(async move {
                match host.as_str() {
                    "public.test" => Ok(vec!["93.184.216.34".parse().unwrap()]),
                    "rebind.test" => Ok(vec!["10.0.0.5".parse().unwrap()]),
                    _ => Err(std::io::Error::other("no dns")),
                }
            })
        });
        let flags =
            vec!["--host-resolver-rules=MAP public.test 127.0.0.1, MAP rebind.test 127.0.0.1".to_string(), "--no-proxy-server".to_string()];
        let mgr = Arc::new(BrowserManager::for_test(dir.path().join("profile"), resolve, flags));
        let t = tools(mgr.clone());
        let ctx = ToolContext::new("r", dir.path().to_path_buf());
        let base = format!("http://public.test:{port}");

        // Sub-requests (fetch, XHR, iframe) to loopback / a private-resolving name are blocked,
        // the same-origin one goes through.
        let nav = t[0].call(json!({"url": format!("{base}/fetch")}), &ctx).await.unwrap();
        assert!(!nav.is_error && nav.content.contains("Titolo: Pubblica"), "{}", nav.content);
        let mut text = String::new();
        for _ in 0..40 {
            text = t[1].call(json!({}), &ctx).await.unwrap().content;
            if text.contains("esito ") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        assert!(text.contains("esito bloccato,bloccato,bloccato,raggiunto xhr=bloccato"), "{text}");

        // A delayed JS redirect to loopback never reaches it.
        let nav = t[0].call(json!({"url": format!("{base}/delayed")}), &ctx).await.unwrap();
        assert!(!nav.is_error && nav.content.contains("pagina pubblica"), "{}", nav.content);
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let after = t[1].call(json!({}), &ctx).await.unwrap().content;
        assert!(!after.contains("SEGRETO"), "{after}");

        // A click on a link to loopback does not either.
        t[0].call(json!({"url": format!("{base}/link")}), &ctx).await.unwrap();
        let clicked = t[2].call(json!({"ref": 1}), &ctx).await.unwrap();
        assert!(!clicked.content.contains("SEGRETO"), "{}", clicked.content);

        assert_eq!(secret_hits.load(Ordering::SeqCst), 0, "the loopback server must never be contacted");
        assert!(mgr.close().await);
    }

    /// Drives a real Chrome/Chromium against a local page. Run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn navigate_snapshot_type_click() {
        if find_chrome().is_none() {
            return;
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let body = if req.starts_with("GET /done") {
                        "<html><head><title>Fatto</title></head><body><h1>Ricerca inviata</h1></body></html>".to_string()
                    } else {
                        "<html><head><title>Comune</title></head><body><h1>Orari anagrafe</h1><form action=\"/done\"><input name=\"q\" placeholder=\"Cerca\"><button type=\"submit\">Cerca</button></form><a href=\"/done\">Vai</a></body></html>".to_string()
                    };
                    let _ = s
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            )
                            .as_bytes(),
                        )
                        .await;
                });
            }
        });
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("AIW_BROWSER_HEADLESS", "1");
        let mgr = Arc::new(BrowserManager::new(dir.path().join("profile")));
        let t = tools(mgr.clone());
        let mut ctx = ToolContext::new("r", dir.path().to_path_buf());
        ctx.allow_private_network = true;
        let nav = t[0].call(json!({"url": format!("http://{addr}/")}), &ctx).await.unwrap();
        assert!(nav.content.contains("Titolo: Comune") && nav.content.contains("Orari anagrafe"), "{}", nav.content);
        assert!(nav.content.contains("[1] input \"Cerca\"") && nav.content.contains("[3] link \"Vai\""), "{}", nav.content);
        let typed = t[3].call(json!({"ref": 1, "text": "carta d'identità"}), &ctx).await.unwrap();
        assert!(typed.content.contains("value=\"carta d'identità\""), "{}", typed.content);
        let clicked = t[2].call(json!({"ref": 3}), &ctx).await.unwrap();
        assert!(!clicked.is_error, "{}", clicked.content);
        tokio::time::sleep(Duration::from_millis(800)).await;
        let snap = t[1].call(json!({}), &ctx).await.unwrap();
        assert!(snap.content.contains("Ricerca inviata"), "{}", snap.content);
        let shot = t[4].call(json!({}), &ctx).await.unwrap();
        assert!(shot.images[0].starts_with("data:image/png;base64,"));
        assert!(mgr.close().await);
    }

    /// A page that redirects to an address outside the policy is dropped. Needs a real Chrome:
    /// `cargo test -p aiw-tools -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn redirect_off_the_allowlist_is_dropped() {
        if find_chrome().is_none() {
            return;
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let resp = if req.starts_with("GET /bounce") {
                        format!("HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/segreto\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    } else {
                        let body = "<html><head><title>Pagina</title></head><body>contenuto</body></html>";
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    };
                    let _ = s.write_all(resp.as_bytes()).await;
                });
            }
        });
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("AIW_BROWSER_HEADLESS", "1");
        let mgr = Arc::new(BrowserManager::new(dir.path().join("profile")));
        let t = tools(mgr.clone());
        let mut ctx = ToolContext::new("r", dir.path().to_path_buf());
        // The server is on loopback: allowed, but only under its numeric name.
        ctx.allow_private_network = true;
        ctx.allow_domains = vec!["127.0.0.1".into()];
        let ok = t[0].call(json!({"url": format!("http://127.0.0.1:{port}/")}), &ctx).await.unwrap();
        assert!(!ok.is_error && ok.content.contains("contenuto"), "{}", ok.content);
        let bounced = t[0].call(json!({"url": format!("http://127.0.0.1:{port}/bounce")}), &ctx).await.unwrap();
        assert!(bounced.is_error && bounced.content.contains("non consentito"), "{}", bounced.content);
        assert!(!bounced.content.contains("contenuto"), "the page content must not reach the model");
        let after = t[1].call(json!({}), &ctx).await.unwrap();
        assert!(after.content.contains("URL: about:blank"), "{}", after.content);
        assert!(mgr.close().await);
    }
}
