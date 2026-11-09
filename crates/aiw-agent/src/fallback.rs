//! Tool calls written as text by models/runtimes without native function calling
//! (Hermes/Qwen `<tool_call>{json}</tool_call>` blocks).

use serde_json::Value;

/// Returns the text without the blocks and the parsed calls `(name, arguments)`.
///
/// Small local models are sloppy with the protocol: the closing tag may be missing (generation
/// stopped right after the JSON) or the JSON may lack its last braces. Both are accepted; blocks
/// that still cannot be parsed are left in the text.
pub fn extract_text_tool_calls(text: &str) -> (String, Vec<(String, Value)>) {
    const OPEN: &str = "<tool_call>";
    const CLOSE: &str = "</tool_call>";
    let normalised = normalise_markers(text);
    let text = normalised.as_str();
    let mut calls = Vec::new();
    let mut cleaned = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        cleaned.push_str(&rest[..start]);
        let after = &rest[start + OPEN.len()..];
        // A block ends at its closing tag, or (tag missing) where the next block starts.
        let (body, next) = match (after.find(CLOSE), after.find(OPEN)) {
            (Some(end), Some(n)) if n < end => (&after[..n], &after[n..]),
            (Some(end), _) => (&after[..end], &after[end + CLOSE.len()..]),
            (None, Some(n)) => (&after[..n], &after[n..]),
            (None, None) => (after, ""),
        };
        match parse_call(body) {
            Some(call) => calls.push(call),
            None => cleaned.push_str(&rest[start..rest.len() - next.len()]),
        }
        rest = next;
    }
    cleaned.push_str(rest);
    (cleaned.trim().to_string(), calls)
}

/// Variants of the call tags that small models (Apple Foundation Models writes `{tool_call}`)
/// emit instead of `<tool_call>`; rewritten to the canonical tags.
const OPEN_VARIANTS: &[&str] = &["{tool_call}", "[tool_call]", "<|tool_call|>", "<tool_call >", "{{tool_call}}", "<toolcall>"];
const CLOSE_VARIANTS: &[&str] = &["{/tool_call}", "[/tool_call]", "<|/tool_call|>", "{{/tool_call}}", "</toolcall>"];

fn normalise_markers(text: &str) -> String {
    let mut out = text.to_string();
    for v in OPEN_VARIANTS {
        out = out.replace(v, "<tool_call>");
    }
    for v in CLOSE_VARIANTS {
        out = out.replace(v, "</tool_call>");
    }
    out
}

/// True when `text` may hold a tool call written as text (tagged, or a bare JSON call).
pub fn has_text_tool_call(text: &str) -> bool {
    normalise_markers(text).contains("<tool_call>") || bare_json_call(text).is_some()
}

/// Whole answer is just a JSON call: `{"name": .., "arguments": ..}`, or an A2UI surface
/// (`surfaceId` + `components`) that the model wrote instead of calling `render_ui`.
pub fn bare_json_call(text: &str) -> Option<(String, Value)> {
    let t = text.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).map(|s| s.trim_end().trim_end_matches("```").trim()).unwrap_or(t);
    if !t.starts_with('{') {
        return None;
    }
    let v = first_json_value(t)?;
    if v.get("surfaceId").is_some() && v.get("components").is_some_and(Value::is_array) {
        return Some(("render_ui".to_string(), v));
    }
    if v.get("name").is_some_and(Value::is_string) && (v.get("arguments").is_some() || v.get("parameters").is_some()) {
        return parse_call(t);
    }
    None
}

fn parse_call(body: &str) -> Option<(String, Value)> {
    let v = first_json_value(&body[body.find('{')?..])?;
    let name = v.get("name")?.as_str()?.to_string();
    let args = v.get("arguments").or_else(|| v.get("parameters")).cloned().unwrap_or(Value::Object(Default::default()));
    let args = match args {
        Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
        other => other,
    };
    Some((name, args))
}

/// First JSON value of `s` (trailing text ignored); closes up to three objects left open.
fn first_json_value(s: &str) -> Option<Value> {
    match serde_json::Deserializer::from_str(s).into_iter::<Value>().next()? {
        Ok(v) => Some(v),
        Err(e) if e.is_eof() => (1..=3).find_map(|n| serde_json::from_str(&format!("{}{}", s.trim_end(), "}".repeat(n))).ok()),
        Err(_) => None,
    }
}

/// Instructions appended to the system prompt when the runtime lacks native tool calling.
pub fn protocol_prompt(tools: &[aiw_runtimes::ToolSpec]) -> String {
    let list: Vec<String> =
        tools.iter().map(|t| format!("- {}: {} — argomenti: {}", t.function.name, t.function.description, t.function.parameters)).collect();
    format!(
        "Per usare uno strumento rispondi SOLO con un blocco:\n<tool_call>{{\"name\": \"nome_tool\", \"arguments\": {{...}}}}</tool_call>\nStrumenti disponibili:\n{}",
        list.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_hermes_style_calls() {
        let (t, calls) =
            extract_text_tool_calls("Controllo.\n<tool_call>{\"name\": \"list_dir\", \"arguments\": {\"path\": \".\"}}</tool_call>");
        assert_eq!(t, "Controllo.");
        assert_eq!(calls, vec![("list_dir".to_string(), serde_json::json!({"path": "."}))]);
    }

    #[test]
    fn tolerates_missing_closing_tag_and_braces() {
        let (t, calls) =
            extract_text_tool_calls("<tool_call>{\"name\": \"write_file\", \"arguments\": {\"path\": \"a.md\", \"content\": \"x\"}}");
        assert_eq!(t, "");
        assert_eq!(calls, vec![("write_file".to_string(), serde_json::json!({"path": "a.md", "content": "x"}))]);

        let (_, calls) = extract_text_tool_calls(
            "<tool_call>\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \".\"}\n<tool_call>{\"name\": \"glob\", \"arguments\": \"{\\\"pattern\\\": \\\"*.md\\\"}\"}</tool_call>",
        );
        assert_eq!(calls.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), vec!["list_dir", "glob"]);
        assert_eq!(calls[1].1, serde_json::json!({"pattern": "*.md"}));

        // Variants seen on Apple Foundation Models.
        let (t, calls) = extract_text_tool_calls("{tool_call}{\"name\": \"browser_navigate\", \"arguments\": {\"url\": \"https://a.it\"}}");
        assert_eq!(t, "");
        assert_eq!(calls, vec![("browser_navigate".to_string(), serde_json::json!({"url": "https://a.it"}))]);
        let ui = bare_json_call("{\"surfaceId\":\"s\",\"components\":[{\"id\":\"root\",\"component\":\"Text\",\"text\":\"x\"}]}").unwrap();
        assert_eq!(ui.0, "render_ui");
        assert!(bare_json_call("Ciao {\"a\": 1}").is_none());
        assert_eq!(bare_json_call("{\"name\":\"glob\",\"arguments\":{}}").unwrap().0, "glob");

        // Not a call: left as text.
        let (t, calls) = extract_text_tool_calls("Uso il tag <tool_call> per chiamare gli strumenti.");
        assert!(calls.is_empty());
        assert!(t.contains("<tool_call>"));
    }
}
