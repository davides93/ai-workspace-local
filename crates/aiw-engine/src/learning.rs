//! Learning without weights: after a conversation the agent can propose a lesson (a note for
//! `AGENTS.md` or a new skill). A lesson is only a proposal until the user approves it; nothing is
//! written to disk before that, and never inside the workspace folder (it is repository content).

use crate::traces::{mcp_secret_values, Redactor, TraceFilter};
use crate::Engine;
use aiw_core::api::{ChatMessage, Lesson, RUN_ERROR_MESSAGE};
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use serde_json::{json, Value};

const LESSON_KIND: &str = "lesson";
/// Heading of `AGENTS.md` under which approved memory lessons are collected.
const MEMORY_HEADING: &str = "## Lezioni apprese";
/// Messages of the conversation the agent gets to reflect on.
const TRACE_MESSAGES: usize = 30;
/// Longest text of one message, tool call or note in that excerpt.
const TRACE_CHARS: usize = 1_200;
const MAX_TITLE_CHARS: usize = 120;
const MAX_CONTENT_CHARS: usize = 6_000;
const MAX_REASON_CHARS: usize = 600;
/// Skill folders get a numeric suffix when the name is taken; this many are tried.
const MAX_SLUG_SUFFIX: u32 = 99;

/// When a reflection starts by itself (`learning.reflect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReflectTrigger {
    /// A thumbs down.
    Feedback,
    /// A run that failed or ran out of budget.
    Failure,
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("… [troncato]");
    out
}

