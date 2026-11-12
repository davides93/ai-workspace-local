//! The model adviser ("consigliere"): which model is best for which task on this device.
//!
//! Pure and deterministic: [`advise`] takes the device, the models on disk, the catalog and what
//! the runtimes can serve, and returns for each [`AdviserTask`] the best *installed* choice and,
//! when nothing installed fits or something clearly better would, the best one *to download*.
//! [`Engine::recommend`] feeds it with the live state; `auto:<task>` inference profiles are
//! resolved through it when a run starts ([`Engine::resolve_auto_profile`]).
//!
//! ## One fit formula
//! How comfortably a model runs is decided by `aiw_models::fit_need` (`size × 1.1 + 700 MB`
//! against the device memory budget of `aiw_hw`); [`Fit`] only parses its answer, so the catalog
//! cards, the first-run seeding ([`crate::defaults::default_profile`]) and the adviser agree.
//!
//! ## Suitability scores
//! A coarse ordinal per task, `0` (a required capability is missing) to `3` (very good), kept in
//! the optional `suitability` field of a catalog entry and derived by [`derive_scores`] for models
//! the catalog does not describe. It comes only from what an entry already declares (capabilities,
//! `params_b`, tags): it is a ranking aid, not a benchmark.
//!
//! | task | needs | 1 | 2 | 3 |
//! |------|-------|---|---|---|
//! | chat | `chat` | under 1B | 1B to under 3B | 3B and more |
//! | agent | `chat`, `tools` | under 2B | 2B to under 6B (or tagged `agent`) | 6B and more |
//! | reasoning | `chat`, `tools`, `reasoning` | under 2B | 2B to under 6B | 6B and more |
//! | vision | `chat`, `vision` | under 3B | 3B to 6B | over 6B |
//! | embeddings | `embeddings` | | | always 3 (small and fixed) |
//!
//! ## Choosing
//! Among the models that fit (`great` or `ok`; `tight` only as a last resort, smallest first):
//! * `chat`: a system model when one is available (instant, no memory), then the highest score,
//!   then a general-purpose model (one that also calls tools) over a specialist, then the one that
//!   fits most comfortably, then the largest of those up to 9B (a bigger one is slower for no
//!   visible gain in everyday chat), then the smaller file;
//! * `agent`, `reasoning`, `vision`: the highest score, then the most comfortable fit (room for
//!   the context), then the largest. `reasoning` falls back to the agent choice, without
//!   thinking, when no model reasons at least as well as that one works;
//! * `embeddings`: the highest score, then the smallest file.
//!
//! A download is proposed when nothing installed is suited, when the installed choice only runs
//! with partial offload, or when the catalog has a model with a strictly higher score.

use crate::defaults::Availability;
use aiw_core::api::{
    AdviserTask, CatalogModel, DeviceProfile, LocalModel, ModelChoice, ModelSource, Recommendations, TaskAdvice, TaskScores, ThinkingMode,
};
use std::cmp::Ordering;

/// How comfortably a model runs on a device (the bands of `aiw_models::fit_need`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Fit {
    No,
    Tight,
    Ok,
    Great,
}

impl Fit {
    pub(crate) fn from_label(label: &str) -> Fit {
        match label {
            "great" => Fit::Great,
            "ok" => Fit::Ok,
            "tight" => Fit::Tight,
            _ => Fit::No,
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Fit::Great => "great",
            Fit::Ok => "ok",
            Fit::Tight => "tight",
            Fit::No => "no",
        }
    }
}

/// Fit of a model file already on disk: its real size through the shared formula.
pub(crate) fn local_fit(m: &LocalModel, device: &DeviceProfile) -> Fit {
    if m.format == "system" {
        return Fit::Great;
    }
    Fit::from_label(&aiw_models::fit_need(aiw_models::min_memory_mb(m.size_bytes, 0), &m.format, device).0)
}

/// Context window for a profile: what the model supports, capped by how comfortably it fits
/// (the KV cache grows with it) and never below 2048.
pub(crate) fn context_for(model_context: Option<u64>, fit: Fit) -> u32 {
    let cap: u32 = match fit {
        Fit::Great => 16384,
        Fit::Ok => 8192,
        Fit::Tight | Fit::No => 4096,
    };
    model_context.map(|c| c.min(cap as u64) as u32).unwrap_or(cap.min(8192)).max(2048)
}

fn has(caps: &[String], c: &str) -> bool {
    caps.iter().any(|x| x == c)
}

/// Scores of a model from what it declares (see the module documentation for the table).
pub fn derive_scores(caps: &[String], params_b: f64, tags: &[String]) -> TaskScores {
    let ladder = |small: f64, big: f64| -> u8 {
        if params_b < small {
            1
        } else if params_b < big {
            2
        } else {
            3
        }
    };
    let chat = has(caps, "chat");
    let tools = chat && has(caps, "tools");
    let tagged_agent = tags.iter().any(|t| t == "agent");
    TaskScores {
        chat: if chat { ladder(1.0, 3.0) } else { 0 },
        agent: if tools { ladder(2.0, 6.0).max(if tagged_agent { 2 } else { 0 }) } else { 0 },
        reasoning: if tools && has(caps, "reasoning") { ladder(2.0, 6.0) } else { 0 },
        vision: if chat && has(caps, "vision") {
            if params_b < 3.0 {
                1
            } else if params_b <= 6.0 {
                2
            } else {
                3
            }
        } else {
            0
        },
        embeddings: if has(caps, "embeddings") { 3 } else { 0 },
    }
}

fn score(s: &TaskScores, t: AdviserTask) -> u8 {
    match t {
        AdviserTask::Chat => s.chat,
        AdviserTask::Agent => s.agent,
        AdviserTask::Reasoning => s.reasoning,
        AdviserTask::Vision => s.vision,
        AdviserTask::Embeddings => s.embeddings,
    }
}

