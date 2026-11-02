use super::*;
use aiw_engine::EngineOptions;
use aiw_runtimes::ScriptedBackend;
use std::sync::Arc;

async fn app() -> (Router, Engine, tempfile::TempDir, tempfile::TempDir) {
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
    e.register_native_runtime(
        "llama-embedded",
        Arc::new(ScriptedBackend::new(vec![
            ScriptedBackend::tool_turn("c1", "list_dir", json!({"path": "."})),
            ScriptedBackend::text_turn("Vuota"),
            ScriptedBackend::text_turn("Ciao dal gateway"),
        ])),
    );
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

#[tokio::test]
async fn rest_run_and_gateway_roundtrip() {
    let (app, e, _d, _w) = app().await;
    let (st, sys) = dispatch(app.clone(), "GET", "/api/system", None).await;
    assert_eq!(st, 200);
    assert!(sys["pairing_code"].is_string(), "local callers see the pairing code");
    let (_, rts) = dispatch(app.clone(), "GET", "/api/runtimes", None).await;
    assert!(rts.as_array().unwrap().len() >= 25);
    let (st, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "planner"}))).await;
    assert_eq!(st, 200, "{ses}");
    let sid = ses["id"].as_str().unwrap().to_string();
    let (st, run) = dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/runs"), Some(json!({"input": "Cosa c'è qui?"}))).await;
    assert_eq!(st, 200, "{run}");
    let rid = run["run_id"].as_str().unwrap().to_string();
    for _ in 0..200 {
        if e.run_done(&rid) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let (_, evs) = dispatch(app.clone(), "GET", &format!("/api/runs/{rid}/events.json"), None).await;
    let types: Vec<String> = evs.as_array().unwrap().iter().map(|e| e["event"]["type"].as_str().unwrap().to_string()).collect();
    assert_eq!(types.first().map(String::as_str), Some("RUN_STARTED"));
    assert_eq!(types.last().map(String::as_str), Some("RUN_FINISHED"));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let (_, msgs) = dispatch(app.clone(), "GET", &format!("/api/sessions/{sid}/messages"), None).await;
    assert_eq!(msgs.as_array().unwrap().len(), 4);

    // OpenAI-compatible gateway (non-streaming) backed by the same runtime.
    let (st, out) = dispatch(
        app.clone(),
        "POST",
        "/v1/chat/completions",
        Some(json!({"model": "default", "messages": [{"role": "user", "content": "ciao"}]})),
    )
    .await;
    assert_eq!(st, 200, "{out}");
    assert_eq!(out["choices"][0]["message"]["content"], "Ciao dal gateway");
    let (_, models) = dispatch(app.clone(), "GET", "/v1/models", None).await;
    assert!(models["data"].as_array().unwrap().iter().any(|m| m["id"] == "default"));

    // Validation errors map to 4xx.
    let (st, _) =
        dispatch(app.clone(), "PUT", "/api/config", Some(json!({"layer": "global", "key": "policy.mode", "value": "nope"}))).await;
    assert_eq!(st, 400);
    let (st, _) = dispatch(app, "GET", "/api/sessions/none/messages", None).await;
    assert_eq!(st, 200);
}

#[tokio::test]
async fn lan_requests_need_the_token() {
    let (app, e, _d, _w) = app().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // Simulate a LAN peer by rewriting ConnectInfo to a private address.
    let lan_app = app.layer(axum::middleware::from_fn(|mut req: Request, next: Next| async move {
        req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([192, 168, 1, 50], 5555))));
        next.run(req).await
    }));
    tokio::spawn(async move { axum::serve(listener, lan_app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap() });
    let base = format!("http://{addr}");
    let client = reqwest_like_get(&format!("{base}/api/system"), None).await;
    assert_eq!(client.0, 401);
    let ok = reqwest_like_get(&format!("{base}/api/system"), Some(e.token())).await;
    assert_eq!(ok.0, 200);
    assert!(ok.1.get("pairing_code").is_none(), "remote callers never see the pairing code");
}

/// Minimal HTTP GET without extra dependencies.
async fn reqwest_like_get(url: &str, token: Option<&str>) -> (u16, Value) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let u = url.strip_prefix("http://").unwrap();
    let (host, path) = u.split_once('/').unwrap();
    let mut s = tokio::net::TcpStream::connect(host).await.unwrap();
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    s.write_all(format!("GET /{path} HTTP/1.1\r\nHost: {host}\r\n{auth}Connection: close\r\n\r\n").as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = text.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
    // chunked encoding may wrap the JSON; extract the outermost object
    let start = body.find('{').unwrap_or(0);
    let end = body.rfind('}').map(|i| i + 1).unwrap_or(body.len());
    (status, serde_json::from_str(&body[start..end]).unwrap_or(Value::Null))
}

