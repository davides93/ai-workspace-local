//! Training on the engine: trainers and the doctor, datasets, jobs with a fake trainer, adapters.

use super::tests::engine;
use super::*;
use aiw_core::api::{Accelerator, AdapterRef, CpuInfo};

fn device(os: &str, arch: &str, tier: &str, mem_gb: u64, accel: Vec<(&str, Option<u64>)>) -> DeviceProfile {
    DeviceProfile {
        id: "dev_test".into(),
        name: "test".into(),
        os: os.into(),
        os_version: String::new(),
        arch: arch.into(),
        form_factor: "desktop".into(),
        cpu: CpuInfo { brand: "cpu".into(), cores: 8, threads: 16, features: vec![] },
        memory_mb: mem_gb * 1024,
        available_memory_mb: mem_gb * 1024 / 2,
        accelerators: accel
            .into_iter()
            .map(|(kind, gb)| Accelerator {
                kind: kind.into(),
                name: kind.into(),
                vendor: None,
                memory_mb: gb.map(|g| g * 1024),
                driver: None,
                details: None,
            })
            .collect(),
        tier: tier.into(),
    }
}

/// A `python` that "imports" only the listed modules, in the environment AIW manages.
#[cfg(unix)]
pub(crate) fn fake_pyenv(e: &Engine, has: &[&str]) {
    use std::os::unix::fs::PermissionsExt;
    let bin = e.0.dirs.root.join("pyenv").join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = format!(
        "#!/bin/sh\nfor m in $(echo \"$2\" | tr ';' ' ' | sed 's/import //g'); do\n  case \" {} \" in *\" $m \"*) ;; *) exit 1;; esac\ndone\nexit 0\n",
        has.join(" ")
    );
    let py = bin.join("python");
    std::fs::write(&py, script).unwrap();
    std::fs::set_permissions(&py, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_mac_can_train_with_mlx_once_it_is_installed() {
    let (e, _d, _w) = engine(vec![]).await;
    e.set_device(device("macos", "aarch64", "m", 24, vec![("metal", None), ("cpu", None)]));
    let doc = e.train_doctor().await;
    let mlx = doc.trainers.iter().find(|t| t.id == "mlx-lm-lora").unwrap();
    assert_eq!(mlx.state, "missing");
    assert_eq!(doc.can_train, vec!["LoRA SFT fino a ~4B su questo Mac (MLX LM (LoRA), da installare)"]);
    assert!(doc.advice.iter().any(|a| a.contains("aiw train install mlx-lm-lora")), "{:?}", doc.advice);
    let trl = doc.trainers.iter().find(|t| t.id == "trl").unwrap();
    assert_eq!(trl.state, "unsupported");
    assert!(trl.reason.as_deref().unwrap().contains("disponibile su"));

    fake_pyenv(&e, &["mlx_lm"]);
    let mlx = e.trainers().await.into_iter().find(|t| t.id == "mlx-lm-lora").unwrap();
    assert_eq!(mlx.state, "ready");
    assert!(e.train_doctor().await.can_train[0].ends_with("(MLX LM (LoRA))"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_cuda_workstation_is_ready_for_trl_and_the_class_limits_grpo() {
    let (e, _d, _w) = engine(vec![]).await;
    e.set_device(device("linux", "x86_64", "m", 64, vec![("cuda", Some(24)), ("cpu", None)]));
    fake_pyenv(&e, &["trl", "peft", "transformers", "datasets"]);
    let trl = e.trainers().await.into_iter().find(|t| t.id == "trl").unwrap();
    assert_eq!(trl.state, "ready");
    let grpo = trl.methods.iter().find(|m| m.method == "grpo").unwrap();
    assert!(grpo.blocked.as_deref().unwrap().contains("classe L"));
    // The installation of a trainer this device cannot run is refused with the reason.
    let err = e.trainer_install_start("mlx-lm-lora").await.unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)) && err.to_string().contains("disponibile su"), "{err}");
}

#[tokio::test]
async fn a_weak_device_is_told_what_a_paired_one_could_do() {
    let (e, _d, _w) = engine(vec![]).await;
    e.set_device(device("linux", "x86_64", "xs", 8, vec![("cpu", None)]));
    let doc = e.train_doctor().await;
    assert!(doc.can_train.is_empty() && doc.trainers.iter().all(|t| t.state == "unsupported"));
    assert!(doc.advice.iter().any(|a| a.contains("Accoppia")), "{:?}", doc.advice);
    e.0.store
        .put_doc(
            "peer",
            "ws",
            &PeerDevice {
                id: "ws".into(),
                name: "Workstation".into(),
                os: "linux".into(),
                address: "http://10.0.0.2:7777".into(),
                status: "online".into(),
                shared_runtimes: vec![],
                hardware: Some(PeerHardware {
                    arch: "x86_64".into(),
                    tier: "xl".into(),
                    memory_mb: 128 * 1024,
                    vram_mb: 24 * 1024,
                    accelerators: vec!["cuda".into()],
                    unified_memory: false,
                }),
            },
        )
        .unwrap();
    let doc = e.train_doctor().await;
    assert_eq!(doc.remote.len(), 1);
    assert!(
        doc.remote[0].contains("Workstation") && doc.remote[0].contains("classe XL") && doc.remote[0].contains("LoRA SFT"),
        "{:?}",
        doc.remote
    );
    assert!(!doc.advice.iter().any(|a| a.contains("Accoppia")));
}

fn profile_with(runtime: &str, adapters: Vec<AdapterRef>) -> InferenceProfile {
    InferenceProfile {
        id: "ft".into(),
        name: "Fine-tuned".into(),
        description: None,
        runtime_id: runtime.into(),
        model: "base".into(),
        params: InferenceParams { adapters, ..Default::default() },
    }
}

#[tokio::test]
async fn profiles_with_adapters_are_checked_when_they_are_saved() {
    let (e, d, _w) = engine(vec![]).await;
    let gguf = d.path().join("lora.gguf");
    std::fs::write(&gguf, b"x").unwrap();
    let adapter = |p: &std::path::Path, scale| AdapterRef { path: p.display().to_string(), scale };

    // llama-server takes a GGUF adapter, with or without a scale, and the profile keeps it.
    let saved = e.save_profile(profile_with("llama-server", vec![adapter(&gguf, Some(0.7))])).unwrap();
    assert_eq!(e.profile(&saved.id).unwrap().params.adapters, vec![adapter(&gguf, Some(0.7))]);
    // A profile without adapters stores nothing about them.
    assert!(!serde_json::to_string(&InferenceParams::default()).unwrap().contains("adapters"));

    // Ollama and the in-process engine cannot load it at launch: refused now, with what to do.
    let err = e.save_profile(profile_with("ollama", vec![adapter(&gguf, None)])).unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)) && err.to_string().contains("ADAPTER"), "{err}");
    assert!(matches!(e.save_profile(profile_with("llama-embedded", vec![adapter(&gguf, None)])), Err(Error::Unsupported(_))));
    // A path that is not there, or not the right kind of file, is refused too.
    let err = e.save_profile(profile_with("llama-server", vec![adapter(&d.path().join("none.gguf"), None)])).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)) && err.to_string().contains("non esiste"), "{err}");
    let err = e.save_profile(profile_with("mlx-lm", vec![adapter(&gguf, None)])).unwrap_err();
    assert!(err.to_string().contains("serve la cartella"), "{err}");
    // Ollama without adapters is fine.
    assert!(e.save_profile(profile_with("ollama", vec![])).is_ok());
}

