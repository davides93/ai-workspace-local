//! Native tool calling and native conversation history for Apple Foundation Models.
//!
//! The agent loop speaks OpenAI-style messages and tool specs. Foundation Models wants tools as
//! `Tool` objects and the history as a `Transcript`. This module prepares both for the Swift side
//! (see `ios/Sources/AiwNativePlugin/AppleFMCore.swift`):
//!
//! * the conversation becomes `NativeTurn`s plus the message to answer;
//! * the tools are *selected* for the turn, because every schema costs context and the system
//!   model has a small window: the ones the request is about (and the ones already used in this
//!   exchange) go first, as many as fit a budget; each is compacted.
//!
//! Tools are never run by the model's side: the Swift side reports the calls the model decides to
//! make and the agent loop runs them, with its permissions and approvals.

use crate::{NativeCall, NativeTool, NativeTurn};
use aiw_runtimes::{ChatRequest, Msg, ToolSpec};
use serde_json::{Map, Value};
use std::collections::HashSet;

/// Estimated tokens the offered tools may take. The Swift side prunes further to the window it
/// actually has; this keeps the request small and puts the relevant tools first.
pub const TOOL_BUDGET_TOKENS: usize = 2400;

/// Asked of the model when the conversation ends with tool results instead of a user message.
pub const CONTINUE_PROMPT: &str = "Usa i risultati degli strumenti per rispondere.";

const GUIDANCE: &str = "Usa gli strumenti quando servono e rispondi con il loro esito. Non scrivere le chiamate come testo.";

/// Messages from the last `RECENT` are the exchange the model is in the middle of: the tools
/// they used stay available (a call whose tool is missing would be dropped from the history).
const RECENT: usize = 10;

pub struct NativePlan {
    pub system: String,
    /// What the model answers now.
    pub prompt: String,
    pub transcript: Vec<NativeTurn>,
    pub tools: Vec<NativeTool>,
}

pub fn plan(req: &ChatRequest, tool_budget: usize) -> NativePlan {
    let mut system = req.messages.iter().filter(|m| m.role == "system").map(Msg::text).collect::<Vec<_>>().join("\n\n");
    let rest: Vec<&Msg> = req.messages.iter().filter(|m| m.role != "system").collect();
    let tools = select_tools(&req.tools, &rest, tool_budget);
    if !tools.is_empty() {
        if !system.is_empty() {
            system.push_str("\n\n");
        }
        system.push_str(GUIDANCE);
    }

    // The conversation ends with the user's message (that is the prompt), or with tool results
    // (the model continues from them).
    let (history, prompt) = match rest.split_last() {
        Some((last, before)) if last.role == "user" => (before, last.text()),
        _ => (rest.as_slice(), CONTINUE_PROMPT.to_string()),
    };
    NativePlan { system, prompt, transcript: history.iter().filter_map(|m| turn(m)).collect(), tools }
}

fn turn(m: &Msg) -> Option<NativeTurn> {
    match m.role.as_str() {
        "user" => Some(NativeTurn::User { text: m.text() }),
        "assistant" => Some(NativeTurn::Assistant {
            text: m.text(),
            tool_calls: m
                .tool_calls
                .iter()
                .flatten()
                .map(|c| NativeCall { id: c.id.clone(), name: c.function.name.clone(), arguments: c.function.arguments.clone() })
                .collect(),
        }),
        "tool" => Some(NativeTurn::Tool {
            id: m.tool_call_id.clone().unwrap_or_default(),
            name: m.name.clone().unwrap_or_default(),
            text: m.text(),
        }),
        _ => None,
    }
}

/// The tools to offer for this request, most relevant first, within `budget` estimated tokens
/// (the first one is always kept).
pub fn select_tools(specs: &[ToolSpec], messages: &[&Msg], budget: usize) -> Vec<NativeTool> {
    if specs.is_empty() {
        return vec![];
    }
    let recent_start = messages.len().saturating_sub(RECENT);
    let used: HashSet<&str> =
        messages[recent_start..].iter().flat_map(|m| m.tool_calls.iter().flatten()).map(|c| c.function.name.as_str()).collect();
    let query: HashSet<String> = messages.iter().rev().find(|m| m.role == "user").map(|m| stems(&m.text())).unwrap_or_default();

    let mut ranked: Vec<(usize, usize, NativeTool)> = specs
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let tool = compact(s);
            let mut score = 0;
            if used.contains(s.function.name.as_str()) {
                score += 100;
            }
            let name_stems = stems(&s.function.name.replace(['_', '.', '-'], " "));
            let doc_stems = stems(&s.function.description);
            score += 3 * query.intersection(&name_stems).count() + query.intersection(&doc_stems).count();
            (score, i, tool)
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut out = Vec::new();
    let mut spent = 0;
    for (_, _, tool) in ranked {
        let cost = tool_tokens(&tool);
        if !out.is_empty() && spent + cost > budget {
            continue; // a smaller one further down may still fit
        }
        spent += cost;
        out.push(tool);
    }
    out
}

