//! Management tools for settings, connectors, agents and memories: what the Assistant uses to
//! change the app from the chat (`aiw_setting_set`, `aiw_setting_set_protected`,
//! `aiw_connector_save`, `aiw_connector_remove`, `aiw_agent_save`, `aiw_agent_delete`,
//! `aiw_memory_review`, `aiw_memory_delete`).
//!
//! All eight change something, so each one starts with [`require_owner`] (which includes the check of
//! the origin of the run) and the built-in rule of `management/mod.rs` asks for a confirmation before
//! it runs. On top of that the tools keep their own limits, which do not depend on the policy:
//!
//! * a setting is changed only if the tool has decided about it ([`classify`]): secrets go through
//!   `aiw_secret_request`, a few machine settings only through the strong tool, the protections of
//!   the app (rules, sandbox, network, TLS, timeouts) never;
//! * a connector never takes a secret value: only the NAMES of its secret variables and headers, whose
//!   values arrive later through the protected field; the command of a `stdio` server is confirmed
//!   (`approve_mcp_server`) only when it is the one the confirmation card showed;
//! * an agent made here stays inside a safe envelope: mode `plan` or `ask`, sandbox `read-only` or
//!   `workspace-write`, no `*`, no management tool, no tool that needs an explicit grant, no
//!   `delegate_tools`; the agents the app ships (the Assistant included) are not touched;
//! * what a result tells the model is short and never a secret: the previous value of a setting is
//!   cut and redacted, a connector is described by its names and status, and the whole answer is
//!   cleaned of the exact values of every secret the app stores.

use super::require_owner;
use crate::{Engine, Inner};
use aiw_core::api::{AgentProfile, McpServerConfig, SECRET_MASK};
use aiw_core::config::{is_management_forbidden_key, is_management_protected_key, management_protected_keys};
use aiw_core::schema::{FieldSchema, SettingsSchema};
use aiw_core::{Error, Result};
use aiw_tools::mcp::launch::{approval_hash, command_line};
use aiw_tools::{Risk, Tool, ToolContext, ToolOutput};
use serde_json::{json, Map, Value};
use std::sync::{Arc, OnceLock, Weak};

/// Longest answer, in characters: the limit the agent loop puts on a tool result.
const MAX_CHARS: usize = 24_000;
/// A stored value shorter than this is not hunted for in the answer: it would also match ordinary words.
const MIN_SECRET_LEN: usize = 6;
/// Longest value of a variable or header the model may write (a longer one is not a setting).
const MAX_VALUE_CHARS: usize = 2_000;
/// Longest name, description and system prompt of an agent made from the chat.
const MAX_NAME_CHARS: usize = 80;
const MAX_DESCRIPTION_CHARS: usize = 500;
const MAX_PROMPT_CHARS: usize = 12_000;

pub(crate) fn tools(e: &Engine) -> Vec<Arc<dyn Tool>> {
    KINDS.iter().map(|kind| Arc::new(Admin { kind: *kind, engine: Arc::downgrade(&e.0) }) as Arc<dyn Tool>).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    SettingSet,
    SettingSetProtected,
    ConnectorSave,
    ConnectorRemove,
    AgentSave,
    AgentDelete,
    MemoryReview,
    MemoryDelete,
}

const KINDS: [Kind; 8] = [
    Kind::SettingSet,
    Kind::SettingSetProtected,
    Kind::ConnectorSave,
    Kind::ConnectorRemove,
    Kind::AgentSave,
    Kind::AgentDelete,
    Kind::MemoryReview,
    Kind::MemoryDelete,
];

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::SettingSet => "aiw_setting_set",
            Kind::SettingSetProtected => "aiw_setting_set_protected",
            Kind::ConnectorSave => "aiw_connector_save",
            Kind::ConnectorRemove => "aiw_connector_remove",
            Kind::AgentSave => "aiw_agent_save",
            Kind::AgentDelete => "aiw_agent_delete",
            Kind::MemoryReview => "aiw_memory_review",
            Kind::MemoryDelete => "aiw_memory_delete",
        }
    }

    /// `Write` for what changes the configuration, `Exec` for what deletes, runs commands or touches
    /// the machine (the lists of `management/mod.rs` say the same; a test compares them).
    fn risk(self) -> Risk {
        match self {
            Kind::SettingSet | Kind::AgentSave | Kind::MemoryReview => Risk::Write,
            _ => Risk::Exec,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Kind::SettingSet => "Cambia un'impostazione ordinaria di AI Workspace (per esempio tema, lingua, ricerca web, assistente proattivo, ricordi). Chiede sempre la conferma dell'utente e risponde con il valore di prima, così puoi rimetterlo (`value: null` torna al predefinito). Non cambia i segreti (per quelli usa aiw_secret_request), le impostazioni di macchina (aiw_setting_set_protected) né le protezioni dell'app (regole, sandbox, rete, TLS, tempi di conferma): quelle si cambiano dalla vista Sviluppatore. Guarda prima chiavi e valori con aiw_inspect (topic settings).",
            Kind::SettingSetProtected => "Cambia una delle sole impostazioni di macchina ammesse: workspace.root (cartella di lavoro), models.dir (cartella dei modelli), server.lan (accesso dalla rete locale), update.channel, update.auto_check, update.auto_download. È un'azione delicata: chiede sempre la conferma dell'utente. Risponde con il valore di prima. Tutte le altre impostazioni hanno altri strumenti o non si cambiano da qui.",
            Kind::ConnectorSave => "Aggiunge o modifica un connettore (server MCP) che dà all'assistente nuovi strumenti. Con `stdio` si avvia un comando su questo computer: la conferma mostra la riga di comando e, se l'utente la accetta, quel comando viene anche autorizzato. Con `http` ci si collega a un indirizzo. In `env` e `headers` vanno solo valori che non sono segreti. Token, chiavi e password si dichiarano solo per nome in `secret_env` e `secret_headers`: il valore lo inserisce poi l'utente con aiw_secret_request, mai in chat. Per modificarne uno passa `id` (da aiw_inspect, topic connectors) e ripeti `command` e `args`; i segreti già salvati restano.",
            Kind::ConnectorRemove => "Rimuove un connettore (server MCP) con le sue variabili, intestazioni e accessi salvati. Azione delicata: chiede sempre la conferma dell'utente e non si annulla. Trova gli `id` con aiw_inspect (topic connectors).",
            Kind::AgentSave => "Crea o modifica un assistente dell'utente, con dei limiti: `mode` solo plan o ask, `sandbox` solo read-only o workspace-write, `tools` elencati uno per uno (niente `*`, niente strumenti aiw_*, niente strumenti a concessione esplicita). Non si possono creare né modificare l'Assistente e gli altri assistenti integrati: per cambiarne uno se ne crea una copia con un altro nome. Per modificare un assistente creato dall'utente passa `id` (da aiw_inspect, topic agents). Chiede sempre la conferma dell'utente.",
            Kind::AgentDelete => "Elimina un assistente creato dall'utente (non quelli integrati). Azione delicata: chiede sempre la conferma dell'utente e non si annulla. Trova gli `id` con aiw_inspect (topic agents).",
            Kind::MemoryReview => "Decide su un ricordo: `restore` riporta in uso un ricordo dimenticato, `approve` conferma una proposta, `reject` scarta una proposta (la elimina per sempre: era solo una proposta). Chiede sempre la conferma dell'utente. Trova gli `id` con aiw_inspect (topic memory).",
            Kind::MemoryDelete => "Cancella per sempre un ricordo (`id`) oppure tutti i ricordi (`all: true`). Azione delicata e irreversibile: chiede sempre la conferma dell'utente. Per solo dimenticare un ricordo senza perderlo usa memory_forget.",
        }
    }

    fn parameters(self) -> Value {
        match self {
            Kind::SettingSet => json!({
                "type": "object",
                "required": ["key", "value"],
                "properties": {
                    "key": {"type": "string", "description": "Chiave dell'impostazione, per esempio ui.theme (le chiavi si vedono con aiw_inspect, topic settings)."},
                    "value": {"description": "Nuovo valore, del tipo dell'impostazione (testo, numero, true/false o elenco di testi). null rimette il valore predefinito."}
                }
            }),
            Kind::SettingSetProtected => json!({
                "type": "object",
                "required": ["key", "value"],
                "properties": {
                    "key": {"type": "string", "enum": management_protected_keys(), "description": "Una delle impostazioni di macchina ammesse."},
                    "value": {"description": "Nuovo valore: un percorso per workspace.root e models.dir, true/false per server.lan, update.auto_check e update.auto_download, auto|stable|beta|alpha per update.channel. null rimette il predefinito."}
                }
            }),
            Kind::ConnectorSave => json!({
                "type": "object",
                "required": ["name", "transport"],
                "properties": {
                    "id": {"type": "string", "description": "Solo per modificare un connettore esistente; ometterlo per crearne uno nuovo."},
                    "name": {"type": "string", "description": "Nome del connettore (per esempio github)."},
                    "transport": {"type": "string", "enum": ["stdio", "http"], "description": "stdio avvia un comando su questo computer; http si collega a un indirizzo."},
                    "command": {"type": "string", "description": "Solo stdio: il programma da avviare (per esempio npx)."},
                    "args": {"type": "array", "items": {"type": "string"}, "description": "Solo stdio: gli argomenti del programma, uno per voce. Niente segreti."},
                    "url": {"type": "string", "description": "Solo http: l'indirizzo del server (http o https), senza utente, password o token."},
                    "env": {"type": "object", "description": "Solo stdio: variabili d'ambiente NON segrete (nome: valore)."},
                    "secret_env": {"type": "array", "items": {"type": "string"}, "description": "Solo stdio: SOLO i nomi delle variabili segrete (per esempio GITHUB_TOKEN). Il valore non va scritto qui."},
                    "headers": {"type": "object", "description": "Solo http: intestazioni NON segrete (nome: valore)."},
                    "secret_headers": {"type": "array", "items": {"type": "string"}, "description": "Solo http: SOLO i nomi delle intestazioni segrete (per esempio Authorization). Il valore non va scritto qui."},
                    "enabled": {"type": "boolean", "description": "Se collegarlo subito (predefinito sì)."}
                }
            }),
            Kind::ConnectorRemove => json!({
                "type": "object",
                "required": ["id"],
                "properties": {"id": {"type": "string", "description": "Identificativo (o nome) del connettore."}}
            }),
            Kind::AgentSave => json!({
                "type": "object",
                "required": ["name", "system_prompt", "tools"],
                "properties": {
                    "id": {"type": "string", "description": "Solo per modificare un assistente creato dall'utente; ometterlo per crearne uno nuovo."},
                    "name": {"type": "string", "description": "Nome dell'assistente."},
                    "description": {"type": "string", "description": "A che cosa serve, in una frase."},
                    "system_prompt": {"type": "string", "description": "Le istruzioni dell'assistente."},
                    "tools": {"type": "array", "items": {"type": "string"}, "description": "Gli strumenti che può usare, uno per voce. Niente \"*\", niente aiw_*, niente ask_user."},
                    "mode": {"type": "string", "enum": ["plan", "ask"], "description": "plan: solo analisi; ask: chiede conferma per ogni modifica (predefinito)."},
                    "sandbox": {"type": "string", "enum": ["read-only", "workspace-write"], "description": "read-only: non scrive; workspace-write: scrive solo nella cartella di lavoro (predefinito)."},
                    "model": {"type": "string", "description": "Profilo di inferenza: l'id di un profilo esistente oppure auto:agent, auto:chat, auto:reasoning, auto:vision. Predefinito auto:agent. Per fissare un modello preciso usa aiw_model_use."}
                }
            }),
            Kind::AgentDelete => json!({
                "type": "object",
                "required": ["id"],
                "properties": {"id": {"type": "string", "description": "Identificativo dell'assistente creato dall'utente."}}
            }),
            Kind::MemoryReview => json!({
                "type": "object",
                "required": ["id", "action"],
                "properties": {
                    "id": {"type": "string", "description": "Identificativo del ricordo."},
                    "action": {"type": "string", "enum": ["restore", "approve", "reject"], "description": "restore: riporta in uso un ricordo dimenticato; approve: conferma una proposta; reject: scarta una proposta."}
                }
            }),
            Kind::MemoryDelete => json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Identificativo del ricordo da cancellare per sempre."},
                    "all": {"type": "boolean", "description": "true per cancellare per sempre TUTTI i ricordi (al posto di `id`)."}
                }
            }),
        }
    }
}

struct Admin {
    kind: Kind,
    engine: Weak<Inner>,
}

#[async_trait::async_trait]
impl Tool for Admin {
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn description(&self) -> &str {
        self.kind.description()
    }

    fn parameters(&self) -> Value {
        self.kind.parameters()
    }

    fn risk(&self) -> Risk {
        self.kind.risk()
    }

    fn explicit_grant(&self) -> bool {
        true
    }

    /// What the confirmation card shows besides the arguments, and what a rule such as
    /// `aiw_setting_set(ui.*)` is matched against: the key, the command line or address of the
    /// connector, the agent or memory the action is about. Never a secret value.
    fn subject(&self, args: &Value) -> Option<String> {
        let e = Engine(self.engine.upgrade()?);
        match self.kind {
            Kind::SettingSet | Kind::SettingSetProtected => opt_str(args, "key").ok().flatten().map(|k| clip(&k, 120)),
            Kind::ConnectorSave => {
                let cfg = connector_config(&e, &parse_connector(args).ok()?, true).ok()?;
                Some(connector_target(&cfg, false))
            }
            Kind::ConnectorRemove => {
                let id = opt_str(args, "id").ok().flatten()?;
                let cfg = find_connector(&e, &id)?;
                Some(format!("«{}»: {}", text(&cfg.name, 60), connector_target(&cfg, false)))
            }
            Kind::AgentSave => opt_str(args, "id").ok().flatten().or_else(|| opt_str(args, "name").ok().flatten()).map(|s| text(&s, 80)),
            Kind::AgentDelete => {
                let id = opt_str(args, "id").ok().flatten()?;
                match e.agent(&id) {
                    Ok(a) => Some(format!("«{}» ({id})", text(&a.name, 60))),
                    Err(_) => Some(text(&id, 80)),
                }
            }
            Kind::MemoryReview => {
                let (id, action) = (opt_str(args, "id").ok().flatten()?, opt_str(args, "action").ok().flatten().unwrap_or_default());
                Some(format!("{action} {id}"))
            }
            Kind::MemoryDelete => {
                if args.get("all").and_then(Value::as_bool) == Some(true) {
                    return Some("tutti i ricordi".into());
                }
                let id = opt_str(args, "id").ok().flatten()?;
                match e.memory(&id) {
                    Ok(m) => Some(format!("{id}: {}", text(&m.text, 120))),
                    Err(_) => Some(id),
                }
            }
        }
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let e = Engine(self.engine.upgrade().ok_or_else(|| Error::Conflict("motore in arresto".into()))?);
        // The tool works only in a chat run the server started for the owner, whatever the policy said.
        require_owner(&e, ctx)?;
        let value = match self.kind {
            Kind::SettingSet => setting_change(&e, &args, false)?,
            Kind::SettingSetProtected => setting_change(&e, &args, true)?,
            Kind::ConnectorSave => connector_save(&e, &args).await?,
            Kind::ConnectorRemove => connector_remove(&e, &args).await?,
            Kind::AgentSave => agent_save(&e, &args)?,
            Kind::AgentDelete => agent_delete(&e, &args)?,
            Kind::MemoryReview => memory_review(&e, &args)?,
            Kind::MemoryDelete => memory_delete(&e, &args)?,
        };
        Ok(ToolOutput::text(finish(&e, value)))
    }
}

