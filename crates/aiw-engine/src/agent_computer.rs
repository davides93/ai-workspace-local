//! The agent's computer: one container per agent, started and stopped by the person, never by a model.
//!
//! First piece: a Chromium the agent drives over the DevTools Protocol, on the host's loopback only, with the agent's own
//! workspace folder (`<data>/agents/<id>/workspace`) mounted as `/workspace`. The image is built on this computer from
//! `assets/agent-computer/` (nothing pulled from an AIW registry); the compose file is written next to the agent's folder
//! and started with `docker compose -p aiw-computer-<id>`. No secret goes into the container's environment.
//!
//! Not here yet: a shell inside the container (`sandbox.backend = container`), the screen view for a paired phone, and
//! digest-pinned base images. Never run against a live Docker/Podman daemon by the author: the command lines are tested
//! against a fake runner; see `docs/STATUS.md`.
//!
//! Every action is an explicit one: `up` can take minutes and download an image, so it only runs when asked.

use crate::Engine;
use aiw_core::api::AgentComputer;
use aiw_core::{Error, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const DOCKERFILE: &str = include_str!("../assets/agent-computer/Dockerfile");
const COMPOSE: &str = include_str!("../assets/agent-computer/compose.yml");

/// A long build is fine; a hung command is not.
const T_UP: Duration = Duration::from_secs(30 * 60);
const T_QUICK: Duration = Duration::from_secs(20);

/// What a command printed and whether it succeeded.
pub(crate) struct CmdOut {
    pub ok: bool,
    pub text: String,
}

/// Runs `cmd args` in `cwd` with `env` added, or `None` when it cannot start or times out. A fake in tests.
pub(crate) type Runner = dyn Fn(&str, &[String], &[(String, String)], &Path, Duration) -> Option<CmdOut> + Send + Sync;

/// Progress of the actions asked in this process: `starting`, or the words of a failure. Not stored: after a restart the
/// answer comes from the engine's own `ps`.
fn ops() -> &'static Mutex<HashMap<String, Op>> {
    static OPS: OnceLock<Mutex<HashMap<String, Op>>> = OnceLock::new();
    OPS.get_or_init(Default::default)
}
#[derive(Clone)]
enum Op {
    Starting,
    Failed(String),
}

/// An id that makes a valid compose project name and a folder on every system: lowercase letters, digits, `-`, `_`.
pub fn computer_id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub fn project_name(agent_id: &str) -> String {
    format!("aiw-computer-{agent_id}")
}

/// `docker compose -p <project> -f <file> <args…>`.
pub(crate) fn compose_args(project: &str, file: &Path, rest: &[&str]) -> Vec<String> {
    let mut v = vec!["compose".to_string(), "-p".into(), project.into(), "-f".into(), file.display().to_string()];
    v.extend(rest.iter().map(|s| s.to_string()));
    v
}

/// `[{"Service":"browser","State":"running"}]` or one object per line, whichever this compose version prints.
pub(crate) fn parse_ps_state(out: &str) -> Option<String> {
    let t = out.trim();
    if t.is_empty() {
        return None;
    }
    let values: Vec<serde_json::Value> = if t.starts_with('[') {
        serde_json::from_str(t).ok()?
    } else {
        t.lines().filter_map(|l| serde_json::from_str(l.trim()).ok()).collect()
    };
    let browser = values.iter().find(|v| v["Service"].as_str() == Some("browser")).or(values.first())?;
    browser["State"].as_str().map(|s| s.to_ascii_lowercase())
}

/// `127.0.0.1:49153` (or `0.0.0.0:49153` / `[::]:49153`) → `http://127.0.0.1:49153`: always the loopback address, whatever
/// the engine printed.
pub(crate) fn parse_port(out: &str) -> Option<String> {
    let line = out.lines().map(str::trim).find(|l| !l.is_empty())?;
    let port: u16 = line.rsplit(':').next()?.trim().parse().ok().filter(|p| *p > 0)?;
    Some(format!("http://127.0.0.1:{port}"))
}

