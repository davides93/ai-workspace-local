use crate::manifest::{builtin, Launch, RuntimeManifest};
use crate::ollama::{self, OllamaBackend};
use crate::openai::OpenAiBackend;
use crate::supervisor::{find_binary, LaunchSpec, Supervisor};
use crate::types::ChatBackend;
use aiw_core::api::RuntimeInfo;
use aiw_core::{Error, Result};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock};
use std::time::Duration;

/// Resolves a model id (as shown in the UI) to a file or directory path.
pub type ModelResolver = Arc<dyn Fn(&str) -> Option<PathBuf> + Send + Sync>;

#[derive(Debug, Clone, Default)]
struct Status {
    status: String,
    detail: Option<String>,
    endpoint: Option<String>,
    loaded: Vec<String>,
}

pub struct RuntimeRegistry {
    manifests: RwLock<Vec<RuntimeManifest>>,
    os: String,
    /// CPU architecture (`aarch64`, `x86_64`…), for manifests that gate on it (MLX).
    arch: RwLock<String>,
    supervisor: Supervisor,
    bridges: RwLock<HashMap<String, Arc<dyn ChatBackend>>>,
    overrides: RwLock<HashMap<String, Map<String, Value>>>,
    status: RwLock<HashMap<String, Status>>,
    http: reqwest::Client,
    bin_dirs: Vec<PathBuf>,
    models_dir: PathBuf,
    resolver: RwLock<Option<ModelResolver>>,
    /// Single-flight lock per runtime id: check / stop / start of a sidecar never interleave.
    start_locks: StdMutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Chat requests in flight per sidecar runtime (see [`Tracked`]).
    inflight: StdMutex<HashMap<String, Arc<AtomicUsize>>>,
}

impl RuntimeRegistry {
    /// Builds the registry from the built-in catalog plus `*.json` manifests in `user_dir`
    /// (a user manifest with the same id replaces the built-in one).
    pub fn new(os: impl Into<String>, user_dir: Option<&Path>, bin_dirs: Vec<PathBuf>, models_dir: PathBuf) -> Self {
        let mut manifests = builtin();
        if let Some(dir) = user_dir {
            for m in load_user_manifests(dir) {
                manifests.retain(|x| x.id != m.id);
                manifests.push(m);
            }
        }
        let http = reqwest::Client::builder().connect_timeout(Duration::from_millis(800)).build().expect("http client");
        Self {
            manifests: RwLock::new(manifests),
            os: os.into(),
            arch: RwLock::new(std::env::consts::ARCH.to_string()),
            supervisor: Supervisor::default(),
            bridges: RwLock::new(HashMap::new()),
            overrides: RwLock::new(HashMap::new()),
            status: RwLock::new(HashMap::new()),
            http,
            bin_dirs,
            models_dir,
            resolver: RwLock::new(None),
            start_locks: StdMutex::new(HashMap::new()),
            inflight: StdMutex::new(HashMap::new()),
        }
    }

    pub fn os(&self) -> &str {
        &self.os
    }

    /// Overrides the CPU architecture (defaults to the one this binary was built for); the
    /// engine passes the one reported by the device profile.
    pub fn set_arch(&self, arch: &str) {
        *self.arch.write().unwrap() = arch.to_string();
    }

    /// Whether the manifest can run on this device: OS and, when the manifest gates on it, CPU
    /// architecture (an Intel Mac has no MLX).
    pub fn supports(&self, m: &RuntimeManifest) -> bool {
        m.supports_platform(&self.os) && m.supports_arch(&self.arch.read().unwrap())
    }

    fn unsupported_detail(&self, m: &RuntimeManifest) -> String {
        if !m.supports_platform(&self.os) {
            format!("disponibile su: {}", m.platforms.join(", "))
        } else if self.os == "macos" && m.arch.iter().any(|a| a == "aarch64") {
            "richiede un Mac con Apple Silicon".to_string()
        } else {
            format!("richiede un processore {}", m.arch.join("/"))
        }
    }

    pub fn set_model_resolver(&self, r: ModelResolver) {
        *self.resolver.write().unwrap() = Some(r);
    }

