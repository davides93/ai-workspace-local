//! AG-UI entry point: `POST /agui/{agent}` takes a `RunAgentInput` and answers with the SSE stream of
//! the run, which is what an AG-UI client (`HttpAgent` of `@ag-ui/client`, CopilotKit) expects. The
//! events themselves are already AG-UI (`aiw_core::events::AgEvent`, ADR 0004): this module only
//! maps the request onto a session and a run, and follows the run on the bus. See `docs/agui.md`.
//!
//! First cut: the text and the files of the LAST user message start the run; the history stays the
//! one the server keeps for the session (what the client sends before is not reconciled), and tools,
//! context and state sent by the client are accepted and ignored.

use crate::extract::{Json, Path};
use crate::{ApiError, AppState};
use aiw_core::api::AttachmentUpload;
use aiw_core::events::SeqEvent;
use aiw_core::Error;
use aiw_engine::Engine;
use axum::extract::State;
use axum::http::{HeaderName, HeaderValue};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::stream;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

/// `threadId` -> session id for the threads the server has seen. Memory only: after a restart the
/// map is empty and a known `threadId` starts a new session (unless it is a session id itself).
pub(crate) type Threads = Arc<Mutex<HashMap<String, String>>>;

/// Body of the request (camelCase). `tools`, `context`, `state`, `forwardedProps`, `parentRunId`
/// and any other field are accepted and ignored in this first cut.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunAgentInput {
    thread_id: String,
    run_id: String,
    messages: Vec<InMessage>,
}

#[derive(Deserialize)]
struct InMessage {
    role: String,
    /// A string or a list of parts; only the last user message is read, and only then.
    #[serde(default)]
    content: Value,
}

fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}

/// The text and the inline files of a user message. Errors are for the caller (HTTP 400).
fn parse_user_content(content: &Value) -> Result<(String, Vec<AttachmentUpload>), Error> {
    let parts = match content {
        Value::String(t) => return Ok((t.clone(), vec![])),
        Value::Array(parts) => parts,
        _ => return Err(invalid("il contenuto del messaggio deve essere una stringa o una lista di parti")),
    };
    let mut texts: Vec<&str> = vec![];
    let mut uploads: Vec<AttachmentUpload> = vec![];
    for part in parts {
        let kind = part.get("type").and_then(Value::as_str).ok_or_else(|| invalid("una parte del contenuto non ha il campo «type»"))?;
        let str_field = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let filename = || {
            str_field(part, "filename")
                .or_else(|| part.get("metadata").and_then(|m| str_field(m, "filename").or_else(|| str_field(m, "name"))))
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| format!("allegato-{}", uploads.len() + 1))
        };
        match kind {
            "text" => {
                texts.push(part.get("text").and_then(Value::as_str).ok_or_else(|| invalid("una parte «text» non ha il campo «text»"))?)
            }
            // Legacy form: `{type: "binary", mimeType, data (base64), id?, url?, filename?}`.
            "binary" => {
                let data = str_field(part, "data")
                    .filter(|d| !d.is_empty())
                    .ok_or_else(|| invalid("la parte «binary» non ha dati inline (solo url o id): non è supportata"))?;
                uploads.push(AttachmentUpload { name: filename(), mime: str_field(part, "mimeType").unwrap_or_default(), data });
            }
            // Current form: `{type: "image" | "audio" | "video" | "document", source: {type, value, mimeType}}`.
            "image" | "audio" | "video" | "document" => {
                let source = part.get("source").ok_or_else(|| invalid(format!("la parte «{kind}» non ha il campo «source»")))?;
                let source_type = str_field(source, "type").unwrap_or_default();
                if source_type != "data" {
                    return Err(invalid(format!(
                        "la parte «{kind}» con sorgente «{source_type}» non ha dati inline: sono supportate solo le sorgenti «data»"
                    )));
                }
                let data = str_field(source, "value")
                    .filter(|d| !d.is_empty())
                    .ok_or_else(|| invalid(format!("la parte «{kind}» non ha dati nella sorgente")))?;
                uploads.push(AttachmentUpload { name: filename(), mime: str_field(source, "mimeType").unwrap_or_default(), data });
            }
            other => return Err(invalid(format!("tipo di parte non supportato: «{other}»"))),
        }
    }
    Ok((texts.join("\n"), uploads))
}

