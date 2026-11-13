//! The search tool against a local server; nothing here reaches the internet.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

type Seen = Arc<Mutex<Vec<String>>>;

/// Serves `handler(request) -> response` on a free local port; every full request is recorded.
async fn serve(handler: impl Fn(&str) -> String + Send + Sync + 'static) -> (u16, Seen, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Seen = Arc::default();
    let hits = Arc::new(AtomicUsize::new(0));
    let (s, h, handler) = (seen.clone(), hits.clone(), Arc::new(handler));
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let (s, h, handler) = (s.clone(), h.clone(), handler.clone());
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let len = head
                            .lines()
                            .find_map(|l| l.to_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok()))
                            .unwrap_or(0);
                        if body.len() >= len {
                            break;
                        }
                    }
                }
                let req = String::from_utf8_lossy(&buf).to_string();
                h.fetch_add(1, Ordering::SeqCst);
                let reply = handler(&req);
                s.lock().unwrap().push(req);
                let _ = sock.write_all(reply.as_bytes()).await;
            });
        }
    });
    (port, seen, hits)
}

fn reply(status: &str, ctype: &str, body: &str) -> String {
    format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

fn json_ok(body: Value) -> String {
    reply("200 OK", "application/json", &body.to_string())
}

fn ctx() -> ToolContext {
    ToolContext::new("r", std::env::temp_dir())
}

fn tool(cfg: Config, brave: &str, ddg: &str) -> WebSearchTool {
    WebSearchTool::with_endpoints(SearchSettings::new(cfg), brave, ddg)
}

fn cfg(provider: &str) -> Config {
    Config { provider: provider.into(), ..Default::default() }
}

const DDG_PAGE: &str = r##"<html><body><div id="links" class="results">
<div class="result results_links results_links_deep web-result">
  <div class="links_main links_deep result__body">
    <h2 class="result__title"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust-lang.org%2Flearn%3Fa%3D1%26b%3D2&amp;rut=abc123">Rust <b>Programming</b> Language</a></h2>
    <div class="result__extras"><a class="result__url" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust-lang.org%2F">www.rust-lang.org</a></div>
    <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust-lang.org%2F&amp;rut=abc123">A language empowering everyone to build <b>reliable</b> &amp; efficient software. It&#x27;s fast.</a>
  </div>
</div>
<div class="result results_links result--ad">
  <h2 class="result__title"><a class="result__a" href="//duckduckgo.com/y.js?ad_domain=shop.example&amp;u3=zzz">Annuncio</a></h2>
  <a class="result__snippet" href="#">Compra ora</a>
</div>
<div class="result results_links results_links_deep web-result">
  <h2 class="result__title"><a class="result__a" href='https://doc.rust-lang.org/book/' rel="nofollow">The Rust Programming Language &ndash; Book</a></h2>
  <div class="result__snippet">Il libro   ufficiale
    su Rust.</div>
</div>
<div class="result"><h2 class="result__title"><a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fcrates.io%2F">crates.io</a></h2></div>
</div></body></html>"##;

#[test]
fn provider_selection_in_auto() {
    let mut c = Config::default();
    assert_eq!(resolve(&c), Some(Provider::DuckDuckGo), "nothing set: no key needed");
    c.provider = "auto".into();
    assert_eq!(resolve(&c), Some(Provider::DuckDuckGo));
    c.brave_api_key = "k".into();
    assert_eq!(resolve(&c), Some(Provider::Brave), "a key beats the default");
    c.searxng_url = "http://localhost:8080".into();
    assert_eq!(resolve(&c), Some(Provider::Searxng), "SearXNG beats the key");
    c.searxng_url = "   ".into();
    assert_eq!(resolve(&c), Some(Provider::Brave), "a blank address counts as not set");
    // An explicit choice wins over what auto would pick.
    c.searxng_url = "http://localhost:8080".into();
    for (name, want) in [("duckduckgo", Provider::DuckDuckGo), ("brave", Provider::Brave), ("searxng", Provider::Searxng)] {
        c.provider = name.into();
        assert_eq!(resolve(&c), Some(want));
    }
    c.provider = "off".into();
    assert_eq!(resolve(&c), None);
}

#[test]
fn off_hides_the_tool() {
    let settings = SearchSettings::new(cfg("off"));
    let t = WebSearchTool::new(settings.clone());
    assert!(t.available().unwrap_err().contains("spenta"));
    settings.set(cfg("auto"));
    assert!(t.available().is_ok());
    assert_eq!((t.name(), t.risk()), ("web_search", Risk::Network));
    assert_eq!(t.subject(&json!({"query": "rust async"})).as_deref(), Some("rust async"));
}

#[test]
fn the_key_is_not_in_the_debug_output() {
    let c = Config { provider: "brave".into(), brave_api_key: "SEGRETO-123".into(), ..Default::default() };
    assert!(!format!("{c:?}").contains("SEGRETO"));
}

#[tokio::test]
async fn searxng_results_are_parsed_and_the_query_is_encoded() {
    let (port, seen, _) = serve(|_| {
        json_ok(json!({"query": "x", "results": [
            {"title": "Primo &amp; <b>migliore</b>", "url": "https://a.example/1", "content": "Anteprima  con\n spazi &quot;e&quot; entità"},
            {"title": "Senza snippet", "url": "https://b.example/2"},
            {"title": "Schema strano", "url": "javascript:alert(1)", "content": "no"},
            {"title": "Terzo", "url": "http://c.example/3", "content": "x".repeat(1000)}
        ]}))
    })
    .await;
    let c = Config { provider: "searxng".into(), searxng_url: format!("http://127.0.0.1:{port}/"), ..Default::default() };
    let out = tool(c, "", "").call(json!({"query": "rust & più cose"}), &ctx()).await.unwrap();
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.starts_with("Risultati della ricerca web per «rust & più cose» (motore: SearXNG):"), "{}", out.content);
    assert!(out.content.contains("1. Primo & migliore\n   https://a.example/1\n   Anteprima con spazi \"e\" entità\n"), "{}", out.content);
    assert!(out.content.contains("2. Senza snippet\n   https://b.example/2\n"));
    assert!(out.content.contains("3. Terzo"), "the unusable entry is skipped: {}", out.content);
    assert!(!out.content.contains("javascript:"));
    let long = out.content.lines().find(|l| l.trim_start().starts_with("xxx")).unwrap();
    assert!(long.trim().chars().count() <= SNIPPET_CHARS + 1, "snippet clipped");
    let req = seen.lock().unwrap()[0].clone();
    let first = req.lines().next().unwrap();
    assert!(first.starts_with("GET /search?q=rust+%26+pi%C3%B9+cose&format=json "), "{first}");
}

