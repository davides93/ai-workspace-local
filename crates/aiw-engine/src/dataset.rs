//! Training datasets built from what the user already has: sessions the user rated well, runs that
//! succeeded, tasks that passed an evaluation. Secrets are always masked (the same masking as the
//! trajectory export, which this module reuses).
//!
//! A dataset is a folder in `~/.aiw/datasets/<id>/`: `train.jsonl`, `valid.jsonl` (90/10, in an order
//! that depends only on the examples) and `meta.json`.

use crate::traces::{Redactor, TraceFilter, TraceRecord};
use crate::Engine;
use aiw_core::api::{DatasetInfo, DatasetSpec};
use aiw_core::events::AgEvent;
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Fewest examples a dataset needs: fewer than this does not train anything worth keeping.
pub const MIN_EXAMPLES: usize = 10;
/// Fixed seed of the order that picks the validation examples.
const SPLIT_SEED: &str = "aiw-dataset-v1";

/// One line of `train.jsonl` / `valid.jsonl`.
pub(crate) struct Example {
    /// What the order of the split depends on.
    pub key: String,
    pub value: Value,
    pub agent: String,
}

/// Is `id` safe to use as a folder name?
fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// A message of a trace as the chat template of a model wants it: the tool-call arguments are an object
/// (not a string), and only a tool message keeps its `name`.
fn chat_message(m: &Value) -> Value {
    let mut out = serde_json::Map::new();
    let role = m["role"].as_str().unwrap_or("user");
    out.insert("role".into(), json!(role));
    out.insert("content".into(), json!(m["content"].as_str().unwrap_or("")));
    if let Some(calls) = m["tool_calls"].as_array() {
        let calls: Vec<Value> = calls
            .iter()
            .map(|c| {
                let raw = c["function"]["arguments"].as_str().unwrap_or("{}");
                let args = serde_json::from_str::<Value>(raw).ok().filter(Value::is_object).unwrap_or_else(|| json!(raw));
                json!({"id": c["id"], "type": "function", "function": {"name": c["function"]["name"], "arguments": args}})
            })
            .collect();
        out.insert("tool_calls".into(), Value::Array(calls));
    }
    if let Some(id) = m.get("tool_call_id").filter(|v| v.is_string()) {
        out.insert("tool_call_id".into(), id.clone());
    }
    if role == "tool" {
        if let Some(n) = m.get("name").filter(|v| v.is_string()) {
            out.insert("name".into(), n.clone());
        }
    }
    Value::Object(out)
}

/// The conversation as a training example: it ends with the final answer of the assistant (what comes
/// after it is dropped) and has at least a user message. `None` when there is nothing to learn from.
pub(crate) fn chat_example(messages: &[Value]) -> Option<Value> {
    let end = messages.iter().rposition(|m| m["role"] == "assistant" && m["tool_calls"].as_array().is_none_or(|t| t.is_empty()))?;
    let kept = &messages[..=end];
    if !kept.iter().any(|m| m["role"] == "user") || kept[end]["content"].as_str().is_none_or(|c| c.trim().is_empty()) {
        return None;
    }
    Some(json!({"messages": kept.iter().map(chat_message).collect::<Vec<_>>()}))
}

/// The text the assistant answered with in a run, from its events (the last message of the run).
pub(crate) fn final_text(events: &[aiw_core::events::SeqEvent]) -> String {
    let mut order: Vec<String> = vec![];
    let mut texts: std::collections::HashMap<String, String> = Default::default();
    for e in events {
        if let AgEvent::TextMessageContent { message_id, delta } = &e.event.event {
            if !texts.contains_key(message_id) {
                order.push(message_id.clone());
            }
            texts.entry(message_id.clone()).or_default().push_str(delta);
        }
    }
    order.last().and_then(|id| texts.remove(id)).unwrap_or_default()
}

/// Order of an example in the split: a hash of the seed and its key, so it does not change when
/// other examples come or go.
fn split_rank(key: &str) -> String {
    let mut h = Sha256::new();
    h.update(SPLIT_SEED.as_bytes());
    h.update(key.as_bytes());
    format!("{:x}", h.finalize())
}

/// 90/10 by the hash order; always at least one validation example.
pub(crate) fn split(mut examples: Vec<Example>) -> (Vec<Example>, Vec<Example>) {
    examples.sort_by_cached_key(|e| split_rank(&e.key));
    let valid_n = ((examples.len() as f64) * 0.1).round().max(1.0) as usize;
    let train = examples.split_off(valid_n.min(examples.len()));
    (train, examples)
}

