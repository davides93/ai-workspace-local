//! Who drives the controlled browser: the agent, nobody (the person paused it) or the person (live
//! view, login, two-factor codes). Mutual exclusion lives here and nowhere else.
//!
//! * Every action of the agent first goes through [`Control::enter`]: it passes only while the
//!   mode is `Agent`, and holds an [`OpGuard`] until it ends. While the person pauses or drives, the
//!   call waits (bounded, and ended by the stop of the run), so the agent never acts meanwhile.
//! * The person's input (live view) is accepted only in `Human` mode and only when no agent action
//!   is still in flight ([`Control::human_begin`]); [`Control::take`] flips the mode at once, so no
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
    /// Inputs of the person being sent to the page right now: no action of the agent starts until they are done, even if
    /// the browser was just given back.
    human_ops: usize,
    /// How many times the browser came back from the person (the addresses of the pages open then are hidden from the model).
    handbacks: u64,
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
}

impl Control {
    pub fn new(human: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self { inner: Mutex::new(Inner::default()), changed: Notify::new(), limits: Mutex::new(Limits::default()), human })
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
                if g.mode == Mode::Agent && g.human_ops == 0 {
                    g.inflight += 1;
                    return Ok(OpGuard { control: self.clone() });
                }
                // (A person's input still being sent keeps the agent waiting even if the browser was just given back.)
                if g.mode == Mode::Agent {
                    Mode::Human
                } else {
                    g.mode
                }
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
    /// the agent starts; the person's input waits for the running one ([`Control::human_begin`]).
    pub fn take(&self) -> Mode {
        let mut g = self.inner.lock().unwrap();
        if g.mode != Mode::Human {
            self.set_mode(&mut g, Mode::Human);
            self.changed.notify_waiters();
        }
        Mode::Human
    }

    /// The agent asks for the handover: the browser goes to the person and the request is remembered (the state tells
    /// the client which question ends it). Only from `Agent`: when the person already paused or drives, the agent has
    /// nothing to ask (`Err` with the mode), and a handover cannot take the place of another.
    pub fn begin_handover(&self, h: Handover) -> Result<(), Mode> {
        let mut g = self.inner.lock().unwrap();
        if g.mode != Mode::Agent {
            return Err(g.mode);
        }
        g.handover = Some(h);
        self.set_mode(&mut g, Mode::Human);
        self.changed.notify_waiters();
        Ok(())
    }

    /// The handover whose question is `question_id` is over (answered, refused, expired, the run stopped): the browser goes
    /// back to the agent, but only if it is still *that* handover that holds it (the person may have given it back already, or
    /// taken it again since: then it is theirs and stays so).
    pub fn end_handover(&self, question_id: &str) -> bool {
        let mine = self.inner.lock().unwrap().handover.as_ref().is_some_and(|h| h.question_id == question_id);
        if mine {
            self.give_back();
        }
        mine
    }

    /// The question of the handover that is open, if any.
    pub fn handover(&self) -> Option<Handover> {
        self.inner.lock().unwrap().handover.clone()
    }

    /// The browser goes back to the agent. The references are stale from now on and the addresses of the pages open now are
    /// hidden from the model until it navigates them itself. A no-op for the references when the agent already had it.
    pub fn give_back(&self) -> Mode {
        let mut g = self.inner.lock().unwrap();
        let from_human = g.mode == Mode::Human;
        g.handover = None;
        if g.mode != Mode::Agent {
            self.set_mode(&mut g, Mode::Agent);
        }
        if from_human {
            g.stale = true;
            g.handbacks += 1;
        }
        self.changed.notify_waiters();
        Mode::Agent
    }

