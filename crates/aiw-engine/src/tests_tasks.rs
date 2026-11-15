//! Background tasks: observation, pause, instructions during the task, resume, the rules of the proactive assistant
//! (quiet hours, battery, the day's time, the kill switch), the budget across resumes, the restart and the approvals,
//! with a scripted model, a tool that holds a step until the test lets it go, and no real clock beyond the minutes.

use super::tests::engine;
use super::*;
use aiw_core::api::{BackgroundTask, BudgetLimits, Proposal, TaskResume, TaskStart};
use aiw_runtimes::{ChatBackend, ChatDelta, ChatRequest, DeltaStream, ScriptedBackend};
use aiw_tools::{Risk, Tool, ToolContext, ToolOutput};
use async_trait::async_trait;
use chrono::{Duration as D, Local};
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Notify;

/// A tool that holds the step it is called in until the test lets it go (or the run is cancelled).
struct Hold {
    entered: Arc<Notify>,
    release: Arc<Notify>,
    open: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for Hold {
    fn name(&self) -> &str {
        "hold_step"
    }
    fn description(&self) -> &str {
        "Strumento di prova che aspetta."
    }
    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {}})
    }
    fn risk(&self) -> Risk {
        Risk::Read
    }
    async fn call(&self, _args: serde_json::Value, ctx: &ToolContext) -> Result<ToolOutput> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.open.load(Ordering::SeqCst) {
            self.entered.notify_one();
            tokio::select! {
                _ = self.release.notified() => {}
                _ = ctx.cancel.cancelled() => return Err(Error::Cancelled),
            }
        }
        Ok(ToolOutput::text("passo fatto"))
    }
}

struct Rig {
    e: Engine,
    backend: ScriptedBackend,
    entered: Arc<Notify>,
    release: Arc<Notify>,
    open: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    _d: tempfile::TempDir,
    ws: tempfile::TempDir,
}

async fn rig(turns: Vec<Vec<ChatDelta>>) -> Rig {
    let (e, d, ws) = engine(vec![]).await;
    let backend = ScriptedBackend::new(turns);
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    let (entered, release, open, calls) =
        (Arc::new(Notify::new()), Arc::new(Notify::new()), Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)));
    e.tool_registry().register(Arc::new(Hold {
        entered: entered.clone(),
        release: release.clone(),
        open: open.clone(),
        calls: calls.clone(),
    }));
    Rig { e, backend, entered, release, open, calls, _d: d, ws }
}

fn hold(id: &str) -> Vec<ChatDelta> {
    ScriptedBackend::tool_turn(id, "hold_step", json!({}))
}

fn text(t: &str) -> Vec<ChatDelta> {
    ScriptedBackend::text_turn(t)
}

fn requests(r: &Rig) -> usize {
    r.backend.requests.lock().unwrap().len()
}

fn user_texts(req: &ChatRequest) -> Vec<String> {
    req.messages.iter().filter(|m| m.role == "user").map(|m| m.text()).collect()
}

/// `coder` has `auto-edit` mode and every tool: what a background task must still not let write by itself.
fn start(goal: &str) -> TaskStart {
    TaskStart { instruction: goal.into(), title: None, agent_id: Some("coder".into()), limits: None }
}

