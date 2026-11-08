//! Browser tools on mobile: the same navigate / snapshot / click / type / screenshot, driven
//! through the app's own WebView (WKWebView on iOS and iPadOS, `android.webkit.WebView` on
//! Android) instead of Chrome over CDP.
//!
//! The native side only implements [`WebViewBridge`] (open a URL, run JavaScript, take a
//! screenshot). The snapshot and the click/type scripts are the desktop ones, and the network
//! policy is enforced for every request of the page by a local HTTPS proxy ([`start_proxy`]): the
//! WebView is routed through it, and it decides each `CONNECT` with the same rules as `web_fetch`
//! and connects to the address it has just vetted (so DNS rebinding cannot swap it afterwards).
//! Only HTTPS is allowed: a plain-HTTP request is refused.

use crate::browser::{ref_arg, request_verdict, vet_with, BrowserManager, Op, Policy, SNAPSHOT_JS};
use crate::{arg_str, ToolContext, ToolOutput};
use aiw_core::{Error, Result};
use serde_json::Value;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Platform side of the in-app browser. Every method blocks (the host calls them from
/// `spawn_blocking`) and is serialised: one page, one call at a time.
pub trait WebViewBridge: Send + Sync + 'static {
    /// Routes the WebView through the policy proxy on `127.0.0.1:proxy_port` (and shows it).
    /// An error means the platform cannot do it (e.g. iOS older than 17): the tool is unusable.
    fn configure(&self, proxy_port: u16) -> Result<()>;
    /// Loads `url`, waits for the load to finish (or a timeout) and returns the final URL.
    fn navigate(&self, url: &str) -> Result<String>;
    /// Runs a JavaScript expression in the page and returns its value (a string for our scripts).
    /// Security note: this is the WebView's own script evaluation (the page is untrusted and
    /// sandboxed from the app), and the scripts are the fixed ones of this module; the only
    /// dynamic part, the text to type, goes in as a JSON string literal.
    fn eval(&self, js: &str) -> Result<Value>;
    /// Screenshot of the page as a `data:image/...;base64,...` URL.
    fn screenshot(&self) -> Result<String>;
    /// Hides the WebView and drops the page.
    fn close(&self);
}

/// The bridge plus the proxy that guards it (started on first use).
pub struct WebViewHost {
    bridge: Arc<dyn WebViewBridge>,
    ready: tokio::sync::OnceCell<()>,
}

impl WebViewHost {
    pub fn new(bridge: Arc<dyn WebViewBridge>) -> Self {
        Self { bridge, ready: tokio::sync::OnceCell::new() }
    }

    async fn ensure(&self, policy: &Policy) -> Result<()> {
        self.ready
            .get_or_try_init(|| async {
                let port = start_proxy(policy.clone()).await.map_err(|e| Error::Runtime(format!("proxy del browser: {e}")))?;
                let b = self.bridge.clone();
                tokio::task::spawn_blocking(move || b.configure(port)).await.map_err(Error::other)?
            })
            .await
            .map(|_| ())
    }

    async fn blocking<T: Send + 'static>(&self, f: impl FnOnce(&dyn WebViewBridge) -> Result<T> + Send + 'static) -> Result<T> {
        let b = self.bridge.clone();
        tokio::task::spawn_blocking(move || f(&*b)).await.map_err(Error::other)?
    }

    pub async fn close(&self) {
        let b = self.bridge.clone();
        let _ = tokio::task::spawn_blocking(move || b.close()).await;
    }
}

const NAVIGATE_TIMEOUT: Duration = Duration::from_secs(40);

