use aiw_core::api::{OnError, StepOptions, WorkflowDef, WorkflowRun, WorkflowStep, WorkflowStepRun};
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;

/// Services a workflow needs from the engine (policy and approvals apply as for agents).
#[async_trait]
pub trait WorkflowHost: Send + Sync {
    async fn call_tool(&self, run_id: &str, tool: &str, args: Value) -> Result<String>;
    async fn run_agent(&self, run_id: &str, agent: &str, prompt: &str) -> Result<String>;
    async fn complete(&self, run_id: &str, profile: &str, prompt: &str) -> Result<String>;
    async fn approval(&self, run_id: &str, message: &str) -> bool;
    fn update(&self, run: &WorkflowRun);
    /// Called after every change of a run that has not finished: the state is enough to go on with
    /// [`run_from`] after a restart.
    fn save_state(&self, _state: &WorkflowState) {}
    /// Called once the run has finished (any outcome).
    fn clear_state(&self, _run_id: &str) {}
}

/// What the placeholders of a template can see.
pub struct RenderCtx<'a> {
    pub inputs: &'a Map<String, Value>,
    pub outputs: &'a HashMap<String, String>,
    /// Errors of the steps that failed with `on_error: continue`.
    pub errors: &'a HashMap<String, String>,
}

/// Replaces `{{inputs.x}}`, `{{steps.id.output}}` and `{{steps.id.error}}` placeholders.
pub fn render_ctx(template: &str, ctx: &RenderCtx) -> String {
    let re = regex::Regex::new(r"\{\{\s*([a-zA-Z0-9_.\-]+)\s*\}\}").unwrap();
    re.replace_all(template, |c: &regex::Captures| {
        let path = &c[1];
        if let Some(k) = path.strip_prefix("inputs.") {
            return ctx.inputs.get(k).map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
        }
        if let Some(rest) = path.strip_prefix("steps.") {
            if let Some(id) = rest.strip_suffix(".error") {
                return ctx.errors.get(id).cloned().unwrap_or_default();
            }
            let id = rest.trim_end_matches(".output");
            return ctx.outputs.get(id).cloned().unwrap_or_default();
        }
        if path == "now" {
            return chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        }
        c[0].to_string()
    })
    .to_string()
}

pub fn render_value_ctx(v: &Value, ctx: &RenderCtx) -> Value {
    match v {
        Value::String(s) => Value::String(render_ctx(s, ctx)),
        Value::Array(a) => Value::Array(a.iter().map(|x| render_value_ctx(x, ctx)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), render_value_ctx(x, ctx))).collect()),
        other => other.clone(),
    }
}

/// Replaces `{{inputs.x}}` and `{{steps.id.output}}` placeholders.
pub fn render(template: &str, inputs: &Map<String, Value>, outputs: &HashMap<String, String>) -> String {
    render_ctx(template, &RenderCtx { inputs, outputs, errors: &HashMap::new() })
}

pub fn render_value(v: &Value, inputs: &Map<String, Value>, outputs: &HashMap<String, String>) -> Value {
    render_value_ctx(v, &RenderCtx { inputs, outputs, errors: &HashMap::new() })
}

/// Tiny condition language (after templating): `A contains B`, `A == B`, `A != B`, `not A`, `A`.
pub fn evaluate_condition(expr: &str) -> bool {
    let e = expr.trim();
    let unq = |s: &str| s.trim().trim_matches(['\'', '"']).to_string();
    if let Some(rest) = e.strip_prefix("not ") {
        return !evaluate_condition(rest);
    }
    for (op, f) in [(" contains ", 0), (" == ", 1), (" != ", 2)] {
        if let Some((a, b)) = e.split_once(op) {
            let (a, b) = (unq(a), unq(b));
            return match f {
                0 => a.to_lowercase().contains(&b.to_lowercase()),
                1 => a == b,
                _ => a != b,
            };
        }
    }
    let v = unq(e).to_lowercase();
    !(v.is_empty() || v == "false" || v == "0" || v == "no")
}

