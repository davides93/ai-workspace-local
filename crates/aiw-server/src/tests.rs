use super::*;
use aiw_engine::EngineOptions;
use aiw_runtimes::ScriptedBackend;
use std::sync::Arc;

pub(crate) async fn app() -> (Router, Engine, tempfile::TempDir, tempfile::TempDir) {
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

/// 16 kHz mono 16-bit PCM WAV of `ms` milliseconds of silence.
fn wav(ms: u32) -> Vec<u8> {
    let data_len = 32 * ms;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&16_000u32.to_le_bytes());
    b.extend_from_slice(&32_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    b.resize(b.len() + data_len as usize, 0);
    b
}

/// Sends a non-JSON body through the router; returns the status and the body (JSON or text).
async fn send_bytes(app: Router, path: &str, content_type: &str, body: Vec<u8>) -> (u16, Value) {
    use tower::ServiceExt;
    let req = axum::http::Request::builder().method("POST").uri(path).header("content-type", content_type).body(Body::from(body)).unwrap();
    let res = app.oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned())))
}

/// OpenAI-style multipart upload to `/v1/audio/transcriptions`.
async fn upload(app: Router, audio: Vec<u8>, response_format: &str) -> (u16, Value) {
    let b = "aiwBoundary7MA4YWxk";
    let mut body = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(&audio);
    body.extend_from_slice(
        format!("\r\n--{b}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nwhisper-1\r\n--{b}\r\nContent-Disposition: form-data; name=\"response_format\"\r\n\r\n{response_format}\r\n--{b}--\r\n").as_bytes(),
    );
    send_bytes(app, "/v1/audio/transcriptions", &format!("multipart/form-data; boundary={b}"), body).await
}

