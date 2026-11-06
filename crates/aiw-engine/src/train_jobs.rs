//! Fine-tuning jobs and the registry of the adapters they leave (architecture §5.5). A job is an
//! external trainer run as a supervised process: log on file, progress read from the log, one at a
//! time on a device, cancellable. When it succeeds the adapter is registered and, if the job names a
//! suite, compared with the base model.

use crate::Engine;
use aiw_core::api::{AdapterInfo, AdapterRef, DatasetInfo, TrainJob, TrainJobRequest, TrainParams};
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use aiw_runtimes::job::{self, JobEnd, JobSpec};
use aiw_runtimes::trainer::{self, DeviceFacts, TrainerManifest};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// The script AIW hands to TRL and Unsloth; written to `~/.aiw/scripts/train/` when a job starts.
const TRL_SCRIPT: &str = include_str!("../../../scripts/train/trl_train.py");
/// Most jobs the list shows.
const MAX_JOBS: usize = 100;
/// Lines of log the API returns by default.
pub const DEFAULT_LOG_LINES: usize = 200;

/// Jobs in flight: id → the token that cancels it. One entry at a time (one job per device).
#[derive(Default)]
pub(crate) struct TrainState {
    pub running: Mutex<HashMap<String, CancellationToken>>,
    /// Where this process serves the API, once it does.
    pub local_url: Mutex<Option<String>>,
}

/// What a line of a trainer's output says about how far it got.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    pub step: u64,
    pub total: Option<u64>,
    pub loss: Option<f64>,
    pub val_loss: Option<f64>,
}

/// Reads a line of output: `Iter 10: Train loss 2.31, …` / `Iter 200: Val loss 1.9, …` (mlx-lm), or
/// `AIW_PROGRESS {"step":…,"total":…,"loss":…}` (the AIW scripts).
pub fn parse_progress(line: &str) -> Option<Progress> {
    let line = line.trim();
    if let Some(json) = line.strip_prefix("AIW_PROGRESS ") {
        let v: Value = serde_json::from_str(json).ok()?;
        return Some(Progress {
            step: v["step"].as_u64()?,
            total: v["total"].as_u64().filter(|t| *t > 0),
            loss: v["loss"].as_f64(),
            val_loss: v["val_loss"].as_f64(),
        });
    }
    let rest = line.strip_prefix("Iter ")?;
    let (n, rest) = rest.split_once(':')?;
    let step: u64 = n.trim().parse().ok()?;
    let rest = rest.trim_start();
    let number = |after: &str| -> Option<f64> {
        let v = rest.strip_prefix(after)?;
        v.split(|c: char| c == ',' || c.is_whitespace()).next()?.parse().ok()
    };
    if rest.starts_with("Train loss ") {
        Some(Progress { step, loss: number("Train loss "), ..Default::default() })
    } else if rest.starts_with("Val loss ") {
        Some(Progress { step, val_loss: number("Val loss "), ..Default::default() })
    } else {
        None
    }
}

fn check_params(p: &TrainParams) -> Result<()> {
    let bad = |what: &str| Err(Error::Invalid(what.to_string()));
    if p.epochs.is_some_and(|e| !e.is_finite() || e <= 0.0 || e > 100.0) {
        return bad("epochs deve essere un numero tra 0 (escluso) e 100");
    }
    if p.iters.is_some_and(|i| i == 0 || i > 1_000_000) {
        return bad("iters deve essere tra 1 e 1.000.000");
    }
    if p.learning_rate.is_some_and(|l| !l.is_finite() || l <= 0.0 || l >= 1.0) {
        return bad("learning_rate deve essere un numero tra 0 (escluso) e 1 (escluso)");
    }
    if p.rank.is_some_and(|r| r == 0 || r > 256) {
        return bad("rank deve essere tra 1 e 256");
    }
    if p.batch_size.is_some_and(|b| b == 0 || b > 1024) {
        return bad("batch_size deve essere tra 1 e 1024");
    }
    Ok(())
}

/// Steps mlx-lm runs: `iters` when given, else `epochs` over the training examples, else 3 epochs.
fn mlx_iters(p: &TrainParams, train_examples: usize) -> u32 {
    if let Some(i) = p.iters {
        return i;
    }
    let batch = p.batch_size.unwrap_or(4).max(1) as f64;
    let epochs = p.epochs.unwrap_or(3.0) as f64;
    ((epochs * train_examples as f64 / batch).ceil() as u32).max(1)
}

