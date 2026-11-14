//! `aiw_inspect`: the Assistant reads the state of the app before it changes anything: models,
//! downloads, engines, agents, connectors, settings, memories, which secrets are set, recent activity.
//!
//! The answer is a short JSON text (at most [`MAX_ITEMS`] entries per list, [`MAX_CHARS`] characters
//! in all) and it never holds a secret value, by construction and by a last check:
//!
//! * the fields that can carry one are not copied at all: the values of the environment and the
//!   headers of a connector (only their names, and whether they have a value), the endpoint and
//!   command of a connector with credentials and query stripped, the secret settings of the app and
//!   of the engines (only "set" or not), provider tokens (only "configured" or not);
//! * free text that comes from elsewhere (memories, activity, errors, arguments) goes through the
//!   redactor of the diagnostics;
//! * the finished answer is cleaned of the exact values of every secret the app stores.

use super::require_origin;
use crate::{Engine, Inner};
use aiw_core::api::{AgentProfile, McpServerInfo, RuntimeInfo, SECRET_MASK};
use aiw_core::{Error, Result};
use aiw_tools::{Risk, Tool, ToolContext, ToolOutput};
use serde_json::{json, Map, Value};
use std::sync::{Arc, OnceLock, Weak};

/// What the tool can look at.
pub(crate) const TOPICS: [&str; 10] =
    ["overview", "models", "downloads", "runtimes", "agents", "connectors", "settings", "memory", "secrets", "activity"];

/// Most entries of a list (the answer says how many there are and how to narrow it down).
const MAX_ITEMS: usize = 30;
/// Longest answer, in characters: the limit the agent loop puts on a tool result.
const MAX_CHARS: usize = 24_000;
/// A stored value shorter than this is not hunted for in the answer: it would also match ordinary words.
const MIN_SECRET_LEN: usize = 6;

pub(crate) fn tools(e: &Engine) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Inspect(Arc::downgrade(&e.0)))]
}

struct Inspect(Weak<Inner>);

#[async_trait::async_trait]
impl Tool for Inspect {
    fn name(&self) -> &str {
        "aiw_inspect"
    }

    fn description(&self) -> &str {
        "Legge lo stato di AI Workspace prima di cambiare qualcosa: modelli installati e consigliati, download, motori di esecuzione, assistenti, connettori, impostazioni, ricordi, quali segreti sono impostati, attività recente. Risposta breve (al massimo 30 voci per elenco), mai con valori segreti. Non indovinare nomi di modelli, chiavi di impostazioni o identificativi: guardali qui."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["topic"],
            "properties": {
                "topic": {"type": "string", "enum": TOPICS, "description": "Che cosa guardare: overview (riepilogo), models (modelli e consigli), downloads, runtimes (motori), agents (assistenti, profili, team), connectors (server MCP), settings, memory (ricordi), secrets (quali segreti sono impostati), activity (registro recente)."},
                "id": {"type": "string", "description": "Identificativo di una voce, per il dettaglio (modello, motore, assistente, connettore). Per un motore include il piano di installazione."},
                "query": {"type": "string", "description": "Filtra le voci per testo (nome, chiave, contenuto)."},
                "refresh": {"type": "boolean", "description": "Solo per runtimes: rileva di nuovo i motori installati prima di rispondere."}
            }
        })
    }

    fn risk(&self) -> Risk {
        Risk::Read
    }

    fn explicit_grant(&self) -> bool {
        true
    }

    /// The policy sees the topic, so a rule such as `aiw_inspect(secrets)` can close one.
    fn subject(&self, args: &Value) -> Option<String> {
        args.get("topic").and_then(Value::as_str).map(String::from)
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let engine = Engine(self.0.upgrade().ok_or_else(|| Error::Conflict("motore in arresto".into()))?);
        require_origin(&engine, ctx)?;
        Ok(ToolOutput::text(render(&engine, &args).await?))
    }
}

struct Args {
    topic: &'static str,
    id: Option<String>,
    query: Option<String>,
    refresh: bool,
}

fn parse(args: &Value) -> Result<Args> {
    let list = TOPICS.join(", ");
    let asked = args.get("topic").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty());
    let Some(asked) = asked else { return Err(Error::Invalid(format!("manca `topic`: usa uno tra {list}"))) };
    let Some(topic) = TOPICS.iter().find(|t| **t == asked) else {
        return Err(Error::Invalid(format!("topic sconosciuto «{asked}»: usa uno tra {list}")));
    };
    let text = |k: &str| args.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    Ok(Args { topic, id: text("id"), query: text("query"), refresh: args.get("refresh").and_then(Value::as_bool).unwrap_or(false) })
}

/// The answer of the tool for these arguments (the origin of the run is checked by the caller).
pub(crate) async fn render(e: &Engine, args: &Value) -> Result<String> {
    let a = parse(args)?;
    let value = match a.topic {
        "overview" => overview(e),
        "models" => models(e, &a)?,
        "downloads" => downloads(e, &a),
        "runtimes" => runtimes(e, &a).await?,
        "agents" => agents(e, &a)?,
        "connectors" => connectors(e, &a)?,
        "settings" => settings(e, &a),
        "memory" => memory(e, &a)?,
        "secrets" => secrets(e),
        _ => activity(e, &a),
    };
    Ok(finish(e, value))
}

// ------------------------------------------------------------------ the last check

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
    format!("{cut}… [risposta troncata: restringi con `query` o `id`]")
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

// ------------------------------------------------------------------ helpers

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

