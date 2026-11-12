//! Model roles inside one run. An agent normally uses one inference profile for every model call;
//! with roles the loop picks the model per call:
//! - `plan`: the first call of the run and the call after a failure (where `thinking: auto` turns
//!   on): the most capable model, reasoning on;
//! - `act`: the calls that carry on after successful tool results: reasoning off;
//! - `check`: a short verification pass before the run declares itself finished, only when the run
//!   used tools that write or act; one extra round at most;
//! - `summarize`: context compaction: the smallest model.
//!
//! The engine decides what each role maps to ([`RolePlan`]); a run without a plan behaves exactly
//! as before. The message history is shared between the roles and is in the neutral internal format
//! (`Msg`: OpenAI-style `tool_calls` with ids and JSON arguments), which every backend translates
//! to what its model's chat template wants, so a model never sees another model's tool syntax.

use crate::runner::RunHost;
use crate::thinking::{self, CallSite};
use aiw_core::api::{InferenceProfile, ModelRole, ThinkingMode};
use aiw_runtimes::{ChatBackend, Msg, ThinkingRequest};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// One role that does not use the agent's own profile.
#[derive(Debug, Clone, PartialEq)]
pub struct RoleModel {
    pub profile: InferenceProfile,
    /// The model has the `reasoning` capability (else nothing is sent about thinking).
    pub can_reason: bool,
}

/// The models of a run by role; `None` for a role means "the agent's own profile".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RolePlan {
    pub plan: Option<RoleModel>,
    pub act: Option<RoleModel>,
    pub check: Option<RoleModel>,
    pub summarize: Option<RoleModel>,
    /// The agent's own model has the `reasoning` capability.
    pub base_can_reason: bool,
    /// Run the `check` pass (`auto`, or a custom plan that sets `check`).
    pub check_enabled: bool,
    /// `auto` | `custom`.
    pub mode: String,
    /// Why the plan is what it is (a collapse to one model, a model left out): recorded in the
    /// run events, one sentence each.
    pub notes: Vec<String>,
}

impl RolePlan {
    pub fn model(&self, role: ModelRole) -> Option<&RoleModel> {
        match role {
            ModelRole::Plan => self.plan.as_ref(),
            ModelRole::Act => self.act.as_ref(),
            ModelRole::Check => self.check.as_ref(),
            ModelRole::Summarize => self.summarize.as_ref(),
        }
    }
    pub fn set(&mut self, role: ModelRole, m: Option<RoleModel>) {
        match role {
            ModelRole::Plan => self.plan = m,
            ModelRole::Act => self.act = m,
            ModelRole::Check => self.check = m,
            ModelRole::Summarize => self.summarize = m,
        }
    }
    /// How many distinct models the plan uses besides the agent's own.
    pub fn distinct_models(&self, base: &InferenceProfile) -> usize {
        let mut seen: Vec<(&str, &str)> = vec![(base.runtime_id.as_str(), base.model.as_str())];
        for r in ModelRole::ALL {
            if let Some(m) = self.model(r) {
                let key = (m.profile.runtime_id.as_str(), m.profile.model.as_str());
                if !seen.contains(&key) {
                    seen.push(key);
                }
            }
        }
        seen.len()
    }
}