// ------------------------------------------------------------------ the answer

/// Serialises the answer after removing the exact value of every secret the app stores, and keeps it
/// within the size the agent loop accepts.
fn finish(e: &Engine, mut value: Value) -> String {
    let mut secrets: Vec<String> = e.known_secrets().into_iter().filter(|s| s.chars().count() >= MIN_SECRET_LEN).collect();
    // The longest first, so that a value that contains another is replaced whole.
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    secrets.dedup();
    scrub(&mut value, &secrets);
    let text = value.to_string();
    if text.chars().count() <= MAX_CHARS {
        return text;
    }
    let cut: String = text.chars().take(MAX_CHARS - 80).collect();
    format!("{cut}… [risposta troncata]")
}

fn scrub(v: &mut Value, secrets: &[String]) {
    match v {
        Value::String(s) => {
            for secret in secrets {
                if s.contains(secret.as_str()) {
                    *s = s.replace(secret.as_str(), SECRET_MASK);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| scrub(x, secrets)),
        Value::Object(m) => m.values_mut().for_each(|x| scrub(x, secrets)),
        _ => {}
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

/// Free text from elsewhere (a memory, an error, an argument): `token=…`, `password: …`, `Bearer …` and
/// credentials in URLs are hidden by the redactor of the diagnostics.
fn redact(s: &str) -> String {
    static PATTERNS: OnceLock<Vec<(regex::Regex, &'static str)>> = OnceLock::new();
    crate::diagnostics::scrub_line(PATTERNS.get_or_init(crate::diagnostics::line_patterns), s)
}

fn text(s: &str, max: usize) -> String {
    redact(&clip(s, max))
}

/// Does this text carry something the redactor would hide (a credential in a URL, `token=…`, `Bearer …`)?
fn looks_secret(s: &str) -> bool {
    redact(s) != s
}

/// An address without what could be a secret: no user or password, no query, no fragment, and a path
/// segment that looks like a token (long) cut.
fn url_safe(raw: &str) -> String {
    let Ok(mut u) = reqwest::Url::parse(raw.trim()) else { return "(indirizzo non leggibile)".into() };
    let _ = u.set_username("");
    let _ = u.set_password(None);
    u.set_query(None);
    u.set_fragment(None);
    let path: Vec<String> =
        u.path().split('/').map(|s| if s.chars().count() > 20 { "omesso".to_string() } else { s.to_string() }).collect();
    u.set_path(&path.join("/"));
    u.to_string()
}

/// A text value of a setting as the model reads it back: an address without credentials, query or long
/// path segments (a private calendar link is a secret in its path), any other text redacted and cut.
fn safe_text(s: &str) -> String {
    let lower = s.trim().to_ascii_lowercase();
    if ["http://", "https://", "webcal://"].iter().any(|p| lower.starts_with(p)) {
        url_safe(s)
    } else {
        text(s, 200)
    }
}

/// A value of a setting for the answer: text redacted and cut, a short list of texts the same, anything
/// with a structure only as a count.
fn shown(v: Option<&Value>) -> Value {
    const MAX_ITEMS: usize = 20;
    match v {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(s)) => Value::String(safe_text(s)),
        Some(Value::Array(a)) if a.iter().all(Value::is_string) => {
            Value::Array(a.iter().take(MAX_ITEMS).map(|x| Value::String(safe_text(x.as_str().unwrap_or_default()))).collect())
        }
        Some(Value::Array(a)) => json!({"entries": a.len()}),
        Some(Value::Object(m)) => json!({"entries": m.len()}),
        Some(other) => other.clone(),
    }
}

// ------------------------------------------------------------------ arguments

fn opt_str(args: &Value, key: &str) -> Result<Option<String>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim().to_string()).filter(|s| !s.is_empty())),
        Some(_) => Err(Error::Invalid(format!("`{key}` deve essere un testo"))),
    }
}

fn req_str(args: &Value, key: &str) -> Result<String> {
    opt_str(args, key)?.ok_or_else(|| Error::Invalid(format!("manca `{key}`")))
}

fn opt_bool(args: &Value, key: &str) -> Result<Option<bool>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(Value::String(s)) if s.trim().eq_ignore_ascii_case("true") => Ok(Some(true)),
        Some(Value::String(s)) if s.trim().eq_ignore_ascii_case("false") => Ok(Some(false)),
        Some(_) => Err(Error::Invalid(format!("`{key}` deve essere true o false"))),
    }
}

/// A list of texts, kept as the model wrote them (the spaces of an argument count).
fn opt_list(args: &Value, key: &str) -> Result<Option<Vec<String>>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| v.as_str().map(String::from).ok_or_else(|| Error::Invalid(format!("`{key}` deve essere un elenco di testi"))))
            .collect::<Result<Vec<_>>>()
            .map(Some),
        Some(_) => Err(Error::Invalid(format!("`{key}` deve essere un elenco di testi"))),
    }
}

/// An object of names and values (numbers and true/false are written as text, as the environment and the
/// headers are).
fn opt_pairs(args: &Value, key: &str) -> Result<Option<Vec<(String, String)>>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(m)) => m
            .iter()
            .map(|(k, v)| match v {
                Value::String(s) => Ok((k.clone(), s.clone())),
                Value::Number(n) => Ok((k.clone(), n.to_string())),
                Value::Bool(b) => Ok((k.clone(), b.to_string())),
                _ => Err(Error::Invalid(format!("`{key}`: il valore di `{k}` deve essere un testo"))),
            })
            .collect::<Result<Vec<_>>>()
            .map(Some),
        Some(_) => Err(Error::Invalid(format!("`{key}` deve essere un oggetto nome: valore"))),
    }
}

// ------------------------------------------------------------------ settings

/// The settings of the app that an ordinary change may touch: every key of the schema that is not a
/// secret, not protected, not forbidden and not the list of mail accounts is listed here BY NAME. The
/// list is on purpose a decision, not a default: a key added to the schema is refused by
/// `aiw_setting_set` until somebody puts it here, and a test fails to say so.
const ORDINARY: &[&str] = &[
    "ui.mode",
    "ui.theme",
    "ui.language",
    "app.close_to_tray",
    "app.start_at_login",
    "app.notifications",
    "agent.default",
    "context.compact_at",
    "agents.max_parallel_subagents",
    "agents.budget.max_tokens",
    "agents.budget.max_turns",
    "agents.budget.max_seconds",
    "mcp.sampling.max_calls",
    "mcp.sampling.max_tokens",
    "routing.bandit",
    "learning.reflect",
    "learning.profile",
    "memory.enabled",
    "proactive.level",
    "proactive.interval_minutes",
    "proactive.briefing_time",
    "proactive.quiet_hours",
    "proactive.daily_budget_minutes",
    "proactive.only_on_power",
    "proactive.agent",
    "proactive.watch_folders",
    "connectors.calendar.enabled",
    "connectors.calendar.sources",
    "connectors.mail.enabled",
    "connectors.mail.sources",
    "connectors.mail.imap_days",
    "connectors.mail.imap_max_messages",
    "search.provider",
    "search.searxng_url",
    "voice.language",
    "voice.stt_runtime",
    "voice.stt_model",
    "voice.tts_voice",
    "voice.tts.engine",
    "voice.tts.voice",
    "voice.tts.speed",
    "voice.tts.server_url",
    "voice.tts.server_model",
];

/// The list of mail accounts: its password lives in the secret store and the engine itself refuses to
/// change it through the settings.
const MAIL_ACCOUNTS: &str = "connectors.mail.imap";

/// What the management tools may do with a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    /// `aiw_setting_set`.
    Ordinary,
    /// A value that is a secret (the schema says so): `aiw_secret_request`.
    Secret,
    /// A machine setting: only `aiw_setting_set_protected`.
    Protected,
    /// A protection of the app: never.
    Forbidden,
    /// The mail accounts: never from here.
    Accounts,
    /// Not a key of the schema.
    Unknown,
    /// A key of the schema that nobody classified yet: refused until somebody decides.
    Unclassified,
}

fn classify(schema: &SettingsSchema, key: &str) -> Access {
    let Some(field) = schema.properties.get(key) else { return Access::Unknown };
    if field.secret {
        Access::Secret
    } else if key == MAIL_ACCOUNTS {
        Access::Accounts
    } else if is_management_forbidden_key(key) {
        Access::Forbidden
    } else if is_management_protected_key(key) {
        Access::Protected
    } else if ORDINARY.contains(&key) {
        Access::Ordinary
    } else {
        Access::Unclassified
    }
}

/// Why the tool (`protected`: the strong one) does not change this key, with what to use instead.
fn refusal(access: Access, key: &str, protected: bool) -> Option<Error> {
    let message = match access {
        Access::Ordinary if protected => format!(
            "`{key}` è un'impostazione ordinaria: usa aiw_setting_set. aiw_setting_set_protected accetta solo {}.",
            management_protected_keys().join(", ")
        ),
        Access::Protected if !protected => format!("`{key}` è un'impostazione di macchina: usa aiw_setting_set_protected."),
        Access::Ordinary | Access::Protected => return None,
        Access::Secret => format!(
            "`{key}` è un segreto: non si imposta con questo strumento e il valore non va scritto in chat. Usa aiw_secret_request con target `setting:{key}`."
        ),
        Access::Forbidden => format!(
            "`{key}` fa parte delle protezioni dell'app (regole, sandbox, rete, TLS, tempi di conferma) e l'Assistente non le cambia: si cambia dalla vista Sviluppatore, in Impostazioni."
        ),
        Access::Accounts => "Gli account di posta (con le password) non si cambiano da qui: si gestiscono dall'editor «Account di posta (IMAP)» nella vista Sviluppatore.".to_string(),
        Access::Unknown => format!("Impostazione sconosciuta `{key}`: guarda l'elenco con aiw_inspect (topic settings)."),
        Access::Unclassified => format!("`{key}` non è tra le impostazioni che l'Assistente può cambiare: si cambia dalla vista Sviluppatore, in Impostazioni."),
    };
    Some(if access == Access::Unknown { Error::NotFound(message) } else { Error::Denied(message) })
}

/// A value written as text where the schema wants a number or a true/false (models do it often).
fn coerce(field: &FieldSchema, value: Value) -> Value {
    let Value::String(s) = &value else { return value };
    let s = s.trim();
    match field.kind.as_str() {
        "boolean" if s.eq_ignore_ascii_case("true") => Value::Bool(true),
        "boolean" if s.eq_ignore_ascii_case("false") => Value::Bool(false),
        "integer" => s.parse::<i64>().map(Value::from).unwrap_or(value),
        "number" => s.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or(value),
        _ => value,
    }
}

/// `agent.default` and `proactive.agent` must name an agent that exists: a typo would leave the app
/// without one.
fn check_agent_reference(e: &Engine, key: &str, value: &Value) -> Result<()> {
    if !["agent.default", "proactive.agent"].contains(&key) {
        return Ok(());
    }
    let Some(id) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) else { return Ok(()) };
    e.agent(id).map(|_| ()).map_err(|_| {
        Error::Invalid(format!("Non esiste un assistente `{}`: guarda l'elenco con aiw_inspect (topic agents).", text(id, 60)))
    })
}

/// `aiw_setting_set` and `aiw_setting_set_protected`: changes a key of the `global` layer.
fn setting_change(e: &Engine, args: &Value, protected: bool) -> Result<Value> {
    let key = req_str(args, "key")?;
    let schema = e.0.config.schema();
    let access = classify(schema, &key);
    if let Some(err) = refusal(access, &key, protected) {
        return Err(err);
    }
    let field = schema.properties.get(&key).ok_or_else(|| Error::NotFound(format!("Impostazione sconosciuta `{key}`")))?;
    let Some(value) = args.get("value").cloned() else {
        return Err(Error::Invalid("manca `value`: il nuovo valore (null per rimettere il predefinito)".into()));
    };
    let value = coerce(field, value);
    check_agent_reference(e, &key, &value)?;

    // What the key was before: the value in use, and whether the layer we write had a value of its own.
    let before_view = e.config();
    let before = before_view.effective.get(&key).cloned().or_else(|| field.default.clone());
    let had_value = before_view.layers.iter().any(|l| l.name == "global" && l.values.contains_key(&key));

    let after_view = e.set_config("global", &key, value.clone())?;
    let now = after_view.effective.get(&key).cloned().or_else(|| field.default.clone());

    let mut notes: Vec<String> = Vec::new();
    // A device or workspace value wins over the global one: the change was saved but is not the one in use.
    if !value.is_null() && now.as_ref() != Some(&value) {
        notes.push(format!(
            "Il valore è stato salvato, ma quello in uso resta {} perché è fissato nello strato «{}».",
            shown(now.as_ref()),
            e.0.config.source_of(&key)
        ));
    }
    notes.push(if had_value {
        "Per rimettere com'era, imposta di nuovo il valore di prima.".to_string()
    } else {
        "Prima non c'era un valore salvato (valeva quello predefinito): per tornare indietro imposta `value: null`.".to_string()
    });
    Ok(json!({
        "key": key,
        "previous": shown(before.as_ref()),
        "previous_saved": had_value,
        "now": shown(now.as_ref()),
        "layer": "global",
        "note": notes.join(" "),
    }))
}

// ------------------------------------------------------------------ connectors

/// What the model said about a connector. Every field is optional here: an edit repeats only what
/// changes, a new connector is checked for what it needs by [`connector_config`].
struct ConnectorArgs {
    id: Option<String>,
    name: Option<String>,
    transport: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
    url: Option<String>,
    env: Option<Vec<(String, String)>>,
    secret_env: Option<Vec<String>>,
    headers: Option<Vec<(String, String)>>,
    secret_headers: Option<Vec<String>>,
    enabled: Option<bool>,
}

fn parse_connector(args: &Value) -> Result<ConnectorArgs> {
    Ok(ConnectorArgs {
        id: opt_str(args, "id")?,
        name: opt_str(args, "name")?,
        transport: opt_str(args, "transport")?,
        command: opt_str(args, "command")?,
        args: opt_list(args, "args")?,
        url: opt_str(args, "url")?,
        env: opt_pairs(args, "env")?,
        secret_env: opt_list(args, "secret_env")?,
        headers: opt_pairs(args, "headers")?,
        secret_headers: opt_list(args, "secret_headers")?,
        enabled: opt_bool(args, "enabled")?,
    })
}

