//! Fine-tuning (architecture §5.5): AIW orchestrates external trainers, it does not train in Rust.
//! Here: the trainers of this device and `aiw train doctor`.

use crate::Engine;
use aiw_core::{Error, Result};
use aiw_runtimes::trainer::{self, DeviceFacts, TrainerInfo};
use serde::Serialize;

/// The device as `aiw train doctor` shows it.
#[derive(Debug, Clone, Serialize)]
pub struct DoctorDevice {
    pub name: String,
    pub os: String,
    pub arch: String,
    /// xs | s | m | l | xl
    pub tier: String,
    pub memory_mb: u64,
    pub vram_mb: u64,
    pub unified_memory: bool,
    pub accelerators: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrainDoctor {
    pub device: DoctorDevice,
    pub trainers: Vec<TrainerInfo>,
    /// What can be trained here, one phrase per method ("LoRA SFT fino a ~4B su questo Mac").
    pub can_train: Vec<String>,
    /// Paired devices that could do what this one cannot.
    pub remote: Vec<String>,
    /// What to do next, in words.
    pub advice: Vec<String>,
}

impl Engine {
    fn device_facts(&self) -> DeviceFacts {
        DeviceFacts::from_profile(&self.device())
    }

    // ------------------------------------------------------------------ trainers
    /// Every trainer with its state on this device.
    pub async fn trainers(&self) -> Vec<TrainerInfo> {
        let facts = self.device_facts();
        let mut out = Vec::new();
        for m in self.0.runtimes.trainers() {
            let installed = trainer::unsupported_reason(&m, &facts).is_none() && self.0.runtimes.trainer_python(&m.id).await.is_some();
            out.push(trainer::assess(&m, &facts, installed));
        }
        out
    }

    pub async fn trainer_install_plan(&self, id: &str) -> Result<aiw_runtimes::install::InstallPlan> {
        self.0.runtimes.trainer_install_plan(id).await
    }

    /// Installs the trainer in the Python environment of AIW, in the background. The job is
    /// followed with [`trainer_install_status`](Self::trainer_install_status).
    pub async fn trainer_install_start(&self, id: &str) -> Result<aiw_runtimes::install::InstallJob> {
        let m = self.0.runtimes.trainer(id)?;
        if let Some(why) = trainer::unsupported_reason(&m, &self.device_facts()) {
            return Err(Error::Unsupported(format!("{} non si può usare su questo dispositivo: {why}", m.name)));
        }
        let (cmds, job) = self.0.runtimes.begin_trainer_install(id).await?;
        self.audit_user("trainer.install", &format!("{}: {}", m.name, cmds.iter().map(|c| c.display()).collect::<Vec<_>>().join(" && ")));
        let initial = job.lock().unwrap().clone();
        tokio::spawn(async move {
            aiw_runtimes::install::run(cmds, job).await;
        });
        Ok(initial)
    }

    pub fn trainer_install_status(&self, id: &str) -> Option<aiw_runtimes::install::InstallJob> {
        self.0.runtimes.trainer_install_job(id)
    }

    // ------------------------------------------------------------------ doctor
    pub async fn train_doctor(&self) -> TrainDoctor {
        let d = self.device();
        let facts = DeviceFacts::from_profile(&d);
        let trainers = self.trainers().await;
        let manifests = self.0.runtimes.trainers();
        let mut can_train = Vec::new();
        let mut advice = Vec::new();
        for (m, info) in manifests.iter().zip(&trainers) {
            if info.state == "unsupported" {
                continue;
            }
            for phrase in trainer::describe(m, &facts) {
                can_train.push(if info.state == "ready" {
                    format!("{phrase} ({})", m.name)
                } else {
                    format!("{phrase} ({}, da installare)", m.name)
                });
            }
            if info.state == "missing" {
                advice.push(format!("Installa {} con `aiw train install {}` o dalla pagina Addestramento.", m.name, m.id));
            }
        }
        // Peers that could train what this device cannot.
        let mut remote = Vec::new();
        if can_train.is_empty() {
            for peer in self.peers() {
                let Some(pf) = DeviceFacts::from_peer(&peer) else { continue };
                for m in &manifests {
                    if trainer::unsupported_reason(m, &pf).is_none() {
                        let phrases = trainer::describe(m, &pf);
                        if let Some(best) = phrases.first() {
                            remote.push(format!(
                                "{} (classe {}) potrebbe: {}",
                                peer.name,
                                pf.tier.to_uppercase(),
                                best.replace(&format!("su questo {}", pf.label()), "su quel dispositivo")
                            ));
                            break;
                        }
                    }
                }
            }
            if remote.is_empty() {
                advice.push(
                    "Questo dispositivo non può addestrare: serve un Mac con Apple Silicon o un PC con una GPU CUDA. Accoppia un dispositivo che ce l'ha."
                        .to_string(),
                );
            } else {
                advice.push("Esegui l'addestramento sul dispositivo accoppiato indicato e usa l'adapter qui.".to_string());
            }
        }
        TrainDoctor {
            device: DoctorDevice {
                name: d.name.clone(),
                os: d.os.clone(),
                arch: d.arch.clone(),
                tier: d.tier.clone(),
                memory_mb: d.memory_mb,
                vram_mb: facts.vram_mb,
                unified_memory: facts.unified,
                accelerators: facts.accelerators.clone(),
            },
            trainers,
            can_train,
            remote,
            advice,
        }
    }
}