async fn wait_for<F: Fn() -> bool>(what: &str, cond: F) {
    for _ in 0..400 {
        if cond() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("timeout: {what}");
}

async fn wait_status(e: &Engine, id: &str, status: &str) -> BackgroundTask {
    wait_for(&format!("task {status}"), || e.task(id).map(|t| t.status == status).unwrap_or(false)).await;
    e.task(id).unwrap()
}

/// A window of quiet hours that contains this moment (and one that does not).
fn quiet_now() -> String {
    let n = Local::now();
    format!("{}-{}", (n - D::hours(1)).format("%H:%M"), (n + D::hours(1)).format("%H:%M"))
}

fn set(e: &Engine, key: &str, v: serde_json::Value) {
    e.set_config("global", key, v).unwrap();
}

fn proposal(e: &Engine, instruction: &str) -> Proposal {
    let now = Local::now();
    let p = Proposal {
        id: new_id("prp"),
        kind: "proposal".into(),
        title: "Riordina".into(),
        noticed: "Notato".into(),
        instruction: instruction.into(),
        source: format!("test:{}", new_id("x")),
        agent_id: "coder".into(),
        status: "open".into(),
        created_at: now.to_rfc3339(),
        expires_at: (now + D::days(7)).to_rfc3339(),
        decided_at: None,
        run_id: None,
        session_id: None,
    };
    e.0.store.proposal_insert(&p).unwrap();
    p
}

// ------------------------------------------------------------------------------------ pause, instruct, resume

#[tokio::test]
async fn a_task_is_paused_between_two_steps_hears_the_person_and_goes_on_without_repeating_anything() {
    let r = rig(vec![hold("c1"), hold("c2"), text("Finito il lavoro.")]).await;
    let e = &r.e;
    let t = e.task_start(start("Fai il lavoro")).await.unwrap();
    assert_eq!((t.status.as_str(), t.segments, t.origin.as_str()), ("running", 1, "user"));
    // The first step is going on: ask for a pause and give an instruction. The step finishes, then it stops.
    r.entered.notified().await;
    let asked = e.task_pause(&t.id).unwrap();
    assert!(asked.stopping && asked.status == "running", "the step in progress is not cut: {asked:?}");
    e.task_instruct(&t.id, "Scrivi in tono formale", None).unwrap();
    r.release.notify_one();
    let paused = wait_status(e, &t.id, "paused").await;
    assert_eq!((paused.pause_reason.as_deref(), paused.auto_resume), (Some("user"), false));
    assert_eq!(requests(&r), 1, "no second model call after the pause");
    assert_eq!(r.calls.load(Ordering::SeqCst), 1);
    assert_eq!(paused.progress.turns, 1);
    assert_eq!(paused.progress.last_tool.as_deref(), Some("hold_step"));
    // What it did is in the session already (the user message, the step and its result), once.
    let msgs = e.messages(&paused.session_id).unwrap();
    assert_eq!(msgs.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(), ["user", "assistant", "tool"], "{msgs:?}");
    // The instruction waits for the resume; nothing is lost and nothing is delivered to a task that is stopping.
    assert_eq!(paused.instructions.len(), 1);
    assert_eq!(paused.instructions[0].status, "pending");

    let resumed = e.task_resume(&t.id, TaskResume::default()).await.unwrap();
    assert_eq!((resumed.status.as_str(), resumed.segments), ("running", 2));
    // The new run starts from the session and carries the instruction in its first message.
    r.entered.notified().await;
    {
        let seen = r.backend.requests.lock().unwrap();
        let first_of_second = &seen[1];
        let users = user_texts(first_of_second);
        assert!(users.iter().any(|u| u.contains("Riprendi il lavoro") && u.contains("Scrivi in tono formale")), "{users:?}");
        assert_eq!(users.iter().filter(|u| u.as_str() == "Fai il lavoro").count(), 1, "the goal is not repeated");
    }
    r.release.notify_one();
    let done = wait_status(e, &t.id, "succeeded").await;
    assert_eq!(done.result.as_deref(), Some("Finito il lavoro."));
    assert_eq!((done.segments, done.progress.turns), (2, 3));
    assert_eq!(done.instructions[0].status, "delivered");
    // The whole session, once: no message of the first segment is stored twice.
    let msgs = e.messages(&done.session_id).unwrap();
    let ids: Vec<_> = msgs.iter().flat_map(|m| m.tool_calls.iter().flatten().map(|c| c.id.clone())).collect();
    assert_eq!(ids, ["c1", "c2"], "{msgs:?}");
    assert_eq!(msgs.iter().filter(|m| m.content == "Fai il lavoro").count(), 1);
    assert_eq!(msgs.iter().filter(|m| m.role == "tool").count(), 2);
    // Finished: it cannot be paused, resumed or told anything more.
    assert!(matches!(e.task_pause(&t.id), Err(Error::Conflict(_))));
    assert!(matches!(e.task_resume(&t.id, TaskResume::default()).await, Err(Error::Conflict(_))));
    assert!(matches!(e.task_instruct(&t.id, "ancora", None), Err(Error::Conflict(_))));
}

#[tokio::test]
async fn an_instruction_given_while_the_task_runs_reaches_the_model_at_its_next_step() {
    let r = rig(vec![hold("c1"), text("Fatto, in tono formale.")]).await;
    let e = &r.e;
    let t = e.task_start(start("Scrivi una nota")).await.unwrap();
    r.entered.notified().await;
    let told = e.task_instruct(&t.id, "Sii breve", Some("voice")).unwrap();
    assert_eq!((told.instructions[0].source.as_str(), told.instructions[0].status.as_str()), ("voice", "pending"));
    r.release.notify_one();
    let done = wait_status(e, &t.id, "succeeded").await;
    assert_eq!(done.segments, 1, "it did not need a second run");
    let second = &r.backend.requests.lock().unwrap()[1];
    let last_user = user_texts(second).pop().unwrap();
    assert!(last_user.contains("Istruzione dell'utente") && last_user.contains("Sii breve"), "{last_user}");
    assert_eq!(done.instructions[0].status, "delivered");
    let stored = e.messages(&done.session_id).unwrap();
    assert!(stored.iter().any(|m| m.name.as_deref() == Some(aiw_agent::INSTRUCTION_NAME) && m.content.contains("Sii breve")));
    // The event is in the run's events too, so that a client sees it.
    let run = done.run_ids[0].clone();
    assert!(e
        .run_events(&run)
        .iter()
        .any(|ev| matches!(&ev.event.event, aiw_core::events::AgEvent::Custom { name, .. } if name == "aiw.instruction")));
    // Empty, too long, or an unknown source are refused.
    assert!(matches!(e.task_instruct(&t.id, "  ", None), Err(Error::Invalid(_))));
}

/// Answers the first call with a step and the second with the final answer, and gives an instruction in between,
/// when the run can no longer hear it (it is writing its last answer).
struct Late {
    engine: Engine,
    calls: AtomicUsize,
}

#[async_trait]
impl ChatBackend for Late {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec!["scripted".into()])
    }
    async fn chat(&self, _req: ChatRequest, cancel: tokio_util::sync::CancellationToken) -> Result<DeltaStream> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            let id = self.engine.tasks(None).unwrap()[0].id.clone();
            self.engine.task_instruct(&id, "ultima cosa", None).unwrap();
        }
        let _ = cancel;
        let turn = if n == 0 { text("Ho finito.") } else { text("Anche l'ultima cosa è fatta.") };
        Ok(futures::stream::iter(turn.into_iter().map(Ok)).boxed())
    }
}

