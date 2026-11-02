//! The agent loop (architecture §5.1): stream → tool calls → policy/hooks/approval → tools → repeat,
//! emitting AG-UI events and returning the messages to persist.

use crate::fallback::extract_text_tool_calls;
use crate::hooks::{run_hooks, HookConfig};
use crate::policy::{risk_level, Decision, Policy};
use crate::skills::{self, Skill};
use crate::think::{Piece, ThinkSplitter};
use aiw_core::api::{AgentProfile, ApprovalRequest, AuditEntry, ChatMessage, InferenceProfile, RunState, Todo, ToolCall, Usage};
use aiw_core::events::AgEvent;
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use aiw_runtimes::{ChatBackend, ChatDelta, ChatRequest, ContentPart, ImageUrl, Msg, MsgContent, OaiFunctionCall, OaiToolCall, ToolSpec};
use aiw_tools::{Risk, ToolContext, ToolOutput, ToolRegistry};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

pub const MAX_DEPTH: u32 = 2;
const MAX_TOOL_OUTPUT: usize = 24_000;

/// Services the loop needs from the engine.
#[async_trait]
pub trait RunHost: Send + Sync {
    fn emit(&self, run_id: &str, ev: AgEvent);
    /// Registers the request, waits for the user's decision (true = allowed).
    async fn approve(&self, req: ApprovalRequest) -> bool;
    fn audit(&self, entry: AuditEntry);
    async fn backend(&self, profile: &InferenceProfile) -> Result<Arc<dyn ChatBackend>>;
    fn tools(&self) -> Arc<ToolRegistry>;
    fn skills(&self) -> Vec<Skill>;
    /// Runs `task` with another agent profile in an isolated context; returns its final answer.
    async fn subagent(&self, parent: &RunInput, agent_id: &str, task: &str) -> Result<String>;
}

pub struct RunInput {
    pub run_id: String,
    pub thread_id: String,
    pub agent: AgentProfile,
    pub profile: InferenceProfile,
    pub history: Vec<Msg>,
    pub user_input: String,
    /// Extra system sections (environment, AGENTS.md…).
    pub system_extra: Vec<String>,
    pub tool_ctx: ToolContext,
    pub policy: Policy,
    pub hooks: Vec<HookConfig>,
    pub compact_at: f64,
    pub depth: u32,
    /// Tools the user allowed "for this session".
    pub session_allow: Arc<Mutex<HashSet<String>>>,
    /// Other agents that can be delegated to (id → description).
    pub subagents: Vec<(String, String)>,
}

#[derive(Debug, Default)]
pub struct RunOutcome {
    pub final_text: String,
    pub new_messages: Vec<ChatMessage>,
    pub usage: Usage,
}

const MAX_SELF_CHECKS: u32 = 2;
/// `name` of the stored user messages written by the self-check (the UI shows them as notes).
pub const SELF_CHECK_NAME: &str = "aiw.self_check";

fn self_check_note(failed: &[(String, String)]) -> String {
    let list = failed.iter().map(|(n, e)| format!("- {n}: {e}")).collect::<Vec<_>>().join("\n");
    format!(
        "[Controllo automatico] Prima di concludere: queste azioni sono fallite e non le hai ripetute:\n{list}\nCorreggi la chiamata e riprova; se non è possibile, di' chiaramente all'utente cosa non è stato fatto. Non dichiarare completate azioni fallite."
    )
}

/// Converts stored chat messages to the OpenAI wire format.
pub fn to_msgs(history: &[ChatMessage]) -> Vec<Msg> {
    history
        .iter()
        .filter(|m| m.role != "system")
        .map(|m| Msg {
            role: m.role.clone(),
            content: if m.content.is_empty() && m.tool_calls.is_some() { None } else { Some(MsgContent::Text(m.content.clone())) },
            tool_calls: m.tool_calls.as_ref().map(|tcs| {
                tcs.iter()
                    .map(|t| OaiToolCall {
                        id: t.id.clone(),
                        kind: "function".into(),
                        function: OaiFunctionCall { name: t.name.clone(), arguments: t.arguments.clone() },
                    })
                    .collect()
            }),
            tool_call_id: m.tool_call_id.clone(),
            name: m.name.clone(),
        })
        .collect()
}

