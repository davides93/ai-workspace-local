//! The engine side of [`aiw_agent::RunHost`] and [`aiw_workflow::WorkflowHost`].

use crate::Engine;
use aiw_agent::{narrow_sandbox, narrow_tools, AgentRunner, RunHost, RunInput, Skill};
use aiw_core::api::{ApprovalOutcome, ApprovalRequest, AuditEntry, InferenceProfile, WorkflowDef, WorkflowRun, WorkflowStep};
use aiw_core::events::AgEvent;
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use aiw_runtimes::{ChatBackend, ChatDelta, ChatRequest, Msg};
use aiw_tools::ToolRegistry;
use aiw_workflow::WorkflowState;
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{json, Map, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub struct EngineHost {
    engine: Engine,
    session_id: Option<String>,
    /// For sub-agents: approval events are mirrored to the parent run so the UI sees them.
    parent_run: Option<String>,
    workflow: Option<WorkflowDef>,
    first_update: Mutex<Option<oneshot::Sender<WorkflowRun>>>,
    workflow_session: Mutex<Option<String>>,
    /// Nobody can answer: every confirmation the run asks for is refused (evaluation runs).
    deny_approvals: bool,
}

impl EngineHost {
    pub fn new(engine: Engine, session_id: Option<String>, parent_run: Option<String>) -> Self {
        Self {
            engine,
            session_id,
            parent_run,
            workflow: None,
            first_update: Mutex::new(None),
            workflow_session: Mutex::new(None),
            deny_approvals: false,
        }
    }
    /// `first` receives the first state of the run (a resumed run has no one waiting for it).
    pub fn for_workflow(engine: Engine, wf: WorkflowDef, first: Option<oneshot::Sender<WorkflowRun>>) -> Self {
        Self {
            engine,
            session_id: None,
            parent_run: None,
            workflow: Some(wf),
            first_update: Mutex::new(first),
            workflow_session: Mutex::new(None),
            deny_approvals: false,
        }
    }
    /// Refuses every confirmation instead of waiting for a person (sub-agents inherit this).
    pub fn denying_approvals(mut self) -> Self {
        self.deny_approvals = true;
        self
    }

    async fn collect_text(backend: Arc<dyn ChatBackend>, req: ChatRequest, cancel: CancellationToken) -> Result<String> {
        let mut s = backend.chat(req, cancel).await?;
        let mut out = String::new();
        while let Some(d) = s.next().await {
            if let ChatDelta::Text(t) = d? {
                out.push_str(&t);
            }
        }
        // Drop inline reasoning blocks.
        let re = regex_lite_think(&out);
        Ok(re.trim().to_string())
    }
}

fn regex_lite_think(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("<think>") {
        out.push_str(&rest[..i]);
        match rest[i..].find("</think>") {
            Some(j) => rest = &rest[i + j + "</think>".len()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

#[async_trait]
impl RunHost for EngineHost {
    fn emit(&self, run_id: &str, ev: AgEvent) {
        if let (Some(parent), AgEvent::Custom { name, .. }) = (&self.parent_run, &ev) {
            if name.starts_with("aiw.approval") {
                self.engine.emit(parent, ev.clone());
            }
        }
        self.engine.emit(run_id, ev);
    }

    async fn approve(&self, req: ApprovalRequest) -> bool {
        if self.deny_approvals {
            return false;
        }
        self.engine.wait_approval(req, self.session_id.clone()).await
    }

    async fn approve_outcome(&self, req: ApprovalRequest) -> ApprovalOutcome {
        self.engine.wait_approval_outcome(req, self.session_id.clone(), self.engine.approval_timeout()).await
    }

    fn audit(&self, entry: AuditEntry) {
        let _ = self.engine.0.store.add_audit(&entry);
    }

    async fn backend(&self, profile: &InferenceProfile) -> Result<Arc<dyn ChatBackend>> {
        // The profile's context window goes to the runtime (launch argument of a sidecar, or the
        // in-process engine), not only to the compaction threshold of the runner.
        self.engine
            .0
            .runtimes
            .backend_with(&profile.runtime_id, (!profile.model.is_empty()).then_some(profile.model.as_str()), profile.params.context)
            .await
    }

    fn tools(&self) -> Arc<ToolRegistry> {
        self.engine.tool_registry()
    }

    fn skills(&self) -> Vec<Skill> {
        self.engine.skills()
    }

    async fn subagent(&self, parent: &RunInput, agent_id: &str, task: &str, run_id: &str) -> Result<String> {
        if agent_id.starts_with(crate::REMOTE_PREFIX) {
            return self.engine.delegate_remote(agent_id, task, &parent.tool_ctx.cancel, Some(&parent.run_id)).await;
        }
        let agent = self.engine.agent(agent_id)?;
        let profile = self.engine.profile(&agent.inference_profile)?;
        let cancel = parent.tool_ctx.cancel.child_token();
        let mut input = self.engine.build_input(
            run_id,
            &parent.thread_id,
            agent,
            profile,
            vec![],
            task.to_string(),
            cancel,
            parent.depth + 1,
            parent.session_allow.clone(),
        );
        // A sub-agent never gets more freedom than its parent.
        if mode_rank(&input.policy.mode) > mode_rank(&parent.policy.mode) {
            input.policy.mode = parent.policy.mode.clone();
        }
        // The run budget is shared with the parent: delegating does not buy more.
        input.budget = parent.budget.clone();
        // Same for the sandbox and the tool list.
        input.tool_ctx.sandbox = narrow_sandbox(parent.tool_ctx.sandbox, input.tool_ctx.sandbox);
        input.agent.tools = narrow_tools(&parent.agent.tools, &input.agent.tools);
        // The members of a team share its state (the tool is harmless: it touches nothing outside the run).
        input.in_team = parent.in_team;
        input.shared = parent.shared.clone();
        input.root_run = parent.root_run.clone();
        if input.in_team && !input.agent.tools.iter().any(|t| t == "*" || t == aiw_agent::internal::TEAM_STATE) {
            input.agent.tools.push(aiw_agent::internal::TEAM_STATE.into());
        }
        let mut host = EngineHost::new(self.engine.clone(), self.session_id.clone(), Some(parent.run_id.clone()));
        host.deny_approvals = self.deny_approvals;
        let host = Arc::new(host);
        let out = AgentRunner::run(input, host).await?;
        Ok(if out.final_text.is_empty() { "(nessuna risposta)".into() } else { out.final_text })
    }

    fn handoff_target(&self, from: &RunInput, agent_id: &str) -> Result<String> {
        if agent_id.starts_with(crate::REMOTE_PREFIX) {
            return Err(Error::Denied("la conversazione non può passare a un agente remoto: delegagli il lavoro".into()));
        }
        let target = self.engine.agent(agent_id)?;
        // The conversation never moves to an agent with more freedom than the one that started it.
        if mode_rank(&target.mode) > mode_rank(&from.policy.mode) {
            return Err(Error::Denied(format!(
                "«{}» ha una modalità più permissiva ({}) di quella di partenza ({})",
                target.name, target.mode, from.policy.mode
            )));
        }
        Ok(target.name)
    }
}

pub(crate) fn mode_rank(m: &str) -> u8 {
    match m {
        "plan" => 0,
        "ask" => 1,
        "auto-edit" => 2,
        "auto" => 3,
        _ => 4,
    }
}

#[async_trait]
impl aiw_workflow::WorkflowHost for EngineHost {
    async fn call_tool(&self, run_id: &str, tool: &str, args: Value) -> Result<String> {
        let actor = self.workflow.as_ref().map(|w| format!("Workflow \"{}\"", w.name)).unwrap_or_else(|| "Workflow".into());
        let out = self.engine.execute_tool(run_id, "workflow", &actor, tool, args).await?;
        if out.is_error {
            return Err(Error::Runtime(out.content));
        }
        Ok(out.content)
    }

    async fn run_agent(&self, _run_id: &str, agent_id: &str, prompt: &str) -> Result<String> {
        // Each workflow keeps one session per run so the work is inspectable in the Workspace view.
        // A session runs one thing at a time: the session is taken while an agent works in it, so
        // agents that run together (foreach, parallel) each get their own.
        let existing = self.workflow_session.lock().unwrap().take();
        let name = self.workflow.as_ref().map(|w| w.name.clone()).unwrap_or_else(|| "Workflow".into());
        match self.engine.run_agent_task(agent_id, prompt, existing.as_deref(), &format!("⚙ {name}")).await {
            Ok((text, session_id)) => {
                self.workflow_session.lock().unwrap().get_or_insert(session_id);
                Ok(text)
            }
            Err(e) => {
                if let Some(s) = existing {
                    self.workflow_session.lock().unwrap().get_or_insert(s);
                }
                Err(e)
            }
        }
    }

    async fn complete(&self, _run_id: &str, profile_id: &str, prompt: &str) -> Result<String> {
        let p = self.engine.profile(profile_id)?;
        let backend =
            self.engine.0.runtimes.backend_with(&p.runtime_id, (!p.model.is_empty()).then_some(p.model.as_str()), p.params.context).await?;
        let req = ChatRequest {
            model: p.model.clone(),
            messages: vec![Msg::user(prompt)],
            temperature: p.params.temperature,
            max_tokens: p.params.max_tokens,
            ..Default::default()
        };
        Self::collect_text(backend, req, CancellationToken::new()).await
    }

    async fn run_subworkflow(&self, parent_run_id: &str, workflow_id: &str, inputs: Map<String, Value>, depth: u32) -> Result<String> {
        /// A child that stops being waited for (the parent step timed out or was dropped) must not
        /// stay `running` in the list of the runs.
        struct Abandon<'a>(&'a Engine, String, bool);
        impl Drop for Abandon<'_> {
            fn drop(&mut self) {
                if self.2 {
                    self.0.abandon_workflow_run(&self.1, "annullato: il workflow padre non lo aspetta più");
                }
            }
        }
        let wf: WorkflowDef = self
            .engine
            .0
            .store
            .get_doc("workflow", workflow_id)?
            .ok_or_else(|| Error::NotFound(format!("il sotto-workflow {workflow_id} non esiste")))?;
        let state = WorkflowState::new(&wf, inputs).child_of(parent_run_id, depth);
        let _ = self.engine.0.store.add_audit(&AuditEntry {
            id: new_id("aud"),
            ts: now_rfc3339(),
            run_id: Some(state.run.id.clone()),
            actor: "workflow".into(),
            action: "workflow.run".into(),
            tool: None,
            decision: None,
            detail: format!("{} (sotto-workflow di {parent_run_id})", wf.name),
        });
        let mut guard = Abandon(&self.engine, state.run.id.clone(), true);
        let host = EngineHost::for_workflow(self.engine.clone(), wf.clone(), None);
        let done = aiw_workflow::run_state(state, &host).await;
        guard.2 = false;
        match done.run.status.as_str() {
            "succeeded" => {
                // The output of the last step that produced one: a condition only says which way it went.
                let last = done
                    .run
                    .steps
                    .iter()
                    .zip(&wf.steps)
                    .rev()
                    .find(|(r, s)| r.status == "done" && !matches!(s, WorkflowStep::Condition { .. }));
                Ok(last.and_then(|(r, _)| done.outputs.get(&r.id).cloned()).unwrap_or_default())
            }
            "cancelled" => Err(Error::Runtime(format!("il sotto-workflow «{}» è stato annullato", wf.name))),
            _ => {
                let why = done.run.steps.iter().find_map(|s| s.error.clone()).unwrap_or_else(|| "errore sconosciuto".into());
                Err(Error::Runtime(format!("il sotto-workflow «{}» è fallito: {why}", wf.name)))
            }
        }
    }

    async fn approval(&self, run_id: &str, message: &str, timeout_s: Option<u64>) -> ApprovalOutcome {
        let name = self.workflow.as_ref().map(|w| w.name.clone()).unwrap_or_default();
        let req = ApprovalRequest {
            id: new_id("apr"),
            run_id: run_id.into(),
            tool: "workflow".into(),
            args: json!({"workflow": name, "message": message}),
            reason: message.into(),
            risk: "medium".into(),
        };
        self.engine.wait_approval_outcome(req, None, timeout_s.map(Duration::from_secs)).await
    }

    fn update(&self, run: &WorkflowRun) {
        {
            let mut runs = self.engine.0.workflow_runs.lock().unwrap();
            match runs.iter_mut().find(|r| r.id == run.id) {
                Some(r) => *r = run.clone(),
                None => runs.insert(0, run.clone()),
            }
            runs.truncate(100);
        }
        let _ = self.engine.0.store.save_workflow_run(run);
        if let Some(tx) = self.first_update.lock().unwrap().take() {
            let _ = tx.send(run.clone());
        }
    }

    fn save_state(&self, state: &WorkflowState) {
        let _ = self.engine.0.store.save_workflow_state(state);
    }

    fn clear_state(&self, run_id: &str) {
        let _ = self.engine.0.store.clear_workflow_state(run_id);
    }
}
