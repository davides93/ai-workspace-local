//! Reasoning per model call. The profile (or the run) says how much the model should reason
//! ([`ThinkingMode`]); `auto` is decided here for every call of the loop, so that a small model
//! thinks where it pays (planning, recovering from a mistake) and not while it only carries on
//! after tool results that came back fine.

use aiw_core::api::ThinkingMode;
use aiw_runtimes::ThinkingRequest;

/// Reasoning tokens allowed for an `auto` call that thinks, when the profile sets no budget.
pub const AUTO_BUDGET: u32 = 2048;
/// Reasoning tokens allowed in `deep` mode, when the profile sets no budget.
pub const DEEP_BUDGET: u32 = 8192;

const _: () = assert!(DEEP_BUDGET > AUTO_BUDGET);

/// Where a model call sits in the run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CallSite {
    /// No model call has been made yet in this run (a sub-agent's run starts its own).
    pub first_call: bool,
    /// The call comes right after a tool error, a failed self-check or a failed format check.
    pub after_failure: bool,
}

/// What to ask the runtime for this call; `None` (no mode) sends nothing, as before.
/// - `off`: never; `deep`: always, with the larger budget;
/// - `auto`: on for the first call and for the call right after a failure, off while the model only
///   continues after successful tool results.
///
/// An explicit `budget` (the profile's `thinking_budget`) replaces the default of the mode.
pub fn resolve(mode: Option<ThinkingMode>, budget: Option<u32>, site: CallSite) -> Option<ThinkingRequest> {
    let on = |deep: bool, default: u32| ThinkingRequest { enabled: true, deep, budget: Some(budget.unwrap_or(default)) };
    Some(match mode? {
        ThinkingMode::Off => ThinkingRequest { enabled: false, deep: false, budget: None },
        ThinkingMode::Deep => on(true, DEEP_BUDGET),
        ThinkingMode::Auto if site.first_call || site.after_failure => on(false, AUTO_BUDGET),
        ThinkingMode::Auto => ThinkingRequest { enabled: false, deep: false, budget: None },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: CallSite = CallSite { first_call: true, after_failure: false };
    const CONTINUING: CallSite = CallSite { first_call: false, after_failure: false };
    const RECOVERING: CallSite = CallSite { first_call: false, after_failure: true };

    #[test]
    fn without_a_mode_nothing_is_asked() {
        for site in [FIRST, CONTINUING, RECOVERING] {
            assert_eq!(resolve(None, Some(100), site), None);
        }
    }

    #[test]
    fn off_never_thinks() {
        for site in [FIRST, CONTINUING, RECOVERING] {
            assert_eq!(resolve(Some(ThinkingMode::Off), None, site), Some(ThinkingRequest { enabled: false, deep: false, budget: None }));
        }
    }

    #[test]
    fn auto_thinks_first_and_after_a_failure_but_not_while_continuing() {
        let auto = |site| resolve(Some(ThinkingMode::Auto), None, site).unwrap();
        assert_eq!(auto(FIRST), ThinkingRequest { enabled: true, deep: false, budget: Some(AUTO_BUDGET) });
        assert_eq!(auto(RECOVERING), ThinkingRequest { enabled: true, deep: false, budget: Some(AUTO_BUDGET) });
        assert!(!auto(CONTINUING).enabled);
        assert!(auto(CallSite { first_call: true, after_failure: true }).enabled);
    }

    #[test]
    fn deep_always_thinks_with_the_larger_budget() {
        for site in [FIRST, CONTINUING, RECOVERING] {
            assert_eq!(
                resolve(Some(ThinkingMode::Deep), None, site),
                Some(ThinkingRequest { enabled: true, deep: true, budget: Some(DEEP_BUDGET) })
            );
        }
    }

    #[test]
    fn a_profile_budget_replaces_the_default_of_the_mode() {
        assert_eq!(resolve(Some(ThinkingMode::Auto), Some(512), FIRST).unwrap().budget, Some(512));
        assert_eq!(resolve(Some(ThinkingMode::Deep), Some(4096), CONTINUING).unwrap().budget, Some(4096));
        assert_eq!(resolve(Some(ThinkingMode::Off), Some(512), FIRST).unwrap().budget, None);
    }
}
