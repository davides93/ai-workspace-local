//! The tools that talk to the person: `ask_user` (a question with options, a yes or no, a small
//! form), `aiw_secret_request` (a secret typed into a protected field) and `aiw_secret_clear`.
//!
//! `ask_user` and `aiw_secret_request` stop the run until the person answers, in the same way the
//! questions of an MCP server do (`mcp_host.rs`): the request sits in a map of the engine, the tool
//! waits on a channel, an event (`aiw.question`, `aiw.secret_request`) tells the client, and the
//! answer comes back on a REST endpoint (`POST /api/questions/{id}`, `POST /api/secret-requests/{id}`).
//! The wait ends with the answer, with `approvals.timeout_s` (300 s when it is not set), when the run
//! is cancelled, or at the kill switch, and the tool then writes the closing event itself, so it
//! always lands before the end of the run. (A secret that is being written when the time runs out is
//! waited for: the model hears what happened to it, not a `timeout` for a value that is saved.)
//!
//! **A secret never goes through the model.** The model names only a `target` (the destination);
//! the engine checks it, writes in words where the value will go (`destination`: computed here,
//! never taken from the model) and asks. The request keeps the raw identity of the destination
//! (the whole endpoint or URL): when the value arrives the target is resolved again, and the value
//! is written only if it is the destination the person was shown. The value travels in the body of
//! one HTTP request, is written by the existing function of the destination, and is in no tool
//! argument, tool result, event, saved message, audit row, confirmation or error message: the one
//! audit row (`secret.set`) holds the `target` only, and an error of the destination is cleaned of
//! the value before it is returned.

use super::inspect::{args_safe, clip, url_safe};
use super::{require_origin, require_owner};
use crate::{Engine, Inner, PendingQuestion, PendingSecret};
use aiw_core::api::*;
use aiw_core::events::AgEvent;
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use aiw_tools::{Risk, Tool, ToolContext, ToolOutput};
use serde_json::{json, Map, Value};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::oneshot;

/// How long a question or a request for a secret waits when `approvals.timeout_s` is not set
/// (the same as the questions of the MCP servers).
const DEFAULT_WAIT: Duration = Duration::from_secs(300);

/// What the model is told when the client of the run cannot show questions.
const QUESTION_HINT: &str = "Fai la domanda in testo e aspetta il prossimo messaggio.";
/// What the model is told when the client of the run cannot take a secret.
const SECRET_HINT: &str =
    "Non chiedere il segreto in chat. Spiega all'utente che va inserito dall'app su questo computer (nelle impostazioni o nella scheda del servizio).";
/// What the person is told when the destination is not the one the request showed (the value is not written).
const DESTINATION_CHANGED: &str = "La destinazione è cambiata dopo la richiesta: chiedi di nuovo il segreto";
/// What the model is told in the same case, next to the outcome `declined`: the person did not refuse.
const CHANGED_HINT: &str =
    "La destinazione del segreto è cambiata dopo la richiesta e il valore non è stato salvato. Se serve ancora, chiedilo di nuovo con aiw_secret_request.";

/// Longest text answer of a form or free answer «Altro», in characters.
const MAX_ANSWER_CHARS: usize = 2000;
/// Longest secret value, in characters.
const MAX_SECRET_CHARS: usize = 4096;

pub(crate) fn tools(e: &Engine) -> Vec<Arc<dyn Tool>> {
    let weak = Arc::downgrade(&e.0);
    vec![Arc::new(AskUser(weak.clone())), Arc::new(SecretRequestTool(weak.clone())), Arc::new(SecretClear(weak))]
}

fn gone(eng: &Weak<Inner>) -> Result<Engine> {
    eng.upgrade().map(Engine).ok_or_else(|| Error::Conflict("motore in arresto".into()))
}

/// The answer of a tool: a JSON text.
fn json_output(v: Value) -> ToolOutput {
    ToolOutput::text(v.to_string())
}

fn unsupported(hint: &str) -> ToolOutput {
    json_output(json!({"outcome": "unsupported", "hint": hint}))
}

fn not_open(what: &str, id: &str) -> Error {
    Error::NotFound(format!("{what} {id} non trovata (forse già chiusa)"))
}

// ------------------------------------------------------------------ ask_user

struct AskUser(Weak<Inner>);

#[async_trait::async_trait]
impl Tool for AskUser {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn description(&self) -> &str {
        "Fa una domanda all'utente e aspetta la risposta prima di proseguire: a scelta singola o multipla (`choice`), sì o no (`confirm`) o un piccolo modulo (`form`). Usala quando ti serve una decisione o un dato che non ricavi da solo: da 2 a 4 opzioni chiare, la tua preferita per prima. Non serve per chiedere il permesso di un'azione (la conferma arriva da sola) e non va usata per segreti: per quelli c'è `aiw_secret_request`. Se risponde `unsupported`, fai la domanda in testo."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["kind", "message"],
            "properties": {
                "kind": {"type": "string", "enum": ["choice", "confirm", "form"], "description": "choice: scelta tra opzioni; confirm: sì o no; form: uno o più campi da compilare."},
                "title": {"type": "string", "maxLength": 80, "description": "Titolo breve."},
                "message": {"type": "string", "maxLength": 1000, "description": "La domanda."},
                "options": {
                    "type": "array", "minItems": 2, "maxItems": 8,
                    "description": "Solo per choice.",
                    "items": {
                        "type": "object", "required": ["id", "label"],
                        "properties": {
                            "id": {"type": "string", "maxLength": 40},
                            "label": {"type": "string", "maxLength": 80},
                            "description": {"type": "string", "maxLength": 200}
                        }
                    }
                },
                "multiple": {"type": "boolean", "default": false, "description": "Solo per choice: si possono scegliere più opzioni."},
                "allow_other": {"type": "boolean", "default": false, "description": "Solo per choice: l'utente può scrivere una risposta sua («Altro»)."},
                "fields": {
                    "type": "array", "minItems": 1, "maxItems": 8,
                    "description": "Solo per form.",
                    "items": {
                        "type": "object", "required": ["name", "label", "type"],
                        "properties": {
                            "name": {"type": "string", "maxLength": 40},
                            "label": {"type": "string", "maxLength": 80},
                            "type": {"type": "string", "enum": ["string", "number", "integer", "boolean", "enum"]},
                            "options": {"type": "array", "items": {"type": "string"}, "maxItems": 50, "description": "I valori di un campo enum."},
                            "required": {"type": "boolean", "default": false},
                            "placeholder": {"type": "string", "maxLength": 80}
                        }
                    }
                },
                "confirm_label": {"type": "string", "maxLength": 40, "description": "Solo per confirm: testo del pulsante di conferma."},
                "deny_label": {"type": "string", "maxLength": 40, "description": "Solo per confirm: testo del pulsante di rifiuto."}
            }
        })
    }

    fn risk(&self) -> Risk {
        Risk::Read
    }

    fn explicit_grant(&self) -> bool {
        true
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let engine = gone(&self.0)?;
        let origin = require_origin(&engine, ctx)?;
        if !origin.questions {
            return Ok(unsupported(QUESTION_HINT));
        }
        let question = parse_question(&args)?;
        Ok(engine.ask(ctx, question).await)
    }
}

/// The text of an argument, trimmed; `None` when it is missing, not a string or empty.
fn text_arg(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// An optional text of at most `max` characters.
fn optional_text(v: &Value, key: &str, max: usize, what: &str) -> Result<Option<String>> {
    let text = text_arg(v, key);
    if text.as_ref().is_some_and(|t| t.chars().count() > max) {
        return Err(Error::Invalid(format!("{what}: al massimo {max} caratteri")));
    }
    Ok(text)
}

/// A required text of at most `max` characters.
fn required_text(v: &Value, key: &str, max: usize, what: &str) -> Result<String> {
    optional_text(v, key, max, what)?.ok_or_else(|| Error::Invalid(format!("{what}: manca o è vuoto")))
}

/// The question the arguments describe, checked as the schema says (`id`, `run_id` and `expires_at` are
/// filled in when it is asked).
fn parse_question(args: &Value) -> Result<Question> {
    let kind = args.get("kind").and_then(Value::as_str).unwrap_or_default();
    if !["choice", "confirm", "form"].contains(&kind) {
        return Err(Error::Invalid("`kind` deve essere choice, confirm o form".into()));
    }
    let mut q = Question {
        id: String::new(),
        run_id: String::new(),
        kind: kind.into(),
        title: optional_text(args, "title", 80, "`title`")?,
        message: required_text(args, "message", 1000, "`message`")?,
        options: vec![],
        multiple: false,
        allow_other: false,
        fields: vec![],
        confirm_label: None,
        deny_label: None,
        expires_at: None,
    };
    match kind {
        "choice" => {
            q.multiple = args.get("multiple").and_then(Value::as_bool).unwrap_or(false);
            q.allow_other = args.get("allow_other").and_then(Value::as_bool).unwrap_or(false);
            let options = args.get("options").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            if !(2..=8).contains(&options.len()) {
                return Err(Error::Invalid("`options`: una domanda `choice` vuole da 2 a 8 opzioni".into()));
            }
            for o in options {
                let id = required_text(o, "id", 40, "opzione, `id`")?;
                if q.options.iter().any(|x| x.id == id) {
                    return Err(Error::Invalid(format!("opzione «{id}»: gli `id` devono essere tutti diversi")));
                }
                q.options.push(QuestionOption {
                    label: required_text(o, "label", 80, &format!("opzione «{id}», `label`"))?,
                    description: optional_text(o, "description", 200, &format!("opzione «{id}», `description`"))?,
                    id,
                });
            }
        }
        "confirm" => {
            q.confirm_label = optional_text(args, "confirm_label", 40, "`confirm_label`")?;
            q.deny_label = optional_text(args, "deny_label", 40, "`deny_label`")?;
        }
        _ => {
            let fields = args.get("fields").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            if !(1..=8).contains(&fields.len()) {
                return Err(Error::Invalid("`fields`: una domanda `form` vuole da 1 a 8 campi".into()));
            }
            for f in fields {
                let name = required_text(f, "name", 40, "campo, `name`")?;
                if q.fields.iter().any(|x| x.name == name) {
                    return Err(Error::Invalid(format!("campo «{name}»: i `name` devono essere tutti diversi")));
                }
                let what = format!("campo «{name}»");
                let label = required_text(f, "label", 80, &format!("{what}, `label`"))?;
                let placeholder = optional_text(f, "placeholder", 80, &format!("{what}, `placeholder`"))?;
                // A form is not the place to ask for a secret: the value would stay in the conversation.
                for shown in [Some(&name), Some(&label), placeholder.as_ref()].into_iter().flatten() {
                    if aiw_tools::mcp::looks_secret(shown) {
                        return Err(Error::Invalid(format!(
                            "{what}: chiede un segreto (password, token, chiave). Non si può con `ask_user`: usa `aiw_secret_request`"
                        )));
                    }
                }
                let kind = f.get("type").and_then(Value::as_str).unwrap_or_default();
                if !["string", "number", "integer", "boolean", "enum"].contains(&kind) {
                    return Err(Error::Invalid(format!("{what}, `type`: usa string, number, integer, boolean o enum")));
                }
                let mut options = vec![];
                if kind == "enum" {
                    let given = f.get("options").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
                    if given.is_empty() || given.len() > 50 {
                        return Err(Error::Invalid(format!("{what}: un campo `enum` vuole da 1 a 50 `options`")));
                    }
                    for o in given {
                        let o = o.as_str().map(str::trim).filter(|s| !s.is_empty() && s.chars().count() <= 80);
                        let Some(o) = o else {
                            return Err(Error::Invalid(format!("{what}: ogni valore di `options` è un testo da 1 a 80 caratteri")));
                        };
                        if options.iter().any(|x| x == o) {
                            return Err(Error::Invalid(format!("{what}: i valori di `options` devono essere tutti diversi")));
                        }
                        options.push(o.to_string());
                    }
                }
                q.fields.push(QuestionField {
                    name,
                    label,
                    kind: kind.into(),
                    options,
                    required: f.get("required").and_then(Value::as_bool).unwrap_or(false),
                    placeholder,
                });
            }
        }
    }
    Ok(q)
}

/// The answer of the person checked against the question it was shown for (what the form of the UI
/// already makes sure of, repeated here because the endpoint is open to any client). A wrong answer is
/// an error and leaves the question open.
fn check_answer(q: &Question, a: QuestionAnswer) -> Result<QuestionAnswer> {
    let bad = |m: &str| Error::Invalid(m.to_string());
    match q.kind.as_str() {
        "confirm" => {
            let confirmed = a.confirmed.ok_or_else(|| bad("la conferma richiede «sì» o «no»"))?;
            Ok(QuestionAnswer { confirmed: Some(confirmed), ..Default::default() })
        }
        "form" => {
            let mut values = a.values.unwrap_or_default();
            // What the person left empty is not filled in: a text of nothing and a null are the same as no value.
            values.retain(|_, v| !v.is_null() && v.as_str().is_none_or(|s| !s.is_empty()));
            if values.values().any(|v| v.as_str().is_some_and(|s| s.chars().count() > MAX_ANSWER_CHARS)) {
                return Err(bad("un testo è troppo lungo"));
            }
            let mut properties = Map::new();
            for f in &q.fields {
                let schema = match f.kind.as_str() {
                    "enum" => json!({"type": "string", "enum": f.options}),
                    other => json!({"type": other}),
                };
                properties.insert(f.name.clone(), schema);
            }
            let required: Vec<&str> = q.fields.iter().filter(|f| f.required).map(|f| f.name.as_str()).collect();
            let schema = json!({"type": "object", "properties": properties, "required": required});
            let values = aiw_tools::mcp::validate_elicit_content(&schema, &values).map_err(Error::Invalid)?;
            Ok(QuestionAnswer { values: Some(values), ..Default::default() })
        }
        _ => {
            let mut selected: Vec<String> = vec![];
            for id in a.selected.unwrap_or_default() {
                if !q.options.iter().any(|o| o.id == id) {
                    return Err(bad("la risposta nomina un'opzione che non esiste"));
                }
                if !selected.contains(&id) {
                    selected.push(id);
                }
            }
            if !q.multiple && selected.len() > 1 {
                return Err(bad("la domanda ammette una sola risposta"));
            }
            let other = a.other.map(|o| o.trim().to_string()).filter(|o| !o.is_empty());
            if other.is_some() && !q.allow_other {
                return Err(bad("la domanda non ammette una risposta libera"));
            }
            if other.as_ref().is_some_and(|o| o.chars().count() > MAX_ANSWER_CHARS) {
                return Err(bad("la risposta libera è troppo lunga"));
            }
            // With a single answer, «Altro» is an answer of its own: an option and a free text at once is two answers.
            if !q.multiple && !selected.is_empty() && other.is_some() {
                return Err(bad("scegli un'opzione oppure scrivi una risposta tua, non entrambe"));
            }
            if selected.is_empty() && other.is_none() {
                return Err(bad("scegli almeno una risposta"));
            }
            Ok(QuestionAnswer { selected: (!selected.is_empty()).then_some(selected), other, ..Default::default() })
        }
    }
}

/// The result of `ask_user` for the model.
fn question_result(outcome: QuestionOutcome, answer: Option<&QuestionAnswer>) -> Value {
    let mut v = json!({"outcome": outcome.as_str()});
    if let Some(a) = answer {
        v["selected"] = json!(a.selected);
        v["other"] = json!(a.other);
        v["confirmed"] = json!(a.confirmed);
        v["values"] = json!(a.values);
    }
    v
}

/// The moment the engine stops waiting, as the events show it (RFC 3339, to the second).
fn expires_at(wait: Duration) -> String {
    let wait = chrono::Duration::from_std(wait).unwrap_or_else(|_| chrono::Duration::seconds(300));
    (chrono::Utc::now() + wait).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Takes a question out of the map when the wait ends for any reason (answered, expired, cancelled, or
/// the run dropped): a stale one must not stay in the list.
struct WithdrawQuestion<'a>(&'a Engine, String);

impl Drop for WithdrawQuestion<'_> {
    fn drop(&mut self) {
        self.0 .0.questions.lock().unwrap().remove(&self.1);
    }
}

/// The same for a request for a secret.
struct WithdrawSecret<'a>(&'a Engine, String);

