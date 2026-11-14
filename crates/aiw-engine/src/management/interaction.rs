//! The tools that talk to the person: `ask_user` (a question with options, a yes or no, a small
//! form), `aiw_secret_request` (a secret typed into a protected field) and `aiw_secret_clear`.
//!
//! `ask_user` and `aiw_secret_request` stop the run until the person answers, in the same way the
//! questions of an MCP server do (`mcp_host.rs`): the request sits in a map of the engine, the tool
//! waits on a channel, an event (`aiw.question`, `aiw.secret_request`) tells the client, and the
//! answer comes back on a REST endpoint (`POST /api/questions/{id}`, `POST /api/secret-requests/{id}`).
//! The wait ends with the answer, with `approvals.timeout_s` (300 s when it is not set), when the run
//! is cancelled, or at the kill switch, and the tool then writes the closing event itself, so it
//! always lands before the end of the run.
//!
//! **A secret never goes through the model.** The model names only a `target` (the destination);
//! the engine checks it, writes in words where the value will go (`destination`: computed here,
//! never taken from the model) and asks. The value travels in the body of one HTTP request, is
//! written by the existing function of the destination, and is in no tool argument, tool result,
//! event, saved message, audit row, confirmation or error message: the one audit row (`secret.set`)
//! holds the `target` only, and an error of the destination is cleaned of the value before it is
//! returned.

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
            let _ = s.tx.send(SecretOutcome::Cancelled);
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
            Ok(Resolved { target: SecretTarget::ProviderToken { id: info.id.clone() }, canonical: format!("provider_token:{}", info.id), destination })
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
                    "Connettore MCP «{}», variabile {stored}, passata al comando `{}`",
                    shown(&cfg.name),
                    clip(&aiw_tools::mcp::launch::command_line(&safe), 200)
                )
            } else {
                format!(
                    "Connettore MCP «{}», header {stored}, inviato a {}",
                    shown(&cfg.name),
                    cfg.url.as_deref().map(url_safe).unwrap_or_else(|| "(indirizzo non impostato)".into())
                )
            };
            let target = if env {
                SecretTarget::McpEnv { server: cfg.id.clone(), name: stored.clone() }
            } else {
                SecretTarget::McpHeader { server: cfg.id.clone(), name: stored.clone() }
            };
            Ok(Resolved { target, canonical: format!("{kind}:{}:{stored}", cfg.id), destination })
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

    /// Writes the value the person typed to the destination. Used only by [`Engine::respond_secret`].
    async fn store_secret(&self, target: &str, value: &str) -> Result<()> {
        let resolved = resolve_target(self, target)?;
        self.write_secret(&resolved.target, Some(value)).await
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
        self.0.secret_requests.lock().unwrap().insert(request.id.clone(), PendingSecret { request: request.clone(), tx, busy: false });
        let _withdraw = WithdrawSecret(self, request.id.clone());
        self.emit(
            &ctx.run_id,
            AgEvent::Custom { name: "aiw.secret_request".into(), value: serde_json::to_value(&request).unwrap_or(Value::Null) },
        );
        let outcome = tokio::select! {
            biased;
            a = rx => a.unwrap_or(SecretOutcome::Cancelled),
            _ = ctx.cancel.cancelled() => SecretOutcome::Cancelled,
            _ = tokio::time::sleep(wait) => SecretOutcome::Timeout,
        };
        self.0.secret_requests.lock().unwrap().remove(&request.id);
        self.emit(
            &ctx.run_id,
            AgEvent::Custom { name: "aiw.secret_resolved".into(), value: json!({"id": request.id, "outcome": outcome.as_str()}) },
        );
        if outcome != SecretOutcome::Stored {
            // The row of a stored secret (`secret.set`) is written when the value arrives.
            let actor = if outcome == SecretOutcome::Declined { "user" } else { "system" };
            self.audit_interaction(
                &ctx.run_id,
                actor,
                "secret.request",
                Some("aiw_secret_request"),
                Some(outcome.as_str()),
                &request.target,
            );
        }
        match outcome {
            SecretOutcome::Stored => json_output(json!({"outcome": "stored", "target": request.target})),
            other => json_output(json!({"outcome": other.as_str()})),
        }
    }

    /// The requests for a secret that wait for the person (`GET /api/secret-requests`), the oldest first.
    /// They never hold a value.
    pub fn secret_requests(&self) -> Vec<SecretRequest> {
        let mut all: Vec<SecretRequest> = self.0.secret_requests.lock().unwrap().values().map(|p| p.request.clone()).collect();
        all.sort_by(|a, b| a.id.cmp(&b.id));
        all
    }

    /// The person gives the secret (`provide`) or declines (`decline`). `owner` says whether the caller has
    /// full access to this computer; anyone else is refused. A value the destination refuses is an error
    /// without the value in it, and the request stays open for another try; a request closed already is `NotFound`.
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
                    Some(p) => p.tx.send(SecretOutcome::Declined).map_err(|_| not_open("richiesta", id)),
                    None => Err(not_open("richiesta", id)),
                }
            }
            "provide" => {
                let value = clean_value(answer.value)?;
                // The request is claimed (no second value meanwhile) but stays in the list until the value is stored.
                let (target, run_id) = {
                    let mut all = self.0.secret_requests.lock().unwrap();
                    let p = all.get_mut(id).ok_or_else(|| not_open("richiesta", id))?;
                    if p.busy {
                        return Err(Error::Conflict("il valore è in fase di salvataggio".into()));
                    }
                    p.busy = true;
                    (p.request.target.clone(), p.request.run_id.clone())
                };
                let written = self.store_secret(&target, &value).await;
                match written {
                    Err(e) => {
                        if let Some(p) = self.0.secret_requests.lock().unwrap().get_mut(id) {
                            p.busy = false;
                        }
                        Err(without_value(e, &value))
                    }
                    Ok(()) => {
                        self.audit_interaction(&run_id, "user", "secret.set", None, None, &target);
                        let pending = self.0.secret_requests.lock().unwrap().remove(id);
                        // The run may have been cancelled while the value was being written: it is stored all the same.
                        if let Some(p) = pending {
                            let _ = p.tx.send(SecretOutcome::Stored);
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
mod tests {}