fn jsonl(examples: &[Example]) -> Result<String> {
    let mut out = String::new();
    for e in examples {
        out.push_str(&serde_json::to_string(&e.value)?);
        out.push('\n');
    }
    Ok(out)
}

impl Engine {
    fn dataset_dir(&self, id: &str) -> Result<PathBuf> {
        if !safe_id(id) {
            return Err(Error::NotFound(format!("dataset {id} non trovato")));
        }
        Ok(self.0.dirs.datasets().join(id))
    }

    /// Writes a dataset from the sessions, runs and evaluations `spec` names.
    pub fn build_dataset(&self, spec: &DatasetSpec) -> Result<DatasetInfo> {
        let (examples, sources) = match spec.method.as_str() {
            "sft" => self.sft_examples(spec)?,
            "dpo" => self.dpo_examples(spec)?,
            other => return Err(Error::Invalid(format!("metodo «{other}» sconosciuto: usa sft, dpo o grpo"))),
        };
        self.write_dataset(spec, examples, sources)
    }

    /// Splits, checks the minimum and writes the files.
    pub(crate) fn write_dataset(&self, spec: &DatasetSpec, examples: Vec<Example>, sources: Vec<String>) -> Result<DatasetInfo> {
        let noun = if spec.method == "dpo" { "coppie" } else { "esempi" };
        if examples.len() < MIN_EXAMPLES {
            return Err(Error::Invalid(format!(
                "servono almeno {MIN_EXAMPLES} {noun} per addestrare, ne ho trovati {}. Dai un pollice alle risposte buone o allarga i filtri.",
                examples.len()
            )));
        }
        let chars: usize = examples.iter().map(|e| e.value.to_string().len()).sum();
        let agents: BTreeSet<String> = examples.iter().map(|e| e.agent.clone()).filter(|a| !a.is_empty()).collect();
        let total = examples.len();
        let (train, valid) = split(examples);
        let id = new_id("ds");
        let dir = self.dataset_dir(&id)?;
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("train.jsonl"), jsonl(&train)?)?;
        std::fs::write(dir.join("valid.jsonl"), jsonl(&valid)?)?;
        let info = DatasetInfo {
            name: spec
                .name
                .clone()
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("{} {}", spec.method.to_uppercase(), &id[3..9])),
            id,
            method: spec.method.clone(),
            created_at: now_rfc3339(),
            examples: total,
            train: train.len(),
            valid: valid.len(),
            tokens_est: (chars / 4) as u64,
            agents: agents.into_iter().collect(),
            sources,
            path: dir.display().to_string(),
        };
        std::fs::write(dir.join("meta.json"), serde_json::to_string_pretty(&info)?)?;
        self.audit_user("dataset.build", &format!("{} ({}, {total})", info.name, info.method));
        Ok(info)
    }

    /// The datasets on disk, newest first.
    pub fn datasets(&self) -> Result<Vec<DatasetInfo>> {
        let mut out: Vec<DatasetInfo> = vec![];
        if let Ok(rd) = std::fs::read_dir(self.0.dirs.datasets()) {
            for e in rd.flatten() {
                let Ok(text) = std::fs::read_to_string(e.path().join("meta.json")) else { continue };
                match serde_json::from_str::<DatasetInfo>(&text) {
                    Ok(mut info) => {
                        info.path = e.path().display().to_string();
                        out.push(info);
                    }
                    Err(err) => tracing::warn!("dataset {}: {err}", e.path().display()),
                }
            }
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        Ok(out)
    }

    pub fn dataset(&self, id: &str) -> Result<DatasetInfo> {
        let dir = self.dataset_dir(id)?;
        let text = std::fs::read_to_string(dir.join("meta.json")).map_err(|_| Error::NotFound(format!("dataset {id} non trovato")))?;
        let mut info: DatasetInfo = serde_json::from_str(&text)?;
        info.path = dir.display().to_string();
        Ok(info)
    }

    pub fn delete_dataset(&self, id: &str) -> Result<()> {
        let info = self.dataset(id)?;
        if self.train_jobs()?.iter().any(|j| j.dataset_id == id && matches!(j.status.as_str(), "queued" | "running")) {
            return Err(Error::Conflict("il dataset è in uso da un addestramento in corso".into()));
        }
        std::fs::remove_dir_all(self.dataset_dir(id)?)?;
        self.audit_user("dataset.delete", &info.name);
        Ok(())
    }

    // ------------------------------------------------------------------ DPO
    /// Preference pairs: the same prompt with an answer rated up and one rated down (in one session
    /// after a regeneration, or in different sessions with the same user message), plus the repeated
    /// attempts of an evaluation task where one passed and one failed.
    fn dpo_examples(&self, spec: &DatasetSpec) -> Result<(Vec<Example>, Vec<String>)> {
        let mut sources: Vec<String> = spec.sources.clone();
        if sources.is_empty() {
            sources.push("feedback".into());
            if !spec.eval_reports.is_empty() {
                sources.push("eval".into());
            }
        }
        for s in &sources {
            if !matches!(s.as_str(), "feedback" | "eval") {
                return Err(Error::Invalid(format!("sorgente «{s}» sconosciuta per dpo: usa feedback o eval")));
            }
        }
        sources.sort();
        sources.dedup();
        let mut out = vec![];
        if sources.iter().any(|s| s == "feedback") {
            out.extend(self.feedback_pairs(spec)?);
        }
        if sources.iter().any(|s| s == "eval") {
            out.extend(self.eval_pairs(spec)?);
        }
        if let Some(limit) = spec.limit {
            out.truncate(limit);
        }
        Ok((out, sources))
    }

    fn feedback_pairs(&self, spec: &DatasetSpec) -> Result<Vec<Example>> {
        let store = &self.0.store;
        let since = spec.since.as_deref().map(|s| crate::traces::parse_bound(s, false, "since")).transpose()?;
        let until = spec.until.as_deref().map(|s| crate::traces::parse_bound(s, true, "until")).transpose()?;
        let redactor = Redactor::new(crate::traces::mcp_secret_values(store));
        // Rated answers grouped by the normalized user message they answer.
        let mut groups: std::collections::BTreeMap<String, (Vec<Answer>, Vec<Answer>)> = Default::default();
        for s in store.all_sessions()? {
            if spec.agent.as_deref().is_some_and(|a| a != s.agent_id) {
                continue;
            }
            if since.is_some() || until.is_some() {
                let Ok(updated) = chrono::DateTime::parse_from_rfc3339(&s.updated_at).map(|t| t.with_timezone(&chrono::Utc)) else {
                    continue;
                };
                if since.is_some_and(|b| updated < b) || until.is_some_and(|b| updated > b) {
                    continue;
                }
            }
            let ratings = store.session_feedback(&s.id)?;
            if ratings.is_empty() {
                continue;
            }
            let stored = store.messages(&s.id)?;
            let convo: Vec<_> = stored.iter().filter(|m| !crate::traces::is_note(m)).collect();
            for (i, m) in convo.iter().enumerate() {
                let Some(f) = ratings.iter().find(|f| f.message_id == m.id && f.rating != 0) else { continue };
                if m.role != "assistant" || m.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) || m.content.trim().is_empty() {
                    continue;
                }
                let Some(last_user) = convo[..i].iter().rposition(|x| x.role == "user") else { continue };
                // The prompt is the conversation up to the last user message, tools included.
                let prompt: Vec<Value> = convo[..=last_user]
                    .iter()
                    .map(|x| {
                        let v = serde_json::to_value(x).unwrap_or(Value::Null);
                        let mut c = chat_message(&v);
                        c["content"] = json!(redactor.apply(x.content.as_str()));
                        c
                    })
                    .collect();
                let key = normalize(&convo[last_user].content);
                if key.is_empty() {
                    continue;
                }
                let answer = Answer { prompt, text: redactor.apply(&m.content) };
                let entry = groups.entry(key).or_default();
                if f.rating > 0 {
                    entry.0.push(answer);
                } else {
                    entry.1.push(answer);
                }
            }
        }
        let mut out = vec![];
        for (key, (up, down)) in groups {
            // Equal answers are not a preference.
            for (c, r) in up.iter().zip(down.iter()).filter(|(c, r)| c.text.trim() != r.text.trim()) {
                out.push(pair_example(&format!("feedback:{key}"), &c.prompt, &c.text, &r.text, ""));
            }
        }
        Ok(out)
    }

    /// One pair per task of the reports that has an attempt that passed and one that failed.
    fn eval_pairs(&self, spec: &DatasetSpec) -> Result<Vec<Example>> {
        if spec.eval_reports.is_empty() {
            return Err(Error::Invalid(
                "la sorgente eval richiede i report di valutazione (con --repeat) da cui prendere le coppie".into(),
            ));
        }
        let redactor = Redactor::new(crate::traces::mcp_secret_values(&self.0.store));
        let mut out = vec![];
        for report_id in &spec.eval_reports {
            let report = self.eval_report(report_id)?;
            let suite = self.eval_suite(&report.suite_id).map_err(|_| {
                Error::Invalid(format!("la suite «{}» del report {report_id} non esiste più: servono i prompt dei task", report.suite_id))
            })?;
            let agent = report.agent.clone().or_else(|| suite.agent.clone()).unwrap_or_default();
            let mut ids: Vec<&str> = report.tasks.iter().map(|t| t.task_id.as_str()).collect();
            ids.sort();
            ids.dedup();
            for task_id in ids {
                let Some(task) = suite.task(task_id) else { continue };
                let answers = |passed: bool| -> Vec<String> {
                    report
                        .tasks
                        .iter()
                        .filter(|t| t.task_id == task_id && t.passed == passed)
                        .filter_map(|t| t.run_id.as_ref())
                        .map(|r| final_text(&self.run_events(r)))
                        .filter(|a| !a.trim().is_empty())
                        .collect()
                };
                let prompt = vec![json!({"role": "user", "content": redactor.apply(&task.prompt)})];
                for (c, r) in answers(true).iter().zip(answers(false).iter()).filter(|(c, r)| c.trim() != r.trim()) {
                    let (c, r) = (redactor.apply(c), redactor.apply(r));
                    let mut ex = pair_example(&format!("eval:{report_id}:{task_id}"), &prompt, &c, &r, &agent);
                    ex.agent = agent.clone();
                    out.push(ex);
                }
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------------ SFT
    fn sft_examples(&self, spec: &DatasetSpec) -> Result<(Vec<Example>, Vec<String>)> {
        let mut sources: Vec<String> = spec.sources.clone();
        if sources.is_empty() {
            sources.push("feedback".into());
        }
        for s in &sources {
            if !matches!(s.as_str(), "feedback" | "succeeded" | "eval") {
                return Err(Error::Invalid(format!("sorgente «{s}» sconosciuta: usa feedback, succeeded o eval")));
            }
        }
        sources.sort();
        sources.dedup();
        let mut out: Vec<Example> = vec![];
        let (feedback, succeeded) = (sources.iter().any(|s| s == "feedback"), sources.iter().any(|s| s == "succeeded"));
        if feedback || succeeded {
            let filter = TraceFilter {
                agent: spec.agent.clone(),
                since: spec.since.clone(),
                until: spec.until.clone(),
                redact: true,
                ..Default::default()
            };
            for r in self.export_traces(&filter)? {
                if let Some(value) = session_example(&r, feedback, succeeded) {
                    out.push(Example { key: format!("session:{}", r.session_id), value, agent: r.agent_id });
                }
            }
        }
        if sources.iter().any(|s| s == "eval") {
            out.extend(self.eval_examples(spec)?);
        }
        if let Some(limit) = spec.limit {
            out.truncate(limit);
        }
        Ok((out, sources))
    }

    /// One example per task that passed in the reports named by `spec`: the prompt of the task and
    /// the answer the run gave (single turn: the tools the run used are not replayed).
    fn eval_examples(&self, spec: &DatasetSpec) -> Result<Vec<Example>> {
        if spec.eval_reports.is_empty() {
            return Err(Error::Invalid("la sorgente eval richiede i report di valutazione da cui prendere i task riusciti".into()));
        }
        let redactor = Redactor::new(crate::traces::mcp_secret_values(&self.0.store));
        let mut out = vec![];
        for report_id in &spec.eval_reports {
            let report = self.eval_report(report_id)?;
            let suite = self.eval_suite(&report.suite_id).map_err(|_| {
                Error::Invalid(format!("la suite «{}» del report {report_id} non esiste più: servono i prompt dei task", report.suite_id))
            })?;
            for t in report.tasks.iter().filter(|t| t.passed) {
                let (Some(task), Some(run_id)) = (suite.task(&t.task_id), t.run_id.as_ref()) else { continue };
                let answer = final_text(&self.run_events(run_id));
                if answer.trim().is_empty() {
                    continue;
                }
                let value = json!({"messages": [
                    {"role": "user", "content": redactor.apply(&task.prompt)},
                    {"role": "assistant", "content": redactor.apply(&answer)},
                ]});
                let agent = report.agent.clone().or_else(|| suite.agent.clone()).unwrap_or_default();
                out.push(Example { key: format!("eval:{report_id}:{}:{}", t.task_id, t.attempt), value, agent });
            }
        }
        Ok(out)
    }
}

/// A rated answer with the conversation that led to it.
struct Answer {
    prompt: Vec<Value>,
    text: String,
}

/// A user message with the case and the spacing taken out, to tell when two sessions asked the same thing.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// A line of a DPO dataset. The key depends on the content, so the split is stable.
fn pair_example(origin: &str, prompt: &[Value], chosen: &str, rejected: &str, agent: &str) -> Example {
    let mut h = Sha256::new();
    for part in [chosen, rejected] {
        h.update(part.as_bytes());
        h.update([0]);
    }
    h.update(serde_json::to_string(prompt).unwrap_or_default().as_bytes());
    let digest = format!("{:x}", h.finalize());
    Example {
        key: format!("{origin}:{}", &digest[..16]),
        value: json!({"prompt": prompt, "chosen": chosen, "rejected": rejected}),
        agent: agent.to_string(),
    }
}

/// The example of a session, when it qualifies: no thumbs down anywhere, and either a thumbs up
/// (`feedback`) or a last run that succeeded (`succeeded`).
fn session_example(r: &TraceRecord, feedback: bool, succeeded: bool) -> Option<Value> {
    if r.feedback.iter().any(|f| f.rating < 0) {
        return None;
    }
    let liked = feedback && r.feedback.iter().any(|f| f.rating > 0);
    let worked = succeeded && r.last_status.as_deref() == Some("succeeded");
    if !(liked || worked) {
        return None;
    }
    chat_example(&r.messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> Value {
        json!({"role": role, "content": content})
    }

    #[test]
    fn a_conversation_ends_with_the_final_answer_and_tool_arguments_are_objects() {
        let messages = vec![
            msg("user", "crea nota.md"),
            json!({"role": "assistant", "content": "", "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "write_file", "arguments": "{\"path\":\"nota.md\"}"}}
            ]}),
            json!({"role": "tool", "content": "ok", "tool_call_id": "c1", "name": "write_file"}),
            json!({"role": "assistant", "content": "Fatto", "name": "x"}),
            msg("user", "grazie"),
        ];
        let ex = chat_example(&messages).unwrap();
        let m = ex["messages"].as_array().unwrap();
        assert_eq!(m.len(), 4, "the trailing user message is dropped");
        assert_eq!(m[1]["tool_calls"][0]["function"]["arguments"], json!({"path": "nota.md"}));
        assert_eq!(m[2]["name"], "write_file");
        assert!(m[3].get("name").is_none(), "only tool messages keep a name");
        // Nothing to learn from: no answer, an empty answer, no user message.
        assert!(chat_example(&[msg("user", "ciao")]).is_none());
        assert!(chat_example(&[msg("user", "ciao"), msg("assistant", "  ")]).is_none());
        assert!(chat_example(&[msg("assistant", "ciao")]).is_none());
    }

    #[test]
    fn the_split_is_ninety_ten_and_does_not_depend_on_the_order_of_the_examples() {
        let make = |n: usize| -> Vec<Example> {
            (0..n).map(|i| Example { key: format!("k{i}"), value: json!({"i": i}), agent: String::new() }).collect()
        };
        let (train, valid) = split(make(20));
        assert_eq!((train.len(), valid.len()), (18, 2));
        let mut reversed = make(20);
        reversed.reverse();
        let (_, valid2) = split(reversed);
        let keys = |v: &[Example]| v.iter().map(|e| e.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&valid), keys(&valid2));
        // Always at least one example to validate with.
        let (train, valid) = split(make(10));
        assert_eq!((train.len(), valid.len()), (9, 1));
    }

    #[test]
    fn final_text_is_the_last_message_of_the_run() {
        use aiw_core::events::{SeqEvent, TimedEvent};
        let ev = |seq, id: &str, delta: &str| SeqEvent {
            seq,
            event: TimedEvent::now(AgEvent::TextMessageContent { message_id: id.into(), delta: delta.into() }),
        };
        let events = vec![ev(0, "m1", "Cerco"), ev(1, "m2", "Ecco "), ev(2, "m2", "fatto")];
        assert_eq!(final_text(&events), "Ecco fatto");
        assert_eq!(final_text(&[]), "");
    }

    #[test]
    fn pairs_depend_on_their_content_and_user_messages_are_normalized() {
        let prompt = vec![msg("user", "ciao")];
        let a = pair_example("x", &prompt, "si", "no", "");
        assert_eq!(a.key, pair_example("x", &prompt, "si", "no", "").key);
        assert_ne!(a.key, pair_example("x", &prompt, "no", "si", "").key);
        assert_eq!(a.value["chosen"], "si");
        assert_eq!(normalize("  Che   ORA è? "), "che ora è?");
    }
}
