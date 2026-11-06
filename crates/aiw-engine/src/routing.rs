//! Router a bandit: an agent with `candidate_profiles` tries its profiles and keeps using the ones
//! that earn thumbs up and passed evaluation tasks. UCB1 over rewards in 0..1.
//!
//! A reward has a key (`fb:<session>:<message>`, `eval:<report>:<task>:<attempt>`), so that changing
//! a thumb replaces the old reward instead of adding one, and taking it back removes it.

use crate::Engine;
use aiw_core::api::{AgentProfile, InferenceProfile, RoutingCandidate, RoutingStats};
use aiw_core::{Error, Result};
use serde::{Deserialize, Serialize};

/// Which profile ran a run, saved when the run starts: a thumb on its answer is a reward for it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RunRoute {
    pub agent_id: String,
    pub profile_id: String,
}

/// The profile the router picked and why (shown in the `aiw.route` event).
#[derive(Debug, Clone, PartialEq)]
pub struct RouteChoice {
    pub profile: String,
    pub reason: String,
}

/// UCB1: the index of the candidate to try next, given `(n, reward_sum)` for each, and the reason.
/// A candidate never rewarded is tried first (the earliest in the list); otherwise the highest
/// `mean + sqrt(2 ln N / n)`, the earliest on a tie.
pub fn ucb1_pick(stats: &[(u64, f64)]) -> Option<(usize, String)> {
    if stats.is_empty() {
        return None;
    }
    if let Some(i) = stats.iter().position(|(n, _)| *n == 0) {
        return Some((i, "mai provato".into()));
    }
    let total: f64 = stats.iter().map(|(n, _)| *n as f64).sum();
    let score = |(n, sum): &(u64, f64)| {
        let n = *n as f64;
        (sum / n, (2.0 * total.ln() / n).sqrt())
    };
    let mut best = (0usize, f64::NEG_INFINITY);
    for (i, s) in stats.iter().enumerate() {
        let (mean, bonus) = score(s);
        if mean + bonus > best.1 + 1e-12 {
            best = (i, mean + bonus);
        }
    }
    let (mean, bonus) = score(&stats[best.0]);
    Some((best.0, format!("UCB1: media {mean:.2} + esplorazione {bonus:.2}")))
}

impl Engine {
    fn bandit_enabled(&self) -> bool {
        self.0.config.get("routing.bandit").and_then(|v| v.as_bool()).unwrap_or(false)
    }

    /// The candidates of `agent` that still exist, with what they earned so far.
    fn routing_candidates(&self, agent: &AgentProfile) -> Result<Vec<RoutingCandidate>> {
        let rewards = self.0.store.profile_rewards(&agent.id)?;
        Ok(agent
            .candidate_profiles
            .iter()
            .filter(|p| self.profile(p).is_ok())
            .map(|p| {
                let (n, sum) = rewards.iter().find(|(id, ..)| id == p).map(|(_, n, s)| (*n, *s)).unwrap_or((0, 0.0));
                RoutingCandidate { profile: p.clone(), n, reward_sum: sum, mean: (n > 0).then(|| sum / n as f64) }
            })
            .collect())
    }

    /// The statistics of an agent's candidates.
    pub fn routing_stats(&self, agent_id: &str) -> Result<RoutingStats> {
        let agent = self.agent(agent_id)?;
        Ok(RoutingStats { enabled: self.bandit_enabled(), candidates: self.routing_candidates(&agent)? })
    }

    /// Forgets what the router learned about an agent.
    pub fn reset_routing(&self, agent_id: &str) -> Result<()> {
        self.agent(agent_id)?;
        self.0.store.reset_profile_rewards(agent_id)?;
        self.audit_user("routing.reset", agent_id);
        Ok(())
    }

