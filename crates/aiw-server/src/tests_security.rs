//! Local API hardening: CSRF and DNS rebinding, CORS allowlist, pairing brute force, SSE resume
//! and the UI's delete/rename routes. Requests go through the router with a chosen peer address
//! (`ConnectInfo`) instead of a socket, so every case is explicit.

use super::*;
use aiw_engine::EngineOptions;
use aiw_runtimes::ScriptedBackend;
use axum::http::HeaderMap;
use std::sync::Arc;

const LOOPBACK: [u8; 4] = [127, 0, 0, 1];
const LAN: [u8; 4] = [192, 168, 1, 50];

async fn fixture(turns: Vec<Vec<aiw_runtimes::ChatDelta>>) -> (Router, Engine, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().into()),
        workspace_root: Some(ws.path().into()),
        mode: "daemon".into(),
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap();
    e.register_native_runtime("llama-embedded", Arc::new(ScriptedBackend::new(turns)));
    e.save_profile(InferenceProfile {
        id: "default".into(),
        name: "T".into(),
        description: None,
        runtime_id: "llama-embedded".into(),
        model: "scripted".into(),
        params: Default::default(),
    })
    .unwrap();
    (router(e.clone(), None), e, data, ws)
}

/// A request as the listener would hand it to the router: `peer` is the TCP peer (`None` for the
/// in-process dispatcher of the Tauri shell).
fn req(method: &str, uri: &str, peer: Option<[u8; 4]>, headers: &[(&str, &str)], body: Option<Value>) -> Request {
    let mut b = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let body = match body {
        Some(v) => {
            b = b.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let mut r = b.body(body).unwrap();
    if let Some(ip) = peer {
        r.extensions_mut().insert(ConnectInfo(SocketAddr::from((ip, 40000))));
    }
    r
}

async fn send(app: &Router, r: Request) -> (StatusCode, HeaderMap, Value) {
    let res = tower::ServiceExt::oneshot(app.clone(), r).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, headers, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// GET /api/system from `peer` with these headers.
async fn get_system(app: &Router, peer: Option<[u8; 4]>, headers: &[(&str, &str)]) -> (StatusCode, HeaderMap, Value) {
    send(app, req("GET", "/api/system", peer, headers, None)).await
}

fn acao(h: &HeaderMap) -> Option<&str> {
    h.get("access-control-allow-origin").and_then(|v| v.to_str().ok())
}

// ------------------------------------------------------------------ rebinding and CSRF

#[tokio::test]
async fn a_loopback_peer_must_use_a_loopback_host() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    for host in ["127.0.0.1:7777", "localhost:7777", "localhost", "[::1]:7777", "LOCALHOST:5173"] {
        let (st, ..) = get_system(&app, Some(LOOPBACK), &[("host", host)]).await;
        assert_eq!(st, StatusCode::OK, "{host}");
    }
    // DNS rebinding: the page's own name now resolves to 127.0.0.1, but the Host header says so.
    for host in
        ["evil.example:7777", "evil.example", "192.168.1.5:7777", "0.0.0.0:7777", "localhost.evil.example:7777", "127.0.0.1.evil.example"]
    {
        let (st, _, body) = get_system(&app, Some(LOOPBACK), &[("host", host)]).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{host}");
        assert_eq!(body["error"], "host non consentito", "{host}");
    }
    // No Host at all is not a rebinding page (browsers always send one).
    assert_eq!(get_system(&app, Some(LOOPBACK), &[]).await.0, StatusCode::OK);
    // The API token is an explicit credential that a rebinding page cannot have: it always works.
    let bearer = format!("Bearer {}", e.token());
    let by_query = format!("/api/system?token={}", e.token());
    let (st, ..) = get_system(&app, Some(LOOPBACK), &[("host", "evil.example"), ("authorization", &bearer)]).await;
    assert_eq!(st, StatusCode::OK);
    let (st, ..) = send(&app, req("GET", &by_query, Some(LOOPBACK), &[("host", "evil.example")], None)).await;
    assert_eq!(st, StatusCode::OK);
    let (st, ..) = get_system(&app, Some(LOOPBACK), &[("host", "evil.example"), ("x-api-key", e.token())]).await;
    assert_eq!(st, StatusCode::OK);
    let (st, ..) = get_system(&app, Some(LOOPBACK), &[("host", "evil.example"), ("authorization", "Bearer sbagliato")]).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "a wrong token does not help");
    // Pages that are not the API (agent card, static UI, webhooks) are not behind this rule.
    let (st, ..) = send(&app, req("GET", "/.well-known/agent-card.json", Some(LOOPBACK), &[("host", "evil.example")], None)).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn a_web_page_cannot_drive_the_api() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let hooks = json!({"hooks": [{"event": "UserPromptSubmit", "command": "touch /tmp/pwned", "timeout_s": 5}]});
    let mcp = json!({"name": "x", "transport": "stdio", "command": "sh", "args": ["-c", "touch /tmp/pwned"], "enabled": true});
    for (origin, fetch_site) in [
        ("https://evil.example", "cross-site"),
        ("http://evil.example:7777", "cross-site"),
        ("null", "cross-site"),
        ("http://localhost.evil.example:5173", "cross-site"),
        ("http://127.0.0.1.evil.example", "cross-site"),
        ("http://192.168.1.77:8080", "cross-site"),
        // Whatever the browser says about the fetch, the origin itself decides.
        ("https://evil.example", "same-site"),
    ] {
        let h = [("host", "127.0.0.1:7777"), ("origin", origin), ("sec-fetch-site", fetch_site)];
        let (st, _, body) = send(&app, req("PUT", "/api/hooks", Some(LOOPBACK), &h, Some(hooks.clone()))).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{origin}");
        assert_eq!(body["error"], "origine non consentita");
        let (st, ..) = send(&app, req("POST", "/api/mcp/servers", Some(LOOPBACK), &h, Some(mcp.clone()))).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{origin}");
        let (st, ..) = send(&app, req("POST", "/api/kill", Some(LOOPBACK), &h, None)).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{origin}");
        let (st, ..) = send(&app, req("GET", "/api/system", Some(LOOPBACK), &h, None)).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{origin}");
    }
    assert!(e.hooks().is_empty(), "no hook was installed");
    assert!(e.mcp_servers().is_empty(), "no MCP server was registered");

    // A cross-site fetch that carries no Origin (an <img>, a navigation) is not one of ours either.
    let h = [("host", "127.0.0.1:7777"), ("sec-fetch-site", "cross-site")];
    assert_eq!(get_system(&app, Some(LOOPBACK), &h).await.0, StatusCode::FORBIDDEN);
    // The webview of the app, a dev server and pages of this machine are the allowlist.
    for origin in [
        "tauri://localhost",
        "http://tauri.localhost",
        "https://tauri.localhost",
        "http://localhost:5173",
        "http://localhost",
        "http://127.0.0.1:7777",
        "http://[::1]:5173",
        "https://localhost:8443",
    ] {
        for fetch_site in ["cross-site", "same-site", "same-origin"] {
            let h = [("host", "127.0.0.1:7777"), ("origin", origin), ("sec-fetch-site", fetch_site)];
            let (st, headers, _) = send(&app, req("PUT", "/api/hooks", Some(LOOPBACK), &h, Some(json!({"hooks": []})))).await;
            assert_eq!(st, StatusCode::OK, "{origin} {fetch_site}");
            assert_eq!(acao(&headers), Some(origin), "the allowed origin may read the answer");
            assert!(headers.get("access-control-allow-credentials").is_none());
        }
    }
    for fetch_site in ["same-origin", "same-site", "none"] {
        let h = [("host", "127.0.0.1:7777"), ("sec-fetch-site", fetch_site)];
        assert_eq!(get_system(&app, Some(LOOPBACK), &h).await.0, StatusCode::OK, "{fetch_site}");
    }

    // With the token a cross-origin request is accepted by the server (the token cannot be forged),
    // but the browser is still not allowed to read the answer: no CORS headers.
    let bearer = format!("Bearer {}", e.token());
    let h = [("host", "192.168.1.5:7777"), ("origin", "https://evil.example"), ("authorization", &bearer)];
    let (st, headers, _) = get_system(&app, Some(LAN), &h).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(acao(&headers), None);
}

/// The same rules over a real socket, with the peer address the listener reports.
#[tokio::test]
async fn the_rules_hold_over_a_real_loopback_connection() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (app, e, _d, _w) = fixture(vec![]).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap() });
    let call = |headers: Vec<(String, String)>| async move {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let extra: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
        s.write_all(format!("GET /api/agents HTTP/1.1\r\n{extra}Connection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await.unwrap();
        let text = String::from_utf8_lossy(&buf).to_string();
        let status: u16 = text.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, text.to_lowercase())
    };
    let h = |k: &str, v: String| (k.to_string(), v);
    let (st, _) = call(vec![h("Host", format!("127.0.0.1:{port}"))]).await;
    assert_eq!(st, 200);
    let (st, _) = call(vec![h("Host", format!("localhost:{port}"))]).await;
    assert_eq!(st, 200);
    let (st, _) = call(vec![h("Host", format!("attacker.example:{port}"))]).await;
    assert_eq!(st, 403, "rebinding");
    let (st, text) = call(vec![h("Host", format!("127.0.0.1:{port}")), h("Origin", "https://attacker.example".into())]).await;
    assert_eq!(st, 403, "cross-origin");
    assert!(!text.contains("access-control-allow-origin"));
    let (st, text) = call(vec![h("Host", format!("127.0.0.1:{port}")), h("Origin", "http://localhost:5173".into())]).await;
    assert_eq!(st, 200);
    assert!(text.contains("access-control-allow-origin: http://localhost:5173"), "{text}");
    let (st, _) = call(vec![h("Host", "attacker.example".into()), h("Authorization", format!("Bearer {}", e.token()))]).await;
    assert_eq!(st, 200, "the token is accepted whatever the name");
}

