//! Sidecar supervisor: spawns runtime processes from launch recipes, waits for readiness,
//! keeps a ring buffer of logs and stops everything on shutdown or kill switch.

use crate::manifest::{expand_args, Launch, RuntimeManifest};
use aiw_core::{Error, Result};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LOG_LINES: usize = 400;

pub struct SidecarHandle {
    pub runtime_id: String,
    pub endpoint: String,
    pub port: u16,
    pub model: Option<String>,
    pub started_at: Instant,
    pub logs: Arc<Mutex<VecDeque<String>>>,
    child: Option<tokio::process::Child>,
}

#[derive(Default)]
pub struct Supervisor {
    running: tokio::sync::Mutex<HashMap<String, SidecarHandle>>,
}

pub struct LaunchSpec<'a> {
    pub manifest: &'a RuntimeManifest,
    pub settings: &'a Map<String, Value>,
    pub binary: PathBuf,
    pub model: Option<String>,
    pub model_path: Option<PathBuf>,
    pub models_dir: PathBuf,
}

fn free_port() -> Result<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(l.local_addr()?.port())
}

fn push_log(logs: &Arc<Mutex<VecDeque<String>>>, line: String) {
    let mut l = logs.lock().unwrap();
    if l.len() >= LOG_LINES {
        l.pop_front();
    }
    l.push_back(line);
}

impl Supervisor {
    pub async fn is_running(&self, id: &str) -> bool {
        let mut map = self.running.lock().await;
        if let Some(h) = map.get_mut(id) {
            if let Some(child) = h.child.as_mut() {
                if let Ok(Some(_)) = child.try_wait() {
                    map.remove(id);
                    return false;
                }
            }
            return true;
        }
        false
    }

    pub async fn endpoint(&self, id: &str) -> Option<(String, Option<String>)> {
        self.running.lock().await.get(id).map(|h| (h.endpoint.clone(), h.model.clone()))
    }

    pub async fn logs(&self, id: &str) -> Vec<String> {
        match self.running.lock().await.get(id) {
            Some(h) => h.logs.lock().unwrap().iter().cloned().collect(),
            None => vec![],
        }
    }

    pub async fn running_ids(&self) -> Vec<String> {
        self.running.lock().await.keys().cloned().collect()
    }

