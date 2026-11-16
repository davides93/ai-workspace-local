//! AG-UI entry point: `POST /agui/{agent}` takes a `RunAgentInput` and answers with the SSE stream of
//! the run, which is what an AG-UI client (`HttpAgent` of `@ag-ui/client`, CopilotKit) expects. The
//! events themselves are the ones of the run (ADR 0004); `agui_wire` turns them into what a conformant
//! client takes, and this module maps the request onto a session and a run and follows the run on the bus.
//! See `docs/agui.md`.
//!
//! What a request means:
//! - the text and the files of the LAST user message start the run; the history is the one the server
//!   keeps for the session (it is authoritative: a client that comes without it, or with another one, is
//!   sent a `MESSAGES_SNAPSHOT`), and a message the server has run (same `id`, text and files) is not run
//!   twice: the run it started is sent again;
//! - `context` and `state` go to the model as data for this run (see `agui_wire::app_context_section`), and the
//!   client's `state` is kept in every `STATE_SNAPSHOT`; `tools` (client-defined) and `parentRunId` are accepted
//!   and ignored;
//! - a client that says it can resume (`forwardedProps.aiw.interactive` has `"interrupts"`) gets a pause on a
//!   confirmation or a question as `RUN_FINISHED` with an interrupt outcome, and continues with `resume`;
//! - of `forwardedProps` only `aiw.interactive` is read: the questions and secret requests the client can show.

use crate::agui_wire::{self, Wire};
use crate::extract::{Json, Path};
use crate::{ApiError, AppState, IsOwner};
use aiw_core::api::{AttachmentUpload, QuestionAnswer, QuestionReply};
use aiw_core::events::{AgEvent, SeqEvent};
use aiw_core::Error;
use aiw_engine::agui_threads::{AguiRun, AguiThread};
use aiw_engine::Engine;
use axum::extract::State;
use axum::http::{HeaderName, HeaderValue};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast::error::RecvError;

/// What the endpoint keeps in memory: a lock for the lookups of threads (the record itself is in the store, see
/// `aiw_engine::agui_threads`), the runs that are paused on an interrupt, and how many response streams follow each run.
#[derive(Default)]
pub(crate) struct AguiState {
    threads: Mutex<()>,
    pauses: Mutex<HashMap<String, Slot>>,
    followers: Mutex<HashMap<String, usize>>,
}

pub(crate) type Threads = Arc<AguiState>;

/// Most pauses kept, and how long. A pause nobody answers for [`PAUSE_TTL`] (or the oldest one over the limit) is dropped and
/// its run is cancelled: it would otherwise hold its session for ever.
const MAX_PAUSES: usize = 256;
const PAUSE_TTL: Duration = Duration::from_secs(24 * 3600);
/// Answered pauses kept per thread, so that an answer sent again (a response that got lost) after the run paused once more is
/// still recognised.
const MAX_ANSWERED: usize = 8;
/// How long the stream waits after the first interrupt for the others of the same moment (the questions and the subagents of
/// one turn run together).
const SETTLE: Duration = Duration::from_millis(150);
/// Longest `threadId` and `runId`, and largest `state` of the client (serialized), in bytes.
const MAX_ID_LEN: usize = 256;
const MAX_STATE_BYTES: usize = 1 << 20;

/// The pause a thread waits on, and the ones before it that were answered.
struct Slot {
    current: Pause,
    answered: Vec<Pause>,
}

/// Drops the pauses that nobody answered in time, and cancels their runs (a run that is waiting for an answer that will not come).
fn drop_stale(pauses: &mut HashMap<String, Slot>, engine: &Engine) {
    let stale: Vec<String> = pauses.iter().filter(|(_, sl)| sl.current.since.elapsed() >= PAUSE_TTL).map(|(k, _)| k.clone()).collect();
    for k in stale {
        if let Some(sl) = pauses.remove(&k) {
            cancel_unanswered(&sl.current, engine);
        }
    }
}

fn cancel_unanswered(p: &Pause, engine: &Engine) {
    if p.applied.is_none() && !engine.run_done(&p.run_id) {
        let _ = engine.cancel_run(&p.run_id);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Approval,
    Question,
}

#[derive(Clone, Debug)]
struct OpenInterrupt {
    id: String,
    kind: Kind,
}

/// A run that stopped on `RUN_FINISHED` with an interrupt outcome and waits for `resume`.
#[derive(Clone)]
struct Pause {
    /// The run of the engine.
    run_id: String,
    /// The `runId` of the client that was interrupted (the `parentRunId` of the one that resumes).
    client_run_id: String,
    /// The first event of the run that has not been sent.
    next_seq: u64,
    wire: Wire,
    interrupts: Vec<OpenInterrupt>,
    /// The answer that was applied, once there was one: a replay of the same resume is safe, another is a conflict.
    applied: Option<String>,
    since: Instant,
}

/// Body of the request (camelCase). Unknown fields are tolerated; of `forwardedProps` only `aiw.interactive` is read.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunAgentInput {
    thread_id: String,
    run_id: String,
    messages: Vec<InMessage>,
    /// The shared state of the client: an object is kept in every snapshot and given to the model; anything else is no state.
    #[serde(default)]
    state: Value,
    /// `[{description, value}]`, given to the model as data.
    #[serde(default)]
    context: Value,
    #[serde(default)]
    forwarded_props: Value,
    /// The answers to the interrupts of the run that paused.
    #[serde(default)]
    resume: Option<Vec<ResumeEntry>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResumeEntry {
    interrupt_id: String,
    status: String,
    #[serde(default)]
    payload: Option<Value>,
}

/// What the client says it can show, from `forwardedProps.aiw.interactive`: a list that may hold
/// `"questions"` (it shows `aiw.question` and answers on the REST endpoint), `"secrets"` (the same for
/// `aiw.secret_request`) and `"interrupts"` (it takes a pause as `RUN_FINISHED` with an interrupt outcome and
/// continues with `resume`). Anything else, or nothing, is none of them: the agent then asks in plain text, and
/// says that a secret has to be entered in the app.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Declared {
    questions: bool,
    secrets: bool,
    interrupts: bool,
}

fn declared_interactive(props: &Value) -> Declared {
    let list = props.get("aiw").and_then(|a| a.get("interactive")).and_then(Value::as_array);
    let has = |name: &str| list.is_some_and(|l| l.iter().any(|v| v.as_str() == Some(name)));
    Declared { questions: has("questions"), secrets: has("secrets"), interrupts: has("interrupts") }
}

#[derive(Deserialize)]
struct InMessage {
    #[serde(default)]
    id: Option<String>,
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

/// The thread of a `threadId` and whether this call created its session. A `threadId` that is the id of an existing
/// session uses it; otherwise the record in the store decides (it survives a restart), and a thread not seen before gets
/// a new session. A session of another agent is a conflict.
fn resolve_thread(s: &AppState, agent: &str, thread_id: &str, title: Option<String>) -> Result<(AguiThread, bool), Error> {
    let check = |ses: &aiw_core::api::Session| {
        if ses.agent_id == agent {
            Ok(())
        } else {
            Err(Error::Conflict(format!("il thread {thread_id} appartiene a una sessione dell'agente {}, non di {agent}", ses.agent_id)))
        }
    };
    // Lookup and creation under one lock (no await inside): two first requests of one thread cannot create two sessions.
    let _guard = s.agui.threads.lock().unwrap();
    let record = s.engine.agui_thread(thread_id)?;
    match s.engine.session(thread_id) {
        Ok(ses) => {
            check(&ses)?;
            let t = record.filter(|t| t.session_id == ses.id).unwrap_or_else(|| AguiThread::new(thread_id, &ses.id, agent));
            return Ok((t, false));
        }
        Err(Error::NotFound(_)) => {}
        Err(e) => return Err(e),
    }
    if let Some(t) = record {
        match s.engine.session(&t.session_id) {
            Ok(ses) => {
                check(&ses)?;
                return Ok((t, false));
            }
            // The session was deleted meanwhile: the thread starts over.
            Err(Error::NotFound(_)) => s.engine.remove_agui_thread(thread_id)?,
            Err(e) => return Err(e),
        }
    }
    let ses = s.engine.create_session(agent, title)?;
    let t = AguiThread::new(thread_id, &ses.id, agent);
    s.engine.save_agui_thread(&t)?;
    Ok((t, true))
}

/// Cancels the run when the last stream that follows it goes away while armed: in AG-UI, closing the request is how a client
/// stops a run. A stream that is sent again for a run (a retry) must not be cancelled by the old one that was cut.
struct CancelGuard {
    engine: Engine,
    state: Threads,
    run_id: String,
    armed: bool,
}

impl CancelGuard {
    fn new(state: Threads, engine: Engine, run_id: &str, armed: bool) -> Self {
        *state.followers.lock().unwrap().entry(run_id.to_string()).or_default() += 1;
        Self { engine, state, run_id: run_id.to_string(), armed }
    }
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        let last = {
            let mut followers = self.state.followers.lock().unwrap();
            let left = followers.get_mut(&self.run_id).map(|n| {
                *n = n.saturating_sub(1);
                *n
            });
            if left.unwrap_or(0) == 0 {
                followers.remove(&self.run_id);
            }
            left.unwrap_or(0) == 0
        };
        if self.armed && last {
            let _ = self.engine.cancel_run(&self.run_id);
        }
    }
}

