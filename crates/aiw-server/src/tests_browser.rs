//! The live view of the browser over the real router: who may see and drive it, what a frame looks like, that the input is
//! the person's only, and that nothing of it (frames, keys, typed text, titles, addresses) is in the audit or in an error.

use super::tests_security::{fixture, req, send, LAN};
use super::*;
use aiw_tools::browser::fake::{FakeCdp, Reply};
use aiw_tools::{ToolContext, ToolOutput};

fn token(e: &Engine, kind: &str) -> String {
    format!("Bearer {}", e.create_api_token("Prova", kind, None, None, None).unwrap().token)
}

async fn as_token(app: &Router, auth: &str, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let (st, _, v) = send(app, req(method, uri, Some(LAN), &[("host", "192.168.1.5:7777"), ("authorization", auth)], body)).await;
    (st, v)
}

/// An engine whose browser is a fake Chrome the person attached, with its tab open (an action of the agent started it).
async fn running() -> (Router, Engine, FakeCdp, tempfile::TempDir, tempfile::TempDir) {
    let (app, e, a, b) = fixture(vec![]).await;
    let fake = FakeCdp::start(|req, _| match req.method.as_str() {
        "Page.getLayoutMetrics" => {
            Some(Reply::ok(json!({"cssVisualViewport": {"clientWidth": 800.0, "clientHeight": 600.0, "pageX": 0.0, "pageY": 0.0}})))
        }
        "Page.captureScreenshot" => Some(Reply::ok(json!({"data": "SlBFRy1EQVRJ"}))),
        _ => None,
    })
    .await;
    e.browser().set_attach(Some(fake.url.clone()));
    let ctx = ToolContext::new("run1", e.workspace_root());
    let out: ToolOutput = e.tool_registry().get("browser_snapshot").unwrap().call(json!({}), &ctx).await.unwrap();
    assert!(!out.is_error, "{}", out.content);
    (app, e, fake, a, b)
}

#[tokio::test]
async fn the_live_view_is_the_owners_a_paired_device_neither_sees_nor_drives_the_browser() {
    let (app, e, _fake, _a, _b) = running().await;
    use crate::scopes::{required_scope, Required};
    use aiw_engine::tokens::Scope;
    let routes = [
        ("GET", "/api/browser", None),
        ("GET", "/api/browser/frame", None),
        ("POST", "/api/browser/control", Some(json!({"action": "take"}))),
        ("POST", "/api/browser/input", Some(json!({"type": "text", "text": "x"}))),
        ("POST", "/api/browser/dialog", Some(json!({"action": "accept"}))),
    ];
    for (method, path, _) in &routes {
        assert_eq!(required_scope(method, path), Some(Required::Scope(Scope::Full)), "{method} {path}");
    }
    let (dev, gw) = (token(&e, "device"), token(&e, "gateway"));
    for (method, path, body) in &routes {
        for (who, auth) in [("device", &dev), ("gateway", &gw)] {
            let (st, v) = as_token(&app, auth, method, path, body.clone()).await;
            assert_eq!(st, StatusCode::FORBIDDEN, "{who}: {method} {path}: {v}");
            assert!(!v.to_string().contains("e.example") && !v.to_string().contains("JPEG"), "{who}: {v}");
        }
    }
    // The paired device did not move anything: the agent still has the browser.
    assert_eq!(e.browser_state().await.control.mode, aiw_tools::browser::Mode::Agent);
    // The owner (this computer) gets through.
    let (st, v) = dispatch(app.clone(), "GET", "/api/browser", None).await;
    assert_eq!((st, v["running"].clone(), v["mode"].clone()), (200, json!(true), json!("agent")), "{v}");
}

