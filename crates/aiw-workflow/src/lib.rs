//! Workflow layer (architecture §5.3).

pub mod engine;
pub mod schedule;

pub use engine::{
    evaluate_condition, render, render_ctx, render_value, render_value_ctx, run_from, run_state, run_workflow, RenderCtx, WorkflowHost,
    WorkflowState,
};
pub use schedule::{cron_to_six_fields, Scheduler};

use aiw_core::api::{StepOptions, WorkflowDef, WorkflowStep, MAX_FOREACH_PARALLEL, MAX_RETRY_DELAY_S, MAX_STEP_RETRY, MAX_STEP_TIMEOUT_S};
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
    let mut top_level = std::collections::HashSet::new();
    for s in &wf.steps {
        if !seen.insert(s.id().to_string()) {
            return Err(Error::Invalid(format!("id di passo duplicato: {}", s.id())));
        }
        top_level.insert(s.id().to_string());
        // The inline steps of foreach/parallel count for the uniqueness of the ids.
        for inner in s.inner() {
            if !matches!(
                inner,
                WorkflowStep::Tool { .. } | WorkflowStep::Agent { .. } | WorkflowStep::Team { .. } | WorkflowStep::Llm { .. }
            ) {
                return Err(Error::Invalid(format!(
                    "il passo {}: i passi interni possono essere solo tool, agent, team o llm (niente annidamento, approval o condition); {} non va bene",
                    s.id(),
                    inner.id()
                )));
            }
            if !seen.insert(inner.id().to_string()) {
                return Err(Error::Invalid(format!("id di passo duplicato: {}", inner.id())));
            }
            check_team_step(inner)?;
            if let Some(o) = inner.opts() {
                check_options(inner.id(), o)?;
            }
        }
        check_team_step(s)?;
        match s {
            WorkflowStep::Workflow { workflow, .. } if workflow.trim().is_empty() => {
                return Err(Error::Invalid(format!("il passo {}: indica il workflow da avviare", s.id())));
            }
            WorkflowStep::Workflow { workflow, .. } if *workflow == wf.id => {
                return Err(Error::Invalid(format!("il passo {}: un workflow non può richiamare se stesso", s.id())));
            }
            WorkflowStep::Foreach { parallel, .. } if *parallel == 0 || *parallel > MAX_FOREACH_PARALLEL => {
                return Err(Error::Invalid(format!("il passo {}: parallel deve essere tra 1 e {MAX_FOREACH_PARALLEL}", s.id())));
            }
            WorkflowStep::Parallel { steps, .. } if steps.is_empty() => {
                return Err(Error::Invalid(format!("il passo {}: serve almeno un passo interno", s.id())));
            }
            _ => {}
        }
    }
    for s in &wf.steps {
        if let Some(o) = s.opts() {
            check_options(s.id(), o)?;
        }
        if let WorkflowStep::Approval { timeout_s: Some(t), .. } = s {
            if *t == 0 || *t > MAX_STEP_TIMEOUT_S {
                return Err(Error::Invalid(format!("il passo {}: timeout_s deve essere tra 1 e {MAX_STEP_TIMEOUT_S}", s.id())));
            }
        }
    }
    for s in &wf.steps {
        if let WorkflowStep::Condition { then, otherwise, .. } = s {
            for id in then.iter().chain(otherwise.iter().flatten()) {
                if !seen.contains(id) {
                    return Err(Error::Invalid(format!("la condizione {} punta a un passo inesistente: {id}", s.id())));
                }
                if !top_level.contains(id) {
                    return Err(Error::Invalid(format!(
                        "la condizione {} punta a {id}, che è un passo interno di foreach/parallel",
                        s.id()
                    )));
                }
            }
        }
    }
    if let aiw_core::api::WorkflowTrigger::Cron { expr } = &wf.trigger {
        schedule::parse_cron(expr)?;
    }
    Ok(())
}

