//! Budget of a whole run: tokens, model turns and wall-clock time, shared by the run and every
//! sub-agent it starts (they hold the same `Arc`), so delegating cannot be used to get around it.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub struct RunBudget {
    max_tokens: Option<u64>,
    max_turns: Option<u32>,
    deadline: Option<Instant>,
    used_tokens: AtomicU64,
    used_turns: AtomicU32,
}

impl RunBudget {
    /// The clock starts now.
    pub fn new(max_tokens: Option<u64>, max_turns: Option<u32>, max_seconds: Option<u64>) -> Self {
        Self {
            max_tokens,
            max_turns,
            deadline: max_seconds.map(|s| Instant::now() + Duration::from_secs(s)),
            used_tokens: AtomicU64::new(0),
            used_turns: AtomicU32::new(0),
        }
    }

    pub fn unlimited() -> Self {
        Self::new(None, None, None)
    }

    /// Counts one model call.
    pub fn record_turn(&self) {
        self.used_turns.fetch_add(1, Ordering::Relaxed);
    }

    /// Adds the tokens (prompt + completion) of one model call.
    pub fn record_tokens(&self, n: u64) {
        self.used_tokens.fetch_add(n, Ordering::Relaxed);
    }

    pub fn used_tokens(&self) -> u64 {
        self.used_tokens.load(Ordering::Relaxed)
    }

    pub fn used_turns(&self) -> u32 {
        self.used_turns.load(Ordering::Relaxed)
    }

    /// Why the run has to stop (in Italian, for the user), or `None` while there is budget left.
    pub fn exceeded(&self) -> Option<String> {
        if let Some(max) = self.max_turns.filter(|m| self.used_turns() >= *m) {
            return Some(format!("{max} turni del modello usati"));
        }
        if let Some(max) = self.max_tokens.filter(|m| self.used_tokens() >= *m) {
            return Some(format!("{max} token usati"));
        }
        if self.deadline.is_some_and(|d| Instant::now() >= d) {
            return Some("tempo massimo del run scaduto".into());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_never_runs_out() {
        let b = RunBudget::unlimited();
        for _ in 0..1000 {
            b.record_turn();
        }
        b.record_tokens(u64::MAX / 2);
        assert_eq!(b.exceeded(), None);
    }

    #[test]
    fn each_limit_reports_its_reason() {
        let b = RunBudget::new(None, Some(2), None);
        b.record_turn();
        assert_eq!(b.exceeded(), None);
        b.record_turn();
        assert!(b.exceeded().unwrap().contains("2 turni"));

        let b = RunBudget::new(Some(100), None, None);
        b.record_tokens(60);
        assert_eq!(b.exceeded(), None);
        b.record_tokens(40);
        assert!(b.exceeded().unwrap().contains("100 token"));
        assert_eq!((b.used_tokens(), b.used_turns()), (100, 0));

        let b = RunBudget::new(None, None, Some(0));
        assert!(b.exceeded().unwrap().contains("tempo"));
    }
}
