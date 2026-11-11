//! The proactive assistant: a heartbeat that looks at what is new and leaves **proposals** for the
//! person; it never acts by itself.
//!
//! ## Flow of one tick
//! 1. Gates, in this order: the level (`proactive.level`, off by default), the kill switch, the quiet
//!    hours, the power state (where the platform reports it), the daily budget, a run already going
//!    on. A skipped tick records why ([`ProactiveStatus::skipped_reason`]).
//! 2. Due reminders from the memory (`kind = reminder`, `due_at` passed) become proposals directly:
//!    no model is asked.
//! 3. A compact context: failed workflow runs since the last tick, approvals waiting, new files in
//!    `proactive.watch_folders`. **If there is nothing in it no model is called.** (Pinned memories and the
//!    proposals already open are background for the question, not reasons to ask it.)
//! 4. The smallest suitable model is asked ONE question with a strict, tiny JSON answer: is there
//!    something useful to do now, and which single item.
//! 5. If yes, the chosen agent runs in plan / read-only mode with a small budget and writes the proposal
//!    (title, what was noticed, a ready-to-run instruction). Approving it later starts a normal run
//!    under the agent's usual policy.
//! 6. At `proactive.briefing_time` (or at the first tick after it, once a day) the morning briefing is
//!    written, from data only, with no model and no connector.
//!
//! The wake sources reuse the workflow trigger machinery ([`aiw_workflow::Scheduler`] with virtual
//! workflows: an interval and a cron), plus due reminders and `POST /api/proactive/tick`.

use crate::adviser::{self, MainModel};
use crate::{defaults, Engine};
use aiw_core::api::{AdviserTask, AgentProfile, BudgetLimits, InferenceProfile, ProactiveStatus, Proposal, TickResult, WorkflowDef};
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use aiw_runtimes::{ChatDelta, ChatRequest, Msg};
use aiw_tools::Risk;
use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveTime, TimeZone, Timelike};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Prefix of the id of the agent a proposal run uses: the configured agent, read-only.
pub const PROACTIVE_PREFIX: &str = "proactive:";
/// Days an open proposal waits before it expires.
pub const PROPOSAL_DAYS: i64 = 7;
const STATE_KIND: &str = "proactive";
const STATE_ID: &str = "state";
const MAX_ITEMS: usize = 8;

/// What woke the heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    Interval,
    Briefing,
    Reminder,
    Manual,
}

impl Wake {
    pub fn as_str(self) -> &'static str {
        match self {
            Wake::Interval => "interval",
            Wake::Briefing => "briefing",
            Wake::Reminder => "reminder",
            Wake::Manual => "manual",
        }
    }
    pub fn parse(s: &str) -> Option<Wake> {
        Some(match s {
            "interval" => Wake::Interval,
            "briefing" => Wake::Briefing,
            "reminder" => Wake::Reminder,
            "manual" => Wake::Manual,
            _ => return None,
        })
    }
}

/// Runtime state of the heartbeat that is not a document.
#[derive(Default)]
pub(crate) struct ProactiveRuntime {
    /// One tick at a time.
    pub lock: tokio::sync::Mutex<()>,
    /// The kill switch was used: nothing runs until the level is set again.
    pub killed: std::sync::atomic::AtomicBool,
    /// The scheduler of the wake sources (the workflow trigger machinery, with virtual workflows).
    pub sched: Mutex<aiw_workflow::Scheduler>,
    /// Power state, when the platform reports it (`None` = unknown: the setting is ignored).
    pub on_power: Mutex<Option<bool>>,
    /// Cancels the model call or run of the tick in progress (kill switch).
    pub current: Mutex<Option<CancellationToken>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct State {
    /// Last tick that was not skipped (RFC 3339): "since the last tick" starts here.
    last_tick_at: Option<String>,
    last_result: Option<TickResult>,
    last_skip: Option<String>,
    /// `YYYY-MM-DD` the budget below belongs to.
    spent_day: String,
    spent_s: f64,
}

// ------------------------------------------------------------------------------------ pure helpers

fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.trim().split_once(':')?;
    NaiveTime::from_hms_opt(h.trim().parse().ok()?, m.trim().parse().ok()?, 0)
}

/// `22:00-07:00` → `(22:00, 07:00)`; anything else (including an empty value) → `None` (no quiet hours).
pub fn parse_quiet_hours(spec: &str) -> Option<(NaiveTime, NaiveTime)> {
    let (a, b) = spec.split_once('-')?;
    let (a, b) = (parse_hhmm(a)?, parse_hhmm(b)?);
    (a != b).then_some((a, b))
}

/// Is `t` inside the quiet hours? The window may cross midnight; the start is included, the end is not.
pub fn in_quiet_hours(spec: &str, t: NaiveTime) -> bool {
    match parse_quiet_hours(spec) {
        Some((a, b)) if a < b => t >= a && t < b,
        Some((a, b)) => t >= a || t < b,
        None => false,
    }
}

