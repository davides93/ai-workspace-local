//! The deterministic graders.

use crate::host::{EvalHost, TaskRun};
use crate::report::GraderResult;
use crate::suite::{resolve_inside, GraderSpec, DEFAULT_COMMAND_TIMEOUT_S, DEFAULT_JUDGE_THRESHOLD};
use regex::Regex;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

/// Longest part of a file a grader reads.
const MAX_READ: usize = 1_000_000;
/// Longest piece of command output kept in a result.
const MAX_OUTPUT: usize = 400;

pub struct GradeContext<'a> {
    pub workdir: &'a Path,
    pub run: &'a TaskRun,
    /// Asks the model for the `judge` grader.
    pub host: &'a dyn EvalHost,
    /// The prompt the agent was given.
    pub prompt: &'a str,
    /// What the run changed in the working folder, one line per file (`creato: note.md`).
    pub changes: &'a [String],
}

fn result(kind: &str, passed: bool, detail: impl Into<String>) -> GraderResult {
    GraderResult { kind: kind.into(), passed, score: if passed { 1.0 } else { 0.0 }, detail: detail.into() }
}

/// Applies one grader. It never fails: whatever goes wrong is a failed grader with the reason.
pub async fn grade(spec: &GraderSpec, ctx: &GradeContext<'_>) -> GraderResult {
    let kind = spec.kind();
    match spec {
        GraderSpec::FileExists { path } => match resolve_inside(ctx.workdir, path) {
            Ok(p) if p.is_file() => result(kind, true, format!("{path} esiste")),
            Ok(_) => result(kind, false, format!("{path} non esiste")),
            Err(e) => result(kind, false, e.to_string()),
        },
        GraderSpec::FileContains { path, pattern } => {
            let re = match Regex::new(pattern) {
                Ok(re) => re,
                Err(e) => return result(kind, false, format!("espressione regolare non valida: {e}")),
            };
            let file = match resolve_inside(ctx.workdir, path) {
                Ok(p) => p,
                Err(e) => return result(kind, false, e.to_string()),
            };
            match read_text(&file) {
                Ok(text) if re.is_match(&text) => result(kind, true, format!("{path} contiene «{pattern}»")),
                Ok(_) => result(kind, false, format!("{path} non contiene «{pattern}»")),
                Err(_) => result(kind, false, format!("{path} non esiste o non è leggibile")),
            }
        }
        GraderSpec::OutputMatches { pattern } => match Regex::new(pattern) {
            Ok(re) if re.is_match(&ctx.run.final_text) => result(kind, true, format!("la risposta contiene «{pattern}»")),
            Ok(_) => result(kind, false, format!("la risposta non contiene «{pattern}»")),
            Err(e) => result(kind, false, format!("espressione regolare non valida: {e}")),
        },
        GraderSpec::JsonSchema { schema } => {
            let Some(value) = ctx.run.structured.clone().or_else(|| aiw_agent::schema::extract_json(&ctx.run.final_text)) else {
                return result(kind, false, "la risposta non contiene un JSON valido");
            };
            match aiw_agent::schema::validate(schema, &value) {
                Ok(()) => result(kind, true, "la risposta rispetta lo schema"),
                Err(errors) => result(kind, false, errors.into_iter().take(3).collect::<Vec<_>>().join("; ")),
            }
        }
        GraderSpec::Judge { criteria, profile, threshold } => {
            judge(ctx, criteria, profile.as_deref(), threshold.unwrap_or(DEFAULT_JUDGE_THRESHOLD)).await
        }
        GraderSpec::Command { run, expect_exit, timeout_s } => {
            let limit = Duration::from_secs(timeout_s.unwrap_or(DEFAULT_COMMAND_TIMEOUT_S));
            match run_command(run, ctx.workdir, limit).await {
                Ok((code, _)) if code == Some(*expect_exit) => result(kind, true, format!("`{run}` è uscito con {expect_exit}")),
                Ok((code, output)) => {
                    let got = code.map_or("nessun codice (terminato da un segnale)".to_string(), |c| c.to_string());
                    let tail = if output.is_empty() { String::new() } else { format!(": {output}") };
                    result(kind, false, format!("`{run}` è uscito con {got}, atteso {expect_exit}{tail}"))
                }
                Err(why) => result(kind, false, format!("`{run}` {why}")),
            }
        }
    }
}

