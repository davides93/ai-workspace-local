//! Who drives the controlled browser: the agent, nobody (the person paused it) or the person (live
//! view, login, two-factor codes). Mutual exclusion lives here and nowhere else.
//!
//! * Every action of the agent first goes through [`Control::enter`]: it passes only while the
//!   mode is `Agent`, and holds an [`OpGuard`] until it ends. While the person pauses or drives, the
//!   call waits (bounded, and ended by the stop of the run), so the agent never acts meanwhile.
//! * The person's input (live view) is accepted only in `Human` mode and only when no agent action
//!   is still in flight ([`Control::human_may_act`]); [`Control::take`] flips the mode at once, so no
//!   new agent action starts, and the one that is running finishes by itself.
//! * Giving the browser back marks the references of the last snapshot as stale (the page changed
//!   under them: the agent has to look again before it clicks) and hides the query and the fragment
//!   of addresses from the model until it navigates by itself (a login can land on a URL that carries
//!   a code or a token).
//!
//! Nothing here holds a frame, a key or a credential: only the mode, a counter and two flags.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// Who drives the browser.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The agent acts.
    #[default]
    Agent,
    /// The person paused the agent: nobody acts, the agent's calls wait.
    Paused,
    /// The person drives (live view or the real window): the agent's calls wait.
    Human,
}

/// Why an agent action could not start.
#[derive(Debug, PartialEq, Eq)]
pub enum Gate {
    /// The run was stopped while the call waited.
    Cancelled,
    /// The wait ran out while the person still had the browser (the mode at that moment).
    Waited(Mode),
}

/// How long the calls of the agent wait for the browser to be handed back.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// An action of the agent that finds the browser paused or driven by the person.
    pub op_wait: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self { op_wait: Duration::from_secs(300) }
    }
}

#[derive(Default)]
struct Inner {
    mode: Mode,
    /// Actions of the agent running now.
    inflight: usize,
    /// The references of the last snapshot are not trustworthy: the person drove in between.
    stale: bool,
    /// The agent asked for the handover (login, two-factor): the question that waits for the person.
    handover: Option<Handover>,
}

/// A request of the agent to the person: «log in here».
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Handover {
    /// The question of the chat that carries it (`POST /api/questions/{id}` ends it).
    pub question_id: String,
    pub run_id: String,
    /// What the agent asked the person to do (written by the agent, shown to the person).
    pub reason: String,
}

/// What the clients may know: no address, no title, no frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ControlState {
    pub mode: Mode,
    /// An action of the agent is still running (the person's input waits for it).
    pub agent_busy: bool,
    /// The agent will have to look at the page again before it acts on an element.
    pub stale: bool,
    pub handover: Option<Handover>,
}

pub struct Control {
    inner: Mutex<Inner>,
    changed: Notify,
    limits: Mutex<Limits>,
    /// Mirrors `mode == Human` for the readers of the browser's events (downloads), which never see this struct.
    human: Arc<AtomicBool>,
    /// Query and fragment of addresses are hidden from the model (see the module docs).
    redact: AtomicBool,
}