#[tokio::test]
async fn the_vite_dev_proxy_and_the_in_process_dispatcher_keep_working() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    // `vite` forwards from http://localhost:5173 with changeOrigin: Host is the target, Origin stays.
    let h = [("host", "127.0.0.1:7777"), ("origin", "http://localhost:5173"), ("sec-fetch-site", "same-origin")];
    let (st, headers, ses) = send(&app, req("POST", "/api/sessions", Some(LOOPBACK), &h, Some(json!({"agent_id": "planner"})))).await;
    assert_eq!(st, StatusCode::OK, "{ses}");
    assert_eq!(acao(&headers), Some("http://localhost:5173"));
    let (st, ..) = send(&app, req("GET", "/api/agents", Some(LOOPBACK), &h, None)).await;
    assert_eq!(st, StatusCode::OK);
    // Without changeOrigin the Host would be the dev server itself.
    let h = [("host", "localhost:5173"), ("origin", "http://localhost:5173")];
    assert_eq!(get_system(&app, Some(LOOPBACK), &h).await.0, StatusCode::OK);

    // The Tauri shell has no socket: no ConnectInfo, no Host, no Origin.
    let (st, sys) = dispatch(app.clone(), "GET", "/api/system", None).await;
    assert_eq!(st, 200);
    assert!(sys["pairing_code"].is_string());
    let (st, _) = dispatch(app.clone(), "PUT", "/api/hooks", Some(json!({"hooks": []}))).await;
    assert_eq!(st, 200);
    // ...and a request without a peer is never mistaken for a browser, whatever its headers say.
    let (st, ..) = get_system(&app, None, &[("host", "pc.local:7777")]).await;
    assert_eq!(st, StatusCode::OK);
    // Another device on the LAN still needs the token.
    assert_eq!(get_system(&app, Some(LAN), &[("host", "192.168.1.5:7777")]).await.0, StatusCode::UNAUTHORIZED);
    let bearer = format!("Bearer {}", e.token());
    let (st, _, sys) = get_system(&app, Some(LAN), &[("host", "192.168.1.5:7777"), ("authorization", &bearer)]).await;
    assert_eq!(st, StatusCode::OK);
    assert!(sys.get("pairing_code").is_none());
}