/// One SSE frame: the event, and the position in the run log of the event it comes from.
struct Frame {
    seq: Option<u64>,
    data: Value,
}

/// What a stream needs to pause on an interrupt: where to record the pause and how to build the snapshot of the messages.
struct PauseCtx {
    state: Threads,
    thread_id: String,
    session_id: String,
    /// What the client called its user messages (server id → client id), for `MESSAGES_SNAPSHOT`.
    client_ids: BTreeMap<String, String>,
    /// The interrupts are the answer to a pause that already happened in this run: their ids are not announced again as `aiw.*` events.
    managed: HashSet<String>,
    /// Pause on confirmations and questions (the client declared `"interrupts"`).
    interrupts: bool,
    /// Questions go as interrupts too (the client did not declare that it shows `aiw.question`).
    questions_as_interrupts: bool,
}

/// Where the stream of one run is: the events still to send, the next seq expected from the bus.
struct Follow {
    rx: tokio::sync::broadcast::Receiver<aiw_engine::RunEventMsg>,
    engine: Engine,
    run_id: String,
    wire: Wire,
    queue: VecDeque<SeqEvent>,
    next_seq: u64,
    finished: bool,
    /// `RUN_STARTED` (or a terminal event) has been seen: nothing is held back any more.
    started: bool,
    /// Events the engine emits before the agent loop sends `RUN_STARTED` (routing, runtime choice,
    /// attachment warnings), held until it arrives so that the stream opens with it.
    held: Vec<SeqEvent>,
    /// Events ready to go out, in order.
    ready: VecDeque<SeqEvent>,
    /// Frames to send, in order.
    out: VecDeque<Frame>,
    /// Frames that go right behind `RUN_STARTED` (a snapshot of the history).
    after_started: Vec<Value>,
    guard: CancelGuard,
    pause: Option<PauseCtx>,
    done: bool,
}

impl Follow {
    /// The next event of the run, `None` once the terminal one has been delivered. AG-UI wants
    /// `RUN_STARTED` first: the `CUSTOM` events that precede it in the run log are sent right behind it
    /// (their `id:` is then out of order, which clients do not rely on). A run that ends before
    /// `RUN_STARTED` is sent as it happened.
    async fn next(&mut self) -> Option<SeqEvent> {
        loop {
            if let Some(e) = self.ready.pop_front() {
                return Some(e);
            }
            let e = self.next_raw().await?;
            if self.started {
                return Some(e);
            }
            match &e.event.event {
                AgEvent::RunStarted { .. } => {
                    self.started = true;
                    self.ready.push_back(e);
                    self.ready.extend(self.held.drain(..));
                }
                ev if ev.is_terminal() => {
                    self.started = true;
                    self.ready.extend(self.held.drain(..));
                    self.ready.push_back(e);
                }
                _ => self.held.push(e),
            }
        }
    }

