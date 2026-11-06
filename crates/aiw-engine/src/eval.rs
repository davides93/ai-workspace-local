//! Evaluation: suites as documents, reports in the database, and the engine as the [`EvalHost`] that
//! runs a task with an agent in a throwaway working folder.

use crate::host::EngineHost;
use crate::{Engine, RunHandle};
use aiw_agent::AgentRunner;
use aiw_core::events::AgEvent;
use aiw_core::{ids::new_id, Error, Result};
use aiw_eval::{EvalEvent, EvalHost, EvalReport, RunOptions, Suite, TaskRequest, TaskRun};
use aiw_tools::SandboxMode;
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const SUITE_KIND: &str = "eval_suite";
/// Most reports kept in the list the API and the CLI show.
const MAX_REPORTS: usize = 200;
/// Time a run gets to wind up after its time is over, before it is stopped.
const STOP_GRACE: Duration = Duration::from_secs(30);

/// How to run a suite (all optional).
#[derive(Debug, Clone, Default)]
pub struct EvalRequest {
    pub agent: Option<String>,
    pub profile: Option<String>,
    /// System prompt that replaces the agent's own in every task (see [`Engine::optimize_prompt`]).
    pub system_prompt: Option<String>,
    /// Times each task is run (default 1).
    pub repeat: Option<u32>,
    /// Keep the working folders of the tasks.
    pub keep: bool,
    /// Id of the report, when it was made beforehand (see [`Engine::start_eval`]).
    pub report_id: Option<String>,
}

impl Engine {
    // ------------------------------------------------------------------ suites
    pub fn eval_suites(&self) -> Result<Vec<Suite>> {
        self.0.store.list_docs(SUITE_KIND)
    }
    pub fn eval_suite(&self, id: &str) -> Result<Suite> {
        self.0.store.get_doc(SUITE_KIND, id)?.ok_or_else(|| Error::NotFound(format!("suite di valutazione {id} non trovata")))
    }
    /// Stores a suite after checking it, including that the agents and profiles it names exist.
    pub fn save_eval_suite(&self, suite: Suite) -> Result<Suite> {
        suite.validate()?;
        self.check_suite_refs(&suite)?;
        self.0.store.put_doc(SUITE_KIND, &suite.id, &suite)?;
        Ok(suite)
    }
    /// [`Engine::save_eval_suite`] for suite text (YAML or JSON), as the editors send it.
    pub fn save_eval_suite_text(&self, text: &str) -> Result<Suite> {
        self.save_eval_suite(Suite::parse(text)?)
    }
    pub fn delete_eval_suite(&self, id: &str) -> Result<()> {
        if !self.0.store.delete_doc(SUITE_KIND, id)? {
            return Err(Error::NotFound(format!("suite di valutazione {id} non trovata")));
        }
        Ok(())
    }

    fn check_suite_refs(&self, suite: &Suite) -> Result<()> {
        let agents = suite.agent.iter().chain(suite.tasks.iter().filter_map(|t| t.agent.as_ref()));
        for a in agents {
            self.agent(a).map_err(|_| Error::Invalid(format!("l'agente `{a}` della suite non esiste")))?;
        }
        let profiles = suite.profile.iter().chain(suite.tasks.iter().filter_map(|t| t.profile.as_ref()));
        for p in profiles {
            self.profile(p).map_err(|_| Error::Invalid(format!("il profilo di inferenza `{p}` della suite non esiste")))?;
        }
        let judges = suite.tasks.iter().flat_map(|t| &t.graders).filter_map(|g| match g {
            aiw_eval::GraderSpec::Judge { profile, .. } => profile.as_ref(),
            _ => None,
        });
        for p in judges {
            self.profile(p).map_err(|_| Error::Invalid(format!("il profilo di inferenza `{p}` del giudice non esiste")))?;
        }
        Ok(())
    }

    /// The suite of `reference`: a file when one exists at that path, else a stored suite by id.
    pub fn resolve_eval_suite(&self, reference: &str) -> Result<Suite> {
        let path = std::path::Path::new(reference);
        if path.is_file() {
            return Suite::load_file(path);
        }
        self.eval_suite(reference)
    }

    // ------------------------------------------------------------------ reports
    /// The most recent reports first.
    pub fn eval_reports(&self) -> Result<Vec<EvalReport>> {
        self.0.store.eval_reports(MAX_REPORTS)
    }
    pub fn eval_report(&self, id: &str) -> Result<EvalReport> {
        self.0.store.eval_report(id)?.ok_or_else(|| Error::NotFound(format!("report di valutazione {id} non trovato")))
    }
    /// Reports still `running` when the engine starts belong to a run that was cut short.
    pub(crate) fn fail_interrupted_eval_reports(&self) -> Result<()> {
        for mut r in self.0.store.eval_reports::<EvalReport>(MAX_REPORTS)?.into_iter().filter(|r| r.status == "running") {
            r.status = "failed".into();
            r.error = Some("interrotta: l'applicazione è stata chiusa durante la valutazione".into());
            r.finished_at = Some(aiw_core::now_rfc3339());
            self.save_eval_report(&r);
        }
        Ok(())
    }

