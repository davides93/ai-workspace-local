//! Sidecar supervisor: spawns runtime processes from launch recipes, waits for readiness,
//! keeps a ring buffer of logs and stops everything on shutdown or kill switch.

use crate::manifest::{expand_args, expand_env, Launch, RuntimeManifest};
use aiw_core::api::AdapterRef;
use aiw_core::{Error, Result};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LOG_LINES: usize = 400;
/// How long `stop` waits for a sidecar to exit after SIGTERM before killing it.
const STOP_GRACE: Duration = Duration::from_secs(3);

pub struct SidecarHandle {
    pub runtime_id: String,
    pub endpoint: String,
    pub port: u16,
    pub model: Option<String>,
    /// Context window the process was launched with (profile context or the runtime setting),
    /// `None` when the runtime has no context setting.
    pub ctx: Option<u32>,
    /// LoRA adapters the process was launched with.
    pub adapters: Vec<AdapterRef>,
    pub started_at: Instant,
    pub logs: Arc<Mutex<VecDeque<String>>>,
    /// Local path the process was launched with (`{model}` / `{model_path}`), when the model
    /// was resolved to one: single-model servers may serve the model under that name.
    pub model_path: Option<PathBuf>,
    /// Process id of the sidecar, also the id of its process group (Unix).
    pid: Option<u32>,
    child: Option<tokio::process::Child>,
}

impl Drop for SidecarHandle {
    fn drop(&mut self) {
        // `kill_on_drop` only reaches the parent: take the workers down with it.
        if self.child.is_some() {
            if let Some(pid) = self.pid {
                kill_group(pid, true);
            }
        }
    }
}

/// Sends a termination signal to a sidecar and every process it spawned: on Unix the sidecar is
/// its own process-group leader (`process_group(0)`), so the signal goes to the whole group;
/// on Windows `taskkill /T` walks the process tree. `force`: SIGKILL / `/F` instead of SIGTERM.
/// Best effort, errors are ignored (the group may already be gone).
pub(crate) fn kill_group(pid: u32, force: bool) {
    #[cfg(unix)]
    {
        let sig = if force { "KILL" } else { "TERM" };
        let _ = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("kill -s {sig} -- -{pid} 2>/dev/null"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = force; // a console process tree does not reliably honour a polite request
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, force);
    }
}

#[derive(Default)]
pub struct Supervisor {
    last_logs: Mutex<HashMap<String, Arc<Mutex<VecDeque<String>>>>>,
    running: tokio::sync::Mutex<HashMap<String, SidecarHandle>>,
}

pub struct LaunchSpec<'a> {
    pub manifest: &'a RuntimeManifest,
    pub settings: &'a Map<String, Value>,
    pub binary: PathBuf,
    pub model: Option<String>,
    pub model_path: Option<PathBuf>,
    pub models_dir: PathBuf,
    /// Context window of the inference profile the sidecar is started for. It overrides the
    /// runtime's own context setting (`-c`, `--max-model-len`, `--contextsize`,
    /// `OLLAMA_CONTEXT_LENGTH`…), see [`RuntimeManifest::apply_ctx`].
    pub ctx: Option<u32>,
    /// LoRA adapters of the inference profile, loaded at launch for runtimes that declare
    /// `adapters` in their manifest (see [`crate::adapters`]).
    pub adapters: &'a [AdapterRef],
}

/// What [`Supervisor::start`] executes, as plain data so the expansion of a launch recipe can be
/// unit-tested without spawning anything.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchPlan {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Context window actually passed to the process (after clamping to the schema range).
    pub ctx: Option<u32>,
}