/// Parameters in billions guessed from a file size and its quantization, for models that do not say.
fn estimate_params_b(size_bytes: u64, quant: Option<&str>) -> f64 {
    let q = quant.unwrap_or("").to_uppercase();
    let gb_per_b = if q.starts_with("Q8") {
        1.06
    } else if q.starts_with("Q6") {
        0.82
    } else if q.starts_with("Q5") {
        0.72
    } else if q.starts_with("F16") || q.starts_with("BF16") {
        2.0
    } else {
        0.6
    };
    size_bytes as f64 / 1e9 / gb_per_b
}

/// `qwen3:4b`, `llama3.2:3b-instruct`… → 4.0, 3.0.
fn params_from_name(name: &str) -> Option<f64> {
    let l = name.to_lowercase();
    let b = l.as_bytes();
    for (i, c) in l.char_indices() {
        if c == 'b' && i > 0 && (i + 1 == l.len() || !b[i + 1].is_ascii_alphabetic()) {
            let start = l[..i].rfind(|ch: char| !(ch.is_ascii_digit() || ch == '.')).map(|p| p + 1).unwrap_or(0);
            if let Ok(v) = l[start..i].parse::<f64>() {
                return Some(v);
            }
        }
    }
    None
}

/// What a model on disk or on Ollama is, for ranking.
#[derive(Debug, Clone)]
struct Candidate {
    id: String,
    name: String,
    runtime: String,
    /// The runtime can serve it now (else it still needs to be installed or started).
    runtime_ready: bool,
    system: bool,
    params_b: f64,
    size_bytes: u64,
    context: Option<u64>,
    fit: Fit,
    tools_cap: bool,
    reasoning_cap: bool,
    scores: TaskScores,
    catalog_id: Option<String>,
}

/// The file name of a catalog entry (`Qwen3-4B-Q4_K_M.gguf`), when it is one file.
fn catalog_file(c: &CatalogModel) -> Option<&str> {
    match &c.source {
        ModelSource::Hf { file, .. } if !file.is_empty() => Some(file.rsplit('/').next().unwrap_or(file)),
        _ => None,
    }
}

fn file_name(m: &LocalModel) -> Option<&str> {
    m.path.as_deref().and_then(|p| p.rsplit(['/', '\\']).next())
}

fn catalog_scores(c: &CatalogModel) -> TaskScores {
    c.suitability.unwrap_or_else(|| derive_scores(&c.capabilities, c.params_b, &c.tags))
}

fn local_candidate(m: &LocalModel, device: &DeviceProfile, avail: &Availability, catalog: &[CatalogModel]) -> Option<Candidate> {
    let phone = matches!(device.os.as_str(), "android" | "ios");
    let (runtime, ready) = match m.format.as_str() {
        "system" => (m.runtimes.first()?.clone(), true),
        "gguf" => match avail.gguf_runtime() {
            Some(rt) => (rt.to_string(), true),
            // No engine yet: the first use tells the person to install llama-server (as the profile seeding does).
            None => ((if phone { "llama-embedded" } else { "llama-server" }).to_string(), false),
        },
        // Other formats (MLX, LiteRT…): only when a runtime for them is usable now.
        _ => (m.runtime_compat.iter().find(|c| c.available).map(|c| c.id.clone())?, true),
    };
    let from_catalog = catalog.iter().find(|c| match (&c.source, file_name(m)) {
        (ModelSource::System { .. }, _) => c.id == m.id,
        (_, Some(f)) => catalog_file(c).is_some_and(|cf| cf.eq_ignore_ascii_case(f)),
        _ => false,
    });
    let params_b = m
        .params_b
        .or(from_catalog.map(|c| c.params_b))
        .filter(|p| *p > 0.0)
        .unwrap_or_else(|| estimate_params_b(m.size_bytes, m.quant.as_deref()));
    let scores = from_catalog.map(catalog_scores).unwrap_or_else(|| derive_scores(&m.capabilities, params_b, &[]));
    Some(Candidate {
        id: m.id.clone(),
        name: m.name.clone(),
        runtime,
        runtime_ready: ready,
        system: m.format == "system",
        params_b,
        size_bytes: m.size_bytes,
        context: m.context_length,
        fit: local_fit(m, device),
        tools_cap: has(&m.capabilities, "tools"),
        reasoning_cap: has(&m.capabilities, "reasoning"),
        scores,
        catalog_id: from_catalog.map(|c| c.id.clone()),
    })
}

/// A model served by a running Ollama: only its name is known, so capabilities are guessed from it.
fn ollama_candidate(name: &str) -> Candidate {
    let l = name.to_lowercase();
    let embedding = crate::defaults::looks_like_embedding_model(name);
    let mut caps: Vec<String> = if embedding { vec!["embeddings".into()] } else { vec!["chat".into(), "tools".into()] };
    if !embedding && ["qwen3", "deepseek-r1", "gpt-oss", "think"].iter().any(|k| l.contains(k)) {
        caps.push("reasoning".into());
    }
    if !embedding && ["llava", "-vl", "vision", "gemma3"].iter().any(|k| l.contains(k)) {
        caps.push("vision".into());
    }
    let params_b = params_from_name(name).unwrap_or(4.0);
    Candidate {
        id: name.into(),
        name: name.into(),
        runtime: "ollama".into(),
        runtime_ready: true,
        system: false,
        params_b,
        size_bytes: 0,
        context: None,
        // The size is unknown: assume it was installed for this machine.
        fit: Fit::Ok,
        tools_cap: has(&caps, "tools"),
        reasoning_cap: has(&caps, "reasoning"),
        scores: derive_scores(&caps, params_b, &[]),
        catalog_id: None,
    }
}

