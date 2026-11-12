//! Remote agents: the registry (`remote_agent` documents, plus the paired devices) and the A2A client
//! that delegates a task to one of them. The client talks only to the registered URL: the Agent Card must
//! point at the same origin, and redirects are not followed. Every delegation is audited.

use crate::{Engine, REMOTE_PREFIX};
use aiw_core::api::{AuditEntry, RemoteAgent, SECRET_MASK};
use aiw_core::ids::new_id;
use aiw_core::{Error, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const DEFAULT_TIMEOUT_SECS: u64 = 600;
/// Id prefix of the remote agents derived from paired devices.
pub const PEER_PREFIX: &str = "peer-";

fn slug_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 48 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn http_url(raw: &str) -> Result<reqwest::Url> {
    let u = reqwest::Url::parse(raw.trim()).map_err(|_| Error::Invalid(format!("URL non valido: {raw}")))?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
        return Err(Error::Invalid("l'URL dell'agente remoto deve essere http(s)".into()));
    }
    Ok(u)
}

fn same_origin(a: &reqwest::Url, b: &reqwest::Url) -> bool {
    a.scheme() == b.scheme() && a.host_str() == b.host_str() && a.port_or_known_default() == b.port_or_known_default()
}

/// The Agent Card URL for a registered URL: itself when it names a JSON document, else the well-known path.
fn card_url(url: &reqwest::Url) -> reqwest::Url {
    if url.path().ends_with(".json") {
        return url.clone();
    }
    let mut u = url.clone();
    u.set_path("/.well-known/agent-card.json");
    u.set_query(None);
    u
}

fn state_of(task: &Value) -> String {
    task["status"]["state"].as_str().unwrap_or_default().trim_start_matches("TASK_STATE_").to_lowercase().replace('_', "-")
}

