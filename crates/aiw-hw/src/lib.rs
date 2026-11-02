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
    let os = normalized_os();
    // A process translated by Rosetta reports x86_64 on an Apple Silicon Mac: the device is aarch64.
    let arch = match std::env::consts::ARCH {
        "x86_64" if os == "macos" && probes::mac_is_apple_silicon() => "aarch64".to_string(),
        other => other.to_string(),
    };
    let base = base_info();
    let (name, os_version, memory_mb, available_memory_mb) = (base.name, base.os_version, base.memory_mb, base.available_mb);
    let brand = if base.brand.is_empty() { arch.clone() } else { base.brand };
    let (threads, cores) = (base.threads.max(1), base.cores.max(1));
    let cpu = CpuInfo { brand: brand.clone(), cores, threads, features: cpu_features() };

    let mut accelerators = vec![Accelerator {
        kind: "cpu".into(),
        name: brand,
        vendor: None,
        memory_mb: None,
        driver: None,
        details: Some(format!("{cores} core / {threads} thread")),
    }];
    accelerators.extend(probes::gpus_and_npus(&os, &arch, memory_mb));

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

struct BaseInfo {
    name: String,
    os_version: String,
    memory_mb: u64,
    available_mb: u64,
    brand: String,
    threads: u32,
    cores: u32,
}

#[cfg(not(target_os = "ios"))]
fn base_info() -> BaseInfo {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let threads = sys.cpus().len().max(1) as u32;
    BaseInfo {
        name: sysinfo::System::host_name().unwrap_or_else(|| "this-device".into()),
        os_version: sysinfo::System::long_os_version().or_else(sysinfo::System::os_version).unwrap_or_default(),
        memory_mb: sys.total_memory() / (1024 * 1024),
        available_mb: sys.available_memory() / (1024 * 1024),
        brand: sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default(),
        threads,
        cores: sysinfo::System::physical_core_count().map(|c| c as u32).unwrap_or(threads),
    }
}

#[cfg(target_os = "ios")]
fn base_info() -> BaseInfo {
    fn sysctl_string(name: &str) -> Option<String> {
        let c = std::ffi::CString::new(name).ok()?;
        let mut len: libc::size_t = 0;
        unsafe {
            if libc::sysctlbyname(c.as_ptr(), std::ptr::null_mut(), &mut len, std::ptr::null_mut(), 0) != 0 || len == 0 {
                return None;
            }
            let mut buf = vec![0u8; len];
            if libc::sysctlbyname(c.as_ptr(), buf.as_mut_ptr() as *mut libc::c_void, &mut len, std::ptr::null_mut(), 0) != 0 {
                return None;
            }
            Some(String::from_utf8_lossy(&buf[..len.saturating_sub(1)]).into_owned())
        }
    }
    fn sysctl_u64(name: &str) -> Option<u64> {
        let c = std::ffi::CString::new(name).ok()?;
        let mut v: u64 = 0;
        let mut len = std::mem::size_of::<u64>() as libc::size_t;
        let ok = unsafe { libc::sysctlbyname(c.as_ptr(), &mut v as *mut u64 as *mut libc::c_void, &mut len, std::ptr::null_mut(), 0) } == 0;
        ok.then_some(v)
    }
    let threads = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1);
    let memory_mb = sysctl_u64("hw.memsize").unwrap_or(0) / (1024 * 1024);
    BaseInfo {
        name: sysctl_string("hw.machine").unwrap_or_else(|| "iPhone".into()),
        os_version: sysctl_string("kern.osproductversion").map(|v| format!("iOS {v}")).unwrap_or_else(|| "iOS".into()),
        memory_mb,
        // iOS does not expose free memory to apps; assume roughly half is usable.
        available_mb: memory_mb / 2,
        brand: "Apple Silicon".into(),
        threads,
        cores: threads,
    }
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

/// Share of RAM that a unified-memory SoC (Apple Silicon, phones) can give to a model: Metal's
/// working-set limit is about two thirds to three quarters of RAM, and iOS/Android kill apps that
/// grow past their share. 100 % of RAM is never available.
pub const UNIFIED_MEMORY_PERCENT: u64 = 70;

/// Memory a model can use on a unified-memory device with `ram_mb` of RAM.
pub fn unified_budget_mb(ram_mb: u64) -> u64 {
    ram_mb * UNIFIED_MEMORY_PERCENT / 100
}