impl Control {
    pub fn new(human: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner::default()),
            changed: Notify::new(),
            limits: Mutex::new(Limits::default()),
            human,
            redact: AtomicBool::new(false),
        })
    }

    pub fn set_limits(&self, l: Limits) {
        *self.limits.lock().unwrap() = l;
    }

    pub fn limits(&self) -> Limits {
        *self.limits.lock().unwrap()
    }

    pub fn mode(&self) -> Mode {
        self.inner.lock().unwrap().mode
    }

    pub fn state(&self) -> ControlState {
        let g = self.inner.lock().unwrap();
        ControlState { mode: g.mode, agent_busy: g.inflight > 0, stale: g.stale, handover: g.handover.clone() }
    }

    fn set_mode(&self, g: &mut Inner, mode: Mode) {
        g.mode = mode;
        self.human.store(mode == Mode::Human, Ordering::SeqCst);
    }

    /// Lets one action of the agent start, waiting (bounded) while the person has the browser. The guard
    /// keeps the action counted until it is dropped.
    pub async fn enter(self: &Arc<Self>, cancel: &CancellationToken) -> Result<OpGuard, Gate> {
        let deadline = tokio::time::Instant::now() + self.limits().op_wait;
        loop {
            if cancel.is_cancelled() {
                return Err(Gate::Cancelled);
            }
            // Registered before the check, so a change between the check and the wait is not missed.
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let mode = {
                let mut g = self.inner.lock().unwrap();
                if g.mode == Mode::Agent {
                    g.inflight += 1;
                    return Ok(OpGuard { control: self.clone() });
                }
                g.mode
            };
            tokio::select! {
                _ = &mut notified => {}
                _ = cancel.cancelled() => return Err(Gate::Cancelled),
                _ = tokio::time::sleep_until(deadline) => return Err(Gate::Waited(mode)),
            }
        }
    }

    /// The person pauses the agent. Whatever is running finishes; nothing new starts.
    pub fn pause(&self) -> Mode {
        let mut g = self.inner.lock().unwrap();
        if g.mode == Mode::Agent {
            self.set_mode(&mut g, Mode::Paused);
            self.changed.notify_waiters();
        }
        g.mode
    }

    /// The person lets the agent go on (from a pause). `Human` is left with [`Control::give_back`].
    pub fn resume(&self) -> Mode {
        let mut g = self.inner.lock().unwrap();
        if g.mode == Mode::Paused {
            self.set_mode(&mut g, Mode::Agent);
            self.changed.notify_waiters();
        }
        g.mode
    }

    /// The person takes the browser (or the agent hands it over). The mode flips at once: no new action of
    /// the agent starts; the person's input waits for the running one ([`Control::human_may_act`]).
    pub fn take(&self) -> Mode {
        let mut g = self.inner.lock().unwrap();
        if g.mode != Mode::Human {
            self.set_mode(&mut g, Mode::Human);
            self.changed.notify_waiters();
        }
        Mode::Human
    }

    /// The agent asks for the handover: the browser goes to the person and the request is remembered
    /// (the state tells the client which question ends it).
    pub fn begin_handover(&self, h: Handover) {
        let mut g = self.inner.lock().unwrap();
        g.handover = Some(h);
        self.set_mode(&mut g, Mode::Human);
        self.changed.notify_waiters();
    }

    /// The question of the handover that is open, if any.
    pub fn handover(&self) -> Option<Handover> {
        self.inner.lock().unwrap().handover.clone()
    }

    /// The browser goes back to the agent. The references are stale from now on and the addresses are hidden
    /// from the model until it navigates. A no-op for the references when the agent already had it.
    pub fn give_back(&self) -> Mode {
        let mut g = self.inner.lock().unwrap();
        let from_human = g.mode == Mode::Human;
        g.handover = None;
        if g.mode != Mode::Agent {
            self.set_mode(&mut g, Mode::Agent);
        }
        if from_human {
            g.stale = true;
            self.redact.store(true, Ordering::SeqCst);
        }
        self.changed.notify_waiters();
        Mode::Agent
    }

    /// The kill switch and the closing of the browser: everything back to the start, every waiting call woken.
    pub fn reset(&self) {
        let mut g = self.inner.lock().unwrap();
        *g = Inner::default();
        self.human.store(false, Ordering::SeqCst);
        self.redact.store(false, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    /// The person may act in the page now: they drive and no action of the agent is still running.
    pub fn human_may_act(&self) -> Result<(), &'static str> {
        let g = self.inner.lock().unwrap();
        match (g.mode, g.inflight) {
            (Mode::Human, 0) => Ok(()),
            (Mode::Human, _) => Err("l'agente sta finendo un'azione: riprova tra un istante"),
            _ => Err("prima prendi il controllo del browser"),
        }
    }

    pub fn is_stale(&self) -> bool {
        self.inner.lock().unwrap().stale
    }

    /// A fresh snapshot gave the agent references it can trust.
    pub fn clear_stale(&self) {
        self.inner.lock().unwrap().stale = false;
    }

    pub fn redacting(&self) -> bool {
        self.redact.load(Ordering::SeqCst)
    }

    /// The agent navigated by itself: the addresses it sees are its own again.
    pub fn stop_redacting(&self) {
        self.redact.store(false, Ordering::SeqCst);
    }
}

/// One action of the agent in progress.
pub struct OpGuard {
    control: Arc<Control>,
}

impl Drop for OpGuard {
    fn drop(&mut self) {
        let mut g = self.control.inner.lock().unwrap();
        g.inflight = g.inflight.saturating_sub(1);
        drop(g);
        self.control.changed.notify_waiters();
    }
}