#[tokio::test]
async fn searxng_failures_have_clear_messages() {
    let (port, _, _) = serve(|_| reply("403 Forbidden", "text/plain", "no")).await;
    let c = |url: String| Config { provider: "searxng".into(), searxng_url: url, ..Default::default() };
    let out = tool(c(format!("http://127.0.0.1:{port}")), "", "").call(json!({"query": "q"}), &ctx()).await.unwrap();
    assert!(out.is_error && out.content.contains("403") && out.content.contains("json"), "{}", out.content);

    let (port, _, _) = serve(|_| reply("200 OK", "text/html", "<html>login</html>")).await;
    let out = tool(c(format!("http://127.0.0.1:{port}")), "", "").call(json!({"query": "q"}), &ctx()).await.unwrap();
    assert!(out.is_error && out.content.contains("JSON"), "{}", out.content);

    // Nothing listens there: bind and drop to get a closed port.
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let out = tool(c(format!("http://127.0.0.1:{closed}")), "", "").call(json!({"query": "q"}), &ctx()).await.unwrap();
    assert!(out.is_error && out.content.contains("SearXNG non è raggiungibile"), "{}", out.content);
    assert!(!out.content.contains("q="), "the query is not echoed in the transport error");

    let out = tool(c(String::new()), "", "").call(json!({"query": "q"}), &ctx()).await.unwrap();
    assert!(out.is_error && out.content.contains("non è configurato"), "{}", out.content);
    let out = tool(c("ftp://host".into()), "", "").call(json!({"query": "q"}), &ctx()).await.unwrap();
    assert!(out.is_error && out.content.contains("http://"), "{}", out.content);
}