#[tokio::test]
async fn an_instruction_that_arrives_as_the_task_ends_starts_one_more_segment() {
    let (e, _d, _w) = engine(vec![]).await;
    let late = Arc::new(Late { engine: e.clone(), calls: AtomicUsize::new(0) });
    e.register_native_runtime("llama-embedded", late.clone());
    let t = e.task_start(start("Fai una cosa")).await.unwrap();
    let done = wait_status(&e, &t.id, "succeeded").await;
    assert_eq!(done.segments, 2, "{done:?}");
    assert_eq!(done.instructions[0].status, "delivered");
    assert_eq!(done.result.as_deref(), Some("Anche l'ultima cosa è fatta."));
    assert_eq!(late.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn two_resumes_at_once_start_one_segment() {
    let r = rig(vec![hold("c1"), text("Fatto.")]).await;
    let e = &r.e;
    let t = e.task_start(start("Lavora")).await.unwrap();
    r.entered.notified().await;
    e.task_pause(&t.id).unwrap();
    r.release.notify_one();
    wait_status(e, &t.id, "paused").await;
    let (a, b) = tokio::join!(e.task_resume(&t.id, TaskResume::default()), e.task_resume(&t.id, TaskResume::default()));
    let (ok, err): (Vec<_>, Vec<_>) = [a, b].into_iter().partition(|x| x.is_ok());
    assert_eq!((ok.len(), err.len()), (1, 1), "exactly one wins");
    assert!(matches!(err[0], Err(Error::Conflict(_))));
    wait_status(e, &t.id, "succeeded").await;
    assert_eq!(e.task(&t.id).unwrap().segments, 2);
    assert_eq!(requests(&r), 2);
}

#[tokio::test]
async fn cancelling_stops_the_run_for_good() {
    let r = rig(vec![hold("c1"), text("non arriva")]).await;
    let e = &r.e;
    let t = e.task_start(start("Lavora")).await.unwrap();
    r.entered.notified().await;
    let c = e.task_cancel(&t.id).unwrap();
    assert_eq!(c.status, "cancelled");
    let run = c.run_ids[0].clone();
    super::tests::wait_finished(e, &run).await;
    let after = e.task(&t.id).unwrap();
    assert_eq!(after.status, "cancelled", "the run that ended did not turn it into a failure or a pause");
    assert_eq!(requests(&r), 1);
    assert!(matches!(e.task_resume(&t.id, TaskResume::default()).await, Err(Error::Conflict(_))));
}

// ------------------------------------------------------------------------------------ the rules

#[tokio::test]
async fn quiet_hours_hold_a_task_back_and_it_goes_on_by_itself_when_they_end() {
    let r = rig(vec![text("Fatto.")]).await;
    let e = &r.e;
    set(e, "proactive.quiet_hours", json!(quiet_now()));
    let t = e.task_start(start("Riordina")).await.unwrap();
    assert_eq!((t.status.as_str(), t.pause_reason.as_deref(), t.auto_resume, t.segments), ("paused", Some("quiet_hours"), true, 0));
    assert!(t.pause_text.as_deref().unwrap().contains("Orario di silenzio"));
    assert_eq!(requests(&r), 0, "nothing ran in the quiet hours");
    // Still quiet: the periodic pass leaves it alone. The person is told how to go on anyway.
    e.tasks_pass_at(Local::now());
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(e.task(&t.id).unwrap().status, "paused");
    let err = e.task_resume(&t.id, TaskResume::default()).await.unwrap_err().to_string();
    assert!(err.contains("Orario di silenzio") && err.contains("Riprendi comunque"), "{err}");
    // The quiet hours end: it goes on by itself, with the goal as its first message.
    set(e, "proactive.quiet_hours", json!(""));
    e.tasks_pass_at(Local::now());
    let done = wait_status(e, &t.id, "succeeded").await;
    assert_eq!(user_texts(&r.backend.requests.lock().unwrap()[0]).last().map(String::as_str), Some("Riordina"));
    assert_eq!(done.segments, 1);
}

#[tokio::test]
async fn quiet_hours_that_begin_during_a_task_stop_it_at_the_next_step_and_forcing_goes_on_anyway() {
    let r = rig(vec![hold("c1"), hold("c2"), text("Fatto.")]).await;
    let e = &r.e;
    let t = e.task_start(start("Lavora a lungo")).await.unwrap();
    r.entered.notified().await;
    set(e, "proactive.quiet_hours", json!(quiet_now()));
    r.release.notify_one();
    let paused = wait_status(e, &t.id, "paused").await;
    assert_eq!((paused.pause_reason.as_deref(), paused.auto_resume), (Some("quiet_hours"), true));
    assert_eq!(requests(&r), 1, "the step in progress finished, the next one did not start");
    // «Riprendi comunque»: it goes on in the quiet hours, and does not stop again for them.
    r.open.store(true, Ordering::SeqCst);
    e.task_resume(&t.id, TaskResume { force: true, extend: None }).await.unwrap();
    let done = wait_status(e, &t.id, "succeeded").await;
    assert!(!done.forced, "forcing ends with the task");
}

#[tokio::test]
async fn the_battery_stops_a_task_and_the_plug_lets_it_go_on() {
    let r = rig(vec![text("Fatto.")]).await;
    let e = &r.e;
    e.set_on_power(Some(false));
    let t = e.task_start(start("Riordina")).await.unwrap();
    assert_eq!((t.status.as_str(), t.pause_reason.as_deref(), t.auto_resume), ("paused", Some("battery"), true));
    e.set_on_power(Some(true));
    e.tasks_pass_at(Local::now());
    wait_status(e, &t.id, "succeeded").await;
}

#[tokio::test]
async fn the_days_time_is_shared_with_the_heartbeat_and_the_next_day_starts_again() {
    let r = rig(vec![text("Fatto.")]).await;
    let e = &r.e;
    set(e, "proactive.daily_budget_minutes", json!(1));
    // Tasks used the minute already: the status says so, and a new one waits for tomorrow.
    e.spend_add(Local::now(), 61.0);
    assert!(e.proactive_status().budget_used_minutes >= 1.0);
    let t = e.task_start(start("Riordina")).await.unwrap();
    assert_eq!((t.status.as_str(), t.pause_reason.as_deref(), t.auto_resume), ("paused", Some("daily_budget"), true));
    e.tasks_pass_at(Local::now());
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(e.task(&t.id).unwrap().status, "paused");
    // Tomorrow the rule no longer applies; today, with more time given, it does not either.
    let task = e.task(&t.id).unwrap();
    assert!(e.task_gate(&task, Local::now()).is_some());
    assert!(e.task_gate(&task, Local::now() + D::days(1)).is_none());
    set(e, "proactive.daily_budget_minutes", json!(100));
    e.tasks_pass_at(Local::now());
    wait_status(e, &t.id, "succeeded").await;
    // The time of the segment is counted for the day it ran.
    assert!(e.tasks_seconds_today(Local::now()) >= 61.0);
}

#[tokio::test]
async fn the_kill_switch_stops_every_task_until_the_level_is_set_again() {
    let r = rig(vec![hold("c1"), text("Fatto.")]).await;
    let e = &r.e;
    set(e, "proactive.level", json!("propose"));
    let t = e.task_start(start("Lavora")).await.unwrap();
    r.entered.notified().await;
    e.kill().await;
    let paused = wait_status(e, &t.id, "paused").await;
    assert_eq!((paused.pause_reason.as_deref(), paused.auto_resume), (Some("kill_switch"), false));
    e.tasks_pass_at(Local::now());
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(e.task(&t.id).unwrap().status, "paused");
    // Not even «anyway» goes past it.
    let err = e.task_resume(&t.id, TaskResume { force: true, extend: None }).await.unwrap_err().to_string();
    assert!(err.contains("emergenza"), "{err}");
    set(e, "proactive.level", json!("propose"));
    r.open.store(true, Ordering::SeqCst);
    e.task_resume(&t.id, TaskResume::default()).await.unwrap();
    wait_status(e, &t.id, "succeeded").await;
}

#[tokio::test]
async fn approving_a_proposal_makes_a_task_that_the_proposal_follows_to_the_end() {
    let r = rig(vec![text("Fatto."), text("Fatto anche questo.")]).await;
    let e = &r.e;
    let p = proposal(e, "Riordina i file");
    let approved = e.proposal_approve(&p.id, None).await.unwrap();
    let task = e.tasks(None).unwrap().remove(0);
    assert_eq!((task.origin.as_str(), task.proposal_id.as_deref()), ("proposal", Some(p.id.as_str())));
    assert_eq!(approved.session_id.as_deref(), Some(task.session_id.as_str()));
    assert!(approved.run_id.is_some(), "the first run is linked");
    wait_status(e, &task.id, "succeeded").await;
    assert_eq!(e.proposal(&p.id).unwrap().status, "done", "the proposal is done when its task is");
    // In the quiet hours the approval is taken, and the task waits (the proposal stays approved, with its session).
    set(e, "proactive.quiet_hours", json!(quiet_now()));
    let p2 = proposal(e, "Un'altra cosa");
    let a2 = e.proposal_approve(&p2.id, None).await.unwrap();
    let t2 = e.task_for_proposal(&p2.id).unwrap();
    assert_eq!((t2.status.as_str(), t2.pause_reason.as_deref()), ("paused", Some("quiet_hours")));
    assert_eq!((a2.status.as_str(), a2.run_id.as_deref()), ("approved", None));
    assert_eq!(a2.session_id.as_deref(), Some(t2.session_id.as_str()));
    assert_eq!(e.proposal(&p2.id).unwrap().status, "approved", "still approved while its task waits");
    set(e, "proactive.quiet_hours", json!(""));
    e.tasks_pass_at(Local::now());
    wait_status(e, &t2.id, "succeeded").await;
    assert_eq!(e.proposal(&p2.id).unwrap().status, "done");
}

// ------------------------------------------------------------------------------------ the budget

#[tokio::test]
async fn the_limits_of_a_task_hold_across_pauses_and_a_resume_cannot_get_around_them() {
    let r = rig(vec![hold("c1"), hold("c2"), hold("c3"), text("Fatto.")]).await;
    r.open.store(true, Ordering::SeqCst);
    let e = &r.e;
    let t = e
        .task_start(TaskStart { limits: Some(BudgetLimits { max_turns: Some(2), ..Default::default() }), ..start("Lavora") })
        .await
        .unwrap();
    let paused = wait_status(e, &t.id, "paused").await;
    assert_eq!((paused.pause_reason.as_deref(), paused.auto_resume), (Some("task_budget"), false));
    assert_eq!(paused.progress.turns, 2);
    assert_eq!(requests(&r), 2);
    // Resuming gives it no new allowance: it stays stopped until the person adds some.
    let err = e.task_resume(&t.id, TaskResume::default()).await.unwrap_err().to_string();
    assert!(err.contains("budget"), "{err}");
    e.tasks_pass_at(Local::now());
    assert_eq!(requests(&r), 2);
    // Only the limits the task has are raised: asking for tokens does not give it a token limit.
    let more = BudgetLimits { max_turns: Some(2), max_tokens: Some(5000), ..Default::default() };
    e.task_resume(&t.id, TaskResume { force: false, extend: Some(more) }).await.unwrap();
    let done = wait_status(e, &t.id, "succeeded").await;
    assert_eq!((done.progress.turns, done.limits.max_turns, done.limits.max_tokens), (4, Some(4), None));
    assert_eq!(requests(&r), 4);
}

#[tokio::test]
async fn working_time_counts_against_the_limit_but_the_time_spent_waiting_for_a_confirmation_does_not() {
    // Working: a step that takes longer than the whole allowance stops the task at the next turn.
    let r = rig(vec![hold("c1"), hold("c2"), text("Fatto.")]).await;
    let e = &r.e;
    let one_second = Some(BudgetLimits { max_seconds: Some(1), ..Default::default() });
    let t = e.task_start(TaskStart { limits: one_second.clone(), ..start("Lavora") }).await.unwrap();
    r.entered.notified().await;
    tokio::time::sleep(std::time::Duration::from_millis(1300)).await;
    r.release.notify_one();
    let paused = wait_status(e, &t.id, "paused").await;
    assert_eq!((paused.pause_reason.as_deref(), paused.auto_resume), (Some("task_budget"), false), "{paused:?}");
    assert!(paused.progress.seconds >= 1);
    assert_eq!(requests(&r), 1, "the second step did not start");

    // Waiting: the same allowance, but the time goes in waiting for the person: the task is still within it.
    let r =
        rig(vec![ScriptedBackend::tool_turn("w1", "write_file", json!({"path": "nota.md", "content": "# Nota\n"})), text("Fatto.")]).await;
    let e = &r.e;
    let t = e.task_start(TaskStart { limits: one_second, ..start("Crea una nota") }).await.unwrap();
    wait_for("a confirmation is asked", || !e.approvals().is_empty()).await;
    tokio::time::sleep(std::time::Duration::from_millis(2300)).await;
    assert!(e.tasks_seconds_today(Local::now()) < 2.0, "waiting does not eat the day's time either");
    let ask = e.approvals().remove(0);
    e.respond_approval(&ask.id, true, false).unwrap();
    let done = wait_status(e, &t.id, "succeeded").await;
    assert!(r.ws.path().join("nota.md").exists());
    assert!(done.progress.seconds < 2, "{:?}", done.progress);
}

// ------------------------------------------------------------------------------------ approvals

#[tokio::test]
async fn a_task_never_writes_without_a_confirmation_even_when_its_agent_is_freer() {
    let r =
        rig(vec![ScriptedBackend::tool_turn("w1", "write_file", json!({"path": "nota.md", "content": "# Nota\n"})), text("Fatto.")]).await;
    let e = &r.e;
    // The agent's own mode is freer than «ask».
    assert_eq!(e.agent("coder").unwrap().mode, "auto-edit");
    let t = e.task_start(start("Crea una nota")).await.unwrap();
    wait_for("a confirmation is asked", || !e.approvals().is_empty()).await;
    let seen = e.task(&t.id).unwrap();
    assert_eq!(seen.waiting_approval.as_ref().map(|a| a.tool.as_str()), Some("write_file"), "the task says what it waits for");
    assert!(!r.ws.path().join("nota.md").exists());
    // Refused: nothing is written, and the task goes on to its answer.
    let ask = e.approvals().remove(0);
    e.respond_approval(&ask.id, false, false).unwrap();
    wait_status(e, &t.id, "succeeded").await;
    assert!(!r.ws.path().join("nota.md").exists());
}

#[tokio::test]
async fn nobody_answering_a_confirmation_means_no_write() {
    let r = rig(vec![
        ScriptedBackend::tool_turn("w1", "write_file", json!({"path": "nota.md", "content": "# Nota\n"})),
        text("Non l'ho scritta."),
    ])
    .await;
    let e = &r.e;
    set(e, "approvals.timeout_s", json!(1));
    let t = e.task_start(start("Crea una nota")).await.unwrap();
    wait_status(e, &t.id, "succeeded").await;
    assert!(!r.ws.path().join("nota.md").exists(), "an unanswered confirmation is a refusal");
}

#[tokio::test]
async fn pausing_a_task_that_waits_for_a_confirmation_stops_it_at_once_and_writes_nothing() {
    let r = rig(vec![
        ScriptedBackend::tool_turn("w1", "write_file", json!({"path": "nota.md", "content": "# Nota\n"})),
        text("Rifatto senza scrivere."),
    ])
    .await;
    let e = &r.e;
    let t = e.task_start(start("Crea una nota")).await.unwrap();
    wait_for("a confirmation is asked", || !e.approvals().is_empty()).await;
    e.task_pause(&t.id).unwrap();
    let paused = wait_status(e, &t.id, "paused").await;
    assert_eq!(paused.pause_reason.as_deref(), Some("user"));
    assert!(e.approvals().is_empty(), "the request is withdrawn with the run");
    assert!(!r.ws.path().join("nota.md").exists());
    // The cut step is sealed in the session, so that the next segment knows it did not happen.
    let msgs = e.messages(&paused.session_id).unwrap();
    assert!(msgs.iter().any(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some("w1")), "{msgs:?}");
    e.task_resume(&t.id, TaskResume::default()).await.unwrap();
    wait_status(e, &t.id, "succeeded").await;
    assert!(!r.ws.path().join("nota.md").exists());
}

#[tokio::test]
async fn a_task_has_no_management_tools() {
    // The run has no origin, so a management tool refuses in it (the same rule as for every run nobody asked in a chat).
    let r = rig(vec![ScriptedBackend::tool_turn("m1", "aiw_inspect", json!({"what": "settings"})), text("Fatto.")]).await;
    let e = &r.e;
    let t = e.task_start(TaskStart { agent_id: Some("assistant".into()), ..start("Guarda le impostazioni") }).await.unwrap();
    wait_status(e, &t.id, "succeeded").await;
    let msgs = e.messages(&t.session_id).unwrap();
    let out = msgs.iter().find(|m| m.role == "tool").map(|m| m.content.clone()).unwrap_or_default();
    assert!(out.contains("Negato"), "{out}");
}

// ------------------------------------------------------------------------------------ restart

fn crash_state(e: &Engine, instruction_delivered: bool) -> BackgroundTask {
    let session = e.create_session("coder", Some("Lavoro: prova".into())).unwrap();
    let now = now_rfc3339();
    let mut t = BackgroundTask {
        id: new_id("tsk"),
        title: "Prova".into(),
        instruction: "Fai la prova".into(),
        agent_id: "coder".into(),
        session_id: session.id,
        origin: "user".into(),
        status: "running".into(),
        segments: 1,
        current_run_id: Some("run_perso".into()),
        run_ids: vec!["run_perso".into()],
        limits: BudgetLimits { max_seconds: Some(600), max_turns: Some(50), max_tokens: None },
        created_at: now.clone(),
        updated_at: now.clone(),
        segment_started_at: Some(now.clone()),
        ..Default::default()
    };
    t.progress.turns = 3;
    t.instructions.push(aiw_core::api::TaskInstruction {
        id: "tin_1".into(),
        text: "Controlla due volte".into(),
        source: "user".into(),
        created_at: now.clone(),
        status: if instruction_delivered { "delivered".into() } else { "pending".into() },
        delivered_at: instruction_delivered.then(|| now.clone()),
        delivered_in: instruction_delivered.then_some(1),
        note: None,
    });
    e.0.store.put_doc("bg_task", &t.id, &t).unwrap();
    t
}

async fn engine_at(data: &std::path::Path, ws: &std::path::Path, turns: Vec<Vec<ChatDelta>>) -> (Engine, ScriptedBackend) {
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.to_path_buf()),
        workspace_root: Some(ws.to_path_buf()),
        mode: "embedded".into(),
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap();
    let backend = ScriptedBackend::new(turns);
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    e.save_profile(InferenceProfile {
        id: "default".into(),
        name: "Test".into(),
        description: None,
        runtime_id: "llama-embedded".into(),
        model: "scripted".into(),
        params: InferenceParams { context: Some(8192), ..Default::default() },
    })
    .unwrap();
    (e, backend)
}

#[tokio::test]
async fn after_a_restart_a_task_that_was_running_is_paused_nothing_starts_and_nothing_is_lost_or_doubled() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let (first, _) = engine_at(data.path(), ws.path(), vec![]).await;
    // What a crash leaves: the task says running, its instruction was handed to a segment that is lost.
    let crashed = crash_state(&first, true);
    let queued = crash_state(&first, false);
    drop(first);

    let (e, backend) = engine_at(data.path(), ws.path(), vec![text("Fatto due volte.")]).await;
    let t = e.task(&crashed.id).unwrap();
    assert_eq!((t.status.as_str(), t.pause_reason.as_deref(), t.auto_resume), ("paused", Some("restart"), false));
    assert!(t.pause_text.as_deref().unwrap().contains("riavvio"));
    assert_eq!(t.progress.turns, 3, "what the task had done is kept");
    assert_eq!(t.instructions[0].status, "pending", "the instruction that reached the lost segment is delivered again");
    assert!(e.0.runs.lock().unwrap().is_empty(), "nothing started by itself");
    assert_eq!(e.task(&queued.id).unwrap().instructions[0].status, "pending");
    e.tasks_pass_at(Local::now());
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(e.0.runs.lock().unwrap().is_empty(), "the periodic pass does not resume it either");
    assert_eq!(backend.requests.lock().unwrap().len(), 0);

    // Resuming starts one segment, with the instruction; a second resume is refused.
    let (a, b) = tokio::join!(e.task_resume(&crashed.id, TaskResume::default()), e.task_resume(&crashed.id, TaskResume::default()));
    assert_eq!([a.is_ok(), b.is_ok()].iter().filter(|x| **x).count(), 1);
    let done = wait_status(&e, &crashed.id, "succeeded").await;
    assert_eq!(done.segments, 2);
    assert_eq!(backend.requests.lock().unwrap().len(), 1);
    let users = user_texts(&backend.requests.lock().unwrap()[0]);
    assert!(users.iter().any(|u| u.contains("Controlla due volte")), "{users:?}");
    assert_eq!(done.instructions[0].status, "delivered");
    // The history of the task survived with it.
    assert!(done.notes.iter().any(|n| n.text.contains("riavvio")));
}