/// Free text from elsewhere (an error, a memory, an argument): `token=…`, `password: …`,
/// `Bearer …` and credentials in URLs are hidden by the redactor of the diagnostics.
fn redact(s: &str) -> String {
    static PATTERNS: OnceLock<Vec<(regex::Regex, &'static str)>> = OnceLock::new();
    crate::diagnostics::scrub_line(PATTERNS.get_or_init(crate::diagnostics::line_patterns), s)
}

fn text(s: &str, max: usize) -> String {
    redact(&clip(s, max))
}

/// An address without what could be a secret: no user or password, no query, no fragment, and a
/// path segment that looks like a token (long) cut.
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

/// The arguments of a command line, without what follows a flag that names a secret (`--token abc`)
/// and with `--key=abc` hidden by the redactor.
fn args_safe(args: &[String]) -> Vec<String> {
    let mut hide_next = false;
    args.iter()
        .map(|a| {
            let masked = std::mem::replace(&mut hide_next, false);
            if masked {
                return SECRET_MASK.to_string();
            }
            if a.starts_with('-') && !a.contains('=') && crate::secrets::is_secret_name(a) {
                hide_next = true;
            }
            text(a, 200)
        })
        .collect()
}

fn hit(query: &Option<String>, fields: &[&str]) -> bool {
    query.as_ref().is_none_or(|q| {
        let q = q.to_lowercase();
        fields.iter().any(|f| f.to_lowercase().contains(&q))
    })
}

/// The list with at most [`MAX_ITEMS`] entries, how many there were, and a hint when it was cut.
fn listing(mut items: Vec<Value>) -> Value {
    let total = items.len();
    items.truncate(MAX_ITEMS);
    let mut v = json!({"total": total, "shown": items.len(), "items": items});
    if total > MAX_ITEMS {
        v["note"] = json!(format!("Mostrate {MAX_ITEMS} voci su {total}: restringi con `query`."));
    }
    v
}

fn mb(bytes: u64) -> u64 {
    bytes / (1024 * 1024)
}

fn not_found(what: &str, id: &str, topic: &str) -> Error {
    Error::NotFound(format!("{what} `{id}` non trovato: guarda l'elenco con aiw_inspect (topic {topic})"))
}

// ------------------------------------------------------------------ overview

fn overview(e: &Engine) -> Value {
    let sys = e.system(false);
    let d = &sys.device;
    let downloads = e.downloads();
    let active = downloads.iter().filter(|x| matches!(x.status.as_str(), "queued" | "running" | "verifying")).count();
    let running: Vec<String> = e.runtimes().into_iter().filter(|r| r.status == "running").map(|r| r.id).collect();
    let servers = e.mcp_servers();
    let models = e.models();
    json!({
        "version": sys.version,
        "mode": sys.mode,
        "workspace_root": sys.workspace_root,
        "sandboxed_app": sys.sandboxed,
        "device": {
            "os": d.os, "arch": d.arch, "tier": d.tier, "memory_mb": d.memory_mb,
            "accelerators": d.accelerators.iter().map(|a| a.kind.clone()).collect::<Vec<_>>(),
        },
        "installed_models": models.len(),
        "active_downloads": active,
        "running_runtimes": running,
        "agents": e.agents().map(|a| a.len()).unwrap_or(0),
        "default_agent": e.0.config.get_str("agent.default"),
        "connectors": {"total": servers.len(), "connected": servers.iter().filter(|s| s.status == "connected").count()},
        "memories": e.memories(None, None).map(|m| m.len()).unwrap_or(0),
        "note": if models.is_empty() { "Nessun modello installato: senza un modello nessun assistente può rispondere." } else { "" },
    })
}

// ------------------------------------------------------------------ models and downloads

fn fit_rank(fit: &str) -> u8 {
    match fit {
        "great" => 0,
        "ok" => 1,
        "tight" => 2,
        _ => 3,
    }
}

fn choice(c: &Option<aiw_core::api::ModelChoice>) -> Value {
    match c {
        None => Value::Null,
        Some(c) => json!({
            "model": c.model, "name": c.name, "runtime": c.runtime_id, "fit": c.fit,
            "size_mb": mb(c.size_bytes), "catalog_id": c.catalog_id,
        }),
    }
}

fn models(e: &Engine, a: &Args) -> Result<Value> {
    if let Some(id) = &a.id {
        if let Some(m) = e.models().into_iter().find(|m| &m.id == id || &m.name == id) {
            return Ok(json!({
                "installed": true, "id": m.id, "name": m.name, "format": m.format, "quant": m.quant, "params_b": m.params_b,
                "context_length": m.context_length, "size_mb": mb(m.size_bytes), "capabilities": m.capabilities,
                "runtimes": m.runtimes, "origin": m.origin, "removable": m.removable, "source": m.source,
                "fit": m.fit, "fit_detail": text(&m.fit_detail, 200),
            }));
        }
        let c = e.catalog().into_iter().find(|c| &c.id == id).ok_or_else(|| not_found("modello", id, "models"))?;
        return Ok(json!({
            "installed": c.installed, "id": c.id, "name": c.name, "publisher": c.publisher, "family": c.family,
            "description": text(&c.description, 300), "params_b": c.params_b, "format": c.format, "quant": c.quant,
            "size_mb": mb(c.size_bytes), "context_length": c.context_length, "capabilities": c.capabilities, "license": c.license,
            "min_memory_mb": c.min_memory_mb, "fit": c.fit, "fit_detail": text(&c.fit_detail, 200), "downloadable": c.downloadable,
        }));
    }
    let installed = e
        .models()
        .into_iter()
        .filter(|m| hit(&a.query, &[&m.id, &m.name]))
        .map(|m| {
            json!({
                "id": m.id, "name": m.name, "size_mb": mb(m.size_bytes), "format": m.format, "quant": m.quant,
                "capabilities": m.capabilities, "runtimes": m.runtimes, "origin": m.origin, "removable": m.removable, "fit": m.fit,
            })
        })
        .collect();
    let mut catalog: Vec<_> = e.catalog().into_iter().filter(|c| hit(&a.query, &[&c.id, &c.name, &c.family, &c.tags.join(" ")])).collect();
    catalog.sort_by_key(|c| (fit_rank(&c.fit), c.installed, c.size_bytes));
    let catalog = catalog
        .into_iter()
        .map(|c| {
            json!({
                "id": c.id, "name": c.name, "size_mb": mb(c.size_bytes), "fit": c.fit, "installed": c.installed,
                "downloadable": c.downloadable, "capabilities": c.capabilities,
            })
        })
        .collect();
    let rec = e.recommend();
    let advice = |t: &aiw_core::api::TaskAdvice| json!({"installed": choice(&t.installed), "download": choice(&t.download)});
    Ok(json!({
        "device_tier": rec.tier,
        "installed": listing(installed),
        "recommended": {"chat": advice(&rec.chat), "agent": advice(&rec.agent), "reasoning": advice(&rec.reasoning), "vision": advice(&rec.vision)},
        "catalog": listing(catalog),
    }))
}

fn downloads(e: &Engine, a: &Args) -> Value {
    let items = e
        .downloads()
        .into_iter()
        .filter(|d| hit(&a.query, &[&d.model_id, &d.status]))
        .map(|d| {
            json!({
                "id": d.id, "model": d.model_id, "status": d.status,
                "percent": d.received.saturating_mul(100).checked_div(d.total),
                "received_mb": mb(d.received), "total_mb": mb(d.total), "speed_mb_s": d.speed_bps / (1024 * 1024),
                "files": format!("{}/{}", d.files_done, d.files_total), "error": d.error.as_deref().map(|x| text(x, 200)),
            })
        })
        .collect();
    json!({"downloads": listing(items)})
}

// ------------------------------------------------------------------ runtimes

fn status_rank(status: &str) -> u8 {
    match status {
        "running" => 0,
        "starting" => 1,
        "available" => 2,
        "stopped" => 3,
        "installable" => 4,
        "error" => 5,
        _ => 6,
    }
}

/// The settings of an engine: the ordinary ones with their value, the secret ones only as set or not.
fn runtime_settings(r: &RuntimeInfo) -> Vec<Value> {
    r.settings_schema
        .properties
        .iter()
        .map(|(k, f)| {
            let v = r.settings.get(k);
            if f.secret {
                json!({"key": k, "secret": true, "set": v.and_then(Value::as_str).is_some_and(|s| !s.is_empty())})
            } else {
                json!({"key": k, "value": v.map(|v| plain(k, v))})
            }
        })
        .collect()
}

/// A value of a setting as the model reads it: scalars as they are, text redacted and cut, lists of
/// text cut, anything with a structure (accounts, objects) only as a count.
fn plain(key: &str, v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(text(s, 200)),
        Value::Array(a) if key != "connectors.mail.imap" && a.iter().all(Value::is_string) => {
            Value::Array(a.iter().take(10).map(|x| Value::String(text(x.as_str().unwrap_or_default(), 120))).collect())
        }
        Value::Array(a) => json!({"entries": a.len()}),
        Value::Object(m) => json!({"entries": m.len()}),
        other => other.clone(),
    }
}

