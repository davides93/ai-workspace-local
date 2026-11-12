//! MCP client (pattern P12): stdio (desktop) and streamable HTTP (all platforms).
//!
//! * Lifecycle: `initialize` with capability negotiation (protocol revisions 2025-06-18, 2025-03-26
//!   and 2024-11-05), `notifications/initialized`, `ping`, request timeouts and
//!   `notifications/cancelled` when a run is cancelled.
//! * Both transports deliver server → client requests and notifications: `ping`, `roots/list`,
//!   `sampling/createMessage` and `elicitation/create` (answered through [`RunMcp`], the hooks of
//!   the run that is waiting for the tool call), `notifications/*/list_changed`.
//!   Streamable HTTP reads the SSE stream of each POST response and the optional GET stream,
//!   keeps `Mcp-Session-Id` and sends `MCP-Protocol-Version`.
//! * Reconnection: a stdio server that exits or an HTTP session that expires (404 on the session
//!   id) is started again with exponential backoff; after the maximum number of attempts the
//!   server is "unreachable" until the person asks for a new attempt.
//! * Tools (`mcp__<server>__<tool>`), plus two tools per server that advertises resources
//!   (`…list_resources`, `…read_resource`). Prompts are read through [`McpClient::list_prompts`]
//!   and [`McpClient::get_prompt`] (the UI puts the text in the composer).
//! * OAuth: the access token comes from [`McpEvents::access_token`] and is sent only to the URL
//!   of the server itself (redirects are never followed); a 401 puts the server in the
//!   `AuthRequired` state with the `WWW-Authenticate` challenge (see `mcp_oauth`).
//!
//! Everything a server sends is untrusted data.

use crate::openai_safe_name;
use crate::{Risk, Tool, ToolContext, ToolOutput};
use aiw_core::api::{McpCapabilities, McpServerConfig};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// Text of a resource returned to the model, in characters.
pub const RESOURCE_TEXT_CAP: usize = 100_000;
/// Largest image of a resource handed to a vision model (size of the base64 text).
const RESOURCE_IMAGE_CAP: usize = 4_000_000;
/// Entries of a resource list shown to the model.
const RESOURCE_LIST_CAP: usize = 200;

#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum McpState {
    Connecting,
    Connected,
    /// `attempt` 0 = the connection was just lost.
    Reconnecting {
        attempt: u32,
        last_error: String,
    },
    Unreachable(String),
    AuthRequired(String),
}