/// Longest piece of the task prompt and of the final answer shown to the judge.
const JUDGE_PROMPT_CHARS: usize = 4_000;
const JUDGE_ANSWER_CHARS: usize = 8_000;

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    format!("{}… (troncato)", text.chars().take(max).collect::<String>())
}

/// What the judge is asked.
pub fn judge_prompt(criteria: &str, task_prompt: &str, answer: &str, changes: &[String]) -> String {
    let files = if changes.is_empty() {
        "(nessun file cambiato)".to_string()
    } else {
        changes.iter().map(|c| format!("- {c}")).collect::<Vec<_>>().join("\n")
    };
    format!(
        "Sei un valutatore rigoroso e imparziale. Giudica il lavoro di un agente su un compito.\n\n\
         # Compito assegnato\n{}\n\n# Risposta finale dell'agente\n{}\n\n# File cambiati nella cartella di lavoro\n{files}\n\n\
         # Criteri\n{criteria}\n\n\
         Rispondi SOLO con un oggetto JSON: {{\"score\": <numero da 0 a 1>, \"reason\": \"<motivazione breve>\"}}. \
         1 vuol dire che i criteri sono soddisfatti del tutto, 0 che non lo sono affatto.",
        clip(task_prompt, JUDGE_PROMPT_CHARS),
        if answer.trim().is_empty() { "(nessuna risposta)".to_string() } else { clip(answer, JUDGE_ANSWER_CHARS) },
    )
}

async fn judge(ctx: &GradeContext<'_>, criteria: &str, profile: Option<&str>, threshold: f64) -> GraderResult {
    let kind = "judge";
    let prompt = judge_prompt(criteria, ctx.prompt, &ctx.run.final_text, ctx.changes);
    let answer = match ctx.host.complete(profile, &prompt).await {
        Ok(a) => a,
        Err(e) => return result(kind, false, format!("il giudice non ha risposto: {e}")),
    };
    let Some(v) = aiw_agent::schema::extract_json(&answer) else {
        return result(kind, false, format!("la risposta del giudice non contiene un JSON: {}", clip(answer.trim(), 200)));
    };
    let reason = v["reason"].as_str().map(str::trim).filter(|r| !r.is_empty()).unwrap_or("nessuna motivazione");
    match v["score"].as_f64() {
        Some(score) if (0.0..=1.0).contains(&score) => {
            let passed = score >= threshold;
            GraderResult { kind: kind.into(), passed, score, detail: format!("{score:.2} (soglia {threshold:.2}): {reason}") }
        }
        Some(score) => result(kind, false, format!("il punteggio del giudice ({score}) è fuori dall'intervallo da 0 a 1: {reason}")),
        None => result(kind, false, format!("la risposta del giudice non ha un punteggio numerico: {reason}")),
    }
}