async fn runtimes(e: &Engine, a: &Args) -> Result<Value> {
    let list = if a.refresh { e.refresh_runtimes().await } else { e.runtimes() };
    if let Some(id) = &a.id {
        let r = list.iter().find(|r| &r.id == id).ok_or_else(|| not_found("motore", id, "runtimes"))?;
        let plan = e.runtime_install_plan(id).await.ok().and_then(|p| serde_json::to_value(p).ok());
        let job = e.runtime_install_status(id).map(|j| {
            json!({
                "state": j.state, "step": j.step, "steps": j.steps, "error": j.error.as_deref().map(|x| text(x, 200)),
                "log_tail": j.log.iter().rev().take(10).rev().map(|l| text(l, 200)).collect::<Vec<_>>(),
            })
        });
        return Ok(json!({
            "id": r.id, "name": r.name, "kind": r.kind, "status": r.status, "detail": r.status_detail.as_deref().map(|x| text(x, 200)),
            "description": text(&r.description, 300), "capabilities": r.capabilities, "formats": r.formats, "accelerators": r.accelerators,
            "endpoint": r.endpoint.as_deref().map(url_safe), "loaded_models": r.loaded_models.iter().take(10).collect::<Vec<_>>(),
            "install_auto": r.install_auto, "install_hint": r.install_hint.as_deref().map(|x| text(x, 300)),
            "settings": runtime_settings(r), "install_plan": plan, "install_job": job,
        }));
    }
    let mut list: Vec<&RuntimeInfo> = list.iter().filter(|r| hit(&a.query, &[&r.id, &r.name, &r.kind])).collect();
    list.sort_by_key(|r| status_rank(&r.status));
    let items = list
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id, "name": r.name, "kind": r.kind, "status": r.status,
                "detail": r.status_detail.as_deref().map(|x| text(x, 120)), "loaded_models": r.loaded_models.len(), "install_auto": r.install_auto,
            })
        })
        .collect();
    Ok(json!({"runtimes": listing(items)}))
}

// ------------------------------------------------------------------ agents

