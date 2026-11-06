//! `Engine` — the single facade used by the HTTP server, the CLI and the Tauri shells (ADR 0002).

pub mod a2a_client;
pub mod defaults;
mod host;
mod models;
pub mod pairing;
pub mod peers;
mod secrets;
pub mod store;
pub mod update;
pub mod voice;
pub mod webhook;
pub mod worktree;

pub use host::EngineHost;
pub use pairing::PairOutcome;

use aiw_agent::{to_msgs, AgentRunner, HookConfig, Policy, PolicyRule, RunBudget, RunInput};
use aiw_core::api::*;
use aiw_core::config::{app_schema, Layer, LayeredConfig};
use aiw_core::events::{AgEvent, SeqEvent, TimedEvent};
use aiw_core::paths::DataDirs;
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use aiw_models::{DownloadManager, DownloadOptions, ExternalRoots, Hub, HubConfig};
use aiw_runtimes::{ChatBackend, RuntimeRegistry};
use aiw_tools::browser::BrowserManager;
use aiw_tools::mcp::{McpClient, McpTool};
use aiw_tools::{SandboxMode, ToolContext, ToolRegistry};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use store::Store;
use tokio::sync::{broadcast, oneshot};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default)]
pub struct EngineOptions {
    pub data_dir: Option<PathBuf>,
    pub workspace_root: Option<PathBuf>,
    /// embedded | daemon
    pub mode: String,
    /// Mobile shells pass the profile reported by the platform plugin.
    pub device: Option<DeviceProfile>,
    pub extra_bin_dirs: Vec<PathBuf>,
    /// Skip network probes and background tasks (tests).
    pub offline: bool,
    /// Base URL of the GitHub API used to look for updates (tests point it at a fake server).
    pub update_api_base: Option<String>,
    /// Version to compare releases against (default: this build's; tests set their own).
    pub current_version: Option<String>,
}

/// One event on the global bus (Tauri forwards these as `aiw://run-event`).
#[derive(Debug, Clone, Serialize)]
pub struct RunEventMsg {
    pub run_id: String,
    pub seq: u64,
    pub event: TimedEvent,
}

pub(crate) struct RunHandle {
    session_id: Option<String>,
    cancel: CancellationToken,
    events: Vec<SeqEvent>,
    done: bool,
}

pub(crate) struct PendingApproval {
    req: ApprovalRequest,
    tx: oneshot::Sender<bool>,
    session_id: Option<String>,
}

/// Body and stored form of the policy. `rules` is required: a body without it must not be read
/// as "no rules" and wipe the existing ones.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyDoc {
    pub rules: Vec<PolicyRule>,
}

/// Body and stored form of the hooks. `hooks` is required, for the same reason as [`PolicyDoc::rules`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksDoc {
    pub hooks: Vec<HookConfig>,
}

pub(crate) struct Inner {
    pub mode: String,
    pub started: Instant,
    pub dirs: DataDirs,
    pub config: LayeredConfig,
    pub workspace_root: RwLock<PathBuf>,
    pub device: RwLock<DeviceProfile>,
    pub runtimes: RuntimeRegistry,
    pub downloads: DownloadManager,
    /// Multi-provider model hub (search, variants, download plans).
    pub hub: Hub,
    /// `DownloadManager::completed()` at the last `downloads()` poll: rescan only on a change.
    pub downloads_seen: std::sync::atomic::AtomicU64,
    /// Where the other apps keep their models (Ollama, LM Studio, Hugging Face cache).
    pub external_roots: RwLock<ExternalRoots>,
    pub models: Arc<RwLock<Vec<LocalModel>>>,
    /// Model id/name (or path) → file or folder, shared by the in-process engines.
    pub resolver: aiw_runtimes::ModelResolver,
    pub models_dir: PathBuf,
    pub tools: Arc<ToolRegistry>,
    pub browser: Arc<BrowserManager>,
    pub mcp: tokio::sync::Mutex<HashMap<String, Arc<McpClient>>>,
    pub mcp_status: RwLock<HashMap<String, (String, Option<String>)>>,
    pub store: Store,
    pub runs: Mutex<HashMap<String, RunHandle>>,
    pub approvals: Mutex<HashMap<String, PendingApproval>>,
    pub session_allow: Mutex<HashMap<String, Arc<Mutex<HashSet<String>>>>>,
    pub bus: broadcast::Sender<RunEventMsg>,
    pub workflow_runs: Mutex<Vec<WorkflowRun>>,
    pub scheduler: Mutex<aiw_workflow::Scheduler>,
    pub token: String,
    pub pairing: Mutex<(String, Instant)>,
    /// Failed-attempt limiter of the pairing endpoint.
    pub pair_guard: Mutex<pairing::PairGuard>,
    pub updater: update::Updater,
    pub current_version: semver::Version,
}

#[derive(Clone)]
pub struct Engine(pub(crate) Arc<Inner>);

/// Longest session title accepted by [`Engine::rename_session`].
pub const MAX_TITLE_CHARS: usize = 200;
/// Prefix of the ids of remote agents (`remote:<id>`) in teams and sub-agent lists.
pub const REMOTE_PREFIX: &str = "remote:";
/// Prefix of a session's agent id that points at a team (`team:<id>`).
pub const TEAM_PREFIX: &str = aiw_core::api::TEAM_AGENT_PREFIX;

fn random_digits(n: usize) -> String {
    let u = uuid::Uuid::new_v4().as_u128();
    format!("{:0width$}", u % 10u128.pow(n as u32), width = n)
}