    pub fn manifest(&self, id: &str) -> Result<RuntimeManifest> {
        self.manifests
            .read()
            .unwrap()
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("runtime `{id}` non trovato")))
    }

    pub fn manifests(&self) -> Vec<RuntimeManifest> {
        self.manifests.read().unwrap().clone()
    }

    /// Registers an in-process backend (FFI engine or platform plugin) for an embedded/system runtime.
    pub fn register_bridge(&self, id: &str, backend: Arc<dyn ChatBackend>) {
        self.bridges.write().unwrap().insert(id.to_string(), backend);
        self.status
            .write()
            .unwrap()
            .insert(id.to_string(), Status { status: "running".into(), detail: Some("motore in-process".into()), ..Default::default() });
    }

    /// Loads persisted user settings (validated against each manifest schema).
    pub fn load_overrides(&self, all: HashMap<String, Map<String, Value>>) {
        *self.overrides.write().unwrap() = all;
    }

    /// Effective settings = schema defaults + user overrides.
    pub fn settings(&self, id: &str) -> Result<Map<String, Value>> {
        let m = self.manifest(id)?;
        let o = self.overrides.read().unwrap().get(id).cloned().unwrap_or_default();
        m.settings.resolve(&o).map_err(Error::Invalid)
    }

    /// Validates and stores new settings; returns the map to persist.
    pub fn update_settings(&self, id: &str, values: Map<String, Value>) -> Result<Map<String, Value>> {
        let m = self.manifest(id)?;
        let mut cur = self.overrides.read().unwrap().get(id).cloned().unwrap_or_default();
        for (k, v) in values {
            cur.insert(k, v);
        }
        m.settings.resolve(&cur).map_err(Error::Invalid)?;
        self.overrides.write().unwrap().insert(id.to_string(), cur.clone());
        Ok(cur)
    }

    fn external_endpoint(&self, m: &RuntimeManifest, settings: &Map<String, Value>) -> Option<String> {
        settings
            .get("base_url")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| m.external_endpoint.clone())
    }

    async fn probe_models(&self, endpoint: &str, api_key: Option<&str>) -> Option<Vec<String>> {
        let mut rb = self.http.get(format!("{}/models", endpoint.trim_end_matches('/'))).timeout(Duration::from_millis(900));
        if let Some(k) = api_key.filter(|k| !k.is_empty()) {
            rb = rb.bearer_auth(k);
        }
        let r = rb.send().await.ok()?;
        if !r.status().is_success() {
            return None;
        }
        let v: Value = r.json().await.ok()?;
        Some(v["data"].as_array().map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect()).unwrap_or_default())
    }

    /// Models a running server exposes. Ollama lists the **installed** models (`/api/tags`), not
    /// only what `/v1/models` reports; every other runtime uses `/models`.
    async fn probe_loaded(&self, m: &RuntimeManifest, endpoint: &str, api_key: Option<&str>) -> Option<Vec<String>> {
        if m.api == "ollama" {
            if let Some(models) = ollama::list_installed(&self.http, endpoint, api_key, Duration::from_millis(900)).await {
                return Some(models);
            }
        }
        self.probe_models(endpoint, api_key).await
    }

    /// Readiness of a server that has no OpenAI `/models` (e.g. whisper-server): its health path.
    async fn probe_health(&self, endpoint: &str, m: &RuntimeManifest) -> bool {
        let path = m.launch.as_ref().map(|l| l.health.as_str()).filter(|h| *h != "tcp").unwrap_or("/");
        let url = format!("{}{path}", endpoint.trim_end_matches('/'));
        matches!(self.http.get(url).timeout(Duration::from_millis(900)).send().await, Ok(r) if r.status().is_success())
    }

    async fn compute_status(&self, m: &RuntimeManifest) -> Status {
        if !self.supports(m) {
            return Status { status: "unsupported".into(), detail: Some(self.unsupported_detail(m)), ..Default::default() };
        }
        let bridge = self.bridges.read().unwrap().get(&m.id).cloned();
        if let Some(b) = bridge {
            return match b.availability() {
                Ok(()) => Status { status: "running".into(), detail: Some("motore in-process".into()), ..Default::default() },
                Err(why) => Status { status: "installable".into(), detail: Some(why), ..Default::default() },
            };
        }
        let settings = self.settings(&m.id).unwrap_or_default();
        if let Some((endpoint, model)) = self.supervisor.endpoint(&m.id).await {
            if self.supervisor.is_running(&m.id).await {
                let loaded = self.probe_loaded(m, &endpoint, None).await.unwrap_or_else(|| model.into_iter().collect());
                return Status { status: "running".into(), detail: Some("avviato da AIW".into()), endpoint: Some(endpoint), loaded };
            }
        }
        match m.kind.as_str() {
            "sidecar" => {
                if let Some(ep) = self.external_endpoint(m, &settings) {
                    let probed = if m.api == "openai" || m.api == "ollama" {
                        self.probe_loaded(m, &ep, settings.get("api_key").and_then(|v| v.as_str())).await
                    } else {
                        self.probe_health(&ep, m).await.then(Vec::new)
                    };
                    if let Some(models) = probed {
                        return Status {
                            status: "running".into(),
                            detail: Some("server già attivo".into()),
                            endpoint: Some(ep),
                            loaded: models,
                        };
                    }
                }
                if let Some(bin) = find_binary(m, &settings, &self.bin_dirs) {
                    return Status { status: "available".into(), detail: Some(bin.display().to_string()), ..Default::default() };
                }
                if !m.detect.python_modules.is_empty() && python_has(&m.detect.python_modules).await {
                    // `start` can only run what it can launch: `python3 -m …` recipes, or a
                    // command found in PATH. A Python module without its CLI is not "available".
                    return match m.launch.as_ref().map(|l| l.command.as_str()) {
                        Some("python3") => {
                            Status { status: "available".into(), detail: Some("modulo Python presente".into()), ..Default::default() }
                        }
                        Some(cmd) => Status {
                            status: "installable".into(),
                            detail: Some(format!(
                                "modulo Python presente ma `{cmd}` non è nel PATH: indica il percorso dell'eseguibile nelle impostazioni"
                            )),
                            ..Default::default()
                        },
                        None => Status { status: "installable".into(), detail: m.install_hint(&self.os), ..Default::default() },
                    };
                }
                Status { status: "installable".into(), detail: m.install_hint(&self.os), ..Default::default() }
            }
            "remote" => {
                let ep = self.external_endpoint(m, &settings);
                match ep {
                    Some(ep) => match self.probe_models(&ep, settings.get("api_key").and_then(|v| v.as_str())).await {
                        Some(models) => {
                            Status { status: "running".into(), detail: Some("raggiungibile".into()), endpoint: Some(ep), loaded: models }
                        }
                        None => Status {
                            status: "stopped".into(),
                            detail: Some(format!("{ep} non raggiungibile")),
                            endpoint: Some(ep),
                            ..Default::default()
                        },
                    },
                    None => Status { status: "stopped".into(), detail: Some("configura l'endpoint".into()), ..Default::default() },
                }
            }
            // embedded / system without a registered bridge
            _ => Status {
                status: "installable".into(),
                detail: Some("serve il plugin nativo dell'app per questa piattaforma".into()),
                ..Default::default()
            },
        }
    }

    /// Re-detects every runtime (concurrently) and caches the result.
    pub async fn refresh(&self) {
        let ms = self.manifests();
        let futs = ms.iter().map(|m| async move { (m.id.clone(), self.compute_status(m).await) });
        let results = futures::future::join_all(futs).await;
        let mut st = self.status.write().unwrap();
        for (id, s) in results {
            st.insert(id, s);
        }
    }

    pub async fn refresh_one(&self, id: &str) -> Result<RuntimeInfo> {
        let m = self.manifest(id)?;
        let s = self.compute_status(&m).await;
        self.status.write().unwrap().insert(id.to_string(), s);
        self.info(id)
    }

    pub fn info(&self, id: &str) -> Result<RuntimeInfo> {
        let m = self.manifest(id)?;
        let s =
            self.status.read().unwrap().get(id).cloned().unwrap_or(Status {
                status: if self.supports(&m) { "stopped".into() } else { "unsupported".into() },
                ..Default::default()
            });
        let settings = self.settings(id).unwrap_or_default();
        Ok(RuntimeInfo {
            id: m.id.clone(),
            name: m.name.clone(),
            vendor: m.vendor.clone(),
            kind: m.kind.clone(),
            description: m.description.clone(),
            platforms: m.platforms.clone(),
            accelerators: m.accelerators.clone(),
            formats: m.formats.clone(),
            api: m.api.clone(),
            capabilities: m.capabilities.clone(),
            status: s.status,
            status_detail: s.detail,
            endpoint: s.endpoint,
            loaded_models: s.loaded,
            settings_schema: m.settings.clone(),
            settings: redact(&m, settings),
            install_hint: m.install_hint(&self.os),
            homepage: m.homepage.clone(),
        })
    }

    pub fn list(&self) -> Vec<RuntimeInfo> {
        self.manifests().iter().filter_map(|m| self.info(&m.id).ok()).collect()
    }

    /// Starts (or connects to) a runtime. `model` is a model id for runtimes that need one.
    pub async fn start(&self, id: &str, model: Option<String>) -> Result<RuntimeInfo> {
        self.start_with(id, model, None).await
    }

    /// Like [`start`](Self::start), for an inference profile: `ctx` is the profile's context
    /// window and overrides the runtime's own context setting (`ctx_size`, `max_model_len`,
    /// `OLLAMA_CONTEXT_LENGTH`…) for the process that is launched. A server that is already
    /// answering (started outside AIW) is reused as it is.
    pub async fn start_with(&self, id: &str, model: Option<String>, ctx: Option<u32>) -> Result<RuntimeInfo> {
        let lock = self.start_lock(id);
        let _guard = lock.lock().await;
        // A healthy sidecar that already serves this model with enough context is reused: a
        // second concurrent start must not spawn (and then kill) another process.
        if self.supervisor.is_running(id).await {
            if let Ok(m) = self.manifest(id) {
                let (_, served) = self.supervisor.endpoint(id).await.unwrap_or_default();
                let served_ctx = self.supervisor.served_ctx(id).await;
                let single = m.launch.as_ref().map(launch_uses_model).unwrap_or(false);
                if !needs_restart(single, m.ctx_setting().is_some(), model.as_deref(), served.as_deref(), ctx, served_ctx) {
                    return self.refresh_one(id).await;
                }
            }
        }
        self.start_unlocked(id, model, ctx).await
    }

    fn start_lock(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.start_locks.lock().unwrap().entry(id.to_string()).or_default().clone()
    }

    fn inflight_counter(&self, id: &str) -> Arc<AtomicUsize> {
        self.inflight.lock().unwrap().entry(id.to_string()).or_default().clone()
    }

    /// The body of [`start_with`](Self::start_with); the caller holds the runtime's start lock.
    async fn start_unlocked(&self, id: &str, model: Option<String>, ctx: Option<u32>) -> Result<RuntimeInfo> {
        let m = self.manifest(id)?;
        if !self.supports(&m) {
            return Err(Error::Unsupported(if m.supports_platform(&self.os) {
                format!("{} {}", m.name, self.unsupported_detail(&m))
            } else {
                format!("{} non è disponibile su {}", m.name, self.os)
            }));
        }
        let settings = self.settings(id)?;
        match m.kind.as_str() {
            "sidecar" => {
                // An external server that already answers is reused as is.
                if let Some(ep) = self.external_endpoint(&m, &settings) {
                    if self.probe_models(&ep, None).await.is_some() && m.launch.as_ref().map(|l| !l.requires_model).unwrap_or(true) {
                        return self.refresh_one(id).await;
                    }
                }
                let launch =
                    m.launch.as_ref().ok_or_else(|| Error::Unsupported(format!("{}: avvialo manualmente e AIW lo rileverà", m.name)))?;
                let binary = find_binary(&m, &settings, &self.bin_dirs)
                    .or_else(|| (launch.command == "python3").then(|| which::which("python3").ok()).flatten())
                    .ok_or_else(|| {
                        Error::Unsupported(format!("{} non è installato. {}", m.name, m.install_hint(&self.os).unwrap_or_default()))
                    })?;
                let model_path = match (&model, self.resolver.read().unwrap().as_ref()) {
                    (Some(mid), Some(r)) => r(mid),
                    _ => None,
                };
                if launch.args.iter().any(|a| a.contains("{model_path}")) && model_path.is_none() {
                    return Err(Error::Invalid(format!(
                        "{} ha bisogno del file del modello: scaricalo da Modelli o indica un percorso",
                        m.name
                    )));
                }
                self.supervisor
                    .start(
                        LaunchSpec {
                            manifest: &m,
                            settings: &settings,
                            binary,
                            model: model.clone(),
                            model_path,
                            models_dir: self.models_dir.clone(),
                            ctx,
                        },
                        &self.http,
                    )
                    .await?;
                self.refresh_one(id).await
            }
            "remote" => self.refresh_one(id).await,
            _ => {
                if self.bridges.read().unwrap().contains_key(id) {
                    self.refresh_one(id).await
                } else {
                    Err(Error::Unsupported(format!("{} richiede il plugin nativo dell'app su questo dispositivo", m.name)))
                }
            }
        }
    }

    pub async fn stop(&self, id: &str) -> Result<RuntimeInfo> {
        self.supervisor.stop(id).await?;
        self.refresh_one(id).await
    }

    pub async fn stop_all(&self) -> usize {
        self.supervisor.stop_all().await
    }

    pub async fn logs(&self, id: &str) -> Vec<String> {
        self.supervisor.logs(id).await
    }

    /// Model currently served by an AIW-started sidecar (single-model servers like llama-server).
    pub async fn served_model(&self, id: &str) -> Option<String> {
        self.supervisor.endpoint(id).await.and_then(|(_, m)| m)
    }

    /// Where to send audio for `id` (a runtime that transcribes, or a paired device), starting its
    /// sidecar with `model` when it is not running or serves another model.
    pub async fn stt_target(&self, id: &str, model: Option<&str>) -> Result<crate::stt::SttTarget> {
        use crate::stt::{SttApi, SttTarget};
        let m = self.manifest(id)?;
        if !m.capabilities.iter().any(|c| c == "stt") {
            return Err(Error::Unsupported(format!("{} non trascrive l'audio", m.name)));
        }
        let settings = self.settings(id)?;
        let api_key = settings.get("api_key").and_then(|v| v.as_str()).filter(|k| !k.is_empty()).map(String::from);
        let api = if m.api == "openai" { SttApi::OpenAi } else { SttApi::Whisper };
        let target = |endpoint: String| SttTarget { api, endpoint, api_key: api_key.clone(), model: None };
        match m.kind.as_str() {
            "remote" => {
                let ep =
                    self.external_endpoint(&m, &settings).ok_or_else(|| Error::Invalid(format!("configura l'endpoint di {}", m.name)))?;
                Ok(target(ep))
            }
            "sidecar" => {
                if self.supervisor.is_running(id).await {
                    if let Some((ep, served)) = self.supervisor.endpoint(id).await {
                        if model.is_none() || served.as_deref() == model {
                            return Ok(target(ep));
                        }
                    }
                }
                // A server the user already runs (settings `base_url`) is used as it is.
                if let Some(ep) = self.external_endpoint(&m, &settings) {
                    let up = match api {
                        SttApi::OpenAi => self.probe_models(&ep, api_key.as_deref()).await.is_some(),
                        SttApi::Whisper => self.probe_health(&ep, &m).await,
                    };
                    if up {
                        return Ok(target(ep));
                    }
                }
                self.start(id, model.map(String::from)).await?;
                let (ep, _) = self.supervisor.endpoint(id).await.ok_or_else(|| Error::Runtime(format!("{} non è partito", m.name)))?;
                Ok(target(ep))
            }
            _ => Err(Error::Unsupported(format!("{} richiede il plugin nativo dell'app", m.name))),
        }
    }

    /// Returns a backend for `id`, starting the sidecar with `model` if needed (auto-start).
    pub async fn backend(&self, id: &str, model: Option<&str>) -> Result<Arc<dyn ChatBackend>> {
        self.backend_with(id, model, None).await
    }

    /// Like [`backend`](Self::backend), for an inference profile whose context window is `ctx`:
    /// - a sidecar started now gets it instead of the runtime's own context setting, and a
    ///   sidecar that AIW started earlier with a smaller window is restarted (see
    ///   [`needs_restart`]);
    /// - an in-process engine runs its generations with it ([`ChatBackend::with_context`]).
    pub async fn backend_with(&self, id: &str, model: Option<&str>, ctx: Option<u32>) -> Result<Arc<dyn ChatBackend>> {
        let bridge = self.bridges.read().unwrap().get(id).cloned();
        if let Some(b) = bridge {
            return Ok(ctx.and_then(|c| b.with_context(c)).unwrap_or(b));
        }
        let m = self.manifest(id)?;
        let settings = self.settings(id)?;
        let timeout = Duration::from_secs(settings.get("timeout_s").and_then(|v| v.as_u64()).unwrap_or(900));
        let api_key = settings.get("api_key").and_then(|v| v.as_str()).map(String::from);
        let mut model_to_start = model.map(String::from);
        // One start/stop/restart at a time per runtime: concurrent callers wait here and then
        // find the sidecar the first one started.
        let lock = self.start_lock(id);
        let _guard = if m.kind == "sidecar" { Some(lock.lock().await) } else { None };
        if self.supervisor.is_running(id).await {
            let (ep, served) = self.supervisor.endpoint(id).await.unwrap_or_default();
            let served_ctx = self.supervisor.served_ctx(id).await;
            let single_model = m.launch.as_ref().map(launch_uses_model).unwrap_or(false);
            let supports_ctx = m.ctx_setting().is_some();
            let mut restart = needs_restart(single_model, supports_ctx, model, served.as_deref(), ctx, served_ctx);
            if restart && !needs_restart(single_model, supports_ctx, model, served.as_deref(), None, served_ctx) {
                // Only the context grew. Restarting would cut the streams of the requests that
                // are running right now: serve with the current context and say so.
                let busy = self.inflight_counter(id).load(Ordering::SeqCst);
                if busy > 0 {
                    tracing::warn!(
                        runtime = %id, busy, ?served_ctx, wanted = ?ctx,
                        "context larger than the running sidecar's, but {busy} request(s) are in flight: not restarting"
                    );
                    restart = false;
                }
            }
            if !restart {
                return Ok(self.sidecar_backend(id, &m, ep, api_key, timeout).await);
            }
            // Different model or a larger context: stop it and take the start path below.
            model_to_start = model_to_start.or(served);
            self.supervisor.stop(id).await?;
        }
        match m.kind.as_str() {
            "remote" => {
                let ep =
                    self.external_endpoint(&m, &settings).ok_or_else(|| Error::Invalid(format!("configura l'endpoint di {}", m.name)))?;
                Ok(make_backend(&m, ep, api_key, timeout, None))
            }
            "sidecar" => {
                if let Some(ep) = self.external_endpoint(&m, &settings) {
                    if self.probe_models(&ep, api_key.as_deref()).await.is_some() {
                        return Ok(make_backend(&m, ep, api_key, timeout, None));
                    }
                }
                self.start_unlocked(id, model_to_start, ctx).await?;
                let (ep, _) = self.supervisor.endpoint(id).await.ok_or_else(|| Error::Runtime(format!("{} non è partito", m.name)))?;
                Ok(self.sidecar_backend(id, &m, ep, api_key, timeout).await)
            }
            _ => Err(Error::Unsupported(format!("{} richiede il plugin nativo dell'app", m.name))),
        }
    }

    /// The backend for a sidecar AIW started: it sends the model name the server really serves
    /// (see [`pick_served_name`]) and counts its in-flight requests.
    async fn sidecar_backend(
        &self,
        id: &str,
        m: &RuntimeManifest,
        endpoint: String,
        api_key: Option<String>,
        timeout: Duration,
    ) -> Arc<dyn ChatBackend> {
        let mut served_name = None;
        if m.api != "ollama" && m.launch.as_ref().is_some_and(launch_uses_model) {
            let (_, model_id) = self.supervisor.endpoint(id).await.unwrap_or_default();
            let path = self.supervisor.served_model_path(id).await.map(|p| p.display().to_string());
            let ids = self.probe_models(&endpoint, api_key.as_deref()).await.unwrap_or_default();
            served_name = pick_served_name(&ids, model_id.as_deref(), path.as_deref());
        }
        Arc::new(Tracked {
            inner: make_backend(m, endpoint, api_key, timeout, served_name),
            inflight: self.inflight_counter(id),
            ctx: self.supervisor.served_ctx(id).await,
        })
    }
}

