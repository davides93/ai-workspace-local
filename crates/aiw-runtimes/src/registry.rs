use crate::manifest::{builtin, RuntimeManifest};
use crate::openai::OpenAiBackend;
use crate::supervisor::{find_binary, LaunchSpec, Supervisor};
use crate::types::ChatBackend;
use aiw_core::api::RuntimeInfo;
use aiw_core::{Error, Result};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
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
    supervisor: Supervisor,
    bridges: RwLock<HashMap<String, Arc<dyn ChatBackend>>>,
    overrides: RwLock<HashMap<String, Map<String, Value>>>,
    status: RwLock<HashMap<String, Status>>,
    http: reqwest::Client,
    bin_dirs: Vec<PathBuf>,
    models_dir: PathBuf,
    resolver: RwLock<Option<ModelResolver>>,
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
            supervisor: Supervisor::default(),
            bridges: RwLock::new(HashMap::new()),
            overrides: RwLock::new(HashMap::new()),
            status: RwLock::new(HashMap::new()),
            http,
            bin_dirs,
            models_dir,
            resolver: RwLock::new(None),
        }
    }

    pub fn os(&self) -> &str {
        &self.os
    }

    pub fn set_model_resolver(&self, r: ModelResolver) {
        *self.resolver.write().unwrap() = Some(r);
    }

    pub fn manifest(&self, id: &str) -> Result<RuntimeManifest> {
        self.manifests.read().unwrap().iter().find(|m| m.id == id).cloned().ok_or_else(|| Error::NotFound(format!("runtime `{id}`")))
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

    async fn compute_status(&self, m: &RuntimeManifest) -> Status {
        if !m.supports_platform(&self.os) {
            return Status {
                status: "unsupported".into(),
                detail: Some(format!("disponibile su: {}", m.platforms.join(", "))),
                ..Default::default()
            };
        }
        if self.bridges.read().unwrap().contains_key(&m.id) {
            return Status { status: "running".into(), detail: Some("motore in-process".into()), ..Default::default() };
        }
        let settings = self.settings(&m.id).unwrap_or_default();
        if let Some((endpoint, model)) = self.supervisor.endpoint(&m.id).await {
            if self.supervisor.is_running(&m.id).await {
                let loaded = self.probe_models(&endpoint, None).await.unwrap_or_else(|| model.into_iter().collect());
                return Status { status: "running".into(), detail: Some("avviato da AIW".into()), endpoint: Some(endpoint), loaded };
            }
        }
        match m.kind.as_str() {
            "sidecar" => {
                if let Some(ep) = self.external_endpoint(m, &settings) {
                    if let Some(models) = self.probe_models(&ep, settings.get("api_key").and_then(|v| v.as_str())).await {
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
                    return Status { status: "available".into(), detail: Some("modulo Python presente".into()), ..Default::default() };
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
        let s = self.status.read().unwrap().get(id).cloned().unwrap_or(Status {
            status: if m.supports_platform(&self.os) { "stopped".into() } else { "unsupported".into() },
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
        let m = self.manifest(id)?;
        if !m.supports_platform(&self.os) {
            return Err(Error::Unsupported(format!("{} non è disponibile su {}", m.name, self.os)));
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

    /// Returns a backend for `id`, starting the sidecar with `model` if needed (auto-start).
    pub async fn backend(&self, id: &str, model: Option<&str>) -> Result<Arc<dyn ChatBackend>> {
        if let Some(b) = self.bridges.read().unwrap().get(id).cloned() {
            return Ok(b);
        }
        let m = self.manifest(id)?;
        let settings = self.settings(id)?;
        let timeout = Duration::from_secs(settings.get("timeout_s").and_then(|v| v.as_u64()).unwrap_or(900));
        let api_key = settings.get("api_key").and_then(|v| v.as_str()).map(String::from);
        if self.supervisor.is_running(id).await {
            let (ep, served) = self.supervisor.endpoint(id).await.unwrap_or_default();
            // Single-model servers must be restarted to switch model.
            let single_model =
                m.launch.as_ref().map(|l| l.args.iter().any(|a| a.contains("{model_path}") || a.contains("{model}"))).unwrap_or(false);
            match (model, served) {
                (Some(want), Some(have)) if single_model && want != have => {
                    self.start(id, Some(want.to_string())).await?;
                    let (ep, _) = self.supervisor.endpoint(id).await.unwrap_or_default();
                    return Ok(Arc::new(OpenAiBackend::new(ep, api_key, timeout)));
                }
                _ => return Ok(Arc::new(OpenAiBackend::new(ep, api_key, timeout))),
            }
        }
        match m.kind.as_str() {
            "remote" => {
                let ep =
                    self.external_endpoint(&m, &settings).ok_or_else(|| Error::Invalid(format!("configura l'endpoint di {}", m.name)))?;
                Ok(Arc::new(OpenAiBackend::new(ep, api_key, timeout)))
            }
            "sidecar" => {
                if let Some(ep) = self.external_endpoint(&m, &settings) {
                    if self.probe_models(&ep, api_key.as_deref()).await.is_some() {
                        return Ok(Arc::new(OpenAiBackend::new(ep, api_key, timeout)));
                    }
                }
                self.start(id, model.map(String::from)).await?;
                let (ep, _) = self.supervisor.endpoint(id).await.ok_or_else(|| Error::Runtime(format!("{} non è partito", m.name)))?;
                Ok(Arc::new(OpenAiBackend::new(ep, api_key, timeout)))
            }
            _ => Err(Error::Unsupported(format!("{} richiede il plugin nativo dell'app", m.name))),
        }
    }
}

fn redact(m: &RuntimeManifest, mut s: Map<String, Value>) -> Map<String, Value> {
    for (k, f) in &m.settings.properties {
        if f.secret {
            if let Some(v) = s.get_mut(k) {
                if v.as_str().map(|x| !x.is_empty()).unwrap_or(false) {
                    *v = Value::String("••••••".into());
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