impl McpState {
    pub fn status(&self) -> &'static str {
        match self {
            McpState::Connecting => "connecting",
            McpState::Connected => "connected",
            McpState::Reconnecting { .. } => "reconnecting",
            McpState::Unreachable(_) => "unreachable",
            McpState::AuthRequired(_) => "auth_required",
        }
    }
    pub fn error(&self) -> Option<String> {
        match self {
            McpState::Reconnecting { last_error, .. } if !last_error.is_empty() => Some(last_error.clone()),
            McpState::Unreachable(e) | McpState::AuthRequired(e) => Some(e.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpChange {
    State,
    Tools,
    Resources,
    Prompts,
}

/// What the owner of the client (the engine) provides.
#[async_trait]
pub trait McpEvents: Send + Sync {
    /// The OAuth access token for this server, when the person signed in. `force_refresh`: the
    /// last one was refused, get a new one from the refresh token if there is one.
    async fn access_token(&self, _server_id: &str, _force_refresh: bool) -> Option<String> {
        None
    }
    /// The state, the tools, the resources or the prompts of a server changed.
    fn changed(&self, _server_id: &str, _what: McpChange) {}
}

struct NoEvents;
impl McpEvents for NoEvents {}

/// A sampling request of a server, validated (text only).
#[derive(Debug, Clone)]
pub struct SamplingRequest {
    pub system: Option<String>,
    /// (role, text), role is `user` or `assistant`.
    pub messages: Vec<(String, String)>,
    pub max_tokens: Option<u64>,
    pub temperature: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct SamplingReply {
    pub text: String,
    pub model: String,
}

/// The answer of the person to an elicitation.
#[derive(Debug, Clone, PartialEq)]
pub enum Elicited {
    Accept(Map<String, Value>),
    Decline,
    Cancel,
}

/// What the run that is waiting for a tool call lets its MCP server do (see the engine).
#[async_trait]
pub trait RunMcp: Send + Sync {
    /// Answers a sampling request with the profile of the run (caps and budget apply).
    async fn sample(&self, server: &str, req: SamplingRequest) -> Result<SamplingReply>;
    /// Asks the person to fill a form; `Decline` when nobody can answer.
    async fn elicit(&self, server: &str, message: &str, schema: &Value) -> Elicited;
}

#[derive(Debug, Clone)]
pub struct McpOptions {
    pub request_timeout: Duration,
    pub call_timeout: Duration,
    pub backoff_base: Duration,
    pub backoff_cap: Duration,
    pub max_attempts: u32,
}

impl Default for McpOptions {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            call_timeout: Duration::from_secs(300),
            backoff_base: Duration::from_secs(1),
            backoff_cap: Duration::from_secs(30),
            max_attempts: 6,
        }
    }
}

impl McpOptions {
    /// Delay before reconnection attempt `attempt` (1-based): base · 2^(attempt-1), capped.
    pub fn delay(&self, attempt: u32) -> Duration {
        let factor = 1u32.checked_shl(attempt.saturating_sub(1).min(20)).unwrap_or(u32::MAX);
        self.backoff_base.saturating_mul(factor).min(self.backoff_cap)
    }
}

/// Internal failure of a request.
#[derive(Debug)]
enum Fail {
    /// HTTP 404 on a session id.
    Expired,
    /// HTTP 401, with the `WWW-Authenticate` challenge.
    Auth(String),
    /// The connection is gone.
    Gone,
    Timeout,
    Cancelled,
    Rpc(i64, String),
    /// Will not get better by retrying (command not found, bad configuration).
    Fatal(String),
    Other(String),
}

impl Fail {
    fn into_error(self, server: &str, method: &str) -> Error {
        self.to_error(server, method)
    }
    fn to_error(&self, server: &str, method: &str) -> Error {
        match self {
            Fail::Expired => Error::Runtime(format!("sessione MCP di {server} scaduta")),
            Fail::Auth(_) => Error::Denied(format!("il server MCP {server} richiede l'accesso: premi «Accedi» in Integrazioni")),
            Fail::Gone => Error::Runtime(format!("il server MCP {server} si è disconnesso")),
            Fail::Timeout => Error::Runtime(format!("MCP {server} {method}: tempo scaduto")),
            Fail::Cancelled => Error::Cancelled,
            Fail::Rpc(_, m) => Error::Runtime(format!("MCP {server} {method}: {m}")),
            Fail::Fatal(m) | Fail::Other(m) => Error::Runtime(m.clone()),
        }
    }
    fn retryable(&self) -> bool {
        !matches!(self, Fail::Fatal(_) | Fail::Auth(_))
    }
    fn text(&self) -> String {
        match self {
            Fail::Expired => "sessione scaduta".into(),
            Fail::Auth(_) => "serve l'accesso".into(),
            Fail::Gone => "il server si è disconnesso".into(),
            Fail::Timeout => "tempo scaduto".into(),
            Fail::Cancelled => "annullato".into(),
            Fail::Rpc(c, m) => format!("{m} (codice {c})"),
            Fail::Fatal(m) | Fail::Other(m) => m.clone(),
        }
    }
}

fn rpc(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

// ------------------------------------------------------------------ SSE

/// Incremental parser of a `text/event-stream`: feed it the chunks, it yields the JSON payloads of
/// the complete events (`data:` lines, joined). Events that are not JSON are dropped.
#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Value> {
        self.buf.extend(chunk.iter().filter(|b| **b != b'\r'));
        let mut out = Vec::new();
        while let Some(pos) = self.buf.windows(2).position(|w| w == b"\n\n") {
            let event: Vec<u8> = self.buf.drain(..pos + 2).collect();
            let text = String::from_utf8_lossy(&event);
            let data: Vec<&str> = text.lines().filter_map(|l| l.strip_prefix("data:")).map(|d| d.strip_prefix(' ').unwrap_or(d)).collect();
            if data.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(&data.join("\n")) {
                out.push(v);
            }
            // A server that never ends an event must not make the buffer grow without limit.
        }
        if self.buf.len() > 8_000_000 {
            self.buf.clear();
        }
        out
    }
}

/// The JSON-RPC message with `id` in an SSE body.
#[cfg(test)]
fn from_sse(body: &str, id: u64) -> Option<Value> {
    SseParser::default().push(format!("{body}\n\n").as_bytes()).into_iter().find(|v| v.get("id").and_then(|i| i.as_u64()) == Some(id))
}

// ------------------------------------------------------------------ connection

#[allow(clippy::large_enum_variant)]
enum Kind {
    Stdio {
        tx: mpsc::UnboundedSender<String>,
        child: tokio::sync::Mutex<Option<tokio::process::Child>>,
        stdout: Mutex<Option<tokio::process::ChildStdout>>,
    },
    Http {
        url: String,
        http: reqwest::Client,
        session: Mutex<Option<String>>,
    },
}

/// One live connection (one process, or one HTTP session). A reconnection makes a new one.
struct Conn {
    gen: u64,
    kind: Kind,
    pending: Mutex<HashMap<u64, oneshot::Sender<Value>>>,
    /// Cancelled when the connection ends or is replaced.
    closed: CancellationToken,
    client: Weak<McpClient>,
    next: AtomicU64,
    version: Mutex<String>,
}

impl Conn {
    async fn open(client: &Arc<McpClient>, gen: u64) -> std::result::Result<Arc<Conn>, Fail> {
        let cfg = &client.cfg;
        let kind = match cfg.transport.as_str() {
            "stdio" => Self::spawn_stdio(client, gen)?,
            "http" => {
                let mut headers = reqwest::header::HeaderMap::new();
                for (k, v) in cfg.headers.iter().flatten() {
                    let (Ok(name), Some(Ok(value))) =
                        (reqwest::header::HeaderName::from_bytes(k.as_bytes()), v.as_str().map(reqwest::header::HeaderValue::from_str))
                    else {
                        return Err(Fail::Fatal(format!("header MCP non valido: {k}")));
                    };
                    headers.insert(name, value);
                }
                let url = cfg.url.clone().filter(|u| !u.is_empty()).ok_or_else(|| Fail::Fatal("server MCP HTTP senza url".into()))?;
                let http = reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(5))
                    // The token of the person goes to this address and nowhere else.
                    .redirect(reqwest::redirect::Policy::none())
                    .default_headers(headers)
                    .build()
                    .map_err(|e| Fail::Fatal(e.to_string()))?;
                Kind::Http { url, http, session: Mutex::new(None) }
            }
            other => return Err(Fail::Fatal(format!("trasporto MCP sconosciuto: {other}"))),
        };
        let conn = Arc::new(Conn {
            gen,
            kind,
            pending: Default::default(),
            closed: CancellationToken::new(),
            client: Arc::downgrade(client),
            next: AtomicU64::new(1),
            version: Mutex::new(PROTOCOL_VERSION.into()),
        });
        if let Kind::Stdio { .. } = conn.kind {
            conn.start_stdio_reader(client);
        }
        Ok(conn)
    }

    fn spawn_stdio(client: &Arc<McpClient>, _gen: u64) -> std::result::Result<Kind, Fail> {
        let cfg = &client.cfg;
        if cfg!(target_os = "ios") {
            return Err(Fail::Fatal("su iOS i server MCP devono essere HTTP".into()));
        }
        let cmd = cfg.command.clone().filter(|c| !c.is_empty()).ok_or_else(|| Fail::Fatal("server MCP stdio senza comando".into()))?;
        let mut command = tokio::process::Command::new(&cmd);
        command
            .args(cfg.args.clone().unwrap_or_default())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        for (k, v) in cfg.env.clone().unwrap_or_default() {
            command.env(k, v.as_str().map(String::from).unwrap_or_else(|| v.to_string()));
        }
        let mut child = command.spawn().map_err(|e| Fail::Fatal(format!("avvio server MCP `{cmd}`: {e}")))?;
        let mut stdin = child.stdin.take().ok_or_else(|| Fail::Fatal("stdin non disponibile".into()))?;
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            while let Some(line) = rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err() || stdin.write_all(b"\n").await.is_err() || stdin.flush().await.is_err()
                {
                    break;
                }
            }
        });
        let stdout = child.stdout.take();
        Ok(Kind::Stdio { tx, child: tokio::sync::Mutex::new(Some(child)), stdout: Mutex::new(stdout) })
    }

    fn start_stdio_reader(self: &Arc<Self>, client: &Arc<McpClient>) {
        let Kind::Stdio { stdout, .. } = &self.kind else { return };
        let Some(stdout) = stdout.lock().unwrap().take() else { return };
        let conn = self.clone();
        let client = client.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    conn.dispatch(v);
                }
            }
            // The process ended (or closed its output): whoever waits for an answer will not get one.
            conn.closed.cancel();
            conn.pending.lock().unwrap().clear();
            client.on_lost(conn.gen);
        });
    }

    fn version(&self) -> String {
        self.version.lock().unwrap().clone()
    }

    async fn close(&self) {
        self.closed.cancel();
        self.pending.lock().unwrap().clear();
        if let Kind::Stdio { child, .. } = &self.kind {
            if let Some(mut c) = child.lock().await.take() {
                let _ = c.kill().await;
            }
        }
    }

    /// Request with a timeout; `cancel` aborts it and tells the server (`notifications/cancelled`).
    async fn request(
        self: &Arc<Self>,
        method: &str,
        params: Value,
        timeout: Duration,
        cancel: Option<&CancellationToken>,
    ) -> std::result::Result<Value, Fail> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let msg = rpc(id, method, params);
        let outcome = tokio::select! {
            r = tokio::time::timeout(timeout, self.exchange(id, msg)) => r.unwrap_or(Err(Fail::Timeout)),
            _ = async { match cancel { Some(c) => c.cancelled().await, None => std::future::pending().await } } => Err(Fail::Cancelled),
        };
        if matches!(outcome, Err(Fail::Timeout | Fail::Cancelled)) {
            self.pending.lock().unwrap().remove(&id);
            // `initialize` must never be cancelled (spec); anything else is told to stop.
            if method != "initialize" {
                let reason = if matches!(outcome, Err(Fail::Timeout)) { "tempo scaduto" } else { "annullato dall'utente" };
                let _ = tokio::time::timeout(
                    Duration::from_secs(3),
                    self.notify("notifications/cancelled", json!({"requestId": id, "reason": reason})),
                )
                .await;
            }
        }
        let v = outcome?;
        if let Some(e) = v.get("error") {
            return Err(Fail::Rpc(e["code"].as_i64().unwrap_or(-32000), e["message"].as_str().unwrap_or("errore").to_string()));
        }
        Ok(v["result"].clone())
    }

    async fn exchange(self: &Arc<Self>, id: u64, msg: Value) -> std::result::Result<Value, Fail> {
        match &self.kind {
            Kind::Stdio { tx, .. } => {
                let (s, r) = oneshot::channel();
                self.pending.lock().unwrap().insert(id, s);
                tx.send(msg.to_string()).map_err(|_| Fail::Gone)?;
                if self.closed.is_cancelled() {
                    return Err(Fail::Gone);
                }
                r.await.map_err(|_| Fail::Gone)
            }
            Kind::Http { .. } => self.http_post(&msg, Some(id)).await.and_then(|v| v.ok_or(Fail::Other("risposta MCP vuota".into()))),
        }
    }

    async fn notify(self: &Arc<Self>, method: &str, params: Value) -> std::result::Result<(), Fail> {
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.send(&msg).await
    }

    /// A message that expects no answer (notification, or the response to a server request).
    async fn send(self: &Arc<Self>, msg: &Value) -> std::result::Result<(), Fail> {
        match &self.kind {
            Kind::Stdio { tx, .. } => tx.send(msg.to_string()).map_err(|_| Fail::Gone),
            Kind::Http { .. } => self.http_post(msg, None).await.map(|_| ()),
        }
    }

    /// POST of one message. With `want`, reads the JSON or SSE answer until the response with that id
    /// (anything the server sends before it is dispatched), and answers 401 with one refresh of the token.
    async fn http_post(self: &Arc<Self>, msg: &Value, want: Option<u64>) -> std::result::Result<Option<Value>, Fail> {
        let Kind::Http { url, http, session } = &self.kind else { return Err(Fail::Gone) };
        let client = self.client.upgrade().ok_or(Fail::Gone)?;
        let mut force = false;
        loop {
            let token = client.events.access_token(&client.id, force).await;
            let mut rb = http
                .post(url)
                .header("Accept", "application/json, text/event-stream")
                .header("MCP-Protocol-Version", self.version())
                .json(msg);
            let sent_session = session.lock().unwrap().clone();
            if let Some(s) = &sent_session {
                rb = rb.header("Mcp-Session-Id", s);
            }
            if let Some(t) = &token {
                rb = rb.bearer_auth(t);
            }
            let mut res = rb.send().await.map_err(|e| Fail::Other(format!("MCP {}: {e}", client.name)))?;
            let status = res.status();
            if status == reqwest::StatusCode::UNAUTHORIZED {
                let challenge = res.headers().get("www-authenticate").and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
                if token.is_some() && !force {
                    force = true;
                    continue;
                }
                return Err(Fail::Auth(challenge));
            }
            if status == reqwest::StatusCode::NOT_FOUND && sent_session.is_some() {
                return Err(Fail::Expired);
            }
            if let Some(s) = res.headers().get("mcp-session-id").and_then(|h| h.to_str().ok()) {
                *session.lock().unwrap() = Some(s.to_string());
            }
            if !status.is_success() {
                let body = res.text().await.unwrap_or_default();
                return Err(Fail::Other(format!("MCP {} → {status}: {}", client.name, body.chars().take(300).collect::<String>())));
            }
            let Some(want) = want else { return Ok(None) };
            let ctype = res.headers().get(reqwest::header::CONTENT_TYPE).and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
            let is_answer = |v: &Value| v.get("id").and_then(|i| i.as_u64()) == Some(want) && v.get("method").is_none();
            if ctype.contains("text/event-stream") {
                let mut parser = SseParser::default();
                while let Some(chunk) = res.chunk().await.map_err(|e| Fail::Other(format!("MCP {}: {e}", client.name)))? {
                    for m in parser.push(&chunk) {
                        if is_answer(&m) {
                            return Ok(Some(m));
                        }
                        self.dispatch(m);
                    }
                }
                return Err(Fail::Other(format!("MCP {}: risposta SSE senza id {want}", client.name)));
            }
            let body = res.text().await.map_err(|e| Fail::Other(e.to_string()))?;
            let parsed: Value =
                serde_json::from_str(&body).map_err(|e| Fail::Other(format!("MCP {}: risposta non valida: {e}", client.name)))?;
            // 2025-03-26 servers may answer with a batch.
            let items = match parsed {
                Value::Array(a) => a,
                one => vec![one],
            };
            let mut found = None;
            for m in items {
                if is_answer(&m) {
                    found = Some(m);
                } else {
                    self.dispatch(m);
                }
            }
            return found.map(Some).ok_or_else(|| Fail::Other(format!("MCP {}: risposta senza id {want}", client.name)));
        }
    }

    /// The optional GET stream of streamable HTTP: server notifications and requests outside any POST.
    fn start_get_stream(self: &Arc<Self>) {
        let conn = self.clone();
        tokio::spawn(async move {
            let Kind::Http { url, http, session } = &conn.kind else { return };
            let Some(client) = conn.client.upgrade() else { return };
            let mut failures = 0;
            while !conn.closed.is_cancelled() && failures < 5 {
                let token = client.events.access_token(&client.id, false).await;
                let mut rb = http.get(url).header("Accept", "text/event-stream").header("MCP-Protocol-Version", conn.version());
                if let Some(s) = session.lock().unwrap().clone() {
                    rb = rb.header("Mcp-Session-Id", s);
                }
                if let Some(t) = token {
                    rb = rb.bearer_auth(t);
                }
                let sent = tokio::select! { r = rb.send() => r, _ = conn.closed.cancelled() => return };
                let mut res = match sent {
                    Ok(r) if r.status().is_success() => r,
                    // 405: the server has no stream for us; anything else will not change by asking again.
                    Ok(_) => return,
                    Err(_) => {
                        failures += 1;
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };
                let mut parser = SseParser::default();
                loop {
                    let chunk = tokio::select! { c = res.chunk() => c, _ = conn.closed.cancelled() => return };
                    match chunk {
                        Ok(Some(c)) => {
                            for m in parser.push(&c) {
                                conn.dispatch(m);
                            }
                        }
                        _ => break,
                    }
                }
                failures += 1;
                tokio::select! { _ = tokio::time::sleep(Duration::from_secs(2)) => {}, _ = conn.closed.cancelled() => return };
            }
        });
    }

    /// One message from the server.
    fn dispatch(self: &Arc<Self>, msg: Value) {
        let method = msg.get("method").and_then(|m| m.as_str()).map(String::from);
        match (method, msg.get("id").cloned()) {
            (Some(method), Some(id)) => {
                let conn = self.clone();
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                tokio::spawn(async move {
                    let result = match conn.client.upgrade() {
                        Some(client) => client.handle_server_request(&method, params).await,
                        None => Err((-32603, "client MCP chiuso".to_string())),
                    };
                    let resp = match result {
                        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
                        Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
                    };
                    let _ = conn.send(&resp).await;
                });
            }
            (Some(method), None) => {
                if let Some(client) = self.client.upgrade() {
                    client.handle_notification(&method);
                }
            }
            (None, Some(id)) => {
                if let Some(n) = id.as_u64() {
                    if let Some(s) = self.pending.lock().unwrap().remove(&n) {
                        let _ = s.send(msg);
                    }
                }
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ client

struct ActiveCall {
    token: u64,
    run: Arc<dyn RunMcp>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct McpPromptDef {
    pub name: String,
    pub description: String,
    pub arguments: Vec<(String, String, bool)>,
}

pub struct McpClient {
    pub name: String,
    pub id: String,
    cfg: McpServerConfig,
    events: Arc<dyn McpEvents>,
    opts: McpOptions,
    conn: Mutex<Option<Arc<Conn>>>,
    state: Mutex<McpState>,
    tools: Mutex<Vec<McpToolDef>>,
    server_caps: Mutex<Value>,
    server_info: Mutex<Value>,
    challenge: Mutex<Option<String>>,
    reconnecting: AtomicBool,
    shutting: AtomicBool,
    gen: AtomicU64,
    reinit: tokio::sync::Mutex<()>,
    refreshing: AtomicBool,
    refresh_again: AtomicBool,
    active: Mutex<Vec<ActiveCall>>,
    call_seq: AtomicU64,
    me: Weak<McpClient>,
}

impl McpClient {
    pub fn new(cfg: &McpServerConfig, events: Arc<dyn McpEvents>) -> Arc<Self> {
        Self::with_options(cfg, events, McpOptions::default())
    }

    pub fn with_options(cfg: &McpServerConfig, events: Arc<dyn McpEvents>, opts: McpOptions) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            name: cfg.name.clone(),
            id: cfg.id.clone(),
            cfg: cfg.clone(),
            events,
            opts,
            conn: Mutex::new(None),
            state: Mutex::new(McpState::Connecting),
            tools: Mutex::new(vec![]),
            server_caps: Mutex::new(Value::Null),
            server_info: Mutex::new(Value::Null),
            challenge: Mutex::new(None),
            reconnecting: AtomicBool::new(false),
            shutting: AtomicBool::new(false),
            gen: AtomicU64::new(0),
            reinit: tokio::sync::Mutex::new(()),
            refreshing: AtomicBool::new(false),
            refresh_again: AtomicBool::new(false),
            active: Mutex::new(vec![]),
            call_seq: AtomicU64::new(1),
            me: me.clone(),
        })
    }

    /// Connects and connects with no events (tests, one-shot uses). The first attempt is made here;
    /// when it fails for a reason that may pass, the client keeps trying in the background.
    pub async fn connect(cfg: &McpServerConfig) -> Result<Arc<Self>> {
        let c = Self::new(cfg, Arc::new(NoEvents));
        c.start().await?;
        Ok(c)
    }

    // ---------------------------------------------------------- state

    pub fn state(&self) -> McpState {
        self.state.lock().unwrap().clone()
    }
    pub fn tools(&self) -> Vec<McpToolDef> {
        self.tools.lock().unwrap().clone()
    }
    pub fn server_info(&self) -> Value {
        self.server_info.lock().unwrap().clone()
    }
    /// The `WWW-Authenticate` header of the last 401 (OAuth discovery starts from it).
    pub fn auth_challenge(&self) -> Option<String> {
        self.challenge.lock().unwrap().clone()
    }
    pub fn config(&self) -> &McpServerConfig {
        &self.cfg
    }
    pub fn capabilities(&self) -> McpCapabilities {
        let caps = self.server_caps.lock().unwrap();
        McpCapabilities {
            tools: caps.get("tools").is_some() || !self.tools.lock().unwrap().is_empty(),
            resources: caps.get("resources").is_some(),
            prompts: caps.get("prompts").is_some(),
        }
    }

    fn set_state(&self, s: McpState) {
        let changed = {
            let mut cur = self.state.lock().unwrap();
            let changed = *cur != s;
            *cur = s;
            changed
        };
        if changed {
            self.events.changed(&self.id, McpChange::State);
        }
    }

    fn current(&self) -> Option<Arc<Conn>> {
        self.conn.lock().unwrap().clone()
    }

    // ---------------------------------------------------------- lifecycle

    /// First connection. An error that may pass (server not up yet) leaves the client trying again with
    /// backoff; one that will not (command not found, sign-in needed) is final.
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        self.set_state(McpState::Connecting);
        match self.connect_once().await {
            Ok(()) => Ok(()),
            Err(f) => Err(self.after_failed_attempt(f, true)),
        }
    }

    /// Manual reconnection («Riconnetti»): one attempt now, and if it fails for a reason that may pass,
    /// the backoff loop starts again from the beginning.
    pub async fn reconnect(self: &Arc<Self>) -> Result<()> {
        self.shutting.store(false, Ordering::SeqCst);
        let taken = self.conn.lock().unwrap().take();
        if let Some(old) = taken {
            old.close().await;
        }
        self.set_state(McpState::Connecting);
        match self.connect_once().await {
            Ok(()) => Ok(()),
            Err(f) => Err(self.after_failed_attempt(f, true)),
        }
    }

    fn after_failed_attempt(self: &Arc<Self>, f: Fail, spawn_loop: bool) -> Error {
        let err = f.to_error(&self.name, "initialize");
        match &f {
            Fail::Auth(h) => {
                *self.challenge.lock().unwrap() = Some(h.clone());
                self.set_state(McpState::AuthRequired(err.to_string()));
            }
            f if !f.retryable() => self.set_state(McpState::Unreachable(f.text())),
            f => {
                self.set_state(McpState::Reconnecting { attempt: 0, last_error: f.text() });
                if spawn_loop {
                    let me = self.clone();
                    tokio::spawn(async move { me.reconnect_loop().await });
                }
            }
        }
        err
    }

    async fn connect_once(self: &Arc<Self>) -> std::result::Result<(), Fail> {
        let gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let conn = Conn::open(self, gen).await?;
        let mut caps = json!({"roots": {"listChanged": false}, "elicitation": {}});
        if self.cfg.sampling {
            caps["sampling"] = json!({});
        }
        let init = conn
            .request(
                "initialize",
                json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": caps, "clientInfo": {"name": "ai-workspace-local", "version": aiw_core::VERSION}}),
                self.opts.request_timeout,
                None,
            )
            .await;
        let init = match init {
            Ok(v) => v,
            Err(f) => {
                conn.close().await;
                return Err(f);
            }
        };
        let version = init["protocolVersion"].as_str().unwrap_or(PROTOCOL_VERSION).to_string();
        if !SUPPORTED_VERSIONS.contains(&version.as_str()) {
            conn.close().await;
            return Err(Fail::Fatal(format!("il server MCP {} usa la versione di protocollo {version}, che non è supportata", self.name)));
        }
        *conn.version.lock().unwrap() = version;
        *self.server_info.lock().unwrap() = init.get("serverInfo").cloned().unwrap_or(Value::Null);
        *self.server_caps.lock().unwrap() = init.get("capabilities").cloned().unwrap_or(Value::Null);
        if let Err(f) = conn.notify("notifications/initialized", json!({})).await {
            conn.close().await;
            return Err(f);
        }
        let tools = match self.list_tools_on(&conn).await {
            Ok(t) => t,
            // A server that does not advertise tools may still refuse the question.
            Err(Fail::Rpc(..)) if self.server_caps.lock().unwrap().get("tools").is_none() => vec![],
            Err(f) => {
                conn.close().await;
                return Err(f);
            }
        };
        *self.tools.lock().unwrap() = tools;
        *self.challenge.lock().unwrap() = None;
        let old = self.conn.lock().unwrap().replace(conn.clone());
        if let Some(old) = old {
            old.close().await;
        }
        if matches!(conn.kind, Kind::Http { .. }) {
            conn.start_get_stream();
        }
        self.set_state(McpState::Connected);
        self.events.changed(&self.id, McpChange::Tools);
        Ok(())
    }

    /// The connection `gen` ended (stdio: the process exited). Restart it unless the client was shut down.
    fn on_lost(self: &Arc<Self>, gen: u64) {
        if self.shutting.load(Ordering::SeqCst) || self.gen.load(Ordering::SeqCst) != gen {
            return;
        }
        if !matches!(self.state(), McpState::Connected) {
            return;
        }
        self.set_state(McpState::Reconnecting { attempt: 0, last_error: "il server si è chiuso".into() });
        let me = self.clone();
        tokio::spawn(async move { me.reconnect_loop().await });
    }

    async fn reconnect_loop(self: Arc<Self>) {
        if self.reconnecting.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut last = match self.state() {
            McpState::Reconnecting { last_error, .. } => last_error,
            _ => String::new(),
        };
        for attempt in 1..=self.opts.max_attempts {
            self.set_state(McpState::Reconnecting { attempt, last_error: last.clone() });
            tokio::time::sleep(self.opts.delay(attempt)).await;
            if self.shutting.load(Ordering::SeqCst) {
                self.reconnecting.store(false, Ordering::SeqCst);
                return;
            }
            let taken = self.conn.lock().unwrap().take();
            if let Some(old) = taken {
                old.close().await;
            }
            match self.connect_once().await {
                Ok(()) => {
                    self.reconnecting.store(false, Ordering::SeqCst);
                    return;
                }
                Err(Fail::Auth(h)) => {
                    *self.challenge.lock().unwrap() = Some(h);
                    self.set_state(McpState::AuthRequired("serve l'accesso".into()));
                    self.reconnecting.store(false, Ordering::SeqCst);
                    return;
                }
                Err(f) if !f.retryable() => {
                    last = f.text();
                    break;
                }
                Err(f) => last = f.text(),
            }
        }
        self.set_state(McpState::Unreachable(last));
        self.reconnecting.store(false, Ordering::SeqCst);
    }

    /// An HTTP session that expired: start a new one (once for all the requests that failed on `gen`).
    async fn reinitialise(self: &Arc<Self>, failed_gen: u64) -> std::result::Result<(), Fail> {
        let _guard = self.reinit.lock().await;
        if self.gen.load(Ordering::SeqCst) != failed_gen {
            return Ok(());
        }
        self.set_state(McpState::Reconnecting { attempt: 0, last_error: "sessione scaduta".into() });
        let r = self.connect_once().await;
        if let Err(f) = &r {
            if f.retryable() {
                let me = self.clone();
                tokio::spawn(async move { me.reconnect_loop().await });
            } else if let Fail::Auth(h) = f {
                *self.challenge.lock().unwrap() = Some(h.clone());
                self.set_state(McpState::AuthRequired("serve l'accesso".into()));
            } else {
                self.set_state(McpState::Unreachable(f.text()));
            }
        }
        r
    }

    pub async fn shutdown(&self) {
        self.shutting.store(true, Ordering::SeqCst);
        let taken = self.conn.lock().unwrap().take();
        if let Some(c) = taken {
            c.close().await;
        }
    }

    // ---------------------------------------------------------- requests

    async fn rpc(self: &Arc<Self>, method: &str, params: Value, timeout: Duration, cancel: Option<&CancellationToken>) -> Result<Value> {
        for attempt in 0..2 {
            let Some(conn) = self.current() else {
                return Err(Error::Runtime(format!("il server MCP {} non è raggiungibile ({})", self.name, self.state().status_it())));
            };
            match conn.request(method, params.clone(), timeout, cancel).await {
                Ok(v) => return Ok(v),
                Err(Fail::Expired) if attempt == 0 => {
                    // The request never ran: a new session and one more try.
                    self.reinitialise(conn.gen).await.map_err(|f| f.into_error(&self.name, method))?;
                }
                Err(Fail::Auth(h)) => {
                    *self.challenge.lock().unwrap() = Some(h.clone());
                    self.set_state(McpState::AuthRequired("serve l'accesso".into()));
                    return Err(Fail::Auth(h).into_error(&self.name, method));
                }
                Err(f) => return Err(f.into_error(&self.name, method)),
            }
        }
        Err(Error::Runtime(format!("sessione MCP di {} scaduta", self.name)))
    }

    /// `ping` (health check).
    pub async fn ping(self: &Arc<Self>) -> Result<()> {
        self.rpc("ping", json!({}), self.opts.request_timeout, None).await.map(|_| ())
    }

    async fn list_paged(&self, conn: &Arc<Conn>, method: &str, key: &str) -> std::result::Result<Vec<Value>, Fail> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let r = conn.request(method, params, self.opts.request_timeout, None).await?;
            out.extend(r[key].as_array().cloned().unwrap_or_default());
            cursor = r["nextCursor"].as_str().map(String::from);
            if cursor.is_none() || out.len() > 1000 {
                break;
            }
        }
        Ok(out)
    }

    async fn list_tools_on(&self, conn: &Arc<Conn>) -> std::result::Result<Vec<McpToolDef>, Fail> {
        Ok(self
            .list_paged(conn, "tools/list", "tools")
            .await?
            .into_iter()
            .map(|t| McpToolDef {
                name: t["name"].as_str().unwrap_or_default().to_string(),
                description: t["description"].as_str().unwrap_or_default().to_string(),
                input_schema: t.get("inputSchema").cloned().unwrap_or_else(|| json!({"type":"object","properties":{}})),
                read_only: t["annotations"]["readOnlyHint"].as_bool().unwrap_or(false),
            })
            .filter(|t| !t.name.is_empty())
            .collect())
    }

    /// `notifications/tools/list_changed`: list again (one refresh at a time; changes that arrive
    /// meanwhile cause one more).
    fn spawn_refresh_tools(self: &Arc<Self>) {
        self.refresh_again.store(true, Ordering::SeqCst);
        if self.refreshing.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            while me.refresh_again.swap(false, Ordering::SeqCst) {
                let Some(conn) = me.current() else { break };
                if let Ok(tools) = me.list_tools_on(&conn).await {
                    *me.tools.lock().unwrap() = tools;
                    me.events.changed(&me.id, McpChange::Tools);
                }
            }
            me.refreshing.store(false, Ordering::SeqCst);
        });
    }

    fn handle_notification(self: &Arc<Self>, method: &str) {
        match method {
            "notifications/tools/list_changed" => self.spawn_refresh_tools(),
            "notifications/resources/list_changed" => self.events.changed(&self.id, McpChange::Resources),
            "notifications/prompts/list_changed" => self.events.changed(&self.id, McpChange::Prompts),
            _ => {}
        }
    }

    pub async fn call_tool(self: &Arc<Self>, name: &str, args: Value, ctx: Option<&ToolContext>) -> Result<ToolOutput> {
        let _active = ctx.and_then(|c| c.mcp_run.clone()).map(|run| self.begin_call(run));
        let r = self.rpc("tools/call", json!({"name": name, "arguments": args}), self.opts.call_timeout, ctx.map(|c| &c.cancel)).await?;
        let mut out = ToolOutput { is_error: r["isError"].as_bool().unwrap_or(false), ..Default::default() };
        let mut texts = Vec::new();
        for c in r["content"].as_array().cloned().unwrap_or_default() {
            match c["type"].as_str() {
                Some("text") => texts.push(c["text"].as_str().unwrap_or_default().to_string()),
                Some("image") => out.images.push(format!(
                    "data:{};base64,{}",
                    c["mimeType"].as_str().unwrap_or("image/png"),
                    c["data"].as_str().unwrap_or_default()
                )),
                Some("resource") => {
                    texts.push(c["resource"]["text"].as_str().map(String::from).unwrap_or_else(|| c["resource"]["uri"].to_string()))
                }
                _ => texts.push(c.to_string()),
            }
        }
        if let Some(sc) = r.get("structuredContent") {
            if texts.is_empty() {
                texts.push(sc.to_string());
            }
        }
        out.content = texts.join("\n");
        Ok(out)
    }

    fn begin_call(&self, run: Arc<dyn RunMcp>) -> ActiveGuard {
        let token = self.call_seq.fetch_add(1, Ordering::SeqCst);
        self.active.lock().unwrap().push(ActiveCall { token, run });
        ActiveGuard { client: self.me.clone(), token }
    }

    // ---------------------------------------------------------- resources and prompts

    /// `resources/list` and `resources/templates/list` (the second may be missing: it is then empty).
    pub async fn list_resources(self: &Arc<Self>) -> Result<(Vec<Value>, Vec<Value>)> {
        let conn = self.current().ok_or_else(|| Error::Runtime(format!("il server MCP {} non è raggiungibile", self.name)))?;
        let resources =
            self.list_paged(&conn, "resources/list", "resources").await.map_err(|f| f.into_error(&self.name, "resources/list"))?;
        let templates = self.list_paged(&conn, "resources/templates/list", "resourceTemplates").await.unwrap_or_default();
        Ok((resources, templates))
    }

    pub async fn read_resource(self: &Arc<Self>, uri: &str) -> Result<Value> {
        self.rpc("resources/read", json!({"uri": uri}), self.opts.request_timeout, None).await
    }

    pub async fn list_prompts(self: &Arc<Self>) -> Result<Vec<McpPromptDef>> {
        let conn = self.current().ok_or_else(|| Error::Runtime(format!("il server MCP {} non è raggiungibile", self.name)))?;
        let items = self.list_paged(&conn, "prompts/list", "prompts").await.map_err(|f| f.into_error(&self.name, "prompts/list"))?;
        Ok(items
            .into_iter()
            .map(|p| McpPromptDef {
                name: p["name"].as_str().unwrap_or_default().to_string(),
                description: p["description"].as_str().unwrap_or_default().to_string(),
                arguments: p["arguments"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|x| {
                                (
                                    x["name"].as_str().unwrap_or_default().to_string(),
                                    x["description"].as_str().unwrap_or_default().to_string(),
                                    x["required"].as_bool().unwrap_or(false),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .filter(|p| !p.name.is_empty())
            .collect())
    }

    /// `prompts/get`: the description and the messages as one text (`role: text`, images are named, not shown).
    pub async fn get_prompt(self: &Arc<Self>, name: &str, arguments: &Map<String, Value>) -> Result<(String, String)> {
        let args: Map<String, Value> = arguments
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.as_str().map(String::from).unwrap_or_else(|| v.to_string()))))
            .collect();
        let r = self.rpc("prompts/get", json!({"name": name, "arguments": args}), self.opts.request_timeout, None).await?;
        let messages = r["messages"].as_array().cloned().unwrap_or_default();
        let mut parts = Vec::new();
        for m in &messages {
            let text = match m["content"]["type"].as_str() {
                Some("text") => m["content"]["text"].as_str().unwrap_or_default().to_string(),
                Some("resource") => m["content"]["resource"]["text"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| format!("[risorsa {}]", m["content"]["resource"]["uri"].as_str().unwrap_or(""))),
                Some(other) => format!("[contenuto {other} non mostrato]"),
                None => m["content"].as_str().unwrap_or_default().to_string(),
            };
            parts.push(if messages.len() > 1 { format!("{}: {text}", m["role"].as_str().unwrap_or("user")) } else { text });
        }
        Ok((r["description"].as_str().unwrap_or_default().to_string(), parts.join("\n\n")))
    }

    // ---------------------------------------------------------- server → client requests

    async fn handle_server_request(self: &Arc<Self>, method: &str, params: Value) -> std::result::Result<Value, (i64, String)> {
        match method {
            "ping" => Ok(json!({})),
            "roots/list" => Ok(json!({"roots": []})),
            "sampling/createMessage" => self.handle_sampling(params).await,
            "elicitation/create" => self.handle_elicitation(params).await,
            _ => Err((-32601, "metodo non supportato da AI Workspace Local".into())),
        }
    }

    /// The run waiting for the tool call that the server is serving (the latest, if several).
    fn active_run(&self) -> Option<Arc<dyn RunMcp>> {
        self.active.lock().unwrap().last().map(|a| a.run.clone())
    }

    async fn handle_sampling(&self, params: Value) -> std::result::Result<Value, (i64, String)> {
        if !self.cfg.sampling {
            return Err((-32601, "il campionamento non è abilitato per questo server MCP".into()));
        }
        let Some(run) = self.active_run() else {
            return Err((-32000, "nessuna esecuzione in corso: richiesta di campionamento rifiutata".into()));
        };
        let req = parse_sampling(&params).map_err(|m| (-32602, m))?;
        match run.sample(&self.name, req).await {
            Ok(r) => {
                Ok(json!({"role": "assistant", "content": {"type": "text", "text": r.text}, "model": r.model, "stopReason": "endTurn"}))
            }
            Err(e) => Err((-32000, e.to_string())),
        }
    }

    async fn handle_elicitation(&self, params: Value) -> std::result::Result<Value, (i64, String)> {
        let message: String = params["message"].as_str().unwrap_or_default().chars().take(2000).collect();
        let schema = params.get("requestedSchema").cloned().unwrap_or(Value::Null);
        validate_elicit_schema(&schema).map_err(|m| (-32602, m))?;
        let Some(run) = self.active_run() else { return Ok(json!({"action": "decline"})) };
        Ok(match run.elicit(&self.name, &message, &schema).await {
            Elicited::Accept(content) => match validate_elicit_content(&schema, &content) {
                Ok(c) => json!({"action": "accept", "content": c}),
                Err(_) => json!({"action": "decline"}),
            },
            Elicited::Decline => json!({"action": "decline"}),
            Elicited::Cancel => json!({"action": "cancel"}),
        })
    }
}

impl McpState {
    fn status_it(&self) -> &'static str {
        match self {
            McpState::Connecting => "connessione in corso",
            McpState::Connected => "connesso",
            McpState::Reconnecting { .. } => "riconnessione in corso",
            McpState::Unreachable(_) => "non raggiungibile",
            McpState::AuthRequired(_) => "serve l'accesso",
        }
    }
}

