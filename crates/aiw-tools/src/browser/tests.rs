//! Tests of the browser tools: pure helpers, a fake CDP endpoint (always run) and a real
//! headless Chromium against local pages (`#[ignore]`, they skip themselves when no browser is
//! found: `cargo test -p aiw-tools browser -- --ignored`).

use super::*;
use std::sync::atomic::AtomicUsize;

fn tool(t: &[Arc<dyn Tool>], name: &str) -> Arc<dyn Tool> {
    t.iter().find(|x| x.name() == name).unwrap_or_else(|| panic!("tool {name} not registered")).clone()
}

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
    let answer = |ip: &'static str| move |_: String, _: u16| async move { Ok::<_, std::io::Error>(vec![ip.parse::<IpAddr>().unwrap()]) };
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
    let nav = tool(&t, "browser_navigate").call(json!({"url": format!("{base}/fetch")}), &ctx).await.unwrap();
    assert!(!nav.is_error && nav.content.contains("Titolo: Pubblica"), "{}", nav.content);
    let mut text = String::new();
    for _ in 0..40 {
        text = tool(&t, "browser_snapshot").call(json!({}), &ctx).await.unwrap().content;
        if text.contains("esito ") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(text.contains("esito bloccato,bloccato,bloccato,raggiunto xhr=bloccato"), "{text}");

    // A delayed JS redirect to loopback never reaches it.
    let nav = tool(&t, "browser_navigate").call(json!({"url": format!("{base}/delayed")}), &ctx).await.unwrap();
    assert!(!nav.is_error && nav.content.contains("pagina pubblica"), "{}", nav.content);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let after = tool(&t, "browser_snapshot").call(json!({}), &ctx).await.unwrap().content;
    assert!(!after.contains("SEGRETO"), "{after}");

    // A click on a link to loopback does not either.
    tool(&t, "browser_navigate").call(json!({"url": format!("{base}/link")}), &ctx).await.unwrap();
    let clicked = tool(&t, "browser_click").call(json!({"ref": 1}), &ctx).await.unwrap();
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
    let nav = tool(&t, "browser_navigate").call(json!({"url": format!("http://{addr}/")}), &ctx).await.unwrap();
    assert!(nav.content.contains("Titolo: Comune") && nav.content.contains("Orari anagrafe"), "{}", nav.content);
    assert!(nav.content.contains("[1] input \"Cerca\"") && nav.content.contains("[3] link \"Vai\""), "{}", nav.content);
    let typed = tool(&t, "browser_type").call(json!({"ref": 1, "text": "carta d'identità"}), &ctx).await.unwrap();
    assert!(typed.content.contains("value=\"carta d'identità\""), "{}", typed.content);
    let clicked = tool(&t, "browser_click").call(json!({"ref": 3}), &ctx).await.unwrap();
    assert!(!clicked.is_error, "{}", clicked.content);
    tokio::time::sleep(Duration::from_millis(800)).await;
    let snap = tool(&t, "browser_snapshot").call(json!({}), &ctx).await.unwrap();
    assert!(snap.content.contains("Ricerca inviata"), "{}", snap.content);
    let shot = tool(&t, "browser_screenshot").call(json!({}), &ctx).await.unwrap();
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
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/segreto\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
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
    let ok = tool(&t, "browser_navigate").call(json!({"url": format!("http://127.0.0.1:{port}/")}), &ctx).await.unwrap();
    assert!(!ok.is_error && ok.content.contains("contenuto"), "{}", ok.content);
    let bounced = tool(&t, "browser_navigate").call(json!({"url": format!("http://127.0.0.1:{port}/bounce")}), &ctx).await.unwrap();
    assert!(bounced.is_error && bounced.content.contains("non consentito"), "{}", bounced.content);
    assert!(!bounced.content.contains("contenuto"), "the page content must not reach the model");
    let after = tool(&t, "browser_snapshot").call(json!({}), &ctx).await.unwrap();
    assert!(after.content.contains("URL: about:blank"), "{}", after.content);
    assert!(mgr.close().await);
}

// ---------------------------------------------------------------------- real Chromium

/// A real browser with the policy in force: `public.test` is mapped to the local server by
/// Chrome's resolver rules and counts as a public name for ours; `rebind.test` as a private one.
struct Real {
    mgr: Arc<BrowserManager>,
    t: Vec<Arc<dyn Tool>>,
    ctx: ToolContext,
    dir: tempfile::TempDir,
    base: String,
}

impl Real {
    async fn start(handler: impl Fn(&str) -> String + Send + Sync + 'static) -> Option<Real> {
        if find_chrome().is_none() {
            eprintln!("skipped: no Chrome/Chromium found");
            return None;
        }
        let (port, _) = serve(handler).await;
        Some(Self::on_port(port))
    }

    fn on_port(port: u16) -> Real {
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
        let ctx = ToolContext::new("run1", dir.path().to_path_buf());
        Real { mgr, t, ctx, dir, base: format!("http://public.test:{port}") }
    }

    async fn run(&self, name: &str, args: Value) -> ToolOutput {
        tool(&self.t, name).call(args, &self.ctx).await.unwrap_or_else(|e| ToolOutput::error(format!("ERR: {e}")))
    }

    async fn go(&self, path: &str) -> ToolOutput {
        let out = self.run("browser_navigate", json!({"url": format!("{}{path}", self.base)})).await;
        assert!(!out.is_error, "{}", out.content);
        out
    }
}

/// The `[ref]` of the first snapshot line that contains `needle`.
fn ref_of(snapshot: &str, needle: &str) -> String {
    let line = snapshot
        .lines()
        .map(str::trim_start)
        .find(|l| l.starts_with('[') && l.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle} in:\n{snapshot}"));
    line[1..line.find(']').unwrap()].to_string()
}

#[tokio::test]
#[ignore]
async fn real_accessibility_snapshot_forms_hover_and_text() {
    let page = r#"<html><head><title>Modulo</title></head><body><h1>Iscrizione</h1>
      <form onsubmit="return false"><label for=n>Nome</label><input id=n name=n>
      <label><input type=checkbox id=c> Accetto</label>
      <label><input type=radio name=r value=a id=ra> Opzione A</label><label><input type=radio name=r value=b id=rb> Opzione B</label>
      <label for=s>Paese</label><select id=s><option>Italia</option><option>Francia</option></select>
      <button type=button onclick="document.getElementById('out').textContent='inviato'">Invia</button></form>
      <div role=button tabindex=0 onclick="document.getElementById('out').textContent='speciale'">Pulsante speciale</div>
      <button id=menu onmouseover="document.getElementById('sub').style.display='block'">Menu</button><div id=sub style="display:none">Sottovoce</div>
      <div id=out>vuoto</div><p>Un testo di prova che deve comparire nella versione leggibile.</p></body></html>"#;
    let Some(r) = Real::start(move |_| html(page)).await else { return };
    r.go("/").await;

    // Accessibility tree: roles, names, state; stable refs that click/type/select accept.
    let ax = r.run("browser_snapshot", json!({"format": "ax"})).await;
    assert!(!ax.is_error, "{}", ax.content);
    for want in [
        "heading[1] \"Iscrizione\"",
        "textbox \"Nome\"",
        "checkbox \"Accetto\" (non selezionato)",
        "combobox \"Paese\"",
        "button \"Invia\"",
        "button \"Pulsante speciale\"",
    ] {
        assert!(ax.content.contains(want), "missing {want}:\n{}", ax.content);
    }
    let (name, check, radio, select, special) = (
        ref_of(&ax.content, "textbox \"Nome\""),
        ref_of(&ax.content, "checkbox \"Accetto\""),
        ref_of(&ax.content, "radio \"Opzione B\""),
        ref_of(&ax.content, "combobox \"Paese\""),
        ref_of(&ax.content, "button \"Pulsante speciale\""),
    );
    let typed = r.run("browser_type", json!({"ref": name, "text": "Mario Rossi"})).await;
    assert!(!typed.is_error && typed.content.contains("textbox \"Nome\" value=\"Mario Rossi\""), "{}", typed.content);
    // Checkbox and radio toggle through click on refs.
    let on = r.run("browser_click", json!({"ref": check})).await;
    assert!(on.content.contains("checkbox \"Accetto\" (selezionato)"), "{}", on.content);
    let off = r.run("browser_click", json!({"ref": check})).await;
    assert!(off.content.contains("checkbox \"Accetto\" (non selezionato)"), "{}", off.content);
    let rad = r.run("browser_click", json!({"ref": radio})).await;
    assert!(
        rad.content.contains("radio \"Opzione B\" (selezionato)") && rad.content.contains("radio \"Opzione A\" (non selezionato)"),
        "{}",
        rad.content
    );
    // Select by text, then by value; an unknown option lists the real ones.
    let sel = r.run("browser_select", json!({"ref": select, "option": "Francia"})).await;
    assert!(!sel.is_error && sel.content.starts_with("Scelto «Francia»"), "{}", sel.content);
    assert_eq!(r.run("browser_eval", json!({"expression": "document.getElementById('s').value"})).await.content, "\"Francia\"");
    let bad = r.run("browser_select", json!({"ref": select, "option": "Atlantide"})).await;
    assert!(bad.is_error && bad.content.contains("Italia"), "{}", bad.content);
    let not_select = r.run("browser_select", json!({"ref": check, "option": "x"})).await;
    assert!(not_select.is_error && not_select.content.contains("non è un menu"), "{}", not_select.content);
    // A custom widget (role=button on a div) clicks too, and the refs did not change meanwhile.
    let sp = r.run("browser_click", json!({"ref": special})).await;
    assert!(sp.content.contains("speciale") && sp.content.contains(&format!("[{special}] button \"Pulsante speciale\"")), "{}", sp.content);
    // A ref that does not exist.
    assert!(r.run("browser_click", json!({"ref": "e999"})).await.is_error);

    // Hover opens the menu (a real pointer move), by number from the elements snapshot.
    let el = r.run("browser_snapshot", json!({})).await;
    assert!(el.content.contains("Elementi interattivi") && !el.content.contains("Sottovoce"), "{}", el.content);
    let h = r.run("browser_hover", json!({"ref": ref_of(&el.content, "button \"Menu\"")})).await;
    assert!(!h.is_error && h.content.contains("Sottovoce"), "{}", h.content);

    // Readable text: no markup, the paragraph is there, the page says it is complete.
    let text = r.run("browser_snapshot", json!({"format": "text"})).await;
    assert!(
        text.content.contains("Titolo: Modulo") && text.content.contains("Un testo di prova") && !text.content.contains("<p>"),
        "{}",
        text.content
    );
    assert!(text.content.contains("fine della pagina"), "{}", text.content);
    assert!(r.run("browser_snapshot", json!({"format": "xml"})).await.is_error);
    assert!(r.mgr.close().await);
}

