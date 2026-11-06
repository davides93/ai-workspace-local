//! Prompt optimization: baseline, variants, choice of the best, diff, application.

use super::tests::engine;
use super::*;
use crate::optimize::{unified_diff, OptimizeOptions};
use aiw_runtimes::ScriptedBackend;

const SUITE: &str =
    "id: nota\nname: Nota\ntasks:\n  - id: crea\n    prompt: crea note.md\n    graders: [{ kind: file_exists, path: note.md }]";

fn variants_json(prompts: &[&str]) -> Vec<aiw_runtimes::ChatDelta> {
    let v: Vec<Value> = prompts.iter().map(|p| json!({"prompt": p, "rationale": format!("perché {p}")})).collect();
    ScriptedBackend::text_turn(&json!({"variants": v}).to_string())
}

/// An agent that works only with the prompt it has: the baseline does nothing.
async fn setup(turns: Vec<Vec<aiw_runtimes::ChatDelta>>) -> (Engine, tempfile::TempDir, tempfile::TempDir, ScriptedBackend) {
    let (e, d, w) = engine(vec![]).await;
    let backend = ScriptedBackend::new(turns);
    e.register_native_runtime("llama-embedded", Arc::new(backend.clone()));
    e.save_eval_suite_text(SUITE).unwrap();
    let mut a = e.agent("operator").unwrap();
    a.system_prompt = "Sei un assistente.\nRispondi breve.".into();
    e.save_agent(a).unwrap();
    (e, d, w, backend)
}

fn opts(variants: u32) -> OptimizeOptions {
    OptimizeOptions { variants: Some(variants), ..Default::default() }
}

#[tokio::test]
async fn a_variant_that_passes_the_grader_is_chosen_and_the_diff_is_right() {
    let turns = vec![
        ScriptedBackend::text_turn("Non ho scritto nulla"),
        variants_json(&["Sei un assistente.\nRispondi breve.\nCrea sempre i file richiesti."]),
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "note.md", "content": "ciao"})),
        ScriptedBackend::text_turn("Fatto"),
    ];
    let (e, _d, _w, backend) = setup(turns).await;
    let o = e.optimize_prompt("nota", "operator", &opts(1)).await.unwrap();
    assert_eq!((o.status.as_str(), o.suite_id.as_str(), o.agent_id.as_str()), ("done", "nota", "operator"));
    assert_eq!((o.baseline_score, o.baseline_pass_rate), (0.0, 0.0));
    assert_eq!(o.variants.len(), 1);
    let v = &o.variants[0];
    assert_eq!((v.score, v.pass_rate), (1.0, 1.0));
    assert_eq!(o.best, Some(0));
    assert_eq!(
        v.diff,
        "--- attuale\n+++ variante\n@@ -1,2 +1,3 @@\n Sei un assistente.\n Rispondi breve.\n+Crea sempre i file richiesti.\n"
    );
    // Two reports, found again; the baseline ran with the agent's own prompt, the variant with its own.
    assert!(e.eval_report(o.baseline_report_id.as_deref().unwrap()).unwrap().tasks[0].error.is_none());
    assert_eq!(e.eval_report(v.report_id.as_deref().unwrap()).unwrap().summary.passed, 1);
    let system_of = |i: usize| backend.requests.lock().unwrap()[i].messages[0].text();
    assert!(system_of(0).starts_with("Sei un assistente.\nRispondi breve.") && !system_of(0).contains("Crea sempre"));
    assert!(system_of(2).contains("Crea sempre i file richiesti."));
    // The failures of the baseline went into the request for variants.
    let asked = backend.requests.lock().unwrap()[1].messages.last().unwrap().text();
    assert!(asked.contains("note.md non esiste") && asked.contains("Rispondi breve."), "{asked}");
    // Stored, listed, and nothing applied: the agent is as it was.
    assert_eq!(e.prompt_optimization(&o.id).unwrap(), o);
    assert_eq!(e.prompt_optimizations().unwrap()[0].id, o.id);
    assert_eq!(e.agent("operator").unwrap().system_prompt, "Sei un assistente.\nRispondi breve.");
    assert!(matches!(e.prompt_optimization("opt_none"), Err(Error::NotFound(_))));
}