struct ActiveGuard {
    client: Weak<McpClient>,
    token: u64,
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        if let Some(c) = self.client.upgrade() {
            c.active.lock().unwrap().retain(|a| a.token != self.token);
        }
    }
}

// ------------------------------------------------------------------ sampling and elicitation parameters

fn parse_sampling(params: &Value) -> std::result::Result<SamplingRequest, String> {
    let messages = params["messages"].as_array().ok_or("messages mancante")?;
    if messages.is_empty() || messages.len() > 40 {
        return Err("numero di messaggi non valido (da 1 a 40)".into());
    }
    let mut out = Vec::new();
    let mut total = 0usize;
    for m in messages {
        let role = match m["role"].as_str() {
            Some(r @ ("user" | "assistant")) => r.to_string(),
            _ => return Err("ruolo non valido".into()),
        };
        let content = if m["content"].is_array() { m["content"][0].clone() } else { m["content"].clone() };
        if content["type"].as_str() != Some("text") {
            return Err("sono ammessi solo contenuti di testo".into());
        }
        let text = content["text"].as_str().unwrap_or_default().to_string();
        total += text.chars().count();
        if total > 60_000 {
            return Err("richiesta troppo lunga".into());
        }
        out.push((role, text));
    }
    Ok(SamplingRequest {
        system: params["systemPrompt"].as_str().map(|s| s.chars().take(8000).collect()),
        messages: out,
        max_tokens: params["maxTokens"].as_u64(),
        temperature: params["temperature"].as_f64(),
    })
}

