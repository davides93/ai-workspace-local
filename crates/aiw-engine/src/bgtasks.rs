//! Background tasks: what the assistant does while the person is elsewhere, and how the person watches it, pauses it,
//! talks to it and lets it go on.
//!
//! A task is a **thin, durable record on top of the runs of one session**. The agent's work is already in the
//! session (its messages), so nothing else has to be saved to resume it: pausing stops the agent between two steps,
//! resuming starts a new run in the same session. A task is therefore a chain of *segments* (runs), started one after
//! the other until the agent is done, a rule stops it, or the person does.
//!
//! ## What keeps it safe
//! * It runs the agent's usual policy but **never freer than «ask»**: nothing that writes or acts happens without a
//!   confirmation (the same clamp as the channels in `ask` mode). A confirmation waits in the approvals like any
//!   other (`waiting_approval` tells which). The run has no origin, so the management tools (`aiw_*`) refuse in it.
//! * The rules of the proactive assistant stop it at the next step, and it goes on by itself when they end:
//!   quiet hours (`proactive.quiet_hours`), the battery (`proactive.only_on_power`) and the day's time
//!   (`proactive.daily_budget_minutes`, shared with the heartbeat). The kill switch stops it for good until the
//!   level is set again. The person can
//!   resume «anyway» (`force`): that overrides quiet hours, battery and the day's time for that task, never the
//!   kill switch or its own limits.
//! * Its own limits (`limits`: seconds, model calls, tokens, over **all** its segments) are subtracted from the budget
//!   of every new segment, so that pausing and resuming is no way around them.
//!
//! ## What survives a restart
//! The record (state, history, instructions, time spent) and, thanks to a save at every step, the messages of the
//! session up to the last finished step. What does not survive is the step that was running: a task that was running
//! comes back `paused` («interrotto dal riavvio») and **nothing starts by itself** (the step that was cut short may have
//! left a side effect; the person decides). Resuming it starts a new segment, so that step may be done again
//! (at-least-once, like the workflows). An instruction that had reached a segment the restart cut short is delivered
//! again. A task is claimed under a lock before a segment starts, so a double click, a retry or the periodic pass
//! never start two segments.

use crate::proactive::{in_quiet_hours, parse_quiet_hours, short};
use crate::Engine;
use aiw_agent::{Checkpoint, RunBudget, RunEnd, RunInput};
use aiw_core::api::{
    AgentProfile, BackgroundTask, BudgetLimits, ChatMessage, Proposal, TaskInstruction, TaskNote, TaskResume, TaskStart, TASK_STATUSES,
};
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const KIND: &str = "bg_task";
const SPEND_KIND: &str = "proactive";
const SPEND_ID: &str = "task_spend";
/// Defaults of the limits of a task that nobody gave any (the whole task, not one segment).
pub const DEFAULT_MAX_SECONDS: u64 = 1800;
pub const DEFAULT_MAX_TURNS: u32 = 150;
/// The time limit given to the run of a segment, so that its own clock never ends it (see [`remaining`]).
const RUN_CLOCK_OFF_S: u64 = 315_360_000;
const MAX_NOTES: usize = 40;
const MAX_INSTRUCTIONS: usize = 40;
const MAX_PENDING: usize = 10;
const MAX_INSTRUCTION_CHARS: usize = 4000;
/// Finished tasks kept (the oldest are forgotten).
const KEEP_FINISHED: usize = 100;

/// Runtime state of the background tasks that is not a document.
#[derive(Default)]
pub(crate) struct TaskRuntime {
    /// This process drives background tasks (recovers them at start, resumes them by itself). A one-shot command of the CLI
    /// next to the running app does not: it would pause the app's live tasks, or start a second segment of one.
    pub(crate) driven: bool,
    /// Every read-modify-write of a task document happens under this lock (never held across an `.await`).
    lock: Mutex<()>,
    /// The read-modify-write of the day's time (two tasks end a turn at the same time).
    spend: Mutex<()>,
}

/// The day's time spent by tasks (a document of its own: the heartbeat rewrites its state as a whole).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Spend {
    day: String,
    seconds: f64,
}

/// What a run needs to know about the task it is a segment of (kept by the host of the run).
#[derive(Clone)]
pub(crate) struct RunCtx {
    pub task_id: String,
    pub segment: u32,
    /// Progress of the task when the segment started: the segment adds its own to these.
    base_turns: u32,
    base_tokens: u64,
    base_seconds: u64,
    started: std::time::Instant,
    /// Messages of the run already written to the session (the first, the user message, is written at the start).
    pub saved: Arc<AtomicUsize>,
    pub budget: Arc<RunBudget>,
}

/// The facts a segment starts from (see [`Engine::start_run_inner`]).
#[derive(Clone)]
pub(crate) struct Seed {
    pub task_id: String,
    pub segment: u32,
    pub base_turns: u32,
    pub base_tokens: u64,
    pub base_seconds: u64,
    /// What is left of the task's limits: the budget of the run.
    pub remaining: BudgetLimits,
}

impl TaskRuntime {
    pub(crate) fn new(driven: bool) -> Self {
        Self { driven, ..Default::default() }
    }
}

impl Seed {
    pub(crate) fn context(&self, budget: Arc<RunBudget>) -> RunCtx {
        RunCtx {
            task_id: self.task_id.clone(),
            segment: self.segment,
            base_turns: self.base_turns,
            base_tokens: self.base_tokens,
            base_seconds: self.base_seconds,
            started: std::time::Instant::now(),
            saved: Arc::new(AtomicUsize::new(0)),
            budget,
        }
    }
}

/// A rule that stops a task now.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Gate {
    pub reason: &'static str,
    pub text: String,
    /// It goes on by itself when the rule stops applying.
    pub auto: bool,
    /// The person's «anyway» does not lift it.
    pub hard: bool,
}