/// Title of a new session: the first non-empty line of the message, as the engine does.
fn title_of(text: &str) -> Option<String> {
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.chars().take(60).collect())
}

/// The session a thread belongs to and whether this call created it. A `threadId` that is the id of
/// an existing session uses it; otherwise the in-memory map decides, and a thread not seen before
/// gets a new session. A session of another agent is a conflict.
fn resolve_session(s: &AppState, agent: &str, thread_id: &str, title: Option<String>) -> Result<(String, bool), Error> {
    let check = |ses: aiw_core::api::Session| {
        if ses.agent_id == agent {
            Ok((ses.id, false))
        } else {
            Err(Error::Conflict(format!("il thread {thread_id} appartiene a una sessione dell'agente {}, non di {agent}", ses.agent_id)))
        }
    };
    match s.engine.session(thread_id) {
        Ok(ses) => return check(ses),
        Err(Error::NotFound(_)) => {}
        Err(e) => return Err(e),
    }
    // Lookup and creation under one lock (no await inside): two first requests of one thread
    // cannot create two sessions.
    let mut map = s.agui_threads.lock().unwrap();
    if let Some(sid) = map.get(thread_id).cloned() {
        match s.engine.session(&sid) {
            Ok(ses) => return check(ses),
            // The session was deleted meanwhile: the thread starts over.
            Err(Error::NotFound(_)) => {
                map.remove(thread_id);
            }
            Err(e) => return Err(e),
        }
    }
    let ses = s.engine.create_session(agent, title)?;
    map.insert(thread_id.to_string(), ses.id.clone());
    Ok((ses.id, true))
}

/// Cancels the run when dropped while armed: in AG-UI, closing the request is how a client stops a run.
struct CancelGuard {
    engine: Engine,
    run_id: String,
    armed: bool,
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.engine.cancel_run(&self.run_id);
        }
    }
}

/// Where the stream of one run is: the events still to send, the next seq expected from the bus.
struct Follow {
    rx: tokio::sync::broadcast::Receiver<aiw_engine::RunEventMsg>,
    engine: Engine,
    run_id: String,
    thread_id: String,
    client_run_id: String,
    queue: VecDeque<SeqEvent>,
    next_seq: u64,
    finished: bool,
    guard: CancelGuard,
}

impl Follow {
    /// The next event of the run, `None` once the terminal one has been delivered.
    async fn next(&mut self) -> Option<SeqEvent> {
        loop {
            if self.finished {
                return None;
            }
            if let Some(e) = self.queue.pop_front() {
                self.next_seq = e.seq + 1;
                if e.event.event.is_terminal() {
                    self.finished = true;
                    self.guard.armed = false;
                }
                return Some(e);
            }
            match self.rx.recv().await {
                Ok(m) if m.run_id == self.run_id && m.seq >= self.next_seq => self.queue.push_back(SeqEvent { seq: m.seq, event: m.event }),
                Ok(_) => {}
                // The bus overran this reader: what it missed is still in the run log.
                Err(RecvError::Lagged(_)) => {
                    let next = self.next_seq;
                    self.queue.extend(self.engine.run_events(&self.run_id).into_iter().filter(|e| e.seq >= next));
                }
                Err(RecvError::Closed) => return None,
            }
        }
    }

    /// The SSE frame: the event of `/api/runs/{id}/events` with the thread and run ids of the client.
    fn frame(&self, e: &SeqEvent) -> Event {
        let mut v = serde_json::to_value(&e.event).unwrap_or(Value::Null);
        if matches!(v.get("type").and_then(Value::as_str), Some("RUN_STARTED" | "RUN_FINISHED")) {
            v["threadId"] = Value::String(self.thread_id.clone());
            v["runId"] = Value::String(self.client_run_id.clone());
        }
        Event::default().id(e.seq.to_string()).data(v.to_string())
    }
}

