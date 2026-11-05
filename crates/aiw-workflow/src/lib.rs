//! Workflow layer (architecture §5.3).

pub mod engine;
pub mod schedule;

pub use engine::{
    evaluate_condition, render, render_ctx, render_value, render_value_ctx, run_from, run_workflow, RenderCtx, WorkflowHost, WorkflowState,
};
pub use schedule::{cron_to_six_fields, Scheduler};

use aiw_core::api::{StepOptions, WorkflowDef, MAX_RETRY_DELAY_S, MAX_STEP_RETRY, MAX_STEP_TIMEOUT_S};
use aiw_core::{Error, Result};
use std::path::Path;

/// Loads a workflow from a `.json`, `.yaml` or `.yml` file.
pub fn load_file(path: &Path) -> Result<WorkflowDef> {
    let text = std::fs::read_to_string(path)?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let wf: WorkflowDef = if ext == "json" {
        serde_json::from_str(&text)?
    } else {
        serde_yaml::from_str(&text).map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))?
    };
    validate(&wf)?;
    Ok(wf)
}

pub fn validate(wf: &WorkflowDef) -> Result<()> {
    if wf.id.trim().is_empty() || wf.name.trim().is_empty() {
        return Err(Error::Invalid("il workflow deve avere id e nome".into()));
    }
    let mut seen = std::collections::HashSet::new();
    for s in &wf.steps {
        if !seen.insert(s.id().to_string()) {
            return Err(Error::Invalid(format!("id di passo duplicato: {}", s.id())));
        }
    }
    for s in &wf.steps {
        if let Some(o) = s.opts() {
            check_options(s.id(), o)?;
        }
    }
    for s in &wf.steps {
        if let aiw_core::api::WorkflowStep::Condition { then, otherwise, .. } = s {
            for id in then.iter().chain(otherwise.iter().flatten()) {
                if !seen.contains(id) {
                    return Err(Error::Invalid(format!("la condizione {} punta a un passo inesistente: {id}", s.id())));
                }
            }
        }
    }
    if let aiw_core::api::WorkflowTrigger::Cron { expr } = &wf.trigger {
        schedule::parse_cron(expr)?;
    }
    Ok(())
}

fn check_options(id: &str, o: &StepOptions) -> Result<()> {
    if o.retry > MAX_STEP_RETRY {
        return Err(Error::Invalid(format!("il passo {id}: retry al massimo {MAX_STEP_RETRY}")));
    }
    if o.retry_delay_s > MAX_RETRY_DELAY_S {
        return Err(Error::Invalid(format!("il passo {id}: retry_delay_s al massimo {MAX_RETRY_DELAY_S}")));
    }
    if matches!(o.timeout_s, Some(t) if t == 0 || t > MAX_STEP_TIMEOUT_S) {
        return Err(Error::Invalid(format!("il passo {id}: timeout_s deve essere tra 1 e {MAX_STEP_TIMEOUT_S}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_yaml_workflows() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("digest.yaml");
        std::fs::write(
            &p,
            r#"
id: digest
name: Digest download
enabled: true
trigger: { kind: cron, expr: "0 8 * * 1-5" }
inputs: { folder: "~/Downloads" }
steps:
  - { id: scan, kind: tool, tool: list_dir, args: { path: "{{inputs.folder}}" } }
  - { id: ok, kind: approval, message: "Procedo?" }
  - { id: act, kind: agent, agent: operator, prompt: "Sposta: {{steps.scan.output}}" }
"#,
        )
        .unwrap();
        let wf = load_file(&p).unwrap();
        assert_eq!(wf.steps.len(), 3);
        assert!(validate(&wf).is_ok());
    }

    #[test]
    fn validates_step_options() {
        let mk = |extra: &str| -> WorkflowDef {
            serde_yaml::from_str(&format!(
                "id: w\nname: W\ntrigger: {{ kind: manual }}\nsteps:\n  - {{ id: a, kind: llm, profile: p, prompt: x{extra} }}\n"
            ))
            .unwrap()
        };
        assert!(validate(&mk("")).is_ok());
        assert!(validate(&mk(", retry: 5, retry_delay_s: 300, timeout_s: 86400, on_error: continue")).is_ok());
        assert!(validate(&mk(", retry: 6")).unwrap_err().to_string().contains("retry"));
        assert!(validate(&mk(", retry_delay_s: 301")).is_err());
        assert!(validate(&mk(", timeout_s: 0")).is_err());
        assert!(validate(&mk(", timeout_s: 86401")).is_err());
    }

    #[test]
    fn default_options_are_not_serialized() {
        let wf = load_str_for_test("{ id: a, kind: tool, tool: list_dir }");
        let out = serde_json::to_value(&wf.steps[0]).unwrap();
        assert_eq!(out, serde_json::json!({"kind": "tool", "id": "a", "tool": "list_dir", "args": {}}));
        let wf = load_str_for_test("{ id: a, kind: tool, tool: list_dir, retry: 2, on_error: continue }");
        let out = serde_json::to_value(&wf.steps[0]).unwrap();
        assert_eq!(out["retry"], 2);
        assert_eq!(out["on_error"], "continue");
        assert!(out.get("retry_delay_s").is_none() && out.get("timeout_s").is_none());
    }

    fn load_str_for_test(step: &str) -> WorkflowDef {
        serde_yaml::from_str(&format!("id: w\nname: W\ntrigger: {{ kind: manual }}\nsteps:\n  - {step}\n")).unwrap()
    }
}