fn agent_detail(e: &Engine, a: &AgentProfile) -> Value {
    let profile = e.profile(&a.inference_profile).ok().map(|p| json!({"id": p.id, "runtime": p.runtime_id, "model": p.model}));
    json!({
        "id": a.id, "name": a.name, "description": text(&a.description, 300), "builtin": crate::defaults::is_builtin_agent(&a.id),
        "mode": a.mode, "sandbox": a.sandbox, "max_turns": a.max_turns, "profile": a.inference_profile, "profile_detail": profile,
        "tools": a.tools, "delegate_tools": a.delegate_tools, "candidate_profiles": a.candidate_profiles, "knowledge": a.knowledge,
        "system_prompt_chars": a.system_prompt.chars().count(), "system_prompt_start": text(&a.system_prompt, 400),
    })
}

fn agents(e: &Engine, a: &Args) -> Result<Value> {
    if let Some(id) = &a.id {
        let found = e.agents()?.into_iter().find(|x| &x.id == id).ok_or_else(|| not_found("assistente", id, "agents"))?;
        return Ok(agent_detail(e, &found));
    }
    let agents = e
        .agents()?
        .into_iter()
        .filter(|x| hit(&a.query, &[&x.id, &x.name, &x.description]))
        .map(|x| {
            json!({
                "id": x.id, "name": x.name, "description": text(&x.description, 120), "profile": x.inference_profile, "mode": x.mode,
                "sandbox": x.sandbox, "tools": x.tools.len(), "builtin": crate::defaults::is_builtin_agent(&x.id),
            })
        })
        .collect();
    let profiles = e
        .profiles()?
        .into_iter()
        .filter(|p| hit(&a.query, &[&p.id, &p.name, &p.model]))
        .map(|p| json!({"id": p.id, "name": p.name, "runtime": p.runtime_id, "model": p.model, "context": p.params.context}))
        .collect();
    let teams = e
        .teams()?
        .into_iter()
        .filter(|t| hit(&a.query, &[&t.id, &t.name]))
        .map(|t| json!({"id": t.id, "name": t.name, "strategy": t.strategy, "members": t.members.len()}))
        .collect();
    Ok(json!({"agents": listing(agents), "profiles": listing(profiles), "teams": listing(teams)}))
}

// ------------------------------------------------------------------ connectors

/// The names of the variables or headers of a connector, never their values: `secret_name` says
/// that the name looks like a secret (`Authorization`, `GITHUB_TOKEN`), `has_value` that something is stored.
fn entry_names(map: &Option<Map<String, Value>>) -> Vec<Value> {
    map.iter()
        .flatten()
        .take(MAX_ITEMS)
        .map(|(k, v)| {
            let has_value = !v.is_null() && v.as_str().is_none_or(|s| !s.is_empty());
            json!({"name": k, "secret_name": crate::secrets::is_secret_name(k), "has_value": has_value})
        })
        .collect()
}

fn connector(s: &McpServerInfo, detail: bool) -> Value {
    let c = &s.config;
    let shown = if detail { MAX_ITEMS } else { 10 };
    json!({
        "id": c.id, "name": c.name, "transport": c.transport, "enabled": c.enabled, "status": s.status,
        "error": s.error.as_deref().map(|x| text(x, 200)), "approval_required": s.approval_required, "signed_in": s.signed_in,
        "tools_total": s.tools.len(), "tools": s.tools.iter().take(shown).collect::<Vec<_>>(),
        "url": c.url.as_deref().map(url_safe), "command": c.command.as_deref().map(|x| text(x, 200)),
        "args": c.args.as_deref().map(args_safe), "env": entry_names(&c.env), "headers": entry_names(&c.headers),
        "sandbox": c.sandbox, "sandbox_network": c.sandbox_network,
    })
}

fn connectors(e: &Engine, a: &Args) -> Result<Value> {
    let list = e.mcp_servers();
    if let Some(id) = &a.id {
        let s =
            list.iter().find(|s| &s.config.id == id || &s.config.name == id).ok_or_else(|| not_found("connettore", id, "connectors"))?;
        return Ok(connector(s, true));
    }
    let items = list.iter().filter(|s| hit(&a.query, &[&s.config.id, &s.config.name])).map(|s| connector(s, false)).collect();
    Ok(json!({"connectors": listing(items)}))
}

// ------------------------------------------------------------------ settings

fn settings(e: &Engine, a: &Args) -> Value {
    let view = e.config();
    let items = view
        .schema
        .properties
        .iter()
        .filter(|(k, f)| hit(&a.query, &[k, f.title.as_deref().unwrap_or_default(), f.description.as_deref().unwrap_or_default()]))
        .map(|(k, f)| {
            let value = view.effective.get(k).or(f.default.as_ref());
            let mut item = json!({"key": k, "type": f.kind, "title": f.title.as_deref().map(|t| clip(t, 80))});
            if f.secret {
                item["secret"] = json!(true);
                item["set"] = json!(value.and_then(Value::as_str).is_some_and(|s| !s.is_empty()));
            } else {
                item["value"] = value.map(|v| plain(k, v)).unwrap_or(Value::Null);
            }
            item
        })
        .collect();
    json!({"settings": listing(items)})
}

// ------------------------------------------------------------------ memory

fn memory(e: &Engine, a: &Args) -> Result<Value> {
    let all = e.memories(None, None)?;
    let count = |status: &str| all.iter().filter(|m| m.status == status).count();
    let counts = json!({"active": count("active"), "proposed": count("proposed")});
    let items = all
        .into_iter()
        .filter(|m| hit(&a.query, &[&m.text, &m.kind]))
        .map(|m| {
            json!({
                "id": m.id, "kind": m.kind, "status": m.status, "pinned": m.pinned, "scope": m.scope,
                "text": text(&m.text, 240), "due_at": m.due_at,
            })
        })
        .collect();
    Ok(json!({"counts": counts, "memories": listing(items)}))
}