#[tokio::test]
async fn an_instruction_the_session_already_has_is_not_delivered_again_after_a_restart() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let (first, _) = engine_at(data.path(), ws.path(), vec![]).await;
    let t = crash_state(&first, true);
    // The segment got to save the message that carries the instruction before the crash.
    first
        .0
        .store
        .append_messages(
            &t.session_id,
            &[aiw_core::api::ChatMessage {
                id: new_id("msg"),
                role: "user".into(),
                content: "[Istruzione dell'utente, arrivata mentre lavoravi] Controlla due volte".into(),
                reasoning: None,
                tool_calls: None,
                tool_call_id: None,
                name: Some(aiw_agent::INSTRUCTION_NAME.into()),
                attachments: None,
                created_at: now_rfc3339(),
            }],
        )
        .unwrap();
    drop(first);
    let (e, _) = engine_at(data.path(), ws.path(), vec![]).await;
    assert_eq!(e.task(&t.id).unwrap().instructions[0].status, "delivered");
}

#[tokio::test]
async fn a_paused_task_survives_a_restart_with_its_instructions_and_resumes_from_its_session() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let (first, backend1) = engine_at(data.path(), ws.path(), vec![]).await;
    let (entered, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let open = Arc::new(AtomicBool::new(false));
    first.tool_registry().register(Arc::new(Hold {
        entered: entered.clone(),
        release: release.clone(),
        open,
        calls: Arc::new(AtomicUsize::new(0)),
    }));
    let script = vec![hold("c1")];
    let backend = ScriptedBackend::new(script);
    first.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    let _ = backend1;
    let t = first.task_start(start("Lavoro lungo")).await.unwrap();
    entered.notified().await;
    first.task_pause(&t.id).unwrap();
    first.task_instruct(&t.id, "Poi controlla", None).unwrap();
    release.notify_one();
    let paused = wait_status(&first, &t.id, "paused").await;
    let session = paused.session_id.clone();
    let stored_before = first.messages(&session).unwrap().len();
    drop(first);

    let (e, backend2) = engine_at(data.path(), ws.path(), vec![text("Fatto.")]).await;
    let again = e.task(&t.id).unwrap();
    assert_eq!(
        (again.status.as_str(), again.pause_reason.as_deref()),
        ("paused", Some("user")),
        "a pause the person asked for is not turned into a restart"
    );
    assert_eq!(again.instructions.len(), 1);
    e.task_resume(&t.id, TaskResume::default()).await.unwrap();
    let done = wait_status(&e, &t.id, "succeeded").await;
    assert_eq!(done.segments, 2);
    // The model of the new process saw the whole story: the goal, the step already done, the instruction.
    let req = &backend2.requests.lock().unwrap()[0];
    let roles: Vec<_> = req.messages.iter().map(|m| m.role.as_str()).collect();
    assert!(roles.contains(&"tool"), "{roles:?}");
    assert!(user_texts(req).iter().any(|u| u == "Lavoro lungo"));
    assert!(user_texts(req).iter().any(|u| u.contains("Poi controlla")));
    assert!(e.messages(&session).unwrap().len() > stored_before);
}