#[tokio::test]
async fn cors_preflights_follow_the_allowlist() {
    let (app, _e, _d, _w) = fixture(vec![]).await;
    let preflight = |origin: &str, uri: &str| {
        req(
            "OPTIONS",
            uri,
            Some(LOOPBACK),
            &[
                ("host", "127.0.0.1:7777"),
                ("origin", origin),
                ("access-control-request-method", "DELETE"),
                ("access-control-request-headers", "authorization,content-type,last-event-id"),
            ],
            None,
        )
    };
    let (st, headers, _) = send(&app, preflight("http://localhost:5173", "/api/agents/x")).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    assert_eq!(acao(&headers), Some("http://localhost:5173"));
    let methods = headers.get("access-control-allow-methods").unwrap().to_str().unwrap();
    assert!(methods.contains("PATCH") && methods.contains("DELETE"), "{methods}");
    assert_eq!(headers.get("access-control-allow-headers").unwrap(), "authorization,content-type,last-event-id");
    assert!(headers.get("access-control-allow-credentials").is_none());
    assert!(headers.get("vary").unwrap().to_str().unwrap().contains("Origin"));

    for origin in ["https://evil.example", "null", "http://192.168.1.77:8080"] {
        let (st, headers, _) = send(&app, preflight(origin, "/api/agents/x")).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{origin}");
        assert_eq!(acao(&headers), None, "{origin}");
    }
}

