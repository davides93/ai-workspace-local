//! Export of the agent trajectories: one record per session, in a shape that fits fine-tuning and
//! evaluation pipelines (OpenAI-style messages plus the run, approval and feedback signals).

use crate::store::Store;
use crate::Engine;
use aiw_core::api::{ChatMessage, McpServerConfig, BLOCKED_PROMPT_MESSAGE, RUN_ERROR_MESSAGE};
use aiw_core::{Error, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};

/// What replaces a secret in an export.
pub const REDACTED: &str = "[SEGRETO]";
/// What replaces the content of a tool message that cites an excluded path.
pub const EXCLUDED: &str = "[escluso]";

/// Which sessions to export and how to treat what they contain.
#[derive(Debug, Clone)]
pub struct TraceFilter {
    /// Only this session.
    pub session: Option<String>,
    pub agent: Option<String>,
    /// RFC 3339 or `YYYY-MM-DD`, compared with the last activity of the session.
    pub since: Option<String>,
    pub until: Option<String>,
    /// Last status of the session (`succeeded`, `failed`, `cancelled`, `running`).
    pub status: Option<String>,
    pub limit: Option<usize>,
    /// Only sessions with a thumbs up (`up`), a thumbs down (`down`) or any rating (`any`).
    pub rated: Option<String>,
    /// Mask API keys, tokens and private keys (on by default).
    pub redact: bool,
    /// Globs: tool messages that cite a matching path lose their content.
    pub exclude_paths: Vec<String>,
}