/// Counts the chat requests of a sidecar that are in flight (until their stream is dropped).
struct Tracked {
    inner: Arc<dyn ChatBackend>,
    inflight: Arc<AtomicUsize>,
    /// Context window the sidecar was launched with.
    ctx: Option<u32>,
}

struct InflightGuard(Arc<AtomicUsize>);

impl InflightGuard {
    fn new(c: &Arc<AtomicUsize>) -> Self {
        c.fetch_add(1, Ordering::SeqCst);
        Self(c.clone())
    }
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl ChatBackend for Tracked {
    async fn list_models(&self) -> Result<Vec<String>> {
        self.inner.list_models().await
    }

    async fn chat(&self, req: crate::types::ChatRequest, cancel: tokio_util::sync::CancellationToken) -> Result<crate::types::DeltaStream> {
        use futures::StreamExt;
        let guard = InflightGuard::new(&self.inflight);
        let stream = self.inner.chat(req, cancel).await?;
        Ok(stream
            .map(move |d| {
                let _held = &guard;
                d
            })
            .boxed())
    }

    fn availability(&self) -> std::result::Result<(), String> {
        self.inner.availability()
    }

    fn served_context(&self) -> Option<u32> {
        self.ctx
    }
}

/// The model name to put in chat requests for a single-model sidecar, `None` = keep the
/// requested one. `ids` is what the server lists under `/v1/models`:
/// - the profile's model id is listed: nothing to change;
/// - the local path the server was launched with is listed: that is the served name (vLLM and
///   SGLang without `--served-model-name`);
/// - the server lists exactly one model: that one;
/// - otherwise the local path itself, which is what `mlx_lm.server` accepts for the model it
///   was started with (it would try to download any other name).
pub fn pick_served_name(ids: &[String], model_id: Option<&str>, model_path: Option<&str>) -> Option<String> {
    if model_id.is_some_and(|m| ids.iter().any(|i| i == m)) {
        return None;
    }
    if let Some(p) = model_path {
        if ids.iter().any(|i| i == p) {
            return Some(p.to_string());
        }
    }
    if let [only] = ids {
        return Some(only.clone());
    }
    model_path.map(String::from)
}

/// The chat backend for a runtime endpoint: the OpenAI adapter, with the native Ollama bits
/// (installed-model listing) for `api: "ollama"`.
fn make_backend(
    m: &RuntimeManifest,
    endpoint: String,
    api_key: Option<String>,
    timeout: Duration,
    served_name: Option<String>,
) -> Arc<dyn ChatBackend> {
    if m.api == "ollama" {
        Arc::new(OllamaBackend::new(endpoint, api_key, timeout))
    } else {
        Arc::new(OpenAiBackend::new(endpoint, api_key, timeout).with_model_override(served_name))
    }
}

/// The launch recipe names a model (`{model_path}`, `{model}`, `{model_id}`): the server serves a
/// single model and has to be restarted to change it.
fn launch_uses_model(l: &Launch) -> bool {
    l.args.iter().any(|a| a.contains("{model_path}") || a.contains("{model}") || a.contains("{model_id}"))
}

/// Whether a sidecar that AIW started earlier must be restarted for a new request.
/// - a single-model server serving another model;
/// - a context window larger than the one the process was launched with (`served_ctx`, `None`
///   = the server's own default). A smaller request does not restart anything: a server with
///   more context serves it correctly, since the profile context only drives compaction, and
///   alternating profiles would otherwise bounce the process (and cut other runs' streams).
pub fn needs_restart(
    single_model: bool,
    supports_ctx: bool,
    want_model: Option<&str>,
    served_model: Option<&str>,
    want_ctx: Option<u32>,
    served_ctx: Option<u32>,
) -> bool {
    let model_changed = single_model && matches!((want_model, served_model), (Some(w), Some(h)) if w != h);
    let ctx_grew = supports_ctx && want_ctx.is_some_and(|w| served_ctx.is_none_or(|h| w > h));
    model_changed || ctx_grew
}

fn redact(m: &RuntimeManifest, mut s: Map<String, Value>) -> Map<String, Value> {
    for (k, f) in &m.settings.properties {
        if f.secret {
            if let Some(v) = s.get_mut(k) {
                if v.as_str().map(|x| !x.is_empty()).unwrap_or(false) {
                    *v = Value::String(aiw_core::api::SECRET_MASK.into());
                }
            }
        }
    }
    s
}

fn load_user_manifests(dir: &Path) -> Vec<RuntimeManifest> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read_to_string(&p)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<RuntimeManifest>(&s).map_err(|e| e.to_string()))
        {
            Ok(m) => out.push(m),
            Err(e) => tracing::warn!(path = %p.display(), "invalid runtime manifest: {e}"),
        }
    }
    out
}

