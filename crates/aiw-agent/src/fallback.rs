//! Tool calls written as text by models/runtimes without native function calling
//! (Hermes/Qwen `<tool_call>{json}</tool_call>` blocks).

use serde_json::Value;

/// Returns the text without the blocks and the parsed calls `(name, arguments)`.
pub fn extract_text_tool_calls(text: &str) -> (String, Vec<(String, Value)>) {
    let re = regex::Regex::new(r"(?s)<tool_call>\s*(\{.*?\})\s*</tool_call>").unwrap();
    let mut calls = Vec::new();
    for c in re.captures_iter(text) {
        if let Ok(v) = serde_json::from_str::<Value>(&c[1]) {
            if let Some(name) = v.get("name").and_then(|n| n.as_str()) {
                let args = v.get("arguments").or_else(|| v.get("parameters")).cloned().unwrap_or(Value::Object(Default::default()));
                let args = match args {
                    Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
                    other => other,
                };
                calls.push((name.to_string(), args));
            }
        }
    }
    let cleaned = re.replace_all(text, "").trim().to_string();
    (cleaned, calls)
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
}