/// An address without its query and fragment (they may carry a code or a token): `https://host/path`.
/// Not an address (`about:blank`, a `data:` page) comes back as it is.
pub fn without_query(url: &str) -> String {
    let cut = url.find(['?', '#']).unwrap_or(url.len());
    url[..cut].to_string()
}

/// The same for every line of a snapshot that shows the address of the page (`URL: …`).
pub fn scrub_url_lines(text: &str) -> String {
    if !text.contains("URL: ") {
        return text.to_string();
    }
    text.split('\n')
        .map(|l| match l.strip_prefix("URL: ") {
            Some(u) => format!("URL: {}", without_query(u)),
            None => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control() -> Arc<Control> {
        let c = Control::new(Arc::new(AtomicBool::new(false)));
        c.set_limits(Limits { op_wait: Duration::from_millis(400) });
        c
    }

    #[tokio::test]
    async fn the_agent_passes_only_while_it_has_the_browser() {
        let c = control();
        let cancel = CancellationToken::new();
        let g = c.enter(&cancel).await.expect("agent mode: passes");
        assert!(c.state().agent_busy);
        drop(g);
        assert!(!c.state().agent_busy);
        c.pause();
        assert_eq!(c.enter(&cancel).await.err(), Some(Gate::Waited(Mode::Paused)));
        c.take();
        assert_eq!(c.enter(&cancel).await.err(), Some(Gate::Waited(Mode::Human)));
    }

    #[tokio::test]
    async fn a_waiting_call_goes_on_when_the_browser_is_given_back() {
        let c = control();
        c.set_limits(Limits { op_wait: Duration::from_secs(10) });
        c.take();
        let c2 = c.clone();
        let call = tokio::spawn(async move { c2.enter(&CancellationToken::new()).await.is_ok() });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!call.is_finished(), "the call waits while the person drives");
        assert_eq!(c.give_back(), Mode::Agent);
        assert!(call.await.unwrap(), "and goes on as soon as the browser is back");
        assert!(c.is_stale(), "the references are stale after a human turn");
        assert!(c.redacting());
    }

    #[tokio::test]
    async fn pausing_and_resuming_does_not_make_the_references_stale() {
        let c = control();
        c.pause();
        c.resume();
        assert!(!c.is_stale() && !c.redacting());
        assert_eq!(c.mode(), Mode::Agent);
    }

    #[tokio::test]
    async fn the_person_cannot_act_while_the_agent_is_running_and_the_agent_cannot_while_the_person_is() {
        let c = control();
        let cancel = CancellationToken::new();
        assert!(c.human_may_act().is_err(), "not their turn");
        let g = c.enter(&cancel).await.unwrap();
        c.take();
        assert!(c.human_may_act().is_err(), "the agent's action is still in flight");
        assert!(c.enter(&cancel).await.is_err(), "no new action starts");
        drop(g);
        assert!(c.human_may_act().is_ok());
    }

    #[tokio::test]
    async fn the_stop_of_the_run_ends_the_wait_and_reset_wakes_everybody() {
        let c = control();
        c.set_limits(Limits { op_wait: Duration::from_secs(30) });
        c.take();
        let cancel = CancellationToken::new();
        let (c2, k2) = (c.clone(), cancel.clone());
        let stopped = tokio::spawn(async move { c2.enter(&k2).await.err() });
        let c3 = c.clone();
        let reset = tokio::spawn(async move { c3.enter(&CancellationToken::new()).await.is_ok() });
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
        assert_eq!(stopped.await.unwrap(), Some(Gate::Cancelled));
        c.reset();
        assert!(reset.await.unwrap(), "the kill switch hands the browser back and wakes the call");
        assert_eq!(c.state(), ControlState { mode: Mode::Agent, agent_busy: false, stale: false, handover: None });
    }

    #[test]
    fn addresses_lose_query_and_fragment() {
        assert_eq!(without_query("https://a.example/cb?code=SEGRETO&state=x#access_token=T"), "https://a.example/cb");
        assert_eq!(without_query("about:blank"), "about:blank");
        let snap = "Titolo: T\nURL: https://a.example/p?token=abc#f\nPosizione: tutta";
        assert_eq!(scrub_url_lines(snap), "Titolo: T\nURL: https://a.example/p\nPosizione: tutta");
        assert_eq!(scrub_url_lines("niente"), "niente");
    }
}