impl Default for TraceFilter {
    fn default() -> Self {
        Self {
            session: None,
            agent: None,
            since: None,
            until: None,
            status: None,
            limit: None,
            rated: None,
            redact: true,
            exclude_paths: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceRun {
    pub run_id: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
}

/// A decision of the user that shaped the trajectory.
#[derive(Debug, Clone, Serialize)]
pub struct TraceSignal {
    pub run_id: String,
    /// `approval`
    pub kind: String,
    pub tool: Option<String>,
    /// approved | rejected | cancelled
    pub decision: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceFeedback {
    pub message_id: String,
    pub rating: i32,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceRecord {
    pub session_id: String,
    pub title: String,
    pub agent_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_status: Option<String>,
    /// OpenAI chat format: `role`, `content`, `tool_calls`, `tool_call_id`, `name`.
    pub messages: Vec<Value>,
    /// Internal notes left out of `messages` (self-checks, format checks, run errors, blocked prompts).
    pub notes: usize,
    pub runs: Vec<TraceRun>,
    pub signals: Vec<TraceSignal>,
    pub feedback: Vec<TraceFeedback>,
}

/// One JSON object per line, as the JSONL export and the NDJSON route write them.
pub fn to_jsonl(records: &[TraceRecord]) -> Result<String> {
    let mut out = String::new();
    for r in records {
        out.push_str(&serde_json::to_string(r)?);
        out.push('\n');
    }
    Ok(out)
}

/// Masks the secrets that agents and tools tend to leave in a conversation.
pub struct Redactor {
    patterns: Vec<(Regex, &'static str)>,
    /// Exact values of secrets the app itself stores (MCP headers and variables).
    known: Vec<String>,
}

impl Redactor {
    pub fn new(known: Vec<String>) -> Self {
        let patterns = [
            (r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?(?:-----END [A-Z ]*PRIVATE KEY-----|$)", "[SEGRETO]"),
            (r"sk-[A-Za-z0-9_\-]{16,}", "[SEGRETO]"),
            (r"gh[pousr]_[A-Za-z0-9]{20,}", "[SEGRETO]"),
            (r"github_pat_[A-Za-z0-9_]{20,}", "[SEGRETO]"),
            (r"\bAKIA[0-9A-Z]{16}\b", "[SEGRETO]"),
            (r"xox[bp]-[A-Za-z0-9\-]{10,}", "[SEGRETO]"),
            (r"(?i)(\bBearer\s+)[A-Za-z0-9._~+/=\-]{8,}", "${1}[SEGRETO]"),
        ]
        .into_iter()
        .map(|(re, to)| (Regex::new(re).expect("static pattern"), to))
        .collect();
        // Longest first, so that a value which contains another is masked whole.
        let mut known: Vec<String> = known.into_iter().filter(|v| v.chars().count() >= 6).collect();
        known.sort_by_key(|v| std::cmp::Reverse(v.len()));
        known.dedup();
        Self { patterns, known }
    }

    pub fn apply(&self, text: &str) -> String {
        let mut out = text.to_string();
        for v in &self.known {
            if out.contains(v.as_str()) {
                out = out.replace(v.as_str(), REDACTED);
            }
        }
        for (re, to) in &self.patterns {
            if let std::borrow::Cow::Owned(s) = re.replace_all(&out, *to) {
                out = s;
            }
        }
        out
    }
}

/// The values of the stored MCP servers that are secrets (see `secrets::is_secret_name`).
pub(crate) fn mcp_secret_values(store: &Store) -> Vec<String> {
    let mut out = crate::mcp_oauth::secret_values(store);
    for cfg in store.list_docs::<McpServerConfig>("mcp").unwrap_or_default() {
        for map in [cfg.headers.as_ref(), cfg.env.as_ref()].into_iter().flatten() {
            for (k, v) in map {
                if crate::secrets::is_secret_name(k) {
                    if let Some(s) = v.as_str() {
                        out.push(s.to_string());
                        // `Authorization: Bearer xyz` is quoted by the model without the scheme.
                        if let Some((_, token)) = s.split_once(' ') {
                            out.push(token.to_string());
                        }
                    }
                }
            }
        }
    }
    out
}

/// `since`/`until` as instants. A bare date means the whole day (UTC): the start for `since`,
/// the last millisecond for `until`.
pub(crate) fn parse_bound(s: &str, end_of_day: bool, what: &str) -> Result<DateTime<Utc>> {
    let s = s.trim();
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Ok(t.with_timezone(&Utc));
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let start = d.and_hms_opt(0, 0, 0).expect("midnight").and_utc();
        return Ok(if end_of_day { start + Duration::days(1) - Duration::milliseconds(1) } else { start });
    }
    Err(Error::Invalid(format!("«{s}» non è una data valida per {what}: usa AAAA-MM-GG oppure RFC 3339")))
}

fn globs(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(Glob::new(p).map_err(|e| Error::Invalid(format!("glob non valido «{p}»: {e}")))?);
    }
    Ok(Some(b.build().map_err(|e| Error::Invalid(format!("glob non validi: {e}")))?))
}

/// Does `text` (arguments or output of a tool) name a path that one of the globs matches?
fn cites(set: &GlobSet, text: &str) -> bool {
    let token_ok = |t: &str| {
        let t = t.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | '(' | ')' | '[' | ']' | '{' | '}' | ':' | ';' | '`'));
        let t = t.strip_prefix("./").unwrap_or(t);
        !t.is_empty() && set.is_match(t)
    };
    fn walk(v: &Value, f: &dyn Fn(&str) -> bool) -> bool {
        match v {
            Value::String(s) => f(s) || s.split_whitespace().any(f),
            Value::Array(a) => a.iter().any(|x| walk(x, f)),
            Value::Object(o) => o.values().any(|x| walk(x, f)),
            _ => false,
        }
    }
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        if walk(&v, &token_ok) {
            return true;
        }
    }
    text.split_whitespace().any(token_ok)
}

/// Is this message an internal note rather than part of the conversation?
pub(crate) fn is_note(m: &ChatMessage) -> bool {
    match m.name.as_deref() {
        Some(n) if n == BLOCKED_PROMPT_MESSAGE || n == RUN_ERROR_MESSAGE => true,
        Some(n) if n == aiw_agent::runner::SELF_CHECK_NAME || n == aiw_agent::runner::FORMAT_CHECK_NAME => true,
        _ => m.role == "system",
    }
}

fn message_json(m: &ChatMessage, content: String, args: impl Fn(&str, &str) -> String) -> Value {
    let mut o = serde_json::Map::new();
    o.insert("role".into(), json!(m.role));
    o.insert("content".into(), json!(content));
    if let Some(tcs) = &m.tool_calls {
        let tcs: Vec<Value> = tcs
            .iter()
            .map(|t| json!({"id": t.id, "type": "function", "function": {"name": t.name, "arguments": args(&t.id, &t.arguments)}}))
            .collect();
        o.insert("tool_calls".into(), Value::Array(tcs));
    }
    if let Some(id) = &m.tool_call_id {
        o.insert("tool_call_id".into(), json!(id));
    }
    if let Some(n) = &m.name {
        o.insert("name".into(), json!(n));
    }
    Value::Object(o)
}

impl Engine {
    /// One [`TraceRecord`] per session that matches `filter`, most recently active first.
    pub fn export_traces(&self, filter: &TraceFilter) -> Result<Vec<TraceRecord>> {
        let store = &self.0.store;
        let since = filter.since.as_deref().map(|s| parse_bound(s, false, "since")).transpose()?;
        let until = filter.until.as_deref().map(|s| parse_bound(s, true, "until")).transpose()?;
        let excluded = globs(&filter.exclude_paths)?;
        let rated = match filter.rated.as_deref() {
            None | Some("") => None,
            Some("up") => Some(1),
            Some("down") => Some(-1),
            Some("any") => Some(0),
            Some(other) => return Err(Error::Invalid(format!("«{other}» non è un filtro valido per rated: usa up, down o any"))),
        };
        let redactor = filter.redact.then(|| Redactor::new(mcp_secret_values(store)));
        let clean = |s: &str| redactor.as_ref().map(|r| r.apply(s)).unwrap_or_else(|| s.to_string());
        let mut out = Vec::new();
        for s in store.all_sessions()? {
            if out.len() >= filter.limit.unwrap_or(usize::MAX) {
                break;
            }
            if filter.session.as_deref().is_some_and(|id| id != s.id) {
                continue;
            }
            if filter.agent.as_deref().is_some_and(|a| a != s.agent_id) {
                continue;
            }
            if filter.status.as_deref().is_some_and(|st| s.last_status.as_deref() != Some(st)) {
                continue;
            }
            let ratings = store.session_feedback(&s.id)?;
            if rated.is_some_and(|want| !ratings.iter().any(|f| want == 0 || f.rating == want)) {
                continue;
            }
            if since.is_some() || until.is_some() {
                let Ok(updated) = DateTime::parse_from_rfc3339(&s.updated_at).map(|t| t.with_timezone(&Utc)) else { continue };
                if since.is_some_and(|b| updated < b) || until.is_some_and(|b| updated > b) {
                    continue;
                }
            }
            let stored = store.messages(&s.id)?;
            let notes = stored.iter().filter(|m| is_note(m)).count();
            let convo: Vec<&ChatMessage> = stored.iter().filter(|m| !is_note(m)).collect();
            // Tool calls whose arguments cite an excluded path: their result goes too.
            let mut hidden_calls = std::collections::HashSet::new();
            if let Some(set) = &excluded {
                for m in &convo {
                    for t in m.tool_calls.iter().flatten() {
                        if cites(set, &t.arguments) {
                            hidden_calls.insert(t.id.clone());
                        }
                    }
                }
            }
            let messages = convo
                .iter()
                .map(|m| {
                    let hidden_result = m.role == "tool"
                        && (m.tool_call_id.as_ref().is_some_and(|id| hidden_calls.contains(id))
                            || excluded.as_ref().is_some_and(|set| cites(set, &m.content)));
                    let content = if hidden_result { EXCLUDED.to_string() } else { clean(&m.content) };
                    message_json(m, content, |id, raw| if hidden_calls.contains(id) { EXCLUDED.to_string() } else { clean(raw) })
                })
                .collect();
            let run_rows = store.session_runs(&s.id)?;
            let mut signals = Vec::new();
            for r in &run_rows {
                for a in store.audit_for_run(&r.id)? {
                    if a.action != "tool" {
                        continue;
                    }
                    if let Some(d) = a.decision.as_deref().filter(|d| matches!(*d, "approved" | "rejected" | "cancelled")) {
                        signals.push(TraceSignal {
                            run_id: r.id.clone(),
                            kind: "approval".into(),
                            tool: a.tool.clone(),
                            decision: d.into(),
                        });
                    }
                }
            }
            out.push(TraceRecord {
                title: clean(&s.title),
                session_id: s.id,
                agent_id: s.agent_id,
                created_at: s.created_at,
                updated_at: s.updated_at,
                last_status: s.last_status,
                messages,
                notes,
                runs: run_rows
                    .into_iter()
                    .map(|r| TraceRun { run_id: r.id, status: r.status, started_at: r.started_at, finished_at: r.finished_at })
                    .collect(),
                signals,
                feedback: ratings
                    .into_iter()
                    .map(|f| TraceFeedback { message_id: f.message_id, rating: f.rating, note: f.note.map(|n| clean(&n)) })
                    .collect(),
            });
        }
        Ok(out)
    }
}
