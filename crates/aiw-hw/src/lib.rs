//! Cross-platform hardware probe producing an [`aiw_core::api::DeviceProfile`].
//!
//! Probes are best-effort and never fail: missing tools (nvidia-smi, rocm-smi, vulkaninfo,
//! PowerShell) simply yield fewer accelerators. Mobile shells can add accelerators reported by
//! the platform plugin with [`DeviceProfile`] mutation before handing it to the engine.

use aiw_core::api::{Accelerator, CpuInfo, DeviceProfile};
use std::time::Duration;

mod probes;

/// Detects the current device. Takes up to a few seconds the first time (external tools).
pub fn probe() -> DeviceProfile {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();

    let os = normalized_os();
    let name = sysinfo::System::host_name().unwrap_or_else(|| "this-device".into());
    let os_version = sysinfo::System::long_os_version().or_else(sysinfo::System::os_version).unwrap_or_default();
    let arch = std::env::consts::ARCH.to_string();
    let memory_mb = sys.total_memory() / (1024 * 1024);
    let available_memory_mb = sys.available_memory() / (1024 * 1024);

    let brand = sys.cpus().first().map(|c| c.brand().trim().to_string()).filter(|b| !b.is_empty()).unwrap_or_else(|| arch.clone());
    let threads = sys.cpus().len().max(1) as u32;
    let cores = sysinfo::System::physical_core_count().map(|c| c as u32).unwrap_or(threads);
    let cpu = CpuInfo { brand: brand.clone(), cores, threads, features: cpu_features() };

    let mut accelerators = vec![Accelerator {
        kind: "cpu".into(),
        name: brand,
        vendor: None,
        memory_mb: None,
        driver: None,
        details: Some(format!("{cores} core / {threads} thread")),
    }];
    accelerators.extend(probes::gpus_and_npus(&os, memory_mb));

    let form_factor = probes::form_factor(&os);
    let mut profile = DeviceProfile {
        id: format!("dev_{:016x}", fnv1a(format!("{name}|{os}|{arch}").as_bytes())),
        name,
        os,
        os_version,
        arch,
        form_factor,
        cpu,
        memory_mb,
        available_memory_mb,
        accelerators,
        tier: String::new(),
    };
    profile.tier = tier_for(&profile).to_string();
    profile
}

/// `windows | macos | linux | android | ios`
pub fn normalized_os() -> String {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "android" => "android",
        "ios" => "ios",
        _ => "linux",
    }
    .to_string()
}

/// Memory budget usable for model weights + KV cache, in MB.
pub fn model_budget_mb(p: &DeviceProfile) -> u64 {
    let ram_share = if p.unified_memory() { p.memory_mb * 7 / 10 } else { p.memory_mb / 2 };
    p.vram_mb().max(ram_share)
}

/// Device class used for recommendations (DESIGN §Dispositivi).
pub fn tier_for(p: &DeviceProfile) -> &'static str {
    match model_budget_mb(p) {
        b if b < 4 * 1024 => "xs",
        b if b < 8 * 1024 => "s",
        b if b < 16 * 1024 => "m",
        b if b < 32 * 1024 => "l",
        _ => "xl",
    }
}

fn cpu_features() -> Vec<String> {
    #[allow(unused_mut)]
    let mut f: Vec<&str> = Vec::new();
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("avx") {
            f.push("avx");
        }
        if std::is_x86_feature_detected!("avx2") {
            f.push("avx2");
        }
        if std::is_x86_feature_detected!("fma") {
            f.push("fma");
        }
        if std::is_x86_feature_detected!("f16c") {
            f.push("f16c");
        }
        if std::is_x86_feature_detected!("avx512f") {
            f.push("avx512f");
        }
        if std::is_x86_feature_detected!("avx512bw") {
            f.push("avx512bw");
        }
        if std::is_x86_feature_detected!("avx512vnni") {
            f.push("avx512_vnni");
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        f.push("neon");
        if std::arch::is_aarch64_feature_detected!("dotprod") {
            f.push("dotprod");
        }
        if std::arch::is_aarch64_feature_detected!("i8mm") {
            f.push("i8mm");
        }
        if std::arch::is_aarch64_feature_detected!("sve") {
            f.push("sve");
        }
        if std::arch::is_aarch64_feature_detected!("fp16") {
            f.push("fp16");
        }
    }
    f.into_iter().map(String::from).collect()
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Runs an external probe with a timeout. Never used on mobile (no process spawning there).
pub(crate) fn run(cmd: &str, args: &[&str], timeout: Duration) -> Option<String> {
    #[cfg(any(target_os = "ios", target_os = "android"))]
    {
        let _ = (cmd, args, timeout);
        None
    }
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        use std::io::Read;
        use std::process::{Command, Stdio};
        let mut child = Command::new(cmd).args(args).stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null()).spawn().ok()?;
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut out = String::new();
                    child.stdout.take()?.read_to_string(&mut out).ok()?;
                    return status.success().then_some(out);
                }
                Ok(None) if start.elapsed() > timeout => {
                    let _ = child.kill();
                    tracing::debug!(cmd, "hardware probe timed out");
                    return None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(_) => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_returns_a_sane_profile() {
        let p = probe();
        assert!(p.memory_mb > 0);
        assert!(p.cpu.threads >= 1);
        assert_eq!(p.accelerators[0].kind, "cpu");
        assert!(["xs", "s", "m", "l", "xl"].contains(&p.tier.as_str()));
        assert!(p.id.starts_with("dev_"));
    }

    #[test]
    fn tiers_follow_memory_budget() {
        let mut p = probe();
        p.os = "linux".into();
        p.accelerators.retain(|a| a.kind == "cpu");
        p.memory_mb = 16 * 1024;
        assert_eq!(tier_for(&p), "m");
        p.accelerators.push(Accelerator {
            kind: "cuda".into(),
            name: "GPU".into(),
            vendor: None,
            memory_mb: Some(24 * 1024),
            driver: None,
            details: None,
        });
        assert_eq!(tier_for(&p), "l");
        p.os = "ios".into();
        p.accelerators.retain(|a| a.kind == "cpu");
        p.memory_mb = 8 * 1024;
        assert_eq!(tier_for(&p), "s");
    }
}