impl Engine {
    pub async fn new(opts: EngineOptions) -> Result<Self> {
        let dirs = opts.data_dir.clone().map(DataDirs::at).unwrap_or_else(DataDirs::resolve);
        dirs.ensure()?;
        let device = match opts.device.clone() {
            Some(d) => d,
            None => tokio::task::spawn_blocking(aiw_hw::probe).await.map_err(Error::other)?,
        };
        let config = LayeredConfig::load(app_schema(), dirs.config_file(), dirs.device_config(&device.name), None);
        let workspace_root = opts
            .workspace_root
            .clone()
            .or_else(|| config.get_str("workspace.root").map(|s| aiw_core::paths::expand_home(&s)))
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join("AIW-Workspace"));
        std::fs::create_dir_all(&workspace_root)?;
        let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);
        config.set_workspace_root(Some(&workspace_root));
        let models_dir = config.get_str("models.dir").map(|s| aiw_core::paths::expand_home(&s)).unwrap_or_else(|| dirs.models());
        std::fs::create_dir_all(&models_dir)?;
        let mut bin_dirs = vec![dirs.root.join("bin")];
        bin_dirs.extend(opts.extra_bin_dirs.clone());
        let runtimes = RuntimeRegistry::new(device.os.clone(), Some(&dirs.runtimes()), bin_dirs, models_dir.clone());
        runtimes.set_arch(&device.arch); // MLX needs Apple Silicon, not just macOS
        runtimes.set_pyenv_dir(dirs.root.join("pyenv")); // pip-installed runtimes (MLX…) live here
        let store = Store::open(&dirs.db_file())?;
        let overrides: Vec<Value> = store.list_docs("runtime_settings")?;
        runtimes.load_overrides(
            overrides.into_iter().filter_map(|v| Some((v["id"].as_str()?.to_string(), v["settings"].as_object()?.clone()))).collect(),
        );

        let tools = Arc::new(ToolRegistry::default());
        let browser = Arc::new(BrowserManager::new(dirs.root.join("browser-profile")));
        aiw_tools::register_builtins(&tools, browser.clone());
        if let Some(d) = store.get_doc::<Vec<String>>("tool_settings", "disabled")? {
            tools.set_disabled(d);
        }
        // Tests (`offline`) never look into the real home of the developer.
        let external_roots = if opts.offline { ExternalRoots::none() } else { ExternalRoots::detect() };
        let hub = Hub::new(HubConfig::from_env());
        Engine::load_provider_settings(&hub, &store);
        let models = Arc::new(RwLock::new(aiw_models::scan_all(&models_dir, &external_roots)));
        let m2 = models.clone();
        let resolver: aiw_runtimes::ModelResolver = Arc::new(move |id: &str| {
            let list = m2.read().unwrap();
            list.iter().find(|m| m.id == id || m.name == id).and_then(|m| m.path.clone()).map(PathBuf::from).or_else(|| {
                let p = PathBuf::from(id);
                p.exists().then_some(p)
            })
        });
        runtimes.set_model_resolver(resolver.clone());
        #[cfg(feature = "llama-embedded")]
        register_embedded(&runtimes, resolver.clone());
        #[cfg(not(target_os = "ios"))]
        register_ort_genai(&runtimes, resolver.clone());
        let token = {
            let p = dirs.root.join("token");
            match std::fs::read_to_string(&p) {
                Ok(t) if t.trim().len() >= 24 => t.trim().to_string(),
                _ => {
                    let t = uuid::Uuid::new_v4().simple().to_string() + &uuid::Uuid::new_v4().simple().to_string()[..8];
                    std::fs::write(&p, &t)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
                    }
                    t
                }
            }
        };
        let (bus, _) = broadcast::channel(8192);
        let (updater_os, updater_arch) = (device.os.clone(), device.arch.clone());
        let updates_dir = dirs.root.join("updates");
        let engine = Engine(Arc::new(Inner {
            mode: if opts.mode.is_empty() { "embedded".into() } else { opts.mode.clone() },
            started: Instant::now(),
            downloads: DownloadManager::with_options(models_dir.clone(), DownloadOptions::default()),
            hub,
            downloads_seen: Default::default(),
            external_roots: RwLock::new(external_roots),
            resolver,
            models_dir,
            dirs,
            config,
            workspace_root: RwLock::new(workspace_root),
            device: RwLock::new(device),
            runtimes,
            models,
            tools,
            browser,
            mcp: Default::default(),
            mcp_status: Default::default(),
            store,
            runs: Default::default(),
            approvals: Default::default(),
            session_allow: Default::default(),
            bus,
            workflow_runs: Default::default(),
            scheduler: Default::default(),
            token,
            pairing: Mutex::new((random_digits(6), Instant::now())),
            pair_guard: Default::default(),
            updater: update::Updater::new(opts.update_api_base.clone(), &updater_os, &updater_arch, updates_dir),
            current_version: semver::Version::parse(opts.current_version.as_deref().unwrap_or(aiw_core::VERSION))
                .map_err(|e| Error::Invalid(format!("versione non valida: {e}")))?,
        }));
        if !opts.offline {
            engine.0.runtimes.refresh().await;
        }
        engine.seed()?;
        engine.resume_workflows();
        if !opts.offline {
            engine.start_background();
            engine.start_update_checks();
        }
        Ok(engine)
    }

    fn seed(&self) -> Result<()> {
        let s = &self.0.store;
        // Tasks served to other agents expire after a day.
        let _ = s.a2a_cleanup(&(chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        // Nothing can be running yet: whatever says so was interrupted by a crash or a kill.
        if s.fail_interrupted_runs()? > 0 {
            for mut session in s.sessions()?.into_iter().filter(|x| x.last_status.as_deref() == Some("running")) {
                session.last_status = Some("failed".into());
                s.upsert_session(&session)?;
            }
        }
        if s.list_docs::<AgentProfile>("agent")?.is_empty() {
            for a in defaults::agents() {
                s.put_doc("agent", &a.id, &a)?;
            }
        }
        if s.get_doc::<InferenceProfile>("profile", "default")?.is_none() {
            // Runtime and model are chosen for this device: what is installed/registered (phones
            // have no sidecars, so the in-process engine wins there) and what fits in memory.
            let avail = defaults::Availability::from_runtimes(&self.0.runtimes.list());
            let p = defaults::default_profile(&self.0.models.read().unwrap(), &self.device(), &avail);
            s.put_doc("profile", &p.id, &p)?;
        }
        if s.list_docs::<WorkflowDef>("workflow")?.is_empty() {
            for w in defaults::workflows() {
                s.put_doc("workflow", &w.id, &w)?;
            }
        }
        // Workflows dropped in ~/.aiw/workflows (the user's own folder) or
        // <workspace>/.aiw/workflows (YAML/JSON).
        if let Ok(rd) = std::fs::read_dir(self.0.dirs.workflows()) {
            for e in rd.flatten() {
                if let Ok(wf) = aiw_workflow::load_file(&e.path()) {
                    s.put_doc("workflow", &wf.id, &wf)?;
                }
            }
        }
        if let Ok(rd) = std::fs::read_dir(self.workspace_root().join(".aiw").join("workflows")) {
            for e in rd.flatten() {
                if let Ok(wf) = aiw_workflow::load_file(&e.path()) {
                    let stored: Option<WorkflowDef> = s.get_doc("workflow", &wf.id)?;
                    let wf = workspace_workflow(wf, stored.as_ref());
                    s.put_doc("workflow", &wf.id, &wf)?;
                }
            }
        }
        Ok(())
    }

    fn start_background(&self) {
        // MCP servers.
        let e = self.clone();
        tokio::spawn(async move {
            for cfg in e.0.store.list_docs::<McpServerConfig>("mcp").unwrap_or_default().into_iter().filter(|c| c.enabled) {
                let _ = e.connect_mcp(cfg).await;
            }
        });
        // Workflow scheduler + periodic runtime detection.
        let e = self.clone();
        tokio::spawn(async move {
            let mut tick = 0u64;
            loop {
                tokio::time::sleep(Duration::from_secs(20)).await;
                tick += 1;
                let wfs: Vec<WorkflowDef> = e.0.store.list_docs("workflow").unwrap_or_default();
                let due = e.0.scheduler.lock().unwrap().due(&wfs, chrono::Local::now());
                for id in due {
                    tracing::info!(workflow = %id, "scheduled trigger");
                    let _ = e.run_workflow(&id, Map::new(), "workflow").await;
                }
                if tick % 3 == 0 {
                    e.0.runtimes.refresh().await;
                }
            }
        });
    }

    // ------------------------------------------------------------------ system
    pub fn workspace_root(&self) -> PathBuf {
        self.0.workspace_root.read().unwrap().clone()
    }
    pub fn token(&self) -> &str {
        &self.0.token
    }
    pub fn subscribe(&self) -> broadcast::Receiver<RunEventMsg> {
        self.0.bus.subscribe()
    }
    pub fn device(&self) -> DeviceProfile {
        self.0.device.read().unwrap().clone()
    }
    /// Lets platform plugins add accelerators (NNAPI/QNN/ANE…) discovered natively.
    pub fn set_device(&self, d: DeviceProfile) {
        *self.0.device.write().unwrap() = d;
    }
    /// Registers an in-process backend (native plugin or embedded engine) for a runtime id.
    pub fn register_native_runtime(&self, id: &str, backend: Arc<dyn ChatBackend>) {
        self.0.runtimes.register_bridge(id, backend);
    }

    /// Current 6-digit pairing code (rotates every 10 minutes).
    pub fn pairing_code(&self) -> String {
        let mut p = self.0.pairing.lock().unwrap();
        if p.1.elapsed() > Duration::from_secs(600) {
            *p = (random_digits(6), Instant::now());
        }
        p.0.clone()
    }
    /// Exchanges a pairing code for the API token (LAN devices). `None` when the code is wrong or
    /// pairing is locked; use [`Engine::pair_attempt`] to tell the two apart.
    pub fn pair(&self, code: &str) -> Option<String> {
        match self.pair_attempt(code, None) {
            PairOutcome::Paired(token) => Some(token),
            _ => None,
        }
    }

    /// One pairing attempt from `client` (its address, when known). Failures are rate limited
    /// across all clients and per client: after too many the code is rotated and every attempt,
    /// even with the right code, is refused with a retry hint (see [`pairing`]).
    pub fn pair_attempt(&self, code: &str, client: Option<&str>) -> PairOutcome {
        let now = Instant::now();
        if let Some(retry_after) = self.0.pair_guard.lock().unwrap().check(client, now) {
            return PairOutcome::Locked { retry_after };
        }
        let given = code.replace(' ', "");
        if pairing::constant_time_eq(&given, &self.pairing_code()) {
            self.0.pair_guard.lock().unwrap().record_success(client);
            *self.0.pairing.lock().unwrap() = (random_digits(6), Instant::now());
            self.audit_system("pair", "nuovo dispositivo accoppiato");
            PairOutcome::Paired(self.0.token.clone())
        } else {
            let tripped = self.0.pair_guard.lock().unwrap().record_failure(client, now);
            if tripped {
                // The guesses made so far are void: whoever was trying now faces a new code.
                *self.0.pairing.lock().unwrap() = (random_digits(6), Instant::now());
                self.audit_system(
                    "pair_lockout",
                    &format!("{} tentativi di pairing falliti: codice rinnovato, nuovi tentativi sospesi", pairing::MAX_FAILURES),
                );
            }
            PairOutcome::Invalid
        }
    }

    pub fn system(&self, include_pairing: bool) -> SystemInfo {
        let peers: Vec<PeerDevice> = self.0.store.list_docs("peer").unwrap_or_default();
        SystemInfo {
            version: aiw_core::VERSION.into(),
            mode: self.0.mode.clone(),
            data_dir: self.0.dirs.root.display().to_string(),
            workspace_root: self.workspace_root().display().to_string(),
            uptime_s: self.0.started.elapsed().as_secs(),
            pairing_code: include_pairing.then(|| self.pairing_code()),
            device: self.device(),
            peers,
        }
    }

    pub(crate) fn audit_system(&self, action: &str, detail: &str) {
        let _ = self.0.store.add_audit(&AuditEntry {
            id: new_id("aud"),
            ts: now_rfc3339(),
            run_id: None,
            actor: "system".into(),
            action: action.into(),
            tool: None,
            decision: None,
            detail: detail.into(),
        });
    }
    fn audit_user(&self, action: &str, detail: &str) {
        let _ = self.0.store.add_audit(&AuditEntry {
            id: new_id("aud"),
            ts: now_rfc3339(),
            run_id: None,
            actor: "user".into(),
            action: action.into(),
            tool: None,
            decision: None,
            detail: detail.into(),
        });
    }

    /// Kill switch: cancels runs, rejects approvals, stops AIW-started sidecars and the browser.
    pub async fn kill(&self) -> Value {
        let runs: Vec<CancellationToken> = self.0.runs.lock().unwrap().values().filter(|r| !r.done).map(|r| r.cancel.clone()).collect();
        for c in &runs {
            c.cancel();
        }
        let approvals: Vec<PendingApproval> = self.0.approvals.lock().unwrap().drain().map(|(_, v)| v).collect();
        let n_appr = approvals.len();
        for a in approvals {
            let _ = a.tx.send(false);
        }
        let sidecars = self.0.runtimes.stop_all().await;
        let browser = self.0.browser.close().await;
        let detail = format!(
            "{} run, {} conferme, {} sidecar fermati{}",
            runs.len(),
            n_appr,
            sidecars,
            if browser { ", browser chiuso" } else { "" }
        );
        self.audit_system("kill", &detail);
        json!({"runs": runs.len(), "approvals": n_appr, "sidecars": sidecars, "browser": browser})
    }

    // ------------------------------------------------------------------ runtimes
    pub fn runtimes(&self) -> Vec<RuntimeInfo> {
        self.0.runtimes.list()
    }
    pub async fn refresh_runtimes(&self) -> Vec<RuntimeInfo> {
        self.0.runtimes.refresh().await;
        self.runtimes()
    }
    pub async fn runtime_action(&self, id: &str, action: &str, model: Option<String>) -> Result<RuntimeInfo> {
        let r = match action {
            "start" => {
                // The profile that uses this runtime (and the requested model, if any) supplies
                // the model and its context window, which overrides the runtime's `ctx_size`.
                let profile = self.profiles().ok().and_then(|ps| {
                    ps.into_iter().find(|p| p.runtime_id == id && !p.model.is_empty() && model.as_deref().is_none_or(|m| m == p.model))
                });
                let ctx = profile.as_ref().and_then(|p| p.params.context);
                let model = model.or_else(|| profile.map(|p| p.model));
                self.0.runtimes.start_with(id, model, ctx).await
            }
            "stop" => self.0.runtimes.stop(id).await,
            "refresh" => self.0.runtimes.refresh_one(id).await,
            other => Err(Error::Invalid(format!("azione sconosciuta: {other}"))),
        }?;
        self.audit_user(&format!("runtime.{action}"), &r.name);
        Ok(r)
    }
    /// What installing the runtime takes on this machine (steps, missing prerequisites).
    pub async fn runtime_install_plan(&self, id: &str) -> Result<aiw_runtimes::install::InstallPlan> {
        self.0.runtimes.install_plan(id).await
    }

    /// Installs the runtime in the background (one click, no further questions) and returns the
    /// job to follow with [`runtime_install_status`](Self::runtime_install_status). When it
    /// ends the runtimes are detected again, so the UI sees the new state.
    pub async fn runtime_install_start(&self, id: &str) -> Result<aiw_runtimes::install::InstallJob> {
        let (cmds, job) = self.0.runtimes.begin_install(id).await?;
        let name = self.0.runtimes.manifest(id).map(|m| m.name).unwrap_or_else(|_| id.to_string());
        self.audit_user("runtime.install", &format!("{name}: {}", cmds.iter().map(|c| c.display()).collect::<Vec<_>>().join(" && ")));
        let initial = job.lock().unwrap().clone();
        let engine = self.clone();
        tokio::spawn(async move {
            aiw_runtimes::install::run(cmds, job).await;
            engine.0.runtimes.refresh().await;
        });
        Ok(initial)
    }

    pub fn runtime_install_status(&self, id: &str) -> Option<aiw_runtimes::install::InstallJob> {
        self.0.runtimes.install_job(id)
    }

    pub async fn runtime_logs(&self, id: &str) -> Vec<String> {
        self.0.runtimes.logs(id).await
    }
    pub fn update_runtime_settings(&self, id: &str, settings: Map<String, Value>) -> Result<RuntimeInfo> {
        let info = self.update_runtime_settings_inner(id, settings)?;
        // In-process engines read their settings when they are created: recreate them.
        #[cfg(feature = "llama-embedded")]
        if id == "llama-embedded" {
            register_embedded(&self.0.runtimes, self.0.resolver.clone());
        }
        #[cfg(not(target_os = "ios"))]
        if id == "ort-genai" {
            register_ort_genai(&self.0.runtimes, self.0.resolver.clone());
        }
        Ok(info)
    }

    fn update_runtime_settings_inner(&self, id: &str, settings: Map<String, Value>) -> Result<RuntimeInfo> {
        let m = self.0.runtimes.manifest(id)?;
        // A redacted secret coming back from the UI keeps the stored value.
        let settings: Map<String, Value> = settings
            .into_iter()
            .filter(|(k, v)| !(m.settings.properties.get(k).map(|f| f.secret).unwrap_or(false) && v.as_str() == Some(SECRET_MASK)))
            .collect();
        let saved = self.0.runtimes.update_settings(id, settings)?;
        self.0.store.put_doc("runtime_settings", id, &json!({"id": id, "settings": saved}))?;
        self.0.runtimes.info(id)
    }

    // Models, providers and downloads: see models.rs.

    // ------------------------------------------------------------------ profiles & agents
    pub fn profiles(&self) -> Result<Vec<InferenceProfile>> {
        self.0.store.list_docs("profile")
    }
    pub fn profile(&self, id: &str) -> Result<InferenceProfile> {
        self.0.store.get_doc("profile", id)?.ok_or_else(|| Error::NotFound(format!("profilo {id} non trovato")))
    }
    pub fn save_profile(&self, p: InferenceProfile) -> Result<InferenceProfile> {
        if p.id.trim().is_empty() || p.runtime_id.trim().is_empty() {
            return Err(Error::Invalid("profilo senza id o runtime".into()));
        }
        self.0.runtimes.manifest(&p.runtime_id)?;
        self.0.store.put_doc("profile", &p.id, &p)?;
        Ok(p)
    }
    pub fn agents(&self) -> Result<Vec<AgentProfile>> {
        self.0.store.list_docs("agent")
    }
    pub fn agent(&self, id: &str) -> Result<AgentProfile> {
        self.0.store.get_doc("agent", id)?.ok_or_else(|| Error::NotFound(format!("agente {id} non trovato")))
    }
    pub fn save_agent(&self, a: AgentProfile) -> Result<AgentProfile> {
        if !["plan", "ask", "auto-edit", "auto", "yolo"].contains(&a.mode.as_str()) {
            return Err(Error::Invalid(format!("modalità non valida: {}", a.mode)));
        }
        if !["read-only", "workspace-write", "off"].contains(&a.sandbox.as_str()) {
            return Err(Error::Invalid(format!("sandbox non valida: {}", a.sandbox)));
        }
        if a.max_turns < 1 {
            return Err(Error::Invalid("max_turns deve essere almeno 1".into()));
        }
        if let Some(b) = &a.budget {
            for (name, v) in [("max_tokens", b.max_tokens), ("max_turns", b.max_turns.map(u64::from)), ("max_seconds", b.max_seconds)] {
                if v == Some(0) {
                    return Err(Error::Invalid(format!("budget.{name} deve essere almeno 1 (ometti il campo per nessun limite)")));
                }
            }
        }
        if let Some(schema) = &a.output_schema {
            aiw_agent::schema::check_schema(schema).map_err(|e| Error::Invalid(format!("output_schema non valido: {e}")))?;
        }
        if self.0.store.get_doc::<InferenceProfile>("profile", &a.inference_profile)?.is_none() {
            return Err(Error::Invalid(format!("il profilo di inferenza `{}` non esiste", a.inference_profile)));
        }
        let unknown: Vec<&str> = a.tools.iter().map(String::as_str).filter(|t| !self.is_known_tool(t)).collect();
        if !unknown.is_empty() {
            return Err(Error::Invalid(format!(
                "strumenti sconosciuti: {} (usa \"*\" oppure i nomi elencati in Strumenti)",
                unknown.join(", ")
            )));
        }
        self.0.store.put_doc("agent", &a.id, &a)?;
        Ok(a)
    }
    /// A name an agent profile may list: `*` (every enabled tool), a registry tool, an internal
    /// tool of the agent loop, or an MCP tool (`mcp__server__tool`; the server may be offline).
    fn is_known_tool(&self, name: &str) -> bool {
        name == "*" || name.starts_with("mcp__") || aiw_agent::internal::is_internal(name) || self.0.tools.get(name).is_some()
    }
    /// Deletes a custom agent. The agents seeded by [`defaults::agents`] and the ones a workflow
    /// still delegates to are refused with a conflict.
    pub fn delete_agent(&self, id: &str) -> Result<()> {
        let agent = self.agent(id)?;
        if defaults::is_builtin_agent(id) {
            return Err(Error::Conflict(format!(
                "«{}» è un agente integrato e non si può eliminare: puoi modificarlo o crearne una copia.",
                agent.name
            )));
        }
        let used_by: Vec<String> = self
            .workflows()?
            .into_iter()
            .filter(|w| w.steps.iter().flat_map(|s| s.with_inner()).any(|s| matches!(s, WorkflowStep::Agent { agent: a, .. } if a == id)))
            .map(|w| w.name)
            .collect();
        if !used_by.is_empty() {
            return Err(Error::Conflict(format!(
                "L'agente «{}» è usato dai workflow: {}. Modificali prima di eliminarlo.",
                agent.name,
                used_by.join(", ")
            )));
        }
        let teams = self.teams_using_agent(id)?;
        if !teams.is_empty() {
            return Err(Error::Conflict(format!(
                "L'agente «{}» è usato dai team: {}. Modificali prima di eliminarlo.",
                agent.name,
                teams.join(", ")
            )));
        }
        self.0.store.delete_doc("agent", id)?;
        self.audit_user("agent.delete", &format!("{} ({id})", agent.name));
        Ok(())
    }
    /// The profile a session runs with, and the team when its agent id is `team:<id>`. A
    /// supervisor team runs as its supervisor (the team's budget and output schema win); the
    /// other strategies have no model in charge, so they run under a neutral profile that never
    /// narrows the members: the widest mode and sandbox among them.
    pub(crate) fn session_agent(&self, agent_id: &str) -> Result<(AgentProfile, Option<TeamDef>)> {
        let Some(team_id) = agent_id.strip_prefix(TEAM_PREFIX) else { return Ok((self.agent(agent_id)?, None)) };
        let team = self.team(team_id)?;
        let locals: Vec<AgentProfile> = team
            .members
            .iter()
            .map(|m| m.agent.as_str())
            .chain(team.supervisor.as_deref())
            .chain(team.aggregator.as_deref())
            .filter(|id| !id.starts_with(REMOTE_PREFIX))
            .map(|id| self.agent(id))
            .collect::<Result<_>>()?;
        let mut agent = match (team.strategy.as_str(), &team.supervisor) {
            ("supervisor", Some(sup)) => {
                let mut a = self.agent(sup)?;
                for tool in ["agent", aiw_agent::internal::TEAM_STATE] {
                    if !a.tools.iter().any(|t| t == "*" || t == tool) {
                        a.tools.push(tool.into());
                    }
                }
                a
            }
            _ => {
                let first = locals.first().ok_or_else(|| Error::Invalid("il team non ha agenti locali".into()))?;
                let widest = |f: fn(&AgentProfile) -> u8| locals.iter().max_by_key(|a| f(a)).unwrap_or(first);
                AgentProfile {
                    id: agent_id.into(),
                    name: team.name.clone(),
                    description: team.description.clone(),
                    inference_profile: first.inference_profile.clone(),
                    system_prompt: String::new(),
                    tools: vec!["*".into()],
                    mode: widest(|a| host::mode_rank(&a.mode)).mode.clone(),
                    sandbox: widest(|a| sandbox_rank(&a.sandbox)).sandbox.clone(),
                    max_turns: first.max_turns,
                    output_schema: None,
                    budget: None,
                }
            }
        };
        if team.output_schema.is_some() {
            agent.output_schema = team.output_schema.clone();
        }
        if team.budget.is_some() {
            agent.budget = team.budget.clone();
        }
        Ok((agent, Some(team)))
    }

    /// Adds what a supervisor team needs to a run input: only the members as sub-agents and the
    /// `# Team` section of the system prompt.
    fn apply_team(&self, input: &mut RunInput, team: &TeamDef) {
        input.in_team = true;
        if team.strategy != "supervisor" {
            return;
        }
        let described = |m: &TeamMember| {
            if !m.role.trim().is_empty() {
                return m.role.trim().to_string();
            }
            self.agent(&m.agent).map(|a| a.description).unwrap_or_default()
        };
        input.subagents = team.members.iter().map(|m| (m.agent.clone(), described(m))).collect();
        input.system_extra.push(aiw_agent::team::supervisor_section(team, &input.subagents));
    }

    // ------------------------------------------------------------------ teams
    pub fn teams(&self) -> Result<Vec<TeamDef>> {
        self.0.store.list_docs("team")
    }
    pub fn team(&self, id: &str) -> Result<TeamDef> {
        self.0.store.get_doc("team", id)?.ok_or_else(|| Error::NotFound(format!("team {id} non trovato")))
    }
    pub fn save_team(&self, t: TeamDef) -> Result<TeamDef> {
        if t.id.trim().is_empty() || t.name.trim().is_empty() {
            return Err(Error::Invalid("il team ha bisogno di un id e di un nome".into()));
        }
        if !["supervisor", "parallel", "pipeline"].contains(&t.strategy.as_str()) {
            return Err(Error::Invalid(format!("strategia non valida: {} (supervisor, parallel o pipeline)", t.strategy)));
        }
        if t.members.is_empty() || t.members.len() > 8 {
            return Err(Error::Invalid("un team ha da 1 a 8 membri".into()));
        }
        let mut seen = std::collections::HashSet::new();
        for m in &t.members {
            if !seen.insert(m.agent.as_str()) {
                return Err(Error::Invalid(format!("il membro `{}` compare più volte", m.agent)));
            }
            self.check_team_agent(&m.agent)?;
        }
        match (t.strategy.as_str(), &t.supervisor) {
            ("supervisor", None) => return Err(Error::Invalid("la strategia supervisor richiede un supervisore".into())),
            (_, Some(sup)) => {
                self.check_team_agent(sup)?;
                if sup.starts_with(REMOTE_PREFIX) {
                    return Err(Error::Invalid("il supervisore deve essere un agente locale".into()));
                }
                if seen.contains(sup.as_str()) {
                    return Err(Error::Invalid(format!("il supervisore `{sup}` non può essere anche un membro")));
                }
            }
            _ => {}
        }
        if let Some(agg) = &t.aggregator {
            self.check_team_agent(agg)?;
            if agg.starts_with(REMOTE_PREFIX) {
                return Err(Error::Invalid("l'aggregatore deve essere un agente locale".into()));
            }
        }
        if let Some(b) = &t.budget {
            for (name, v) in [("max_tokens", b.max_tokens), ("max_turns", b.max_turns.map(u64::from)), ("max_seconds", b.max_seconds)] {
                if v == Some(0) {
                    return Err(Error::Invalid(format!("budget.{name} deve essere almeno 1 (ometti il campo per nessun limite)")));
                }
            }
        }
        if let Some(schema) = &t.output_schema {
            aiw_agent::schema::check_schema(schema).map_err(|e| Error::Invalid(format!("output_schema non valido: {e}")))?;
        }
        self.0.store.put_doc("team", &t.id, &t)?;
        Ok(t)
    }
    /// A team member, supervisor or aggregator: an existing local agent or `remote:<id>`.
    fn check_team_agent(&self, id: &str) -> Result<()> {
        if id.starts_with(REMOTE_PREFIX) {
            return Ok(());
        }
        if self.0.store.get_doc::<AgentProfile>("agent", id)?.is_none() {
            return Err(Error::Invalid(format!("l'agente `{id}` non esiste")));
        }
        Ok(())
    }
    pub fn delete_team(&self, id: &str) -> Result<()> {
        let team = self.team(id)?;
        let used_by: Vec<String> = self
            .workflows()?
            .into_iter()
            .filter(|w| w.steps.iter().flat_map(|s| s.with_inner()).any(|s| matches!(s, WorkflowStep::Team { team: t, .. } if t == id)))
            .map(|w| w.name)
            .collect();
        if !used_by.is_empty() {
            return Err(Error::Conflict(format!(
                "Il team «{}» è usato dai workflow: {}. Modificali prima di eliminarlo.",
                team.name,
                used_by.join(", ")
            )));
        }
        self.0.store.delete_doc("team", id)?;
        self.audit_user("team.delete", &format!("{} ({id})", team.name));
        Ok(())
    }
    /// Names of the teams that use the agent as supervisor, member or aggregator.
    fn teams_using_agent(&self, id: &str) -> Result<Vec<String>> {
        Ok(self
            .teams()?
            .into_iter()
            .filter(|t| {
                t.supervisor.as_deref() == Some(id) || t.aggregator.as_deref() == Some(id) || t.members.iter().any(|m| m.agent == id)
            })
            .map(|t| t.name)
            .collect())
    }
    /// Deletes an inference profile that no agent (or workflow step) uses any more.
    pub fn delete_profile(&self, id: &str) -> Result<()> {
        let profile = self.profile(id)?;
        let mut used_by: Vec<String> = self.agents()?.into_iter().filter(|a| a.inference_profile == id).map(|a| a.name).collect();
        used_by.extend(
            self.workflows()?
                .into_iter()
                .filter(|w| {
                    w.steps.iter().flat_map(|s| s.with_inner()).any(|s| matches!(s, WorkflowStep::Llm { profile: p, .. } if p == id))
                })
                .map(|w| format!("workflow {}", w.name)),
        );
        if !used_by.is_empty() {
            return Err(Error::Conflict(format!(
                "Il profilo «{}» è usato da: {}. Assegna loro un altro profilo prima di eliminarlo.",
                profile.name,
                used_by.join(", ")
            )));
        }
        self.0.store.delete_doc("profile", id)?;
        self.audit_user("profile.delete", &format!("{} ({id})", profile.name));
        Ok(())
    }
    pub fn policy_rules(&self) -> Vec<PolicyRule> {
        self.0.store.get_doc::<PolicyDoc>("policy", "default").ok().flatten().unwrap_or_default().rules
    }
    /// Replaces the policy rules after validating each one (see [`PolicyRule::validated`]).
    pub fn set_policy_rules(&self, rules: Vec<PolicyRule>) -> Result<()> {
        let rules = rules
            .into_iter()
            .enumerate()
            .map(|(i, r)| r.validated().map_err(|e| Error::Invalid(format!("regola {}: {e}", i + 1))))
            .collect::<Result<Vec<_>>>()?;
        self.0.store.put_doc("policy", "default", &PolicyDoc { rules })
    }
    pub fn hooks(&self) -> Vec<HookConfig> {
        self.0.store.get_doc::<HooksDoc>("hooks", "default").ok().flatten().unwrap_or_default().hooks
    }
    /// Replaces the hooks after validating each one (see [`HookConfig::validated`]).
    pub fn set_hooks(&self, hooks: Vec<HookConfig>) -> Result<()> {
        let hooks = hooks
            .into_iter()
            .enumerate()
            .map(|(i, h)| h.validated().map_err(|e| Error::Invalid(format!("hook {}: {e}", i + 1))))
            .collect::<Result<Vec<_>>>()?;
        self.0.store.put_doc("hooks", "default", &HooksDoc { hooks })
    }

    // ------------------------------------------------------------------ sessions & runs
    pub fn sessions(&self) -> Result<Vec<Session>> {
        let mut list = self.0.store.sessions()?;
        for s in &mut list {
            s.worktree = self.worktree_info(&s.id);
        }
        Ok(list)
    }
    pub fn session(&self, id: &str) -> Result<Session> {
        let mut s = self.0.store.session(id)?;
        s.worktree = self.worktree_info(id);
        Ok(s)
    }
    pub fn create_session(&self, agent_id: &str, title: Option<String>) -> Result<Session> {
        self.create_session_with(agent_id, title, None)
    }
    /// `isolation: Some("worktree")` gives the session its own git worktree and branch.
    pub fn create_session_with(&self, agent_id: &str, title: Option<String>, isolation: Option<&str>) -> Result<Session> {
        match agent_id.strip_prefix(TEAM_PREFIX) {
            Some(team_id) => self.team(team_id).map(|_| ())?,
            None => self.agent(agent_id).map(|_| ())?,
        }
        let now = now_rfc3339();
        let mut s = Session {
            id: new_id("ses"),
            title: title.unwrap_or_else(|| "Nuova sessione".into()),
            agent_id: agent_id.into(),
            created_at: now.clone(),
            updated_at: now,
            last_status: None,
            worktree: None,
        };
        match isolation.unwrap_or("none") {
            "none" | "" => {}
            "worktree" => {
                let wt = worktree::create(&self.workspace_root(), &self.0.dirs.worktrees(), &s.id)?;
                self.0.store.put_doc("worktree", &s.id, &wt)?;
                s.worktree = Some(wt);
            }
            other => return Err(Error::Invalid(format!("isolamento sconosciuto: {other} (usa none o worktree)"))),
        }
        self.0.store.upsert_session(&s)?;
        Ok(s)
    }
    /// Renames a session; the title is a single trimmed line of at most [`MAX_TITLE_CHARS`].
    pub fn rename_session(&self, id: &str, title: &str) -> Result<Session> {
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            return Err(Error::Invalid("il titolo non può essere vuoto".into()));
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(Error::Invalid(format!("il titolo supera i {MAX_TITLE_CHARS} caratteri")));
        }
        self.0.store.rename_session(id, &title)?;
        self.session(id)
    }
    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.0.store.session(id)?; // not found: 404, like every other route of a session
                                   // A run still going would keep executing tools for a session that no longer exists.
        if let Some(run) = self.active_run(id) {
            let _ = self.cancel_run(&run);
        }
        if let Some(wt) = self.worktree_info(id) {
            if let Err(e) = worktree::remove(&wt) {
                tracing::warn!("worktree {}: {e}", wt.path);
            }
            self.0.store.delete_doc("worktree", id)?;
        }
        self.0.store.delete_session(id)
    }

    // ------------------------------------------------------------------ worktrees
    fn worktree_info(&self, session_id: &str) -> Option<WorktreeInfo> {
        self.0.store.get_doc::<WorktreeInfo>("worktree", session_id).ok().flatten()
    }
    /// Directory the tools of a session work in: its worktree, or the shared workspace.
    pub fn session_root(&self, session_id: &str) -> PathBuf {
        match self.worktree_info(session_id) {
            Some(wt) if Path::new(&wt.root).is_dir() => PathBuf::from(wt.root),
            _ => self.workspace_root(),
        }
    }
    fn session_worktree(&self, session_id: &str) -> Result<WorktreeInfo> {
        self.0.store.session(session_id)?;
        self.worktree_info(session_id).ok_or_else(|| Error::Invalid("Questa sessione non usa un worktree.".into()))
    }
    fn ensure_idle(&self, session_id: &str) -> Result<()> {
        if self.active_run(session_id).is_some() {
            return Err(Error::Invalid("Aspetta la fine del run in corso (o fermalo) prima di applicare o scartare.".into()));
        }
        Ok(())
    }
    pub fn worktree_status(&self, session_id: &str) -> Result<WorktreeStatus> {
        worktree::status(&self.session_worktree(session_id)?)
    }
    /// Applies the session changes to the user's checkout (as uncommitted changes).
    pub fn worktree_apply(&self, session_id: &str) -> Result<WorktreeStatus> {
        self.ensure_idle(session_id)?;
        let mut wt = self.session_worktree(session_id)?;
        let files = worktree::apply(&mut wt)?;
        self.0.store.put_doc("worktree", session_id, &wt)?;
        self.audit_worktree(session_id, "worktree_apply", format!("{} file applicati da {} al workspace", files.len(), wt.branch));
        worktree::status(&wt)
    }
    /// Deletes the worktree and its branch; the session continues on the shared workspace.
    pub fn worktree_discard(&self, session_id: &str) -> Result<()> {
        self.ensure_idle(session_id)?;
        let wt = self.session_worktree(session_id)?;
        worktree::remove(&wt)?;
        self.0.store.delete_doc("worktree", session_id)?;
        self.audit_worktree(session_id, "worktree_discard", format!("worktree {} e branch {} eliminati", wt.path, wt.branch));
        Ok(())
    }
    fn audit_worktree(&self, session_id: &str, action: &str, detail: String) {
        let _ = self.0.store.add_audit(&AuditEntry {
            id: new_id("aud"),
            ts: now_rfc3339(),
            run_id: None,
            actor: "user".into(),
            action: action.into(),
            tool: None,
            decision: Some("approved".into()),
            detail: format!("{detail} (sessione {session_id})"),
        });
    }
    pub fn messages(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        self.0.store.session(session_id)?; // an unknown session is not an empty conversation
        self.0.store.messages(session_id)
    }

    pub(crate) fn emit(&self, run_id: &str, ev: AgEvent) {
        let terminal = ev.is_terminal();
        let timed = TimedEvent::now(ev);
        let mut to_persist = None;
        let seq = {
            let mut runs = self.0.runs.lock().unwrap();
            let h = runs.entry(run_id.to_string()).or_insert_with(|| RunHandle {
                session_id: None,
                cancel: CancellationToken::new(),
                events: vec![],
                done: false,
            });
            let seq = h.events.len() as u64;
            h.events.push(SeqEvent { seq, event: timed.clone() });
            if terminal {
                h.done = true;
                to_persist = Some(h.events.clone());
            }
            seq
        };
        let _ = self.0.bus.send(RunEventMsg { run_id: run_id.to_string(), seq, event: timed });
        if let Some(evs) = to_persist {
            let _ = self.0.store.save_events(run_id, &evs);
        }
    }

    /// Events of a run so far (memory first, then the database for finished runs).
    pub fn run_events(&self, run_id: &str) -> Vec<SeqEvent> {
        if let Some(h) = self.0.runs.lock().unwrap().get(run_id) {
            return h.events.clone();
        }
        self.0.store.events(run_id).unwrap_or_default()
    }
    /// Message of the RUN_ERROR that closed the run, when it ended in error.
    fn run_error_code(&self, run_id: &str) -> Option<String> {
        match self.run_events(run_id).last().map(|e| &e.event.event) {
            Some(AgEvent::RunError { code, .. }) => code.clone(),
            _ => None,
        }
    }
    fn run_error_text(&self, run_id: &str) -> Option<String> {
        match self.run_events(run_id).last().map(|e| &e.event.event) {
            Some(AgEvent::RunError { message, .. }) => Some(message.clone()),
            _ => None,
        }
    }
    /// The run currently in progress for a session, if any (lets clients re-attach).
    pub fn active_run(&self, session_id: &str) -> Option<String> {
        self.0.runs.lock().unwrap().iter().find(|(_, h)| !h.done && h.session_id.as_deref() == Some(session_id)).map(|(id, _)| id.clone())
    }
    pub fn run_done(&self, run_id: &str) -> bool {
        self.0.runs.lock().unwrap().get(run_id).map(|h| h.done).unwrap_or(true)
    }

    fn session_allow(&self, session_id: &str) -> Arc<Mutex<HashSet<String>>> {
        self.0.session_allow.lock().unwrap().entry(session_id.to_string()).or_default().clone()
    }

    fn agents_md(&self) -> String {
        let mut out = String::new();
        for (label, p) in [("globali", self.0.dirs.root.join("AGENTS.md")), ("del workspace", self.workspace_root().join("AGENTS.md"))] {
            if let Ok(t) = std::fs::read_to_string(&p) {
                out.push_str(&format!("# Istruzioni {label} (AGENTS.md)\n{}\n\n", t.chars().take(20_000).collect::<String>()));
            }
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_input(
        &self,
        run_id: &str,
        thread_id: &str,
        agent: AgentProfile,
        profile: InferenceProfile,
        history: Vec<aiw_runtimes::Msg>,
        user_input: String,
        cancel: CancellationToken,
        depth: u32,
        session_allow: Arc<Mutex<HashSet<String>>>,
    ) -> RunInput {
        // Chat runs and their sub-agents use the session id as thread id: an isolated session
        // works in its worktree.
        let ws = self.session_root(thread_id);
        let isolated =
            self.worktree_info(thread_id).map(|wt| format!(" (git worktree isolato, branch `{}`)", wt.branch)).unwrap_or_default();
        let device = self.device();
        let mut tool_ctx = ToolContext::new(run_id, ws.clone());
        tool_ctx.read_roots = vec![self.0.dirs.skills(), ws.join(".aiw")];
        tool_ctx.sandbox = SandboxMode::parse(&agent.sandbox);
        tool_ctx.require_read_before_overwrite = true;
        tool_ctx.cancel = cancel;
        tool_ctx.checkpoint_dir = Some(self.0.dirs.checkpoints());
        tool_ctx.allow_domains =
            self.0.config.get("network.allow_domains").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        let env = format!(
            "# Ambiente\n- Data e ora: {}\n- Dispositivo: {} ({} {}, classe {})\n- Cartella di lavoro: {}{isolated}\n- Inferenza locale: runtime `{}`, modello `{}`",
            chrono::Local::now().format("%A %d %B %Y, %H:%M"),
            device.name,
            device.os,
            device.arch,
            device.tier.to_uppercase(),
            ws.display(),
            profile.runtime_id,
            profile.model
        );
        let subagents = self
            .agents()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.id != agent.id)
            .map(|a| (a.id, a.description))
            .chain(self.remote_subagents())
            .collect();
        let agent_budget = agent.budget.clone().unwrap_or_default();
        RunInput {
            run_id: run_id.into(),
            thread_id: thread_id.into(),
            policy: Policy {
                mode: agent.mode.clone(),
                rules: self.policy_rules(),
                sandbox_available: aiw_tools::shell::sandbox_backend().is_some(),
            },
            agent,
            profile,
            history,
            user_input,
            system_extra: vec![env, self.agents_md()],
            tool_ctx,
            hooks: self.hooks(),
            compact_at: self.0.config.get_f64("context.compact_at").unwrap_or(0.8),
            depth,
            session_allow,
            subagents,
            budget: Arc::new(if depth == 0 { self.run_budget(&agent_budget) } else { RunBudget::unlimited() }),
            max_parallel_subagents: self.0.config.get("agents.max_parallel_subagents").and_then(|v| v.as_u64()).unwrap_or(3).max(1)
                as usize,
            in_team: false,
            shared: Default::default(),
            root_run: run_id.into(),
        }
    }

    /// The budget of a new top-level run: the agent's own limits, else the `agents.budget.*` settings
    /// (0 there means no limit).
    fn run_budget(&self, own: &aiw_core::api::BudgetLimits) -> RunBudget {
        let setting = |key: &str| self.0.config.get(key).and_then(|v| v.as_u64()).filter(|n| *n > 0);
        RunBudget::new(
            own.max_tokens.or_else(|| setting("agents.budget.max_tokens")),
            own.max_turns.or_else(|| setting("agents.budget.max_turns").map(|n| n.min(u32::MAX as u64) as u32)),
            own.max_seconds.or_else(|| setting("agents.budget.max_seconds")),
        )
    }

    pub(crate) fn skills(&self) -> Vec<aiw_agent::Skill> {
        aiw_agent::skills::discover(&[self.0.dirs.skills(), self.workspace_root().join(".aiw").join("skills")])
    }

    /// Starts an agent run in a session; events stream on the bus and via [`Engine::run_events`].
    pub async fn start_run(&self, session_id: &str, input: String) -> Result<String> {
        self.start_run_with(session_id, input, None).await
    }

    /// `kind: Some("a2ui.action")` marks the user turn as an A2UI action sent by a surface.
    pub async fn start_run_with(&self, session_id: &str, input: String, kind: Option<&str>) -> Result<String> {
        if input.trim().is_empty() {
            return Err(Error::Invalid("messaggio vuoto".into()));
        }
        let name = match kind {
            None | Some("") => None,
            Some(aiw_agent::a2ui::ACTION_NAME) => {
                if !input.starts_with(aiw_agent::a2ui::ACTION_PREFIX) {
                    return Err(Error::Invalid(format!("un'azione A2UI deve iniziare con {}", aiw_agent::a2ui::ACTION_PREFIX)));
                }
                Some(aiw_agent::a2ui::ACTION_NAME.to_string())
            }
            Some(other) => return Err(Error::Invalid(format!("tipo di messaggio non valido: {other}"))),
        };
        self.start_run_inner(session_id, input, name, 0).await
    }

    /// Most handoffs one user message can chain before the conversation stops with a note.
    const MAX_HANDOFFS: u32 = 3;

    /// Starts a run; `hops` is how many handoffs led to it (0 for a run the user asked for). Boxed:
    /// a handoff starts the next run from inside the task of the previous one.
    fn start_run_inner<'a>(
        &'a self,
        session_id: &'a str,
        input: String,
        name: Option<String>,
        hops: u32,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(self.start_run_body(session_id, input, name, hops))
    }

    async fn start_run_body(&self, session_id: &str, input: String, name: Option<String>, hops: u32) -> Result<String> {
        let mut session = self.0.store.session(session_id)?;
        // 404 is reserved for the session itself: a session whose agent, profile or runtime was
        // removed meanwhile is a conflict the user can fix (the UI must not treat it as deleted).
        let gone = |what: &str, e: Error| match e {
            Error::NotFound(m) => Error::Conflict(format!("{m}: {what} di questa sessione non esiste più, scegline un altro in Agenti")),
            other => other,
        };
        let (agent, team) = self.session_agent(&session.agent_id).map_err(|e| gone("l'agente o il team", e))?;
        let profile = self.profile(&agent.inference_profile).map_err(|e| gone("il profilo", e))?;
        let manifest = self.0.runtimes.manifest(&profile.runtime_id).map_err(|e| gone("il runtime", e))?;
        if profile.model.is_empty() && manifest.launch.as_ref().map(|l| l.requires_model).unwrap_or(false) {
            return Err(Error::Invalid(format!(
                "Il profilo \"{}\" non ha un modello: scaricane uno da Modelli e selezionalo in Agenti → Profili.",
                profile.name
            )));
        }
        let run_id = new_id("run");
        let cancel = CancellationToken::new();
        {
            // Check and insert under the same lock: two runs on one session would interleave
            // their messages without seeing each other.
            let mut runs = self.0.runs.lock().unwrap();
            if runs.values().any(|h| !h.done && h.session_id.as_deref() == Some(session_id)) {
                return Err(Error::Conflict("in questa sessione c'è già un run in corso: aspetta che finisca o fermalo".into()));
            }
            runs.insert(
                run_id.clone(),
                RunHandle { session_id: Some(session_id.into()), cancel: cancel.clone(), events: vec![], done: false },
            );
        }
        self.0.store.start_run(&run_id, Some(session_id))?;
        let history = to_msgs(&self.0.store.messages(session_id)?);
        let user_msg = ChatMessage {
            id: new_id("msg"),
            role: "user".into(),
            content: input.clone(),
            reasoning: None,
            tool_calls: None,
            tool_call_id: None,
            name,
            created_at: now_rfc3339(),
        };
        self.0.store.append_messages(session_id, std::slice::from_ref(&user_msg))?;
        if session.title == "Nuova sessione" && user_msg.name.is_none() {
            session.title = session_title(&input);
        }
        session.updated_at = now_rfc3339();
        session.last_status = Some("running".into());
        self.0.store.upsert_session(&session)?;
        let mut run_input =
            self.build_input(&run_id, session_id, agent, profile, history, input, cancel, 0, self.session_allow(session_id));
        if let Some(team) = &team {
            self.apply_team(&mut run_input, team);
        }
        let engine = self.clone();
        let rid = run_id.clone();
        let (from_name, from_mode) = (run_input.agent.name.clone(), run_input.policy.mode.clone());
        tokio::spawn(async move {
            let host = Arc::new(EngineHost::new(engine.clone(), Some(session.id.clone()), None));
            // What the run produced is stored whatever its outcome: when it was cancelled or failed
            // its files were still written and its commands run, and the next turn must know.
            // (`new_messages[0]` is the user message, stored above.)
            let (out, res) = match team {
                Some(team) if aiw_agent::team::is_scripted(&team.strategy) => {
                    aiw_agent::team::run_team_partial(run_input, team, host).await
                }
                _ => AgentRunner::run_partial(run_input, host).await,
            };
            let status = match &res {
                Ok(()) => "succeeded",
                Err(Error::Cancelled) => "cancelled",
                Err(_) => "failed",
            };
            if let Err(e) = &res {
                if !engine.run_done(&rid) {
                    engine.emit(&rid, AgEvent::RunError { message: e.to_string(), code: None });
                }
            }
            let mut stored: Vec<ChatMessage> = out.new_messages.get(1..).unwrap_or(&[]).to_vec();
            // A prompt refused by a UserPromptSubmit hook stays visible in the chat but must never
            // reach the model on a later turn.
            if engine.run_error_code(&rid).as_deref() == Some("hook_blocked") {
                let blocked = ChatMessage { name: Some(BLOCKED_PROMPT_MESSAGE.into()), ..user_msg.clone() };
                let _ = engine.0.store.replace_message(&session.id, &blocked);
            }
            // A run that ended in error leaves it in the history, so that the chat shows the same
            // thing after a reload as it showed live (RUN_ERROR).
            stored.extend(engine.run_error_text(&rid).map(|text| ChatMessage {
                id: new_id("msg"),
                role: "system".into(),
                content: text,
                reasoning: None,
                tool_calls: None,
                tool_call_id: None,
                name: Some(RUN_ERROR_MESSAGE.into()),
                created_at: now_rfc3339(),
            }));
            let _ = engine.0.store.append_messages(&session.id, &stored);
            let _ = engine.0.store.finish_run(&rid, status);
            if let Ok(mut s) = engine.0.store.session(&session.id) {
                s.last_status = Some(status.into());
                s.updated_at = now_rfc3339();
                let _ = engine.0.store.upsert_session(&s);
            }
            if let (Ok(()), Some((to, summary))) = (&res, out.handoff) {
                engine.continue_handoff(&session.id, &from_name, &from_mode, &to, &summary, hops).await;
            }
        });
        Ok(run_id)
    }

    /// The run of a session ended asking for a handoff: moves the session to the new agent and
    /// starts it with the summary. A handoff that cannot be carried out leaves a note instead.
    async fn continue_handoff(&self, session_id: &str, from_name: &str, from_mode: &str, to: &str, summary: &str, hops: u32) {
        let note = |text: String| {
            let msg = ChatMessage {
                id: new_id("msg"),
                role: "system".into(),
                content: text,
                reasoning: None,
                tool_calls: None,
                tool_call_id: None,
                name: Some(HANDOFF_MESSAGE.into()),
                created_at: now_rfc3339(),
            };
            let _ = self.0.store.append_messages(session_id, &[msg]);
        };
        if hops >= Self::MAX_HANDOFFS {
            return note(format!("Limite di {} passaggi tra agenti raggiunto: la conversazione si ferma qui.", Self::MAX_HANDOFFS));
        }
        let target = match self.agent(to) {
            Ok(a) if host::mode_rank(&a.mode) <= host::mode_rank(from_mode) => a,
            Ok(a) => return note(format!("Passaggio a «{}» rifiutato: ha una modalità più permissiva di quella di partenza.", a.name)),
            Err(e) => return note(format!("Passaggio a `{to}` non riuscito: {e}")),
        };
        match self.0.store.session(session_id) {
            Ok(mut s) => {
                s.agent_id = to.to_string();
                s.updated_at = now_rfc3339();
                let _ = self.0.store.upsert_session(&s);
            }
            Err(_) => return, // the session was deleted meanwhile
        }
        self.audit_user("agent.handoff", &format!("{from_name} → {} (sessione {session_id})", target.name));
        let input = format!("[Passaggio da {from_name}] {summary}");
        if let Err(e) = self.start_run_inner(session_id, input, Some(HANDOFF_MESSAGE.into()), hops + 1).await {
            note(format!("Il passaggio a «{}» non è partito: {e}", target.name));
        }
    }

    pub fn cancel_run(&self, run_id: &str) -> Result<()> {
        let runs = self.0.runs.lock().unwrap();
        let h = runs.get(run_id).ok_or_else(|| Error::NotFound(format!("run {run_id} non trovato")))?;
        h.cancel.cancel();
        Ok(())
    }

    // ------------------------------------------------------------------ approvals
    pub fn approvals(&self) -> Vec<ApprovalRequest> {
        self.0.approvals.lock().unwrap().values().map(|p| p.req.clone()).collect()
    }
    /// Seconds after which an approval request of an agent is refused (`approvals.timeout_s`, 0 = never).
    fn approval_timeout(&self) -> Option<Duration> {
        self.0.config.get("approvals.timeout_s").and_then(|v| v.as_u64()).filter(|s| *s > 0).map(Duration::from_secs)
    }
    pub(crate) async fn wait_approval(&self, req: ApprovalRequest, session_id: Option<String>) -> bool {
        self.wait_approval_outcome(req, session_id, self.approval_timeout()).await == ApprovalOutcome::Allowed
    }
    /// Waits for the user's answer for at most `timeout`. The request is withdrawn when the wait
    /// ends for any reason, and a timeout is written to the audit trail as `decision: "timeout"`.
    pub(crate) async fn wait_approval_outcome(
        &self,
        req: ApprovalRequest,
        session_id: Option<String>,
        timeout: Option<Duration>,
    ) -> ApprovalOutcome {
        /// Withdraws the request when the run stops waiting (answered, cancelled, dropped): a stale
        /// request must not stay in the inbox, where a later "allow" would record a decision
        /// that nobody was waiting for.
        struct Withdraw<'a>(&'a Engine, String);
        impl Drop for Withdraw<'_> {
            fn drop(&mut self) {
                self.0 .0.approvals.lock().unwrap().remove(&self.1);
            }
        }
        let (tx, rx) = oneshot::channel();
        let _withdraw = Withdraw(self, req.id.clone());
        let (run_id, tool, reason) = (req.run_id.clone(), req.tool.clone(), req.reason.clone());
        self.0.approvals.lock().unwrap().insert(req.id.clone(), PendingApproval { req, tx, session_id });
        let answer = match timeout {
            Some(limit) => match tokio::time::timeout(limit, rx).await {
                Ok(a) => a,
                Err(_) => {
                    let _ = self.0.store.add_audit(&AuditEntry {
                        id: new_id("aud"),
                        ts: now_rfc3339(),
                        run_id: Some(run_id),
                        actor: "system".into(),
                        action: "approval".into(),
                        tool: Some(tool),
                        decision: Some("timeout".into()),
                        detail: format!("nessuna risposta entro {} s: {reason}", limit.as_secs()),
                    });
                    return ApprovalOutcome::TimedOut;
                }
            },
            None => rx.await,
        };
        if answer.unwrap_or(false) {
            ApprovalOutcome::Allowed
        } else {
            ApprovalOutcome::Denied
        }
    }
    pub fn respond_approval(&self, id: &str, allow: bool, remember: bool) -> Result<()> {
        let p = self
            .0
            .approvals
            .lock()
            .unwrap()
            .remove(id)
            .ok_or_else(|| Error::NotFound(format!("richiesta {id} non trovata (forse già gestita)")))?;
        if allow && remember {
            if let Some(s) = &p.session_id {
                self.session_allow(s).lock().unwrap().insert(p.req.tool.clone());
            }
        }
        let _ = self.0.store.add_audit(&AuditEntry {
            id: new_id("aud"),
            ts: now_rfc3339(),
            run_id: Some(p.req.run_id.clone()),
            actor: "user".into(),
            action: "approval".into(),
            tool: Some(p.req.tool.clone()),
            decision: Some(if allow { "approved" } else { "rejected" }.into()),
            detail: if remember && allow { "consentito per tutta la sessione".into() } else { p.req.reason.clone() },
        });
        let _ = p.tx.send(allow);
        Ok(())
    }

    // ------------------------------------------------------------------ workflows
    pub fn workflows(&self) -> Result<Vec<WorkflowDef>> {
        self.0.store.list_docs("workflow")
    }
    pub fn save_workflow(&self, mut w: WorkflowDef) -> Result<WorkflowDef> {
        aiw_workflow::validate(&w)?;
        if let WorkflowTrigger::Webhook { secret } = &mut w.trigger {
            if secret.trim().len() < 16 {
                *secret = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
            }
        }
        self.0.store.put_doc("workflow", &w.id, &w)?;
        Ok(w)
    }

    /// Starts a webhook-triggered workflow after checking its secret (plain or HMAC-SHA256 of
    /// the body, GitHub style). A JSON object body is merged into the inputs; any body is also
    /// available as `{{inputs.payload}}`.
    pub async fn trigger_webhook(&self, id: &str, secret: Option<&str>, signature: Option<&str>, body: &[u8]) -> Result<WorkflowRun> {
        let wf: WorkflowDef = self.0.store.get_doc("workflow", id)?.ok_or_else(|| Error::NotFound(format!("workflow {id} non trovato")))?;
        let WorkflowTrigger::Webhook { secret: expected } = &wf.trigger else {
            return Err(Error::NotFound(format!("workflow {id} senza trigger webhook")));
        };
        if !wf.enabled {
            return Err(Error::Denied("workflow disattivato".into()));
        }
        let by_secret = secret.map(|s| webhook::constant_time_eq(s.as_bytes(), expected.as_bytes())).unwrap_or(false);
        let by_signature = signature.map(|sig| webhook::verify_signature(expected.as_bytes(), body, sig)).unwrap_or(false);
        if expected.is_empty() || !(by_secret || by_signature) {
            return Err(Error::Denied("segreto del webhook non valido".into()));
        }
        let mut inputs = Map::new();
        match serde_json::from_slice::<Value>(body) {
            Ok(Value::Object(o)) => {
                inputs.insert("payload".into(), Value::String(String::from_utf8_lossy(body).into_owned()));
                inputs.extend(o);
            }
            Ok(other) => {
                inputs.insert("payload".into(), other);
            }
            Err(_) => {
                inputs.insert("payload".into(), Value::String(String::from_utf8_lossy(body).chars().take(100_000).collect()));
            }
        }
        self.run_workflow(id, inputs, "webhook").await
    }
    pub fn delete_workflow(&self, id: &str) -> Result<bool> {
        self.0.store.delete_doc("workflow", id)
    }
    pub fn workflow_runs(&self) -> Vec<WorkflowRun> {
        let mem = self.0.workflow_runs.lock().unwrap().clone();
        let mut all = self.0.store.workflow_runs(50).unwrap_or_default();
        for r in mem {
            if let Some(x) = all.iter_mut().find(|x| x.id == r.id) {
                *x = r;
            } else {
                all.insert(0, r);
            }
        }
        all.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        all
    }
    /// Starts a workflow in the background and returns its first state.
    pub async fn run_workflow(&self, id: &str, inputs: Map<String, Value>, actor: &str) -> Result<WorkflowRun> {
        let wf: WorkflowDef = self.0.store.get_doc("workflow", id)?.ok_or_else(|| Error::NotFound(format!("workflow {id} non trovato")))?;
        let (tx, rx) = oneshot::channel();
        let host = EngineHost::for_workflow(self.clone(), wf.clone(), Some(tx));
        let _ = self.0.store.add_audit(&AuditEntry {
            id: new_id("aud"),
            ts: now_rfc3339(),
            run_id: None,
            actor: actor.into(),
            action: "workflow.run".into(),
            tool: None,
            decision: None,
            detail: wf.name.clone(),
        });
        tokio::spawn(async move {
            aiw_workflow::run_workflow(&wf, inputs, &host).await;
        });
        rx.await.map_err(|_| Error::Runtime("il workflow non è partito".into()))
    }

    /// Ends a run that nobody is going to finish: the steps that were going are `failed`, the run
    /// `cancelled`, its saved state is dropped.
    pub(crate) fn abandon_workflow_run(&self, run_id: &str, reason: &str) {
        let known = self.0.workflow_runs.lock().unwrap().iter().find(|r| r.id == run_id).cloned();
        let run = known.or_else(|| self.0.store.workflow_run(run_id).ok().flatten());
        if let Some(mut run) = run.filter(|r| matches!(r.status.as_str(), "running" | "waiting_approval")) {
            for s in run.steps.iter_mut().filter(|s| matches!(s.status.as_str(), "running" | "waiting")) {
                s.status = "failed".into();
                s.error = Some(reason.into());
            }
            run.status = "cancelled".into();
            run.finished_at = Some(now_rfc3339());
            let _ = self.0.store.save_workflow_run(&run);
            if let Some(m) = self.0.workflow_runs.lock().unwrap().iter_mut().find(|r| r.id == run_id) {
                *m = run;
            }
        }
        let _ = self.0.store.clear_workflow_state(run_id);
    }

    /// Goes on with the workflow runs that were interrupted by a restart (state saved at every
    /// step). The step that was running starts again from the beginning, so a step can run more
    /// than once (at-least-once): a step with side effects should be safe to repeat. An approval
    /// that was pending is asked again. A run whose workflow no longer exists is marked failed.
    fn resume_workflows(&self) {
        let states = self.0.store.workflow_states().unwrap_or_default();
        for mut st in states {
            let run_id = st.run.id.clone();
            // A sub-workflow is never resumed on its own: its parent starts it again from the step
            // that was waiting for it.
            if st.run.parent_run_id.is_some() {
                self.abandon_workflow_run(&run_id, "interrotto dal riavvio: il workflow padre lo riavvia");
                continue;
            }
            let live = matches!(st.run.status.as_str(), "running" | "waiting_approval");
            let finished = self
                .0
                .store
                .workflow_run(&run_id)
                .ok()
                .flatten()
                .is_some_and(|r| !matches!(r.status.as_str(), "running" | "waiting_approval"));
            if !live || finished {
                let _ = self.0.store.clear_workflow_state(&run_id);
                continue;
            }
            let wf: Option<WorkflowDef> = self.0.store.get_doc("workflow", &st.run.workflow_id).ok().flatten();
            let Some(wf) = wf else {
                let at = st.next.min(st.run.steps.len().saturating_sub(1));
                if let Some(step) = st.run.steps.get_mut(at) {
                    step.status = "failed".into();
                    step.error = Some(format!("il workflow «{}» non esiste più: run interrotto dal riavvio", st.run.workflow_id));
                }
                st.run.status = "failed".into();
                st.run.finished_at = Some(now_rfc3339());
                let _ = self.0.store.save_workflow_run(&st.run);
                let _ = self.0.store.clear_workflow_state(&run_id);
                continue;
            };
            let _ = self.0.store.add_audit(&AuditEntry {
                id: new_id("aud"),
                ts: now_rfc3339(),
                run_id: Some(run_id.clone()),
                actor: "workflow".into(),
                action: "workflow.resume".into(),
                tool: None,
                decision: None,
                detail: format!("{} (dal passo {})", wf.name, st.next + 1),
            });
            st.run.status = "running".into();
            let host = EngineHost::for_workflow(self.clone(), wf, None);
            tokio::spawn(async move {
                aiw_workflow::run_from(st, &host).await;
            });
        }
    }

    // ------------------------------------------------------------------ tools & MCP
    /// The registry tools (built-in and MCP) followed by the internal tools of the agent loop
    /// (`source: "internal"`: always on, listed so that an agent profile can name them).
    pub fn tools(&self) -> Vec<ToolInfo> {
        let mut all = self.0.tools.info();
        all.extend(aiw_agent::internal_tools().into_iter().map(|t| ToolInfo {
            name: t.name.into(),
            description: t.description.into(),
            source: "internal".into(),
            server: None,
            // They act on nothing by themselves: whatever they trigger goes through the policy as a tool call of its own.
            risk: "read".into(),
            enabled: true,
            available: true,
            unavailable_reason: None,
            parameters: t.parameters,
        }));
        all
    }
    pub fn set_tool_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        if aiw_agent::internal::is_internal(name) {
            return Err(Error::Invalid(format!("«{name}» è uno strumento interno dell'agente: non si può attivare o disattivare")));
        }
        self.0.tools.get(name).ok_or_else(|| Error::NotFound(format!("tool {name} non trovato")))?;
        self.0.tools.set_enabled(name, enabled);
        self.0.store.put_doc("tool_settings", "disabled", &self.0.tools.disabled())
    }
    pub fn mcp_servers(&self) -> Vec<McpServerInfo> {
        let status = self.0.mcp_status.read().unwrap().clone();
        let tools = self.0.tools.info();
        self.0
            .store
            .list_docs::<McpServerConfig>("mcp")
            .unwrap_or_default()
            .into_iter()
            .map(|c| {
                let (st, err) = status.get(&c.id).cloned().unwrap_or_else(|| ("disconnected".into(), None));
                let t = tools.iter().filter(|t| t.server.as_deref() == Some(c.name.as_str())).map(|t| t.name.clone()).collect();
                McpServerInfo { config: secrets::masked(c), status: st, tools: t, error: err }
            })
            .collect()
    }
    async fn connect_mcp(&self, cfg: McpServerConfig) -> Result<()> {
        self.0.mcp_status.write().unwrap().insert(cfg.id.clone(), ("disconnected".into(), None));
        match McpClient::connect(&cfg).await {
            Ok(c) => {
                let c = Arc::new(c);
                self.0.tools.unregister_prefix(&aiw_tools::openai_safe_name(&format!("mcp__{}__", cfg.name)));
                for t in McpTool::wrap(c.clone()) {
                    self.0.tools.register(t);
                }
                if let Some(old) = self.0.mcp.lock().await.insert(cfg.id.clone(), c) {
                    old.shutdown().await;
                }
                self.0.mcp_status.write().unwrap().insert(cfg.id.clone(), ("connected".into(), None));
                Ok(())
            }
            Err(e) => {
                tracing::warn!(server = %cfg.name, "MCP connection failed: {e}");
                self.0.mcp_status.write().unwrap().insert(cfg.id.clone(), ("error".into(), Some(e.to_string())));
                Err(e)
            }
        }
    }
    /// Saves (and connects, when enabled) an MCP server. The configuration is checked before
    /// anything is stored; a header or variable that comes back as [`SECRET_MASK`] keeps the
    /// secret already stored for it.
    pub async fn save_mcp_server(&self, mut cfg: McpServerConfig) -> Result<McpServerInfo> {
        validate_mcp(&cfg)?;
        cfg.name = cfg.name.trim().replace(' ', "_");
        cfg.url = cfg.url.map(|u| u.trim().to_string());
        cfg.command = cfg.command.map(|c| c.trim().to_string());
        if cfg.id.is_empty() {
            cfg.id = new_id("mcp");
        }
        let stored = self.0.store.get_doc::<McpServerConfig>("mcp", &cfg.id)?;
        let cfg = secrets::unmasked(cfg, stored.as_ref());
        self.0.store.put_doc("mcp", &cfg.id, &cfg)?;
        if cfg.enabled {
            let _ = self.connect_mcp(cfg.clone()).await;
        }
        self.mcp_servers()
            .into_iter()
            .find(|s| s.config.id == cfg.id)
            .ok_or_else(|| Error::NotFound(format!("server MCP {} non trovato", cfg.id)))
    }
    pub async fn remove_mcp_server(&self, id: &str) -> Result<()> {
        let Some(cfg) = self.0.store.get_doc::<McpServerConfig>("mcp", id)? else {
            return Err(Error::NotFound(format!("server MCP {id} non trovato")));
        };
        self.0.tools.unregister_prefix(&aiw_tools::openai_safe_name(&format!("mcp__{}__", cfg.name)));
        if let Some(c) = self.0.mcp.lock().await.remove(id) {
            c.shutdown().await;
        }
        self.0.mcp_status.write().unwrap().remove(id);
        self.0.store.delete_doc("mcp", id)?;
        Ok(())
    }

    // ------------------------------------------------------------------ config & audit
    pub fn config(&self) -> ConfigView {
        self.0.config.view()
    }
    pub fn set_config(&self, layer: &str, key: &str, value: Value) -> Result<ConfigView> {
        self.0.config.set(Layer::parse(layer)?, key, value)?;
        if key == "workspace.root" {
            if let Some(p) = self.0.config.get_str("workspace.root") {
                let root = aiw_core::paths::expand_home(&p);
                std::fs::create_dir_all(&root)?;
                *self.0.workspace_root.write().unwrap() = root.clone();
                self.0.config.set_workspace_root(Some(&root));
            }
        }
        self.audit_user("config.set", &format!("{layer}:{key}"));
        Ok(self.config())
    }
    pub fn audit(&self) -> Vec<AuditEntry> {
        self.0.store.audit(500).unwrap_or_default()
    }

    // ------------------------------------------------------------------ checkpoints
    /// What a rewind of the run would do. `NotFound` when the run saved no checkpoint.
    pub fn checkpoint_info(&self, run_id: &str) -> Result<CheckpointInfo> {
        if !aiw_tools::checkpoint::valid_run_id(run_id) {
            return Err(Error::Invalid("identificativo di run non valido".into()));
        }
        aiw_tools::checkpoint::plan(&self.0.dirs.checkpoints(), run_id)
            .ok_or_else(|| Error::NotFound(format!("nessun checkpoint per il run {run_id}")))
    }

    /// Checkpoints of the runs of a session, newest run first.
    pub fn session_checkpoints(&self, session_id: &str) -> Result<Vec<CheckpointInfo>> {
        let ids = self.0.store.session_run_ids(session_id)?;
        Ok(ids.iter().filter_map(|id| aiw_tools::checkpoint::plan(&self.0.dirs.checkpoints(), id)).collect())
    }

    /// Puts back the originals saved by the run and deletes the files it created. Refused while the run is going.
    pub fn rewind(&self, run_id: &str) -> Result<RewindResult> {
        let info = self.checkpoint_info(run_id)?;
        let agent_running = self.0.runs.lock().unwrap().get(run_id).is_some_and(|r| !r.done);
        let workflow_running = self
            .0
            .workflow_runs
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.id == run_id && matches!(r.status.as_str(), "running" | "waiting_approval"));
        if agent_running || workflow_running {
            return Err(Error::Conflict("il run è ancora in corso: fermalo prima di ripristinare i file".into()));
        }
        let res = aiw_tools::checkpoint::rewind(&self.0.dirs.checkpoints(), run_id)?;
        let _ = self.0.store.add_audit(&AuditEntry {
            id: new_id("aud"),
            ts: now_rfc3339(),
            run_id: Some(run_id.into()),
            actor: "user".into(),
            action: "checkpoint.rewind".into(),
            tool: None,
            decision: None,
            detail: format!(
                "ripristinati {}, eliminati {}, esterni {}, falliti {} (previsti {} + {})",
                res.restored.len(),
                res.deleted.len(),
                res.external.len(),
                res.failed.len(),
                info.restore.len(),
                info.delete.len()
            ),
        });
        Ok(res)
    }

    // ------------------------------------------------------------------ OpenAI-compatible gateway
    /// Model ids exposed on `/v1/models`: inference profiles and local GGUF files.
    pub fn gateway_models(&self) -> Vec<String> {
        let mut v: Vec<String> = self.profiles().unwrap_or_default().into_iter().map(|p| p.id).collect();
        v.extend(self.0.models.read().unwrap().iter().filter(|m| m.format == "gguf").map(|m| m.id.clone()));
        v
    }
    /// Resolves a gateway model name to a backend and the model to send to it.
    pub async fn gateway_backend(&self, model: &str) -> Result<(Arc<dyn ChatBackend>, String)> {
        if let Ok(p) = self.profile(model) {
            return Ok((self.0.runtimes.backend_with(&p.runtime_id, Some(&p.model), p.params.context).await?, p.model));
        }
        if self.0.models.read().unwrap().iter().any(|m| m.id == model) {
            // A bare local GGUF: llama-server when it is there, else the in-process engine.
            let avail = defaults::Availability::from_runtimes(&self.0.runtimes.list());
            let runtime = avail.gguf_runtime().unwrap_or("llama-server");
            return Ok((self.0.runtimes.backend(runtime, Some(model)).await?, model.to_string()));
        }
        if let Some((rt, m)) = model.split_once('/') {
            if self.0.runtimes.manifest(rt).is_ok() {
                return Ok((self.0.runtimes.backend(rt, Some(m)).await?, m.to_string()));
            }
        }
        let p = self.fallback_profile()?;
        Ok((self.0.runtimes.backend_with(&p.runtime_id, Some(&p.model), p.params.context).await?, p.model))
    }

    /// Profile used when a gateway client names no known model: `default`, else the profile of the
    /// default agent (`agent.default`), else the first profile (`default` may have been deleted).
    fn fallback_profile(&self) -> Result<InferenceProfile> {
        if let Ok(p) = self.profile("default") {
            return Ok(p);
        }
        let agent = self.0.config.get_str("agent.default").unwrap_or_else(|| "operator".into());
        if let Some(p) = self.agent(&agent).ok().and_then(|a| self.profile(&a.inference_profile).ok()) {
            return Ok(p);
        }
        self.profiles()?
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound("nessun profilo di inferenza: creane uno in Agenti → Profili".into()))
    }

    /// Runs one tool on behalf of an external caller (workflow, MCP client, API) with the
    /// workspace defaults: policy from `policy.mode` + rules, approvals in the global inbox,
    /// sandbox from `sandbox.mode`, audit trail.
    pub async fn execute_tool(
        &self,
        run_id: &str,
        actor_kind: &str,
        actor: &str,
        tool: &str,
        args: Value,
    ) -> Result<aiw_tools::ToolOutput> {
        let reg = self.tool_registry();
        let t = reg.get(tool).ok_or_else(|| Error::NotFound(format!("tool {tool} non trovato")))?;
        if !reg.is_enabled(tool) {
            return Err(Error::Denied(format!("il tool {tool} è disattivato")));
        }
        if let Err(why) = t.available() {
            return Err(Error::Unsupported(why));
        }
        let mode = self.0.config.get_str("policy.mode").unwrap_or_else(|| "ask".into());
        let policy = Policy { mode, rules: self.policy_rules(), sandbox_available: aiw_tools::shell::sandbox_backend().is_some() };
        let subject = t.subject(&args);
        let audit = |decision: &str| {
            let _ = self.0.store.add_audit(&AuditEntry {
                id: new_id("aud"),
                ts: now_rfc3339(),
                run_id: Some(run_id.into()),
                actor: actor_kind.into(),
                action: "tool".into(),
                tool: Some(tool.into()),
                decision: Some(decision.into()),
                detail: format!("{actor}: {}", subject.clone().unwrap_or_default()),
            });
        };
        match policy.evaluate(tool, t.risk(), subject.as_deref(), false) {
            aiw_agent::Decision::Allow => audit("allow"),
            aiw_agent::Decision::Deny(r) => {
                audit("deny");
                return Err(Error::Denied(format!("negato dalla policy: {r}")));
            }
            aiw_agent::Decision::Ask(reason) => {
                let req = ApprovalRequest {
                    id: new_id("apr"),
                    run_id: run_id.into(),
                    tool: tool.into(),
                    args: args.clone(),
                    reason: format!("{actor}: {reason}"),
                    risk: aiw_agent::policy::risk_level(t.risk()).into(),
                };
                match self.wait_approval_outcome(req, None, self.approval_timeout()).await {
                    ApprovalOutcome::Allowed => {}
                    ApprovalOutcome::Denied => {
                        audit("rejected");
                        return Err(Error::Denied("rifiutato dall'utente".into()));
                    }
                    ApprovalOutcome::TimedOut => {
                        audit("timeout");
                        return Err(Error::Denied(
                            "nessuna risposta entro il tempo limite (approvals.timeout_s): richiesta rifiutata".into(),
                        ));
                    }
                }
                audit("approved");
            }
        }
        let mut ctx = ToolContext::new(run_id, self.workspace_root());
        ctx.sandbox = SandboxMode::parse(&self.0.config.get_str("sandbox.mode").unwrap_or_else(|| "workspace-write".into()));
        ctx.checkpoint_dir = Some(self.0.dirs.checkpoints());
        ctx.allow_domains = self.0.config.get("network.allow_domains").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        t.call(args, &ctx).await
    }

    /// Runs an agent to completion and returns `(final answer, session id)`. Reuses
    /// `session_id` when given, otherwise creates a session titled `title`.
    pub async fn run_agent_task(&self, agent_id: &str, prompt: &str, session_id: Option<&str>, title: &str) -> Result<(String, String)> {
        let session_id = match session_id {
            Some(s) => s.to_string(),
            None => self.create_session(agent_id, Some(title.to_string()))?.id,
        };
        let mut rx = self.subscribe();
        let run_id = self.start_run(&session_id, prompt.to_string()).await?;
        // If the caller stops waiting (a workflow step timeout, a dropped task) the agent run must
        // not go on in the background.
        struct CancelOnDrop<'a>(&'a Engine, &'a str);
        impl Drop for CancelOnDrop<'_> {
            fn drop(&mut self) {
                let _ = self.0.cancel_run(self.1);
            }
        }
        let cancel_guard = CancelOnDrop(self, &run_id);
        loop {
            if self.run_done(&run_id) {
                break;
            }
            match tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
                Ok(Ok(m)) if m.run_id == run_id && m.event.event.is_terminal() => break,
                _ => {}
            }
        }
        std::mem::forget(cancel_guard);
        // Let the run task persist its messages.
        for _ in 0..50 {
            if self.0.store.session(&session_id).ok().and_then(|s| s.last_status).as_deref() != Some("running") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if let Some(err) = self.run_events(&run_id).iter().find_map(|e| {
            if let AgEvent::RunError { message, .. } = &e.event.event {
                Some(message.clone())
            } else {
                None
            }
        }) {
            return Err(Error::Runtime(err));
        }
        let text = self
            .messages(&session_id)?
            .into_iter()
            .rev()
            .find(|m| m.role == "assistant" && !m.content.is_empty())
            .map(|m| m.content)
            .unwrap_or_default();
        Ok((text, session_id))
    }

    /// Tool execution for workflows and the API (policy + approval apply).
    pub(crate) fn tool_registry(&self) -> Arc<ToolRegistry> {
        self.0.tools.clone()
    }
}