// ------------------------------------------------------------------ secrets

/// Which secrets are set, by the `target` that `aiw_secret_request` takes: never a value.
fn secrets(e: &Engine) -> Value {
    let mut targets: Vec<Value> = Vec::new();
    let mut add = |target: String, set: bool| targets.push(json!({"target": target, "set": set}));
    for p in e.model_providers().into_iter().filter(|p| p.auth != "none") {
        add(format!("provider_token:{}", p.id), p.auth_configured);
    }
    let view = e.config();
    for (k, f) in view.schema.properties.iter().filter(|(_, f)| f.secret) {
        let set = view.effective.get(k).or(f.default.as_ref()).and_then(Value::as_str).is_some_and(|s| !s.is_empty());
        add(format!("setting:{k}"), set);
    }
    for s in e.mcp_servers() {
        for (kind, map) in [("mcp_env", &s.config.env), ("mcp_header", &s.config.headers)] {
            for (k, v) in map.iter().flatten().filter(|(k, _)| crate::secrets::is_secret_name(k)) {
                add(format!("{kind}:{}:{k}", s.config.id), v.as_str().is_some_and(|s| !s.is_empty()));
            }
        }
    }
    for r in e.runtimes() {
        for (k, _) in r.settings_schema.properties.iter().filter(|(_, f)| f.secret) {
            add(format!("runtime_setting:{}:{k}", r.id), r.settings.get(k).and_then(Value::as_str).is_some_and(|s| !s.is_empty()));
        }
    }
    json!({
        "note": "Elenco dei segreti che l'app sa tenere e se sono impostati. I valori non si vedono mai: per impostarne uno usa aiw_secret_request con il `target`.",
        "secrets": listing(targets),
    })
}

// ------------------------------------------------------------------ activity