// ------------------------------------------------------------------------------------ voice (without a real voice)

/// A speech recognizer that answers with a fixed sentence: the real one (the system's, or whisper.cpp) needs a
/// microphone and a speaker of flesh and blood, which these tests do not have.
struct FakeStt;

impl crate::voice::NativeStt for FakeStt {
    fn transcribe(&self, wav: &[u8], language: Option<&str>) -> Result<String> {
        if wav.len() < 44 {
            return Err(Error::Invalid("audio vuoto".into()));
        }
        assert_eq!(language, Some("it"));
        Ok("Aggiungi anche il totale in fondo".into())
    }
}

#[tokio::test]
async fn a_dictated_instruction_is_transcribed_first_and_reaches_the_task_like_a_typed_one() {
    let r = rig(vec![hold("c1"), text("Fatto, con il totale.")]).await;
    let e = &r.e;
    e.register_native_stt(Arc::new(FakeStt));
    let t = e.task_start(start("Prepara il rendiconto")).await.unwrap();
    r.entered.notified().await;
    // The app records, the engine transcribes, the person reads the text and sends it as an instruction «voice».
    let wav = vec![0u8; 4096];
    let said = e.transcribe(&wav, "audio/wav", Some("it")).await.unwrap();
    assert_eq!(said.text, "Aggiungi anche il totale in fondo");
    // A recording that gives nothing is an error before any instruction exists: the task is untouched.
    assert!(e.transcribe(&[], "audio/wav", Some("it")).await.is_err());
    assert!(e.task(&t.id).unwrap().instructions.is_empty());
    e.task_instruct(&t.id, &said.text, Some("voice")).unwrap();
    r.release.notify_one();
    let done = wait_status(e, &t.id, "succeeded").await;
    assert_eq!((done.instructions[0].source.as_str(), done.instructions[0].status.as_str()), ("voice", "delivered"));
    let second = &r.backend.requests.lock().unwrap()[1];
    assert!(user_texts(second).iter().any(|u| u.contains("Aggiungi anche il totale in fondo")));
}