#[tokio::test]
#[ignore]
async fn real_tabs_site_opened_pages_and_the_network_policy() {
    if find_chrome().is_none() {
        eprintln!("skipped: no Chrome/Chromium found");
        return;
    }
    let (secret_port, secret_hits) = serve(|_| html("<html><title>SEGRETO</title><body>SEGRETO</body></html>")).await;
    let (port, _) = serve(move |p| match p {
        "/b" => html("<html><title>Seconda</title><body>pagina due</body></html>"),
        "/c" => html("<html><title>Terza</title><body>pagina tre</body></html>"),
        _ => html(&format!(
            "<html><title>Prima</title><body><a href='/b' target=_blank>apri</a><button onclick=\"window.open('/c')\">popup</button>\
             <button onclick=\"window.open('http://127.0.0.1:{secret_port}/x')\">segreto</button></body></html>"
        )),
    })
    .await;
    let r = Real::on_port(port);
    r.go("/").await;
    let list = r.run("browser_tabs", json!({})).await;
    assert!(
        list.content.contains("[1] «Prima»") && list.content.contains("(attiva)") && list.content.contains("1 su 8"),
        "{}",
        list.content
    );

    // target=_blank: the new page is listed, announced and becomes the active one.
    let snap = r.run("browser_snapshot", json!({})).await;
    let clicked = r.run("browser_click", json!({"ref": ref_of(&snap.content, "link \"apri\"")})).await;
    assert!(clicked.content.contains("Il sito ha aperto una nuova scheda [2]"), "{}", clicked.content);
    assert!(clicked.content.contains("Scheda attiva: [2] di 2") && clicked.content.contains("pagina due"), "{}", clicked.content);
    let list = r.run("browser_tabs", json!({})).await;
    assert!(list.content.contains("[2] «Seconda»") && list.content.contains("(aperta dal sito)"), "{}", list.content);

    // Back to the first one, window.open of an allowed page, then of loopback: never reaches it.
    let first = r.run("browser_tab_select", json!({"index": 1})).await;
    assert!(first.content.contains("Scheda [1] attiva") && first.content.contains("apri"), "{}", first.content);
    let snap = r.run("browser_snapshot", json!({})).await;
    let popup = r.run("browser_click", json!({"ref": ref_of(&snap.content, "button \"popup\"")})).await;
    assert!(popup.content.contains("nuova scheda [3]"), "{}", popup.content);
    r.run("browser_tab_select", json!({"index": 1})).await;
    let snap = r.run("browser_snapshot", json!({})).await;
    r.run("browser_click", json!({"ref": ref_of(&snap.content, "button \"segreto\"")})).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(secret_hits.load(Ordering::SeqCst), 0, "a page opened by the site must go through the network policy");
    let list = r.run("browser_tabs", json!({})).await;
    assert!(!list.content.contains("SEGRETO"), "{}", list.content);

    // New tab on an address, a refused address, close, and the cap.
    let new = r.run("browser_tab_new", json!({"url": format!("{}/c", r.base)})).await;
    assert!(new.content.contains("Nuova scheda") && new.content.contains("pagina tre"), "{}", new.content);
    let denied = tool(&r.t, "browser_tab_new").call(json!({"url": format!("http://127.0.0.1:{secret_port}/")}), &r.ctx).await;
    assert!(denied.is_err() || denied.unwrap().is_error);
    assert_eq!(secret_hits.load(Ordering::SeqCst), 0);
    let before = r.run("browser_tabs", json!({})).await.content;
    let n = before.lines().filter(|l| l.starts_with('[')).count();
    let closed = r.run("browser_tab_close", json!({"index": n})).await;
    assert!(
        closed.content.contains("chiusa") && closed.content.lines().filter(|l| l.starts_with('[')).count() == n - 1,
        "{}",
        closed.content
    );
    let mut last = ToolOutput::default();
    for _ in 0..10 {
        last = r.run("browser_tab_new", json!({})).await;
        if last.is_error {
            break;
        }
    }
    assert!(last.is_error && last.content.contains("già 8 schede"), "{}", last.content);
    assert!(r.run("browser_tab_select", json!({"index": 42})).await.is_error);
    assert!(r.mgr.close().await);
}

