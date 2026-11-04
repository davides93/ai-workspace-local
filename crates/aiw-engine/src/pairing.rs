//! Brute-force protection for the 6-digit pairing code.
//!
//! `/api/pair` is the only endpoint reachable from the LAN without a token, and the code has only
//! 10^6 values. Two limiters guard it: a global one (every client together, so that parallel
//! connections or many source addresses do not help) and one per client address. When the global
//! limit trips the code is rotated, so the guesses made so far are worthless, and pairing is
//! refused for a lockout that doubles every time it is hit again.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Sliding window over which failures are counted.
pub const WINDOW: Duration = Duration::from_secs(600);
/// Failed attempts tolerated per [`WINDOW`], all clients together.
pub const MAX_FAILURES: usize = 10;
/// Failed attempts tolerated per [`WINDOW`] from one client address.
pub const MAX_FAILURES_PER_CLIENT: usize = 5;
/// First lockout after the global limit is hit; it doubles at every consecutive lockout.
pub const LOCKOUT_BASE: Duration = Duration::from_secs(300);
pub const LOCKOUT_MAX: Duration = Duration::from_secs(3600);
/// Upper bound on the per-client table (source addresses are attacker controlled on a LAN).
const MAX_TRACKED_CLIENTS: usize = 512;

/// Result of one pairing attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairOutcome {
    /// Right code: the API token.
    Paired(String),
    /// Wrong code.
    Invalid,
    /// Too many failures: nothing is evaluated (not even a right code) until the wait is over.
    Locked { retry_after: Duration },
}

#[derive(Default)]
struct Client {
    failures: VecDeque<Instant>,
    locked_until: Option<Instant>,
}

/// State of both limiters. All methods take the current time so that they can be tested.
#[derive(Default)]
pub(crate) struct PairGuard {
    failures: VecDeque<Instant>,
    locked_until: Option<Instant>,
    /// Consecutive global lockouts (decays after [`LOCKOUT_MAX`] without a new one).
    strikes: u32,
    last_strike: Option<Instant>,
    clients: HashMap<String, Client>,
}

fn prune(q: &mut VecDeque<Instant>, now: Instant) {
    while q.front().is_some_and(|t| now.duration_since(*t) >= WINDOW) {
        q.pop_front();
    }
}

impl PairGuard {
    /// `Some(wait)` while attempts from `client` are refused.
    pub fn check(&mut self, client: Option<&str>, now: Instant) -> Option<Duration> {
        let mut wait = None;
        if let Some(until) = self.locked_until {
            if until > now {
                wait = Some(until - now);
            } else {
                self.locked_until = None;
            }
        }
        if let Some(c) = client.and_then(|k| self.clients.get_mut(k)) {
            if let Some(until) = c.locked_until {
                if until > now {
                    wait = wait.max(Some(until - now));
                } else {
                    c.locked_until = None;
                }
            }
        }
        wait
    }

    /// Records a wrong code. Returns true when this failure tripped the global limit: the caller
    /// must rotate the code.
    pub fn record_failure(&mut self, client: Option<&str>, now: Instant) -> bool {
        if let Some(key) = client {
            if self.clients.len() >= MAX_TRACKED_CLIENTS && !self.clients.contains_key(key) {
                self.clients.retain(|_, c| {
                    prune(&mut c.failures, now);
                    !c.failures.is_empty() || c.locked_until.is_some_and(|u| u > now)
                });
            }
            if self.clients.len() < MAX_TRACKED_CLIENTS || self.clients.contains_key(key) {
                let c = self.clients.entry(key.to_string()).or_default();
                prune(&mut c.failures, now);
                c.failures.push_back(now);
                if c.failures.len() >= MAX_FAILURES_PER_CLIENT {
                    c.locked_until = Some(now + WINDOW);
                    c.failures.clear();
                }
            }
        }
        prune(&mut self.failures, now);
        self.failures.push_back(now);
        if self.failures.len() < MAX_FAILURES {
            return false;
        }
        if self.last_strike.is_some_and(|t| now.duration_since(t) > LOCKOUT_MAX) {
            self.strikes = 0;
        }
        self.strikes = self.strikes.saturating_add(1);
        self.last_strike = Some(now);
        let wait = LOCKOUT_BASE.saturating_mul(1u32 << (self.strikes - 1).min(6)).min(LOCKOUT_MAX);
        self.locked_until = Some(now + wait);
        self.failures.clear();
        true
    }

