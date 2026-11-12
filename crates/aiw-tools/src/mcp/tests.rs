//! Tests of the MCP client against fake servers: a Python script on stdio and an in-process axum
//! server for streamable HTTP. Nothing here touches the network beyond loopback.

use super::*;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use std::sync::atomic::AtomicU32;

fn fast() -> McpOptions {
    McpOptions {
        request_timeout: Duration::from_secs(5),
        call_timeout: Duration::from_secs(10),
        backoff_base: Duration::from_millis(20),
        backoff_cap: Duration::from_millis(80),
        max_attempts: 4,
    }
}

#[derive(Default)]
struct Recorder {
    changes: Mutex<Vec<McpChange>>,
    token: Mutex<Option<String>>,
    refreshes: AtomicU32,
}

#[async_trait]
impl McpEvents for Recorder {
    async fn access_token(&self, _id: &str, force: bool) -> Option<String> {
        if force {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
        }
        self.token.lock().unwrap().clone()
    }
    fn changed(&self, _id: &str, what: McpChange) {
        self.changes.lock().unwrap().push(what);
    }
}

impl Recorder {
    fn count(&self, what: McpChange) -> usize {
        self.changes.lock().unwrap().iter().filter(|c| **c == what).count()
    }
}

async fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..300 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timeout waiting for {what}");
}

/// A run that answers sampling and elicitation in a fixed way and remembers what it was asked.
#[derive(Default)]
struct FakeRun {
    sampled: Mutex<Vec<SamplingRequest>>,
    elicit_with: Mutex<Option<Elicited>>,
    asked: Mutex<Vec<(String, Value)>>,
}

#[async_trait]
impl RunMcp for FakeRun {
    async fn sample(&self, _server: &str, req: SamplingRequest) -> Result<SamplingReply> {
        let echo = req.messages.last().map(|m| m.1.clone()).unwrap_or_default();
        self.sampled.lock().unwrap().push(req);
        Ok(SamplingReply { text: format!("risposta a: {echo}"), model: "fake-model".into() })
    }
    async fn elicit(&self, _server: &str, message: &str, schema: &Value) -> Elicited {
        self.asked.lock().unwrap().push((message.into(), schema.clone()));
        self.elicit_with.lock().unwrap().clone().unwrap_or(Elicited::Decline)
    }
}

fn ctx_with(run: Arc<FakeRun>) -> ToolContext {
    let mut c = ToolContext::new("r", std::env::temp_dir());
    c.mcp_run = Some(run);
    c
}

// ------------------------------------------------------------------ stdio