/// Same words as the engine uses to hide secrets: a server must not ask for them in a form.
fn looks_secret(s: &str) -> bool {
    let n = s.to_ascii_lowercase();
    ["password", "passwd", "passcode", "token", "secret", "api key", "api_key", "apikey", "credential", "private key", "pin", "chiave"]
        .iter()
        .any(|w| n.contains(w))
}

/// Flat schema only (MCP elicitation): an object whose properties are string, number, integer,
/// boolean or a string enum. Anything else, and anything that asks for a secret, is refused.
pub fn validate_elicit_schema(schema: &Value) -> std::result::Result<(), String> {
    if schema["type"].as_str() != Some("object") {
        return Err("requestedSchema deve essere un oggetto".into());
    }
    let props = schema["properties"].as_object().ok_or("requestedSchema senza properties")?;
    if props.is_empty() || props.len() > 20 {
        return Err("numero di campi non valido (da 1 a 20)".into());
    }
    for (name, p) in props {
        let ty = p["type"].as_str().unwrap_or_default();
        if !matches!(ty, "string" | "number" | "integer" | "boolean") {
            return Err(format!("campo `{name}`: tipo non supportato"));
        }
        if let Some(e) = p.get("enum") {
            if ty != "string" || !e.as_array().is_some_and(|a| !a.is_empty() && a.len() <= 50 && a.iter().all(|x| x.is_string())) {
                return Err(format!("campo `{name}`: enum non valido"));
            }
        }
        if looks_secret(name) || p["title"].as_str().is_some_and(looks_secret) {
            return Err(format!("campo `{name}`: un server non può chiedere password o chiavi"));
        }
    }
    Ok(())
}