    /// A right code: forgets the failures of that client and the global ones.
    pub fn record_success(&mut self, client: Option<&str>) {
        self.failures.clear();
        self.locked_until = None;
        self.strikes = 0;
        if let Some(key) = client {
            self.clients.remove(key);
        }
    }
}

/// Comparison that does not stop at the first different byte.
pub(crate) fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_limit_locks_everyone_and_backs_off() {
        let mut g = PairGuard::default();
        let t0 = Instant::now();
        for i in 0..MAX_FAILURES - 1 {
            // Different addresses: only the global limit can stop them.
            assert!(!g.record_failure(Some(&format!("10.0.0.{i}")), t0));
            assert_eq!(g.check(Some("10.0.0.99"), t0), None);
        }
        assert!(g.record_failure(Some("10.0.0.50"), t0), "the 10th failure trips the limit");
        let wait = g.check(Some("10.0.0.77"), t0).expect("locked for every client");
        assert_eq!(wait, LOCKOUT_BASE);
        assert_eq!(g.check(None, t0 + Duration::from_secs(100)), Some(LOCKOUT_BASE - Duration::from_secs(100)));
        assert_eq!(g.check(None, t0 + LOCKOUT_BASE), None, "lockout expires");

        // Second lockout in a row lasts twice as long.
        for _ in 0..MAX_FAILURES - 1 {
            g.record_failure(None, t0 + LOCKOUT_BASE);
        }
        assert!(g.record_failure(None, t0 + LOCKOUT_BASE));
        assert_eq!(g.check(None, t0 + LOCKOUT_BASE), Some(LOCKOUT_BASE * 2));
    }

    #[test]
    fn failures_age_out_of_the_window() {
        let mut g = PairGuard::default();
        let t0 = Instant::now();
        for _ in 0..MAX_FAILURES - 1 {
            assert!(!g.record_failure(None, t0));
        }
        // The old ones expired, so this is failure number 1 again.
        assert!(!g.record_failure(None, t0 + WINDOW));
        assert_eq!(g.check(None, t0 + WINDOW), None);
    }

    #[test]
    fn one_noisy_client_is_locked_alone() {
        let mut g = PairGuard::default();
        let t0 = Instant::now();
        for _ in 0..MAX_FAILURES_PER_CLIENT {
            g.record_failure(Some("192.168.1.9"), t0);
        }
        assert_eq!(g.check(Some("192.168.1.9"), t0), Some(WINDOW));
        assert_eq!(g.check(Some("192.168.1.10"), t0), None, "others are not affected");
        assert_eq!(g.check(Some("192.168.1.9"), t0 + WINDOW), None);
    }

    #[test]
    fn success_resets_and_the_table_is_bounded() {
        let mut g = PairGuard::default();
        let t0 = Instant::now();
        for _ in 0..4 {
            g.record_failure(Some("a"), t0);
        }
        g.record_success(Some("a"));
        assert!(!g.record_failure(Some("a"), t0));
        for i in 0..MAX_TRACKED_CLIENTS * 2 {
            g.record_failure(Some(&format!("c{i}")), t0);
        }
        assert!(g.clients.len() <= MAX_TRACKED_CLIENTS);
    }

    #[test]
    fn constant_time_compare() {
        assert!(constant_time_eq("123456", "123456"));
        assert!(!constant_time_eq("123456", "123457"));
        assert!(!constant_time_eq("123456", "12345"));
        assert!(!constant_time_eq("", "1"));
    }
}