/// How the run of a segment ended.
pub(crate) enum SegmentEnd {
    Done(RunEnd),
    Cancelled,
    Failed(String),
}

enum Claim {
    /// From `queued` or `paused`.
    Start { force: Option<bool> },
    /// The segment `n` ended and the task goes straight on.
    Continue(u32),
}

fn is_terminal(status: &str) -> bool {
    matches!(status, "succeeded" | "failed" | "cancelled")
}

fn note(t: &mut BackgroundTask, text: impl Into<String>) {
    t.notes.push(TaskNote { at: now_rfc3339(), text: text.into() });
    if t.notes.len() > MAX_NOTES {
        let extra = t.notes.len() - MAX_NOTES;
        t.notes.drain(..extra);
    }
}

/// `s` cut to `n` characters, keeping its line breaks (the answer of an agent is read as written).
fn clip_text(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>().trim_end())
    }
}

fn pending(t: &BackgroundTask) -> Vec<&TaskInstruction> {
    t.instructions.iter().filter(|i| i.status == "pending").collect()
}

/// Which of the task's own limits is used up, in words.
fn exhausted(t: &BackgroundTask) -> Option<String> {
    let l = &t.limits;
    if let Some(m) = l.max_seconds.filter(|m| t.progress.seconds >= *m) {
        return Some(format!("{} minuti di lavoro", m.div_ceil(60)));
    }
    if let Some(m) = l.max_turns.filter(|m| t.progress.turns >= *m) {
        return Some(format!("{m} passi del modello"));
    }
    if let Some(m) = l.max_tokens.filter(|m| t.progress.tokens >= *m) {
        return Some(format!("{m} token"));
    }
    None
}

/// What is left of the limits of the task, as the budget of the next run. The time is not part of it: the run's own clock
/// counts the minutes the person takes to answer a confirmation, so the time of a task is checked at every turn instead,
/// without that waiting (see [`Engine::task_checkpoint`]).
fn remaining(t: &BackgroundTask) -> BudgetLimits {
    BudgetLimits {
        // Ten years: the clock of the run is off (a missing limit would fall back to `agents.budget.max_seconds`).
        max_seconds: Some(RUN_CLOCK_OFF_S),
        max_turns: t.limits.max_turns.map(|m| m.saturating_sub(t.progress.turns).max(1)),
        max_tokens: t.limits.max_tokens.map(|m| m.saturating_sub(t.progress.tokens).max(1)),
    }
}

/// Does this stored message carry the instruction? Only a user message counts: the one the run wrote when it handed the
/// instruction over (`aiw.instruction`, which ends with it) or the first message of a segment, which lists it after a dash.
/// A short instruction («ok») is not found in a tool result or in the agent's own words.
fn instruction_in(m: &ChatMessage, text: &str) -> bool {
    m.role == "user"
        && ((m.name.as_deref() == Some(aiw_agent::INSTRUCTION_NAME) && m.content.trim_end().ends_with(text))
            || m.content.contains(&format!("\n- {text}")))
}

fn instruction_lines(list: &[&TaskInstruction]) -> String {
    list.iter().map(|i| format!("- {}", i.text.trim())).collect::<Vec<_>>().join("\n")
}

impl Engine {
    // ------------------------------------------------------------------ storage
    fn task_load(&self, id: &str) -> Result<BackgroundTask> {
        self.0.store.get_doc(KIND, id)?.ok_or_else(|| Error::NotFound(format!("lavoro {id} non trovato")))
    }

    fn task_save(&self, t: &mut BackgroundTask) -> Result<()> {
        t.updated_at = now_rfc3339();
        self.0.store.put_doc(KIND, &t.id, t)
    }

    /// Reads, changes and writes a task under the lock. Nothing is written when `f` fails.
    fn task_mut<R>(&self, id: &str, f: impl FnOnce(&mut BackgroundTask) -> Result<R>) -> Result<(BackgroundTask, R)> {
        let _guard = self.0.tasks.lock.lock().unwrap();
        let mut t = self.task_load(id)?;
        let r = f(&mut t)?;
        self.task_save(&mut t)?;
        Ok((t, r))
    }

    /// The confirmation a task is waiting for, when it is: the pending request of its session.
    fn waiting_approval(&self, t: &BackgroundTask) -> Option<aiw_core::api::ApprovalRequest> {
        if t.status != "running" {
            return None;
        }
        self.0.approvals.lock().unwrap().values().find(|p| p.session_id.as_deref() == Some(t.session_id.as_str())).map(|p| p.req.clone())
    }

    fn task_view(&self, mut t: BackgroundTask) -> BackgroundTask {
        t.waiting_approval = self.waiting_approval(&t);
        t
    }

    /// Tasks, newest first; `status` narrows the list.
    pub fn tasks(&self, status: Option<&str>) -> Result<Vec<BackgroundTask>> {
        if let Some(s) = status.filter(|s| !s.is_empty()) {
            if !TASK_STATUSES.contains(&s) {
                return Err(Error::Invalid(format!("stato non valido «{s}»: usa {}", TASK_STATUSES.join(", "))));
            }
        }
        let mut list: Vec<BackgroundTask> = self.0.store.list_docs(KIND)?;
        if let Some(s) = status.filter(|s| !s.is_empty()) {
            list.retain(|t| t.status == s);
        }
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        Ok(list.into_iter().map(|t| self.task_view(t)).collect())
    }

    pub fn task(&self, id: &str) -> Result<BackgroundTask> {
        Ok(self.task_view(self.task_load(id)?))
    }

    /// The task a proposal became, when it did.
    pub(crate) fn task_for_proposal(&self, proposal_id: &str) -> Option<BackgroundTask> {
        self.0.store.list_docs::<BackgroundTask>(KIND).ok()?.into_iter().find(|t| t.proposal_id.as_deref() == Some(proposal_id))
    }