impl Drop for WithdrawSecret<'_> {
    fn drop(&mut self) {
        self.0 .0.secret_requests.lock().unwrap().remove(&self.1);
    }
}

impl Engine {
    /// How long a question or a request for a secret waits for the person.
    fn interaction_wait(&self) -> Duration {
        self.approval_timeout().unwrap_or(DEFAULT_WAIT)
    }

    fn audit_interaction(&self, run_id: &str, actor: &str, action: &str, tool: Option<&str>, decision: Option<&str>, detail: &str) {
        let _ = self.0.store.add_audit(&AuditEntry {
            id: new_id("aud"),
            ts: now_rfc3339(),
            run_id: Some(run_id.to_string()),
            actor: actor.into(),
            action: action.into(),
            tool: tool.map(String::from),
            decision: decision.map(String::from),
            detail: detail.into(),
        });
    }

    /// Asks the question, stops the run until it is answered, and returns what the model gets.
    async fn ask(&self, ctx: &ToolContext, mut q: Question) -> ToolOutput {
        let wait = self.interaction_wait();
        q.id = new_id("ask");
        q.run_id = ctx.run_id.clone();
        q.expires_at = Some(expires_at(wait));
        let (tx, rx) = oneshot::channel();
        self.0.questions.lock().unwrap().insert(q.id.clone(), PendingQuestion { question: q.clone(), tx });
        let _withdraw = WithdrawQuestion(self, q.id.clone());
        self.emit(&ctx.run_id, AgEvent::Custom { name: "aiw.question".into(), value: serde_json::to_value(&q).unwrap_or(Value::Null) });
        let (outcome, answer) = tokio::select! {
            biased;
            a = rx => a.unwrap_or((QuestionOutcome::Cancelled, None)),
            _ = ctx.cancel.cancelled() => (QuestionOutcome::Cancelled, None),
            _ = tokio::time::sleep(wait) => (QuestionOutcome::Timeout, None),
        };
        self.0.questions.lock().unwrap().remove(&q.id);
        self.emit(
            &ctx.run_id,
            AgEvent::Custom {
                name: "aiw.question_resolved".into(),
                value: json!({"id": q.id, "outcome": outcome.as_str(), "answer": answer}),
            },
        );
        let actor = if matches!(outcome, QuestionOutcome::Answered | QuestionOutcome::Declined) { "user" } else { "system" };
        self.audit_interaction(&ctx.run_id, actor, "question", Some("ask_user"), Some(outcome.as_str()), &format!("domanda `{}`", q.kind));
        json_output(question_result(outcome, answer.as_ref()))
    }

    /// The questions of the agent that wait for the person (`GET /api/questions`), the oldest first.
    pub fn questions(&self) -> Vec<Question> {
        let mut all: Vec<Question> = self.0.questions.lock().unwrap().values().map(|p| p.question.clone()).collect();
        all.sort_by(|a, b| a.id.cmp(&b.id));
        all
    }

    /// The person answers a question (`answer`) or declines it (`decline`). An answer that does not fit
    /// the question is an error and leaves it open; a question that is closed already is `NotFound`.
    pub fn respond_question(&self, id: &str, reply: QuestionReply) -> Result<()> {
        let question = self.0.questions.lock().unwrap().get(id).map(|p| p.question.clone()).ok_or_else(|| not_open("domanda", id))?;
        let result = match reply.action.as_str() {
            "answer" => (QuestionOutcome::Answered, Some(check_answer(&question, reply.answer)?)),
            "decline" => (QuestionOutcome::Declined, None),
            other => return Err(Error::Invalid(format!("azione `{other}` non valida: usa answer o decline"))),
        };
        let pending = self.0.questions.lock().unwrap().remove(id).ok_or_else(|| not_open("domanda", id))?;
        // The wait may have ended in the meantime (the run was cancelled): then nobody got the answer.
        pending.tx.send(result).map_err(|_| not_open("domanda", id))
    }

    /// Closes every question and request for a secret that waits, each with the outcome `cancelled`:
    /// the tools that wait wake up by themselves (the kill switch calls this first). Returns how many of each.
    pub(crate) fn close_interactions(&self) -> (usize, usize) {
        let questions: Vec<PendingQuestion> = self.0.questions.lock().unwrap().drain().map(|(_, v)| v).collect();
        let secrets: Vec<PendingSecret> = self.0.secret_requests.lock().unwrap().drain().map(|(_, v)| v).collect();
        let counts = (questions.len(), secrets.len());
        for q in questions {
            let _ = q.tx.send((QuestionOutcome::Cancelled, None));
        }
        for s in secrets {
            let _ = s.tx.send((SecretOutcome::Cancelled, None));
        }
        counts
    }
}

// ------------------------------------------------------------------ the destination of a secret

/// Where a secret goes, as the `target` of `aiw_secret_request` names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SecretTarget {
    /// `provider_token:<provider>`
    ProviderToken { id: String },
    /// `mcp_env:<server_id>:<NAME>`
    McpEnv { server: String, name: String },
    /// `mcp_header:<server_id>:<Nome>`
    McpHeader { server: String, name: String },
    /// `setting:<chiave>`
    Setting { key: String },
    /// `runtime_setting:<runtime_id>:<chiave>`
    RuntimeSetting { runtime: String, key: String },
}

/// The slot of a connector that holds a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpSlot {
    Env,
    Header,
}

/// A checked `target`: the destination, the canonical way to write it, and what to tell the person.
struct Resolved {
    target: SecretTarget,
    canonical: String,
    destination: String,
    /// What the destination is right now, from the raw fields that decide where the value goes (the whole
    /// endpoint or URL, not the clipped text of `destination`). The request keeps it, and the value is
    /// written only if resolving the target again gives the same. Never shown, never sent anywhere.
    identity: String,
}

fn shown(s: &str) -> String {
    clip(s, 60)
}

/// Checks a `target` against what the app has now, and describes where the value will go.
fn resolve_target(e: &Engine, raw: &str) -> Result<Resolved> {
    let raw = raw.trim();
    let (kind, rest) = raw.split_once(':').unwrap_or((raw, ""));
    let bad = |m: String| Error::Invalid(m);
    match kind {
        "provider_token" => {
            let provider = e.0.hub.get(rest).map_err(|_| bad(format!("provider `{rest}` sconosciuto: guarda l'elenco con aiw_inspect (topic secrets)")))?;
            let info = provider.info();
            if info.auth == "none" {
                return Err(bad(format!("{} non usa un token", info.name)));
            }
            let host = reqwest::Url::parse(&info.endpoint).ok().and_then(|u| u.host_str().map(String::from));
            let destination = match host {
                Some(h) => format!("{} ({h})", info.name),
                None => info.name.clone(),
            };
            Ok(Resolved {
                target: SecretTarget::ProviderToken { id: info.id.clone() },
                canonical: format!("provider_token:{}", info.id),
                destination,
                identity: json!(["provider_token", info.id, info.endpoint]).to_string(),
            })
        }
        "mcp_env" | "mcp_header" => {
            let env = kind == "mcp_env";
            let (server, name) = rest.split_once(':').filter(|(s, n)| !s.is_empty() && !n.is_empty()).ok_or_else(|| {
                bad(format!("destinazione «{raw}» non valida: la forma è {kind}:<server>:<nome>"))
            })?;
            let cfg = e
                .0
                .store
                .list_docs::<McpServerConfig>("mcp")?
                .into_iter()
                .find(|c| c.id == server || c.name == server)
                .ok_or_else(|| bad(format!("connettore `{server}` non trovato: guarda l'elenco con aiw_inspect (topic connectors)")))?;
            let (map, transport, what) = if env { (&cfg.env, "stdio", "variabile") } else { (&cfg.headers, "http", "header") };
            if cfg.transport != transport {
                let other = if env { "header (mcp_header)" } else { "variabili d'ambiente (mcp_env)" };
                return Err(bad(format!("il connettore «{}» usa {}: per lui valgono le {other}", shown(&cfg.name), cfg.transport)));
            }
            // The name must be declared in the connector already: the model cannot invent a place for a secret.
            let stored = map
                .iter()
                .flatten()
                .map(|(k, _)| k)
                .find(|k| if env { k.as_str() == name } else { k.eq_ignore_ascii_case(name) })
                .cloned()
                .ok_or_else(|| {
                    bad(format!(
                        "{what} «{name}» non dichiarata nel connettore «{}»: aggiungila prima al connettore (anche con valore vuoto), poi chiedi il segreto",
                        shown(&cfg.name)
                    ))
                })?;
            // A value stored under a name that does not look like a secret would be shown by the API as it is.
            if !crate::secrets::is_secret_name(&stored) {
                return Err(bad(format!(
                    "«{stored}» non sembra il nome di un segreto: un valore che non lo è si imposta dal connettore, non con aiw_secret_request"
                )));
            }
            let destination = if env {
                let mut safe = cfg.clone();
                safe.args = cfg.args.as_deref().map(args_safe);
                format!(
                    "Server MCP «{}», variabile {stored}, passata al comando `{}`",
                    shown(&cfg.name),
                    clip(&aiw_tools::mcp::launch::command_line(&safe), 200)
                )
            } else {
                format!(
                    "Server MCP «{}», header {stored}, inviato a {}",
                    shown(&cfg.name),
                    cfg.url.as_deref().map(url_safe).unwrap_or_else(|| "(indirizzo non impostato)".into())
                )
            };
            let target = if env {
                SecretTarget::McpEnv { server: cfg.id.clone(), name: stored.clone() }
            } else {
                SecretTarget::McpHeader { server: cfg.id.clone(), name: stored.clone() }
            };
            // Where the value goes: the URL the header is sent to, or the command that gets the variable.
            let identity = if env {
                json!([kind, cfg.id, stored, cfg.transport, cfg.command.as_deref().map(str::trim), cfg.args])
            } else {
                json!([kind, cfg.id, stored, cfg.transport, cfg.url])
            };
            Ok(Resolved { target, canonical: format!("{kind}:{}:{stored}", cfg.id), destination, identity: identity.to_string() })
        }
        "setting" => {
            let field = e.0.config.schema().properties.get(rest).filter(|f| f.secret).ok_or_else(|| {
                bad(format!("«{rest}» non è un'impostazione segreta di AI Workspace: guarda l'elenco con aiw_inspect (topic secrets)"))
            })?;
            let title = field.title.clone().unwrap_or_else(|| rest.to_string());
            Ok(Resolved {
                target: SecretTarget::Setting { key: rest.into() },
                canonical: format!("setting:{rest}"),
                destination: format!("Impostazione «{}» ({rest}), salvata su questo computer", shown(&title)),
                identity: json!(["setting", rest]).to_string(),
            })
        }
        "runtime_setting" => {
            let (runtime, key) = rest.split_once(':').filter(|(r, k)| !r.is_empty() && !k.is_empty()).ok_or_else(|| {
                bad(format!("destinazione «{raw}» non valida: la forma è runtime_setting:<motore>:<chiave>"))
            })?;
            let info = e
                .runtimes()
                .into_iter()
                .find(|r| r.id == runtime)
                .ok_or_else(|| bad(format!("motore `{runtime}` sconosciuto: guarda l'elenco con aiw_inspect (topic runtimes)")))?;
            let field = info.settings_schema.properties.get(key).filter(|f| f.secret).ok_or_else(|| {
                bad(format!("«{key}» non è un'impostazione segreta del motore «{}»: guarda l'elenco con aiw_inspect (topic secrets)", shown(&info.name)))
            })?;
            let title = field.title.clone().unwrap_or_else(|| key.to_string());
            Ok(Resolved {
                target: SecretTarget::RuntimeSetting { runtime: runtime.into(), key: key.into() },
                canonical: format!("runtime_setting:{runtime}:{key}"),
                destination: format!("Motore «{}»: impostazione «{}» ({key}), salvata su questo computer", shown(&info.name), shown(&title)),
                identity: json!(["runtime_setting", runtime, key]).to_string(),
            })
        }
        _ => Err(bad(format!(
            "destinazione «{}» sconosciuta: usa provider_token:<provider>, mcp_env:<server>:<NOME>, mcp_header:<server>:<Nome>, setting:<chiave> o runtime_setting:<motore>:<chiave>; l'elenco è in aiw_inspect (topic secrets)",
            clip(raw, 80)
        ))),
    }
}

impl Engine {
    /// Writes `value` in the connector `server_id`, in the environment or the headers, with the function
    /// the connector editor uses: the configuration is checked, saved, and the server reconnects with the
    /// new value. The command the person confirmed does not change (its confirmation covers the names of
    /// the variables, not their values), so a confirmed server stays confirmed.
    pub(crate) async fn set_mcp_secret(&self, server_id: &str, slot: McpSlot, name: &str, value: &str) -> Result<()> {
        let mut cfg = self
            .0
            .store
            .get_doc::<McpServerConfig>("mcp", server_id)?
            .ok_or_else(|| Error::NotFound(format!("connettore {server_id} non trovato")))?;
        let map = match slot {
            McpSlot::Env => &mut cfg.env,
            McpSlot::Header => &mut cfg.headers,
        };
        map.get_or_insert_with(Map::new).insert(name.to_string(), Value::String(value.to_string()));
        self.save_mcp_server(cfg).await?;
        Ok(())
    }

    /// Writes (or with `None` clears) the secret of a checked destination, with the function that exists for it.
    async fn write_secret(&self, target: &SecretTarget, value: Option<&str>) -> Result<()> {
        match target {
            SecretTarget::ProviderToken { id } => {
                self.set_provider_auth(id, Some(value.map(String::from)), None).await?;
            }
            SecretTarget::McpEnv { server, name } => self.set_mcp_secret(server, McpSlot::Env, name, value.unwrap_or_default()).await?,
            SecretTarget::McpHeader { server, name } => {
                self.set_mcp_secret(server, McpSlot::Header, name, value.unwrap_or_default()).await?
            }
            SecretTarget::Setting { key } => match value {
                Some(v) => {
                    self.set_config("global", key, json!(v))?;
                }
                None => {
                    // A value kept in another layer (this device, the workspace) would stay in force.
                    let layers = self.0.config.view().layers;
                    for layer in ["global", "device", "workspace"] {
                        if layers.iter().any(|l| l.name == layer && l.values.contains_key(key)) {
                            self.set_config(layer, key, Value::Null)?;
                        }
                    }
                }
            },
            SecretTarget::RuntimeSetting { runtime, key } => {
                let mut settings = Map::new();
                settings.insert(key.clone(), Value::String(value.unwrap_or_default().to_string()));
                self.update_runtime_settings(runtime, settings)?;
            }
        }
        Ok(())
    }
}

/// `e` without `value` in its words: what the destination says about a refusal must not repeat the
/// secret that was refused.
fn without_value(e: Error, value: &str) -> Error {
    let hide = |m: String| m.replace(value, SECRET_MASK);
    match e {
        Error::NotFound(m) => Error::NotFound(hide(m)),
        Error::Invalid(m) => Error::Invalid(hide(m)),
        Error::Unsupported(m) => Error::Unsupported(hide(m)),
        Error::Denied(m) => Error::Denied(hide(m)),
        Error::Conflict(m) => Error::Conflict(hide(m)),
        Error::Runtime(m) => Error::Runtime(hide(m)),
        Error::Unauthorized(m) => Error::Unauthorized(hide(m)),
        Error::InsufficientStorage(m) => Error::InsufficientStorage(hide(m)),
        Error::Upstream(m) => Error::Upstream(hide(m)),
        Error::Cancelled => Error::Cancelled,
        other => Error::Other(hide(other.to_string())),
    }
}