fn internal_specs(input: &RunInput, skills: &[Skill]) -> Vec<ToolSpec> {
    let allowed = |n: &str| input.agent.tools.iter().any(|t| t == "*" || t == n);
    let mut v = Vec::new();
    if allowed("todo") {
        v.push(ToolSpec::function(
            "todo",
            "Pubblica o aggiorna il piano di lavoro (lista completa ogni volta). Usalo per i compiti con più passi.",
            json!({"type":"object","properties":{"todos":{"type":"array","items":{"type":"object","properties":{"content":{"type":"string"},"status":{"type":"string","enum":["pending","in_progress","completed"]}},"required":["content","status"]}}},"required":["todos"]}),
        ));
    }
    if allowed("agent") && input.depth < MAX_DEPTH && !input.subagents.is_empty() {
        let names: Vec<Value> = input.subagents.iter().map(|(id, _)| Value::String(id.clone())).collect();
        let desc: Vec<String> = input.subagents.iter().map(|(id, d)| format!("{id}: {d}")).collect();
        v.push(ToolSpec::function(
            "agent",
            format!("Delega un sotto-compito a un altro agente con contesto separato; restituisce solo la sua risposta finale. Agenti: {}", desc.join("; ")),
            json!({"type":"object","properties":{"agent":{"type":"string","enum":names},"task":{"type":"string","description":"Istruzioni complete e autosufficienti"}},"required":["agent","task"]}),
        ));
    }
    if allowed(crate::a2ui::TOOL) {
        v.push(crate::a2ui::spec());
    }
    if allowed("skill") && !skills.is_empty() {
        let names: Vec<Value> = skills.iter().map(|s| Value::String(s.name.clone())).collect();
        v.push(ToolSpec::function(
            "skill",
            "Carica le istruzioni complete di una skill elencata nel prompt di sistema.",
            json!({"type":"object","properties":{"name":{"type":"string","enum":names}},"required":["name"]}),
        ));
    }
    v
}

fn system_prompt(input: &RunInput, skills: &[Skill]) -> String {
    let mut s = String::new();
    s.push_str(if input.agent.system_prompt.trim().is_empty() {
        "Sei un assistente operativo che lavora in locale sul dispositivo dell'utente."
    } else {
        input.agent.system_prompt.trim()
    });
    s.push_str("\n\n# Regole di lavoro\n- Usa gli strumenti per verificare invece di indovinare; leggi prima di modificare.\n- Per i compiti con più passi pubblica il piano con il tool `todo` e aggiornalo.\n- Se un'azione viene negata, non riprovarla identica: proponi un'alternativa.\n- Rispondi nella lingua dell'utente, in modo conciso, e riporta cosa hai fatto davvero.\n");
    s.push_str(&format!("- Modalità di approvazione: {}. Sandbox: {:?}.\n", input.policy.mode, input.tool_ctx.sandbox));
    for extra in &input.system_extra {
        if !extra.trim().is_empty() {
            s.push('\n');
            s.push_str(extra.trim());
            s.push('\n');
        }
    }
    if input.agent.tools.iter().any(|t| t == "*" || t == crate::a2ui::TOOL) {
        s.push('\n');
        s.push_str(crate::a2ui::prompt_hint());
    }
    if !skills.is_empty() {
        s.push_str("\n# Skill disponibili (caricale con il tool `skill` quando servono)\n");
        for sk in skills {
            s.push_str(&format!("- {}: {}\n", sk.name, sk.description));
        }
    }
    s
}

fn estimate_tokens(msgs: &[Msg]) -> u64 {
    msgs.iter()
        .map(|m| {
            m.text().len() as u64 / 3
                + m.tool_calls.as_ref().map(|t| t.iter().map(|c| c.function.arguments.len() as u64 / 3).sum()).unwrap_or(0)
                + 4
        })
        .sum()
}

struct Emitter<'a> {
    host: &'a dyn RunHost,
    run_id: String,
}

impl Emitter<'_> {
    fn emit(&self, e: AgEvent) {
        self.host.emit(&self.run_id, e);
    }
}

#[derive(Default)]
struct PendingCall {
    id: String,
    name: String,
    args: String,
    started: bool,
}

pub struct AgentRunner;

