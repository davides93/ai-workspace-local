//! First-run seed data: agent presets, the default inference profile and an example workflow.

use crate::adviser::{context_for, local_fit, Fit};
use aiw_core::api::{AgentProfile, DeviceProfile, InferenceParams, InferenceProfile, LocalModel, RuntimeInfo, WorkflowDef};
use serde_json::json;

/// Agents seeded on a fresh install. They follow the model adviser (`auto:agent`, the planner
/// `auto:reasoning`): they use what is installed now, so a model downloaded later is picked up
/// without editing any profile. Installs that already have these agents keep their profile.
pub fn agents() -> Vec<AgentProfile> {
    let a = |id: &str, name: &str, desc: &str, prompt: &str, tools: &[&str], mode: &str, sandbox: &str, turns: u32| AgentProfile {
        id: id.into(),
        name: name.into(),
        description: desc.into(),
        inference_profile: if id == "planner" { "auto:reasoning" } else { "auto:agent" }.into(),
        system_prompt: prompt.into(),
        tools: tools.iter().map(|s| s.to_string()).collect(),
        mode: mode.into(),
        sandbox: sandbox.into(),
        max_turns: turns,
        output_schema: None,
        budget: None,
        candidate_profiles: vec![],
        roles: None,
        knowledge: vec![],
    };
    vec![
        a(
            "operator",
            "Operatore",
            "Task operativi su file, cartelle, documenti e comandi.",
            "Sei un operatore preciso che lavora sul computer dell'utente. Pianifica i passi, verifica con gli strumenti, chiedi conferma prima delle azioni distruttive e alla fine riepiloga cosa hai fatto davvero.",
            &["read_file", "write_file", "edit_file", "move_file", "list_dir", "glob", "grep", "shell", "web_fetch", "web_search", "todo", "agent", "skill", "render_ui", "code", "memory_save", "memory_search", "memory_update", "memory_forget", "kb_search", "calendar_today", "calendar_upcoming", "mail_recent", "mail_search"],
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
            &["browser_navigate", "browser_snapshot", "browser_click", "browser_type", "browser_scroll", "browser_back", "browser_press", "browser_wait", "browser_screenshot", "browser_tabs", "browser_tab_new", "browser_tab_select", "browser_select", "browser_hover", "browser_downloads", "browser_console", "web_fetch", "web_search", "todo"],
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
            &["read_file", "list_dir", "glob", "grep", "web_fetch", "web_search", "todo", "skill", "render_ui", "code", "memory_search", "kb_search", "calendar_today", "calendar_upcoming", "mail_recent", "mail_search"],
            "plan",
            "read-only",
            20,
        ),
        a("teacher", "Insegnante", "Impara da ciò che gli racconti: salva, corregge e dimentica i ricordi.", TEACHER_PROMPT, TEACHER_TOOLS, "auto", "read-only", 12),
    ]
}

/// Agents added after the first release: an install that predates them gets them at start-up when
/// they are missing (the others are only seeded on an empty install and then belong to the user).
pub const LATE_AGENTS: &[&str] = &["teacher"];

/// The assistant of the "Insegna" page. Only the memory tools plus the two harmless internal ones:
/// it cannot touch files, the shell or the network, which is why it can run without asking.
pub const TEACHER_TOOLS: &[&str] = &["memory_save", "memory_search", "memory_update", "memory_forget", "render_ui", "todo"];

