//! Evaluation on the engine: a whole suite run with a scripted model, suites as documents, reports.

use super::tests::engine;
use super::*;
use crate::eval::EvalRequest;
use aiw_eval::{EvalEvent, Suite};
use aiw_runtimes::ScriptedBackend;

fn suite(yaml: &str) -> Suite {
    Suite::parse(yaml).unwrap()
}

const NOTE_SUITE: &str = r#"
id: nota
name: Nota
tasks:
  - id: crea-nota
    prompt: "Crea note.md con scritto ciao"
    files: { "input.txt": "contenuto iniziale" }
    graders:
      - { kind: file_exists, path: note.md }
      - { kind: file_contains, path: note.md, pattern: "(?i)ciao" }
      - { kind: file_contains, path: input.txt, pattern: iniziale }
      - { kind: output_matches, pattern: "note\\.md" }
"#;

#[tokio::test]
async fn an_agent_that_does_the_task_passes_every_grader_without_touching_the_workspace() {
    let turns = vec![
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "note.md", "content": "Ciao!\n"})),
        ScriptedBackend::text_turn("Ho creato note.md"),
    ];
    let (e, _d, ws) = engine(turns).await;
    let mut seen = vec![];
    let report = e
        .run_eval(&suite(NOTE_SUITE), &EvalRequest::default(), &mut |ev| {
            seen.push(match ev {
                EvalEvent::Started(r) => format!("start {}", r.status),
                EvalEvent::TaskStarted { task_id, .. } => format!("run {task_id}"),
                EvalEvent::TaskFinished { result, .. } => format!("done {}", result.passed),
            })
        })
        .await
        .unwrap();
    assert_eq!(seen, vec!["start running", "run crea-nota", "done true"]);
    assert_eq!(report.status, "completed");
    let t = &report.tasks[0];
    assert!(t.passed && t.error.is_none(), "{t:?}");
    assert!(t.graders.iter().all(|g| g.passed), "{:?}", t.graders);
    assert_eq!((t.turns, t.tokens), (2, 15));

    // The run is stored like any other (events, run row, audit) but belongs to no session.
    let run_id = t.run_id.clone().unwrap();
    assert!(e.run_events(&run_id).iter().any(|ev| matches!(ev.event.event, AgEvent::RunFinished { .. })));
    assert_eq!(e.0.store.events(&run_id).unwrap().len(), e.run_events(&run_id).len());
    assert!(e.sessions().unwrap().is_empty());
    assert!(e.audit().iter().any(|a| a.run_id.as_deref() == Some(run_id.as_str()) && a.tool.as_deref() == Some("write_file")));
    // The file went to the task's folder, which is gone; the user's workspace is as it was.
    assert!(!ws.path().join("note.md").exists());
    assert!(std::fs::read_dir(e.0.dirs.eval_work()).map(|mut d| d.next().is_none()).unwrap_or(true));
    // The report is stored, and found again.
    assert_eq!(e.eval_report(&report.id).unwrap(), report);
    assert_eq!(e.eval_reports().unwrap()[0].id, report.id);
    assert!(matches!(e.eval_report("evr_none"), Err(Error::NotFound(_))));
}

#[tokio::test]
async fn a_wrong_or_missing_result_fails_the_task() {
    let (e, _d, _w) = engine(vec![ScriptedBackend::text_turn("Fatto, tutto a posto")]).await;
    let req = EvalRequest { keep: true, ..Default::default() };
    let report = e.run_eval(&suite(NOTE_SUITE), &req, &mut |_| {}).await.unwrap();
    let t = &report.tasks[0];
    assert!(!t.passed);
    assert_eq!(t.graders.iter().map(|g| g.passed).collect::<Vec<_>>(), vec![false, false, true, false]);
    assert!(t.graders[0].detail.contains("note.md non esiste"));
    assert_eq!((report.summary.passed, report.summary.total, report.summary.pass_rate), (0, 1, 0.0));
    // `keep`: the folder is still there, with the starting file.
    let dir = std::path::PathBuf::from(t.workdir.as_deref().unwrap());
    assert_eq!(std::fs::read_to_string(dir.join("input.txt")).unwrap(), "contenuto iniziale");
}

#[tokio::test]
async fn the_agent_cannot_leave_its_working_folder() {
    let turns = vec![
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "../fuori.txt", "content": "x"})),
        ScriptedBackend::text_turn("Non ci sono riuscito"),
    ];
    let (e, _d, ws) = engine(turns).await;
    let s = suite("id: s\nname: S\ntasks:\n  - id: t\n    prompt: scrivi fuori\n    graders: [{ kind: file_exists, path: fuori.txt }]");
    let report = e.run_eval(&s, &EvalRequest::default(), &mut |_| {}).await.unwrap();
    assert!(!report.tasks[0].passed);
    assert!(!ws.path().join("fuori.txt").exists());
    assert!(!e.0.dirs.eval_work().join("fuori.txt").exists() && !e.0.dirs.eval_work().join(&report.id).join("fuori.txt").exists());
}

