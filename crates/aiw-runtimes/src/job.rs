//! A supervised batch process: what a fine-tuning runs as. Unlike a sidecar it has no port to
//! wait for: it runs to the end, writes every line it prints to a log file, and can be cancelled
//! (the whole process group is signalled, so the workers a trainer spawns go with it).

use crate::supervisor::kill_group;
use aiw_core::{Error, Result};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// How long a cancelled process gets to exit after the polite signal before it is killed.
const CANCEL_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct JobSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    /// Every line of stdout and stderr is appended here.
    pub log_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobEnd {
    /// The process exited with this code (`None`: ended by a signal).
    Exited(Option<i32>),
    /// The caller cancelled it and it is gone.
    Cancelled,
}

impl JobEnd {
    pub fn success(&self) -> bool {
        *self == JobEnd::Exited(Some(0))
    }
}

/// Runs `spec` to the end or until `cancel` fires. `on_line` hears every line (for progress).
pub async fn run_logged(spec: JobSpec, cancel: CancellationToken, mut on_line: impl FnMut(&str) + Send) -> Result<JobEnd> {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};
    if let Some(dir) = spec.log_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut log = std::fs::OpenOptions::new().create(true).append(true).open(&spec.log_path)?;
    let mut cmd = tokio::process::Command::new(&spec.program);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("PYTHONUNBUFFERED", "1");
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    // Own process group: cancelling signals the trainer and the workers it started.
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().map_err(|e| Error::Runtime(format!("impossibile avviare {}: {e}", spec.program.display())))?;
    let pid = child.id();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    for pipe in [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(crate::supervisor::clean_log_line(&line)).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    let mut handle = |line: String, log: &mut std::fs::File| {
        let _ = writeln!(log, "{line}");
        on_line(&line);
    };
    let mut cancelled = false;
    let mut status = None;
    // Lines and the end of the process are handled in one loop, so the log is in order and complete.
    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Some(l) => handle(l, &mut log),
                None => break,
            },
            _ = cancel.cancelled(), if !cancelled => {
                cancelled = true;
                if let Some(pid) = pid { kill_group(pid, false); }
                let waited = tokio::time::timeout(CANCEL_GRACE, child.wait()).await;
                if waited.is_err() {
                    if let Some(pid) = pid { kill_group(pid, true); }
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                } else if let Ok(Ok(s)) = waited {
                    status = Some(s);
                }
                if let Some(pid) = pid { kill_group(pid, true); }
            }
        }
    }
    // The pipes are closed: the process is done (or killed). Drain what the channel still holds.
    while let Ok(l) = rx.try_recv() {
        handle(l, &mut log);
    }
    let status = match status {
        Some(s) => s,
        None => child.wait().await.map_err(|e| Error::Runtime(format!("{}: {e}", spec.program.display())))?,
    };
    let _ = log.flush();
    Ok(if cancelled { JobEnd::Cancelled } else { JobEnd::Exited(status.code()) })
}

/// The last `n` lines of a log file (all of them when it is shorter), oldest first. An absent file is empty.
pub fn tail(path: &std::path::Path, n: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else { return vec![] };
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|l| l.to_string()).collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn sh(dir: &std::path::Path, script: &str) -> JobSpec {
        JobSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: vec![],
            cwd: dir.to_path_buf(),
            log_path: dir.join("logs").join("job.log"),
        }
    }

    #[tokio::test]
    async fn output_goes_to_the_log_and_to_the_listener() {
        let dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(vec![]));
        let s2 = seen.clone();
        let end = run_logged(sh(dir.path(), "echo uno; echo due 1>&2; exit 3"), CancellationToken::new(), move |l| {
            s2.lock().unwrap().push(l.to_string())
        })
        .await
        .unwrap();
        assert_eq!(end, JobEnd::Exited(Some(3)));
        assert!(!end.success());
        let mut lines = seen.lock().unwrap().clone();
        lines.sort();
        assert_eq!(lines, ["due", "uno"]);
        let mut logged = tail(&dir.path().join("logs").join("job.log"), 10);
        logged.sort();
        assert_eq!(logged, ["due", "uno"]);
        assert_eq!(tail(&dir.path().join("logs").join("job.log"), 1).len(), 1);
        assert!(tail(&dir.path().join("none.log"), 5).is_empty());
        assert!(run_logged(sh(dir.path(), "exit 0"), CancellationToken::new(), |_| {}).await.unwrap().success());
    }

    #[tokio::test]
    async fn cancelling_stops_the_process_and_its_workers() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("worker.pid");
        let script = format!("sleep 60 & echo $! > {}; echo started; wait", pidfile.display());
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        let task = tokio::spawn(run_logged(sh(dir.path(), &script), cancel, move |l| {
            if l == "started" {
                c2.cancel();
            }
        }));
        let end = tokio::time::timeout(Duration::from_secs(20), task).await.expect("cancelled in time").unwrap().unwrap();
        assert_eq!(end, JobEnd::Cancelled);
        let worker: u32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let out = std::process::Command::new("ps").args(["-o", "stat=", "-p", &worker.to_string()]).output().unwrap();
        let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert!(state.is_empty() || state.starts_with('Z'), "the worker is still alive: {state:?}");
    }

    #[tokio::test]
    async fn a_program_that_is_not_there_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut spec = sh(dir.path(), "");
        spec.program = "/nonexistent/prog".into();
        assert!(run_logged(spec, CancellationToken::new(), |_| {}).await.is_err());
    }
}