/// Lowercase ASCII words joined by `-`; empty when the text has none.
fn slug(s: &str) -> String {
    let out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    out.split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(48)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

/// The prompt that asks for a lesson, with the conversation excerpt in it.
fn reflection_prompt(trace: &str) -> String {
    format!(
        "Sei l'agente che ha appena lavorato alla conversazione qui sotto. Rifletti su cosa è andato storto o poteva andare meglio \
         e proponi UNA sola lezione breve e riutilizzabile, valida anche per conversazioni future.\n\
         - kind \"memory\": una regola o un'informazione da ricordare sempre (finisce in AGENTS.md).\n\
         - kind \"skill\": una procedura da seguire passo passo per un tipo di compito (diventa una skill).\n\
         Non copiare dati personali, segreti o dettagli specifici di questa conversazione.\n\n\
         Rispondi solo con un oggetto JSON, senza altro testo: {{\"kind\": \"memory\" | \"skill\", \"title\": \"titolo breve\", \
         \"content\": \"testo in Markdown\", \"reason\": \"perché serve, in una frase\"}}\n\n\
         # Conversazione\n{trace}"
    )
}

fn lesson_schema() -> Value {
    json!({
        "type": "object",
        "required": ["kind", "title", "content", "reason"],
        "properties": {
            "kind": {"type": "string", "enum": ["memory", "skill"]},
            "title": {"type": "string"},
            "content": {"type": "string"},
            "reason": {"type": "string"}
        }
    })
}

/// A lesson read from the model's answer: the JSON, validated.
fn parse_lesson(answer: &str) -> std::result::Result<(String, String, String, String), String> {
    let v = aiw_agent::schema::extract_json(answer).ok_or_else(|| "la risposta non contiene un oggetto JSON".to_string())?;
    aiw_agent::schema::validate(&lesson_schema(), &v).map_err(|e| e.join("; "))?;
    let text = |k: &str| v[k].as_str().unwrap_or_default().trim().to_string();
    let (kind, title, content, reason) = (text("kind"), text("title"), text("content"), text("reason"));
    if title.is_empty() || content.is_empty() {
        return Err("title e content non possono essere vuoti".into());
    }
    if title.chars().count() > MAX_TITLE_CHARS || content.chars().count() > MAX_CONTENT_CHARS || reason.chars().count() > MAX_REASON_CHARS {
        return Err(format!(
            "testi troppo lunghi (title {MAX_TITLE_CHARS}, content {MAX_CONTENT_CHARS}, reason {MAX_REASON_CHARS} caratteri)"
        ));
    }
    Ok((kind, title, content, reason))
}

fn one_line(s: &str, max: usize) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

impl Engine {
    // ------------------------------------------------------------------ list
    /// The lessons, most recent first; `status` keeps only those `pending`, `approved` or `rejected`.
    pub fn lessons(&self, status: Option<&str>) -> Result<Vec<Lesson>> {
        if let Some(s) = status.filter(|s| !s.is_empty()) {
            if !["pending", "approved", "rejected"].contains(&s) {
                return Err(Error::Invalid(format!("«{s}» non è uno stato valido: usa pending, approved o rejected")));
            }
        }
        let mut all: Vec<Lesson> = self.0.store.list_docs(LESSON_KIND)?;
        all.retain(|l| status.filter(|s| !s.is_empty()).is_none_or(|s| l.status == s));
        all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(all)
    }
    pub fn lesson(&self, id: &str) -> Result<Lesson> {
        self.0.store.get_doc(LESSON_KIND, id)?.ok_or_else(|| Error::NotFound(format!("lezione {id} non trovata")))
    }

    // ------------------------------------------------------------------ reflect
    /// The conversation as the agent reflects on it: the last messages, tool errors, refusals,
    /// failed runs and the notes of the thumbs down, long contents cut and secrets masked.
    fn reflection_trace(&self, session_id: &str, run_id: Option<&str>) -> Result<String> {
        let rec = self
            .export_traces(&TraceFilter { session: Some(session_id.into()), ..Default::default() })?
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound(format!("sessione {session_id} non trovata")))?;
        let mut out = String::new();
        let skip = rec.messages.len().saturating_sub(TRACE_MESSAGES);
        for m in rec.messages.iter().skip(skip) {
            let role = m["role"].as_str().unwrap_or("?");
            let content = truncate(m["content"].as_str().unwrap_or_default(), TRACE_CHARS);
            out.push_str(&format!("[{role}] {content}\n"));
            for t in m["tool_calls"].as_array().into_iter().flatten() {
                let args = truncate(t["function"]["arguments"].as_str().unwrap_or_default(), TRACE_CHARS);
                out.push_str(&format!("  chiama {}: {args}\n", t["function"]["name"].as_str().unwrap_or("?")));
            }
        }
        // Run errors are notes, left out of the export's messages.
        let redactor = Redactor::new(mcp_secret_values(&self.0.store));
        let errors: Vec<ChatMessage> =
            self.messages(session_id)?.into_iter().filter(|m| m.name.as_deref() == Some(RUN_ERROR_MESSAGE)).collect();
        for e in errors.iter().rev().take(3).rev() {
            out.push_str(&format!("[errore del run] {}\n", truncate(&redactor.apply(&e.content), TRACE_CHARS)));
        }
        for s in rec.signals.iter().filter(|s| s.decision != "approved") {
            out.push_str(&format!("[rifiuto] {} {}\n", s.tool.as_deref().unwrap_or("?"), s.decision));
        }
        for r in rec.runs.iter().filter(|r| r.status == "failed" || run_id == Some(r.run_id.as_str())) {
            out.push_str(&format!("[run {}] stato {}\n", r.run_id, r.status));
        }
        for f in rec.feedback.iter().filter(|f| f.rating < 0) {
            match &f.note {
                Some(n) => out.push_str(&format!("[pollice giù] {}\n", truncate(n, TRACE_CHARS))),
                None => out.push_str("[pollice giù]\n"),
            }
        }
        Ok(out)
    }

    /// Asks the model for a lesson on `session_id` (about `run_id` when given) and stores it
    /// `pending`. The answer must be a JSON object that validates; it gets one correction, then
    /// the error is returned and nothing is stored.
    pub async fn reflect(&self, session_id: &str, run_id: Option<&str>) -> Result<Lesson> {
        let session = self.0.store.session(session_id)?;
        let trace = self.reflection_trace(session_id, run_id)?;
        let profile = match self.0.config.get_str("learning.profile") {
            Some(p) => {
                self.profile(&p).map_err(|_| Error::Invalid(format!("il profilo di inferenza `{p}` per la riflessione non esiste")))?
            }
            None => match self.agent(&session.agent_id).ok().and_then(|a| self.profile(&a.inference_profile).ok()) {
                Some(p) => p,
                None => self.fallback_profile()?,
            },
        };
        use aiw_eval::EvalHost;
        let prompt = reflection_prompt(&trace);
        let mut answer = self.complete(Some(&profile.id), &prompt).await?;
        let parsed = match parse_lesson(&answer) {
            Ok(p) => p,
            Err(first) => {
                let retry = format!(
                    "{prompt}\n\n# Correzione\nLa tua risposta precedente non era valida ({first}). Rispondi di nuovo solo con l'oggetto JSON richiesto."
                );
                answer = self.complete(Some(&profile.id), &retry).await?;
                parse_lesson(&answer).map_err(|e| Error::Invalid(format!("la riflessione non ha prodotto una lezione valida: {e}")))?
            }
        };
        let (kind, title, content, reason) = parsed;
        let lesson = Lesson {
            id: new_id("les"),
            session_id: session_id.into(),
            run_id: run_id.map(String::from),
            agent_id: session.agent_id,
            kind,
            title,
            content,
            reason,
            status: "pending".into(),
            created_at: now_rfc3339(),
            decided_at: None,
        };
        self.0.store.put_doc(LESSON_KIND, &lesson.id, &lesson)?;
        Ok(lesson)
    }

    /// Starts a reflection in the background when `learning.reflect` asks for one on `trigger`,
    /// at most one lesson waiting per session. Failures only go to the log.
    pub(crate) fn maybe_reflect(&self, session_id: &str, run_id: Option<String>, trigger: ReflectTrigger) {
        let wanted = match self.0.config.get_str("learning.reflect").as_deref() {
            Some("off") => false,
            Some("on_failure") => true,
            _ => trigger == ReflectTrigger::Feedback,
        };
        if !wanted {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else { return };
        let has_pending = self.lessons(Some("pending")).map(|l| l.iter().any(|l| l.session_id == session_id)).unwrap_or(true);
        if has_pending || !self.0.reflecting.lock().unwrap().insert(session_id.to_string()) {
            return;
        }
        let engine = self.clone();
        let session_id = session_id.to_string();
        handle.spawn(async move {
            if let Err(e) = engine.reflect(&session_id, run_id.as_deref()).await {
                tracing::warn!("riflessione sulla sessione {session_id}: {e}");
            }
            engine.0.reflecting.lock().unwrap().remove(&session_id);
        });
    }

    // ------------------------------------------------------------------ decide
    /// Approves a pending lesson, optionally with a new title and content, and writes it: a
    /// `memory` under `## Lezioni apprese` in `~/.aiw/AGENTS.md`, a `skill` as a new folder in
    /// `~/.aiw/skills`. The lesson stays pending when the file cannot be written.
    pub fn approve_lesson(&self, id: &str, title: Option<String>, content: Option<String>) -> Result<Lesson> {
        let mut l = self.lesson(id)?;
        if l.status != "pending" {
            return Err(Error::Conflict(format!(
                "la lezione è già stata {}",
                if l.status == "approved" { "approvata" } else { "scartata" }
            )));
        }
        if let Some(t) = title.map(|t| t.trim().to_string()) {
            l.title = t;
        }
        if let Some(c) = content.map(|c| c.trim().to_string()) {
            l.content = c;
        }
        if l.title.is_empty() || l.content.is_empty() {
            return Err(Error::Invalid("titolo e contenuto della lezione non possono essere vuoti".into()));
        }
        if l.title.chars().count() > MAX_TITLE_CHARS || l.content.chars().count() > MAX_CONTENT_CHARS {
            return Err(Error::Invalid(format!(
                "titolo (max {MAX_TITLE_CHARS}) o contenuto (max {MAX_CONTENT_CHARS} caratteri) troppo lunghi"
            )));
        }
        let target = match l.kind.as_str() {
            "memory" => self.write_memory_lesson(&l)?,
            "skill" => self.write_skill_lesson(&l)?,
            other => return Err(Error::Invalid(format!("tipo di lezione sconosciuto: {other}"))),
        };
        l.status = "approved".into();
        l.decided_at = Some(now_rfc3339());
        self.0.store.put_doc(LESSON_KIND, &l.id, &l)?;
        if l.kind == "memory" {
            // The same lesson is also a pinned memory, so that the "Insegna" page shows and edits it.
            self.memory_from_lesson(&l.title, &l.content, &l.session_id);
        }
        self.audit_user("lesson.approve", &format!("{} «{}» → {target}", l.kind, l.title));
        Ok(l)
    }

    pub fn reject_lesson(&self, id: &str) -> Result<Lesson> {
        let mut l = self.lesson(id)?;
        if l.status != "pending" {
            return Err(Error::Conflict(format!(
                "la lezione è già stata {}",
                if l.status == "approved" { "approvata" } else { "scartata" }
            )));
        }
        l.status = "rejected".into();
        l.decided_at = Some(now_rfc3339());
        self.0.store.put_doc(LESSON_KIND, &l.id, &l)?;
        self.audit_user("lesson.reject", &format!("{} «{}»", l.kind, l.title));
        Ok(l)
    }

    /// Adds the entry to `~/.aiw/AGENTS.md`, at the end of the `## Lezioni apprese` section (made if missing).
    fn write_memory_lesson(&self, l: &Lesson) -> Result<String> {
        let path = self.0.dirs.root.join("AGENTS.md");
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        let entry =
            format!("### {} ({})\n{}\n", one_line(&l.title, MAX_TITLE_CHARS), chrono::Local::now().format("%Y-%m-%d"), l.content.trim());
        let updated = match current.lines().position(|line| line.trim_end() == MEMORY_HEADING) {
            None => {
                let sep = if current.trim().is_empty() { "" } else { "\n" };
                format!(
                    "{}{sep}{MEMORY_HEADING}\n\n{entry}",
                    if current.is_empty() || current.ends_with('\n') { current } else { format!("{current}\n") }
                )
            }
            Some(i) => {
                let lines: Vec<&str> = current.lines().collect();
                let end = lines.iter().skip(i + 1).position(|line| line.starts_with("## ")).map_or(lines.len(), |p| i + 1 + p);
                let mut before = lines[..end].join("\n");
                while before.ends_with('\n') {
                    before.pop();
                }
                let after = lines[end..].join("\n");
                let tail = if after.is_empty() { String::new() } else { format!("\n{after}\n") };
                format!("{before}\n\n{entry}{tail}")
            }
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, updated)?;
        Ok(path.display().to_string())
    }

    /// Creates `~/.aiw/skills/<slug>/SKILL.md`; a name already taken gets `-2`, `-3`…
    fn write_skill_lesson(&self, l: &Lesson) -> Result<String> {
        let root = self.0.dirs.skills();
        let base = Some(slug(&l.title)).filter(|s| !s.is_empty()).unwrap_or_else(|| "lezione".into());
        let mut name = base.clone();
        let mut n = 1;
        while root.join(&name).exists() {
            n += 1;
            if n > MAX_SLUG_SUFFIX {
                return Err(Error::Conflict(format!("troppe skill chiamate «{base}»: cambia il titolo")));
            }
            name = format!("{base}-{n}");
        }
        let first_line = l.content.lines().map(|x| x.trim().trim_start_matches('#').trim()).find(|x| !x.is_empty()).unwrap_or_default();
        let description = one_line(if first_line.is_empty() { &l.reason } else { first_line }, 200);
        let body = if l.content.trim_start().starts_with('#') { l.content.clone() } else { format!("# {}\n\n{}", l.title, l.content) };
        let dir = root.join(&name);
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("SKILL.md");
        if let Err(e) = std::fs::write(&file, format!("---\nname: {name}\ndescription: {description}\n---\n\n{}\n", body.trim())) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e.into());
        }
        Ok(file.display().to_string())
    }
}