/// Which roles must give up their own model because a runtime that holds a single model would be
/// asked for two different ones (a restart on every switch: thrashing). Pure; `holds_one(runtime)`
/// says whether the runtime can serve only one model at a time.
///
/// Every role that sits on such a runtime together with another model (the agent's own `base`
/// included) goes back to the agent's own profile, so the whole group runs on one model. Returns
/// the roles that collapsed, in role order, and one Italian sentence for the run events.
pub fn collapse_single_model_conflicts(
    base: &InferenceProfile,
    picks: &mut BTreeMap<ModelRole, RoleModel>,
    holds_one: impl Fn(&str) -> bool,
) -> (Vec<ModelRole>, Option<String>) {
    let mut by_runtime: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    by_runtime.entry(base.runtime_id.as_str()).or_default().push(base.model.as_str());
    for m in picks.values() {
        by_runtime.entry(m.profile.runtime_id.as_str()).or_default().push(m.profile.model.as_str());
    }
    let conflicting: Vec<String> = by_runtime
        .iter()
        .filter(|(rt, models)| holds_one(rt) && models.iter().any(|m| *m != models[0]))
        .map(|(rt, _)| rt.to_string())
        .collect();
    if conflicting.is_empty() {
        return (vec![], None);
    }
    // A role on a conflicting runtime that uses the very model of the agent is already fine.
    let collapsed: Vec<ModelRole> = picks
        .iter()
        .filter(|(_, m)| {
            conflicting.contains(&m.profile.runtime_id) && !(m.profile.runtime_id == base.runtime_id && m.profile.model == base.model)
        })
        .map(|(r, _)| *r)
        .collect();
    for r in picks.keys().copied().collect::<Vec<_>>() {
        if conflicting.contains(&picks[&r].profile.runtime_id) {
            picks.remove(&r);
        }
    }
    let names: Vec<&str> = collapsed.iter().map(|r| r.as_str()).collect();
    let note = format!(
        "I ruoli ({}) userebbero due modelli sullo stesso motore ({}), che ne regge uno solo per volta: per non ricaricarlo di continuo il run usa un solo modello.",
        names.join(", "),
        conflicting.join(", ")
    );
    (collapsed, Some(note))
}

/// What to send as reasoning for a call of `role`, with a role plan active.
/// `override_mode` is the run's reasoning mode: `off` is respected everywhere.
pub fn role_thinking(
    role: ModelRole,
    can_reason: bool,
    override_mode: Option<ThinkingMode>,
    budget: Option<u32>,
) -> Option<ThinkingRequest> {
    if !can_reason {
        return None;
    }
    let mode = match (role, override_mode) {
        (_, Some(ThinkingMode::Off)) => ThinkingMode::Off,
        (ModelRole::Plan, Some(ThinkingMode::Deep)) => ThinkingMode::Deep,
        (ModelRole::Plan, _) => ThinkingMode::Auto,
        _ => ThinkingMode::Off,
    };
    // For a plan call `auto` thinks (the call site is a planning one by definition).
    thinking::resolve(Some(mode), budget, CallSite { first_call: role == ModelRole::Plan, after_failure: false })
}

/// The role of a normal loop call: `plan` where `thinking: auto` turns on, `act` otherwise.
pub fn role_for_call(site: CallSite) -> ModelRole {
    if site.first_call || site.after_failure {
        ModelRole::Plan
    } else {
        ModelRole::Act
    }
}

/// The model (and backend) chosen for one call.
#[derive(Clone)]
pub struct Picked {
    pub profile: InferenceProfile,
    pub backend: Arc<dyn ChatBackend>,
    pub can_reason: bool,
    /// The role this call is for (`None`: no role plan in this run).
    pub role: Option<ModelRole>,
}

/// Resolves and keeps the backend of each role of a run. Without a plan every call is the agent's
/// own. A role whose backend cannot be started falls back to the agent's own model (the note goes
/// in `fallbacks`, for the run events).
pub struct Routing {
    base: Picked,
    plan: Option<RolePlan>,
    cache: HashMap<ModelRole, Picked>,
    pub fallbacks: Vec<(ModelRole, String)>,
}

impl Routing {
    pub fn new(base_profile: InferenceProfile, base_backend: Arc<dyn ChatBackend>, plan: Option<RolePlan>) -> Self {
        let can_reason = plan.as_ref().is_some_and(|p| p.base_can_reason);
        Self {
            base: Picked { profile: base_profile, backend: base_backend, can_reason, role: None },
            plan,
            cache: HashMap::new(),
            fallbacks: vec![],
        }
    }

    pub fn active(&self) -> bool {
        self.plan.is_some()
    }

    pub fn plan(&self) -> Option<&RolePlan> {
        self.plan.as_ref()
    }

    pub fn base(&self) -> &Picked {
        &self.base
    }

    pub fn check_enabled(&self) -> bool {
        self.plan.as_ref().is_some_and(|p| p.check_enabled)
    }