/// Lowercase word stems (first four letters of words of four or more) for rough matching of
/// Italian and English words that share a root (`apri`/`apre`, `naviga`/`navigate`).
fn stems(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4)
        .map(|w| w.chars().take(4).collect())
        .collect()
}

fn tool_tokens(t: &NativeTool) -> usize {
    16 + est_tokens(&t.name) + est_tokens(&t.description) + est_tokens(&t.parameters.to_string()) / 2
}

/// Same estimate as `afmTokens` on the Swift side: letters and spaces about a third of a token,
/// punctuation nearly one.
pub fn est_tokens(s: &str) -> usize {
    let (mut plain, mut punct) = (0usize, 0usize);
    for c in s.chars() {
        if c.is_alphanumeric() || c.is_whitespace() {
            plain += 1;
        } else {
            punct += 1;
        }
    }
    plain * 10 / 34 + punct * 9 / 10 + 1
}

fn compact(s: &ToolSpec) -> NativeTool {
    NativeTool {
        name: s.function.name.clone(),
        description: first_line(&s.function.description, 140),
        parameters: compact_schema(&s.function.parameters, 0),
    }
}

fn first_line(s: &str, max: usize) -> String {
    let line = s.trim().lines().next().unwrap_or("").trim();
    if line.chars().count() <= max {
        return line.to_string();
    }
    let cut: String = line.chars().take(max - 1).collect();
    format!("{}…", cut.trim_end())
}

