//! Model roles of an agent run (`AgentProfile.roles`): turns the agent's setting into the
//! [`RolePlan`] the loop uses. `auto` is decided by [`crate::adviser::plan_auto_roles`] from the
//! device class; a custom plan names a profile for each role. Whatever the choice, two roles that
//! would make a single-model runtime swap models are collapsed to one model
//! ([`aiw_agent::roles::collapse_single_model_conflicts`]) and the reason goes in the run events.

use crate::adviser::{self, MainModel, PeerPlanner, RolePick};
use crate::{defaults, Engine};
use aiw_agent::roles::{collapse_single_model_conflicts, RoleModel, RolePlan};
use aiw_core::api::{AgentProfile, AgentRoles, InferenceProfile, ModelRole, ThinkingMode};
use std::collections::BTreeMap;

/// Models the name of which says they reason (a paired device's gateway only lists names).
fn name_reasons(model: &str) -> bool {
    let l = model.to_lowercase();
    ["qwen3", "deepseek-r1", "gpt-oss", "think"].iter().any(|k| l.contains(k))
}

impl Engine {
    /// The model plan of a run of `agent` whose own model is `base`; `None` when the agent has no
    /// `roles` (one model for everything, as before).
    pub(crate) fn resolve_roles(&self, agent: &AgentProfile, base: &InferenceProfile) -> Option<RolePlan> {
        let roles = agent.roles.as_ref()?;
        let mut plan = RolePlan {
            base_can_reason: self.can_reason(base),
            mode: match roles {
                AgentRoles::Auto(_) => "auto".into(),
                AgentRoles::Custom(_) => "custom".into(),
            },
            ..Default::default()
        };
        let mut picks: BTreeMap<ModelRole, RoleModel> = BTreeMap::new();
        match roles {
            AgentRoles::Auto(_) => {
                plan.check_enabled = true;
                let main = MainModel { runtime_id: base.runtime_id.clone(), model: base.model.clone() };
                let auto = adviser::plan_auto_roles(
                    &self.device(),
                    &self.live_local_models(),
                    &aiw_models::builtin_catalog(),
                    &defaults::Availability::from_runtimes(&self.0.runtimes.list()),
                    &main,
                    self.peer_planner().as_ref(),
                );
                plan.notes.extend(auto.notes);
                for (role, pick) in [(ModelRole::Plan, auto.plan), (ModelRole::Act, auto.act), (ModelRole::Summarize, auto.summarize)] {
                    if let Some(p) = pick {
                        picks.insert(role, self.role_model(&p));
                    }
                }
            }
            AgentRoles::Custom(rp) => {
                plan.check_enabled = rp.check.is_some();
                for role in ModelRole::ALL {
                    let Some(id) = rp.get(role).filter(|s| !s.is_empty()) else { continue };
                    match self.profile(id) {
                        Ok(p) => {
                            let can_reason = self.can_reason(&p);
                            picks.insert(role, RoleModel { profile: p, can_reason });
                        }
                        // A profile that is gone must not stop the run: that role uses the agent's own model.
                        Err(e) => plan
                            .notes
                            .push(format!("Il profilo «{id}» del ruolo {} non c'è più ({e}): uso il modello dell'agente.", role.as_str())),
                    }
                }
            }
        }
        let (_, note) = collapse_single_model_conflicts(base, &mut picks, |rt| self.0.runtimes.holds_one_model(rt));
        plan.notes.extend(note);
        for (role, m) in picks {
            plan.set(role, Some(m));
        }
        Some(plan)
    }

    fn can_reason(&self, p: &InferenceProfile) -> bool {
        self.thinking_for(p, Some(ThinkingMode::Auto)).is_some() || (p.runtime_id == "aiw-peer" && name_reasons(&p.model))
    }

    fn role_model(&self, p: &RolePick) -> RoleModel {
        let mut profile = defaults::profile(p.name.clone(), &p.runtime_id, &p.model, p.context);
        profile.id = format!("role:{}", p.model);
        profile.name = p.name.clone();
        let can_reason = self.can_reason(&profile);
        RoleModel { profile, can_reason }
    }

    /// The paired device that could take the `plan` role: online, with a model on its gateway, and
    /// the `aiw-peer` runtime pointing at it. The most powerful one if there are several.
    pub(crate) fn peer_planner(&self) -> Option<PeerPlanner> {
        let gateway = self.0.runtimes.settings("aiw-peer").ok()?.get("base_url")?.as_str()?.trim_end_matches('/').to_string();
        let rank = |t: &str| ["xs", "s", "m", "l", "xl"].iter().position(|x| *x == t).unwrap_or(0);
        self.peers()
            .into_iter()
            .filter(|p| p.status == "online" && format!("{}/v1", p.address) == gateway)
            .filter_map(|p| {
                let tier = p.hardware.as_ref()?.tier.clone();
                let model = p.shared_runtimes.iter().find(|m| !defaults::looks_like_embedding_model(m))?.clone();
                Some(PeerPlanner { name: p.name, tier, model })
            })
            .max_by_key(|p| rank(&p.tier))
    }
}