fn activity(e: &Engine, a: &Args) -> Value {
    let items = e
        .audit()
        .into_iter()
        .filter(|x| hit(&a.query, &[&x.action, x.tool.as_deref().unwrap_or_default(), &x.detail]))
        .map(|x| {
            json!({
                "ts": x.ts, "actor": x.actor, "action": x.action, "tool": x.tool, "decision": x.decision, "detail": text(&x.detail, 160),
            })
        })
        .collect();
    json!({"activity": listing(items)})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::management::test_ctx;
    use crate::tests::engine;
    use crate::RunOrigin;
    use aiw_core::api::{McpServerConfig, MemoryCreate};

    const PROVIDER_TOKEN: &str = "FAKE_PROVIDER_TOKEN_hf_9f8e7d6c5b4a";
    const HEADER_SECRET: &str = "FAKE_MCP_HEADER_SECRET_1a2b3c";
    const CUSTOM_HEADER: &str = "FAKE_CUSTOM_HEADER_VALUE_4d5e6f";
    const ENV_SECRET: &str = "FAKE_MCP_ENV_SECRET_7a8b9c";
    const ENV_PLAIN: &str = "FAKE_ENV_PLAIN_VALUE_0d1e2f";
    const URL_PASSWORD: &str = "FAKE_URL_PASSWORD_3a4b5c";
    const URL_QUERY: &str = "FAKE_URL_QUERY_TOKEN_6d7e8f";
    const URL_PATH: &str = "FAKE_URL_PATH_SEGMENT_9a0b1c2d3e4f";
    const ARG_SECRET: &str = "FAKE_ARG_KEY_5a6b7c";
    const ARG_NEXT: &str = "FAKE_ARG_FOLLOWING_VALUE_8d9e0f";
    const BRAVE_KEY: &str = "FAKE_BRAVE_KEY_1b2c3d";
    const RUNTIME_SECRET: &str = "FAKE_RUNTIME_SECRET_4e5f6a";
    const MEMORY_SECRET: &str = "FAKE_MEMORY_SECRET_7b8c9d";
    const AUDIT_SECRET: &str = "FAKE_AUDIT_SECRET_0a1b2c";
    const FAKES: [&str; 14] = [
        PROVIDER_TOKEN,
        HEADER_SECRET,
        CUSTOM_HEADER,
        ENV_SECRET,
        ENV_PLAIN,
        URL_PASSWORD,
        URL_QUERY,
        URL_PATH,
        ARG_SECRET,
        ARG_NEXT,
        BRAVE_KEY,
        RUNTIME_SECRET,
        MEMORY_SECRET,
        AUDIT_SECRET,
    ];

    fn http_server() -> McpServerConfig {
        McpServerConfig {
            id: String::new(),
            name: "Server di prova".into(),
            transport: "http".into(),
            command: None,
            args: None,
            url: Some(format!("http://utente:{URL_PASSWORD}@127.0.0.1:9/mcp/{URL_PATH}?token={URL_QUERY}")),
            env: None,
            headers: serde_json::from_value(json!({"Authorization": format!("Bearer {HEADER_SECRET}"), "X-Custom": CUSTOM_HEADER})).ok(),
            enabled: false,
            sampling: false,
            oauth_client_id: None,
            sandbox: "off".into(),
            sandbox_network: true,
        }
    }

    fn stdio_server() -> McpServerConfig {
        McpServerConfig {
            name: "Locale di prova".into(),
            transport: "stdio".into(),
            command: Some("npx".into()),
            args: Some(vec![format!("--api-key={ARG_SECRET}"), "--token".into(), ARG_NEXT.into(), "server.js".into()]),
            url: None,
            env: serde_json::from_value(json!({"GITHUB_TOKEN": ENV_SECRET, "MODE": ENV_PLAIN})).ok(),
            headers: None,
            ..http_server()
        }
    }

    /// An engine that holds a fake secret of every kind the app stores, in every place the tool reads from.
    async fn with_secrets() -> (Engine, Vec<String>, String, tempfile::TempDir, tempfile::TempDir) {
        let (e, d, w) = engine(vec![]).await;
        e.0.hub.set_auth("hf", Some(Some(PROVIDER_TOKEN.into())), None).unwrap();
        e.0.store
            .put_doc(
                "model_provider",
                "hf",
                &crate::models::ProviderDoc { id: "hf".into(), token: Some(PROVIDER_TOKEN.into()), endpoint: None },
            )
            .unwrap();
        let http = e.save_mcp_server(http_server()).await.unwrap().config.id;
        let stdio = e.save_mcp_server(stdio_server()).await.unwrap().config.id;
        e.set_config("global", "search.brave_api_key", json!(BRAVE_KEY)).unwrap();
        let (runtime, key) = e
            .runtimes()
            .into_iter()
            .find_map(|r| r.settings_schema.properties.iter().find(|(_, f)| f.secret).map(|(k, _)| (r.id.clone(), k.clone())))
            .expect("a runtime with a secret setting");
        e.update_runtime_settings(&runtime, serde_json::from_value(json!({ key: RUNTIME_SECRET })).unwrap()).unwrap();
        e.memory_create(MemoryCreate {
            text: format!("la mia password: {MEMORY_SECRET}"),
            kind: None,
            scope: None,
            pinned: None,
            due_at: None,
        })
        .unwrap();
        e.audit_user("test.secret", &format!("token={AUDIT_SECRET}"));
        // A stored value written in a place where no keyword gives it away: only the exact-value check can hide it.
        e.audit_user("test.known", &format!("collegato con {PROVIDER_TOKEN} e basta"));
        (e, vec![http, stdio], runtime, d, w)
    }

    async fn ask(e: &Engine, args: Value) -> String {
        render(e, &args).await.unwrap_or_else(|err| panic!("{args}: {err}"))
    }

    #[tokio::test]
    async fn every_topic_answers_and_none_of_them_ever_holds_a_secret_value() {
        let (e, connectors, runtime, _d, _w) = with_secrets().await;
        let first_catalog_model = e.catalog().first().unwrap().id.clone();
        let mut cases: Vec<Value> = TOPICS.iter().map(|t| json!({"topic": t})).collect();
        cases.extend([
            json!({"topic": "runtimes", "id": runtime}),
            json!({"topic": "agents", "id": "operator"}),
            json!({"topic": "models", "id": first_catalog_model}),
            json!({"topic": "settings", "query": "brave"}),
            json!({"topic": "settings", "query": "chiave"}),
            json!({"topic": "settings", "query": "workspace.root"}),
            json!({"topic": "memory", "query": "password"}),
            json!({"topic": "activity", "query": "secret"}),
            json!({"topic": "activity", "query": "collegato"}),
            json!({"topic": "secrets", "query": "x"}),
        ]);
        cases.extend(connectors.iter().map(|id| json!({"topic": "connectors", "id": id})));
        let mut out = std::collections::BTreeMap::new();
        for case in &cases {
            let text = ask(&e, case.clone()).await;
            for fake in FAKES {
                assert!(!text.contains(fake), "{case} leaks {fake}:\n{text}");
            }
            let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|err| panic!("{case} is not JSON ({err}): {text}"));
            assert!(parsed.is_object(), "{case}");
            out.insert(case.to_string(), text);
        }
        let of = |case: Value| out[&case.to_string()].clone();

        // What the model does need is there, in a form it can use.
        let connectors_text = of(json!({"topic": "connectors"}));
        for needed in
            ["Server_di_prova", "Locale_di_prova", "Authorization", "X-Custom", "GITHUB_TOKEN", "MODE", "npx", "127.0.0.1:9", "server.js"]
        {
            assert!(connectors_text.contains(needed), "{needed} missing from {connectors_text}");
        }
        // The address lost its credentials, query and token-like path segment; the flag that hides a value hid it.
        assert!(!connectors_text.contains("utente:") && !connectors_text.contains("?token"), "{connectors_text}");
        assert!(connectors_text.contains("--api-key=") && connectors_text.contains(SECRET_MASK), "{connectors_text}");
        let secrets_text = of(json!({"topic": "secrets"}));
        let secrets: Value = serde_json::from_str(&secrets_text).unwrap();
        let set_of = |target: &str| -> Option<bool> {
            secrets["secrets"]["items"].as_array().unwrap().iter().find(|x| x["target"] == target).map(|x| x["set"].as_bool().unwrap())
        };
        assert_eq!(set_of("provider_token:hf"), Some(true));
        assert_eq!(set_of("setting:search.brave_api_key"), Some(true));
        assert_eq!(set_of(&format!("mcp_header:{}:Authorization", connectors[0])), Some(true));
        assert_eq!(set_of(&format!("mcp_env:{}:GITHUB_TOKEN", connectors[1])), Some(true));
        assert_eq!(
            set_of(&format!(
                "runtime_setting:{runtime}:{}",
                secrets["secrets"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find_map(|x| x["target"].as_str().and_then(|t| t.strip_prefix(&format!("runtime_setting:{runtime}:"))))
                    .unwrap()
            )),
            Some(true)
        );
        // A secret that is not set says so (the Tavily key, for example, has nothing stored).
        assert!(secrets["secrets"]["items"].as_array().unwrap().iter().any(|x| x["set"] == false), "{secrets}");
        // The ordinary settings show their value, the secret one only that it is set.
        let brave: Value = serde_json::from_str(&of(json!({"topic": "settings", "query": "brave"}))).unwrap();
        let brave = &brave["settings"]["items"][0];
        assert_eq!(brave["key"], "search.brave_api_key");
        assert!(brave["secret"] == true && brave["set"] == true && brave.get("value").is_none(), "{brave}");
        let workspace = of(json!({"topic": "settings", "query": "workspace.root"}));
        assert!(workspace.contains("\"value\""), "{workspace}");
        // Memory and activity keep their text but not the secret in it; the exact-value check hid the bare token.
        assert!(of(json!({"topic": "memory", "query": "password"})).contains("la mia password"));
        let known = of(json!({"topic": "activity", "query": "collegato"}));
        assert!(known.contains("collegato con") && known.contains(SECRET_MASK), "{known}");
        // The agent detail lists the tools and the profile, the engine detail the install plan.
        let operator = of(json!({"topic": "agents", "id": "operator"}));
        assert!(operator.contains("read_file") && operator.contains("\"profile\":\"auto:agent\""), "{operator}");
        assert!(of(json!({"topic": "runtimes", "id": runtime})).contains("install_plan"));
    }

    /// An engine that fails with an error that repeats the key it was given.
    struct LeakyEngine;

    #[async_trait::async_trait]
    impl aiw_runtimes::ChatBackend for LeakyEngine {
        async fn list_models(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }
        async fn chat(
            &self,
            _req: aiw_runtimes::ChatRequest,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> Result<aiw_runtimes::DeltaStream> {
            Err(Error::Unsupported("prova".into()))
        }
        fn availability(&self) -> std::result::Result<(), String> {
            Err(format!("rifiutato dal server con la chiave {RUNTIME_SECRET} e basta"))
        }
    }

    /// The secret settings of an engine are among the values the last check hides: an error that repeats the key
    /// (`status_detail`, in the list and in the detail) does not reach the model, whatever words surround it.
    #[tokio::test]
    async fn a_runtime_secret_repeated_in_the_status_never_reaches_the_answer() {
        let (e, _d, _w) = engine(vec![]).await;
        e.update_runtime_settings("remote-openai", serde_json::from_value(json!({ "api_key": RUNTIME_SECRET })).unwrap()).unwrap();
        e.register_native_runtime("remote-openai", Arc::new(LeakyEngine));
        e.0.runtimes.refresh_one("remote-openai").await.unwrap();
        // The leak is really there to be found: the status of the engine carries the key.
        let status = e.runtimes().into_iter().find(|r| r.id == "remote-openai").unwrap();
        assert_eq!(status.status, "installable");
        assert!(status.status_detail.as_deref().unwrap_or_default().contains(RUNTIME_SECRET), "{status:?}");
        // ... and the engine does not hand it out as a setting either: the list shows the mask.
        assert_eq!(status.settings["api_key"], SECRET_MASK);

        for args in [
            json!({"topic": "runtimes"}),
            json!({"topic": "runtimes", "id": "remote-openai"}),
            json!({"topic": "runtimes", "query": "openai"}),
            json!({"topic": "overview"}),
        ] {
            let text = ask(&e, args.clone()).await;
            assert!(!text.contains(RUNTIME_SECRET), "{args} leaks the key:\n{text}");
            if args["topic"] == "runtimes" {
                assert!(text.contains("rifiutato dal server") && text.contains(SECRET_MASK), "{args}: {text}");
            }
        }
        // The key is a secret the app knows: it is among the values the diagnostics hide too.
        assert!(e.known_secrets().iter().any(|s| s == RUNTIME_SECRET));
    }

    #[tokio::test]
    async fn overview_models_downloads_and_agents_say_what_is_there() {
        let (e, _d, _w) = engine(vec![]).await;
        let overview: Value = serde_json::from_str(&ask(&e, json!({"topic": "overview"})).await).unwrap();
        assert_eq!(overview["installed_models"], 0);
        assert!(overview["note"].as_str().unwrap().contains("Nessun modello installato"), "{overview}");
        assert_eq!(overview["default_agent"], "operator");
        assert!(overview["agents"].as_u64().unwrap() >= 6);
        let models: Value = serde_json::from_str(&ask(&e, json!({"topic": "models"})).await).unwrap();
        assert!(models["catalog"]["total"].as_u64().unwrap() > 10 && models["catalog"]["shown"].as_u64().unwrap() <= 30, "{models}");
        assert_eq!(models["installed"]["total"], 0);
        let narrowed: Value = serde_json::from_str(&ask(&e, json!({"topic": "models", "query": "qwen3-0.6b"})).await).unwrap();
        assert!(narrowed["catalog"]["items"][0]["id"].as_str().unwrap().contains("qwen3-0.6b"), "{narrowed}");
        let one: Value = serde_json::from_str(&ask(&e, json!({"topic": "models", "id": "qwen3-0.6b"})).await).unwrap();
        assert_eq!(one["installed"], false);
        assert!(one["fit"].is_string());
        let downloads: Value = serde_json::from_str(&ask(&e, json!({"topic": "downloads"})).await).unwrap();
        assert_eq!(downloads["downloads"]["total"], 0);
        let agents: Value = serde_json::from_str(&ask(&e, json!({"topic": "agents"})).await).unwrap();
        let ids: Vec<&str> = agents["agents"]["items"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
        assert!(ids.contains(&"operator") && ids.contains(&"teacher"), "{ids:?}");
        assert!(agents["profiles"]["items"].as_array().unwrap().iter().any(|p| p["id"] == "default"));
        // A name that does not exist is a clear error, in Italian.
        for case in [
            json!({"topic": "models", "id": "non-esiste"}),
            json!({"topic": "agents", "id": "non-esiste"}),
            json!({"topic": "connectors", "id": "x"}),
            json!({"topic": "runtimes", "id": "x"}),
        ] {
            let err = render(&e, &case).await.unwrap_err();
            assert!(matches!(err, Error::NotFound(_)) && err.to_string().contains("aiw_inspect"), "{case}: {err}");
        }
    }

    #[tokio::test]
    async fn lists_are_cut_at_thirty_entries_and_say_so() {
        let (e, _d, _w) = engine(vec![]).await;
        let words = ["alfa", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu"];
        for i in 0..35 {
            let text = format!("{} {} {} {}", words[i % 12], words[(i * 5 + 3) % 12], words[(i * 7 + 1) % 12], i * 1013);
            e.memory_create(MemoryCreate { text, kind: None, scope: None, pinned: None, due_at: None }).unwrap();
        }
        let v: Value = serde_json::from_str(&ask(&e, json!({"topic": "memory"})).await).unwrap();
        assert_eq!(v["memories"]["total"], 35);
        assert_eq!(v["memories"]["shown"], 30);
        assert_eq!(v["memories"]["items"].as_array().unwrap().len(), 30);
        assert!(v["memories"]["note"].as_str().unwrap().contains("query"), "{v}");
        assert_eq!(v["counts"]["active"], 35);
        // A filter narrows the list and the note goes away.
        let one: Value = serde_json::from_str(&ask(&e, json!({"topic": "memory", "query": "34442"})).await).unwrap();
        assert_eq!(one["memories"]["total"], 1);
        assert!(one["memories"].get("note").is_none());
        // Whatever the topic, the answer stays within what the agent loop accepts.
        for t in TOPICS {
            assert!(ask(&e, json!({"topic": t})).await.chars().count() <= MAX_CHARS, "{t}");
        }
        let long = "x".repeat(MAX_CHARS * 2);
        let cut = finish(&e, json!({"testo": long}));
        assert!(cut.chars().count() <= MAX_CHARS && cut.contains("troncata"), "{}", cut.len());
    }

    #[tokio::test]
    async fn the_tool_needs_a_chat_run_but_not_the_owner_and_rejects_bad_topics_in_italian() {
        let (e, _d, _w) = engine(vec![]).await;
        let tool = e.tool_registry().get("aiw_inspect").expect("registered by the engine");
        assert_eq!(tool.risk(), Risk::Read);
        assert!(tool.explicit_grant(), "a `*` in an agent's tools does not include it");
        assert_eq!(tool.subject(&json!({"topic": "secrets"})), Some("secrets".into()));
        assert_eq!(tool.parameters()["properties"]["topic"]["enum"].as_array().unwrap().len(), 10);
        let args = json!({"topic": "overview"});
        let err = tool.call(args.clone(), &test_ctx(&e, None)).await.unwrap_err();
        assert!(matches!(err, Error::Denied(_)) && err.to_string().contains("disponibile solo all'Assistente in una chat"), "{err}");
        // Reading needs no ownership: a paired phone's chat can look.
        let phone = RunOrigin { owner: false, questions: true, secrets: false };
        assert!(tool.call(args, &test_ctx(&e, Some(phone))).await.is_ok());
        for (bad, why) in [
            (json!({}), "manca `topic`"),
            (json!({"topic": ""}), "manca `topic`"),
            (json!({"topic": "tutto"}), "topic sconosciuto «tutto»"),
        ] {
            let err = tool.call(bad.clone(), &test_ctx(&e, Some(phone))).await.unwrap_err();
            assert!(
                matches!(err, Error::Invalid(_)) && err.to_string().contains(why) && err.to_string().contains("overview"),
                "{bad}: {err}"
            );
        }
        // From a workflow or an MCP client the engine refuses it before the tool is even called.
        let err = e.execute_tool("mcp_1", "agent", "client", "aiw_inspect", json!({"topic": "overview"})).await.unwrap_err();
        assert!(matches!(err, Error::Denied(_)), "{err}");
    }

    #[test]
    fn helpers_cut_and_clean_what_they_are_given() {
        assert_eq!(clip("abcdef", 3), "abc…");
        assert_eq!(clip("abc", 3), "abc");
        assert_eq!(url_safe("https://u:p@ex.org:8443/a/b?token=zz#frag"), "https://ex.org:8443/a/b");
        assert_eq!(url_safe("https://ex.org/mcp/0123456789abcdef01234567"), "https://ex.org/mcp/omesso");
        assert_eq!(url_safe("non è un indirizzo"), "(indirizzo non leggibile)");
        let hidden = args_safe(&["--api-key=abc12345".into(), "--password".into(), "hunter22".into(), "--verbose".into(), "plain".into()]);
        assert_eq!(hidden, ["--api-key=••••••", "--password", SECRET_MASK, "--verbose", "plain"]);
        assert!(hit(&None, &["x"]) && hit(&Some("BET".into()), &["alfa beta"]) && !hit(&Some("zeta".into()), &["alfa beta"]));
        let cut = listing((0..31).map(|i| json!(i)).collect());
        assert!(cut["total"] == 31 && cut["shown"] == 30 && cut["items"].as_array().unwrap().len() == 30);
        assert!(cut["note"].as_str().unwrap().contains("query"));
        assert!(listing(vec![json!(1)]).get("note").is_none());
        let mut v = json!({"a": ["il segreto è S3CRET-LUNGO ok"], "b": {"c": "S3CRET-LUNGO"}, "n": 3});
        scrub(&mut v, &["S3CRET-LUNGO".to_string()]);
        assert_eq!(v, json!({"a": ["il segreto è •••••• ok"], "b": {"c": "••••••"}, "n": 3}));
    }
}
