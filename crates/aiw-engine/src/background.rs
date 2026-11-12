//! The proactive check woken by the operating system of a phone, with a hard time budget.
//!
//! Android (WorkManager) and iOS (BGTaskScheduler, Shortcuts) start the app for a few seconds and ask it to look
//! around. [`Engine::proactive_background_tick`] is that one entry: it runs the same tick as
//! `POST /api/proactive/tick` (`proactive_tick_at`; the gates, the briefing, the reminders, the calendar and mail
//! counts, the filter, all unchanged) inside a time budget and answers with a [`BackgroundSummary`] small enough for a
//! local notification. Two things are specific to this entry:
//!
//! * `reason` (`interval`, `briefing`, `shortcut`) says what woke it. `briefing` and `shortcut` also hand back
//!   today's briefing text, writing it first if it is not there yet (the "Riepilogo di oggi" Shortcut).
//! * With no local model the model-free parts still run; the filter step goes to a paired device if one answers,
//!   else it is skipped and the reason is recorded (see `TickResult::model_skipped`).
//!
//! [`run_background_blocking`] is what the native glue calls: it takes and returns JSON text, uses the engine of the
//! running app when there is one, and otherwise builds a headless one (no sidecars, no network probes, no background
//! loops) on the app's data folder, so the check also works when the process was started by the system alone.

use crate::proactive::Wake;
use crate::{Engine, EngineOptions};
use aiw_core::api::{BackgroundRequest, BackgroundSummary, LocalNotice, Proposal};
use aiw_core::{Error, Result};
use chrono::{DateTime, Local};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Default and bounds of the time budget, in seconds. Android gives a worker minutes, iOS gives a refresh task about
/// 30 seconds: stay under the smaller one.
pub const DEFAULT_BUDGET_S: u32 = 25;
const MIN_BUDGET_S: u32 = 5;
const MAX_BUDGET_S: u32 = 600;

/// The engine of the app that is running in this process (set by the shell at start).
static RUNNING: Mutex<Option<Engine>> = Mutex::new(None);

/// The shell calls this once the engine exists, so that a system wake-up uses it instead of building a second one.
pub fn register_running(engine: &Engine) {
    *RUNNING.lock().unwrap() = Some(engine.clone());
}

/// The wake that a `reason` stands for.
pub fn wake_for(reason: &str) -> Wake {
    match reason {
        "briefing" => Wake::Briefing,
        "shortcut" => Wake::Manual,
        _ => Wake::Interval,
    }
}

/// `reason` as it is reported back: anything unknown is an `interval` wake.
pub fn normalize_reason(reason: &str) -> &'static str {
    match reason.trim() {
        "briefing" => "briefing",
        "shortcut" => "shortcut",
        _ => "interval",
    }
}

/// The text of a local notification for what a check found. `None` when there is nothing worth interrupting for.
fn notice_for(new: &[Proposal], briefing: Option<&str>) -> Option<LocalNotice> {
    let proposals: Vec<&Proposal> = new.iter().filter(|p| p.kind == "proposal").collect();
    match (proposals.len(), briefing) {
        (0, None) => None,
        (0, Some(b)) => Some(LocalNotice { title: "Il riepilogo di oggi".into(), body: first_line(b, 140) }),
        (1, _) => Some(LocalNotice { title: "Una proposta per te".into(), body: first_line(&proposals[0].title, 140) }),
        (n, _) => Some(LocalNotice {
            title: format!("{n} proposte per te"),
            body: proposals.iter().take(2).map(|p| first_line(&p.title, 50)).collect::<Vec<_>>().join(" · "),
        }),
    }
}

fn first_line(s: &str, max: usize) -> String {
    let line = s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.chars().count() <= max {
        line.to_string()
    } else {
        format!("{}…", line.chars().take(max.saturating_sub(1)).collect::<String>().trim_end())
    }
}

impl Engine {
    /// One proactive check woken by the system, inside `budget`. Never returns an error for something a tick would
    /// skip (those come back as `outcome: "skipped"` with the reason); an error means the engine itself failed.
    pub async fn proactive_background_tick(&self, req: &BackgroundRequest) -> Result<BackgroundSummary> {
        self.proactive_background_tick_at(req, Local::now()).await
    }

    /// [`Self::proactive_background_tick`] with the clock as a parameter.
    pub async fn proactive_background_tick_at(&self, req: &BackgroundRequest, now: DateTime<Local>) -> Result<BackgroundSummary> {
        let budget = Duration::from_secs(req.budget_s.unwrap_or(DEFAULT_BUDGET_S).clamp(MIN_BUDGET_S, MAX_BUDGET_S) as u64);
        self.background_tick_within(req, now, budget).await
    }