// ---------------------------------------------------------------------------------------------
// Datasets and jobs

use aiw_core::api::{DatasetSpec, TrainJob, TrainJobRequest};

/// A session with one exchange, rated when `rating` is given.
fn rated_session(e: &Engine, n: usize, rating: Option<i32>, answer: &str) -> String {
    let s = e.create_session("operator", None).unwrap();
    let msg = |role: &str, content: String| ChatMessage {
        id: new_id("msg"),
        role: role.into(),
        content,
        reasoning: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        created_at: now_rfc3339(),
    };
    let reply = msg("assistant", answer.to_string());
    let reply_id = reply.id.clone();
    e.0.store.append_messages(&s.id, &[msg("user", format!("domanda numero {n}")), reply]).unwrap();
    if let Some(r) = rating {
        e.set_feedback(&s.id, &reply_id, r, None).unwrap();
    }
    s.id
}

fn sft_spec() -> DatasetSpec {
    DatasetSpec { method: "sft".into(), ..Default::default() }
}

#[tokio::test]
async fn an_sft_dataset_needs_enough_liked_sessions_and_hides_secrets() {
    let (e, _d, _w) = engine(vec![]).await;
    for n in 0..9 {
        rated_session(&e, n, Some(1), "risposta utile");
    }
    // Nine are not enough, and the message says how many there were.
    let err = e.build_dataset(&sft_spec()).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)) && err.to_string().contains("ne ho trovati 9"), "{err}");

    rated_session(&e, 9, Some(1), "la chiave è sk-abcdefghijklmnop1234567890");
    // A thumbs down and an unrated session are left out.
    rated_session(&e, 10, Some(-1), "risposta sbagliata");
    rated_session(&e, 11, None, "risposta neutra");
    let info = e.build_dataset(&DatasetSpec { name: Some("  Mio  ".into()), ..sft_spec() }).unwrap();
    assert_eq!((info.name.as_str(), info.examples, info.train + info.valid), ("Mio", 10, 10));
    assert_eq!(info.valid, 1);
    assert_eq!(info.sources, vec!["feedback"]);

    let train = std::fs::read_to_string(format!("{}/train.jsonl", info.path)).unwrap();
    let valid = std::fs::read_to_string(format!("{}/valid.jsonl", info.path)).unwrap();
    let all = format!("{train}{valid}");
    assert!(!all.contains("sk-abcdefghij") && all.contains(crate::traces::REDACTED), "secrets are masked");
    assert!(!all.contains("sbagliata") && !all.contains("neutra"));
    let first: Value = serde_json::from_str(train.lines().next().unwrap()).unwrap();
    assert_eq!(first["messages"][0]["role"], "user");

    // Listed, readable by id, removable; an unknown id or a path-like one is a 404.
    assert_eq!(e.datasets().unwrap().len(), 1);
    assert_eq!(e.dataset(&info.id).unwrap().examples, 10);
    assert!(matches!(e.dataset("../x"), Err(Error::NotFound(_))));
    e.delete_dataset(&info.id).unwrap();
    assert!(e.datasets().unwrap().is_empty());
    assert!(matches!(e.build_dataset(&DatasetSpec { method: "boh".into(), ..Default::default() }), Err(Error::Invalid(_))));
    assert!(matches!(e.build_dataset(&DatasetSpec { sources: vec!["boh".into()], ..sft_spec() }), Err(Error::Invalid(_))));
}