    // ------------------------------------------------------------------ the day's time
    fn spend_today(&self, now: DateTime<Local>) -> f64 {
        let today = now.format("%Y-%m-%d").to_string();
        let s: Spend = self.0.store.get_doc(SPEND_KIND, SPEND_ID).ok().flatten().unwrap_or_default();
        if s.day == today {
            s.seconds
        } else {
            0.0
        }
    }

    pub(crate) fn spend_add(&self, now: DateTime<Local>, seconds: f64) {
        let _guard = self.0.tasks.spend.lock().unwrap();
        let today = now.format("%Y-%m-%d").to_string();
        let mut s: Spend = self.0.store.get_doc(SPEND_KIND, SPEND_ID).ok().flatten().unwrap_or_default();
        if s.day != today {
            s = Spend { day: today, seconds: 0.0 };
        }
        s.seconds += seconds.max(0.0);
        let _ = self.0.store.put_doc(SPEND_KIND, SPEND_ID, &s);
    }

    /// Seconds of work the tasks used today. A step is added when it ends a turn (waiting for the person is never added),
    /// so what is going on right now is counted a turn late, never twice.
    pub(crate) fn tasks_seconds_today(&self, now: DateTime<Local>) -> f64 {
        self.spend_today(now)
    }

    // ------------------------------------------------------------------ the rules
    /// What stops `t` right now, if anything.
    pub(crate) fn task_gate(&self, t: &BackgroundTask, now: DateTime<Local>) -> Option<Gate> {
        if self.0.proactive.killed.load(Ordering::SeqCst) {
            return Some(Gate {
                reason: "kill_switch",
                text: "Fermato dal pulsante di emergenza: riparte solo quando scegli di nuovo il livello nelle impostazioni.".into(),
                auto: false,
                hard: true,
            });
        }
        if t.forced {
            return None;
        }
        let quiet = self.cfg_str("proactive.quiet_hours", "");
        if in_quiet_hours(&quiet, now.time()) {
            let end = parse_quiet_hours(&quiet).map(|(_, b)| b.format("%H:%M").to_string()).unwrap_or_default();
            return Some(Gate {
                reason: "quiet_hours",
                text: format!("Orario di silenzio ({}): riprende da solo alle {end}.", quiet.trim()),
                auto: true,
                hard: false,
            });
        }
        let only_on_power = self.0.config.get("proactive.only_on_power").and_then(|v| v.as_bool()).unwrap_or(true);
        if only_on_power && *self.0.proactive.on_power.lock().unwrap() == Some(false) {
            return Some(Gate {
                reason: "battery",
                text: "Il computer è a batteria: riprende da solo quando è collegato.".into(),
                auto: true,
                hard: false,
            });
        }
        let budget_min = self.cfg_u64("proactive.daily_budget_minutes", 15);
        if budget_min == 0 {
            // No time at all is not a day that ends: waiting for tomorrow would never end it.
            return Some(Gate {
                reason: "daily_budget",
                text:
                    "Il tempo al giorno dell'assistente proattivo è 0 minuti: aumentalo nelle impostazioni, o scegli «Riprendi comunque»."
                        .into(),
                auto: false,
                hard: false,
            });
        }
        if self.proactive_spent_s(now) + self.tasks_seconds_today(now) >= (budget_min * 60) as f64 {
            return Some(Gate {
                reason: "daily_budget",
                text: format!("Il tempo di oggi è finito ({budget_min} minuti): riprende da solo domani."),
                auto: true,
                hard: false,
            });
        }
        None
    }

    // ------------------------------------------------------------------ creating
    fn task_limits(&self, agent: &AgentProfile, asked: Option<BudgetLimits>) -> BudgetLimits {
        let own = agent.budget.clone().unwrap_or_default();
        let ask = asked.unwrap_or_default();
        let setting = |key: &str| self.0.config.get(key).and_then(|v| v.as_u64()).filter(|n| *n > 0);
        BudgetLimits {
            max_seconds: Some(
                ask.max_seconds
                    .filter(|n| *n > 0)
                    .or(own.max_seconds)
                    .or_else(|| setting("agents.budget.max_seconds"))
                    .unwrap_or(DEFAULT_MAX_SECONDS),
            ),
            max_turns: Some(
                ask.max_turns
                    .filter(|n| *n > 0)
                    .or(own.max_turns)
                    .or_else(|| setting("agents.budget.max_turns").map(|n| n.min(u32::MAX as u64) as u32))
                    .unwrap_or(DEFAULT_MAX_TURNS),
            ),
            max_tokens: ask.max_tokens.filter(|n| *n > 0).or(own.max_tokens).or_else(|| setting("agents.budget.max_tokens")),
        }
    }

    fn task_new(
        &self,
        instruction: &str,
        title: Option<&str>,
        agent: &AgentProfile,
        origin: &str,
        proposal: Option<&Proposal>,
        limits: Option<BudgetLimits>,
    ) -> Result<BackgroundTask> {
        let text = instruction.trim();
        if text.is_empty() {
            return Err(Error::Invalid("il lavoro non ha istruzioni da eseguire".into()));
        }
        if text.chars().count() > MAX_INSTRUCTION_CHARS {
            return Err(Error::Invalid(format!("le istruzioni sono troppo lunghe (al massimo {MAX_INSTRUCTION_CHARS} caratteri)")));
        }
        let title = title.map(str::trim).filter(|t| !t.is_empty()).map(|t| short(t, 80)).unwrap_or_else(|| short(text, 60));
        let session = self.create_session(&agent.id, Some(format!("Lavoro: {}", short(&title, 50))))?;
        let now = now_rfc3339();
        let mut t = BackgroundTask {
            id: new_id("tsk"),
            title,
            instruction: text.to_string(),
            agent_id: agent.id.clone(),
            session_id: session.id,
            origin: origin.into(),
            proposal_id: proposal.map(|p| p.id.clone()),
            status: "queued".into(),
            limits: self.task_limits(agent, limits),
            created_at: now.clone(),
            updated_at: now,
            ..Default::default()
        };
        note(&mut t, if proposal.is_some() { "Approvato: il lavoro è in coda." } else { "Avviato dalla persona." });
        self.0.store.put_doc(KIND, &t.id, &t)?;
        Ok(t)
    }