// ------------------------------------------------------------------------------------ audit

#[tokio::test]
async fn the_audit_names_the_task_and_never_the_text_of_an_instruction() {
    let r = rig(vec![hold("c1"), text("Fatto.")]).await;
    let e = &r.e;
    let secret = "hunter2-SEGRETISSIMO";
    let t = e.task_start(start(&format!("Entra con la password {secret} e riordina"))).await.unwrap();
    r.entered.notified().await;
    e.task_instruct(&t.id, &format!("Riprova con {secret}"), None).unwrap();
    e.task_pause(&t.id).unwrap();
    r.release.notify_one();
    wait_status(e, &t.id, "paused").await;
    e.task_resume(&t.id, TaskResume::default()).await.unwrap();
    wait_status(e, &t.id, "succeeded").await;
    let audit = e.audit();
    assert!(
        !audit.iter().any(|a| a.detail.contains(secret) || a.tool.as_deref().is_some_and(|x| x.contains(secret))),
        "no text of an instruction in the audit"
    );
    for action in ["task.start", "task.instruct", "task.pause", "task.resume"] {
        let entry = audit.iter().find(|a| a.action == action).unwrap_or_else(|| panic!("{action} is audited"));
        assert!(entry.detail.starts_with(&t.id), "{action}: {}", entry.detail);
    }
}