#[tokio::test]
async fn brave_sends_the_key_in_a_header_and_parses_the_results() {
    let (port, seen, _) = serve(|_| {
        json_ok(json!({"web": {"results": [
            {"title": "Rust", "url": "https://www.rust-lang.org/", "description": "Un linguaggio <strong>affidabile</strong> &amp; veloce"},
            {"title": "Docs", "url": "https://doc.rust-lang.org/", "description": "Documentazione"}
        ]}}))
    })
    .await;
    let c = Config { provider: "brave".into(), brave_api_key: "chiave-di-prova".into(), ..Default::default() };
    let endpoint = format!("http://127.0.0.1:{port}/res/v1/web/search");
    let out = tool(c, &endpoint, "").call(json!({"query": "rust lang", "max_results": 2}), &ctx()).await.unwrap();
    assert!(!out.is_error, "{}", out.content);
    assert!(
        out.content.contains("(motore: Brave Search)")
            && out.content.contains("1. Rust\n   https://www.rust-lang.org/\n   Un linguaggio affidabile & veloce\n"),
        "{}",
        out.content
    );
    assert!(out.content.contains("2. Docs"));
    assert!(!out.content.contains("chiave-di-prova"));
    let req = seen.lock().unwrap()[0].clone();
    assert!(req.starts_with("GET /res/v1/web/search?q=rust+lang&count=2 "), "{req}");
    assert!(req.to_lowercase().contains("x-subscription-token: chiave-di-prova\r\n"), "{req}");
    assert!(!req.lines().next().unwrap().contains("chiave"), "the key is not in the URL");
}