#[tokio::test]
async fn webhooks_answer_any_origin_but_never_with_credentials() {
    let (app, e, _d, ws) = fixture(vec![]).await;
    std::fs::write(ws.path().join("ordine-7.txt"), "x").unwrap();
    let wf = e
        .save_workflow(
            serde_json::from_value(json!({
                "id": "hook", "name": "Da webhook", "enabled": true, "trigger": {"kind": "webhook"}, "inputs": {},
                "steps": [{"id": "find", "kind": "tool", "tool": "glob", "args": {"pattern": "*{{inputs.order}}*"}}]
            }))
            .unwrap(),
        )
        .unwrap();
    let WorkflowTrigger::Webhook { secret } = &wf.trigger else { panic!("trigger") };

    // Preflight from a random site: allowed, any origin, POST only, no credentials.
    let pre = req(
        "OPTIONS",
        "/hooks/hook",
        Some(LAN),
        &[
            ("origin", "https://ci.example.com"),
            ("access-control-request-method", "POST"),
            ("access-control-request-headers", "x-aiw-secret,content-type"),
        ],
        None,
    );
    let (st, headers, _) = send(&app, pre).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    assert_eq!(acao(&headers), Some("*"));
    assert_eq!(headers.get("access-control-allow-methods").unwrap(), "POST, OPTIONS");
    assert!(headers.get("access-control-allow-credentials").is_none());

    // The call itself: the secret is the credential. Loopback or LAN, cross-site or not.
    for peer in [Some(LAN), Some(LOOPBACK)] {
        let h = [("origin", "https://ci.example.com"), ("sec-fetch-site", "cross-site"), ("x-aiw-secret", secret.as_str())];
        let (st, headers, body) = send(&app, req("POST", "/hooks/hook", peer, &h, Some(json!({"order": "7"})))).await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(body["run_id"].is_string());
        assert_eq!(acao(&headers), Some("*"));
        assert!(headers.get("access-control-allow-credentials").is_none());
    }
    // Without the right secret it is still refused, by the webhook itself.
    let h = [("origin", "https://ci.example.com"), ("x-aiw-secret", "sbagliato")];
    let (st, _, body) = send(&app, req("POST", "/hooks/hook", Some(LAN), &h, Some(json!({})))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert_ne!(body["error"], "origine non consentita");
}

// ------------------------------------------------------------------ pairing

#[tokio::test]
async fn pairing_from_a_web_page_is_refused_and_does_not_cost_an_attempt() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let code = e.pairing_code();
    let host = ("host", "192.168.1.5:7777");
    // A page on another site (a DNS-rebinding page included: its own name is not a LAN name).
    for origin in ["https://evil.example", "http://attacker.example:7777", "null"] {
        for _ in 0..8 {
            let h = [host, ("origin", origin)];
            let (st, _, body) = send(&app, req("POST", "/api/pair", Some(LAN), &h, Some(json!({"code": code})))).await;
            assert_eq!(st, StatusCode::FORBIDDEN, "{origin}");
            assert_eq!(body["error"], "origine non consentita");
        }
    }
    let h = [host, ("sec-fetch-site", "cross-site")];
    let (st, ..) = send(&app, req("POST", "/api/pair", Some(LAN), &h, Some(json!({"code": code})))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert_eq!(e.pairing_code(), code, "the code was neither guessed nor consumed");
    // The page served by this very listener, on the LAN, pairs normally: the wrong code first.
    let h = [host, ("origin", "http://192.168.1.5:7777"), ("sec-fetch-site", "same-origin")];
    let (st, _, body) = send(&app, req("POST", "/api/pair", Some(LAN), &h, Some(json!({"code": "abcdef"})))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "codice di pairing non valido");
    let (st, _, body) = send(&app, req("POST", "/api/pair", Some(LAN), &h, Some(json!({"code": code})))).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["token"], e.token());
    // A client that sends no Origin at all (another AIW, curl) works too.
    let code = e.pairing_code();
    let (st, ..) = send(&app, req("POST", "/api/pair", Some([192, 168, 1, 60]), &[host], Some(json!({"code": code})))).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn pairing_brute_force_is_stopped_with_a_retry_hint() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let code = e.pairing_code();
    let first = if code.starts_with('0') { '1' } else { '0' };
    let wrong = format!("{first}{}", &code[1..]);
    let pair = |ip: [u8; 4], code: &str| req("POST", "/api/pair", Some(ip), &[("host", "192.168.1.5:7777")], Some(json!({"code": code})));

    // One client: five wrong codes, then it is out, even with the right one.
    for _ in 0..aiw_engine::pairing::MAX_FAILURES_PER_CLIENT {
        let (st, ..) = send(&app, pair([192, 168, 1, 9], &wrong)).await;
        assert_eq!(st, StatusCode::FORBIDDEN);
    }
    let (st, headers, body) = send(&app, pair([192, 168, 1, 9], &code)).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "{body}");
    let retry: u64 = headers.get("retry-after").unwrap().to_str().unwrap().parse().unwrap();
    assert!((1..=600).contains(&retry), "{retry}");
    assert_eq!(body["retry_after_s"], retry);
    assert!(body["error"].as_str().unwrap().contains("riprova tra"), "{body}");

    // A swarm of addresses: the global limit stops them all, and the code changes under their feet.
    for i in 0..aiw_engine::pairing::MAX_FAILURES - aiw_engine::pairing::MAX_FAILURES_PER_CLIENT {
        let (st, ..) = send(&app, pair([10, 0, 0, i as u8], &wrong)).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "attempt {i}");
    }
    let rotated = e.pairing_code();
    assert_ne!(rotated, code, "the code is renewed when the limit trips");
    let (st, headers, body) = send(&app, pair([10, 9, 9, 9], &rotated)).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "even the right code waits: {body}");
    let retry: u64 = headers.get("retry-after").unwrap().to_str().unwrap().parse().unwrap();
    assert!(retry > 60, "{retry}");
    // The in-process dispatcher is a client like any other.
    let (st, _) = dispatch(app.clone(), "POST", "/api/pair", Some(json!({"code": rotated}))).await;
    assert_eq!(st, 429);
    // And the token never leaked.
    assert!(body.get("token").is_none());
}