/// Memory budget usable for model weights + KV cache, in MB.
///
/// - Unified memory (Apple Silicon, phones): CPU and GPU draw from the same pool, so the
///   "GPU memory" is a share of RAM ([`unified_budget_mb`]), never the whole of it. A smaller
///   limit reported for the GPU (`iogpu.wired_limit_mb`) is honoured; a larger one, or the total
///   RAM that a platform plugin may have reported as VRAM, is capped.
/// - Otherwise: the larger of the dedicated VRAM and half of the RAM (CPU offload).
pub fn model_budget_mb(p: &DeviceProfile) -> u64 {
    if p.unified_memory() {
        let share = unified_budget_mb(p.memory_mb);
        return match p.vram_mb() {
            0 => share,
            gpu => gpu.min(share),
        };
    }
    p.vram_mb().max(p.memory_mb / 2)
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
///
/// Stdout is drained by a separate thread: a tool that prints more than a pipe buffer
/// (`rocminfo` on a multi-GPU box) would otherwise block on write and always hit the timeout.
/// On timeout the child is killed and the reader thread is left to finish on its own, because a
/// grandchild may still hold the pipe open; nothing ever waits for it unboundedly.
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
        let mut stdout = child.stdout.take()?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut out = String::new();
            let _ = stdout.read_to_string(&mut out);
            let _ = tx.send(out);
        });
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // The process has exited; the pipe normally closes with it. Bounded wait: a
                    // daemonized grandchild could keep it open.
                    let out = rx.recv_timeout(Duration::from_millis(500)).ok()?;
                    return status.success().then_some(out);
                }
                Ok(None) if start.elapsed() > timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    tracing::debug!(cmd, "hardware probe timed out");
                    return None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
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

    fn profile(os: &str, arch: &str, ram_gb: u64, accelerators: Vec<Accelerator>) -> DeviceProfile {
        let mut p = probe();
        p.os = os.into();
        p.arch = arch.into();
        p.memory_mb = ram_gb * 1024;
        let cpu = p.accelerators.remove(0);
        p.accelerators = std::iter::once(cpu).chain(accelerators).collect();
        p
    }

    fn gpu(kind: &str, memory_mb: Option<u64>) -> Accelerator {
        Accelerator { kind: kind.into(), name: kind.into(), vendor: None, memory_mb, driver: None, details: None }
    }

    #[test]
    fn unified_memory_devices_get_70_percent_of_ram_not_all_of_it() {
        // Apple Silicon with the GPU memory reported as the total RAM (what the probe used to do,
        // and what a platform plugin may still send): capped.
        let mac = profile("macos", "aarch64", 16, vec![gpu("metal", Some(16 * 1024))]);
        assert!(mac.unified_memory());
        assert_eq!(unified_budget_mb(16 * 1024), 11468);
        assert_eq!(model_budget_mb(&mac), 11468);
        assert_eq!(tier_for(&mac), "m");
        // No GPU memory reported at all: the same share.
        assert_eq!(model_budget_mb(&profile("macos", "aarch64", 16, vec![gpu("metal", None)])), 11468);
        // A lower limit (`iogpu.wired_limit_mb`) is honoured.
        assert_eq!(model_budget_mb(&profile("macos", "aarch64", 16, vec![gpu("metal", Some(8192))])), 8192);
        // Android with a SoC GPU that claims all the RAM.
        let phone = profile("android", "aarch64", 12, vec![gpu("vulkan", Some(12 * 1024))]);
        assert_eq!(model_budget_mb(&phone), 8601);
        assert_eq!(tier_for(&phone), "m");
        // 8 GB iPhone: 5.6 GB, class `s`.
        let iphone = profile("ios", "aarch64", 8, vec![gpu("metal", Some(8 * 1024))]);
        assert_eq!(model_budget_mb(&iphone), 5734);
        assert_eq!(tier_for(&iphone), "s");
    }

    #[test]
    fn an_intel_mac_is_not_unified_memory() {
        let intel = profile("macos", "x86_64", 16, vec![gpu("metal", None)]);
        assert!(!intel.unified_memory());
        // Half of the RAM for CPU inference, like any discrete-GPU PC; not 70 %.
        assert_eq!(model_budget_mb(&intel), 8 * 1024);
        // A discrete AMD GPU with its own VRAM on that Mac still counts as VRAM.
        let with_vram = profile("macos", "x86_64", 16, vec![gpu("metal", Some(16 * 1024))]);
        assert_eq!(model_budget_mb(&with_vram), 16 * 1024);
        // Same RAM on Apple Silicon is a different budget.
        assert!(model_budget_mb(&profile("macos", "aarch64", 16, vec![gpu("metal", None)])) > model_budget_mb(&intel));
    }

    /// A tool that prints more than a pipe buffer must not deadlock against its own timeout, and a
    /// hung one must be cut off.
    #[cfg(unix)]
    #[test]
    fn external_probes_handle_big_output_and_time_out() {
        let big = run("sh", &["-c", "yes aiw | head -c 300000"], Duration::from_secs(5)).expect("big output is read to the end");
        assert_eq!(big.len(), 300000);
        let started = std::time::Instant::now();
        assert!(run("sleep", &["30"], Duration::from_millis(200)).is_none());
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(run("sh", &["-c", "exit 3"], Duration::from_secs(2)).is_none(), "a failing tool yields nothing");
        assert!(run("definitely-not-a-tool-aiw", &[], Duration::from_secs(2)).is_none());
    }

    #[test]
    fn probe_output_matches_the_architecture_rules() {
        let p = probe();
        if p.os == "macos" && p.arch == "x86_64" {
            assert!(!p.unified_memory());
            assert!(!p.has_accelerator("npu"));
        }
        // No desktop device is ever credited with all of its RAM as GPU memory unless the GPU
        // reports it itself: on unified-memory devices the budget stays below total RAM.
        if p.unified_memory() {
            assert!(model_budget_mb(&p) < p.memory_mb);
        }
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