    /// The kill switch and the closing of the browser: everything back to the start, every waiting call woken. The actions and
    /// inputs still running keep their count (their guards give it back when they end).
    pub fn reset(&self) {
        let mut g = self.inner.lock().unwrap();
        let (inflight, human_ops, handbacks) = (g.inflight, g.human_ops, g.handbacks);
        *g = Inner { inflight, human_ops, handbacks, ..Inner::default() };
        self.human.store(false, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    /// An input of the person is about to go to the page: allowed while they drive and no action of the agent is running,
    /// and counted until the guard is dropped, so that a handback in the middle does not let the agent in too early.
    pub fn human_begin(self: &Arc<Self>) -> Result<HumanGuard, &'static str> {
        let mut g = self.inner.lock().unwrap();
        match (g.mode, g.inflight) {
            (Mode::Human, 0) => {
                g.human_ops += 1;
                Ok(HumanGuard { control: self.clone() })
            }
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

    /// How many times the browser came back from the person: the pages open at each new count get their addresses hidden.
    pub fn handbacks(&self) -> u64 {
        self.inner.lock().unwrap().handbacks
    }
}

/// An input of the person being sent to the page (see [`Control::human_begin`]).
pub struct HumanGuard {
    control: Arc<Control>,
}

impl Drop for HumanGuard {
    fn drop(&mut self) {
        let mut g = self.control.inner.lock().unwrap();
        g.human_ops = g.human_ops.saturating_sub(1);
        drop(g);
        self.control.changed.notify_waiters();
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
        assert_eq!(c.handbacks(), 1);
    }

    #[tokio::test]
    async fn pausing_and_resuming_does_not_make_the_references_stale() {
        let c = control();
        c.pause();
        c.resume();
        assert!(!c.is_stale() && c.handbacks() == 0);
        assert_eq!(c.mode(), Mode::Agent);
    }

    #[tokio::test]
    async fn the_person_cannot_act_while_the_agent_is_running_and_the_agent_cannot_while_the_person_is() {
        let c = control();
        let cancel = CancellationToken::new();
        assert!(c.human_begin().is_err(), "not their turn");
        let g = c.enter(&cancel).await.unwrap();
        c.take();
        assert!(c.human_begin().is_err(), "the agent's action is still in flight");
        assert!(c.enter(&cancel).await.is_err(), "no new action starts");
        drop(g);
        assert!(c.human_begin().is_ok());
    }

    #[tokio::test]
    async fn an_input_of_the_person_in_flight_keeps_the_agent_out_even_after_the_handback() {
        let c = control();
        c.set_limits(Limits { op_wait: Duration::from_secs(10) });
        c.take();
        let input = c.human_begin().expect("their turn");
        c.give_back();
        let c2 = c.clone();
        let call = tokio::spawn(async move { c2.enter(&CancellationToken::new()).await.is_ok() });
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!call.is_finished(), "the agent waits for the click that is still going to the page");
        drop(input);
        assert!(call.await.unwrap());
    }

    #[test]
    fn a_handover_needs_the_agent_to_have_the_browser_and_only_its_own_ending_gives_it_back() {
        let c = control();
        let h = |id: &str| Handover { question_id: id.into(), run_id: "r".into(), reason: "x".into() };
        c.pause();
        assert_eq!(c.begin_handover(h("q0")), Err(Mode::Paused), "the person paused: nothing to ask, and the pause stays");
        assert_eq!(c.mode(), Mode::Paused);
        c.resume();
        c.take();
        assert_eq!(c.begin_handover(h("q0")), Err(Mode::Human), "the person drives: the same");
        c.give_back();
        assert_eq!(c.begin_handover(h("q1")), Ok(()));
        assert_eq!(c.begin_handover(h("q2")), Err(Mode::Human), "a second handover does not take the first one's place");
        assert!(!c.end_handover("q2"), "and its ending is not the first one's");
        assert_eq!(c.mode(), Mode::Human);
        // The person gave it back and took it again: the old handover ending later must not take it from them.
        c.give_back();
        c.take();
        assert!(!c.end_handover("q1"));
        assert_eq!(c.mode(), Mode::Human, "still theirs");
        c.give_back();
        assert_eq!(c.begin_handover(h("q3")), Ok(()));
        assert!(c.end_handover("q3"));
        assert_eq!(c.mode(), Mode::Agent);
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
        // (A guard that outlived the reset still counts: its drop gives the count back.)
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
