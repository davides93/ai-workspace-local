//! Minimal MCP client (pattern P12): stdio (desktop) and streamable HTTP (all platforms).
//! Implements initialize → notifications/initialized → tools/list → tools/call, answers server
//! `ping`/`roots/list` requests, and exposes each server tool as `mcp__<server>__<tool>`.

use crate::openai_safe_name;
use crate::{Risk, Tool, ToolContext, ToolOutput};
use aiw_core::api::McpServerConfig;
use aiw_core::{Error, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

pub const PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub read_only: bool,
}

enum Transport {
    Stdio {
        tx: mpsc::UnboundedSender<String>,
        pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
        child: tokio::sync::Mutex<Option<tokio::process::Child>>,
    },
    Http {
        url: String,
        http: reqwest::Client,
        session: Mutex<Option<String>>,
    },
}

pub struct McpClient {
    pub name: String,
    transport: Transport,
    next: AtomicU64,
    pub tools: Vec<McpToolDef>,
    pub server_info: Value,
}

fn rpc(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// Extracts the JSON-RPC message with `id` from an SSE body.
fn from_sse(body: &str, id: u64) -> Option<Value> {
    body.lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
        .find(|v| v.get("id").and_then(|i| i.as_u64()) == Some(id))
}

impl McpClient {
    pub async fn connect(cfg: &McpServerConfig) -> Result<Self> {
        let transport = match cfg.transport.as_str() {
            "stdio" => Self::spawn_stdio(cfg)?,
            "http" => {
                let mut headers = reqwest::header::HeaderMap::new();
                for (k, v) in cfg.headers.iter().flatten() {
                    let (Ok(name), Some(Ok(value))) =
                        (reqwest::header::HeaderName::from_bytes(k.as_bytes()), v.as_str().map(reqwest::header::HeaderValue::from_str))
                    else {
                        return Err(Error::Invalid(format!("header MCP non valido: {k}")));
                    };
                    headers.insert(name, value);
                }
                Transport::Http {
                    url: cfg.url.clone().filter(|u| !u.is_empty()).ok_or_else(|| Error::Invalid("server MCP HTTP senza url".into()))?,
                    http: reqwest::Client::builder()
                        .connect_timeout(Duration::from_secs(5))
                        .timeout(Duration::from_secs(600))
                        .default_headers(headers)
                        .build()
                        .map_err(Error::other)?,
                    session: Mutex::new(None),
                }
            }
            other => return Err(Error::Invalid(format!("trasporto MCP sconosciuto: {other}"))),
        };
        let mut c = Self { name: cfg.name.clone(), transport, next: AtomicU64::new(1), tools: vec![], server_info: Value::Null };
        let init = c
            .request("initialize", json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {"roots": {"listChanged": false}}, "clientInfo": {"name": "ai-workspace-local", "version": aiw_core::VERSION}}))
            .await?;
        c.server_info = init.get("serverInfo").cloned().unwrap_or(Value::Null);
        c.notify("notifications/initialized", json!({})).await?;
        c.tools = c.list_tools().await?;
        Ok(c)
    }

    fn spawn_stdio(cfg: &McpServerConfig) -> Result<Transport> {
        if cfg!(target_os = "ios") {
            return Err(Error::Unsupported("su iOS i server MCP devono essere HTTP".into()));
        }
        let cmd = cfg.command.clone().filter(|c| !c.is_empty()).ok_or_else(|| Error::Invalid("server MCP stdio senza comando".into()))?;
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
        let mut child = command.spawn().map_err(|e| Error::Runtime(format!("avvio server MCP `{cmd}`: {e}")))?;
        let mut stdin = child.stdin.take().ok_or_else(|| Error::Runtime("stdin non disponibile".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| Error::Runtime("stdout non disponibile".into()))?;
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>> = Default::default();
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            while let Some(line) = rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err() || stdin.write_all(b"\n").await.is_err() || stdin.flush().await.is_err()
                {
                    break;
                }
            }
        });
        let p2 = pending.clone();
        let reply = tx.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                let id = v.get("id").cloned();
                match (v.get("method").and_then(|m| m.as_str()), id) {
                    (Some(method), Some(id)) => {
                        // Server → client request.
                        let resp = match method {
                            "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
                            "roots/list" => json!({"jsonrpc":"2.0","id":id,"result":{"roots":[]}}),
                            _ => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"not supported by AI Workspace Local"}}),
                        };
                        let _ = reply.send(resp.to_string());
                    }
                    (None, Some(id)) => {
                        if let Some(n) = id.as_u64() {
                            if let Some(s) = p2.lock().unwrap().remove(&n) {
                                let _ = s.send(v);
                            }
                        }
                    }
                    _ => {} // notifications
                }
            }
        });
        Ok(Transport::Stdio { tx, pending, child: tokio::sync::Mutex::new(Some(child)) })
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let msg = rpc(id, method, params);
        let v = match &self.transport {
            Transport::Stdio { tx, pending, .. } => {
                let (s, r) = oneshot::channel();
                pending.lock().unwrap().insert(id, s);
                tx.send(msg.to_string()).map_err(|_| Error::Runtime(format!("server MCP {} terminato", self.name)))?;
                tokio::time::timeout(Duration::from_secs(120), r)
                    .await
                    .map_err(|_| Error::Runtime(format!("MCP {method}: tempo scaduto")))?
                    .map_err(|_| Error::Runtime("server MCP terminato".into()))?
            }
            Transport::Http { url, http, session } => {
                let mut rb = http
                    .post(url)
                    .header("Accept", "application/json, text/event-stream")
                    .header("MCP-Protocol-Version", PROTOCOL_VERSION)
                    .json(&msg);
                if let Some(s) = session.lock().unwrap().clone() {
                    rb = rb.header("Mcp-Session-Id", s);
                }
                let res = rb.send().await.map_err(|e| Error::Runtime(format!("MCP {}: {e}", self.name)))?;
                if let Some(s) = res.headers().get("mcp-session-id").and_then(|h| h.to_str().ok()) {
                    *session.lock().unwrap() = Some(s.to_string());
                }
                let status = res.status();
                let ctype = res.headers().get(reqwest::header::CONTENT_TYPE).and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
                let body = res.text().await.map_err(Error::other)?;
                if !status.is_success() {
                    return Err(Error::Runtime(format!("MCP {method} → {status}: {}", body.chars().take(300).collect::<String>())));
                }
                if ctype.contains("text/event-stream") {
                    from_sse(&body, id).ok_or_else(|| Error::Runtime(format!("MCP {method}: risposta SSE senza id {id}")))?
                } else {
                    serde_json::from_str(&body)?
                }
            }
        };
        if let Some(e) = v.get("error") {
            return Err(Error::Runtime(format!("MCP {method}: {}", e["message"].as_str().unwrap_or("errore"))));
        }
        Ok(v["result"].clone())
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        match &self.transport {
            Transport::Stdio { tx, .. } => {
                let _ = tx.send(msg.to_string());
            }
            Transport::Http { url, http, session } => {
                let mut rb = http
                    .post(url)
                    .header("Accept", "application/json, text/event-stream")
                    .header("MCP-Protocol-Version", PROTOCOL_VERSION)
                    .json(&msg);
                if let Some(s) = session.lock().unwrap().clone() {
                    rb = rb.header("Mcp-Session-Id", s);
                }
                let _ = rb.send().await;
            }
        }
        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDef>> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let r = self.request("tools/list", params).await?;
            for t in r["tools"].as_array().cloned().unwrap_or_default() {
                out.push(McpToolDef {
                    name: t["name"].as_str().unwrap_or_default().to_string(),
                    description: t["description"].as_str().unwrap_or_default().to_string(),
                    input_schema: t.get("inputSchema").cloned().unwrap_or_else(|| json!({"type":"object","properties":{}})),
                    read_only: t["annotations"]["readOnlyHint"].as_bool().unwrap_or(false),
                });
            }
            cursor = r["nextCursor"].as_str().map(String::from);
            if cursor.is_none() || out.len() > 1000 {
                break;
            }
        }
        Ok(out)
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<ToolOutput> {
        let r = self.request("tools/call", json!({"name": name, "arguments": args})).await?;
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

    pub async fn shutdown(&self) {
        if let Transport::Stdio { child, .. } = &self.transport {
            if let Some(mut c) = child.lock().await.take() {
                let _ = c.kill().await;
            }
        }
    }
}