/// What a trainer needs to be launched.
struct Launch<'a> {
    job: &'a TrainJob,
    trainer: &'a TrainerManifest,
    dataset: &'a DatasetInfo,
    python: PathBuf,
    job_dir: PathBuf,
    adapter_dir: PathBuf,
    /// The base model: a local path or the repository id as given.
    model: String,
    script: PathBuf,
    /// Where the trainer asks for rewards (`grpo`).
    reward: Option<Reward>,
}

/// The reward endpoint of a GRPO job: the API of this process and the suite whose graders score the answers.
struct Reward {
    url: String,
    token: String,
    suite: String,
}

/// The process that runs `job`.
fn training_command(l: &Launch<'_>) -> Result<JobSpec> {
    let p = &l.job.params;
    let log_path = PathBuf::from(&l.job.log_path);
    let mut spec = JobSpec { program: l.python.clone(), args: vec![], env: vec![], cwd: l.job_dir.clone(), log_path };
    match l.trainer.id.as_str() {
        "mlx-lm-lora" => {
            if l.job.method != "sft" {
                return Err(Error::Unsupported(format!("mlx-lm non sa fare {}", l.job.method)));
            }
            // The rank goes through a config file (mlx-lm has no flag for it).
            let rank = p.rank.unwrap_or(8);
            let yaml = l.job_dir.join("lora.yaml");
            std::fs::write(&yaml, format!("lora_parameters:\n  rank: {rank}\n  scale: 20.0\n  dropout: 0.0\n"))?;
            let iters = mlx_iters(p, l.dataset.train);
            spec.args = vec![
                "-m".into(),
                "mlx_lm.lora".into(),
                "--train".into(),
                "--model".into(),
                l.model.clone(),
                "--data".into(),
                l.dataset.path.clone(),
                "--adapter-path".into(),
                l.adapter_dir.display().to_string(),
                "--iters".into(),
                iters.to_string(),
                "--batch-size".into(),
                p.batch_size.unwrap_or(4).to_string(),
                "-c".into(),
                yaml.display().to_string(),
            ];
            if let Some(lr) = p.learning_rate {
                spec.args.extend(["--learning-rate".into(), lr.to_string()]);
            }
        }
        "trl" | "unsloth" => {
            let mut cfg = json!({
                "method": l.job.method,
                "backend": l.trainer.id,
                "base_model": l.model,
                "dataset_dir": l.dataset.path,
                "output_dir": l.adapter_dir,
                "work_dir": l.job_dir.join("work"),
                "metrics_path": l.adapter_dir.join("metrics.json"),
            });
            for (k, v) in [
                ("epochs", p.epochs.map(|v| json!(v))),
                ("iters", p.iters.map(|v| json!(v))),
                ("learning_rate", p.learning_rate.map(|v| json!(v))),
                ("rank", p.rank.map(|v| json!(v))),
                ("batch_size", p.batch_size.map(|v| json!(v))),
            ] {
                if let Some(v) = v {
                    cfg[k] = v;
                }
            }
            if let Some(r) = &l.reward {
                // Only the configuration file (readable by the user alone) holds the token; the log never does.
                cfg["reward_url"] = json!(r.url);
                cfg["reward_token"] = json!(r.token);
                cfg["reward_suite"] = json!(r.suite);
            }
            write_private(&l.job_dir.join("config.json"), &serde_json::to_string_pretty(&cfg)?)?;
            spec.args = vec![l.script.display().to_string(), "--config".into(), l.job_dir.join("config.json").display().to_string()];
        }
        other => return Err(Error::Unsupported(format!("il trainer «{other}» non ha un comando di lancio"))),
    }
    Ok(spec)
}

