use aiw_core::api::{
    ApprovalDefault, ApprovalOutcome, OnError, StepOptions, WorkflowDef, WorkflowRun, WorkflowStep, WorkflowStepRun, MAX_FOREACH_ITEMS,
    MAX_FOREACH_PARALLEL, MAX_SUBWORKFLOW_DEPTH,
};
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use async_trait::async_trait;
use futures::stream::{FuturesUnordered, StreamExt};
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
    /// Asks the user and waits for the answer, at most `timeout_s` seconds when given.
    async fn approval(&self, run_id: &str, message: &str, timeout_s: Option<u64>) -> ApprovalOutcome;
    fn update(&self, run: &WorkflowRun);
    /// Runs the workflow `workflow_id` as a child of the run `parent_run_id` (at nesting level
    /// `depth`) and waits for it; returns the output of the child.
    async fn run_subworkflow(&self, _parent_run_id: &str, workflow_id: &str, _inputs: Map<String, Value>, _depth: u32) -> Result<String> {
        Err(Error::Invalid(format!("il sotto-workflow {workflow_id} non si può avviare qui")))
    }
    /// Called after every change of a run that has not finished: the state is enough to go on with
    /// [`run_from`] after a restart.
    fn save_state(&self, _state: &WorkflowState) {}
    /// Called once the run has finished (any outcome).
    fn clear_state(&self, _run_id: &str) {}
}

/// What the placeholders of a template can see.
#[derive(Clone, Copy)]
pub struct RenderCtx<'a> {
    pub inputs: &'a Map<String, Value>,
    pub outputs: &'a HashMap<String, String>,
    /// Errors of the steps that failed with `on_error: continue`.
    pub errors: &'a HashMap<String, String>,
    /// The entry and its position inside a `foreach`.
    pub item: Option<(&'a Value, usize)>,
}

/// A value as it goes into a template: strings without quotes, anything else as compact JSON.
fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Follows `a.b.0.c` into a JSON value (a number indexes an array).
fn json_get<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    for key in path.split('.').filter(|k| !k.is_empty()) {
        cur = match cur {
            Value::Object(o) => o.get(key)?,
            Value::Array(a) => a.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// The output of a step read as JSON: the whole text, or the first fenced ```json block.
pub fn parse_json_output(text: &str) -> Option<Value> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return Some(v);
    }
    let re = regex::Regex::new(r"(?s)```json\s*(.*?)```").unwrap();
    re.captures(t).and_then(|c| serde_json::from_str(c[1].trim()).ok())
}

