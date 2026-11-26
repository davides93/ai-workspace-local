//! A paired phone looking at the browser of the agent, read-only, with the computer's yes.
//!
//! The live view of the browser (`browser_live`) is the owner's: the page may show a signed-in session, so a `device`
//! token gets 403 there. This is the one way a phone can look: it **asks**, the person says yes **on this computer**
//! (a route of the `full` scope), and the phone then gets frames of the page, nothing else. It cannot click, type,
//! pause the agent, answer a window of the site or take the browser (those routes stay `full`).
//!
//! The yes is **short and in memory only**: it lasts [`VIEW_TTL`], belongs to one token (revoking the token ends it),
//! and does not survive a restart or the kill switch. A request not answered in [`REQUEST_TTL`] is forgotten. The audit
//! holds the transitions with the name of the device (never a frame, a title or an address).

use crate::Engine;
use aiw_core::{ids::new_id, Error, Result};
use serde::Serialize;
use std::time::{Duration, Instant};

/// How long one yes lasts.
pub const VIEW_TTL: Duration = Duration::from_secs(10 * 60);
/// How long a request waits for the person at the computer.
pub const REQUEST_TTL: Duration = Duration::from_secs(2 * 60);
const MAX_PENDING: usize = 8;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ViewRequest {
    pub id: String,
    pub device: String,
    /// Seconds left before the request is forgotten.
    pub expires_in_s: u64,
}

/// What a phone is told about its own request.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ViewStatus {
    /// `none` (no request), `pending` (waiting for the computer), `granted`.
    pub state: &'static str,
    /// Seconds the yes still lasts (`granted`) or the request still waits (`pending`).
    pub seconds_left: u64,
}

struct Pending {
    id: String,
    token: String,
    device: String,
    at: Instant,
}

struct Grant {
    token: String,
    device: String,
    until: Instant,
}

#[derive(Default)]
pub(crate) struct ViewGrants {
    pending: Vec<Pending>,
    granted: Vec<Grant>,
}

impl ViewGrants {
    fn sweep(&mut self, now: Instant) {
        self.pending.retain(|p| now.duration_since(p.at) < REQUEST_TTL);
        self.granted.retain(|g| g.until > now);
    }
}

impl Engine {
    /// A phone asks to look. Asking again while a request waits (or a yes lasts) changes nothing.
    pub fn browser_view_request(&self, token: &str, device: &str) -> Result<ViewStatus> {
        let now = Instant::now();
        {
            let mut g = self.0.view_grants.lock().unwrap();
            g.sweep(now);
            if g.granted.iter().any(|x| x.token == token) || g.pending.iter().any(|p| p.token == token) {
                drop(g);
                return Ok(self.browser_view_status(token));
            }
            if g.pending.len() >= MAX_PENDING {
                return Err(Error::Conflict("ci sono già troppe richieste in attesa sul computer".into()));
            }
            g.pending.push(Pending { id: new_id("vw"), token: token.into(), device: clip(device), at: now });
        }
        self.audit_system("browser.view_request", &format!("{} chiede di vedere il browser dell'agente", clip(device)));
        Ok(self.browser_view_status(token))
    }

    pub fn browser_view_status(&self, token: &str) -> ViewStatus {
        let now = Instant::now();
        let mut g = self.0.view_grants.lock().unwrap();
        g.sweep(now);
        if let Some(x) = g.granted.iter().find(|x| x.token == token) {
            return ViewStatus { state: "granted", seconds_left: (x.until - now).as_secs() };
        }
        if let Some(p) = g.pending.iter().find(|p| p.token == token) {
            return ViewStatus { state: "pending", seconds_left: REQUEST_TTL.saturating_sub(now.duration_since(p.at)).as_secs() };
        }
        ViewStatus { state: "none", seconds_left: 0 }
    }

    /// May this token look at the page now?
    pub fn browser_view_allowed(&self, token: &str) -> bool {
        self.browser_view_status(token).state == "granted"
    }

    /// The requests waiting for the person at the computer.
    pub fn browser_view_requests(&self) -> Vec<ViewRequest> {
        let now = Instant::now();
        let mut g = self.0.view_grants.lock().unwrap();
        g.sweep(now);
        g.pending
            .iter()
            .map(|p| ViewRequest {
                id: p.id.clone(),
                device: p.device.clone(),
                expires_in_s: REQUEST_TTL.saturating_sub(now.duration_since(p.at)).as_secs(),
            })
            .collect()
    }