/// A `python` that "imports" mlx_lm and, when run with `-m mlx_lm.lora`, behaves like a (fast) training:
/// it prints the lines mlx-lm prints and writes the adapter. A model with "slow" in its name hangs.
#[cfg(unix)]
fn fake_mlx_trainer(e: &Engine) {
    use std::os::unix::fs::PermissionsExt;
    let bin = e.0.dirs.root.join("pyenv").join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = r#"#!/bin/sh
if [ "$1" = "-c" ]; then
  case "$2" in "import mlx_lm") exit 0;; *) exit 1;; esac
fi
while [ $# -gt 0 ]; do
  case "$1" in
    --adapter-path) AD="$2";;
    --model) M="$2";;
    --iters) IT="$2";;
  esac
  shift
done
case "$M" in *fail*) echo "boom: out of memory"; exit 3;; esac
case "$M" in *slow*) echo "Iter 1: Train loss 2.5, Learning Rate 1e-5"; sleep 30;; esac
echo "Iter 1: Train loss 2.500, Learning Rate 1e-5"
echo "Iter $IT: Val loss 1.250, Val took 0.1s"
echo "Iter $IT: Train loss 0.750, Learning Rate 1e-5"
mkdir -p "$AD"
echo weights > "$AD/adapters.safetensors"
echo '{}' > "$AD/adapter_config.json"
"#;
    let py = bin.join("python");
    std::fs::write(&py, script).unwrap();
    std::fs::set_permissions(&py, std::fs::Permissions::from_mode(0o755)).unwrap();
}