fn snapshot_text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// One browser tool call on the WebView (the counterpart of the CDP branch of the tools).
pub(crate) async fn call(mgr: &BrowserManager, host: Arc<WebViewHost>, op: Op, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
    *mgr.policy.ctx.lock().unwrap() = ctx.clone();
    host.ensure(&mgr.policy).await?;
    match op {
        Op::Navigate => {
            let url = arg_str(&args, "url")?.to_string();
            if !url.to_ascii_lowercase().starts_with("https://") {
                return Ok(ToolOutput::error("Su questo dispositivo il browser apre solo pagine https."));
            }
            let resolve = mgr.policy.resolve.clone();
            let url = vet_with(&url, ctx, &resolve).await?.to_string();
            mgr.policy.blocked.lock().unwrap().clear();
            let landed = match tokio::time::timeout(NAVIGATE_TIMEOUT, host.blocking(move |b| b.navigate(&url))).await {
                Ok(r) => r?,
                Err(_) => return Ok(ToolOutput::error("La pagina non ha finito di caricarsi in tempo.")),
            };
            // Redirects are followed by the WebView: the page that actually loaded gets the same check.
            if landed.starts_with("http://") || landed.starts_with("https://") {
                if let Err(Error::Denied(why)) = vet_with(&landed, ctx, &resolve).await {
                    let _ = host.blocking(|b| b.navigate("about:blank")).await;
                    return Ok(ToolOutput::error(format!(
                        "Navigazione annullata: la pagina ha reindirizzato a {landed}, non consentito ({why})"
                    )));
                }
            }
            let snap = host.blocking(|b| b.eval(SNAPSHOT_JS)).await?;
            Ok(ToolOutput::text(snapshot_text(&snap)))
        }
        Op::Snapshot => {
            let snap = host.blocking(|b| b.eval(SNAPSHOT_JS)).await?;
            Ok(ToolOutput::text(snapshot_text(&snap)))
        }
        Op::Click => {
            let r = ref_arg(&args)?;
            let js = format!("(() => {{ const el = document.querySelector('[data-aiw-ref=\"{r}\"]'); if (!el) return 'missing'; el.scrollIntoView({{block:'center'}}); el.click(); return 'ok'; }})()");
            if host.blocking(move |b| b.eval(&js)).await?.as_str() != Some("ok") {
                return Ok(ToolOutput::error(format!("Elemento [{r}] non trovato: rifai browser_snapshot")));
            }
            tokio::time::sleep(Duration::from_millis(900)).await;
            let snap = host.blocking(|b| b.eval(SNAPSHOT_JS)).await?;
            Ok(ToolOutput::text(format!("Cliccato [{r}].\n\n{}", snapshot_text(&snap))))
        }
        Op::Type => {
            let r = ref_arg(&args)?;
            let text = arg_str(&args, "text")?.to_string();
            let submit = args.get("submit").and_then(|v| v.as_bool()).unwrap_or(false);
            let js = type_js(r, &text, submit);
            if host.blocking(move |b| b.eval(&js)).await?.as_str() != Some("ok") {
                return Ok(ToolOutput::error(format!("Campo [{r}] non trovato: rifai browser_snapshot")));
            }
            tokio::time::sleep(Duration::from_millis(if submit { 1500 } else { 200 })).await;
            let snap = host.blocking(|b| b.eval(SNAPSHOT_JS)).await?;
            Ok(ToolOutput::text(format!("Scritto in [{r}].\n\n{}", snapshot_text(&snap))))
        }
        Op::Screenshot => {
            let data = host.blocking(|b| b.screenshot()).await?;
            Ok(ToolOutput { content: "Screenshot della pagina allegato.".into(), images: vec![data], is_error: false })
        }
    }
}

fn type_js(r: u64, text: &str, submit: bool) -> String {
    format!(
        "(() => {{ const el = document.querySelector('[data-aiw-ref=\"{r}\"]'); if (!el) return 'missing'; el.focus();\
         const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;\
         const setter = Object.getOwnPropertyDescriptor(proto, 'value'); if (setter && 'value' in el) setter.set.call(el, {text}); else el.textContent = {text};\
         el.dispatchEvent(new Event('input', {{bubbles:true}})); el.dispatchEvent(new Event('change', {{bubbles:true}}));\
         if ({submit}) {{ if (el.form && el.form.requestSubmit) el.form.requestSubmit(); else el.dispatchEvent(new KeyboardEvent('keydown', {{key:'Enter', bubbles:true}})); }}\
         return 'ok'; }})()",
        text = serde_json::to_string(text).unwrap()
    )
}

// ------------------------------------------------------------------ policy proxy

const HEAD_LIMIT: usize = 8 * 1024;
const VERDICT_TIMEOUT: Duration = Duration::from_secs(8);

/// Starts the policy proxy on a random loopback port and returns the port.
pub(crate) async fn start_proxy(policy: Policy) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    tokio::spawn(async move {
        loop {
            let Ok((conn, _)) = listener.accept().await else { break };
            let policy = policy.clone();
            tokio::spawn(async move {
                let _ = serve(conn, policy).await;
            });
        }
    });
    Ok(port)
}

/// `host:port` of a CONNECT target (IPv6 literals in brackets).
fn split_authority(a: &str) -> Option<(String, u16)> {
    let (host, port) = a.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.is_empty() {
        return None;
    }
    Some((host.to_string(), port.parse().ok()?))
}