/// Reply of the filter: `Some(index)` (1-based) when it found one item worth acting on. A reply that is
/// not the strict form (a boolean `act`, an in-range integer `item` when `act` is true) counts as "no".
pub fn parse_filter(answer: &str, items: usize) -> Option<usize> {
    let v = aiw_agent::schema::extract_json(answer)?;
    if !v.get("act")?.as_bool()? {
        return None;
    }
    let n = v.get("item")?.as_u64()? as usize;
    (1..=items).contains(&n).then_some(n)
}

fn short(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        format!("{}…", one.chars().take(n.saturating_sub(1)).collect::<String>().trim_end())
    }
}

/// Files in `folders` (two levels deep, no hidden ones, at most 400 entries looked at) modified after `since`, newest first.
fn new_files(folders: &[String], since: DateTime<Local>) -> Vec<(String, PathBuf, DateTime<Local>)> {
    let since: std::time::SystemTime = since.into();
    let mut out = Vec::new();
    let mut seen = 0usize;
    for f in folders {
        let root = aiw_core::paths::expand_home(f);
        let mut stack = vec![(root.clone(), 0u8)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                seen += 1;
                if seen > 400 {
                    break;
                }
                if e.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                let Ok(meta) = e.metadata() else { continue };
                if meta.is_dir() {
                    if depth < 1 {
                        stack.push((e.path(), depth + 1));
                    }
                } else if let Ok(m) = meta.modified().map(DateTime::<Local>::from) {
                    if std::time::SystemTime::from(m) > since {
                        out.push((f.clone(), e.path(), m));
                    }
                }
            }
        }
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.2));
    out
}

/// One thing the filter may pick.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    /// Key that keeps the same thing from being proposed twice.
    source: String,
    /// One line for the filter and the proposal run.
    text: String,
}

fn filter_messages(items: &[Item], pinned: &[String], open_titles: &[String]) -> Vec<Msg> {
    let mut u = String::from("Cose da guardare:\n");
    for (i, it) in items.iter().enumerate() {
        u.push_str(&format!("{}. {}\n", i + 1, short(&it.text, 160)));
    }
    if !pinned.is_empty() {
        u.push_str("\nCose che l'utente vuole sempre presenti:\n");
        for p in pinned {
            u.push_str(&format!("- {}\n", short(p, 120)));
        }
    }
    if !open_titles.is_empty() {
        u.push_str("\nProposte già in attesa (non ripeterle):\n");
        for t in open_titles {
            u.push_str(&format!("- {}\n", short(t, 100)));
        }
    }
    vec![
        Msg::system(
            "Sei un filtro. Decidi se UNA delle cose elencate merita che l'assistente prepari ora una proposta per l'utente. \
Nel dubbio, no. Rispondi SOLO con un JSON: {\"act\": true, \"item\": <numero>, \"why\": \"<una frase>\"} oppure {\"act\": false}.",
        ),
        Msg::user(u),
    ]
}

fn proposal_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": {"type": "string"},
            "noticed": {"type": "string"},
            "instruction": {"type": "string"}
        },
        "required": ["title", "noticed", "instruction"]
    })
}

// ------------------------------------------------------------------------------------ the engine