#[tokio::test]
async fn when_no_variant_beats_the_baseline_the_result_says_so_and_nothing_is_proposed() {
    let turns = vec![
        ScriptedBackend::text_turn("Niente"),
        variants_json(&["Prompt uno", "Prompt due"]),
        ScriptedBackend::text_turn("Niente ancora"),
        ScriptedBackend::text_turn("Niente di nuovo"),
    ];
    let (e, _d, _w, _b) = setup(turns).await;
    let o = e.optimize_prompt("nota", "operator", &opts(2)).await.unwrap();
    assert_eq!((o.status.as_str(), o.variants.len(), o.best), ("done", 2, None));
    assert!(o.variants.iter().all(|v| v.score <= o.baseline_score));
    // A variant that does not exist cannot be applied.
    assert!(matches!(e.apply_prompt_optimization(&o.id, 5), Err(Error::Invalid(_))));
}

#[tokio::test]
async fn the_best_variant_wins_by_score_then_pass_rate_then_the_shorter_prompt() {
    // Both variants pass: the shorter one is chosen.
    let turns = vec![
        ScriptedBackend::text_turn("Niente"),
        variants_json(&["Un prompt molto più lungo del necessario", "Corto"]),
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "note.md", "content": "a"})),
        ScriptedBackend::text_turn("ok"),
        ScriptedBackend::tool_turn("c2", "write_file", json!({"path": "note.md", "content": "b"})),
        ScriptedBackend::text_turn("ok"),
    ];
    let (e, _d, _w, _b) = setup(turns).await;
    let o = e.optimize_prompt("nota", "operator", &opts(2)).await.unwrap();
    assert_eq!((o.variants[0].score, o.variants[1].score), (1.0, 1.0));
    assert_eq!(o.best, Some(1));
}

#[tokio::test]
async fn applying_saves_the_prompt_in_the_agent_audits_and_cannot_be_repeated() {
    let turns = vec![
        ScriptedBackend::text_turn("Niente"),
        variants_json(&["Nuovo prompt"]),
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "note.md", "content": "a"})),
        ScriptedBackend::text_turn("ok"),
    ];
    let (e, _d, _w, _b) = setup(turns).await;
    let o = e.optimize_prompt("nota", "operator", &opts(1)).await.unwrap();
    let applied = e.apply_prompt_optimization(&o.id, 0).unwrap();
    assert_eq!((applied.status.as_str(), applied.applied), ("applied", Some(0)));
    assert_eq!(e.agent("operator").unwrap().system_prompt, "Nuovo prompt");
    assert!(e.audit().iter().any(|a| a.action == "prompt.apply" && a.actor == "user"));
    assert!(matches!(e.apply_prompt_optimization(&o.id, 0), Err(Error::Conflict(_))));
}

#[tokio::test]
async fn an_agent_edited_meanwhile_is_not_overwritten() {
    let turns = vec![ScriptedBackend::text_turn("Niente"), variants_json(&["Nuovo prompt"]), ScriptedBackend::text_turn("Niente")];
    let (e, _d, _w, _b) = setup(turns).await;
    let o = e.optimize_prompt("nota", "operator", &opts(1)).await.unwrap();
    let mut a = e.agent("operator").unwrap();
    a.system_prompt = "Modificato a mano".into();
    e.save_agent(a).unwrap();
    assert!(matches!(e.apply_prompt_optimization(&o.id, 0), Err(Error::Conflict(_))));
    assert_eq!(e.agent("operator").unwrap().system_prompt, "Modificato a mano");
}