/// The value as the person typed it, cleaned of the blanks a paste brings along and checked: one
/// line, not empty, not the mask the API shows, not absurdly long. The messages say nothing of the value.
fn clean_value(value: Option<String>) -> Result<String> {
    let value = value.unwrap_or_default().trim().to_string();
    if value.is_empty() {
        return Err(Error::Invalid("manca il valore".into()));
    }
    if value.chars().count() > MAX_SECRET_CHARS {
        return Err(Error::Invalid("il valore è troppo lungo".into()));
    }
    if value == SECRET_MASK {
        return Err(Error::Invalid("questo non è il valore, è la maschera che l'app mostra al posto dei segreti".into()));
    }
    if value.chars().any(char::is_control) {
        return Err(Error::Invalid("il valore contiene caratteri non validi (per esempio un a capo)".into()));
    }
    Ok(value)
}

// ------------------------------------------------------------------ aiw_secret_request

struct SecretRequestTool(Weak<Inner>);

#[async_trait::async_trait]
impl Tool for SecretRequestTool {
    fn name(&self) -> &str {
        "aiw_secret_request"
    }

    fn description(&self) -> &str {
        "Chiede all'utente un segreto (token, chiave, password) in un campo protetto: tu non lo vedi mai e il valore va direttamente nella destinazione. Indichi solo `target` (stessa forma dell'elenco di `aiw_inspect`, topic `secrets`: `provider_token:<provider>`, `mcp_env:<connettore>:<NOME>`, `mcp_header:<connettore>:<Nome>`, `setting:<chiave>`, `runtime_setting:<motore>:<chiave>`), un'etichetta e il motivo. Mai chiedere un segreto in chat o con `ask_user`. Se risponde `unsupported`, spiega che va inserito dall'app."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["target", "label"],
            "properties": {
                "target": {"type": "string", "description": "Destinazione del segreto, per esempio provider_token:hf."},
                "label": {"type": "string", "maxLength": 80, "description": "Come si chiama per l'utente, per esempio «Token di Hugging Face»."},
                "reason": {"type": "string", "maxLength": 300, "description": "A che cosa serve."}
            }
        })
    }

    fn risk(&self) -> Risk {
        Risk::Read
    }

    fn explicit_grant(&self) -> bool {
        true
    }

    /// The policy and the activity log see the destination, never a value (there is none to see).
    fn subject(&self, args: &Value) -> Option<String> {
        args.get("target").and_then(Value::as_str).map(|t| clip(t, 120))
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let engine = gone(&self.0)?;
        let origin = require_origin(&engine, ctx)?;
        // A secret is typed at an endpoint that needs full access: a client that cannot show the field,
        // or whose person is not the owner, cannot be asked for one.
        if !origin.secrets || !origin.owner {
            return Ok(unsupported(SECRET_HINT));
        }
        let target = required_text(&args, "target", 200, "`target`")?;
        let label = required_text(&args, "label", 80, "`label`")?;
        let reason = optional_text(&args, "reason", 300, "`reason`")?;
        let resolved = resolve_target(&engine, &target)?;
        Ok(engine.request_secret(ctx, resolved, label, reason).await)
    }
}

impl Engine {
    /// Asks for the secret, stops the run until the person gives it, and returns what the model gets.
    async fn request_secret(&self, ctx: &ToolContext, resolved: Resolved, label: String, reason: Option<String>) -> ToolOutput {
        let wait = self.interaction_wait();
        let request = SecretRequest {
            id: new_id("sec"),
            run_id: ctx.run_id.clone(),
            target: resolved.canonical.clone(),
            label,
            reason,
            destination: resolved.destination,
            expires_at: Some(expires_at(wait)),
        };
        let (tx, rx) = oneshot::channel();
        self.0.secret_requests.lock().unwrap().insert(
            request.id.clone(),
            PendingSecret { request: request.clone(), tx, identity: resolved.identity, busy: false, expired: false },
        );
        let _withdraw = WithdrawSecret(self, request.id.clone());
        self.emit(
            &ctx.run_id,
            AgEvent::Custom { name: "aiw.secret_request".into(), value: serde_json::to_value(&request).unwrap_or(Value::Null) },
        );
        let (outcome, hint) = self.await_secret(&request.id, ctx, wait, rx).await;
        self.0.secret_requests.lock().unwrap().remove(&request.id);
        self.emit(
            &ctx.run_id,
            AgEvent::Custom { name: "aiw.secret_resolved".into(), value: json!({"id": request.id, "outcome": outcome.as_str()}) },
        );
        if outcome != SecretOutcome::Stored {
            // The row of a stored secret (`secret.set`) is written when the value arrives. A `declined` with
            // a hint is the engine closing the request (the destination changed), not the person declining.
            let actor = if outcome == SecretOutcome::Declined && hint.is_none() { "user" } else { "system" };
            let detail = match hint {
                Some(_) => format!("{} (destinazione cambiata dopo la richiesta)", request.target),
                None => request.target.clone(),
            };
            self.audit_interaction(&ctx.run_id, actor, "secret.request", Some("aiw_secret_request"), Some(outcome.as_str()), &detail);
        }
        match outcome {
            SecretOutcome::Stored => json_output(json!({"outcome": "stored", "target": request.target})),
            other => {
                let mut result = json!({"outcome": other.as_str()});
                if let Some(hint) = hint {
                    result["hint"] = json!(hint);
                }
                json_output(result)
            }
        }
    }

    /// Waits for the person to answer a request for a secret: until the answer, the cancellation of the run
    /// or `wait`. A value that is being written when the time runs out is not cut off: the request stays
    /// open until the write ends, and the model gets what really happened (`stored`), not a `timeout`
    /// for a secret that is saved all the same. The write that does not store the value ends the request
    /// with `timeout` (see [`Engine::release_secret`]).
    async fn await_secret(
        &self,
        id: &str,
        ctx: &ToolContext,
        wait: Duration,
        mut rx: oneshot::Receiver<(SecretOutcome, Option<&'static str>)>,
    ) -> (SecretOutcome, Option<&'static str>) {
        tokio::select! {
            biased;
            a = &mut rx => return a.unwrap_or((SecretOutcome::Cancelled, None)),
            _ = ctx.cancel.cancelled() => return (SecretOutcome::Cancelled, None),
            _ = tokio::time::sleep(wait) => {}
        }
        // The time is up. Decided under the lock that the answer takes too, so that the two cannot both win:
        // a request that is not being written is closed here (a value that comes later finds nothing); one
        // that is being written is marked and waited for; one that is gone was just closed by someone else,
        // and their outcome is on its way.
        let closed_here = {
            let mut all = self.0.secret_requests.lock().unwrap();
            match all.get_mut(id) {
                Some(p) if p.busy => {
                    p.expired = true;
                    false
                }
                Some(_) => all.remove(id).is_some(),
                None => false,
            }
        };
        if closed_here {
            return (SecretOutcome::Timeout, None);
        }
        tokio::select! {
            biased;
            a = &mut rx => a.unwrap_or((SecretOutcome::Cancelled, None)),
            _ = ctx.cancel.cancelled() => (SecretOutcome::Cancelled, None),
        }
    }

    /// The requests for a secret that wait for the person (`GET /api/secret-requests`), the oldest first.
    /// They never hold a value.
    pub fn secret_requests(&self) -> Vec<SecretRequest> {
        let mut all: Vec<SecretRequest> = self.0.secret_requests.lock().unwrap().values().map(|p| p.request.clone()).collect();
        all.sort_by(|a, b| a.id.cmp(&b.id));
        all
    }

    /// A value that was being written was not stored. The request is open again for another try, unless the
    /// destination `changed` (the person was shown another one: the request is closed with `declined` and a
    /// hint for the model) or its time ran out while the value was being written (closed with `timeout`).
    fn release_secret(&self, id: &str, changed: bool) {
        let closing = {
            let mut all = self.0.secret_requests.lock().unwrap();
            match all.get_mut(id) {
                Some(p) if p.expired => all.remove(id).map(|p| (p, (SecretOutcome::Timeout, None))),
                Some(_) if changed => all.remove(id).map(|p| (p, (SecretOutcome::Declined, Some(CHANGED_HINT)))),
                Some(p) => {
                    p.busy = false;
                    None
                }
                None => None,
            }
        };
        if let Some((p, end)) = closing {
            let _ = p.tx.send(end);
        }
    }

    /// The person gives the secret (`provide`) or declines (`decline`). `owner` says whether the caller has
    /// full access to this computer; anyone else is refused. A value the destination refuses is an error
    /// without the value in it, and the request stays open for another try; a request closed already is `NotFound`.
    /// A destination that is not the one the request showed any more is a `Conflict` (409): nothing is written
    /// and the request is closed (the model hears `declined` with a hint, see [`DESTINATION_CHANGED`]).
    pub async fn respond_secret(&self, id: &str, answer: SecretAnswer, owner: bool) -> Result<()> {
        if !owner {
            return Err(Error::Denied(
                "Un segreto si può dare solo dall'app su questo computer (accesso completo): da questo dispositivo non è consentito."
                    .into(),
            ));
        }
        match answer.action.as_str() {
            "decline" => {
                let pending = {
                    let mut all = self.0.secret_requests.lock().unwrap();
                    match all.get(id) {
                        None => return Err(not_open("richiesta", id)),
                        Some(p) if p.busy => return Err(Error::Conflict("il valore è in fase di salvataggio".into())),
                        Some(_) => all.remove(id),
                    }
                };
                match pending {
                    Some(p) => p.tx.send((SecretOutcome::Declined, None)).map_err(|_| not_open("richiesta", id)),
                    None => Err(not_open("richiesta", id)),
                }
            }
            "provide" => {
                let value = clean_value(answer.value)?;
                // The request is claimed (no second value meanwhile) but stays in the list until the value is stored.
                let (target, identity, run_id) = {
                    let mut all = self.0.secret_requests.lock().unwrap();
                    let p = all.get_mut(id).ok_or_else(|| not_open("richiesta", id))?;
                    if p.busy {
                        return Err(Error::Conflict("il valore è in fase di salvataggio".into()));
                    }
                    p.busy = true;
                    (p.request.target.clone(), p.identity.clone(), p.request.run_id.clone())
                };
                // The destination is looked at again right before the write: the person was shown where the value
                // goes when the request was made, and it goes there or nowhere. A destination that cannot be resolved
                // any more (the connector is gone) is a destination that changed.
                let resolved = match resolve_target(self, &target) {
                    Ok(r) if r.identity == identity => r,
                    _ => {
                        self.release_secret(id, true);
                        return Err(Error::Conflict(DESTINATION_CHANGED.into()));
                    }
                };
                match self.write_secret(&resolved.target, Some(&value)).await {
                    Err(e) => {
                        self.release_secret(id, false);
                        Err(without_value(e, &value))
                    }
                    Ok(()) => {
                        self.audit_interaction(&run_id, "user", "secret.set", None, None, &target);
                        let pending = self.0.secret_requests.lock().unwrap().remove(id);
                        // The run may have been cancelled while the value was being written: it is stored all the same.
                        if let Some(p) = pending {
                            let _ = p.tx.send((SecretOutcome::Stored, None));
                        }
                        Ok(())
                    }
                }
            }
            other => Err(Error::Invalid(format!("azione `{other}` non valida: usa provide o decline"))),
        }
    }
}

// ------------------------------------------------------------------ aiw_secret_clear

struct SecretClear(Weak<Inner>);

#[async_trait::async_trait]
impl Tool for SecretClear {
    fn name(&self) -> &str {
        "aiw_secret_clear"
    }

