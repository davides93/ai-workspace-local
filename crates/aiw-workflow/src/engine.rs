use aiw_core::api::{WorkflowDef, WorkflowRun, WorkflowStep, WorkflowStepRun};
use aiw_core::{ids::new_id, now_rfc3339, Result};
use async_trait::async_trait;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// Services a workflow needs from the engine (policy and approvals apply as for agents).
#[async_trait]
pub trait WorkflowHost: Send + Sync {
    async fn call_tool(&self, run_id: &str, tool: &str, args: Value) -> Result<String>;
    async fn run_agent(&self, run_id: &str, agent: &str, prompt: &str) -> Result<String>;
    async fn complete(&self, run_id: &str, profile: &str, prompt: &str) -> Result<String>;
    async fn approval(&self, run_id: &str, message: &str) -> bool;
    fn update(&self, run: &WorkflowRun);
}

/// Replaces `{{inputs.x}}` and `{{steps.id.output}}` placeholders.
pub fn render(template: &str, inputs: &Map<String, Value>, outputs: &HashMap<String, String>) -> String {
    let re = regex::Regex::new(r"\{\{\s*([a-zA-Z0-9_.\-]+)\s*\}\}").unwrap();
    re.replace_all(template, |c: &regex::Captures| {
        let path = &c[1];
        if let Some(k) = path.strip_prefix("inputs.") {
            return inputs.get(k).map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
        }
        if let Some(rest) = path.strip_prefix("steps.") {
            let id = rest.trim_end_matches(".output");
            return outputs.get(id).cloned().unwrap_or_default();
        }
        if path == "now" {
            return chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        }
        c[0].to_string()
    })
    .to_string()
}

pub fn render_value(v: &Value, inputs: &Map<String, Value>, outputs: &HashMap<String, String>) -> Value {
    match v {
        Value::String(s) => Value::String(render(s, inputs, outputs)),
        Value::Array(a) => Value::Array(a.iter().map(|x| render_value(x, inputs, outputs)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), render_value(x, inputs, outputs))).collect()),
        other => other.clone(),
    }
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

pub async fn run_workflow(wf: &WorkflowDef, inputs: Map<String, Value>, host: &dyn WorkflowHost) -> WorkflowRun {
    let mut run = WorkflowRun {
        id: new_id("wfr"),
        workflow_id: wf.id.clone(),
        status: "running".into(),
        started_at: now_rfc3339(),
        finished_at: None,
        steps: wf
            .steps
            .iter()
            .map(|s| WorkflowStepRun { id: s.id().to_string(), status: "pending".into(), output: None, error: None })
            .collect(),
    };
    let mut merged = wf.inputs.clone();
    merged.extend(inputs);
    host.update(&run);
    let gated: HashSet<String> = wf
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
    let mut enabled: HashSet<String> = HashSet::new();
    let mut outputs: HashMap<String, String> = HashMap::new();
    let run_id = run.id.clone();
    for (i, step) in wf.steps.iter().enumerate() {
        let id = step.id().to_string();
        if gated.contains(&id) && !enabled.contains(&id) {
            run.steps[i].status = "skipped".into();
            host.update(&run);
            continue;
        }
        run.steps[i].status = if matches!(step, WorkflowStep::Approval { .. }) { "waiting".into() } else { "running".into() };
        if matches!(step, WorkflowStep::Approval { .. }) {
            run.status = "waiting_approval".into();
        }
        host.update(&run);
        let result: Result<String> = match step {
            WorkflowStep::Tool { tool, args, .. } => {
                host.call_tool(&run_id, tool, render_value(&Value::Object(args.clone()), &merged, &outputs)).await
            }
            WorkflowStep::Agent { agent, prompt, .. } => host.run_agent(&run_id, agent, &render(prompt, &merged, &outputs)).await,
            WorkflowStep::Llm { profile, prompt, .. } => host.complete(&run_id, profile, &render(prompt, &merged, &outputs)).await,
            WorkflowStep::Approval { message, .. } => {
                let ok = host.approval(&run_id, &render(message, &merged, &outputs)).await;
                run.status = "running".into();
                if ok {
                    Ok("approvato".into())
                } else {
                    run.steps[i].status = "failed".into();
                    run.steps[i].error = Some("rifiutato dall'utente".into());
                    run.status = "cancelled".into();
                    run.finished_at = Some(now_rfc3339());
                    host.update(&run);
                    return run;
                }
            }
            WorkflowStep::Condition { condition, then, otherwise, .. } => {
                let rendered = render(condition, &merged, &outputs);
                let yes = evaluate_condition(&rendered);
                let chosen = if yes { then.clone() } else { otherwise.clone().unwrap_or_default() };
                enabled.extend(chosen.iter().cloned());
                Ok(format!(
                    "{} → {}",
                    if yes { "vero" } else { "falso" },
                    if chosen.is_empty() { "nessun passo".into() } else { chosen.join(", ") }
                ))
            }
        };
        match result {
            Ok(out) => {
                run.steps[i].status = "done".into();
                run.steps[i].output = Some(out.chars().take(8000).collect());
                outputs.insert(id, out);
            }
            Err(e) => {
                run.steps[i].status = "failed".into();
                run.steps[i].error = Some(e.to_string());
                run.status = "failed".into();
                run.finished_at = Some(now_rfc3339());
                host.update(&run);
                return run;
            }
        }
        host.update(&run);
    }
    run.status = "succeeded".into();
    run.finished_at = Some(now_rfc3339());
    host.update(&run);
    run
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
}