fn stored_connectors(e: &Engine) -> Vec<McpServerConfig> {
    e.0.store.list_docs::<McpServerConfig>("mcp").unwrap_or_default()
}

/// A connector by id or by name, as the store has it (secret values included: never show them).
fn find_connector(e: &Engine, id: &str) -> Option<McpServerConfig> {
    stored_connectors(e).into_iter().find(|c| c.id == id || c.name == id)
}

/// The name of a connector as the engine stores it.
fn normalized_name(raw: &str) -> String {
    raw.trim().replace(' ', "_")
}

/// What the confirmation card shows of a connector: the command line of a `stdio` server (what will run)
/// or the address of an `http` one (`safe`: an address without credentials, query and long segments, for
/// the answer; the card shows it as it was written, redacted).
fn connector_target(cfg: &McpServerConfig, safe: bool) -> String {
    if cfg.transport == "stdio" {
        text(&command_line(cfg), 400)
    } else {
        let url = cfg.url.as_deref().unwrap_or_default();
        if safe {
            url_safe(url)
        } else {
            text(url, 400)
        }
    }
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+.^_`|~-".contains(c))
}

/// The entries of `env` or `headers` of the call (`plain`, with their values) and the names of the secret
/// ones (`secret`), checked: no name that looks like a secret among the plain ones, only names that look
/// like one among the secret ones, no value that carries a credential, no duplicates.
fn check_entries(
    plain: &[(String, String)],
    secret: &[String],
    (plain_field, secret_field): (&str, &str),
    valid: fn(&str) -> bool,
) -> Result<()> {
    let mut seen: Vec<String> = Vec::new();
    let mut note = |name: &str| -> Result<()> {
        let lower = name.to_ascii_lowercase();
        if seen.contains(&lower) {
            return Err(Error::Invalid(format!("`{}` compare più volte in `{plain_field}` e `{secret_field}`", clip(name, 60))));
        }
        seen.push(lower);
        Ok(())
    };
    for (name, value) in plain {
        if !valid(name) {
            return Err(Error::Invalid(format!("`{}` non è un nome valido in `{plain_field}`", clip(name, 60))));
        }
        note(name)?;
        if crate::secrets::is_secret_name(name) {
            return Err(Error::Invalid(format!(
                "`{name}` sembra un segreto: non scriverne il valore qui. Dichiaralo solo per nome in `{secret_field}` e fai inserire il valore all'utente con aiw_secret_request."
            )));
        }
        if value.chars().count() > MAX_VALUE_CHARS || value.contains('\0') {
            return Err(Error::Invalid(format!("il valore di `{name}` non è valido (troppo lungo o con caratteri non ammessi)")));
        }
        if looks_secret(value) {
            return Err(Error::Invalid(format!(
                "Il valore di `{name}` contiene delle credenziali: non scriverle qui. Dichiara un nome da segreto (per esempio `{}_TOKEN`) in `{secret_field}` e fai inserire il valore all'utente con aiw_secret_request.",
                name.to_ascii_uppercase().replace('-', "_")
            )));
        }
    }
    for name in secret {
        if !valid(name) {
            return Err(Error::Invalid(format!("`{}` non è un nome valido in `{secret_field}`", clip(name, 60))));
        }
        note(name)?;
        if !crate::secrets::is_secret_name(name) {
            return Err(Error::Invalid(format!(
                "`{name}` non ha un nome da segreto (deve contenere per esempio token, key, secret, password o auth): se non è un segreto mettilo in `{plain_field}` con il suo valore."
            )));
        }
    }
    Ok(())
}

/// The configuration a call to `aiw_connector_save` stands for. `inherit`: what the call does not repeat
/// (command, arguments, variable names) is taken from the connector it edits; without it only what the
/// call itself says counts, which is what the confirmation card showed.
fn connector_config(e: &Engine, a: &ConnectorArgs, inherit: bool) -> Result<McpServerConfig> {
    let others = stored_connectors(e);
    let stored = match &a.id {
        Some(id) => Some(others.iter().find(|c| &c.id == id || &c.name == id).cloned().ok_or_else(|| {
            Error::NotFound(format!(
                "Il connettore `{}` non esiste: per aggiungerne uno nuovo ometti `id`, per modificarne uno guarda aiw_inspect (topic connectors).",
                clip(id, 60)
            ))
        })?),
        None => None,
    };
    let name = normalized_name(
        a.name
            .as_deref()
            .or(stored.as_ref().map(|s| s.name.as_str()))
            .ok_or_else(|| Error::Invalid("manca `name`: il nome del connettore".into()))?,
    );
    if name.is_empty() || name.chars().count() > 60 {
        return Err(Error::Invalid("`name` deve essere un nome di al massimo 60 caratteri".into()));
    }
    let stored_id = stored.as_ref().map(|s| s.id.as_str());
    if let Some(other) = others.iter().find(|c| c.name == name && Some(c.id.as_str()) != stored_id) {
        return Err(Error::Conflict(format!(
            "Esiste già un connettore chiamato «{name}» (id `{}`): per modificarlo passa `id`, altrimenti scegli un altro nome.",
            other.id
        )));
    }
    let transport = a
        .transport
        .clone()
        .or_else(|| stored.as_ref().map(|s| s.transport.clone()))
        .ok_or_else(|| Error::Invalid("manca `transport`: stdio oppure http".into()))?;
    // What an edit inherits is what the connector already had and only if it keeps its kind.
    let same_kind = stored.as_ref().filter(|s| s.transport == transport);
    let mut cfg = McpServerConfig {
        id: stored.as_ref().map(|s| s.id.clone()).unwrap_or_default(),
        name,
        transport: transport.clone(),
        command: None,
        args: None,
        url: None,
        env: None,
        headers: None,
        enabled: a.enabled.or(stored.as_ref().map(|s| s.enabled)).unwrap_or(true),
        sampling: stored.as_ref().map(|s| s.sampling).unwrap_or(false),
        oauth_client_id: stored.as_ref().and_then(|s| s.oauth_client_id.clone()),
        sandbox: "off".into(),
        sandbox_network: stored.as_ref().map(|s| s.sandbox_network).unwrap_or(true),
    };
    match transport.as_str() {
        "stdio" => {
            if a.url.is_some() || a.headers.is_some() || a.secret_headers.is_some() {
                return Err(Error::Invalid(
                    "`url`, `headers` e `secret_headers` valgono solo per http: un connettore stdio avvia un comando".into(),
                ));
            }
            let command = a
                .command
                .clone()
                .or_else(|| same_kind.filter(|_| inherit).and_then(|s| s.command.clone()))
                .ok_or_else(|| Error::Invalid("manca `command`: il programma da avviare".into()))?;
            if looks_secret(&command) {
                return Err(Error::Invalid("Il comando contiene una credenziale: non scriverla qui.".into()));
            }
            if let Some(args) = &a.args {
                if let Some(i) = args.iter().position(|x| looks_secret(x) || x.contains('\0')) {
                    return Err(Error::Invalid(format!(
                        "L'argomento n. {} della riga di comando contiene una credenziale (token, chiave o password): non scriverla qui. Se il programma la legge da una variabile d'ambiente, dichiara il nome in `secret_env` e fai inserire il valore all'utente con aiw_secret_request.",
                        i + 1
                    )));
                }
            }
            // `--api-key abc`: the value of a flag that names a secret is an argument of its own.
            if let Some(args) = &a.args {
                if let Some(i) = (0..args.len()).find(|&i| {
                    args[i].starts_with('-')
                        && !args[i].contains('=')
                        && crate::secrets::is_secret_name(&args[i])
                        && args.get(i + 1).is_some_and(|next| !next.starts_with('-'))
                }) {
                    return Err(Error::Invalid(format!(
                        "L'argomento n. {} (`{}`) è il nome di un segreto seguito da un valore: non scrivere il valore qui. Se il programma legge il segreto da una variabile d'ambiente, dichiara il nome in `secret_env` e fai inserire il valore all'utente con aiw_secret_request; se il valore non è un segreto, scrivi flag e valore insieme con =.",
                        i + 1,
                        clip(&args[i], 60)
                    )));
                }
            }
            cfg.command = Some(command);
            cfg.args = a.args.clone().or_else(|| same_kind.filter(|_| inherit).and_then(|s| s.args.clone())).filter(|v| !v.is_empty());
            check_entries(
                a.env.as_deref().unwrap_or_default(),
                a.secret_env.as_deref().unwrap_or_default(),
                ("env", "secret_env"),
                valid_env_name,
            )?;
            cfg.env = entries(
                a.env.as_deref(),
                a.secret_env.as_deref(),
                same_kind.filter(|_| inherit).and_then(|s| s.env.as_ref()),
                same_kind.and_then(|s| s.env.as_ref()),
            );
            // A new server is isolated like the interface proposes (when this system can); an edit keeps its isolation.
            cfg.sandbox = match same_kind {
                Some(s) => s.sandbox.clone(),
                None if aiw_tools::sandbox::can_wrap_process() => "workspace-write".into(),
                None => "off".into(),
            };
        }
        "http" => {
            if a.command.is_some() || a.args.is_some() || a.env.is_some() || a.secret_env.is_some() {
                return Err(Error::Invalid(
                    "`command`, `args`, `env` e `secret_env` valgono solo per stdio: un connettore http si collega a un indirizzo".into(),
                ));
            }
            if let Some(url) = &a.url {
                check_url(url)?;
            }
            cfg.url = Some(
                a.url
                    .clone()
                    .or_else(|| same_kind.and_then(|s| s.url.clone()))
                    .ok_or_else(|| Error::Invalid("manca `url`: l'indirizzo del server".into()))?,
            );
            check_entries(
                a.headers.as_deref().unwrap_or_default(),
                a.secret_headers.as_deref().unwrap_or_default(),
                ("headers", "secret_headers"),
                valid_header_name,
            )?;
            cfg.headers = entries(
                a.headers.as_deref(),
                a.secret_headers.as_deref(),
                same_kind.and_then(|s| s.headers.as_ref()),
                same_kind.and_then(|s| s.headers.as_ref()),
            );
        }
        other => return Err(Error::Invalid(format!("trasporto `{}` non valido: usa stdio oppure http", clip(other, 30)))),
    }
    Ok(cfg)
}

/// The variables or headers of a connector: the plain ones the call wrote, and the secret ones by name,
/// each holding what is already stored under that name (kept through the mask) or an empty value (to be
/// filled in with `aiw_secret_request`: the name is declared, there is nothing to hide yet).
/// `inherited`: the entries an edit keeps when the call does not repeat them; `stored`: where the
/// secret values live (always looked at, so that a secret survives an edit).
fn entries(
    plain: Option<&[(String, String)]>,
    secret: Option<&[String]>,
    inherited: Option<&Map<String, Value>>,
    stored: Option<&Map<String, Value>>,
) -> Option<Map<String, Value>> {
    let mut out = Map::new();
    match plain {
        Some(list) => out.extend(list.iter().map(|(k, v)| (k.clone(), Value::String(v.clone())))),
        None => out.extend(
            inherited.into_iter().flatten().filter(|(k, _)| !crate::secrets::is_secret_name(k)).map(|(k, v)| (k.clone(), v.clone())),
        ),
    }
    let secret_names: Vec<String> = match secret {
        Some(list) => list.to_vec(),
        None => inherited.into_iter().flatten().filter(|(k, _)| crate::secrets::is_secret_name(k)).map(|(k, _)| k.clone()).collect(),
    };
    for name in secret_names {
        let has_value =
            stored.and_then(|m| m.get(&name)).is_some_and(|v| !v.is_null() && v.as_str() != Some("") && v.as_str() != Some(SECRET_MASK));
        out.insert(name, Value::String(if has_value { SECRET_MASK.into() } else { String::new() }));
    }
    Some(out).filter(|m| !m.is_empty())
}

/// An address for a connector: http or https, no user or password, nothing in it that is a secret.
fn check_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url.trim())
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
        .ok_or_else(|| Error::Invalid(format!("indirizzo non valido: {} (serve un URL http o https)", clip(url, 80))))?;
    if !parsed.username().is_empty() || parsed.password().is_some() || looks_secret(url) {
        return Err(Error::Invalid(
            "L'indirizzo contiene delle credenziali (utente e password o un token nei parametri): non scriverle qui. Se servono, dichiara il nome dell'intestazione in `secret_headers` (per esempio Authorization) e fai inserire il valore all'utente con aiw_secret_request.".into(),
        ));
    }
    Ok(())
}

/// The secret variables and headers of a connector, by name and by the `target` that
/// `aiw_secret_request` takes, and whether each has a value: never the value.
fn secret_slots(cfg: &McpServerConfig) -> Vec<Value> {
    let mut slots = Vec::new();
    for (kind, map) in [("mcp_env", &cfg.env), ("mcp_header", &cfg.headers)] {
        for (name, value) in map.iter().flatten().filter(|(k, _)| crate::secrets::is_secret_name(k)) {
            let set = !value.is_null() && value.as_str().is_none_or(|s| !s.is_empty());
            slots.push(json!({"name": name, "target": format!("{kind}:{}:{name}", cfg.id), "set": set}));
        }
    }
    slots
}

async fn connector_save(e: &Engine, args: &Value) -> Result<Value> {
    let a = parse_connector(args)?;
    let cfg = connector_config(e, &a, true)?;
    let creating = cfg.id.is_empty();
    // The command line the confirmation card showed, and the fingerprint of everything the card showed: the
    // command, its arguments and the names of its variables, as the call itself wrote them.
    let shown_hash =
        (cfg.transport == "stdio" && a.command.is_some()).then(|| connector_config(e, &a, false).ok().map(|c| approval_hash(&c))).flatten();
    let saved = e.save_mcp_server(cfg).await?;
    let id = saved.config.id.clone();
    let stored =
        e.0.store
            .get_doc::<McpServerConfig>("mcp", &id)?
            .ok_or_else(|| Error::NotFound(format!("connettore {id} non trovato dopo il salvataggio")))?;

    // A `stdio` server runs a command: the owner confirmed the one on the card, so that one is authorized
    // (the connector would otherwise wait for a second confirmation in the app). Only when the stored
    // command is exactly what the card showed; anything inherited from an earlier version is left to the app.
    let mut note = String::new();
    if stored.transport == "stdio" && !e.mcp_approved(&stored) {
        let hash = approval_hash(&stored);
        if shown_hash.as_deref() == Some(hash.as_str()) {
            e.approve_mcp_server(&id, &hash).await?;
        } else {
            note.push_str("Il comando di questo connettore non è stato confermato qui (la conferma non lo mostrava per intero): l'utente lo conferma dall'app. ");
        }
    }
    e.audit_user("connector.save", &format!("{} ({id}): {}", stored.name, connector_target(&stored, true)));

    let info = e.mcp_servers().into_iter().find(|s| s.config.id == id);
    let slots = secret_slots(&stored);
    let missing: Vec<String> = slots.iter().filter(|s| s["set"] == false).filter_map(|s| s["target"].as_str().map(String::from)).collect();
    if !missing.is_empty() {
        note.push_str(&format!(
            "Manca il valore dei segreti dichiarati: non chiederlo in chat, usa aiw_secret_request con il target di ciascuno ({}). ",
            missing.join(", ")
        ));
    }
    if let Some(error) = info.as_ref().and_then(|i| i.error.as_deref()).filter(|_| stored.enabled) {
        note.push_str(&format!("Stato: {}. ", text(error, 200)));
    }
    Ok(json!({
        "id": id,
        "name": stored.name,
        "created": creating,
        "transport": stored.transport,
        "target": connector_target(&stored, true),
        "enabled": stored.enabled,
        "command_confirmed": stored.transport != "stdio" || e.mcp_approved(&stored),
        "status": info.as_ref().map(|i| i.status.clone()),
        "tools": info.as_ref().map(|i| i.tools.len()),
        "secrets": slots,
        "note": note.trim(),
    }))
}

async fn connector_remove(e: &Engine, args: &Value) -> Result<Value> {
    let id = req_str(args, "id")?;
    let cfg = find_connector(e, &id).ok_or_else(|| {
        Error::NotFound(format!("Connettore `{}` non trovato: guarda l'elenco con aiw_inspect (topic connectors).", clip(&id, 60)))
    })?;
    e.remove_mcp_server(&cfg.id).await?;
    e.audit_user("connector.remove", &format!("{} ({}): {}", cfg.name, cfg.id, connector_target(&cfg, true)));
    let names = |map: &Option<Map<String, Value>>| map.iter().flat_map(|m| m.keys().cloned()).collect::<Vec<_>>();
    Ok(json!({
        "removed": cfg.id,
        "name": cfg.name,
        "transport": cfg.transport,
        "target": connector_target(&cfg, true),
        "variables": names(&cfg.env),
        "headers": names(&cfg.headers),
        "note": "Rimosso con i valori che aveva salvato (variabili, intestazioni, accesso): per rimetterlo serve crearlo di nuovo e reinserire i segreti.",
    }))
}

// ------------------------------------------------------------------ agents

/// A tool an agent made here may not have beyond the ones the contract bans by name: `handoff` moves the
/// session to another agent with that agent's own (possibly freer) mode.
const NOT_FOR_MADE_AGENTS: &[&str] = &["handoff"];

fn is_assistant(id: &str) -> bool {
    id == crate::defaults::ASSISTANT_ID
}

/// Why an agent with these settings is outside what the tool manages (and so is not made or edited from the chat), if it is.
fn envelope_problem(e: &Engine, a: &AgentProfile) -> Option<String> {
    if !["plan", "ask"].contains(&a.mode.as_str()) {
        return Some(format!("la modalità «{}» non è tra quelle ammesse (solo plan e ask)", a.mode));
    }
    if !["read-only", "workspace-write"].contains(&a.sandbox.as_str()) {
        return Some(format!("la sandbox «{}» non è tra quelle ammesse (solo read-only e workspace-write)", a.sandbox));
    }
    if !a.delegate_tools.is_empty() {
        return Some("concede strumenti ai suoi subagenti (delegate_tools)".into());
    }
    let registry = e.tool_registry();
    for tool in &a.tools {
        if tool == "*" {
            return Some("usa «*» (tutti gli strumenti): vanno elencati uno per uno".into());
        }
        if super::is_management(tool) {
            return Some(format!("ha lo strumento di gestione `{tool}`: solo l'Assistente li usa"));
        }
        if NOT_FOR_MADE_AGENTS.contains(&tool.as_str()) {
            return Some(format!("ha lo strumento `{tool}`, che sposta la conversazione su un altro assistente"));
        }
        if registry.get(tool).is_some_and(|t| t.explicit_grant()) {
            return Some(format!("ha lo strumento `{tool}`, che richiede una concessione esplicita"));
        }
    }
    None
}

/// An id for a new agent: the name without accents or punctuation, unique among the agents.
fn new_agent_id(e: &Engine, name: &str) -> String {
    let mut base = String::new();
    for c in name.trim().to_lowercase().chars() {
        let c = match c {
            'à' | 'á' | 'â' | 'ä' => 'a',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            base.push(c);
        } else if !base.is_empty() && !base.ends_with('-') {
            base.push('-');
        }
    }
    let base: String = base.trim_matches('-').chars().take(36).collect();
    let base = base.trim_matches('-').to_string();
    let base = if base.is_empty() { "agente".to_string() } else { base };
    let taken = |id: &str| crate::defaults::is_builtin_agent(id) || e.agent(id).is_ok();
    if !taken(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|id| !taken(id)).unwrap_or(base)
}

/// A new agent before the call fills it in: mode `ask`, writing only in the working folder, the model adviser.
fn blank_agent(id: String) -> AgentProfile {
    AgentProfile {
        id,
        name: String::new(),
        description: String::new(),
        inference_profile: "auto:agent".into(),
        system_prompt: String::new(),
        tools: vec![],
        mode: "ask".into(),
        sandbox: "workspace-write".into(),
        max_turns: 30,
        output_schema: None,
        budget: None,
        candidate_profiles: vec![],
        roles: None,
        knowledge: vec![],
        delegate_tools: vec![],
    }
}

fn agent_save(e: &Engine, args: &Value) -> Result<Value> {
    // `delegate_tools` is never set from here, and saying so is clearer than ignoring it.
    if args.get("delegate_tools").is_some_and(|v| !v.is_null()) {
        return Err(Error::Denied(
            "`delegate_tools` non si imposta da qui: un assistente creato dalla chat non concede strumenti ai subagenti.".into(),
        ));
    }
    let id = opt_str(args, "id")?;
    let name = opt_str(args, "name")?;
    let description = opt_str(args, "description")?;
    let system_prompt = opt_str(args, "system_prompt")?;
    let tools = opt_list(args, "tools")?;
    let mode = opt_str(args, "mode")?;
    let sandbox = opt_str(args, "sandbox")?;
    let model = opt_str(args, "model")?;

    let stored = match &id {
        Some(id) if is_assistant(id) => {
            return Err(Error::Denied("L'Assistente non si crea né si modifica da qui: è l'assistente di questa chat.".into()))
        }
        Some(id) if crate::defaults::is_builtin_agent(id) => {
            return Err(Error::Denied(format!(
                "«{}» è un assistente integrato e non si modifica da qui: per cambiarlo si usa la vista Sviluppatore, oppure se ne crea una copia con un altro nome.",
                text(id, 60)
            )))
        }
        Some(id) => Some(e.agent(id).map_err(|_| {
            Error::NotFound(format!(
                "Non esiste un assistente `{}`: per crearne uno nuovo ometti `id`, per modificarne uno guarda aiw_inspect (topic agents).",
                text(id, 60)
            ))
        })?),
        None => None,
    };
    if let Some(s) = &stored {
        // An edit never reaches an agent that already has more than the tool would give: that one is edited by its owner.
        if let Some(problem) = envelope_problem(e, s) {
            return Err(Error::Denied(format!(
                "L'assistente «{}» ha già permessi che da qui non si gestiscono ({problem}): si modifica dalla vista Sviluppatore.",
                text(&s.name, 60)
            )));
        }
    }
    let creating = stored.is_none();
    if creating {
        for (field, given) in [("name", name.is_some()), ("system_prompt", system_prompt.is_some()), ("tools", tools.is_some())] {
            if !given {
                return Err(Error::Invalid(format!("manca `{field}`")));
            }
        }
    }
    for (field, value, max) in [
        ("name", &name, MAX_NAME_CHARS),
        ("description", &description, MAX_DESCRIPTION_CHARS),
        ("system_prompt", &system_prompt, MAX_PROMPT_CHARS),
    ] {
        if value.as_ref().is_some_and(|v| v.chars().count() > max) {
            return Err(Error::Invalid(format!("`{field}` è troppo lungo (al massimo {max} caratteri)")));
        }
    }
    if let Some(m) = &mode {
        if !["plan", "ask"].contains(&m.as_str()) {
            return Err(Error::Denied(format!(
                "`mode` può essere solo plan o ask (non «{}»): le modalità più libere si scelgono dalla vista Sviluppatore.",
                clip(m, 20)
            )));
        }
    }
    if let Some(s) = &sandbox {
        if !["read-only", "workspace-write"].contains(&s.as_str()) {
            return Err(Error::Denied(format!("`sandbox` può essere solo read-only o workspace-write (non «{}»).", clip(s, 20))));
        }
    }
    let tools = tools.map(|list| {
        let mut seen: Vec<String> = Vec::new();
        for t in list.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
            if !seen.contains(&t) {
                seen.push(t);
            }
        }
        seen
    });
    if let Some(list) = &tools {
        // The same rules as for the stored profile, with the words for what the model wrote.
        let probe = AgentProfile { tools: list.clone(), ..blank_agent(String::new()) };
        if let Some(problem) = envelope_problem(e, &probe) {
            return Err(Error::Denied(format!("`tools` non è ammesso: {problem}. Elenca gli strumenti uno per uno, senza strumenti di gestione né a concessione esplicita.")));
        }
    }
    let profile = match model.as_deref() {
        None => None,
        Some("auto") => Some("auto:agent".to_string()),
        Some(m) if crate::adviser::parse_auto_profile(m).is_some() || e.profile(m).is_ok() => Some(m.to_string()),
        Some(m) => {
            return Err(Error::Invalid(format!(
                "Il profilo `{}` non esiste. Usa l'id di un profilo esistente (aiw_inspect, topic agents) oppure auto:agent, auto:chat, auto:reasoning, auto:vision; per fissare un modello preciso usa aiw_model_use.",
                text(m, 60)
            )))
        }
    };

    let mut agent = match stored {
        Some(s) => s,
        None => blank_agent(new_agent_id(e, name.as_deref().unwrap_or_default())),
    };
    let same_name_elsewhere = name
        .as_deref()
        .filter(|_| creating)
        .and_then(|n| e.agents().ok()?.into_iter().find(|a| a.name.eq_ignore_ascii_case(n)).map(|a| a.id));
    if let Some(v) = name {
        agent.name = v;
    }
    if let Some(v) = description {
        agent.description = v;
    }
    if let Some(v) = system_prompt {
        agent.system_prompt = v;
    }
    if let Some(v) = tools {
        agent.tools = v;
    }
    if let Some(v) = mode {
        agent.mode = v;
    }
    if let Some(v) = sandbox {
        agent.sandbox = v;
    }
    if let Some(v) = profile {
        agent.inference_profile = v;
    }
    agent.delegate_tools = vec![];
    // The whole profile, once more, as it will be stored.
    if let Some(problem) = envelope_problem(e, &agent) {
        return Err(Error::Denied(format!("L'assistente così com'è non è ammesso: {problem}.")));
    }
    let agent = e.save_agent(agent)?;
    e.audit_user("agent.save", &format!("{} ({}){}", agent.name, agent.id, if creating { " · nuovo" } else { "" }));
    let mut note = String::new();
    if let Some(other) = same_name_elsewhere {
        note = format!("Esisteva già un assistente con questo nome (id `{other}`): se volevi modificare quello, passa `id`.");
    }
    Ok(json!({
        "id": agent.id,
        "name": agent.name,
        "created": creating,
        "mode": agent.mode,
        "sandbox": agent.sandbox,
        "tools": agent.tools,
        "model": agent.inference_profile,
        "note": note,
    }))
}

fn agent_delete(e: &Engine, args: &Value) -> Result<Value> {
    let id = req_str(args, "id")?;
    if crate::defaults::is_builtin_agent(&id) {
        return Err(Error::Denied(format!(
            "«{}» è un assistente integrato e non si elimina: si possono eliminare solo quelli creati dall'utente.",
            text(&id, 60)
        )));
    }
    let agent = e.agent(&id).map_err(|_| {
        Error::NotFound(format!("Assistente `{}` non trovato: guarda l'elenco con aiw_inspect (topic agents).", text(&id, 60)))
    })?;
    for key in ["agent.default", "proactive.agent"] {
        if e.0.config.get_str(key).as_deref() == Some(id.as_str()) {
            return Err(Error::Conflict(format!(
                "«{}» è l'assistente scelto in `{key}`: scegline un altro con aiw_setting_set prima di eliminarlo.",
                text(&agent.name, 60)
            )));
        }
    }
    // The engine refuses (in Italian) an agent that a workflow or a team still uses, and records the deletion.
    e.delete_agent(&id)?;
    Ok(json!({
        "deleted": id,
        "name": text(&agent.name, 80),
        "note": "Eliminato: da qui non si annulla. Per averne uno uguale va creato di nuovo.",
    }))
}

// ------------------------------------------------------------------ memories

fn memory_summary(m: &aiw_core::api::Memory) -> Value {
    json!({"id": m.id, "kind": m.kind, "status": m.status, "text": text(&m.text, 160)})
}

fn memory_review(e: &Engine, args: &Value) -> Result<Value> {
    let id = req_str(args, "id")?;
    let action = req_str(args, "action")?;
    let (memory, note) = match action.as_str() {
        "restore" => (e.memory_restore(&id)?, "Il ricordo è di nuovo in uso."),
        "approve" => (e.memory_approve(&id)?, "La proposta è confermata: ora è un ricordo in uso."),
        "reject" => (e.memory_reject(&id)?, "La proposta è scartata ed eliminata per sempre."),
        other => {
            return Err(Error::Invalid(format!("azione sconosciuta «{}»: usa restore, approve o reject", clip(other, 30))));
        }
    };
    Ok(json!({"action": action, "memory": memory_summary(&memory), "note": note}))
}

fn memory_delete(e: &Engine, args: &Value) -> Result<Value> {
    let id = opt_str(args, "id")?;
    let all = opt_bool(args, "all")?.unwrap_or(false);
    match (id, all) {
        (Some(_), true) => Err(Error::Invalid("indica `id` oppure `all: true`, non tutti e due".into())),
        (None, false) => Err(Error::Invalid("manca `id` (un ricordo) oppure `all: true` (tutti i ricordi)".into())),
        (None, true) => {
            let n = e.memory_delete_all()?;
            Ok(json!({"deleted": n, "note": "Tutti i ricordi sono stati cancellati per sempre: da qui non si annulla."}))
        }
        (Some(id), false) => {
            let memory = e.memory(&id)?;
            e.memory_delete(&id)?;
            Ok(json!({"deleted": memory_summary(&memory), "note": "Cancellato per sempre: da qui non si annulla."}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::management::{is_management, test_ctx, ALL, MODIFYING, STRONG};
    use crate::tests::{engine, wait_finished};
    use crate::RunOrigin;
    use aiw_core::api::ApprovalRequest;
    use aiw_core::config::Layer;
    use aiw_runtimes::ScriptedBackend;
    use std::collections::BTreeSet;
    use std::time::Duration;

    const OWNER: RunOrigin = RunOrigin { owner: true, questions: true, secrets: true };

    /// A made-up secret that no real service would accept, long enough to be hunted for in an answer.
    const FAKE_SECRET: &str = "fake-secret-value-0123456789";

    /// The tool by name, called in a run of the owner.
    async fn run(e: &Engine, name: &str, args: Value) -> Result<ToolOutput> {
        e.tool_registry().get(name).unwrap_or_else(|| panic!("{name} is not registered")).call(args, &test_ctx(e, Some(OWNER))).await
    }

    /// The JSON an accepted call answers with.
    async fn ok(e: &Engine, name: &str, args: Value) -> Value {
        let out = run(e, name, args).await.unwrap_or_else(|err| panic!("{name} failed: {err}"));
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.chars().count() <= MAX_CHARS);
        serde_json::from_str(&out.content).unwrap_or_else(|err| panic!("{err}: {}", out.content))
    }

    /// The message of a refused call.
    async fn refused(e: &Engine, name: &str, args: Value) -> String {
        match run(e, name, args).await {
            Err(err) => err.to_string(),
            Ok(out) => panic!("{name} was accepted: {}", out.content),
        }
    }

    /// The values of the `global` layer, as the API shows them.
    fn global(e: &Engine) -> Map<String, Value> {
        e.config().layers.into_iter().find(|l| l.name == "global").unwrap().values
    }

    async fn next_approval(e: &Engine) -> ApprovalRequest {
        for _ in 0..250 {
            if let Some(a) = e.approvals().into_iter().next() {
                return a;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("no confirmation was asked");
    }

    // ------------------------------------------------------------------ the tools and the contract

    #[tokio::test]
    async fn the_eight_tools_are_registered_and_declare_the_risks_of_the_contract() {
        let (e, _d, _w) = engine(vec![]).await;
        let mine = [
            "aiw_setting_set",
            "aiw_setting_set_protected",
            "aiw_connector_save",
            "aiw_connector_remove",
            "aiw_agent_save",
            "aiw_agent_delete",
            "aiw_memory_review",
            "aiw_memory_delete",
        ];
        let list = tools(&e);
        assert_eq!(list.iter().map(|t| t.name().to_string()).collect::<Vec<_>>(), mine);
        for tool in &list {
            let name = tool.name();
            assert!(ALL.contains(&name) && is_management(name), "{name} is not in the contract");
            // The risk is what the lists of `management/mod.rs` say: Write for MODIFYING, Exec for STRONG, nothing else.
            let expected = match (MODIFYING.contains(&name), STRONG.contains(&name)) {
                (true, false) => Risk::Write,
                (false, true) => Risk::Exec,
                other => panic!("{name} must be in exactly one of MODIFYING and STRONG: {other:?}"),
            };
            assert_eq!(tool.risk(), expected, "{name}");
            assert!(tool.explicit_grant(), "{name}: a `*` in an agent must not include it");
            assert!(tool.description().chars().count() > 80, "{name}");
            let params = tool.parameters();
            assert_eq!(params["type"], "object");
            for required in params["required"].as_array().into_iter().flatten() {
                assert!(
                    params["properties"].get(required.as_str().unwrap()).is_some(),
                    "{name}: `{required}` is required but not described"
                );
            }
            // The engine has registered the same tool, which is what an agent run and `execute_tool` use.
            assert_eq!(e.tool_registry().get(name).map(|t| t.risk()), Some(expected), "{name}");
        }
        // The two lists name these eight and, for the other areas, only the tools of the other items: nothing was left without an owner.
        let others =
            ["aiw_model_download", "aiw_model_use", "aiw_model_delete", "aiw_runtime_control", "aiw_runtime_install", "aiw_secret_clear"];
        let contract: BTreeSet<&str> = MODIFYING.iter().chain(STRONG.iter()).copied().collect();
        let owned: BTreeSet<&str> = mine.iter().chain(others.iter()).copied().collect();
        assert_eq!(contract, owned);
    }

    #[tokio::test]
    async fn without_an_origin_or_without_ownership_none_of_the_eight_does_anything() {
        let (e, _d, _w) = engine(vec![]).await;
        let before = global(&e);
        let not_owner = RunOrigin { owner: false, questions: true, secrets: false };
        let valid = |name: &str| match name {
            "aiw_setting_set" | "aiw_setting_set_protected" => json!({"key": "ui.theme", "value": "dark"}),
            "aiw_connector_save" => json!({"name": "x", "transport": "http", "url": "http://127.0.0.1:9/mcp", "enabled": false}),
            "aiw_connector_remove" | "aiw_agent_delete" => json!({"id": "x"}),
            "aiw_agent_save" => json!({"name": "Prova", "system_prompt": "ciao", "tools": ["todo"]}),
            _ => json!({"all": true}),
        };
        for tool in tools(&e) {
            let name = tool.name().to_string();
            let err = tool.call(valid(&name), &test_ctx(&e, None)).await.unwrap_err();
            assert!(
                matches!(err, Error::Denied(_)) && err.to_string().contains("disponibile solo all'Assistente in una chat"),
                "{name}: {err}"
            );
            let err = tool.call(valid(&name), &test_ctx(&e, Some(not_owner))).await.unwrap_err();
            assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("accesso completo"), "{name}: {err}");
        }
        assert_eq!(global(&e), before);
        assert!(e.mcp_servers().is_empty() && e.memories(Some("all"), None).unwrap().is_empty());
        assert!(e.agents().unwrap().iter().all(|a| a.name != "Prova"));
        // The owner gets past the gate: what comes back is the tool's own answer, not a refusal of the origin.
        let err = refused(&e, "aiw_connector_remove", json!({"id": "x"})).await;
        assert!(err.contains("non trovato"), "{err}");
    }

    // ------------------------------------------------------------------ settings

    #[tokio::test]
    async fn every_key_of_the_schema_has_a_decision_and_the_tools_follow_it() {
        let (e, _d, _w) = engine(vec![]).await;
        let schema = e.config().schema;
        let secret: BTreeSet<&str> = schema.properties.iter().filter(|(_, f)| f.secret).map(|(k, _)| k.as_str()).collect();
        let protected: BTreeSet<&str> = management_protected_keys().iter().copied().collect();
        let forbidden: BTreeSet<&str> = aiw_core::config::management_forbidden_keys().into_iter().collect();
        let ordinary: BTreeSet<&str> = ORDINARY.iter().copied().collect();
        assert_eq!(ordinary.len(), ORDINARY.len(), "a key listed twice in ORDINARY");
        let all: BTreeSet<&str> = schema.properties.keys().map(String::as_str).collect();

        // Every key is decided: a key added to the schema fails here until it is classified (ordinary, secret, protected or forbidden).
        for key in &all {
            let access = classify(&schema, key);
            assert!(
                !matches!(access, Access::Unknown | Access::Unclassified),
                "`{key}` is a key of the settings schema that nobody classified: decide it in ORDINARY (here) or in the lists of aiw-core's config.rs"
            );
        }
        // The classes are disjoint and together are the whole schema (the mail accounts being the one special case).
        let mut classes = BTreeSet::new();
        for class in [&secret, &protected, &forbidden, &ordinary, &BTreeSet::from([MAIL_ACCOUNTS])] {
            for key in class {
                assert!(classes.insert(*key), "`{key}` is in two classes");
            }
        }
        assert_eq!(classes, all, "a key of the schema is in no class, or a class names a key that is not in the schema");
        assert_eq!((secret.len(), protected.len(), forbidden.len()), (1, 6, 13));
        assert_eq!(secret, BTreeSet::from(["search.brave_api_key"]));
        for key in &ordinary {
            assert_eq!(classify(&schema, key), Access::Ordinary, "{key}");
        }

        // The tools do what the classes say, for every key: nothing but ordinary keys goes through `aiw_setting_set`,
        // nothing but the six through the strong one, and a refusal writes nothing.
        let before = global(&e);
        for key in &all {
            let access = classify(&schema, key);
            if access != Access::Ordinary {
                let message = refused(&e, "aiw_setting_set", json!({"key": key, "value": "x"})).await;
                match access {
                    Access::Protected => assert!(message.contains("aiw_setting_set_protected"), "{key}: {message}"),
                    Access::Secret => {
                        assert!(message.contains("aiw_secret_request") && message.contains(&format!("setting:{key}")), "{key}: {message}")
                    }
                    _ => assert!(message.contains("vista Sviluppatore"), "{key}: {message}"),
                }
            }
            if access != Access::Protected {
                let message = refused(&e, "aiw_setting_set_protected", json!({"key": key, "value": "x"})).await;
                if access == Access::Ordinary {
                    assert!(message.contains("usa aiw_setting_set") && message.contains("workspace.root"), "{key}: {message}");
                }
            }
        }
        assert_eq!(global(&e), before, "a refusal wrote something");
    }

    #[tokio::test]
    async fn an_ordinary_setting_changes_and_the_answer_says_what_it_was() {
        let (e, _d, _w) = engine(vec![]).await;
        let first = ok(&e, "aiw_setting_set", json!({"key": "ui.theme", "value": "dark"})).await;
        assert_eq!(first["key"], "ui.theme");
        assert_eq!(
            (first["previous"].clone(), first["previous_saved"].clone(), first["now"].clone()),
            (json!("system"), json!(false), json!("dark"))
        );
        assert_eq!(first["layer"], "global");
        assert!(first["note"].as_str().unwrap().contains("value: null"), "{first}");
        assert_eq!(e.0.config.get_str("ui.theme").as_deref(), Some("dark"));
        assert_eq!(global(&e).get("ui.theme"), Some(&json!("dark")));
        // The next change finds a value of its own to report.
        let second = ok(&e, "aiw_setting_set", json!({"key": "ui.theme", "value": "light"})).await;
        assert_eq!(
            (second["previous"].clone(), second["previous_saved"].clone(), second["now"].clone()),
            (json!("dark"), json!(true), json!("light"))
        );
        // `null` puts the default back.
        let back = ok(&e, "aiw_setting_set", json!({"key": "ui.theme", "value": null})).await;
        assert_eq!((back["previous"].clone(), back["now"].clone()), (json!("light"), json!("system")));
        assert!(!global(&e).contains_key("ui.theme"));
        // The changes are in the audit trail, and they are the person's choices made through the app's own function.
        assert!(e.audit().iter().filter(|a| a.action == "config.set" && a.detail == "global:ui.theme").count() >= 3);

        // A number or a true/false written as text is understood, one that does not fit the schema is refused with the schema's words.
        let n = ok(&e, "aiw_setting_set", json!({"key": "proactive.interval_minutes", "value": "45"})).await;
        assert_eq!(n["now"], json!(45));
        let b = ok(&e, "aiw_setting_set", json!({"key": "routing.bandit", "value": "true"})).await;
        assert_eq!((b["previous"].clone(), b["now"].clone()), (json!(false), json!(true)));
        let list = ok(&e, "aiw_setting_set", json!({"key": "proactive.watch_folders", "value": ["una", "due"]})).await;
        assert_eq!(list["now"], json!(["una", "due"]));
        assert!(refused(&e, "aiw_setting_set", json!({"key": "ui.theme", "value": "fucsia"})).await.contains("deve essere uno tra"));
        assert!(refused(&e, "aiw_setting_set", json!({"key": "proactive.interval_minutes", "value": 1})).await.contains("almeno"));
        assert!(refused(&e, "aiw_setting_set", json!({"key": "ui.theme"})).await.contains("manca `value`"));
        assert!(refused(&e, "aiw_setting_set", json!({"value": "dark"})).await.contains("manca `key`"));
        // None of the refusals wrote anything: the theme is still the default.
        assert!(!global(&e).contains_key("ui.theme"));

        // The agent that new sessions use must exist.
        assert!(refused(&e, "aiw_setting_set", json!({"key": "agent.default", "value": "inesistente"}))
            .await
            .contains("Non esiste un assistente"));
        assert_eq!(ok(&e, "aiw_setting_set", json!({"key": "agent.default", "value": "planner"})).await["previous"], "operator");
    }

    #[tokio::test]
    async fn a_value_fixed_by_another_layer_is_reported_instead_of_pretending_it_changed() {
        let (e, _d, _w) = engine(vec![]).await;
        e.0.config.set(Layer::Device, "ui.language", json!("en")).unwrap();
        let answer = ok(&e, "aiw_setting_set", json!({"key": "ui.language", "value": "it"})).await;
        assert_eq!(answer["now"], "en", "{answer}");
        let note = answer["note"].as_str().unwrap();
        assert!(note.contains("device") && note.contains("resta"), "{note}");
        // It was written to the global layer all the same.
        assert_eq!(global(&e).get("ui.language"), Some(&json!("it")));
    }

    #[tokio::test]
    async fn secrets_protected_and_forbidden_keys_are_refused_with_what_to_use_instead() {
        let (e, _d, _w) = engine(vec![]).await;
        let before = global(&e);
        // A secret: the message says which tool and which target, and the value written in the call is not echoed.
        let message = refused(&e, "aiw_setting_set", json!({"key": "search.brave_api_key", "value": FAKE_SECRET})).await;
        assert!(message.contains("aiw_secret_request") && message.contains("setting:search.brave_api_key"), "{message}");
        assert!(!message.contains(FAKE_SECRET));
        assert!(e.0.config.get_str("search.brave_api_key").is_none());
        // The strong tool refuses it as well, and a mask of a secret is not a way in either.
        let message = refused(&e, "aiw_setting_set_protected", json!({"key": "search.brave_api_key", "value": FAKE_SECRET})).await;
        assert!(message.contains("aiw_secret_request"), "{message}");
        // A machine setting through the ordinary tool.
        for key in management_protected_keys() {
            let message = refused(&e, "aiw_setting_set", json!({"key": key, "value": "x"})).await;
            assert!(message.contains("aiw_setting_set_protected"), "{key}: {message}");
        }
        // The protections of the app: never, from either tool, and the values stay as they were.
        let effective_before = e.config().effective;
        for key in aiw_core::config::management_forbidden_keys() {
            for tool in ["aiw_setting_set", "aiw_setting_set_protected"] {
                let err = run(&e, tool, json!({"key": key, "value": "yolo"})).await.unwrap_err();
                assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("vista Sviluppatore"), "{key} via {tool}: {err}");
            }
        }
        for (key, value) in [
            ("policy.mode", json!("yolo")),
            ("sandbox.mode", json!("off")),
            ("network.allow_domains", json!(["x.com"])),
            ("approvals.timeout_s", json!(5)),
        ] {
            refused(&e, "aiw_setting_set", json!({"key": key, "value": value})).await;
        }
        assert_eq!(e.config().effective, effective_before);
        // The mail accounts, a key nobody knows, and the missing pieces.
        assert!(refused(&e, "aiw_setting_set", json!({"key": "connectors.mail.imap", "value": []})).await.contains("Account di posta"));
        let err = run(&e, "aiw_setting_set", json!({"key": "ui.inventata", "value": 1})).await.unwrap_err();
        assert!(matches!(err, Error::NotFound(_)) && err.to_string().contains("aiw_inspect"), "{err}");
        assert_eq!(global(&e), before, "a refusal wrote something");
        assert!(e.audit().iter().all(|a| a.action != "config.set"));
    }

    #[tokio::test]
    async fn the_strong_tool_changes_only_the_six_machine_settings() {
        let (e, _d, _w) = engine(vec![]).await;
        let channel = ok(&e, "aiw_setting_set_protected", json!({"key": "update.channel", "value": "beta"})).await;
        assert_eq!((channel["previous"].clone(), channel["now"].clone()), (json!("auto"), json!("beta")));
        let check = ok(&e, "aiw_setting_set_protected", json!({"key": "update.auto_check", "value": false})).await;
        assert_eq!((check["previous"].clone(), check["now"].clone()), (json!(true), json!(false)));
        let download = ok(&e, "aiw_setting_set_protected", json!({"key": "update.auto_download", "value": "true"})).await;
        assert_eq!(download["now"], json!(true));
        let lan = ok(&e, "aiw_setting_set_protected", json!({"key": "server.lan", "value": true})).await;
        assert_eq!((lan["previous"].clone(), lan["now"].clone()), (json!(false), json!(true)));
        // The folders: where models live, and the working folder (which the engine switches to at once).
        let models = tempfile::tempdir().unwrap();
        let models_path = models.path().display().to_string();
        let answer = ok(&e, "aiw_setting_set_protected", json!({"key": "models.dir", "value": models_path})).await;
        assert_eq!(answer["previous"], "");
        let work = tempfile::tempdir().unwrap();
        let root = work.path().join("nuova").display().to_string();
        let answer = ok(&e, "aiw_setting_set_protected", json!({"key": "workspace.root", "value": root})).await;
        assert_eq!(answer["now"], json!(root));
        assert!(work.path().join("nuova").is_dir(), "the engine made the folder");
        assert_eq!(e.workspace_root(), work.path().join("nuova"));
        // A value the schema does not accept is refused as ever.
        assert!(refused(&e, "aiw_setting_set_protected", json!({"key": "update.channel", "value": "nightly"}))
            .await
            .contains("deve essere uno tra"));
        assert_eq!(e.0.config.get_str("update.channel").as_deref(), Some("beta"));
    }

    #[tokio::test]
    async fn what_an_answer_says_about_a_setting_never_holds_a_secret() {
        let (e, _d, _w) = engine(vec![]).await;
        e.set_config("global", "search.brave_api_key", json!(FAKE_SECRET)).unwrap();
        // The last check: a value that is exactly a stored secret is masked even where the tool did not expect one.
        let answer = run(&e, "aiw_setting_set", json!({"key": "learning.profile", "value": FAKE_SECRET})).await.unwrap().content;
        assert!(!answer.contains(FAKE_SECRET), "{answer}");
        assert!(answer.contains(SECRET_MASK));
        assert_eq!(e.0.config.get_str("learning.profile").as_deref(), Some(FAKE_SECRET), "the value itself was saved");
        // An address with a user and a password: they are not in the answer, neither now nor as the value it replaces.
        let url = "http://alice:pw-fake-0042@localhost:8080/cerca";
        let answer = run(&e, "aiw_setting_set", json!({"key": "search.searxng_url", "value": url})).await.unwrap().content;
        assert!(!answer.contains("pw-fake-0042") && !answer.contains("alice"), "{answer}");
        assert!(answer.contains("http://localhost:8080/cerca"), "{answer}");
        let answer =
            run(&e, "aiw_setting_set", json!({"key": "search.searxng_url", "value": "http://localhost:9090"})).await.unwrap().content;
        assert!(!answer.contains("pw-fake-0042") && !answer.contains("alice"), "{answer}");
        // A private calendar link carries its secret in the path and the query: only the host and the short parts remain.
        let link = "https://calendar.example.com/ical/private-0123456789abcdef0123456789/basic.ics?token=fake-token-777";
        run(&e, "aiw_setting_set", json!({"key": "connectors.calendar.sources", "value": [link]})).await.unwrap();
        let answer = run(&e, "aiw_setting_set", json!({"key": "connectors.calendar.sources", "value": []})).await.unwrap().content;
        assert!(!answer.contains("private-0123") && !answer.contains("fake-token-777"), "{answer}");
        assert!(answer.contains("calendar.example.com"), "{answer}");
        // Plain text with a credential in it is redacted.
        let answer =
            run(&e, "aiw_setting_set", json!({"key": "learning.profile", "value": "token=fake-token-999 e altro"})).await.unwrap().content;
        let answer2 = run(&e, "aiw_setting_set", json!({"key": "learning.profile", "value": ""})).await.unwrap().content;
        assert!(!answer.contains("fake-token-999") && !answer2.contains("fake-token-999"), "{answer} / {answer2}");
    }

    #[tokio::test]
    async fn a_change_is_confirmed_even_in_yolo_and_changes_nothing_before_the_owner_says_yes() {
        let (e, _d, _w) = engine(vec![
            ScriptedBackend::tool_turn("c1", "aiw_setting_set", json!({"key": "ui.theme", "value": "dark"})),
            ScriptedBackend::text_turn("Fatto: tema scuro."),
        ])
        .await;
        let gestore = AgentProfile {
            id: "gestore".into(),
            name: "Gestore".into(),
            mode: "yolo".into(),
            tools: vec!["aiw_setting_set".into()],
            ..e.agent("planner").unwrap()
        };
        e.save_agent(gestore).unwrap();
        let s = e.create_session("gestore", None).unwrap();
        let run_id = e.start_run_from(&s.id, "metti il tema scuro".into(), None, None, vec![], Some(OWNER)).await.unwrap();
        let req = next_approval(&e).await;
        assert_eq!(req.tool, "aiw_setting_set");
        assert_eq!(req.risk, "medium");
        assert!(
            req.reason.contains("L'Assistente vuole cambiare la configurazione dell'app") && req.reason.ends_with("ui.theme"),
            "{}",
            req.reason
        );
        assert_eq!(req.args["value"], "dark");
        assert!(!global(&e).contains_key("ui.theme"), "nothing changed before the answer");
        // A channel or a paired phone cannot answer; the request stays open.
        assert!(e.respond_approval(&req.id, true, false).is_err());
        assert_eq!(e.approvals().len(), 1);
        e.respond_approval_as(&req.id, true, false, true).unwrap();
        wait_finished(&e, &run_id).await;
        assert_eq!(global(&e).get("ui.theme"), Some(&json!("dark")));
        let messages = e.messages(&s.id).unwrap();
        let tool_message = messages.iter().find(|m| m.role == "tool").unwrap();
        assert!(tool_message.content.contains("\"previous\":\"system\""), "{}", tool_message.content);
        assert_eq!(messages.last().unwrap().content, "Fatto: tema scuro.");

        // The owner may also say no: nothing changes and the model is told not to insist.
        let (e, _d, _w) = engine(vec![
            ScriptedBackend::tool_turn("c1", "aiw_setting_set", json!({"key": "ui.theme", "value": "dark"})),
            ScriptedBackend::text_turn("Va bene, lascio com'è."),
        ])
        .await;
        e.save_agent(AgentProfile {
            id: "gestore".into(),
            name: "Gestore".into(),
            mode: "auto".into(),
            tools: vec!["aiw_setting_set".into()],
            ..e.agent("planner").unwrap()
        })
        .unwrap();
        let s = e.create_session("gestore", None).unwrap();
        let run_id = e.start_run_from(&s.id, "metti il tema scuro".into(), None, None, vec![], Some(OWNER)).await.unwrap();
        let req = next_approval(&e).await;
        e.respond_approval_as(&req.id, false, false, true).unwrap();
        wait_finished(&e, &run_id).await;
        assert!(!global(&e).contains_key("ui.theme"));
        let told = e.messages(&s.id).unwrap().into_iter().find(|m| m.role == "tool").unwrap().content;
        assert!(told.contains("Negato dall'utente") && told.contains("Non ripetere"), "{told}");
    }

    // ------------------------------------------------------------------ connectors

    fn stored(e: &Engine, id: &str) -> McpServerConfig {
        e.0.store.get_doc::<McpServerConfig>("mcp", id).unwrap().unwrap_or_else(|| panic!("connector {id} is not stored"))
    }

    /// The connector as the API shows it.
    fn info(e: &Engine, id: &str) -> aiw_core::api::McpServerInfo {
        e.mcp_servers().into_iter().find(|s| s.config.id == id).unwrap()
    }

    const PROGRAM: &str = "/definitely/not/a/program";

    #[tokio::test]
    async fn a_stdio_connector_is_saved_with_secret_names_only_and_the_command_on_the_card_is_confirmed() {
        let (e, _d, _w) = engine(vec![]).await;
        let args = json!({
            "name": "Github", "transport": "stdio", "command": PROGRAM, "args": ["--flag", "due parole"],
            "env": {"MODE": "a"}, "secret_env": ["GITHUB_TOKEN"], "enabled": false
        });
        // The card shows what will run: the command line, as the person would read it.
        let tool = e.tool_registry().get("aiw_connector_save").unwrap();
        assert_eq!(tool.subject(&args).as_deref(), Some("/definitely/not/a/program --flag \"due parole\""));
        let answer = ok(&e, "aiw_connector_save", args).await;
        let id = answer["id"].as_str().unwrap().to_string();
        assert!(id.starts_with("mcp_") && answer["created"] == true && answer["name"] == "Github", "{answer}");
        assert_eq!(answer["target"], "/definitely/not/a/program --flag \"due parole\"");

        let saved = stored(&e, &id);
        assert_eq!((saved.transport.as_str(), saved.command.as_deref(), saved.enabled), ("stdio", Some(PROGRAM), false));
        assert_eq!(saved.args, Some(vec!["--flag".to_string(), "due parole".to_string()]));
        // The secret is declared by its name and holds nothing yet; the plain variable keeps its value.
        assert_eq!(Value::Object(saved.env.clone().unwrap()), json!({"MODE": "a", "GITHUB_TOKEN": ""}));
        let isolation = if aiw_tools::sandbox::can_wrap_process() { "workspace-write" } else { "off" };
        assert_eq!(saved.sandbox, isolation, "a new server is isolated as the interface proposes, where the system can");
        // The command the card showed is confirmed: it will not wait for a second confirmation in the app.
        assert!(e.mcp_approved(&saved));
        assert_eq!(answer["command_confirmed"], true);
        assert!(e.audit().iter().any(|a| a.action == "mcp_approve" && a.detail.contains("--flag")), "the confirmation is in the audit");
        assert!(e.audit().iter().any(|a| a.action == "connector.save" && a.detail.contains(&id)));
        // Control: the same server saved through the API, not through the tool, has no confirmation.
        let mut plain = saved.clone();
        plain.id = String::new();
        plain.name = "altro".into();
        let other = e.save_mcp_server(plain).await.unwrap();
        assert!(!e.mcp_approved(&stored(&e, &other.config.id)), "only the tool confirms, and only what it showed");
        // What the model reads: the names and whether a value is set, and the target to ask for it; never a value.
        assert_eq!(answer["secrets"], json!([{"name": "GITHUB_TOKEN", "target": format!("mcp_env:{id}:GITHUB_TOKEN"), "set": false}]));
        assert!(answer["note"].as_str().unwrap().contains("aiw_secret_request"), "{answer}");
        assert!(!answer.to_string().contains("\"a\""), "no value of an environment variable in the answer: {answer}");
    }

    #[tokio::test]
    async fn an_enabled_stdio_connector_starts_once_its_command_is_confirmed() {
        let (e, _d, _w) = engine(vec![]).await;
        let answer = ok(&e, "aiw_connector_save", json!({"name": "locale", "transport": "stdio", "command": PROGRAM})).await;
        let id = answer["id"].as_str().unwrap();
        // Without the confirmation of the tool it would be waiting in the app (`approval_required`); with it, it was started
        // (this command does not exist, so the connection fails, but the attempt is made).
        assert_ne!(info(&e, id).status, "approval_required");
        assert!(!info(&e, id).approval_required);
        assert!(e.mcp_client(id).is_some(), "the confirmed command was started");
        assert_eq!(answer["command_confirmed"], true);
        assert_ne!(answer["status"], "approval_required");
        // A connector of the same name is refused, and so is an `id` that does not exist.
        let message = refused(&e, "aiw_connector_save", json!({"name": "locale", "transport": "stdio", "command": PROGRAM})).await;
        assert!(message.contains("Esiste già un connettore") && message.contains(id), "{message}");
        let err = run(&e, "aiw_connector_save", json!({"id": "mcp_inventato", "name": "x", "transport": "stdio", "command": PROGRAM}))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::NotFound(_)) && err.to_string().contains("ometti `id`"), "{err}");
        e.remove_mcp_server(id).await.unwrap();
    }

    #[tokio::test]
    async fn a_connector_never_takes_a_secret_value_in_its_command_environment_headers_or_address() {
        let (e, _d, _w) = engine(vec![]).await;
        let github = "ghp_fakefakefakefakefake0123456789";
        let cases: Vec<(Value, &str)> = vec![
            (json!({"name": "a", "transport": "stdio", "command": PROGRAM, "env": {"GITHUB_TOKEN": github}}), "secret_env"),
            (
                json!({"name": "a", "transport": "stdio", "command": PROGRAM, "env": {"DATABASE_URL": "postgres://utente:fake-pass-0042@db/x"}}),
                "credenziali",
            ),
            (json!({"name": "a", "transport": "stdio", "command": PROGRAM, "secret_env": ["PLAIN_NAME"]}), "non ha un nome da segreto"),
            (
                json!({"name": "a", "transport": "stdio", "command": PROGRAM, "args": ["--ok", "--api-key=fake-key-12345678"]}),
                "argomento n. 2",
            ),
            (
                json!({"name": "a", "transport": "stdio", "command": PROGRAM, "args": ["--ok", "--api-key", "fake-key-12345678"]}),
                "argomento n. 2",
            ),
            (json!({"name": "a", "transport": "stdio", "command": PROGRAM, "env": {"Mode": "a", "MODE": "b"}}), "più volte"),
            (json!({"name": "a", "transport": "stdio", "command": PROGRAM, "env": {"1MODE": "a"}}), "non è un nome valido"),
            (json!({"name": "a", "transport": "stdio", "command": PROGRAM, "url": "http://127.0.0.1:9/mcp"}), "valgono solo per http"),
            (
                json!({"name": "a", "transport": "http", "url": "http://127.0.0.1:9/mcp", "headers": {"Authorization": "Bearer fake-token-1234567"}}),
                "secret_headers",
            ),
            (
                json!({"name": "a", "transport": "http", "url": "http://127.0.0.1:9/mcp", "headers": {"X-Note": "Bearer fake-token-1234567"}}),
                "credenziali",
            ),
            (
                json!({"name": "a", "transport": "http", "url": "http://127.0.0.1:9/mcp", "secret_headers": ["Accept"]}),
                "non ha un nome da segreto",
            ),
            (json!({"name": "a", "transport": "http", "url": "https://utente:fake-pass-0042@example.com/mcp"}), "credenziali"),
            (json!({"name": "a", "transport": "http", "url": "https://example.com/mcp?token=fake-token-9"}), "credenziali"),
            (json!({"name": "a", "transport": "http", "url": "ftp://example.com/mcp"}), "indirizzo non valido"),
            (json!({"name": "a", "transport": "http", "url": "http://127.0.0.1:9/mcp", "command": PROGRAM}), "valgono solo per stdio"),
            (json!({"name": "a", "transport": "stdio"}), "manca `command`"),
            (json!({"name": "a", "transport": "http"}), "manca `url`"),
            (json!({"name": "a", "transport": "grpc", "url": "http://x"}), "trasporto"),
            (json!({"transport": "http", "url": "http://127.0.0.1:9/mcp"}), "manca `name`"),
        ];
        for (args, expected) in cases {
            let message = refused(&e, "aiw_connector_save", args.clone()).await;
            assert!(message.contains(expected), "{args}: {message}");
            for fake in [github, "fake-pass-0042", "fake-key-12345678", "fake-token-1234567", "fake-token-9"] {
                assert!(!message.contains(fake), "the refusal repeats a secret value: {message}");
            }
        }
        assert!(e.mcp_servers().is_empty(), "a refused call stored nothing");
        assert!(e.audit().iter().all(|a| a.action != "connector.save" && a.action != "mcp_approve"));
        // The same names and values are fine when they are not secrets.
        let answer = ok(
            &e,
            "aiw_connector_save",
            json!({"name": "ok", "transport": "stdio", "command": PROGRAM, "args": ["--auth-type=basic", "--verbose", "--keyboard"], "env": {"LANG": "it_IT.UTF-8", "RETRIES": 3, "VERBOSE": true}, "enabled": false}),
        )
        .await;
        let saved = stored(&e, answer["id"].as_str().unwrap());
        assert_eq!(Value::Object(saved.env.unwrap()), json!({"LANG": "it_IT.UTF-8", "RETRIES": "3", "VERBOSE": "true"}));
    }

    #[tokio::test]
    async fn a_secret_stays_when_a_connector_is_edited_and_an_http_server_needs_no_command_confirmation() {
        let (e, _d, _w) = engine(vec![]).await;
        let created = ok(
            &e,
            "aiw_connector_save",
            json!({"name": "Remoto", "transport": "http", "url": "http://127.0.0.1:9/mcp", "headers": {"Accept": "application/json"}, "secret_headers": ["Authorization"], "enabled": false}),
        )
        .await;
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(Value::Object(stored(&e, &id).headers.unwrap()), json!({"Accept": "application/json", "Authorization": ""}));
        assert_eq!(created["command_confirmed"], true, "nothing to confirm for an address");
        assert!(
            e.0.store.get_doc::<Value>("mcp_approval", &id).unwrap().is_none(),
            "no command confirmation was recorded for an http server"
        );
        assert!(e.audit().iter().all(|a| a.action != "mcp_approve"));
        assert_eq!(created["target"], "http://127.0.0.1:9/mcp");
        // The person puts the value in (what `aiw_secret_request` will do): the API keeps it and shows a mask.
        let mut with_secret = stored(&e, &id);
        with_secret.headers.as_mut().unwrap().insert("Authorization".into(), json!(format!("Bearer {FAKE_SECRET}")));
        e.save_mcp_server(with_secret).await.unwrap();
        assert_eq!(info(&e, &id).config.headers.unwrap()["Authorization"], SECRET_MASK);

        // An edit that repeats the secret name keeps the value, and one that does not mention headers keeps everything.
        let edited = ok(
            &e,
            "aiw_connector_save",
            json!({"id": id, "name": "Remoto", "transport": "http", "headers": {"Accept": "text/plain"}, "secret_headers": ["Authorization"]}),
        )
        .await;
        assert_eq!(edited["created"], false);
        let headers = stored(&e, &id).headers.unwrap();
        assert_eq!(headers["Authorization"], json!(format!("Bearer {FAKE_SECRET}")), "the secret survived the edit");
        assert_eq!(headers["Accept"], "text/plain");
        assert_eq!(edited["secrets"], json!([{"name": "Authorization", "target": format!("mcp_header:{id}:Authorization"), "set": true}]));
        assert!(!edited.to_string().contains(FAKE_SECRET));
        ok(&e, "aiw_connector_save", json!({"id": "Remoto", "enabled": false, "name": "Remoto-2"})).await;
        let headers = stored(&e, &id).headers.unwrap();
        assert_eq!(
            (headers["Authorization"].clone(), headers["Accept"].clone()),
            (json!(format!("Bearer {FAKE_SECRET}")), json!("text/plain"))
        );
        assert_eq!(stored(&e, &id).name, "Remoto-2");
        // Leaving the secret name out of an edit that lists them removes it, value included.
        ok(&e, "aiw_connector_save", json!({"id": id, "name": "Remoto-2", "secret_headers": []})).await;
        assert_eq!(Value::Object(stored(&e, &id).headers.unwrap()), json!({"Accept": "text/plain"}));
    }

    #[tokio::test]
    async fn the_tool_confirms_a_command_only_when_the_card_showed_all_of_it() {
        let (e, _d, _w) = engine(vec![]).await;
        // A server made through the API, still waiting for the person's confirmation.
        let waiting = e
            .save_mcp_server(McpServerConfig {
                id: "srv".into(),
                name: "srv".into(),
                transport: "stdio".into(),
                command: Some(PROGRAM.into()),
                args: Some(vec!["--flag".into()]),
                url: None,
                env: Some(json!({"MODE": "a"}).as_object().unwrap().clone()),
                headers: None,
                enabled: false,
                sampling: false,
                oauth_client_id: None,
                sandbox: "off".into(),
                sandbox_network: true,
            })
            .await
            .unwrap();
        assert_eq!(waiting.config.sandbox, "off");
        assert!(!e.mcp_approved(&stored(&e, "srv")));
        // A call that does not repeat the command is not a confirmation of it.
        let answer = ok(&e, "aiw_connector_save", json!({"id": "srv", "name": "srv", "enabled": false})).await;
        assert!(!e.mcp_approved(&stored(&e, "srv")) && answer["command_confirmed"] == false, "{answer}");
        assert!(answer["note"].as_str().unwrap().contains("non è stato confermato qui"), "{answer}");
        // Nor one that repeats the command but leaves out the arguments and variables it inherits.
        let answer = ok(&e, "aiw_connector_save", json!({"id": "srv", "name": "srv", "command": PROGRAM, "enabled": false})).await;
        assert!(!e.mcp_approved(&stored(&e, "srv")) && answer["command_confirmed"] == false, "{answer}");
        // One that shows all of it is. The isolation of the server stays what it was.
        let all = json!({"id": "srv", "name": "srv", "transport": "stdio", "command": PROGRAM, "args": ["--flag"], "env": {"MODE": "a"}, "enabled": false});
        let answer = ok(&e, "aiw_connector_save", all.clone()).await;
        assert!(e.mcp_approved(&stored(&e, "srv")) && answer["command_confirmed"] == true, "{answer}");
        assert_eq!(stored(&e, "srv").sandbox, "off");
        // The value of a variable is not part of the confirmation: changing it does not ask again.
        let mut changed = all.clone();
        changed["env"] = json!({"MODE": "b"});
        ok(&e, "aiw_connector_save", changed).await;
        assert!(e.mcp_approved(&stored(&e, "srv")));
        assert_eq!(stored(&e, "srv").env.unwrap()["MODE"], "b");
        // A different argument is a different command: the engine drops the confirmation and the tool, which showed it, gives it again.
        let mut other = all.clone();
        other["args"] = json!(["--other"]);
        let answer = ok(&e, "aiw_connector_save", other).await;
        assert!(e.mcp_approved(&stored(&e, "srv")) && answer["command_confirmed"] == true);
        assert_eq!(stored(&e, "srv").args, Some(vec!["--other".to_string()]));
        assert_eq!(e.audit().iter().filter(|a| a.action == "mcp_approve").count(), 2, "one confirmation for each command shown");
        // Control: the confirmation the engine keeps is the one of the stored command, so the old command is not approved any more.
        assert!(!e.mcp_approved(&McpServerConfig { args: Some(vec!["--flag".into()]), ..stored(&e, "srv") }));
    }

    #[tokio::test]
    async fn a_connector_is_removed_with_what_it_had_saved() {
        let (e, _d, _w) = engine(vec![]).await;
        let created = ok(
            &e,
            "aiw_connector_save",
            json!({"name": "Da togliere", "transport": "stdio", "command": PROGRAM, "env": {"MODE": "a"}, "secret_env": ["API_KEY"], "enabled": false}),
        )
        .await;
        let id = created["id"].as_str().unwrap().to_string();
        let mut with_secret = stored(&e, &id);
        with_secret.env.as_mut().unwrap().insert("API_KEY".into(), json!(FAKE_SECRET));
        e.save_mcp_server(with_secret).await.unwrap();
        assert!(e.0.store.get_doc::<Value>("mcp_approval", &id).unwrap().is_some());

        let tool = e.tool_registry().get("aiw_connector_remove").unwrap();
        let card = tool.subject(&json!({"id": "Da_togliere"})).unwrap();
        assert!(card.contains("Da_togliere") && card.contains(PROGRAM), "{card}");
        let answer = ok(&e, "aiw_connector_remove", json!({"id": "Da_togliere"})).await;
        assert_eq!((answer["removed"].clone(), answer["variables"].clone()), (json!(id), json!(["API_KEY", "MODE"])));
        assert!(!answer.to_string().contains(FAKE_SECRET) && !answer.to_string().contains("\"a\""), "{answer}");
        assert!(e.mcp_servers().is_empty());
        assert!(e.0.store.get_doc::<Value>("mcp_approval", &id).unwrap().is_none(), "the confirmation of its command went with it");
        assert!(e.audit().iter().any(|a| a.action == "connector.remove" && a.detail.contains(&id)));
        let err = run(&e, "aiw_connector_remove", json!({"id": id})).await.unwrap_err();
        assert!(matches!(err, Error::NotFound(_)) && err.to_string().contains("aiw_inspect"), "{err}");
        assert!(tool.subject(&json!({"id": "inesistente"})).is_none());
    }

    // ------------------------------------------------------------------ agents

    /// A call that makes (or, with `id`, edits) an agent: the three required fields and whatever else is given.
    fn make(name: &str, tools: Value, extra: Value) -> Value {
        let mut args = json!({"name": name, "system_prompt": "Sei un assistente di prova.", "tools": tools});
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            args[k] = v;
        }
        args
    }

    #[tokio::test]
    async fn an_agent_is_made_and_edited_from_the_chat_inside_the_safe_envelope() {
        let (e, _d, _w) = engine(vec![]).await;
        let created = ok(
            &e,
            "aiw_agent_save",
            make("Revisore di codice", json!(["read_file", "grep", "todo", "todo", " "]), json!({"description": "Rilegge il codice"})),
        )
        .await;
        assert_eq!(created["id"], "revisore-di-codice");
        assert_eq!(
            (created["created"].clone(), created["mode"].clone(), created["sandbox"].clone(), created["model"].clone()),
            (json!(true), json!("ask"), json!("workspace-write"), json!("auto:agent"))
        );
        assert_eq!(created["tools"], json!(["read_file", "grep", "todo"]), "duplicates and blanks are dropped");
        let agent = e.agent("revisore-di-codice").unwrap();
        assert_eq!((agent.name.as_str(), agent.description.as_str(), agent.max_turns), ("Revisore di codice", "Rilegge il codice", 30));
        assert_eq!(agent.system_prompt, "Sei un assistente di prova.");
        assert!(agent.delegate_tools.is_empty() && agent.budget.is_none() && agent.roles.is_none());
        assert!(e
            .audit()
            .iter()
            .any(|a| a.action == "agent.save" && a.detail.contains("revisore-di-codice") && a.detail.contains("nuovo")));
        let tool = e.tool_registry().get("aiw_agent_save").unwrap();
        assert_eq!(tool.subject(&make("Revisore di codice", json!([]), json!({}))).as_deref(), Some("Revisore di codice"));
        assert_eq!(tool.subject(&json!({"id": "revisore-di-codice"})).as_deref(), Some("revisore-di-codice"));

        // The same name again is another agent, and the answer says the first one exists.
        let again = ok(
            &e,
            "aiw_agent_save",
            make("Revisore di codice", json!(["todo"]), json!({"mode": "plan", "sandbox": "read-only", "model": "auto:reasoning"})),
        )
        .await;
        assert_eq!(again["id"], "revisore-di-codice-2");
        assert!(again["note"].as_str().unwrap().contains("revisore-di-codice"), "{again}");
        let second = e.agent("revisore-di-codice-2").unwrap();
        assert_eq!(
            (second.mode.as_str(), second.sandbox.as_str(), second.inference_profile.as_str()),
            ("plan", "read-only", "auto:reasoning")
        );
        // An existing profile is a valid model, and so is an MCP tool of a server that is not connected now.
        let third = ok(&e, "aiw_agent_save", make("Con profilo", json!(["mcp__github__list_issues"]), json!({"model": "default"}))).await;
        assert_eq!(third["model"], "default");
        // Names with accents and symbols make a plain id; a name with nothing usable makes `agente`.
        assert_eq!(ok(&e, "aiw_agent_save", make("Più città!", json!([]), json!({}))).await["id"], "piu-citta");
        assert_eq!(ok(&e, "aiw_agent_save", make("???", json!([]), json!({}))).await["id"], "agente");

        // An edit changes what it is given and keeps the rest, including what the tool cannot set.
        let mut mine = e.agent("revisore-di-codice").unwrap();
        mine.max_turns = 12;
        mine.budget = Some(aiw_core::api::BudgetLimits { max_tokens: Some(5000), ..Default::default() });
        e.save_agent(mine).unwrap();
        let edited =
            ok(&e, "aiw_agent_save", json!({"id": "revisore-di-codice", "system_prompt": "Nuove istruzioni.", "mode": "plan"})).await;
        assert_eq!(
            (edited["created"].clone(), edited["id"].clone(), edited["mode"].clone()),
            (json!(false), json!("revisore-di-codice"), json!("plan"))
        );
        let edited = e.agent("revisore-di-codice").unwrap();
        assert_eq!(edited.system_prompt, "Nuove istruzioni.");
        assert_eq!((edited.name.as_str(), edited.description.as_str(), edited.max_turns), ("Revisore di codice", "Rilegge il codice", 12));
        assert_eq!(edited.tools, ["read_file", "grep", "todo"]);
        assert_eq!(edited.budget.unwrap().max_tokens, Some(5000));
        assert_eq!((edited.sandbox.as_str(), edited.mode.as_str()), ("workspace-write", "plan"));
    }

    #[tokio::test]
    async fn an_agent_made_from_the_chat_is_refused_whenever_it_would_be_freer_than_the_envelope() {
        let (e, _d, _w) = engine(vec![]).await;
        // The tools the contract keeps out really are the ones that need an explicit grant.
        for explicit in ["channel_send", "training_harness"] {
            assert!(
                e.tool_registry().get(explicit).is_some_and(|t| t.explicit_grant()),
                "{explicit} is no longer an explicit-grant tool: pick another"
            );
        }
        let agents_before = e.agents().unwrap();
        let cases: Vec<(Value, &str)> = vec![
            (make("x", json!(["todo"]), json!({"mode": "yolo"})), "`mode` può essere solo plan o ask"),
            (make("x", json!(["todo"]), json!({"mode": "auto"})), "`mode` può essere solo plan o ask"),
            (make("x", json!(["todo"]), json!({"mode": "auto-edit"})), "`mode` può essere solo plan o ask"),
            (make("x", json!(["todo"]), json!({"sandbox": "off"})), "`sandbox` può essere solo"),
            (make("x", json!(["*"]), json!({})), "uno per uno"),
            (make("x", json!(["todo", "aiw_inspect"]), json!({})), "strumento di gestione"),
            (make("x", json!(["aiw_setting_set"]), json!({})), "strumento di gestione"),
            (make("x", json!(["aiw_secret_request"]), json!({})), "strumento di gestione"),
            (make("x", json!(["ask_user"]), json!({})), "strumento di gestione"),
            (make("x", json!(["channel_send"]), json!({})), "concessione esplicita"),
            (make("x", json!(["training_harness"]), json!({})), "concessione esplicita"),
            (make("x", json!(["handoff"]), json!({})), "sposta la conversazione"),
            (make("x", json!(["todo"]), json!({"delegate_tools": ["*"]})), "`delegate_tools` non si imposta da qui"),
            (make("x", json!(["strumento_inventato"]), json!({})), "strumenti sconosciuti"),
            (make("x", json!(["todo"]), json!({"model": "non-esiste"})), "aiw_model_use"),
            (make(&"n".repeat(81), json!(["todo"]), json!({})), "troppo lungo"),
            (json!({"name": "x", "system_prompt": "p"}), "manca `tools`"),
            (json!({"name": "x", "tools": []}), "manca `system_prompt`"),
            (json!({"system_prompt": "p", "tools": []}), "manca `name`"),
            (json!({"id": "inventato", "name": "x"}), "ometti `id`"),
        ];
        for (args, expected) in cases {
            let message = refused(&e, "aiw_agent_save", args.clone()).await;
            assert!(message.contains(expected), "{args}: {message}");
        }
        // The Assistant and the other agents the app ships are not created over and not edited, even with harmless fields.
        let assistant_before = e.agent("assistant").unwrap();
        let operator_before = e.agent("operator").unwrap();
        let err = run(&e, "aiw_agent_save", make("Assistente", json!(["todo"]), json!({"id": "assistant"}))).await.unwrap_err();
        assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("L'Assistente non si crea né si modifica"), "{err}");
        for id in ["operator", "coder", "planner", "teacher", "browser", "desktop"] {
            let message = refused(&e, "aiw_agent_save", make("Copia", json!(["todo"]), json!({"id": id, "mode": "plan"}))).await;
            assert!(message.contains("integrato") && message.contains("una copia"), "{id}: {message}");
        }
        assert_eq!(e.agent("assistant").unwrap(), assistant_before);
        assert_eq!(e.agent("operator").unwrap(), operator_before);
        assert_eq!(e.agents().unwrap(), agents_before, "a refusal stored nothing");
        assert!(e.audit().iter().all(|a| a.action != "agent.save"));
    }

    #[tokio::test]
    async fn an_agent_that_already_has_more_than_the_envelope_is_not_edited_from_the_chat() {
        let (e, _d, _w) = engine(vec![]).await;
        let planner = e.agent("planner").unwrap();
        let custom = |id: &str, f: &dyn Fn(&mut AgentProfile)| {
            let mut a = AgentProfile { id: id.into(), name: id.into(), ..planner.clone() };
            f(&mut a);
            e.save_agent(a).unwrap();
        };
        custom("libero", &|a| a.mode = "auto-edit".into());
        custom("senza-sandbox", &|a| a.sandbox = "off".into());
        custom("jolly", &|a| a.tools = vec!["*".into()]);
        custom("deleghe", &|a| a.delegate_tools = vec!["*".into()]);
        custom("con-gestione", &|a| a.tools = vec!["aiw_inspect".into()]);
        for id in ["libero", "senza-sandbox", "jolly", "deleghe", "con-gestione"] {
            let before = e.agent(id).unwrap();
            let message = refused(&e, "aiw_agent_save", json!({"id": id, "name": "Nuovo nome"})).await;
            assert!(message.contains("vista Sviluppatore") && message.contains("già permessi"), "{id}: {message}");
            assert_eq!(e.agent(id).unwrap(), before);
        }
        // One made inside the envelope cannot be widened by an edit either.
        let made = ok(&e, "aiw_agent_save", make("Ristretto", json!(["todo"]), json!({"mode": "plan", "sandbox": "read-only"}))).await;
        let id = made["id"].as_str().unwrap();
        for widen in [
            json!({"mode": "yolo"}),
            json!({"sandbox": "off"}),
            json!({"tools": ["*"]}),
            json!({"tools": ["aiw_inspect"]}),
            json!({"delegate_tools": ["todo"]}),
        ] {
            let mut args = json!({"id": id});
            for (k, v) in widen.as_object().unwrap() {
                args[k] = v.clone();
            }
            refused(&e, "aiw_agent_save", args).await;
        }
        let kept = e.agent(id).unwrap();
        assert_eq!(
            (kept.mode.as_str(), kept.sandbox.as_str(), kept.tools.as_slice()),
            ("plan", "read-only", ["todo".to_string()].as_slice())
        );
        // Within the envelope an edit may change the permissions.
        ok(&e, "aiw_agent_save", json!({"id": id, "mode": "ask", "sandbox": "workspace-write"})).await;
        assert_eq!(e.agent(id).unwrap().mode, "ask");
    }

    #[tokio::test]
    async fn only_an_agent_made_by_the_person_is_deleted_and_not_one_that_is_in_use() {
        let (e, _d, _w) = engine(vec![]).await;
        let made = ok(&e, "aiw_agent_save", make("Da eliminare", json!(["todo"]), json!({}))).await;
        let id = made["id"].as_str().unwrap().to_string();
        let tool = e.tool_registry().get("aiw_agent_delete").unwrap();
        assert_eq!(tool.subject(&json!({"id": id})).as_deref(), Some("«Da eliminare» (da-eliminare)"));
        // The agents the app ships stay, whoever asks.
        for builtin in ["assistant", "operator", "coder", "planner", "teacher", "browser", "desktop"] {
            let err = run(&e, "aiw_agent_delete", json!({"id": builtin})).await.unwrap_err();
            assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("integrato"), "{builtin}: {err}");
            assert!(e.agent(builtin).is_ok());
        }
        let err = run(&e, "aiw_agent_delete", json!({"id": "non-c-e"})).await.unwrap_err();
        assert!(matches!(err, Error::NotFound(_)) && err.to_string().contains("aiw_inspect"), "{err}");
        // A team that uses it, and the settings that name it, keep it from being deleted (and nothing is lost).
        e.save_team(aiw_core::api::TeamDef {
            id: "t1".into(),
            name: "Squadra".into(),
            description: String::new(),
            strategy: "pipeline".into(),
            supervisor: None,
            members: vec![aiw_core::api::TeamMember { agent: id.clone(), role: String::new() }],
            aggregator: None,
            budget: None,
            output_schema: None,
        })
        .unwrap();
        let message = refused(&e, "aiw_agent_delete", json!({"id": id})).await;
        assert!(message.contains("team") && message.contains("Squadra"), "{message}");
        e.delete_team("t1").unwrap();
        ok(&e, "aiw_setting_set", json!({"key": "agent.default", "value": id})).await;
        let message = refused(&e, "aiw_agent_delete", json!({"id": id})).await;
        assert!(message.contains("agent.default") && message.contains("aiw_setting_set"), "{message}");
        assert!(e.agent(&id).is_ok());
        ok(&e, "aiw_setting_set", json!({"key": "agent.default", "value": "operator"})).await;
        // Now it goes, and the answer says it does not come back.
        let answer = ok(&e, "aiw_agent_delete", json!({"id": id})).await;
        assert_eq!((answer["deleted"].clone(), answer["name"].clone()), (json!(id), json!("Da eliminare")));
        assert!(answer["note"].as_str().unwrap().contains("non si annulla"), "{answer}");
        assert!(e.agent(&id).is_err());
        assert!(e.audit().iter().any(|a| a.action == "agent.delete" && a.detail.contains(&id)));
    }

    // ------------------------------------------------------------------ memories

    fn remember(e: &Engine, text: &str) -> aiw_core::api::Memory {
        e.memory_create(aiw_core::api::MemoryCreate { text: text.into(), ..Default::default() }).unwrap()
    }

    fn propose(e: &Engine, text: &str) -> aiw_core::api::Memory {
        e.memory_store(text, "fact", "all", false, None, None, "proposed").unwrap().0
    }

    #[tokio::test]
    async fn a_memory_is_restored_approved_or_rejected_and_only_when_it_is_in_the_right_state() {
        let (e, _d, _w) = engine(vec![]).await;
        let coffee = remember(&e, "Mi piace il caffè senza zucchero");
        // Restoring a memory that was never forgotten is refused with the engine's words, and nothing changes.
        let err = run(&e, "aiw_memory_review", json!({"id": coffee.id, "action": "restore"})).await.unwrap_err();
        assert!(matches!(err, Error::Conflict(_)) && err.to_string().contains("non è dimenticato"), "{err}");
        e.memory_forget(&coffee.id).unwrap();
        assert_eq!(e.memory(&coffee.id).unwrap().status, "forgotten");
        let tool = e.tool_registry().get("aiw_memory_review").unwrap();
        assert_eq!(tool.subject(&json!({"id": coffee.id, "action": "restore"})), Some(format!("restore {}", coffee.id)));
        let answer = ok(&e, "aiw_memory_review", json!({"id": coffee.id, "action": "restore"})).await;
        assert_eq!((answer["action"].clone(), answer["memory"]["status"].clone()), (json!("restore"), json!("active")));
        assert_eq!(e.memory(&coffee.id).unwrap().status, "active");
        assert!(e.audit().iter().any(|a| a.action == "memory.restore" && a.detail.contains(&coffee.id)));

        // A proposal of the assistant is confirmed, or discarded for good.
        let yes = propose(&e, "Preferisce lavorare la sera");
        let no = propose(&e, "Ha un gatto di nome Pippo");
        assert_eq!((yes.status.as_str(), no.status.as_str()), ("proposed", "proposed"));
        let answer = ok(&e, "aiw_memory_review", json!({"id": yes.id, "action": "approve"})).await;
        assert_eq!(answer["memory"]["status"], "active");
        assert_eq!(e.memory(&yes.id).unwrap().status, "active");
        let answer = ok(&e, "aiw_memory_review", json!({"id": no.id, "action": "reject"})).await;
        assert!(answer["note"].as_str().unwrap().contains("eliminata per sempre"), "{answer}");
        assert!(matches!(e.memory(&no.id), Err(Error::NotFound(_))));
        // What is not a proposal cannot be rejected (that would delete a memory the person has), and what is
        // not forgotten cannot be restored.
        let err = run(&e, "aiw_memory_review", json!({"id": coffee.id, "action": "reject"})).await.unwrap_err();
        assert!(matches!(err, Error::Conflict(_)), "{err}");
        assert_eq!(e.memory(&coffee.id).unwrap().status, "active", "the refusal deleted nothing");
        let err = run(&e, "aiw_memory_review", json!({"id": coffee.id, "action": "approve"})).await.unwrap_err();
        assert!(matches!(err, Error::Conflict(_)), "{err}");
        assert!(refused(&e, "aiw_memory_review", json!({"id": coffee.id, "action": "forget"})).await.contains("azione sconosciuta"));
        let err = run(&e, "aiw_memory_review", json!({"id": "mem_inventato", "action": "approve"})).await.unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "{err}");
        assert!(refused(&e, "aiw_memory_review", json!({"id": coffee.id})).await.contains("manca `action`"));
        // The answer shows a short text, never a whole long memory.
        let long = remember(&e, &format!("{} fine", "parola ".repeat(120)));
        e.memory_forget(&long.id).unwrap();
        let answer = ok(&e, "aiw_memory_review", json!({"id": long.id, "action": "restore"})).await;
        assert!(answer["memory"]["text"].as_str().unwrap().chars().count() <= 161, "{answer}");
    }

    #[tokio::test]
    async fn a_memory_or_every_memory_is_deleted_for_good_and_the_call_must_say_which() {
        let (e, _d, _w) = engine(vec![]).await;
        let one = remember(&e, "Il compleanno è a maggio");
        let two = remember(&e, "Usa il tema scuro");
        remember(&e, "Lavora a Catania");
        let tool = e.tool_registry().get("aiw_memory_delete").unwrap();
        assert!(tool.subject(&json!({"id": one.id})).unwrap().contains("compleanno"));
        assert_eq!(tool.subject(&json!({"all": true})).as_deref(), Some("tutti i ricordi"));
        // Nothing, or both, is not a request.
        assert!(refused(&e, "aiw_memory_delete", json!({})).await.contains("manca `id`"));
        assert!(refused(&e, "aiw_memory_delete", json!({"id": one.id, "all": true})).await.contains("non tutti e due"));
        assert!(refused(&e, "aiw_memory_delete", json!({"all": false})).await.contains("manca `id`"));
        assert_eq!(e.memories(Some("all"), None).unwrap().len(), 3);
        // One.
        let answer = ok(&e, "aiw_memory_delete", json!({"id": one.id})).await;
        assert_eq!(answer["deleted"]["id"], json!(one.id));
        assert!(answer["note"].as_str().unwrap().contains("non si annulla"));
        assert!(matches!(e.memory(&one.id), Err(Error::NotFound(_))));
        let err = run(&e, "aiw_memory_delete", json!({"id": one.id})).await.unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "{err}");
        assert!(e.memory(&two.id).is_ok(), "the others are still there");
        assert!(e.audit().iter().any(|a| a.action == "memory.delete" && a.detail.contains(&one.id)));
        // All of them, with the count.
        let answer = ok(&e, "aiw_memory_delete", json!({"all": true})).await;
        assert_eq!(answer["deleted"], 2);
        assert!(e.memories(Some("all"), None).unwrap().is_empty());
        assert!(e.audit().iter().any(|a| a.action == "memory.delete_all" && a.detail.contains("2 ricordi")));
    }
}