/// A JSON Schema reduced to what the model needs to fill it: type, properties, items, enum,
/// required and short descriptions. Everything else (`title`, `default`, `format`, `examples`,
/// numeric limits…) only costs context.
fn compact_schema(v: &Value, depth: usize) -> Value {
    let Some(o) = v.as_object() else { return v.clone() };
    let mut out = Map::new();
    for key in ["type", "enum", "required"] {
        if let Some(x) = o.get(key) {
            out.insert(key.into(), x.clone());
        }
    }
    if let Some(d) = o.get("description").and_then(Value::as_str) {
        out.insert("description".into(), Value::String(first_line(d, if depth == 0 { 120 } else { 80 })));
    }
    if let Some(p) = o.get("properties").and_then(Value::as_object) {
        out.insert("properties".into(), Value::Object(p.iter().map(|(k, x)| (k.clone(), compact_schema(x, depth + 1))).collect()));
    }
    if let Some(i) = o.get("items") {
        out.insert("items".into(), compact_schema(i, depth + 1));
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(a) = o.get(key).and_then(Value::as_array) {
            out.insert(key.into(), Value::Array(a.iter().map(|x| compact_schema(x, depth + 1)).collect()));
        }
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiw_runtimes::{OaiFunctionCall, OaiToolCall};
    use serde_json::json;

    fn spec(name: &str, desc: &str) -> ToolSpec {
        ToolSpec::function(
            name,
            desc,
            json!({"type": "object", "title": "X", "properties": {"path": {"type": "string", "description": "Percorso del file.\nSeconda riga", "default": "."}},
                   "required": ["path"], "additionalProperties": false}),
        )
    }

    fn call(id: &str, name: &str, args: &str) -> Msg {
        Msg {
            role: "assistant".into(),
            tool_calls: Some(vec![OaiToolCall {
                id: id.into(),
                kind: "function".into(),
                function: OaiFunctionCall { name: name.into(), arguments: args.into() },
            }]),
            ..Default::default()
        }
    }

    #[test]
    fn user_message_is_the_prompt_and_history_is_the_transcript() {
        let req = ChatRequest {
            messages: vec![Msg::system("Sei utile."), Msg::user("ciao"), Msg::assistant("Ciao!"), Msg::user("elenca i file")],
            tools: vec![spec("list_dir", "Elenca una cartella")],
            ..Default::default()
        };
        let p = plan(&req, TOOL_BUDGET_TOKENS);
        assert_eq!(p.prompt, "elenca i file");
        assert_eq!(
            p.transcript,
            vec![NativeTurn::User { text: "ciao".into() }, NativeTurn::Assistant { text: "Ciao!".into(), tool_calls: vec![] }]
        );
        assert!(p.system.starts_with("Sei utile.") && p.system.contains("Non scrivere le chiamate come testo"));
        assert!(!p.system.contains("<tool_call>"), "the text protocol is not described to a model that has native tools");
        assert_eq!(p.tools.len(), 1);
    }

    #[test]
    fn tool_results_stay_in_the_transcript_and_the_model_continues_from_them() {
        let req = ChatRequest {
            messages: vec![Msg::user("elenca i file"), call("c1", "list_dir", "{\"path\": \".\"}"), Msg::tool("c1", "list_dir", "a.txt")],
            tools: vec![spec("list_dir", "Elenca una cartella")],
            ..Default::default()
        };
        let p = plan(&req, TOOL_BUDGET_TOKENS);
        assert_eq!(p.prompt, CONTINUE_PROMPT);
        assert_eq!(p.transcript.len(), 3);
        assert!(
            matches!(&p.transcript[1], NativeTurn::Assistant { tool_calls, .. } if tool_calls[0].id == "c1" && tool_calls[0].name == "list_dir")
        );
        assert!(matches!(&p.transcript[2], NativeTurn::Tool { id, name, text } if id == "c1" && name == "list_dir" && text == "a.txt"));
    }

    #[test]
    fn no_tools_means_no_guidance_and_no_tool_list() {
        let req = ChatRequest { messages: vec![Msg::system("Sei utile."), Msg::user("ciao")], ..Default::default() };
        let p = plan(&req, TOOL_BUDGET_TOKENS);
        assert_eq!(p.system, "Sei utile.");
        assert!(p.tools.is_empty() && p.transcript.is_empty());
    }

    #[test]
    fn schemas_are_compacted() {
        let t = compact(&spec("read_file", "Legge un file.\nDettagli lunghi che non servono"));
        assert_eq!(t.description, "Legge un file.");
        let p = &t.parameters;
        assert!(p.get("title").is_none() && p.get("additionalProperties").is_none());
        assert_eq!(p["properties"]["path"], json!({"type": "string", "description": "Percorso del file."}));
        assert_eq!(p["required"], json!(["path"]));
    }

    #[test]
    fn tools_that_match_the_request_come_first_and_the_budget_is_respected() {
        let specs = vec![
            spec("shell", "Esegue un comando di shell"),
            spec("browser_navigate", "Apre un indirizzo web nel browser"),
            spec("list_dir", "Elenca i file di una cartella"),
            spec("write_file", "Scrive un file"),
        ];
        let user = Msg::user("Apri il sito del comune nel browser");
        let t = select_tools(&specs, &[&user], TOOL_BUDGET_TOKENS);
        assert_eq!(t[0].name, "browser_navigate");
        assert_eq!(t.len(), 4, "everything fits the default budget");

        let one = select_tools(&specs, &[&user], 1);
        assert_eq!(one.len(), 1, "the best tool is always offered");
        assert_eq!(one[0].name, "browser_navigate");
    }

    #[test]
    fn tools_used_in_this_exchange_stay_available() {
        let specs = vec![spec("a_tool", "Prima"), spec("b_tool", "Seconda"), spec("write_file", "Scrive un file")];
        let msgs = [Msg::user("scrivi qualcosa"), call("c1", "b_tool", "{}"), Msg::tool("c1", "b_tool", "ok")];
        let refs: Vec<&Msg> = msgs.iter().collect();
        let t = select_tools(&specs, &refs, 1);
        assert_eq!(t[0].name, "b_tool", "a call in progress needs its tool even if the budget is tiny");
    }

    #[test]
    fn many_tools_are_cut_to_the_budget() {
        let specs: Vec<ToolSpec> = (0..60).map(|i| spec(&format!("tool_{i}"), "Fa qualcosa di utile con un file")).collect();
        let user = Msg::user("ciao");
        let t = select_tools(&specs, &[&user], TOOL_BUDGET_TOKENS);
        let cost: usize = t.iter().map(tool_tokens).sum();
        assert!(!t.is_empty() && t.len() < 60 && cost <= TOOL_BUDGET_TOKENS, "{} tools, {cost} tokens", t.len());
    }

    #[test]
    fn token_estimate_counts_punctuation_heavily() {
        assert!(est_tokens("{\"path\": \"src/main.rs\", \"limit\": 100}") > est_tokens("elenca i file nella cartella"));
        assert_eq!(est_tokens(""), 1);
    }
}