/// Checks an MCP server configuration before it is stored: a name, a known transport, the
/// command of a `stdio` server, a valid `http(s)` address for an `http` one.
fn validate_mcp(cfg: &McpServerConfig) -> Result<()> {
    if cfg.name.trim().is_empty() {
        return Err(Error::Invalid("serve un nome".into()));
    }
    match cfg.transport.as_str() {
        "stdio" => {
            if cfg.command.as_deref().is_none_or(|c| c.trim().is_empty()) {
                return Err(Error::Invalid("un server MCP stdio richiede il comando da avviare".into()));
            }
        }
        "http" => {
            let url = cfg.url.as_deref().map(str::trim).filter(|u| !u.is_empty());
            let Some(url) = url else {
                return Err(Error::Invalid("un server MCP http richiede l'indirizzo (url)".into()));
            };
            match reqwest::Url::parse(url) {
                Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => {}
                _ => return Err(Error::Invalid(format!("indirizzo MCP non valido: {url} (serve un URL http o https)"))),
            }
        }
        other => return Err(Error::Invalid(format!("trasporto MCP `{other}` non valido: usa stdio o http"))),
    }
    Ok(())
}

/// Registers in-process llama.cpp as the `llama-embedded` runtime with its current settings.
/// Title of a new session: the first non-empty line of its first message, at most 60 chars.
fn session_title(input: &str) -> String {
    input.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.chars().take(60).collect()).unwrap_or_else(|| "Sessione".into())
}