    fn description(&self) -> &str {
        "Rimuove un segreto salvato (token, chiave, password), con lo stesso `target` di `aiw_secret_request`. Chiede sempre conferma all'utente; il valore tolto non si può recuperare."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["target"],
            "properties": {
                "target": {"type": "string", "description": "Il segreto da togliere, per esempio provider_token:hf. L'elenco è in aiw_inspect (topic secrets)."}
            }
        })
    }

    fn risk(&self) -> Risk {
        Risk::Write
    }

    fn explicit_grant(&self) -> bool {
        true
    }

    fn subject(&self, args: &Value) -> Option<String> {
        args.get("target").and_then(Value::as_str).map(|t| clip(t, 120))
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let engine = gone(&self.0)?;
        require_owner(&engine, ctx)?;
        let target = required_text(&args, "target", 200, "`target`")?;
        let resolved = resolve_target(&engine, &target)?;
        engine.write_secret(&resolved.target, None).await?;
        engine.audit_interaction(&ctx.run_id, "user", "secret.clear", None, None, &resolved.canonical);
        Ok(json_output(json!({"outcome": "cleared", "target": resolved.canonical})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::management::test_ctx;
    use crate::tests::{engine, wait_finished};
    use crate::RunOrigin;
    use aiw_core::events::AgEvent;
    use aiw_runtimes::{ChatDelta, ScriptedBackend};
    use std::time::Duration;

    const OWNER: RunOrigin = RunOrigin { owner: true, questions: true, secrets: true };
    /// A client that shows questions but cannot take a secret (a paired phone).
    const PHONE: RunOrigin = RunOrigin { owner: false, questions: true, secrets: false };
    /// A client that declared nothing.
    const BARE: RunOrigin = RunOrigin { owner: true, questions: false, secrets: false };

    /// The token the fake Hugging Face accepts, and one it refuses.
    const GOOD_TOKEN: &str = "FAKE_HF_TOKEN_hf_Q7w3Zk9x2LmN8vB4";
    const WRONG_TOKEN: &str = "FAKE_WRONG_TOKEN_p0o9i8u7y6t5";
    const HEADER_VALUE: &str = "FAKE_HEADER_VALUE_Bearer_5c4d3e2f1a";
    const ENV_VALUE: &str = "FAKE_ENV_VALUE_8a7b6c5d4e";
    const SETTING_VALUE: &str = "FAKE_BRAVE_KEY_2f3e4d5c6b";
    const RUNTIME_VALUE: &str = "FAKE_RUNTIME_KEY_9e8d7c6b5a";

    fn answer(action: &str, a: QuestionAnswer) -> QuestionReply {
        QuestionReply { action: action.into(), answer: a }
    }

    fn pick(ids: &[&str]) -> QuestionAnswer {
        QuestionAnswer { selected: Some(ids.iter().map(|s| s.to_string()).collect()), ..Default::default() }
    }

    fn provide(value: &str) -> SecretAnswer {
        SecretAnswer { action: "provide".into(), value: Some(value.into()) }
    }

    fn decline() -> SecretAnswer {
        SecretAnswer { action: "decline".into(), value: None }
    }

    fn models() -> Value {
        json!({
            "kind": "choice", "title": "Quale modello?", "message": "Ho trovato tre varianti adatte a questo computer.",
            "options": [
                {"id": "q4", "label": "Q4_K_M · 4,7 GB", "description": "Consigliata"},
                {"id": "q8", "label": "Q8_0 · 8,5 GB"},
                {"id": "q5", "label": "Q5_K_M · 5,6 GB"}
            ]
        })
    }

    fn form() -> Value {
        json!({
            "kind": "form", "message": "Come lo vuoi?",
            "fields": [
                {"name": "nome", "label": "Nome", "type": "string", "required": true, "placeholder": "Posta"},
                {"name": "lingua", "label": "Lingua", "type": "enum", "options": ["italiano", "inglese"], "required": true},
                {"name": "bozze", "label": "Quante bozze", "type": "integer"},
                {"name": "peso", "label": "Peso", "type": "number"},
                {"name": "da_solo", "label": "Risponde da solo", "type": "boolean"}
            ]
        })
    }

    /// The custom events of a run with this name, in order.
    fn customs(e: &Engine, run_id: &str, name: &str) -> Vec<Value> {
        e.run_events(run_id)
            .into_iter()
            .filter_map(|s| match s.event.event {
                AgEvent::Custom { name: n, value } if n == name => Some(value),
                _ => None,
            })
            .collect()
    }

    async fn wait_until<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        for _ in 0..300 {
            if let Some(v) = f() {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timeout: {what}");
    }

    async fn next_question(e: &Engine) -> Question {
        wait_until("a question", || e.questions().into_iter().next()).await
    }

    async fn next_secret(e: &Engine) -> SecretRequest {
        wait_until("a request for a secret", || e.secret_requests().into_iter().next()).await
    }

    /// The tool call, started in the background: the run id of its context, the task and the question it asked.
    async fn ask_in_background(
        e: &Engine,
        origin: RunOrigin,
        args: Value,
    ) -> (ToolContext, tokio::task::JoinHandle<Result<ToolOutput>>, Question) {
        let tool = e.tool_registry().get("ask_user").expect("ask_user is registered");
        let ctx = test_ctx(e, Some(origin));
        let c = ctx.clone();
        let task = tokio::spawn(async move { tool.call(args, &c).await });
        let q = next_question(e).await;
        (ctx, task, q)
    }

    async fn finished(task: tokio::task::JoinHandle<Result<ToolOutput>>) -> Value {
        let out = tokio::time::timeout(Duration::from_secs(10), task).await.expect("the tool ends").unwrap().unwrap();
        serde_json::from_str(&out.content).unwrap_or_else(|_| panic!("the result is JSON: {}", out.content))
    }

    /// An assistant run with this script; the first message of the person is `text`.
    async fn assistant_run(e: &Engine, turns: Vec<Vec<ChatDelta>>, text: &str, origin: RunOrigin) -> (String, String, ScriptedBackend) {
        let backend = ScriptedBackend::new(turns);
        e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
        let s = e.create_session("assistant", None).unwrap();
        let run = e.start_run_from(&s.id, text.into(), None, None, vec![], Some(origin)).await.unwrap();
        (s.id, run, backend)
    }

    /// The content of the result of the (only) tool call of a session.
    fn tool_result(e: &Engine, session: &str) -> Value {
        let m = e.messages(session).unwrap().into_iter().find(|m| m.role == "tool").expect("a tool result was stored");
        serde_json::from_str(&m.content).unwrap_or_else(|_| panic!("the result is JSON: {}", m.content))
    }

    // ------------------------------------------------------------------ the arguments of ask_user

    #[test]
    fn the_arguments_of_a_question_are_checked_as_the_schema_says() {
        let q = parse_question(&models()).unwrap();
        assert_eq!((q.kind.as_str(), q.title.as_deref(), q.options.len()), ("choice", Some("Quale modello?"), 3));
        assert_eq!(q.options[0].description.as_deref(), Some("Consigliata"));
        assert_eq!(q.options[1].description, None);
        assert!(q.fields.is_empty() && !q.multiple && !q.allow_other && q.confirm_label.is_none());
        let f = parse_question(&form()).unwrap();
        assert_eq!((f.fields.len(), f.fields[1].options.len(), f.fields[0].required, f.fields[2].required), (5, 2, true, false));
        assert!(f.options.is_empty());
        let c = parse_question(&json!({"kind": "confirm", "message": "Li cancello?", "confirm_label": "Cancella", "deny_label": "Tienili", "options": [{"id": "x", "label": "x"}]})).unwrap();
        assert_eq!((c.confirm_label.as_deref(), c.deny_label.as_deref(), c.options.len()), (Some("Cancella"), Some("Tienili"), 0));

        let long = |n: usize| "x".repeat(n);
        let with = |key: &str, v: Value| {
            let mut a = models();
            a[key] = v;
            a
        };
        let opts = |n: usize| Value::Array((0..n).map(|i| json!({"id": format!("o{i}"), "label": format!("Opzione {i}")})).collect());
        let bad: Vec<(&str, Value)> = vec![
            ("kind sconosciuto", json!({"kind": "radio", "message": "?"})),
            ("manca il kind", json!({"message": "?"})),
            ("messaggio mancante", json!({"kind": "confirm"})),
            ("messaggio vuoto", json!({"kind": "confirm", "message": "   "})),
            ("messaggio troppo lungo", json!({"kind": "confirm", "message": long(1001)})),
            ("titolo troppo lungo", with("title", json!(long(81)))),
            ("una sola opzione", with("options", opts(1))),
            ("nove opzioni", with("options", opts(9))),
            ("id doppi", with("options", json!([{"id": "a", "label": "A"}, {"id": "a", "label": "B"}]))),
            ("id troppo lungo", with("options", json!([{"id": long(41), "label": "A"}, {"id": "b", "label": "B"}]))),
            ("etichetta vuota", with("options", json!([{"id": "a", "label": ""}, {"id": "b", "label": "B"}]))),
            (
                "descrizione troppo lunga",
                with("options", json!([{"id": "a", "label": "A", "description": long(201)}, {"id": "b", "label": "B"}])),
            ),
            ("etichetta del pulsante troppo lunga", json!({"kind": "confirm", "message": "?", "confirm_label": long(41)})),
            ("form senza campi", json!({"kind": "form", "message": "?"})),
            (
                "nove campi",
                json!({"kind": "form", "message": "?", "fields": (0..9).map(|i| json!({"name": format!("c{i}"), "label": "C", "type": "string"})).collect::<Vec<_>>()}),
            ),
            (
                "campi con lo stesso nome",
                json!({"kind": "form", "message": "?", "fields": [{"name": "a", "label": "A", "type": "string"}, {"name": "a", "label": "B", "type": "string"}]}),
            ),
            (
                "tipo del campo sconosciuto",
                json!({"kind": "form", "message": "?", "fields": [{"name": "a", "label": "A", "type": "date"}]}),
            ),
            ("enum senza valori", json!({"kind": "form", "message": "?", "fields": [{"name": "a", "label": "A", "type": "enum"}]})),
            (
                "enum con valori doppi",
                json!({"kind": "form", "message": "?", "fields": [{"name": "a", "label": "A", "type": "enum", "options": ["x", "x"]}]}),
            ),
        ];
        for (what, args) in bad {
            assert!(matches!(parse_question(&args), Err(Error::Invalid(_))), "{what} must be refused");
        }
    }

    #[test]
    fn a_form_that_asks_for_a_secret_is_refused_with_a_pointer_to_the_right_tool() {
        for (name, label, placeholder) in [
            ("password", "Valore", None),
            ("valore", "La tua password", None),
            ("campo", "Campo", Some("incolla il token")),
            ("api_key", "Chiave", None),
            ("x", "Chiave privata", None),
        ] {
            let mut field = json!({"name": name, "label": label, "type": "string"});
            if let Some(p) = placeholder {
                field["placeholder"] = json!(p);
            }
            let err = parse_question(&json!({"kind": "form", "message": "Mi serve un dato", "fields": [field]})).unwrap_err();
            assert!(matches!(err, Error::Invalid(_)) && err.to_string().contains("aiw_secret_request"), "{name}/{label}: {err}");
        }
        // The same words in the message or in the title of a question are not a field: the model may talk about them.
        assert!(parse_question(&json!({"kind": "confirm", "title": "Token", "message": "Uso il token già salvato?"})).is_ok());
    }

    // ------------------------------------------------------------------ the answers

    #[test]
    fn an_answer_is_checked_against_the_question_it_was_shown_for() {
        let q = |args: Value| parse_question(&args).unwrap();
        let single = q(models());
        let mut multiple = q(models());
        multiple.multiple = true;
        let mut other_single = q(models());
        other_single.allow_other = true;
        let mut other_multiple = multiple.clone();
        other_multiple.allow_other = true;
        let free = |t: &str| QuestionAnswer { other: Some(t.into()), ..Default::default() };
        let both = |ids: &[&str], t: &str| QuestionAnswer { other: Some(t.into()), ..pick(ids) };

        // A single choice: one id that exists; «Altro» only when it is allowed, and then it is an answer of its own.
        assert_eq!(check_answer(&single, pick(&["q4"])).unwrap(), pick(&["q4"]));
        assert!(check_answer(&single, pick(&["zz"])).is_err(), "an option that does not exist");
        assert!(check_answer(&single, pick(&["q4", "q8"])).is_err(), "two answers to a single choice");
        assert!(check_answer(&single, pick(&[])).is_err(), "no answer at all");
        assert!(check_answer(&single, QuestionAnswer::default()).is_err());
        assert!(check_answer(&single, free("boh")).is_err(), "no free answer unless allowed");
        assert_eq!(check_answer(&other_single, free("  una mia idea ")).unwrap(), free("una mia idea"));
        assert_eq!(
            check_answer(&other_single, QuestionAnswer { other: Some("   ".into()), ..pick(&["q8"]) }).unwrap(),
            pick(&["q8"]),
            "a blank «Altro» is not there"
        );
        assert!(check_answer(&other_single, both(&["q4"], "e anche questo")).is_err(), "single: an option and «Altro» exclude each other");
        // A multiple choice: several ids, the same one once; «Altro» next to them only when allowed.
        assert_eq!(check_answer(&multiple, pick(&["q4", "q8", "q4"])).unwrap(), pick(&["q4", "q8"]));
        assert!(check_answer(&multiple, both(&["q4"], "altro")).is_err(), "multiple, but «Altro» is not allowed");
        assert_eq!(check_answer(&other_multiple, both(&["q4", "q5"], "altro")).unwrap(), both(&["q4", "q5"], "altro"));
        assert_eq!(check_answer(&other_multiple, free("solo questo")).unwrap(), free("solo questo"));
        assert!(check_answer(&other_multiple, free(&"x".repeat(MAX_ANSWER_CHARS + 1))).is_err());

        // A confirmation needs a yes or a no, and carries nothing else.
        let confirm = q(json!({"kind": "confirm", "message": "Procedo?"}));
        let yes = QuestionAnswer { confirmed: Some(true), ..Default::default() };
        assert_eq!(check_answer(&confirm, QuestionAnswer { selected: Some(vec!["x".into()]), ..yes.clone() }).unwrap(), yes);
        assert!(check_answer(&confirm, QuestionAnswer::default()).is_err());

        // A form: the types, the enum, the required fields, nothing unknown; what is empty is not filled in.
        let form = q(form());
        let values = |v: Value| QuestionAnswer { values: Some(v.as_object().unwrap().clone()), ..Default::default() };
        let ok = values(json!({"nome": "Posta", "lingua": "italiano", "bozze": 3, "peso": 0.5, "da_solo": false}));
        assert_eq!(check_answer(&form, ok.clone()).unwrap(), ok);
        let sparse = check_answer(&form, values(json!({"nome": "Posta", "lingua": "inglese", "bozze": null, "peso": ""}))).unwrap();
        assert_eq!(sparse, values(json!({"nome": "Posta", "lingua": "inglese"})));
        for (what, v) in [
            ("a required field is missing", json!({"lingua": "italiano"})),
            ("a required field is empty", json!({"nome": "", "lingua": "italiano"})),
            ("an unknown field", json!({"nome": "P", "lingua": "italiano", "altro": 1})),
            ("a text where a number goes", json!({"nome": "P", "lingua": "italiano", "bozze": "tre"})),
            ("a fraction for an integer", json!({"nome": "P", "lingua": "italiano", "bozze": 1.5})),
            ("a value the enum does not have", json!({"nome": "P", "lingua": "klingon"})),
            ("a number for a text", json!({"nome": 7, "lingua": "italiano"})),
            ("a text for a boolean", json!({"nome": "P", "lingua": "italiano", "da_solo": "sì"})),
            ("a text that is too long", json!({"nome": "x".repeat(MAX_ANSWER_CHARS + 1), "lingua": "italiano"})),
        ] {
            assert!(matches!(check_answer(&form, values(v)), Err(Error::Invalid(_))), "{what}");
        }
        assert!(check_answer(&form, QuestionAnswer::default()).is_err(), "required fields and nothing sent");
    }

    // ------------------------------------------------------------------ ask_user in a run

    #[tokio::test]
    async fn a_run_that_asks_emits_the_question_waits_and_gets_the_answer() {
        let (e, _d, _w) = engine(vec![]).await;
        let turns = vec![ScriptedBackend::tool_turn("a1", "ask_user", models()), ScriptedBackend::text_turn("Scarico la Q4.")];
        let (session, run, backend) = assistant_run(&e, turns, "Scarica un modello", OWNER).await;
        let q = next_question(&e).await;

        // The event has the exact shape of the contract, with an explicit null for what is not there.
        let mut event = customs(&e, &run, "aiw.question").remove(0);
        assert_eq!(event["id"], q.id);
        assert_eq!(event["run_id"], run);
        assert!(q.id.starts_with("ask_") && run.starts_with("run_"));
        let expires = event["expires_at"].as_str().unwrap().to_string();
        let at = chrono::DateTime::parse_from_rfc3339(&expires).expect("RFC 3339");
        let left = (at.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds();
        assert!((290..=300).contains(&left), "five minutes by default: {left}");
        assert!(expires.ends_with('Z'));
        event["id"] = json!("ID");
        event["run_id"] = json!("RUN");
        event["expires_at"] = json!("T");
        assert_eq!(
            event,
            json!({
                "id": "ID", "run_id": "RUN", "kind": "choice", "title": "Quale modello?",
                "message": "Ho trovato tre varianti adatte a questo computer.",
                "options": [
                    {"id": "q4", "label": "Q4_K_M · 4,7 GB", "description": "Consigliata"},
                    {"id": "q8", "label": "Q8_0 · 8,5 GB", "description": null},
                    {"id": "q5", "label": "Q5_K_M · 5,6 GB", "description": null}
                ],
                "multiple": false, "allow_other": false, "fields": [], "confirm_label": null, "deny_label": null, "expires_at": "T"
            })
        );
        assert_eq!(serde_json::to_value(&q).unwrap()["title"], "Quale modello?");

        // The run is stopped: nothing was said after the question, and it is still going.
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!e.run_done(&run));
        assert_eq!(backend.requests.lock().unwrap().len(), 1, "the model was not asked again");

        // A wrong answer is an error and the question stays open; an unknown one is not found.
        let err = e.respond_question(&q.id, answer("answer", pick(&["zz"]))).unwrap_err();
        assert!(matches!(err, Error::Invalid(_)) && err.status() == 400, "{err}");
        assert_eq!(e.questions().len(), 1);
        assert!(matches!(e.respond_question(&q.id, answer("boh", QuestionAnswer::default())), Err(Error::Invalid(_))));
        assert!(matches!(e.respond_question("ask_non_esiste", answer("decline", QuestionAnswer::default())), Err(Error::NotFound(_))));

        e.respond_question(&q.id, answer("answer", pick(&["q4"]))).unwrap();
        wait_finished(&e, &run).await;
        assert!(e.questions().is_empty());
        // Closing event and result, as the contract says.
        assert_eq!(
            customs(&e, &run, "aiw.question_resolved"),
            [json!({"id": q.id, "outcome": "answered", "answer": {"selected": ["q4"], "other": null, "confirmed": null, "values": null}})]
        );
        assert_eq!(
            tool_result(&e, &session),
            json!({"outcome": "answered", "selected": ["q4"], "other": null, "confirmed": null, "values": null})
        );
        // The model got the answer in its next request, and went on.
        let told = backend.requests.lock().unwrap()[1].messages.last().map(|m| m.text()).unwrap_or_default();
        assert!(told.contains("\"selected\":[\"q4\"]"), "{told}");
        assert_eq!(e.messages(&session).unwrap().last().unwrap().content, "Scarico la Q4.");
        // A closed question is not found any more, whoever asks.
        assert!(matches!(e.respond_question(&q.id, answer("decline", QuestionAnswer::default())), Err(Error::NotFound(_))));
        // The activity log has what happened, with no content of the answer.
        let row = e.audit().into_iter().find(|a| a.action == "question").expect("an audit row");
        assert_eq!(
            (row.tool.as_deref(), row.decision.as_deref(), row.run_id.as_deref()),
            (Some("ask_user"), Some("answered"), Some(run.as_str()))
        );
    }

    #[tokio::test]
    async fn every_kind_of_question_comes_back_in_its_own_shape() {
        let (e, _d, _w) = engine(vec![]).await;
        // A confirmation.
        let (_, task, q) = ask_in_background(&e, OWNER, json!({"kind": "confirm", "message": "Procedo?"})).await;
        e.respond_question(&q.id, answer("answer", QuestionAnswer { confirmed: Some(false), ..Default::default() })).unwrap();
        assert_eq!(
            finished(task).await,
            json!({"outcome": "answered", "selected": null, "other": null, "confirmed": false, "values": null})
        );
        // A form.
        let (_, task, q) = ask_in_background(&e, OWNER, form()).await;
        let values = json!({"nome": "Posta", "lingua": "italiano", "da_solo": true}).as_object().unwrap().clone();
        e.respond_question(&q.id, answer("answer", QuestionAnswer { values: Some(values), ..Default::default() })).unwrap();
        let r = finished(task).await;
        assert_eq!(r["values"], json!({"nome": "Posta", "lingua": "italiano", "da_solo": true}));
        assert_eq!((&r["selected"], &r["confirmed"]), (&Value::Null, &Value::Null));
        // A multiple choice with «Altro».
        let mut args = models();
        args["multiple"] = json!(true);
        args["allow_other"] = json!(true);
        let (_, task, q) = ask_in_background(&e, OWNER, args).await;
        assert!(q.multiple && q.allow_other);
        e.respond_question(&q.id, answer("answer", QuestionAnswer { other: Some("Phi".into()), ..pick(&["q4", "q8"]) })).unwrap();
        assert_eq!(
            finished(task).await,
            json!({"outcome": "answered", "selected": ["q4", "q8"], "other": "Phi", "confirmed": null, "values": null})
        );
        // «Non ora»: no answer, nothing but the outcome.
        let (ctx, task, q) = ask_in_background(&e, OWNER, models()).await;
        e.respond_question(&q.id, answer("decline", QuestionAnswer::default())).unwrap();
        assert_eq!(finished(task).await, json!({"outcome": "declined"}));
        assert_eq!(customs(&e, &ctx.run_id, "aiw.question_resolved"), [json!({"id": q.id, "outcome": "declined", "answer": null})]);
        assert!(e.questions().is_empty());
    }

    #[tokio::test]
    async fn the_listing_has_every_open_question_in_order() {
        let (e, _d, _w) = engine(vec![]).await;
        let (_, first, q1) = ask_in_background(&e, OWNER, models()).await;
        let (_, second, _) = {
            let tool = e.tool_registry().get("ask_user").unwrap();
            let ctx = test_ctx(&e, Some(OWNER));
            let task = tokio::spawn({
                let ctx = ctx.clone();
                async move { tool.call(json!({"kind": "confirm", "message": "E questa?"}), &ctx).await }
            });
            wait_until("two questions", || (e.questions().len() == 2).then_some(())).await;
            (ctx, task, ())
        };
        let ids: Vec<String> = e.questions().into_iter().map(|q| q.id).collect();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], q1.id, "the oldest first");
        e.respond_question(&ids[0], answer("decline", QuestionAnswer::default())).unwrap();
        e.respond_question(&ids[1], answer("decline", QuestionAnswer::default())).unwrap();
        finished(first).await;
        finished(second).await;
    }

    #[tokio::test]
    async fn a_run_without_what_it_takes_gets_unsupported_at_once() {
        let (e, _d, _w) = engine(vec![]).await;
        let tool = e.tool_registry().get("ask_user").unwrap();
        // The client declared nothing: `unsupported` with the hint, no event, no question left behind.
        let ctx = test_ctx(&e, Some(BARE));
        let out = tokio::time::timeout(Duration::from_secs(2), tool.call(models(), &ctx)).await.expect("it does not wait").unwrap();
        assert!(!out.is_error);
        assert_eq!(
            serde_json::from_str::<Value>(&out.content).unwrap(),
            json!({"outcome": "unsupported", "hint": "Fai la domanda in testo e aspetta il prossimo messaggio."})
        );
        assert!(customs(&e, &ctx.run_id, "aiw.question").is_empty() && e.questions().is_empty());
        // Even arguments that would not pass: the model is told to ask in text, and that is what it does.
        let out = tool.call(json!({"kind": "?"}), &test_ctx(&e, Some(BARE))).await.unwrap();
        assert!(out.content.contains("unsupported"));
        // No origin at all: the usual refusal of the management tools.
        let err = tool.call(models(), &test_ctx(&e, None)).await.unwrap_err();
        assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("disponibile solo all'Assistente in una chat"), "{err}");
        // A client that can show questions gets asked, even when its person is not the owner (a phone answers them).
        let (_, task, q) = ask_in_background(&e, PHONE, models()).await;
        e.respond_question(&q.id, answer("decline", QuestionAnswer::default())).unwrap();
        finished(task).await;
        // And through a whole run, with the arguments checked: a wrong one is told to the model as an error.
        let turns =
            vec![ScriptedBackend::tool_turn("a1", "ask_user", json!({"kind": "choice", "message": "?"})), ScriptedBackend::text_turn("ok")];
        let (session, run, _) = assistant_run(&e, turns, "chiedi", OWNER).await;
        wait_finished(&e, &run).await;
        let m = e.messages(&session).unwrap().into_iter().find(|m| m.role == "tool").unwrap();
        assert!(m.content.contains("da 2 a 8 opzioni"), "{}", m.content);
        assert!(e.questions().is_empty());
    }

    #[tokio::test]
    async fn the_wait_ends_on_time_with_the_outcome_timeout() {
        let (e, _d, _w) = engine(vec![]).await;
        e.set_config("global", "approvals.timeout_s", json!(1)).unwrap();
        let started = std::time::Instant::now();
        let (ctx, task, q) = ask_in_background(&e, OWNER, models()).await;
        let at = chrono::DateTime::parse_from_rfc3339(q.expires_at.as_deref().unwrap()).unwrap();
        assert!((at.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds() <= 1, "the limit of the setting, not five minutes");
        assert_eq!(finished(task).await, json!({"outcome": "timeout"}));
        assert!(started.elapsed() >= Duration::from_millis(900), "it waited for the limit");
        assert!(e.questions().is_empty(), "an expired question is withdrawn");
        assert_eq!(customs(&e, &ctx.run_id, "aiw.question_resolved"), [json!({"id": q.id, "outcome": "timeout", "answer": null})]);
        assert!(matches!(e.respond_question(&q.id, answer("decline", QuestionAnswer::default())), Err(Error::NotFound(_))), "too late");
        assert!(e.audit().iter().any(|a| a.action == "question" && a.decision.as_deref() == Some("timeout") && a.actor == "system"));
    }

    #[tokio::test]
    async fn cancelling_the_run_closes_the_question_as_cancelled() {
        let (e, _d, _w) = engine(vec![]).await;
        let turns = vec![ScriptedBackend::tool_turn("a1", "ask_user", models()), ScriptedBackend::text_turn("non arrivo qui")];
        let (session, run, _) = assistant_run(&e, turns, "chiedi", OWNER).await;
        let q = next_question(&e).await;
        e.cancel_run(&run).unwrap();
        wait_finished(&e, &run).await;
        assert!(e.questions().is_empty());
        assert_eq!(customs(&e, &run, "aiw.question_resolved"), [json!({"id": q.id, "outcome": "cancelled", "answer": null})]);
        assert_eq!(e.sessions().unwrap().into_iter().find(|s| s.id == session).unwrap().last_status.as_deref(), Some("cancelled"));
        // The closing event comes before the end of the run.
        let events = e.run_events(&run);
        let resolved =
            events.iter().position(|s| matches!(&s.event.event, AgEvent::Custom { name, .. } if name == "aiw.question_resolved")).unwrap();
        let end = events.iter().position(|s| s.event.event.is_terminal()).unwrap();
        assert!(resolved < end);
    }

    #[tokio::test]
    async fn the_kill_switch_closes_questions_and_secret_requests_before_stopping_the_runs() {
        let (e, _d, _w) = engine(vec![]).await;
        let turns = vec![ScriptedBackend::tool_turn("a1", "ask_user", models()), ScriptedBackend::text_turn("non arrivo qui")];
        let (_, ask_run, _) = assistant_run(&e, turns, "chiedi", OWNER).await;
        let q = next_question(&e).await;
        let turns = vec![
            ScriptedBackend::tool_turn(
                "s1",
                "aiw_secret_request",
                json!({"target": "setting:search.brave_api_key", "label": "Chiave di Brave"}),
            ),
            ScriptedBackend::text_turn("non arrivo qui"),
        ];
        let (_, secret_run, _) = assistant_run(&e, turns, "chiedi il segreto", OWNER).await;
        let req = next_secret(&e).await;

        let killed = e.kill().await;
        assert_eq!((killed["questions"].as_u64(), killed["secret_requests"].as_u64()), (Some(1), Some(1)), "{killed}");
        wait_finished(&e, &ask_run).await;
        wait_finished(&e, &secret_run).await;
        assert!(e.questions().is_empty() && e.secret_requests().is_empty());
        assert_eq!(customs(&e, &ask_run, "aiw.question_resolved"), [json!({"id": q.id, "outcome": "cancelled", "answer": null})]);
        assert_eq!(customs(&e, &secret_run, "aiw.secret_resolved"), [json!({"id": req.id, "outcome": "cancelled"})]);
        assert!(e
            .audit()
            .iter()
            .any(|a| a.action == "kill" && a.detail.contains("1 domande") && a.detail.contains("1 richieste di segreti")));
        // What was closed cannot be answered any more.
        assert!(matches!(e.respond_question(&q.id, answer("decline", QuestionAnswer::default())), Err(Error::NotFound(_))));
        assert!(matches!(e.respond_secret(&req.id, provide(SETTING_VALUE), true).await, Err(Error::NotFound(_))));
        assert!(e.0.config.get_str("search.brave_api_key").unwrap_or_default().is_empty(), "nothing was stored");
    }

    // ------------------------------------------------------------------ a fake Hugging Face

    /// A local Hugging Face that accepts one token and answers 401 to any other. Returns its address.
    async fn fake_hf(good: &'static str) -> String {
        fake_hf_after(good, 0).await
    }

    /// The same, answering after `delay_ms` milliseconds (a destination that takes its time).
    async fn fake_hf_after(good: &'static str, delay_ms: u64) -> String {
        use axum::http::{HeaderMap, StatusCode};
        let app = axum::Router::new().route(
            "/api/whoami-v2",
            axum::routing::get(move |h: HeaderMap| async move {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                let given = h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
                if given == format!("Bearer {good}") {
                    (StatusCode::OK, axum::Json(json!({"name": "tester"})))
                } else {
                    (StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "Invalid credentials"})))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://127.0.0.1:{port}")
    }

    /// A fake Hugging Face that accepts `good` and writes down the `Authorization` header of every request it gets
    /// (a destination that would have been sent the value shows it here).
    async fn fake_hf_recording(good: &'static str) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
        use axum::http::{HeaderMap, StatusCode};
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        let app = axum::Router::new().route(
            "/api/whoami-v2",
            axum::routing::get(move |h: HeaderMap| {
                let log = log.clone();
                async move {
                    let given = h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
                    log.lock().unwrap().push(given.clone());
                    if given == format!("Bearer {good}") {
                        (StatusCode::OK, axum::Json(json!({"name": "tester"})))
                    } else {
                        (StatusCode::UNAUTHORIZED, axum::Json(json!({"error": "Invalid credentials"})))
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://127.0.0.1:{port}"), seen)
    }

    /// Every place a value could leak to, as text: the events of the run (live and as stored), the messages
    /// of the session with their tool calls, the whole activity log, the confirmations and the requests
    /// still open, what the model was sent, and the result of the tool.
    fn everything(e: &Engine, session: &str, run: &str, backend: &ScriptedBackend) -> String {
        let live = serde_json::to_string(&e.run_events(run)).unwrap();
        let stored = serde_json::to_string(&e.0.store.events(run).unwrap()).unwrap();
        let messages = serde_json::to_string(&e.messages(session).unwrap()).unwrap();
        let audit = serde_json::to_string(&e.audit()).unwrap();
        let open = serde_json::to_string(&(e.approvals(), e.secret_requests(), e.questions())).unwrap();
        let sent = serde_json::to_string(&*backend.requests.lock().unwrap()).unwrap();
        [live, stored, messages, audit, open, sent].join("\n")
    }

    #[tokio::test]
    async fn a_secret_goes_to_the_provider_and_is_in_none_of_the_places_the_model_or_the_history_see() {
        let (e, _d, _w) = engine(vec![]).await;
        let hf = fake_hf(GOOD_TOKEN).await;
        e.set_provider_auth("hf", None, Some(hf)).await.unwrap();
        let args =
            json!({"target": "provider_token:hf", "label": "Token di Hugging Face", "reason": "CANARY_ARGS serve per i modelli riservati"});
        let turns = vec![ScriptedBackend::tool_turn("s1", "aiw_secret_request", args), ScriptedBackend::text_turn("Il token è salvato.")];
        e.audit_user("canary", "CANARY_AUDIT");
        let (session, run, backend) = assistant_run(&e, turns, "Aggiungi il token di Hugging Face CANARY_USER", OWNER).await;
        let req = next_secret(&e).await;

        // The event has the shape of the contract and says where the value goes, in the engine's words.
        let mut event = customs(&e, &run, "aiw.secret_request").remove(0);
        assert_eq!(event["id"], req.id);
        assert!(req.id.starts_with("sec_"));
        assert!(event["expires_at"].as_str().is_some_and(|t| t.ends_with('Z')));
        event["id"] = json!("ID");
        event["run_id"] = json!("RUN");
        event["expires_at"] = json!("T");
        assert_eq!(
            event,
            json!({
                "id": "ID", "run_id": "RUN", "target": "provider_token:hf", "label": "Token di Hugging Face",
                "reason": "CANARY_ARGS serve per i modelli riservati", "destination": "Hugging Face (127.0.0.1)", "expires_at": "T"
            })
        );
        assert_eq!(req.run_id, run);
        assert!(!e.run_done(&run), "the run waits");

        // A value the service refuses: 401, nothing of the value in the words, the request still open, nothing stored.
        let err = e.respond_secret(&req.id, provide(WRONG_TOKEN), true).await.unwrap_err();
        assert!(matches!(err, Error::Unauthorized(_)) && err.status() == 401, "{err:?}");
        assert!(!err.to_string().contains(WRONG_TOKEN) && !format!("{err:?}").contains(WRONG_TOKEN));
        assert_eq!(e.secret_requests().len(), 1, "it can be tried again");
        assert!(!e.model_providers().into_iter().find(|p| p.id == "hf").unwrap().auth_configured);
        assert!(e.0.store.get_doc::<crate::models::ProviderDoc>("model_provider", "hf").unwrap().is_none_or(|d| d.token.is_none()));
        // Someone who is not the owner cannot give it, and the request stays.
        assert!(matches!(e.respond_secret(&req.id, provide(GOOD_TOKEN), false).await, Err(Error::Denied(_))));
        assert_eq!(e.secret_requests().len(), 1);

        // The right one.
        e.respond_secret(&req.id, provide(GOOD_TOKEN), true).await.unwrap();
        wait_finished(&e, &run).await;
        assert!(e.secret_requests().is_empty());
        assert!(e.model_providers().into_iter().find(|p| p.id == "hf").unwrap().auth_configured, "the token is stored");
        let doc = e.0.store.get_doc::<crate::models::ProviderDoc>("model_provider", "hf").unwrap().expect("stored");
        assert_eq!(doc.token.as_deref(), Some(GOOD_TOKEN));
        assert_eq!(customs(&e, &run, "aiw.secret_resolved"), [json!({"id": req.id, "outcome": "stored"})]);
        assert_eq!(tool_result(&e, &session), json!({"outcome": "stored", "target": "provider_token:hf"}));
        assert_eq!(e.messages(&session).unwrap().last().unwrap().content, "Il token è salvato.");
        // The audit trail has one row for it, with the destination and nothing else.
        let sets: Vec<_> = e.audit().into_iter().filter(|a| a.action == "secret.set").collect();
        assert_eq!(sets.len(), 1);
        assert_eq!(
            (sets[0].detail.as_str(), sets[0].run_id.as_deref(), sets[0].actor.as_str()),
            ("provider_token:hf", Some(run.as_str()), "user")
        );
        // The model asked for it; the engine knows it as a secret from now on.
        assert!(e.known_secrets().iter().any(|s| s == GOOD_TOKEN));
        // Typing the secret is the consent: no confirmation card was asked for (a card would carry the arguments).
        assert!(e.audit().iter().all(|a| a.action != "approval"));

        // The value, and the one that was refused, are nowhere. The canaries show every place is looked at.
        let all = everything(&e, &session, &run, &backend);
        for canary in ["CANARY_USER", "CANARY_ARGS", "CANARY_AUDIT", "aiw.secret_request", "aiw.secret_resolved", "provider_token:hf"] {
            assert!(all.contains(canary), "{canary} is not in the text searched: the search would prove nothing");
        }
        for value in [GOOD_TOKEN, WRONG_TOKEN] {
            assert!(!all.contains(value), "a secret value is in the events, the messages, the audit, the requests or the prompt");
        }
        let seen = format!("{:?} {:?} {:?}", e.secret_requests(), e.approvals(), provide(GOOD_TOKEN));
        assert!(!seen.contains(GOOD_TOKEN), "Debug of the answer hides the value");
    }

    #[tokio::test]
    async fn a_secret_for_each_kind_of_destination_leaves_no_trace_in_the_run_the_history_or_the_audit() {
        let (e, _d, _w) = engine(vec![]).await;
        e.set_provider_auth("hf", None, Some(fake_hf(GOOD_TOKEN).await)).await.unwrap();
        let http = e.save_mcp_server(http_connector()).await.unwrap().config.id;
        let stdio = e.save_mcp_server(stdio_connector()).await.unwrap().config.id;
        let (runtime, key) = e
            .runtimes()
            .into_iter()
            .find_map(|r| r.settings_schema.properties.iter().find(|(_, f)| f.secret).map(|(k, _)| (r.id.clone(), k.clone())))
            .expect("an engine with a secret setting");
        // One request for each of the five kinds of destination, in the same run, one after the other.
        let given: Vec<(String, &str)> = vec![
            ("provider_token:hf".into(), GOOD_TOKEN),
            (format!("mcp_header:{http}:Authorization"), HEADER_VALUE),
            (format!("mcp_env:{stdio}:GITHUB_TOKEN"), ENV_VALUE),
            ("setting:search.brave_api_key".into(), SETTING_VALUE),
            (format!("runtime_setting:{runtime}:{key}"), RUNTIME_VALUE),
        ];
        let mut turns = vec![];
        for (i, (target, _)) in given.iter().enumerate() {
            turns.push(ScriptedBackend::tool_turn(&format!("s{i}"), "aiw_secret_request", json!({"target": target, "label": "Segreto"})));
        }
        turns.push(ScriptedBackend::text_turn("Salvati tutti."));
        let (session, run, backend) = assistant_run(&e, turns, "Aggiungi tutti i segreti", OWNER).await;
        for (target, value) in &given {
            let req = next_secret(&e).await;
            assert_eq!(&req.target, target);
            e.respond_secret(&req.id, provide(value), true).await.unwrap();
        }
        wait_finished(&e, &run).await;
        assert_eq!(e.messages(&session).unwrap().last().unwrap().content, "Salvati tutti.");
        assert_eq!(customs(&e, &run, "aiw.secret_resolved").len(), given.len());
        assert_eq!(e.audit().iter().filter(|a| a.action == "secret.set").count(), given.len());
        // Every value is stored, and every one is a secret the engine knows by value now.
        let known = e.known_secrets();
        for (_, value) in &given {
            assert!(known.iter().any(|s| s == value), "{value} is not stored where the engine reads it");
        }
        // And in none of the places the model, the client or the history see. The targets show the search looks at the right text.
        let all = everything(&e, &session, &run, &backend);
        for (target, value) in &given {
            assert!(all.contains(target.as_str()), "{target} is not in the text searched: the search would prove nothing");
            assert!(!all.contains(value), "{value} (for {target}) is in the events, the messages, the audit, the requests or the prompt");
        }
    }

    #[tokio::test]
    async fn a_secret_refused_by_the_destination_leaves_the_request_open_for_another_try() {
        let (e, _d, _w) = engine(vec![]).await;
        e.set_provider_auth("hf", None, Some(fake_hf(GOOD_TOKEN).await)).await.unwrap();
        let turns = vec![
            ScriptedBackend::tool_turn("s1", "aiw_secret_request", json!({"target": "provider_token:hf", "label": "Token"})),
            ScriptedBackend::text_turn("Fatto."),
        ];
        let (session, run, _) = assistant_run(&e, turns, "token", OWNER).await;
        let req = next_secret(&e).await;
        // A value that is not valid in itself, before any destination is asked.
        for (what, value) in [
            ("empty", Some("   ".to_string())),
            ("missing", None),
            ("the mask", Some(SECRET_MASK.to_string())),
            ("a newline in the middle", Some(format!("{WRONG_TOKEN}\nx"))),
            ("too long", Some("x".repeat(MAX_SECRET_CHARS + 1))),
        ] {
            let err = e.respond_secret(&req.id, SecretAnswer { action: "provide".into(), value: value.clone() }, true).await.unwrap_err();
            assert!(matches!(err, Error::Invalid(_)), "{what}: {err:?}");
            let typed = value.as_deref().map(str::trim).unwrap_or_default();
            assert!(typed.is_empty() || !err.to_string().contains(typed), "{what}: the message repeats the value");
        }
        assert!(matches!(
            e.respond_secret(&req.id, SecretAnswer { action: "boh".into(), value: None }, true).await,
            Err(Error::Invalid(_))
        ));
        // Refused twice, then the right one: the same request takes it, the paste blanks are trimmed.
        for _ in 0..2 {
            let err = e.respond_secret(&req.id, provide(WRONG_TOKEN), true).await.unwrap_err();
            assert_eq!(err.status(), 401);
            assert_eq!(e.secret_requests().len(), 1);
        }
        e.respond_secret(&req.id, provide(&format!("  {GOOD_TOKEN}\n")), true).await.unwrap();
        wait_finished(&e, &run).await;
        assert_eq!(tool_result(&e, &session)["outcome"], "stored");
        assert_eq!(
            e.0.store.get_doc::<crate::models::ProviderDoc>("model_provider", "hf").unwrap().unwrap().token.as_deref(),
            Some(GOOD_TOKEN)
        );
        // Closed: a second answer finds nothing.
        assert!(matches!(e.respond_secret(&req.id, provide(GOOD_TOKEN), true).await, Err(Error::NotFound(_))));
    }

    #[tokio::test]
    async fn while_a_value_is_being_written_no_second_answer_is_taken() {
        let (e, _d, _w) = engine(vec![]).await;
        e.set_provider_auth("hf", None, Some(fake_hf_after(GOOD_TOKEN, 600).await)).await.unwrap();
        let tool = e.tool_registry().get("aiw_secret_request").unwrap();
        let ctx = test_ctx(&e, Some(OWNER));
        let task = tokio::spawn({
            let (ctx, args) = (ctx.clone(), secret_args("provider_token:hf"));
            async move { tool.call(args, &ctx).await }
        });
        let req = next_secret(&e).await;
        let first = tokio::spawn({
            let (e, id) = (e.clone(), req.id.clone());
            async move { e.respond_secret(&id, provide(GOOD_TOKEN), true).await }
        });
        wait_until("the value to be in flight", || e.0.secret_requests.lock().unwrap().get(&req.id).filter(|p| p.busy).map(|_| ())).await;
        // The request is still listed, but neither a second value nor a refusal gets in the way of the first.
        assert_eq!(e.secret_requests().len(), 1);
        assert!(matches!(e.respond_secret(&req.id, provide(GOOD_TOKEN), true).await, Err(Error::Conflict(_))));
        assert!(matches!(e.respond_secret(&req.id, decline(), true).await, Err(Error::Conflict(_))));
        first.await.unwrap().unwrap();
        assert_eq!(finished(task).await, json!({"outcome": "stored", "target": "provider_token:hf"}));
        assert_eq!(e.audit().iter().filter(|a| a.action == "secret.set").count(), 1, "stored once");
    }

    #[tokio::test]
    async fn a_run_cancelled_while_its_value_is_being_written_still_ends_with_the_value_stored_once() {
        let (e, _d, _w) = engine(vec![]).await;
        e.set_provider_auth("hf", None, Some(fake_hf_after(GOOD_TOKEN, 500).await)).await.unwrap();
        let tool = e.tool_registry().get("aiw_secret_request").unwrap();
        let ctx = test_ctx(&e, Some(OWNER));
        let task = tokio::spawn({
            let (ctx, args) = (ctx.clone(), secret_args("provider_token:hf"));
            async move { tool.call(args, &ctx).await }
        });
        let req = next_secret(&e).await;
        let write = tokio::spawn({
            let (e, id) = (e.clone(), req.id.clone());
            async move { e.respond_secret(&id, provide(GOOD_TOKEN), true).await }
        });
        wait_until("the value to be in flight", || e.0.secret_requests.lock().unwrap().get(&req.id).filter(|p| p.busy).map(|_| ())).await;
        ctx.cancel.cancel();
        assert_eq!(finished(task).await, json!({"outcome": "cancelled"}));
        // The write goes on and is honest about it: the value is stored and the request leaves nothing behind.
        write.await.unwrap().unwrap();
        assert!(e.model_providers().into_iter().find(|p| p.id == "hf").unwrap().auth_configured);
        assert!(e.secret_requests().is_empty());
    }

    /// What a test of a value that is still being written when the time runs out follows.
    struct Late {
        /// The tool that waits for the person.
        task: tokio::task::JoinHandle<Result<ToolOutput>>,
        run: String,
        request: SecretRequest,
        /// The answer of the person: it returns when the write ends.
        write: tokio::task::JoinHandle<Result<()>>,
        given: std::time::Instant,
    }

    /// A request for the token of the fake Hugging Face, with a wait of one second, and `value` given at once to a
    /// service that takes `delay_ms` to check it: the value is still being checked when the time of the request runs out.
    async fn written_past_the_time_limit(e: &Engine, value: &'static str, delay_ms: u64) -> Late {
        e.set_config("global", "approvals.timeout_s", json!(1)).unwrap();
        e.set_provider_auth("hf", None, Some(fake_hf_after(GOOD_TOKEN, delay_ms).await)).await.unwrap();
        let tool = e.tool_registry().get("aiw_secret_request").unwrap();
        let ctx = test_ctx(e, Some(OWNER));
        let run = ctx.run_id.clone();
        let task = tokio::spawn({
            let args = secret_args("provider_token:hf");
            async move { tool.call(args, &ctx).await }
        });
        let request = next_secret(e).await;
        let given = std::time::Instant::now();
        let write = tokio::spawn({
            let (e, id) = (e.clone(), request.id.clone());
            async move { e.respond_secret(&id, provide(value), true).await }
        });
        wait_until("the value to be in flight", || e.0.secret_requests.lock().unwrap().get(&request.id).filter(|p| p.busy).map(|_| ()))
            .await;
        assert!(given.elapsed() < Duration::from_millis(900), "the value was in flight before the time ran out");
        Late { task, run, request, write, given }
    }

    #[tokio::test]
    async fn a_value_still_being_written_when_the_time_runs_out_is_not_cut_off_and_the_model_hears_stored() {
        let (e, _d, _w) = engine(vec![]).await;
        let Late { task, run, request: req, write, given } = written_past_the_time_limit(&e, GOOD_TOKEN, 1800).await;
        // The limit (one second) passes while the service still checks the token: the request is not closed.
        tokio::time::sleep(Duration::from_millis(1300)).await;
        assert!(given.elapsed() > Duration::from_millis(1200));
        assert!(!task.is_finished(), "the wait did not end with the time while the value was being written");
        assert_eq!(e.secret_requests().len(), 1);
        assert!(matches!(e.respond_secret(&req.id, provide(GOOD_TOKEN), true).await, Err(Error::Conflict(_))), "no second value meanwhile");

        // The write ends: the secret is stored, and that is what the model is told (not `timeout`, which would make it ask again).
        write.await.unwrap().unwrap();
        assert_eq!(finished(task).await, json!({"outcome": "stored", "target": "provider_token:hf"}));
        assert!(given.elapsed() > Duration::from_millis(1700), "it waited for the write");
        assert!(e.model_providers().into_iter().find(|p| p.id == "hf").unwrap().auth_configured, "the token is stored");
        assert!(e.secret_requests().is_empty());
        assert_eq!(customs(&e, &run, "aiw.secret_resolved"), [json!({"id": req.id, "outcome": "stored"})]);
        assert_eq!(e.audit().iter().filter(|a| a.action == "secret.set").count(), 1, "stored once");
        assert!(e.audit().iter().all(|a| a.action != "secret.request"), "no row that says the request expired");
    }

    #[tokio::test]
    async fn a_value_the_destination_refuses_after_the_time_ran_out_ends_the_request_with_timeout() {
        let (e, _d, _w) = engine(vec![]).await;
        let Late { task, run, request: req, write, given } = written_past_the_time_limit(&e, WRONG_TOKEN, 1800).await;
        // The limit passes while the service still checks the value: the request is not closed by it.
        tokio::time::sleep(Duration::from_millis(1300)).await;
        assert!(
            !task.is_finished() && e.secret_requests().len() == 1,
            "the wait did not end with the time while the value was being written"
        );
        // The write fails after the limit: the person is told why, and the request is over (no retry: the time is up).
        let err = write.await.unwrap().unwrap_err();
        assert!(matches!(err, Error::Unauthorized(_)) && err.status() == 401, "{err:?}");
        assert!(!err.to_string().contains(WRONG_TOKEN) && !format!("{err:?}").contains(WRONG_TOKEN));
        assert_eq!(finished(task).await, json!({"outcome": "timeout"}), "the tool does not wait for ever");
        assert!(given.elapsed() > Duration::from_millis(1700), "the timeout came with the end of the write, not before");
        assert!(e.secret_requests().is_empty());
        assert!(matches!(e.respond_secret(&req.id, provide(GOOD_TOKEN), true).await, Err(Error::NotFound(_))), "too late");
        assert!(!e.model_providers().into_iter().find(|p| p.id == "hf").unwrap().auth_configured);
        assert_eq!(customs(&e, &run, "aiw.secret_resolved"), [json!({"id": req.id, "outcome": "timeout"})]);
        assert!(e.audit().iter().any(|a| a.action == "secret.request" && a.decision.as_deref() == Some("timeout") && a.actor == "system"));
        assert!(e.audit().iter().all(|a| a.action != "secret.set"));
    }

    #[tokio::test]
    async fn a_provider_whose_endpoint_changed_after_the_request_gets_no_value_and_the_request_is_closed() {
        let (e, _d, _w) = engine(vec![]).await;
        let shown_to_the_person = fake_hf(GOOD_TOKEN).await;
        // Another endpoint on the same host: the words the person saw are the same, the place is not.
        let (elsewhere, seen_there) = fake_hf_recording(GOOD_TOKEN).await;
        e.set_provider_auth("hf", None, Some(shown_to_the_person)).await.unwrap();
        let turns = vec![
            ScriptedBackend::tool_turn(
                "s1",
                "aiw_secret_request",
                json!({"target": "provider_token:hf", "label": "Token di Hugging Face"}),
            ),
            ScriptedBackend::text_turn("Va bene."),
        ];
        let (session, run, backend) = assistant_run(&e, turns, "Aggiungi il token", OWNER).await;
        let req = next_secret(&e).await;
        assert_eq!(req.destination, "Hugging Face (127.0.0.1)");
        // From another chat, or from the API: the provider now sends its requests to a different endpoint.
        e.set_provider_auth("hf", None, Some(elsewhere)).await.unwrap();

        let err = e.respond_secret(&req.id, provide(GOOD_TOKEN), true).await.unwrap_err();
        assert!(matches!(err, Error::Conflict(_)) && err.status() == 409, "{err:?}");
        assert_eq!(err.to_string(), "La destinazione è cambiata dopo la richiesta: chiedi di nuovo il segreto");
        assert!(!err.to_string().contains(GOOD_TOKEN) && !format!("{err:?}").contains(GOOD_TOKEN));
        // The value went nowhere: the new endpoint was never asked (it would have been sent the token to check it).
        assert!(seen_there.lock().unwrap().is_empty(), "the token reached the new endpoint: {:?}", seen_there.lock().unwrap());
        assert!(!e.model_providers().into_iter().find(|p| p.id == "hf").unwrap().auth_configured);
        assert!(e.0.store.get_doc::<crate::models::ProviderDoc>("model_provider", "hf").unwrap().is_none_or(|d| d.token.is_none()));
        assert!(!e.known_secrets().iter().any(|s| s == GOOD_TOKEN));

        // The request is closed (not open for a retry that would only fail again), and the model is told what happened.
        wait_finished(&e, &run).await;
        assert!(e.secret_requests().is_empty());
        assert!(matches!(e.respond_secret(&req.id, provide(GOOD_TOKEN), true).await, Err(Error::NotFound(_))));
        assert_eq!(customs(&e, &run, "aiw.secret_resolved"), [json!({"id": req.id, "outcome": "declined"})]);
        let said = tool_result(&e, &session);
        assert_eq!(said["outcome"], "declined", "{said}");
        assert!(said["hint"].as_str().is_some_and(|h| h.contains("destinazione") && h.contains("chiedilo di nuovo")), "{said}");
        assert_eq!(e.messages(&session).unwrap().last().unwrap().content, "Va bene.");
        // The activity log says the engine closed it, and has no row of a stored secret.
        assert!(e.audit().iter().all(|a| a.action != "secret.set"));
        let row = e.audit().into_iter().find(|a| a.action == "secret.request").expect("the closing is logged");
        assert_eq!((row.actor.as_str(), row.decision.as_deref()), ("system", Some("declined")));
        assert!(row.detail.starts_with("provider_token:hf") && row.detail.contains("destinazione cambiata"), "{}", row.detail);
        // The value is in none of the places the model or the history see ("provider_token:hf" shows the search looks at the right text).
        let all = everything(&e, &session, &run, &backend);
        assert!(all.contains("provider_token:hf") && all.contains("destinazione"));
        assert!(!all.contains(GOOD_TOKEN), "the value is in the events, the messages, the audit, the requests or the prompt");
    }

    #[tokio::test]
    async fn a_connector_that_changed_after_the_request_gets_no_value_and_a_harmless_edit_does_not_matter() {
        let (e, _d, _w) = engine(vec![]).await;
        let http = e.save_mcp_server(http_connector()).await.unwrap().config.id;
        let stdio = e.save_mcp_server(stdio_connector()).await.unwrap().config.id;
        let stored = |id: &str| e.0.store.get_doc::<McpServerConfig>("mcp", id).unwrap().unwrap();
        let ask = |target: String| {
            let tool = e.tool_registry().get("aiw_secret_request").unwrap();
            let ctx = test_ctx(&e, Some(OWNER));
            tokio::spawn(async move { tool.call(secret_args(&target), &ctx).await })
        };

        // The header is sent to another URL than the one shown: nothing is written.
        let task = ask(format!("mcp_header:{http}:Authorization"));
        let req = next_secret(&e).await;
        let mut cfg = stored(&http);
        cfg.url = Some("https://altro.example.org/mcp".into());
        e.save_mcp_server(cfg).await.unwrap();
        let err = e.respond_secret(&req.id, provide(HEADER_VALUE), true).await.unwrap_err();
        assert!(matches!(err, Error::Conflict(_)) && err.to_string().contains("La destinazione è cambiata"), "{err:?}");
        let said = finished(task).await;
        assert_eq!(said["outcome"], "declined");
        assert!(said["hint"].is_string());
        assert_eq!(stored(&http).headers.as_ref().unwrap()["Authorization"], "", "nothing was written");
        assert!(e.secret_requests().is_empty());

        // The variable would be passed to another command than the one shown: nothing is written.
        let task = ask(format!("mcp_env:{stdio}:GITHUB_TOKEN"));
        let req = next_secret(&e).await;
        let mut cfg = stored(&stdio);
        cfg.args = Some(vec!["-y".into(), "server-diverso".into()]);
        e.save_mcp_server(cfg).await.unwrap();
        assert!(matches!(e.respond_secret(&req.id, provide(ENV_VALUE), true).await, Err(Error::Conflict(_))));
        assert_eq!(finished(task).await["outcome"], "declined");
        assert_eq!(stored(&stdio).env.as_ref().unwrap()["GITHUB_TOKEN"], "", "nothing was written");

        // A connector that is gone is a destination that changed too, not an error to retry.
        let task = ask(format!("mcp_header:{http}:Authorization"));
        let req = next_secret(&e).await;
        e.remove_mcp_server(&http).await.unwrap();
        assert!(matches!(e.respond_secret(&req.id, provide(HEADER_VALUE), true).await, Err(Error::Conflict(_))));
        assert_eq!(finished(task).await["outcome"], "declined");
        assert!(e.secret_requests().is_empty());

        // What does not decide where the value goes may change: another header of the connector, the value of another variable.
        let task = ask(format!("mcp_env:{stdio}:GITHUB_TOKEN"));
        let req = next_secret(&e).await;
        let mut cfg = stored(&stdio);
        cfg.env.as_mut().unwrap().insert("MODE".into(), json!("altro"));
        e.save_mcp_server(cfg).await.unwrap();
        e.respond_secret(&req.id, provide(ENV_VALUE), true).await.unwrap();
        assert_eq!(finished(task).await, json!({"outcome": "stored", "target": format!("mcp_env:{stdio}:GITHUB_TOKEN")}));
        assert_eq!(stored(&stdio).env.as_ref().unwrap()["GITHUB_TOKEN"], ENV_VALUE);
        assert_eq!(stored(&stdio).env.as_ref().unwrap()["MODE"], "altro");

        // The values that were refused are not in the activity log, and the other one is only stored.
        let audit = serde_json::to_string(&e.audit()).unwrap();
        assert!(!audit.contains(HEADER_VALUE) && !audit.contains(ENV_VALUE));
        assert_eq!(e.audit().iter().filter(|a| a.action == "secret.set").count(), 1);
    }

    #[tokio::test]
    async fn a_request_for_a_secret_can_be_declined_and_a_missing_one_is_not_found() {
        let (e, _d, _w) = engine(vec![]).await;
        let turns = vec![
            ScriptedBackend::tool_turn("s1", "aiw_secret_request", json!({"target": "setting:search.brave_api_key", "label": "Chiave"})),
            ScriptedBackend::text_turn("Va bene."),
        ];
        let (session, run, _) = assistant_run(&e, turns, "chiave", OWNER).await;
        let req = next_secret(&e).await;
        assert!(matches!(e.respond_secret(&req.id, decline(), false).await, Err(Error::Denied(_))), "not from a device");
        e.respond_secret(&req.id, decline(), true).await.unwrap();
        wait_finished(&e, &run).await;
        assert_eq!(tool_result(&e, &session), json!({"outcome": "declined"}));
        assert_eq!(customs(&e, &run, "aiw.secret_resolved"), [json!({"id": req.id, "outcome": "declined"})]);
        assert!(e.0.config.get_str("search.brave_api_key").unwrap_or_default().is_empty());
        assert!(e.audit().iter().any(|a| a.action == "secret.request"
            && a.decision.as_deref() == Some("declined")
            && a.detail == "setting:search.brave_api_key"));
        assert!(e.audit().iter().all(|a| a.action != "secret.set"));
        assert!(matches!(e.respond_secret(&req.id, decline(), true).await, Err(Error::NotFound(_))));
        assert!(matches!(e.respond_secret("sec_non_esiste", provide(GOOD_TOKEN), true).await, Err(Error::NotFound(_))));
    }

    #[tokio::test]
    async fn a_request_for_a_secret_expires_and_is_cancelled_like_a_question() {
        let (e, _d, _w) = engine(vec![]).await;
        let tool = e.tool_registry().get("aiw_secret_request").unwrap();
        let args = json!({"target": "setting:search.brave_api_key", "label": "Chiave"});
        // Time.
        e.set_config("global", "approvals.timeout_s", json!(1)).unwrap();
        let ctx = test_ctx(&e, Some(OWNER));
        let out = tokio::time::timeout(Duration::from_secs(10), tool.call(args.clone(), &ctx)).await.expect("it ends").unwrap();
        assert_eq!(serde_json::from_str::<Value>(&out.content).unwrap(), json!({"outcome": "timeout"}));
        assert!(e.secret_requests().is_empty());
        let resolved = customs(&e, &ctx.run_id, "aiw.secret_resolved");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0]["outcome"], "timeout");
        // Too late: a value that comes after the time finds nothing, and nothing is written.
        let late = customs(&e, &ctx.run_id, "aiw.secret_request").remove(0)["id"].as_str().unwrap().to_string();
        assert!(matches!(e.respond_secret(&late, provide(SETTING_VALUE), true).await, Err(Error::NotFound(_))));
        assert!(e.0.config.get_str("search.brave_api_key").unwrap_or_default().is_empty());
        // The run is cancelled.
        e.set_config("global", "approvals.timeout_s", json!(0)).unwrap();
        let ctx = test_ctx(&e, Some(OWNER));
        let task = tokio::spawn({
            let (tool, ctx, args) = (tool.clone(), ctx.clone(), args.clone());
            async move { tool.call(args, &ctx).await }
        });
        next_secret(&e).await;
        ctx.cancel.cancel();
        assert_eq!(finished(task).await, json!({"outcome": "cancelled"}));
        assert!(e.secret_requests().is_empty());
        assert_eq!(customs(&e, &ctx.run_id, "aiw.secret_resolved")[0]["outcome"], "cancelled");
    }

    #[tokio::test]
    async fn a_secret_is_asked_only_of_the_owner_whose_client_can_take_it() {
        let (e, _d, _w) = engine(vec![]).await;
        let tool = e.tool_registry().get("aiw_secret_request").unwrap();
        let args = json!({"target": "provider_token:hf", "label": "Token"});
        let hint = |v: &Value| v["hint"].as_str().unwrap_or_default().to_string();
        for (who, origin) in [
            ("a client that declared nothing", BARE),
            ("a phone", PHONE),
            ("owner, but secrets not shown", RunOrigin { owner: true, questions: true, secrets: false }),
            ("secrets declared, not the owner", RunOrigin { owner: false, questions: true, secrets: true }),
        ] {
            let ctx = test_ctx(&e, Some(origin));
            let out = tokio::time::timeout(Duration::from_secs(2), tool.call(args.clone(), &ctx)).await.expect("it does not wait").unwrap();
            let v: Value = serde_json::from_str(&out.content).unwrap();
            assert_eq!(v["outcome"], "unsupported", "{who}");
            assert!(hint(&v).contains("dall'app su questo computer"), "{who}: {v}");
            assert!(customs(&e, &ctx.run_id, "aiw.secret_request").is_empty(), "{who}");
        }
        assert!(e.secret_requests().is_empty());
        // No origin: the usual refusal.
        let err = tool.call(args, &test_ctx(&e, None)).await.unwrap_err();
        assert!(matches!(err, Error::Denied(_)), "{err:?}");
    }

    // ------------------------------------------------------------------ the destinations

    fn http_connector() -> McpServerConfig {
        serde_json::from_value(json!({
            "name": "Remoto di prova", "transport": "http", "enabled": false,
            "url": "https://utente:FAKE_URL_PASSWORD_1a2b@api.example.com/mcp/FAKE_PATH_SEGMENT_9a0b1c2d3e4f5g?token=FAKE_QUERY_TOKEN_3c4d",
            "headers": {"Authorization": "", "X-Region": "eu"}
        }))
        .unwrap()
    }

    fn stdio_connector() -> McpServerConfig {
        serde_json::from_value(json!({
            "name": "Locale di prova", "transport": "stdio", "enabled": false, "command": "npx",
            "args": ["-y", "server-github", "--token", "FAKE_ARG_VALUE_7e8f9a"],
            "env": {"GITHUB_TOKEN": "", "MODE": "test"}
        }))
        .unwrap()
    }

    fn secret_args(target: &str) -> Value {
        json!({"target": target, "label": "Segreto"})
    }

    /// Asks for `target` in a run, gives `value`, and returns what the tool said.
    async fn give(e: &Engine, target: &str, value: &str) -> Value {
        let tool = e.tool_registry().get("aiw_secret_request").unwrap();
        let ctx = test_ctx(e, Some(OWNER));
        let task = tokio::spawn({
            let (tool, ctx, args) = (tool.clone(), ctx.clone(), secret_args(target));
            async move { tool.call(args, &ctx).await }
        });
        let req = next_secret(e).await;
        e.respond_secret(&req.id, provide(value), true).await.unwrap();
        finished(task).await
    }

    async fn refused(e: &Engine, target: &str) -> String {
        let tool = e.tool_registry().get("aiw_secret_request").unwrap();
        let err = tool.call(secret_args(target), &test_ctx(e, Some(OWNER))).await.unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "{target}: {err:?}");
        assert!(e.secret_requests().is_empty(), "nothing was asked for {target}");
        err.to_string()
    }

    #[tokio::test]
    async fn the_header_of_a_connector_takes_the_secret_and_the_request_says_where_it_goes() {
        let (e, _d, _w) = engine(vec![]).await;
        let id = e.save_mcp_server(http_connector()).await.unwrap().config.id;
        let tool = e.tool_registry().get("aiw_secret_request").unwrap();
        let ctx = test_ctx(&e, Some(OWNER));
        let task = tokio::spawn({
            let (ctx, args) = (ctx.clone(), secret_args("mcp_header:Remoto_di_prova:authorization"));
            async move { tool.call(args, &ctx).await }
        });
        let req = next_secret(&e).await;
        // The connector is found by its name too, the header whatever its case, and the target is made canonical.
        assert_eq!(req.target, format!("mcp_header:{id}:Authorization"));
        assert_eq!(req.destination, "Server MCP «Remoto_di_prova», header Authorization, inviato a https://api.example.com/mcp/omesso");
        for leaked in ["FAKE_URL_PASSWORD", "FAKE_PATH_SEGMENT", "FAKE_QUERY_TOKEN", "utente"] {
            assert!(!req.destination.contains(leaked), "{leaked} in {}", req.destination);
        }
        e.respond_secret(&req.id, provide(HEADER_VALUE), true).await.unwrap();
        assert_eq!(finished(task).await, json!({"outcome": "stored", "target": format!("mcp_header:{id}:Authorization")}));
        // The value is where the connector reads it, the other header is untouched, and the API shows the mask.
        let stored = e.0.store.get_doc::<McpServerConfig>("mcp", &id).unwrap().unwrap();
        assert_eq!(stored.headers.as_ref().unwrap()["Authorization"], HEADER_VALUE);
        assert_eq!(stored.headers.as_ref().unwrap()["X-Region"], "eu");
        let shown = serde_json::to_string(&e.mcp_servers()).unwrap();
        assert!(!shown.contains(HEADER_VALUE) && shown.contains(SECRET_MASK));
        assert!(e.known_secrets().iter().any(|s| s == HEADER_VALUE));
        // What cannot be a destination: another transport's slot, an undeclared name, a name that is no secret.
        assert!(refused(&e, &format!("mcp_env:{id}:GITHUB_TOKEN")).await.contains("usa http"));
        assert!(refused(&e, &format!("mcp_header:{id}:X-Nuovo-Token")).await.contains("non dichiarata"));
        assert!(refused(&e, &format!("mcp_header:{id}:X-Region")).await.contains("non sembra il nome di un segreto"));
        assert!(refused(&e, "mcp_header:inesistente:Authorization").await.contains("non trovato"));
        assert!(refused(&e, &format!("mcp_header:{id}")).await.contains("la forma è"));
    }

    #[tokio::test]
    async fn the_environment_of_a_confirmed_stdio_connector_takes_the_secret_and_stays_confirmed() {
        let (e, _d, _w) = engine(vec![]).await;
        let info = e.save_mcp_server(stdio_connector()).await.unwrap();
        let id = info.config.id.clone();
        e.approve_mcp_server(&id, &aiw_tools::mcp::launch::approval_hash(&info.config)).await.unwrap();
        assert!(e.mcp_approved(&e.0.store.get_doc::<McpServerConfig>("mcp", &id).unwrap().unwrap()));

        let tool = e.tool_registry().get("aiw_secret_request").unwrap();
        let ctx = test_ctx(&e, Some(OWNER));
        let task = tokio::spawn({
            let (ctx, args) = (ctx.clone(), secret_args(&format!("mcp_env:{id}:GITHUB_TOKEN")));
            async move { tool.call(args, &ctx).await }
        });
        let req = next_secret(&e).await;
        // The command shown to the person has no value of an argument that follows a flag that names a secret.
        assert_eq!(
            req.destination,
            "Server MCP «Locale_di_prova», variabile GITHUB_TOKEN, passata al comando `npx -y server-github --token ••••••`"
        );
        assert!(!req.destination.contains("FAKE_ARG_VALUE"));
        e.respond_secret(&req.id, provide(ENV_VALUE), true).await.unwrap();
        finished(task).await;
        let stored = e.0.store.get_doc::<McpServerConfig>("mcp", &id).unwrap().unwrap();
        assert_eq!(stored.env.as_ref().unwrap()["GITHUB_TOKEN"], ENV_VALUE);
        assert_eq!(stored.env.as_ref().unwrap()["MODE"], "test");
        assert!(e.mcp_approved(&stored), "the confirmation covers the names of the variables, not their values: it still stands");
        assert!(!serde_json::to_string(&e.mcp_servers()).unwrap().contains(ENV_VALUE));
        // A stdio connector has no headers, and the plain variable is not a secret slot.
        assert!(refused(&e, &format!("mcp_header:{id}:Authorization")).await.contains("usa stdio"));
        assert!(refused(&e, &format!("mcp_env:{id}:MODE")).await.contains("non sembra il nome di un segreto"));
        assert!(refused(&e, &format!("mcp_env:{id}:github_token")).await.contains("non dichiarata"), "variable names are exact");
    }

    #[tokio::test]
    async fn an_app_setting_and_an_engine_setting_take_the_secret_only_when_the_schema_calls_it_secret() {
        let (e, _d, _w) = engine(vec![]).await;
        let said = give(&e, "setting:search.brave_api_key", SETTING_VALUE).await;
        assert_eq!(said, json!({"outcome": "stored", "target": "setting:search.brave_api_key"}));
        assert_eq!(e.0.config.get_str("search.brave_api_key").as_deref(), Some(SETTING_VALUE));
        assert!(!serde_json::to_string(&e.config()).unwrap().contains(SETTING_VALUE), "the API shows the mask");
        // The settings that are not secret, and the ones that do not exist, are not destinations.
        assert!(refused(&e, "setting:ui.theme").await.contains("non è un'impostazione segreta"));
        assert!(refused(&e, "setting:non.esiste").await.contains("non è un'impostazione segreta"));
        // An engine with a secret setting in its manifest.
        let (runtime, key) = e
            .runtimes()
            .into_iter()
            .find_map(|r| r.settings_schema.properties.iter().find(|(_, f)| f.secret).map(|(k, _)| (r.id.clone(), k.clone())))
            .expect("an engine with a secret setting");
        let target = format!("runtime_setting:{runtime}:{key}");
        assert_eq!(give(&e, &target, RUNTIME_VALUE).await, json!({"outcome": "stored", "target": target}));
        assert_eq!(e.0.runtimes.settings(&runtime).unwrap()[&key], RUNTIME_VALUE);
        assert!(!serde_json::to_string(&e.runtimes()).unwrap().contains(RUNTIME_VALUE));
        assert!(e.0.store.get_doc::<Value>("runtime_settings", &runtime).unwrap().is_some(), "it is persisted like the editor does");
        assert!(refused(&e, &format!("runtime_setting:{runtime}:nessuna_chiave"))
            .await
            .contains("non è un'impostazione segreta del motore"));
        assert!(refused(&e, "runtime_setting:motore-che-non-c-e:x").await.contains("motore"));
        // A provider that has no token, an unknown one, a destination of a kind that does not exist.
        assert!(refused(&e, "provider_token:ollama").await.contains("non usa un token"));
        assert!(refused(&e, "provider_token:nessuno").await.contains("sconosciuto"));
        assert!(refused(&e, "password_di_sistema:x").await.contains("destinazione"));
        assert!(refused(&e, "").await.contains("manca"));
    }

    #[tokio::test]
    async fn the_destination_is_written_by_the_engine_in_the_words_of_the_plan() {
        let (e, _d, _w) = engine(vec![]).await;
        let said = |target: &str| resolve_target(&e, target).unwrap_or_else(|err| panic!("{target}: {err}"));
        // The default Hugging Face: the name and the host the token will be sent to.
        let hf = said("provider_token:hf");
        assert_eq!((hf.canonical.as_str(), hf.destination.as_str()), ("provider_token:hf", "Hugging Face (huggingface.co)"));
        assert_eq!(hf.target, SecretTarget::ProviderToken { id: "hf".into() });
        // Surrounding blanks do not matter.
        assert_eq!(said("  provider_token:hf ").canonical, "provider_token:hf");
        let brave = said("setting:search.brave_api_key");
        assert_eq!(brave.destination, "Impostazione «Chiave di Brave Search» (search.brave_api_key), salvata su questo computer");
        let (runtime, key) = e
            .runtimes()
            .into_iter()
            .find_map(|r| r.settings_schema.properties.iter().find(|(_, f)| f.secret).map(|(k, _)| (r.id.clone(), k.clone())))
            .unwrap();
        let rt = said(&format!("runtime_setting:{runtime}:{key}"));
        assert!(
            rt.destination.starts_with("Motore «") && rt.destination.contains(&format!("({key}), salvata su questo computer")),
            "{}",
            rt.destination
        );
        // A destination that sends the value to another host says so: the model cannot hide where it goes.
        e.set_provider_auth("hf", None, Some("http://127.0.0.1:9".into())).await.unwrap();
        assert_eq!(said("provider_token:hf").destination, "Hugging Face (127.0.0.1)");
    }

    #[tokio::test]
    async fn a_secret_that_is_stored_is_hidden_by_the_diagnostics_the_traces_and_aiw_inspect() {
        let (e, _d, _w) = engine(vec![]).await;
        give(&e, "setting:search.brave_api_key", SETTING_VALUE).await;
        let inspect = super::super::inspect::render(&e, &json!({"topic": "secrets"})).await.unwrap();
        assert!(!inspect.contains(SETTING_VALUE) && inspect.contains("\"set\":true"), "{inspect}");
        let diagnostics = e.diagnostics().await.unwrap().to_string();
        assert!(!diagnostics.contains(SETTING_VALUE));
    }

    // ------------------------------------------------------------------ aiw_secret_clear

    #[tokio::test]
    async fn clearing_a_secret_removes_it_from_where_it_was_stored_and_asks_for_confirmation_with_the_target_only() {
        let (e, _d, _w) = engine(vec![]).await;
        let hf = fake_hf(GOOD_TOKEN).await;
        e.set_provider_auth("hf", Some(Some(GOOD_TOKEN.into())), Some(hf)).await.unwrap();
        let id = e.save_mcp_server(http_connector()).await.unwrap().config.id;
        give(&e, &format!("mcp_header:{id}:Authorization"), HEADER_VALUE).await;
        give(&e, "setting:search.brave_api_key", SETTING_VALUE).await;
        // The same key also lives in the layer of this device: clearing takes it away from every layer.
        e.set_config("device", "search.brave_api_key", json!(SETTING_VALUE)).unwrap();

        // Through a whole run: it asks the owner first (in every mode), the card has the target and no value.
        let turns = vec![
            ScriptedBackend::tool_turn("c1", "aiw_secret_clear", json!({"target": "provider_token:hf"})),
            ScriptedBackend::text_turn("Tolto."),
        ];
        let (session, run, backend) = assistant_run(&e, turns, "togli il token", OWNER).await;
        let approval = wait_until("a confirmation", || e.approvals().into_iter().next()).await;
        assert_eq!((approval.tool.as_str(), approval.risk.as_str()), ("aiw_secret_clear", "medium"));
        assert_eq!(approval.args, json!({"target": "provider_token:hf"}));
        assert!(e.model_providers().into_iter().find(|p| p.id == "hf").unwrap().auth_configured, "nothing happens before the answer");
        assert!(e.respond_approval(&approval.id, true, false).is_err(), "a channel cannot confirm it");
        e.respond_approval_as(&approval.id, true, false, true).unwrap();
        wait_finished(&e, &run).await;
        assert_eq!(tool_result(&e, &session), json!({"outcome": "cleared", "target": "provider_token:hf"}));
        assert!(!e.model_providers().into_iter().find(|p| p.id == "hf").unwrap().auth_configured);
        assert!(e.0.store.get_doc::<crate::models::ProviderDoc>("model_provider", "hf").unwrap().is_none_or(|d| d.token.is_none()));
        let all = everything(&e, &session, &run, &backend);
        assert!(!all.contains(GOOD_TOKEN), "the value of a secret that is cleared is not in what the run left behind");

        // The tool itself, for the rest of the destinations.
        let tool = e.tool_registry().get("aiw_secret_clear").unwrap();
        assert_eq!((tool.risk(), tool.explicit_grant()), (Risk::Write, true));
        let owner = test_ctx(&e, Some(OWNER));
        for (target, expect) in [
            (format!("mcp_header:{id}:Authorization"), format!("mcp_header:{id}:Authorization")),
            ("setting:search.brave_api_key".into(), "setting:search.brave_api_key".into()),
        ] {
            let out = tool.call(json!({"target": target}), &owner).await.unwrap();
            assert_eq!(serde_json::from_str::<Value>(&out.content).unwrap(), json!({"outcome": "cleared", "target": expect}));
        }
        let stored = e.0.store.get_doc::<McpServerConfig>("mcp", &id).unwrap().unwrap();
        assert_eq!(stored.headers.as_ref().unwrap()["Authorization"], "", "the name stays declared, the value is gone");
        assert!(
            e.0.config.get_str("search.brave_api_key").unwrap_or_default().is_empty(),
            "gone from the global layer and from the device's"
        );
        assert!(e.audit().iter().any(|a| a.action == "secret.clear" && a.detail == "setting:search.brave_api_key"));
        assert!(e.audit().iter().all(|a| !a.detail.contains(SETTING_VALUE) && !a.detail.contains(HEADER_VALUE)));
        // Not for anyone but the owner, not without an origin, and not for what is not a destination.
        let err = tool.call(json!({"target": "setting:search.brave_api_key"}), &test_ctx(&e, Some(PHONE))).await.unwrap_err();
        assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("accesso completo"), "{err}");
        assert!(matches!(tool.call(json!({"target": "setting:search.brave_api_key"}), &test_ctx(&e, None)).await, Err(Error::Denied(_))));
        assert!(matches!(tool.call(json!({"target": "setting:ui.theme"}), &owner).await, Err(Error::Invalid(_))));
        assert!(matches!(tool.call(json!({}), &owner).await, Err(Error::Invalid(_))));
    }

    // ------------------------------------------------------------------ what the tools say about themselves

    #[tokio::test]
    async fn the_three_tools_are_registered_with_the_risk_and_the_grant_the_contract_gives_them() {
        let (e, _d, _w) = engine(vec![]).await;
        for (name, risk) in [("ask_user", Risk::Read), ("aiw_secret_request", Risk::Read), ("aiw_secret_clear", Risk::Write)] {
            let tool = e.tool_registry().get(name).unwrap_or_else(|| panic!("{name} is not registered"));
            assert_eq!((tool.risk(), tool.explicit_grant()), (risk, true), "{name}");
            assert!(tool.available().is_ok());
            assert_eq!(tool.parameters()["type"], "object");
            assert!(tool.description().len() < 800, "{name}: the description is part of every request to the model");
            assert!(crate::management::is_known(name) && crate::management::is_management(name));
        }
        // The questions schema is the one of the plan: kind and message required, the same limits.
        let schema = e.tool_registry().get("ask_user").unwrap().parameters();
        assert_eq!(schema["required"], json!(["kind", "message"]));
        assert_eq!(schema["properties"]["kind"]["enum"], json!(["choice", "confirm", "form"]));
        assert_eq!(
            (schema["properties"]["options"]["minItems"].as_u64(), schema["properties"]["options"]["maxItems"].as_u64()),
            (Some(2), Some(8))
        );
        assert_eq!(schema["properties"]["fields"]["maxItems"], 8);
        assert_eq!(schema["properties"]["message"]["maxLength"], 1000);
        // A secret request has no field that could take a value.
        let schema = e.tool_registry().get("aiw_secret_request").unwrap().parameters();
        let fields: Vec<&String> = schema["properties"].as_object().unwrap().keys().collect();
        assert_eq!(fields, ["label", "reason", "target"]);
        // The policy and the activity log see the destination.
        assert_eq!(
            e.tool_registry().get("aiw_secret_request").unwrap().subject(&json!({"target": "provider_token:hf"})).as_deref(),
            Some("provider_token:hf")
        );
        // The builtin rules ask for the clearing and let the other two through, in every mode.
        let rules = crate::management::builtin_rules();
        assert!(
            rules.iter().any(|r| r.tool == "aiw_secret_clear")
                && !rules.iter().any(|r| r.tool == "ask_user" || r.tool == "aiw_secret_request")
        );
    }

    #[test]
    fn a_value_the_destination_refuses_is_cleaned_out_of_its_error_and_the_status_stays() {
        for (e, status) in [
            (Error::Invalid("rifiutato: SEGRETO-X non va".into()), 400),
            (Error::Unauthorized("SEGRETO-X".into()), 401),
            (Error::Runtime("sì SEGRETO-X no SEGRETO-X".into()), Error::Runtime(String::new()).status()),
            (Error::Upstream("risposta: SEGRETO-X".into()), 502),
            (Error::Io(std::io::Error::other("disco SEGRETO-X")), Error::Other(String::new()).status()),
        ] {
            let clean = without_value(e, "SEGRETO-X");
            assert!(!clean.to_string().contains("SEGRETO-X") && clean.to_string().contains(SECRET_MASK), "{clean}");
            assert_eq!(clean.status(), status, "{clean:?}");
        }
    }
}
