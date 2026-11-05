//! Training on the engine: trainers and the doctor, datasets, jobs with a fake trainer, adapters.

use super::tests::engine;
use super::*;
use aiw_core::api::{Accelerator, CpuInfo};

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
