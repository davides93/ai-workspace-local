//! Trainers: external programs (mlx-lm, TRL, Unsloth) that AIW orchestrates to fine-tune a model.
//! AIW does not train in Rust. A trainer is a manifest like a runtime, in its own file
//! (`manifests/trainers.json`, `kind: "trainer"`) so that it never shows up among the inference
//! runtimes or the profiles; the code here only decides what a device can do with it.

use crate::manifest::{Detect, Installer};
use aiw_core::api::{DeviceProfile, PeerDevice};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How much memory a LoRA fine-tune of a model up to `up_to_b` billion parameters needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryTier {
    pub up_to_b: f64,
    pub memory_gb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainerManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub vendor: Option<String>,
    /// Always `trainer`.
    pub kind: String,
    #[serde(default)]
    pub description: String,
    pub platforms: Vec<String>,
    #[serde(default)]
    pub arch: Vec<String>,
    /// At least one of these must be on the device (`metal`, `cuda`).
    #[serde(default)]
    pub accelerators: Vec<String>,
    /// `sft`, `dpo`, `grpo`.
    pub methods: Vec<String>,
    /// What the adapters it writes look like: `mlx` or `peft`.
    pub adapter_format: String,
    /// Class (`xs`..`xl`) a device needs for a method, when more than the memory is required.
    #[serde(default)]
    pub min_tier: BTreeMap<String, String>,
    #[serde(default)]
    pub detect: Detect,
    /// Memory needed for SFT, by model size, smallest first. `dpo` and `grpo` need more
    /// ([`method_factor`]).
    pub memory: Vec<MemoryTier>,
    #[serde(default)]
    pub install: BTreeMap<String, String>,
    #[serde(default)]
    pub installer: BTreeMap<String, Installer>,
    #[serde(default)]
    pub homepage: Option<String>,
}

impl TrainerManifest {
    pub fn installer_for(&self, os: &str) -> Option<&Installer> {
        self.installer.get(os).or_else(|| self.installer.get("any"))
    }
    pub fn install_hint(&self, os: &str) -> Option<String> {
        self.install.get(os).or_else(|| self.install.get("any")).cloned()
    }
}

/// Built-in trainers shipped in the binary.
pub fn builtin() -> Vec<TrainerManifest> {
    serde_json::from_str(include_str!("../manifests/trainers.json")).expect("builtin trainer manifests are valid")
}

/// What a device offers to a trainer. Built from this device's profile or from what a paired
/// device reported about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceFacts {
    pub os: String,
    pub arch: String,
    pub tier: String,
    pub memory_mb: u64,
    pub vram_mb: u64,
    pub unified: bool,
    pub accelerators: Vec<String>,
}

impl DeviceFacts {
    pub fn from_profile(d: &DeviceProfile) -> Self {
        Self {
            os: d.os.clone(),
            arch: d.arch.clone(),
            tier: d.tier.clone(),
            memory_mb: d.memory_mb,
            vram_mb: d.vram_mb(),
            unified: d.unified_memory(),
            accelerators: d.accelerators.iter().map(|a| a.kind.clone()).collect(),
        }
    }

    /// `None` for a peer paired before it reported its hardware.
    pub fn from_peer(p: &PeerDevice) -> Option<Self> {
        let h = p.hardware.as_ref()?;
        Some(Self {
            os: p.os.clone(),
            arch: h.arch.clone(),
            tier: h.tier.clone(),
            memory_mb: h.memory_mb,
            vram_mb: h.vram_mb,
            unified: h.unified_memory,
            accelerators: h.accelerators.clone(),
        })
    }

    /// The memory a fine-tune can use: the whole RAM when it is shared with the GPU, otherwise
    /// the memory of the GPU.
    pub fn training_memory_gb(&self) -> f64 {
        let mb = if self.unified { self.memory_mb } else { self.vram_mb };
        mb as f64 / 1024.0
    }

    /// "Mac" for Apple computers, "dispositivo" otherwise.
    pub fn label(&self) -> &'static str {
        if self.os == "macos" {
            "Mac"
        } else {
            "dispositivo"
        }
    }
}

fn tier_rank(t: &str) -> usize {
    ["xs", "s", "m", "l", "xl"].iter().position(|x| *x == t).unwrap_or(0)
}

/// Memory a method needs compared with SFT: DPO keeps a reference model next to the policy, GRPO
/// also generates several answers per prompt.
pub fn method_factor(method: &str) -> f64 {
    match method {
        "dpo" => 1.5,
        "grpo" => 2.0,
        _ => 1.0,
    }
}

/// `sft` -> "LoRA SFT", `dpo` -> "LoRA DPO"…
pub fn method_label(method: &str) -> String {
    format!("LoRA {}", method.to_uppercase())
}

/// What one method can do on the device.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MethodFit {
    pub method: String,
    /// Largest model (billions of parameters) that fits, when the method can run at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_params_b: Option<f64>,
    /// Why it cannot run here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
}