#[tokio::test]
async fn the_frame_is_a_json_data_url_that_is_never_cached_and_never_starts_a_browser() {
    let (app, _e, _fake, _a, _b) = running().await;
    let (st, headers, v) = send(&app, req("GET", "/api/browser/frame", None, &[], None)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(headers.get("cache-control").and_then(|h| h.to_str().ok()), Some("no-store"));
    assert_eq!((v["running"].clone(), v["width"].clone(), v["height"].clone()), (json!(true), json!(800), json!(600)));
    assert_eq!(v["image"], "data:image/jpeg;base64,SlBFRy1EQVRJ");
    // The state is not cached either.
    let (_, headers, _) = send(&app, req("GET", "/api/browser", None, &[], None)).await;
    assert_eq!(headers.get("cache-control").and_then(|h| h.to_str().ok()), Some("no-store"));
    // With no browser running the route says so and does not launch one.
    let (app2, e2, _a2, _b2) = fixture(vec![]).await;
    let (st, v) = dispatch(app2.clone(), "GET", "/api/browser/frame", None).await;
    assert_eq!((st, v), (200, json!({"running": false})));
    assert!(!e2.browser_state().await.running);
}

#[tokio::test]
async fn the_person_takes_the_browser_types_in_it_and_gives_it_back() {
    let (app, e, fake, _a, _b) = running().await;
    let post = |path: &'static str, body: Value| {
        let app = app.clone();
        async move { dispatch(app, "POST", path, Some(body)).await }
    };
    // Not their turn: the input is refused and nothing reaches the page.
    let (st, v) = post("/api/browser/input", json!({"type": "text", "text": "PASSWORD-DI-PROVA"})).await;
    assert_eq!(st, 409, "{v}");
    assert!(!v.to_string().contains("PASSWORD-DI-PROVA"), "{v}");
    assert!(fake.commands("Input.insertText").is_empty());
    let (st, v) = post("/api/browser/control", json!({"action": "take"})).await;
    assert_eq!((st, v["mode"].clone()), (200, json!("human")), "{v}");
    let (st, v) = post("/api/browser/input", json!({"type": "text", "text": "PASSWORD-DI-PROVA"})).await;
    assert_eq!((st, v), (200, json!({})));
    assert_eq!(fake.commands("Input.insertText")[0].params["text"], "PASSWORD-DI-PROVA");
    let (st, _) = post("/api/browser/input", json!({"type": "click", "x": 0.5, "y": 0.5})).await;
    assert_eq!(st, 200);
    let (st, v) = post("/api/browser/control", json!({"action": "give_back"})).await;
    assert_eq!((st, v["mode"].clone(), v["stale"].clone()), (200, json!("agent"), json!(true)), "{v}");
    // Back to the agent: the person's input is refused again.
    assert_eq!(post("/api/browser/input", json!({"type": "text", "text": "x"})).await.0, 409);

    // Nothing the person typed is in the audit or in what the API said; the rows are the transitions.
    let rows: Vec<_> = e.audit().into_iter().filter(|a| a.action == "browser.control").collect();
    assert_eq!(rows.len(), 2, "{rows:?}");
    for a in e.audit() {
        assert!(!a.detail.contains("PASSWORD-DI-PROVA") && !a.detail.contains("JPEG"), "{a:?}");
    }
}