/// The content the person sent, checked against the schema it was shown for.
pub fn validate_elicit_content(schema: &Value, content: &Map<String, Value>) -> std::result::Result<Map<String, Value>, String> {
    let props = schema["properties"].as_object().ok_or("schema non valido")?;
    for (k, v) in content {
        let p = props.get(k).ok_or_else(|| format!("campo sconosciuto: {k}"))?;
        let ok = match p["type"].as_str().unwrap_or_default() {
            "string" => v.as_str().is_some_and(|s| p["enum"].as_array().is_none_or(|e| e.iter().any(|x| x.as_str() == Some(s)))),
            "number" => v.is_number(),
            "integer" => v.is_i64() || v.is_u64(),
            "boolean" => v.is_boolean(),
            _ => false,
        };
        if !ok {
            return Err(format!("valore non valido per {k}"));
        }
    }
    for r in schema["required"].as_array().into_iter().flatten().filter_map(|r| r.as_str()) {
        if !content.contains_key(r) {
            return Err(format!("manca il campo {r}"));
        }
    }
    Ok(content.clone())
}

// ------------------------------------------------------------------ tools

/// Tool wrapper registered in the [`crate::ToolRegistry`].
pub struct McpTool {
    pub client: Arc<McpClient>,
    pub def: McpToolDef,
    full_name: String,
}

