//! Workflow layer (architecture §5.3).

pub mod engine;
pub mod schedule;

pub use engine::{evaluate_condition, render, render_value, run_workflow, WorkflowHost};
pub use schedule::{cron_to_six_fields, Scheduler};

use aiw_core::api::WorkflowDef;
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
}