// ------------------------------------------------------------------------------------ what the review found

/// Asks for a pause (and leaves an instruction) while the model writes the last answer of the segment.
struct LatePause {
    engine: Engine,
    calls: AtomicUsize,
}

#[async_trait]
impl ChatBackend for LatePause {
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec!["scripted".into()])
    }
    async fn chat(&self, _req: ChatRequest, _cancel: tokio_util::sync::CancellationToken) -> Result<DeltaStream> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            let id = self.engine.tasks(None).unwrap()[0].id.clone();
            self.engine.task_instruct(&id, "una cosa in più", None).unwrap();
            self.engine.task_pause(&id).unwrap();
        }
        let turn = text(if n == 0 { "Ho finito." } else { "Fatto anche questo." });
        Ok(futures::stream::iter(turn.into_iter().map(Ok)).boxed())
    }
}

#[tokio::test]
async fn a_pause_that_comes_while_the_last_answer_is_written_is_not_lost_to_the_next_segment() {
    let (e, _d, _w) = engine(vec![]).await;
    let backend = Arc::new(LatePause { engine: e.clone(), calls: AtomicUsize::new(0) });
    e.register_native_runtime("llama-embedded", backend.clone());
    let t = e.task_start(start("Fai una cosa")).await.unwrap();
    // The answer ends the step and an instruction is waiting (a new step would normally read it), but the person asked to pause.
    let paused = wait_status(&e, &t.id, "paused").await;
    assert_eq!((paused.segments, paused.pause_reason.as_deref(), paused.auto_resume), (1, Some("user"), false), "{paused:?}");
    assert_eq!(paused.instructions[0].status, "pending", "it waits for the resume");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1, "no second segment started");
    e.task_resume(&t.id, TaskResume::default()).await.unwrap();
    let done = wait_status(&e, &t.id, "succeeded").await;
    assert_eq!((done.segments, done.instructions[0].status.as_str()), (2, "delivered"));
}

#[tokio::test]
async fn a_pause_the_person_asks_for_wins_over_a_rule_that_was_already_stopping_the_task() {
    let r = rig(vec![hold("c1"), text("Fatto.")]).await;
    let e = &r.e;
    let t = e.task_start(start("Lavora")).await.unwrap();
    r.entered.notified().await;
    set(e, "proactive.quiet_hours", json!(quiet_now()));
    // Whatever stops it, the person asked it to stay stopped: the end of the quiet hours does not undo that.
    e.task_pause(&t.id).unwrap();
    r.release.notify_one();
    let paused = wait_status(e, &t.id, "paused").await;
    assert_eq!((paused.pause_reason.as_deref(), paused.auto_resume), (Some("user"), false));
    set(e, "proactive.quiet_hours", json!(""));
    e.tasks_pass_at(Local::now());
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(e.task(&t.id).unwrap().status, "paused");
}

#[tokio::test]
async fn a_task_does_not_follow_a_handoff_and_says_so_instead_of_finishing() {
    let r = rig(vec![ScriptedBackend::tool_turn("h1", "handoff", json!({"agent": "planner", "summary": "continua tu"}))]).await;
    let e = &r.e;
    let t = e.task_start(start("Fai una cosa")).await.unwrap();
    let failed = wait_status(e, &t.id, "failed").await;
    assert!(failed.error.as_deref().unwrap().contains("passare la conversazione"), "{failed:?}");
    assert_eq!(e.session(&failed.session_id).unwrap().agent_id, "coder", "the session stays with the agent of the task");
}