impl AgentRunner {
    pub async fn run(input: RunInput, host: Arc<dyn RunHost>) -> Result<RunOutcome> {
        let ev = Emitter { host: host.as_ref(), run_id: input.run_id.clone() };
        let skills = host.skills();
        let registry = host.tools();
        let allowed = |n: &str| input.agent.tools.iter().any(|t| t == "*" || t == n);
        let mut specs: Vec<ToolSpec> = registry
            .all()
            .into_iter()
            .filter(|t| registry.is_enabled(t.name()) && t.available().is_ok() && allowed(t.name()))
            .map(|t| ToolSpec::function(t.name(), t.description(), t.parameters()))
            .collect();
        specs.extend(internal_specs(&input, &skills));

        let mut outcome = RunOutcome::default();
        let mut state = RunState {
            runtime: Some(input.profile.runtime_id.clone()),
            model: Some(input.profile.model.clone()),
            todos: None,
            usage: None,
        };
        ev.emit(AgEvent::RunStarted { thread_id: input.thread_id.clone(), run_id: input.run_id.clone() });
        ev.emit(AgEvent::StateSnapshot { snapshot: state.clone() });

        // UserPromptSubmit hooks may block the prompt or add context.
        let hook = run_hooks(&input.hooks, "UserPromptSubmit", None, &json!({"prompt": input.user_input, "run_id": input.run_id})).await;
        if let Some(reason) = hook.block {
            ev.emit(AgEvent::RunError { message: format!("Prompt bloccato da un hook: {reason}"), code: Some("hook_blocked".into()) });
            return Err(Error::Denied(reason));
        }
        let mut messages = vec![Msg::system(system_prompt(&input, &skills))];
        messages.extend(input.history.iter().cloned());
        let mut user_text = input.user_input.clone();
        for c in hook.context {
            user_text.push_str(&format!("\n\n[contesto aggiunto da un hook]\n{c}"));
        }
        messages.push(Msg::user(user_text));
        outcome.new_messages.push(ChatMessage {
            id: new_id("msg"),
            role: "user".into(),
            content: input.user_input.clone(),
            reasoning: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
            created_at: now_rfc3339(),
        });

        let backend = match host.backend(&input.profile).await {
            Ok(b) => b,
            Err(e) => {
                ev.emit(AgEvent::RunError { message: format!("Runtime non disponibile: {e}"), code: Some("runtime".into()) });
                return Err(e);
            }
        };
        let ctx_max = input.profile.params.context.map(|c| c as u64).unwrap_or(8192);
        let mut strip_images = false;
        // Tool errors of the latest batch that the model has not retried yet (self-check, below).
        let mut unresolved: Vec<(String, String)> = Vec::new();
        let mut self_checks = 0u32;

        for turn in 0..input.agent.max_turns.max(1) {
            if input.tool_ctx.cancel.is_cancelled() {
                ev.emit(AgEvent::RunError { message: "Run annullato".into(), code: Some("cancelled".into()) });
                return Err(Error::Cancelled);
            }
            // Context compaction (P10).
            if estimate_tokens(&messages) as f64 > input.compact_at * ctx_max as f64 && messages.len() > 6 {
                if let Some(n) = compact(&mut messages, backend.as_ref(), &input).await {
                    ev.emit(AgEvent::Custom { name: "aiw.compacted".into(), value: json!({"removed_messages": n}) });
                }
            }
            let req_messages = if strip_images { without_images(&messages) } else { messages.clone() };
            let req = ChatRequest {
                model: input.profile.model.clone(),
                messages: req_messages,
                tools: specs.clone(),
                temperature: input.profile.params.temperature,
                top_p: input.profile.params.top_p,
                max_tokens: input.profile.params.max_tokens,
                stop: vec![],
            };
            let stream = match backend.chat(req, input.tool_ctx.cancel.clone()).await {
                Ok(s) => s,
                Err(e) if !strip_images && has_images(&messages) && looks_like_vision_error(&e.to_string()) => {
                    strip_images = true;
                    tracing::info!("backend rejected images; retrying without them: {e}");
                    continue;
                }
                Err(Error::Cancelled) => {
                    ev.emit(AgEvent::RunError { message: "Run annullato".into(), code: Some("cancelled".into()) });
                    return Err(Error::Cancelled);
                }
                Err(e) => {
                    ev.emit(AgEvent::RunError { message: format!("Errore del runtime: {e}"), code: Some("runtime".into()) });
                    return Err(e);
                }
            };
            let (text, reasoning, mut calls, usage) = Self::consume(stream, &ev, turn).await?;
            if let Some(u) = usage {
                let mut acc = outcome.usage.clone();
                acc.prompt_tokens = u.prompt_tokens.max(acc.prompt_tokens);
                acc.completion_tokens += u.completion_tokens;
                acc.tokens_per_s = u.tokens_per_s.or(acc.tokens_per_s);
                acc.context_used = Some(u.prompt_tokens + u.completion_tokens);
                acc.context_max = Some(ctx_max);
                outcome.usage = acc;
                state.usage = Some(outcome.usage.clone());
                ev.emit(AgEvent::StateSnapshot { snapshot: state.clone() });
            }
            // Fallback: tool calls written as text.
            let mut text = text;
            if calls.is_empty() && text.contains("<tool_call>") {
                let (clean, parsed) = extract_text_tool_calls(&text);
                text = clean;
                for (i, (name, args)) in parsed.into_iter().enumerate() {
                    let id = format!("call_{}_{i}", new_id("t"));
                    ev.emit(AgEvent::ToolCallStart { tool_call_id: id.clone(), tool_call_name: name.clone(), parent_message_id: None });
                    ev.emit(AgEvent::ToolCallArgs { tool_call_id: id.clone(), delta: args.to_string() });
                    ev.emit(AgEvent::ToolCallEnd { tool_call_id: id.clone() });
                    calls.push(PendingCall { id, name, args: args.to_string(), started: true });
                }
            }
            let tool_calls: Vec<ToolCall> = calls
                .iter()
                .map(|c| ToolCall {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    arguments: if c.args.trim().is_empty() { "{}".into() } else { c.args.clone() },
                })
                .collect();
            messages.push(Msg {
                role: "assistant".into(),
                content: if text.is_empty() && !tool_calls.is_empty() { None } else { Some(MsgContent::Text(text.clone())) },
                tool_calls: (!tool_calls.is_empty()).then(|| {
                    tool_calls
                        .iter()
                        .map(|t| OaiToolCall {
                            id: t.id.clone(),
                            kind: "function".into(),
                            function: OaiFunctionCall { name: t.name.clone(), arguments: t.arguments.clone() },
                        })
                        .collect()
                }),
                ..Default::default()
            });
            outcome.new_messages.push(ChatMessage {
                id: new_id("msg"),
                role: "assistant".into(),
                content: text.clone(),
                reasoning: (!reasoning.is_empty()).then_some(reasoning),
                tool_calls: (!tool_calls.is_empty()).then(|| tool_calls.clone()),
                tool_call_id: None,
                name: None,
                created_at: now_rfc3339(),
            });
            if tool_calls.is_empty() {
                // Small local models often report success right after a failed tool call. Before
                // accepting the final answer, hand the errors back once (at most twice per run).
                if !unresolved.is_empty() && self_checks < MAX_SELF_CHECKS && turn + 1 < input.agent.max_turns {
                    self_checks += 1;
                    let note = self_check_note(&unresolved);
                    ev.emit(AgEvent::Custom {
                        name: "aiw.self_check".into(),
                        value: json!({"failed": unresolved.iter().map(|(n, _)| n).collect::<Vec<_>>()}),
                    });
                    messages.push(Msg::user(note.clone()));
                    outcome.new_messages.push(ChatMessage {
                        id: new_id("msg"),
                        role: "user".into(),
                        content: note,
                        reasoning: None,
                        tool_calls: None,
                        tool_call_id: None,
                        name: Some(SELF_CHECK_NAME.into()),
                        created_at: now_rfc3339(),
                    });
                    unresolved.clear();
                    continue;
                }
                outcome.final_text = text;
                break;
            }
            unresolved.clear();
            // Execute in the order the model asked for: consecutive read-only registry tools
            // run concurrently as one batch; anything with side effects (and internal tools such
            // as todo/agent/skill) runs alone, so a later read sees the effect of an earlier write.
            let mut results: BTreeMap<usize, (ToolCall, ToolOutput)> = BTreeMap::new();
            let mut batches: Vec<Vec<(usize, ToolCall)>> = Vec::new();
            let mut last_parallel = false;
            for (i, tc) in tool_calls.iter().enumerate() {
                let parallel = registry.get(&tc.name).map(|t| t.risk() == Risk::Read).unwrap_or(false);
                match batches.last_mut() {
                    Some(b) if parallel && last_parallel => b.push((i, tc.clone())),
                    _ => batches.push(vec![(i, tc.clone())]),
                }
                last_parallel = parallel;
            }
            'batches: for batch in batches {
                let futs = batch.into_iter().map(|(i, tc)| {
                    let host = host.clone();
                    let input = &input;
                    let skills = &skills;
                    let state = &state;
                    async move {
                        let out = Self::exec_tool(&tc, input, host, skills, state).await;
                        (i, tc, out)
                    }
                });
                for (i, tc, (out, new_todos)) in futures::future::join_all(futs).await {
                    if let Some(t) = new_todos {
                        state.todos = Some(t);
                        ev.emit(AgEvent::StateSnapshot { snapshot: state.clone() });
                    }
                    results.insert(i, (tc, out));
                }
                if input.tool_ctx.cancel.is_cancelled() {
                    break 'batches;
                }
            }
            let mut images = Vec::new();
            for (_, (tc, out)) in results {
                let content = aiw_tools::truncate_middle(&out.content, MAX_TOOL_OUTPUT);
                // User/policy denials are final: they are not handed back by the self-check.
                if out.is_error && !content.starts_with("Negato") && content != "annullato" {
                    unresolved.push((tc.name.clone(), content.chars().take(300).collect()));
                }
                let content = if out.is_error { format!("ERRORE: {content}") } else { content };
                ev.emit(AgEvent::ToolCallResult { message_id: new_id("msg"), tool_call_id: tc.id.clone(), content: content.clone() });
                messages.push(Msg::tool(&tc.id, &tc.name, &content));
                outcome.new_messages.push(ChatMessage {
                    id: new_id("msg"),
                    role: "tool".into(),
                    content,
                    reasoning: None,
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    name: Some(tc.name.clone()),
                    created_at: now_rfc3339(),
                });
                for img in out.images {
                    images.push((tc.name.clone(), img));
                }
            }
            if !images.is_empty() {
                let mut parts = vec![ContentPart::Text {
                    text: format!(
                        "Immagini restituite dagli strumenti: {}",
                        images.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                }];
                parts.extend(images.into_iter().map(|(_, url)| ContentPart::ImageUrl { image_url: ImageUrl { url } }));
                messages.push(Msg { role: "user".into(), content: Some(MsgContent::Parts(parts)), ..Default::default() });
            }
            if turn + 1 == input.agent.max_turns {
                outcome.final_text = format!("{text}\n\n(Limite di {} turni raggiunto: dimmi se devo continuare.)", input.agent.max_turns)
                    .trim()
                    .to_string();
            }
        }
        let stop = run_hooks(&input.hooks, "Stop", None, &json!({"run_id": input.run_id, "final": outcome.final_text})).await;
        if let Some(r) = stop.block {
            tracing::info!("stop hook asked to continue: {r} (not supported in v0.1, recorded only)");
        }
        ev.emit(AgEvent::RunFinished {
            thread_id: input.thread_id.clone(),
            run_id: input.run_id.clone(),
            result: Some(json!({"text": outcome.final_text})),
        });
        Ok(outcome)
    }

