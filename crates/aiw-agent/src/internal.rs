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

/// Name of the tool that hands the conversation over to another agent.
pub const HANDOFF: &str = "handoff";

/// Name of the tool that reads and writes the state shared by the agents of a team run.
pub const TEAM_STATE: &str = "team_state";

/// Names of the internal tools.
pub const NAMES: [&str; 7] = ["todo", "agent", "skill", crate::a2ui::TOOL, crate::code::TOOL, HANDOFF, TEAM_STATE];

/// Is `name` an internal tool?
pub fn is_internal(name: &str) -> bool {
    NAMES.contains(&name)
}

/// The live `handoff` spec of a run: `agents` is `(id, description)` of the possible targets.
pub fn handoff_spec(agents: &[(String, String)]) -> aiw_runtimes::ToolSpec {
    let names: Vec<Value> = agents.iter().map(|(id, _)| Value::String(id.clone())).collect();
    let desc: Vec<String> = agents.iter().map(|(id, d)| format!("{id}: {d}")).collect();
    aiw_runtimes::ToolSpec::function(
        HANDOFF,
        format!(
            "Passa la conversazione a un altro agente quando il resto del lavoro è di sua competenza: il tuo run termina e continua lui, con il riassunto che scrivi. Agenti: {}",
            desc.join("; ")
        ),
        json!({"type":"object","properties":{"agent":{"type":"string","enum":names},"summary":{"type":"string","description":"Cosa è stato fatto e cosa deve fare l'agente successivo"}},"required":["agent","summary"]}),
    )
}

/// The `team_state` spec (the same in every run).
pub fn team_state_spec() -> aiw_runtimes::ToolSpec {
    aiw_runtimes::ToolSpec::function(
        TEAM_STATE,
        "Stato condiviso del team: tutti gli agenti di questo lavoro leggono e scrivono le stesse chiavi (get senza chiave restituisce tutto, set sostituisce, append aggiunge a una lista). Usalo per scambiare risultati e decisioni senza ripeterli nei compiti.",
        json!({"type":"object","properties":{"op":{"type":"string","enum":["get","set","append"]},"key":{"type":"string"},"value":{"description":"Qualsiasi valore JSON (set e append)"}},"required":["op"]}),
    )
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
        InternalTool {
            name: HANDOFF,
            description: "Passa la conversazione a un altro agente: il run termina e il nuovo agente prosegue con il tuo riassunto.",
            parameters: parameters_of(handoff_spec(&[])),
        },
        InternalTool {
            name: TEAM_STATE,
            description: "Legge e scrive lo stato condiviso tra gli agenti di un team (solo nei run di team).",
            parameters: parameters_of(team_state_spec()),
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

    #[tokio::test]
    async fn handoff_is_offered_only_to_a_top_level_agent_that_lists_it() {
        use crate::tests_support::{input, setup};
        let offered = |tools: &[&str], depth: u32| {
            let tools: Vec<String> = tools.iter().map(|t| t.to_string()).collect();
            async move {
                let (host, dir) = setup(vec![aiw_runtimes::ScriptedBackend::text_turn("ok")], true);
                let mut inp = input(dir.path(), "auto");
                inp.agent.tools = tools;
                inp.depth = depth;
                crate::AgentRunner::run(inp, host.clone()).await.unwrap();
                let reqs = host.backend.requests.lock().unwrap();
                reqs[0].tools.iter().any(|t| t.function.name == HANDOFF)
            }
        };
        assert!(offered(&["*"], 0).await);
        assert!(offered(&["handoff"], 0).await);
        assert!(!offered(&["read_file"], 0).await);
        assert!(!offered(&["*"], 1).await, "a sub-agent cannot hand the conversation over");
    }
}