    /// The check with an exact budget (the public entry keeps it between 5 and 600 seconds; tests use less).
    pub(crate) async fn background_tick_within(
        &self,
        req: &BackgroundRequest,
        now: DateTime<Local>,
        budget: Duration,
    ) -> Result<BackgroundSummary> {
        let reason = normalize_reason(&req.reason);
        if let Some(p) = req.on_power {
            self.set_on_power(Some(p));
        }
        let started = Instant::now();
        let before: HashSet<String> = self.0.store.proposal_list(None, 500)?.into_iter().map(|p| p.id).collect();

        let tick = self.proactive_tick_at(wake_for(reason), now);
        let (result, timed_out) = match tokio::time::timeout(budget, tick).await {
            Ok(r) => (Some(r?), false),
            Err(_) => {
                // Out of time: stop the model call or run of the tick. What it already saved (briefing, reminders)
                // stays, and the next check does not repeat it.
                (None, true)
            }
        };

        let new: Vec<Proposal> = self.0.store.proposal_list(None, 500)?.into_iter().filter(|p| !before.contains(&p.id)).collect();
        let today = format!("briefing:{}", now.format("%Y-%m-%d"));
        let todays_briefing = self.0.store.proposal_list(None, 500)?.into_iter().find(|p| p.kind == "briefing" && p.source == today);
        let new_briefing = new.iter().find(|p| p.kind == "briefing" && p.status == "open").map(|p| p.noticed.clone());
        // `briefing` and `shortcut` give today's text back even when an earlier check wrote it.
        let briefing = match reason {
            "interval" => new_briefing.clone(),
            _ => todays_briefing.map(|p| p.noticed).or(new_briefing.clone()),
        };
        let skipped_reason = result.as_ref().filter(|r| r.outcome == "skipped").and_then(|r| r.reason.clone());
        let (model_step, model_note) = match &result {
            Some(r) if r.delegated_to.is_some() && r.model_called => ("delegated", r.delegated_to.clone()),
            Some(r) if r.model_called => ("ran", None),
            Some(r) if r.model_skipped.is_some() => ("skipped", r.model_skipped.clone()),
            Some(r) if r.outcome == "skipped" => ("skipped", r.reason.clone()),
            None => ("skipped", Some("Il tempo a disposizione è finito prima del filtro.".to_string())),
            _ => ("not_needed", None),
        };
        let outcome = match &result {
            Some(r) if r.outcome == "skipped" => "skipped",
            _ if new.iter().any(|p| p.kind == "proposal") => "proposed",
            _ => "nothing",
        };
        // Quiet hours hold the check itself back, so a notification is only made for what a check did.
        let notification = if outcome == "skipped" { None } else { notice_for(&new, new_briefing.as_deref()) };
        Ok(BackgroundSummary {
            reason: reason.into(),
            outcome: outcome.into(),
            new_proposals: new.iter().filter(|p| p.kind == "proposal").count() as u32,
            briefing,
            model_step: model_step.into(),
            model_note,
            skipped_reason,
            notification,
            timed_out,
            elapsed_ms: started.elapsed().as_millis() as u64,
            at: now.to_rfc3339(),
        })
    }
}

/// Request of the native glue: a [`BackgroundRequest`] plus where the data lives when no engine is running yet.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GlueRequest {
    #[serde(default)]
    reason: String,
    #[serde(default)]
    budget_s: Option<u32>,
    #[serde(default)]
    on_power: Option<bool>,
    /// The app's data folder (the one the shell gives `EngineOptions::data_dir`).
    #[serde(default)]
    data_dir: Option<String>,
    /// The agents' folder (mobile: inside the app container).
    #[serde(default)]
    workspace_root: Option<String>,
}

/// The answer of the glue, always JSON: the summary, or `{"error": "..."}`.
fn answer(r: Result<BackgroundSummary>) -> String {
    match r {
        Ok(s) => serde_json::to_string(&s).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}")),
        Err(e) => serde_json::json!({ "error": e.to_string() }).to_string(),
    }
}

/// What the Android worker and the iOS background tasks call, on their own thread: `request` is JSON
/// (`reason`, `budgetS`, `onPower`, `dataDir`, `workspaceRoot`), the answer is JSON (a [`BackgroundSummary`] or
/// `{"error": …}`). Blocks until the check ends or the budget runs out.
pub fn run_background_blocking(request: &str) -> String {
    let glue: GlueRequest = match serde_json::from_str(request) {
        Ok(g) => g,
        Err(e) => return answer(Err(Error::Invalid(format!("richiesta non valida: {e}")))),
    };
    let req = BackgroundRequest { reason: glue.reason.clone(), budget_s: glue.budget_s, on_power: glue.on_power };
    let running = RUNNING.lock().unwrap().clone();
    let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(rt) => rt,
        Err(e) => return answer(Err(Error::Runtime(format!("runtime: {e}")))),
    };
    answer(rt.block_on(async move {
        if let Some(engine) = running {
            return engine.proactive_background_tick(&req).await;
        }
        let Some(dir) = glue.data_dir.filter(|d| !d.trim().is_empty()) else {
            return Err(Error::Invalid("l'app non è avviata e manca la cartella dei dati".into()));
        };
        let engine = Engine::new(EngineOptions {
            data_dir: Some(PathBuf::from(dir)),
            workspace_root: glue.workspace_root.filter(|d| !d.trim().is_empty()).map(PathBuf::from),
            mode: "embedded".into(),
            // No network probes and no background loops: this process lives for a few seconds.
            offline: true,
            ..Default::default()
        })
        .await?;
        engine.proactive_background_tick(&req).await
    }))
}