impl McpTool {
    /// The tools of the server, plus `list_resources` and `read_resource` when it advertises resources.
    pub fn wrap(client: Arc<McpClient>) -> Vec<Arc<dyn Tool>> {
        let defs = client.tools();
        let mut out: Vec<Arc<dyn Tool>> = defs
            .iter()
            .map(|d| {
                Arc::new(McpTool {
                    full_name: openai_safe_name(&format!("mcp__{}__{}", client.name, d.name)),
                    client: client.clone(),
                    def: d.clone(),
                }) as Arc<dyn Tool>
            })
            .collect();
        if client.capabilities().resources {
            for kind in [ResourceTool::List, ResourceTool::Read] {
                let full_name = openai_safe_name(&format!("mcp__{}__{}", client.name, kind.suffix()));
                // A tool of the server with the same name wins.
                if !out.iter().any(|t| t.name() == full_name) {
                    out.push(Arc::new(McpResourceTool { client: client.clone(), kind, full_name }));
                }
            }
        }
        out
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.full_name
    }
    fn description(&self) -> &str {
        &self.def.description
    }
    fn parameters(&self) -> Value {
        self.def.input_schema.clone()
    }
    fn risk(&self) -> Risk {
        if self.def.read_only {
            Risk::Read
        } else {
            Risk::Write
        }
    }
    fn server(&self) -> Option<&str> {
        Some(&self.client.name)
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        self.client.call_tool(&self.def.name, args, Some(ctx)).await
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResourceTool {
    List,
    Read,
}

impl ResourceTool {
    fn suffix(&self) -> &'static str {
        match self {
            ResourceTool::List => "list_resources",
            ResourceTool::Read => "read_resource",
        }
    }
}

/// `list_resources` / `read_resource` of a server that has resources.
pub struct McpResourceTool {
    client: Arc<McpClient>,
    kind: ResourceTool,
    full_name: String,
}

#[async_trait]
impl Tool for McpResourceTool {
    fn name(&self) -> &str {
        &self.full_name
    }
    fn description(&self) -> &str {
        match self.kind {
            ResourceTool::List => {
                "Elenca le risorse (file, dati, pagine) che il server MCP mette a disposizione, con i modelli di indirizzo."
            }
            ResourceTool::Read => "Legge una risorsa del server MCP dato il suo indirizzo (uri) preso dall'elenco delle risorse.",
        }
    }
    fn parameters(&self) -> Value {
        match self.kind {
            ResourceTool::List => json!({"type": "object", "properties": {}}),
            ResourceTool::Read => json!({
                "type": "object",
                "properties": {"uri": {"type": "string", "description": "Indirizzo della risorsa, come nell'elenco"}},
                "required": ["uri"]
            }),
        }
    }
    fn risk(&self) -> Risk {
        Risk::Read
    }
    fn server(&self) -> Option<&str> {
        Some(&self.client.name)
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let work = async {
            match self.kind {
                ResourceTool::List => {
                    let (resources, templates) = self.client.list_resources().await?;
                    Ok(ToolOutput::text(format_resource_list(&self.client.name, &resources, &templates)))
                }
                ResourceTool::Read => {
                    let uri = crate::arg_str(&args, "uri")?;
                    if uri.len() > 2048 {
                        return Err(Error::Invalid("uri troppo lunga".into()));
                    }
                    let r = self.client.read_resource(uri).await?;
                    Ok(format_resource_read(&self.client.name, uri, &r))
                }
            }
        };
        tokio::select! {
            r = work => r,
            _ = ctx.cancel.cancelled() => Err(Error::Cancelled),
        }
    }
}

/// The list the model sees: untrusted names and descriptions, so a fixed header and a hard cap.
pub fn format_resource_list(server: &str, resources: &[Value], templates: &[Value]) -> String {
    let clip = |s: &str, n: usize| s.chars().take(n).collect::<String>();
    let mut out = format!("Risorse del server {server} ({} risorse, {} modelli):\n", resources.len(), templates.len());
    for r in resources.iter().take(RESOURCE_LIST_CAP) {
        out.push_str(&format!(
            "- {} · {} ({}) {}\n",
            clip(r["uri"].as_str().unwrap_or(""), 300),
            clip(r["name"].as_str().unwrap_or(""), 100),
            clip(r["mimeType"].as_str().unwrap_or("tipo non indicato"), 60),
            clip(r["description"].as_str().unwrap_or(""), 200),
        ));
    }
    if resources.len() > RESOURCE_LIST_CAP {
        out.push_str(&format!("… e altre {} risorse non mostrate\n", resources.len() - RESOURCE_LIST_CAP));
    }
    if !templates.is_empty() {
        out.push_str("Modelli di indirizzo:\n");
        for t in templates.iter().take(RESOURCE_LIST_CAP) {
            out.push_str(&format!(
                "- {} · {} {}\n",
                clip(t["uriTemplate"].as_str().unwrap_or(""), 300),
                clip(t["name"].as_str().unwrap_or(""), 100),
                clip(t["description"].as_str().unwrap_or(""), 200),
            ));
        }
    }
    out
}

/// A `resources/read` result as a tool output: text with a size cap, images as images, the rest summarised.
pub fn format_resource_read(server: &str, uri: &str, result: &Value) -> ToolOutput {
    let mut out = ToolOutput::default();
    let mut texts = Vec::new();
    let mut budget = RESOURCE_TEXT_CAP;
    for c in result["contents"].as_array().cloned().unwrap_or_default() {
        let mime = c["mimeType"].as_str().unwrap_or("");
        let u = c["uri"].as_str().unwrap_or(uri);
        if let Some(text) = c["text"].as_str() {
            let n = text.chars().count();
            let shown: String = text.chars().take(budget).collect();
            budget = budget.saturating_sub(n);
            let mut s =
                format!("[risorsa {u} del server {server}{}]\n{shown}", if mime.is_empty() { String::new() } else { format!(", {mime}") });
            if n > shown.chars().count() {
                s.push_str(&format!("\n[… testo tagliato: mostrati {} caratteri su {n}]", shown.chars().count()));
            }
            texts.push(s);
        } else if let Some(blob) = c["blob"].as_str() {
            let bytes = blob.len() / 4 * 3;
            if mime.starts_with("image/") && blob.len() <= RESOURCE_IMAGE_CAP {
                out.images.push(format!("data:{mime};base64,{blob}"));
                texts.push(format!("[risorsa {u}: immagine {mime}, circa {bytes} byte]"));
            } else {
                texts.push(format!(
                    "[risorsa {u}: contenuto binario{} di circa {bytes} byte, non mostrato]",
                    if mime.is_empty() { String::new() } else { format!(" ({mime})") }
                ));
            }
        }
    }
    if texts.is_empty() {
        texts.push(format!("[risorsa {uri}: nessun contenuto]"));
    }
    out.content = texts.join("\n\n");
    out
}

#[cfg(test)]
mod tests;