    /// A task the person asks for directly. It starts at once unless a rule says to wait (then it is paused with the
    /// reason, and goes on by itself).
    pub async fn task_start(&self, req: TaskStart) -> Result<BackgroundTask> {
        let agent_id = req.agent_id.clone().filter(|a| !a.trim().is_empty()).unwrap_or_else(|| self.cfg_str("proactive.agent", "operator"));
        let agent = self.agent(&agent_id)?;
        let t = self.task_new(&req.instruction, req.title.as_deref(), &agent, "user", None, req.limits)?;
        // The audit says which task and how long the instruction was, never the text: a secret pasted in it must not reach the log.
        self.audit_user("task.start", &format!("{} ({} caratteri di istruzioni)", t.id, t.instruction.chars().count()));
        self.task_begin(t).await
    }

    /// Approving a proposal: it becomes a task of the proposal's agent, which starts like any other.
    pub(crate) async fn task_from_proposal(&self, p: &Proposal, instruction: &str) -> Result<BackgroundTask> {
        let agent = self.agent(&p.agent_id)?;
        let t = self.task_new(instruction, Some(&p.title), &agent, "proposal", Some(p), None)?;
        self.task_begin(t).await
    }

    /// First start of a new task: a rule that applies now holds it back instead.
    async fn task_begin(&self, t: BackgroundTask) -> Result<BackgroundTask> {
        let now = Local::now();
        if let Some(g) = self.task_gate(&t, now) {
            let (t, _) = self.task_mut(&t.id, |t| {
                self.mark_paused(t, g.reason, &g.text, g.auto);
                note(t, format!("In attesa: {}", g.text));
                Ok(())
            })?;
            return Ok(self.task_view(t));
        }
        match self.task_launch(&t.id, Claim::Start { force: None }).await {
            Ok(t) => Ok(self.task_view(t)),
            Err(e) => {
                // It never started: it leaves no trace of a task (the session stays, like for any run that fails to start).
                let _ = self.0.store.delete_doc(KIND, &t.id);
                Err(e)
            }
        }
    }

    fn mark_paused(&self, t: &mut BackgroundTask, reason: &str, text: &str, auto: bool) {
        t.status = "paused".into();
        t.pause_reason = Some(reason.into());
        t.pause_text = Some(text.into());
        t.auto_resume = auto;
        t.stopping = false;
        t.forced = false;
        t.current_run_id = None;
        t.segment_started_at = None;
    }

    // ------------------------------------------------------------------ starting a segment
    /// Claims the task (under the lock, so that two callers never both win) and starts a run of its session.
    async fn task_launch(&self, id: &str, claim: Claim) -> Result<BackgroundTask> {
        let (t, (text, seed)) = self.task_mut(id, |t| {
            match &claim {
                Claim::Start { .. } if !matches!(t.status.as_str(), "queued" | "paused") => {
                    return Err(Error::Conflict(format!("il lavoro «{}» non è in pausa ({})", t.title, t.status)));
                }
                Claim::Continue(n) if t.status != "running" || t.segments != *n || t.current_run_id.is_some() => {
                    return Err(Error::Conflict("il lavoro è già andato avanti".into()));
                }
                _ => {}
            }
            if let Some(what) = exhausted(t) {
                return Err(Error::Conflict(format!("Il lavoro ha usato tutto il suo budget ({what}): aggiungine altro per riprendere.")));
            }
            let first = t.segments == 0;
            let list = pending(t);
            let text = match (first, list.is_empty()) {
                (true, true) => t.instruction.clone(),
                (true, false) => {
                    format!("{}\n\nIstruzioni aggiunte prima della partenza:\n{}", t.instruction, instruction_lines(&list))
                }
                (false, true) => "Riprendi il lavoro da dove eri arrivato.".to_string(),
                (false, false) => format!(
                    "Riprendi il lavoro da dove eri arrivato. Nel frattempo l'utente ti ha dato queste istruzioni, tienile presenti:\n{}",
                    instruction_lines(&list)
                ),
            };
            let segment = t.segments + 1;
            let stamp = now_rfc3339();
            for i in t.instructions.iter_mut().filter(|i| i.status == "pending") {
                i.status = "delivered".into();
                i.delivered_at = Some(stamp.clone());
                i.delivered_in = Some(segment);
            }
            if let Claim::Start { force: Some(f) } = &claim {
                t.forced = *f;
            }
            t.segments = segment;
            t.status = "running".into();
            t.pause_reason = None;
            t.pause_text = None;
            t.auto_resume = false;
            t.stopping = false;
            t.segment_started_at = Some(stamp);
            t.segment_wait_ms = 0;
            t.segment_billed_s = 0;
            let seed = Seed {
                task_id: t.id.clone(),
                segment,
                base_turns: t.progress.turns,
                base_tokens: t.progress.tokens,
                base_seconds: t.progress.seconds,
                remaining: remaining(t),
            };
            note(t, if first { "Partito.".to_string() } else { format!("Ripreso (passo {segment}).") });
            Ok((text, seed))
        })?;
        match self.start_run_inner(&t.session_id, text, None, 0, None, vec![], None, None, Some(seed.clone())).await {
            Ok(run_id) => {
                let (t, ()) = self.task_mut(id, |t| {
                    // A run that was over before this ran (a task that finishes at once) leaves the record as it is.
                    if t.segments == seed.segment && t.status == "running" {
                        t.current_run_id = Some(run_id.clone());
                    }
                    if t.segments == seed.segment {
                        t.run_ids.push(run_id.clone());
                    }
                    Ok(())
                })?;
                Ok(t)
            }
            Err(e) => {
                // It did not start: back to where it was, with its instructions waiting again.
                let _ = self.task_mut(id, |t| {
                    if t.segments == seed.segment {
                        t.segments -= 1;
                        for i in t.instructions.iter_mut().filter(|i| i.delivered_in == Some(seed.segment)) {
                            i.status = "pending".into();
                            i.delivered_at = None;
                            i.delivered_in = None;
                        }
                        self.mark_paused(t, "error", &format!("Non è riuscito a partire: {e}"), false);
                        note(t, format!("Non è partito: {e}"));
                    }
                    Ok(())
                });
                Err(e)
            }
        }
    }