/// Tool wrapper registered in the [`crate::ToolRegistry`].
pub struct McpTool {
    pub client: Arc<McpClient>,
    pub def: McpToolDef,
    full_name: String,
}

impl McpTool {
    pub fn wrap(client: Arc<McpClient>) -> Vec<Arc<dyn Tool>> {
        client
            .tools
            .iter()
            .map(|d| {
                Arc::new(McpTool {
                    full_name: openai_safe_name(&format!("mcp__{}__{}", client.name, d.name)),
                    client: client.clone(),
                    def: d.clone(),
                }) as Arc<dyn Tool>
            })
            .collect()
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
        tokio::select! {
            r = self.client.call_tool(&self.def.name, args) => r,
            _ = ctx.cancel.cancelled() => Err(Error::Cancelled),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny MCP server written in shell+python is not portable; instead we exercise the
    /// stdio transport with a fake server implemented as a Python one-liner when available.
    #[tokio::test]
    async fn stdio_roundtrip_with_fake_server() {
        let Ok(py) = which::which("python3") else { return };
        let script = r#"
import sys, json
for line in sys.stdin:
    m = json.loads(line)
    if "id" not in m: continue
    if m["method"] == "initialize":
        r = {"protocolVersion": m["params"]["protocolVersion"], "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1"}}
    elif m["method"] == "tools/list":
        r = {"tools": [{"name": "add", "description": "Somma", "inputSchema": {"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}}, "annotations": {"readOnlyHint": True}}]}
    elif m["method"] == "tools/call":
        a = m["params"]["arguments"]
        r = {"content": [{"type": "text", "text": str(a["a"] + a["b"])}]}
    else:
        r = {}
    print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": r}), flush=True)
"#;
        let cfg = McpServerConfig {
            id: "t".into(),
            name: "fake".into(),
            transport: "stdio".into(),
            command: Some(py.display().to_string()),
            args: Some(vec!["-c".into(), script.into()]),
            url: None,
            env: None,
            headers: None,
            enabled: true,
        };
        let client = Arc::new(McpClient::connect(&cfg).await.unwrap());
        assert_eq!(client.tools.len(), 1);
        let tools = McpTool::wrap(client.clone());
        assert_eq!(tools[0].name(), "mcp__fake__add");
        assert_eq!(tools[0].risk(), Risk::Read);
        let ctx = ToolContext::new("r", std::env::temp_dir());
        let out = tools[0].call(json!({"a": 2, "b": 3}), &ctx).await.unwrap();
        assert_eq!(out.content.trim(), "5");
        client.shutdown().await;
    }

    #[test]
    fn parses_sse_responses() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n";
        assert_eq!(from_sse(body, 7).unwrap()["result"]["ok"], true);
        assert!(from_sse(body, 8).is_none());
    }
}