    /// The next event of the run in log order, `None` once the terminal one has been taken.
    async fn next_raw(&mut self) -> Option<SeqEvent> {
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

    /// The next frame to send, `None` when the stream is over.
    async fn next_frame(&mut self) -> Option<Frame> {
        loop {
            if let Some(f) = self.out.pop_front() {
                return Some(f);
            }
            if self.done {
                return None;
            }
            let Some(e) = self.next().await else {
                self.done = true;
                continue;
            };
            if self.is_interrupt(&e) {
                self.pause_on(e).await;
                continue;
            }
            self.push(&e);
        }
    }

    /// Translates an event into frames; the snapshot of the history goes right behind `RUN_STARTED`.
    fn push(&mut self, e: &SeqEvent) {
        if let Some(pc) = &self.pause {
            // The answer to an interrupt that was already announced on the wire as one: it says nothing new.
            if let AgEvent::Custom { name, value } = &e.event.event {
                let is_closing = matches!(name.as_str(), "aiw.approval_resolved" | "aiw.question_resolved");
                if is_closing && value.get("id").and_then(Value::as_str).is_some_and(|id| pc.managed.contains(id)) {
                    return;
                }
            }
        }
        let is_start = matches!(e.event.event, AgEvent::RunStarted { .. });
        let seq = Some(e.seq);
        for data in self.wire.translate(e) {
            self.out.push_back(Frame { seq, data });
        }
        if is_start {
            for data in std::mem::take(&mut self.after_started) {
                self.out.push_back(Frame { seq: None, data });
            }
        }
        if e.event.event.is_terminal() {
            self.done = true;
        }
    }

    /// Is this event the request of a confirmation or a question that goes out as an interrupt?
    fn is_interrupt(&self, e: &SeqEvent) -> bool {
        let Some(pc) = self.pause.as_ref().filter(|p| p.interrupts) else { return false };
        match &e.event.event {
            AgEvent::Custom { name, .. } => name == "aiw.approval_request" || (name == "aiw.question" && pc.questions_as_interrupts),
            _ => false,
        }
    }

    /// Ends this response with `RUN_FINISHED` and an interrupt outcome for the confirmations and questions that are open: after
    /// the others of the same moment have come in, and after the snapshots of the messages and of the state that the spec asks
    /// for before it. The run itself goes on waiting in the engine; the pause is recorded for the `resume`.
    async fn pause_on(&mut self, first: SeqEvent) {
        let mut found = vec![first];
        let deadline = tokio::time::Instant::now() + SETTLE;
        loop {
            match tokio::time::timeout_at(deadline, self.next()).await {
                Err(_) => break,
                Ok(None) => {
                    self.done = true;
                    break;
                }
                Ok(Some(e)) if self.is_interrupt(&e) => found.push(e),
                Ok(Some(e)) => {
                    self.push(&e);
                    if self.done {
                        break;
                    }
                }
            }
        }
        if self.done {
            // The run ended while the interrupts were being collected (they were answered elsewhere): it is all in the stream.
            return;
        }
        let open_approvals: HashSet<String> = self.engine.approvals().into_iter().map(|a| a.id).collect();
        let open_questions: HashSet<String> = self.engine.questions().into_iter().map(|q| q.id).collect();
        let mut interrupts: Vec<Value> = vec![];
        let mut open: Vec<OpenInterrupt> = vec![];
        // The runs of the subagents that wait on an interrupt, with the id of the interrupt.
        let mut waiting: Vec<(String, String)> = vec![];
        for e in &found {
            let AgEvent::Custom { name, value } = &e.event.event else { continue };
            if name == "aiw.approval_request" {
                let Ok(req) = serde_json::from_value::<aiw_core::api::ApprovalRequest>(value.clone()) else { continue };
                if !open_approvals.contains(&req.id) {
                    continue;
                }
                let call = self.wire.claim_call(&req.tool, &req.args).or_else(|| self.wire.claim_call_named(&req.tool));
                let mut i = json!({
                    "id": req.id,
                    "reason": if call.is_some() { "tool_call" } else { "confirmation" },
                    "message": req.reason,
                    "responseSchema": {"type": "object", "properties": {"approved": {"type": "boolean"}}, "required": ["approved"]},
                    "metadata": {"ag-ui:tool-execution": "server", "aiw": {"tool": req.tool, "args": req.args, "risk": req.risk}},
                });
                if let Some(c) = call {
                    i["toolCallId"] = json!(c);
                }
                // The wait for a confirmation has a limit only when `approvals.timeout_s` sets one.
                if let Some(t) = approval_expiry(&self.engine) {
                    i["expiresAt"] = json!(t);
                }
                if self.wire.is_open_subagent(&req.run_id) {
                    i["subagentRunId"] = json!(req.run_id);
                    waiting.push((req.run_id.clone(), req.id.clone()));
                }
                open.push(OpenInterrupt { id: req.id.clone(), kind: Kind::Approval });
                interrupts.push(i);
            } else if name == "aiw.question" {
                let Ok(q) = serde_json::from_value::<aiw_core::api::Question>(value.clone()) else { continue };
                if !open_questions.contains(&q.id) {
                    continue;
                }
                let call = self.wire.claim_call_named("ask_user");
                let message = match &q.title {
                    Some(t) if !t.trim().is_empty() => format!("{t}\n{}", q.message),
                    _ => q.message.clone(),
                };
                let mut i = json!({
                    "id": q.id,
                    "reason": "input_required",
                    "message": message,
                    "responseSchema": agui_wire::question_schema(&q),
                    "metadata": {"aiw": {"question": value}},
                });
                if let Some(c) = call {
                    i["toolCallId"] = json!(c);
                }
                if let Some(t) = &q.expires_at {
                    i["expiresAt"] = json!(t);
                }
                if self.wire.is_open_subagent(&q.run_id) {
                    i["subagentRunId"] = json!(q.run_id);
                    waiting.push((q.run_id.clone(), q.id.clone()));
                }
                open.push(OpenInterrupt { id: q.id.clone(), kind: Kind::Question });
                interrupts.push(i);
            }
        }
        if open.is_empty() {
            // All of them were answered in the meantime (in the app): the run goes on, and this stream with it.
            return;
        }
        let Some(pc) = self.pause.as_ref() else { return };
        let ts = chrono::Utc::now().timestamp_millis();
        // The client refuses `RUN_FINISHED` while a subagent is open: they are closed as suspended (the spec's outcome for a
        // subagent that waits for outside input) and announced again, with the same ids, when the run is resumed.
        for data in self.wire.suspend_subagents(&waiting, ts) {
            self.out.push_back(Frame { seq: None, data });
        }
        // The spec: the state needed to resume goes out before the interrupt.
        let history = self.engine.messages(&pc.session_id).unwrap_or_default();
        let mut messages = agui_wire::history_messages(&history, &pc.client_ids, None);
        messages.extend(self.wire.messages.iter().cloned());
        self.out.push_back(Frame { seq: None, data: agui_wire::messages_snapshot(messages, ts) });
        let snapshot = self.wire.state_snapshot(ts);
        self.out.push_back(Frame { seq: None, data: snapshot });
        let finished = json!({
            "type": "RUN_FINISHED", "threadId": pc.thread_id, "runId": self.wire.client_run_id(), "timestamp": ts,
            "outcome": {"type": "interrupt", "interrupts": interrupts},
        });
        self.out.push_back(Frame { seq: None, data: finished });
        // The connection that closes now must not cancel the run: it is paused, not abandoned.
        self.guard.armed = false;
        self.done = true;
        let pause = Pause {
            run_id: self.run_id.clone(),
            client_run_id: self.wire.client_run_id().to_string(),
            next_seq: self.next_seq,
            wire: self.wire.clone().light(),
            interrupts: open,
            applied: None,
            since: Instant::now(),
        };
        let mut pauses = pc.state.pauses.lock().unwrap();
        drop_stale(&mut pauses, &self.engine);
        // The one that replaces an answered pause of this thread keeps it as an answered one; an unanswered one is simply replaced.
        let mut answered = match pauses.remove(&pc.thread_id) {
            Some(old) if old.current.applied.is_some() => {
                let mut list = old.answered;
                list.push(old.current);
                list
            }
            Some(old) => old.answered,
            None => vec![],
        };
        let extra = answered.len().saturating_sub(MAX_ANSWERED);
        answered.drain(..extra);
        while pauses.len() >= MAX_PAUSES {
            let Some(oldest) = pauses.iter().min_by_key(|(_, sl)| sl.current.since).map(|(k, _)| k.clone()) else { break };
            if let Some(sl) = pauses.remove(&oldest) {
                cancel_unanswered(&sl.current, &self.engine);
            }
        }
        pauses.insert(pc.thread_id.clone(), Slot { current: pause, answered });
    }

    fn sse(f: Frame) -> Event {
        let ev = Event::default().data(f.data.to_string());
        match f.seq {
            Some(seq) => ev.id(seq.to_string()),
            None => ev,
        }
    }
}

/// The stream of a response made of ready frames (a protocol error: `RUN_STARTED` then `RUN_ERROR`).
fn frames_response(frames: Vec<Value>, headers: &[(&'static str, String)]) -> Response {
    let events = stream::iter(frames.into_iter().map(|d| Ok::<_, std::convert::Infallible>(Event::default().data(d.to_string()))));
    let mut res = Sse::new(events).into_response();
    for (k, v) in headers {
        res.headers_mut().insert(HeaderName::from_static(k), HeaderValue::from_str(v).unwrap_or(HeaderValue::from_static("")));
    }
    res
}

/// A refusal that the protocol wants as an event (`RUN_ERROR`), after a `RUN_STARTED` so that the stream is a valid run.
fn run_error(input: &RunAgentInput, code: &str, message: &str, headers: &[(&'static str, String)]) -> Response {
    let ts = chrono::Utc::now().timestamp_millis();
    frames_response(
        vec![
            json!({"type": "RUN_STARTED", "threadId": input.thread_id, "runId": input.run_id, "timestamp": ts}),
            json!({"type": "RUN_ERROR", "message": message, "code": code, "timestamp": ts}),
        ],
        headers,
    )
}

fn header(v: &str) -> HeaderValue {
    HeaderValue::from_str(v).unwrap_or(HeaderValue::from_static(""))
}

/// The response that follows a run.
fn stream_response(follow: Follow, session_id: &str, run_id: &str) -> Response {
    let events = stream::unfold(follow, |mut f| async move {
        let frame = f.next_frame().await?;
        Some((Ok::<_, std::convert::Infallible>(Follow::sse(frame)), f))
    });
    let sse = Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)));
    let mut res = sse.into_response();
    res.headers_mut().insert(HeaderName::from_static("x-aiw-session-id"), header(session_id));
    res.headers_mut().insert(HeaderName::from_static("x-aiw-run-id"), header(run_id));
    res
}

/// The user message that started the request: the last one with the role `user`, its id, its text and its files.
struct LastUser {
    id: Option<String>,
    text: String,
    uploads: Vec<AttachmentUpload>,
}

fn last_user(input: &RunAgentInput) -> Result<Option<LastUser>, Error> {
    let Some(m) = input.messages.iter().rev().find(|m| m.role == "user") else { return Ok(None) };
    let (text, uploads) = parse_user_content(&m.content)?;
    Ok(Some(LastUser { id: m.id.clone().filter(|i| !i.trim().is_empty()), text, uploads }))
}

/// A `Follow` of a run from `from_seq`, with the translation `wire`.
fn follow_run(s: &AppState, run_id: &str, from_seq: u64, wire: Wire, started: bool, pause: Option<PauseCtx>, armed: bool) -> Follow {
    // Subscribe BEFORE the log is read: the bus keeps no history, and what the run emits in between is also in the
    // run log (the `seq` tells the two apart).
    let rx = s.engine.subscribe();
    let queue: VecDeque<SeqEvent> = s.engine.run_events(run_id).into_iter().filter(|e| e.seq >= from_seq).collect();
    Follow {
        rx,
        engine: s.engine.clone(),
        run_id: run_id.to_string(),
        wire,
        queue,
        next_seq: from_seq,
        finished: false,
        started,
        held: vec![],
        ready: VecDeque::new(),
        out: VecDeque::new(),
        after_started: vec![],
        guard: CancelGuard::new(s.agui.clone(), s.engine.clone(), run_id, armed),
        pause,
        done: false,
    }
}

fn pause_ctx(s: &AppState, thread: &AguiThread, declared: Declared, managed: HashSet<String>) -> PauseCtx {
    PauseCtx {
        state: s.agui.clone(),
        thread_id: thread.thread_id.clone(),
        session_id: thread.session_id.clone(),
        client_ids: thread.user_ids.clone(),
        managed,
        interrupts: declared.interrupts,
        questions_as_interrupts: declared.interrupts && !declared.questions,
    }
}

/// `POST /agui/{agent}`: starts a run from a `RunAgentInput` and streams its AG-UI events.
pub(crate) async fn run_agent(
    State(s): State<AppState>,
    Path(agent): Path<String>,
    IsOwner(owner): IsOwner,
    Json(input): Json<RunAgentInput>,
) -> Result<Response, ApiError> {
    s.engine.agent(&agent)?;
    if input.thread_id.trim().is_empty() || input.run_id.trim().is_empty() {
        return Err(invalid("threadId e runId non possono essere vuoti").into());
    }
    if input.thread_id.len() > MAX_ID_LEN || input.run_id.len() > MAX_ID_LEN {
        return Err(invalid(format!("threadId e runId non possono superare {MAX_ID_LEN} byte")).into());
    }
    if input.state.is_object() && input.state.to_string().len() > MAX_STATE_BYTES {
        return Err(invalid(format!("lo stato condiviso (`state`) supera {} KiB", MAX_STATE_BYTES >> 10)).into());
    }
    let declared = declared_interactive(&input.forwarded_props);
    let user = last_user(&input)?;
    let resuming = input.resume.as_ref().is_some_and(|r| !r.is_empty());
    if user.is_none() && !resuming {
        return Err(invalid("nessun messaggio dell'utente nella richiesta").into());
    }
    if let Some(u) = &user {
        if !resuming && u.text.trim().is_empty() && u.uploads.is_empty() {
            return Err(invalid("il messaggio dell'utente è vuoto").into());
        }
    }
    // An answer for a thread nobody knows has nothing to answer: it makes no session.
    if resuming && s.engine.agui_thread(&input.thread_id)?.is_none() && s.engine.session(&input.thread_id).is_err() {
        let msg = "nessuna interruzione aperta per questo thread (il server è stato riavviato, oppure il thread non esiste)";
        return Ok(run_error(&input, "interrupt_not_found", msg, &[]));
    }
    let title = user.as_ref().and_then(|u| title_of(&u.text));
    let (mut thread, created) = resolve_thread(&s, &agent, &input.thread_id, title)?;

    // 1. The answer to an interrupt.
    if resuming {
        return resume_run(&s, owner, &input, declared, thread, user).await;
    }
    // 2. A message the server has seen: the run it started is sent again, it is not run twice (unless that run failed or was
    // cancelled: then the client wants it done again, and the message stays twice in the history).
    let retry = user
        .as_ref()
        .and_then(|u| Some((u, thread.run_of_message(u.id.as_deref()?)?)))
        .filter(|(u, run)| same_message(&s.engine, &thread.session_id, run, u) && replayable(&s.engine, &run.run_id))
        .map(|(_, run)| run.clone());
    if let Some(run) = retry {
        return Ok(replay_run(&s, &input, declared, thread, run));
    }
    // 3. New input while a run waits for the answer to its interrupts.
    let paused = {
        let mut pauses = s.agui.pauses.lock().unwrap();
        drop_stale(&mut pauses, &s.engine);
        pauses.get(&input.thread_id).map(|sl| sl.current.clone())
    };
    if let Some(p) = paused.filter(|p| p.applied.is_none() && !s.engine.run_done(&p.run_id)) {
        let ids: Vec<&str> = p.interrupts.iter().map(|i| i.id.as_str()).collect();
        let msg =
            format!("il thread ha interruzioni in attesa di risposta ({}): rispondi con `resume` prima di mandare altro", ids.join(", "));
        return Ok(run_error(&input, "interrupt_pending", &msg, &[("x-aiw-session-id", thread.session_id.clone())]));
    }
    s.agui.pauses.lock().unwrap().remove(&input.thread_id);
    // 4. A new run.
    let user = user.expect("a user message was checked above");
    let history = s.engine.messages(&thread.session_id).unwrap_or_default();
    let known: HashSet<&str> = history.iter().map(|m| m.id.as_str()).chain(thread.user_ids.values().map(String::as_str)).collect();
    let client_prefix: Vec<&str> = {
        let at = input.messages.iter().rposition(|m| m.role == "user").unwrap_or(0);
        input.messages[..at].iter().filter_map(|m| m.id.as_deref()).collect()
    };
    // A client that comes without the history of the thread (a page that was loaded again), or with another one, gets the server's.
    let needs_snapshot = !history.is_empty() && !client_prefix.iter().any(|id| known.contains(id));
    let origin = aiw_engine::RunOrigin { owner, questions: declared.questions || declared.interrupts, secrets: declared.secrets && owner };
    let app_context = agui_wire::app_context_section(&input.context, &input.state);
    let run_id =
        match s.engine.start_run_ctx(&thread.session_id, user.text.clone(), None, None, user.uploads, Some(origin), app_context).await {
            Ok(id) => id,
            Err(e) => {
                if created {
                    abandon_new_session(&s, &thread.session_id, &input.thread_id);
                }
                return Err(e.into());
            }
        };
    // The user message the engine stored for this run is the last one of the session: what the client called it is recorded.
    let stored = s.engine.messages(&thread.session_id).unwrap_or_default();
    let server_user = stored.iter().rev().find(|m| m.role == "user").map(|m| m.id.clone()).unwrap_or_default();
    let client_user = user.id.clone().unwrap_or_else(|| server_user.clone());
    record_run(
        &s,
        &mut thread,
        AguiRun {
            client_run_id: input.run_id.clone(),
            run_id: run_id.clone(),
            user_message_id: client_user,
            user_server_id: server_user.clone(),
        },
    );
    let wire = Wire::new(&input.thread_id, &input.run_id, &input.state);
    let mut follow = follow_run(&s, &run_id, 0, wire, false, Some(pause_ctx(&s, &thread, declared, HashSet::new())), true);
    if needs_snapshot {
        let messages = agui_wire::history_messages(&stored, &thread.user_ids, Some(&server_user));
        follow.after_started.extend(agui_wire::history_snapshots(messages, chrono::Utc::now().timestamp_millis()));
    }
    Ok(stream_response(follow, &thread.session_id, &run_id))
}

/// When a request that made its session cannot start its run, the session goes too: nothing was said in it. Unless another request
/// of the same thread got there first and is using it (it has a run or messages): that one is not touched.
fn abandon_new_session(s: &AppState, session_id: &str, thread_id: &str) {
    let _guard = s.agui.threads.lock().unwrap();
    let in_use = s.engine.active_run(session_id).is_some() || s.engine.messages(session_id).map(|m| !m.is_empty()).unwrap_or(true);
    if !in_use {
        let _ = s.engine.delete_session(session_id);
        let _ = s.engine.remove_agui_thread(thread_id);
    }
}

/// When a confirmation stops being answerable, as an RFC 3339 time: `seconds` after `now`, `None` for no limit (0).
fn expiry_after(now: chrono::DateTime<chrono::Utc>, seconds: u64) -> Option<String> {
    let secs = i64::try_from(seconds).ok().filter(|s| *s > 0)?;
    Some((now + chrono::Duration::seconds(secs)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// The `expiresAt` of a confirmation: the setting `approvals.timeout_s` (0, the default, is no limit).
fn approval_expiry(engine: &Engine) -> Option<String> {
    let secs = engine.config().effective.get("approvals.timeout_s").and_then(Value::as_u64)?;
    expiry_after(chrono::Utc::now(), secs)
}

/// Adds a run to the thread record, read again under the lock so that two requests of one thread do not lose each other's run.
fn record_run(s: &AppState, thread: &mut AguiThread, run: AguiRun) {
    let _guard = s.agui.threads.lock().unwrap();
    if let Ok(Some(stored)) = s.engine.agui_thread(&thread.thread_id) {
        if stored.session_id == thread.session_id {
            *thread = stored;
        }
    }
    thread.record(run);
    let _ = s.engine.save_agui_thread(thread);
}

/// Is this message the one the run was started with? The id says so for a client that makes its ids up as it should; the text and
/// the number of files say so for one that sends `m1` every time (another text with a known id is another message).
fn same_message(engine: &Engine, session_id: &str, run: &AguiRun, user: &LastUser) -> bool {
    engine
        .messages(session_id)
        .unwrap_or_default()
        .iter()
        .find(|m| m.id == run.user_server_id)
        .is_some_and(|m| m.content == user.text && m.attachments.as_ref().map_or(0, Vec::len) == user.uploads.len())
}

/// Can the run be sent again to a client that asks for it a second time? Yes while it goes and after it finished; no after
/// it failed or was cancelled (the client asks for another go), and no when there is nothing of it left.
fn replayable(engine: &Engine, run_id: &str) -> bool {
    let events = engine.run_events(run_id);
    match events.last() {
        None => false,
        Some(last) => !engine.run_done(run_id) || matches!(last.event.event, AgEvent::RunFinished { .. }),
    }
}

/// A request the server has seen before is answered with the run it started: `RUN_STARTED` with the ids of this request,
/// the history up to the user message (so that the client has the same base as the first time), and the events of the run,
/// from the start, until its end (or its next pause).
fn replay_run(s: &AppState, input: &RunAgentInput, declared: Declared, thread: AguiThread, run: AguiRun) -> Response {
    let done = s.engine.run_done(&run.run_id);
    let wire = Wire::new(&input.thread_id, &input.run_id, &input.state);
    let mut follow = follow_run(s, &run.run_id, 0, wire, false, Some(pause_ctx(s, &thread, declared, HashSet::new())), !done);
    let stored = s.engine.messages(&thread.session_id).unwrap_or_default();
    let messages = agui_wire::history_messages(&stored, &thread.user_ids, Some(&run.user_server_id));
    follow.after_started.extend(agui_wire::history_snapshots(messages, chrono::Utc::now().timestamp_millis()));
    stream_response(follow, &thread.session_id, &run.run_id)
}

/// A stable text for the answer of a `resume`, to tell a replay of it from another answer.
fn fingerprint(entries: &[ResumeEntry]) -> String {
    let mut v: Vec<String> =
        entries.iter().map(|e| format!("{}|{}|{}", e.interrupt_id, e.status, e.payload.clone().unwrap_or(Value::Null))).collect();
    v.sort();
    v.join("\n")
}

/// The decision a resume entry stands for.
enum Decision {
    Allow(bool),
    Reply(QuestionReply),
}

/// `resume`: answers the interrupts of the run that paused, and continues its stream. The spec's rules
/// (<https://docs.ag-ui.com/concepts/interrupts>): the same thread, every open interrupt addressed, an answer that fits
/// the schema, a replay that is safe; what does not hold is a `RUN_ERROR`. Nothing is applied until every answer has been
/// checked, so that an answer that does not fit leaves everything as it was.
async fn resume_run(
    s: &AppState,
    owner: bool,
    input: &RunAgentInput,
    declared: Declared,
    thread: AguiThread,
    user: Option<LastUser>,
) -> Result<Response, ApiError> {
    let entries = input.resume.as_deref().unwrap_or_default();
    // A client that sends `resume` has shown that it handles interrupts, whether or not it says so again: the next confirmation
    // of the run is another pause, not a blocking `CUSTOM` it may not know.
    let declared = Declared { interrupts: true, ..declared };
    let headers = [("x-aiw-session-id", thread.session_id.clone())];
    let refuse = |code: &str, msg: &str| -> Result<Response, ApiError> { Ok(run_error(input, code, msg, &headers)) };
    let (current, answered) = {
        let mut pauses = s.agui.pauses.lock().unwrap();
        drop_stale(&mut pauses, &s.engine);
        match pauses.get(&input.thread_id) {
            Some(sl) => (sl.current.clone(), sl.answered.clone()),
            None => {
                return refuse(
                    "interrupt_not_found",
                    "nessuna interruzione aperta per questo thread (il server è stato riavviato, oppure la pausa è scaduta)",
                );
            }
        }
    };
    // A new message together with the answer is two things at once.
    if user.as_ref().and_then(|u| u.id.as_deref()).is_some_and(|id| !thread.is_client_message(id)) {
        return refuse("resume_with_new_input", "una richiesta con `resume` non può portare anche un nuovo messaggio dell'utente");
    }
    let ids: HashSet<&str> = entries.iter().map(|e| e.interrupt_id.as_str()).collect();
    if ids.len() != entries.len() {
        return refuse("resume_invalid", "`resume` risponde due volte alla stessa interruzione");
    }
    let print = fingerprint(entries);
    // Which pause is this the answer to: the one the run waits on, or an earlier one that was answered (a client that lost the
    // response sends the same answer again, even after the run paused once more).
    let open: HashSet<&str> = current.interrupts.iter().map(|i| i.id.as_str()).collect();
    let earlier = answered.iter().rev().find(|p| {
        let theirs: HashSet<&str> = p.interrupts.iter().map(|i| i.id.as_str()).collect();
        theirs == ids
    });
    let mut lost: Vec<String> = vec![];
    let pause = if ids.iter().all(|id| open.contains(id)) {
        if let Some(missing) = current.interrupts.iter().find(|i| !ids.contains(i.id.as_str())) {
            return refuse("resume_incomplete", &format!("`resume` deve rispondere a tutte le interruzioni aperte: manca {}", missing.id));
        }
        &current
    } else if let Some(p) = earlier {
        p
    } else {
        let unknown = entries.iter().find(|e| !open.contains(e.interrupt_id.as_str())).map(|e| e.interrupt_id.as_str()).unwrap_or_default();
        return refuse("interrupt_not_found", &format!("l'interruzione {unknown} non esiste tra quelle aperte del thread"));
    };
    if std::ptr::eq(pause, &current) && current.applied.is_none() {
        // The answer is new. Everything is checked first; nothing is applied until it all fits.
        let mut decisions: Vec<(&OpenInterrupt, Decision)> = vec![];
        for e in entries {
            let it = current.interrupts.iter().find(|i| i.id == e.interrupt_id).expect("checked above");
            let cancelled = match e.status.as_str() {
                "resolved" => false,
                "cancelled" => true,
                other => return refuse("resume_invalid", &format!("lo stato «{other}» non esiste: usa resolved o cancelled")),
            };
            let decision = match (&it.kind, cancelled) {
                (Kind::Approval, true) => Decision::Allow(false),
                (Kind::Approval, false) => match e.payload.as_ref().and_then(|p| p.get("approved")).and_then(Value::as_bool) {
                    Some(a) => Decision::Allow(a),
                    None => return refuse("resume_invalid", &format!("la risposta a {} deve avere `approved` (true o false)", it.id)),
                },
                (Kind::Question, true) => Decision::Reply(QuestionReply { action: "decline".into(), answer: QuestionAnswer::default() }),
                (Kind::Question, false) => match e.payload.clone().map(serde_json::from_value::<QuestionAnswer>) {
                    Some(Ok(a)) => Decision::Reply(QuestionReply { action: "answer".into(), answer: a }),
                    _ => return refuse("resume_invalid", &format!("la risposta a {} non ha la forma attesa dalla domanda", it.id)),
                },
            };
            decisions.push((it, decision));
        }
        let mut closed: HashSet<String> = HashSet::new();
        for (it, decision) in &decisions {
            let r = match decision {
                Decision::Allow(_) => s.engine.check_approval_answer(&it.id, owner),
                Decision::Reply(reply) => s.engine.check_question_reply(&it.id, reply),
            };
            match r {
                Ok(()) => {}
                Err(Error::NotFound(_)) => {
                    closed.insert(it.id.clone());
                }
                Err(Error::Invalid(m)) => return refuse("resume_invalid", &m),
                // Not the owner of a management tool, and the like: an HTTP error, the pause stays.
                Err(e) => return Err(e.into()),
            }
        }
        // Closed meanwhile, in the app or because the wait ran out: the run went on without them. The stream goes on all the same (a
        // client that has them pending cannot continue the thread any other way). An answer that loses nothing (a refusal, a
        // cancellation, a declined question) is just not needed; one that would have been acted on is told in `aiw.interrupt_closed`.
        lost = decisions
            .iter()
            .filter(|(it, d)| {
                closed.contains(&it.id) && matches!(d, Decision::Allow(true))
                    || matches!(d, Decision::Reply(r) if r.action == "answer" && closed.contains(&it.id))
            })
            .map(|(it, _)| it.id.clone())
            .collect();
        decisions.retain(|(it, _)| !closed.contains(&it.id));
        // The answer is recorded first, under the lock: of two answers that arrive together, one wins and the other is a replay or a conflict.
        {
            let mut pauses = s.agui.pauses.lock().unwrap();
            match pauses.get_mut(&input.thread_id) {
                Some(sl) if sl.current.run_id == current.run_id && sl.current.next_seq == current.next_seq => match &sl.current.applied {
                    None => sl.current.applied = Some(print.clone()),
                    Some(done) if *done == print => {}
                    Some(_) => return refuse("resume_conflict", "queste interruzioni hanno già ricevuto una risposta diversa"),
                },
                _ => return refuse("interrupt_not_found", "la pausa del thread è cambiata mentre arrivava la risposta: riprova"),
            }
        }
        // Questions first, then the confirmations. What was checked above can only fail now if it was closed in the meantime.
        decisions.sort_by_key(|(it, _)| it.kind == Kind::Approval);
        for (it, decision) in decisions {
            let r = match decision {
                Decision::Allow(allow) => s.engine.respond_approval_as(&it.id, allow, false, owner),
                Decision::Reply(reply) => s.engine.respond_question(&it.id, reply),
            };
            if let Err(e) = r {
                if !matches!(e, Error::NotFound(_)) {
                    if let Some(sl) = s.agui.pauses.lock().unwrap().get_mut(&input.thread_id) {
                        sl.current.applied = None;
                    }
                    return match e {
                        Error::Invalid(m) => refuse("resume_invalid", &m),
                        other => Err(other.into()),
                    };
                }
            }
        }
    } else {
        // A pause that was answered: the same answer again is safe, another one is a conflict.
        match &pause.applied {
            Some(done) if *done == print => {}
            _ => return refuse("resume_conflict", "queste interruzioni hanno già ricevuto una risposta diversa"),
        }
    }
    // The run goes on: a new `runId` for the client, the suspended subagents announced again, and the events from where the pause
    // left off.
    let mut wire = pause.wire.resumed(&input.run_id, &input.state);
    let managed: HashSet<String> = pause.interrupts.iter().map(|i| i.id.clone()).collect();
    let done = s.engine.run_done(&pause.run_id);
    let ts = chrono::Utc::now().timestamp_millis();
    let again = wire.resume_subagents(ts);
    let mut follow = follow_run(s, &pause.run_id, pause.next_seq, wire, true, Some(pause_ctx(s, &thread, declared, managed)), !done);
    follow.out.push_back(Frame {
        seq: None,
        data: json!({"type": "RUN_STARTED", "threadId": input.thread_id, "runId": input.run_id, "parentRunId": pause.client_run_id, "timestamp": ts}),
    });
    for data in again {
        follow.out.push_back(Frame { seq: None, data });
    }
    if !lost.is_empty() {
        let value = json!({"interrupt_ids": lost, "reason": "closed_elsewhere"});
        follow.out.push_back(Frame {
            seq: None,
            data: json!({"type": "CUSTOM", "name": "aiw.interrupt_closed", "value": value, "timestamp": ts}),
        });
    }
    Ok(stream_response(follow, &thread.session_id, &pause.run_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_security::{fixture, req, send, LAN};
    use aiw_core::events::TimedEvent;
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
        // The stream is the log of the run, in order, with `id:` = seq; RUN_* carry the ids of the client, the state goes
        // under `aiw` (and a state that did not change is not sent again), the other events are the log's, as they are.
        let log = e.run_events(&rid);
        let ids: Vec<u64> = body.lines().filter_map(|l| l.strip_prefix("id:")).map(|v| v.trim().parse().unwrap()).collect();
        assert_eq!(ids.len(), evs.len(), "every event of the stream has the position of the log event it comes from");
        assert!(ids.windows(2).all(|w| w[0] <= w[1]), "{ids:?}");
        for (frame, id) in evs.iter().zip(&ids) {
            let source = log.iter().find(|l| l.seq == *id).expect("an event of the log");
            match frame["type"].as_str().unwrap() {
                "RUN_STARTED" | "RUN_FINISHED" | "STATE_SNAPSHOT" | "STATE_DELTA" => {}
                _ => assert_eq!(frame, &serde_json::to_value(&source.event).unwrap()),
            }
        }
        let snapshot = evs.iter().find(|v| v["type"] == "STATE_SNAPSHOT").expect("a state snapshot");
        assert!(snapshot["snapshot"]["aiw"].is_object() && snapshot["snapshot"].get("model").is_none(), "{snapshot}");
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

    #[tokio::test]
    async fn the_stream_opens_with_run_started_even_when_the_engine_emitted_other_events_first() {
        let (_app, e, _d, _w) = fixture(vec![]).await;
        let ev = |seq: u64, event: AgEvent| SeqEvent { seq, event: TimedEvent::now(event) };
        let custom = |name: &str| AgEvent::Custom { name: name.into(), value: json!({}) };
        let started = || AgEvent::RunStarted { thread_id: "i".into(), run_id: "i".into() };
        let finished = || AgEvent::RunFinished { thread_id: "i".into(), run_id: "i".into(), result: None };
        let follow = |events: Vec<SeqEvent>| Follow {
            rx: e.subscribe(),
            engine: e.clone(),
            run_id: "run_x".into(),
            wire: Wire::new("t", "r", &Value::Null),
            queue: events.into(),
            next_seq: 0,
            finished: false,
            started: false,
            held: vec![],
            ready: VecDeque::new(),
            out: VecDeque::new(),
            after_started: vec![],
            guard: CancelGuard::new(Default::default(), e.clone(), "run_x", false),
            pause: None,
            done: false,
        };
        let order = |mut f: Follow| async move {
            let mut out = vec![];
            while let Some(e) = f.next().await {
                out.push(e.seq);
            }
            out
        };
        // Routing events come before the agent loop starts: RUN_STARTED still leads, the others keep their order.
        let evs =
            vec![ev(0, custom("aiw.route")), ev(1, custom("aiw.runtime")), ev(2, started()), ev(3, custom("aiw.x")), ev(4, finished())];
        assert_eq!(order(follow(evs)).await, vec![2, 0, 1, 3, 4]);
        // The usual order is untouched.
        assert_eq!(order(follow(vec![ev(0, started()), ev(1, custom("aiw.x")), ev(2, finished())])).await, vec![0, 1, 2]);
        // A run that fails before it starts: what happened, as it happened, and the stream ends.
        let err = AgEvent::RunError { message: "no".into(), code: None };
        assert_eq!(order(follow(vec![ev(0, custom("aiw.route")), ev(1, err)])).await, vec![0, 1]);
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

    // ------------------------------------------------------------------ the Assistant: questions and secrets

    fn models() -> Value {
        json!({
            "kind": "choice", "title": "Quale modello?", "message": "Ho trovato due varianti.",
            "options": [{"id": "q4", "label": "Q4_K_M", "description": "Consigliata"}, {"id": "q8", "label": "Q8_0"}]
        })
    }

    /// The request of an AG-UI client that says (or not) what it can show.
    fn input_with(thread: &str, props: Option<Value>) -> Value {
        let mut body = input(thread, "r1", json!("Scarica un modello"));
        if let Some(p) = props {
            body["forwardedProps"] = p;
        }
        body
    }

    fn both() -> Option<Value> {
        Some(json!({"aiw": {"interactive": ["questions", "secrets"]}}))
    }

    /// The events of `frames` with this custom name, in order.
    fn customs(evs: &[Value], name: &str) -> Vec<Value> {
        evs.iter().filter(|v| v["type"] == "CUSTOM" && v["name"] == name).map(|v| v["value"].clone()).collect()
    }

    /// What the rest of a stream says once the client stopped reading in the middle.
    async fn rest_of(body: axum::body::Body) -> String {
        let bytes = tokio::time::timeout(Duration::from_secs(10), axum::body::to_bytes(body, 1 << 22))
            .await
            .expect("the stream ends with the run")
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn tool_result_of(evs: &[Value], tool_call_id: &str) -> Value {
        let v = evs
            .iter()
            .find(|v| v["type"] == "TOOL_CALL_RESULT" && v["toolCallId"] == tool_call_id)
            .unwrap_or_else(|| panic!("no result for {tool_call_id}"));
        serde_json::from_str(v["content"].as_str().unwrap()).unwrap()
    }

    #[tokio::test]
    async fn without_a_declaration_ask_user_answers_unsupported_at_once_and_the_agent_asks_in_text() {
        let turns = vec![ScriptedBackend::tool_turn("a1", "ask_user", models()), ScriptedBackend::text_turn("Quale preferisci: Q4 o Q8?")];
        let (app, e, _d, _w) = fixture(turns).await;
        let (st, _, rid, evs) = run(&app, "assistant", input_with("thr-q0", None)).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(evs.last().unwrap()["type"], "RUN_FINISHED", "the run is not stopped by a question nobody can answer");
        // The call is seen (a generic client shows the question in the arguments), the answer is `unsupported` with the way out.
        let start = evs.iter().find(|v| v["type"] == "TOOL_CALL_START").unwrap();
        assert_eq!(start["toolCallName"], "ask_user");
        assert_eq!(
            tool_result_of(&evs, start["toolCallId"].as_str().unwrap()),
            json!({"outcome": "unsupported", "hint": "Fai la domanda in testo e aspetta il prossimo messaggio."})
        );
        assert!(customs(&evs, "aiw.question").is_empty() && customs(&evs, "aiw.question_resolved").is_empty());
        assert!(e.questions().is_empty());
        let text: String = evs.iter().filter(|v| v["type"] == "TEXT_MESSAGE_CONTENT").map(|v| v["delta"].as_str().unwrap()).collect();
        assert_eq!(text, "Quale preferisci: Q4 o Q8?");
        assert!(e.run_done(&rid));
    }

    #[tokio::test]
    async fn declaring_questions_and_secrets_the_owner_gets_aiw_question_and_the_run_waits_for_the_rest_endpoint() {
        let turns = vec![ScriptedBackend::tool_turn("a1", "ask_user", models()), ScriptedBackend::text_turn("Scarico la Q4.")];
        let (app, e, _d, _w) = fixture(turns).await;
        let res = post_raw(&app, "assistant", input_with("thr-q1", both()), &[]).await;
        assert_eq!(res.status(), StatusCode::OK);
        let rid = header(&res, "x-aiw-run-id");
        let mut body = res.into_body();
        let seen = read_until(&mut body, "aiw.question").await;
        let q = customs(&frames(&seen), "aiw.question").pop().expect("the question event");
        assert_eq!(
            (q["kind"].as_str(), q["title"].as_str(), q["run_id"].as_str()),
            (Some("choice"), Some("Quale modello?"), Some(rid.as_str()))
        );
        // The null of what is not there are written out, and the options come as the model gave them.
        assert_eq!((&q["options"][1]["description"], &q["confirm_label"], &q["deny_label"]), (&Value::Null, &Value::Null, &Value::Null));
        assert_eq!(q["fields"], json!([]));
        assert!(!e.run_done(&rid), "the run waits for the person");
        assert_eq!(e.questions().len(), 1);
        // The answer comes on the REST endpoint, in a request of its own: the stream goes on with the result and ends.
        let id = q["id"].as_str().unwrap();
        let reply = json!({"action": "answer", "selected": ["q4"], "other": null, "confirmed": null, "values": null});
        let (st, v) = crate::dispatch(app.clone(), "POST", &format!("/api/questions/{id}"), Some(reply)).await;
        assert_eq!((st, v), (200, json!({})));
        let evs = frames(&format!("{seen}{}", rest_of(body).await));
        assert_eq!(evs.last().unwrap()["type"], "RUN_FINISHED");
        assert_eq!(
            customs(&evs, "aiw.question_resolved"),
            [json!({"id": id, "outcome": "answered", "answer": {"selected": ["q4"], "other": null, "confirmed": null, "values": null}})]
        );
        let start = evs.iter().find(|v| v["type"] == "TOOL_CALL_START").unwrap();
        assert_eq!(tool_result_of(&evs, start["toolCallId"].as_str().unwrap())["selected"], json!(["q4"]));
        assert!(e.questions().is_empty());
    }

    #[tokio::test]
    async fn declaring_secrets_the_owner_gets_aiw_secret_request_without_a_value_and_the_run_waits_for_the_rest_endpoint() {
        const FAKE_KEY: &str = "FAKE_BRAVE_KEY_agui_7d6c5b4a3f";
        let args = json!({"target": "setting:search.brave_api_key", "label": "Chiave di Brave", "reason": "Serve per cercare sul web"});
        let turns = vec![ScriptedBackend::tool_turn("s1", "aiw_secret_request", args), ScriptedBackend::text_turn("Chiave salvata.")];
        let (app, e, _d, _w) = fixture(turns).await;
        let res = post_raw(&app, "assistant", input_with("thr-s0", both()), &[]).await;
        assert_eq!(res.status(), StatusCode::OK);
        let rid = header(&res, "x-aiw-run-id");
        let mut body = res.into_body();
        let seen = read_until(&mut body, "aiw.secret_request").await;
        let evs = frames(&seen);
        // The call is seen like any other (name and arguments), and the request comes next as a custom event.
        let start = evs.iter().find(|v| v["type"] == "TOOL_CALL_START").expect("the call of the tool");
        assert_eq!(start["toolCallName"], "aiw_secret_request");
        let ev = customs(&evs, "aiw.secret_request").pop().expect("the request event");
        // Exactly the fields of the contract: where the value goes in words, and no field that could hold a value.
        let mut keys: Vec<&str> = ev.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["destination", "expires_at", "id", "label", "reason", "run_id", "target"]);
        assert_eq!(
            (ev["target"].as_str(), ev["label"].as_str(), ev["reason"].as_str(), ev["run_id"].as_str()),
            (Some("setting:search.brave_api_key"), Some("Chiave di Brave"), Some("Serve per cercare sul web"), Some(rid.as_str()))
        );
        assert_eq!(ev["destination"], "Impostazione «Chiave di Brave Search» (search.brave_api_key), salvata su questo computer");
        assert!(ev["id"].as_str().is_some_and(|i| i.starts_with("sec_")) && ev["expires_at"].as_str().is_some_and(|t| t.ends_with('Z')));
        // The run waits for the person: no result yet, nothing ended, and the request is listed (without a value).
        assert!(evs.iter().all(|v| v["type"] != "TOOL_CALL_RESULT" && v["type"] != "RUN_FINISHED"));
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!e.run_done(&rid), "the run waits for the person");
        let open = e.secret_requests();
        assert_eq!((open.len(), open[0].id.as_str()), (1, ev["id"].as_str().unwrap()));
        // The value comes on the REST endpoint, in a request of its own: the stream goes on with the result and ends,
        // and the value is in none of it.
        let id = ev["id"].as_str().unwrap();
        let (st, v) = crate::dispatch(
            app.clone(),
            "POST",
            &format!("/api/secret-requests/{id}"),
            Some(json!({"action": "provide", "value": FAKE_KEY})),
        )
        .await;
        assert_eq!((st, v), (200, json!({"stored": true})));
        let all = format!("{seen}{}", rest_of(body).await);
        let evs = frames(&all);
        assert_eq!(evs.last().unwrap()["type"], "RUN_FINISHED");
        assert_eq!(customs(&evs, "aiw.secret_resolved"), [json!({"id": id, "outcome": "stored"})]);
        assert_eq!(
            tool_result_of(&evs, start["toolCallId"].as_str().unwrap()),
            json!({"outcome": "stored", "target": "setting:search.brave_api_key"})
        );
        assert!(!all.contains(FAKE_KEY), "the value is in the stream of the run");
        assert!(e.secret_requests().is_empty());
    }

    #[tokio::test]
    async fn declaring_only_questions_or_being_a_device_a_secret_request_is_unsupported_at_once() {
        let args = json!({"target": "provider_token:hf", "label": "Token di Hugging Face"});
        let turns = vec![
            ScriptedBackend::tool_turn("s1", "aiw_secret_request", args.clone()),
            ScriptedBackend::text_turn("Il token va inserito dall'app."),
            ScriptedBackend::tool_turn("s2", "aiw_secret_request", args),
            ScriptedBackend::text_turn("Il token va inserito dall'app."),
        ];
        let (app, e, _d, _w) = fixture(turns).await;
        let hint = "dall'app su questo computer";
        // The owner's client shows questions but declared no secrets.
        let only_questions = Some(json!({"aiw": {"interactive": ["questions"]}}));
        let (_, _, _, evs) = run(&app, "assistant", input_with("thr-s1", only_questions)).await;
        let start = evs.iter().find(|v| v["type"] == "TOOL_CALL_START").unwrap();
        assert_eq!(start["toolCallName"], "aiw_secret_request");
        let result = tool_result_of(&evs, start["toolCallId"].as_str().unwrap());
        assert_eq!(result["outcome"], "unsupported");
        assert!(result["hint"].as_str().unwrap().contains(hint), "{result}");
        assert!(customs(&evs, "aiw.secret_request").is_empty());
        // A paired device that declares both: a secret is typed at an endpoint it cannot use, so it is not asked for one.
        let dev = format!("Bearer {}", e.create_api_token("Telefono", "device", None, None, None).unwrap().token);
        let mut r = req(
            "POST",
            "/agui/assistant",
            Some(LAN),
            &[("host", "192.168.1.5:7777"), ("authorization", dev.as_str())],
            Some(input_with("thr-s2", both())),
        );
        r.extensions_mut().insert(crate::security::TlsConn);
        let res = tower::ServiceExt::oneshot(app.clone(), r).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let evs = frames(&read_all(res).await);
        assert_eq!(evs.last().unwrap()["type"], "RUN_FINISHED");
        let start = evs.iter().find(|v| v["type"] == "TOOL_CALL_START").unwrap();
        let result = tool_result_of(&evs, start["toolCallId"].as_str().unwrap());
        assert_eq!(result["outcome"], "unsupported");
        assert!(customs(&evs, "aiw.secret_request").is_empty() && e.secret_requests().is_empty());
    }

    // ------------------------------------------------------------------ interrupts and resume: the rules of the protocol

    fn state_with_pause(e: &Engine, thread: &str, interrupts: &[(&str, Kind)]) -> AppState {
        let st = AppState { engine: e.clone(), agui: Default::default() };
        st.agui.pauses.lock().unwrap().insert(
            thread.into(),
            Slot {
                answered: vec![],
                current: Pause {
                    run_id: "run_x".into(),
                    client_run_id: "r0".into(),
                    next_seq: 0,
                    wire: Wire::new(thread, "r0", &Value::Null),
                    interrupts: interrupts.iter().map(|(id, kind)| OpenInterrupt { id: (*id).into(), kind: kind.clone() }).collect(),
                    applied: None,
                    since: Instant::now(),
                },
            },
        );
        st
    }

    /// A `resume` sent to the endpoint, as the events of the answer.
    async fn resume_with(st: &AppState, thread: &str, resume: Value, messages: Value) -> Vec<Value> {
        let input: RunAgentInput =
            serde_json::from_value(json!({"threadId": thread, "runId": "r1", "messages": messages, "resume": resume})).unwrap();
        match run_agent(State(st.clone()), Path("planner".into()), IsOwner(true), Json(input)).await {
            Ok(res) => frames(&read_all(res).await),
            Err(_) => panic!("an HTTP error, not a stream"),
        }
    }

    #[tokio::test]
    async fn a_resume_must_follow_the_rules_of_the_protocol_and_what_does_not_is_a_run_error() {
        let (_app, e, _d, _w) = fixture(vec![]).await;
        let ses = e.create_session("planner", Some("Pausa".into())).unwrap();
        let st = state_with_pause(&e, &ses.id, &[("apr_1", Kind::Approval), ("ask_1", Kind::Question)]);
        let resolved = |id: &str, payload: Value| json!({"interruptId": id, "status": "resolved", "payload": payload});
        let code = |evs: &[Value]| {
            assert_eq!(evs.iter().map(|v| v["type"].as_str().unwrap()).collect::<Vec<_>>(), ["RUN_STARTED", "RUN_ERROR"], "{evs:?}");
            (evs[1]["code"].as_str().unwrap().to_string(), evs[0]["runId"].as_str().unwrap().to_string())
        };
        // Every open interrupt must be addressed: partial resumes are not supported.
        let evs = resume_with(&st, &ses.id, json!([resolved("apr_1", json!({"approved": true}))]), json!([])).await;
        assert_eq!(code(&evs), ("resume_incomplete".to_string(), "r1".to_string()));
        assert!(evs[1]["message"].as_str().unwrap().contains("ask_1"), "{evs:?}");
        // An id that is not among the open ones.
        let evs =
            resume_with(&st, &ses.id, json!([resolved("apr_1", json!({"approved": true})), resolved("zzz", json!({}))]), json!([])).await;
        assert_eq!(code(&evs).0, "interrupt_not_found");
        // A state that does not exist, an approval without `approved`, an answer of the wrong shape.
        let both = |a: Value, q: Value| json!([a, q]);
        let bad_status = both(json!({"interruptId": "apr_1", "status": "maybe"}), resolved("ask_1", json!({"confirmed": true})));
        assert_eq!(code(&resume_with(&st, &ses.id, bad_status, json!([])).await).0, "resume_invalid");
        let no_approved = both(resolved("apr_1", json!({"si": 1})), resolved("ask_1", json!({"confirmed": true})));
        assert_eq!(code(&resume_with(&st, &ses.id, no_approved, json!([])).await).0, "resume_invalid");
        let bad_answer = both(resolved("apr_1", json!({"approved": true})), resolved("ask_1", json!({"selected": "q4"})));
        assert_eq!(code(&resume_with(&st, &ses.id, bad_answer, json!([])).await).0, "resume_invalid");
        // The answer comes with a new message of the user: two things at once.
        let fine = both(resolved("apr_1", json!({"approved": true})), resolved("ask_1", json!({"confirmed": true})));
        let evs = resume_with(&st, &ses.id, fine, json!([{"id": "m-new", "role": "user", "content": "altro"}])).await;
        assert_eq!(code(&evs).0, "resume_with_new_input");
        // Nothing of this touched the pause: it is still there, unanswered.
        {
            let pauses = st.agui.pauses.lock().unwrap();
            assert!(pauses.get(&ses.id).is_some_and(|sl| sl.current.applied.is_none() && sl.current.interrupts.len() == 2));
        }
        // A thread nobody paused: no interrupt to answer, and no session made for it.
        let evs = resume_with(&st, "altro-thread", json!([resolved("apr_1", json!({"approved": true}))]), json!([])).await;
        assert_eq!(code(&evs).0, "interrupt_not_found");
        assert_eq!(e.sessions().unwrap().len(), 1, "a resume for a thread nobody knows made a session");
    }

    #[test]
    fn the_client_declares_what_it_can_show() {
        let d = |v: Value| declared_interactive(&v);
        assert_eq!(d(json!({})), Declared::default());
        assert_eq!(d(json!({"aiw": {"interactive": ["interrupts"]}})), Declared { interrupts: true, ..Default::default() });
        assert_eq!(
            d(json!({"aiw": {"interactive": ["questions", "secrets", "interrupts", "zzz"]}})),
            Declared { questions: true, secrets: true, interrupts: true }
        );
        assert_eq!(d(json!({"aiw": {"interactive": "interrupts"}})), Declared::default(), "a string is no declaration");
    }

    #[tokio::test]
    async fn a_device_token_cannot_answer_the_confirmation_of_a_management_tool_with_resume() {
        let args = json!({"key": "ui.theme", "value": "dark"});
        let turns = vec![ScriptedBackend::tool_turn("m1", "aiw_setting_set", args), ScriptedBackend::text_turn("Lascio il tema")];
        let (app, e, _d, _w) = fixture(turns).await;
        let props = Some(json!({"aiw": {"interactive": ["interrupts"]}}));
        let res = post_raw(&app, "assistant", input_with("thr-mgmt", props), &[]).await;
        assert_eq!(res.status(), StatusCode::OK);
        let rid = header(&res, "x-aiw-run-id");
        // The stream ends by itself at the interruption, and the run keeps waiting for the owner.
        let evs = frames(&read_all(res).await);
        let finished = evs.last().unwrap();
        assert_eq!(
            (finished["type"].as_str(), finished["outcome"]["type"].as_str()),
            (Some("RUN_FINISHED"), Some("interrupt")),
            "{finished}"
        );
        let interrupt = &finished["outcome"]["interrupts"][0];
        assert_eq!(
            (interrupt["reason"].as_str(), interrupt["metadata"]["aiw"]["tool"].as_str()),
            (Some("tool_call"), Some("aiw_setting_set"))
        );
        let types: Vec<&str> = evs.iter().map(|v| v["type"].as_str().unwrap()).collect();
        let last_two = &types[types.len() - 3..];
        assert_eq!(
            last_two,
            ["MESSAGES_SNAPSHOT", "STATE_SNAPSHOT", "RUN_FINISHED"],
            "the snapshots go right before the interruption: {types:?}"
        );
        assert!(!e.run_done(&rid) && e.approvals().len() == 1, "the closed connection did not cancel the paused run");
        // A paired device answers: refused, and nothing was decided.
        let id = interrupt["id"].as_str().unwrap();
        let resume = json!({
            "threadId": "thr-mgmt", "runId": "r2", "messages": [{"id": "m1", "role": "user", "content": "Scarica un modello"}],
            "resume": [{"interruptId": id, "status": "resolved", "payload": {"approved": true}}]
        });
        let dev = format!("Bearer {}", e.create_api_token("Telefono", "device", None, None, None).unwrap().token);
        let mut r = req(
            "POST",
            "/agui/assistant",
            Some(LAN),
            &[("host", "192.168.1.5:7777"), ("authorization", dev.as_str())],
            Some(resume.clone()),
        );
        r.extensions_mut().insert(crate::security::TlsConn);
        let res = tower::ServiceExt::oneshot(app.clone(), r).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let v: Value = serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap()).unwrap();
        assert!(v["error"].as_str().is_some_and(|m| m.contains("proprietario")), "{v}");
        assert!(!e.run_done(&rid) && e.approvals().len() == 1, "a refused answer left the confirmation open");
        // The owner (this computer) answers, with a denial: the run goes on to the end, once.
        let mut deny = resume;
        deny["resume"][0]["payload"] = json!({"approved": false});
        let res = post_raw(&app, "assistant", deny, &[]).await;
        assert_eq!(res.status(), StatusCode::OK);
        let evs = frames(&read_all(res).await);
        assert_eq!(
            (evs[0]["type"].as_str(), evs[0]["parentRunId"].as_str(), evs[0]["runId"].as_str()),
            (Some("RUN_STARTED"), Some("r1"), Some("r2"))
        );
        assert_eq!(evs.last().unwrap()["type"], "RUN_FINISHED");
        assert!(evs.last().unwrap().get("outcome").is_none());
        assert!(e.run_done(&rid) && e.approvals().is_empty());
    }

    #[tokio::test]
    async fn the_last_stream_that_follows_a_run_cancels_it_and_not_the_first_one_to_go() {
        let (app, e, _d, _w) = fixture(write_turns()).await;
        let res = post_raw(&app, "operator", input("thr-guard", "r1", json!("scrivi nota.md")), &[]).await;
        let rid = header(&res, "x-aiw-run-id");
        let mut body = res.into_body();
        read_until(&mut body, "aiw.approval_request").await;
        // Two streams for one run (a response that was cut and the one that follows it): the first to go does not stop the run.
        let state: Threads = Default::default();
        let first = CancelGuard::new(state.clone(), e.clone(), &rid, true);
        let second = CancelGuard::new(state.clone(), e.clone(), &rid, true);
        drop(first);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!e.run_done(&rid), "the first stream to go cancelled a run that another one still follows");
        drop(second);
        wait_for("the last stream to go cancels the run", || e.run_done(&rid)).await;
        assert!(state.followers.lock().unwrap().is_empty(), "the count of followers is cleaned up");
        drop(body);
    }

    #[tokio::test]
    async fn an_answer_for_a_thread_nobody_knows_makes_no_session_and_ids_are_bounded() {
        let (app, e, _d, _w) = fixture(vec![]).await;
        let long = "x".repeat(MAX_ID_LEN + 1);
        let (st, msg) = failure(&app, "planner", input(&long, "r", json!("ciao"))).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{msg}");
        let mut big = input("thr-big", "r", json!("ciao"));
        big["state"] = json!({"x": "y".repeat(MAX_STATE_BYTES)});
        let (st, msg) = failure(&app, "planner", big).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{msg}");
        assert!(msg.contains("state"), "{msg}");
        assert!(e.sessions().unwrap().is_empty());
    }

    #[test]
    fn a_confirmation_expires_only_when_the_setting_says_so() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-15T10:00:00Z").unwrap().with_timezone(&chrono::Utc);
        assert_eq!(expiry_after(now, 0), None);
        assert_eq!(expiry_after(now, 300).as_deref(), Some("2026-11-15T10:05:00Z"));
    }

    #[tokio::test]
    async fn a_session_made_for_a_request_that_failed_goes_unless_another_request_uses_it() {
        let (app, e, _d, _w) = fixture(vec![ScriptedBackend::text_turn("Ok")]).await;
        let st = AppState { engine: e.clone(), agui: Default::default() };
        assert_eq!(approval_expiry(&e), None, "no limit unless approvals.timeout_s is set");
        let empty = e.create_session("planner", None).unwrap();
        abandon_new_session(&st, &empty.id, "thr-empty");
        assert!(e.session(&empty.id).is_err(), "an empty session made for the failed request stays behind");
        // A session that another request of the thread already used (it has messages) is left alone.
        let (_, sid, _, _) = run(&app, "planner", input("thr-used", "r1", json!("ciao"))).await;
        wait_for("the message is stored", || e.messages(&sid).map(|m| !m.is_empty()).unwrap_or(false)).await;
        abandon_new_session(&st, &sid, "thr-used");
        assert!(e.session(&sid).is_ok(), "a session in use was deleted");
    }
}