#[tokio::test]
async fn bad_requests_and_bad_answers_are_refused_with_a_failed_record() {
    let (e, _d, _w, backend) = setup(vec![]).await;
    assert!(matches!(e.optimize_prompt("nota", "operator", &opts(0)).await, Err(Error::Invalid(_))));
    assert!(matches!(e.optimize_prompt("nota", "operator", &opts(6)).await, Err(Error::Invalid(_))));
    assert!(matches!(e.optimize_prompt("nota", "fantasma", &opts(1)).await, Err(Error::NotFound(_))));
    assert!(matches!(e.optimize_prompt("nessuna", "operator", &opts(1)).await, Err(Error::NotFound(_))));
    let wrong_profile = OptimizeOptions { profile: Some("fantasma".into()), ..opts(1) };
    assert!(matches!(e.optimize_prompt("nota", "operator", &wrong_profile).await, Err(Error::NotFound(_))));
    assert!(backend.requests.lock().unwrap().is_empty() && e.prompt_optimizations().unwrap().is_empty());

    // The model answers with no usable JSON twice: the optimization fails and says why.
    let err = e.optimize_prompt("nota", "operator", &opts(2)).await.unwrap_err();
    assert!(err.to_string().contains("varianti valide"), "{err}");
    let stored = &e.prompt_optimizations().unwrap()[0];
    assert_eq!(stored.status, "failed");
    assert!(stored.error.as_deref().unwrap().contains("varianti valide") && stored.finished_at.is_some());
    assert!(stored.baseline_report_id.is_some() && stored.variants.is_empty());
    assert!(matches!(e.apply_prompt_optimization(&stored.id.clone(), 0), Err(Error::Conflict(_))));
    // An optimization left running by a dead engine is marked failed at the next start.
    let mut running = stored.clone();
    running.id = "opt_orfana".into();
    running.status = "running".into();
    running.error = None;
    e.0.store.put_doc("prompt_optimization", &running.id, &running).unwrap();
    e.fail_interrupted_optimizations().unwrap();
    assert!(e.prompt_optimization("opt_orfana").unwrap().error.unwrap().contains("interrotta"));
}

#[tokio::test]
async fn the_background_start_returns_a_running_record_that_ends_in_the_store() {
    let turns = vec![ScriptedBackend::text_turn("Niente"), variants_json(&["Altro"]), ScriptedBackend::text_turn("Niente")];
    let (e, _d, _w, _b) = setup(turns).await;
    let started = e.start_prompt_optimization(e.eval_suite("nota").unwrap(), "operator", opts(1)).unwrap();
    assert_eq!(started.status, "running");
    for _ in 0..200 {
        if e.prompt_optimization(&started.id).unwrap().status == "done" {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("l'ottimizzazione non è finita");
}

#[test]
fn the_diff_is_unified_with_context_and_merged_hunks() {
    assert_eq!(unified_diff("a\nb", "a\nb"), "");
    assert_eq!(unified_diff("a\nb\nc", "a\nB\nc"), "--- attuale\n+++ variante\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n");
    assert_eq!(unified_diff("", "nuovo"), "--- attuale\n+++ variante\n@@ -1,0 +1,1 @@\n+nuovo\n");
    // Far apart changes make two hunks, with line numbers of each side.
    let from: Vec<String> = (1..=20).map(|n| format!("riga {n}")).collect();
    let mut to = from.clone();
    to[1] = "DUE".into();
    to[17] = "DICIOTTO".into();
    let d = unified_diff(&from.join("\n"), &to.join("\n"));
    assert_eq!(d.matches("@@").count(), 4, "{d}");
    assert!(d.contains("@@ -1,5 +1,5 @@") && d.contains("@@ -15,6 +15,6 @@"), "{d}");
    assert!(d.contains("-riga 2\n+DUE\n") && d.contains("-riga 18\n+DICIOTTO\n"), "{d}");
}
