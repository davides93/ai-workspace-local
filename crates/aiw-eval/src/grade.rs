//! The deterministic graders.

use crate::host::TaskRun;
use crate::report::GraderResult;
use crate::suite::{resolve_inside, GraderSpec, DEFAULT_COMMAND_TIMEOUT_S};
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
    use serde_json::json;

    async fn run(spec: GraderSpec, dir: &Path, output: &str) -> GraderResult {
        let run = TaskRun { final_text: output.into(), ..Default::default() };
        grade(&spec, &GradeContext { workdir: dir, run: &run }).await
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
        assert!(grade(&schema, &GradeContext { workdir: dir.path(), run: &with_structured }).await.passed);
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