    /// Starts `suite` in the background and returns its report as it is before the first task
    /// (`running`): the run goes on and keeps the stored report up to date, so callers poll
    /// [`Engine::eval_report`].
    pub fn start_eval(&self, suite: Suite, req: EvalRequest) -> Result<EvalReport> {
        suite.validate()?;
        self.check_suite_refs(&suite)?;
        if let Some(a) = &req.agent {
            self.agent(a)?;
        }
        if let Some(p) = &req.profile {
            self.profile(p)?;
        }
        let repeat = req.repeat.unwrap_or(1);
        if repeat == 0 || repeat > aiw_eval::MAX_REPEAT {
            return Err(Error::Invalid(format!("repeat deve essere tra 1 e {}", aiw_eval::MAX_REPEAT)));
        }
        let id = new_id("evr");
        let initial = EvalReport {
            id: id.clone(),
            suite_id: suite.id.clone(),
            suite_name: suite.name.clone(),
            status: "running".into(),
            started_at: aiw_core::now_rfc3339(),
            finished_at: None,
            agent: req.agent.clone(),
            profile: req.profile.clone(),
            repeat,
            tasks: vec![],
            summary: Default::default(),
            error: None,
        };
        self.save_eval_report(&initial);
        let engine = self.clone();
        let req = EvalRequest { report_id: Some(id.clone()), ..req };
        tokio::spawn(async move {
            if let Err(e) = engine.run_eval(&suite, &req, &mut |_| {}).await {
                if let Ok(mut r) = engine.eval_report(&id) {
                    r.status = "failed".into();
                    r.error = Some(e.to_string());
                    r.finished_at = Some(aiw_core::now_rfc3339());
                    engine.save_eval_report(&r);
                }
            }
        });
        Ok(initial)
    }

    /// The report of `reference`: a JSON file when one exists at that path, else a stored report by id.
    pub fn resolve_eval_report(&self, reference: &str) -> Result<EvalReport> {
        let path = std::path::Path::new(reference);
        if path.is_file() {
            let text = std::fs::read_to_string(path)?;
            return serde_json::from_str(&text).map_err(|e| Error::Invalid(format!("{reference} non è un report di valutazione: {e}")));
        }
        self.eval_report(reference)
    }
    /// Compares two reports (stored ids or files): the deltas are `b - a`.
    pub fn compare_eval_runs(&self, a: &str, b: &str) -> Result<aiw_eval::Comparison> {
        Ok(aiw_eval::compare(&self.resolve_eval_report(a)?, &self.resolve_eval_report(b)?))
    }
    fn save_eval_report(&self, r: &EvalReport) {
        if let Err(e) = self.0.store.save_eval_report(&r.id, &r.suite_id, &r.started_at, r) {
            tracing::warn!("report di valutazione {}: {e}", r.id);
        }
    }