// ------------------------------------------------------------------ SSE resume

/// Ids of the events in an SSE body, in order.
fn sse_ids(body: &str) -> Vec<u64> {
    body.lines().filter_map(|l| l.strip_prefix("id: ").or_else(|| l.strip_prefix("id:"))).filter_map(|v| v.trim().parse().ok()).collect()
}

async fn sse(app: &Router, uri: &str, headers: &[(&str, &str)]) -> String {
    let res = tower::ServiceExt::oneshot(app.clone(), req("GET", uri, None, headers, None)).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    String::from_utf8(axum::body::to_bytes(res.into_body(), 1 << 22).await.unwrap().to_vec()).unwrap()
}

async fn finished_run(app: &Router, e: &Engine) -> String {
    let (_, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "planner"}))).await;
    let sid = ses["id"].as_str().unwrap().to_string();
    let (st, run) = dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/runs"), Some(json!({"input": "Cosa c'è qui?"}))).await;
    assert_eq!(st, 200, "{run}");
    let rid = run["run_id"].as_str().unwrap().to_string();
    for _ in 0..200 {
        if e.run_done(&rid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    rid
}

#[tokio::test]
async fn sse_resumes_after_the_last_event_id() {
    let (app, e, _d, _w) =
        fixture(vec![ScriptedBackend::tool_turn("c1", "list_dir", json!({"path": "."})), ScriptedBackend::text_turn("Vuota")]).await;
    let rid = finished_run(&app, &e).await;
    let all: Vec<u64> = e.run_events(&rid).iter().map(|e| e.seq).collect();
    assert!(all.len() > 8, "{all:?}");
    let uri = format!("/api/runs/{rid}/events");

    assert_eq!(sse_ids(&sse(&app, &uri, &[]).await), all, "a fresh client gets everything");
    // EventSource sends Last-Event-ID by itself when it reconnects.
    for cut in [0u64, 3, all.len() as u64 - 2] {
        let body = sse(&app, &uri, &[("last-event-id", &cut.to_string())]).await;
        let want: Vec<u64> = all.iter().copied().filter(|s| *s > cut).collect();
        assert_eq!(sse_ids(&body), want, "Last-Event-ID: {cut}");
        assert!(!want.is_empty() && body.contains("RUN_FINISHED"), "the end of the run is still delivered");
        // The query parameter does the same.
        let by_query = sse(&app, &format!("{uri}?after={cut}"), &[]).await;
        assert_eq!(sse_ids(&by_query), want, "?after={cut}");
    }
    assert!(!sse(&app, &uri, &[("last-event-id", "3")]).await.contains("RUN_STARTED"), "events already seen are not replayed");
    // Nothing left to send: the stream just ends.
    let last = all.last().unwrap().to_string();
    assert_eq!(sse_ids(&sse(&app, &uri, &[("last-event-id", &last)]).await), Vec::<u64>::new());
    // The query wins over the header; garbage in the header means "from the start".
    assert_eq!(sse_ids(&sse(&app, &format!("{uri}?after=5"), &[("last-event-id", "2")]).await).first(), Some(&6));
    assert_eq!(sse_ids(&sse(&app, &uri, &[("last-event-id", "boh")]).await), all);
    // An unknown run has nothing to follow: the stream is closed, not left open.
    assert!(sse_ids(&sse(&app, "/api/runs/run_assente/events", &[]).await).is_empty());
}

#[tokio::test]
async fn sse_resume_also_filters_the_live_part_of_a_running_run() {
    let (app, e, _d, ws) = fixture(vec![
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "nota.md", "content": "ciao"})),
        ScriptedBackend::text_turn("Fatto"),
    ])
    .await;
    // The operator asks before writing: the run stays open until we answer.
    let (_, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "operator"}))).await;
    let sid = ses["id"].as_str().unwrap().to_string();
    let (_, run) = dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/runs"), Some(json!({"input": "scrivi"}))).await;
    let rid = run["run_id"].as_str().unwrap().to_string();
    for _ in 0..200 {
        if !e.approvals().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let seen: Vec<u64> = e.run_events(&rid).iter().map(|e| e.seq).collect();
    assert!(seen.len() > 3 && !e.run_done(&rid), "{seen:?}");
    let resume_from = seen[2];

    let (app2, uri) = (app.clone(), format!("/api/runs/{rid}/events"));
    let reader = tokio::spawn(async move { sse(&app2, &uri, &[("last-event-id", &resume_from.to_string())]).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    let id = e.approvals()[0].id.clone();
    e.respond_approval(&id, true, false).unwrap();
    let body = tokio::time::timeout(Duration::from_secs(10), reader).await.expect("stream ends with the run").unwrap();

    let all: Vec<u64> = e.run_events(&rid).iter().map(|e| e.seq).collect();
    let want: Vec<u64> = all.iter().copied().filter(|s| *s > resume_from).collect();
    assert_eq!(sse_ids(&body), want, "history after the cut, then the live events, no gaps or repeats");
    assert!(body.contains("RUN_FINISHED"));
    assert_eq!(std::fs::read_to_string(ws.path().join("nota.md")).unwrap(), "ciao");
}

// ------------------------------------------------------------------ delete and rename

#[tokio::test]
async fn delete_and_rename_routes() {
    let (app, e, _d, _w) = fixture(vec![]).await;

    // Built-in agents are refused with a conflict and an Italian message; custom ones go away.
    let (st, body) = dispatch(app.clone(), "DELETE", "/api/agents/operator", None).await;
    assert_eq!(st, 409, "{body}");
    assert!(body["error"].as_str().unwrap().contains("integrato"), "{body}");
    assert!(e.agent("operator").is_ok());
    let (st, _) = dispatch(app.clone(), "DELETE", "/api/agents/non-esiste", None).await;
    assert_eq!(st, 404);
    let mut mine = e.agent("planner").unwrap();
    mine.id = "revisore".into();
    mine.name = "Revisore".into();
    let (st, _) = dispatch(app.clone(), "PUT", "/api/agents/revisore", Some(serde_json::to_value(&mine).unwrap())).await;
    assert_eq!(st, 200);
    let (st, _) = dispatch(app.clone(), "DELETE", "/api/agents/revisore", None).await;
    assert_eq!(st, 200);
    let (_, agents) = dispatch(app.clone(), "GET", "/api/agents", None).await;
    assert!(agents.as_array().unwrap().iter().all(|a| a["id"] != "revisore"));

    // A profile an agent points to is refused; a free one is deleted.
    let (st, body) = dispatch(app.clone(), "DELETE", "/api/profiles/default", None).await;
    assert_eq!(st, 409, "{body}");
    assert!(body["error"].as_str().unwrap().contains("Operatore"), "{body}");
    let spare = json!({"id": "x", "name": "Scorta", "runtime_id": "llama-embedded", "model": "m", "params": {}});
    let (st, _) = dispatch(app.clone(), "PUT", "/api/profiles/scorta", Some(spare)).await;
    assert_eq!(st, 200);
    let (st, _) = dispatch(app.clone(), "DELETE", "/api/profiles/scorta", None).await;
    assert_eq!(st, 200);
    let (st, _) = dispatch(app.clone(), "DELETE", "/api/profiles/scorta", None).await;
    assert_eq!(st, 404);

    // PATCH /api/sessions/{id} renames and returns the session.
    let (_, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "operator"}))).await;
    let sid = ses["id"].as_str().unwrap();
    let (st, renamed) = dispatch(app.clone(), "PATCH", &format!("/api/sessions/{sid}"), Some(json!({"title": "  Bilancio 2026 "}))).await;
    assert_eq!(st, 200, "{renamed}");
    assert_eq!(renamed["id"], sid);
    assert_eq!(renamed["title"], "Bilancio 2026");
    assert_eq!(renamed["agent_id"], "operator");
    let (_, got) = dispatch(app.clone(), "GET", &format!("/api/sessions/{sid}"), None).await;
    assert_eq!(got["title"], "Bilancio 2026");
    let (st, _) = dispatch(app.clone(), "PATCH", &format!("/api/sessions/{sid}"), Some(json!({"title": "   "}))).await;
    assert_eq!(st, 400);
    let (st, _) = dispatch(app.clone(), "PATCH", "/api/sessions/ses_assente", Some(json!({"title": "x"}))).await;
    assert_eq!(st, 404);

    // The new verbs go through the same guard as the others.
    let h = [("host", "127.0.0.1:7777"), ("origin", "https://evil.example")];
    let (st, ..) = send(&app, req("DELETE", "/api/sessions/ses_assente", Some(LOOPBACK), &h, None)).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, ..) = send(&app, req("PATCH", &format!("/api/sessions/{sid}"), Some(LOOPBACK), &h, Some(json!({"title": "x"})))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert_eq!(e.session(sid).unwrap().title, "Bilancio 2026");
}