fn parts_text(v: &Value) -> String {
    v["parts"].as_array().map(|p| p.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default()
}

/// The answer of a finished task: its artifacts, else the status message, else the last agent message.
fn answer_of(task: &Value) -> String {
    let art: Vec<String> =
        task["artifacts"].as_array().map(|a| a.iter().map(parts_text).filter(|t| !t.is_empty()).collect()).unwrap_or_default();
    if !art.is_empty() {
        return art.join("\n\n");
    }
    let status = parts_text(&task["status"]["message"]);
    if !status.is_empty() {
        return status;
    }
    task["history"]
        .as_array()
        .and_then(|h| h.iter().rev().find(|m| matches!(m["role"].as_str(), Some("ROLE_AGENT" | "agent"))))
        .map(parts_text)
        .unwrap_or_default()
}

#[derive(PartialEq)]
enum Flow {
    Continue,
    Done,
}

/// Folds one event of a task stream into `task` (the answer so far). Everything in it comes from the remote
/// agent: the text kept is capped, and only the answer and the status are read.
fn apply_stream_event(task: &mut Value, result: &Value, text_len: &mut usize, progress: &(dyn Fn(String) + Sync)) -> Flow {
    const MAX_TEXT: usize = 2_000_000;
    let kind = result["kind"].as_str().unwrap_or_default();
    let (what, payload) = if let Some(t) = result.get("task") {
        ("task", t)
    } else if let Some(t) = result.get("statusUpdate") {
        ("status", t)
    } else if let Some(t) = result.get("artifactUpdate") {
        ("artifact", t)
    } else {
        match kind {
            "task" => ("task", result),
            "status-update" => ("status", result),
            "artifact-update" => ("artifact", result),
            _ => return Flow::Continue,
        }
    };
    match what {
        "task" => {
            *task = payload.clone();
            if !task["artifacts"].is_array() {
                task["artifacts"] = json!([]);
            }
        }
        "status" => {
            if !task.is_object() {
                *task = json!({"id": payload["taskId"], "artifacts": []});
            }
            task["status"] = payload["status"].clone();
            let note = parts_text(&payload["status"]["message"]);
            if !note.is_empty() && !is_final_state(&state_of(task)) {
                progress(note.chars().take(200).collect());
            }
        }
        _ => {
            if !task.is_object() {
                *task = json!({"id": payload["taskId"], "artifacts": []});
            }
            let art = &payload["artifact"];
            let text = parts_text(art);
            *text_len += text.len();
            if *text_len > MAX_TEXT {
                return Flow::Continue;
            }
            let id = art["artifactId"].as_str().unwrap_or("risposta").to_string();
            let list = task["artifacts"].as_array_mut().expect("artifacts is an array");
            let append = payload["append"].as_bool() == Some(true);
            match list.iter_mut().find(|a| a["artifactId"].as_str() == Some(id.as_str())) {
                Some(existing) if append => {
                    let merged = format!("{}{}", parts_text(existing), text);
                    existing["parts"] = json!([{"text": merged}]);
                }
                Some(existing) => existing["parts"] = json!([{"text": text}]),
                None => list.push(json!({"artifactId": id, "parts": [{"text": text}]})),
            }
        }
    }
    if is_final_state(&state_of(task)) {
        Flow::Done
    } else {
        Flow::Continue
    }
}

fn is_final_state(state: &str) -> bool {
    matches!(state, "completed" | "failed" | "rejected" | "canceled" | "cancelled")
}

impl Engine {
    /// The registered remote agents followed by the paired devices, with their tokens (internal use).
    fn remote_agents_raw(&self) -> Vec<RemoteAgent> {
        let mut all: Vec<RemoteAgent> = self.0.store.list_docs("remote_agent").unwrap_or_default();
        for p in self.peers() {
            let token =
                self.0.store.get_doc::<Value>("peer_token", &p.id).ok().flatten().and_then(|v| v["token"].as_str().map(String::from));
            all.push(RemoteAgent {
                id: format!("{PEER_PREFIX}{}", p.id),
                name: p.name.clone(),
                url: p.address.clone(),
                token: token.unwrap_or_default(),
                skill: None,
                description: format!("Dispositivo accoppiato ({})", p.os),
                paired: true,
            });
        }
        all
    }
    fn mask_remote(mut r: RemoteAgent) -> RemoteAgent {
        if !r.token.is_empty() {
            r.token = SECRET_MASK.into();
        }
        r
    }
    /// The remote agents as the API shows them (token masked).
    pub fn remote_agents(&self) -> Vec<RemoteAgent> {
        self.remote_agents_raw().into_iter().map(Self::mask_remote).collect()
    }
    /// A remote agent by id, with or without the `remote:` prefix.
    pub(crate) fn remote_agent(&self, id: &str) -> Result<RemoteAgent> {
        let id = id.strip_prefix(REMOTE_PREFIX).unwrap_or(id);
        self.remote_agents_raw().into_iter().find(|r| r.id == id).ok_or_else(|| Error::NotFound(format!("agente remoto {id} non trovato")))
    }
    pub fn save_remote_agent(&self, mut r: RemoteAgent) -> Result<RemoteAgent> {
        r.id = r.id.trim().to_string();
        if !slug_ok(&r.id) {
            return Err(Error::Invalid("id dell'agente remoto non valido (lettere, cifre, - e _, al massimo 48)".into()));
        }
        if r.id.starts_with(PEER_PREFIX) {
            return Err(Error::Invalid(format!("gli id che iniziano con `{PEER_PREFIX}` sono riservati ai dispositivi accoppiati")));
        }
        r.name = r.name.trim().to_string();
        if r.name.is_empty() {
            return Err(Error::Invalid("il nome dell'agente remoto è obbligatorio".into()));
        }
        r.url = http_url(&r.url)?.to_string();
        r.skill = r.skill.take().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        r.paired = false;
        if r.token == SECRET_MASK {
            // The mask means "keep what is stored"; with nothing stored it is not a token.
            r.token = self.0.store.get_doc::<RemoteAgent>("remote_agent", &r.id)?.map(|old| old.token).unwrap_or_default();
        }
        self.0.store.put_doc("remote_agent", &r.id, &r)?;
        self.audit_user("remote_agent.save", &format!("{} ({})", r.name, r.url));
        Ok(Self::mask_remote(r))
    }
    pub fn delete_remote_agent(&self, id: &str) -> Result<()> {
        let r: RemoteAgent =
            self.0.store.get_doc("remote_agent", id)?.ok_or_else(|| Error::NotFound(format!("agente remoto {id} non trovato")))?;
        let used = self.teams_using_agent(&format!("{REMOTE_PREFIX}{id}"))?;
        if !used.is_empty() {
            return Err(Error::Conflict(format!("«{}» è usato dai team: {}", r.name, used.join(", "))));
        }
        self.0.store.delete_doc("remote_agent", id)?;
        self.audit_user("remote_agent.delete", &format!("{} ({id})", r.name));
        Ok(())
    }

    /// Tasks served to other agents over A2A (kept in SQLite for a day).
    pub fn a2a_put_task(&self, task: &Value, session: Option<&str>) -> Result<()> {
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
        let _ = self.0.store.a2a_cleanup(&cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        self.0.store.a2a_put(task, session)
    }
    pub fn a2a_task(&self, id: &str) -> Result<Option<(Value, Option<String>)>> {
        self.0.store.a2a_get(id)
    }
    /// Changes a task atomically; `None` when it does not exist (any more).
    pub fn a2a_update_task<R>(&self, id: &str, f: impl FnOnce(&mut Value, &mut Option<String>) -> R) -> Result<Option<R>> {
        self.0.store.a2a_update(id, f)
    }
    pub fn a2a_context_session(&self, context: &str) -> Result<Option<String>> {
        self.0.store.a2a_context_session(context)
    }

    /// Entries `(remote:<id>, description)` for the agents' sub-agent lists.
    pub(crate) fn remote_subagents(&self) -> Vec<(String, String)> {
        self.remote_agents_raw()
            .into_iter()
            .map(|r| {
                let extra = if r.description.is_empty() { String::new() } else { format!(": {}", r.description) };
                (format!("{REMOTE_PREFIX}{}", r.id), format!("{} (agente remoto){extra}", r.name))
            })
            .collect()
    }

    /// Delegates `task` to the remote agent over A2A and returns its answer. Waits for the task to end, polling
    /// with a growing delay up to `a2a.timeout_seconds` (default 10 minutes); cancelling `cancel` cancels the
    /// remote task too.
    pub async fn delegate_remote(&self, remote_id: &str, task: &str, cancel: &CancellationToken, run_id: Option<&str>) -> Result<String> {
        let remote = self.remote_agent(remote_id)?;
        let secs = self.0.config.get("a2a.timeout_seconds").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_TIMEOUT_SECS).max(1);
        let short: String = task.chars().take(80).collect();
        let audit = |action: &str, detail: String| {
            let _ = self.0.store.add_audit(&AuditEntry {
                id: new_id("aud"),
                ts: aiw_core::now_rfc3339(),
                run_id: run_id.map(String::from),
                actor: "agent".into(),
                action: action.into(),
                tool: Some("a2a".into()),
                decision: None,
                detail,
            });
        };
        audit("a2a.delegate", format!("{} ({}) · {short}", remote.name, remote.url));
        // What the remote agent reports while it works (the tool it uses) goes to the trace of the run that delegated.
        let progress = |note: String| {
            if let Some(r) = run_id {
                self.emit(
                    r,
                    aiw_core::events::AgEvent::Custom {
                        name: "aiw.remote_progress".into(),
                        value: json!({"agent": remote.name, "note": note}),
                    },
                );
            }
        };
        let out = A2aClient::new(&remote, Duration::from_secs(secs))?.run(task, cancel, &progress).await;
        match &out {
            Ok(_) => audit("a2a.result", format!("{}: completato", remote.name)),
            Err(e) => audit("a2a.result", format!("{}: {e}", remote.name)),
        }
        out
    }
}

struct A2aClient {
    http: reqwest::Client,
    base: reqwest::Url,
    card: reqwest::Url,
    token: String,
    skill: Option<String>,
    timeout: Duration,
}

impl A2aClient {
    fn new(remote: &RemoteAgent, timeout: Duration) -> Result<Self> {
        let base = http_url(&remote.url)?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(Error::other)?;
        Ok(Self { http, card: card_url(&base), base, token: remote.token.clone(), skill: remote.skill.clone(), timeout })
    }
    fn authed(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.token.is_empty() {
            rb
        } else {
            rb.bearer_auth(&self.token)
        }
    }
    fn err(&self, what: &str, e: impl std::fmt::Display) -> Error {
        Error::Runtime(format!("agente remoto {}: {what}: {e}", self.base))
    }

    /// The JSON-RPC endpoint announced by the Agent Card, which must stay on the registered origin.
    async fn endpoint(&self) -> Result<(reqwest::Url, bool)> {
        let r = self.authed(self.http.get(self.card.clone())).send().await.map_err(|e| self.err("Agent Card non raggiungibile", e))?;
        if !r.status().is_success() {
            return Err(self.err("Agent Card", format!("risposta {}", r.status())));
        }
        let card: Value = r.json().await.map_err(|e| self.err("Agent Card non valida", e))?;
        let url = card["supportedInterfaces"]
            .as_array()
            .and_then(|a| a.iter().find(|i| i["protocolBinding"].as_str().is_none_or(|b| b.eq_ignore_ascii_case("JSONRPC"))))
            .and_then(|i| i["url"].as_str())
            .or_else(|| card["url"].as_str())
            .ok_or_else(|| self.err("Agent Card", "nessuna interfaccia JSON-RPC"))?;
        let ep = reqwest::Url::parse(url).map_err(|_| self.err("Agent Card", format!("URL non valido: {url}")))?;
        if !same_origin(&ep, &self.base) {
            return Err(Error::Denied(format!("l'Agent Card di {} indica un altro indirizzo ({ep}): non è un URL registrato", self.base)));
        }
        Ok((ep, card["capabilities"]["streaming"].as_bool() == Some(true)))
    }

    async fn rpc(&self, ep: &reqwest::Url, method: &str, params: Value) -> Result<Value> {
        let body = json!({"jsonrpc": "2.0", "id": new_id("rpc"), "method": method, "params": params});
        let r = self.authed(self.http.post(ep.clone())).json(&body).send().await.map_err(|e| self.err(method, e))?;
        let status = r.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(Error::Denied(format!("agente remoto {}: accesso rifiutato, controlla il token", self.base)));
        }
        let v: Value = r.json().await.map_err(|e| self.err(method, format!("risposta non valida ({status}): {e}")))?;
        if let Some(e) = v.get("error") {
            return Err(self.err(method, e["message"].as_str().unwrap_or("errore")));
        }
        let result = v.get("result").cloned().ok_or_else(|| self.err(method, "risposta senza risultato"))?;
        // 1.0 wraps the task in `task`; the 0.3 shape is the task itself.
        Ok(result.get("task").cloned().unwrap_or(result))
    }

    /// `SendStreamingMessage` (0.3: `message/stream`): follows the SSE stream until the task ends. `Ok(None)`
    /// means the remote did not stream (the caller sends the message again the plain way); `Ok(Some(task))`
    /// is the task as built from the stream, which is not finished when the stream broke half way (the caller
    /// then polls `GetTask`).
    async fn run_streaming(
        &self,
        ep: &reqwest::Url,
        params: &Value,
        cancel: &CancellationToken,
        deadline: Instant,
        progress: &(dyn Fn(String) + Sync),
    ) -> Result<Option<Value>> {
        let body = json!({"jsonrpc": "2.0", "id": new_id("rpc"), "method": "SendStreamingMessage", "params": params});
        let sent = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            r = self.authed(self.http.post(ep.clone())).header("Accept", "text/event-stream").json(&body).send() => r,
        };
        let Ok(mut res) = sent else { return Ok(None) };
        let status = res.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(Error::Denied(format!("agente remoto {}: accesso rifiutato, controlla il token", self.base)));
        }
        let is_sse =
            res.headers().get(reqwest::header::CONTENT_TYPE).and_then(|h| h.to_str().ok()).is_some_and(|c| c.contains("text/event-stream"));
        if !status.is_success() || !is_sse {
            return Ok(None);
        }
        let mut parser = aiw_tools::mcp::SseParser::default();
        let mut task = Value::Null;
        let mut text_len = 0usize;
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => {
                    if let Some(id) = task["id"].as_str() {
                        let _ = self.rpc(ep, "CancelTask", json!({"id": id})).await;
                    }
                    return Err(Error::Cancelled);
                }
                _ = tokio::time::sleep_until(deadline.into()) => {
                    if let Some(id) = task["id"].as_str() {
                        let _ = self.rpc(ep, "CancelTask", json!({"id": id})).await;
                    }
                    return Err(self.err("tempo scaduto", format!("nessuna risposta entro {} secondi", self.timeout.as_secs())));
                }
                c = res.chunk() => c,
            };
            let Ok(Some(chunk)) = chunk else { break };
            for msg in parser.push(&chunk) {
                if let Some(e) = msg.get("error") {
                    return Err(self.err("SendStreamingMessage", e["message"].as_str().unwrap_or("errore")));
                }
                if apply_stream_event(&mut task, &msg["result"], &mut text_len, progress) == Flow::Done {
                    return Ok(Some(task));
                }
            }
        }
        // Closed without an end: no task at all means the remote does not really stream.
        Ok(task.is_object().then_some(task))
    }

    async fn run(&self, text: &str, cancel: &CancellationToken, progress: &(dyn Fn(String) + Sync)) -> Result<String> {
        let (ep, streaming) = tokio::select! { _ = cancel.cancelled() => return Err(Error::Cancelled), r = self.endpoint() => r? };
        let mut params = json!({
            "message": {"messageId": new_id("msg"), "role": "ROLE_USER", "parts": [{"text": text}]},
            "configuration": {"blocking": false}
        });
        if let Some(skill) = &self.skill {
            params["metadata"] = json!({"agent": skill});
        }
        let started = Instant::now();
        let streamed = if streaming { self.run_streaming(&ep, &params, cancel, started + self.timeout, progress).await? } else { None };
        let mut task = match streamed {
            Some(t) => t,
            None => {
                tokio::select! { _ = cancel.cancelled() => return Err(Error::Cancelled), r = self.rpc(&ep, "SendMessage", params) => r? }
            }
        };
        let id = task["id"].as_str().ok_or_else(|| self.err("SendMessage", "il task non ha un id"))?.to_string();
        let mut delay = Duration::from_millis(400);
        loop {
            match state_of(&task).as_str() {
                "completed" => {
                    let a = answer_of(&task);
                    return Ok(if a.is_empty() { "(nessuna risposta)".into() } else { a });
                }
                "failed" | "rejected" => {
                    let why = parts_text(&task["status"]["message"]);
                    return Err(self.err("il task è fallito", if why.is_empty() { "nessun dettaglio".into() } else { why }));
                }
                "canceled" | "cancelled" => return Err(self.err("il task è stato annullato", "dall'agente remoto")),
                "input-required" | "auth-required" => return Err(self.err("il task richiede un intervento", "non gestibile da una delega")),
                _ => {}
            }
            if started.elapsed() >= self.timeout {
                let _ = self.rpc(&ep, "CancelTask", json!({"id": id})).await;
                return Err(self.err("tempo scaduto", format!("nessuna risposta entro {} secondi", self.timeout.as_secs())));
            }
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = self.rpc(&ep, "CancelTask", json!({"id": id})).await;
                    return Err(Error::Cancelled);
                }
                _ = tokio::time::sleep(delay) => {}
            }
            delay = (delay * 3 / 2).min(Duration::from_secs(5));
            task = self.rpc(&ep, "GetTask", json!({"id": id})).await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_url_and_origin() {
        let base = http_url("http://pc.local:7777/").unwrap();
        assert_eq!(card_url(&base).as_str(), "http://pc.local:7777/.well-known/agent-card.json");
        let card = http_url("https://x.example/agents/a/card.json?v=1").unwrap();
        assert_eq!(card_url(&card).as_str(), "https://x.example/agents/a/card.json?v=1");
        assert!(same_origin(&base, &http_url("http://pc.local:7777/a2a").unwrap()));
        assert!(!same_origin(&base, &http_url("http://pc.local:8888/a2a").unwrap()));
        assert!(http_url("ftp://x").is_err());
    }

    #[test]
    fn states_and_answers_in_both_shapes() {
        assert_eq!(state_of(&json!({"status": {"state": "TASK_STATE_INPUT_REQUIRED"}})), "input-required");
        assert_eq!(state_of(&json!({"status": {"state": "completed"}})), "completed");
        let t = json!({"artifacts": [{"parts": [{"text": "a"}]}, {"parts": [{"text": "b"}]}]});
        assert_eq!(answer_of(&t), "a\n\nb");
        let t = json!({"history": [{"role": "ROLE_USER", "parts": [{"text": "q"}]}, {"role": "agent", "parts": [{"text": "r"}]}]});
        assert_eq!(answer_of(&t), "r");
    }
}