/// A workflow file inside the working folder is repository content: whoever wrote the repository
/// could schedule commands with it. It is imported disabled; once the user enables it from the
/// app the choice holds only while the file stays exactly as it was when it was enabled.
fn workspace_workflow(mut wf: WorkflowDef, stored: Option<&WorkflowDef>) -> WorkflowDef {
    let unchanged = stored.map(|st| WorkflowDef { enabled: wf.enabled, ..st.clone() } == wf).unwrap_or(false);
    wf.enabled = unchanged && stored.map(|st| st.enabled).unwrap_or(false);
    wf
}

#[cfg(feature = "llama-embedded")]
fn register_embedded(runtimes: &RuntimeRegistry, resolver: aiw_runtimes::ModelResolver) {
    let s = runtimes.settings("llama-embedded").unwrap_or_default();
    let settings = aiw_runtimes::embedded::EmbeddedSettings {
        ctx_size: s.get("ctx_size").and_then(|v| v.as_u64()).unwrap_or(8192) as u32,
        gpu_layers: s.get("gpu_layers").and_then(|v| v.as_u64()).unwrap_or(999) as u32,
        threads: None,
    };
    runtimes.register_bridge("llama-embedded", Arc::new(aiw_runtimes::embedded::EmbeddedLlama::new(resolver, settings)));
}