#[tokio::test]
#[ignore]
async fn real_waits_dialogs_console_and_eval() {
    let page = r#"<html><head><title>Prova</title></head><body>
      <button onclick="alert('ciao'); document.getElementById('o').textContent='dopo alert'">A</button>
      <button onclick="document.getElementById('o').textContent = confirm('Sicuro?') ? 'si' : 'no'">C</button>
      <button onclick="setTimeout(() => { document.getElementById('late').textContent = 'arrivato'; document.getElementById('gone').remove(); }, 700)">L</button>
      <button onclick="console.error('boom'); console.warn('attenzione'); fetch('/mancante.json')">E</button>
      <button onclick="setTimeout(() => { location.href = '/altra'; }, 1500)">N</button>
      <div id=o></div><div id=late>attesa</div><div id=gone>da togliere</div></body></html>"#;
    let Some(r) =
        Real::start(move |p| if p == "/altra" { html("<html><title>Altra</title><body>altra pagina</body></html>") } else { html(page) })
            .await
    else {
        return;
    };
    r.go("/").await;
    let snap = r.run("browser_snapshot", json!({})).await.content;

    // Dialogs never hang a call: the text is reported and the page goes on.
    let a = r.run("browser_click", json!({"ref": ref_of(&snap, "button \"A\"")})).await;
    assert!(!a.is_error && a.content.contains("Finestra del sito (alert): «ciao»") && a.content.contains("dopo alert"), "{}", a.content);
    let c = r.run("browser_click", json!({"ref": ref_of(&snap, "button \"C\"")})).await;
    assert!(c.content.contains("(confirm): «Sicuro?»") && c.content.contains("annullata") && c.content.contains("\nno\n"), "{}", c.content);

    // Waits: text appears, another disappears, a selector, a ref, a timeout.
    r.run("browser_click", json!({"ref": ref_of(&snap, "button \"L\"")})).await;
    let w = r.run("browser_wait", json!({"text": "arrivato", "text_gone": "da togliere", "ms": 5000})).await;
    assert!(w.content.starts_with("Atteso: il testo «arrivato» è comparso, il testo «da togliere» è scomparso"), "{}", w.content);
    let sel = r.run("browser_wait", json!({"selector": "#late", "ms": 2000})).await;
    assert!(sel.content.starts_with("Atteso: «#late» è presente"), "{}", sel.content);
    let t = std::time::Instant::now();
    let none = r.run("browser_wait", json!({"text": "mai", "ms": 600})).await;
    assert!(
        none.content.starts_with("Non soddisfatto entro 600 ms: il testo «mai» non è comparso") && t.elapsed() < Duration::from_secs(5),
        "{}",
        none.content
    );
    let bad = r.run("browser_wait", json!({"selector": "div[", "ms": 500})).await;
    assert!(bad.is_error && bad.content.contains("selettore"), "{}", bad.content);
    // Navigation and network idle.
    let snap = r.run("browser_snapshot", json!({})).await.content;
    r.run("browser_click", json!({"ref": ref_of(&snap, "button \"N\"")})).await;
    let nav = r.run("browser_wait", json!({"navigation": true, "ms": 6000})).await;
    assert!(nav.content.starts_with("Atteso: la pagina ha navigato") && nav.content.contains("altra pagina"), "{}", nav.content);
    let idle = r.run("browser_wait", json!({"network_idle": true, "ms": 5000})).await;
    assert!(idle.content.starts_with("Atteso: la rete è inattiva"), "{}", idle.content);

    // Console: errors and warnings, including the failed request.
    r.go("/").await;
    let snap = r.run("browser_snapshot", json!({})).await.content;
    r.run("browser_click", json!({"ref": ref_of(&snap, "button \"E\"")})).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let console = r.run("browser_console", json!({})).await;
    assert!(console.content.contains("errore: boom") && console.content.contains("avviso: attenzione"), "{}", console.content);
    assert!(r.run("browser_console", json!({"clear": true})).await.content.contains("boom"));
    assert!(r.run("browser_console", json!({})).await.content.contains("Nessun errore"));

    // Eval: JSON result, promises, errors, big results are cut.
    assert_eq!(r.run("browser_eval", json!({"expression": "document.title"})).await.content, "\"Prova\"");
    assert_eq!(r.run("browser_eval", json!({"expression": "[1,2,3].map(x => x * 2)"})).await.content, "[2,4,6]");
    assert_eq!(
        r.run("browser_eval", json!({"expression": "new Promise(r => setTimeout(() => r({a: 7}), 50))"})).await.content,
        "{\"a\":7}"
    );
    let err = r.run("browser_eval", json!({"expression": "nonEsiste.x"})).await;
    assert!(err.is_error && err.content.contains("Errore JavaScript"), "{}", err.content);
    let big = r.run("browser_eval", json!({"expression": "'x'.repeat(50000)"})).await;
    assert!(big.content.contains("troncato") && big.content.chars().count() < 20_200, "{}", big.content.len());
    assert!(r.mgr.close().await);
}

#[tokio::test]
#[ignore]
async fn real_downloads_and_upload() {
    if find_chrome().is_none() {
        eprintln!("skipped: no Chrome/Chromium found");
        return;
    }
    let (secret_port, secret_hits) = serve(|_| html("SEGRETO")).await;
    let (port, _) = serve(move |p| match p {
        "/file.txt" => "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"../rapporto.txt\"\r\nContent-Length: 12\r\nConnection: close\r\n\r\nciao scarico".to_string(),
        "/up" => html("<html><title>Carica</title><body><input type=file id=f><div id=o>nulla</div><script>f.onchange = () => { o.textContent = 'file:' + f.files[0].name + ':' + f.files.length; };</script><button>x</button></body></html>"),
        _ => html(&format!("<html><title>Scarichi</title><body><a href='/file.txt'>scarica</a><a href='http://127.0.0.1:{secret_port}/f.bin' download>segreto</a></body></html>")),
    })
    .await;
    let mut r = Real::on_port(port);
    r.ctx.checkpoint_dir = Some(r.dir.path().join("ckpt"));
    r.go("/").await;
    let snap = r.run("browser_snapshot", json!({})).await.content;
    let got = r.run("browser_click", json!({"ref": ref_of(&snap, "link \"scarica\"")})).await;
    assert!(
        !got.is_error && got.content.contains("Scaricato «_rapporto.txt» (12 byte) in downloads/run1/_rapporto.txt"),
        "{}",
        got.content
    );
    let saved = r.dir.path().join("downloads/run1/_rapporto.txt");
    assert_eq!(
        std::fs::read_to_string(&saved).unwrap(),
        "ciao scarico",
        "the name sent by the server is sanitised, the file is in the run's folder"
    );
    assert!(!r.dir.path().join("_rapporto.txt").exists() && !r.dir.path().join("rapporto.txt").exists());
    // Recorded for the rewind.
    let meta =
        crate::checkpoint::read_meta(&crate::checkpoint::run_dir(r.ctx.checkpoint_dir.as_ref().unwrap(), "run1")).expect("checkpoint meta");
    assert!(meta.created.iter().any(|c| c.ends_with("_rapporto.txt")), "{:?}", meta.created);
    // A navigation to a file is a download too (second copy gets another name).
    let nav = r.run("browser_navigate", json!({"url": format!("{}/file.txt", r.base)})).await;
    assert!(!nav.is_error && nav.content.contains("_rapporto (1).txt"), "{}", nav.content);
    let list = r.run("browser_downloads", json!({})).await.content;
    assert!(list.contains("[1] Scaricato «_rapporto.txt»") && list.contains("[2] Scaricato «_rapporto.txt»"), "{list}");
    // The download URL goes through the network policy.
    let seg = r.run("browser_click", json!({"ref": ref_of(&snap, "link \"segreto\"")})).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(secret_hits.load(Ordering::SeqCst), 0, "{}", seg.content);
    assert!(!r.run("browser_downloads", json!({})).await.content.contains("[3]"));
    // Read-only sandbox: refused, nothing written.
    r.ctx.sandbox = crate::SandboxMode::ReadOnly;
    r.go("/").await;
    let snap = r.run("browser_snapshot", json!({})).await.content;
    let refused = r.run("browser_click", json!({"ref": ref_of(&snap, "link \"scarica\"")})).await;
    assert!(refused.content.contains("Download non riuscito") && refused.content.contains("sola lettura"), "{}", refused.content);
    assert_eq!(std::fs::read_dir(r.dir.path().join("downloads/run1")).unwrap().count(), 2);
    r.ctx.sandbox = crate::SandboxMode::WorkspaceWrite;

    // Upload: a file of the workspace or of a read root; nothing else.
    std::fs::write(r.dir.path().join("doc.txt"), "contenuto").unwrap();
    let att = tempfile::tempdir().unwrap();
    std::fs::write(att.path().join("allegato.txt"), "a").unwrap();
    r.ctx.read_roots = vec![att.path().to_path_buf()];
    r.go("/up").await;
    let snap = r.run("browser_snapshot", json!({})).await.content;
    let field = ref_of(&snap, "input[file]");
    let up = r.run("browser_upload", json!({"ref": field, "path": "doc.txt"})).await;
    assert!(!up.is_error && up.content.contains("file:doc.txt:1"), "{}", up.content);
    let up2 = r.run("browser_upload", json!({"ref": field, "path": att.path().join("allegato.txt").to_string_lossy()})).await;
    assert!(up2.content.contains("file:allegato.txt:1"), "{}", up2.content);
    let out = tool(&r.t, "browser_upload").call(json!({"ref": field, "path": "/etc/passwd"}), &r.ctx).await;
    assert!(matches!(out, Err(Error::Denied(_))), "{out:?}");
    let not_input = r.run("browser_upload", json!({"ref": ref_of(&snap, "button \"x\""), "path": "doc.txt"})).await;
    assert!(not_input.is_error && not_input.content.contains("non è un campo"), "{}", not_input.content);
    assert!(r.mgr.close().await);
}

// ---------------------------------------------------------------------- fake CDP endpoint

use fake::{attached, event, FakeCdp, FakeState, Reply, Req};

struct Fk {
    fake: FakeCdp,
    mgr: Arc<BrowserManager>,
    t: Vec<Arc<dyn Tool>>,
    ctx: ToolContext,
    dir: tempfile::TempDir,
}