fn real_runner() -> Box<Runner> {
    Box::new(|cmd, args, env, cwd, timeout| {
        use std::io::Read;
        use std::process::{Command, Stdio};
        let mut c = Command::new(cmd);
        c.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for (k, v) in env {
            c.env(k, v);
        }
        let mut child = c.spawn().ok()?;
        let (mut out, mut err) = (child.stdout.take()?, child.stderr.take()?);
        let (tx, rx) = std::sync::mpsc::channel();
        let tx2 = tx.clone();
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = out.read_to_string(&mut s);
            let _ = tx.send((true, s));
        });
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = err.read_to_string(&mut s);
            let _ = tx2.send((false, s));
        });
        let start = std::time::Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if start.elapsed() > timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => return None,
            }
        };
        let (mut stdout, mut stderr) = (String::new(), String::new());
        for _ in 0..2 {
            if let Ok((is_out, s)) = rx.recv_timeout(Duration::from_millis(500)) {
                if is_out {
                    stdout = s;
                } else {
                    stderr = s;
                }
            }
        }
        // stdout is what is parsed; stderr is what a failure says.
        Some(CmdOut { ok: status.success(), text: if status.success() { stdout } else { format!("{stdout}{stderr}") } })
    })
}

/// The few lines of a failure worth showing: the last non-empty ones, no longer than a short message.
fn failure_words(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let tail = lines.iter().rev().take(3).rev().copied().collect::<Vec<_>>().join(" · ");
    let t: String = tail.chars().take(300).collect();
    if t.is_empty() {
        "il comando non ha risposto".into()
    } else {
        t
    }
}

impl Engine {
    fn computer_dir(&self, agent_id: &str) -> Result<PathBuf> {
        if !computer_id_ok(agent_id) {
            return Err(Error::Invalid(
                "l'id dell'agente deve avere solo lettere minuscole, cifre, «-» e «_» per avere un computer".into(),
            ));
        }
        Ok(self.0.dirs.agents().join(agent_id).join("computer"))
    }