#[tokio::test]
async fn brave_without_a_key_fails_without_calling_out() {
    let (port, _, hits) = serve(|_| json_ok(json!({}))).await;
    let endpoint = format!("http://127.0.0.1:{port}/");
    let out = tool(cfg("brave"), &endpoint, "").call(json!({"query": "q"}), &ctx()).await.unwrap();
    assert!(out.is_error && out.content.contains("Manca la chiave"), "{}", out.content);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn brave_http_errors_are_explained_and_an_empty_answer_is_not_an_error() {
    let with_key = Config { provider: "brave".into(), brave_api_key: "k".into(), ..Default::default() };
    for (status, needle) in [
        ("401 Unauthorized", "rifiutato la chiave"),
        ("429 Too Many Requests", "limite di richieste"),
        ("500 Internal Server Error", "errore 500"),
    ] {
        let (port, _, _) = serve(move |_| reply(status, "application/json", "{}")).await;
        let out = tool(with_key.clone(), &format!("http://127.0.0.1:{port}/"), "").call(json!({"query": "q"}), &ctx()).await.unwrap();
        assert!(out.is_error && out.content.contains(needle), "{status}: {}", out.content);
    }
    let (port, _, _) = serve(|_| json_ok(json!({"type": "search"}))).await;
    let out =
        tool(with_key, &format!("http://127.0.0.1:{port}/"), "").call(json!({"query": "qualcosa di introvabile"}), &ctx()).await.unwrap();
    assert!(!out.is_error);
    assert_eq!(out.content, "Nessun risultato per «qualcosa di introvabile» (motore: Brave Search).");
}

#[test]
fn duckduckgo_html_is_parsed_and_the_redirects_are_decoded() {
    let hits = parse_duckduckgo(DDG_PAGE, 10);
    assert_eq!(hits.len(), 3, "the ad is skipped: {hits:#?}");
    assert_eq!(hits[0].title, "Rust Programming Language");
    assert_eq!(hits[0].url, "https://www.rust-lang.org/learn?a=1&b=2", "uddg is percent-decoded");
    assert_eq!(hits[0].snippet, "A language empowering everyone to build reliable & efficient software. It's fast.");
    assert_eq!(hits[1].url, "https://doc.rust-lang.org/book/", "a direct link is kept");
    assert_eq!(hits[1].title, "The Rust Programming Language – Book");
    assert_eq!(hits[1].snippet, "Il libro ufficiale su Rust.");
    assert_eq!(hits[2], Hit { title: "crates.io".into(), url: "https://crates.io/".into(), snippet: String::new() });
    assert_eq!(parse_duckduckgo(DDG_PAGE, 2).len(), 2);
    assert!(parse_duckduckgo("<html><body>niente</body></html>", 5).is_empty());
}

#[tokio::test]
async fn duckduckgo_posts_the_query_as_a_form() {
    let (port, seen, _) = serve(|_| reply("200 OK", "text/html", DDG_PAGE)).await;
    let t = tool(cfg("duckduckgo"), "", &format!("http://127.0.0.1:{port}/html/"));
    let out = t.call(json!({"query": "rust è bello", "max_results": 2}), &ctx()).await.unwrap();
    assert!(!out.is_error, "{}", out.content);
    assert!(
        out.content.contains("(motore: DuckDuckGo)")
            && out.content.contains("1. Rust Programming Language\n   https://www.rust-lang.org/learn?a=1&b=2\n"),
        "{}",
        out.content
    );
    assert!(out.content.contains("2. The Rust Programming Language – Book") && !out.content.contains("3. "), "{}", out.content);
    let req = seen.lock().unwrap()[0].clone();
    assert!(req.starts_with("POST /html/ "), "{req}");
    assert!(req.to_lowercase().contains("application/x-www-form-urlencoded"));
    assert!(req.ends_with("q=rust+%C3%A8+bello"), "{req}");
}

#[tokio::test]
async fn duckduckgo_challenge_pages_are_reported() {
    let (port, _, _) =
        serve(|_| reply("202 Accepted", "text/html", "<html><body>Unfortunately, bots use DuckDuckGo too. anomaly-modal</body></html>"))
            .await;
    let t = tool(cfg("duckduckgo"), "", &format!("http://127.0.0.1:{port}/html/"));
    let out = t.call(json!({"query": "q"}), &ctx()).await.unwrap();
    assert!(out.is_error && out.content.contains("bloccato"), "{}", out.content);
    let (port, _, _) = serve(|_| reply("200 OK", "text/html", "<html><body>No results.</body></html>")).await;
    let t = tool(cfg("duckduckgo"), "", &format!("http://127.0.0.1:{port}/html/"));
    let out = t.call(json!({"query": "q"}), &ctx()).await.unwrap();
    assert!(!out.is_error && out.content.starts_with("Nessun risultato"), "{}", out.content);
}

#[tokio::test]
async fn max_results_is_clamped_and_defaults_to_five() {
    let many: Vec<Value> =
        (1..=20).map(|i| json!({"title": format!("R{i}"), "url": format!("https://x.example/{i}"), "content": "c"})).collect();
    let (port, _, _) = serve(move |_| json_ok(json!({"results": many}))).await;
    let c = Config { provider: "searxng".into(), searxng_url: format!("http://127.0.0.1:{port}"), ..Default::default() };
    let t = tool(c, "", "");
    let count = |out: &ToolOutput| {
        out.content.lines().filter(|l| l.chars().next().is_some_and(|c| c.is_ascii_digit()) && l.contains(". R")).count()
    };
    assert_eq!(count(&t.call(json!({"query": "q"}), &ctx()).await.unwrap()), 5);
    assert_eq!(count(&t.call(json!({"query": "q", "max_results": 0}), &ctx()).await.unwrap()), 1);
    assert_eq!(count(&t.call(json!({"query": "q", "max_results": -4}), &ctx()).await.unwrap()), 1);
    assert_eq!(count(&t.call(json!({"query": "q", "max_results": 3}), &ctx()).await.unwrap()), 3);
    assert_eq!(count(&t.call(json!({"query": "q", "max_results": 99}), &ctx()).await.unwrap()), 10);
    assert!(t.call(json!({"query": "   "}), &ctx()).await.is_err());
    assert!(t.call(json!({}), &ctx()).await.is_err());
}

#[tokio::test]
async fn an_allowlist_keeps_the_fixed_providers_in_check() {
    let (port, _, hits) = serve(|_| json_ok(json!({}))).await;
    let mut c = ctx();
    c.allow_domains = vec!["docs.rs".into()];
    let t = tool(cfg("duckduckgo"), "", &format!("http://127.0.0.1:{port}/html/"));
    let out = t.call(json!({"query": "q"}), &c).await.unwrap();
    assert!(out.is_error && out.content.contains("network.allow_domains"), "{}", out.content);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn entities_and_clipping() {
    assert_eq!(clean_text("<p>Caff&egrave; &amp; &#233;&#x41; &lt;b&gt;</p>"), "Caff&egrave; & éA <b>");
    assert_eq!(clip("abcdef", 3), "abc…");
    assert_eq!(clip("abc", 3), "abc");
}