    /// The person at the computer says yes or no to one request.
    pub fn browser_view_decide(&self, id: &str, approve: bool) -> Result<()> {
        let now = Instant::now();
        let device = {
            let mut g = self.0.view_grants.lock().unwrap();
            g.sweep(now);
            let at = g.pending.iter().position(|p| p.id == id).ok_or_else(|| Error::NotFound("la richiesta non c'è più".into()))?;
            let p = g.pending.remove(at);
            if approve {
                g.granted.retain(|x| x.token != p.token);
                g.granted.push(Grant { token: p.token, device: p.device.clone(), until: now + VIEW_TTL });
            }
            p.device
        };
        self.audit_user(
            if approve { "browser.view_allow" } else { "browser.view_deny" },
            &format!("{device}: {}", if approve { "può vedere per qualche minuto" } else { "richiesta rifiutata" }),
        );
        Ok(())
    }

    /// Ends every yes now (the button on the computer, the kill switch, a revoked token).
    pub fn browser_view_revoke_all(&self) -> usize {
        let mut g = self.0.view_grants.lock().unwrap();
        let n = g.granted.len() + g.pending.len();
        g.granted.clear();
        g.pending.clear();
        drop(g);
        if n > 0 {
            self.audit_user("browser.view_end", "la vista del browser dai dispositivi è chiusa");
        }
        n
    }

    /// The devices that can look right now (for the computer's screen).
    pub fn browser_view_grants(&self) -> Vec<String> {
        let now = Instant::now();
        let mut g = self.0.view_grants.lock().unwrap();
        g.sweep(now);
        g.granted.iter().map(|x| x.device.clone()).collect()
    }
}

fn clip(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).take(60).collect();
    if s.trim().is_empty() {
        "un dispositivo".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::engine;

    #[tokio::test]
    async fn a_phone_sees_nothing_until_the_computer_says_yes_and_the_yes_is_its_own() {
        let (e, _a, _b) = engine(vec![]).await;
        assert!(!e.browser_view_allowed("tok-a"));
        assert_eq!(e.browser_view_request("tok-a", "iPhone di Dave").unwrap().state, "pending");
        assert_eq!(e.browser_view_request("tok-a", "iPhone di Dave").unwrap().state, "pending", "asking twice is one request");
        assert!(!e.browser_view_allowed("tok-a"), "a request is not a yes");
        let waiting = e.browser_view_requests();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].device, "iPhone di Dave");
        e.browser_view_decide(&waiting[0].id, true).unwrap();
        assert!(e.browser_view_allowed("tok-a"));
        assert!(!e.browser_view_allowed("tok-b"), "the yes belongs to one token");
        assert!(e.browser_view_decide(&waiting[0].id, true).is_err(), "a request is answered once");
    }

    #[tokio::test]
    async fn no_and_revoke_end_it_and_the_audit_names_the_device_only() {
        let (e, _a, _b) = engine(vec![]).await;
        e.browser_view_request("tok-a", "iPad").unwrap();
        let id = e.browser_view_requests()[0].id.clone();
        e.browser_view_decide(&id, false).unwrap();
        assert!(!e.browser_view_allowed("tok-a"));
        e.browser_view_request("tok-a", "iPad").unwrap();
        let id = e.browser_view_requests()[0].id.clone();
        e.browser_view_decide(&id, true).unwrap();
        assert_eq!(e.browser_view_grants(), ["iPad"]);
        assert_eq!(e.browser_view_revoke_all(), 1);
        assert!(!e.browser_view_allowed("tok-a"));
        let actions: Vec<String> = e.audit().into_iter().map(|a| a.action).collect();
        for a in ["browser.view_request", "browser.view_deny", "browser.view_allow", "browser.view_end"] {
            assert!(actions.iter().any(|x| x == a), "missing audit row {a}: {actions:?}");
        }
    }

    #[tokio::test]
    async fn too_many_pending_requests_are_refused() {
        let (e, _a, _b) = engine(vec![]).await;
        for i in 0..MAX_PENDING {
            e.browser_view_request(&format!("t{i}"), "x").unwrap();
        }
        assert!(e.browser_view_request("tx", "x").is_err());
    }
}