    /// Streams one model turn, emitting text/reasoning/tool-call events.
    async fn consume(
        mut stream: aiw_runtimes::DeltaStream,
        ev: &Emitter<'_>,
        turn: u32,
    ) -> Result<(String, String, Vec<PendingCall>, Option<aiw_runtimes::TokenUsage>)> {
        let msg_id = new_id("msg");
        let reason_id = format!("{msg_id}_r");
        let (mut text, mut reasoning) = (String::new(), String::new());
        let (mut text_open, mut reason_open) = (false, false);
        let mut calls: BTreeMap<usize, PendingCall> = BTreeMap::new();
        let mut usage = None;
        let mut splitter = ThinkSplitter::default();
        // Text tool calls are parsed from `raw` at the end but never streamed as visible text.
        let mut tool_filter = crate::think::TagSplitter::new("<tool_call>", "</tool_call>");
        let mut raw = String::new();
        let pieces = |p: Piece, text: &mut String, reasoning: &mut String, text_open: &mut bool, reason_open: &mut bool| match p {
            Piece::Text(t) => {
                if t.is_empty() || (!*text_open && t.trim().is_empty()) {
                    return;
                }
                if *reason_open {
                    ev.emit(AgEvent::ReasoningEnd { message_id: reason_id.clone() });
                    *reason_open = false;
                }
                if !*text_open {
                    ev.emit(AgEvent::TextMessageStart { message_id: msg_id.clone(), role: "assistant".into() });
                    *text_open = true;
                }
                let t = if text.is_empty() { t.trim_start().to_string() } else { t };
                text.push_str(&t);
                ev.emit(AgEvent::TextMessageContent { message_id: msg_id.clone(), delta: t });
            }
            Piece::Reasoning(r) => {
                if r.is_empty() {
                    return;
                }
                if !*reason_open {
                    ev.emit(AgEvent::ReasoningStart { message_id: reason_id.clone() });
                    *reason_open = true;
                }
                reasoning.push_str(&r);
                ev.emit(AgEvent::ReasoningMessageContent { message_id: reason_id.clone(), delta: r });
            }
        };
        while let Some(d) = stream.next().await {
            match d? {
                ChatDelta::Text(t) => {
                    for p in splitter.push(&t) {
                        match p {
                            Piece::Text(visible) => {
                                raw.push_str(&visible);
                                for (inside, s) in tool_filter.push(&visible) {
                                    if !inside {
                                        pieces(Piece::Text(s), &mut text, &mut reasoning, &mut text_open, &mut reason_open);
                                    }
                                }
                            }
                            other => pieces(other, &mut text, &mut reasoning, &mut text_open, &mut reason_open),
                        }
                    }
                }
                ChatDelta::Reasoning(r) => pieces(Piece::Reasoning(r), &mut text, &mut reasoning, &mut text_open, &mut reason_open),
                ChatDelta::ToolCall { index, id, name, arguments } => {
                    let c = calls.entry(index).or_default();
                    if let Some(id) = id {
                        if c.id.is_empty() {
                            c.id = id;
                        }
                    }
                    if let Some(n) = name {
                        c.name.push_str(&n);
                    }
                    if !c.started && !c.name.is_empty() {
                        if c.id.is_empty() {
                            c.id = format!("call_{turn}_{index}_{}", new_id("c"));
                        }
                        ev.emit(AgEvent::ToolCallStart {
                            tool_call_id: c.id.clone(),
                            tool_call_name: c.name.clone(),
                            parent_message_id: Some(msg_id.clone()),
                        });
                        c.started = true;
                        if !c.args.is_empty() {
                            ev.emit(AgEvent::ToolCallArgs { tool_call_id: c.id.clone(), delta: c.args.clone() });
                        }
                    }
                    if !arguments.is_empty() {
                        c.args.push_str(&arguments);
                        if c.started {
                            ev.emit(AgEvent::ToolCallArgs { tool_call_id: c.id.clone(), delta: arguments });
                        }
                    }
                }
                ChatDelta::Usage(u) => usage = Some(u),
                ChatDelta::Done { .. } => {}
            }
        }
        for p in splitter.finish() {
            match p {
                Piece::Text(visible) => {
                    raw.push_str(&visible);
                    for (inside, s) in tool_filter.push(&visible) {
                        if !inside {
                            pieces(Piece::Text(s), &mut text, &mut reasoning, &mut text_open, &mut reason_open);
                        }
                    }
                }
                other => pieces(other, &mut text, &mut reasoning, &mut text_open, &mut reason_open),
            }
        }
        for (inside, s) in tool_filter.finish() {
            if !inside {
                pieces(Piece::Text(s), &mut text, &mut reasoning, &mut text_open, &mut reason_open);
            }
        }
        if reason_open {
            ev.emit(AgEvent::ReasoningEnd { message_id: reason_id.clone() });
        }
        if text_open {
            ev.emit(AgEvent::TextMessageEnd { message_id: msg_id.clone() });
        }
        let calls: Vec<PendingCall> = calls.into_values().filter(|c| !c.name.is_empty()).collect();
        for c in &calls {
            ev.emit(AgEvent::ToolCallEnd { tool_call_id: c.id.clone() });
        }
        // When the model wrote tool calls as text, hand the raw text to the caller for parsing.
        let text_out = if calls.is_empty() && raw.contains("<tool_call>") { raw } else { text };
        Ok((text_out.trim_end().to_string(), reasoning, calls, usage))
    }