fn download_candidate(c: &CatalogModel, device: &DeviceProfile, avail: &Availability) -> Option<Candidate> {
    if c.format == "system" || matches!(c.source, ModelSource::System { .. }) {
        return None;
    }
    let fit = Fit::from_label(&aiw_models::fit(c, device).0);
    // gguf runs on the llama.cpp engines; other formats only when a runtime for them is usable.
    let (runtime, ready) = if c.format == "gguf" {
        match avail.gguf_runtime() {
            Some(rt) => (rt.to_string(), true),
            None => ((if matches!(device.os.as_str(), "android" | "ios") { "llama-embedded" } else { "llama-server" }).to_string(), false),
        }
    } else {
        (c.runtime_compat.iter().find(|r| r.available)?.id.clone(), true)
    };
    // The id the model has once installed: the file stem, lowercase (as the scan names it).
    let model = catalog_file(c).map(|f| f.rsplit_once('.').map(|(s, _)| s).unwrap_or(f).to_lowercase()).unwrap_or_else(|| c.id.clone());
    Some(Candidate {
        id: model,
        name: c.name.clone(),
        runtime,
        runtime_ready: ready,
        system: false,
        params_b: c.params_b,
        size_bytes: c.size_bytes,
        context: (c.context_length > 0).then_some(c.context_length),
        fit,
        tools_cap: has(&c.capabilities, "tools"),
        reasoning_cap: has(&c.capabilities, "reasoning"),
        scores: catalog_scores(c),
        catalog_id: Some(c.id.clone()),
    })
}

/// Orders two candidates for a task: `Greater` is better.
fn compare(task: AdviserTask, a: &Candidate, b: &Candidate) -> Ordering {
    let (sa, sb) = (score(&a.scores, task), score(&b.scores, task));
    let tenths = |p: f64| (p * 10.0) as u32;
    let ordering = a.runtime_ready.cmp(&b.runtime_ready);
    let ordering = match task {
        AdviserTask::Chat => ordering
            .then(a.system.cmp(&b.system))
            .then(sa.cmp(&sb))
            // A general-purpose model (one that also calls tools) over a specialist.
            .then(a.tools_cap.cmp(&b.tools_cap))
            .then(a.fit.cmp(&b.fit))
            // Past 9B a model is slower for no visible gain in chat: smaller ones first, then the largest of them.
            .then((a.params_b <= 9.0).cmp(&(b.params_b <= 9.0)))
            .then_with(|| {
                if a.params_b <= 9.0 {
                    tenths(a.params_b).cmp(&tenths(b.params_b))
                } else {
                    tenths(b.params_b).cmp(&tenths(a.params_b))
                }
            })
            .then_with(|| b.size_bytes.cmp(&a.size_bytes)),
        AdviserTask::Agent | AdviserTask::Reasoning | AdviserTask::Vision => ordering
            .then(sa.cmp(&sb))
            .then(a.fit.cmp(&b.fit))
            .then_with(|| tenths(a.params_b).cmp(&tenths(b.params_b)))
            .then_with(|| a.size_bytes.cmp(&b.size_bytes)),
        AdviserTask::Embeddings => ordering.then(sa.cmp(&sb)).then_with(|| b.size_bytes.cmp(&a.size_bytes)),
    };
    // Equal in everything that matters: the smaller id wins, so the scan order never does.
    ordering.then_with(|| b.id.cmp(&a.id))
}

/// The best candidate for a task: among those that fit comfortably, else (last resort) the
/// smallest one that only fits with partial offload; `None` when nothing is suited.
fn best(task: AdviserTask, cands: &[Candidate]) -> Option<&Candidate> {
    let suited = |c: &&Candidate| score(&c.scores, task) > 0 && c.fit != Fit::No;
    let pool = cands.iter().filter(suited);
    pool.clone()
        .filter(|c| c.fit >= Fit::Ok)
        .max_by(|a, b| compare(task, a, b))
        .or_else(|| pool.filter(|c| c.fit == Fit::Tight).min_by(|a, b| a.size_bytes.cmp(&b.size_bytes).then_with(|| a.id.cmp(&b.id))))
}

fn reason(task: AdviserTask, c: &Candidate, reasoning_missing: bool) -> String {
    let squeezed = if c.fit == Fit::Great {
        ""
    } else if c.fit == Fit::Ok {
        " Su questo dispositivo la memoria è un po' stretta: la conversazione ricordata sarà più corta."
    } else {
        " Su questo dispositivo va più lento, perché usa anche la memoria del computer."
    };
    let base = match task {
        AdviserTask::Chat if c.system => "È già nel sistema: parte subito e non occupa spazio.".to_string(),
        AdviserTask::Chat => "Risponde veloce e va bene per le conversazioni di ogni giorno.".to_string(),
        AdviserTask::Agent => "Sa usare bene gli strumenti e lascia memoria per ricordare la conversazione.".to_string(),
        AdviserTask::Reasoning if reasoning_missing => {
            "Non ragiona a fondo come i modelli dedicati, ma è il più adatto tra quelli che hai.".to_string()
        }
        AdviserTask::Reasoning => "Ragiona prima di rispondere: adatto ai compiti difficili.".to_string(),
        AdviserTask::Vision => "Capisce le immagini e le schermate.".to_string(),
        AdviserTask::Embeddings => "Piccolo e veloce: serve a cercare tra i tuoi documenti.".to_string(),
    };
    format!("{base}{squeezed}")
}

fn thinking_for(task: AdviserTask, c: &Candidate) -> Option<ThinkingMode> {
    match task {
        AdviserTask::Reasoning if c.reasoning_cap => Some(ThinkingMode::Deep),
        // A model that can reason answers everyday chat faster with it off.
        AdviserTask::Chat if c.reasoning_cap => Some(ThinkingMode::Off),
        _ => None,
    }
}

fn choice(task: AdviserTask, c: &Candidate, reasoning_missing: bool) -> ModelChoice {
    let context = match task {
        AdviserTask::Embeddings => c.context.map(|n| n.min(8192) as u32).unwrap_or(2048).max(512),
        _ => context_for(c.context, c.fit),
    };
    ModelChoice {
        model: c.id.clone(),
        name: c.name.clone(),
        runtime_id: c.runtime.clone(),
        context,
        thinking: if reasoning_missing { None } else { thinking_for(task, c) },
        fit: c.fit.label().into(),
        size_bytes: c.size_bytes,
        catalog_id: c.catalog_id.clone(),
        reason: reason(task, c, reasoning_missing),
    }
}

