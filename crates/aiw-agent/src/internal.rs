//! The tools the agent loop implements itself (no entry in the tool registry): the plan, the
//! delegation to a sub-agent, the skills, the A2UI surfaces and code mode. An agent profile lists
//! them by name next to the registry tools, so the API and the UI must know them.

use serde_json::{json, Value};

/// Description of one internal tool, for listings (the live schema the model sees is built per
/// run by the runner: it depends on the available sub-agents and skills).
#[derive(Debug, Clone, PartialEq)]
pub struct InternalTool {
    pub name: &'static str,
    /// Italian, one sentence.
    pub description: &'static str,
    /// JSON Schema of the arguments.
    pub parameters: Value,
}

/// Names of the internal tools.
pub const NAMES: [&str; 5] = ["todo", "agent", "skill", crate::a2ui::TOOL, crate::code::TOOL];

/// Is `name` an internal tool?
pub fn is_internal(name: &str) -> bool {
    NAMES.contains(&name)
}

fn parameters_of(spec: aiw_runtimes::ToolSpec) -> Value {
    spec.function.parameters
}

/// Every internal tool, in the order the agent editor shows them.
pub fn internal_tools() -> Vec<InternalTool> {
    vec![
        InternalTool {
            name: "todo",
            description: "Pubblica e aggiorna il piano di lavoro dell'agente: elenco di passi con il loro stato.",
            parameters: json!({"type": "object", "properties": {"todos": {"type": "array", "items": {"type": "object", "properties": {"content": {"type": "string"}, "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]}}, "required": ["content", "status"]}}}, "required": ["todos"]}),
        },
        InternalTool {
            name: "agent",
            description: "Delega un sotto-compito a un altro agente con contesto separato e ne restituisce la risposta finale.",
            parameters: json!({"type": "object", "properties": {"agent": {"type": "string"}, "task": {"type": "string", "description": "Istruzioni complete e autosufficienti"}}, "required": ["agent", "task"]}),
        },
        InternalTool {
            name: "skill",
            description: "Carica le istruzioni complete di una skill elencata nel prompt di sistema.",
            parameters: json!({"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}),
        },
        InternalTool {
            name: crate::a2ui::TOOL,
            description: "Mostra all'utente un'interfaccia interattiva (A2UI): moduli, scelte, riepiloghi, tabelle di card.",
            parameters: parameters_of(crate::a2ui::spec()),
        },
        InternalTool {
            name: crate::code::TOOL,
            description: "Esegue uno script Rhai che orchestra più strumenti in un solo passo (cicli, filtri, conteggi).",
            parameters: parameters_of(crate::code::spec()),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listing_covers_every_internal_name_with_a_schema() {
        let tools = internal_tools();
        assert_eq!(tools.iter().map(|t| t.name).collect::<Vec<_>>(), NAMES);
        for t in &tools {
            assert!(is_internal(t.name));
            assert!(!t.description.is_empty() && t.parameters["type"] == "object", "{}", t.name);
        }
        assert!(!is_internal("shell") && !is_internal("mcp__github__list_issues"));
    }
}