    /// Policy → hooks → approval → execution for one call. Returns the output and, for the
    /// `todo` tool, the new plan.
    async fn exec_tool(
        tc: &ToolCall,
        input: &RunInput,
        host: Arc<dyn RunHost>,
        skills: &[Skill],
        state: &RunState,
    ) -> (ToolOutput, Option<Vec<Todo>>) {
        let _ = state;
        let mut args: Value = match serde_json::from_str(if tc.arguments.trim().is_empty() { "{}" } else { &tc.arguments }) {
            Ok(v) => v,
            Err(e) => return (ToolOutput::error(format!("Argomenti JSON non validi ({e}). Riprova con JSON valido.")), None),
        };
        let audit = |decision: &str, detail: String| {
            host.audit(AuditEntry {
                id: new_id("aud"),
                ts: now_rfc3339(),
                run_id: Some(input.run_id.clone()),
                actor: "agent".into(),
                action: "tool".into(),
                tool: Some(tc.name.clone()),
                decision: Some(decision.into()),
                detail,
            });
        };
        // Internal tools.
        match tc.name.as_str() {
            "todo" => {
                let todos: Vec<Todo> =
                    serde_json::from_value(args.get("todos").cloned().unwrap_or(Value::Array(vec![]))).unwrap_or_default();
                let summary = todos.iter().map(|t| format!("[{}] {}", t.status, t.content)).collect::<Vec<_>>().join("\n");
                return (ToolOutput::text(format!("Piano aggiornato:\n{summary}")), Some(todos));
            }
            crate::a2ui::TOOL => {
                return match crate::a2ui::build(&args) {
                    Ok((surface, messages)) => {
                        let n = messages.iter().find_map(|m| m["updateComponents"]["components"].as_array().map(|c| c.len())).unwrap_or(0);
                        host.emit(
                            &input.run_id,
                            AgEvent::Custom { name: crate::a2ui::EVENT.into(), value: json!({"surfaceId": surface, "messages": messages}) },
                        );
                        audit("allow", format!("interfaccia A2UI `{surface}` ({n} componenti)"));
                        (
                            ToolOutput::text(format!(
                                "Interfaccia `{surface}` mostrata all'utente ({n} componenti). Quando l'utente interagisce ricevi un messaggio che inizia con {}: aspettalo invece di inventare i valori.",
                                crate::a2ui::ACTION_PREFIX
                            )),
                            None,
                        )
                    }
                    Err(e) => (ToolOutput::error(e), None),
                };
            }
            "skill" => {
                let name = args["name"].as_str().unwrap_or_default();
                return match skills.iter().find(|s| s.name == name) {
                    Some(s) => {
                        audit("allow", format!("skill {name}"));
                        (ToolOutput::text(skills::load(s)), None)
                    }
                    None => (ToolOutput::error(format!("skill `{name}` non trovata")), None),
                };
            }
            "agent" => {
                let agent_id = args["agent"].as_str().unwrap_or_default().to_string();
                let task = args["task"].as_str().unwrap_or_default().to_string();
                if input.depth >= MAX_DEPTH {
                    return (ToolOutput::error("profondità massima dei subagenti raggiunta"), None);
                }
                let sub_id = new_id("sub");
                host.emit(&input.run_id, AgEvent::SubagentStarted { subagent_id: sub_id.clone(), name: agent_id.clone() });
                audit("allow", format!("subagente {agent_id}: {}", task.chars().take(120).collect::<String>()));
                let r = host.subagent(input, &agent_id, &task).await;
                let (out, res) = match r {
                    Ok(t) => (ToolOutput::text(t.clone()), Some(t)),
                    Err(e) => (ToolOutput::error(format!("subagente fallito: {e}")), None),
                };
                host.emit(
                    &input.run_id,
                    AgEvent::SubagentFinished { subagent_id: sub_id, result: res.map(|r| r.chars().take(300).collect()) },
                );
                return (out, None);
            }
            _ => {}
        }
        let registry = host.tools();
        let Some(tool) = registry.get(&tc.name) else {
            return (ToolOutput::error(format!("Tool sconosciuto `{}`. Usa solo gli strumenti elencati.", tc.name)), None);
        };
        if !input.agent.tools.iter().any(|t| t == "*" || t == &tc.name) || !registry.is_enabled(&tc.name) {
            audit("deny", "tool non abilitato per questo agente".into());
            return (ToolOutput::error(format!("Il tool `{}` non è abilitato per questo agente.", tc.name)), None);
        }
        // PreToolUse hooks.
        let pre =
            run_hooks(&input.hooks, "PreToolUse", Some(&tc.name), &json!({"tool": tc.name, "args": args, "run_id": input.run_id})).await;
        if let Some(reason) = pre.block {
            audit("deny", format!("hook: {reason}"));
            return (ToolOutput::error(format!("Negato da un hook: {reason}")), None);
        }
        if let Some(u) = pre.updated_input {
            args = u;
        }
        let subject = tool.subject(&args);
        let session_ok = input.session_allow.lock().unwrap().contains(&tc.name);
        match input.policy.evaluate(&tc.name, tool.risk(), subject.as_deref(), session_ok) {
            Decision::Allow => audit("allow", subject.clone().unwrap_or_default()),
            Decision::Deny(reason) => {
                audit("deny", format!("{reason} · {}", subject.unwrap_or_default()));
                return (ToolOutput::error(format!("Negato dalla policy: {reason}")), None);
            }
            Decision::Ask(reason) => {
                let req = ApprovalRequest {
                    id: new_id("apr"),
                    run_id: input.run_id.clone(),
                    tool: tc.name.clone(),
                    args: args.clone(),
                    reason: format!("{} · {}", reason_text(&reason, tool.risk()), subject.clone().unwrap_or_default())
                        .trim_end_matches(" · ")
                        .to_string(),
                    risk: risk_level(tool.risk()).into(),
                };
                host.emit(&input.run_id, AgEvent::approval_request(&req));
                let ok = tokio::select! {
                    ok = host.approve(req.clone()) => ok,
                    _ = input.tool_ctx.cancel.cancelled() => false,
                };
                host.emit(&input.run_id, AgEvent::approval_resolved(&req.id, ok));
                if !ok {
                    audit("rejected", subject.unwrap_or_default());
                    return (
                        ToolOutput::error(
                            "Negato dall'utente. Non ripetere la stessa azione: chiedi come procedere o proponi un'alternativa.",
                        ),
                        None,
                    );
                }
                audit("approved", subject.unwrap_or_default());
            }
        }
        let out = match tool.call(args.clone(), &input.tool_ctx).await {
            Ok(o) => o,
            Err(Error::Cancelled) => ToolOutput::error("annullato"),
            Err(e) => ToolOutput::error(e.to_string()),
        };
        let post = run_hooks(
            &input.hooks,
            "PostToolUse",
            Some(&tc.name),
            &json!({"tool": tc.name, "args": args, "output": out.content, "is_error": out.is_error, "run_id": input.run_id}),
        )
        .await;
        let mut out = out;
        for c in post.context {
            out.content.push_str(&format!("\n\n[hook] {c}"));
        }
        (out, None)
    }
}