#[tokio::test]
async fn bodies_that_do_not_fit_are_refused_with_a_fixed_sentence_that_never_quotes_what_was_typed() {
    let (app, _e, _fake, _a, _b) = running().await;
    let typed = "PASSWORD-DI-PROVA";
    let bodies = [
        format!(r#"{{"type":"text","text":"{typed}"#),        // cut off
        format!(r#"{{"type":"tastiera","text":"{typed}"}}"#), // an unknown kind
        format!(r#"{{"type":"key","key":"{typed}"}}"#),       // a key that does not exist
        format!(r#"{{"type":"text","text":"{typed}","extra":{typed:?}}}"#),
        typed.to_string(),
    ];
    for body in bodies {
        use tower::ServiceExt;
        let r = axum::http::Request::builder()
            .method("POST")
            .uri("/api/browser/input")
            .header("content-type", "application/json")
            .body(Body::from(body.clone()))
            .unwrap();
        let res = app.clone().oneshot(r).await.unwrap();
        let status = res.status().as_u16();
        let text = String::from_utf8_lossy(&axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap()).into_owned();
        assert!(status == 400 || status == 409, "{body}: {status} {text}");
        assert!(!text.contains(typed), "{body}: the error quotes what was typed: {text}");
    }
    let (st, v) = dispatch(app.clone(), "POST", "/api/browser/control", Some(json!({"action": "vola"}))).await;
    assert_eq!(st, 400, "{v}");
    let (st, _) = dispatch(app.clone(), "POST", "/api/browser/dialog", Some(json!({"action": "boh"}))).await;
    assert_eq!(st, 400);
}

/// A writer that keeps everything the logs say.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;
    fn make_writer(&'a self) -> Captured {
        self.clone()
    }
}

#[tokio::test]
async fn nothing_the_person_types_or_sees_reaches_the_logs_and_the_addresses_in_them_have_no_query() {
    use tracing_subscriber::util::SubscriberInitExt;
    let logs = Captured::default();
    // The most the app can log: everything at `trace`, with the rules that hold whatever `RUST_LOG` says.
    let filter =
        aiw_core::QUIET_LOG_TARGETS.iter().fold(tracing_subscriber::EnvFilter::new("trace"), |f, d| f.add_directive(d.parse().unwrap()));
    let _guard = tracing_subscriber::fmt().with_env_filter(filter).with_writer(logs.clone()).with_ansi(false).finish().set_default();
    let (app, e, fake, _a, _b) = running().await;
    let (typed, code) = ("PASSWORD-DI-PROVA", "CODICE-DI-PROVA");
    for (path, body) in [
        ("/api/browser/control", json!({"action": "take"})),
        ("/api/browser/input", json!({"type": "text", "text": typed})),
        ("/api/browser/input", json!({"type": "key", "key": "x"})),
        ("/api/browser/input", json!({"type": "click", "x": 0.5, "y": 0.5})),
    ] {
        let (st, v) = dispatch(app.clone(), "POST", path, Some(body)).await;
        assert_eq!(st, 200, "{path}: {v}");
    }
    let (st, _) = dispatch(app.clone(), "GET", "/api/browser/frame", None).await;
    assert_eq!(st, 200);
    // A page that asks for an address the policy refuses, with a code in its query: it is logged (that is useful), without the query.
    fake.push(aiw_tools::browser::fake::event(
        Some("S2"),
        "Fetch.requestPaused",
        json!({"requestId": "r1", "request": {"url": format!("http://127.0.0.1:9/cb?code={code}#t={code}")}}),
    ));
    fake.wait_for(|r| r.method == "Fetch.failRequest").await;
    let (st, _) = dispatch(app.clone(), "POST", "/api/browser/control", Some(json!({"action": "give_back"}))).await;
    assert_eq!(st, 200);
    let text = String::from_utf8_lossy(&logs.0.lock().unwrap()).into_owned();
    for secret in [typed, code, "JPEG-DATI", "SlBFRy1EQVRJ"] {
        assert!(!text.contains(secret), "{secret} is in the logs:\n{text}");
    }
    assert!(text.contains("request blocked by network policy") && text.contains("127.0.0.1:9/cb"), "the block is logged:\n{text}");
    drop(e);
}

#[tokio::test]
async fn a_phone_looks_only_after_the_computer_says_yes_and_only_looks() {
    let (app, e, _fake, _a, _b) = running().await;
    let dev = token(&e, "device");
    let other = token(&e, "device");
    let gw = token(&e, "gateway");

    // Without a yes: no frame; the state says there is no request. A gateway token is no device.
    let (st, v) = as_token(&app, &dev, "GET", "/api/browser/view", None).await;
    assert_eq!((st, v["state"].as_str()), (StatusCode::OK, Some("none")), "{v}");
    let (st, v) = as_token(&app, &dev, "GET", "/api/browser/view/frame", None).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(as_token(&app, &gw, "POST", "/api/browser/view/request", None).await.0, StatusCode::FORBIDDEN);

    // Asking is not a yes, and the phone cannot answer for itself: the answering routes are the computer's.
    let (st, v) = as_token(&app, &dev, "POST", "/api/browser/view/request", None).await;
    assert_eq!((st, v["state"].as_str()), (StatusCode::OK, Some("pending")), "{v}");
    assert_eq!(as_token(&app, &dev, "GET", "/api/browser/view/frame", None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(as_token(&app, &dev, "GET", "/api/browser/view/requests", None).await.0, StatusCode::FORBIDDEN);
    let (_, v) = dispatch(app.clone(), "GET", "/api/browser/view/requests", None).await;
    let id = v["requests"][0]["id"].as_str().expect("the computer sees the request").to_string();
    assert_eq!(as_token(&app, &dev, "POST", &format!("/api/browser/view/requests/{id}/approve"), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(as_token(&app, &dev, "POST", "/api/browser/view/revoke", None).await.0, StatusCode::FORBIDDEN);

    // The computer says yes: this phone gets frames, another phone does not, and nothing else opens.
    let (st, _) = dispatch(app.clone(), "POST", &format!("/api/browser/view/requests/{id}/approve"), None).await;
    assert_eq!(st, StatusCode::OK);
    let (st, v) = as_token(&app, &dev, "GET", "/api/browser/view/frame", None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["image"].as_str().is_some_and(|i| i.starts_with("data:image/jpeg;base64,")), "{v}");
    assert_eq!(as_token(&app, &other, "GET", "/api/browser/view/frame", None).await.0, StatusCode::FORBIDDEN);
    for (method, path, body) in [
        ("GET", "/api/browser", None),
        ("GET", "/api/browser/frame", None),
        ("POST", "/api/browser/control", Some(json!({"action": "take"}))),
        ("POST", "/api/browser/input", Some(json!({"type": "text", "text": "x"}))),
        ("POST", "/api/browser/dialog", Some(json!({"action": "accept"}))),
    ] {
        assert_eq!(as_token(&app, &dev, method, path, body).await.0, StatusCode::FORBIDDEN, "{method} {path}");
    }
    assert_eq!(e.browser_state().await.control.mode, aiw_tools::browser::Mode::Agent, "looking does not move the agent");

    // «Basta» on the computer ends it at once.
    let (st, _) = dispatch(app.clone(), "POST", "/api/browser/view/revoke", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(as_token(&app, &dev, "GET", "/api/browser/view/frame", None).await.0, StatusCode::FORBIDDEN);
}