/// Everything a run needs to go on: the run itself plus what the placeholders and the branches use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    pub run: WorkflowRun,
    pub def: WorkflowDef,
    /// Workflow defaults merged with the inputs of this run.
    pub inputs: Map<String, Value>,
    pub outputs: HashMap<String, String>,
    pub errors: HashMap<String, String>,
    pub enabled: HashSet<String>,
    /// Index of the step that runs (or runs next).
    pub next: usize,
    pub depth: u32,
}

impl WorkflowState {
    pub fn new(wf: &WorkflowDef, inputs: Map<String, Value>) -> Self {
        let mut merged = wf.inputs.clone();
        merged.extend(inputs);
        Self {
            run: WorkflowRun {
                id: new_id("wfr"),
                workflow_id: wf.id.clone(),
                status: "running".into(),
                started_at: now_rfc3339(),
                finished_at: None,
                steps: wf
                    .steps
                    .iter()
                    .map(|s| WorkflowStepRun {
                        id: s.id().to_string(),
                        status: "pending".into(),
                        output: None,
                        error: None,
                        attempts: None,
                    })
                    .collect(),
            },
            def: wf.clone(),
            inputs: merged,
            outputs: HashMap::new(),
            errors: HashMap::new(),
            enabled: HashSet::new(),
            next: 0,
            depth: 0,
        }
    }
}

/// The state of a run behind a lock that is never held across an `.await`; every change is
/// published to the host.
struct Shared<'a> {
    host: &'a dyn WorkflowHost,
    st: Mutex<WorkflowState>,
}

impl Shared<'_> {
    fn publish(&self, f: impl FnOnce(&mut WorkflowState)) {
        let snapshot = {
            let mut g = self.st.lock().unwrap();
            f(&mut g);
            g.clone()
        };
        self.host.update(&snapshot.run);
        if matches!(snapshot.run.status.as_str(), "running" | "waiting_approval") {
            self.host.save_state(&snapshot);
        } else {
            self.host.clear_state(&snapshot.run.id);
        }
    }
}

/// Runs `f` with the retry and timeout options of the step. `on_attempt` is told the attempt number
/// when the step has retries.
async fn run_with_options<F, Fut>(opts: &StepOptions, on_attempt: impl Fn(u32), mut f: F) -> Result<String>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        if opts.retry > 0 {
            on_attempt(attempt);
        }
        let result = match opts.timeout_s {
            Some(t) => tokio::time::timeout(Duration::from_secs(t), f())
                .await
                .unwrap_or_else(|_| Err(Error::Runtime(format!("tempo scaduto dopo {t} s")))),
            None => f().await,
        };
        match result {
            Err(e) if attempt <= opts.retry => {
                tracing::warn!(attempt, "workflow step failed, retrying: {e}");
                tokio::time::sleep(Duration::from_secs(opts.retry_delay_s)).await;
            }
            other => return other,
        }
    }
}

/// Executes one `tool`, `agent` or `llm` step.
async fn exec_leaf(step: &WorkflowStep, run_id: &str, ctx: &RenderCtx<'_>, host: &dyn WorkflowHost) -> Result<String> {
    match step {
        WorkflowStep::Tool { tool, args, .. } => host.call_tool(run_id, tool, render_value_ctx(&Value::Object(args.clone()), ctx)).await,
        WorkflowStep::Agent { agent, prompt, .. } => host.run_agent(run_id, agent, &render_ctx(prompt, ctx)).await,
        WorkflowStep::Llm { profile, prompt, .. } => host.complete(run_id, profile, &render_ctx(prompt, ctx)).await,
        other => Err(Error::Invalid(format!("il passo {} non si può eseguire qui", other.id()))),
    }
}

pub async fn run_workflow(wf: &WorkflowDef, inputs: Map<String, Value>, host: &dyn WorkflowHost) -> WorkflowRun {
    run_from(WorkflowState::new(wf, inputs), host).await
}