const TEACHER_PROMPT: &str = "Sei l'Insegnante: aiuti l'utente a insegnare all'assistente ciò che deve ricordare. Parli in modo semplice e breve, in italiano, senza termini tecnici.\n\
\n\
Come lavori:\n\
- Quando l'utente ti dice qualcosa su di sé, su come preferisce lavorare, un fatto utile o una cosa da non dimenticare, estrai UN ricordo per ogni fatto distinto e salvalo con memory_save (explicit=true, perché è l'utente a dirtelo) scegliendo il tipo giusto: about_you (chi è), preference (cosa gli piace), how_to_work (come vuole che lavori l'assistente), fact (un fatto utile), reminder (una cosa da non dimenticare, con la data in due_at se la dice). Scrivi ogni ricordo come una frase chiara e autosufficiente. Usa pinned=true solo per ciò che serve in ogni conversazione (per esempio il nome o una regola fissa di stile).\n\
- Prima di salvare, se potresti già saperlo, cerca con memory_search: se esiste già, non duplicarlo; se è cambiato, correggilo con memory_update.\n\
- Dopo aver salvato, conferma in una riga cosa hai ricordato (per esempio «Ricordato: preferisci risposte brevi.»). Non fare lunghi riepiloghi.\n\
- Se l'utente chiede «cosa sai di me?» (o simili), cerca con memory_search e rispondi con un elenco breve, diviso per argomento. Se non trovi nulla, dillo con sincerità.\n\
- Se l'utente corregge qualcosa, usa memory_update sul ricordo giusto. Se dice «dimentica…», usa memory_forget e conferma; ricordagli che può ripristinarlo dalla lista «Cosa so».\n\
- Non inventare mai fatti sull'utente e non dedurre ciò che non ha detto. Se una frase è ambigua (per esempio non è chiaro se vale sempre o solo oggi), fai UNA sola domanda breve prima di salvare.\n\
- Non salvare password, chiavi o dati di pagamento: spiega gentilmente che non vanno ricordati.\n\
- Se l'utente ti chiede altro (scrivere, cercare, lavorare su file), digli che per quello c'è un altro assistente e che qui si insegna soltanto.";

/// Agents seeded on first run can be edited but not deleted.
pub fn is_builtin_agent(id: &str) -> bool {
    agents().iter().any(|a| a.id == id)
}

/// What this device can run right now, as seen by the first-run seeding (derived from the
/// runtime registry, so the profile always points at something that is really registered).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Availability {
    /// `llama-server` is installed (status `available`) or already running.
    pub llama_server: bool,
    /// The in-process llama.cpp engine is compiled in and registered (status `running`).
    pub llama_embedded: bool,
    /// Models of a running Ollama server (empty: not running, or nothing installed).
    pub ollama_models: Vec<String>,
}

impl Availability {
    pub fn from_runtimes(list: &[RuntimeInfo]) -> Self {
        let find = |id: &str| list.iter().find(|r| r.id == id);
        Self {
            llama_server: find("llama-server").is_some_and(|r| matches!(r.status.as_str(), "available" | "running")),
            llama_embedded: find("llama-embedded").is_some_and(|r| r.status == "running"),
            ollama_models: find("ollama").filter(|r| r.status == "running").map(|r| r.loaded_models.clone()).unwrap_or_default(),
        }
    }

    /// Runtime that serves a local GGUF file: `llama-server` when present, else the embedded engine.
    pub fn gguf_runtime(&self) -> Option<&'static str> {
        if self.llama_server {
            Some("llama-server")
        } else if self.llama_embedded {
            Some("llama-embedded")
        } else {
            None
        }
    }

    /// First Ollama model that can chat (embedding-only models are skipped).
    fn ollama_chat_model(&self) -> Option<&str> {
        self.ollama_models.iter().map(String::as_str).find(|m| !looks_like_embedding_model(m))
    }
}

/// `nomic-embed-text:latest`, `bge-m3`, `all-minilm`, rerankers… cannot be a chat default.
pub(crate) fn looks_like_embedding_model(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("embed") || n.contains("minilm") || n.contains("rerank") || n.starts_with("bge-")
}

/// Largest model first; equal sizes are ordered by id so the choice never depends on scan order.
fn by_size_then_id(a: &&LocalModel, b: &&LocalModel) -> std::cmp::Ordering {
    a.size_bytes.cmp(&b.size_bytes).then_with(|| b.id.cmp(&a.id))
}