const STDIO_SERVER: &str = r#"
import sys, json, os
marker = os.environ.get("MARKER", "")
tools = [{"name": "add", "description": "Somma", "inputSchema": {"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}}, "annotations": {"readOnlyHint": True}},
         {"name": "crash", "description": "Esce", "inputSchema": {"type": "object"}},
         {"name": "change", "description": "Cambia l'elenco", "inputSchema": {"type": "object"}},
         {"name": "ask", "description": "Chiede al client", "inputSchema": {"type": "object", "properties": {"what": {"type": "string"}}}}]
def send(o): print(json.dumps(o), flush=True)
def ask(method, params):
    send({"jsonrpc": "2.0", "id": "srv1", "method": method, "params": params})
    for line in sys.stdin:
        m = json.loads(line)
        if m.get("id") == "srv1" and "method" not in m:
            return m
for line in sys.stdin:
    m = json.loads(line)
    if "id" not in m or "method" not in m: continue
    meth = m["method"]; p = m.get("params", {})
    err = None
    if meth == "initialize":
        r = {"protocolVersion": p["protocolVersion"], "capabilities": {"tools": {"listChanged": True}, "resources": {}, "prompts": {}}, "serverInfo": {"name": "fake", "version": "1"}}
    elif meth == "tools/list":
        r = {"tools": tools}
    elif meth == "tools/call":
        n = p["name"]; a = p.get("arguments", {})
        if n == "add":
            r = {"content": [{"type": "text", "text": str(a["a"] + a["b"])}]}
        elif n == "crash":
            if marker and not os.path.exists(marker):
                open(marker, "w").write("x")
                os._exit(3)
            r = {"content": [{"type": "text", "text": "dopo il riavvio"}]}
        elif n == "change":
            tools.append({"name": "extra", "description": "Nuovo", "inputSchema": {"type": "object"}})
            send({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
            r = {"content": [{"type": "text", "text": "ok"}]}
        elif n == "ask":
            w = a.get("what")
            if w == "ping":
                resp = ask("ping", {})
                r = {"content": [{"type": "text", "text": "ping:" + json.dumps(resp.get("result"))}]}
            elif w == "sample":
                resp = ask("sampling/createMessage", {"messages": [{"role": "user", "content": {"type": "text", "text": "ciao"}}], "systemPrompt": "sii breve", "maxTokens": 50})
                r = {"content": [{"type": "text", "text": json.dumps(resp.get("result") or resp.get("error"))}]}
            else:
                resp = ask("elicitation/create", {"message": "Come ti chiami?", "requestedSchema": {"type": "object", "properties": {"nome": {"type": "string"}, "eta": {"type": "integer"}}, "required": ["nome"]}})
                r = {"content": [{"type": "text", "text": json.dumps(resp.get("result") or resp.get("error"))}]}
        else:
            r = {"content": [{"type": "text", "text": "?"}]}
    elif meth == "resources/list":
        r = {"resources": [{"uri": "file:///a.txt", "name": "a", "mimeType": "text/plain"}]}
    elif meth == "resources/templates/list":
        r = {"resourceTemplates": [{"uriTemplate": "file:///{name}", "name": "file"}]}
    elif meth == "resources/read":
        u = p["uri"]
        if u.endswith("big"):
            r = {"contents": [{"uri": u, "mimeType": "text/plain", "text": "x" * 300000}]}
        elif u.endswith("img"):
            r = {"contents": [{"uri": u, "mimeType": "image/png", "blob": "iVBORw0KGgo="}]}
        elif u.endswith("bin"):
            r = {"contents": [{"uri": u, "mimeType": "application/zip", "blob": "AAAAAAAAAAAA"}]}
        else:
            r = {"contents": [{"uri": u, "mimeType": "text/plain", "text": "contenuto di " + u}]}
    elif meth == "prompts/list":
        r = {"prompts": [{"name": "rivedi", "description": "Rivedi il codice", "arguments": [{"name": "file", "description": "il file", "required": True}]}]}
    elif meth == "prompts/get":
        r = {"description": "Revisione", "messages": [{"role": "user", "content": {"type": "text", "text": "Rivedi " + p["arguments"].get("file", "?")}}]}
    elif meth == "ping":
        r = {}
    else:
        r = None; err = {"code": -32601, "message": "no"}
    send({"jsonrpc": "2.0", "id": m["id"], **({"result": r} if err is None else {"error": err})})
"#;

fn stdio_cfg(name: &str, marker: Option<&str>, sampling: bool) -> McpServerConfig {
    let mut env = Map::new();
    if let Some(m) = marker {
        env.insert("MARKER".into(), json!(m));
    }
    McpServerConfig {
        id: format!("id-{name}"),
        name: name.into(),
        transport: "stdio".into(),
        command: Some(which::which("python3").map(|p| p.display().to_string()).unwrap_or_default()),
        args: Some(vec!["-c".into(), STDIO_SERVER.into()]),
        url: None,
        env: Some(env),
        headers: None,
        enabled: true,
        sampling,
        oauth_client_id: None,
    }
}

async fn stdio_client(name: &str, marker: Option<&str>, sampling: bool) -> Option<(Arc<McpClient>, Arc<Recorder>)> {
    which::which("python3").ok()?;
    let rec = Arc::new(Recorder::default());
    let c = McpClient::with_options(&stdio_cfg(name, marker, sampling), rec.clone(), fast());
    c.start().await.unwrap();
    Some((c, rec))
}

fn text(r: &Result<ToolOutput>) -> String {
    r.as_ref().map(|o| o.content.clone()).unwrap_or_else(|e| e.to_string())
}

#[tokio::test]
async fn stdio_roundtrip_capabilities_and_ping() {
    let Some((c, _)) = stdio_client("fake", None, false).await else { return };
    assert_eq!(c.state(), McpState::Connected);
    assert_eq!(c.capabilities(), McpCapabilities { tools: true, resources: true, prompts: true });
    assert_eq!(c.tools().len(), 4);
    let tools = McpTool::wrap(c.clone());
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert!(
        names.contains(&"mcp__fake__add") && names.contains(&"mcp__fake__list_resources") && names.contains(&"mcp__fake__read_resource")
    );
    let add = tools.iter().find(|t| t.name() == "mcp__fake__add").unwrap();
    assert_eq!(add.risk(), Risk::Read);
    let ctx = ToolContext::new("r", std::env::temp_dir());
    assert_eq!(add.call(json!({"a": 2, "b": 3}), &ctx).await.unwrap().content.trim(), "5");
    c.ping().await.unwrap();
    // The server pings the client in the middle of a call: the client answers.
    let ask = tools.iter().find(|t| t.name() == "mcp__fake__ask").unwrap();
    assert_eq!(ask.call(json!({"what": "ping"}), &ctx).await.unwrap().content, "ping:{}");
    c.shutdown().await;
}

#[tokio::test]
async fn stdio_server_that_exits_is_restarted_and_tools_are_listed_again() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("crashed");
    let Some((c, rec)) = stdio_client("crashy", Some(marker.to_str().unwrap()), false).await else { return };
    let ctx = ToolContext::new("r", std::env::temp_dir());
    // The call that kills the server fails…
    let r = c.call_tool("crash", json!({}), Some(&ctx)).await;
    assert!(r.is_err(), "{r:?}");
    // …the client goes through "riconnessione" and is connected again, with its tools.
    wait_for("reconnection", || c.state() == McpState::Connected && marker.exists()).await;
    assert!(rec.count(McpChange::Tools) >= 2, "tools listed again after the restart");
    let ok = c.call_tool("crash", json!({}), Some(&ctx)).await.unwrap();
    assert_eq!(ok.content, "dopo il riavvio");
    c.shutdown().await;
}

#[tokio::test]
async fn gives_up_after_the_maximum_attempts_and_a_manual_reconnect_works() {
    let dir = tempfile::tempdir().unwrap();
    let Some(_) = which::which("python3").ok() else { return };
    // A server that exits at once: initialisation fails every time.
    let mut cfg = stdio_cfg("dead", None, false);
    cfg.args = Some(vec!["-c".into(), "import sys; sys.exit(1)".into()]);
    let rec = Arc::new(Recorder::default());
    let c = McpClient::with_options(&cfg, rec.clone(), fast());
    assert!(c.start().await.is_err());
    wait_for("unreachable", || matches!(c.state(), McpState::Unreachable(_))).await;
    assert!(c.state().error().is_some());
    assert_eq!(c.state().status(), "unreachable");
    // «Riconnetti» with a working server starts over.
    let marker = dir.path().join("m");
    let good = stdio_cfg("dead", Some(marker.to_str().unwrap()), false);
    let c2 = McpClient::with_options(&good, rec, fast());
    c2.start().await.unwrap();
    c2.reconnect().await.unwrap();
    assert_eq!(c2.state(), McpState::Connected);
    c2.shutdown().await;
    c.shutdown().await;
}

#[tokio::test]
async fn list_changed_refreshes_the_tools() {
    let Some((c, rec)) = stdio_client("lc", None, false).await else { return };
    let before = rec.count(McpChange::Tools);
    c.call_tool("change", json!({}), None).await.unwrap();
    wait_for("refresh", || c.tools().iter().any(|t| t.name == "extra")).await;
    assert!(rec.count(McpChange::Tools) > before);
    c.shutdown().await;
}

#[tokio::test]
async fn resources_tools_list_read_and_caps() {
    let Some((c, _)) = stdio_client("res", None, false).await else { return };
    let tools = McpTool::wrap(c.clone());
    let ctx = ToolContext::new("r", std::env::temp_dir());
    let list = tools.iter().find(|t| t.name() == "mcp__res__list_resources").unwrap();
    let read = tools.iter().find(|t| t.name() == "mcp__res__read_resource").unwrap();
    assert_eq!(read.risk(), Risk::Read);
    assert_eq!(list.risk(), Risk::Read);
    let l = list.call(json!({}), &ctx).await.unwrap().content;
    assert!(l.contains("file:///a.txt") && l.contains("file:///{name}"), "{l}");
    let t = read.call(json!({"uri": "file:///x"}), &ctx).await.unwrap();
    assert!(t.content.contains("contenuto di file:///x"));
    let big = read.call(json!({"uri": "file:///big"}), &ctx).await.unwrap();
    assert!(big.content.chars().count() < RESOURCE_TEXT_CAP + 400 && big.content.contains("testo tagliato"));
    let img = read.call(json!({"uri": "file:///img"}), &ctx).await.unwrap();
    assert_eq!(img.images, vec!["data:image/png;base64,iVBORw0KGgo=".to_string()]);
    let bin = read.call(json!({"uri": "file:///bin"}), &ctx).await.unwrap();
    assert!(bin.content.contains("binario") && bin.images.is_empty());
    assert!(read.call(json!({}), &ctx).await.is_err());
    c.shutdown().await;
}

#[tokio::test]
async fn prompts_list_and_get() {
    let Some((c, _)) = stdio_client("pr", None, false).await else { return };
    let p = c.list_prompts().await.unwrap();
    assert_eq!(p[0].name, "rivedi");
    assert_eq!(p[0].arguments[0], ("file".to_string(), "il file".to_string(), true));
    let args = json!({"file": "main.rs"}).as_object().unwrap().clone();
    let (desc, text) = c.get_prompt("rivedi", &args).await.unwrap();
    assert_eq!((desc.as_str(), text.as_str()), ("Revisione", "Rivedi main.rs"));
    c.shutdown().await;
}

#[tokio::test]
async fn sampling_is_refused_when_off_and_answered_when_on() {
    let run = Arc::new(FakeRun::default());
    let ctx = ctx_with(run.clone());
    // Off (default): a proper JSON-RPC error goes back to the server, the model is never asked.
    let Some((c, _)) = stdio_client("samp-off", None, false).await else { return };
    let r = c.call_tool("ask", json!({"what": "sample"}), Some(&ctx)).await.unwrap();
    assert!(r.content.contains("-32601") && r.content.contains("abilitato"), "{}", r.content);
    assert!(run.sampled.lock().unwrap().is_empty());
    c.shutdown().await;
    // On: answered by the run.
    let Some((c, _)) = stdio_client("samp-on", None, true).await else { return };
    let r = c.call_tool("ask", json!({"what": "sample"}), Some(&ctx)).await.unwrap();
    assert!(r.content.contains("risposta a: ciao") && r.content.contains("fake-model"), "{}", r.content);
    {
        let seen = run.sampled.lock().unwrap();
        assert_eq!(seen[0].system.as_deref(), Some("sii breve"));
        assert_eq!(seen[0].max_tokens, Some(50));
    }
    // Outside a tool call of a run nobody can answer.
    c.shutdown().await;
}

#[tokio::test]
async fn elicitation_declines_by_default_and_accepts_a_valid_form() {
    let run = Arc::new(FakeRun::default());
    let ctx = ctx_with(run.clone());
    let Some((c, _)) = stdio_client("eli", None, false).await else { return };
    // Nobody can answer (no run hooks): decline.
    let plain = ToolContext::new("r", std::env::temp_dir());
    let r = c.call_tool("ask", json!({"what": "elicit"}), Some(&plain)).await.unwrap();
    assert!(r.content.contains("decline"), "{}", r.content);
    // The run declines.
    let r = c.call_tool("ask", json!({"what": "elicit"}), Some(&ctx)).await.unwrap();
    assert!(r.content.contains("decline"));
    assert_eq!(run.asked.lock().unwrap()[0].0, "Come ti chiami?");
    // The person fills the form.
    *run.elicit_with.lock().unwrap() = Some(Elicited::Accept(json!({"nome": "Ada", "eta": 36}).as_object().unwrap().clone()));
    let r = c.call_tool("ask", json!({"what": "elicit"}), Some(&ctx)).await.unwrap();
    assert!(r.content.contains("accept") && r.content.contains("Ada"), "{}", r.content);
    // An answer that does not fit the schema is turned into a decline.
    *run.elicit_with.lock().unwrap() = Some(Elicited::Accept(json!({"nome": 5}).as_object().unwrap().clone()));
    let r = c.call_tool("ask", json!({"what": "elicit"}), Some(&ctx)).await.unwrap();
    assert!(r.content.contains("decline"));
    *run.elicit_with.lock().unwrap() = Some(Elicited::Cancel);
    let r = c.call_tool("ask", json!({"what": "elicit"}), Some(&ctx)).await.unwrap();
    assert!(r.content.contains("cancel"));
    c.shutdown().await;
}

#[test]
fn elicitation_schema_is_flat_and_never_asks_for_secrets() {
    let ok = json!({"type": "object", "properties": {"a": {"type": "string", "enum": ["x", "y"]}, "b": {"type": "boolean"}}});
    assert!(validate_elicit_schema(&ok).is_ok());
    for bad in [
        json!({"type": "object", "properties": {"a": {"type": "object"}}}),
        json!({"type": "object", "properties": {"password": {"type": "string"}}}),
        json!({"type": "object", "properties": {"a": {"type": "string", "title": "Il tuo token"}}}),
        json!({"type": "object", "properties": {}}),
        json!({"type": "string"}),
        json!({"type": "object", "properties": {"a": {"type": "number", "enum": [1]}}}),
    ] {
        assert!(validate_elicit_schema(&bad).is_err(), "{bad}");
    }
    let content = |v: Value| v.as_object().unwrap().clone();
    assert!(validate_elicit_content(&ok, &content(json!({"a": "x"}))).is_ok());
    assert!(validate_elicit_content(&ok, &content(json!({"a": "z"}))).is_err());
    assert!(validate_elicit_content(&ok, &content(json!({"c": 1}))).is_err());
    let req = json!({"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]});
    assert!(validate_elicit_content(&req, &content(json!({}))).is_err());
}

#[test]
fn sampling_parameters_are_validated() {
    let ok = parse_sampling(&json!({"messages": [{"role": "user", "content": {"type": "text", "text": "a"}}], "maxTokens": 10})).unwrap();
    assert_eq!(ok.messages, vec![("user".to_string(), "a".to_string())]);
    assert!(parse_sampling(&json!({"messages": [{"role": "user", "content": {"type": "image", "data": "x"}}]})).is_err());
    assert!(parse_sampling(&json!({"messages": [{"role": "system", "content": {"type": "text", "text": "a"}}]})).is_err());
    assert!(parse_sampling(&json!({"messages": []})).is_err());
}

#[test]
fn backoff_doubles_up_to_the_cap() {
    let o = McpOptions::default();
    let d: Vec<u64> = (1..=8).map(|a| o.delay(a).as_secs()).collect();
    assert_eq!(d, vec![1, 2, 4, 8, 16, 30, 30, 30]);
}

#[test]
fn parses_sse_responses() {
    let body = "event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\r\n\r\n";
    assert_eq!(from_sse(body, 7).unwrap()["result"]["ok"], true);
    assert!(from_sse(body, 8).is_none());
    // An event split across chunks, with a UTF-8 character cut in the middle.
    let mut p = SseParser::default();
    let all = "data: {\"id\":1,\"result\":\"è\"}\n\n".as_bytes().to_vec();
    assert!(p.push(&all[..20]).is_empty());
    assert_eq!(p.push(&all[20..])[0]["result"], "è");
}

// ------------------------------------------------------------------ streamable HTTP

#[derive(Default)]
struct Fake {
    next_session: AtomicU32,
    /// Sessions numbered below this are expired (404).
    expired_below: AtomicU32,
    version: Mutex<String>,
    require_token: Mutex<Option<String>>,
    seen: Mutex<Vec<(String, Value, HeaderMap)>>,
    waiters: Mutex<HashMap<u64, oneshot::Sender<Value>>>,
    get_rx: Mutex<Option<mpsc::UnboundedReceiver<String>>>,
}

type Shared = Arc<Fake>;

fn sse(events: Vec<Value>) -> Response {
    let body: String = events.iter().map(|e| format!("event: message\ndata: {e}\n\n")).collect();
    ([("content-type", "text/event-stream")], body).into_response()
}

async fn fake_post(State(f): State<Shared>, headers: HeaderMap, body: String) -> Response {
    let msg: Value = serde_json::from_str(&body).unwrap();
    let method = msg["method"].as_str().unwrap_or("").to_string();
    f.seen.lock().unwrap().push((method.clone(), msg.clone(), headers.clone()));
    if let Some(t) = f.require_token.lock().unwrap().clone() {
        if headers.get("authorization").and_then(|h| h.to_str().ok()) != Some(&format!("Bearer {t}")) {
            return (
                StatusCode::UNAUTHORIZED,
                [("www-authenticate", "Bearer resource_metadata=\"http://x/.well-known/oauth-protected-resource\"")],
            )
                .into_response();
        }
    }
    // A response to a request of the server.
    if method.is_empty() {
        if let Some(w) = msg["id"].as_u64().and_then(|i| f.waiters.lock().unwrap().remove(&i)) {
            let _ = w.send(msg);
        }
        return StatusCode::ACCEPTED.into_response();
    }
    let Some(id) = msg.get("id").cloned() else { return StatusCode::ACCEPTED.into_response() };
    let result = |r: Value| json!({"jsonrpc": "2.0", "id": id, "result": r});
    if method == "initialize" {
        let n = f.next_session.fetch_add(1, Ordering::SeqCst) + 1;
        let version = f.version.lock().unwrap().clone();
        let version = if version.is_empty() { msg["params"]["protocolVersion"].as_str().unwrap().to_string() } else { version };
        let r = result(
            json!({"protocolVersion": version, "capabilities": {"tools": {"listChanged": true}}, "serverInfo": {"name": "fake-http", "version": "1"}}),
        );
        return ([("mcp-session-id", format!("s{n}"))], axum::Json(r)).into_response();
    }
    let session = headers.get("mcp-session-id").and_then(|h| h.to_str().ok()).unwrap_or("");
    let n: u32 = session.trim_start_matches('s').parse().unwrap_or(0);
    if n == 0 || n < f.expired_below.load(Ordering::SeqCst) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match method.as_str() {
        "tools/list" => axum::Json(result(json!({"tools": [{"name": "echo", "description": "Eco", "inputSchema": {"type": "object"}}, {"name": "ask", "description": "Chiede", "inputSchema": {"type": "object"}}]}))).into_response(),
        "tools/call" if msg["params"]["name"] == "echo" => {
            axum::Json(result(json!({"content": [{"type": "text", "text": format!("eco da {session}")}]}))).into_response()
        }
        "tools/call" if msg["params"]["name"] == "hang" => std::future::pending().await,
        "tools/call" => {
            // SSE answer with a notification and a sampling request before the result.
            let (tx, rx) = mpsc::unbounded_channel::<std::result::Result<String, std::convert::Infallible>>();
            let (wtx, wrx) = oneshot::channel();
            f.waiters.lock().unwrap().insert(900, wtx);
            let ev = |v: Value| format!("event: message\ndata: {v}\n\n");
            let _ = tx.send(Ok(ev(json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "info"}}))));
            let _ = tx.send(Ok(ev(json!({"jsonrpc": "2.0", "id": 900, "method": "sampling/createMessage", "params": {"messages": [{"role": "user", "content": {"type": "text", "text": "domanda"}}]}}))));
            tokio::spawn(async move {
                let answer = wrx.await.unwrap_or(Value::Null);
                let text = answer["result"]["content"]["text"].as_str().or(answer["error"]["message"].as_str()).unwrap_or("?").to_string();
                let _ = tx.send(Ok(ev(json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": text}]}}))));
            });
            let stream = futures::stream::unfold(rx, |mut rx| async { rx.recv().await.map(|v| (v, rx)) });
            ([("content-type", "text/event-stream")], axum::body::Body::from_stream(stream)).into_response()
        }
        "ping" => axum::Json(result(json!({}))).into_response(),
        _ => sse(vec![json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "no"}})]),
    }
}

async fn fake_get(State(f): State<Shared>) -> Response {
    let Some(rx) = f.get_rx.lock().unwrap().take() else { return StatusCode::METHOD_NOT_ALLOWED.into_response() };
    let stream = futures::stream::unfold(rx, |mut rx| async {
        rx.recv().await.map(|v| (std::result::Result::<String, std::convert::Infallible>::Ok(v), rx))
    });
    ([("content-type", "text/event-stream")], axum::body::Body::from_stream(stream)).into_response()
}

async fn http_fake() -> (String, Shared) {
    let f: Shared = Arc::new(Fake::default());
    let app = Router::new().route("/mcp", post(fake_post).get(fake_get)).with_state(f.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, f)
}

fn http_cfg(url: &str, sampling: bool) -> McpServerConfig {
    McpServerConfig {
        id: "h1".into(),
        name: "remoto".into(),
        transport: "http".into(),
        command: None,
        args: None,
        url: Some(url.into()),
        env: None,
        headers: None,
        enabled: true,
        sampling,
        oauth_client_id: None,
    }
}

#[tokio::test]
async fn http_headers_session_and_protocol_versions() {
    let (url, f) = http_fake().await;
    let c = McpClient::with_options(&http_cfg(&url, false), Arc::new(Recorder::default()), fast());
    c.start().await.unwrap();
    assert_eq!(c.tools().len(), 2);
    let out = c.call_tool("echo", json!({}), None).await.unwrap();
    assert_eq!(out.content, "eco da s1");
    {
        let seen = f.seen.lock().unwrap();
        let call = seen.iter().find(|s| s.0 == "tools/call").unwrap();
        assert_eq!(call.2["mcp-session-id"], "s1");
        assert_eq!(call.2["mcp-protocol-version"], "2025-06-18");
        assert!(seen.iter().any(|s| s.0 == "notifications/initialized"));
    }
    c.shutdown().await;
    // A server on an older revision: the version it answers with is the one sent from then on.
    let (url, f) = http_fake().await;
    *f.version.lock().unwrap() = "2025-03-26".into();
    let c = McpClient::with_options(&http_cfg(&url, false), Arc::new(Recorder::default()), fast());
    c.start().await.unwrap();
    c.call_tool("echo", json!({}), None).await.unwrap();
    assert_eq!(f.seen.lock().unwrap().iter().find(|s| s.0 == "tools/call").unwrap().2["mcp-protocol-version"], "2025-03-26");
    // A revision the client does not know is refused.
    let (url, f) = http_fake().await;
    *f.version.lock().unwrap() = "1999-01-01".into();
    let c2 = McpClient::with_options(&http_cfg(&url, false), Arc::new(Recorder::default()), fast());
    assert!(c2.start().await.is_err());
    assert_eq!(c2.state().status(), "unreachable");
}

#[tokio::test]
async fn http_expired_session_is_reinitialised_and_the_call_retried() {
    let (url, f) = http_fake().await;
    let rec = Arc::new(Recorder::default());
    let c = McpClient::with_options(&http_cfg(&url, false), rec.clone(), fast());
    c.start().await.unwrap();
    assert_eq!(c.call_tool("echo", json!({}), None).await.unwrap().content, "eco da s1");
    f.expired_below.store(2, Ordering::SeqCst); // session s1 is gone
    let out = c.call_tool("echo", json!({}), None).await.unwrap();
    assert_eq!(out.content, "eco da s2");
    assert_eq!(c.state(), McpState::Connected);
    assert!(rec.count(McpChange::Tools) >= 2, "tools listed again");
    c.shutdown().await;
}

#[tokio::test]
async fn http_sse_stream_carries_server_notifications_and_requests() {
    let (url, f) = http_fake().await;
    let run = Arc::new(FakeRun::default());
    let ctx = ctx_with(run.clone());
    let c = McpClient::with_options(&http_cfg(&url, true), Arc::new(Recorder::default()), fast());
    c.start().await.unwrap();
    // The sampling request arrives in the SSE stream of the POST; the answer goes back in another POST.
    let out = c.call_tool("ask", json!({}), Some(&ctx)).await.unwrap();
    assert_eq!(out.content, "risposta a: domanda");
    assert!(f.seen.lock().unwrap().iter().any(|s| s.0.is_empty() && s.1["id"] == 900), "response posted back");
    // With sampling off the server gets an error instead.
    let c2 = McpClient::with_options(&http_cfg(&url, false), Arc::new(Recorder::default()), fast());
    c2.start().await.unwrap();
    let out = c2.call_tool("ask", json!({}), Some(&ctx)).await.unwrap();
    assert!(out.content.contains("abilitato"), "{}", out.content);
    c.shutdown().await;
    c2.shutdown().await;
}

#[tokio::test]
async fn http_get_stream_delivers_list_changed() {
    let (url, f) = http_fake().await;
    let (tx, rx) = mpsc::unbounded_channel();
    *f.get_rx.lock().unwrap() = Some(rx);
    let rec = Arc::new(Recorder::default());
    let c = McpClient::with_options(&http_cfg(&url, false), rec.clone(), fast());
    c.start().await.unwrap();
    let before = rec.count(McpChange::Tools);
    tx.send("data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/tools/list_changed\"}\n\n".into()).unwrap();
    wait_for("refresh from the GET stream", || rec.count(McpChange::Tools) > before).await;
    tx.send("data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/prompts/list_changed\"}\n\n".into()).unwrap();
    wait_for("prompts changed", || rec.count(McpChange::Prompts) == 1).await;
    c.shutdown().await;
}

#[tokio::test]
async fn http_401_needs_sign_in_and_the_token_is_sent() {
    let (url, f) = http_fake().await;
    *f.require_token.lock().unwrap() = Some("tok-1".into());
    let rec = Arc::new(Recorder::default());
    let c = McpClient::with_options(&http_cfg(&url, false), rec.clone(), fast());
    let err = c.start().await.unwrap_err().to_string();
    assert!(err.contains("Accedi"), "{err}");
    assert_eq!(c.state().status(), "auth_required");
    assert!(c.auth_challenge().unwrap().contains("resource_metadata"));
    // Signed in: the token goes out as a bearer.
    *rec.token.lock().unwrap() = Some("tok-1".into());
    c.reconnect().await.unwrap();
    assert_eq!(c.state(), McpState::Connected);
    assert!(f.seen.lock().unwrap().last().unwrap().2["authorization"].to_str().unwrap().ends_with("tok-1"));
    // The token is refused later: one refresh is asked for, then the state is "serve l'accesso".
    *f.require_token.lock().unwrap() = Some("tok-2".into());
    assert!(c.call_tool("echo", json!({}), None).await.is_err());
    assert!(rec.refreshes.load(Ordering::SeqCst) >= 1);
    assert_eq!(c.state().status(), "auth_required");
    c.shutdown().await;
}

#[tokio::test]
async fn http_redirects_are_never_followed() {
    // A server that redirects to another origin must not receive the token.
    let hit = Arc::new(AtomicU32::new(0));
    let h2 = hit.clone();
    let other = Router::new().fallback(move || {
        let h = h2.clone();
        async move {
            h.fetch_add(1, Ordering::SeqCst);
            StatusCode::OK
        }
    });
    let l2 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other_url = format!("http://{}/steal", l2.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l2, other).await.unwrap() });
    let app = Router::new().fallback(move || {
        let to = other_url.clone();
        async move { (StatusCode::TEMPORARY_REDIRECT, [("location", to)]) }
    });
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let rec = Arc::new(Recorder::default());
    *rec.token.lock().unwrap() = Some("secret-token".into());
    let c = McpClient::with_options(&http_cfg(&url, false), rec, fast());
    assert!(c.start().await.is_err());
    assert_eq!(hit.load(Ordering::SeqCst), 0);
    c.shutdown().await;
}

