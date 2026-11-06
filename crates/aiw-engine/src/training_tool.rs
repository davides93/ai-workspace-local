use crate::{Engine, Inner};
use aiw_core::api::{DatasetSpec, TrainJobRequest};
use aiw_core::{Error, Result};
use aiw_tools::{Risk, Tool, ToolContext, ToolOutput};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, Weak};
use std::time::Duration;

const MAX_ATTEMPTS: usize = 3;
const MAX_JOB_TIME: Duration = Duration::from_secs(60 * 60);

pub(crate) fn register(engine: &Engine) {
    engine.0.tools.register(Arc::new(TrainingTool(Arc::downgrade(&engine.0))));
}

struct TrainingTool(Weak<Inner>);

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Action {
    Inspect,
    Prepare { agent: String },
    Start { request: TrainJobRequest },
    Status { job_id: String, wait_seconds: Option<u64> },
    Cancel { job_id: String },
}

#[async_trait::async_trait]
impl Tool for TrainingTool {
    fn name(&self) -> &str {
        "training_harness"
    }

    fn description(&self) -> &str {
        "Addestra un adapter LoRA. Ispeziona capacità e dataset, prepara dati SFT approvati, avvia un job con confronto su una suite separata, controlla lo stato o annulla. Il profilo candidato è una proposta da sottoporre all’utente."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {"enum": ["inspect", "prepare", "start", "status", "cancel"]},
                "agent": {"type": "string", "description": "ID dell’agente di cui usare le conversazioni approvate (prepare)"},
                "job_id": {"type": "string", "description": "ID del job (status o cancel)"},
                "wait_seconds": {"type": "integer", "maximum": 15, "description": "Attendi fino a 15 secondi prima di rileggere lo stato."},
                "request": {
                    "type": "object",
                    "required": ["method", "base_model", "dataset_id", "eval_suite", "eval_profile"],
                    "properties": {
                        "method": {"enum": ["sft"]},
                        "base_model": {"type": "string", "description": "ID di un modello locale"},
                        "dataset_id": {"type": "string"},
                        "eval_suite": {"type": "string", "description": "Suite separata dai dati di training"},
                        "eval_profile": {"type": "string", "description": "Profilo con lo stesso modello base e senza adapter"},
                        "trainer": {"type": "string"},
                        "name": {"type": "string"},
                        "params": {"type": "object", "properties": {
                            "iters": {"type": "integer"}, "epochs": {"type": "number"},
                            "learning_rate": {"type": "number"}, "rank": {"type": "integer"},
                            "batch_size": {"type": "integer"}
                        }}
                    }
                }
            }
        })
    }

    fn risk(&self) -> Risk {
        Risk::Exec
    }

    fn explicit_grant(&self) -> bool {
        true
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let engine = Engine(self.0.upgrade().ok_or_else(|| Error::Conflict("motore in arresto".into()))?);
        let action: Action = serde_json::from_value(args).map_err(|e| Error::Invalid(format!("richiesta di training non valida: {e}")))?;
        let result = match action {
            Action::Inspect => {
                let suites = engine
                    .eval_suites()?
                    .into_iter()
                    .map(|s| json!({"id": s.id, "name": s.name, "tasks": s.tasks.len()}))
                    .collect::<Vec<_>>();
                let profiles = engine
                    .profiles()?
                    .into_iter()
                    .filter(|p| p.params.adapters.is_empty())
                    .map(|p| json!({"id": p.id, "model": p.model, "runtime_id": p.runtime_id}))
                    .collect::<Vec<_>>();
                json!({"suites": suites, "profiles": profiles, "device": engine.train_doctor().await, "datasets": engine.datasets()?, "models": engine.models().into_iter().filter(|m| matches!(m.format.as_str(), "mlx" | "safetensors")).collect::<Vec<_>>(), "tentativi_massimi": MAX_ATTEMPTS})
            }
            Action::Prepare { agent } => {
                engine.agent(&agent)?;
                json!(engine.build_dataset(&DatasetSpec {
                    method: "sft".into(),
                    agent: Some(agent),
                    sources: vec!["feedback".into()],
                    limit: Some(500),
                    ..Default::default()
                })?)
            }
            Action::Start { request } => {
                let mut jobs = ctx.training_jobs.lock().await;
                if jobs.len() >= MAX_ATTEMPTS {
                    return Err(Error::Conflict("Budget esaurito: massimo tre addestramenti per run.".into()));
                }
                validate(&engine, &request)?;
                let job = engine.start_training(request, ctx.cancel.child_token(), Some(MAX_JOB_TIME)).await?;
                jobs.push(job.id.clone());
                json!(job)
            }
            Action::Status { job_id, wait_seconds } => {
                let mut job = engine.train_job(&job_id)?;
                if matches!(job.status.as_str(), "queued" | "running") {
                    let wait = Duration::from_secs(wait_seconds.unwrap_or_default().min(15));
                    if !wait.is_zero() {
                        tokio::select! {
                            _ = tokio::time::sleep(wait) => {},
                            _ = ctx.cancel.cancelled() => return Err(Error::Cancelled),
                        }
                        job = engine.train_job(&job_id)?;
                    }
                }
                let adapter = job.adapter_id.as_deref().map(|id| engine.adapter(id)).transpose()?;
                let candidate = adapter
                    .filter(|a| {
                        a.format == "mlx"
                            && job.status == "succeeded"
                            && job.metrics.as_ref().is_some_and(|m| m["eval"]["delta_pass_rate"].is_number())
                    })
                    .map(|a| {
                        json!({
                            "model": a.base_model, "runtime_id": "mlx-lm", "params": {"adapters": [{"path": a.path}]}
                        })
                    });
                json!({"job": job, "profilo_candidato": candidate})
            }
            Action::Cancel { job_id } => {
                own_job(ctx, &job_id).await?;
                json!(engine.cancel_train_job(&job_id)?)
            }
        };
        Ok(ToolOutput::text(result.to_string()))
    }
}