/// Reasoning falls back to the agent choice when no candidate reasons at least as well as the
/// agent choice works (a 0.6B that thinks is not better than a 3B that does not): a profile that
/// asks for reasoning must still be as capable as an agent one. Returns the candidate and whether
/// the fallback was taken.
fn pick_for(task: AdviserTask, cands: &[Candidate]) -> Option<(&Candidate, bool)> {
    if task != AdviserTask::Reasoning {
        return best(task, cands).map(|c| (c, false));
    }
    let agent = best(AdviserTask::Agent, cands);
    match (best(AdviserTask::Reasoning, cands), agent) {
        (Some(r), Some(a)) if score(&r.scores, AdviserTask::Reasoning) < score(&a.scores, AdviserTask::Agent) => Some((a, true)),
        (Some(r), _) => Some((r, false)),
        (None, a) => a.map(|a| (a, true)),
    }
}

fn advise_task(task: AdviserTask, installed: &[Candidate], downloadable: &[Candidate]) -> TaskAdvice {
    let inst = pick_for(task, installed);
    let down = pick_for(task, downloadable);
    // The catalog entry that is already installed is not a download.
    let inst_catalog = inst.and_then(|(c, _)| c.catalog_id.as_deref());
    let down = down.filter(|(d, d_fallback)| {
        if d.catalog_id.as_deref() == inst_catalog && inst_catalog.is_some() {
            return false;
        }
        match inst {
            None => true,
            Some((i, i_fallback)) => {
                // A model that really reasons beats one that only stands in for it.
                if i_fallback != *d_fallback {
                    return i_fallback;
                }
                let (si, sd) = (score(&i.scores, task), score(&d.scores, task));
                sd > si || (i.fit == Fit::Tight && d.fit >= Fit::Ok)
            }
        }
    });
    TaskAdvice { installed: inst.map(|(c, fb)| choice(task, c, fb)), download: down.map(|(c, fb)| choice(task, c, fb)) }
}

/// The adviser. `catalog` is the curated catalog (the installed flag and runtime compatibility
/// of its entries are used when present; fit is always recomputed for `device`).
pub fn advise(device: &DeviceProfile, local: &[LocalModel], catalog: &[CatalogModel], avail: &Availability) -> Recommendations {
    let mut installed: Vec<Candidate> = local.iter().filter_map(|m| local_candidate(m, device, avail, catalog)).collect();
    installed.extend(avail.ollama_models.iter().map(|n| ollama_candidate(n)));
    let is_installed = |c: &CatalogModel| {
        c.installed || catalog_file(c).is_some_and(|f| local.iter().any(|m| file_name(m).is_some_and(|n| n.eq_ignore_ascii_case(f))))
    };
    let downloadable: Vec<Candidate> =
        catalog.iter().filter(|c| !is_installed(c)).filter_map(|c| download_candidate(c, device, avail)).collect();
    let mut rec = Recommendations { tier: aiw_hw::tier_for(device).into(), ..Default::default() };
    for task in AdviserTask::ALL {
        let advice = advise_task(task, &installed, &downloadable);
        match task {
            AdviserTask::Chat => rec.chat = advice,
            AdviserTask::Agent => rec.agent = advice,
            AdviserTask::Reasoning => rec.reasoning = advice,
            AdviserTask::Vision => rec.vision = advice,
            AdviserTask::Embeddings => rec.embeddings = advice,
        }
    }
    rec
}

/// Inference profile id of a task: `auto:agent`…
pub fn auto_profile_id(task: AdviserTask) -> String {
    format!("auto:{}", task.as_str())
}

/// The task of an `auto:<task>` profile id. Embeddings are not an agent profile.
pub fn parse_auto_profile(id: &str) -> Option<AdviserTask> {
    id.strip_prefix("auto:").and_then(AdviserTask::parse).filter(|t| *t != AdviserTask::Embeddings)
}

/// The profile an `auto:<task>` id stands for right now, from the adviser's installed choice.
pub fn auto_profile(task: AdviserTask, rec: &Recommendations) -> Option<aiw_core::api::InferenceProfile> {
    let c = rec.for_task(task).installed.as_ref()?;
    let label = match task {
        AdviserTask::Chat => "chat",
        AdviserTask::Agent => "agente",
        AdviserTask::Reasoning => "ragionamento",
        AdviserTask::Vision => "immagini",
        AdviserTask::Embeddings => "ricerca",
    };
    let mut p = crate::defaults::profile(format!("{} ({})", c.name, c.reason), &c.runtime_id, &c.model, c.context);
    p.id = auto_profile_id(task);
    p.name = format!("Automatico: {label}");
    p.params.thinking = c.thinking;
    Some(p)
}

// ------------------------------------------------------------------------- model roles ("auto")

/// The model the agent runs on by itself (its own inference profile), as far as the roles care.
#[derive(Debug, Clone, PartialEq)]
pub struct MainModel {
    pub runtime_id: String,
    pub model: String,
}

/// A paired device that may take the `plan` role: online, with a model to offer.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerPlanner {
    pub name: String,
    /// Device class of the paired device (xs..xl).
    pub tier: String,
    /// The model to ask for on its gateway.
    pub model: String,
}

/// What `roles: "auto"` assigns to a role instead of the agent's own model.
#[derive(Debug, Clone, PartialEq)]
pub struct RolePick {
    pub runtime_id: String,
    pub model: String,
    pub name: String,
    pub context: u32,
    /// The model is on a paired device, not here.
    pub peer: bool,
}

/// The decision of `roles: "auto"` for a device class. A role left `None` runs on the agent's own
/// model; `check` is never assigned a model of its own by `auto` (it asks the model that did the work).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutoRolePicks {
    pub tier: String,
    pub plan: Option<RolePick>,
    pub act: Option<RolePick>,
    pub summarize: Option<RolePick>,
    /// Why, one sentence each, in Italian (run events).
    pub notes: Vec<String>,
}

fn tier_rank(t: &str) -> usize {
    ["xs", "s", "m", "l", "xl"].iter().position(|x| *x == t).unwrap_or(0)
}

/// Memory a candidate takes while loaded (system models and unknown sizes take none of ours).
fn need_mb(c: &Candidate) -> u64 {
    if c.system || c.size_bytes == 0 {
        0
    } else {
        aiw_models::min_memory_mb(c.size_bytes, 0)
    }
}