impl Engine {
    fn cfg_str(&self, key: &str, default: &str) -> String {
        self.0.config.get(key).and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| default.to_string())
    }
    fn cfg_u64(&self, key: &str, default: u64) -> u64 {
        self.0.config.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
    }
    pub fn proactive_level(&self) -> String {
        match self.cfg_str("proactive.level", "off").as_str() {
            l @ ("propose" | "act_readonly") => l.to_string(),
            _ => "off".into(),
        }
    }
    fn briefing_time(&self) -> NaiveTime {
        parse_hhmm(&self.cfg_str("proactive.briefing_time", "08:00")).unwrap_or_else(|| NaiveTime::from_hms_opt(8, 0, 0).unwrap())
    }
    fn state(&self) -> State {
        self.0.store.get_doc(STATE_KIND, STATE_ID).ok().flatten().unwrap_or_default()
    }
    fn save_state(&self, s: &State) {
        let _ = self.0.store.put_doc(STATE_KIND, STATE_ID, s);
    }

    /// Quiet hours right now (for the desktop shell, which holds notifications back).
    pub fn proactive_quiet_now(&self) -> bool {
        self.in_quiet_hours_now(Local::now())
    }

    /// Whether `now` is inside `proactive.quiet_hours` (used by the heartbeat and by the notifications).
    pub fn in_quiet_hours_now(&self, now: DateTime<Local>) -> bool {
        in_quiet_hours(&self.cfg_str("proactive.quiet_hours", ""), now.time())
    }

    /// The platform tells whether the computer is on mains power (`None`: it does not know).
    pub fn set_on_power(&self, on_power: Option<bool>) {
        *self.0.proactive.on_power.lock().unwrap() = on_power;
    }

    /// Called by the kill switch: stops what a tick is doing and keeps the heartbeat off until the level is set again.
    pub(crate) fn proactive_killed(&self) {
        self.0.proactive.killed.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(t) = self.0.proactive.current.lock().unwrap().take() {
            t.cancel();
        }
    }

    /// The person set the level: the heartbeat may run again.
    pub(crate) fn proactive_level_set(&self) {
        self.0.proactive.killed.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    // -------------------------------------------------------------------------------- status & API
    pub fn proactive_status(&self) -> ProactiveStatus {
        self.proactive_status_at(Local::now())
    }

    pub fn proactive_status_at(&self, now: DateTime<Local>) -> ProactiveStatus {
        let st = self.state();
        let level = self.proactive_level();
        let today = now.format("%Y-%m-%d").to_string();
        let spent = if st.spent_day == today { st.spent_s } else { 0.0 };
        let next_wake = (level != "off").then(|| self.next_wake(&st, now).to_rfc3339());
        ProactiveStatus {
            level,
            last_tick: st.last_result.clone(),
            skipped_reason: st.last_skip.clone(),
            next_wake,
            budget_used_minutes: (spent / 60.0 * 10.0).round() / 10.0,
            budget_minutes: self.cfg_u64("proactive.daily_budget_minutes", 15) as u32,
            open_proposals: self
                .0
                .store
                .proposal_list(Some("open"), 500)
                .map(|l| l.iter().filter(|p| p.kind == "proposal").count() as u32)
                .unwrap_or(0),
            stopped_by_kill_switch: self.0.proactive.killed.load(std::sync::atomic::Ordering::SeqCst),
            on_power: *self.0.proactive.on_power.lock().unwrap(),
        }
    }

    fn next_wake(&self, st: &State, now: DateTime<Local>) -> DateTime<Local> {
        let every = ChronoDuration::minutes(self.cfg_u64("proactive.interval_minutes", 30).max(5) as i64);
        let last = st.last_tick_at.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&Local));
        let interval = last.map(|l| l + every).filter(|t| *t > now).unwrap_or(now + every);
        let today = Local.from_local_datetime(&now.date_naive().and_time(self.briefing_time())).earliest().unwrap_or(now);
        let briefing = if today > now { today } else { today + ChronoDuration::days(1) };
        interval.min(briefing)
    }

    /// Proposals, newest first. `status` / `kind` narrow the list. Approved proposals whose run ended become `done`,
    /// and open ones past their date `expired`, before the list is read.
    pub fn proposals(&self, status: Option<&str>, kind: Option<&str>) -> Result<Vec<Proposal>> {
        if let Some(s) = status.filter(|s| !s.is_empty()) {
            if !aiw_core::api::PROPOSAL_STATUSES.contains(&s) {
                return Err(Error::Invalid(format!("stato non valido «{s}»: usa {}", aiw_core::api::PROPOSAL_STATUSES.join(", "))));
            }
        }
        if let Some(k) = kind.filter(|k| !k.is_empty()) {
            if !aiw_core::api::PROPOSAL_KINDS.contains(&k) {
                return Err(Error::Invalid(format!("tipo non valido «{k}»: usa {}", aiw_core::api::PROPOSAL_KINDS.join(", "))));
            }
        }
        self.refresh_proposals(Local::now());
        let mut list = self.0.store.proposal_list(status.filter(|s| !s.is_empty()), 500)?;
        if let Some(k) = kind.filter(|k| !k.is_empty()) {
            list.retain(|p| p.kind == k);
        }
        Ok(list)
    }

    pub fn proposal(&self, id: &str) -> Result<Proposal> {
        self.refresh_proposals(Local::now());
        self.0.store.proposal_get(id)?.ok_or_else(|| Error::NotFound(format!("proposta {id} non trovata")))
    }

    fn refresh_proposals(&self, now: DateTime<Local>) {
        let _ = self.0.store.proposal_expire(&now.to_rfc3339());
        for p in self.0.store.proposal_list(Some("approved"), 100).unwrap_or_default() {
            if p.run_id.as_deref().is_some_and(|r| self.run_done(r)) {
                let _ = self.0.store.proposal_transition(&p.id, &["approved"], "done", None, None, None);
            }
        }
    }

    pub fn proposal_dismiss(&self, id: &str) -> Result<Proposal> {
        let p = self.proposal(id)?;
        if !self.0.store.proposal_transition(id, &["open"], "dismissed", Some(&now_rfc3339()), None, None)? {
            return Err(Error::Conflict(format!("la proposta «{}» non è più in attesa ({})", p.title, p.status)));
        }
        self.audit_user("proposal.dismiss", &p.title);
        self.proposal(id)
    }

    /// Approves a proposal: a normal run of its agent starts in a new session with the instruction (edited or not),
    /// with the agent's usual policy and approvals. Run and proposal are linked.
    pub async fn proposal_approve(&self, id: &str, instruction: Option<&str>) -> Result<Proposal> {
        let p = self.proposal(id)?;
        if p.kind != "proposal" {
            return Err(Error::Conflict("un riepilogo non si esegue: puoi solo leggerlo o scartarlo".into()));
        }
        if p.status != "open" {
            return Err(Error::Conflict(format!("la proposta «{}» non è più in attesa ({})", p.title, p.status)));
        }
        let text = instruction.map(str::trim).filter(|t| !t.is_empty()).unwrap_or(p.instruction.trim()).to_string();
        if text.is_empty() {
            return Err(Error::Invalid("la proposta non ha istruzioni da eseguire".into()));
        }
        let agent = self
            .agent(&p.agent_id)
            .map_err(|_| Error::Conflict(format!("l'assistente «{}» non esiste più: scegline un altro nelle impostazioni", p.agent_id)))?;
        // Claim it first: a double click must not start two runs.
        let edited = instruction.map(str::trim).filter(|t| !t.is_empty() && *t != p.instruction.trim());
        if !self.0.store.proposal_transition(id, &["open"], "approved", Some(&now_rfc3339()), None, edited)? {
            return Err(Error::Conflict("la proposta è già stata decisa".into()));
        }
        let started = async {
            let session = self.create_session(&agent.id, Some(format!("Proposta: {}", short(&p.title, 50))))?;
            let run = self.start_run(&session.id, text).await?;
            Ok::<_, Error>((run, session.id))
        }
        .await;
        match started {
            Ok((run, session)) => {
                self.0.store.proposal_transition(id, &["approved"], "approved", None, Some((&run, &session)), None)?;
                self.audit_user("proposal.approve", &format!("{} → run {run}", p.title));
                self.proposal(id)
            }
            Err(e) => {
                // It did not start: it is waiting again.
                let _ = self.0.store.proposal_transition(id, &["approved"], "open", None, None, None);
                Err(e)
            }
        }
    }

    // -------------------------------------------------------------------------------- wake sources
    /// The virtual workflows of the wake sources: the same trigger machinery as the user's workflows.
    pub(crate) fn proactive_wake_defs(&self) -> Vec<WorkflowDef> {
        let t = self.briefing_time();
        let mins = self.cfg_u64("proactive.interval_minutes", 30).max(5);
        let def = |id: &str, trigger: Value| -> WorkflowDef {
            serde_json::from_value(json!({"id": id, "name": id, "enabled": true, "trigger": trigger, "steps": []}))
                .expect("static workflow")
        };
        vec![
            def("proactive:interval", json!({"kind": "interval", "seconds": mins * 60})),
            def("proactive:briefing", json!({"kind": "cron", "expr": format!("{} {} * * *", t.minute(), t.hour())})),
        ]
    }

    /// Which wake sources are due at `now`: the scheduler's interval and daily time, and due reminders that
    /// have no proposal yet. Called by the background loop every few seconds.
    pub(crate) fn proactive_due_wakes(&self, now: DateTime<Local>) -> Vec<Wake> {
        if self.proactive_level() == "off" {
            return vec![];
        }
        let defs = self.proactive_wake_defs();
        let mut wakes: Vec<Wake> = self
            .0
            .proactive
            .sched
            .lock()
            .unwrap()
            .due(&defs, now)
            .into_iter()
            .filter_map(|id| match id.as_str() {
                "proactive:interval" => Some(Wake::Interval),
                "proactive:briefing" => Some(Wake::Briefing),
                _ => None,
            })
            .collect();
        if wakes.is_empty() && !self.unproposed_due_reminders(now).is_empty() {
            wakes.push(Wake::Reminder);
        }
        wakes
    }

    /// Runs the wakes that are due, each in its own task (the tick lock keeps them from overlapping).
    pub(crate) fn proactive_pass(&self) {
        for wake in self.proactive_due_wakes(Local::now()) {
            let e = self.clone();
            tokio::spawn(async move {
                if let Err(err) = e.proactive_tick(wake).await {
                    tracing::warn!("controllo proattivo: {err}");
                }
            });
        }
    }

    fn unproposed_due_reminders(&self, now: DateTime<Local>) -> Vec<aiw_core::api::Memory> {
        let Ok(list) = self.memories(Some("active"), Some("reminder")) else { return vec![] };
        list.into_iter()
            .filter(|m| m.due_at.as_deref().and_then(|d| DateTime::parse_from_rfc3339(d).ok()).is_some_and(|d| d <= now))
            .filter(|m| !self.0.store.proposal_source_exists(&format!("reminder:{}", m.id)).unwrap_or(true))
            .collect()
    }

    // -------------------------------------------------------------------------------- the tick
    pub async fn proactive_tick(&self, wake: Wake) -> Result<TickResult> {
        self.proactive_tick_at(wake, Local::now()).await
    }

    fn result(wake: Wake, outcome: &str, reason: Option<String>, now: DateTime<Local>) -> TickResult {
        TickResult {
            wake: wake.as_str().into(),
            outcome: outcome.into(),
            reason,
            proposals: vec![],
            model_called: false,
            at: now.to_rfc3339(),
        }
    }

    /// One heartbeat at `now` (the clock is a parameter so that tests can move it). Never runs two at once:
    /// a tick that finds another one going answers `skipped` and records nothing.
    pub async fn proactive_tick_at(&self, wake: Wake, now: DateTime<Local>) -> Result<TickResult> {
        let Ok(_guard) = self.0.proactive.lock.try_lock() else {
            return Ok(Self::result(wake, "skipped", Some("Un controllo è già in corso.".into()), now));
        };
        let mut state = self.state();
        let today = now.format("%Y-%m-%d").to_string();
        if state.spent_day != today {
            state.spent_day = today;
            state.spent_s = 0.0;
        }
        if let Some(why) = self.tick_gate(&state, now) {
            let r = Self::result(wake, "skipped", Some(why.clone()), now);
            state.last_skip = Some(why);
            state.last_result = Some(r.clone());
            self.save_state(&state);
            return Ok(r);
        }
        let started = std::time::Instant::now();
        let cancel = CancellationToken::new();
        *self.0.proactive.current.lock().unwrap() = Some(cancel.clone());
        let mut result = self.tick_work(wake, now, &state, &cancel).await.unwrap_or_else(|e| {
            let mut r = Self::result(wake, "nothing", Some(format!("Non sono riuscito a completare il controllo: {e}")), now);
            r.model_called = false;
            r
        });
        result.at = now.to_rfc3339();
        *self.0.proactive.current.lock().unwrap() = None;
        state.spent_s += started.elapsed().as_secs_f64();
        state.last_tick_at = Some(now.to_rfc3339());
        state.last_skip = None;
        state.last_result = Some(result.clone());
        self.save_state(&state);
        Ok(result)
    }

    /// Why the tick must not run now, in Italian; `None` when it may.
    fn tick_gate(&self, state: &State, now: DateTime<Local>) -> Option<String> {
        if self.proactive_level() == "off" {
            return Some("L'assistente proattivo è spento: lo accendi dalle impostazioni.".into());
        }
        if self.0.proactive.killed.load(std::sync::atomic::Ordering::SeqCst) {
            return Some("Fermato dal pulsante di emergenza: ricomincia quando scegli di nuovo il livello nelle impostazioni.".into());
        }
        let quiet = self.cfg_str("proactive.quiet_hours", "");
        if in_quiet_hours(&quiet, now.time()) {
            return Some(format!("Orario di silenzio ({}).", quiet.trim()));
        }
        let only_on_power = self.0.config.get("proactive.only_on_power").and_then(|v| v.as_bool()).unwrap_or(true);
        if only_on_power && *self.0.proactive.on_power.lock().unwrap() == Some(false) {
            return Some("Il computer è a batteria: aspetto che sia collegato.".into());
        }
        let budget_min = self.cfg_u64("proactive.daily_budget_minutes", 15);
        if state.spent_s >= (budget_min * 60) as f64 {
            return Some(format!("Il tempo di oggi è finito ({budget_min} minuti): riprovo domani."));
        }
        if self.0.runs.lock().unwrap().values().any(|h| !h.done) {
            return Some("Stai usando l'assistente: riprovo più tardi.".into());
        }
        None
    }

    async fn tick_work(&self, wake: Wake, now: DateTime<Local>, state: &State, cancel: &CancellationToken) -> Result<TickResult> {
        self.refresh_proposals(now);
        let mut created: Vec<String> = vec![];
        let agent_id = self.cfg_str("proactive.agent", "operator");
        let expires = (now + ChronoDuration::days(PROPOSAL_DAYS)).to_rfc3339();
        let new_proposal = |kind: &str, title: String, noticed: String, instruction: String, source: String, status: &str| Proposal {
            id: new_id("prp"),
            kind: kind.into(),
            title,
            noticed,
            instruction,
            source,
            agent_id: agent_id.clone(),
            status: status.into(),
            created_at: now.to_rfc3339(),
            expires_at: expires.clone(),
            decided_at: None,
            run_id: None,
            session_id: None,
        };

        let since = state
            .last_tick_at
            .as_deref()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Local))
            .unwrap_or(now - ChronoDuration::days(1));
        let folders: Vec<String> =
            self.0.config.get("proactive.watch_folders").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();

        // The morning briefing: once a day, from the briefing time on, from data only.
        let day_key = format!("briefing:{}", now.format("%Y-%m-%d"));
        if now.time() >= self.briefing_time() && !self.0.store.proposal_source_exists(&day_key)? {
            let (text, empty) = self.briefing_text(now, &folders);
            let status = if empty { "done" } else { "open" };
            let p = new_proposal("briefing", "Il riepilogo di oggi".into(), text, String::new(), day_key, status);
            self.0.store.proposal_insert(&p)?;
            created.push(p.id);
        }

        // Due reminders need no model.
        for m in self.unproposed_due_reminders(now) {
            let p = new_proposal(
                "proposal",
                format!("Promemoria: {}", short(&m.text, 60)),
                format!("Avevi chiesto di ricordartelo: «{}».", short(&m.text, 200)),
                format!("Aiutami con questo promemoria: {}", m.text),
                format!("reminder:{}", m.id),
                "open",
            );
            self.0.store.proposal_insert(&p)?;
            created.push(p.id);
        }

        // The context for the filter.
        let mut items: Vec<Item> = vec![];
        let workflows = self.workflows().unwrap_or_default();
        for r in self.0.store.workflow_runs(50)?.into_iter().filter(|r| r.status == "failed") {
            let started = DateTime::parse_from_rfc3339(&r.started_at).map(|t| t.with_timezone(&Local)).unwrap_or(now);
            if started > since {
                let name =
                    workflows.iter().find(|w| w.id == r.workflow_id).map(|w| w.name.clone()).unwrap_or_else(|| r.workflow_id.clone());
                items.push(Item { source: format!("workflow_failed:{}", r.id), text: format!("Il workflow «{name}» è fallito.") });
            }
        }
        for a in self.approvals() {
            items.push(Item {
                source: format!("approval:{}", a.id),
                text: format!("Una conferma aspetta una risposta: {}", short(&a.reason, 100)),
            });
        }
        let files = new_files(&folders, since);
        for folder in &folders {
            let mine: Vec<_> = files.iter().filter(|f| &f.0 == folder).collect();
            if let Some(newest) = mine.first() {
                let names = mine
                    .iter()
                    .take(3)
                    .map(|f| f.1.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join(", ");
                items.push(Item {
                    source: format!("files:{folder}:{}", newest.2.timestamp()),
                    text: format!("{} file nuovi in {folder}: {names}", mine.len()),
                });
            }
        }
        let mut fresh = vec![];
        for it in items {
            if !self.0.store.proposal_source_exists(&it.source)? {
                fresh.push(it);
            }
        }
        fresh.truncate(MAX_ITEMS);

        let mut result = Self::result(wake, if created.is_empty() { "nothing" } else { "proposed" }, None, now);
        if fresh.is_empty() {
            result.reason =
                Some(if created.is_empty() { "Niente di nuovo da guardare.".into() } else { "Promemoria e riepilogo pronti.".into() });
            result.proposals = created;
            return Ok(result);
        }

        // One question to the smallest model.
        result.model_called = true;
        let pinned: Vec<String> = self
            .0
            .store
            .memory_list(&crate::store::MemoryFilter {
                statuses: vec!["active".into()],
                pinned: Some(true),
                limit: Some(5),
                ..Default::default()
            })?
            .into_iter()
            .map(|m| m.text)
            .collect();
        let open_titles: Vec<String> = self.0.store.proposal_list(Some("open"), 10)?.into_iter().map(|p| p.title).collect();
        let profile = self.proactive_filter_profile().await?;
        let answer = self.ask_once(&profile, filter_messages(&fresh, &pinned, &open_titles), cancel).await?;
        let Some(n) = parse_filter(&answer, fresh.len()) else {
            result.reason = Some("Il filtro non ha trovato nulla che meriti una proposta adesso.".into());
            result.proposals = created;
            return Ok(result);
        };
        let item = fresh[n - 1].clone();

        // The proposal run: the configured agent, read-only, with a small budget.
        let prompt = format!(
            "Hai notato questo: {}\n\nPrepara una proposta per l'utente. Se serve, leggi qualcosa con gli strumenti di sola lettura. \
Non puoi modificare nulla. Rispondi con un JSON: \"title\" (titolo breve), \"noticed\" (cosa hai notato, in una o due frasi) \
e \"instruction\" (cosa faresti, scritto come istruzione pronta da eseguire).",
            item.text
        );
        let (run_agent, run_title) = (format!("{PROACTIVE_PREFIX}{agent_id}"), format!("Proposta: {}", short(&item.text, 40)));
        let run = self.run_agent_task(&run_agent, &prompt, None, &run_title);
        let (text, _session) = tokio::select! {
            r = run => r?,
            _ = cancel.cancelled() => return Err(Error::Cancelled),
        };
        let parsed = aiw_agent::schema::extract_json(&text)
            .filter(|v| v.get("instruction").and_then(|i| i.as_str()).is_some_and(|i| !i.trim().is_empty()));
        let (title, noticed, instruction) = match parsed {
            Some(v) => (
                v["title"].as_str().unwrap_or("Una proposta").trim().to_string(),
                v["noticed"].as_str().unwrap_or(&item.text).trim().to_string(),
                v["instruction"].as_str().unwrap_or_default().trim().to_string(),
            ),
            // The model did not answer in the form: keep what it said as the note, and the item as the task.
            None => (short(&item.text, 60), item.text.clone(), format!("Occupati di questo: {}", item.text)),
        };
        let p = new_proposal("proposal", short(&title, 80), short(&noticed, 400), instruction, item.source, "open");
        self.0.store.proposal_insert(&p)?;
        created.push(p.id);
        result.outcome = "proposed".into();
        result.proposals = created;
        Ok(result)
    }

    /// The text of the briefing and whether there was nothing to report.
    fn briefing_text(&self, now: DateTime<Local>, folders: &[String]) -> (String, bool) {
        let mut lines: Vec<String> = vec![];
        let day_start = Local.from_local_datetime(&now.date_naive().and_hms_opt(0, 0, 0).unwrap()).earliest().unwrap_or(now);
        let due = self.unproposed_due_reminders(now).len()
            + self
                .memories(Some("active"), Some("reminder"))
                .unwrap_or_default()
                .iter()
                .filter(|m| {
                    m.due_at.as_deref().and_then(|d| DateTime::parse_from_rfc3339(d).ok()).is_some_and(|d| d <= now)
                        && self.0.store.proposal_source_exists(&format!("reminder:{}", m.id)).unwrap_or(false)
                })
                .count();
        let upcoming: Vec<String> = self
            .memories(Some("active"), Some("reminder"))
            .unwrap_or_default()
            .into_iter()
            .filter(|m| {
                m.due_at
                    .as_deref()
                    .and_then(|d| DateTime::parse_from_rfc3339(d).ok())
                    .is_some_and(|d| d > now && d <= now + ChronoDuration::hours(24))
            })
            .map(|m| short(&m.text, 60))
            .collect();
        if due > 0 {
            lines.push(format!("Promemoria da guardare: {due}."));
        }
        if !upcoming.is_empty() {
            lines.push(format!("Promemoria nelle prossime 24 ore: {}.", upcoming.join("; ")));
        }
        let open = self.0.store.proposal_list(Some("open"), 100).unwrap_or_default().into_iter().filter(|p| p.kind == "proposal").count();
        if open > 0 {
            lines.push(format!("Proposte in attesa: {open}."));
        }
        let (ran, failed) =
            self.0.store.run_counts_between(&(day_start - ChronoDuration::days(1)).to_rfc3339(), &day_start.to_rfc3339()).unwrap_or((0, 0));
        if ran > 0 {
            lines.push(if failed > 0 {
                format!("Ieri: {ran} lavori, {failed} non riusciti.")
            } else {
                format!("Ieri: {ran} lavori, tutti riusciti.")
            });
        }
        let files = new_files(folders, now - ChronoDuration::hours(24));
        if !files.is_empty() {
            let names = files
                .iter()
                .take(3)
                .map(|f| f.1.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("File nuovi nelle cartelle osservate: {} ({names}).", files.len()));
        }
        if lines.is_empty() {
            return ("Niente da segnalare per oggi.".into(), true);
        }
        (lines.join("\n"), false)
    }

    // -------------------------------------------------------------------------------- model plumbing
    /// The smallest suitable model for the filter: the `summarize` role of the automatic roles when the device
    /// has one, else the adviser's chat choice. On a runtime that holds one model at a time it never asks for
    /// a different model than the one that is running (that would restart it under the person's chat).
    async fn proactive_filter_profile(&self) -> Result<InferenceProfile> {
        let main_profile = self.resolve_auto_profile(AdviserTask::Agent).or_else(|_| self.profile("default")).ok();
        let picked = main_profile.as_ref().and_then(|main| {
            let picks = adviser::plan_auto_roles(
                &self.device(),
                &self.live_local_models(),
                &aiw_models::builtin_catalog(),
                &defaults::Availability::from_runtimes(&self.0.runtimes.list()),
                &MainModel { runtime_id: main.runtime_id.clone(), model: main.model.clone() },
                None,
            );
            picks.summarize.map(|p| {
                let mut prof = defaults::profile(p.name.clone(), &p.runtime_id, &p.model, p.context.min(8192));
                prof.id = format!("role:{}", p.model);
                prof
            })
        });
        let mut profile = match picked {
            Some(p) => p,
            None => self.resolve_auto_profile(AdviserTask::Chat)?,
        };
        if self.0.runtimes.holds_one_model(&profile.runtime_id) {
            if let Some(current) = self.0.runtimes.serving_model(&profile.runtime_id).await {
                profile.model = current;
            }
        }
        profile.params.max_tokens = Some(120);
        profile.params.temperature = Some(0.0);
        Ok(profile)
    }

    /// One short question, the whole answer as text.
    async fn ask_once(&self, p: &InferenceProfile, messages: Vec<Msg>, cancel: &CancellationToken) -> Result<String> {
        let backend = self
            .0
            .runtimes
            .backend_for(&p.runtime_id, (!p.model.is_empty()).then_some(p.model.as_str()), p.params.context, &p.params.adapters)
            .await?;
        let req = ChatRequest {
            model: p.model.clone(),
            messages,
            temperature: p.params.temperature,
            max_tokens: p.params.max_tokens,
            thinking: self.thinking_for(p, Some(aiw_core::api::ThinkingMode::Off)).map(|_| aiw_runtimes::ThinkingRequest {
                enabled: false,
                deep: false,
                budget: None,
            }),
            ..Default::default()
        };
        let mut stream = backend.chat(req, cancel.clone()).await?;
        let mut out = String::new();
        while let Some(d) = stream.next().await {
            if let ChatDelta::Text(t) = d? {
                out.push_str(&t);
            }
        }
        // Inline reasoning blocks are not part of the answer.
        let mut text = out.as_str();
        let mut clean = String::new();
        while let Some(i) = text.find("<think>") {
            clean.push_str(&text[..i]);
            match text[i..].find("</think>") {
                Some(j) => text = &text[i + j + "</think>".len()..],
                None => {
                    text = "";
                    break;
                }
            }
        }
        clean.push_str(text);
        Ok(clean.trim().to_string())
    }

    /// The agent a proposal run uses: the configured one, read-only. Never more permissive than its own
    /// policy: plan mode (so only reading is allowed), a read-only sandbox, only the tools that read (plus
    /// `todo` and the memory search), no sub-agents and a small budget.
    pub(crate) fn proactive_agent(&self, base_id: &str) -> Result<AgentProfile> {
        let mut a = self.agent(base_id)?;
        let network = self.proactive_level() == "act_readonly";
        let reg = self.tool_registry();
        let allowed = |name: &str| a.tools.iter().any(|t| t == "*" || t == name);
        let mut tools: Vec<String> = reg
            .all()
            .into_iter()
            .filter(|t| {
                reg.is_enabled(t.name())
                    && !t.explicit_grant()
                    && (t.risk() == Risk::Read || (network && t.risk() == Risk::Network))
                    && allowed(t.name())
            })
            .map(|t| t.name().to_string())
            .collect();
        for internal in ["todo", "memory_search"] {
            if allowed(internal) {
                tools.push(internal.into());
            }
        }
        a.id = format!("{PROACTIVE_PREFIX}{}", a.id);
        a.name = format!("{} (proposte)", a.name);
        a.tools = tools;
        a.mode = "plan".into();
        a.sandbox = "read-only".into();
        a.max_turns = a.max_turns.min(6);
        let own = a.budget.clone().unwrap_or_default();
        a.budget = Some(BudgetLimits {
            max_tokens: Some(own.max_tokens.unwrap_or(20_000).min(20_000)),
            max_turns: Some(own.max_turns.unwrap_or(8).min(8)),
            max_seconds: Some(own.max_seconds.unwrap_or(120).min(120)),
        });
        a.output_schema = Some(proposal_schema());
        a.roles = None;
        a.candidate_profiles = vec![];
        a.system_prompt =
            format!("{}\n\nStai lavorando in background per preparare una proposta: non puoi modificare nulla.", a.system_prompt.trim());
        Ok(a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn quiet_hours_cross_midnight_and_ignore_nonsense() {
        assert!(in_quiet_hours("22:00-07:00", t(23, 30)));
        assert!(in_quiet_hours("22:00-07:00", t(3, 0)));
        assert!(in_quiet_hours("22:00-07:00", t(22, 0)), "the start is inside");
        assert!(!in_quiet_hours("22:00-07:00", t(7, 0)), "the end is outside");
        assert!(!in_quiet_hours("22:00-07:00", t(12, 0)));
        assert!(in_quiet_hours("13:00-14:30", t(13, 45)) && !in_quiet_hours("13:00-14:30", t(14, 30)));
        for bad in ["", "22:00", "22-07", "aa:bb-cc:dd", "08:00-08:00", "25:00-07:00"] {
            assert!(!in_quiet_hours(bad, t(23, 0)), "{bad}");
        }
    }

    #[test]
    fn the_filter_answer_is_strict() {
        assert_eq!(parse_filter(r#"{"act": true, "item": 2, "why": "fallito"}"#, 3), Some(2));
        assert_eq!(parse_filter("Ecco: ```json\n{\"act\": true, \"item\": 1}\n```", 3), Some(1));
        assert_eq!(parse_filter(r#"{"act": false}"#, 3), None);
        assert_eq!(parse_filter(r#"{"act": true, "item": 4}"#, 3), None, "out of range");
        assert_eq!(parse_filter(r#"{"act": true, "item": 0}"#, 3), None);
        assert_eq!(parse_filter(r#"{"act": true}"#, 3), None, "no item");
        assert_eq!(parse_filter(r#"{"act": "sì", "item": 1}"#, 3), None, "act must be a boolean");
        assert_eq!(parse_filter("forse sì", 3), None);
    }

    #[test]
    fn the_filter_question_lists_the_items_and_the_background() {
        let items = vec![Item { source: "a".into(), text: "Il workflow «X» è fallito.".into() }];
        let m = filter_messages(&items, &["Odio le riunioni".into()], &["Già proposta".into()]);
        let u = m[1].text();
        assert!(u.contains("1. Il workflow") && u.contains("Odio le riunioni") && u.contains("Già proposta"));
        assert!(m[0].text().contains("act"));
    }

    #[test]
    fn wakes_round_trip() {
        for w in [Wake::Interval, Wake::Briefing, Wake::Reminder, Wake::Manual] {
            assert_eq!(Wake::parse(w.as_str()), Some(w));
        }
        assert_eq!(Wake::parse("x"), None);
    }
}