    /// Writes the files of the computer and the agent's workspace folder; returns the compose file and the environment.
    fn computer_files(&self, agent_id: &str) -> Result<(PathBuf, Vec<(String, String)>)> {
        let dir = self.computer_dir(agent_id)?;
        let ws = self.dedicated_workspace_path(agent_id)?;
        std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::create_dir_all(&ws))
            .map_err(|e| Error::Runtime(format!("cartella del computer non creata: {e}")))?;
        let file = dir.join("compose.yml");
        for (name, body) in [("Dockerfile", DOCKERFILE), ("compose.yml", COMPOSE)] {
            std::fs::write(dir.join(name), body).map_err(|e| Error::Runtime(format!("{name} non scritto: {e}")))?;
        }
        let mut env = vec![("AIW_WORKSPACE".to_string(), ws.display().to_string())];
        // On Linux the container writes the workspace as the folder's owner, so the person can read and delete what it makes.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let Ok(m) = std::fs::metadata(&ws) {
                env.push(("AIW_UID".into(), m.uid().to_string()));
                env.push(("AIW_GID".into(), m.gid().to_string()));
            }
        }
        Ok((file, env))
    }

    /// Where the computer of `agent_id` stands. Read-only: it only asks the engine.
    pub async fn agent_computer(&self, agent_id: &str) -> Result<AgentComputer> {
        self.agent(agent_id)?;
        let e = self.clone();
        let id = agent_id.to_string();
        tokio::task::spawn_blocking(move || e.agent_computer_with(&id, &real_runner(), None))
            .await
            .map_err(|e| Error::Runtime(e.to_string()))?
    }

    pub(crate) fn agent_computer_with(&self, agent_id: &str, run: &Runner, engine: Option<&str>) -> Result<AgentComputer> {
        let project = project_name(agent_id);
        let mut out = AgentComputer { agent_id: agent_id.into(), project: project.clone(), state: "absent".into(), ..Default::default() };
        match ops().lock().unwrap().get(agent_id).cloned() {
            Some(Op::Starting) => {
                out.state = "starting".into();
                return Ok(out);
            }
            Some(Op::Failed(why)) => {
                out.state = "error".into();
                out.detail = Some(why);
                return Ok(out);
            }
            None => {}
        }
        let dir = self.computer_dir(agent_id)?;
        let file = dir.join("compose.yml");
        // Nothing was ever written: there is nothing to ask.
        if !file.exists() {
            return Ok(out);
        }
        let engine = engine.map(String::from).or_else(|| aiw_hw::probe_containers().engine).unwrap_or_else(|| "docker".into());
        out.engine = Some(engine.clone());
        let ps = run(&engine, &compose_args(&project, &file, &["ps", "-a", "--format", "json"]), &[], &dir, T_QUICK);
        let Some(ps) = ps.filter(|p| p.ok) else {
            out.state = "error".into();
            out.detail = Some("non riesco a chiedere a Docker: è acceso?".into());
            return Ok(out);
        };
        match parse_ps_state(&ps.text).as_deref() {
            None => out.state = "absent".into(),
            Some("running") => {
                out.state = "running".into();
                out.cdp_url = run(&engine, &compose_args(&project, &file, &["port", "browser", "9222"]), &[], &dir, T_QUICK)
                    .filter(|p| p.ok)
                    .and_then(|p| parse_port(&p.text));
            }
            Some(_) => out.state = "stopped".into(),
        }
        Ok(out)
    }

    /// Starts the computer of `agent_id`: writes its files, builds the image (first time: minutes and a download) and starts
    /// the container, in the background. Returns at once with `starting`; ask `agent_computer` for the end.
    pub async fn agent_computer_up(&self, agent_id: &str) -> Result<AgentComputer> {
        self.agent(agent_id)?;
        let status = self.containers_status().await;
        let Some(engine) = status.engine.clone().filter(|_| status.state == "ready") else {
            return Err(Error::Conflict(match status.state.as_str() {
                "engine_stopped" => "Docker è installato ma spento: avvialo e riprova".into(),
                "compose_missing" => "manca Docker Compose".into(),
                "unsupported" => "su questo dispositivo non si possono avviare container".into(),
                _ => "Docker non è installato".into(),
            }));
        };
        let (file, env) = self.computer_files(agent_id)?;
        {
            let mut o = ops().lock().unwrap();
            if matches!(o.get(agent_id), Some(Op::Starting)) {
                return Err(Error::Conflict("il computer di questo agente si sta già avviando".into()));
            }
            o.insert(agent_id.to_string(), Op::Starting);
        }
        let (id, project) = (agent_id.to_string(), project_name(agent_id));
        tokio::task::spawn_blocking(move || {
            let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
            let done = real_runner()(&engine, &compose_args(&project, &file, &["up", "-d", "--build"]), &env, &dir, T_UP);
            let mut o = ops().lock().unwrap();
            match done {
                Some(c) if c.ok => {
                    o.remove(&id);
                }
                Some(c) => {
                    o.insert(id, Op::Failed(failure_words(&c.text)));
                }
                None => {
                    o.insert(id, Op::Failed("l'avvio non è finito entro 30 minuti o Docker non risponde".into()));
                }
            }
        });
        Ok(AgentComputer {
            agent_id: agent_id.into(),
            project: project_name(agent_id),
            state: "starting".into(),
            engine: Some(status.engine.unwrap_or_default()),
            ..Default::default()
        })
    }

    /// Stops and removes the container. The agent's workspace folder (and the browser profile in it) stays.
    pub async fn agent_computer_down(&self, agent_id: &str) -> Result<AgentComputer> {
        self.agent(agent_id)?;
        let dir = self.computer_dir(agent_id)?;
        let file = dir.join("compose.yml");
        ops().lock().unwrap().remove(agent_id);
        if file.exists() {
            let engine = aiw_hw::probe_containers().engine.unwrap_or_else(|| "docker".into());
            let args = compose_args(&project_name(agent_id), &file, &["down", "--remove-orphans"]);
            let d = dir.clone();
            let r = tokio::task::spawn_blocking(move || real_runner()(&engine, &args, &[], &d, T_QUICK * 3))
                .await
                .map_err(|e| Error::Runtime(e.to_string()))?;
            if !r.is_some_and(|c| c.ok) {
                return Err(Error::Runtime("Docker non ha fermato il computer: è acceso?".into()));
            }
        }
        self.agent_computer(agent_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_project_names() {
        for ok in ["assistant", "ricerca-1", "a_b"] {
            assert!(computer_id_ok(ok), "{ok}");
        }
        for bad in ["", "Assistant", "a b", "../x", "-x", "a.b", &"x".repeat(41)] {
            assert!(!computer_id_ok(bad), "{bad}");
        }
        assert_eq!(project_name("alfa"), "aiw-computer-alfa");
    }

    #[test]
    fn compose_command_lines() {
        let a = compose_args("aiw-computer-alfa", Path::new("/d/compose.yml"), &["up", "-d", "--build"]);
        assert_eq!(a, ["compose", "-p", "aiw-computer-alfa", "-f", "/d/compose.yml", "up", "-d", "--build"]);
    }

    #[test]
    fn reads_ps_in_both_shapes() {
        assert_eq!(parse_ps_state(r#"[{"Service":"browser","State":"running"}]"#).as_deref(), Some("running"));
        assert_eq!(parse_ps_state("{\"Service\":\"browser\",\"State\":\"Exited\"}\n").as_deref(), Some("exited"));
        assert_eq!(parse_ps_state("  \n"), None);
        assert_eq!(parse_ps_state("[]"), None);
    }

    #[test]
    fn the_port_is_always_loopback() {
        assert_eq!(parse_port("127.0.0.1:49153\n").as_deref(), Some("http://127.0.0.1:49153"));
        assert_eq!(parse_port("0.0.0.0:32768").as_deref(), Some("http://127.0.0.1:32768"));
        assert_eq!(parse_port("[::]:5000").as_deref(), Some("http://127.0.0.1:5000"));
        assert_eq!(parse_port("nonsense"), None);
        assert_eq!(parse_port(""), None);
        assert_eq!(parse_port("127.0.0.1:0"), None);
    }

    #[test]
    fn failures_are_short() {
        let long = format!("{}\nerrore uno\n\nerrore due\nerrore tre\nerrore quattro", "x".repeat(1000));
        assert_eq!(failure_words(&long), "errore due · errore tre · errore quattro");
        assert_eq!(failure_words("  \n"), "il comando non ha risposto");
    }

    /// What is written for the container: no secret, the loopback publish, the dropped capabilities, the agent's workspace.
    #[test]
    fn the_compose_file_is_closed() {
        assert!(COMPOSE.contains("127.0.0.1::9222"), "published on the loopback only");
        assert!(COMPOSE.contains("cap_drop: [ALL]") && COMPOSE.contains("no-new-privileges:true") && COMPOSE.contains("read_only: true"));
        assert!(COMPOSE.contains("${AIW_WORKSPACE}:/workspace"));
        assert!(!COMPOSE.lines().any(|l| l.trim_start().starts_with("environment:")), "no environment passes into the container");
        assert!(!COMPOSE.contains("privileged") && !COMPOSE.contains("docker.sock") && !COMPOSE.contains("network_mode: host"));
        assert!(DOCKERFILE.contains("USER 1000:1000"));
    }

    /// The state the person sees, from the engine's answers (a fake runner), and what is written for the container.
    #[tokio::test]
    async fn state_comes_from_the_engine_and_files_stay_in_the_agents_folder() {
        use crate::tests_isolation::{apart, engine_at};
        let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let e = engine_at(data.path(), ws.path()).await;
        e.save_agent(apart("alfa", &[])).unwrap();
        let none: &Runner = &|_, _, _, _, _| panic!("nothing was ever written: nothing is asked");
        assert_eq!(e.agent_computer_with("alfa", none, Some("docker")).unwrap().state, "absent");
        assert!(e.agent_computer_with("../x", none, Some("docker")).is_err(), "a bad id never reaches a folder");

        let (file, env) = e.computer_files("alfa").unwrap();
        assert_eq!(file, data.path().join("agents/alfa/computer/compose.yml"));
        assert!(file.with_file_name("Dockerfile").exists());
        let workspace = data.path().join("agents/alfa/workspace");
        assert!(workspace.is_dir());
        assert!(env.iter().any(|(k, v)| k == "AIW_WORKSPACE" && Path::new(v) == workspace));
        assert!(!env.iter().any(|(k, _)| k.contains("TOKEN") || k.contains("KEY") || k.contains("SECRET")), "no secret in the environment");

        let running: &Runner = &|cmd, args, _, _, _| {
            assert_eq!(cmd, "docker");
            let text = if args.iter().any(|a| a == "ps") {
                r#"[{"Service":"browser","State":"running"}]"#.to_string()
            } else {
                "0.0.0.0:49153\n".to_string()
            };
            Some(CmdOut { ok: true, text })
        };
        let st = e.agent_computer_with("alfa", running, Some("docker")).unwrap();
        assert_eq!(
            (st.state.as_str(), st.cdp_url.as_deref(), st.project.as_str()),
            ("running", Some("http://127.0.0.1:49153"), "aiw-computer-alfa")
        );

        let stopped: &Runner = &|_, _, _, _, _| Some(CmdOut { ok: true, text: "{\"Service\":\"browser\",\"State\":\"exited\"}".into() });
        assert_eq!(e.agent_computer_with("alfa", stopped, Some("docker")).unwrap().state, "stopped");
        let down: &Runner = &|_, _, _, _, _| None;
        let st = e.agent_computer_with("alfa", down, Some("docker")).unwrap();
        assert_eq!(st.state, "error");
        assert!(st.detail.unwrap().contains("Docker"));
    }
}