#[tokio::test]
async fn allowing_for_the_session_never_covers_a_task() {
    let r = rig(vec![
        ScriptedBackend::tool_turn("w1", "write_file", json!({"path": "uno.md", "content": "1\n"})),
        ScriptedBackend::tool_turn("w2", "write_file", json!({"path": "due.md", "content": "2\n"})),
        text("Fatto."),
    ])
    .await;
    let e = &r.e;
    let t = e.task_start(start("Scrivi due note")).await.unwrap();
    wait_for("the first confirmation", || !e.approvals().is_empty()).await;
    // «Consenti per la sessione» on the first one…
    let ask = e.approvals().remove(0);
    e.respond_approval(&ask.id, true, true).unwrap();
    // …does not cover the second: it is asked on its own.
    wait_for("the second confirmation", || e.approvals().iter().any(|a| a.args["path"] == "due.md")).await;
    assert!(r.ws.path().join("uno.md").exists() && !r.ws.path().join("due.md").exists());
    let second = e.approvals().remove(0);
    e.respond_approval(&second.id, false, false).unwrap();
    wait_status(e, &t.id, "succeeded").await;
    assert!(!r.ws.path().join("due.md").exists());
    let remembered = e.0.session_allow.lock().unwrap().get(&t.session_id).is_some_and(|s| s.lock().unwrap().contains("write_file"));
    assert!(!remembered, "nothing is remembered for the session of a task");
}

#[tokio::test]
async fn with_no_time_at_all_per_day_a_task_does_not_wait_for_a_tomorrow_that_never_comes() {
    let r = rig(vec![text("Fatto.")]).await;
    let e = &r.e;
    set(e, "proactive.daily_budget_minutes", json!(0));
    let t = e.task_start(start("Riordina")).await.unwrap();
    assert_eq!((t.status.as_str(), t.pause_reason.as_deref(), t.auto_resume), ("paused", Some("daily_budget"), false));
    assert!(t.pause_text.as_deref().unwrap().contains("0 minuti"), "{t:?}");
    e.tasks_pass_at(Local::now() + D::days(1));
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(e.task(&t.id).unwrap().status, "paused");
    e.task_resume(&t.id, TaskResume { force: true, extend: None }).await.unwrap();
    wait_status(e, &t.id, "succeeded").await;
}

#[tokio::test]
async fn deleting_the_session_of_a_task_stops_it_and_forgets_it() {
    let r = rig(vec![hold("c1"), text("non arriva")]).await;
    let e = &r.e;
    let t = e.task_start(start("Lavora con la chiave hunter2-SEGRETISSIMO")).await.unwrap();
    r.entered.notified().await;
    let run = e.task(&t.id).unwrap().run_ids[0].clone();
    e.delete_session(&t.session_id).unwrap();
    assert!(matches!(e.task(&t.id), Err(Error::NotFound(_))), "the record with the text of the instruction goes with the conversation");
    super::tests::wait_finished(e, &run).await;
    assert_eq!(requests(&r), 1, "the run was stopped");
}

#[tokio::test]
async fn a_short_instruction_is_not_taken_for_delivered_because_a_tool_said_the_same_word() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let (first, _) = engine_at(data.path(), ws.path(), vec![]).await;
    let t = crash_state(&first, true);
    let mut doc = first.task(&t.id).unwrap();
    doc.instructions[0].text = "ok".into();
    first.0.store.put_doc("bg_task", &doc.id, &doc).unwrap();
    // The session holds a tool result that says «ok» and an answer of the agent that says it too: neither is the instruction.
    let msg = |role: &str, content: &str| aiw_core::api::ChatMessage {
        id: new_id("msg"),
        role: role.into(),
        content: content.into(),
        reasoning: None,
        tool_calls: None,
        tool_call_id: (role == "tool").then(|| "c1".to_string()),
        name: None,
        attachments: None,
        created_at: now_rfc3339(),
    };
    first.0.store.append_messages(&t.session_id, &[msg("tool", "ok"), msg("assistant", "va bene, ok")]).unwrap();
    drop(first);
    let (e, _) = engine_at(data.path(), ws.path(), vec![]).await;
    assert_eq!(e.task(&t.id).unwrap().instructions[0].status, "pending");
}

#[tokio::test]
async fn a_one_shot_process_next_to_the_app_leaves_its_running_tasks_alone() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let (app, _) = engine_at(data.path(), ws.path(), vec![]).await;
    let live = crash_state(&app, true);
    // A command of the CLI opens the same data while the app is working: it must not pause what the app is running.
    let cli = Engine::new(EngineOptions {
        data_dir: Some(data.path().to_path_buf()),
        workspace_root: Some(ws.path().to_path_buf()),
        mode: "embedded".into(),
        offline: true,
        no_tasks: true,
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(app.task(&live.id).unwrap().status, "running", "not turned into «interrotto dal riavvio»");
    // Nor does it resume what is paused by a rule.
    let held = app.task_mut_for_tests(&live.id, |t| {
        t.status = "paused".into();
        t.auto_resume = true;
        t.pause_reason = Some("quiet_hours".into());
    });
    assert_eq!(held.status, "paused");
    cli.tasks_pass_at(Local::now());
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(cli.task(&live.id).unwrap().status, "paused");
    assert!(cli.0.runs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_agents_time_setting_does_not_end_a_task_that_only_waited_for_a_confirmation() {
    // `agents.budget.max_seconds` is the clock of every run; a task's steps run with that clock off, so the minutes a person
    // takes to answer a confirmation do not end the step.
    let r =
        rig(vec![ScriptedBackend::tool_turn("w1", "write_file", json!({"path": "nota.md", "content": "# Nota\n"})), text("Fatto.")]).await;
    let e = &r.e;
    set(e, "agents.budget.max_seconds", json!(1));
    let t = e.task_start(start("Crea una nota")).await.unwrap();
    wait_for("a confirmation is asked", || !e.approvals().is_empty()).await;
    tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
    let ask = e.approvals().remove(0);
    e.respond_approval(&ask.id, true, false).unwrap();
    let done = wait_status(e, &t.id, "succeeded").await;
    assert!(r.ws.path().join("nota.md").exists(), "{done:?}");
}