/// Picks the local model for the default profile, with its fit:
/// 1. the largest GGUF chat model **with tools** whose fit is great or ok (not simply the
///    largest: a 70B next to a 4B on a 16 GB laptop must not win);
/// 2. else the largest chat model without tools that fits;
/// 3. else the smallest one (tools first) that fits only with partial offload;
/// 4. else nothing (too big for this device, a smaller one has to be downloaded).
fn pick_local<'a>(models: &'a [LocalModel], device: &DeviceProfile) -> Option<(&'a LocalModel, Fit)> {
    let chat: Vec<(&LocalModel, Fit)> = models
        .iter()
        .filter(|m| m.format == "gguf" && m.capabilities.iter().any(|c| c == "chat"))
        .map(|m| (m, local_fit(m, device)))
        .collect();
    let has_tools = |m: &LocalModel| m.capabilities.iter().any(|c| c == "tools");
    let fits = |f: Fit| f >= Fit::Ok;
    let largest =
        |tools: bool| chat.iter().filter(|(m, f)| fits(*f) && has_tools(m) == tools).max_by(|a, b| by_size_then_id(&a.0, &b.0)).copied();
    largest(true).or_else(|| largest(false)).or_else(|| {
        chat.iter()
            .filter(|(_, f)| *f == Fit::Tight)
            // `false < true`: models with tools first, then the smallest, then the smaller id.
            .min_by(|a, b| {
                (!has_tools(a.0)).cmp(&!has_tools(b.0)).then_with(|| a.0.size_bytes.cmp(&b.0.size_bytes)).then_with(|| a.0.id.cmp(&b.0.id))
            })
            .copied()
    })
}

pub(crate) fn profile(description: String, runtime_id: &str, model: &str, ctx: u32) -> InferenceProfile {
    InferenceProfile {
        id: "default".into(),
        name: "Predefinito".into(),
        description: Some(description),
        runtime_id: runtime_id.into(),
        model: model.into(),
        params: InferenceParams {
            temperature: Some(0.6),
            top_p: Some(0.95),
            // The answer must leave room for the conversation in a small window.
            max_tokens: Some(4096.min(ctx / 2)),
            context: Some(ctx),
            thinking: None,
            thinking_budget: None,
            adapters: vec![],
            allow_remote: false,
        },
    }
}

/// Picks the default profile for this device (deterministic for the same inputs).
///
/// Runtime: `llama-server` when installed, else the in-process engine when it is compiled in and
/// registered, else a running Ollama that has models. Model: the best local GGUF for the device
/// ([`pick_local`]), or, when there is none (or nothing can run it), the first chat model of Ollama.
pub fn default_profile(models: &[LocalModel], device: &DeviceProfile, avail: &Availability) -> InferenceProfile {
    let gguf_rt = avail.gguf_runtime();
    let ollama = avail.ollama_chat_model();
    let phone = matches!(device.os.as_str(), "android" | "ios");
    let ollama_profile = |model: &str| profile(format!("{model} su Ollama"), "ollama", model, 8192);
    let local = pick_local(models, device);

    match (gguf_rt, local) {
        (Some(rt), Some((m, fit))) => {
            let place = if rt == "llama-embedded" { "llama.cpp (in-process)" } else { "llama.cpp" };
            return profile(format!("{} su {place}", m.name), rt, &m.id, context_for(m.context_length, fit));
        }
        // A local model, but no engine for it yet: an Ollama that is up wins, otherwise keep the
        // pairing with llama-server, which the user is told to install on first use.
        (None, Some((m, fit))) => {
            return match ollama {
                Some(o) => ollama_profile(o),
                None => profile(format!("{} su llama.cpp", m.name), "llama-server", &m.id, context_for(m.context_length, fit)),
            };
        }
        (_, None) => {}
    }
    if let Some(o) = ollama {
        return ollama_profile(o);
    }
    // Nothing fits and nothing else runs. A model that is too large is worse than a placeholder.
    let runtime = gguf_rt.unwrap_or(if phone { "llama-embedded" } else { "llama-server" });
    profile("Scarica un modello da Modelli per iniziare".into(), runtime, "", 8192)
}

