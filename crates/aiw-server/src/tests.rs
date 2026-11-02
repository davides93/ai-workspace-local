use super::*;
use aiw_engine::EngineOptions;
use aiw_runtimes::ScriptedBackend;
use std::sync::Arc;

async fn app() -> (Router, Engine, tempfile::TempDir, tempfile::TempDir) {
    app_on(None).await
}

/// `device` overrides the probed hardware (two engines in one test are two devices).
async fn app_on(device: Option<DeviceProfile>) -> (Router, Engine, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().into()),
        workspace_root: Some(ws.path().into()),
        mode: "daemon".into(),
        offline: true,
        device,
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
    // An unknown session has no messages to show: 404, like every other route of a session.
    let (st, _) = dispatch(app, "GET", "/api/sessions/none/messages", None).await;
    assert_eq!(st, 404);
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
        headers: None,
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

async fn rpc(app: &Router, body: Value) -> Value {
    let req = Request::builder()
        .method("POST")
        .uri("/a2a")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let r = tower::ServiceExt::oneshot(app.clone(), req).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    serde_json::from_slice(&axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()).unwrap()
}

#[tokio::test]
async fn a2a_card_and_tasks() {
    let (app, _e, _d, _w) = app().await;
    let req = Request::builder().uri("/.well-known/agent-card.json").header("host", "pc.local:7777").body(Body::empty()).unwrap();
    let r = tower::ServiceExt::oneshot(app.clone(), req).await.unwrap();
    let card: Value = serde_json::from_slice(&axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap()).unwrap();
    assert_eq!(card["supportedInterfaces"][0]["url"], "http://pc.local:7777/a2a");
    assert_eq!(card["supportedInterfaces"][0]["protocolBinding"], "JSONRPC");
    assert!(card["skills"].as_array().unwrap().iter().any(|s| s["id"] == "operator"));

    // A2A 1.0: SendMessage (blocking) → completed Task with the answer as artifact.
    let v = rpc(
        &app,
        json!({"jsonrpc": "2.0", "id": 1, "method": "SendMessage", "params": {
            "message": {"role": "ROLE_USER", "messageId": "m1", "parts": [{"text": "Cosa c'è nella cartella?"}]}
        }}),
    )
    .await;
    let task = &v["result"]["task"];
    assert_eq!(task["status"]["state"], "TASK_STATE_COMPLETED", "{v}");
    assert_eq!(task["artifacts"][0]["parts"][0]["text"], "Vuota");
    assert_eq!(task["history"][1]["role"], "ROLE_AGENT");
    let tid = task["id"].as_str().unwrap();
    let ctx = task["contextId"].as_str().unwrap();
    let got = rpc(&app, json!({"jsonrpc": "2.0", "id": 2, "method": "GetTask", "params": {"id": tid}})).await;
    assert_eq!(got["result"]["id"], tid);

    // 0.3 names and shapes, same context → same session.
    let v = rpc(&app, json!({"jsonrpc": "2.0", "id": 3, "method": "message/send", "params": {
        "message": {"role": "user", "kind": "message", "messageId": "m2", "contextId": ctx, "parts": [{"kind": "text", "text": "E poi?"}]}
    }}))
    .await;
    assert_eq!(v["result"]["kind"], "task");
    assert_eq!(v["result"]["status"]["state"], "completed", "{v}");
    assert_eq!(v["result"]["artifacts"][0]["parts"][0]["kind"], "text");

    let bad = rpc(&app, json!({"jsonrpc": "2.0", "id": 4, "method": "SendMessage", "params": {"message": {"parts": [{"text": "x"}]}, "metadata": {"agent": "nessuno"}}})).await;
    assert_eq!(bad["error"]["code"], -32602);
    let none = rpc(&app, json!({"jsonrpc": "2.0", "id": 5, "method": "CancelTask", "params": {"id": "task_x"}})).await;
    assert_eq!(none["error"]["code"], -32001);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pairs_with_another_device_over_http() {
    let (_app_a, a, _da, _wa) = app().await;
    let mut phone = a.device();
    phone.id = "telefono-di-prova".into();
    phone.name = "Telefono di prova".into();
    phone.os = "android".into();
    let (app_b, b, _db, _wb) = app_on(Some(phone)).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app_b.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap() });

    assert!(a.pair_with_peer(&addr.to_string(), "000000").await.is_err(), "wrong code is refused");
    let peer = a.pair_with_peer(&format!("http://{addr}/"), &b.pairing_code()).await.unwrap();
    assert_eq!(peer.name, "Telefono di prova");
    assert_eq!(peer.address, format!("http://{addr}"));
    assert!(a.peers().iter().any(|p| p.id == peer.id));

    // Its gateway is now the aiw-peer runtime, its tools and agents an MCP server.
    let rt = a.runtimes().into_iter().find(|r| r.id == "aiw-peer").unwrap();
    assert_eq!(rt.settings["base_url"], format!("http://{addr}/v1"));
    let mcp = a.mcp_servers().into_iter().find(|m| m.config.id == format!("peer-{}", peer.id)).unwrap();
    assert_eq!(mcp.status, "connected", "{:?}", mcp.error);
    assert!(mcp.tools.iter().any(|t| t.ends_with("aiw_agent_run")), "{:?}", mcp.tools);

    a.remove_peer(&peer.id).await.unwrap();
    assert!(a.peers().is_empty());
    assert!(a.mcp_servers().iter().all(|m| m.config.id != format!("peer-{}", peer.id)));
}