async fn own_job(ctx: &ToolContext, id: &str) -> Result<()> {
    if !ctx.training_jobs.lock().await.iter().any(|job| job == id) {
        return Err(Error::Invalid("Questo job non è stato avviato dal run corrente.".into()));
    }
    Ok(())
}

fn validate(engine: &Engine, request: &TrainJobRequest) -> Result<()> {
    let dataset = engine.dataset(&request.dataset_id)?;
    if request.method != "sft" || dataset.sources.len() != 1 || !matches!(dataset.sources[0].as_str(), "feedback" | "import") {
        return Err(Error::Invalid(
            "Il harness usa dataset SFT da feedback approvati o importati dall’utente. Gli altri metodi sono disponibili nella pagina Addestramento.".into(),
        ));
    }
    if !engine.models().iter().any(|m| m.id == request.base_model && matches!(m.format.as_str(), "mlx" | "safetensors")) {
        return Err(Error::Invalid("Scegli l’ID di un modello locale MLX o safetensors.".into()));
    }
    let profile = request.eval_profile.as_deref().ok_or_else(|| Error::Invalid("Indica il profilo di confronto.".into()))?;
    let profile = engine.profile(profile)?;
    if profile.model != request.base_model || !profile.params.adapters.is_empty() {
        return Err(Error::Invalid("Il profilo di confronto deve usare il modello base senza adapter.".into()));
    }
    let suite_id = request
        .eval_suite
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| Error::Invalid("Indica una suite di valutazione separata dal dataset.".into()))?;
    let suite = engine.eval_suite(suite_id)?;
    if suite.tasks.len() > 20 || suite.tasks.iter().flat_map(|task| &task.graders).any(|grader| grader.kind() == "command") {
        return Err(Error::Invalid("Per il harness scegli una suite con al massimo 20 task e senza grader che eseguono comandi.".into()));
    }
    let prompts: std::collections::HashSet<&str> = suite.tasks.iter().map(|task| task.prompt.trim()).collect();
    let training = std::fs::read_to_string(std::path::Path::new(&dataset.path).join("train.jsonl"))?;
    for line in training.lines() {
        let example: Value = serde_json::from_str(line)?;
        if example["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|message| message["role"] == "user" && message["content"].as_str().is_some_and(|prompt| prompts.contains(prompt.trim())))
        {
            return Err(Error::Invalid("La suite contiene un prompt già usato nel training. Scegli esempi separati.".into()));
        }
    }
    Ok(())
}