/// Expands a launch recipe for `spec` on `port`.
///
/// Placeholders: every string/number setting, `{port}`, `{host}`, `{models_dir}`,
/// `{model_path}` (resolved file, empty when unresolved) and:
/// - `{model}`: the resolved local path (file or directory) when the profile's model is a local
///   model, otherwise the model id as given (a Hugging Face repo the runtime downloads itself).
///   This is what directory-format servers (mlx-lm, vLLM, SGLang, TensorRT-LLM, mistral.rs)
///   need: their `{model}` used to receive the lowercase folder name of the local model;
/// - `{model_id}`: the model id, always (OVMS `--source_model` wants the hub name).
pub fn plan_launch(spec: &LaunchSpec<'_>, launch: &Launch, port: u16) -> LaunchPlan {
    let mut settings = spec.settings.clone();
    let ctx = match spec.ctx {
        Some(c) => spec.manifest.apply_ctx(&mut settings, c),
        None => spec.manifest.ctx_setting().and_then(|k| settings.get(&k).and_then(|v| v.as_u64())).map(|c| c as u32),
    };
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in &settings {
        if let Some(s) = v.as_str() {
            vars.insert(k.clone(), s.to_string());
        } else if v.is_number() {
            vars.insert(k.clone(), v.to_string());
        }
    }
    let model_id = spec.model.clone().unwrap_or_default();
    let model_path = spec.model_path.as_ref().map(|p| p.display().to_string());
    vars.insert("port".into(), port.to_string());
    vars.insert("host".into(), "127.0.0.1".into());
    vars.insert("models_dir".into(), spec.models_dir.display().to_string());
    vars.insert("model".into(), model_path.clone().unwrap_or_else(|| model_id.clone()));
    vars.insert("model_id".into(), model_id);
    vars.insert("model_path".into(), model_path.unwrap_or_default());
    let mut args = expand_args(launch, &settings, &vars);
    args.extend(crate::adapters::launch_args(spec.manifest.adapters.as_ref(), spec.adapters));
    LaunchPlan { args, env: expand_env(launch, &vars), ctx }
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
    l.push_back(clean_log_line(&line));
}