    /// Starts the sidecar and waits until its health endpoint answers.
    pub async fn start(&self, spec: LaunchSpec<'_>, http: &reqwest::Client) -> Result<String> {
        let id = spec.manifest.id.clone();
        if self.is_running(&id).await {
            self.stop(&id).await?;
        }
        let launch: &Launch =
            spec.manifest.launch.as_ref().ok_or_else(|| Error::Unsupported(format!("{id} non ha una ricetta di avvio")))?;
        if launch.requires_model && spec.model.is_none() && spec.model_path.is_none() {
            return Err(Error::Invalid(format!(
                "{} richiede un modello: scegline uno nel profilo o nel pannello del runtime",
                spec.manifest.name
            )));
        }
        let configured_port = spec.settings.get("port").and_then(|v| v.as_u64()).filter(|p| *p > 0).map(|p| p as u16);
        let port = launch.fixed_port.or(configured_port).map(Ok).unwrap_or_else(free_port)?;
        let mut vars: BTreeMap<String, String> = BTreeMap::new();
        for (k, v) in spec.settings {
            if let Some(s) = v.as_str() {
                vars.insert(k.clone(), s.to_string());
            } else if v.is_number() {
                vars.insert(k.clone(), v.to_string());
            }
        }
        vars.insert("port".into(), port.to_string());
        vars.insert("host".into(), "127.0.0.1".into());
        vars.insert("models_dir".into(), spec.models_dir.display().to_string());
        vars.insert("model".into(), spec.model.clone().unwrap_or_default());
        vars.insert("model_path".into(), spec.model_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
        let args = expand_args(launch, spec.settings, &vars);
        let mut cmd = tokio::process::Command::new(&spec.binary);
        cmd.args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in &launch.env {
            let mut val = v.clone();
            for (pk, pv) in &vars {
                val = val.replace(&format!("{{{pk}}}"), pv);
            }
            cmd.env(k, val);
        }
        // Make shared libraries next to the binary discoverable (llama.cpp builds ship libggml*.so).
        if let Some(dir) = spec.binary.parent() {
            #[cfg(target_os = "linux")]
            {
                let prev = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
                cmd.env("LD_LIBRARY_PATH", if prev.is_empty() { dir.display().to_string() } else { format!("{}:{prev}", dir.display()) });
            }
            #[cfg(target_os = "macos")]
            {
                cmd.env("DYLD_FALLBACK_LIBRARY_PATH", dir);
            }
        }
        tracing::info!(runtime = %id, binary = %spec.binary.display(), ?args, "starting sidecar");
        let mut child = cmd.spawn().map_err(|e| Error::Runtime(format!("impossibile avviare {}: {e}", spec.binary.display())))?;
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        for pipe in [
            child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let logs = logs.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let mut lines = tokio::io::BufReader::new(pipe).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    push_log(&logs, line);
                }
            });
        }
        let base = format!("http://127.0.0.1:{port}");
        let endpoint = format!("{base}{}", launch.api_base);
        let health = format!("{base}{}", launch.health);
        let deadline = Instant::now() + Duration::from_secs(launch.ready_timeout_s);
        loop {
            if !launch.detached {
                if let Ok(Some(status)) = child.try_wait() {
                    let tail: Vec<String> = logs.lock().unwrap().iter().rev().take(12).rev().cloned().collect();
                    return Err(Error::Runtime(format!("{} si è chiuso ({status}).\n{}", spec.manifest.name, tail.join("\n"))));
                }
            }
            if let Ok(r) = http.get(&health).timeout(Duration::from_secs(2)).send().await {
                if r.status().is_success() {
                    break;
                }
            }
            if Instant::now() > deadline {
                let _ = child.kill().await;
                return Err(Error::Runtime(format!("{} non è pronto dopo {} s", spec.manifest.name, launch.ready_timeout_s)));
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        tracing::info!(runtime = %id, %endpoint, "sidecar ready");
        let handle = SidecarHandle {
            runtime_id: id.clone(),
            endpoint: endpoint.clone(),
            port,
            model: spec.model.clone(),
            started_at: Instant::now(),
            logs,
            child: if launch.detached { None } else { Some(child) },
        };
        self.running.lock().await.insert(id, handle);
        Ok(endpoint)
    }

    pub async fn stop(&self, id: &str) -> Result<bool> {
        let h = self.running.lock().await.remove(id);
        match h {
            Some(mut h) => {
                if let Some(mut c) = h.child.take() {
                    let _ = c.start_kill();
                    let _ = tokio::time::timeout(Duration::from_secs(5), c.wait()).await;
                }
                tracing::info!(runtime = %id, "sidecar stopped");
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub async fn stop_all(&self) -> usize {
        let ids = self.running_ids().await;
        for id in &ids {
            let _ = self.stop(id).await;
        }
        ids.len()
    }
}

/// Finds the executable for a manifest: explicit `binary` setting, PATH, then `<data>/bin`.
pub fn find_binary(manifest: &RuntimeManifest, settings: &Map<String, Value>, extra_dirs: &[PathBuf]) -> Option<PathBuf> {
    if let Some(b) = settings.get("binary").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(b);
        return p.exists().then_some(p);
    }
    let mut names: Vec<String> = manifest.detect.binaries.clone();
    // Python-only runtimes are detected by importing their module, not by finding python3.
    if names.is_empty() && manifest.detect.python_modules.is_empty() {
        if let Some(l) = &manifest.launch {
            names.push(l.command.clone());
        }
    }
    for n in &names {
        if let Ok(p) = which::which(n) {
            return Some(p);
        }
        for d in extra_dirs {
            for cand in [d.join(n), d.join(format!("{n}.exe"))] {
                if is_executable(&cand) {
                    return Some(cand);
                }
            }
        }
    }
    None
}

fn is_executable(p: &Path) -> bool {
    p.is_file()
}