#[tokio::test]
async fn responses_cannot_be_framed_or_sniffed() {
    let (app, _e, _d, _w) = fixture(vec![]).await;
    for path in ["/api/system", "/api/nope"] {
        let (_, h, _) = send(&app, req("GET", path, Some([127, 0, 0, 1]), &[("host", "127.0.0.1:7777")], None)).await;
        assert_eq!(h.get("x-frame-options").unwrap(), "DENY", "{path}");
        assert_eq!(h.get("content-security-policy").unwrap(), "frame-ancestors 'none'", "{path}");
        assert_eq!(h.get("x-content-type-options").unwrap(), "nosniff", "{path}");
        assert_eq!(h.get("referrer-policy").unwrap(), "no-referrer", "{path}");
    }
}

#[tokio::test]
async fn the_reward_endpoint_needs_the_token_from_another_machine() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let body = || Some(json!({"task_id": "t", "output": "x"}));
    let (st, ..) = send(&app, req("POST", "/api/evals/s/reward", Some(LAN), &[], body())).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    // With the token the request reaches the route (the suite does not exist: 404).
    let bearer = format!("Bearer {}", e.token());
    let (st, ..) = send(&app, req("POST", "/api/evals/s/reward", Some(LAN), &[("authorization", &bearer)], body())).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // A web page acting on its own on this machine is refused like for the other routes.
    let (st, ..) = send(&app, req("POST", "/api/evals/s/reward", Some(LOOPBACK), &[("host", "evil.example")], body())).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