/// Runs (or goes on with) a workflow from `state`.
pub async fn run_from(state: WorkflowState, host: &dyn WorkflowHost) -> WorkflowRun {
    let sh = Shared { host, st: Mutex::new(state) };
    sh.publish(|_| {});
    let (def, run_id, first) = {
        let g = sh.st.lock().unwrap();
        (g.def.clone(), g.run.id.clone(), g.next)
    };
    let gated: HashSet<String> = def
        .steps
        .iter()
        .filter_map(|s| {
            if let WorkflowStep::Condition { then, otherwise, .. } = s {
                Some(then.iter().chain(otherwise.iter().flatten()).cloned().collect::<Vec<_>>())
            } else {
                None
            }
        })
        .flatten()
        .collect();
    for (i, step) in def.steps.iter().enumerate().skip(first) {
        let id = step.id().to_string();
        let skip = {
            let g = sh.st.lock().unwrap();
            gated.contains(&id) && !g.enabled.contains(&id)
        };
        if skip {
            sh.publish(|st| {
                st.run.steps[i].status = "skipped".into();
                st.next = i + 1;
            });
            continue;
        }
        let is_approval = matches!(step, WorkflowStep::Approval { .. });
        sh.publish(|st| {
            st.next = i;
            st.run.steps[i].status = if is_approval { "waiting".into() } else { "running".into() };
            if is_approval {
                st.run.status = "waiting_approval".into();
            }
        });
        // Placeholders see the state as it was when the step started.
        let (inputs, outputs, errors) = {
            let g = sh.st.lock().unwrap();
            (g.inputs.clone(), g.outputs.clone(), g.errors.clone())
        };
        let ctx = RenderCtx { inputs: &inputs, outputs: &outputs, errors: &errors };
        let result: Result<String> = match step {
            WorkflowStep::Tool { .. } | WorkflowStep::Agent { .. } | WorkflowStep::Llm { .. } => {
                let opts = step.opts().cloned().unwrap_or_default();
                run_with_options(&opts, |n| sh.publish(|st| st.run.steps[i].attempts = Some(n)), || exec_leaf(step, &run_id, &ctx, host))
                    .await
            }
            WorkflowStep::Approval { message, .. } => {
                let ok = host.approval(&run_id, &render_ctx(message, &ctx)).await;
                if ok {
                    sh.publish(|st| st.run.status = "running".into());
                    Ok("approvato".into())
                } else {
                    sh.publish(|st| {
                        st.run.steps[i].status = "failed".into();
                        st.run.steps[i].error = Some("rifiutato dall'utente".into());
                        st.run.status = "cancelled".into();
                        st.run.finished_at = Some(now_rfc3339());
                    });
                    return sh.st.into_inner().unwrap().run;
                }
            }
            WorkflowStep::Condition { condition, then, otherwise, .. } => {
                let rendered = render_ctx(condition, &ctx);
                let yes = evaluate_condition(&rendered);
                let chosen = if yes { then.clone() } else { otherwise.clone().unwrap_or_default() };
                let summary = format!(
                    "{} → {}",
                    if yes { "vero" } else { "falso" },
                    if chosen.is_empty() { "nessun passo".into() } else { chosen.join(", ") }
                );
                sh.publish(|st| st.enabled.extend(chosen.iter().cloned()));
                Ok(summary)
            }
        };
        let on_error = step.opts().map(|o| o.on_error).unwrap_or_default();
        match result {
            Ok(out) => sh.publish(|st| {
                st.run.steps[i].status = "done".into();
                st.run.steps[i].output = Some(out.chars().take(8000).collect());
                st.outputs.insert(id, out);
                st.next = i + 1;
            }),
            Err(e) if on_error == OnError::Continue => sh.publish(|st| {
                // The step stays failed but the run goes on; `{{steps.<id>.error}}` carries the message.
                st.run.steps[i].status = "failed".into();
                st.run.steps[i].error = Some(e.to_string());
                st.errors.insert(id, e.to_string());
                st.next = i + 1;
            }),
            Err(e) => {
                sh.publish(|st| {
                    st.run.steps[i].status = "failed".into();
                    st.run.steps[i].error = Some(e.to_string());
                    st.run.status = "failed".into();
                    st.run.finished_at = Some(now_rfc3339());
                });
                return sh.st.into_inner().unwrap().run;
            }
        }
    }
    sh.publish(|st| {
        st.run.status = "succeeded".into();
        st.run.finished_at = Some(now_rfc3339());
    });
    sh.st.into_inner().unwrap().run
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiw_core::api::WorkflowTrigger;
    use serde_json::json;
    use std::sync::Mutex;

    #[test]
    fn templating_and_conditions() {
        let inputs = json!({"folder": "~/Downloads"}).as_object().unwrap().clone();
        let outs = HashMap::from([("scan".to_string(), "3 file pdf".to_string())]);
        assert_eq!(render("Cartella {{inputs.folder}}: {{steps.scan.output}}", &inputs, &outs), "Cartella ~/Downloads: 3 file pdf");
        assert_eq!(render_value(&json!({"path": "{{ inputs.folder }}"}), &inputs, &outs), json!({"path": "~/Downloads"}));
        assert!(evaluate_condition("3 file PDF contains pdf"));
        assert!(!evaluate_condition("not 'x'"));
        assert!(evaluate_condition("a != b"));
        assert!(!evaluate_condition(""));
    }

    struct Host {
        approve: bool,
        calls: Mutex<Vec<String>>,
    }
    #[async_trait]
    impl WorkflowHost for Host {
        async fn call_tool(&self, _r: &str, tool: &str, args: Value) -> Result<String> {
            self.calls.lock().unwrap().push(format!("tool:{tool}:{args}"));
            Ok("trovati 2 pdf".into())
        }
        async fn run_agent(&self, _r: &str, agent: &str, prompt: &str) -> Result<String> {
            self.calls.lock().unwrap().push(format!("agent:{agent}:{prompt}"));
            Ok("fatto".into())
        }
        async fn complete(&self, _r: &str, _p: &str, prompt: &str) -> Result<String> {
            self.calls.lock().unwrap().push(format!("llm:{prompt}"));
            Ok("notifica".into())
        }
        async fn approval(&self, _r: &str, _m: &str) -> bool {
            self.approve
        }
        fn update(&self, _run: &WorkflowRun) {}
    }

    fn wf() -> WorkflowDef {
        serde_json::from_value(json!({
            "id": "w", "name": "W", "enabled": true, "trigger": {"kind": "manual"}, "inputs": {"folder": "/tmp"},
            "steps": [
                {"id": "scan", "kind": "tool", "tool": "list_dir", "args": {"path": "{{inputs.folder}}"}},
                {"id": "check", "kind": "condition", "if": "{{steps.scan.output}} contains pdf", "then": ["notify"], "else": ["skipme"]},
                {"id": "notify", "kind": "llm", "profile": "fast", "prompt": "Avvisa: {{steps.scan.output}}"},
                {"id": "skipme", "kind": "agent", "agent": "op", "prompt": "non dovrei girare"},
                {"id": "ok", "kind": "approval", "message": "Procedo?"},
                {"id": "act", "kind": "agent", "agent": "op", "prompt": "Agisci su {{steps.notify.output}}"}
            ]
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn runs_steps_with_branches_and_approval() {
        let host = Host { approve: true, calls: Mutex::new(vec![]) };
        let run = run_workflow(&wf(), Map::new(), &host).await;
        assert_eq!(run.status, "succeeded");
        let st: Vec<_> = run.steps.iter().map(|s| (s.id.as_str(), s.status.as_str())).collect();
        assert_eq!(
            st,
            vec![("scan", "done"), ("check", "done"), ("notify", "done"), ("skipme", "skipped"), ("ok", "done"), ("act", "done")]
        );
        let calls = host.calls.lock().unwrap();
        assert!(calls[0].contains("\"path\":\"/tmp\""));
        assert_eq!(calls.last().unwrap(), "agent:op:Agisci su notifica");
        assert!(matches!(wf().trigger, WorkflowTrigger::Manual));
    }

    #[tokio::test]
    async fn rejected_approval_cancels_the_run() {
        let host = Host { approve: false, calls: Mutex::new(vec![]) };
        let run = run_workflow(&wf(), Map::new(), &host).await;
        assert_eq!(run.status, "cancelled");
        assert_eq!(run.steps.last().unwrap().status, "pending");
    }

    /// A host whose `llm` calls fail a given number of times, or hang.
    struct Flaky {
        failures: Mutex<u32>,
        hang: bool,
        calls: Mutex<u32>,
        last: Mutex<Option<WorkflowRun>>,
    }
    impl Flaky {
        fn new(failures: u32, hang: bool) -> Self {
            Self { failures: Mutex::new(failures), hang, calls: Mutex::new(0), last: Mutex::new(None) }
        }
    }
    #[async_trait]
    impl WorkflowHost for Flaky {
        async fn call_tool(&self, _r: &str, _t: &str, _a: Value) -> Result<String> {
            Ok("tool".into())
        }
        async fn run_agent(&self, _r: &str, _a: &str, _p: &str) -> Result<String> {
            Ok("agent".into())
        }
        async fn complete(&self, _r: &str, _p: &str, prompt: &str) -> Result<String> {
            *self.calls.lock().unwrap() += 1;
            if self.hang {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
            let mut f = self.failures.lock().unwrap();
            if *f > 0 {
                *f -= 1;
                return Err(Error::Runtime("guasto".into()));
            }
            Ok(format!("ok:{prompt}"))
        }
        async fn approval(&self, _r: &str, _m: &str) -> bool {
            true
        }
        fn update(&self, run: &WorkflowRun) {
            *self.last.lock().unwrap() = Some(run.clone());
        }
    }

    fn one_llm(extra: Value) -> WorkflowDef {
        let mut step = json!({"id": "a", "kind": "llm", "profile": "p", "prompt": "x"});
        step.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(json!({
            "id": "w", "name": "W", "trigger": {"kind": "manual"},
            "steps": [step, {"id": "b", "kind": "llm", "profile": "p", "prompt": "dopo: {{steps.a.error}}|{{steps.a.output}}"}]
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn retries_a_failing_step() {
        let host = Flaky::new(2, false);
        let run = run_workflow(&one_llm(json!({"retry": 2, "retry_delay_s": 0})), Map::new(), &host).await;
        assert_eq!(run.status, "succeeded");
        assert_eq!(*host.calls.lock().unwrap(), 4, "3 attempts for a, 1 for b");
        assert_eq!(run.steps[0].attempts, Some(3));
        assert_eq!(run.steps[1].attempts, None);

        let host = Flaky::new(3, false);
        let run = run_workflow(&one_llm(json!({"retry": 2, "retry_delay_s": 0})), Map::new(), &host).await;
        assert_eq!(run.status, "failed");
        assert_eq!(run.steps[0].status, "failed");
        assert_eq!(run.steps[0].attempts, Some(3));
        assert_eq!(run.steps[1].status, "pending");
    }

    #[tokio::test]
    async fn continue_on_error_keeps_going_and_exposes_the_error() {
        let host = Flaky::new(1, false);
        let run = run_workflow(&one_llm(json!({"on_error": "continue"})), Map::new(), &host).await;
        assert_eq!(run.status, "succeeded");
        assert_eq!(run.steps[0].status, "failed");
        assert!(run.steps[0].error.as_deref().unwrap().contains("guasto"));
        assert_eq!(run.steps[1].output.as_deref(), Some("ok:dopo: guasto|"));
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_fails_the_step_and_can_be_retried() {
        let host = Flaky::new(0, true);
        let run = run_workflow(&one_llm(json!({"timeout_s": 5, "retry": 1, "retry_delay_s": 1})), Map::new(), &host).await;
        assert_eq!(run.status, "failed");
        assert!(run.steps[0].error.as_deref().unwrap().contains("tempo scaduto dopo 5 s"));
        assert_eq!(*host.calls.lock().unwrap(), 2);
    }

    /// A host that remembers the saved state and can hang in an agent step, like a process that dies.
    struct Durable {
        hang_agent: bool,
        calls: Mutex<Vec<String>>,
        state: Mutex<Option<WorkflowState>>,
        cleared: Mutex<Vec<String>>,
    }
    impl Durable {
        fn new(hang_agent: bool) -> Self {
            Self { hang_agent, calls: Mutex::new(vec![]), state: Mutex::new(None), cleared: Mutex::new(vec![]) }
        }
    }
    #[async_trait]
    impl WorkflowHost for Durable {
        async fn call_tool(&self, _r: &str, tool: &str, _a: Value) -> Result<String> {
            self.calls.lock().unwrap().push(format!("tool:{tool}"));
            Ok(format!("risultato di {tool}"))
        }
        async fn run_agent(&self, _r: &str, agent: &str, prompt: &str) -> Result<String> {
            self.calls.lock().unwrap().push(format!("agent:{agent}:{prompt}"));
            if self.hang_agent {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
            Ok("fatto".into())
        }
        async fn complete(&self, _r: &str, _p: &str, _prompt: &str) -> Result<String> {
            Ok("llm".into())
        }
        async fn approval(&self, _r: &str, _m: &str) -> bool {
            true
        }
        fn update(&self, _run: &WorkflowRun) {}
        fn save_state(&self, state: &WorkflowState) {
            *self.state.lock().unwrap() = Some(state.clone());
        }
        fn clear_state(&self, run_id: &str) {
            self.cleared.lock().unwrap().push(run_id.to_string());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn resumes_from_the_saved_state_without_repeating_finished_steps() {
        let def: WorkflowDef = serde_json::from_value(json!({
            "id": "w", "name": "W", "trigger": {"kind": "manual"},
            "steps": [
                {"id": "scan", "kind": "tool", "tool": "list_dir"},
                {"id": "plan", "kind": "llm", "profile": "p", "prompt": "x"},
                {"id": "act", "kind": "agent", "agent": "op", "prompt": "Esegui {{steps.scan.output}}"},
                {"id": "last", "kind": "tool", "tool": "notify"}
            ]
        }))
        .unwrap();
        // First process: the agent step never ends and the future is dropped, like a crash.
        let first = Durable::new(true);
        let crashed = tokio::time::timeout(Duration::from_secs(60), run_workflow(&def, Map::new(), &first)).await;
        assert!(crashed.is_err());
        assert!(first.cleared.lock().unwrap().is_empty(), "an unfinished run keeps its state");
        let saved = first.state.lock().unwrap().clone().expect("state saved");
        assert_eq!(saved.next, 2);
        assert_eq!(saved.run.steps[2].status, "running");
        assert_eq!(saved.run.steps[1].status, "done");

        // The state goes through JSON, as in the database.
        let saved: WorkflowState = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        let run_id = saved.run.id.clone();

        // Second process: resumes from `act`, which runs again (at least once), and finishes.
        let second = Durable::new(false);
        let run = run_from(saved, &second).await;
        assert_eq!(run.id, run_id);
        assert_eq!(run.status, "succeeded");
        assert!(run.steps.iter().all(|s| s.status == "done"));
        assert_eq!(*second.calls.lock().unwrap(), vec!["agent:op:Esegui risultato di list_dir".to_string(), "tool:notify".to_string()]);
        assert_eq!(*second.cleared.lock().unwrap(), vec![run_id]);
    }
}