/// `ready`: it can train here and is installed; `missing`: it could train here but is not
/// installed; `unsupported`: this device cannot run it (`reason` says why).
#[derive(Debug, Clone, Serialize)]
pub struct TrainerInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub methods: Vec<MethodFit>,
    pub adapter_format: String,
    pub install_auto: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
}

fn gb(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// Why `m` cannot run on `d` at all (platform, CPU, accelerator or memory), or `None`.
pub fn unsupported_reason(m: &TrainerManifest, d: &DeviceFacts) -> Option<String> {
    if !m.platforms.iter().any(|p| p == &d.os || p == "any") {
        return Some(format!("disponibile su: {}", m.platforms.join(", ")));
    }
    if !m.arch.is_empty() && !m.arch.iter().any(|a| a == &d.arch) {
        return Some(if d.os == "macos" && m.arch.iter().any(|a| a == "aarch64") {
            "richiede un Mac con Apple Silicon".to_string()
        } else {
            format!("richiede un processore {}", m.arch.join("/"))
        });
    }
    if !m.accelerators.is_empty() && !m.accelerators.iter().any(|a| d.accelerators.iter().any(|x| x == a)) {
        return Some(format!("richiede un acceleratore {} (non trovato)", m.accelerators.join(" o ")));
    }
    let smallest = m.memory.first()?;
    let have = d.training_memory_gb();
    if have < smallest.memory_gb {
        let what = if d.unified { "memoria unificata" } else { "VRAM" };
        return Some(format!(
            "servono almeno {} GB di {what} anche per il modello più piccolo, qui ce ne sono {}",
            gb(smallest.memory_gb),
            gb(have)
        ));
    }
    None
}

/// Largest model size (billions of parameters) the memory of `d` allows for `method`.
pub fn max_params_b(m: &TrainerManifest, d: &DeviceFacts, method: &str) -> Option<f64> {
    let have = d.training_memory_gb();
    m.memory.iter().filter(|t| t.memory_gb * method_factor(method) <= have).map(|t| t.up_to_b).fold(None, |best, b| match best {
        Some(x) if x >= b => Some(x),
        _ => Some(b),
    })
}

/// The methods of `m` and what each can do on `d`.
pub fn method_fits(m: &TrainerManifest, d: &DeviceFacts) -> Vec<MethodFit> {
    m.methods
        .iter()
        .map(|method| {
            if let Some(min) = m.min_tier.get(method) {
                if tier_rank(&d.tier) < tier_rank(min) {
                    return MethodFit {
                        method: method.clone(),
                        max_params_b: None,
                        blocked: Some(format!(
                            "serve un dispositivo di classe {} o superiore (questo è {})",
                            min.to_uppercase(),
                            d.tier.to_uppercase()
                        )),
                    };
                }
            }
            match max_params_b(m, d, method) {
                Some(b) => MethodFit { method: method.clone(), max_params_b: Some(b), blocked: None },
                None => MethodFit {
                    method: method.clone(),
                    max_params_b: None,
                    blocked: Some(format!("serve più memoria di quella disponibile ({} GB)", gb(d.training_memory_gb()))),
                },
            }
        })
        .collect()
}

/// The state of trainer `m` on `d`. `installed`: its Python modules import in the environment of AIW.
pub fn assess(m: &TrainerManifest, d: &DeviceFacts, installed: bool) -> TrainerInfo {
    let reason = unsupported_reason(m, d);
    let methods = if reason.is_some() { vec![] } else { method_fits(m, d) };
    let state = match (&reason, installed) {
        (Some(_), _) => "unsupported",
        (None, true) => "ready",
        (None, false) => "missing",
    };
    TrainerInfo {
        id: m.id.clone(),
        name: m.name.clone(),
        description: m.description.clone(),
        state: state.into(),
        reason,
        methods,
        adapter_format: m.adapter_format.clone(),
        install_auto: m.installer_for(&d.os).is_some(),
        install_hint: m.install_hint(&d.os),
        homepage: m.homepage.clone(),
    }
}

/// "LoRA SFT fino a ~4B su questo Mac", one phrase per method that can run.
pub fn describe(m: &TrainerManifest, d: &DeviceFacts) -> Vec<String> {
    method_fits(m, d)
        .into_iter()
        .filter_map(|f| {
            let b = f.max_params_b?;
            Some(format!("{} fino a ~{}B su questo {}", method_label(&f.method), gb(b), d.label()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(os: &str, arch: &str, tier: &str, mem_gb: u64, vram_gb: u64, accel: &[&str]) -> DeviceFacts {
        DeviceFacts {
            os: os.into(),
            arch: arch.into(),
            tier: tier.into(),
            memory_mb: mem_gb * 1024,
            vram_mb: vram_gb * 1024,
            unified: os == "macos" && arch == "aarch64",
            accelerators: accel.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn trainer(id: &str) -> TrainerManifest {
        builtin().into_iter().find(|m| m.id == id).unwrap()
    }

    #[test]
    fn builtin_trainers_are_valid_and_are_not_inference_runtimes() {
        let all = builtin();
        assert!(all.len() >= 3);
        for m in &all {
            assert_eq!(m.kind, "trainer");
            assert!(!m.methods.is_empty() && !m.memory.is_empty(), "{}", m.id);
            assert!(m.memory.windows(2).all(|w| w[0].up_to_b < w[1].up_to_b && w[0].memory_gb < w[1].memory_gb), "{}", m.id);
            assert!(!m.detect.python_modules.is_empty(), "{}", m.id);
        }
        // Neither the catalog of the runtimes nor the profiles know them.
        let runtimes = crate::manifest::builtin();
        assert!(runtimes.iter().all(|r| all.iter().all(|t| t.id != r.id) && r.kind != "trainer"));
    }

    #[test]
    fn a_mac_with_enough_memory_trains_with_mlx() {
        let m = trainer("mlx-lm-lora");
        let mac = facts("macos", "aarch64", "m", 24, 0, &["metal", "cpu"]);
        let info = assess(&m, &mac, false);
        assert_eq!(info.state, "missing");
        assert_eq!(info.methods.len(), 1);
        assert_eq!(info.methods[0].max_params_b, Some(4.0));
        assert_eq!(describe(&m, &mac), vec!["LoRA SFT fino a ~4B su questo Mac"]);
        assert_eq!(assess(&m, &mac, true).state, "ready");
        let big = facts("macos", "aarch64", "xl", 64, 0, &["metal"]);
        assert_eq!(max_params_b(&m, &big, "sft"), Some(14.0));
    }

    #[test]
    fn the_reason_names_what_is_missing() {
        let mlx = trainer("mlx-lm-lora");
        let intel = facts("macos", "x86_64", "m", 32, 0, &["metal"]);
        assert!(unsupported_reason(&mlx, &intel).unwrap().contains("Apple Silicon"));
        let small = facts("macos", "aarch64", "xs", 4, 0, &["metal"]);
        let why = unsupported_reason(&mlx, &small).unwrap();
        assert!(why.contains("almeno 8 GB") && why.contains("memoria unificata") && why.contains("ce ne sono 4"), "{why}");
        let linux = facts("linux", "x86_64", "m", 32, 0, &["cpu"]);
        assert!(unsupported_reason(&mlx, &linux).unwrap().contains("macos"));
        let trl = trainer("trl");
        assert!(unsupported_reason(&trl, &linux).unwrap().contains("cuda"));
        let weak_gpu = facts("linux", "x86_64", "s", 16, 4, &["cuda", "cpu"]);
        assert!(unsupported_reason(&trl, &weak_gpu).unwrap().contains("VRAM"));
        let info = assess(&trl, &weak_gpu, true);
        assert_eq!((info.state.as_str(), info.methods.len()), ("unsupported", 0));
    }

    #[test]
    fn methods_need_more_memory_and_grpo_a_big_device() {
        let trl = trainer("trl");
        // 24 GB of VRAM, class M: SFT up to 8B, DPO (x1.5) up to 4B, GRPO is refused for the class.
        let gpu = facts("linux", "x86_64", "m", 64, 24, &["cuda"]);
        let fits = method_fits(&trl, &gpu);
        let by = |name: &str| fits.iter().find(|f| f.method == name).unwrap();
        assert_eq!(by("sft").max_params_b, Some(8.0));
        assert_eq!(by("dpo").max_params_b, Some(4.0));
        assert!(by("grpo").max_params_b.is_none() && by("grpo").blocked.as_ref().unwrap().contains("classe L"), "{:?}", by("grpo"));
        // A class XL machine with 80 GB can do GRPO (x2) up to 14B.
        let big = facts("linux", "x86_64", "xl", 256, 80, &["cuda"]);
        assert_eq!(method_fits(&trl, &big).iter().find(|f| f.method == "grpo").unwrap().max_params_b, Some(14.0));
        // Unsloth needs less memory than TRL for the same model.
        let unsloth = trainer("unsloth");
        assert!(max_params_b(&unsloth, &gpu, "sft") > max_params_b(&trl, &gpu, "sft"));
    }

    #[test]
    fn peers_are_judged_with_what_they_reported() {
        let trl = trainer("trl");
        let peer = PeerDevice {
            id: "p".into(),
            name: "Workstation".into(),
            os: "linux".into(),
            address: "http://10.0.0.2:7777".into(),
            status: "online".into(),
            shared_runtimes: vec![],
            hardware: Some(aiw_core::api::PeerHardware {
                arch: "x86_64".into(),
                tier: "xl".into(),
                memory_mb: 128 * 1024,
                vram_mb: 48 * 1024,
                accelerators: vec!["cuda".into()],
                unified_memory: false,
            }),
        };
        let f = DeviceFacts::from_peer(&peer).unwrap();
        assert!(unsupported_reason(&trl, &f).is_none());
        let old = PeerDevice { hardware: None, ..peer };
        assert!(DeviceFacts::from_peer(&old).is_none());
    }
}