async fn ready_to_train(turns: Vec<Vec<aiw_runtimes::ChatDelta>>) -> (Engine, tempfile::TempDir, tempfile::TempDir, String) {
    let (e, d, w) = engine(turns).await;
    e.set_device(device("macos", "aarch64", "m", 24, vec![("metal", None), ("cpu", None)]));
    #[cfg(unix)]
    fake_mlx_trainer(&e);
    for n in 0..12 {
        rated_session(&e, n, Some(1), "risposta utile");
    }
    let ds = e.build_dataset(&sft_spec()).unwrap();
    (e, d, w, ds.id)
}

fn sft_request(dataset: &str, model: &str) -> TrainJobRequest {
    TrainJobRequest {
        method: "sft".into(),
        base_model: model.into(),
        dataset_id: dataset.into(),
        params: aiw_core::api::TrainParams { iters: Some(4), ..Default::default() },
        ..Default::default()
    }
}

async fn wait_job(e: &Engine, id: &str) -> TrainJob {
    for _ in 0..400 {
        let j = e.train_job(id).unwrap();
        if !matches!(j.status.as_str(), "queued" | "running") {
            return j;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("job {id} did not finish: {:?}", e.train_job(id).unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn a_job_with_a_fake_trainer_registers_the_adapter() {
    let (e, _d, _w, ds) = ready_to_train(vec![]).await;
    let job =
        e.start_train_job(TrainJobRequest { name: Some("Mio adapter".into()), ..sft_request(&ds, "org/modello-piccolo") }).await.unwrap();
    assert_eq!((job.status.as_str(), job.trainer.as_str()), ("queued", "mlx-lm-lora"));
    let done = wait_job(&e, &job.id).await;
    assert_eq!(done.status, "succeeded", "{:?}", done.error);
    assert_eq!(done.progress, Some(1.0));
    let m = done.metrics.clone().unwrap();
    assert_eq!((m["train_loss"].as_f64(), m["val_loss"].as_f64(), m["steps"].as_u64()), (Some(0.75), Some(1.25), Some(4)));
    let log = e.train_job_log(&job.id, 50).unwrap().join("\n");
    assert!(log.contains("Iter 1: Train loss 2.500"), "{log}");

    // The adapter is registered and its files are where the profile can use them.
    let a = e.adapter(done.adapter_id.as_deref().unwrap()).unwrap();
    assert_eq!((a.name.as_str(), a.format.as_str(), a.method.as_str()), ("Mio adapter", "mlx", "sft"));
    assert!(std::path::Path::new(&a.path).join("adapters.safetensors").is_file());
    assert_eq!(e.adapters().unwrap().len(), 1);
    // The configuration of mlx-lm carries the rank.
    let yaml = std::fs::read_to_string(e.0.dirs.train_jobs().join(&job.id).join("lora.yaml")).unwrap();
    assert!(yaml.contains("rank: 8"), "{yaml}");

    // A profile that uses it keeps it from being deleted; free again, it goes with its files.
    let mut p = profile_with("mlx-lm", vec![AdapterRef { path: a.path.clone(), scale: None }]);
    p.id = "usa-adapter".into();
    e.save_profile(p).unwrap();
    let err = e.delete_adapter(&a.id).unwrap_err();
    assert!(matches!(err, Error::Conflict(_)) && err.to_string().contains("Fine-tuned"), "{err}");
    e.delete_profile("usa-adapter").unwrap();
    e.delete_adapter(&a.id).unwrap();
    assert!(!std::path::Path::new(&a.path).exists() && e.adapters().unwrap().is_empty());
    assert!(matches!(e.adapter(&a.id), Err(Error::NotFound(_))));
}

#[cfg(unix)]
#[tokio::test]
async fn a_failing_trainer_leaves_no_adapter_and_says_why() {
    let (e, _d, _w, ds) = ready_to_train(vec![]).await;
    let job = e.start_train_job(sft_request(&ds, "org/fail")).await.unwrap();
    let done = wait_job(&e, &job.id).await;
    assert_eq!(done.status, "failed");
    let why = done.error.unwrap();
    assert!(why.contains("codice 3") && why.contains("out of memory"), "{why}");
    assert!(e.adapters().unwrap().is_empty());
    assert_eq!(std::fs::read_dir(e.0.dirs.adapters()).map(|d| d.count()).unwrap_or(0), 0, "the partial folder is removed");
}

#[cfg(unix)]
#[tokio::test]
async fn one_job_at_a_time_and_a_running_job_can_be_cancelled() {
    let (e, _d, _w, ds) = ready_to_train(vec![]).await;
    let job = e.start_train_job(sft_request(&ds, "org/slow")).await.unwrap();
    let err = e.start_train_job(sft_request(&ds, "org/altro")).await.unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "{err}");
    // The dataset of a running job cannot be removed.
    assert!(matches!(e.delete_dataset(&ds), Err(Error::Conflict(_))));
    e.cancel_train_job(&job.id).unwrap();
    let done = wait_job(&e, &job.id).await;
    assert_eq!(done.status, "cancelled");
    assert!(e.adapters().unwrap().is_empty());
    // Nothing runs any more: a second cancel is a conflict, and a new job can start.
    assert!(matches!(e.cancel_train_job(&job.id), Err(Error::Conflict(_))));
    assert!(matches!(e.cancel_train_job("trn_none"), Err(Error::NotFound(_))));
    let next = e.start_train_job(sft_request(&ds, "org/ok")).await.unwrap();
    assert_eq!(wait_job(&e, &next.id).await.status, "succeeded");
    assert_eq!(e.train_jobs().unwrap().len(), 2);
}

#[tokio::test]
async fn a_job_is_refused_before_it_starts_when_it_cannot_work() {
    let (e, _d, _w, ds) = ready_to_train(vec![]).await;
    let bad = |r: TrainJobRequest| async { e.start_train_job(r).await.unwrap_err() };
    assert!(matches!(bad(TrainJobRequest { method: "boh".into(), ..sft_request(&ds, "m") }).await, Error::Invalid(_)));
    assert!(matches!(bad(sft_request(&ds, "  ")).await, Error::Invalid(_)));
    assert!(matches!(bad(sft_request("ds_none", "m")).await, Error::NotFound(_)));
    let mut r = sft_request(&ds, "m");
    r.params.rank = Some(0);
    assert!(matches!(bad(r).await, Error::Invalid(_)));
    // A dataset for another method.
    let mut r = sft_request(&ds, "m");
    r.method = "dpo".into();
    assert!(bad(r).await.to_string().contains("è per SFT"));
    // A trainer that does not know the method (or is not here) is refused with the reason.
    let mut r = sft_request(&ds, "m");
    r.trainer = Some("trl".into());
    assert!(matches!(bad(r).await, Error::Unsupported(_)));
    // Comparing the adapter needs a suite and the profile to start from.
    let mut r = sft_request(&ds, "m");
    r.eval_suite = Some("nessuna".into());
    assert!(bad(r).await.to_string().contains("non esiste"));
    assert!(e.train_jobs().unwrap().is_empty(), "a refused job leaves no trace");
}

#[cfg(unix)]
#[tokio::test]
async fn a_job_interrupted_by_a_restart_is_failed() {
    let (e, _d, _w, ds) = ready_to_train(vec![]).await;
    let job = e.start_train_job(sft_request(&ds, "org/slow")).await.unwrap();
    e.cancel_train_job(&job.id).unwrap();
    wait_job(&e, &job.id).await;
    // Simulate a crash: the record says running, nobody is running it.
    let mut j = e.train_job(&job.id).unwrap();
    j.status = "running".into();
    e.0.store.save_train_job(&j.id, &j.method, &j.trainer, &j.status, &j.created_at, &j).unwrap();
    e.fail_interrupted_train_jobs().unwrap();
    let j = e.train_job(&job.id).unwrap();
    assert_eq!(j.status, "failed");
    assert!(j.error.unwrap().contains("interrotto"));
}

// ------------------------------------------------------------------ DPO

/// A session where one question got a good answer and a rejected one (a regeneration).
fn regenerated_session(e: &Engine, question: &str, good: &str, bad: &str) {
    let s = e.create_session("operator", None).unwrap();
    let msg = |role: &str, content: &str| ChatMessage {
        id: new_id("msg"),
        role: role.into(),
        content: content.into(),
        reasoning: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        created_at: now_rfc3339(),
    };
    let (q, down, up) = (msg("user", question), msg("assistant", bad), msg("assistant", good));
    let (down_id, up_id) = (down.id.clone(), up.id.clone());
    e.0.store.append_messages(&s.id, &[q, down, up]).unwrap();
    e.set_feedback(&s.id, &down_id, -1, None).unwrap();
    e.set_feedback(&s.id, &up_id, 1, None).unwrap();
}

#[tokio::test]
async fn a_dpo_dataset_pairs_good_and_rejected_answers_to_the_same_question() {
    let (e, _d, _w) = engine(vec![]).await;
    let dpo = |sources: Vec<String>| DatasetSpec { method: "dpo".into(), sources, ..Default::default() };
    for n in 0..9 {
        regenerated_session(&e, &format!("domanda {n}"), "risposta buona", "risposta scartata");
    }
    let err = e.build_dataset(&dpo(vec![])).unwrap_err();
    assert!(err.to_string().contains("almeno 10 coppie") && err.to_string().contains("ne ho trovati 9"), "{err}");

    regenerated_session(&e, "domanda 9", "la chiave è sk-abcdefghijklmnop1234567890", "no");
    // Answers to different questions make no pair.
    rated_session(&e, 77, Some(1), "ok chiaro");
    rated_session(&e, 78, Some(-1), "ok confuso");
    let info = e.build_dataset(&dpo(vec![])).unwrap();
    assert_eq!((info.method.as_str(), info.examples), ("dpo", 10));
    assert_eq!(info.sources, vec!["feedback"]);

    let all = format!(
        "{}{}",
        std::fs::read_to_string(format!("{}/train.jsonl", info.path)).unwrap(),
        std::fs::read_to_string(format!("{}/valid.jsonl", info.path)).unwrap()
    );
    assert!(!all.contains("sk-abcdefghij"), "secrets are masked");
    let first: Value = serde_json::from_str(all.lines().next().unwrap()).unwrap();
    assert_eq!(first["prompt"][0]["role"], "user");
    assert!(first["chosen"].is_string() && first["rejected"].is_string());
    assert_ne!(first["chosen"], first["rejected"]);

    // The same question (spacing and case aside) rated in another session makes one more pair.
    regenerated_session(&e, "Domanda   0", "altra buona", "altra scartata");
    assert_eq!(e.build_dataset(&dpo(vec![])).unwrap().examples, 11);
    assert!(matches!(e.build_dataset(&dpo(vec!["succeeded".into()])), Err(Error::Invalid(_))));
    assert!(e.build_dataset(&dpo(vec!["eval".into()])).unwrap_err().to_string().contains("report"));
}

/// Like the fake MLX trainer, but for the TRL script: it reads the job configuration and writes a PEFT adapter.
#[cfg(unix)]
fn fake_trl_trainer(e: &Engine) {
    use std::os::unix::fs::PermissionsExt;
    let bin = e.0.dirs.root.join("pyenv").join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = r#"#!/bin/sh
if [ "$1" = "-c" ]; then exit 0; fi
CFG="$3"
OUT=$(sed -n 's/.*"output_dir": "\(.*\)",/\1/p' "$CFG")
METHOD=$(sed -n 's/.*"method": "\(.*\)",/\1/p' "$CFG")
echo "metodo $METHOD"
echo 'AIW_PROGRESS {"step":1,"total":2,"loss":0.9}'
echo 'AIW_PROGRESS {"step":2,"total":2,"loss":0.4}'
mkdir -p "$OUT"
echo weights > "$OUT/adapter_model.safetensors"
echo '{}' > "$OUT/adapter_config.json"
"#;
    let py = bin.join("python");
    std::fs::write(&py, script).unwrap();
    std::fs::set_permissions(&py, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_dpo_job_runs_on_trl_and_mlx_cannot_do_it() {
    let (e, _d, _w) = engine(vec![]).await;
    e.set_device(device("linux", "x86_64", "m", 64, vec![("cuda", Some(24)), ("cpu", None)]));
    fake_trl_trainer(&e);
    for n in 0..10 {
        regenerated_session(&e, &format!("domanda {n}"), "buona", "scartata");
    }
    let ds = e.build_dataset(&DatasetSpec { method: "dpo".into(), ..Default::default() }).unwrap();
    let req = TrainJobRequest { method: "dpo".into(), ..sft_request(&ds.id, "org/modello") };
    let err = e.start_train_job(TrainJobRequest { trainer: Some("mlx-lm-lora".into()), ..req.clone() }).await.unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)), "{err}");

    let job = e.start_train_job(req).await.unwrap();
    assert_eq!(job.trainer, "trl");
    let done = wait_job(&e, &job.id).await;
    assert_eq!(done.status, "succeeded", "{:?}", done.error);
    let cfg: Value =
        serde_json::from_str(&std::fs::read_to_string(e.0.dirs.train_jobs().join(&job.id).join("config.json")).unwrap()).unwrap();
    assert_eq!(cfg["method"], "dpo");
    assert!(e.train_job_log(&job.id, 10).unwrap().join("\n").contains("metodo dpo"));
    let a = e.adapter(done.adapter_id.as_deref().unwrap()).unwrap();
    assert_eq!((a.method.as_str(), a.format.as_str()), ("dpo", "peft"));
}

// ------------------------------------------------------------------ GRPO

/// A suite of `n` arithmetic-like tasks that a regular expression grades.
fn answer_suite(id: &str, n: usize) -> aiw_eval::Suite {
    let tasks: String = (0..n)
        .map(|i| format!("  - id: t{i}\n    prompt: quanto fa {i}+1?\n    graders: [{{ kind: output_matches, pattern: '^{}$' }}]\n", i + 1))
        .collect();
    aiw_eval::Suite::parse(&format!("id: {id}\nname: Somme\ntasks:\n{tasks}")).unwrap()
}

#[tokio::test]
async fn a_reward_is_the_mean_of_the_graders_that_look_at_the_answer() {
    let (e, _d, _w) = engine(vec![]).await;
    let text = "id: mix\nname: Mix\ntasks:\n  - id: due\n    prompt: quanto fa 1+1?\n    graders:\n      - { kind: output_matches, pattern: '^2$' }\n      - { kind: output_matches, pattern: '2' }\n  - id: meta\n    prompt: dimmi due\n    graders:\n      - { kind: output_matches, pattern: 'due' }\n      - { kind: output_matches, pattern: 'tre' }\n  - id: file\n    prompt: crea nota\n    graders: [{ kind: file_exists, path: nota.md }]\n";
    e.save_eval_suite(aiw_eval::Suite::parse(text).unwrap()).unwrap();

    let ok = e.reward("mix", "due", "2").await.unwrap();
    assert_eq!((ok.score, ok.passed, ok.graders.len()), (1.0, true, 2));
    let no = e.reward("mix", "due", "3").await.unwrap();
    assert_eq!((no.score, no.passed), (0.0, false));
    let half = e.reward("mix", "meta", "due").await.unwrap();
    assert_eq!((half.score, half.passed), (0.5, false));

    // Graders on files or commands cannot score a bare answer.
    let err = e.reward("mix", "file", "fatto").await.unwrap_err();
    assert!(matches!(err, Error::Invalid(_)) && err.to_string().contains("file_exists"), "{err}");
    assert!(matches!(e.reward("mix", "boh", "x").await, Err(Error::NotFound(_))));
    assert!(matches!(e.reward("nessuna", "due", "x").await, Err(Error::NotFound(_))));
}

#[tokio::test]
async fn a_grpo_dataset_is_the_prompts_of_a_suite_with_answer_only_graders() {
    let (e, _d, _w) = engine(vec![]).await;
    e.save_eval_suite(answer_suite("somme", 5)).unwrap();
    let spec =
        |suite: Option<&str>, repeat| DatasetSpec { method: "grpo".into(), suite: suite.map(Into::into), repeat, ..Default::default() };
    // Five prompts are not enough; with two repetitions each they are.
    assert!(e.build_dataset(&spec(Some("somme"), None)).unwrap_err().to_string().contains("ne ho trovati 5"));
    let info = e.build_dataset(&spec(Some("somme"), Some(2))).unwrap();
    assert_eq!((info.method.as_str(), info.examples), ("grpo", 10));
    assert_eq!(info.sources, vec!["suite:somme"]);
    let train = std::fs::read_to_string(format!("{}/train.jsonl", info.path)).unwrap();
    let row: Value = serde_json::from_str(train.lines().next().unwrap()).unwrap();
    assert_eq!(row["prompt"][0]["role"], "user");
    assert!(row["task_id"].as_str().unwrap().starts_with('t'));

    assert!(e.build_dataset(&spec(None, None)).unwrap_err().to_string().contains("suite"));
    // A task whose grader needs files is refused, naming it.
    let files = "id: files\nname: File\ntasks:\n  - id: crea\n    prompt: crea nota\n    graders: [{ kind: file_exists, path: nota.md }]\n";
    e.save_eval_suite(aiw_eval::Suite::parse(files).unwrap()).unwrap();
    let err = e.build_dataset(&spec(Some("files"), Some(20))).unwrap_err();
    assert!(matches!(err, Error::Invalid(_)) && err.to_string().contains("«crea»") && err.to_string().contains("file_exists"), "{err}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_grpo_job_gets_the_reward_endpoint_in_its_private_config_and_not_in_its_log() {
    let (e, _d, _w) = engine(vec![]).await;
    e.set_device(device("linux", "x86_64", "l", 128, vec![("cuda", Some(48)), ("cpu", None)]));
    fake_trl_trainer(&e);
    e.save_eval_suite(answer_suite("somme", 5)).unwrap();
    let ds = e
        .build_dataset(&DatasetSpec { method: "grpo".into(), suite: Some("somme".into()), repeat: Some(2), ..Default::default() })
        .unwrap();
    let req = TrainJobRequest { method: "grpo".into(), ..sft_request(&ds.id, "org/modello") };

    // A suite that does not exist is refused up front.
    let err = e.start_train_job(TrainJobRequest { reward_suite: Some("nessuna".into()), ..req.clone() }).await.unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "{err}");

    // Without the API of this process listening there is nobody to ask for rewards.
    let job = e.start_train_job(req.clone()).await.unwrap();
    assert_eq!(job.reward_suite.as_deref(), Some("somme"), "taken from the dataset");
    let failed = wait_job(&e, &job.id).await;
    assert_eq!(failed.status, "failed");
    assert!(failed.error.unwrap().contains("aiw serve"));

    e.set_local_url("http://127.0.0.1:4321".into());
    let job = e.start_train_job(req).await.unwrap();
    let done = wait_job(&e, &job.id).await;
    assert_eq!(done.status, "succeeded", "{:?}", done.error);
    let dir = e.0.dirs.train_jobs().join(&job.id);
    let cfg: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).unwrap()).unwrap();
    assert_eq!(
        (cfg["method"].as_str(), cfg["reward_suite"].as_str(), cfg["reward_url"].as_str()),
        (Some("grpo"), Some("somme"), Some("http://127.0.0.1:4321"))
    );
    assert_eq!(cfg["reward_token"].as_str(), Some(e.token()));
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(dir.join("config.json")).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert!(!e.train_job_log(&job.id, 100).unwrap().join("\n").contains(e.token()));
}