#[tokio::test]
async fn anthropic_messages_gateway() {
    let (app, _e, _d, _w) = app().await;
    // first scripted turn is a tool call → Anthropic tool_use block
    let (st, out) = dispatch(
        app.clone(),
        "POST",
        "/v1/messages",
        Some(json!({"model": "claude-sonnet-x", "max_tokens": 200, "tools": [{"name": "list_dir", "description": "d", "input_schema": {"type": "object"}}], "messages": [{"role": "user", "content": "elenca"}]})),
    )
    .await;
    assert_eq!(st, 200, "{out}");
    assert_eq!(out["type"], "message");
    assert_eq!(out["stop_reason"], "tool_use");
    assert_eq!(out["content"][0]["type"], "tool_use");
    assert_eq!(out["content"][0]["input"]["path"], ".");
    let (st, out) = dispatch(
        app.clone(),
        "POST",
        "/v1/messages",
        Some(json!({"model": "default", "max_tokens": 50, "messages": [{"role": "user", "content": "ciao"}]})),
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(out["content"][0]["text"], "Vuota");
    assert_eq!(out["stop_reason"], "end_turn");
    let (_, n) =
        dispatch(app, "POST", "/v1/messages/count_tokens", Some(json!({"messages": [{"role": "user", "content": "ciao a tutti"}]}))).await;
    assert!(n["input_tokens"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn mcp_server_works_with_our_mcp_client() {
    let (app, _e, _d, ws) = app().await;
    std::fs::write(ws.path().join("hello.txt"), "ciao").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap() });
    let cfg = McpServerConfig {
        id: "x".into(),
        name: "aiw".into(),
        transport: "http".into(),
        command: None,
        args: None,
        url: Some(format!("http://{addr}/mcp")),
        env: None,
        enabled: true,
    };
    let client = aiw_tools::mcp::McpClient::connect(&cfg).await.unwrap();
    assert!(client.tools.iter().any(|t| t.name == "list_dir" && t.read_only));
    assert!(client.tools.iter().any(|t| t.name == "aiw_agent_run"));
    let out = client.call_tool("list_dir", json!({"path": "."})).await.unwrap();
    assert!(!out.is_error && out.content.contains("hello.txt"), "{}", out.content);
    let denied = client.call_tool("read_file", json!({"path": "/etc/passwd"})).await.unwrap();
    assert!(denied.is_error, "outside the workspace must fail: {}", denied.content);
}

#[tokio::test]
async fn webhook_runs_the_workflow_with_the_payload() {
    let (app, e, _d, ws) = app().await;
    std::fs::write(ws.path().join("ordine-42.txt"), "x").unwrap();
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
    assert!(secret.len() >= 32, "secret generated on save");

    let call = |hdr: Option<(&'static str, String)>| {
        let mut b = Request::builder().method("POST").uri("/hooks/hook").header("content-type", "application/json");
        if let Some((k, v)) = hdr {
            b = b.header(k, v);
        }
        b.body(Body::from(r#"{"order": "42"}"#)).unwrap()
    };
    let r = tower::ServiceExt::oneshot(app.clone(), call(Some(("x-aiw-secret", "sbagliato".into())))).await.unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let r = tower::ServiceExt::oneshot(app.clone(), call(None)).await.unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);

    // GitHub-style HMAC signature of the body.
    let mac = aiw_engine::webhook::hmac_sha256(secret.as_bytes(), br#"{"order": "42"}"#);
    let sig = format!("sha256={}", mac.iter().map(|b| format!("{b:02x}")).collect::<String>());
    let r = tower::ServiceExt::oneshot(app.clone(), call(Some(("x-hub-signature-256", sig)))).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let v: Value = serde_json::from_slice(&axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()).unwrap();
    let run_id = v["run_id"].as_str().unwrap().to_string();
    for _ in 0..200 {
        if e.workflow_runs().iter().any(|r| r.id == run_id && r.status == "succeeded") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let run = e.workflow_runs().into_iter().find(|r| r.id == run_id).unwrap();
    assert_eq!(run.status, "succeeded", "{run:?}");
    assert!(run.steps[0].output.as_deref().unwrap_or_default().contains("ordine-42.txt"));

    // The plain secret works too.
    let r = tower::ServiceExt::oneshot(app, call(Some(("x-aiw-secret", secret.clone())))).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}
