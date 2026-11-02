//! `Engine` — the single facade used by the HTTP server, the CLI and the Tauri shells (ADR 0002).

pub mod defaults;
mod host;
pub mod store;

pub use host::EngineHost;

use aiw_agent::{to_msgs, AgentRunner, HookConfig, Policy, PolicyRule, RunInput};
use aiw_core::api::*;
use aiw_core::config::{app_schema, Layer, LayeredConfig};
use aiw_core::events::{AgEvent, SeqEvent, TimedEvent};
use aiw_core::paths::DataDirs;
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use aiw_models::DownloadManager;
use aiw_runtimes::{ChatBackend, RuntimeRegistry};
use aiw_tools::browser::BrowserManager;
use aiw_tools::mcp::{McpClient, McpTool};
use aiw_tools::{SandboxMode, ToolContext, ToolRegistry};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyDoc {
    #[serde(default)]
    pub rules: Vec<PolicyRule>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksDoc {
    #[serde(default)]
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
    pub models: Arc<RwLock<Vec<LocalModel>>>,
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
}

#[derive(Clone)]
pub struct Engine(pub(crate) Arc<Inner>);

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
        let models = Arc::new(RwLock::new(aiw_models::scan_models(&models_dir)));
        let m2 = models.clone();
        runtimes.set_model_resolver(Arc::new(move |id: &str| {
            let list = m2.read().unwrap();
            list.iter().find(|m| m.id == id || m.name == id).and_then(|m| m.path.clone()).map(PathBuf::from).or_else(|| {
                let p = PathBuf::from(id);
                p.exists().then_some(p)
            })
        }));
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
        let engine = Engine(Arc::new(Inner {
            mode: if opts.mode.is_empty() { "embedded".into() } else { opts.mode.clone() },
            started: Instant::now(),
            downloads: DownloadManager::new(models_dir.clone()),
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
        }));
        if !opts.offline {
            engine.0.runtimes.refresh().await;
        }
        engine.seed()?;
        if !opts.offline {
            engine.start_background();
        }
        Ok(engine)
    }

    fn seed(&self) -> Result<()> {
        let s = &self.0.store;
        if s.list_docs::<AgentProfile>("agent")?.is_empty() {
            for a in defaults::agents() {
                s.put_doc("agent", &a.id, &a)?;
            }
        }
        if s.get_doc::<InferenceProfile>("profile", "default")?.is_none() {
            let running_ollama =
                self.0.runtimes.info("ollama").ok().filter(|r| r.status == "running").and_then(|r| r.loaded_models.first().cloned());
            let p = defaults::default_profile(&self.0.models.read().unwrap(), running_ollama.as_deref());
            s.put_doc("profile", &p.id, &p)?;
        }
        if s.list_docs::<WorkflowDef>("workflow")?.is_empty() {
            for w in defaults::workflows() {
                s.put_doc("workflow", &w.id, &w)?;
            }
        }
        // Workflows dropped in ~/.aiw/workflows or <workspace>/.aiw/workflows (YAML/JSON).
        for dir in [self.0.dirs.workflows(), self.workspace_root().join(".aiw").join("workflows")] {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    if let Ok(wf) = aiw_workflow::load_file(&e.path()) {
                        s.put_doc("workflow", &wf.id, &wf)?;
                    }
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
    /// Exchanges a pairing code for the API token (LAN devices).
    pub fn pair(&self, code: &str) -> Option<String> {
        let ok = code.replace(' ', "") == self.pairing_code();
        if ok {
            *self.0.pairing.lock().unwrap() = (random_digits(6), Instant::now());
            self.audit_system("pair", "nuovo dispositivo accoppiato");
            Some(self.0.token.clone())
        } else {
            None
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

    fn audit_system(&self, action: &str, detail: &str) {
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
                let model = model
                    .or_else(|| self.profiles().ok()?.into_iter().find(|p| p.runtime_id == id && !p.model.is_empty()).map(|p| p.model));
                self.0.runtimes.start(id, model).await
            }
            "stop" => self.0.runtimes.stop(id).await,
            "refresh" => self.0.runtimes.refresh_one(id).await,
            other => Err(Error::Invalid(format!("azione sconosciuta: {other}"))),
        }?;
        self.audit_user(&format!("runtime.{action}"), &r.name);
        Ok(r)
    }
    pub async fn runtime_logs(&self, id: &str) -> Vec<String> {
        self.0.runtimes.logs(id).await
    }
    pub fn update_runtime_settings(&self, id: &str, settings: Map<String, Value>) -> Result<RuntimeInfo> {
        let m = self.0.runtimes.manifest(id)?;
        // A redacted secret coming back from the UI keeps the stored value.
        let settings: Map<String, Value> = settings
            .into_iter()
            .filter(|(k, v)| !(m.settings.properties.get(k).map(|f| f.secret).unwrap_or(false) && v.as_str() == Some("••••••")))
            .collect();
        let saved = self.0.runtimes.update_settings(id, settings)?;
        self.0.store.put_doc("runtime_settings", id, &json!({"id": id, "settings": saved}))?;
        self.0.runtimes.info(id)
    }

    // ------------------------------------------------------------------ models
    pub fn rescan_models(&self) -> Vec<LocalModel> {
        let found = aiw_models::scan_models(&self.0.models_dir);
        *self.0.models.write().unwrap() = found.clone();
        found
    }
    pub fn models(&self) -> Vec<LocalModel> {
        self.rescan_models()
    }
    pub fn catalog(&self) -> Vec<CatalogModel> {
        let device = self.device();
        let installed = self.0.models.read().unwrap().clone();
        aiw_models::builtin_catalog()
            .into_iter()
            .map(|mut c| {
                let (fit, detail) = aiw_models::fit(&c, &device);
                c.fit = fit;
                c.fit_detail = detail;
                c.installed = match &c.source {
                    ModelSource::Hf { file, .. } if !file.is_empty() => {
                        installed.iter().any(|m| m.path.as_deref().map(|p| p.ends_with(file.as_str())).unwrap_or(false))
                    }
                    _ => false,
                };
                c
            })
            .collect()
    }
    /// `{"id": catalog_id}` or `{"repo": "...", "file": "..."}`.
    pub fn pull_model(&self, req: &Value) -> Result<DownloadProgress> {
        let (model_id, repo, file) = if let Some(id) = req["id"].as_str() {
            let c =
                aiw_models::builtin_catalog().into_iter().find(|c| c.id == id).ok_or_else(|| Error::NotFound(format!("modello {id}")))?;
            match c.source {
                ModelSource::Hf { repo, file } if !file.is_empty() => (c.id, repo, file),
                ModelSource::Hf { .. } => {
                    return Err(Error::Invalid("questo modello viene scaricato dal suo runtime (es. mlx-lm) al primo avvio".into()))
                }
                ModelSource::System { .. } => return Err(Error::Invalid("i modelli di sistema non si scaricano".into())),
            }
        } else {
            let repo = req["repo"].as_str().unwrap_or_default().to_string();
            let file = req["file"].as_str().unwrap_or_default().to_string();
            (file.clone(), repo, file)
        };
        let d = self.0.downloads.pull_hf(&model_id, &repo, &file)?;
        self.audit_user("model.pull", &format!("{repo}/{file}"));
        Ok(d)
    }
    pub fn downloads(&self) -> Vec<DownloadProgress> {
        let list = self.0.downloads.list();
        if list.iter().any(|d| d.status == "done") {
            self.rescan_models();
        }
        list
    }

    // ------------------------------------------------------------------ profiles & agents
    pub fn profiles(&self) -> Result<Vec<InferenceProfile>> {
        self.0.store.list_docs("profile")
    }
    pub fn profile(&self, id: &str) -> Result<InferenceProfile> {
        self.0.store.get_doc("profile", id)?.ok_or_else(|| Error::NotFound(format!("profilo {id}")))
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
        self.0.store.get_doc("agent", id)?.ok_or_else(|| Error::NotFound(format!("agente {id}")))
    }
    pub fn save_agent(&self, a: AgentProfile) -> Result<AgentProfile> {
        if !["plan", "ask", "auto-edit", "auto", "yolo"].contains(&a.mode.as_str()) {
            return Err(Error::Invalid(format!("modalità non valida: {}", a.mode)));
        }
        if !["read-only", "workspace-write", "off"].contains(&a.sandbox.as_str()) {
            return Err(Error::Invalid(format!("sandbox non valida: {}", a.sandbox)));
        }
        self.0.store.put_doc("agent", &a.id, &a)?;
        Ok(a)
    }
    pub fn policy_rules(&self) -> Vec<PolicyRule> {
        self.0.store.get_doc::<PolicyDoc>("policy", "default").ok().flatten().unwrap_or_default().rules
    }
    pub fn set_policy_rules(&self, rules: Vec<PolicyRule>) -> Result<()> {
        self.0.store.put_doc("policy", "default", &PolicyDoc { rules })
    }
    pub fn hooks(&self) -> Vec<HookConfig> {
        self.0.store.get_doc::<HooksDoc>("hooks", "default").ok().flatten().unwrap_or_default().hooks
    }
    pub fn set_hooks(&self, hooks: Vec<HookConfig>) -> Result<()> {
        self.0.store.put_doc("hooks", "default", &HooksDoc { hooks })
    }

    // ------------------------------------------------------------------ sessions & runs
    pub fn sessions(&self) -> Result<Vec<Session>> {
        self.0.store.sessions()
    }
    pub fn create_session(&self, agent_id: &str, title: Option<String>) -> Result<Session> {
        self.agent(agent_id)?;
        let now = now_rfc3339();
        let s = Session {
            id: new_id("ses"),
            title: title.unwrap_or_else(|| "Nuova sessione".into()),
            agent_id: agent_id.into(),
            created_at: now.clone(),
            updated_at: now,
            last_status: None,
        };
        self.0.store.upsert_session(&s)?;
        Ok(s)
    }
    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.0.store.delete_session(id)
    }
    pub fn messages(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
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
        let ws = self.workspace_root();
        let device = self.device();
        let mut tool_ctx = ToolContext::new(run_id, ws.clone());
        tool_ctx.read_roots = vec![self.0.dirs.skills(), ws.join(".aiw")];
        tool_ctx.sandbox = SandboxMode::parse(&agent.sandbox);
        tool_ctx.cancel = cancel;
        tool_ctx.checkpoint_dir = Some(self.0.dirs.checkpoints());
        tool_ctx.allow_domains =
            self.0.config.get("network.allow_domains").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        let env = format!(
            "# Ambiente\n- Data e ora: {}\n- Dispositivo: {} ({} {}, classe {})\n- Cartella di lavoro: {}\n- Inferenza locale: runtime `{}`, modello `{}`",
            chrono::Local::now().format("%A %d %B %Y, %H:%M"),
            device.name,
            device.os,
            device.arch,
            device.tier.to_uppercase(),
            ws.display(),
            profile.runtime_id,
            profile.model
        );
        let subagents = self.agents().unwrap_or_default().into_iter().filter(|a| a.id != agent.id).map(|a| (a.id, a.description)).collect();
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
        }
    }

    pub(crate) fn skills(&self) -> Vec<aiw_agent::Skill> {
        aiw_agent::skills::discover(&[self.0.dirs.skills(), self.workspace_root().join(".aiw").join("skills")])
    }

    /// Starts an agent run in a session; events stream on the bus and via [`Engine::run_events`].
    pub async fn start_run(&self, session_id: &str, input: String) -> Result<String> {
        if input.trim().is_empty() {
            return Err(Error::Invalid("messaggio vuoto".into()));
        }
        let mut session = self.0.store.session(session_id)?;
        let agent = self.agent(&session.agent_id)?;
        let profile = self.profile(&agent.inference_profile)?;
        let manifest = self.0.runtimes.manifest(&profile.runtime_id)?;
        if profile.model.is_empty() && manifest.launch.as_ref().map(|l| l.requires_model).unwrap_or(false) {
            return Err(Error::Invalid(format!(
                "Il profilo \"{}\" non ha un modello: scaricane uno da Modelli e selezionalo in Agenti → Profili.",
                profile.name
            )));
        }
        let run_id = new_id("run");
        let cancel = CancellationToken::new();
        self.0
            .runs
            .lock()
            .unwrap()
            .insert(run_id.clone(), RunHandle { session_id: Some(session_id.into()), cancel: cancel.clone(), events: vec![], done: false });
        self.0.store.start_run(&run_id, Some(session_id))?;
        let history = to_msgs(&self.0.store.messages(session_id)?);
        let user_msg = ChatMessage {
            id: new_id("msg"),
            role: "user".into(),
            content: input.clone(),
            reasoning: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
            created_at: now_rfc3339(),
        };
        self.0.store.append_messages(session_id, std::slice::from_ref(&user_msg))?;
        if session.title == "Nuova sessione" {
            session.title = input.chars().take(60).collect::<String>().lines().next().unwrap_or("Sessione").to_string();
        }
        session.updated_at = now_rfc3339();
        session.last_status = Some("running".into());
        self.0.store.upsert_session(&session)?;
        let run_input = self.build_input(&run_id, session_id, agent, profile, history, input, cancel, 0, self.session_allow(session_id));
        let engine = self.clone();
        let rid = run_id.clone();
        tokio::spawn(async move {
            let host = Arc::new(EngineHost::new(engine.clone(), Some(session.id.clone()), None));
            let res = AgentRunner::run(run_input, host).await;
            let status = match &res {
                Ok(out) => {
                    let _ = engine.0.store.append_messages(&session.id, out.new_messages.get(1..).unwrap_or(&[]));
                    "succeeded"
                }
                Err(Error::Cancelled) => "cancelled",
                Err(_) => "failed",
            };
            if let Err(e) = &res {
                if !engine.run_done(&rid) {
                    engine.emit(&rid, AgEvent::RunError { message: e.to_string(), code: None });
                }
            }
            let _ = engine.0.store.finish_run(&rid, status);
            if let Ok(mut s) = engine.0.store.session(&session.id) {
                s.last_status = Some(status.into());
                s.updated_at = now_rfc3339();
                let _ = engine.0.store.upsert_session(&s);
            }
        });
        Ok(run_id)
    }

    pub fn cancel_run(&self, run_id: &str) -> Result<()> {
        let runs = self.0.runs.lock().unwrap();
        let h = runs.get(run_id).ok_or_else(|| Error::NotFound(format!("run {run_id}")))?;
        h.cancel.cancel();
        Ok(())
    }

    // ------------------------------------------------------------------ approvals
    pub fn approvals(&self) -> Vec<ApprovalRequest> {
        self.0.approvals.lock().unwrap().values().map(|p| p.req.clone()).collect()
    }
    pub(crate) async fn wait_approval(&self, req: ApprovalRequest, session_id: Option<String>) -> bool {
        let (tx, rx) = oneshot::channel();
        self.0.approvals.lock().unwrap().insert(req.id.clone(), PendingApproval { req, tx, session_id });
        rx.await.unwrap_or(false)
    }
    pub fn respond_approval(&self, id: &str, allow: bool, remember: bool) -> Result<()> {
        let p =
            self.0.approvals.lock().unwrap().remove(id).ok_or_else(|| Error::NotFound(format!("richiesta {id} (forse già gestita)")))?;
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
    pub fn save_workflow(&self, w: WorkflowDef) -> Result<WorkflowDef> {
        aiw_workflow::validate(&w)?;
        self.0.store.put_doc("workflow", &w.id, &w)?;
        Ok(w)
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
        let wf: WorkflowDef = self.0.store.get_doc("workflow", id)?.ok_or_else(|| Error::NotFound(format!("workflow {id}")))?;
        let (tx, rx) = oneshot::channel();
        let host = EngineHost::for_workflow(self.clone(), wf.clone(), tx);
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

    // ------------------------------------------------------------------ tools & MCP
    pub fn tools(&self) -> Vec<ToolInfo> {
        self.0.tools.info()
    }
    pub fn set_tool_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        self.0.tools.get(name).ok_or_else(|| Error::NotFound(format!("tool {name}")))?;
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
                McpServerInfo { config: c, status: st, tools: t, error: err }
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
    pub async fn save_mcp_server(&self, mut cfg: McpServerConfig) -> Result<McpServerInfo> {
        if cfg.name.trim().is_empty() {
            return Err(Error::Invalid("serve un nome".into()));
        }
        cfg.name = cfg.name.trim().replace(' ', "_");
        if cfg.id.is_empty() {
            cfg.id = new_id("mcp");
        }
        self.0.store.put_doc("mcp", &cfg.id, &cfg)?;
        if cfg.enabled {
            let _ = self.connect_mcp(cfg.clone()).await;
        }
        self.mcp_servers().into_iter().find(|s| s.config.id == cfg.id).ok_or(Error::NotFound(cfg.id))
    }
    pub async fn remove_mcp_server(&self, id: &str) -> Result<()> {
        if let Some(cfg) = self.0.store.get_doc::<McpServerConfig>("mcp", id)? {
            self.0.tools.unregister_prefix(&aiw_tools::openai_safe_name(&format!("mcp__{}__", cfg.name)));
        }
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
            return Ok((self.0.runtimes.backend(&p.runtime_id, Some(&p.model)).await?, p.model));
        }
        if self.0.models.read().unwrap().iter().any(|m| m.id == model) {
            return Ok((self.0.runtimes.backend("llama-server", Some(model)).await?, model.to_string()));
        }
        if let Some((rt, m)) = model.split_once('/') {
            if self.0.runtimes.manifest(rt).is_ok() {
                return Ok((self.0.runtimes.backend(rt, Some(m)).await?, m.to_string()));
            }
        }
        let p = self.profile("default")?;
        Ok((self.0.runtimes.backend(&p.runtime_id, Some(&p.model)).await?, p.model))
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
        let t = reg.get(tool).ok_or_else(|| Error::NotFound(format!("tool {tool}")))?;
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
                return Err(Error::Denied(r));
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
                if !self.wait_approval(req, None).await {
                    audit("rejected");
                    return Err(Error::Denied("rifiutato dall'utente".into()));
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
        loop {
            if self.run_done(&run_id) {
                break;
            }
            match tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
                Ok(Ok(m)) if m.run_id == run_id && m.event.event.is_terminal() => break,
                _ => {}
            }
        }
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

#[cfg(test)]
mod tests;