pub(crate) fn clean_log_line(line: &str) -> String {
    let mut chars = line.chars();
    let mut clean = String::with_capacity(line.len());
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    chars.by_ref().find(|c| ('@'..='~').contains(c));
                }
                Some(']') => {
                    let mut escape = false;
                    for c in chars.by_ref() {
                        if c == '\u{7}' || (escape && c == '\\') {
                            break;
                        }
                        escape = c == '\u{1b}';
                    }
                }
                _ => {}
            }
        } else if ch == '\t' || !ch.is_control() {
            clean.push(ch);
        }
    }
    clean
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

    /// Local path the running sidecar was launched with, if its model resolved to one.
    pub async fn served_model_path(&self, id: &str) -> Option<PathBuf> {
        self.running.lock().await.get(id).and_then(|h| h.model_path.clone())
    }

    /// Context window the running sidecar was launched with (`None`: not running, or the
    /// runtime has no context setting).
    pub async fn served_ctx(&self, id: &str) -> Option<u32> {
        self.running.lock().await.get(id).and_then(|h| h.ctx)
    }

    /// LoRA adapters the running sidecar was launched with (empty: none, or not running).
    pub async fn served_adapters(&self, id: &str) -> Vec<AdapterRef> {
        self.running.lock().await.get(id).map(|h| h.adapters.clone()).unwrap_or_default()
    }

    pub async fn logs(&self, id: &str) -> Vec<String> {
        self.last_logs.lock().unwrap().get(id).map(|logs| logs.lock().unwrap().iter().cloned().collect()).unwrap_or_default()
    }

    pub async fn running_ids(&self) -> Vec<String> {
        self.running.lock().await.keys().cloned().collect()
    }

    /// Starts the sidecar and waits until its health endpoint answers.
    pub async fn start(&self, mut spec: LaunchSpec<'_>, http: &reqwest::Client) -> Result<String> {
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
        let workdir = if id == "localai" {
            let path = &spec.models_dir;
            let directory = async {
                tokio::fs::create_dir_all(path).await?;
                tokio::fs::canonicalize(path).await
            }
            .await
            .map_err(|e| {
                Error::Runtime(format!(
                    "LocalAI non riesce ad aprire la cartella dei modelli ({}). Verifica il percorso e i permessi nelle impostazioni: {e}",
                    path.display()
                ))
            })?;
            spec.models_dir = directory.clone();
            Some(directory)
        } else {
            None
        };
        let plan = plan_launch(&spec, launch, port);
        let args = plan.args;
        let mut cmd = tokio::process::Command::new(&spec.binary);
        if let Some(workdir) = workdir {
            cmd.current_dir(workdir);
        }
        cmd.args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        // Own process group: `stop` signals the whole group, so workers spawned by the sidecar
        // (vLLM engine cores, SGLang schedulers, `ollama runner`…) do not outlive it.
        #[cfg(unix)]
        cmd.process_group(0);
        for (k, v) in &plan.env {
            cmd.env(k, v);
        }
        // Make shared libraries next to the binary discoverable (llama.cpp builds ship libggml*.so).
        #[cfg(any(target_os = "linux", target_os = "macos"))]
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
        let pid = child.id();
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        self.last_logs.lock().unwrap().insert(id.clone(), logs.clone());
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
                    if let Some(pid) = pid {
                        kill_group(pid, true);
                    }
                    return Err(Error::Runtime(format!(
                        "{} si è chiuso durante l’avvio ({status}). Apri i log nei dettagli e riprova.",
                        spec.manifest.name
                    )));
                }
            }
            // `health: "tcp"` = the port accepts connections (servers without HTTP, e.g. rpc-server).
            if launch.health == "tcp" {
                if tokio::time::timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(("127.0.0.1", port)))
                    .await
                    .is_ok_and(|r| r.is_ok())
                {
                    break;
                }
            } else if let Ok(r) = http.get(&health).timeout(Duration::from_secs(2)).send().await {
                if r.status().is_success() {
                    break;
                }
            }
            if Instant::now() > deadline {
                if let Some(pid) = pid {
                    kill_group(pid, true);
                }
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
            ctx: plan.ctx,
            adapters: spec.adapters.to_vec(),
            started_at: Instant::now(),
            logs,
            model_path: spec.model_path.clone(),
            pid,
            child: if launch.detached { None } else { Some(child) },
        };
        // Dropping a replaced handle (a concurrent start that slipped past the registry lock)
        // kills its process group, so there is never more than one live process per runtime.
        let old = self.running.lock().await.insert(id, handle);
        drop(old);
        Ok(endpoint)
    }

    pub async fn stop(&self, id: &str) -> Result<bool> {
        let h = self.running.lock().await.remove(id);
        match h {
            Some(mut h) => {
                if let Some(mut c) = h.child.take() {
                    match h.pid {
                        Some(pid) => {
                            // SIGTERM to the group, SIGKILL after a grace period; then a final
                            // SIGKILL for workers that outlived the parent.
                            kill_group(pid, false);
                            if tokio::time::timeout(STOP_GRACE, c.wait()).await.is_err() {
                                kill_group(pid, true);
                                let _ = c.start_kill();
                                let _ = tokio::time::timeout(Duration::from_secs(2), c.wait()).await;
                            } else {
                                kill_group(pid, true);
                            }
                        }
                        None => {
                            let _ = c.start_kill();
                            let _ = tokio::time::timeout(Duration::from_secs(5), c.wait()).await;
                        }
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::builtin;
    use serde_json::json;

    fn manifest(id: &str) -> RuntimeManifest {
        builtin().into_iter().find(|m| m.id == id).unwrap_or_else(|| panic!("no manifest {id}"))
    }

    fn plan(id: &str, overrides: Value, model: Option<&str>, model_path: Option<&str>, ctx: Option<u32>) -> (LaunchPlan, String) {
        let m = manifest(id);
        let settings = m.settings.resolve(overrides.as_object().unwrap()).unwrap();
        let spec = LaunchSpec {
            manifest: &m,
            settings: &settings,
            binary: PathBuf::from("/usr/bin/runtime"),
            model: model.map(String::from),
            model_path: model_path.map(PathBuf::from),
            models_dir: PathBuf::from("/data/models"),
            ctx,
            adapters: &[],
        };
        let p = plan_launch(&spec, m.launch.as_ref().unwrap(), 9100);
        let joined = p.args.join(" ");
        (p, joined)
    }

    fn env(p: &LaunchPlan, k: &str) -> Option<String> {
        p.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }

    /// [`plan`] for a profile that names adapters.
    fn plan_with_adapters(id: &str, model_path: &str, adapters: &[AdapterRef]) -> String {
        let m = manifest(id);
        let settings = m.settings.resolve(json!({}).as_object().unwrap()).unwrap();
        let spec = LaunchSpec {
            manifest: &m,
            settings: &settings,
            binary: PathBuf::from("/usr/bin/runtime"),
            model: Some("q".into()),
            model_path: Some(PathBuf::from(model_path)),
            models_dir: PathBuf::from("/data/models"),
            ctx: None,
            adapters,
        };
        plan_launch(&spec, m.launch.as_ref().unwrap(), 9100).args.join(" ")
    }

    #[test]
    fn adapters_of_the_profile_reach_the_launch_arguments() {
        let a = |path: &str, scale| AdapterRef { path: path.into(), scale };
        let args = plan_with_adapters("llama-server", "/m/q.gguf", &[a("/a/sft.gguf", None), a("/a/style.gguf", Some(0.5))]);
        assert!(args.contains("-m /m/q.gguf"), "{args}");
        assert!(args.contains("--lora /a/sft.gguf"), "{args}");
        assert!(args.contains("--lora-scaled /a/style.gguf 0.5"), "{args}");
        let args = plan_with_adapters("mlx-lm", "/m/mlx-q", &[a("/a/mlx-adapter", None)]);
        assert!(args.contains("--model /m/mlx-q") && args.contains("--adapter-path /a/mlx-adapter"), "{args}");
        // No adapters, no arguments; a runtime without support ignores them (the profile is refused before).
        assert!(!plan_with_adapters("llama-server", "/m/q.gguf", &[]).contains("--lora"));
        assert!(!plan_with_adapters("vllm", "/m/q", &[a("/a/x", None)]).contains("/a/x"));
    }

    #[test]
    fn profile_context_overrides_llama_server_ctx_size() {
        let (p, args) = plan("llama-server", json!({"ctx_size": 4096}), Some("q"), Some("/m/q.gguf"), Some(16384));
        assert!(args.contains("-c 16384"), "{args}");
        assert!(!args.contains("-c 4096"), "{args}");
        assert_eq!(p.ctx, Some(16384));
        // Without a profile context the runtime setting stands.
        let (p, args) = plan("llama-server", json!({"ctx_size": 4096}), Some("q"), Some("/m/q.gguf"), None);
        assert!(args.contains("-c 4096"), "{args}");
        assert_eq!(p.ctx, Some(4096));
        // And the schema range is respected (min 512).
        let (p, args) = plan("llama-server", json!({}), Some("q"), Some("/m/q.gguf"), Some(64));
        assert!(args.contains("-c 512"), "{args}");
        assert_eq!(p.ctx, Some(512));
    }

    #[test]
    fn profile_context_reaches_vllm_koboldcpp_and_llamafile() {
        let (_, args) = plan("vllm", json!({}), Some("Qwen/Qwen3-4B"), None, Some(16384));
        assert!(args.contains("--max-model-len 16384"), "{args}");
        assert!(!args.contains("32768"), "{args}");
        let (p, args) = plan("vllm", json!({}), Some("Qwen/Qwen3-4B"), None, None);
        assert!(args.contains("--max-model-len 32768"), "{args}");
        assert_eq!(p.ctx, Some(32768));
        let (_, args) = plan("koboldcpp", json!({}), Some("m"), Some("/m/m.gguf"), Some(12288));
        assert!(args.contains("--contextsize 12288"), "{args}");
        let (_, args) = plan("llamafile", json!({}), Some("m"), Some("/m/m.gguf"), Some(12288));
        assert!(args.contains("-c 12288") && args.contains("-ngl 999"), "{args}");
    }

    #[test]
    fn runtimes_without_a_context_setting_are_left_alone() {
        let (p, args) = plan("mlx-lm", json!({}), Some("mlx-community/Qwen3-4B-4bit"), None, Some(16384));
        assert_eq!(p.ctx, None);
        assert!(!args.contains("16384"), "{args}");
        let (p, _) = plan("sglang", json!({}), Some("Qwen/Qwen3-4B"), None, Some(16384));
        assert_eq!(p.ctx, None);
    }

    #[test]
    fn ollama_gets_context_and_keep_alive_through_the_environment() {
        let (p, args) = plan("ollama", json!({"keep_alive": "30m"}), Some("qwen3:4b"), None, Some(16384));
        assert_eq!(args, "serve");
        assert_eq!(env(&p, "OLLAMA_CONTEXT_LENGTH").as_deref(), Some("16384"));
        assert_eq!(env(&p, "OLLAMA_KEEP_ALIVE").as_deref(), Some("30m"));
        assert_eq!(env(&p, "OLLAMA_HOST").as_deref(), Some("127.0.0.1:9100"));
        // No profile context and no ctx_size setting: the variable is not exported at all, so
        // Ollama keeps its own default instead of receiving a literal `{ctx_size}`.
        let (p, _) = plan("ollama", json!({}), None, None, None);
        assert_eq!(env(&p, "OLLAMA_CONTEXT_LENGTH"), None);
        assert_eq!(env(&p, "OLLAMA_KEEP_ALIVE").as_deref(), Some("5m"));
        // The runtime setting is used when there is no profile.
        let (p, _) = plan("ollama", json!({"ctx_size": 8192}), None, None, None);
        assert_eq!(env(&p, "OLLAMA_CONTEXT_LENGTH").as_deref(), Some("8192"));
        assert!(p.env.iter().all(|(_, v)| !v.contains('{')), "{:?}", p.env);
    }

    #[test]
    fn directory_runtimes_get_the_resolved_local_path() {
        // (runtime, argument that precedes the model)
        for (id, flag) in
            [("mlx-lm", "--model"), ("mlx-vlm", "--model"), ("vllm", "serve"), ("sglang", "--model-path"), ("mistralrs", "-m")]
        {
            let (_, args) = plan(id, json!({}), Some("qwen3-4b-mlx"), Some("/data/models/qwen3-4b-mlx"), None);
            assert!(args.contains(&format!("{flag} /data/models/qwen3-4b-mlx")), "{id}: {args}");
            assert!(!args.contains("{model"), "{id}: {args}");
        }
        // TensorRT-LLM takes the model as its first positional argument.
        let (p, _) = plan("trtllm", json!({}), Some("llama-engine"), Some("/data/models/llama-engine"), None);
        assert_eq!(p.args[0], "/data/models/llama-engine");
    }

    #[test]
    fn vllm_and_sglang_serve_the_model_under_its_profile_id() {
        for id in ["vllm", "sglang"] {
            let (_, args) = plan(id, json!({}), Some("qwen3-4b"), Some("/data/models/qwen3-4b"), None);
            assert!(args.contains("--served-model-name qwen3-4b"), "{id}: {args}");
        }
    }

    #[test]
    fn directory_runtimes_keep_the_remote_id_when_the_model_is_not_local() {
        for (id, flag) in [("mlx-lm", "--model"), ("vllm", "serve"), ("sglang", "--model-path"), ("mistralrs", "-m")] {
            let (_, args) = plan(id, json!({}), Some("Qwen/Qwen3-4B"), None, None);
            assert!(args.contains(&format!("{flag} Qwen/Qwen3-4B")), "{id}: {args}");
        }
        let (p, _) = plan("trtllm", json!({}), Some("nvidia/Llama-3.1-8B"), None, None);
        assert_eq!(p.args[0], "nvidia/Llama-3.1-8B");
    }

    #[test]
    fn file_runtimes_and_openvino_are_unchanged_by_the_new_placeholder() {
        let (_, args) = plan("llama-server", json!({}), Some("q"), Some("/m/q.gguf"), None);
        assert!(args.contains("-m /m/q.gguf"), "{args}");
        // OVMS pulls `--source_model` from the hub into `models_dir`: it always gets the id.
        let (_, args) = plan("openvino", json!({}), Some("OpenVINO/Phi-3-mini-int4-ov"), Some("/data/models/phi"), None);
        assert!(args.contains("--source_model OpenVINO/Phi-3-mini-int4-ov"), "{args}");
        assert!(args.contains("--model_repository_path /data/models"), "{args}");
        assert!(args.contains("--task text_generation"), "{args}");
    }

    #[test]
    fn env_entries_with_unresolved_placeholders_are_dropped() {
        let launch: Launch = serde_json::from_value(json!({
            "command": "x",
            "env": {"A": "{port}", "B": "{missing}", "C": "pre-{port}-{host}", "D": "literal", "E": "{empty}"}
        }))
        .unwrap();
        let vars = BTreeMap::from([
            ("port".to_string(), "1".to_string()),
            ("host".to_string(), "h".to_string()),
            ("empty".to_string(), String::new()),
        ]);
        let env = expand_env(&launch, &vars);
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("A"), Some("1"));
        assert_eq!(get("B"), None);
        assert_eq!(get("C"), Some("pre-1-h"));
        assert_eq!(get("D"), Some("literal"));
        assert_eq!(get("E"), None);
    }
}