    pub async fn pick(&mut self, role: ModelRole, host: &Arc<dyn RunHost>) -> Picked {
        let Some(plan) = &self.plan else { return self.base.clone() };
        if let Some(p) = self.cache.get(&role) {
            return p.clone();
        }
        let picked = match plan.model(role) {
            None => Picked { role: Some(role), can_reason: plan.base_can_reason, ..self.base.clone() },
            Some(m) => match host.backend(&m.profile).await {
                Ok(backend) => Picked { profile: m.profile.clone(), backend, can_reason: m.can_reason, role: Some(role) },
                Err(e) => {
                    self.fallbacks.push((role, e.to_string()));
                    Picked { role: Some(role), can_reason: plan.base_can_reason, ..self.base.clone() }
                }
            },
        };
        self.cache.insert(role, picked.clone());
        picked
    }
}

// ---------------------------------------------------------------------------------- check pass

/// Name of the stored user message that hands the reviewer's objection back to the model.
pub const CHECK_NAME: &str = "aiw.check";
/// Most extra rounds the check can cause in one run.
pub const MAX_CHECK_ROUNDS: u32 = 1;

pub(crate) const CHECK_SYSTEM: &str =
    "Sei un revisore. Ti vengono dati la richiesta dell'utente, le azioni eseguite e la risposta che l'assistente sta per dare. \
Decidi se la richiesta è stata davvero soddisfatta dalle azioni eseguite (e non solo dichiarata). \
Rispondi SOLO con una riga: `PASS: <una frase>` se è tutto a posto, oppure `FAIL: <una frase che dice cosa manca o è sbagliato>`.";

/// The text the reviewer reads: the request, the acting calls with a trimmed result, the answer.
pub(crate) fn check_prompt(user_input: &str, actions: &[(String, String)], answer: &str) -> String {
    let clip = |s: &str, n: usize| -> String { s.chars().take(n).collect() };
    let mut s = format!("# Richiesta dell'utente\n{}\n\n# Azioni eseguite\n", clip(user_input, 1500));
    for (name, result) in actions.iter().rev().take(8).rev() {
        s.push_str(&format!("- {name}: {}\n", clip(result, 300).replace('\n', " ")));
    }
    s.push_str(&format!("\n# Risposta che l'assistente sta per dare\n{}\n", clip(answer, 1500)));
    s
}

/// The verdict of the reviewer: `Some(sentence)` when it found a problem. An answer that is not in
/// the expected form passes: a confused small model must not block a finished run.
pub(crate) fn parse_check(answer: &str) -> Option<String> {
    let line = answer.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.trim_start_matches(['`', '*', ' ']);
    let lower = line.to_lowercase();
    let word_end = lower.find(|c: char| !c.is_alphabetic()).unwrap_or(lower.len());
    if &lower[..word_end] != "fail" {
        return None;
    }
    let rest = line[word_end..].trim_start_matches([':', '-', '.', ' ', '`', '*']).trim().trim_end_matches('`').trim();
    Some(if rest.is_empty() { "il revisore non ha trovato il lavoro completo".into() } else { rest.chars().take(300).collect() })
}

pub(crate) fn feedback_note(sentence: &str) -> String {
    format!("[Verifica finale] Un controllo prima di concludere ha trovato un problema: {sentence}\nCorreggi quello che manca e poi rispondi; se non è possibile, dillo chiaramente all'utente.")
}