/// A file only the user can read (the configuration of a job can hold the token of a reward endpoint).
pub(crate) fn write_private(path: &Path, text: &str) -> Result<()> {
    std::fs::write(path, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

impl Engine {
    // ------------------------------------------------------------------ jobs
    /// The most recent jobs first.
    pub fn train_jobs(&self) -> Result<Vec<TrainJob>> {
        self.0.store.train_jobs(MAX_JOBS)
    }

    pub fn train_job(&self, id: &str) -> Result<TrainJob> {
        self.0.store.train_job(id)?.ok_or_else(|| Error::NotFound(format!("addestramento {id} non trovato")))
    }

    fn save_train_job(&self, j: &TrainJob) {
        if let Err(e) = self.0.store.save_train_job(&j.id, &j.method, &j.trainer, &j.status, &j.created_at, j) {
            tracing::warn!("addestramento {}: {e}", j.id);
        }
    }

    /// Jobs still `queued` or `running` when the engine starts were cut short by a closed app.
    pub(crate) fn fail_interrupted_train_jobs(&self) -> Result<()> {
        for mut j in
            self.0.store.train_jobs::<TrainJob>(MAX_JOBS)?.into_iter().filter(|j| matches!(j.status.as_str(), "queued" | "running"))
        {
            j.status = "failed".into();
            j.error = Some("interrotto: l'applicazione è stata chiusa durante l'addestramento".into());
            j.finished_at = Some(now_rfc3339());
            self.save_train_job(&j);
        }
        Ok(())
    }

    /// The last `lines` lines of the log of a job.
    pub fn train_job_log(&self, id: &str, lines: usize) -> Result<Vec<String>> {
        let j = self.train_job(id)?;
        Ok(job::tail(Path::new(&j.log_path), lines.clamp(1, 5000)))
    }

    /// Picks the trainer for `method`: the one asked for, or the first that is ready and can do it here.
    async fn pick_trainer(&self, method: &str, wanted: Option<&str>) -> Result<TrainerManifest> {
        let infos = self.trainers().await;
        let manifests = self.0.runtimes.trainers();
        let judge = |m: &TrainerManifest| -> std::result::Result<(), String> {
            if !m.methods.iter().any(|x| x == method) {
                return Err(format!("{} non sa fare {method}", m.name));
            }
            let info = infos.iter().find(|i| i.id == m.id).expect("assessed");
            match info.state.as_str() {
                "unsupported" => return Err(format!("{} non si può usare qui: {}", m.name, info.reason.clone().unwrap_or_default())),
                "missing" => return Err(format!("{} non è installato: `aiw train install {}`", m.name, m.id)),
                _ => {}
            }
            if let Some(why) = info.methods.iter().find(|f| f.method == method).and_then(|f| f.blocked.clone()) {
                return Err(format!("{method} con {} non è possibile qui: {why}", m.name));
            }
            Ok(())
        };
        if let Some(id) = wanted {
            let m = self.0.runtimes.trainer(id)?;
            return judge(&m).map(|_| m).map_err(Error::Unsupported);
        }
        let mut reasons = vec![];
        for m in &manifests {
            match judge(m) {
                Ok(()) => return Ok(m.clone()),
                Err(why) => reasons.push(why),
            }
        }
        Err(Error::Unsupported(format!(
            "nessun trainer è pronto per {method} su questo dispositivo ({}). Prova `aiw train doctor`.",
            reasons.join("; ")
        )))
    }

    /// Starts a fine-tuning in the background and returns the job as it is when it is queued. The
    /// trainer must be installed, the dataset must exist for the method, and no other job may be running.
    pub async fn start_train_job(&self, req: TrainJobRequest) -> Result<TrainJob> {
        if !matches!(req.method.as_str(), "sft" | "dpo" | "grpo") {
            return Err(Error::Invalid(format!("metodo «{}» sconosciuto: usa sft, dpo o grpo", req.method)));
        }
        if req.base_model.trim().is_empty() {
            return Err(Error::Invalid("indica il modello base".into()));
        }
        check_params(&req.params)?;
        let dataset = self.dataset(&req.dataset_id)?;
        if dataset.method != req.method {
            return Err(Error::Invalid(format!(
                "il dataset «{}» è per {}, non per {}",
                dataset.name,
                dataset.method.to_uppercase(),
                req.method.to_uppercase()
            )));
        }
        // What the request names is checked before what this machine has installed, so a wrong
        // suite or profile is reported the same way everywhere.
        if req.eval_suite.is_some() {
            let suite = req.eval_suite.as_deref().unwrap_or_default();
            self.eval_suite(suite).map_err(|_| Error::Invalid(format!("la suite di valutazione `{suite}` non esiste")))?;
            let profile = req.eval_profile.as_deref().ok_or_else(|| {
                Error::Invalid("per confrontare l'adapter serve anche il profilo di inferenza da cui partire (eval_profile)".into())
            })?;
            self.profile(profile).map_err(|_| Error::Invalid(format!("il profilo di inferenza `{profile}` non esiste")))?;
        }
        let trainer = self.pick_trainer(&req.method, req.trainer.as_deref()).await?;
        self.check_model_fits(&trainer, &req)?;
        let reward_suite = self.resolve_reward_suite(&req, &dataset)?;
        let id = new_id("trn");
        let job_dir = self.0.dirs.train_jobs().join(&id);
        std::fs::create_dir_all(&job_dir)?;
        let job = TrainJob {
            id: id.clone(),
            method: req.method.clone(),
            trainer: trainer.id.clone(),
            base_model: req.base_model.trim().to_string(),
            dataset_id: dataset.id.clone(),
            params: req.params.clone(),
            status: "queued".into(),
            created_at: now_rfc3339(),
            started_at: None,
            finished_at: None,
            log_path: job_dir.join("train.log").display().to_string(),
            progress: None,
            adapter_id: None,
            metrics: None,
            error: None,
            name: req.name.clone().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()),
            eval_suite: req.eval_suite.clone(),
            eval_profile: req.eval_profile.clone(),
            reward_suite,
        };
        // One job at a time on a device: the check and the registration happen under one lock.
        let token = CancellationToken::new();
        {
            let mut running = self.0.train.running.lock().unwrap();
            if let Some(other) = running.keys().next() {
                let _ = std::fs::remove_dir_all(&job_dir);
                return Err(Error::Conflict(format!("c'è già un addestramento in corso ({other}): aspetta che finisca o annullalo")));
            }
            running.insert(id.clone(), token.clone());
        }
        self.save_train_job(&job);
        self.audit_user("train.start", &format!("{} {} su {}", req.method, trainer.name, job.base_model));
        let engine = self.clone();
        tokio::spawn(async move {
            engine.run_train_job(id.clone(), trainer, dataset, token).await;
            engine.0.train.running.lock().unwrap().remove(&id);
        });
        Ok(job)
    }

    /// A model the device cannot train (too big for the memory, or a GGUF file) is refused up front.
    fn check_model_fits(&self, trainer: &TrainerManifest, req: &TrainJobRequest) -> Result<()> {
        let name = req.base_model.trim();
        let local = self.0.models.read().unwrap().iter().find(|m| m.id == name || m.name == name).cloned();
        let Some(m) = local else { return Ok(()) };
        if m.format == "gguf" {
            return Err(Error::Invalid(format!(
                "«{}» è un file GGUF e non si addestra: usa il modello in formato safetensors/MLX oppure un repository Hugging Face",
                m.name
            )));
        }
        let facts = DeviceFacts::from_profile(&self.device());
        if let (Some(size), Some(max)) = (m.params_b, trainer::max_params_b(trainer, &facts, &req.method)) {
            if size > max * 1.1 {
                return Err(Error::Unsupported(format!(
                    "«{}» ha circa {size}B parametri: con {} su questo dispositivo {} arriva a ~{max}B",
                    m.name,
                    trainer.name,
                    req.method.to_uppercase()
                )));
            }
        }
        Ok(())
    }

    /// Stops a job. It turns `cancelled` as soon as the process is gone.
    pub fn cancel_train_job(&self, id: &str) -> Result<TrainJob> {
        let job = self.train_job(id)?;
        let token = self.0.train.running.lock().unwrap().get(id).cloned();
        match token {
            Some(t) => {
                t.cancel();
                self.audit_user("train.cancel", &format!("{} ({})", job.method, job.base_model));
                Ok(job)
            }
            None => Err(Error::Conflict(format!("l'addestramento {id} non è in corso ({})", job.status))),
        }
    }

    fn mutate_job(&self, id: &str, f: impl FnOnce(&mut TrainJob)) -> Option<TrainJob> {
        let mut j = self.0.store.train_job::<TrainJob>(id).ok().flatten()?;
        f(&mut j);
        self.save_train_job(&j);
        Some(j)
    }

    fn fail_job(&self, id: &str, why: String) {
        self.mutate_job(id, |j| {
            j.status = "failed".into();
            j.error = Some(why);
            j.finished_at = Some(now_rfc3339());
        });
    }

    /// Writes the TRL script into `~/.aiw/scripts/train/` (when it changed) and returns its path.
    fn ensure_train_script(&self) -> Result<PathBuf> {
        let dir = self.0.dirs.scripts().join("train");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("trl_train.py");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(TRL_SCRIPT) {
            std::fs::write(&path, TRL_SCRIPT)?;
        }
        Ok(path)
    }

    async fn run_train_job(&self, id: String, trainer: TrainerManifest, dataset: DatasetInfo, cancel: CancellationToken) {
        let Ok(job) = self.train_job(&id) else { return };
        let started = std::time::Instant::now();
        self.mutate_job(&id, |j| {
            j.status = "running".into();
            j.started_at = Some(now_rfc3339());
        });
        let Some(python) = self.0.runtimes.trainer_python(&trainer.id).await else {
            return self.fail_job(
                &id,
                format!("{} non è installato nell'ambiente Python di AIW: `aiw train install {}`", trainer.name, trainer.id),
            );
        };
        let job_dir = self.0.dirs.train_jobs().join(&id);
        let adapter_id = new_id("ad");
        let adapter_dir = self.0.dirs.adapters().join(&adapter_id);
        let prepared = (|| -> Result<JobSpec> {
            std::fs::create_dir_all(&adapter_dir)?;
            let script = self.ensure_train_script()?;
            let model = (self.0.resolver)(&job.base_model).map(|p| p.display().to_string()).unwrap_or_else(|| job.base_model.clone());
            let spec = training_command(&Launch {
                job: &job,
                trainer: &trainer,
                dataset: &dataset,
                python,
                job_dir: job_dir.clone(),
                adapter_dir: adapter_dir.clone(),
                model,
                script,
                reward: self.reward_for(&job)?,
            })?;
            Ok(spec)
        })();
        let spec = match prepared {
            Ok(s) => s,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&adapter_dir);
                return self.fail_job(&id, e.to_string());
            }
        };
        let total_iters = if trainer.id == "mlx-lm-lora" { Some(mlx_iters(&job.params, dataset.train) as u64) } else { None };
        let seen = Arc::new(Mutex::new(Progress::default()));
        let (engine, job_id, seen2) = (self.clone(), id.clone(), seen.clone());
        let last_saved = Mutex::new(-1.0f32);
        let outcome = job::run_logged(spec, cancel, move |line| {
            let Some(p) = parse_progress(line) else { return };
            let mut s = seen2.lock().unwrap();
            s.step = p.step;
            s.total = p.total.or(total_iters).or(s.total);
            s.loss = p.loss.or(s.loss);
            s.val_loss = p.val_loss.or(s.val_loss);
            let fraction = s.total.map(|t| (s.step as f32 / t as f32).clamp(0.0, 1.0));
            // Stored only when it moved by a percent: the log can print a line per step.
            if let Some(f) = fraction {
                let mut last = last_saved.lock().unwrap();
                if (f - *last).abs() >= 0.01 || f >= 1.0 {
                    *last = f;
                    engine.mutate_job(&job_id, |j| j.progress = Some(f));
                }
            }
        })
        .await;
        let progress = seen.lock().unwrap().clone();
        match outcome {
            Err(e) => {
                let _ = std::fs::remove_dir_all(&adapter_dir);
                self.fail_job(&id, e.to_string());
            }
            Ok(JobEnd::Cancelled) => {
                let _ = std::fs::remove_dir_all(&adapter_dir);
                self.mutate_job(&id, |j| {
                    j.status = "cancelled".into();
                    j.finished_at = Some(now_rfc3339());
                });
            }
            Ok(JobEnd::Exited(code)) if code != Some(0) => {
                let _ = std::fs::remove_dir_all(&adapter_dir);
                let tail = job::tail(Path::new(&job.log_path), 5).join("\n");
                let how = code.map_or("terminato da un segnale".to_string(), |c| format!("uscito con codice {c}"));
                self.fail_job(&id, format!("{} è {how}. Ultime righe del log:\n{tail}", trainer.name));
            }
            Ok(JobEnd::Exited(_)) => {
                if let Err(why) = aiw_runtimes::adapters::check_files(&adapter_dir, &trainer.adapter_format) {
                    let _ = std::fs::remove_dir_all(&adapter_dir);
                    return self.fail_job(&id, format!("{} è terminato senza un adapter completo: {why}", trainer.name));
                }
                let mut metrics = json!({"steps": progress.step, "duration_s": started.elapsed().as_secs()});
                if let Some(l) = progress.loss {
                    metrics["train_loss"] = json!(l);
                }
                if let Some(l) = progress.val_loss {
                    metrics["val_loss"] = json!(l);
                }
                // The scripts write their own, which are more exact than what the log shows.
                if let Some(Value::Object(from_file)) =
                    std::fs::read_to_string(adapter_dir.join("metrics.json")).ok().and_then(|t| serde_json::from_str(&t).ok())
                {
                    for (k, v) in from_file {
                        metrics[k] = v;
                    }
                }
                let adapter = AdapterInfo {
                    id: adapter_id.clone(),
                    name: job.name.clone().unwrap_or_else(|| format!("{} · {}", job.method.to_uppercase(), short_model(&job.base_model))),
                    base_model: job.base_model.clone(),
                    method: job.method.clone(),
                    trainer: trainer.id.clone(),
                    dataset_id: job.dataset_id.clone(),
                    created_at: now_rfc3339(),
                    path: adapter_dir.display().to_string(),
                    metrics: Some(metrics.clone()),
                    format: trainer.adapter_format.clone(),
                    job_id: Some(id.clone()),
                };
                if let Err(e) = self.0.store.put_doc("adapter", &adapter.id, &adapter) {
                    return self.fail_job(&id, e.to_string());
                }
                self.mutate_job(&id, |j| {
                    j.adapter_id = Some(adapter_id.clone());
                    j.metrics = Some(metrics.clone());
                    j.progress = Some(1.0);
                });
                // The adapter is already registered: the comparison can fail without taking it away.
                if let (Some(suite), Some(profile)) = (job.eval_suite.clone(), job.eval_profile.clone()) {
                    let eval = match self.compare_adapter(&suite, &profile, &adapter).await {
                        Ok(v) => v,
                        Err(e) => json!({"suite": suite, "profile": profile, "error": e.to_string()}),
                    };
                    metrics["eval"] = eval;
                    let mut stored = adapter.clone();
                    stored.metrics = Some(metrics.clone());
                    let _ = self.0.store.put_doc("adapter", &stored.id, &stored);
                }
                self.mutate_job(&id, |j| {
                    j.status = "succeeded".into();
                    j.metrics = Some(metrics);
                    j.finished_at = Some(now_rfc3339());
                });
            }
        }
    }

    /// The suite that rewards a GRPO job: the one asked for, or the one the dataset was made from. It
    /// must exist and have only graders that look at the answer. `None` for the other methods.
    fn resolve_reward_suite(&self, req: &TrainJobRequest, dataset: &DatasetInfo) -> Result<Option<String>> {
        if req.method != "grpo" {
            return Ok(None);
        }
        let from_dataset = dataset.sources.iter().find_map(|s| s.strip_prefix("suite:")).map(str::to_string);
        let id = req
            .reward_suite
            .clone()
            .or(from_dataset)
            .ok_or_else(|| Error::Invalid("GRPO richiede la suite che dà le reward (reward_suite)".into()))?;
        self.eval_suite(&id).map_err(|_| Error::Invalid(format!("la suite di reward `{id}` non esiste")))?;
        self.reward_suite(&id)?;
        Ok(Some(id))
    }

    /// The endpoint a GRPO trainer calls. The server of this process must be listening.
    fn reward_for(&self, job: &TrainJob) -> Result<Option<Reward>> {
        let Some(suite) = job.reward_suite.clone().filter(|_| job.method == "grpo") else { return Ok(None) };
        let url = self.local_url().ok_or_else(|| {
            Error::Unsupported("la reward passa dall'API di AIW, ma il server non è in ascolto: avvia l'app o `aiw serve`".into())
        })?;
        Ok(Some(Reward { url, token: self.token().to_string(), suite }))
    }

    /// Runs `suite` with the profile as it is and with the adapter added, and sums up both.
    async fn compare_adapter(&self, suite_id: &str, profile_id: &str, adapter: &AdapterInfo) -> Result<Value> {
        use crate::eval::EvalRequest;
        let suite = self.eval_suite(suite_id)?;
        let base_profile = self.profile(profile_id)?;
        let summary = |r: &aiw_eval::EvalReport| json!({"report": r.id, "passed": r.summary.passed, "total": r.summary.total, "pass_rate": r.summary.pass_rate, "score": r.summary.score});
        let baseline =
            self.run_eval(&suite, &EvalRequest { profile: Some(profile_id.to_string()), ..Default::default() }, &mut |_| {}).await?;
        // A temporary profile: the same model, with the adapter.
        let temp_id = format!("train-{}", adapter.id);
        let mut temp = base_profile.clone();
        temp.id = temp_id.clone();
        temp.name = format!("{} + {}", base_profile.name, adapter.name);
        temp.params.adapters = vec![AdapterRef { path: adapter.path.clone(), scale: None }];
        self.save_profile(temp)?;
        let with = self.run_eval(&suite, &EvalRequest { profile: Some(temp_id.clone()), ..Default::default() }, &mut |_| {}).await;
        let _ = self.0.store.delete_doc("profile", &temp_id);
        let with = with?;
        Ok(json!({
            "suite": suite_id,
            "profile": profile_id,
            "baseline": summary(&baseline),
            "adapter": summary(&with),
            "delta_pass_rate": with.summary.pass_rate - baseline.summary.pass_rate,
            "delta_score": with.summary.score - baseline.summary.score,
        }))
    }

    // ------------------------------------------------------------------ adapters
    /// The adapters AIW trained, newest first.
    pub fn adapters(&self) -> Result<Vec<AdapterInfo>> {
        let mut v: Vec<AdapterInfo> = self.0.store.list_docs("adapter")?;
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        Ok(v)
    }

    pub fn adapter(&self, id: &str) -> Result<AdapterInfo> {
        self.0.store.get_doc("adapter", id)?.ok_or_else(|| Error::NotFound(format!("adapter {id} non trovato")))
    }

    /// Removes an adapter and its files. A profile that still uses it keeps it from going.
    pub fn delete_adapter(&self, id: &str) -> Result<()> {
        let a = self.adapter(id)?;
        if let Some(p) = self.profiles()?.into_iter().find(|p| p.params.adapters.iter().any(|x| x.path == a.path)) {
            return Err(Error::Conflict(format!("l'adapter è usato dal profilo «{}»: toglilo prima dal profilo", p.name)));
        }
        let root = self.0.dirs.adapters();
        let path = PathBuf::from(&a.path);
        // Only what AIW itself wrote: never a path outside the adapters folder.
        if path.starts_with(&root) && path != root {
            let _ = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        }
        self.0.store.delete_doc("adapter", id)?;
        self.audit_user("adapter.delete", &a.name);
        Ok(())
    }
}

