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

/// The long-term memory tools (handled by the host: the memory lives in the engine's database).
pub const MEMORY_SAVE: &str = "memory_save";
pub const MEMORY_SEARCH: &str = "memory_search";
pub const MEMORY_UPDATE: &str = "memory_update";
pub const MEMORY_FORGET: &str = "memory_forget";
/// The four memory tools.
pub const MEMORY_TOOLS: [&str; 4] = [MEMORY_SAVE, MEMORY_SEARCH, MEMORY_UPDATE, MEMORY_FORGET];

/// Is `name` one of the memory tools?
pub fn is_memory_tool(name: &str) -> bool {
    MEMORY_TOOLS.contains(&name)
}

/// Names of the internal tools.
pub const NAMES: [&str; 11] = [
    "todo",
    "agent",
    "skill",
    crate::a2ui::TOOL,
    crate::code::TOOL,
    HANDOFF,
    TEAM_STATE,
    MEMORY_SAVE,
    MEMORY_SEARCH,
    MEMORY_UPDATE,
    MEMORY_FORGET,
];

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

const KINDS: [&str; 5] = ["about_you", "preference", "how_to_work", "fact", "reminder"];

/// The specs of the memory tools. The rule about `explicit` is part of the description because it
/// is what keeps the assistant from filling the person's memory with guesses.
pub fn memory_specs() -> Vec<aiw_runtimes::ToolSpec> {
    use aiw_runtimes::ToolSpec;
    vec![
        ToolSpec::function(
            MEMORY_SAVE,
            "Salva un ricordo a lungo termine sull'utente (un solo fatto per chiamata, una frase).              REGOLA: metti explicit=true solo se l'utente ha chiesto di ricordarlo o lo ha detto apertamente come informazione da tenere:              viene salvato subito (l'utente può sempre annullare). Se l'hai solo dedotto tu, metti explicit=false:              diventa una proposta che l'utente deve confermare. Non salvare segreti, password o dati che l'utente non ha detto.",
            json!({"type":"object","properties":{
                "text":{"type":"string","description":"Il ricordo, in una frase chiara, scritta in terza persona o neutra"},
                "kind":{"type":"string","enum":KINDS,"description":"about_you = chi è; preference = cosa gli piace; how_to_work = come vuole che tu lavori; fact = un fatto utile; reminder = una cosa da non dimenticare"},
                "explicit":{"type":"boolean","description":"true = l'utente ha chiesto di ricordarlo; false = l'hai dedotto tu (resta una proposta)"},
                "pinned":{"type":"boolean","description":"true = tienilo sempre presente in ogni conversazione (solo per ciò che serve sempre)"},
                "scope":{"type":"string","enum":["all","this_agent"],"description":"all = per ogni assistente (predefinito); this_agent = solo per te"},
                "due_at":{"type":"string","description":"Solo per i promemoria: data e ora RFC 3339"}
            },"required":["text","kind","explicit"]}),
        ),
        ToolSpec::function(
            MEMORY_SEARCH,
            "Cerca tra i ricordi dell'utente (per parole e per significato). Usalo per rispondere a «cosa sai di me?» o prima di salvare qualcosa che potresti già sapere. Restituisce id e testo.",
            json!({"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":20}},"required":["query"]}),
        ),
        ToolSpec::function(
            MEMORY_UPDATE,
            "Corregge un ricordo esistente (per id, da memory_search): nuovo testo, tipo, se tenerlo sempre presente o la data di un promemoria. Usalo quando l'utente corregge qualcosa invece di salvarne una seconda versione.",
            json!({"type":"object","properties":{"id":{"type":"string"},"text":{"type":"string"},"kind":{"type":"string","enum":KINDS},"pinned":{"type":"boolean"},"due_at":{"type":"string"}},"required":["id"]}),
        ),
        ToolSpec::function(
            MEMORY_FORGET,
            "Dimentica un ricordo (per id, da memory_search) quando l'utente lo chiede. Non lo cancella per sempre: l'utente può ripristinarlo.",
            json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}),
        ),
    ]
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
    .into_iter()
    .chain(memory_specs().into_iter().map(|s| InternalTool {
        name: MEMORY_TOOLS.iter().find(|n| **n == s.function.name).copied().expect("memory tool name"),
        description: match s.function.name.as_str() {
            MEMORY_SAVE => "Salva un ricordo a lungo termine: subito se l'utente lo ha chiesto, come proposta da confermare se dedotto.",
            MEMORY_SEARCH => "Cerca tra i ricordi dell'utente per parole e per significato.",
            MEMORY_UPDATE => "Corregge un ricordo esistente.",
            _ => "Dimentica un ricordo (l'utente può ripristinarlo).",
        },
        parameters: s.function.parameters,
    }))
    .collect()
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