/// Registers ONNX Runtime GenAI (loaded at run time, see `aiw_runtimes::ortgenai`) as `ort-genai`.
#[cfg(not(target_os = "ios"))]
fn register_ort_genai(runtimes: &RuntimeRegistry, resolver: aiw_runtimes::ModelResolver) {
    let s = runtimes.settings("ort-genai").unwrap_or_default();
    let d = aiw_runtimes::ortgenai::OrtGenAiSettings::default();
    let settings = aiw_runtimes::ortgenai::OrtGenAiSettings {
        library_path: s.get("library_path").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        execution_provider: s.get("execution_provider").and_then(|v| v.as_str()).unwrap_or(&d.execution_provider).to_string(),
        max_length: s.get("max_length").and_then(|v| v.as_u64()).map(|v| v as u32).unwrap_or(d.max_length),
    };
    runtimes.register_bridge("ort-genai", Arc::new(aiw_runtimes::ortgenai::OrtGenAi::new(resolver, settings)));
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_hardening;
#[cfg(test)]
mod tests_inference;
#[cfg(test)]
mod tests_validation;
#[cfg(test)]
mod tests_workflow;

fn sandbox_rank(s: &str) -> u8 {
    match s {
        "read-only" => 0,
        "workspace-write" => 1,
        _ => 2,
    }
}
