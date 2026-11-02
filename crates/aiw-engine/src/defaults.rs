//! First-run seed data: agent presets, the default inference profile and an example workflow.

use aiw_core::api::{AgentProfile, InferenceParams, InferenceProfile, LocalModel, WorkflowDef};
use serde_json::json;

pub fn agents() -> Vec<AgentProfile> {
    let a = |id: &str, name: &str, desc: &str, prompt: &str, tools: &[&str], mode: &str, sandbox: &str, turns: u32| AgentProfile {
        id: id.into(),
        name: name.into(),
        description: desc.into(),
        inference_profile: "default".into(),
        system_prompt: prompt.into(),
        tools: tools.iter().map(|s| s.to_string()).collect(),
        mode: mode.into(),
        sandbox: sandbox.into(),
        max_turns: turns,
    };
    vec![
        a(
            "operator",
            "Operatore",
            "Task operativi su file, cartelle, documenti e comandi.",
            "Sei un operatore preciso che lavora sul computer dell'utente. Pianifica i passi, verifica con gli strumenti, chiedi conferma prima delle azioni distruttive e alla fine riepiloga cosa hai fatto davvero.",
            &["read_file", "write_file", "edit_file", "move_file", "list_dir", "glob", "grep", "shell", "web_fetch", "todo", "agent", "skill", "render_ui", "code"],
            "ask",
            "workspace-write",
            30,
        ),
        a(
            "coder",
            "Coder",
            "Coding agent sul workspace corrente.",
            "Sei un ingegnere software esperto. Leggi il codice prima di modificarlo, fai modifiche minime e mirate, esegui test e linter per verificarle, e spiega i cambiamenti.",
            &["*"],
            "auto-edit",
            "workspace-write",
            60,
        ),
        a(
            "browser",
            "Navigatore",
            "Ricerca e azioni nel browser controllato (CDP).",
            "Usi il browser per trovare informazioni e completare moduli. Prima di inviare dati personali, pagamenti o moduli definitivi chiedi conferma. Cita le pagine da cui prendi le informazioni.",
            &["browser_navigate", "browser_snapshot", "browser_click", "browser_type", "browser_screenshot", "web_fetch", "todo"],
            "ask",
            "read-only",
            40,
        ),
        a(
            "desktop",
            "Pilota desktop",
            "Computer-use: vede lo schermo e usa mouse e tastiera.",
            "Controlli il desktop con screenshot e azioni di mouse e tastiera. Fai uno screenshot dopo ogni azione importante per verificare l'effetto. Non inviare, pagare o cancellare nulla senza conferma.",
            &["computer_screenshot", "computer_click", "computer_type", "computer_key", "computer_scroll", "todo"],
            "ask",
            "read-only",
            50,
        ),
        a(
            "planner",
            "Pianificatore",
            "Solo lettura: analizza e propone piani.",
            "Analizzi senza modificare nulla. Produci piani numerati, rischi e alternative.",
            &["read_file", "list_dir", "glob", "grep", "web_fetch", "todo", "skill", "render_ui", "code"],
            "plan",
            "read-only",
            20,
        ),
    ]
}

/// Agents seeded on first run can be edited but not deleted.
pub fn is_builtin_agent(id: &str) -> bool {
    agents().iter().any(|a| a.id == id)
}

/// Picks a sensible default profile from what is installed or running.
pub fn default_profile(models: &[LocalModel], running_ollama: Option<&str>, prefer_embedded: bool) -> InferenceProfile {
    let chat_models: Vec<&LocalModel> =
        models.iter().filter(|m| m.format == "gguf" && m.capabilities.iter().any(|c| c == "chat")).collect();
    // Prefer models that support tools, then the largest that is not huge.
    let best = chat_models
        .iter()
        .filter(|m| m.capabilities.iter().any(|c| c == "tools"))
        .max_by_key(|m| m.size_bytes)
        .or_else(|| chat_models.iter().max_by_key(|m| m.size_bytes));
    if let Some(m) = best {
        return InferenceProfile {
            id: "default".into(),
            name: "Predefinito".into(),
            description: Some(format!("{} su llama.cpp{}", m.name, if prefer_embedded { " (in-process)" } else { "" })),
            runtime_id: if prefer_embedded { "llama-embedded".into() } else { "llama-server".into() },
            model: m.id.clone(),
            params: InferenceParams {
                temperature: Some(0.6),
                top_p: Some(0.95),
                max_tokens: Some(4096),
                context: Some(m.context_length.map(|c| c.min(16384) as u32).unwrap_or(8192)),
            },
        };
    }
    if let Some(model) = running_ollama {
        return InferenceProfile {
            id: "default".into(),
            name: "Predefinito".into(),
            description: Some(format!("{model} su Ollama")),
            runtime_id: "ollama".into(),
            model: model.into(),
            params: InferenceParams { temperature: Some(0.6), max_tokens: Some(4096), context: Some(8192), ..Default::default() },
        };
    }
    InferenceProfile {
        id: "default".into(),
        name: "Predefinito".into(),
        description: Some("Scarica un modello da Modelli per iniziare".into()),
        runtime_id: "llama-server".into(),
        model: String::new(),
        params: InferenceParams { temperature: Some(0.6), max_tokens: Some(4096), context: Some(8192), ..Default::default() },
    }
}

pub fn workflows() -> Vec<WorkflowDef> {
    vec![serde_json::from_value(json!({
        "id": "riordino-download",
        "name": "Riordino della cartella Download",
        "description": "Classifica i file di una cartella e, dopo la tua conferma, li sposta in sottocartelle.",
        "enabled": false,
        "trigger": {"kind": "cron", "expr": "0 9 * * 1"},
        "inputs": {"folder": "Download"},
        "steps": [
            {"id": "scan", "kind": "tool", "tool": "list_dir", "args": {"path": "{{inputs.folder}}"}},
            {"id": "plan", "kind": "agent", "agent": "planner", "prompt": "Proponi una cartella di destinazione per ciascun file (per tipo o per anno). Elenco:\n{{steps.scan.output}}"},
            {"id": "confirm", "kind": "approval", "message": "Sposto i file come proposto?\n{{steps.plan.output}}"},
            {"id": "act", "kind": "agent", "agent": "operator", "prompt": "Sposta i file della cartella {{inputs.folder}} seguendo questo piano approvato, usando move_file:\n{{steps.plan.output}}"}
        ]
    }))
    .expect("static workflow")]
}
