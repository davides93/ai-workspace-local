//! Runs a suite: a working folder per task, the agent, the graders, the report.

use crate::grade::{grade, GradeContext};
use crate::host::{EvalHost, TaskRequest, TaskRun};
use crate::report::{EvalReport, GraderResult, TaskResult};
use crate::suite::{safe_relative, Suite, Task, DEFAULT_TIMEOUT_S};
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use std::path::PathBuf;
use std::time::Duration;

/// Agent of the tasks when neither the request nor the suite name one.
pub const DEFAULT_AGENT: &str = "operator";
/// Time the host gets on top of a task's timeout to wind the run up before the runner gives up on it.
const GRACE: Duration = Duration::from_secs(60);
/// Most times a suite can be repeated.
pub const MAX_REPEAT: u32 = 50;

#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Agent that replaces the ones named by the suite.
    pub agent: Option<String>,
    /// Inference profile that replaces the ones named by the suite.
    pub profile: Option<String>,
    /// System prompt that replaces the agent's own in every task.
    pub system_prompt: Option<String>,
    /// Times each task is run (at least 1).
    pub repeat: u32,
    /// Keep the working folders (their path is in the report).
    pub keep: bool,
    /// Where the working folders are made.
    pub work_root: PathBuf,
    /// Id of the report; a new one when missing.
    pub report_id: Option<String>,
    pub cancel: tokio_util::sync::CancellationToken,
}

impl RunOptions {
    pub fn new(work_root: PathBuf) -> Self {
        Self {
            agent: None,
            profile: None,
            system_prompt: None,
            repeat: 1,
            keep: false,
            work_root,
            report_id: None,
            cancel: Default::default(),
        }
    }
}

/// What the runner tells while it works.
pub enum EvalEvent<'a> {
    /// The report as it is before the first task.
    Started(&'a EvalReport),
    TaskStarted {
        task_id: &'a str,
        attempt: u32,
        attempts: u32,
    },
    /// A task is done; the report already holds it.
    TaskFinished {
        result: &'a TaskResult,
        report: &'a EvalReport,
    },
}