/// Stand-in for whisper.cpp's server: `GET /` and multipart `POST /inference`. Records the fields.
async fn fake_whisper() -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let log = seen.clone();
    let app = Router::new().route("/", get(|| async { "whisper.cpp server" })).route(
        "/inference",
        post(move |mut form: axum::extract::Multipart| {
            let log = log.clone();
            async move {
                let mut fields = Vec::new();
                while let Some(f) = form.next_field().await.unwrap() {
                    let name = f.name().unwrap_or_default().to_string();
                    let ct = f.content_type().unwrap_or_default().to_string();
                    let data = f.bytes().await.unwrap();
                    fields.push(match name.as_str() {
                        "file" => format!("file:{ct}:{}", aiw_runtimes::stt::wav_info(&data).map(|w| w.duration_ms).unwrap_or(0)),
                        _ => format!("{name}={}", String::from_utf8_lossy(&data)),
                    });
                }
                log.lock().unwrap().push(fields.join(" "));
                Json(json!({"text": " Ciao dal microfono.\n [BLANK_AUDIO]"}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), seen)
}

#[tokio::test]
async fn voice_transcribes_with_whisper_and_never_bounces_between_devices() {
    use base64::Engine as _;
    let (app, e, _d, _w) = app().await;
    let set = |e: &Engine, id: &str, key: &str, v: &str| {
        let mut m = Map::new();
        m.insert(key.into(), json!(v));
        e.update_runtime_settings(id, m).unwrap();
    };
    let (_, st) = dispatch(app.clone(), "GET", "/api/voice", None).await;
    assert_eq!(st["available"], false, "{st}");
    assert!(st["detail"].as_str().unwrap().contains("whisper.cpp"), "{st}");

    // whisper-server already running (started by the user): used as it is.
    let (base, seen) = fake_whisper().await;
    set(&e, "whisper-server", "base_url", &base);
    e.refresh_runtimes().await;
    let (_, st) = dispatch(app.clone(), "GET", "/api/voice", None).await;
    assert_eq!((st["available"].as_bool(), st["runtime"].as_str()), (Some(true), Some("whisper-server")), "{st}");

    // Raw WAV from the web UI.
    let (code, t) = send_bytes(app.clone(), "/api/voice/transcribe?language=it", "audio/wav", wav(1200)).await;
    assert_eq!(code, 200, "{t}");
    assert_eq!(
        (t["text"].as_str(), t["duration_ms"].as_u64(), t["language"].as_str()),
        (Some("Ciao dal microfono."), Some(1200), Some("it"))
    );
    // JSON + base64 from the app's IPC bridge.
    let audio = base64::engine::general_purpose::STANDARD.encode(wav(500));
    let (code, t) = dispatch(app.clone(), "POST", "/api/voice/transcribe", Some(json!({"audio": audio, "mime": "audio/wav"}))).await;
    assert_eq!((code, t["duration_ms"].as_u64()), (200, Some(500)), "{t}");
    // OpenAI-compatible multipart (other apps, paired devices).
    let (code, t) = upload(app.clone(), wav(300), "text").await;
    assert_eq!((code, t.as_str()), (200, Some("Ciao dal microfono.")));
    let (code, t) = upload(app.clone(), wav(300), "verbose_json").await;
    assert_eq!((code, t["duration"].as_f64()), (200, Some(0.3)), "{t}");
    // Not audio.
    let (code, _) = send_bytes(app.clone(), "/api/voice/transcribe", "audio/wav", b"not audio".to_vec()).await;
    assert_eq!(code, 400);
    let log = seen.lock().unwrap().clone();
    assert!(log[0].contains("file:audio/wav:1200") && log[0].contains("language=it") && log[0].contains("response_format=json"), "{log:?}");
    assert!(!log[1].contains("language="), "with language auto nothing is forced: {log:?}");

    // Only a paired device left: the app may use it, but a request that came from another device
    // is not forwarded (two devices without whisper would bounce the audio forever).
    set(&e, "whisper-server", "base_url", "");
    set(&e, "aiw-peer", "base_url", "http://127.0.0.1:9/v1");
    e.refresh_runtimes().await;
    let (_, st) = dispatch(app.clone(), "GET", "/api/voice", None).await;
    assert_eq!(st["runtime"], "aiw-peer", "{st}");
    let (code, err) = upload(app.clone(), wav(300), "json").await;
    assert_eq!(code, 422, "{err}");
}

#[tokio::test]
async fn voice_endpoints_refuse_in_the_api_error_shape() {
    use axum::http::header::CONTENT_TYPE;
    use tower::ServiceExt;
    let (app, _e, _d, _w) = app().await;
    // Not multipart / no file field / audio that is not WAV: JSON errors in Italian.
    let (code, err) = send_bytes(app.clone(), "/v1/audio/transcriptions", "application/json", b"{}".to_vec()).await;
    assert_eq!(code, 400, "{err}");
    assert!(err["error"].as_str().unwrap().contains("multipart"), "{err}");
    let b = "aiwB";
    let empty = format!("--{b}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nwhisper-1\r\n--{b}--\r\n").into_bytes();
    let (code, err) = send_bytes(app.clone(), "/v1/audio/transcriptions", &format!("multipart/form-data; boundary={b}"), empty).await;
    assert_eq!((code, err["error"].as_str()), (400, Some("manca il campo file")));
    let (code, err) = send_bytes(app.clone(), "/api/voice/transcribe", "application/json", b"{\"audio\": 5}".to_vec()).await;
    assert_eq!(code, 400, "{err}");
    let (code, err) = send_bytes(app.clone(), "/api/voice/transcribe", "application/json", b"{\"audio\": \"!!\"}".to_vec()).await;
    assert_eq!((code, err["error"].as_str()), (400, Some("l'audio non è base64 valido")));
    // The size limits hold on both routes, with a 413 and an Italian message.
    let big = vec![0u8; aiw_engine::voice::MAX_AUDIO_BYTES / 3 * 4 + (2 << 20)];
    let (code, err) = send_bytes(app.clone(), "/api/voice/transcribe", "audio/wav", big.clone()).await;
    assert_eq!(code, 413, "{err}");
    assert!(err["error"].as_str().unwrap().contains("limite"), "{err}");
    let (code, err) = upload(app.clone(), big, "json").await;
    assert_eq!(code, 413, "{err}");
    assert!(err["error"].as_str().unwrap().contains("limite"), "{err}");
    // The routes sit behind the router's middleware (hardening headers).
    let res = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/audio/transcriptions")
                .header(CONTENT_TYPE, "text/plain")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.headers().get("x-frame-options").map(|v| v.to_str().unwrap()), Some("DENY"));
}