/// A `team` step names the team to run; whether it exists is checked when the step runs, like for agents.
fn check_team_step(s: &WorkflowStep) -> Result<()> {
    match s {
        WorkflowStep::Team { team, .. } if team.trim().is_empty() => {
            Err(Error::Invalid(format!("il passo {}: indica il team da avviare", s.id())))
        }
        _ => Ok(()),
    }
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
    fn a_team_step_loads_from_yaml_and_needs_a_team() {
        let wf = |step: &str| -> WorkflowDef {
            serde_yaml::from_str(&format!("id: w\nname: W\ntrigger: {{ kind: manual }}\nsteps:\n  - {step}\n")).unwrap()
        };
        let ok = wf("{ id: a, kind: team, team: squadra, prompt: \"Studia\", retry: 1, timeout_s: 600 }");
        assert!(validate(&ok).is_ok());
        assert!(matches!(&ok.steps[0], WorkflowStep::Team { team, opts, .. } if team == "squadra" && opts.retry == 1));
        // A blank team is refused, also when the step is inline in a parallel or foreach.
        let blank = validate(&wf("{ id: a, kind: team, team: \"  \", prompt: x }")).unwrap_err().to_string();
        assert!(blank.contains("indica il team"), "{blank}");
        assert!(validate(&wf("{ id: p, kind: parallel, steps: [{ id: a, kind: team, team: squadra, prompt: x }] }")).is_ok());
        assert!(validate(&wf("{ id: p, kind: parallel, steps: [{ id: a, kind: team, team: \"\", prompt: x }] }")).is_err());
        assert!(validate(&wf("{ id: f, kind: foreach, items: \"[1]\", do: { id: a, kind: team, team: squadra, prompt: x } }")).is_ok());
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

    #[test]
    fn validates_the_timeout_of_an_approval() {
        let mk = |t: &str| load_str_for_test(&format!("{{ id: a, kind: approval, message: ok, timeout_s: {t} }}"));
        assert!(validate(&mk("60")).is_ok());
        assert!(validate(&mk("0")).is_err());
        assert!(validate(&mk("86401")).is_err());
    }

    #[test]
    fn validates_foreach_and_parallel() {
        let wf = |steps: &str| -> WorkflowDef {
            serde_yaml::from_str(&format!("id: w\nname: W\ntrigger: {{ kind: manual }}\nsteps:\n{steps}")).unwrap()
        };
        let ok = wf("  - { id: f, kind: foreach, items: '[1]', parallel: 8, do: { id: d, kind: llm, profile: p, prompt: x } }\n  - { id: p, kind: parallel, steps: [ { id: a, kind: tool, tool: t }, { id: b, kind: agent, agent: g, prompt: y } ] }\n");
        assert!(validate(&ok).is_ok());
        let err = |steps: &str| validate(&wf(steps)).unwrap_err().to_string();
        assert!(err("  - { id: f, kind: foreach, items: x, parallel: 9, do: { id: d, kind: llm, profile: p, prompt: x } }\n")
            .contains("parallel"));
        assert!(err("  - { id: f, kind: foreach, items: x, parallel: 0, do: { id: d, kind: llm, profile: p, prompt: x } }\n")
            .contains("parallel"));
        assert!(
            err("  - { id: f, kind: foreach, items: x, do: { id: d, kind: approval, message: x } }\n").contains("tool, agent, team o llm")
        );
        assert!(err(
            "  - { id: f, kind: foreach, items: x, do: { id: d, kind: foreach, items: y, do: { id: e, kind: tool, tool: t } } }\n"
        )
        .contains("annidamento"));
        assert!(err("  - { id: p, kind: parallel, steps: [ { id: c, kind: condition, if: x, then: [] } ] }\n")
            .contains("tool, agent, team o llm"));
        assert!(err("  - { id: p, kind: parallel, steps: [] }\n").contains("almeno un passo"));
        assert!(err("  - { id: d, kind: tool, tool: t }\n  - { id: p, kind: parallel, steps: [ { id: d, kind: tool, tool: t } ] }\n")
            .contains("duplicato"));
        assert!(err("  - { id: p, kind: parallel, steps: [ { id: a, kind: tool, tool: t, retry: 9 } ] }\n").contains("retry"));
        assert!(err(
            "  - { id: p, kind: parallel, steps: [ { id: a, kind: tool, tool: t } ] }\n  - { id: c, kind: condition, if: x, then: [a] }\n"
        )
        .contains("passo interno"));
    }

    #[test]
    fn validates_subworkflow_steps() {
        let mk = |step: &str| load_str_for_test(step);
        assert!(validate(&mk("{ id: s, kind: workflow, workflow: altro, inputs: { a: '{{inputs.x}}' }, retry: 1 }")).is_ok());
        assert!(validate(&mk("{ id: s, kind: workflow, workflow: w }")).unwrap_err().to_string().contains("se stesso"));
        assert!(validate(&mk("{ id: s, kind: workflow, workflow: ' ' }")).unwrap_err().to_string().contains("indica il workflow"));
        assert!(validate(&mk("{ id: s, kind: workflow, workflow: altro, retry: 9 }")).is_err());
    }
}