    /// The profile for a new top-level run of `agent`, when the router is on and the agent has
    /// candidates; `None` leaves the agent's own profile.
    pub(crate) fn route_profile(&self, agent: &AgentProfile) -> Option<(InferenceProfile, RouteChoice)> {
        if agent.candidate_profiles.is_empty() || !self.bandit_enabled() {
            return None;
        }
        let candidates = match self.routing_candidates(agent) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("router {}: {e}", agent.id);
                return None;
            }
        };
        let stats: Vec<(u64, f64)> = candidates.iter().map(|c| (c.n, c.reward_sum)).collect();
        let (i, reason) = ucb1_pick(&stats)?;
        let profile = self.profile(&candidates[i].profile).ok()?;
        Some((profile.clone(), RouteChoice { profile: profile.id, reason }))
    }

    pub(crate) fn remember_route(&self, run_id: &str, agent_id: &str, profile_id: &str) {
        let route = RunRoute { agent_id: agent_id.into(), profile_id: profile_id.into() };
        if let Err(e) = self.0.store.put_doc("route", run_id, &route) {
            tracing::warn!("router: {e}");
        }
    }

    /// A thumb on an answer is a reward for the profile that wrote it, when the run was routed.
    pub(crate) fn reward_from_feedback(&self, session_id: &str, message_id: &str, run_id: Option<&str>, rating: i32) {
        let Some(run_id) = run_id else { return };
        let Ok(Some(route)) = self.0.store.get_doc::<RunRoute>("route", run_id) else { return };
        let key = format!("fb:{session_id}:{message_id}");
        if let Err(e) = self.0.store.set_profile_reward(&route.agent_id, &route.profile_id, &key, if rating > 0 { 1.0 } else { 0.0 }) {
            tracing::warn!("router: {e}");
        }
    }

    pub(crate) fn forget_feedback_reward(&self, session_id: &str, message_id: &str) {
        let _ = self.0.store.delete_profile_reward(&format!("fb:{session_id}:{message_id}"));
    }

    /// Passed evaluation tasks are 1 and failed ones 0, for the profile the task ran with, when the
    /// agent lists it among its candidates.
    pub(crate) fn rewards_from_report(
        &self,
        suite: &aiw_eval::Suite,
        req_agent: Option<&str>,
        req_profile: Option<&str>,
        report: &aiw_eval::EvalReport,
    ) {
        for t in &report.tasks {
            if t.run_id.is_none() {
                continue;
            }
            let Some(task) = suite.task(&t.task_id) else { continue };
            let agent_id = req_agent
                .map(str::to_string)
                .or_else(|| task.agent.clone())
                .or_else(|| suite.agent.clone())
                .unwrap_or_else(|| aiw_eval::DEFAULT_AGENT.into());
            let Ok(agent) = self.agent(&agent_id) else { continue };
            let profile = req_profile
                .map(str::to_string)
                .or_else(|| task.profile.clone())
                .or_else(|| suite.profile.clone())
                .unwrap_or_else(|| agent.inference_profile.clone());
            if !agent.candidate_profiles.contains(&profile) {
                continue;
            }
            let key = format!("eval:{}:{}:{}", report.id, t.task_id, t.attempt);
            if let Err(e) = self.0.store.set_profile_reward(&agent_id, &profile, &key, if t.passed { 1.0 } else { 0.0 }) {
                tracing::warn!("router: {e}");
            }
        }
    }
}

/// Candidates must be existing profiles, listed once.
pub(crate) fn check_candidates(engine: &Engine, agent: &AgentProfile) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for p in &agent.candidate_profiles {
        if !seen.insert(p) {
            return Err(Error::Invalid(format!("il profilo candidato `{p}` è elencato due volte")));
        }
        if engine.profile(p).is_err() {
            return Err(Error::Invalid(format!("il profilo candidato `{p}` non esiste")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_never_tried_goes_first_in_the_order_of_the_list() {
        assert_eq!(ucb1_pick(&[(5, 4.0), (0, 0.0), (0, 0.0)]).unwrap().0, 1);
        assert_eq!(ucb1_pick(&[(0, 0.0)]).unwrap(), (0, "mai provato".into()));
        assert!(ucb1_pick(&[]).is_none());
    }

    #[test]
    fn the_better_mean_wins_until_the_other_has_been_neglected_for_too_long() {
        // Same number of tries: the better mean wins.
        assert_eq!(ucb1_pick(&[(10, 3.0), (10, 8.0)]).unwrap().0, 1);
        // A profile with a worse mean but few tries is explored: 0.5 + sqrt(2 ln 101 / 1) = 3.5 beats 0.9 + 0.30.
        assert_eq!(ucb1_pick(&[(100, 90.0), (1, 0.5)]).unwrap().0, 1);
        // With enough tries the good one is kept.
        assert_eq!(ucb1_pick(&[(100, 90.0), (50, 20.0)]).unwrap().0, 0);
        // Ties go to the earliest.
        assert_eq!(ucb1_pick(&[(4, 2.0), (4, 2.0)]).unwrap().0, 0);
        let (_, why) = ucb1_pick(&[(100, 90.0), (50, 20.0)]).unwrap();
        assert!(why.starts_with("UCB1: media 0.90"), "{why}");
    }
}