async fn reply(conn: &mut TcpStream, status: &str) {
    let _ = conn.write_all(format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").as_bytes()).await;
}

async fn serve(mut conn: TcpStream, policy: Policy) -> std::io::Result<()> {
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = conn.read(&mut buf).await?;
        if n == 0 || head.len() + n > HEAD_LIMIT {
            return Ok(());
        }
        head.extend_from_slice(&buf[..n]);
    }
    let line = String::from_utf8_lossy(&head).lines().next().unwrap_or_default().to_string();
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
    if !method.eq_ignore_ascii_case("CONNECT") {
        reply(&mut conn, "405 Method Not Allowed").await;
        return Ok(());
    }
    let Some((host, port)) = split_authority(target) else {
        reply(&mut conn, "400 Bad Request").await;
        return Ok(());
    };
    let ctx = policy.ctx.lock().unwrap().clone();
    // Resolve once, vet exactly those addresses, connect to one of them.
    let resolved = match tokio::time::timeout(VERDICT_TIMEOUT, (policy.resolve)(host.clone(), port)).await {
        Ok(Ok(ips)) if !ips.is_empty() => ips,
        _ => {
            reply(&mut conn, "502 Bad Gateway").await;
            return Ok(());
        }
    };
    let vetted = resolved.clone();
    let verdict = request_verdict(&format!("https://{target}/"), &ctx, move |_: String, _: u16| {
        let ips: Vec<IpAddr> = vetted.clone();
        async move { Ok::<_, std::io::Error>(ips) }
    })
    .await;
    if let Err(why) = verdict {
        tracing::warn!(%target, %why, "webview: request blocked by network policy");
        {
            let mut b = policy.blocked.lock().unwrap();
            if b.len() < 50 {
                b.push((format!("https://{target}/"), why));
            }
        }
        reply(&mut conn, "403 Forbidden").await;
        return Ok(());
    }
    let mut upstream = None;
    for ip in resolved {
        if let Ok(Ok(s)) = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect((ip, port))).await {
            upstream = Some(s);
            break;
        }
    }
    let Some(mut upstream) = upstream else {
        reply(&mut conn, "502 Bad Gateway").await;
        return Ok(());
    };
    conn.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;
    let _ = tokio::io::copy_bidirectional(&mut conn, &mut upstream).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        proxy_port: Mutex<Option<u16>>,
        navigated: Mutex<Vec<String>>,
        scripts: Mutex<Vec<String>>,
        landed: Mutex<Option<String>>,
    }
    struct FakeBridge(Arc<Fake>);
    impl WebViewBridge for FakeBridge {
        fn configure(&self, port: u16) -> Result<()> {
            *self.0.proxy_port.lock().unwrap() = Some(port);
            Ok(())
        }
        fn navigate(&self, url: &str) -> Result<String> {
            self.0.navigated.lock().unwrap().push(url.to_string());
            Ok(self.0.landed.lock().unwrap().clone().unwrap_or_else(|| url.to_string()))
        }
        fn eval(&self, js: &str) -> Result<Value> {
            self.0.scripts.lock().unwrap().push(js.to_string());
            Ok(Value::String(if js.contains("el.click()") || js.contains("setter") { "ok".into() } else { "SNAPSHOT".into() }))
        }
        fn screenshot(&self) -> Result<String> {
            Ok("data:image/jpeg;base64,AAAA".into())
        }
        fn close(&self) {}
    }

    fn manager(fake: &Arc<Fake>, ip: &'static str) -> BrowserManager {
        let mut m = BrowserManager::new(std::env::temp_dir().join("aiw-webview-test"));
        m.policy.resolve = Arc::new(move |_, _| Box::pin(async move { Ok(vec![ip.parse().unwrap()]) }));
        m.set_webview(Arc::new(FakeBridge(fake.clone())));
        m
    }

    fn ctx() -> ToolContext {
        ToolContext::new("r", std::env::temp_dir())
    }

    async fn run(m: &BrowserManager, op: Op, args: Value) -> ToolOutput {
        call(m, m.webview().unwrap(), op, args, &ctx()).await.unwrap()
    }

    #[tokio::test]
    async fn navigate_snapshot_click_type_and_screenshot() {
        let fake = Arc::new(Fake::default());
        let m = manager(&fake, "93.184.216.34");
        let out = run(&m, Op::Navigate, serde_json::json!({"url": "https://example.org/"})).await;
        assert!(!out.is_error && out.content == "SNAPSHOT", "{}", out.content);
        assert!(fake.proxy_port.lock().unwrap().is_some(), "the WebView is routed through the proxy");
        assert!(run(&m, Op::Click, serde_json::json!({"ref": 3})).await.content.starts_with("Cliccato [3]"));
        assert!(run(&m, Op::Type, serde_json::json!({"ref": 2, "text": "ciao \"x\"", "submit": true}))
            .await
            .content
            .starts_with("Scritto in [2]"));
        assert!(fake.scripts.lock().unwrap().iter().any(|s| s.contains(r#"ciao \"x\""#)), "text is escaped as JSON");
        assert_eq!(run(&m, Op::Screenshot, serde_json::json!({})).await.images, vec!["data:image/jpeg;base64,AAAA".to_string()]);
    }

    #[tokio::test]
    async fn refuses_http_private_addresses_and_private_redirects() {
        let fake = Arc::new(Fake::default());
        let m = manager(&fake, "93.184.216.34");
        assert!(run(&m, Op::Navigate, serde_json::json!({"url": "http://example.org/"})).await.is_error);
        let private = manager(&fake, "10.0.0.5");
        let r =
            call(&private, private.webview().unwrap(), Op::Navigate, serde_json::json!({"url": "https://intranet.example/"}), &ctx()).await;
        assert!(r.is_err() || r.unwrap().is_error);
        assert!(fake.navigated.lock().unwrap().is_empty(), "nothing is loaded before the policy says yes");
        // A public page that bounces to the LAN is dropped before the snapshot.
        *fake.landed.lock().unwrap() = Some("https://192.168.1.1/admin".into());
        let out = run(&m, Op::Navigate, serde_json::json!({"url": "https://example.org/"})).await;
        assert!(out.is_error && out.content.contains("reindirizzato"), "{}", out.content);
        assert_eq!(fake.navigated.lock().unwrap().last().map(String::as_str), Some("about:blank"));
    }

    #[test]
    fn parses_connect_targets() {
        assert_eq!(split_authority("example.org:443"), Some(("example.org".into(), 443)));
        assert_eq!(split_authority("[::1]:8443"), Some(("::1".into(), 8443)));
        assert_eq!(split_authority("nohost"), None);
        assert_eq!(split_authority(":443"), None);
    }

    async fn connect_via_proxy(port: u16, target: &str) -> String {
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").as_bytes()).await.unwrap();
        let mut buf = [0u8; 256];
        let n = c.read(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or_default().to_string()
    }

    #[tokio::test]
    async fn proxy_tunnels_public_hosts_and_blocks_the_rest() {
        // A local "origin" that the resolver pretends is the public address of the name.
        let origin = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let origin_port = origin.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = origin.accept().await else { break };
                tokio::spawn(async move {
                    let mut b = [0u8; 16];
                    if let Ok(n) = s.read(&mut b).await {
                        let _ = s.write_all(&b[..n]).await;
                    }
                });
            }
        });
        let policy = |ip: IpAddr| Policy {
            ctx: Arc::new(Mutex::new(ctx())),
            resolve: Arc::new(move |_, _| Box::pin(async move { Ok(vec![ip]) })),
            blocked: Default::default(),
        };
        // Private answer (rebinding): refused and recorded.
        let blocked = policy(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        let log = blocked.blocked.clone();
        let port = start_proxy(blocked).await.unwrap();
        assert!(connect_via_proxy(port, &format!("rebind.example:{origin_port}")).await.contains("403"));
        assert_eq!(log.lock().unwrap().len(), 1);
        // Plain HTTP proxying is not offered.
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(b"GET http://example.org/ HTTP/1.1\r\n\r\n").await.unwrap();
        let mut buf = [0u8; 64];
        let n = c.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).contains("405"));
        // With the private network allowed (policy switch) the same tunnel works end to end.
        let lan = policy(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        lan.ctx.lock().unwrap().allow_private_network = true;
        let port = start_proxy(lan).await.unwrap();
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(format!("CONNECT lan.example:{origin_port} HTTP/1.1\r\n\r\n").as_bytes()).await.unwrap();
        let mut head = [0u8; 39];
        c.read_exact(&mut head).await.unwrap();
        assert!(String::from_utf8_lossy(&head).starts_with("HTTP/1.1 200"));
        c.write_all(b"ping").await.unwrap();
        let mut echo = [0u8; 4];
        c.read_exact(&mut echo).await.unwrap();
        assert_eq!(&echo, b"ping");
    }
}