fn read_text(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)?.take(MAX_READ as u64).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// `sh -c` (PowerShell on Windows) in `dir`. The exit code (none when a signal ended it) and the tail of
/// the output, or why it could not finish.
async fn run_command(run: &str, dir: &Path, limit: Duration) -> Result<(Option<i32>, String), String> {
    let mut cmd = if cfg!(windows) {
        let mut c = tokio::process::Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", run]);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(run);
        c
    };
    cmd.current_dir(dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let out = match tokio::time::timeout(limit, cmd.output()).await {
        Err(_) => return Err(format!("non è finito entro {} secondi", limit.as_secs())),
        Ok(Err(e)) => return Err(format!("non è partito: {e}")),
        Ok(Ok(out)) => out,
    };
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    let text = text.trim();
    let skip = text.chars().count().saturating_sub(MAX_OUTPUT);
    Ok((out.status.code(), text.chars().skip(skip).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::TaskRequest;
    use aiw_core::{Error, Result};
    use async_trait::async_trait;
    use serde_json::json;

    /// A judge that gives a fixed answer and remembers what it was asked.
    struct Judge(std::sync::Mutex<Vec<(Option<String>, String)>>, std::result::Result<String, String>);

    #[async_trait]
    impl EvalHost for Judge {
        async fn run_task(&self, _req: TaskRequest) -> Result<TaskRun> {
            unreachable!("graders do not run tasks")
        }
        async fn complete(&self, profile: Option<&str>, prompt: &str) -> Result<String> {
            self.0.lock().unwrap().push((profile.map(String::from), prompt.to_string()));
            self.1.clone().map_err(Error::Runtime)
        }
    }

    fn judge_host(answer: &str) -> Judge {
        Judge(Default::default(), Ok(answer.into()))
    }

    async fn run(spec: GraderSpec, dir: &Path, output: &str) -> GraderResult {
        let run = TaskRun { final_text: output.into(), ..Default::default() };
        grade(&spec, &GradeContext { workdir: dir, run: &run, host: &judge_host("{}"), prompt: "", changes: &[] }).await
    }

    #[tokio::test]
    async fn file_graders_look_at_the_working_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("note.md"), "Ciao a tutti\n").unwrap();
        let exists = |p: &str| GraderSpec::FileExists { path: p.into() };
        assert!(run(exists("note.md"), dir.path(), "").await.passed);
        let r = run(exists("assente.md"), dir.path(), "").await;
        assert!(!r.passed && r.detail.contains("non esiste") && r.score == 0.0);
        assert!(!run(exists("../fuori"), dir.path(), "").await.passed);
        std::fs::create_dir(dir.path().join("cartella")).unwrap();
        assert!(!run(exists("cartella"), dir.path(), "").await.passed, "a folder is not a file");

        let contains = |p: &str, pat: &str| GraderSpec::FileContains { path: p.into(), pattern: pat.into() };
        assert!(run(contains("note.md", "(?i)ciao"), dir.path(), "").await.passed);
        assert!(!run(contains("note.md", "^addio"), dir.path(), "").await.passed);
        let r = run(contains("assente.md", "x"), dir.path(), "").await;
        assert!(!r.passed && r.detail.contains("non esiste"));
        assert!(!run(contains("note.md", "("), dir.path(), "").await.passed);
    }

    #[tokio::test]
    async fn answer_graders_read_the_final_text() {
        let dir = tempfile::tempdir().unwrap();
        let m = |p: &str| GraderSpec::OutputMatches { pattern: p.into() };
        assert!(run(m(r"note\.md"), dir.path(), "Ho creato note.md").await.passed);
        assert!(!run(m(r"note\.md"), dir.path(), "Fatto").await.passed);

        let schema =
            GraderSpec::JsonSchema { schema: json!({"type": "object", "required": ["ok"], "properties": {"ok": {"type": "boolean"}}}) };
        assert!(run(schema.clone(), dir.path(), "Ecco:\n```json\n{\"ok\": true}\n```").await.passed);
        let r = run(schema.clone(), dir.path(), "{\"ok\": \"si\"}").await;
        assert!(!r.passed && r.detail.contains("ok"), "{}", r.detail);
        assert!(run(schema.clone(), dir.path(), "nessun json").await.detail.contains("JSON"));
        // The structured result of the run wins over the text.
        let with_structured = TaskRun { final_text: "testo".into(), structured: Some(json!({"ok": false})), ..Default::default() };
        let ctx = GradeContext { workdir: dir.path(), run: &with_structured, host: &judge_host("{}"), prompt: "", changes: &[] };
        assert!(grade(&schema, &ctx).await.passed);
    }

    async fn judged(answer: &str, threshold: Option<f64>) -> (GraderResult, Judge) {
        let host = judge_host(answer);
        let dir = tempfile::tempdir().unwrap();
        let run = TaskRun { final_text: "Ho creato note.md".into(), ..Default::default() };
        let changes = ["creato: note.md".to_string()];
        let ctx = GradeContext { workdir: dir.path(), run: &run, host: &host, prompt: "Crea una nota", changes: &changes };
        let spec = GraderSpec::Judge { criteria: "La nota è cortese".into(), profile: Some("giudice".into()), threshold };
        (grade(&spec, &ctx).await, host)
    }

    #[tokio::test]
    async fn the_judge_sees_the_work_and_scores_it() {
        let (r, host) = judged("Ecco: ```json\n{\"score\": 0.85, \"reason\": \"cortese\"}\n```", None).await;
        assert!(r.passed && r.score == 0.85 && r.detail.contains("cortese"), "{r:?}");
        {
            let asked = host.0.lock().unwrap();
            assert_eq!(asked[0].0.as_deref(), Some("giudice"));
            for part in ["Crea una nota", "Ho creato note.md", "- creato: note.md", "La nota è cortese", "\"score\""] {
                assert!(asked[0].1.contains(part), "{part} missing from {}", asked[0].1);
            }
        }
        // Below the threshold it fails but keeps its score; the threshold can be moved.
        let (r, _) = judged("{\"score\": 0.5, \"reason\": \"così così\"}", None).await;
        assert!(!r.passed && r.score == 0.5);
        let (r, _) = judged("{\"score\": 0.5, \"reason\": \"così così\"}", Some(0.5)).await;
        assert!(r.passed);
    }

    #[tokio::test]
    async fn a_judge_that_does_not_answer_properly_fails_the_grader_with_the_reason() {
        for (answer, expect) in [
            ("{\"score\": 1.5, \"reason\": \"x\"}", "fuori dall'intervallo"),
            ("{\"score\": -0.1}", "fuori dall'intervallo"),
            ("{\"reason\": \"boh\"}", "non ha un punteggio"),
            ("{\"score\": \"alto\"}", "non ha un punteggio"),
            ("Direi molto bene", "non contiene un JSON"),
        ] {
            let (r, _) = judged(answer, None).await;
            assert!(!r.passed && r.score == 0.0 && r.detail.contains(expect), "{answer}: {r:?}");
        }
        let host = Judge(Default::default(), Err("modello spento".into()));
        let dir = tempfile::tempdir().unwrap();
        let run = TaskRun::default();
        let spec = GraderSpec::Judge { criteria: "x".into(), profile: None, threshold: None };
        let r = grade(&spec, &GradeContext { workdir: dir.path(), run: &run, host: &host, prompt: "", changes: &[] }).await;
        assert!(!r.passed && r.detail.contains("il giudice non ha risposto") && r.detail.contains("modello spento"));
        assert!(
            host.0.lock().unwrap()[0].1.contains("(nessuna risposta)") && host.0.lock().unwrap()[0].1.contains("(nessun file cambiato)")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn commands_run_in_the_working_folder_with_a_limit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("note.md"), "x").unwrap();
        let cmd = |run: &str, expect_exit: i32, timeout_s: Option<u64>| GraderSpec::Command { run: run.into(), expect_exit, timeout_s };
        assert!(run(cmd("test -s note.md", 0, None), dir.path(), "").await.passed);
        let r = run(cmd("echo troppo tardi >&2; exit 3", 0, None), dir.path(), "").await;
        assert!(!r.passed && r.detail.contains("uscito con 3, atteso 0") && r.detail.contains("troppo tardi"), "{}", r.detail);
        assert!(run(cmd("exit 3", 3, None), dir.path(), "").await.passed, "a non-zero exit can be the expected one");
        let r = run(cmd("sleep 5", 0, Some(1)), dir.path(), "").await;
        assert!(!r.passed && r.detail.contains("entro 1 secondi"), "{}", r.detail);
    }
}