/// `POST /agui/{agent}`: starts a run from a `RunAgentInput` and streams its AG-UI events.
pub(crate) async fn run_agent(
    State(s): State<AppState>,
    Path(agent): Path<String>,
    Json(input): Json<RunAgentInput>,
) -> Result<Response, ApiError> {
    s.engine.agent(&agent)?;
    if input.thread_id.trim().is_empty() || input.run_id.trim().is_empty() {
        return Err(invalid("threadId e runId non possono essere vuoti").into());
    }
    let last_user =
        input.messages.iter().rev().find(|m| m.role == "user").ok_or_else(|| invalid("nessun messaggio dell'utente nella richiesta"))?;
    let (text, uploads) = parse_user_content(&last_user.content)?;
    if text.trim().is_empty() && uploads.is_empty() {
        return Err(invalid("il messaggio dell'utente è vuoto").into());
    }
    let (session_id, created) = resolve_session(&s, &agent, &input.thread_id, title_of(&text))?;
    // Subscribe BEFORE the run starts: the bus keeps no history, and what the run emits in between
    // is also in the run log, which is read right after (the `seq` tells the two apart).
    let rx = s.engine.subscribe();
    let run_id = match s.engine.start_run_attach(&session_id, text, None, None, uploads).await {
        Ok(id) => id,
        Err(e) => {
            if created {
                // Nothing was said in a session that was just made for this request: do not leave it behind.
                let _ = s.engine.delete_session(&session_id);
                s.agui_threads.lock().unwrap().remove(&input.thread_id);
            }
            return Err(e.into());
        }
    };
    let queue: VecDeque<SeqEvent> = s.engine.run_events(&run_id).into();
    let follow = Follow {
        rx,
        engine: s.engine.clone(),
        run_id: run_id.clone(),
        thread_id: input.thread_id,
        client_run_id: input.run_id,
        queue,
        next_seq: 0,
        finished: false,
        guard: CancelGuard { engine: s.engine.clone(), run_id: run_id.clone(), armed: true },
    };
    let events = stream::unfold(follow, |mut f| async move {
        let e = f.next().await?;
        let frame = f.frame(&e);
        Some((Ok::<_, std::convert::Infallible>(frame), f))
    });
    let sse = Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)));
    let header = |v: &str| HeaderValue::from_str(v).unwrap_or(HeaderValue::from_static(""));
    let mut res = sse.into_response();
    res.headers_mut().insert(HeaderName::from_static("x-aiw-session-id"), header(&session_id));
    res.headers_mut().insert(HeaderName::from_static("x-aiw-run-id"), header(&run_id));
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_security::{fixture, req, send, LAN};
    use aiw_runtimes::ScriptedBackend;
    use axum::http::StatusCode;
    use axum::Router;
    use http_body_util::BodyExt;
    use serde_json::json;

    fn input(thread: &str, run: &str, content: Value) -> Value {
        json!({
            "threadId": thread, "runId": run,
            "messages": [{"id": "m1", "role": "user", "content": content}],
            "tools": [], "context": [], "state": {}, "forwardedProps": {}
        })
    }

    async fn post_raw(app: &Router, agent: &str, body: Value, headers: &[(&str, &str)]) -> Response {
        tower::ServiceExt::oneshot(app.clone(), req("POST", &format!("/agui/{agent}"), None, headers, Some(body))).await.unwrap()
    }

    /// Reads the body until `needle` shows up (the stream stays open), with a deadline.
    async fn read_until(body: &mut axum::body::Body, needle: &str) -> String {
        let mut got = String::new();
        let read = async {
            while !got.contains(needle) {
                let frame = body.frame().await.expect("the stream ended before the expected event").unwrap();
                if let Ok(data) = frame.into_data() {
                    got.push_str(&String::from_utf8_lossy(&data));
                }
            }
        };
        if tokio::time::timeout(Duration::from_secs(10), read).await.is_err() {
            panic!("no {needle} in: {got}");
        }
        got
    }

    /// The whole body of a stream that ends by itself.
    async fn read_all(res: Response) -> String {
        let bytes = tokio::time::timeout(Duration::from_secs(10), axum::body::to_bytes(res.into_body(), 1 << 22))
            .await
            .expect("the stream ends with the run")
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn frames(body: &str) -> Vec<Value> {
        body.lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(|d| serde_json::from_str(d.trim()).unwrap_or_else(|e| panic!("data line is not JSON ({e}): {d}")))
            .collect()
    }

    fn header(res: &Response, name: &str) -> String {
        res.headers().get(name).unwrap_or_else(|| panic!("missing header {name}")).to_str().unwrap().to_string()
    }

    /// Runs one request to the end: (status, session id, run id, events).
    async fn run(app: &Router, agent: &str, body: Value) -> (StatusCode, String, String, Vec<Value>) {
        let res = post_raw(app, agent, body, &[]).await;
        let status = res.status();
        if status != StatusCode::OK {
            let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
            return (status, String::new(), String::new(), vec![serde_json::from_slice(&bytes).unwrap()]);
        }
        let (sid, rid) = (header(&res, "x-aiw-session-id"), header(&res, "x-aiw-run-id"));
        (status, sid, rid, frames(&read_all(res).await))
    }

    async fn failure(app: &Router, agent: &str, body: Value) -> (StatusCode, String) {
        let res = post_raw(app, agent, body, &[]).await;
        let status = res.status();
        assert_eq!(res.headers().get("content-type").unwrap(), "application/json", "{status}");
        let v: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
        (status, v["error"].as_str().unwrap_or_else(|| panic!("no error text: {v}")).to_string())
    }

    async fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
        for _ in 0..250 {
            if cond() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timeout: {what}");
    }

    /// A model that asks to write a file: with the `operator` agent (mode `ask`) the run waits for an answer.
    fn write_turns() -> Vec<Vec<aiw_runtimes::ChatDelta>> {
        vec![
            ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "nota.md", "content": "ciao"})),
            ScriptedBackend::text_turn("Fatto"),
        ]
    }

    #[tokio::test]
    async fn a_run_streams_ag_ui_events_with_the_ids_of_the_client() {
        let (app, e, _d, _w) = fixture(vec![ScriptedBackend::text_turn("Ciao AG-UI")]).await;
        let res = post_raw(&app, "planner", input("thr-1", "run-client-1", json!("Saluta")), &[]).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(header(&res, "content-type").starts_with("text/event-stream"));
        let (sid, rid) = (header(&res, "x-aiw-session-id"), header(&res, "x-aiw-run-id"));
        assert!(!sid.is_empty() && rid.starts_with("run"), "{sid} {rid}");
        let body = read_all(res).await;
        let evs = frames(&body);
        let types: Vec<&str> = evs.iter().map(|v| v["type"].as_str().unwrap()).collect();
        assert_eq!(types.first(), Some(&"RUN_STARTED"));
        assert_eq!(types.last(), Some(&"RUN_FINISHED"));
        for t in ["TEXT_MESSAGE_START", "TEXT_MESSAGE_CONTENT", "TEXT_MESSAGE_END"] {
            assert!(types.contains(&t), "{t} in {types:?}");
        }
        for ev in [&evs[0], evs.last().unwrap()] {
            assert_eq!((ev["threadId"].as_str(), ev["runId"].as_str()), (Some("thr-1"), Some("run-client-1")), "{ev}");
        }
        let text: String = evs.iter().filter(|v| v["type"] == "TEXT_MESSAGE_CONTENT").map(|v| v["delta"].as_str().unwrap()).collect();
        assert_eq!(text, "Ciao AG-UI");
        // The stream is the whole log of the run, in order, with `id:` = seq; only the ids of RUN_* change.
        let log = e.run_events(&rid);
        assert_eq!(evs.len(), log.len(), "no event lost between the start of the run and the subscription");
        let ids: Vec<u64> = body.lines().filter_map(|l| l.strip_prefix("id:")).map(|v| v.trim().parse().unwrap()).collect();
        assert_eq!(ids, log.iter().map(|l| l.seq).collect::<Vec<_>>());
        for (got, want) in evs.iter().zip(&log).skip(1).take(evs.len() - 2) {
            assert_eq!(got, &serde_json::to_value(&want.event).unwrap());
        }
        assert!(e.run_done(&rid));
    }

    #[tokio::test]
    async fn a_thread_continues_in_the_same_session() {
        let (app, e, _d, _w) = fixture(vec![ScriptedBackend::text_turn("Uno"), ScriptedBackend::text_turn("Due")]).await;
        let (st, sid1, _, evs) = run(&app, "planner", input("thr-2", "r1", json!("Primo"))).await;
        assert_eq!((st, evs.last().unwrap()["type"].as_str()), (StatusCode::OK, Some("RUN_FINISHED")));
        wait_for("history of the first run", || e.messages(&sid1).map(|m| m.len() == 2).unwrap_or(false)).await;
        let (st, sid2, _, evs) = run(&app, "planner", input("thr-2", "r2", json!("Secondo"))).await;
        assert_eq!((st, evs.last().unwrap()["type"].as_str()), (StatusCode::OK, Some("RUN_FINISHED")));
        assert_eq!(sid1, sid2);
        assert_eq!(evs[0]["runId"], "r2");
        assert_eq!(e.sessions().unwrap().len(), 1, "one session for the whole thread");
        wait_for("history of the second run", || e.messages(&sid1).map(|m| m.len() == 4).unwrap_or(false)).await;
        let msgs = e.messages(&sid1).unwrap();
        assert_eq!((msgs[0].content.as_str(), msgs[2].content.as_str()), ("Primo", "Secondo"));
        assert_eq!(e.session(&sid1).unwrap().title, "Primo");
    }

    #[tokio::test]
    async fn a_thread_id_equal_to_a_session_id_uses_that_session_and_the_agent_must_match() {
        let (app, e, _d, _w) = fixture(vec![ScriptedBackend::text_turn("Ok"), ScriptedBackend::text_turn("Ok")]).await;
        let ses = e.create_session("planner", Some("Esistente".into())).unwrap();
        let (st, sid, _, evs) = run(&app, "planner", input(&ses.id, "r1", json!("ciao"))).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!((sid.as_str(), evs[0]["threadId"].as_str()), (ses.id.as_str(), Some(ses.id.as_str())));
        assert_eq!(e.sessions().unwrap().len(), 1);
        // The session belongs to planner.
        let (st, msg) = failure(&app, "coder", input(&ses.id, "r2", json!("ciao"))).await;
        assert_eq!(st, StatusCode::CONFLICT, "{msg}");
        assert!(msg.contains("planner") && msg.contains("coder"), "{msg}");
        // The same for a thread known only through the in-memory map; another thread is another session.
        let (_, mapped, _, _) = run(&app, "planner", input("thr-map", "r3", json!("ciao"))).await;
        assert_ne!(mapped, ses.id);
        wait_for("run done", || e.active_run(&mapped).is_none()).await;
        let (st, _) = failure(&app, "coder", input("thr-map", "r4", json!("ciao"))).await;
        assert_eq!(st, StatusCode::CONFLICT);
        assert_eq!(e.sessions().unwrap().len(), 2, "a refused request creates nothing");
    }

    #[tokio::test]
    async fn bad_requests_are_json_errors_before_the_stream() {
        let (app, e, _d, _w) = fixture(vec![]).await;
        let (st, msg) = failure(&app, "non-esiste", input("t", "r", json!("ciao"))).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{msg}");
        // No user message (an assistant one only, and an empty list).
        let only_assistant = json!({"threadId": "t", "runId": "r", "messages": [{"id": "m", "role": "assistant", "content": "ciao"}]});
        let (st, msg) = failure(&app, "planner", only_assistant).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{msg}");
        assert!(msg.contains("utente"), "{msg}");
        let (st, _) = failure(&app, "planner", json!({"threadId": "t", "runId": "r", "messages": []})).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        // Required fields, and a part of a type that is not supported (named in the message).
        let (st, msg) = failure(&app, "planner", json!({"runId": "r", "messages": []})).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{msg}");
        assert!(msg.contains("threadId"), "{msg}");
        let parts = |p: Value| input("t", "r", json!([{"type": "text", "text": "ciao"}, p]));
        let (st, msg) = failure(&app, "planner", parts(json!({"type": "hologram", "x": 1}))).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert!(msg.contains("hologram"), "{msg}");
        let url_only = json!({"type": "binary", "mimeType": "image/png", "url": "https://x.example/a.png"});
        let (st, msg) = failure(&app, "planner", parts(url_only)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert!(msg.contains("binary") && msg.contains("inline"), "{msg}");
        let by_url = json!({"type": "image", "source": {"type": "url", "value": "https://x.example/a.png"}});
        let (st, msg) = failure(&app, "planner", parts(by_url)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert!(msg.contains("image") && msg.contains("url"), "{msg}");
        // Undecodable base64 is refused by the engine; the session made for the request does not stay behind.
        let bad = json!({"type": "binary", "mimeType": "text/plain", "data": "@@non base64@@", "filename": "a.txt"});
        let (st, msg) = failure(&app, "planner", parts(bad)).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{msg}");
        assert!(e.sessions().unwrap().is_empty(), "no session left by refused requests");
        // Not JSON at all: the API's own error shape.
        let res = tower::ServiceExt::oneshot(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/agui/planner")
                .header("content-type", "application/json")
                .body(axum::body::Body::from("{non json"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn a_session_with_an_active_run_is_a_conflict() {
        let (app, e, _d, _w) = fixture(write_turns()).await;
        let res = post_raw(&app, "operator", input("thr-busy", "r1", json!("scrivi")), &[]).await;
        assert_eq!(res.status(), StatusCode::OK);
        let rid = header(&res, "x-aiw-run-id");
        let mut body = res.into_body();
        read_until(&mut body, "aiw.approval_request").await;
        let (st, msg) = failure(&app, "operator", input("thr-busy", "r2", json!("ancora"))).await;
        assert_eq!(st, StatusCode::CONFLICT, "{msg}");
        assert_eq!(e.sessions().unwrap().len(), 1, "the refused request did not delete or add sessions");
        drop(body);
        wait_for("the first run is cancelled", || e.run_done(&rid)).await;
    }

    #[tokio::test]
    async fn closing_the_connection_before_the_end_cancels_the_run() {
        let (app, e, _d, w) = fixture(write_turns()).await;
        let res = post_raw(&app, "operator", input("thr-cancel", "r1", json!("scrivi nota.md")), &[]).await;
        assert_eq!(res.status(), StatusCode::OK);
        let rid = header(&res, "x-aiw-run-id");
        let mut body = res.into_body();
        // The run stops on the confirmation: approvals travel as CUSTOM events, unchanged.
        let seen = read_until(&mut body, "aiw.approval_request").await;
        assert!(seen.contains("RUN_STARTED") && seen.contains("\"type\":\"CUSTOM\""), "{seen}");
        assert!(!e.run_done(&rid), "the run waits for an answer");
        assert_eq!(e.approvals().len(), 1);
        drop(body); // the client goes away
        wait_for("the run is cancelled", || e.run_done(&rid)).await;
        let last = e.run_events(&rid).pop().expect("events").event.event;
        assert!(last.is_terminal(), "{last:?}");
        assert!(!matches!(last, aiw_core::events::AgEvent::RunFinished { .. }), "cancelled, not finished: {last:?}");
        wait_for("the confirmation is withdrawn", || e.approvals().is_empty()).await;
        assert!(!w.path().join("nota.md").exists(), "the tool did not run");
    }

    #[tokio::test]
    async fn a_run_that_ends_normally_is_not_cancelled_by_the_guard() {
        let (app, e, _d, _w) = fixture(vec![ScriptedBackend::text_turn("Fine")]).await;
        let (_, _, rid, evs) = run(&app, "planner", input("thr-end", "r1", json!("ciao"))).await;
        assert_eq!(evs.last().unwrap()["type"], "RUN_FINISHED");
        tokio::time::sleep(Duration::from_millis(100)).await;
        let last = e.run_events(&rid).pop().unwrap().event.event;
        assert!(matches!(last, aiw_core::events::AgEvent::RunFinished { .. }), "{last:?}");
    }

    #[tokio::test]
    async fn binary_parts_become_attachments_of_the_message() {
        use base64::Engine as _;
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let (app, e, _d, _w) = fixture(vec![ScriptedBackend::text_turn("Letto"), ScriptedBackend::text_turn("Letto")]).await;
        let content = json!([
            {"type": "text", "text": "Guarda"},
            {"type": "binary", "mimeType": "text/plain", "data": b64(b"ciao mondo"), "filename": "ciao.txt"},
            {"type": "document", "source": {"type": "data", "value": b64(b"# titolo"), "mimeType": "text/markdown"}, "metadata": {"filename": "nota.md"}},
            {"type": "text", "text": "questi file"}
        ]);
        let (st, sid, _, evs) = run(&app, "planner", input("thr-att", "r1", content)).await;
        assert_eq!((st, evs.last().unwrap()["type"].as_str()), (StatusCode::OK, Some("RUN_FINISHED")));
        wait_for("the message is stored", || e.messages(&sid).map(|m| !m.is_empty()).unwrap_or(false)).await;
        let msgs = e.messages(&sid).unwrap();
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "Guarda\nquesti file", "text parts are joined");
        let atts = msgs[0].attachments.clone().expect("attachments");
        assert_eq!(atts.len(), 2, "{atts:?}");
        assert_eq!((atts[0].name.as_str(), atts[0].mime.as_str(), atts[0].size), ("ciao.txt", "text/plain", 10));
        assert_eq!((atts[1].name.as_str(), atts[1].mime.as_str()), ("nota.md", "text/markdown"));
        let data = e.attachment_data(&sid, &atts[0].id).unwrap();
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(data.data).unwrap(), b"ciao mondo");
        // Only files, no text: allowed, like the REST route.
        let only_file = json!([{"type": "binary", "mimeType": "text/plain", "data": b64(b"solo file")}]);
        let (_, sid2, _, _) = run(&app, "planner", input("thr-att2", "r2", only_file)).await;
        wait_for("the second message is stored", || e.messages(&sid2).map(|m| !m.is_empty()).unwrap_or(false)).await;
        assert_eq!(e.messages(&sid2).unwrap()[0].attachments.as_ref().unwrap().len(), 1);
    }

    /// A request from the LAN with a token, over TLS (the way a paired device or a third-party app comes in).
    async fn with_token(app: &Router, token: &str, body: Value) -> (StatusCode, Response) {
        let b = format!("Bearer {token}");
        let mut r = req("POST", "/agui/planner", Some(LAN), &[("host", "192.168.1.5:7777"), ("authorization", b.as_str())], Some(body));
        r.extensions_mut().insert(crate::security::TlsConn);
        let res = tower::ServiceExt::oneshot(app.clone(), r).await.unwrap();
        (res.status(), res)
    }

    #[tokio::test]
    async fn a_gateway_token_is_not_enough_a_device_token_is() {
        let (app, e, _d, _w) = fixture(vec![ScriptedBackend::text_turn("Ok")]).await;
        let gw = e.create_api_token("App", "gateway", None, None, None).unwrap().token;
        let dev = e.create_api_token("Telefono", "device", None, None, None).unwrap().token;
        let (st, res) = with_token(&app, &gw, input("t", "r", json!("ciao"))).await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        let v: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
        assert!(v["error"].is_string(), "{v}");
        assert!(e.sessions().unwrap().is_empty(), "the refused request started nothing");
        // No token at all from the LAN: refused too.
        let mut r = req("POST", "/agui/planner", Some(LAN), &[("host", "192.168.1.5:7777")], Some(input("t", "r", json!("ciao"))));
        r.extensions_mut().insert(crate::security::TlsConn);
        assert_eq!(send(&app, r).await.0, StatusCode::UNAUTHORIZED);
        let (st, res) = with_token(&app, &dev, input("t", "r", json!("ciao"))).await;
        assert_eq!(st, StatusCode::OK);
        let evs = frames(&read_all(res).await);
        assert_eq!(evs.last().unwrap()["type"], "RUN_FINISHED");
    }

    #[tokio::test]
    async fn the_route_is_guarded_and_wrong_requests_are_json_404_405() {
        use crate::security::is_guarded;
        assert!(is_guarded("/agui/planner"));
        assert!(!is_guarded("/agui") && !is_guarded("/aguix/planner"));
        assert!(crate::scopes::required_scope("POST", "/agui/planner").is_some());
        assert!(crate::scopes::required_scope("GET", "/agui/planner").is_none(), "only POST is classified: the rest needs the owner");
        let (_, e, _d, _w) = fixture(vec![]).await;
        let ui = tempfile::tempdir().unwrap();
        std::fs::write(ui.path().join("index.html"), "<!doctype html><title>AIW</title>").unwrap();
        for app in [crate::router(e.clone(), None), crate::router(e.clone(), Some(ui.path().into()))] {
            let (st, h, v) = send(&app, req("GET", "/agui/planner", None, &[], None)).await;
            assert_eq!(st, StatusCode::METHOD_NOT_ALLOWED);
            assert_eq!(h.get("content-type").unwrap(), "application/json");
            assert!(v["error"].is_string());
            for (method, uri) in [("POST", "/agui/planner/extra"), ("GET", "/agui"), ("POST", "/agui")] {
                let (st, h, v) = send(&app, req(method, uri, None, &[], Some(json!({})))).await;
                assert_eq!(st, StatusCode::NOT_FOUND, "{method} {uri}");
                assert_eq!(h.get("content-type").unwrap(), "application/json", "{method} {uri}");
                assert_eq!(v["error"], "percorso non trovato");
            }
        }
    }

    #[test]
    fn parses_the_content_of_a_user_message() {
        assert_eq!(parse_user_content(&json!("ciao")).unwrap(), ("ciao".to_string(), vec![]));
        let (t, up) = parse_user_content(&json!([
            {"type": "text", "text": "a"},
            {"type": "binary", "mimeType": "image/png", "data": "AAAA"},
            {"type": "image", "source": {"type": "data", "value": "BBBB", "mimeType": "image/jpeg"}, "metadata": {"name": "foto.jpg"}},
            {"type": "text", "text": "b"}
        ]))
        .unwrap();
        assert_eq!(t, "a\nb");
        assert_eq!(up.len(), 2);
        assert_eq!((up[0].name.as_str(), up[0].mime.as_str(), up[0].data.as_str()), ("allegato-1", "image/png", "AAAA"));
        assert_eq!((up[1].name.as_str(), up[1].mime.as_str()), ("foto.jpg", "image/jpeg"));
        let bad_parts = [
            json!(null),
            json!(7),
            json!([{"text": "x"}]),
            json!([{"type": "text"}]),
            json!([{"type": "audio", "source": {"type": "file", "value": "f"}}]),
        ];
        for bad in bad_parts {
            assert!(matches!(parse_user_content(&bad), Err(Error::Invalid(_))), "{bad}");
        }
        assert_eq!(title_of("\n  Riordina i PDF \nin Downloads").as_deref(), Some("Riordina i PDF"));
        assert_eq!(title_of("  "), None);
        assert_eq!(title_of(&"x".repeat(100)).unwrap().chars().count(), 60);
    }
}
