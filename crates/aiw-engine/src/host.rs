//! The engine side of [`aiw_agent::RunHost`] and [`aiw_workflow::WorkflowHost`].

use crate::Engine;
use aiw_agent::{narrow_sandbox, narrow_tools, AgentRunner, RunHost, RunInput, Skill};
use aiw_core::api::{ApprovalRequest, AuditEntry, InferenceProfile, WorkflowDef, WorkflowRun};
use aiw_core::events::AgEvent;
use aiw_core::{ids::new_id, Error, Result};
use aiw_runtimes::{ChatBackend, ChatDelta, ChatRequest, Msg};
use aiw_tools::ToolRegistry;
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
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
}

impl EngineHost {
    pub fn new(engine: Engine, session_id: Option<String>, parent_run: Option<String>) -> Self {
        Self { engine, session_id, parent_run, workflow: None, first_update: Mutex::new(None), workflow_session: Mutex::new(None) }
    }
    pub fn for_workflow(engine: Engine, wf: WorkflowDef, first: oneshot::Sender<WorkflowRun>) -> Self {
        Self {
            engine,
            session_id: None,
            parent_run: None,
            workflow: Some(wf),
            first_update: Mutex::new(Some(first)),
            workflow_session: Mutex::new(None),
        }
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
        self.engine.wait_approval(req, self.session_id.clone()).await
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

    async fn subagent(&self, parent: &RunInput, agent_id: &str, task: &str) -> Result<String> {
        let agent = self.engine.agent(agent_id)?;
        let profile = self.engine.profile(&agent.inference_profile)?;
        let run_id = new_id("run");
        let cancel = parent.tool_ctx.cancel.child_token();
        let mut input = self.engine.build_input(
            &run_id,
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
        // Same for the sandbox and the tool list.
        input.tool_ctx.sandbox = narrow_sandbox(parent.tool_ctx.sandbox, input.tool_ctx.sandbox);
        input.agent.tools = narrow_tools(&parent.agent.tools, &input.agent.tools);
        let host = Arc::new(EngineHost::new(self.engine.clone(), self.session_id.clone(), Some(parent.run_id.clone())));
        let out = AgentRunner::run(input, host).await?;
        Ok(if out.final_text.is_empty() { "(nessuna risposta)".into() } else { out.final_text })
    }
}

fn mode_rank(m: &str) -> u8 {
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
        let existing = self.workflow_session.lock().unwrap().clone();
        let name = self.workflow.as_ref().map(|w| w.name.clone()).unwrap_or_else(|| "Workflow".into());
        let (text, session_id) = self.engine.run_agent_task(agent_id, prompt, existing.as_deref(), &format!("⚙ {name}")).await?;
        *self.workflow_session.lock().unwrap() = Some(session_id);
        Ok(text)
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

    async fn approval(&self, run_id: &str, message: &str) -> bool {
        let name = self.workflow.as_ref().map(|w| w.name.clone()).unwrap_or_default();
        let req = ApprovalRequest {
            id: new_id("apr"),
            run_id: run_id.into(),
            tool: "workflow".into(),
            args: json!({"workflow": name, "message": message}),
            reason: message.into(),
            risk: "medium".into(),
        };
        self.engine.wait_approval(req, None).await
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
        if run.status != "running" && run.status != "waiting_approval" || run.steps.iter().all(|s| s.status == "pending") {
            let _ = self.engine.0.store.save_workflow_run(run);
        }
        if let Some(tx) = self.first_update.lock().unwrap().take() {
            let _ = tx.send(run.clone());
        }
    }
}