    // ------------------------------------------------------------------ the person's controls
    /// Asks a running task to stop at its next step; a paused one stays so. A task that is waiting for a
    /// confirmation stops at once (nothing is being done, and the confirmation would never end by itself).
    pub fn task_pause(&self, id: &str) -> Result<BackgroundTask> {
        let waiting = {
            let t = self.task_load(id)?;
            self.waiting_approval(&t).is_some()
        };
        let (t, cancel) = self.task_mut(id, |t| match t.status.as_str() {
            "paused" => Ok(false),
            "queued" => {
                self.mark_paused(t, "user", "Messo in pausa da te.", false);
                note(t, "Messo in pausa.");
                Ok(false)
            }
            "running" => {
                // A pause the person asks for is theirs even when a rule was already stopping the task: it no longer goes
                // on by itself when the rule ends.
                let first = !t.stopping;
                t.stopping = true;
                t.pause_reason = Some("user".into());
                t.pause_text = Some("Messo in pausa da te.".into());
                t.auto_resume = false;
                if first {
                    note(
                        t,
                        if waiting {
                            "Pausa richiesta mentre aspettava una conferma."
                        } else {
                            "Pausa richiesta: si ferma al passo successivo."
                        },
                    );
                }
                Ok(waiting)
            }
            other => Err(Error::Conflict(format!("il lavoro «{}» è finito ({other}): non si può mettere in pausa", t.title))),
        })?;
        if cancel {
            if let Some(run) = &t.current_run_id {
                let _ = self.cancel_run(run);
            }
        }
        self.audit_user("task.pause", &t.id);
        Ok(self.task_view(t))
    }

    /// Resumes a paused task: a new segment starts in the same session. `force` overrides quiet hours, the battery and
    /// the day's time; `extend` adds room to a task that used up its limits.
    pub async fn task_resume(&self, id: &str, req: TaskResume) -> Result<BackgroundTask> {
        {
            let mut probe = self.task_load(id)?;
            if !matches!(probe.status.as_str(), "paused" | "queued") {
                return Err(Error::Conflict(format!("il lavoro «{}» non è in pausa ({})", probe.title, probe.status)));
            }
            probe.forced = req.force;
            if let Some(g) = self.task_gate(&probe, Local::now()) {
                let hint = if g.hard { "" } else { " Se vuoi andare avanti adesso, scegli «Riprendi comunque»." };
                return Err(Error::Conflict(format!("{}{hint}", g.text)));
            }
        }
        if let Some(ext) = &req.extend {
            // Only the limits the task has are raised: a task without a token limit does not get one.
            self.task_mut(id, |t| {
                if let (Some(cur), Some(add)) = (t.limits.max_seconds, ext.max_seconds) {
                    t.limits.max_seconds = Some(cur.saturating_add(add));
                }
                if let (Some(cur), Some(add)) = (t.limits.max_turns, ext.max_turns) {
                    t.limits.max_turns = Some(cur.saturating_add(add));
                }
                if let (Some(cur), Some(add)) = (t.limits.max_tokens, ext.max_tokens) {
                    t.limits.max_tokens = Some(cur.saturating_add(add));
                }
                note(t, "Budget del lavoro aumentato.");
                Ok(())
            })?;
        }
        let t = self.task_launch(id, Claim::Start { force: Some(req.force) }).await?;
        self.audit_user("task.resume", &format!("{}{}", t.id, if req.force { " (comunque)" } else { "" }));
        Ok(self.task_view(t))
    }

