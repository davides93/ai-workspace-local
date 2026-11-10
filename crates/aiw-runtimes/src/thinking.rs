//! Reasoning ("thinking") on and off, per request. The runtime manifest says *how* a runtime takes
//! the switch ([`ThinkingStyle`]); adapters only apply that style, they never look at runtime ids.
//!
//! What each style sends, and where the names come from:
//!
//! | style | request | source |
//! |---|---|---|
//! | `template-kwargs` | `chat_template_kwargs: {"enable_thinking": bool}`, plus a token budget field when the manifest names one | llama.cpp server README (`chat_template_kwargs`, bundled with `llama-cpp-sys-2` 0.1.160); vLLM "Reasoning Outputs" docs. The per-request budget `reasoning_budget_tokens` is from llama.cpp b9982 onwards (a third-party release note, not checked against source): older servers ignore the unknown field |
//! | `reasoning-effort` | `reasoning_effort`: `none` / `medium` / `high` | Ollama's `/v1/chat/completions` (maps it to its `think`; the native `think` field is not read there, ollama/ollama#14820) |
//! | `prompt-suffix` | ` /think` or ` /no_think` appended to the last user message, only for Qwen3 models with the soft switch | Qwen3 model card |
//! | `none` | nothing | system models and every runtime whose switch is not documented |