/// Do these models fit in memory together, by the same formula as one model (`fit_need`: the
/// summed need against the device budget, `ok` or better)?
fn fit_together(device: &DeviceProfile, models: &[&Candidate]) -> bool {
    let mut seen: Vec<&str> = vec![];
    let mut total = 0u64;
    for c in models {
        if !seen.contains(&c.id.as_str()) {
            seen.push(c.id.as_str());
            total += need_mb(c);
        }
    }
    total == 0 || matches!(Fit::from_label(&aiw_models::fit_need(total, "gguf", device).0), Fit::Great | Fit::Ok)
}

fn role_pick(c: &Candidate, task: AdviserTask) -> RolePick {
    RolePick {
        runtime_id: c.runtime.clone(),
        model: c.id.clone(),
        name: c.name.clone(),
        context: match task {
            AdviserTask::Embeddings => 2048,
            _ => context_for(c.context, c.fit),
        },
        peer: false,
    }
}

/// Decides `roles: "auto"` from the device class (see the module documentation of the roles in
/// `aiw_agent::roles`). Pure: the installed models, the catalog, what the runtimes can serve, the
/// agent's own model and a paired device that could plan.
/// - xs, s: one model only (the roles differ by reasoning and prompt, not by model);
/// - m: the agent's model plus the smallest installed chat model for `summarize`, only if both fit
///   in memory together;
/// - l, xl: a `plan` model (adviser `reasoning`) and an `act` model (adviser `agent`) when they are
///   two different installed models that fit together; the smallest chat model for `summarize` when
///   it fits as well;
/// - a paired device of a higher class takes `plan` at any class.
///
/// A second model is never picked if it does not fit: the answer is then one model.
pub fn plan_auto_roles(
    device: &DeviceProfile,
    local: &[LocalModel],
    catalog: &[CatalogModel],
    avail: &Availability,
    main: &MainModel,
    peer: Option<&PeerPlanner>,
) -> AutoRolePicks {
    let rec = advise(device, local, catalog, avail);
    let tier = rec.tier.clone();
    let mut installed: Vec<Candidate> = local.iter().filter_map(|m| local_candidate(m, device, avail, catalog)).collect();
    installed.extend(avail.ollama_models.iter().map(|n| ollama_candidate(n)));
    let mut out = AutoRolePicks { tier: tier.clone(), ..Default::default() };
    let main_c = installed
        .iter()
        .find(|c| c.id == main.model && c.runtime == main.runtime_id)
        .or_else(|| installed.iter().find(|c| c.id == main.model));

    if let Some(p) = peer.filter(|p| tier_rank(&p.tier) > tier_rank(&tier) && tier_rank(&p.tier) >= tier_rank("l")) {
        out.plan =
            Some(RolePick { runtime_id: "aiw-peer".into(), model: p.model.clone(), name: p.name.clone(), context: 8192, peer: true });
        out.notes.push(format!("Il piano lo fa «{}», il dispositivo collegato più potente.", p.name));
    }
    let mut resident: Vec<&Candidate> = main_c.into_iter().collect();

    match tier.as_str() {
        "l" | "xl" if out.plan.is_none() => {
            let by_task = |a: &TaskAdvice| a.installed.as_ref().and_then(|c| installed.iter().find(|i| i.id == c.model));
            // `plan`: the adviser's reasoning choice. `act`: the adviser's agent choice when it is another
            // model; else the faster alternative: the smallest installed model that still works as an agent.
            let plan_c = by_task(&rec.reasoning);
            let act_c = plan_c.and_then(|plan| match by_task(&rec.agent) {
                Some(a) if a.id != plan.id => Some(a),
                _ => installed
                    .iter()
                    .filter(|c| {
                        c.id != plan.id
                            && c.runtime_ready
                            && c.tools_cap
                            && c.scores.agent >= 2
                            && c.fit >= Fit::Ok
                            && c.params_b < plan.params_b
                    })
                    .max_by(|a, b| {
                        a.scores
                            .agent
                            .cmp(&b.scores.agent)
                            .then(((b.params_b * 10.0) as u32).cmp(&((a.params_b * 10.0) as u32)))
                            .then_with(|| b.id.cmp(&a.id))
                    }),
            });
            match (plan_c, act_c) {
                (Some(plan), Some(act)) => {
                    let mut set = resident.clone();
                    set.extend([plan, act]);
                    if fit_together(device, &set) {
                        out.plan = Some(role_pick(plan, AdviserTask::Reasoning));
                        out.act = Some(role_pick(act, AdviserTask::Agent));
                        resident = set;
                        out.notes.push("Due modelli installati stanno insieme in memoria: uno pianifica, l'altro esegue.".into());
                    } else {
                        out.notes.push("Due modelli adatti, ma insieme non stanno in memoria: ne uso uno solo.".into());
                    }
                }
                _ => out.notes.push("C'è un solo modello adatto tra quelli installati: ne uso uno solo.".into()),
            }
        }
        "l" | "xl" => {}
        "m" => {}
        _ => out
            .notes
            .push("Questo dispositivo regge un solo modello alla volta: i ruoli cambiano solo il ragionamento e il compito.".into()),
    }

    if matches!(tier.as_str(), "m" | "l" | "xl") {
        // The smallest installed chat model, if it is a different (and smaller) one than what runs already.
        let smallest = installed.iter().filter(|c| c.runtime_ready && c.scores.chat > 0 && c.fit >= Fit::Ok).min_by(|a, b| {
            ((a.params_b * 10.0) as u32)
                .cmp(&((b.params_b * 10.0) as u32))
                .then(a.size_bytes.cmp(&b.size_bytes))
                .then_with(|| a.id.cmp(&b.id))
        });
        if let Some(small) = smallest {
            let known_main = resident.iter().map(|c| c.params_b).fold(f64::MAX, f64::min);
            let is_new = !resident.iter().any(|c| c.id == small.id);
            let smaller = resident.is_empty() || small.params_b < known_main;
            if is_new && smaller {
                let mut set = resident.clone();
                set.push(small);
                if fit_together(device, &set) {
                    out.summarize = Some(role_pick(small, AdviserTask::Chat));
                    out.notes.push(format!("Per i riassunti uso il modello più piccolo ({}).", small.name));
                } else {
                    out.notes
                        .push("Il modello piccolo per i riassunti non sta in memoria insieme agli altri: uso quello principale.".into());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiw_core::api::{Accelerator, CpuInfo};
    use serde_json::json;

    const GB: u64 = 1024 * 1024 * 1024;

    pub(crate) fn device(os: &str, ram_gb: u64, vram_gb: Option<u64>) -> DeviceProfile {
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

    fn avail() -> Availability {
        Availability { llama_server: true, llama_embedded: false, ollama_models: vec![] }
    }

    /// An installed model, named after its catalog entry so the scores come from the catalog.
    fn installed(cat_id: &str) -> LocalModel {
        let c = aiw_models::builtin_catalog().into_iter().find(|c| c.id == cat_id).unwrap();
        let file = catalog_file(&c).unwrap_or("x.gguf").to_string();
        serde_json::from_value(json!({
            "id": file.trim_end_matches(".gguf").to_lowercase(), "name": c.name, "path": format!("/models/{file}"), "format": c.format,
            "size_bytes": c.size_bytes, "context_length": c.context_length, "capabilities": c.capabilities, "runtimes": ["llama-server"],
            "params_b": c.params_b,
        }))
        .unwrap()
    }

    fn catalog() -> Vec<CatalogModel> {
        aiw_models::builtin_catalog()
    }

    fn model_ids(a: &TaskAdvice) -> (Option<String>, Option<String>) {
        (a.installed.as_ref().map(|c| c.model.clone()), a.download.as_ref().and_then(|c| c.catalog_id.clone()))
    }

    // One fake device per class (budget: xs < 4 GB, s < 8, m < 16, l < 32, xl above).
    fn classes() -> Vec<(&'static str, DeviceProfile)> {
        vec![
            ("xs", device("android", 4, None)),
            ("s", device("linux", 8, None)),
            ("m", device("linux", 24, None)),
            ("l", device("linux", 48, Some(16))),
            ("xl", device("linux", 128, Some(48))),
        ]
    }

    #[test]
    fn the_fake_devices_are_of_the_class_they_claim() {
        for (tier, d) in classes() {
            assert_eq!(aiw_hw::tier_for(&d), tier);
            assert_eq!(advise(&d, &[], &catalog(), &avail()).tier, tier);
        }
    }

    #[test]
    fn nothing_installed_recommends_a_download_that_fits_for_each_class() {
        let expected_agent =
            [("xs", "llama-3.2-3b"), ("s", "llama-3.2-3b"), ("m", "qwen3-8b"), ("l", "gpt-oss-20b"), ("xl", "llama-3.3-70b")];
        for (tier, d) in classes() {
            let rec = advise(&d, &[], &catalog(), &avail());
            assert!(rec.agent.installed.is_none(), "{tier}");
            let dl = rec.agent.download.clone().unwrap_or_else(|| panic!("{tier}: no download"));
            assert!(dl.fit == "great" || dl.fit == "ok", "{tier}: {}", dl.fit);
            assert_eq!(dl.catalog_id.as_deref(), expected_agent.iter().find(|(t, _)| *t == tier).map(|(_, m)| *m), "{tier}");
            assert!(!dl.reason.is_empty());
        }
    }

    #[test]
    fn every_task_gets_a_suited_model_on_a_laptop() {
        let rec = advise(&device("linux", 16, None), &[], &catalog(), &avail());
        let id = |a: &TaskAdvice| a.download.as_ref().unwrap().catalog_id.clone().unwrap();
        assert_eq!(id(&rec.chat), "qwen3-8b");
        assert_eq!(id(&rec.agent), "qwen3-8b");
        let r = rec.reasoning.download.as_ref().unwrap();
        assert_eq!((r.catalog_id.as_deref(), r.thinking), (Some("qwen3-8b"), Some(ThinkingMode::Deep)));
        assert!(matches!(id(&rec.vision).as_str(), "gemma-3-4b" | "gemma-3n-e4b" | "qwen2.5-vl-7b"));
        assert_eq!(id(&rec.embeddings), "qwen3-embedding-0.6b");
        // Chat on a reasoning model runs with the reasoning off.
        assert_eq!(rec.chat.download.as_ref().unwrap().thinking, Some(ThinkingMode::Off));
    }

    #[test]
    fn an_installed_model_that_suits_is_chosen_and_no_download_is_suggested() {
        let d = device("linux", 16, None);
        let rec = advise(&d, &[installed("qwen3-4b")], &catalog(), &avail());
        assert_eq!(model_ids(&rec.chat).0.as_deref(), Some("qwen3-4b-q4_k_m"));
        let agent = rec.agent.installed.as_ref().unwrap();
        assert_eq!((agent.runtime_id.as_str(), agent.catalog_id.as_deref()), ("llama-server", Some("qwen3-4b")));
        // It is a good-enough agent (score 2) but the 8B (score 3) fits too: a clearly better download.
        assert_eq!(rec.agent.download.as_ref().unwrap().catalog_id.as_deref(), Some("qwen3-8b"));
        // The same model is already the best chat: nothing to download.
        assert!(rec.chat.download.is_none());
        // Never suggests what is installed.
        assert_ne!(rec.reasoning.download.as_ref().and_then(|c| c.catalog_id.clone()).as_deref(), Some("qwen3-4b"));
        // Nothing installed can see or embed: those tasks only have downloads.
        assert!(rec.vision.installed.is_none() && rec.vision.download.is_some());
        assert!(rec.embeddings.installed.is_none() && rec.embeddings.download.is_some());
    }

    #[test]
    fn too_big_models_are_not_chosen_and_a_smaller_download_is_proposed() {
        let laptop = device("linux", 16, None);
        let rec = advise(&laptop, &[installed("llama-3.3-70b")], &catalog(), &avail());
        assert!(rec.agent.installed.is_none());
        assert_eq!(rec.agent.download.as_ref().unwrap().catalog_id.as_deref(), Some("qwen3-8b"));
        // On a workstation the 70B only fits with partial offload: last resort, and a download beats it.
        let ws = device("windows", 64, Some(24));
        let rec = advise(&ws, &[installed("llama-3.3-70b")], &catalog(), &avail());
        assert_eq!(rec.agent.installed.as_ref().unwrap().fit, "tight");
        assert!(rec.agent.download.is_some());
    }

    #[test]
    fn reasoning_falls_back_to_the_agent_choice_without_thinking() {
        let d = device("linux", 16, None);
        let rec = advise(&d, &[installed("llama-3.2-3b")], &catalog(), &avail());
        let r = rec.reasoning.installed.as_ref().unwrap();
        assert_eq!((r.model.as_str(), r.thinking), ("llama-3.2-3b-instruct-q4_k_m", None));
        assert!(r.reason.contains("Non ragiona a fondo"));
        // …and a model that really reasons is proposed.
        assert_eq!(rec.reasoning.download.as_ref().unwrap().thinking, Some(ThinkingMode::Deep));
    }

    #[test]
    fn vision_needs_the_vision_capability() {
        let d = device("linux", 16, None);
        let rec = advise(&d, &[installed("qwen3-8b"), installed("gemma-3-4b")], &catalog(), &avail());
        assert_eq!(rec.vision.installed.as_ref().unwrap().catalog_id.as_deref(), Some("gemma-3-4b"));
    }

    #[test]
    fn a_system_model_wins_chat_when_available() {
        let phone = device("ios", 8, None);
        let apple: LocalModel = serde_json::from_value(json!({
            "id": "apple-fm-system", "name": "Apple Foundation Models", "format": "system", "size_bytes": 0,
            "capabilities": ["chat", "tools"], "runtimes": ["apple-fm"], "origin": "system", "params_b": 3.0,
        }))
        .unwrap();
        let rec = advise(
            &phone,
            &[apple.clone(), installed("qwen3-4b")],
            &catalog(),
            &Availability { llama_embedded: true, ..Default::default() },
        );
        let chat = rec.chat.installed.as_ref().unwrap();
        assert_eq!((chat.model.as_str(), chat.runtime_id.as_str()), ("apple-fm-system", "apple-fm"));
        assert!(chat.reason.contains("già nel sistema"));
        // For agent work the larger model with tools is not displaced by the system one.
        assert_eq!(rec.agent.installed.as_ref().unwrap().catalog_id.as_deref(), Some("qwen3-4b"));
    }

    #[test]
    fn phones_use_the_embedded_engine_and_ollama_models_are_candidates() {
        let phone = device("android", 8, None);
        let rec = advise(&phone, &[installed("qwen3-1.7b")], &catalog(), &Availability { llama_embedded: true, ..Default::default() });
        assert_eq!(rec.agent.installed.as_ref().unwrap().runtime_id, "llama-embedded");
        let laptop = device("linux", 16, None);
        let ollama = Availability { ollama_models: vec!["nomic-embed-text:latest".into(), "qwen3:4b".into()], ..Default::default() };
        let rec = advise(&laptop, &[], &catalog(), &ollama);
        let a = rec.agent.installed.as_ref().unwrap();
        assert_eq!((a.runtime_id.as_str(), a.model.as_str()), ("ollama", "qwen3:4b"));
        assert_eq!(rec.embeddings.installed.as_ref().unwrap().model, "nomic-embed-text:latest");
    }

    #[test]
    fn the_answer_does_not_depend_on_the_order_of_the_inputs() {
        let d = device("linux", 24, None);
        let mut local = vec![installed("qwen3-4b"), installed("qwen3-8b"), installed("gemma-3-4b"), installed("llama-3.2-3b")];
        let mut cat = catalog();
        let a = advise(&d, &local, &cat, &avail());
        local.reverse();
        cat.reverse();
        assert_eq!(a, advise(&d, &local, &cat, &avail()));
    }

    #[test]
    fn the_catalog_scores_follow_the_declared_capabilities() {
        for c in catalog() {
            let s = c.suitability.unwrap_or_else(|| panic!("{} has no suitability", c.id));
            assert_eq!(s, derive_scores(&c.capabilities, c.params_b, &c.tags), "{}", c.id);
            let zero_iff = |score: u8, ok: bool| assert_eq!(score == 0, !ok, "{}", c.id);
            let caps = |k: &str| c.capabilities.iter().any(|x| x == k);
            zero_iff(s.chat, caps("chat"));
            zero_iff(s.agent, caps("chat") && caps("tools"));
            zero_iff(s.reasoning, caps("chat") && caps("tools") && caps("reasoning"));
            zero_iff(s.vision, caps("chat") && caps("vision"));
            zero_iff(s.embeddings, caps("embeddings"));
            assert!([s.chat, s.agent, s.reasoning, s.vision, s.embeddings].iter().all(|v| *v <= 3));
        }
    }

    #[test]
    fn a_catalog_without_scores_still_works() {
        let mut cat = catalog();
        for c in cat.iter_mut() {
            c.suitability = None;
        }
        let d = device("linux", 16, None);
        assert_eq!(advise(&d, &[], &cat, &avail()), advise(&d, &[], &catalog(), &avail()));
    }

    #[test]
    fn auto_profile_ids_and_profiles() {
        assert_eq!(parse_auto_profile("auto:agent"), Some(AdviserTask::Agent));
        assert_eq!(parse_auto_profile("auto:reasoning"), Some(AdviserTask::Reasoning));
        assert_eq!(parse_auto_profile("auto:embeddings"), None);
        assert_eq!(parse_auto_profile("auto:nope"), None);
        assert_eq!(parse_auto_profile("default"), None);
        let d = device("linux", 16, None);
        let rec = advise(&d, &[installed("qwen3-8b")], &catalog(), &avail());
        let p = auto_profile(AdviserTask::Reasoning, &rec).unwrap();
        assert_eq!((p.id.as_str(), p.runtime_id.as_str(), p.model.as_str()), ("auto:reasoning", "llama-server", "qwen3-8b-q4_k_m"));
        assert_eq!(p.params.thinking, Some(ThinkingMode::Deep));
        assert!(p.params.context.unwrap() >= 2048);
        assert!(auto_profile(AdviserTask::Vision, &rec).is_none());
    }

    #[test]
    fn name_and_size_guesses() {
        assert_eq!(params_from_name("qwen3:4b"), Some(4.0));
        assert_eq!(params_from_name("llama3.2:3b-instruct"), Some(3.0));
        assert_eq!(params_from_name("gemma3:12b"), Some(12.0));
        assert_eq!(params_from_name("nomic-embed-text:latest"), None);
        assert!((estimate_params_b(5 * GB / 2, Some("Q4_K_M")) - 4.47).abs() < 0.1);
    }

    // ---------------------------------------------------------------- roles: auto

    fn main_model(id: &str) -> MainModel {
        MainModel { runtime_id: "llama-server".into(), model: id.into() }
    }

    /// `main` is a catalog id; the model id it gets once installed is used for the agent's own model.
    fn picks(d: &DeviceProfile, local: &[&str], main: &str, peer: Option<&PeerPlanner>) -> AutoRolePicks {
        let local: Vec<LocalModel> = local.iter().map(|c| installed(c)).collect();
        plan_auto_roles(d, &local, &catalog(), &avail(), &main_model(&installed(main).id), peer)
    }

    fn id_of(p: &Option<RolePick>) -> Option<&str> {
        p.as_ref().map(|p| p.model.as_str())
    }

    #[test]
    fn small_devices_use_one_model_only() {
        for (tier, d) in classes().into_iter().filter(|(t, _)| *t == "xs" || *t == "s") {
            let p = picks(&d, &["qwen3-0.6b", "llama-3.2-3b"], "llama-3.2-3b", None);
            assert_eq!((p.plan, p.act, p.summarize), (None, None, None), "{tier}");
            assert_eq!(p.tier, tier);
            assert!(p.notes[0].contains("un solo modello"), "{tier}: {:?}", p.notes);
        }
    }

    #[test]
    fn a_medium_device_adds_the_smallest_chat_model_for_summaries_when_both_fit() {
        let d = device("linux", 24, None);
        assert_eq!(aiw_hw::tier_for(&d), "m");
        let p = picks(&d, &["qwen3-8b", "qwen3-0.6b"], "qwen3-8b", None);
        assert_eq!(id_of(&p.summarize), Some(installed("qwen3-0.6b").id.as_str()));
        assert_eq!((p.plan, p.act), (None, None));
    }

    #[test]
    fn a_medium_device_with_no_smaller_model_or_no_room_stays_with_one() {
        let d = device("linux", 24, None);
        // Only the main model: nothing smaller.
        assert_eq!(picks(&d, &["qwen3-8b"], "qwen3-8b", None).summarize, None);
        // The small one is not smaller than main (main is the 0.6B).
        assert_eq!(picks(&d, &["qwen3-8b", "qwen3-0.6b"], "qwen3-0.6b", None).summarize, None);
        // Not enough memory for both: 16 GB laptop (8 GB budget) with the 8B (~6.4 GB) and a 4B (~3.2 GB).
        let tight = device("linux", 16, None);
        assert_eq!(aiw_hw::tier_for(&tight), "m");
        let p = picks(&tight, &["qwen3-8b", "qwen3-4b"], "qwen3-8b", None);
        assert_eq!(p.summarize, None);
        assert!(p.notes.iter().any(|n| n.contains("non sta in memoria")), "{:?}", p.notes);
    }

    #[test]
    fn large_devices_split_plan_and_act_when_two_models_fit_together() {
        for (tier, d) in classes().into_iter().filter(|(t, _)| *t == "l" || *t == "xl") {
            let p = picks(&d, &["qwen3-8b", "llama-3.2-3b"], "llama-3.2-3b", None);
            let (plan, act) = (id_of(&p.plan), id_of(&p.act));
            assert!(plan.is_some() && act.is_some() && plan != act, "{tier}: {p:?}");
        }
    }

    #[test]
    fn large_devices_with_one_suitable_model_or_no_room_use_one_model() {
        let l = device("linux", 48, Some(16));
        // One installed model only: reasoning falls back to the agent's choice, the same model.
        let p = picks(&l, &["qwen3-8b"], "qwen3-8b", None);
        assert_eq!((p.plan, p.act), (None, None));
        // Two suitable models that do not fit together: a 22 GB budget, a ~20 GB and a ~3 GB model.
        let l = device("linux", 44, None);
        assert_eq!(aiw_hw::tier_for(&l), "l");
        let p = picks(&l, &["qwen3-30b-a3b", "llama-3.2-3b"], "qwen3-30b-a3b", None);
        assert_eq!((p.plan.clone(), p.act.clone()), (None, None), "{:?}", p.notes);
        assert!(p.notes.iter().any(|n| n.contains("non stanno in memoria")), "{:?}", p.notes);
    }

    #[test]
    fn a_more_powerful_paired_device_takes_the_plan_at_any_class() {
        let peer = PeerPlanner { name: "Studio".into(), tier: "xl".into(), model: "gpt-oss-120b".into() };
        for (tier, d) in classes() {
            let p = picks(&d, &["llama-3.2-3b"], "llama-3.2-3b", Some(&peer));
            if tier == "xl" {
                // Not more powerful than this device: it does not take the plan.
                assert!(p.plan.is_none(), "{tier}");
            } else {
                let plan = p.plan.as_ref().unwrap_or_else(|| panic!("{tier}"));
                assert!(
                    (plan.peer, plan.runtime_id.as_str(), plan.model.as_str()) == (true, "aiw-peer", "gpt-oss-120b"),
                    "{tier}: {plan:?}"
                );
            }
        }
        // A weaker paired device never does.
        let weak = PeerPlanner { name: "Telefono".into(), tier: "s".into(), model: "x".into() };
        assert!(picks(&device("linux", 24, None), &[], "qwen3-8b", Some(&weak)).plan.is_none());
        // Nor one that is only as strong as "m".
        let mid = PeerPlanner { name: "Portatile".into(), tier: "m".into(), model: "x".into() };
        assert!(picks(&device("android", 4, None), &[], "qwen3-8b", Some(&mid)).plan.is_none());
    }
}