async fn python_has(modules: &[String]) -> bool {
    #[cfg(any(target_os = "ios", target_os = "android"))]
    {
        let _ = modules;
        false
    }
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        let Ok(py) = which::which("python3").or_else(|_| which::which("python")) else { return false };
        let code = modules.iter().map(|m| format!("import {m}")).collect::<Vec<_>>().join(";");
        let fut = tokio::process::Command::new(py)
            .arg("-c")
            .arg(code)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        matches!(tokio::time::timeout(Duration::from_secs(4), fut).await, Ok(Ok(s)) if s.success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRequest, DeltaStream};
    use axum::{routing::get, Json, Router};
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    fn registry(os: &str) -> (RuntimeRegistry, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (RuntimeRegistry::new(os, None, vec![], dir.path().to_path_buf()), dir)
    }

    /// A server with the response shapes of Ollama: `/v1/models` lists one model, `/api/tags`
    /// the installed ones.
    async fn fake_ollama() -> String {
        let app = Router::new()
            .route("/v1/models", get(|| async { Json(json!({"object": "list", "data": [{"id": "qwen3:4b"}]})) }))
            .route(
                "/api/tags",
                get(|| async {
                    Json(json!({"models": [
                        {"name": "qwen3:4b", "model": "qwen3:4b", "size": 2497293931u64, "digest": "359d7dd4bcda", "details": {"format": "gguf", "parameter_size": "4.0B"}},
                        {"name": "gemma3:12b", "model": "gemma3:12b", "size": 8149190253u64, "digest": "f4031aab637d", "details": {"format": "gguf", "parameter_size": "12.2B"}}
                    ]}))
                }),
            );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://127.0.0.1:{port}/v1")
    }

    #[tokio::test]
    async fn ollama_status_lists_installed_models_not_only_loaded_ones() {
        let (reg, _dir) = registry("linux");
        reg.update_settings("ollama", json!({"base_url": fake_ollama().await}).as_object().unwrap().clone()).unwrap();
        let info = reg.refresh_one("ollama").await.unwrap();
        assert_eq!(info.status, "running");
        assert_eq!(info.loaded_models, vec!["qwen3:4b", "gemma3:12b"]);
        assert_eq!(info.api, "ollama");
        // The backend handed out for the runtime lists the same installed models.
        let backend = reg.backend("ollama", None).await.unwrap();
        assert_eq!(backend.list_models().await.unwrap(), vec!["qwen3:4b", "gemma3:12b"]);
    }

    #[tokio::test]
    async fn other_runtimes_still_use_v1_models() {
        let (reg, _dir) = registry("linux");
        // LM Studio is `api: openai`: its list is /v1/models, whatever /api/tags says.
        reg.update_settings("lmstudio", json!({"base_url": fake_ollama().await}).as_object().unwrap().clone()).unwrap();
        let info = reg.refresh_one("lmstudio").await.unwrap();
        assert_eq!(info.loaded_models, vec!["qwen3:4b"]);
    }

    #[tokio::test]
    async fn mlx_needs_apple_silicon() {
        let (reg, _dir) = registry("macos");
        reg.set_arch("x86_64");
        for id in ["mlx-lm", "mlx-vlm"] {
            let info = reg.refresh_one(id).await.unwrap();
            assert_eq!(info.status, "unsupported", "{id}");
            assert!(info.status_detail.unwrap().contains("Apple Silicon"), "{id}");
            let err = reg.start(id, Some("m".into())).await.unwrap_err().to_string();
            assert!(err.contains("Apple Silicon"), "{err}");
        }
        // Not an MLX runtime: unaffected by the architecture.
        assert_ne!(reg.refresh_one("llama-server").await.unwrap().status, "unsupported");
        reg.set_arch("aarch64");
        assert_ne!(reg.refresh_one("mlx-lm").await.unwrap().status, "unsupported");
        // Same architecture gate, other OS: still the platform message.
        let (linux, _d) = registry("linux");
        assert_eq!(linux.refresh_one("mlx-lm").await.unwrap().status, "unsupported");
    }

    struct CtxProbe(u32);

    #[async_trait::async_trait]
    impl ChatBackend for CtxProbe {
        async fn list_models(&self) -> Result<Vec<String>> {
            Ok(vec![format!("ctx={}", self.0)])
        }
        async fn chat(&self, _req: ChatRequest, _cancel: CancellationToken) -> Result<DeltaStream> {
            Err(Error::Unsupported("test".into()))
        }
        fn with_context(&self, ctx: u32) -> Option<Arc<dyn ChatBackend>> {
            Some(Arc::new(CtxProbe(ctx)))
        }
    }

    #[tokio::test]
    async fn in_process_engines_receive_the_profile_context() {
        let (reg, _dir) = registry("linux");
        reg.register_bridge("llama-embedded", Arc::new(CtxProbe(8192)));
        let plain = reg.backend("llama-embedded", Some("m")).await.unwrap();
        assert_eq!(plain.list_models().await.unwrap(), vec!["ctx=8192"]);
        let with = reg.backend_with("llama-embedded", Some("m"), Some(16384)).await.unwrap();
        assert_eq!(with.list_models().await.unwrap(), vec!["ctx=16384"]);
    }

    #[test]
    fn restart_rules_for_running_sidecars() {
        // (single_model, supports_ctx, want_model, served_model, want_ctx, served_ctx)
        assert!(!needs_restart(true, true, Some("a"), Some("a"), Some(8192), Some(8192)));
        // Another model on a single-model server.
        assert!(needs_restart(true, true, Some("b"), Some("a"), Some(8192), Some(8192)));
        assert!(!needs_restart(false, true, Some("b"), Some("a"), Some(8192), Some(8192)));
        // A larger context restarts; an equal or smaller one does not.
        assert!(needs_restart(true, true, Some("a"), Some("a"), Some(16384), Some(8192)));
        assert!(!needs_restart(true, true, Some("a"), Some("a"), Some(4096), Some(8192)));
        // No context requested: nothing to restart for.
        assert!(!needs_restart(true, true, Some("a"), Some("a"), None, Some(8192)));
        // A server launched with its own default context (Ollama) cannot be assumed large enough.
        assert!(needs_restart(false, true, None, None, Some(16384), None));
        // Runtimes without a context setting are never restarted for it.
        assert!(!needs_restart(true, false, Some("a"), Some("a"), Some(65536), None));
    }

    #[test]
    fn served_name_follows_what_the_server_lists() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Listed under the profile id: unchanged.
        assert_eq!(pick_served_name(&ids(&["qwen"]), Some("qwen"), Some("/m/qwen")), None);
        // Listed under the path (vLLM without --served-model-name).
        assert_eq!(pick_served_name(&ids(&["/m/qwen", "x"]), Some("qwen"), Some("/m/qwen")).as_deref(), Some("/m/qwen"));
        // A single other name.
        assert_eq!(pick_served_name(&ids(&["model.gguf"]), Some("qwen"), Some("/m/qwen.gguf")).as_deref(), Some("model.gguf"));
        // mlx_lm lists its whole cache: the CLI path is what it accepts.
        assert_eq!(pick_served_name(&ids(&["a", "b"]), Some("qwen"), Some("/m/qwen")).as_deref(), Some("/m/qwen"));
        // Remote id, nothing to go on: unchanged.
        assert_eq!(pick_served_name(&ids(&[]), Some("Qwen/Qwen3"), None), None);
    }

    #[test]
    fn manifests_are_consistent() {
        let all = crate::manifest::builtin();
        // `api` is one of the values the registry understands.
        for m in &all {
            assert!(["openai", "ollama", "native", "none"].contains(&m.api.as_str()), "{}: api `{}`", m.id, m.api);
        }
        // Two runtimes must not claim the same already-running endpoint, or one would be shown as
        // `running` just because the other one is (mistral.rs used to share LM Studio's port).
        let mut seen = std::collections::HashMap::new();
        for m in &all {
            if let Some(ep) = &m.external_endpoint {
                if let Some(other) = seen.insert(ep.clone(), m.id.clone()) {
                    panic!("{} and {} share the external endpoint {ep}", m.id, other);
                }
            }
        }
        // Every sidecar that names a model can be told how large its context is, where it has
        // such a setting, and llamafile can offload to the GPU.
        let llamafile = all.iter().find(|m| m.id == "llamafile").unwrap();
        assert!(llamafile.launch.as_ref().unwrap().settings_args.contains_key("gpu_layers"));
        assert_eq!(llamafile.ctx_setting().as_deref(), Some("ctx_size"));
        for (id, key) in [("llama-server", "ctx_size"), ("koboldcpp", "ctx_size"), ("vllm", "max_model_len"), ("ollama", "ctx_size")] {
            let m = all.iter().find(|m| m.id == id).unwrap();
            assert_eq!(m.ctx_setting().as_deref(), Some(key), "{id}");
            assert!(m.settings.properties.contains_key(key), "{id} has no `{key}` setting");
        }
        // A model-less runtime has none.
        assert_eq!(all.iter().find(|m| m.id == "lmstudio").unwrap().ctx_setting(), None);
    }
}