use crate::types::{ContentPart, Msg, MsgContent, ThinkingRequest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThinkingStyle {
    #[default]
    None,
    TemplateKwargs,
    ReasoningEffort,
    PromptSuffix,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingControl {
    #[serde(default)]
    pub style: ThinkingStyle,
    /// Name of the request field that carries the reasoning token budget (`template-kwargs` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_field: Option<String>,
}

/// Puts the reasoning switch in the JSON body of an OpenAI-style chat request.
pub fn apply_to_body(ctl: &ThinkingControl, t: &ThinkingRequest, body: &mut Value) {
    match ctl.style {
        ThinkingStyle::None | ThinkingStyle::PromptSuffix => {}
        ThinkingStyle::TemplateKwargs => {
            body["chat_template_kwargs"] = json!({"enable_thinking": t.enabled});
            if let (true, Some(field), Some(budget)) = (t.enabled, ctl.budget_field.as_deref(), t.budget) {
                body[field] = json!(budget);
            }
        }
        ThinkingStyle::ReasoningEffort => {
            body["reasoning_effort"] = json!(if !t.enabled {
                "none"
            } else if t.deep {
                "high"
            } else {
                "medium"
            });
        }
    }
}

/// Qwen3 models with the `/think` and `/no_think` soft switch: the first hybrid generation.
/// The 2507 releases, Qwen3.5, Next, VL and Coder are either always on or always off and ignore it.
pub fn has_soft_switch(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("qwen3") && !["2507", "qwen3.5", "qwen3-next", "coder", "-vl", "embedding", "reranker"].iter().any(|x| m.contains(x))
}

/// Appends ` /think` or ` /no_think` to the last user message (Qwen3 follows the most recent one).
/// Does nothing for models without the soft switch.
pub fn apply_prompt_suffix(model: &str, t: &ThinkingRequest, messages: &mut [Msg]) {
    if !has_soft_switch(model) {
        return;
    }
    let Some(m) = messages.iter_mut().rev().find(|m| m.role == "user") else { return };
    let suffix = if t.enabled { " /think" } else { " /no_think" };
    match &mut m.content {
        Some(MsgContent::Text(text)) => text.push_str(suffix),
        Some(MsgContent::Parts(parts)) => {
            match parts.iter_mut().rev().find_map(|p| if let ContentPart::Text { text } = p { Some(text) } else { None }) {
                Some(text) => text.push_str(suffix),
                None => parts.push(ContentPart::Text { text: suffix.trim_start().to_string() }),
            }
        }
        None => m.content = Some(MsgContent::Text(suffix.trim_start().to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: ThinkingRequest = ThinkingRequest { enabled: true, deep: false, budget: Some(2048) };
    const OFF: ThinkingRequest = ThinkingRequest { enabled: false, deep: false, budget: None };

    fn kwargs(budget_field: Option<&str>) -> ThinkingControl {
        ThinkingControl { style: ThinkingStyle::TemplateKwargs, budget_field: budget_field.map(String::from) }
    }

    #[test]
    fn template_kwargs_switch_the_template_and_carry_the_budget_only_when_on() {
        let mut b = json!({"model": "m"});
        apply_to_body(&kwargs(Some("reasoning_budget_tokens")), &ON, &mut b);
        assert_eq!(b["chat_template_kwargs"], json!({"enable_thinking": true}));
        assert_eq!(b["reasoning_budget_tokens"], 2048);
        let mut b = json!({});
        apply_to_body(&kwargs(Some("reasoning_budget_tokens")), &OFF, &mut b);
        assert_eq!(b, json!({"chat_template_kwargs": {"enable_thinking": false}}));
        let mut b = json!({});
        apply_to_body(&kwargs(None), &ON, &mut b);
        assert!(b.get("reasoning_budget_tokens").is_none());
    }

    #[test]
    fn effort_style_maps_off_on_and_deep() {
        let ctl = ThinkingControl { style: ThinkingStyle::ReasoningEffort, budget_field: None };
        let effort = |t: &ThinkingRequest| {
            let mut b = json!({});
            apply_to_body(&ctl, t, &mut b);
            b["reasoning_effort"].clone()
        };
        assert_eq!(effort(&OFF), "none");
        assert_eq!(effort(&ON), "medium");
        assert_eq!(effort(&ThinkingRequest { deep: true, ..ON }), "high");
    }

    #[test]
    fn styles_without_a_body_field_leave_the_body_alone() {
        for style in [ThinkingStyle::None, ThinkingStyle::PromptSuffix] {
            let mut b = json!({"model": "m"});
            apply_to_body(&ThinkingControl { style, budget_field: None }, &ON, &mut b);
            assert_eq!(b, json!({"model": "m"}));
        }
    }

    #[test]
    fn only_hybrid_qwen3_has_the_soft_switch() {
        for yes in ["qwen3-4b-q4_k_m", "Qwen3-8B", "unsloth/Qwen3-14B-GGUF", "qwen3:4b"] {
            assert!(has_soft_switch(yes), "{yes}");
        }
        for no in [
            "qwen3-4b-instruct-2507",
            "qwen3.5-9b",
            "qwen3-coder-30b",
            "qwen3-next-80b",
            "qwen3-vl-4b",
            "llama-3.2-3b",
            "gemma-3-4b",
            "deepseek-r1:8b",
        ] {
            assert!(!has_soft_switch(no), "{no}");
        }
    }

    #[test]
    fn the_suffix_goes_on_the_last_user_message() {
        let mut msgs = vec![Msg::system("s"), Msg::user("uno"), Msg::assistant("a"), Msg::user("due"), Msg::tool("c", "t", "ok")];
        apply_prompt_suffix("qwen3-4b", &OFF, &mut msgs);
        assert_eq!(msgs[1].text(), "uno");
        assert_eq!(msgs[3].text(), "due /no_think");
        assert_eq!(msgs[4].text(), "ok");
        let mut on = vec![Msg::user("tre")];
        apply_prompt_suffix("qwen3-4b", &ON, &mut on);
        assert_eq!(on[0].text(), "tre /think");
    }

    #[test]
    fn the_suffix_leaves_other_models_and_image_parts_alone() {
        let mut msgs = vec![Msg::user("ciao")];
        apply_prompt_suffix("llama-3.2-3b", &OFF, &mut msgs);
        assert_eq!(msgs[0].text(), "ciao");
        let mut with_image = Msg::user("");
        with_image.content = Some(MsgContent::Parts(vec![
            ContentPart::Text { text: "guarda".into() },
            ContentPart::ImageUrl { image_url: crate::types::ImageUrl { url: "data:x".into() } },
        ]));
        let mut msgs = vec![with_image];
        apply_prompt_suffix("qwen3-4b", &OFF, &mut msgs);
        assert_eq!(msgs[0].text(), "guarda /no_think");
    }
}