    // ------------------------------------------------------------------ run
    /// Runs `suite` to the end. The report is stored when the run starts and after every task, so it can be
    /// read while it is still going; `observer` hears about the same moments.
    pub async fn run_eval(&self, suite: &Suite, req: &EvalRequest, observer: &mut (dyn FnMut(EvalEvent<'_>) + Send)) -> Result<EvalReport> {
        self.run_eval_with_cancel(suite, req, observer, CancellationToken::new()).await
    }

    pub(crate) async fn run_eval_with_cancel(
        &self,
        suite: &Suite,
        req: &EvalRequest,
        observer: &mut (dyn FnMut(EvalEvent<'_>) + Send),
        cancel: CancellationToken,
    ) -> Result<EvalReport> {
        suite.validate()?;
        self.check_suite_refs(suite)?;
        for name in req.agent.iter() {
            self.agent(name)?;
        }
        if let Some(p) = &req.profile {
            self.profile(p)?;
        }
        let mut opts = RunOptions::new(self.0.dirs.eval_work());
        opts.agent = req.agent.clone();
        opts.profile = req.profile.clone();
        opts.system_prompt = req.system_prompt.clone();
        opts.repeat = req.repeat.unwrap_or(1);
        opts.keep = req.keep;
        opts.report_id = req.report_id.clone();
        opts.cancel = cancel;
        self.audit_user("eval.run", &format!("{} ({} task × {})", suite.name, suite.tasks.len(), opts.repeat));
        let engine = self.clone();
        let result = aiw_eval::run_suite(self, suite, &opts, &mut |ev| {
            match &ev {
                EvalEvent::Started(r) => engine.save_eval_report(r),
                EvalEvent::TaskFinished { report, .. } => engine.save_eval_report(report),
                EvalEvent::TaskStarted { .. } => {}
            }
            observer(ev);
        })
        .await;
        if let Ok(report) = &result {
            self.save_eval_report(report);
            if report.status == "completed" {
                self.rewards_from_report(suite, req.agent.as_deref(), req.profile.as_deref(), report);
            }
        }
        result
    }
}

#[async_trait]
impl EvalHost for Engine {
    /// One answer of the model of `profile` (the fallback profile when `None`), no tools.
    async fn complete(&self, profile: Option<&str>, prompt: &str) -> Result<String> {
        use aiw_workflow::WorkflowHost;
        let id = match profile {
            Some(p) => p.to_string(),
            None => self.fallback_profile()?.id,
        };
        EngineHost::new(self.clone(), None, None).complete("", &id, prompt).await
    }

    /// The agent works in `req.workdir` as in a workspace of its own: policy mode `auto`, sandbox
    /// `workspace-write`, no confirmations (a run that would need one has it refused), the time of the
    /// task as the budget. The run is not part of any session; its events are stored like the others.
    async fn run_task(&self, req: TaskRequest) -> Result<TaskRun> {
        let mut agent = self.agent(&req.agent)?;
        agent.tools.retain(|name| name != "training_harness");
        if let Some(p) = &req.profile {
            agent.inference_profile = self.profile(p)?.id;
        }
        if let Some(p) = &req.system_prompt {
            agent.system_prompt = p.clone();
        }
        let profile = self.profile(&agent.inference_profile)?;
        let manifest = self.0.runtimes.manifest(&profile.runtime_id)?;
        if profile.model.is_empty() && manifest.launch.as_ref().map(|l| l.requires_model).unwrap_or(false) {
            return Err(Error::Invalid(format!(
                "Il profilo \"{}\" non ha un modello: scaricane uno da Modelli e selezionalo in Agenti → Profili.",
                profile.name
            )));
        }
        let run_id = new_id("run");
        let cancel = req.cancel;
        self.0
            .runs
            .lock()
            .unwrap()
            .insert(run_id.clone(), RunHandle { session_id: None, cancel: cancel.clone(), events: vec![], done: false });
        self.0.store.start_run(&run_id, None)?;

        let own_budget = agent.budget.clone().unwrap_or_default();
        let mut input = self.build_input_at(
            &run_id,
            &run_id,
            agent,
            profile,
            vec![],
            req.prompt.clone(),
            cancel.clone(),
            0,
            Arc::new(Mutex::new(HashSet::new())),
            Some(req.workdir.clone()),
        );
        input.policy.mode = "auto".into();
        // A rule that asks would wait for somebody who is not there.
        input.policy.rules.retain(|r| r.action != "ask");
        input.tool_ctx.sandbox = SandboxMode::WorkspaceWrite;
        input.budget = Arc::new(self.run_budget(&own_budget, Some(req.timeout.as_secs().max(1))));
        let budget = input.budget.clone();

        let host = Arc::new(EngineHost::new(self.clone(), None, None).denying_approvals());
        let started = Instant::now();
        let run = AgentRunner::run_partial(input, host);
        tokio::pin!(run);
        let (out, res) = tokio::select! {
            r = &mut run => r,
            _ = tokio::time::sleep(req.timeout + STOP_GRACE) => {
                cancel.cancel();
                run.await
            }
        };
        let status = match &res {
            Ok(()) => "succeeded",
            Err(Error::Cancelled) => "cancelled",
            Err(_) => "failed",
        };
        if let Err(e) = &res {
            if !self.run_done(&run_id) {
                self.emit(&run_id, AgEvent::RunError { message: e.to_string(), code: None });
            }
        }
        let _ = self.0.store.finish_run(&run_id, status);
        let error = match &res {
            Err(Error::Cancelled) => Some(format!("run fermato: il tempo del task ({} s) è scaduto", req.timeout.as_secs())),
            Err(e) => Some(e.to_string()),
            // A run that hit its budget ends with a notice, not with an error: for a task that is a failure all the same.
            Ok(()) => self.run_events(&run_id).iter().find_map(|e| match &e.event.event {
                AgEvent::Custom { name, value } if name == "aiw.budget_exceeded" => {
                    Some(format!("budget del run esaurito: {}", value["reason"].as_str().unwrap_or("limite raggiunto")))
                }
                _ => None,
            }),
        };
        Ok(TaskRun {
            final_text: out.final_text,
            structured: out.structured,
            duration_ms: started.elapsed().as_millis() as u64,
            tokens: budget.used_tokens(),
            turns: budget.used_turns(),
            run_id: Some(run_id),
            error,
        })
    }
}