#[tokio::test]
async fn a_cancelled_run_tells_the_server_and_a_slow_call_times_out() {
    let (url, f) = http_fake().await;
    let mut o = fast();
    o.call_timeout = Duration::from_millis(300);
    let c = McpClient::with_options(&http_cfg(&url, true), Arc::new(Recorder::default()), o);
    c.start().await.unwrap();
    // `hang` never answers: the call times out.
    let r = c.call_tool("hang", json!({}), None).await;
    assert!(r.unwrap_err().to_string().contains("tempo scaduto"));
    wait_for("cancelled notification", || f.seen.lock().unwrap().iter().any(|s| s.0 == "notifications/cancelled")).await;
    // Cancelling the run does the same at once.
    let ctx = ToolContext::new("r", std::env::temp_dir());
    let cancel = ctx.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
    });
    let c2 = McpClient::with_options(&http_cfg(&url, true), Arc::new(Recorder::default()), fast());
    c2.start().await.unwrap();
    assert!(matches!(c2.call_tool("hang", json!({}), Some(&ctx)).await, Err(Error::Cancelled)));
    assert_eq!(f.seen.lock().unwrap().iter().filter(|s| s.0 == "notifications/cancelled").count(), 2);
    c.shutdown().await;
    c2.shutdown().await;
}

#[test]
fn resource_formatting_is_capped() {
    let many: Vec<Value> = (0..500).map(|i| json!({"uri": format!("u{i}"), "name": "n"})).collect();
    let l = format_resource_list("s", &many, &[]);
    assert!(l.contains("u199") && !l.contains("u200\n") && l.contains("altre 300"));
    let _ = text(&Ok(ToolOutput::default()));
}