impl Fk {
    async fn new(custom: impl Fn(&Req, &mut FakeState) -> Option<Reply> + Send + Sync + 'static) -> Fk {
        let fake = FakeCdp::start(custom).await;
        let dir = tempfile::tempdir().unwrap();
        let resolve: ResolveFn = Arc::new(|_, _| Box::pin(async { Err(std::io::Error::other("no dns")) }));
        let mgr = Arc::new(BrowserManager::for_endpoint(dir.path().join("profile"), fake.url.clone(), resolve));
        let t = tools(mgr.clone());
        let ctx = ToolContext::new("run1", dir.path().to_path_buf());
        Fk { fake, mgr, t, ctx, dir }
    }

    async fn plain() -> Fk {
        Self::new(|_, _| None).await
    }

    async fn run(&self, name: &str, args: Value) -> ToolOutput {
        tool(&self.t, name).call(args, &self.ctx).await.unwrap_or_else(|e| ToolOutput::error(format!("ERR: {e}")))
    }

    /// Lets the events pushed so far reach the reader task.
    async fn settle(&self) {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    fn sent(&self, method: &str, session: &str) -> Vec<Req> {
        self.fake.commands(method).into_iter().filter(|r| r.session.as_deref() == Some(session)).collect()
    }
}

fn ax_nodes() -> Value {
    json!([
        {"nodeId": "1", "role": {"value": "RootWebArea"}, "name": {"value": "P"}, "childIds": ["2", "3"], "properties": []},
        {"nodeId": "2", "parentId": "1", "role": {"value": "checkbox"}, "name": {"value": "Accetto"}, "backendDOMNodeId": 6, "childIds": [],
         "properties": [{"name": "checked", "value": {"value": "false"}}]},
        {"nodeId": "3", "parentId": "1", "role": {"value": "button"}, "name": {"value": "Invia"}, "backendDOMNodeId": 7, "childIds": [], "properties": []}
    ])
}

fn str_value(v: &str) -> Reply {
    Reply::ok(json!({"result": {"type": "string", "value": v}}))
}

#[tokio::test]
async fn fake_start_up_attaches_the_page_with_the_policy_before_resuming_it() {
    let fk = Fk::plain().await;
    let out = fk.run("browser_tabs", json!({})).await;
    assert!(out.content.contains("1 su 8") && out.content.contains("[1]") && out.content.contains("(attiva)"), "{}", out.content);
    let auto = fk.fake.commands("Target.setAutoAttach");
    assert!(
        auto[0].session.is_none() && auto[0].params["waitForDebuggerOnStart"] == true && auto[0].params["flatten"] == true,
        "{:?}",
        auto[0]
    );
    let dl = &fk.fake.commands("Browser.setDownloadBehavior")[0];
    assert_eq!(dl.params["behavior"], "allowAndName");
    assert!(dl.params["downloadPath"].as_str().unwrap().ends_with("downloads-tmp"), "downloads never go straight to the workspace");
    let fetch = fk.fake.position(|r| r.method == "Fetch.enable" && r.session.as_deref() == Some("S1")).expect("Fetch.enable on the page");
    let resume = fk.fake.position(|r| r.method == "Runtime.runIfWaitingForDebugger" && r.session.as_deref() == Some("S1")).unwrap();
    assert!(fetch < resume, "the policy is in place before the page runs");
    assert!(fk.mgr.close().await);
}

#[tokio::test]
async fn fake_a_page_opened_by_the_site_is_paused_vetted_and_listed() {
    let fk = Fk::plain().await;
    fk.run("browser_tabs", json!({})).await;
    fk.fake.push(attached(None, "T9", "S9", "about:blank", Some("T1")));
    fk.fake.wait_for(|r| r.method == "Runtime.runIfWaitingForDebugger" && r.session.as_deref() == Some("S9")).await;
    let fetch = fk.fake.position(|r| r.method == "Fetch.enable" && r.session.as_deref() == Some("S9")).expect("Fetch.enable on the popup");
    let resume = fk.fake.position(|r| r.method == "Runtime.runIfWaitingForDebugger" && r.session.as_deref() == Some("S9")).unwrap();
    assert!(fetch < resume, "the popup gets the network policy before it runs anything");
    // Its requests are decided by the same policy as every other.
    fk.fake.push(event(Some("S9"), "Fetch.requestPaused", json!({"requestId": "r-bad", "request": {"url": "http://127.0.0.1:9/secret"}})));
    fk.fake.push(event(Some("S9"), "Fetch.requestPaused", json!({"requestId": "r-ok", "request": {"url": "http://8.8.8.8/x"}})));
    let failed = fk.fake.wait_for(|r| r.method == "Fetch.failRequest").await;
    assert_eq!(
        (failed.session.as_deref(), failed.params["requestId"].as_str(), failed.params["errorReason"].as_str()),
        (Some("S9"), Some("r-bad"), Some("BlockedByClient"))
    );
    let cont = fk.fake.wait_for(|r| r.method == "Fetch.continueRequest").await;
    assert_eq!((cont.session.as_deref(), cont.params["requestId"].as_str()), (Some("S9"), Some("r-ok")));
    // Listed, marked, announced once, and the active one now.
    let list = fk.run("browser_tabs", json!({})).await;
    assert!(list.content.contains("2 su 8") && list.content.contains("(aperta dal sito)"), "{}", list.content);
    assert!(
        list.content.contains("Il sito ha aperto una nuova scheda [2]") && list.content.contains("ora è quella attiva"),
        "{}",
        list.content
    );
    assert!(!fk.run("browser_tabs", json!({})).await.content.contains("Il sito ha aperto"), "the note is shown once");
    // A child target of the popup (an iframe) is paused and vetted too.
    fk.fake.push(json!({"method": "Target.attachedToTarget", "sessionId": "S9", "params": {"sessionId": "SF", "waitingForDebugger": true,
        "targetInfo": {"targetId": "F1", "type": "iframe", "title": "", "url": "http://e.example/"}}}));
    fk.fake.wait_for(|r| r.method == "Runtime.runIfWaitingForDebugger" && r.session.as_deref() == Some("SF")).await;
    assert!(fk.sent("Fetch.enable", "SF").len() == 1);
    assert!(fk.run("browser_tabs", json!({})).await.content.contains("2 su 8"), "frames are not tabs");
}

#[tokio::test]
async fn fake_tabs_new_select_close_and_the_cap() {
    let fk = Fk::plain().await;
    let blank = fk.run("browser_tab_new", json!({})).await;
    assert!(blank.content.starts_with("Nuova scheda [2] vuota"), "{}", blank.content);
    // With an address: vetted like navigate (a refused one creates nothing), then opened in the new tab.
    let created = fk.fake.commands("Target.createTarget").len();
    let denied = tool(&fk.t, "browser_tab_new").call(json!({"url": "http://127.0.0.1:9/"}), &fk.ctx).await;
    assert!(matches!(denied, Err(Error::Denied(_))), "{denied:?}");
    assert_eq!(fk.fake.commands("Target.createTarget").len(), created);
    let new = fk.run("browser_tab_new", json!({"url": "http://8.8.8.8/pagina"})).await;
    assert!(new.content.starts_with("Nuova scheda [3].") && !new.is_error, "{}", new.content);
    let nav = fk.sent("Page.navigate", "S3");
    assert_eq!(nav.len(), 1, "the address is opened in the new tab, not in the old one");
    assert_eq!(nav[0].params["url"], "http://8.8.8.8/pagina");
    // Select: the browser is told, the active tab changes.
    let sel = fk.run("browser_tab_select", json!({"index": 1})).await;
    assert!(sel.content.starts_with("Scheda [1] attiva"), "{}", sel.content);
    assert_eq!(fk.fake.commands("Target.activateTarget").last().unwrap().params["targetId"], "T1");
    assert!(fk.run("browser_tab_select", json!({"index": 42})).await.content.contains("non esiste"));
    assert!(fk.run("browser_tab_select", json!({"index": 0})).await.is_error);
    // Close.
    let closed = fk.run("browser_tab_close", json!({"index": 3})).await;
    assert!(closed.content.starts_with("Scheda [3] chiusa") && closed.content.contains("2 su 8"), "{}", closed.content);
    assert_eq!(fk.fake.commands("Target.closeTarget").last().unwrap().params["targetId"], "T3");
    // The cap: the 9th is refused with a clear message.
    for _ in 0..6 {
        assert!(!fk.run("browser_tab_new", json!({})).await.is_error);
    }
    let full = fk.run("browser_tab_new", json!({})).await;
    assert!(full.is_error && full.content.contains("già 8 schede") && full.content.contains("browser_tab_close"), "{}", full.content);
    // A page the site opens beyond the cap is closed, and the model is told.
    fk.fake.push(attached(None, "T99", "S99", "about:blank", Some("T1")));
    let closing = fk.fake.wait_for(|r| r.method == "Target.closeTarget" && r.params["targetId"] == "T99").await;
    assert!(closing.session.is_none());
    let list = fk.run("browser_tabs", json!({})).await;
    assert!(list.content.contains("8 su 8") && list.content.contains("è stata chiusa: ci sono già 8 schede"), "{}", list.content);
    assert!(fk.mgr.close().await);
}

#[tokio::test]
async fn fake_closing_the_last_tab_leaves_a_blank_one() {
    let fk = Fk::plain().await;
    fk.run("browser_tabs", json!({})).await;
    let out = fk.run("browser_tab_close", json!({})).await;
    assert!(out.content.contains("Scheda [1] chiusa") && out.content.contains("1 su 8"), "{}", out.content);
    let create = fk.fake.position(|r| r.method == "Target.createTarget").unwrap();
    let close = fk.fake.position(|r| r.method == "Target.closeTarget").unwrap();
    assert!(create < close, "the replacement is opened before the last tab goes");
    // The browser still answers.
    assert!(!fk.run("browser_tabs", json!({})).await.is_error);
}

#[tokio::test]
async fn fake_dialogs_are_answered_at_once_and_reported() {
    let fk = Fk::plain().await;
    fk.run("browser_tabs", json!({})).await;
    for (ty, msg) in [("alert", "Attenzione"), ("confirm", "Sicuro?"), ("prompt", "Nome?"), ("beforeunload", "")] {
        fk.fake.push(event(Some("S1"), "Page.javascriptDialogOpening", json!({"type": ty, "message": msg, "url": "http://e.example/"})));
    }
    fk.fake.wait_for(|r| r.method == "Page.handleJavaScriptDialog").await;
    fk.settle().await;
    let answers: Vec<bool> =
        fk.fake.commands("Page.handleJavaScriptDialog").iter().map(|r| r.params["accept"].as_bool().unwrap()).collect();
    // alert and beforeunload have only one way out; confirm and prompt are dismissed.
    assert_eq!(answers, vec![true, false, false, true]);
    assert!(fk.fake.commands("Page.handleJavaScriptDialog").iter().all(|r| r.session.as_deref() == Some("S1")));
    let out = fk.run("browser_tabs", json!({})).await;
    assert!(
        out.content.contains("(alert): «Attenzione»: chiusa con OK") && out.content.contains("(confirm): «Sicuro?»: annullata"),
        "{}",
        out.content
    );
    assert!(out.content.contains("(prompt): «Nome?»: annullata"), "{}", out.content);
}

#[tokio::test]
async fn fake_console_keeps_a_capped_ring_of_errors_and_warnings() {
    let fk = Fk::plain().await;
    fk.run("browser_tabs", json!({})).await;
    for n in 0..130 {
        fk.fake.push(event(
            Some("S1"),
            "Runtime.consoleAPICalled",
            json!({"type": "error", "args": [{"type": "string", "value": format!("e{n:04}")}]}),
        ));
    }
    fk.fake.push(event(Some("S1"), "Runtime.consoleAPICalled", json!({"type": "log", "args": [{"type": "string", "value": "rumore"}]})));
    fk.fake.push(event(
        Some("S1"),
        "Runtime.consoleAPICalled",
        json!({"type": "warning", "args": [{"type": "string", "value": "attento"}, {"type": "number", "value": 3}]}),
    ));
    fk.fake.push(event(
        Some("S1"),
        "Runtime.exceptionThrown",
        json!({"exceptionDetails": {"text": "Uncaught", "exception": {"description": "TypeError: x is undefined\n    at a.js:1"}}}),
    ));
    fk.fake.push(event(
        Some("S1"),
        "Log.entryAdded",
        json!({"entry": {"level": "error", "text": "Failed to load resource: 404", "url": "http://e.example/x.json"}}),
    ));
    fk.fake.push(event(Some("S1"), "Log.entryAdded", json!({"entry": {"level": "info", "text": "chiacchiere"}})));
    fk.fake.push(event(
        Some("S1"),
        "Runtime.consoleAPICalled",
        json!({"type": "error", "args": [{"type": "string", "value": "x".repeat(2000)}]}),
    ));
    fk.settle().await;
    let out = fk.run("browser_console", json!({})).await;
    assert!(out.content.contains("(100):"), "{}", &out.content[..80]);
    assert!(!out.content.contains("e0000") && !out.content.contains("e0029") && out.content.contains("e0129"), "the oldest are dropped");
    assert!(out.content.contains("avviso: attento 3") && out.content.contains("eccezione: TypeError: x is undefined"), "{}", out.content);
    assert!(out.content.contains("Failed to load resource: 404 (http://e.example/x.json)"));
    assert!(!out.content.contains("rumore") && !out.content.contains("chiacchiere"), "only errors and warnings");
    assert!(out.content.lines().all(|l| l.chars().count() < 340), "each entry is capped");
    let cleared = fk.run("browser_console", json!({"clear": true})).await;
    assert!(cleared.content.contains("e0129"));
    assert!(fk.run("browser_console", json!({})).await.content.contains("Nessun errore o avviso"));
}

#[tokio::test]
async fn fake_accessibility_snapshot_and_refs_drive_click() {
    let fk = Fk::new(|req, _| match req.method.as_str() {
        "Accessibility.getFullAXTree" => Some(Reply::ok(json!({"nodes": ax_nodes()}))),
        "Runtime.evaluate" if req.params["expression"].as_str().unwrap_or_default().contains("document.title") => {
            Some(Reply::ok(json!({"result": {"type": "object", "value": {"t": "Pagina", "u": "http://e.example/"}}})))
        }
        "DOM.resolveNode" => Some(Reply::ok(json!({"object": {"objectId": format!("obj{}", req.params["backendNodeId"])}}))),
        "Runtime.callFunctionOn" => Some(str_value("ok")),
        _ => None,
    })
    .await;
    let snap = fk.run("browser_snapshot", json!({"format": "ax"})).await;
    assert!(!snap.is_error, "{}", snap.content);
    assert!(
        snap.content.contains("Titolo: Pagina")
            && snap.content.contains("[e1] checkbox \"Accetto\" (non selezionato)")
            && snap.content.contains("[e2] button \"Invia\""),
        "{}",
        snap.content
    );
    // The ref resolves to the DOM node of the tree, and the action runs on that object as a user gesture.
    let click = fk.run("browser_click", json!({"ref": "e1"})).await;
    assert!(!click.is_error && click.content.starts_with("Cliccato [e1]."), "{}", click.content);
    assert_eq!(fk.fake.commands("DOM.resolveNode").last().unwrap().params["backendNodeId"], 6);
    let call = fk.fake.commands("Runtime.callFunctionOn").pop().unwrap();
    assert_eq!((call.params["objectId"].as_str(), call.params["userGesture"].as_bool()), (Some("obj6"), Some(true)));
    assert!(call.params["functionDeclaration"].as_str().unwrap().contains("el.click()"));
    // After an action the snapshot stays in the format the model chose (its refs are the ones in use).
    assert!(click.content.contains("Albero di accessibilità"), "{}", click.content);
    // Typed text goes in as a JSON string literal; select and hover resolve the same way.
    fk.run("browser_type", json!({"ref": "[e2]", "text": "a\"b"})).await;
    assert!(fk.fake.commands("Runtime.callFunctionOn").pop().unwrap().params["functionDeclaration"]
        .as_str()
        .unwrap()
        .contains(r#""a\"b""#));
    // A ref that was never handed out, or a malformed one.
    let unknown = fk.run("browser_click", json!({"ref": "e99"})).await;
    assert!(unknown.is_error && unknown.content.contains("non trovato"), "{}", unknown.content);
    assert!(fk.run("browser_click", json!({"ref": "pulsante"})).await.is_error);
    assert!(fk.run("browser_click", json!({})).await.is_error);
    assert!(fk.run("browser_snapshot", json!({"format": "html"})).await.is_error);
}

#[tokio::test]
async fn fake_text_format_pages_through_a_long_article() {
    let html = format!(
        "<html><head><title>Articolo</title></head><body><h1>Titolo</h1><p>{}</p><script>var x=1;</script></body></html>",
        "parola ".repeat(5000)
    );
    let raw = json!({"t": "Articolo", "u": "http://e.example/a", "h": html}).to_string();
    let fk = Fk::new(move |req, _| {
        (req.method == "Runtime.evaluate" && req.params["expression"] == TEXT_JS)
            .then(|| Reply::ok(json!({"result": {"type": "string", "value": raw}})))
    })
    .await;
    let first = fk.run("browser_snapshot", json!({"format": "text"})).await;
    assert!(
        first.content.starts_with("Titolo: Articolo\nURL: http://e.example/a\nTesto da 0 a 12000 di ")
            && first.content.contains("troncato"),
        "{}",
        &first.content[..200]
    );
    assert!(first.content.contains("offset=12000") && !first.content.contains("var x") && !first.content.contains("<p>"));
    let second = fk.run("browser_snapshot", json!({"format": "text", "offset": 12000})).await;
    assert!(second.content.contains("Testo da 12000 a "), "{}", &second.content[..200]);
    let last = fk.run("browser_snapshot", json!({"format": "text", "offset": 30000})).await;
    assert!(last.content.contains("fine della pagina"), "{}", &last.content[..200]);
}

#[tokio::test]
async fn fake_waits_conditions_timeout_and_cancellation() {
    let polls = Arc::new(AtomicUsize::new(0));
    let p2 = polls.clone();
    let fk = Fk::new(move |req, _| {
        let expr = req.params["expression"].as_str().unwrap_or_default();
        (req.method == "Runtime.evaluate" && expr.contains("txt.includes")).then(|| {
            // Not there on the first two polls, there on the third.
            str_value(if p2.fetch_add(1, Ordering::SeqCst) >= 2 { "yes" } else { "no" })
        })
    })
    .await;
    let started = std::time::Instant::now();
    let w = fk.run("browser_wait", json!({"text": "Fatto", "text_gone": "Attendere", "selector": "#r", "ms": 5000})).await;
    assert!(
        w.content.starts_with("Atteso: il testo «Fatto» è comparso, il testo «Attendere» è scomparso, «#r» è presente."),
        "{}",
        w.content
    );
    assert!(polls.load(Ordering::SeqCst) == 3 && started.elapsed() < Duration::from_secs(4));
    // The script the page runs has the texts as JSON literals.
    let js = fk
        .fake
        .commands("Runtime.evaluate")
        .iter()
        .map(|r| r.params["expression"].as_str().unwrap_or_default().to_string())
        .find(|e| e.contains("txt.includes"))
        .unwrap();
    assert!(
        js.contains("txt.includes(\"Fatto\")") && js.contains("!txt.includes(\"Attendere\")") && js.contains("querySelector(\"#r\")"),
        "{js}"
    );

    // Timeout: says what did not happen, and is capped.
    let fk = Fk::new(|req, _| {
        (req.method == "Runtime.evaluate" && req.params["expression"].as_str().unwrap_or_default().contains("txt.includes"))
            .then(|| str_value("no"))
    })
    .await;
    let t = fk.run("browser_wait", json!({"text": "mai", "ms": 400})).await;
    assert!(t.content.starts_with("Non soddisfatto entro 400 ms: il testo «mai» non è comparso."), "{}", t.content);
    // A selector the page rejects ends the wait with a message.
    let fk2 = Fk::new(|req, _| {
        (req.method == "Runtime.evaluate" && req.params["expression"].as_str().unwrap_or_default().contains("const txt"))
            .then(|| str_value("bad"))
    })
    .await;
    assert!(fk2.run("browser_wait", json!({"selector": "div[", "ms": 400})).await.content.contains("selettore"));
    // Without conditions: a plain pause.
    let plain = fk.run("browser_wait", json!({"ms": 100})).await;
    assert!(plain.content.starts_with("Atteso 100 ms."), "{}", plain.content);

    // A stopped run ends a long wait at once.
    let mut ctx = fk.ctx.clone();
    ctx.cancel = tokio_util::sync::CancellationToken::new();
    let cancel = ctx.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        cancel.cancel();
    });
    let started = std::time::Instant::now();
    let out = tool(&fk.t, "browser_wait").call(json!({"text": "mai", "ms": 30000}), &ctx).await.unwrap();
    assert!(out.is_error && out.content.contains("Annullato") && started.elapsed() < Duration::from_secs(3), "{}", out.content);
    // ...and the browser is still usable afterwards.
    assert!(fk.run("browser_wait", json!({"ms": 100})).await.content.starts_with("Atteso"));
}

#[tokio::test]
async fn fake_waits_for_a_navigation_and_for_the_network_to_go_quiet() {
    let fk = Fk::plain().await;
    fk.run("browser_tabs", json!({})).await;
    let tx = fk.fake.sender();
    tokio::spawn({
        let tx = tx.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            let _ =
                tx.send(event(Some("S1"), "Page.frameNavigated", json!({"frame": {"id": "F", "url": "http://e.example/b"}})).to_string());
            tokio::time::sleep(Duration::from_millis(200)).await;
            let _ = tx.send(event(Some("S1"), "Page.loadEventFired", json!({})).to_string());
        }
    });
    let nav = fk.run("browser_wait", json!({"navigation": true, "ms": 5000})).await;
    assert!(nav.content.starts_with("Atteso: la pagina ha navigato"), "{}", nav.content);
    // A navigation of a frame that is not the main one does not count.
    tx.send(
        event(Some("S1"), "Page.frameNavigated", json!({"frame": {"id": "F2", "parentId": "F", "url": "http://e.example/f"}})).to_string(),
    )
    .unwrap();
    tx.send(event(Some("S1"), "Page.loadEventFired", json!({})).to_string()).unwrap();
    fk.settle().await;
    assert!(fk.run("browser_wait", json!({"navigation": true, "ms": 300})).await.content.starts_with("Non soddisfatto"));