/// The example suite (`docs/evals/esempio.yaml`), stored on first run.
pub fn eval_suites() -> Vec<aiw_eval::Suite> {
    vec![aiw_eval::Suite::parse(include_str!("../../../docs/evals/esempio.yaml")).expect("the example suite is valid")]
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

#[cfg(test)]
mod tests {
    use super::*;
    use aiw_core::api::{Accelerator, CpuInfo};

    #[test]
    fn the_browser_agent_gets_the_safe_new_tools_and_no_agent_the_risky_ones() {
        let all = agents();
        let browser = all.iter().find(|a| a.id == "browser").unwrap();
        for t in [
            "browser_tabs",
            "browser_tab_new",
            "browser_tab_select",
            "browser_select",
            "browser_hover",
            "browser_wait",
            "browser_downloads",
            "browser_console",
        ] {
            assert!(browser.tools.iter().any(|n| n == t), "{t}");
        }
        // Running code in the page, handing files to a site and closing tabs are for the person to grant.
        for a in &all {
            for t in ["browser_eval", "browser_upload", "browser_tab_close"] {
                assert!(!a.tools.iter().any(|n| n == t), "{} lists {t}", a.id);
            }
        }
        // Every browser tool an agent lists exists.
        let known: Vec<String> = aiw_tools::browser::tools(std::sync::Arc::new(aiw_tools::browser::BrowserManager::new(
            std::env::temp_dir().join("aiw-defaults-test"),
        )))
        .iter()
        .map(|t| t.name().to_string())
        .collect();
        for a in &all {
            for t in a.tools.iter().filter(|t| t.starts_with("browser_")) {
                assert!(known.contains(t), "{} lists the unknown tool {t}", a.id);
            }
        }
    }

    const GB: u64 = 1024 * 1024 * 1024;

    /// Built from JSON so the helper keeps compiling when `LocalModel` gains defaulted fields.
    fn model(id: &str, size_gb: f64, caps: &[&str], ctx: Option<u64>) -> LocalModel {
        serde_json::from_value(json!({
            "id": id, "name": id, "path": format!("/models/{id}.gguf"), "format": "gguf",
            "size_bytes": (size_gb * GB as f64) as u64, "context_length": ctx,
            "capabilities": caps, "runtimes": ["llama-server"],
        }))
        .unwrap()
    }

    fn device(os: &str, ram_gb: u64, vram_gb: Option<u64>) -> DeviceProfile {
        let mut accelerators =
            vec![Accelerator { kind: "cpu".into(), name: "cpu".into(), vendor: None, memory_mb: None, driver: None, details: None }];
        if let Some(v) = vram_gb {
            accelerators.push(Accelerator {
                kind: "cuda".into(),
                name: "GPU".into(),
                vendor: Some("NVIDIA".into()),
                memory_mb: Some(v * 1024),
                driver: None,
                details: None,
            });
        }
        DeviceProfile {
            id: "dev_test".into(),
            name: "test".into(),
            os: os.into(),
            os_version: String::new(),
            arch: "x86_64".into(),
            form_factor: "laptop".into(),
            cpu: CpuInfo { brand: "cpu".into(), cores: 8, threads: 16, features: vec![] },
            memory_mb: ram_gb * 1024,
            available_memory_mb: ram_gb * 1024,
            accelerators,
            tier: String::new(),
        }
    }

    fn avail(server: bool, embedded: bool, ollama: &[&str]) -> Availability {
        Availability { llama_server: server, llama_embedded: embedded, ollama_models: ollama.iter().map(|s| s.to_string()).collect() }
    }

    const TOOLS: &[&str] = &["chat", "tools"];

    #[test]
    fn picks_the_largest_tool_model_that_fits_not_simply_the_largest() {
        // 16 GB laptop without a GPU: the budget is 8 GB.
        let laptop = device("linux", 16, None);
        let models = vec![
            model("llama-70b", 42.0, TOOLS, Some(131072)),
            model("qwen-14b", 9.0, TOOLS, None),
            model("qwen-8b", 4.9, TOOLS, Some(40960)),
            model("qwen-4b", 2.5, TOOLS, Some(40960)),
        ];
        let p = default_profile(&models, &laptop, &avail(true, false, &[]));
        assert_eq!(p.model, "qwen-8b");
        assert_eq!(p.runtime_id, "llama-server");
        // The same models on a 64 GB box with a 24 GB GPU: the 42 GB one only fits with partial
        // offload ("tight"), so the 14B wins.
        let big = device("linux", 64, Some(24));
        assert_eq!(default_profile(&models, &big, &avail(true, false, &[])).model, "qwen-14b");
    }

    #[test]
    fn speech_models_are_never_chat_profiles() {
        let laptop = device("linux", 16, None);
        let mut whisper = model("ggml-small-q8_0", 0.25, &["stt"], None);
        whisper.format = "ggml".into();
        let mut mislabeled = model("ggml-base", 0.08, &["chat"], None);
        mislabeled.format = "ggml".into();
        let models = vec![whisper, mislabeled, model("qwen-4b", 2.5, TOOLS, None)];
        assert_eq!(default_profile(&models, &laptop, &avail(true, false, &[])).model, "qwen-4b");
        let only_speech = vec![models[0].clone()];
        assert_eq!(default_profile(&only_speech, &laptop, &avail(true, false, &[])).model, "");
    }

    #[test]
    fn tools_beat_size_and_nothing_that_does_not_fit_is_chosen() {
        let laptop = device("linux", 16, None);
        let models = vec![
            model("big-chat", 4.5, &["chat"], None),
            model("small-tools", 2.0, TOOLS, None),
            model("embed", 0.3, &["embeddings"], None),
        ];
        assert_eq!(default_profile(&models, &laptop, &avail(true, false, &[])).model, "small-tools");
        // Without a tools model the largest chat model that fits is used.
        let models = vec![model("a", 4.5, &["chat"], None), model("b", 2.0, &["chat"], None)];
        assert_eq!(default_profile(&models, &laptop, &avail(true, false, &[])).model, "a");
        // Only a model that does not fit at all: placeholder, not a 70B on a laptop.
        let models = vec![model("llama-70b", 42.0, TOOLS, None)];
        let p = default_profile(&models, &laptop, &avail(true, false, &[]));
        assert_eq!(p.model, "");
        assert_eq!(p.runtime_id, "llama-server");
        assert!(p.description.unwrap().contains("Scarica"));
    }

    #[test]
    fn partial_offload_is_the_last_resort_and_prefers_the_smallest() {
        // 8 GB RAM, no GPU: budget 4 GB, offload 4.8 GB. 3.2 and 3.5 GB models only "tight".
        let small = device("linux", 8, None);
        let models = vec![model("m-3-5", 3.5, TOOLS, None), model("m-3-2", 3.2, TOOLS, None), model("m-huge", 30.0, TOOLS, None)];
        let p = default_profile(&models, &small, &avail(true, false, &[]));
        assert_eq!(p.model, "m-3-2");
        assert_eq!(p.params.context, Some(4096));
    }

    #[test]
    fn unified_memory_has_no_offload_beyond_the_budget() {
        // 8 GB Android phone: budget 5.6 GB. A 6 GB file would need ~7.3 GB: it does not fit,
        // and "partial offload" must not add the RAM that the budget already counts.
        let phone = device("android", 8, Some(8));
        let models = vec![model("m-6", 6.0, TOOLS, None)];
        assert_eq!(default_profile(&models, &phone, &avail(true, false, &[])).model, "");
        // A 3 GB file fits.
        let models = vec![model("m-3", 3.0, TOOLS, None)];
        assert_eq!(default_profile(&models, &phone, &avail(true, false, &[])).model, "m-3");
    }

    #[test]
    fn runtime_order_is_llama_server_then_embedded_then_ollama() {
        let laptop = device("linux", 16, None);
        let models = vec![model("qwen-4b", 2.5, TOOLS, Some(40960))];
        assert_eq!(default_profile(&models, &laptop, &avail(true, true, &["x:1"])).runtime_id, "llama-server");
        let p = default_profile(&models, &laptop, &avail(false, true, &["x:1"]));
        assert_eq!((p.runtime_id.as_str(), p.model.as_str()), ("llama-embedded", "qwen-4b"));
        assert!(p.description.unwrap().contains("in-process"));
        // Local models but neither engine, Ollama up: Ollama (models cannot run anywhere else).
        let p = default_profile(&models, &laptop, &avail(false, false, &["gemma3:4b"]));
        assert_eq!((p.runtime_id.as_str(), p.model.as_str()), ("ollama", "gemma3:4b"));
        // No engine and no Ollama: the old pairing with llama-server, to be installed.
        let p = default_profile(&models, &laptop, &avail(false, false, &[]));
        assert_eq!((p.runtime_id.as_str(), p.model.as_str()), ("llama-server", "qwen-4b"));
    }

    #[test]
    fn ollama_is_used_when_there_are_no_local_models_and_skips_embedding_models() {
        let laptop = device("linux", 16, None);
        let p = default_profile(&[], &laptop, &avail(false, true, &["nomic-embed-text:latest", "bge-m3:latest", "qwen3:4b", "gemma3:12b"]));
        assert_eq!((p.runtime_id.as_str(), p.model.as_str()), ("ollama", "qwen3:4b"));
        // Only embedding models: nothing to chat with.
        let p = default_profile(&[], &laptop, &avail(false, true, &["nomic-embed-text:latest"]));
        assert_eq!(p.model, "");
        assert_eq!(p.runtime_id, "llama-embedded");
    }

    #[test]
    fn phones_default_to_the_embedded_engine() {
        for os in ["android", "ios"] {
            let phone = device(os, 8, None);
            // Registered embedded engine + a model.
            let models = vec![model("qwen-1.7b", 1.1, TOOLS, Some(32768))];
            let p = default_profile(&models, &phone, &avail(false, true, &[]));
            assert_eq!((p.runtime_id.as_str(), p.model.as_str()), ("llama-embedded", "qwen-1.7b"), "{os}");
            // Empty phone: the placeholder still points at the engine the shell registers there.
            assert_eq!(default_profile(&[], &phone, &avail(false, false, &[])).runtime_id, "llama-embedded", "{os}");
        }
        // Desktop placeholder keeps llama-server.
        assert_eq!(default_profile(&[], &device("windows", 16, None), &avail(false, false, &[])).runtime_id, "llama-server");
    }

    #[test]
    fn context_follows_the_model_and_the_fit() {
        let laptop = device("linux", 16, None);
        // great fit + long-context model: capped at 16384.
        let p = default_profile(&[model("m", 2.5, TOOLS, Some(131072))], &laptop, &avail(true, false, &[]));
        assert_eq!((p.params.context, p.params.max_tokens), (Some(16384), Some(4096)));
        // the model's own limit wins when lower; unknown limit falls back to 8192.
        let p = default_profile(&[model("m", 2.5, TOOLS, Some(4096))], &laptop, &avail(true, false, &[]));
        assert_eq!((p.params.context, p.params.max_tokens), (Some(4096), Some(2048)));
        let p = default_profile(&[model("m", 2.5, TOOLS, None)], &laptop, &avail(true, false, &[]));
        assert_eq!(p.params.context, Some(8192));
        // an "ok" fit gets a smaller window than a "great" one.
        let p = default_profile(&[model("m", 4.9, TOOLS, Some(131072))], &laptop, &avail(true, false, &[]));
        assert_eq!(p.params.context, Some(8192));
    }

    #[test]
    fn the_choice_does_not_depend_on_scan_order_and_ties_are_stable() {
        let laptop = device("linux", 16, None);
        let mut models = vec![model("b-model", 2.5, TOOLS, None), model("a-model", 2.5, TOOLS, None), model("c-model", 1.0, TOOLS, None)];
        let a = default_profile(&models, &laptop, &avail(true, false, &[]));
        models.reverse();
        let b = default_profile(&models, &laptop, &avail(true, false, &[]));
        assert_eq!(a, b);
        assert_eq!(a.model, "a-model");
    }

    #[test]
    fn availability_is_derived_from_runtime_status() {
        let info = |id: &str, status: &str, loaded: &[&str]| -> RuntimeInfo {
            serde_json::from_value(json!({
                "id": id, "name": id, "kind": "sidecar", "description": "", "platforms": [], "accelerators": [], "formats": [],
                "api": "openai", "capabilities": [], "status": status, "loaded_models": loaded,
                "settings_schema": {"type": "object", "properties": {}}, "settings": {}
            }))
            .unwrap()
        };
        let a = Availability::from_runtimes(&[
            info("llama-server", "available", &[]),
            info("llama-embedded", "running", &[]),
            info("ollama", "running", &["q:1"]),
        ]);
        assert_eq!(a, avail(true, true, &["q:1"]));
        // An embedded engine that is not registered ("installable") and a stopped Ollama do not count.
        let a = Availability::from_runtimes(&[
            info("llama-server", "installable", &[]),
            info("llama-embedded", "installable", &[]),
            info("ollama", "available", &["q:1"]),
        ]);
        assert_eq!(a, avail(false, false, &[]));
        assert_eq!(Availability::from_runtimes(&[info("llama-server", "running", &[])]).gguf_runtime(), Some("llama-server"));
    }
}