#[tokio::test]
async fn repeat_runs_each_task_again_and_the_report_has_the_rates() {
    let turns = vec![
        ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "note.md", "content": "ciao"})),
        ScriptedBackend::text_turn("Ho creato note.md"),
        ScriptedBackend::text_turn("Non ho scritto nulla"),
    ];
    let (e, _d, _w) = engine(turns).await;
    let s = suite("id: s\nname: S\ntasks:\n  - id: t\n    prompt: crea note.md\n    graders: [{ kind: file_exists, path: note.md }]");
    let report = e.run_eval(&s, &EvalRequest { repeat: Some(2), ..Default::default() }, &mut |_| {}).await.unwrap();
    assert_eq!(report.tasks.iter().map(|t| (t.attempt, t.passed)).collect::<Vec<_>>(), vec![(1, true), (2, false)]);
    assert_eq!((report.summary.passed, report.summary.total, report.summary.pass_rate, report.summary.score), (1, 2, 0.5, 0.5));
    assert!(matches!(e.run_eval(&s, &EvalRequest { repeat: Some(0), ..Default::default() }, &mut |_| {}).await, Err(Error::Invalid(_))));
}

#[tokio::test]
async fn an_unknown_agent_or_profile_is_refused_before_anything_runs() {
    let (e, _d, _w) = engine(vec![]).await;
    let s = suite(NOTE_SUITE);
    let bad_agent = EvalRequest { agent: Some("fantasma".into()), ..Default::default() };
    assert!(matches!(e.run_eval(&s, &bad_agent, &mut |_| {}).await, Err(Error::NotFound(_))));
    let bad_profile = EvalRequest { profile: Some("fantasma".into()), ..Default::default() };
    assert!(matches!(e.run_eval(&s, &bad_profile, &mut |_| {}).await, Err(Error::NotFound(_))));
    assert!(e.eval_reports().unwrap().is_empty());
    let mut other = s.clone();
    other.agent = Some("fantasma".into());
    assert!(matches!(e.save_eval_suite(other), Err(Error::Invalid(_))));
}

#[tokio::test]
async fn suites_are_documents_seeded_with_the_example_and_validated() {
    let (e, _d, _w) = engine(vec![]).await;
    assert!(e.eval_suites().unwrap().iter().any(|s| s.id == "esempio"));
    let saved = e.save_eval_suite_text(NOTE_SUITE).unwrap();
    assert_eq!(e.eval_suite("nota").unwrap(), saved);
    assert!(e.save_eval_suite_text("id: x\nname: X\ntasks: []").is_err());
    assert!(e.save_eval_suite_text("non è una suite").is_err());
    // From a file or from the stored id.
    let f = tempfile::tempdir().unwrap();
    std::fs::write(f.path().join("s.yaml"), NOTE_SUITE.replace("id: nota", "id: da-file")).unwrap();
    assert_eq!(e.resolve_eval_suite(f.path().join("s.yaml").to_str().unwrap()).unwrap().id, "da-file");
    assert_eq!(e.resolve_eval_suite("nota").unwrap().id, "nota");
    assert!(matches!(e.resolve_eval_suite("nessuna"), Err(Error::NotFound(_))));
    e.delete_eval_suite("nota").unwrap();
    assert!(matches!(e.delete_eval_suite("nota"), Err(Error::NotFound(_))));
}

#[tokio::test]
async fn only_the_users_folder_brings_suites_in() {
    let data = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(data.path().join("evals")).unwrap();
    std::fs::write(data.path().join("evals/mia.yaml"), NOTE_SUITE.replace("id: nota", "id: mia")).unwrap();
    std::fs::write(data.path().join("evals/rotta.yaml"), "id: [").unwrap();
    // A project can ship a suite with commands in it: it is not read.
    std::fs::create_dir_all(ws.path().join(".aiw/evals")).unwrap();
    std::fs::write(ws.path().join(".aiw/evals/cattiva.yaml"), NOTE_SUITE.replace("id: nota", "id: cattiva")).unwrap();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().to_path_buf()),
        workspace_root: Some(ws.path().to_path_buf()),
        mode: "embedded".into(),
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap();
    let ids: Vec<String> = e.eval_suites().unwrap().into_iter().map(|s| s.id).collect();
    assert!(ids.contains(&"mia".to_string()) && ids.contains(&"esempio".to_string()), "{ids:?}");
    assert!(!ids.contains(&"cattiva".to_string()), "{ids:?}");
}