/// The last part of a model id or path, for names.
fn short_model(m: &str) -> String {
    m.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(m).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_read_from_mlx_lines_and_from_the_aiw_scripts() {
        let p = parse_progress("Iter 10: Train loss 2.345, Learning Rate 1.000e-05, It/sec 0.512, Tokens/sec 120.1").unwrap();
        assert_eq!((p.step, p.loss, p.val_loss), (10, Some(2.345), None));
        let p = parse_progress("Iter 200: Val loss 1.9, Val took 3.2s").unwrap();
        assert_eq!((p.step, p.loss, p.val_loss), (200, None, Some(1.9)));
        let p = parse_progress(r#"AIW_PROGRESS {"step": 5, "total": 20, "loss": 0.5}"#).unwrap();
        assert_eq!((p.step, p.total, p.loss), (5, Some(20), Some(0.5)));
        for other in ["", "Loading model", "Iter x: Train loss 1", "Iter 3: Saved adapter weights", "AIW_PROGRESS nope"] {
            assert!(parse_progress(other).is_none(), "{other:?}");
        }
    }

    #[test]
    fn mlx_steps_follow_iters_or_the_epochs() {
        let p = |iters, epochs, batch_size| TrainParams { iters, epochs, batch_size, ..Default::default() };
        assert_eq!(mlx_iters(&p(Some(50), Some(9.0), None), 100), 50);
        assert_eq!(mlx_iters(&p(None, Some(2.0), Some(4)), 100), 50);
        assert_eq!(mlx_iters(&p(None, None, None), 18), 14);
        assert_eq!(mlx_iters(&p(None, Some(0.01), Some(64)), 5), 1);
    }

    #[test]
    fn the_parameters_are_checked() {
        assert!(check_params(&TrainParams::default()).is_ok());
        for bad in [
            TrainParams { epochs: Some(0.0), ..Default::default() },
            TrainParams { iters: Some(0), ..Default::default() },
            TrainParams { learning_rate: Some(2.0), ..Default::default() },
            TrainParams { rank: Some(0), ..Default::default() },
            TrainParams { batch_size: Some(0), ..Default::default() },
        ] {
            assert!(check_params(&bad).is_err(), "{bad:?}");
        }
    }
}