    // Network idle: a request in flight, finished after 400 ms, then half a second of quiet.
    tx.send(event(Some("S1"), "Network.requestWillBeSent", json!({"requestId": "q1", "type": "XHR"})).to_string()).unwrap();
    tx.send(event(Some("S1"), "Network.requestWillBeSent", json!({"requestId": "q2", "type": "EventSource"})).to_string()).unwrap();
    fk.settle().await;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let _ = tx.send(event(Some("S1"), "Network.loadingFinished", json!({"requestId": "q1"})).to_string());
    });
    let started = std::time::Instant::now();
    let idle = fk.run("browser_wait", json!({"network_idle": true, "ms": 5000})).await;
    assert!(idle.content.starts_with("Atteso: la rete è inattiva"), "{}", idle.content);
    assert!(started.elapsed() >= Duration::from_millis(800), "half a second of quiet is needed: {:?}", started.elapsed());
}

#[tokio::test]
async fn fake_downloads_are_moved_into_the_workspace_checked_and_recorded() {
    let mut fk = Fk::plain().await;
    fk.ctx.checkpoint_dir = Some(fk.dir.path().join("ckpt"));
    fk.run("browser_tabs", json!({})).await;
    let tmp = fk.dir.path().join("profile/downloads-tmp");
    std::fs::write(tmp.join("g1"), "ciao").unwrap();
    fk.fake.push(event(
        None,
        "Browser.downloadWillBegin",
        json!({"guid": "g1", "url": "https://e.example/a.txt", "suggestedFilename": "../a b?.txt"}),
    ));
    fk.fake.push(event(None, "Browser.downloadProgress", json!({"guid": "g1", "totalBytes": 4, "receivedBytes": 4, "state": "completed"})));
    let mut list = String::new();
    for _ in 0..40 {
        list = fk.run("browser_downloads", json!({})).await.content;
        if list.contains("Scaricato") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(list.contains("[1] Scaricato «a b_.txt» (4 byte) in downloads/run1/a b_.txt"), "{list}");
    assert_eq!(std::fs::read_to_string(fk.dir.path().join("downloads/run1/a b_.txt")).unwrap(), "ciao");
    assert!(!tmp.join("g1").exists(), "moved, not copied");
    let meta = crate::checkpoint::read_meta(&crate::checkpoint::run_dir(fk.ctx.checkpoint_dir.as_ref().unwrap(), "run1")).expect("meta");
    assert!(meta.created.iter().any(|c| c.ends_with("a b_.txt")), "{:?}", meta.created);

    // Too big: cancelled as soon as its size is known, nothing kept.
    fk.fake.push(event(
        None,
        "Browser.downloadWillBegin",
        json!({"guid": "g2", "url": "https://e.example/big.bin", "suggestedFilename": "big.bin"}),
    ));
    fk.fake.push(event(
        None,
        "Browser.downloadProgress",
        json!({"guid": "g2", "totalBytes": MAX_DOWNLOAD_BYTES + 1, "receivedBytes": 10, "state": "inProgress"}),
    ));
    fk.fake.wait_for(|r| r.method == "Browser.cancelDownload" && r.params["guid"] == "g2").await;
    fk.settle().await;
    let list = fk.run("browser_downloads", json!({})).await.content;
    assert!(list.contains("[2] Download non riuscito: «big.bin» (più grande del limite di 200 MB)"), "{list}");

    // Same name twice: the second gets another one.
    std::fs::write(tmp.join("g3"), "due").unwrap();
    fk.fake.push(event(
        None,
        "Browser.downloadWillBegin",
        json!({"guid": "g3", "url": "https://e.example/a.txt", "suggestedFilename": "a b?.txt"}),
    ));
    fk.fake.push(event(None, "Browser.downloadProgress", json!({"guid": "g3", "totalBytes": 3, "receivedBytes": 3, "state": "completed"})));
    for _ in 0..40 {
        if fk.dir.path().join("downloads/run1/a b_ (1).txt").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(fk.dir.path().join("downloads/run1/a b_ (1).txt").exists());

    // Count cap per run: the 11th is cancelled with a clear message.
    for n in 10..21 {
        fk.fake.push(event(
            None,
            "Browser.downloadWillBegin",
            json!({"guid": format!("g{n}"), "url": "https://e.example/x", "suggestedFilename": format!("x{n}")}),
        ));
    }
    fk.fake.wait_for(|r| r.method == "Browser.cancelDownload" && r.params["guid"] == "g20").await;
    fk.settle().await;
    let list = fk.run("browser_downloads", json!({})).await.content;
    assert!(list.contains("raggiunto il limite di 10 download per questo run"), "{list}");
    // g1, g2 and g3 plus seven of the new ones make ten; the other four are over the cap.
    assert_eq!(
        fk.fake.commands("Browser.cancelDownload").iter().filter(|r| r.params["guid"] != "g2").count(),
        4,
        "only the ones over the cap"
    );
    // Another run has its own count.
    fk.ctx.run_id = "run2".into();
    fk.run("browser_tabs", json!({})).await;
    fk.fake.push(event(None, "Browser.downloadWillBegin", json!({"guid": "h1", "url": "https://e.example/y", "suggestedFilename": "y"})));
    fk.settle().await;
    assert_eq!(fk.fake.commands("Browser.cancelDownload").iter().filter(|r| r.params["guid"] == "h1").count(), 0);
}

#[tokio::test]
async fn fake_downloads_are_refused_without_write_permission() {
    let mut fk = Fk::plain().await;
    fk.ctx.sandbox = crate::SandboxMode::ReadOnly;
    fk.run("browser_tabs", json!({})).await;
    std::fs::write(fk.dir.path().join("profile/downloads-tmp/g1"), "x").unwrap();
    fk.fake.push(event(
        None,
        "Browser.downloadWillBegin",
        json!({"guid": "g1", "url": "https://e.example/a.txt", "suggestedFilename": "a.txt"}),
    ));
    fk.fake.push(event(None, "Browser.downloadProgress", json!({"guid": "g1", "totalBytes": 1, "receivedBytes": 1, "state": "completed"})));
    fk.fake.wait_for(|r| r.method == "Browser.cancelDownload").await;
    fk.settle().await;
    let list = fk.run("browser_downloads", json!({})).await.content;
    assert!(list.contains("Download non riuscito: «a.txt» (la sandbox è in sola lettura"), "{list}");
    assert!(!fk.dir.path().join("downloads").exists(), "nothing written in read-only mode");
    // The `deny` behaviour of the browser is not used: the refusal is explicit and reported.
}

#[tokio::test]
async fn fake_a_click_that_starts_a_download_reports_it() {
    let dir_probe = Arc::new(Mutex::new(PathBuf::new()));
    let probe = dir_probe.clone();
    let fk = Fk::new(move |req, _| match req.method.as_str() {
        "Runtime.evaluate" if req.params["expression"].as_str().unwrap_or_default().contains("data-aiw-ref") => {
            Some(Reply::ok(json!({"result": {"type": "object", "subtype": "node", "objectId": "o1"}})))
        }
        "Runtime.callFunctionOn" => {
            std::fs::write(probe.lock().unwrap().join("g1"), "contenuto").unwrap();
            Some(Reply::with(
                json!({"result": {"type": "string", "value": "ok"}}),
                vec![
                    event(
                        None,
                        "Browser.downloadWillBegin",
                        json!({"guid": "g1", "url": "https://e.example/r.pdf", "suggestedFilename": "r.pdf"}),
                    ),
                    event(
                        None,
                        "Browser.downloadProgress",
                        json!({"guid": "g1", "totalBytes": 9, "receivedBytes": 9, "state": "completed"}),
                    ),
                ],
            ))
        }
        _ => None,
    })
    .await;
    *dir_probe.lock().unwrap() = fk.dir.path().join("profile/downloads-tmp");
    fk.run("browser_tabs", json!({})).await;
    let out = fk.run("browser_click", json!({"ref": 1})).await;
    assert!(!out.is_error && out.content.contains("Cliccato [1].\nScaricato «r.pdf» (9 byte) in downloads/run1/r.pdf"), "{}", out.content);
}

#[tokio::test]
async fn fake_upload_only_takes_files_the_run_may_read() {
    let fk = Fk::new(|req, _| match req.method.as_str() {
        "Runtime.evaluate" if req.params["expression"].as_str().unwrap_or_default().contains("data-aiw-ref") => {
            Some(Reply::ok(json!({"result": {"type": "object", "subtype": "node", "objectId": "o1"}})))
        }
        "Runtime.callFunctionOn" => Some(str_value("ok")),
        _ => None,
    })
    .await;
    std::fs::write(fk.dir.path().join("doc.txt"), "x").unwrap();
    std::fs::create_dir(fk.dir.path().join("cartella")).unwrap();
    let up = fk.run("browser_upload", json!({"ref": 1, "path": "doc.txt"})).await;
    assert!(!up.is_error && up.content.starts_with("File «doc.txt» scelto nel campo [1]."), "{}", up.content);
    let set = fk.fake.commands("DOM.setFileInputFiles").pop().unwrap();
    assert_eq!(set.params["objectId"], "o1");
    assert!(set.params["files"][0].as_str().unwrap().ends_with("doc.txt"));
    // Outside the workspace, a missing file, a folder: nothing reaches the browser.
    let before = fk.fake.commands("DOM.setFileInputFiles").len();
    let outside = tool(&fk.t, "browser_upload").call(json!({"ref": 1, "path": "/etc/passwd"}), &fk.ctx).await;
    assert!(matches!(outside, Err(Error::Denied(_))), "{outside:?}");
    let up_dir = tool(&fk.t, "browser_upload").call(json!({"ref": 1, "path": "../doc.txt"}), &fk.ctx).await;
    assert!(matches!(up_dir, Err(Error::Denied(_))));
    assert!(matches!(tool(&fk.t, "browser_upload").call(json!({"ref": 1, "path": "manca.txt"}), &fk.ctx).await, Err(Error::NotFound(_))));
    assert!(fk.run("browser_upload", json!({"ref": 1, "path": "cartella"})).await.is_error);
    assert_eq!(fk.fake.commands("DOM.setFileInputFiles").len(), before);
    // The attachment folder of the session is a read root: allowed.
    let att = tempfile::tempdir().unwrap();
    std::fs::write(att.path().join("allegato.png"), "png").unwrap();
    let mut ctx = fk.ctx.clone();
    ctx.read_roots = vec![att.path().to_path_buf()];
    let ok = tool(&fk.t, "browser_upload")
        .call(json!({"ref": 1, "path": att.path().join("allegato.png").to_string_lossy()}), &ctx)
        .await
        .unwrap();
    assert!(!ok.is_error && ok.content.contains("allegato.png"), "{}", ok.content);
    // Read-only sandbox: reading is still fine (the file only goes to the page).
    let mut ro = fk.ctx.clone();
    ro.sandbox = crate::SandboxMode::ReadOnly;
    assert!(!tool(&fk.t, "browser_upload").call(json!({"ref": 1, "path": "doc.txt"}), &ro).await.unwrap().is_error);
}

#[tokio::test]
async fn fake_eval_returns_capped_json_and_reports_errors() {
    let fk = Fk::new(|req, _| {
        if req.method != "Runtime.evaluate" {
            return None;
        }
        match req.params["expression"].as_str().unwrap_or_default() {
            "1+1" => Some(Reply::ok(json!({"result": {"type": "number", "value": 2}}))),
            "obj" => Some(Reply::ok(json!({"result": {"type": "object", "value": {"a": [1, "due"]}}}))),
            "big" => Some(str_value(&"x".repeat(50_000))),
            "undef" => Some(Reply::ok(json!({"result": {"type": "undefined"}}))),
            "node" => Some(Reply::ok(json!({"result": {"type": "object", "subtype": "node", "description": "div#a"}}))),
            "boom" => Some(Reply::ok(json!({"result": {"type": "object"}, "exceptionDetails": {"text": "Uncaught", "exception": {"description": "ReferenceError: boom is not defined\n    at <anonymous>:1:1"}}}))),
            _ => None,
        }
    })
    .await;
    assert_eq!(fk.run("browser_eval", json!({"expression": "1+1"})).await.content, "2");
    assert_eq!(fk.run("browser_eval", json!({"expression": "obj"})).await.content, "{\"a\":[1,\"due\"]}");
    assert_eq!(fk.run("browser_eval", json!({"expression": "undef"})).await.content, "undefined");
    assert!(fk.run("browser_eval", json!({"expression": "node"})).await.content.contains("non serializzabile"));
    let big = fk.run("browser_eval", json!({"expression": "big"})).await;
    assert!(
        big.content.contains("troncato") && big.content.contains("50002 caratteri in tutto") && big.content.chars().count() < 20_200,
        "{}",
        big.content.len()
    );
    let err = fk.run("browser_eval", json!({"expression": "boom"})).await;
    assert!(err.is_error && err.content == "Errore JavaScript: ReferenceError: boom is not defined", "{}", err.content);
    // Awaited, with a time limit, and not run as a user gesture.
    let call = fk.fake.commands("Runtime.evaluate").into_iter().find(|r| r.params["expression"] == "1+1").unwrap();
    assert_eq!(
        (call.params["awaitPromise"].as_bool(), call.params["returnByValue"].as_bool(), call.params["timeout"].as_u64()),
        (Some(true), Some(true), Some(10_000))
    );
    assert!(call.params.get("userGesture").is_none());
    assert!(tool(&fk.t, "browser_eval").call(json!({}), &fk.ctx).await.is_err());
}

#[test]
fn eval_is_exec_explicit_and_described_as_running_code() {
    let mgr = Arc::new(BrowserManager::new(std::env::temp_dir().join("aiw-browser-meta")));
    let all = tools(mgr);
    let eval = tool(&all, "browser_eval");
    assert_eq!(eval.risk(), Risk::Exec);
    assert!(eval.explicit_grant(), "a `*` in an agent's tool list does not include it");
    assert!(eval.description().contains("codice JavaScript nella pagina"), "{}", eval.description());
    assert_eq!(eval.subject(&json!({"expression": "document.cookie"})).as_deref(), Some("document.cookie"));
    for t in all.iter().filter(|t| t.name() != "browser_eval") {
        assert!(!t.explicit_grant(), "{}", t.name());
        assert!(t.risk() != Risk::Exec, "{}", t.name());
    }
    // Names are unique and every schema is an object schema.
    let mut names: Vec<&str> = all.iter().map(|t| t.name()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), all.len());
    assert_eq!(all.len(), 19);
    for t in &all {
        assert_eq!(t.parameters()["type"], "object", "{}", t.name());
        assert!(t.name().starts_with("browser_"));
        assert!(!t.description().is_empty());
    }
    // Risk classes: reading, navigating, acting on the page.
    let risk = |n: &str| tool(&all, n).risk();
    for n in ["browser_snapshot", "browser_wait", "browser_tabs", "browser_tab_select", "browser_downloads", "browser_console"] {
        assert_eq!(risk(n), Risk::Read, "{n}");
    }
    for n in ["browser_navigate", "browser_tab_new"] {
        assert_eq!(risk(n), Risk::Network, "{n}");
    }
    for n in ["browser_click", "browser_type", "browser_select", "browser_hover", "browser_upload", "browser_tab_close"] {
        assert_eq!(risk(n), Risk::Computer, "{n}");
    }
}

#[test]
fn helpers_parse_refs_waits_and_file_names() {
    use cdp::{sanitize_filename, RefArg};
    assert_eq!(RefArg::parse(&json!(3)), Some(RefArg::Num(3)));
    assert_eq!(RefArg::parse(&json!("[7]")), Some(RefArg::Num(7)));
    assert_eq!(RefArg::parse(&json!(" 12 ")), Some(RefArg::Num(12)));
    assert_eq!(RefArg::parse(&json!("e5")), Some(RefArg::Ax("e5".into())));
    assert_eq!(RefArg::parse(&json!("[E5]")), Some(RefArg::Ax("e5".into())));
    for bad in [json!("e"), json!("x5"), json!(""), json!(-1), json!(1.5), json!(null), json!(["e1"])] {
        assert_eq!(RefArg::parse(&bad), None, "{bad}");
    }
    assert!(ref_arg(&json!({"ref": "e1"})).is_err() && ref_arg(&json!({"ref": "4"})).unwrap() == 4);

    let w = WaitSpec::parse(&json!({})).unwrap();
    assert!(!w.has_condition() && w.timeout_ms == 1500 && w.page_js().is_none());
    let w = WaitSpec::parse(&json!({"text": "a", "ms": 999_999})).unwrap();
    assert!(w.has_condition() && w.timeout_ms == WAIT_MAX_MS);
    assert_eq!(WaitSpec::parse(&json!({"selector": "#x"})).unwrap().timeout_ms, 10_000);
    assert_eq!(WaitSpec::parse(&json!({"ms": 1})).unwrap().timeout_ms, 100);
    assert!(WaitSpec::parse(&json!({"ref": "zzz"})).is_err());
    let w = WaitSpec::parse(&json!({"ref": 4, "text": "\"; alert(1); \""})).unwrap();
    let js = w.page_js().unwrap();
    assert!(js.contains(r#"[data-aiw-ref=\"4\"]"#) && js.contains(r#"txt.includes("\"; alert(1); \"")"#), "{js}");
    assert!(
        WaitSpec::parse(&json!({"ref": "e4"})).unwrap().page_js().is_none(),
        "an accessibility ref is resolved through the DOM, not a selector"
    );

    assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
    assert_eq!(sanitize_filename("C:\\Users\\x\\file.pdf"), "file.pdf");
    assert_eq!(sanitize_filename(".hidden"), "hidden");
    assert_eq!(sanitize_filename("a<b>:c|d?.txt"), "a_b__c_d_.txt");
    assert_eq!(sanitize_filename(""), "download");
    assert_eq!(sanitize_filename("..."), "download");
    let long = format!("{}.pdf", "n".repeat(300));
    let s = sanitize_filename(&long);
    assert!(s.chars().count() <= 111 && s.ends_with(".pdf"), "{s}");

    assert_eq!(cap_result("abc"), "abc");
    assert!(cap_result(&"y".repeat(30_000)).contains("troncato"));
    let (a, b) = (
        page_action_body(Op::Press, &json!({"key": "Enter"})).unwrap().unwrap().0,
        page_action_body(Op::Press, &json!({"key": "Enter", "ref": 2})).unwrap().unwrap().0,
    );
    assert!(a.contains("document.activeElement") && !a.contains("'missing'") && b.contains("'missing'"));
    assert!(page_action_body(Op::Press, &json!({"key": "F12"})).is_err());
    assert!(page_action_body(Op::Scroll, &json!({"direction": "left"})).is_err());
    assert!(page_action_body(Op::Click, &json!({})).unwrap().is_none());
    let text =
        text_snapshot(&json!({"t": "", "u": "http://e/", "h": "<html><title>Dal tag</title><body>Ciao</body></html>"}).to_string(), 0);
    assert!(text.starts_with("Titolo: Dal tag\nURL: http://e/") && text.contains("Ciao") && text.contains("fine della pagina"), "{text}");
    assert!(text_snapshot("non json", 5).contains("0 a 0 di 0"));
    assert!(select_error(&RefArg::Num(1), "x", "ok:Uno").is_none());
    assert!(select_error(&RefArg::Num(1), "x", "noopt:A | B").unwrap().contains("A | B"));
}
