//! Browser control over the Chrome DevTools Protocol (pattern P18): launches a dedicated
//! Chrome/Edge/Chromium profile, then exposes navigate / snapshot / click / type / screenshot.
//! Snapshots are text with numbered references so that text-only local models can browse;
//! screenshots are returned as images for vision models.

use crate::web::check_url;
use crate::{arg_str, Risk, Tool, ToolContext, ToolOutput};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

const SNAPSHOT_JS: &str = r#"(() => {
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
    next: AtomicU64,
}

impl Cdp {
    async fn connect(ws_url: &str) -> Result<Self> {
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
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Ok(text) = msg.into_text() else { continue };
                let Ok(v) = serde_json::from_str::<Value>(text.as_ref()) else { continue };
                if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                    if let Some(s) = p2.lock().unwrap().remove(&id) {
                        let _ = s.send(v);
                    }
                } else {
                    let _ = ev2.send(v);
                }
            }
        });
        Ok(Self { tx, pending, events, next: AtomicU64::new(1) })
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (s, r) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, s);
        let msg = json!({"id": id, "method": method, "params": params}).to_string();
        self.tx.send(Message::Text(msg.into())).map_err(|_| Error::Runtime("connessione al browser chiusa".into()))?;
        let v = tokio::time::timeout(Duration::from_secs(30), r)
            .await
            .map_err(|_| Error::Runtime(format!("CDP {method}: timeout")))?
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
        Self { profile_dir, headless, session: tokio::sync::Mutex::new(None) }
    }

    pub fn available(&self) -> std::result::Result<(), String> {
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
        let cdp = Cdp::connect(&ws).await?;
        cdp.call("Page.enable", json!({})).await?;
        cdp.call("Runtime.enable", json!({})).await?;
        Ok(Session { child: Some(child), cdp })
    }

    async fn with<T>(&self, f: impl for<'a> FnOnce(&'a Cdp) -> futures::future::BoxFuture<'a, Result<T>>) -> Result<T> {
        let mut guard = self.session.lock().await;
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

    /// Closes the controlled browser (kill switch).
    pub async fn close(&self) -> bool {
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
enum Op {
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

fn ref_arg(args: &Value) -> Result<u64> {
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
        match self.op {
            Op::Navigate => {
                let url = check_url(arg_str(&args, "url")?, ctx).await?.to_string();
                let policy = ctx.clone();
                self.mgr
                    .with(|cdp| {
                        Box::pin(async move {
                            let mut ev = cdp.events.subscribe();
                            let r = cdp.call("Page.navigate", json!({"url": url})).await?;
                            if let Some(e) = r.get("errorText").and_then(|e| e.as_str()) {
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
                                if let Err(Error::Denied(why)) = check_url(&landed, &policy).await {
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
                    .with(|cdp| {
                        Box::pin(async move { Ok(ToolOutput::text(cdp.eval(SNAPSHOT_JS).await?.as_str().unwrap_or_default().to_string())) })
                    })
                    .await
            }
            Op::Click => {
                let r = ref_arg(&args)?;
                self.mgr
                    .with(|cdp| {
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
                    .with(|cdp| {
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
                    .with(|cdp| {
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