fn reason_text(reason: &str, risk: Risk) -> String {
    let base = match risk {
        Risk::Write => "L'agente vuole modificare dei file",
        Risk::Exec => "L'agente vuole eseguire un comando",
        Risk::Computer => "L'agente vuole usare mouse e tastiera",
        Risk::Network => "L'agente vuole accedere alla rete",
        Risk::Read => "L'agente vuole leggere dei dati",
    };
    if reason.is_empty() {
        base.to_string()
    } else {
        format!("{base} ({reason})")
    }
}

fn has_images(msgs: &[Msg]) -> bool {
    msgs.iter().any(|m| matches!(&m.content, Some(MsgContent::Parts(p)) if p.iter().any(|x| matches!(x, ContentPart::ImageUrl { .. }))))
}

fn without_images(msgs: &[Msg]) -> Vec<Msg> {
    msgs.iter()
        .map(|m| match &m.content {
            Some(MsgContent::Parts(p)) => {
                let text = p
                    .iter()
                    .filter_map(|x| if let ContentPart::Text { text } = x { Some(text.clone()) } else { None })
                    .collect::<Vec<_>>()
                    .join("\n");
                Msg {
                    content: Some(MsgContent::Text(format!("{text}\n(Immagini omesse: il modello attivo non supporta la visione.)"))),
                    ..m.clone()
                }
            }
            _ => m.clone(),
        })
        .collect()
}