/// Replaces the placeholders of a template:
/// - `{{inputs.x}}`, `{{now}}`;
/// - `{{steps.id.output}}` (or `{{steps.id}}`), `{{steps.id.error}}`;
/// - `{{steps.id.json}}` (the output as compact JSON) and `{{steps.id.json.a.b}}`, `{{steps.id.json.items.0.name}}`
///   (a number indexes an array); the output counts as JSON when it is JSON as a whole or holds a
///   ```json block, otherwise the placeholder is empty. A string value is inserted without quotes,
///   anything else as JSON;
/// - `{{item}}`, `{{item.field}}` and `{{index}}` inside a `foreach`.
pub fn render_ctx(template: &str, ctx: &RenderCtx) -> String {
    let re = regex::Regex::new(r"\{\{\s*([a-zA-Z0-9_.\-]+)\s*\}\}").unwrap();
    re.replace_all(template, |c: &regex::Captures| {
        let path = &c[1];
        if let Some((item, index)) = ctx.item {
            if path == "item" {
                return value_text(item);
            }
            if path == "index" {
                return index.to_string();
            }
            if let Some(p) = path.strip_prefix("item.") {
                return json_get(item, p).map(value_text).unwrap_or_default();
            }
        }
        if let Some(k) = path.strip_prefix("inputs.") {
            return ctx.inputs.get(k).map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
        }
        if let Some(rest) = path.strip_prefix("steps.") {
            let (id, tail) = rest.split_once('.').unwrap_or((rest, ""));
            return match tail {
                "" | "output" => ctx.outputs.get(id).cloned().unwrap_or_default(),
                "error" => ctx.errors.get(id).cloned().unwrap_or_default(),
                // The output read as JSON; a placeholder for something that is not JSON stays empty.
                "json" => ctx.outputs.get(id).and_then(|o| parse_json_output(o)).map(|v| value_text(&v)).unwrap_or_default(),
                t => t
                    .strip_prefix("json.")
                    .and_then(|p| json_get(&parse_json_output(ctx.outputs.get(id)?)?, p).map(value_text))
                    .unwrap_or_default(),
            };
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
    render_ctx(template, &RenderCtx { inputs, outputs, errors: &HashMap::new(), item: None })
}

pub fn render_value(v: &Value, inputs: &Map<String, Value>, outputs: &HashMap<String, String>) -> Value {
    render_value_ctx(v, &RenderCtx { inputs, outputs, errors: &HashMap::new(), item: None })
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
    /// Marks the run as a sub-workflow of `parent_run_id`, `depth` levels down.
    pub fn child_of(mut self, parent_run_id: &str, depth: u32) -> Self {
        self.run.parent_run_id = Some(parent_run_id.to_string());
        self.depth = depth;
        self
    }

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
                parent_run_id: None,
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

/// Executes the body of an executable step, composite ones included.
async fn exec_body(sh: &Shared<'_>, i: usize, step: &WorkflowStep, run_id: &str, ctx: &RenderCtx<'_>) -> Result<String> {
    match step {
        WorkflowStep::Foreach { items, body, parallel, .. } => exec_foreach(sh, i, items, body, *parallel, run_id, ctx).await,
        WorkflowStep::Parallel { steps, .. } => exec_parallel(sh, i, steps, run_id, ctx).await,
        WorkflowStep::Workflow { workflow, inputs, .. } => {
            let depth = sh.st.lock().unwrap().depth + 1;
            if depth > MAX_SUBWORKFLOW_DEPTH {
                return Err(Error::Invalid(format!("troppi sotto-workflow annidati: il massimo è {MAX_SUBWORKFLOW_DEPTH} livelli")));
            }
            let Value::Object(inputs) = render_value_ctx(&Value::Object(inputs.clone()), ctx) else {
                unreachable!("an object stays an object")
            };
            sh.host.run_subworkflow(run_id, workflow, inputs, depth).await
        }
        other => exec_leaf(other, run_id, ctx, sh.host).await,
    }
}

/// An inline step of a `foreach` or `parallel`, with its own retry and timeout.
async fn exec_inline(step: &WorkflowStep, run_id: &str, ctx: &RenderCtx<'_>, host: &dyn WorkflowHost) -> Result<String> {
    let opts = step.opts().cloned().unwrap_or_default();
    run_with_options(&opts, |_| {}, || exec_leaf(step, run_id, ctx, host)).await
}

/// One entry of a `foreach`, with `{{item}}` and `{{index}}` bound.
async fn foreach_entry<'a>(
    index: usize,
    item: &'a Value,
    body: &'a WorkflowStep,
    run_id: &'a str,
    ctx: &'a RenderCtx<'a>,
    host: &'a dyn WorkflowHost,
) -> (usize, Result<String>) {
    let ctx = RenderCtx { item: Some((item, index)), ..*ctx };
    (index, exec_inline(body, run_id, &ctx, host).await)
}

async fn parallel_member<'a>(
    step: &'a WorkflowStep,
    run_id: &'a str,
    ctx: &'a RenderCtx<'a>,
    host: &'a dyn WorkflowHost,
) -> (&'a WorkflowStep, Result<String>) {
    (step, exec_inline(step, run_id, ctx, host).await)
}

fn continues_on_error(step: &WorkflowStep) -> bool {
    matches!(step.opts(), Some(o) if o.on_error == OnError::Continue)
}

/// The entries of a `foreach`: a JSON array, or one entry per non-empty line.
fn parse_items(rendered: &str) -> Result<Vec<Value>> {
    let list = match parse_json_output(rendered) {
        Some(Value::Array(a)) => a,
        _ => rendered.lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| Value::String(l.to_string())).collect(),
    };
    if list.len() > MAX_FOREACH_ITEMS {
        return Err(Error::Invalid(format!("troppe voci ({}): il massimo è {MAX_FOREACH_ITEMS}", list.len())));
    }
    Ok(list)
}

async fn exec_foreach(
    sh: &Shared<'_>,
    i: usize,
    items: &str,
    body: &WorkflowStep,
    parallel: u32,
    run_id: &str,
    ctx: &RenderCtx<'_>,
) -> Result<String> {
    let list = parse_items(&render_ctx(items, ctx))?;
    let total = list.len();
    let keep_going = continues_on_error(body);
    let mut results: Vec<Option<String>> = vec![None; total];
    let mut done = 0usize;
    let limit = parallel.clamp(1, MAX_FOREACH_PARALLEL) as usize;
    let mut entries = list.iter().enumerate();
    let mut pending = FuturesUnordered::new();
    loop {
        while pending.len() < limit {
            let Some((index, item)) = entries.next() else { break };
            pending.push(foreach_entry(index, item, body, run_id, ctx, sh.host));
        }
        // The first failure stops the loop: dropping `pending` cancels the entries still running.
        let Some((index, result)) = pending.next().await else { break };
        match result {
            Ok(out) => results[index] = Some(out),
            Err(_) if keep_going => {}
            Err(e) => return Err(Error::Runtime(format!("voce {} di {total}: {e}", index + 1))),
        }
        done += 1;
        sh.publish(|st| st.run.steps[i].output = Some(format!("{done}/{total} completati")));
    }
    Ok(Value::Array(results.into_iter().map(|r| r.map_or(Value::Null, Value::String)).collect()).to_string())
}

async fn exec_parallel(sh: &Shared<'_>, i: usize, steps: &[WorkflowStep], run_id: &str, ctx: &RenderCtx<'_>) -> Result<String> {
    let total = steps.len();
    let mut pending = FuturesUnordered::new();
    for step in steps {
        pending.push(parallel_member(step, run_id, ctx, sh.host));
    }
    let mut outs: Vec<(String, std::result::Result<String, String>)> = Vec::new();
    while let Some((step, result)) = pending.next().await {
        match result {
            Ok(out) => outs.push((step.id().to_string(), Ok(out))),
            Err(e) if continues_on_error(step) => outs.push((step.id().to_string(), Err(e.to_string()))),
            Err(e) => return Err(Error::Runtime(format!("passo {}: {e}", step.id()))),
        }
        let done = outs.len();
        sh.publish(|st| st.run.steps[i].output = Some(format!("{done}/{total} completati")));
    }
    // Every inner output is also readable as {{steps.<inner id>.output}}.
    sh.publish(|st| {
        for (id, r) in &outs {
            match r {
                Ok(out) => {
                    st.outputs.insert(id.clone(), out.clone());
                }
                Err(e) => {
                    st.errors.insert(id.clone(), e.clone());
                }
            }
        }
    });
    let mut obj = Map::new();
    for step in steps {
        let found = outs.iter().find(|(id, _)| id == step.id());
        obj.insert(step.id().to_string(), found.and_then(|(_, r)| r.as_ref().ok()).map_or(Value::Null, |o| Value::String(o.clone())));
    }
    Ok(Value::Object(obj).to_string())
}

pub async fn run_workflow(wf: &WorkflowDef, inputs: Map<String, Value>, host: &dyn WorkflowHost) -> WorkflowRun {
    run_from(WorkflowState::new(wf, inputs), host).await
}

/// Runs (or goes on with) a workflow from `state`.
pub async fn run_from(state: WorkflowState, host: &dyn WorkflowHost) -> WorkflowRun {
    run_state(state, host).await.run
}

/// Like [`run_from`], but returns the whole final state (the outputs in full, not cut for display).
pub async fn run_state(state: WorkflowState, host: &dyn WorkflowHost) -> WorkflowState {
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
        let ctx = RenderCtx { inputs: &inputs, outputs: &outputs, errors: &errors, item: None };
        let result: Result<String> = match step {
            WorkflowStep::Tool { .. }
            | WorkflowStep::Agent { .. }
            | WorkflowStep::Llm { .. }
            | WorkflowStep::Foreach { .. }
            | WorkflowStep::Parallel { .. }
            | WorkflowStep::Workflow { .. } => {
                let opts = step.opts().cloned().unwrap_or_default();
                run_with_options(&opts, |n| sh.publish(|st| st.run.steps[i].attempts = Some(n)), || exec_body(&sh, i, step, &run_id, &ctx))
                    .await
            }
            WorkflowStep::Approval { message, timeout_s, on_timeout, .. } => {
                let outcome = host.approval(&run_id, &render_ctx(message, &ctx), *timeout_s).await;
                // Nobody answered in time: the default of the step is the answer.
                let (allowed, reason, output) = match (outcome, on_timeout) {
                    (ApprovalOutcome::Allowed, _) => (true, "", "approvato"),
                    (ApprovalOutcome::Denied, _) => (false, "rifiutato dall'utente", ""),
                    (ApprovalOutcome::TimedOut, ApprovalDefault::Allow) => (true, "", "scaduto: approvato"),
                    (ApprovalOutcome::TimedOut, ApprovalDefault::Deny) => (false, "scaduto: rifiutato", ""),
                };
                if allowed {
                    sh.publish(|st| st.run.status = "running".into());
                    Ok(output.into())
                } else {
                    sh.publish(|st| {
                        st.run.steps[i].status = "failed".into();
                        st.run.steps[i].error = Some(reason.into());
                        st.run.status = "cancelled".into();
                        st.run.finished_at = Some(now_rfc3339());
                    });
                    return sh.st.into_inner().unwrap();
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
                return sh.st.into_inner().unwrap();
            }
        }
    }
    sh.publish(|st| {
        st.run.status = "succeeded".into();
        st.run.finished_at = Some(now_rfc3339());
    });
    sh.st.into_inner().unwrap()
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
        timed_out: bool,
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
        async fn approval(&self, _r: &str, _m: &str, _t: Option<u64>) -> ApprovalOutcome {
            if self.timed_out {
                ApprovalOutcome::TimedOut
            } else if self.approve {
                ApprovalOutcome::Allowed
            } else {
                ApprovalOutcome::Denied
            }
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
        let host = Host { approve: true, timed_out: false, calls: Mutex::new(vec![]) };
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
        let host = Host { approve: false, timed_out: false, calls: Mutex::new(vec![]) };
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
        async fn approval(&self, _r: &str, _m: &str, _t: Option<u64>) -> ApprovalOutcome {
            ApprovalOutcome::Allowed
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
        async fn approval(&self, _r: &str, _m: &str, _t: Option<u64>) -> ApprovalOutcome {
            ApprovalOutcome::Allowed
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

    fn approval_wf(extra: Value) -> WorkflowDef {
        let mut step = json!({"id": "ok", "kind": "approval", "message": "Procedo?", "timeout_s": 30});
        step.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(json!({
            "id": "w", "name": "W", "trigger": {"kind": "manual"},
            "steps": [step, {"id": "after", "kind": "tool", "tool": "list_dir"}]
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn approval_timeout_takes_the_default_answer() {
        let host = Host { approve: false, timed_out: true, calls: Mutex::new(vec![]) };
        // Default: deny. The run is cancelled like after a refusal.
        let run = run_workflow(&approval_wf(json!({})), Map::new(), &host).await;
        assert_eq!(run.status, "cancelled");
        assert_eq!(run.steps[0].status, "failed");
        assert_eq!(run.steps[0].error.as_deref(), Some("scaduto: rifiutato"));
        assert_eq!(run.steps[1].status, "pending");
        // default: allow. The run goes on and the output says what happened.
        let run = run_workflow(&approval_wf(json!({"default": "allow"})), Map::new(), &host).await;
        assert_eq!(run.status, "succeeded");
        assert_eq!(run.steps[0].output.as_deref(), Some("scaduto: approvato"));
        assert_eq!(run.steps[1].status, "done");
    }

    #[test]
    fn approval_options_are_optional_and_round_trip() {
        let plain: WorkflowStep = serde_json::from_value(json!({"id": "ok", "kind": "approval", "message": "?"})).unwrap();
        assert_eq!(serde_json::to_value(&plain).unwrap(), json!({"kind": "approval", "id": "ok", "message": "?"}));
        let timed = approval_wf(json!({"default": "allow"})).steps.remove(0);
        assert_eq!(
            serde_json::to_value(&timed).unwrap(),
            json!({"kind": "approval", "id": "ok", "message": "Procedo?", "timeout_s": 30, "default": "allow"})
        );
    }

    /// An `llm` host that answers `echo:<prompt>` after a pause and records how many calls overlap.
    #[derive(Default)]
    struct Overlap {
        now: std::sync::atomic::AtomicUsize,
        peak: std::sync::atomic::AtomicUsize,
    }
    #[async_trait]
    impl WorkflowHost for Overlap {
        async fn call_tool(&self, _r: &str, _t: &str, _a: Value) -> Result<String> {
            Ok("tool".into())
        }
        async fn run_agent(&self, _r: &str, _a: &str, _p: &str) -> Result<String> {
            Ok("agent".into())
        }
        async fn complete(&self, _r: &str, _p: &str, prompt: &str) -> Result<String> {
            use std::sync::atomic::Ordering::SeqCst;
            let n = self.now.fetch_add(1, SeqCst) + 1;
            self.peak.fetch_max(n, SeqCst);
            tokio::time::sleep(Duration::from_millis(if prompt.contains("slow") { 500 } else { 50 })).await;
            self.now.fetch_sub(1, SeqCst);
            if prompt == "bad" {
                return Err(Error::Runtime(format!("rifiutato: {prompt}")));
            }
            Ok(format!("echo:{prompt}"))
        }
        async fn approval(&self, _r: &str, _m: &str, _t: Option<u64>) -> ApprovalOutcome {
            ApprovalOutcome::Allowed
        }
        fn update(&self, _run: &WorkflowRun) {}
    }

    fn wf_of(steps: Value) -> WorkflowDef {
        serde_json::from_value(
            json!({"id": "w", "name": "W", "trigger": {"kind": "manual"}, "inputs": {"list": "uno\n\ndue\ntre\n"}, "steps": steps}),
        )
        .unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn foreach_runs_every_entry_in_order_with_limited_concurrency() {
        let wf = wf_of(json!([
            {"id": "src", "kind": "tool", "tool": "list"},
            {"id": "each", "kind": "foreach", "items": "[{\"name\": \"a\"}, {\"name\": \"b\"}, {\"name\": \"c\"}, {\"name\": \"d\"}]", "parallel": 3,
             "do": {"id": "one", "kind": "llm", "profile": "p", "prompt": "{{index}}={{item.name}}/{{item}}"}},
            {"id": "after", "kind": "llm", "profile": "p", "prompt": "{{steps.each.output}}"}
        ]));
        let host = Overlap::default();
        let run = run_workflow(&wf, Map::new(), &host).await;
        assert_eq!(run.status, "succeeded", "{run:?}");
        let out: Vec<String> = serde_json::from_str(run.steps[1].output.as_deref().unwrap()).unwrap();
        assert_eq!(
            out,
            vec![r#"echo:0=a/{"name":"a"}"#, r#"echo:1=b/{"name":"b"}"#, r#"echo:2=c/{"name":"c"}"#, r#"echo:3=d/{"name":"d"}"#]
        );
        assert_eq!(host.peak.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert!(run.steps[2].output.as_deref().unwrap().starts_with("echo:[\"echo:0=a"));
    }

    #[tokio::test(start_paused = true)]
    async fn foreach_takes_one_entry_per_line_and_defaults_to_one_at_a_time() {
        let wf = wf_of(json!([
            {"id": "each", "kind": "foreach", "items": "{{inputs.list}}", "do": {"id": "one", "kind": "llm", "profile": "p", "prompt": "{{item}}"}}
        ]));
        let host = Overlap::default();
        let run = run_workflow(&wf, Map::new(), &host).await;
        assert_eq!(run.steps[0].output.as_deref(), Some(r#"["echo:uno","echo:due","echo:tre"]"#));
        assert_eq!(host.peak.load(std::sync::atomic::Ordering::SeqCst), 1);
        // The output of a step that contains a ```json block works too, and an empty list is fine.
        let wf = wf_of(json!([
            {"id": "src", "kind": "tool", "tool": "list"},
            {"id": "each", "kind": "foreach", "items": "", "do": {"id": "one", "kind": "llm", "profile": "p", "prompt": "{{item}}"}}
        ]));
        let run = run_workflow(&wf, Map::new(), &host).await;
        assert_eq!(run.steps[1].output.as_deref(), Some("[]"));
    }

    #[tokio::test(start_paused = true)]
    async fn foreach_stops_at_the_first_error_unless_the_inner_step_continues() {
        let items = r#"["ok", "bad", "slow"]"#;
        let wf = wf_of(
            json!([{"id": "each", "kind": "foreach", "items": items, "parallel": 2, "do": {"id": "one", "kind": "llm", "profile": "p", "prompt": "{{item}}"}}]),
        );
        let run = run_workflow(&wf, Map::new(), &Overlap::default()).await;
        assert_eq!(run.status, "failed");
        assert!(run.steps[0].error.as_deref().unwrap().contains("voce 2 di 3"), "{run:?}");

        let wf = wf_of(
            json!([{"id": "each", "kind": "foreach", "items": items, "do": {"id": "one", "kind": "llm", "profile": "p", "prompt": "{{item}}", "on_error": "continue"}}]),
        );
        let run = run_workflow(&wf, Map::new(), &Overlap::default()).await;
        assert_eq!(run.status, "succeeded");
        assert_eq!(run.steps[0].output.as_deref(), Some(r#"["echo:ok",null,"echo:slow"]"#));
    }

    #[tokio::test]
    async fn foreach_refuses_too_many_entries() {
        let items: Vec<String> = (0..201).map(|n| n.to_string()).collect();
        let wf = wf_of(
            json!([{"id": "each", "kind": "foreach", "items": items.join("\n"), "do": {"id": "one", "kind": "llm", "profile": "p", "prompt": "{{item}}"}}]),
        );
        let run = run_workflow(&wf, Map::new(), &Overlap::default()).await;
        assert_eq!(run.status, "failed");
        assert!(run.steps[0].error.as_deref().unwrap().contains("troppe voci"));
    }

    #[tokio::test(start_paused = true)]
    async fn parallel_runs_together_and_exposes_the_inner_outputs() {
        let wf = wf_of(json!([
            {"id": "par", "kind": "parallel", "steps": [
                {"id": "x", "kind": "llm", "profile": "p", "prompt": "uno"},
                {"id": "y", "kind": "llm", "profile": "p", "prompt": "due"},
                {"id": "z", "kind": "tool", "tool": "t"}
            ]},
            {"id": "after", "kind": "llm", "profile": "p", "prompt": "{{steps.x.output}}+{{steps.y.output}}+{{steps.z.output}}"}
        ]));
        let host = Overlap::default();
        let run = run_workflow(&wf, Map::new(), &host).await;
        assert_eq!(run.status, "succeeded", "{run:?}");
        let out: Value = serde_json::from_str(run.steps[0].output.as_deref().unwrap()).unwrap();
        assert_eq!(out, json!({"x": "echo:uno", "y": "echo:due", "z": "tool"}));
        assert_eq!(host.peak.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(run.steps[1].output.as_deref(), Some("echo:echo:uno+echo:due+tool"));
    }

    #[tokio::test(start_paused = true)]
    async fn parallel_fails_on_the_first_error_and_can_continue() {
        let steps = |on_error: &str| {
            wf_of(json!([{"id": "par", "kind": "parallel", "steps": [
                {"id": "x", "kind": "llm", "profile": "p", "prompt": "slow"},
                {"id": "y", "kind": "llm", "profile": "p", "prompt": "bad", "on_error": on_error}
            ]}, {"id": "after", "kind": "llm", "profile": "p", "prompt": "[{{steps.y.error}}]"}]))
        };
        let run = run_workflow(&steps("fail"), Map::new(), &Overlap::default()).await;
        assert_eq!(run.status, "failed");
        assert!(run.steps[0].error.as_deref().unwrap().contains("passo y"));
        let run = run_workflow(&steps("continue"), Map::new(), &Overlap::default()).await;
        assert_eq!(run.status, "succeeded");
        assert_eq!(run.steps[0].output.as_deref(), Some(r#"{"x":"echo:slow","y":null}"#));
        assert!(run.steps[1].output.as_deref().unwrap().contains("rifiutato: bad"));
    }

    /// A host that knows a set of workflows and runs them as children.
    struct Tree {
        defs: HashMap<String, WorkflowDef>,
        children: Mutex<Vec<WorkflowRun>>,
    }
    #[async_trait]
    impl WorkflowHost for Tree {
        async fn call_tool(&self, _r: &str, tool: &str, args: Value) -> Result<String> {
            if tool == "boom" {
                return Err(Error::Runtime("esploso".into()));
            }
            Ok(format!("{tool}:{}", args.get("x").and_then(Value::as_str).unwrap_or("-")))
        }
        async fn run_agent(&self, _r: &str, _a: &str, _p: &str) -> Result<String> {
            Ok("agent".into())
        }
        async fn complete(&self, _r: &str, _p: &str, prompt: &str) -> Result<String> {
            Ok(format!("llm:{prompt}"))
        }
        async fn approval(&self, _r: &str, _m: &str, _t: Option<u64>) -> ApprovalOutcome {
            ApprovalOutcome::Allowed
        }
        fn update(&self, _run: &WorkflowRun) {}
        async fn run_subworkflow(&self, parent: &str, id: &str, inputs: Map<String, Value>, depth: u32) -> Result<String> {
            let def = self.defs.get(id).ok_or_else(|| Error::NotFound(format!("il sotto-workflow {id} non esiste")))?;
            let done = run_state(WorkflowState::new(def, inputs).child_of(parent, depth), self).await;
            self.children.lock().unwrap().push(done.run.clone());
            match done.run.status.as_str() {
                "succeeded" => Ok(done.outputs.get("last").cloned().unwrap_or_default()),
                _ => Err(Error::Runtime(format!("fallito: {}", done.run.steps.iter().find_map(|s| s.error.clone()).unwrap_or_default()))),
            }
        }
    }

    fn tree(defs: Value) -> Tree {
        let defs: Vec<WorkflowDef> = serde_json::from_value(defs).unwrap();
        Tree { defs: defs.into_iter().map(|d| (d.id.clone(), d)).collect(), children: Mutex::new(vec![]) }
    }

    fn def(id: &str, steps: Value) -> Value {
        json!({"id": id, "name": id, "enabled": false, "trigger": {"kind": "manual"}, "steps": steps})
    }

    #[tokio::test]
    async fn a_subworkflow_gets_its_inputs_and_hands_back_its_output() {
        let host = tree(json!([
            def(
                "parent",
                json!([
                    {"id": "first", "kind": "tool", "tool": "scan", "args": {"x": "uno"}},
                    {"id": "sub", "kind": "workflow", "workflow": "child", "inputs": {"n": "{{steps.first.output}}", "fisso": 3}},
                    {"id": "after", "kind": "llm", "profile": "p", "prompt": "ricevuto {{steps.sub.output}}"}
                ])
            ),
            def(
                "child",
                json!([
                    {"id": "mid", "kind": "tool", "tool": "work", "args": {"x": "{{inputs.n}}"}},
                    {"id": "last", "kind": "llm", "profile": "p", "prompt": "{{steps.mid.output}} {{inputs.fisso}}"},
                    {"id": "c", "kind": "condition", "if": "x", "then": []}
                ])
            ),
        ]));
        let run = run_workflow(&host.defs["parent"], Map::new(), &host).await;
        assert_eq!(run.status, "succeeded", "{run:?}");
        assert_eq!(run.steps[2].output.as_deref(), Some("llm:ricevuto llm:work:scan:uno 3"));
        let children = host.children.lock().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].parent_run_id.as_deref(), Some(run.id.as_str()));
        assert!(run.parent_run_id.is_none());
    }

    #[tokio::test]
    async fn a_failing_subworkflow_fails_the_step_unless_it_continues() {
        let defs = |on_error: &str| {
            tree(json!([
                def(
                    "parent",
                    json!([
                        {"id": "sub", "kind": "workflow", "workflow": "child", "on_error": on_error},
                        {"id": "after", "kind": "llm", "profile": "p", "prompt": "[{{steps.sub.error}}]"}
                    ])
                ),
                def("child", json!([{"id": "bad", "kind": "tool", "tool": "boom"}])),
            ]))
        };
        let host = defs("fail");
        let run = run_workflow(&host.defs["parent"], Map::new(), &host).await;
        assert_eq!(run.status, "failed");
        assert!(run.steps[0].error.as_deref().unwrap().contains("esploso"));
        let host = defs("continue");
        let run = run_workflow(&host.defs["parent"], Map::new(), &host).await;
        assert_eq!(run.status, "succeeded");
        assert!(run.steps[1].output.as_deref().unwrap().contains("esploso"));
        // A workflow that does not exist is an error of the step as well.
        let host = tree(json!([def("parent", json!([{"id": "sub", "kind": "workflow", "workflow": "nessuno"}]))]));
        let run = run_workflow(&host.defs["parent"], Map::new(), &host).await;
        assert!(run.steps[0].error.as_deref().unwrap().contains("non esiste"));
    }

    #[tokio::test]
    async fn indirect_cycles_stop_at_the_depth_limit() {
        let host = tree(json!([
            def("a", json!([{"id": "go", "kind": "workflow", "workflow": "b"}])),
            def("b", json!([{"id": "go", "kind": "workflow", "workflow": "a"}])),
        ]));
        let run = run_workflow(&host.defs["a"], Map::new(), &host).await;
        assert_eq!(run.status, "failed");
        assert!(run.steps[0].error.as_deref().unwrap().contains("troppi sotto-workflow"), "{run:?}");
        // Levels 1, 2 and 3 were started; the fourth was refused.
        assert_eq!(host.children.lock().unwrap().len(), 3);
    }

    #[test]
    fn json_placeholders_read_the_output_of_a_step() {
        let inputs = Map::new();
        let outputs = HashMap::from([
            ("list".to_string(), r#"{"items":[{"nome":"a"},{"nome":"b"}],"n":2,"ok":true,"s":"testo","nulla":null}"#.to_string()),
            ("fenced".to_string(), "Ecco il risultato:\n```json\n{\"a\": {\"b\": 5}}\n```\nfine".to_string()),
            ("plain".to_string(), "solo testo".to_string()),
            ("arr".to_string(), "[1, 2, 3]".to_string()),
        ]);
        let errors = HashMap::from([("bad".to_string(), "esploso".to_string())]);
        let r = |t: &str| render_ctx(t, &RenderCtx { inputs: &inputs, outputs: &outputs, errors: &errors, item: None });
        // The whole output as compact JSON.
        assert_eq!(r("{{steps.list.json}}"), r#"{"items":[{"nome":"a"},{"nome":"b"}],"n":2,"nulla":null,"ok":true,"s":"testo"}"#);
        assert_eq!(r("{{steps.arr.json}}"), "[1,2,3]");
        // Paths: objects by key, arrays by number.
        assert_eq!(r("{{steps.list.json.items.1.nome}}"), "b");
        assert_eq!(r("{{ steps.list.json.items.0 }}"), r#"{"nome":"a"}"#);
        assert_eq!(r("{{steps.arr.json.2}}"), "3");
        // Strings come out without quotes, everything else as JSON.
        assert_eq!(r("{{steps.list.json.s}}|{{steps.list.json.n}}|{{steps.list.json.ok}}|{{steps.list.json.nulla}}"), "testo|2|true|null");
        // A ```json block counts.
        assert_eq!(r("{{steps.fenced.json.a.b}}"), "5");
        // Anything else stays empty: not JSON, missing key, index out of range, unknown step.
        for t in [
            "{{steps.plain.json}}",
            "{{steps.plain.json.x}}",
            "{{steps.list.json.nope}}",
            "{{steps.list.json.items.9}}",
            "{{steps.arr.json.a}}",
            "{{steps.zzz.json}}",
            "{{steps.list.nonsense}}",
        ] {
            assert_eq!(r(t), "", "{t}");
        }
        // The plain forms keep working, and the error of a step that continued is there.
        assert_eq!(r("{{steps.plain.output}}/{{steps.plain}}/{{steps.bad.error}}/{{steps.list.error}}"), "solo testo/solo testo/esploso/");
        // A condition sees the rendered text.
        assert!(evaluate_condition(&r("{{steps.list.json.ok}} == true")));
        assert!(evaluate_condition(&r("{{steps.list.json.items.0.nome}} == a")));
        assert!(!evaluate_condition(&r("{{steps.list.json.nope}}")));
        // The loop placeholders.
        let item = json!({"nome": "x", "tags": ["t1", "t2"]});
        let r = |t: &str| render_ctx(t, &RenderCtx { inputs: &inputs, outputs: &outputs, errors: &errors, item: Some((&item, 4)) });
        assert_eq!(r("{{index}} {{item.nome}} {{item.tags.1}} {{item.manca}} {{item}}"), r#"4 x t2  {"nome":"x","tags":["t1","t2"]}"#);
    }
}