    /// Adds an instruction. A running task gets it at its next step, a paused one when it resumes.
    pub fn task_instruct(&self, id: &str, text: &str, source: Option<&str>) -> Result<BackgroundTask> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::Invalid("l'istruzione è vuota".into()));
        }
        if text.chars().count() > MAX_INSTRUCTION_CHARS {
            return Err(Error::Invalid(format!("l'istruzione è troppo lunga (al massimo {MAX_INSTRUCTION_CHARS} caratteri)")));
        }
        let source = match source.map(str::trim).filter(|s| !s.is_empty()) {
            None | Some("user") => "user",
            Some("voice") => "voice",
            Some(other) => return Err(Error::Invalid(format!("origine non valida «{other}»: usa user o voice"))),
        };
        let (t, ()) = self.task_mut(id, |t| {
            if is_terminal(&t.status) {
                return Err(Error::Conflict(format!("il lavoro «{}» è finito ({}): non può più ricevere istruzioni", t.title, t.status)));
            }
            if pending(t).len() >= MAX_PENDING {
                return Err(Error::Conflict("ci sono già troppe istruzioni in attesa: aspetta che il lavoro le legga".into()));
            }
            t.instructions.push(TaskInstruction {
                id: new_id("tin"),
                text: text.to_string(),
                source: source.into(),
                created_at: now_rfc3339(),
                status: "pending".into(),
                ..Default::default()
            });
            if t.instructions.len() > MAX_INSTRUCTIONS {
                let drop_n = t.instructions.len() - MAX_INSTRUCTIONS;
                // Only the ones that were delivered or blocked are forgotten.
                let mut dropped = 0;
                t.instructions.retain(|i| {
                    if dropped < drop_n && i.status != "pending" {
                        dropped += 1;
                        false
                    } else {
                        true
                    }
                });
            }
            note(t, format!("Istruzione ricevuta: «{}».", short(text, 80)));
            Ok(())
        })?;
        self.audit_user("task.instruct", &format!("{} ({} caratteri, {source})", t.id, text.chars().count()));
        Ok(self.task_view(t))
    }

    /// Stops a task for good (a running one is cancelled at once).
    pub fn task_cancel(&self, id: &str) -> Result<BackgroundTask> {
        let (t, run) = self.task_mut(id, |t| {
            if is_terminal(&t.status) {
                return Err(Error::Conflict(format!("il lavoro «{}» è già finito ({})", t.title, t.status)));
            }
            let run = (t.status == "running").then(|| t.current_run_id.clone()).flatten();
            t.status = "cancelled".into();
            t.stopping = false;
            t.forced = false;
            t.auto_resume = false;
            t.current_run_id = None;
            t.segment_started_at = None;
            t.finished_at = Some(now_rfc3339());
            note(t, "Annullato da te.");
            Ok(run)
        })?;
        if let Some(run) = run {
            let _ = self.cancel_run(&run);
        }
        self.audit_user("task.cancel", &t.id);
        Ok(self.task_view(t))
    }

    // ------------------------------------------------------------------ inside a run
    /// The turn boundary of a task's run: writes what the run has produced to the session, records the progress, and says
    /// whether to stop (a request, or a rule) and what the person said meanwhile.
    pub(crate) async fn task_checkpoint(&self, ctx: &RunCtx, input: &RunInput, new: &[ChatMessage]) -> Checkpoint {
        let now = Local::now();
        // What the run produced goes to the session now, so that a restart loses at most the step that was running.
        let saved = ctx.saved.load(Ordering::SeqCst);
        let fresh = new.get(1 + saved..).unwrap_or(&[]);
        if !fresh.is_empty() && self.0.store.append_messages(&input.thread_id, fresh).is_ok() {
            ctx.saved.fetch_add(fresh.len(), Ordering::SeqCst);
        }
        let used_turns = ctx.budget.used_turns();
        let used_tokens = ctx.budget.used_tokens();
        let wall_ms = ctx.started.elapsed().as_millis() as u64;
        let last_tool = new.iter().rev().find_map(|m| m.tool_calls.as_ref().and_then(|c| c.last()).map(|c| c.name.clone()));
        let last_text = new.iter().rev().find(|m| m.role == "assistant" && !m.content.trim().is_empty()).map(|m| short(&m.content, 200));
        let later_turn = new.len() > 1;
        let step = self.task_mut(&ctx.task_id, |t| {
            if t.status != "running" || t.segments != ctx.segment {
                return Ok((Some("Il lavoro non è più in corso.".to_string()), vec![]));
            }
            t.progress.turns = ctx.base_turns + used_turns;
            t.progress.tokens = ctx.base_tokens + used_tokens;
            // Time spent waiting for the person is not time spent working.
            t.progress.seconds = ctx.base_seconds + wall_ms.saturating_sub(t.segment_wait_ms) / 1000;
            let working = t.progress.seconds - ctx.base_seconds;
            if working > t.segment_billed_s {
                self.spend_add(now, (working - t.segment_billed_s) as f64);
                t.segment_billed_s = working;
            }
            if last_tool.is_some() {
                t.progress.last_tool.clone_from(&last_tool);
            }
            if last_text.is_some() {
                t.progress.last_text.clone_from(&last_text);
            }
            t.progress.updated_at = Some(now_rfc3339());
            if t.stopping {
                return Ok((Some(t.pause_text.clone().unwrap_or_else(|| "Messo in pausa.".into())), vec![]));
            }
            if let Some(what) = exhausted(t) {
                let text = format!("Il lavoro ha usato tutto il suo budget ({what}): aggiungine altro per riprendere.");
                t.stopping = true;
                t.pause_reason = Some("task_budget".into());
                t.pause_text = Some(text.clone());
                t.auto_resume = false;
                note(t, format!("Si ferma: {text}"));
                return Ok((Some(text), vec![]));
            }
            if let Some(g) = self.task_gate(t, now) {
                t.stopping = true;
                t.pause_reason = Some(g.reason.into());
                t.pause_text = Some(g.text.clone());
                t.auto_resume = g.auto;
                note(t, format!("Si ferma: {}", g.text));
                return Ok((Some(g.text), vec![]));
            }
            // Instructions reach the model after its first step, never as a second user message at the start.
            let waiting: Vec<(String, String)> =
                if later_turn { pending(t).into_iter().map(|i| (i.id.clone(), i.text.clone())).collect() } else { vec![] };
            Ok((None, waiting))
        });
        let Ok((_, (stop, waiting))) = step else {
            return Checkpoint { stop: Some("Il lavoro non esiste più.".into()), instructions: vec![] };
        };
        if stop.is_some() || waiting.is_empty() {
            return Checkpoint { stop, instructions: vec![] };
        }
        // An instruction is a user prompt: the hooks see it like any other.
        let mut deliver: Vec<(String, String)> = vec![];
        let mut blocked: Vec<(String, String)> = vec![];
        for (iid, text) in waiting {
            let h =
                aiw_agent::hooks::run_hooks(&input.hooks, "UserPromptSubmit", None, &json!({"prompt": text, "run_id": input.run_id})).await;
            match h.block {
                Some(reason) => blocked.push((iid, reason)),
                None => {
                    let mut full = text;
                    for c in h.context {
                        full.push_str(&format!("\n\n[contesto aggiunto da un hook]\n{c}"));
                    }
                    deliver.push((iid, full));
                }
            }
        }
        let _ = self.task_mut(&ctx.task_id, |t| {
            let stamp = now_rfc3339();
            for (iid, text) in &deliver {
                if let Some(i) = t.instructions.iter_mut().find(|i| &i.id == iid) {
                    i.status = "delivered".into();
                    i.delivered_at = Some(stamp.clone());
                    i.delivered_in = Some(ctx.segment);
                    note(t, format!("Istruzione letta dall'agente: «{}».", short(text, 80)));
                }
            }
            for (iid, reason) in &blocked {
                if let Some(i) = t.instructions.iter_mut().find(|i| &i.id == iid) {
                    i.status = "blocked".into();
                    i.note = Some(format!("bloccata da un hook: {reason}"));
                    note(t, format!("Un'istruzione è stata bloccata da un hook: {reason}"));
                }
            }
            Ok(())
        });
        Checkpoint { stop: None, instructions: deliver.into_iter().map(|(_, t)| t).collect() }
    }

    /// The run of a segment waited `d` for the person to answer a confirmation: that is not working time.
    pub(crate) fn task_waited(&self, ctx: &RunCtx, d: std::time::Duration) {
        let _ = self.task_mut(&ctx.task_id, |t| {
            if t.segments == ctx.segment && t.status == "running" {
                t.segment_wait_ms += d.as_millis() as u64;
            }
            Ok(())
        });
    }

    /// A segment's run ended. Decides what the task does next: finish, pause, or go straight on with a new segment.
    pub(crate) async fn task_segment_ended(&self, ctx: &RunCtx, outcome: SegmentEnd, final_text: &str) {
        let now = Local::now();
        let wall = ctx.started.elapsed();
        let used_turns = ctx.budget.used_turns();
        let used_tokens = ctx.budget.used_tokens();
        // Only the time the task worked counts (not the minutes it waited for a confirmation).
        let (waited_ms, billed) = self.task_load(&ctx.task_id).map(|t| (t.segment_wait_ms, t.segment_billed_s)).unwrap_or((0, 0));
        let elapsed = wall.saturating_sub(std::time::Duration::from_millis(waited_ms));
        self.spend_add(now, (elapsed.as_secs_f64() - billed as f64).max(0.0));
        let mut next: Option<u32> = None;
        let res = self.task_mut(&ctx.task_id, |t| {
            if t.segments != ctx.segment {
                return Ok(());
            }
            t.progress.turns = ctx.base_turns + used_turns;
            t.progress.tokens = ctx.base_tokens + used_tokens;
            t.progress.seconds = ctx.base_seconds + elapsed.as_secs();
            t.progress.updated_at = Some(now_rfc3339());
            if !final_text.trim().is_empty() {
                t.progress.last_text = Some(short(final_text, 200));
            }
            t.current_run_id = None;
            t.segment_started_at = None;
            t.segment_wait_ms = 0;
            t.segment_billed_s = 0;
            if t.status != "running" {
                // Paused, killed or cancelled from outside while the run was ending: that already decided.
                return Ok(());
            }
            let hold = |t: &mut BackgroundTask, reason: &str, text: String, auto: bool| {
                note(t, format!("In pausa: {text}"));
                self.mark_paused(t, reason, &text, auto);
            };
            match outcome {
                SegmentEnd::Cancelled => {
                    let reason = t.pause_reason.clone().unwrap_or_else(|| "user".into());
                    let text = t.pause_text.clone().unwrap_or_else(|| "Il run è stato fermato.".into());
                    let auto = t.stopping && t.auto_resume;
                    hold(t, &reason, text, auto);
                }
                SegmentEnd::Failed(e) => {
                    t.status = "failed".into();
                    t.error = Some(e.clone());
                    t.finished_at = Some(now_rfc3339());
                    t.forced = false;
                    t.stopping = false;
                    note(t, format!("Non è riuscito: {e}"));
                }
                SegmentEnd::Done(RunEnd::Stopped) => {
                    let reason = t.pause_reason.clone().unwrap_or_else(|| "user".into());
                    let text = t.pause_text.clone().unwrap_or_else(|| "Messo in pausa.".into());
                    let auto = t.auto_resume;
                    hold(t, &reason, text, auto);
                }
                SegmentEnd::Done(RunEnd::Budget) => {
                    let what = exhausted(t).unwrap_or_else(|| "il budget".into());
                    hold(
                        t,
                        "task_budget",
                        format!("Il lavoro ha usato tutto il suo budget ({what}): aggiungine altro per riprendere."),
                        false,
                    );
                }
                SegmentEnd::Done(RunEnd::Handoff) => {
                    // A task does not follow a handoff (the next agent would work outside its rules and its limits).
                    let text = "L'agente ha chiesto di passare la conversazione a un altro assistente: i lavori in background non seguono i passaggi. Riformula il compito.";
                    t.status = "failed".into();
                    t.error = Some(text.into());
                    t.finished_at = Some(now_rfc3339());
                    t.forced = false;
                    t.stopping = false;
                    note(t, "Non riuscito: l'agente ha chiesto un passaggio ad un altro assistente.");
                }
                SegmentEnd::Done(end) => {
                    let more = pending(t).len();
                    let goes_on = matches!(end, RunEnd::TurnLimit) || (matches!(end, RunEnd::Finished) && more > 0);
                    if !goes_on {
                        t.status = "succeeded".into();
                        t.result = (!final_text.trim().is_empty()).then(|| clip_text(final_text.trim(), 2000));
                        t.finished_at = Some(now_rfc3339());
                        t.forced = false;
                        t.stopping = false;
                        note(t, "Finito.");
                    } else if t.stopping {
                        // The pause (or the rule) came while the last turn was going: the next segment does not start.
                        let reason = t.pause_reason.clone().unwrap_or_else(|| "user".into());
                        let text = t.pause_text.clone().unwrap_or_else(|| "Messo in pausa.".into());
                        let auto = t.auto_resume;
                        hold(t, &reason, text, auto);
                    } else if let Some(g) = self.task_gate(t, now) {
                        hold(t, g.reason, g.text, g.auto);
                    } else if let Some(what) = exhausted(t) {
                        hold(
                            t,
                            "task_budget",
                            format!("Il lavoro ha usato tutto il suo budget ({what}): aggiungine altro per riprendere."),
                            false,
                        );
                    } else {
                        next = Some(t.segments);
                    }
                }
            }
            Ok(())
        });
        if res.is_err() {
            return;
        }
        if let Some(n) = next {
            if let Err(e) = self.task_launch(&ctx.task_id, Claim::Continue(n)).await {
                tracing::warn!("lavoro in background: il passo successivo non è partito: {e}");
            }
        }
    }

    // ------------------------------------------------------------------ the heartbeat, the kill switch, the restart
    /// What the periodic loop does for tasks: those paused by a rule that no longer applies go on by themselves.
    pub(crate) fn tasks_pass(&self) {
        self.tasks_pass_at(Local::now());
    }

    pub(crate) fn tasks_pass_at(&self, now: DateTime<Local>) {
        if !self.0.tasks.driven {
            return;
        }
        let list: Vec<BackgroundTask> = self.0.store.list_docs(KIND).unwrap_or_default();
        for t in list.iter().filter(|t| t.status == "paused" && t.auto_resume) {
            if self.task_gate(t, now).is_some() || exhausted(t).is_some() {
                continue;
            }
            let (e, id) = (self.clone(), t.id.clone());
            tokio::spawn(async move {
                match e.task_launch(&id, Claim::Start { force: None }).await {
                    Ok(t) => e.audit_system("task.resume", &format!("{} (da solo)", t.id)),
                    // Someone else got there first (the person, or an earlier pass): nothing to do.
                    Err(Error::Conflict(_)) => {}
                    Err(err) => tracing::warn!("lavoro in background: ripresa non riuscita: {err}"),
                }
            });
        }
        self.tasks_forget_old(&list);
    }

    /// Changes a task document for a test (the state a crash or a rule would leave).
    #[cfg(test)]
    pub(crate) fn task_mut_for_tests(&self, id: &str, f: impl FnOnce(&mut BackgroundTask)) -> BackgroundTask {
        self.task_mut(id, |t| {
            f(t);
            Ok(())
        })
        .unwrap()
        .0
    }

    /// Is this the session of a background task? («Consenti per la sessione» is never remembered for one.)
    pub(crate) fn session_is_task(&self, session_id: &str) -> bool {
        self.0.store.list_docs::<BackgroundTask>(KIND).unwrap_or_default().iter().any(|t| t.session_id == session_id)
    }

    /// The session of a task is deleted: the task goes with it (a running one is stopped first), so that no instruction text
    /// outlives the conversation it belongs to.
    pub(crate) fn tasks_forget_session(&self, session_id: &str) {
        let list: Vec<BackgroundTask> = self.0.store.list_docs(KIND).unwrap_or_default();
        for t in list.into_iter().filter(|t| t.session_id == session_id) {
            let _guard = self.0.tasks.lock.lock().unwrap();
            if let Some(run) = t.current_run_id.as_deref().filter(|_| t.status == "running") {
                let _ = self.cancel_run(run);
            }
            let _ = self.0.store.delete_doc(KIND, &t.id);
        }
    }

    fn tasks_forget_old(&self, list: &[BackgroundTask]) {
        let mut done: Vec<&BackgroundTask> = list.iter().filter(|t| is_terminal(&t.status)).collect();
        if done.len() <= KEEP_FINISHED {
            return;
        }
        done.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        for t in &done[KEEP_FINISHED..] {
            let _ = self.0.store.delete_doc(KIND, &t.id);
        }
    }

    /// The kill switch: every task that is going on stops and stays stopped until the level is set again.
    pub(crate) fn tasks_killed(&self) {
        let list: Vec<BackgroundTask> = self.0.store.list_docs(KIND).unwrap_or_default();
        for t in list.into_iter().filter(|t| matches!(t.status.as_str(), "running" | "queued")) {
            let text = "Fermato dal pulsante di emergenza: riparte solo quando scegli di nuovo il livello nelle impostazioni.";
            let _ = self.task_mut(&t.id, |t| {
                if matches!(t.status.as_str(), "running" | "queued") {
                    self.mark_paused(t, "kill_switch", text, false);
                    note(t, "Fermato dal pulsante di emergenza.");
                }
                Ok(())
            });
        }
    }

    /// At start: whatever says it is going on was cut short by a crash, a kill or a restart. It is paused, and nothing
    /// starts by itself; the instructions that reached a segment that is lost go back to waiting.
    pub(crate) fn tasks_recover(&self) {
        if !self.0.tasks.driven {
            return;
        }
        let list: Vec<BackgroundTask> = self.0.store.list_docs(KIND).unwrap_or_default();
        for t in list.into_iter().filter(|t| matches!(t.status.as_str(), "running" | "queued")) {
            let history: Vec<ChatMessage> = self.0.store.messages(&t.session_id).unwrap_or_default();
            let _ = self.task_mut(&t.id, |t| {
                let segment = t.segments;
                for i in t.instructions.iter_mut().filter(|i| i.status == "delivered" && i.delivered_in == Some(segment)) {
                    if !history.iter().any(|m| instruction_in(m, i.text.trim())) {
                        i.status = "pending".into();
                        i.delivered_at = None;
                        i.delivered_in = None;
                    }
                }
                self.mark_paused(t, "restart", "Interrotto dal riavvio dell'app: riprendi quando vuoi.", false);
                note(t, "Interrotto dal riavvio dell'app.");
                Ok(())
            });
        }
    }

    /// An approved proposal is done when its task is; without a task (approved before tasks existed) when its run is.
    pub(crate) fn proposal_task_finished(&self, proposal_id: &str) -> Option<bool> {
        self.task_for_proposal(proposal_id).map(|t| is_terminal(&t.status))
    }
}