fn looks_like_vision_error(e: &str) -> bool {
    let e = e.to_lowercase();
    e.contains("image") || e.contains("vision") || e.contains("multimodal") || e.contains("mmproj")
}

/// Summarises the oldest part of the conversation in place; returns how many messages were removed.
async fn compact(messages: &mut Vec<Msg>, backend: &dyn ChatBackend, input: &RunInput) -> Option<usize> {
    let keep_tail = 4;
    let mut cut = messages.len().saturating_sub(keep_tail);
    while cut > 1 && messages[cut].role == "tool" {
        cut -= 1; // never separate tool results from the assistant call
    }
    if cut <= 2 {
        return None;
    }
    let old = &messages[1..cut];
    let transcript: String = old
        .iter()
        .map(|m| {
            format!(
                "[{}] {}{}",
                m.role,
                m.text(),
                m.tool_calls
                    .as_ref()
                    .map(|t| format!(" (tool: {})", t.iter().map(|c| c.function.name.clone()).collect::<Vec<_>>().join(", ")))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let req = ChatRequest {
        model: input.profile.model.clone(),
        messages: vec![
            Msg::system("Riassumi la conversazione tra utente e agente per poterla continuare: obiettivo, decisioni, file e risorse toccati, risultati dei tool, passi rimanenti. Massimo 300 parole."),
            Msg::user(transcript.chars().take(60_000).collect::<String>()),
        ],
        temperature: Some(0.2),
        max_tokens: Some(800),
        ..Default::default()
    };
    let mut summary = String::new();
    let mut stream = backend.chat(req, input.tool_ctx.cancel.clone()).await.ok()?;
    while let Some(Ok(d)) = stream.next().await {
        if let ChatDelta::Text(t) = d {
            summary.push_str(&t);
        }
    }
    let summary = extract_text_tool_calls(&summary).0;
    let removed = cut - 1;
    let tail: Vec<Msg> = messages.drain(cut..).collect();
    messages.truncate(1);
    messages.push(Msg::user(format!("[Riassunto automatico del contesto precedente]\n{}", summary.trim())));
    messages.push(Msg::assistant("Ok, riprendo dal riassunto."));
    messages.extend(tail);
    Some(removed)
}