pub(crate) fn check_messages(user_input: &str, actions: &[(String, String)], answer: &str) -> Vec<Msg> {
    vec![Msg::system(CHECK_SYSTEM), Msg::user(check_prompt(user_input, actions, answer))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiw_core::api::InferenceParams;

    fn prof(rt: &str, model: &str) -> InferenceProfile {
        InferenceProfile {
            id: format!("{rt}/{model}"),
            name: model.into(),
            description: None,
            runtime_id: rt.into(),
            model: model.into(),
            params: InferenceParams::default(),
        }
    }
    fn rm(rt: &str, model: &str) -> RoleModel {
        RoleModel { profile: prof(rt, model), can_reason: true }
    }
    fn single(rt: &str) -> bool {
        rt == "llama-server"
    }

    #[test]
    fn two_models_on_a_single_model_runtime_collapse_to_the_agents_own() {
        let base = prof("llama-server", "act-7b");
        let mut picks = BTreeMap::from([(ModelRole::Plan, rm("llama-server", "plan-14b")), (ModelRole::Act, rm("llama-server", "act-7b"))]);
        let (collapsed, note) = collapse_single_model_conflicts(&base, &mut picks, single);
        assert_eq!(collapsed, vec![ModelRole::Plan]);
        assert!(picks.is_empty(), "the whole group goes back to the agent's model: {picks:?}");
        assert!(note.unwrap().contains("un solo modello"));
    }

    #[test]
    fn the_same_model_on_one_runtime_is_not_a_conflict() {
        let base = prof("llama-server", "m");
        let mut picks = BTreeMap::from([(ModelRole::Summarize, rm("llama-server", "m"))]);
        assert_eq!(collapse_single_model_conflicts(&base, &mut picks, single), (vec![], None));
        assert_eq!(picks.len(), 1);
    }

    #[test]
    fn a_runtime_that_holds_several_models_keeps_them() {
        let base = prof("ollama", "a");
        let mut picks = BTreeMap::from([(ModelRole::Plan, rm("ollama", "b"))]);
        assert_eq!(collapse_single_model_conflicts(&base, &mut picks, single), (vec![], None));
        assert_eq!(picks.len(), 1);
    }

    #[test]
    fn different_runtimes_do_not_conflict_and_other_groups_are_left_alone() {
        let base = prof("llama-server", "a");
        let mut picks = BTreeMap::from([
            (ModelRole::Plan, rm("aiw-peer", "big")),
            (ModelRole::Summarize, rm("llama-server", "tiny")),
            (ModelRole::Check, rm("ollama", "c")),
        ]);
        let (collapsed, _) = collapse_single_model_conflicts(&base, &mut picks, single);
        assert_eq!(collapsed, vec![ModelRole::Summarize]);
        assert_eq!(picks.keys().copied().collect::<Vec<_>>(), vec![ModelRole::Plan, ModelRole::Check]);
    }

    #[test]
    fn roles_pick_their_call_site() {
        assert_eq!(role_for_call(CallSite { first_call: true, after_failure: false }), ModelRole::Plan);
        assert_eq!(role_for_call(CallSite { first_call: false, after_failure: true }), ModelRole::Plan);
        assert_eq!(role_for_call(CallSite::default()), ModelRole::Act);
    }

    #[test]
    fn plan_thinks_act_does_not_and_off_wins() {
        let on = role_thinking(ModelRole::Plan, true, None, None).unwrap();
        assert!(on.enabled && !on.deep);
        assert!(role_thinking(ModelRole::Plan, true, Some(ThinkingMode::Deep), None).unwrap().deep);
        assert!(!role_thinking(ModelRole::Act, true, Some(ThinkingMode::Deep), None).unwrap().enabled);
        assert!(!role_thinking(ModelRole::Plan, true, Some(ThinkingMode::Off), None).unwrap().enabled);
        assert_eq!(role_thinking(ModelRole::Plan, false, None, None), None);
    }

    #[test]
    fn the_verdict_is_read_tolerantly_and_garbage_passes() {
        assert_eq!(parse_check("PASS: tutto fatto"), None);
        assert_eq!(parse_check("FAIL: manca il file out.txt").as_deref(), Some("manca il file out.txt"));
        assert_eq!(parse_check("\n  fail - il comando è fallito").as_deref(), Some("il comando è fallito"));
        assert_eq!(parse_check("`FAIL: no`").as_deref(), Some("no"));
        assert_eq!(parse_check("Non saprei dire"), None);
        assert_eq!(parse_check(""), None);
        assert!(parse_check("FAIL").is_some());
    }

    #[test]
    fn the_review_prompt_keeps_only_the_last_actions_trimmed() {
        let actions: Vec<(String, String)> = (0..12).map(|i| (format!("t{i}"), "x".repeat(1000))).collect();
        let p = check_prompt("scrivi", &actions, "fatto");
        assert!(p.contains("t11") && p.contains("t4") && !p.contains("t3:"));
        assert!(p.len() < 4000);
    }
}