/// Runs every task of the suite `repeat` times, one at a time. Whatever goes wrong with a task is
/// in its result; the report is complete (`completed`) when the last one is done.
pub async fn run_suite(
    host: &dyn EvalHost,
    suite: &Suite,
    opts: &RunOptions,
    observer: &mut (dyn FnMut(EvalEvent<'_>) + Send),
) -> Result<EvalReport> {
    suite.validate()?;
    if opts.repeat == 0 || opts.repeat > MAX_REPEAT {
        return Err(Error::Invalid(format!("repeat deve essere tra 1 e {MAX_REPEAT}")));
    }
    let mut report = EvalReport {
        id: opts.report_id.clone().unwrap_or_else(|| new_id("evr")),
        suite_id: suite.id.clone(),
        suite_name: suite.name.clone(),
        status: "running".into(),
        started_at: now_rfc3339(),
        finished_at: None,
        agent: opts.agent.clone(),
        profile: opts.profile.clone(),
        repeat: opts.repeat,
        tasks: vec![],
        summary: Default::default(),
        error: None,
    };
    observer(EvalEvent::Started(&report));
    let attempts = suite.tasks.len() as u32 * opts.repeat;
    'attempts: for attempt in 1..=opts.repeat {
        for task in &suite.tasks {
            if opts.cancel.is_cancelled() {
                break 'attempts;
            }
            observer(EvalEvent::TaskStarted { task_id: &task.id, attempt, attempts });
            let result = run_task(host, suite, task, attempt, opts, &report.id).await;
            report.tasks.push(result);
            report.refresh_summary();
            observer(EvalEvent::TaskFinished { result: report.tasks.last().expect("just pushed"), report: &report });
        }
    }
    report.status = if opts.cancel.is_cancelled() { "cancelled" } else { "completed" }.into();
    report.finished_at = Some(now_rfc3339());
    Ok(report)
}

async fn run_task(host: &dyn EvalHost, suite: &Suite, task: &Task, attempt: u32, opts: &RunOptions, report_id: &str) -> TaskResult {
    let failed = |error: String, run_id: Option<String>| TaskResult {
        task_id: task.id.clone(),
        attempt,
        passed: false,
        score: 0.0,
        graders: vec![],
        duration_ms: 0,
        tokens: 0,
        turns: 0,
        run_id,
        error: Some(error),
        workdir: None,
    };
    let dir = opts.work_root.join(report_id).join(format!("{}-{attempt}", task.id));
    if let Err(e) = prepare(&dir, task) {
        return failed(format!("cartella di lavoro non preparata: {e}"), None);
    }
    let timeout = Duration::from_secs(task.timeout_s.unwrap_or(DEFAULT_TIMEOUT_S));
    let req = TaskRequest {
        agent: opts.agent.clone().or_else(|| task.agent.clone()).or_else(|| suite.agent.clone()).unwrap_or_else(|| DEFAULT_AGENT.into()),
        profile: opts.profile.clone().or_else(|| task.profile.clone()).or_else(|| suite.profile.clone()),
        system_prompt: opts.system_prompt.clone(),
        prompt: task.prompt.clone(),
        workdir: dir.clone(),
        timeout,
        cancel: opts.cancel.child_token(),
    };
    let started = std::time::Instant::now();
    let run = match tokio::time::timeout(timeout + GRACE, host.run_task(req)).await {
        Ok(Ok(run)) => run,
        Ok(Err(e)) => TaskRun { error: Some(e.to_string()), ..Default::default() },
        Err(_) => {
            TaskRun { error: Some(format!("il run non si è fermato entro {} secondi", (timeout + GRACE).as_secs())), ..Default::default() }
        }
    };
    let changes = if task.graders.iter().any(|g| g.kind() == "judge") { workdir_changes(&dir, task) } else { vec![] };
    let ctx = GradeContext { workdir: &dir, run: &run, host, prompt: &task.prompt, changes: &changes };
    let mut graders: Vec<GraderResult> = Vec::with_capacity(task.graders.len());
    for spec in &task.graders {
        tokio::select! {
            biased;
            _ = opts.cancel.cancelled() => break,
            result = grade(spec, &ctx) => graders.push(result),
        }
    }
    let score =
        if run.error.is_some() || graders.is_empty() { 0.0 } else { graders.iter().map(|g| g.score).sum::<f64>() / graders.len() as f64 };
    let kept = opts.keep.then(|| dir.display().to_string());
    if !opts.keep {
        let _ = std::fs::remove_dir_all(&dir);
        // The folder of the report goes away with its last task.
        let _ = std::fs::remove_dir(opts.work_root.join(report_id));
    }
    TaskResult {
        task_id: task.id.clone(),
        attempt,
        passed: run.error.is_none() && graders.iter().all(|g| g.passed),
        score,
        graders,
        duration_ms: if run.duration_ms > 0 { run.duration_ms } else { started.elapsed().as_millis() as u64 },
        tokens: run.tokens,
        turns: run.turns,
        run_id: run.run_id,
        error: run.error,
        workdir: kept,
    }
}

/// Most files and folder levels looked at when telling what a run changed.
const MAX_CHANGE_FILES: usize = 200;
const MAX_CHANGE_DEPTH: usize = 8;

/// What the run changed in the working folder compared to the task's starting files, one line per
/// file (`creato: a.md`, `modificato: b.txt`, `eliminato: c.txt`), sorted.
fn workdir_changes(dir: &std::path::Path, task: &Task) -> Vec<String> {
    fn walk(base: &std::path::Path, dir: &std::path::Path, depth: usize, out: &mut Vec<String>) {
        if depth > MAX_CHANGE_DEPTH || out.len() >= MAX_CHANGE_FILES {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                walk(base, &path, depth + 1, out);
            } else if kind.is_file() {
                if let Ok(rel) = path.strip_prefix(base) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    let mut now = vec![];
    walk(dir, dir, 0, &mut now);
    let start: std::collections::BTreeMap<String, &String> =
        task.files.iter().map(|(rel, content)| (rel.trim_start_matches("./").replace('\\', "/"), content)).collect();
    let mut lines = vec![];
    for rel in &now {
        match start.get(rel) {
            None => lines.push(format!("creato: {rel}")),
            Some(before) => {
                if std::fs::read(dir.join(rel)).map(|b| b != before.as_bytes()).unwrap_or(false) {
                    lines.push(format!("modificato: {rel}"));
                }
            }
        }
    }
    for rel in start.keys() {
        if !now.contains(rel) {
            lines.push(format!("eliminato: {rel}"));
        }
    }
    lines.sort();
    lines.truncate(MAX_CHANGE_FILES);
    lines
}

/// Makes the working folder with the task's starting files.
fn prepare(dir: &std::path::Path, task: &Task) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    for (rel, content) in &task.files {
        let path = dir.join(safe_relative(rel)?);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// Writes `note.md` for the task that asks for it and answers with a fixed text.
    struct FakeHost {
        seen: Mutex<Vec<TaskRequest>>,
        answer: String,
    }

    #[async_trait]
    impl EvalHost for FakeHost {
        async fn run_task(&self, req: TaskRequest) -> Result<TaskRun> {
            self.seen.lock().unwrap().push(req.clone());
            if req.prompt.contains("nota") {
                std::fs::write(req.workdir.join("note.md"), "ciao").unwrap();
            }
            if req.prompt.contains("rompiti") {
                return Err(Error::NotFound("agente x non trovato".into()));
            }
            Ok(TaskRun { final_text: self.answer.clone(), tokens: 30, turns: 2, run_id: Some("run_1".into()), ..Default::default() })
        }
        async fn complete(&self, _profile: Option<&str>, _prompt: &str) -> Result<String> {
            Ok(r#"{"score": 0.9, "reason": "ok"}"#.into())
        }
    }

    fn suite() -> Suite {
        Suite::parse(
            r#"
id: s
name: S
agent: planner
tasks:
  - id: crea
    prompt: crea la nota
    files: { "in/dati.txt": "uno" }
    graders:
      - { kind: file_exists, path: note.md }
      - { kind: file_contains, path: in/dati.txt, pattern: uno }
      - { kind: output_matches, pattern: note }
  - id: rompi
    prompt: rompiti
    agent: coder
    profile: veloce
    graders: [{ kind: file_exists, path: note.md }]
"#,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn runs_every_task_grades_it_and_cleans_up() {
        let work = tempfile::tempdir().unwrap();
        let host = FakeHost { seen: Mutex::new(vec![]), answer: "Ho scritto note.md".into() };
        let mut events = vec![];
        let report = run_suite(&host, &suite(), &RunOptions::new(work.path().into()), &mut |e| {
            events.push(match e {
                EvalEvent::Started(r) => format!("start {}", r.status),
                EvalEvent::TaskStarted { task_id, attempt, attempts } => format!("run {task_id} {attempt}/{attempts}"),
                EvalEvent::TaskFinished { result, report } => format!("done {} {} {}", result.task_id, result.passed, report.summary.total),
            })
        })
        .await
        .unwrap();
        assert_eq!(events, vec!["start running", "run crea 1/2", "done crea true 1", "run rompi 1/2", "done rompi false 2"]);
        assert_eq!(report.status, "completed");
        assert!(report.finished_at.is_some());

        let [crea, rompi] = &report.tasks[..] else { panic!() };
        assert!(crea.passed && crea.score == 1.0 && crea.graders.len() == 3);
        assert_eq!((crea.tokens, crea.turns, crea.run_id.as_deref()), (30, 2, Some("run_1")));
        // A host that cannot start the task fails it, with the reason; the graders still look at the folder.
        assert!(!rompi.passed && rompi.score == 0.0 && rompi.error.as_deref() == Some("agente x non trovato"));
        assert_eq!(rompi.graders.len(), 1);
        assert_eq!((report.summary.passed, report.summary.total, report.summary.score, report.summary.pass_rate), (1, 2, 0.5, 0.5));

        // Agent: the task's, else the suite's. Profile: the task's.
        let seen = host.seen.lock().unwrap();
        assert_eq!((seen[0].agent.as_str(), seen[0].profile.as_deref()), ("planner", None));
        assert!(seen.iter().all(|r| r.system_prompt.is_none()));
        assert_eq!((seen[1].agent.as_str(), seen[1].profile.as_deref()), ("coder", Some("veloce")));
        assert_eq!(seen[0].timeout, Duration::from_secs(DEFAULT_TIMEOUT_S));
        // The files were there for the agent, and the folders are gone.
        assert!(!seen[0].workdir.exists());
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn a_wrong_answer_fails_the_task_and_options_override_the_suite() {
        let work = tempfile::tempdir().unwrap();
        let host = FakeHost { seen: Mutex::new(vec![]), answer: "Fatto".into() };
        let opts = RunOptions {
            agent: Some("operator".into()),
            profile: Some("p".into()),
            system_prompt: Some("Prompt di prova".into()),
            repeat: 2,
            keep: true,
            ..RunOptions::new(work.path().into())
        };
        let report = run_suite(&host, &suite(), &opts, &mut |_| {}).await.unwrap();
        assert_eq!(report.tasks.len(), 4);
        assert_eq!(report.tasks.iter().map(|t| t.attempt).collect::<Vec<_>>(), vec![1, 1, 2, 2]);
        let crea = &report.tasks[0];
        assert!(!crea.passed, "the answer does not mention the note");
        assert_eq!(crea.graders.iter().map(|g| g.passed).collect::<Vec<_>>(), vec![true, true, false]);
        assert!((crea.score - 2.0 / 3.0).abs() < 1e-9);
        assert!(host.seen.lock().unwrap().iter().all(|r| r.agent == "operator" && r.profile.as_deref() == Some("p")));
        // The prompt of the options reaches every task; without it the agent keeps its own.
        assert!(host.seen.lock().unwrap().iter().all(|r| r.system_prompt.as_deref() == Some("Prompt di prova")));
        assert_eq!((report.agent.as_deref(), report.profile.as_deref(), report.repeat), (Some("operator"), Some("p"), 2));
        // `keep`: the folders stay, and the report says where.
        let dir = PathBuf::from(crea.workdir.as_deref().unwrap());
        assert_eq!(std::fs::read_to_string(dir.join("note.md")).unwrap(), "ciao");
    }

    #[tokio::test]
    async fn refuses_a_bad_repeat_and_a_bad_suite() {
        let work = tempfile::tempdir().unwrap();
        let host = FakeHost { seen: Mutex::new(vec![]), answer: String::new() };
        for repeat in [0, MAX_REPEAT + 1] {
            let opts = RunOptions { repeat, ..RunOptions::new(work.path().into()) };
            assert!(matches!(run_suite(&host, &suite(), &opts, &mut |_| {}).await, Err(Error::Invalid(_))));
        }
        let mut bad = suite();
        bad.tasks.clear();
        assert!(run_suite(&host, &bad, &RunOptions::new(work.path().into()), &mut |_| {}).await.is_err());
        assert!(host.seen.lock().unwrap().is_empty());
    }
}
