//! Text tool protocol for in-process engines without native function calling (llama.cpp
//! embedded, ONNX Runtime GenAI): tools are described in the system message, calls travel as
//! `<tool_call>{"name": …, "arguments": …}</tool_call>` blocks (parsed by the agent loop) and
//! results come back as `<tool_response>` user turns.

use crate::types::ChatRequest;

/// The conversation as `(role, content)` turns ready for a chat template.
pub fn chat_turns(req: &ChatRequest) -> Vec<(String, String)> {
    let tools = if req.tools.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = req
            .tools
            .iter()
            .map(|t| format!("- {}: {} (argomenti: {})", t.function.name, t.function.description, t.function.parameters))
            .collect();
        format!(
            "\n\n# Strumenti\nPer usare uno strumento rispondi SOLO con un blocco <tool_call>{{\"name\": \"nome\", \"arguments\": {{...}}}}</tool_call>.\n{}",
            list.join("\n")
        )
    };
    let mut turns: Vec<(String, String)> = Vec::new();
    let mut system_done = false;
    for m in &req.messages {
        let text = m.text();
        match m.role.as_str() {
            "system" => {
                turns.push(("system".into(), format!("{text}{tools}")));
                system_done = true;
            }
            "assistant" => {
                let calls = m
                    .tool_calls
                    .as_ref()
                    .map(|c| {
                        c.iter()
                            .map(|t| {
                                format!(
                                    "<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>",
                                    t.function.name, t.function.arguments
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                turns.push(("assistant".into(), format!("{text}{calls}")));
            }
            "tool" => turns.push((
                "user".into(),
                format!("<tool_response name=\"{}\">\n{text}\n</tool_response>", m.name.clone().unwrap_or_default()),
            )),
            _ => turns.push(("user".into(), text)),
        }
    }
    if !system_done && !tools.is_empty() {
        turns.insert(0, ("system".into(), tools.trim_start().to_string()));
    }
    turns
}

/// ChatML rendering, for models that ship no chat template.
pub fn chatml(turns: &[(String, String)]) -> String {
    let mut s = String::new();
    for (role, content) in turns {
        s.push_str(&format!("<|im_start|>{role}\n{content}<|im_end|>\n"));
    }
    s.push_str("<|im_start|>assistant\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Msg, ToolSpec};

    #[test]
    fn tools_go_in_the_system_turn_and_results_are_user_turns() {
        let mut tool_msg = Msg::tool("c1", "list_dir", "a.txt");
        tool_msg.name = Some("list_dir".into());
        let req = ChatRequest {
            messages: vec![Msg::system("Sei utile."), Msg::user("Elenca"), tool_msg],
            tools: vec![ToolSpec::function("list_dir", "Elenca una cartella", serde_json::json!({"type": "object"}))],
            ..Default::default()
        };
        let t = chat_turns(&req);
        assert_eq!(t[0].0, "system");
        assert!(t[0].1.contains("<tool_call>") && t[0].1.contains("list_dir"));
        assert_eq!(t[2].0, "user");
        assert!(t[2].1.starts_with("<tool_response name=\"list_dir\">"));
        assert!(chatml(&t).ends_with("<|im_start|>assistant\n"));
    }
}
