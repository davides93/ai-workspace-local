use crate::{run, unified_budget_mb};
use aiw_core::api::Accelerator;
use std::path::{Path, PathBuf};
use std::time::Duration;

const T: Duration = Duration::from_secs(4);
const T_WINDOWS: Duration = Duration::from_secs(8);

fn acc(
    kind: &str,
    name: impl Into<String>,
    vendor: Option<&str>,
    memory_mb: Option<u64>,
    driver: Option<String>,
    details: Option<String>,
) -> Accelerator {
    Accelerator { kind: kind.into(), name: name.into(), vendor: vendor.map(String::from), memory_mb, driver, details }
}

pub fn form_factor(os: &str) -> String {
    match os {
        "ios" | "android" => "phone".into(),
        "linux" => {
            let battery = std::fs::read_dir("/sys/class/power_supply")
                .map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("BAT")))
                .unwrap_or(false);
            let display = std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
            if battery {
                "laptop"
            } else if display {
                "desktop"
            } else {
                "server"
            }
            .into()
        }
        _ => "desktop".into(),
    }
}

// ------------------------------------------------------------------ Apple

/// True on an Apple Silicon Mac even for a process that runs translated by Rosetta
/// (`hw.optional.arm64` is 1 on the hardware, whatever the architecture of the process).
pub fn mac_is_apple_silicon() -> bool {
    run("sysctl", &["-n", "hw.optional.arm64"], T).is_some_and(|s| s.trim() == "1")
}

/// `Apple M2 Pro` stays as is; a Rosetta process reports `VirtualApple @ 2.50GHz`, which is not a
/// chip name.
fn apple_chip_name(brand: &str) -> String {
    let b = brand.trim();
    if b.starts_with("Apple") {
        b.to_string()
    } else {
        "Apple Silicon".to_string()
    }
}

/// GPU memory of an Apple Silicon Mac: the unified-memory share of RAM, lowered to the limit
/// the user set with `sudo sysctl iogpu.wired_limit_mb=…` when that is smaller (`0` = default).
pub(crate) fn apple_gpu_budget_mb(ram_mb: u64, wired_limit_mb: Option<u64>) -> u64 {
    let share = unified_budget_mb(ram_mb);
    match wired_limit_mb {
        Some(w) if w > 0 => w.min(share),
        _ => share,
    }
}

fn unified_details(budget_mb: u64) -> String {
    format!("memoria unificata · ~{:.0} GB utilizzabili dalla GPU", budget_mb as f64 / 1024.0)
}

// ------------------------------------------------------------------ Vulkan

/// One physical device of `vulkaninfo --summary`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VkDevice {
    pub name: String,
    pub vendor_id: Option<u32>,
    /// `PHYSICAL_DEVICE_TYPE_DISCRETE_GPU`, `…_INTEGRATED_GPU`, `…_CPU`…
    pub kind: String,
}

impl VkDevice {
    /// Software rasterizers (llvmpipe) are not accelerators.
    fn is_software(&self) -> bool {
        self.kind.ends_with("_CPU") || self.name.to_lowercase().contains("llvmpipe")
    }

    fn vendor(&self) -> Option<&'static str> {
        match self.vendor_id? {
            0x10de => Some("NVIDIA"),
            0x1002 => Some("AMD"),
            0x8086 => Some("Intel"),
            0x106b => Some("Apple"),
            0x5143 => Some("Qualcomm"),
            _ => None,
        }
    }
}

/// Parses `vulkaninfo --summary`: blocks `GPU0:` with `vendorID`, `deviceType` and `deviceName`
/// lines (`key = value`). Software devices are dropped.
pub(crate) fn parse_vulkaninfo(text: &str) -> Vec<VkDevice> {
    #[derive(Default)]
    struct Block {
        name: Option<String>,
        vendor_id: Option<u32>,
        kind: String,
    }
    fn flush(b: &mut Block, out: &mut Vec<VkDevice>) {
        let b = std::mem::take(b);
        if let Some(name) = b.name {
            out.push(VkDevice { name, vendor_id: b.vendor_id, kind: b.kind });
        }
    }
    let mut out = Vec::new();
    let mut cur = Block::default();
    for line in text.lines() {
        let l = line.trim();
        // `GPU0:` starts a device block.
        if l.starts_with("GPU") && l.ends_with(':') {
            flush(&mut cur, &mut out);
            continue;
        }
        let Some((key, value)) = l.split_once('=') else { continue };
        let value = value.trim();
        match key.trim() {
            "vendorID" => cur.vendor_id = u32::from_str_radix(value.trim_start_matches("0x"), 16).ok(),
            "deviceType" => cur.kind = value.to_string(),
            "deviceName" => {
                // Output without `GPUn:` headers: a second name means a new device.
                if cur.name.is_some() {
                    flush(&mut cur, &mut out);
                }
                cur.name = Some(value.to_string());
            }
            _ => {}
        }
    }
    flush(&mut cur, &mut out);
    out.retain(|d| !d.is_software());
    out
}

/// Whether a Vulkan loader library is installed (the tool `vulkaninfo` is optional: it ships in
/// `vulkan-tools`, not with the loader).
fn vulkan_loader_installed(os: &str) -> bool {
    match os {
        "windows" => {
            let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
            loader_in(&[root.join("System32")], &["vulkan-1.dll"])
        }
        "linux" => {
            let dirs: Vec<PathBuf> = [
                "/usr/lib",
                "/usr/lib64",
                "/usr/lib/x86_64-linux-gnu",
                "/usr/lib/aarch64-linux-gnu",
                "/lib/x86_64-linux-gnu",
                "/lib/aarch64-linux-gnu",
                "/usr/local/lib",
            ]
            .iter()
            .map(PathBuf::from)
            .collect();
            loader_in(&dirs, &["libvulkan.so.1", "libvulkan.so"])
        }
        _ => false,
    }
}

fn loader_in(dirs: &[PathBuf], names: &[&str]) -> bool {
    dirs.iter().any(|d| names.iter().any(|n| d.join(n).exists()))
}

/// Adds the devices reported by `vulkaninfo` to the accelerator list without duplicating the
/// ones that were assumed from a vendor tool: a device with the same name, or else the first
/// not-yet-matched Vulkan entry of the same vendor, is renamed to what Vulkan calls it (and keeps
/// its memory); anything else is added.
pub(crate) fn merge_vulkan(out: &mut Vec<Accelerator>, devices: &[VkDevice]) {
    let mut matched: Vec<usize> = Vec::new();
    for d in devices {
        let vendor = d.vendor();
        let by_name = out.iter().enumerate().position(|(i, a)| a.kind == "vulkan" && a.name == d.name && !matched.contains(&i));
        let found = by_name.or_else(|| {
            let v = vendor?;
            out.iter().enumerate().position(|(i, a)| a.kind == "vulkan" && a.vendor.as_deref() == Some(v) && !matched.contains(&i))
        });
        match found {
            Some(i) => {
                out[i].name = d.name.clone();
                matched.push(i);
            }
            None => {
                out.push(acc("vulkan", d.name.clone(), vendor, None, None, None));
                matched.push(out.len() - 1);
            }
        }
    }
}

// ------------------------------------------------------------------ ROCm

/// One card of `rocm-smi --showproductname --showmeminfo vram --json`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RocmCard {
    pub name: String,
    pub vram_mb: Option<u64>,
}

/// Parses the JSON of `rocm-smi --showproductname --showmeminfo vram --json`:
/// `{"card0": {"Card Series": "Radeon RX 7900 XTX", "VRAM Total Memory (B)": "25753026560"}, …}`.
/// Key capitalisation differs between ROCm releases, so keys are compared case-insensitively.
pub(crate) fn parse_rocm_smi(json: &str) -> Vec<RocmCard> {
    let Ok(serde_json::Value::Object(root)) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    let mut cards: Vec<(u32, RocmCard)> = root
        .iter()
        .filter_map(|(k, v)| {
            let idx = k.strip_prefix("card")?.parse::<u32>().ok()?;
            let obj = v.as_object()?;
            let get = |wanted: &str| obj.iter().find(|(key, _)| key.to_lowercase() == wanted).and_then(|(_, v)| v.as_str());
            let name = ["card series", "card model", "card sku"]
                .iter()
                .filter_map(|w| get(w))
                .map(str::trim)
                .find(|s| !s.is_empty() && !s.starts_with("0x"))
                .map(String::from)
                .unwrap_or_else(|| "AMD Radeon GPU".into());
            let vram_mb =
                get("vram total memory (b)").and_then(|b| b.trim().parse::<u64>().ok()).map(|b| b / (1024 * 1024)).filter(|m| *m > 0);
            Some((idx, RocmCard { name, vram_mb }))
        })
        .collect();
    cards.sort_by_key(|(i, _)| *i);
    cards.into_iter().map(|(_, c)| c).collect()
}

/// GPU architectures (`gfx1100`…) listed by `rocminfo`, one per GPU agent. The `Name:` lines of
/// CPU agents and of the `amdgcn-amd-amdhsa--gfx…` ISA entries are ignored.
pub(crate) fn parse_rocminfo_gfx(text: &str) -> Vec<String> {
    text.lines().filter_map(|l| l.trim().strip_prefix("Name:")).map(str::trim).filter(|n| n.starts_with("gfx")).map(String::from).collect()
}

/// What is known about ROCm on this machine.
#[derive(Debug, Clone, Default)]
pub(crate) struct RocmProbe {
    /// Cards answered by `rocm-smi`.
    pub cards: Vec<RocmCard>,
    /// `/opt/rocm` (or `$ROCM_PATH`) exists.
    pub installed: bool,
    /// `/dev/kfd`, the compute device node of the amdgpu driver, exists.
    pub kfd: bool,
    pub gfx: Vec<String>,
}

impl RocmProbe {
    /// ROCm can run compute here: `rocm-smi` sees a GPU, or the toolkit is installed and the
    /// kernel exposes `/dev/kfd`. The mere presence of `/opt/rocm` on a box without the compute
    /// device (an NVIDIA machine with a leftover toolkit, a container) does not count.
    pub fn usable(&self) -> bool {
        !self.cards.is_empty() || (self.installed && self.kfd)
    }

    fn from_parts(smi: Option<&str>, rocminfo: Option<&str>, installed: bool, kfd: bool) -> Self {
        Self {
            cards: smi.map(parse_rocm_smi).unwrap_or_default(),
            installed,
            kfd,
            gfx: rocminfo.map(parse_rocminfo_gfx).unwrap_or_default(),
        }
    }
}

fn rocm_installed() -> bool {
    Path::new("/opt/rocm").exists() || std::env::var_os("ROCM_PATH").is_some_and(|p| Path::new(&p).exists())
}

// ------------------------------------------------------------------ collection

/// Output of the external tools. They are launched concurrently, each with its own timeout, so a
/// hung driver tool costs one timeout and not the sum of all of them.
#[derive(Default)]
struct Raw {
    nvidia_smi: Option<String>,
    vulkaninfo: Option<String>,
    rocm_smi: Option<String>,
    rocminfo: Option<String>,
    mac_brand: Option<String>,
    mac_wired_limit: Option<String>,
    win_gpus: Option<String>,
    win_npus: Option<String>,
    /// JSON of the 64-bit `HardwareInformation.qwMemorySize` registry values of the display
    /// adapters (`Win32_VideoController.AdapterRAM` is a 32-bit field, capped at 4 GB).
    win_gpu_qw: Option<String>,
}

fn collect(os: &str) -> Raw {
    let mut raw = Raw::default();
    std::thread::scope(|s| {
        let nvidia = s.spawn(|| run("nvidia-smi", &["--query-gpu=name,memory.total,driver_version", "--format=csv,noheader,nounits"], T));
        let vulkan = s.spawn(|| (os != "macos").then(|| run("vulkaninfo", &["--summary"], T)).flatten());
        let rocm_smi =
            s.spawn(|| (os == "linux").then(|| run("rocm-smi", &["--showproductname", "--showmeminfo", "vram", "--json"], T)).flatten());
        // rocminfo is only worth its (long) output where the toolkit is installed.
        let rocminfo = s.spawn(|| (os == "linux" && rocm_installed()).then(|| run("rocminfo", &[], T)).flatten());
        let mac = s.spawn(|| {
            if os != "macos" {
                return (None, None);
            }
            (run("sysctl", &["-n", "machdep.cpu.brand_string"], T), run("sysctl", &["-n", "iogpu.wired_limit_mb"], T))
        });
        let win = s.spawn(|| {
            if os != "windows" {
                return (None, None, None);
            }
            let gpus = run(
                "powershell",
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,DriverVersion | ConvertTo-Json -Compress",
                ],
                T_WINDOWS,
            );
            let npus = run(
                "powershell",
                &["-NoProfile", "-NonInteractive", "-Command", "Get-PnpDevice -PresentOnly | Where-Object { $_.Class -eq 'ComputeAccelerator' -or $_.FriendlyName -match 'NPU|Neural|AI Boost' } | Select-Object -ExpandProperty FriendlyName"],
                T_WINDOWS,
            );
            let qw = run("powershell", &["-NoProfile", "-NonInteractive", "-Command", WIN_QW_SCRIPT], T_WINDOWS);
            (gpus, npus, qw)
        });
        raw.nvidia_smi = nvidia.join().ok().flatten();
        raw.vulkaninfo = vulkan.join().ok().flatten();
        raw.rocm_smi = rocm_smi.join().ok().flatten();
        raw.rocminfo = rocminfo.join().ok().flatten();
        (raw.mac_brand, raw.mac_wired_limit) = mac.join().unwrap_or_default();
        (raw.win_gpus, raw.win_npus, raw.win_gpu_qw) = win.join().unwrap_or_default();
    });
    raw
}

/// Reads the 64-bit video memory size that display drivers publish in the registry.
const WIN_QW_SCRIPT: &str = r"Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11cd-bfc1-08002be10318}' -ErrorAction SilentlyContinue | ForEach-Object { $p = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue; if ($p.'HardwareInformation.qwMemorySize') { [pscustomobject]@{ Name = $p.DriverDesc; Qw = [uint64]$p.'HardwareInformation.qwMemorySize' } } } | ConvertTo-Json -Compress";

/// `Win32_VideoController.AdapterRAM` is an unsigned 32-bit value: a card with 4 GB or more is
/// reported as (about) 4 GB, or wrapped to a small number.
const ADAPTER_RAM_CAP: u64 = 0xFF00_0000;

/// `(adapter name, bytes)` pairs from the JSON produced by [`WIN_QW_SCRIPT`] (one object, or an
/// array when several adapters have the value).
fn parse_qw_memory(json: &str) -> Vec<(String, u64)> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    let list = match v {
        serde_json::Value::Array(a) => a,
        o @ serde_json::Value::Object(_) => vec![o],
        _ => vec![],
    };
    list.iter().filter_map(|e| Some((e["Name"].as_str()?.trim().to_string(), e["Qw"].as_u64().filter(|b| *b > 0)?))).collect()
}

/// Video memory of a Windows adapter in MB: `AdapterRAM`, unless that looks capped (4 GB or
/// more, or missing) and the registry knows the real 64-bit size of an adapter with that name.
fn windows_vram_mb(name: &str, adapter_ram: Option<u64>, qw: &[(String, u64)]) -> Option<u64> {
    let mb = |b: u64| b / (1024 * 1024);
    let reported = adapter_ram.filter(|b| *b > 0);
    if reported.is_none_or(|b| b >= ADAPTER_RAM_CAP) {
        if let Some((_, b)) = qw.iter().find(|(n, b)| n.eq_ignore_ascii_case(name.trim()) && *b >= reported.unwrap_or(0)) {
            return Some(mb(*b));
        }
    }
    reported.map(mb)
}

pub fn gpus_and_npus(os: &str, arch: &str, ram_mb: u64) -> Vec<Accelerator> {
    let mut out = Vec::new();
    match os {
        "ios" => {
            // iOS does not hand an app the whole RAM (jetsam), and CPU/GPU share one pool.
            let budget = unified_budget_mb(ram_mb);
            out.push(acc("metal", "Apple GPU", Some("Apple"), Some(budget), None, Some(unified_details(budget))));
            out.push(acc("npu", "Apple Neural Engine", Some("Apple"), None, None, None));
            return out;
        }
        "android" => {
            // The Android plugin refines this (Vulkan/OpenCL/NNAPI/QNN availability) at runtime.
            let budget = unified_budget_mb(ram_mb);
            out.push(acc("vulkan", "GPU del SoC", None, Some(budget), None, Some(unified_details(budget))));
            return out;
        }
        _ => {}
    }

    let raw = collect(os);
    let vk_devices = raw.vulkaninfo.as_deref().map(parse_vulkaninfo).unwrap_or_default();
    // Vulkan is only claimed for GPUs the loader can actually open: `vulkaninfo` answered, or the
    // loader library is installed. An NVIDIA/AMD/Intel GPU alone does not imply it.
    let vulkan_ok = raw.vulkaninfo.is_some() || vulkan_loader_installed(os);

    // NVIDIA on every desktop OS.
    if let Some(s) = &raw.nvidia_smi {
        for line in s.lines().filter(|l| !l.trim().is_empty()) {
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();
            let name = parts.first().copied().unwrap_or("NVIDIA GPU").to_string();
            let mem = parts.get(1).and_then(|m| m.parse::<u64>().ok());
            let drv = parts.get(2).map(|d| format!("driver {d}"));
            out.push(acc("cuda", name.clone(), Some("NVIDIA"), mem, drv, None));
            if vulkan_ok {
                out.push(acc("vulkan", name, Some("NVIDIA"), mem, None, None));
            }
        }
    }

    match os {
        "macos" => {
            let brand = raw.mac_brand.clone().unwrap_or_default();
            if arch == "aarch64" {
                // Apple Silicon: unified memory, so the GPU memory is a share of RAM.
                let wired = raw.mac_wired_limit.as_deref().and_then(|s| s.trim().parse::<u64>().ok());
                let budget = apple_gpu_budget_mb(ram_mb, wired);
                out.push(acc(
                    "metal",
                    format!("{} GPU", apple_chip_name(&brand)),
                    Some("Apple"),
                    Some(budget),
                    None,
                    Some(unified_details(budget)),
                ));
                out.push(acc("npu", "Apple Neural Engine", Some("Apple"), None, None, None));
            } else {
                // Intel Mac: Metal exists, but memory is not unified and there is no Neural Engine
                // (so neither MLX nor Apple Foundation Models).
                out.push(acc("metal", "GPU (Metal)", None, None, None, None));
            }
        }
        "windows" => {
            if let Some(json) = raw.win_gpus.as_deref().and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) {
                let list = if json.is_array() { json.as_array().cloned().unwrap_or_default() } else { vec![json] };
                let qw = raw.win_gpu_qw.as_deref().map(parse_qw_memory).unwrap_or_default();
                for g in list {
                    let name = g["Name"].as_str().unwrap_or("GPU").to_string();
                    let mem = windows_vram_mb(&name, g["AdapterRAM"].as_u64(), &qw).filter(|m| *m > 0);
                    out.push(acc(
                        "directml",
                        name,
                        None,
                        mem,
                        g["DriverVersion"].as_str().map(|d| format!("driver {d}")),
                        Some("DirectML / Windows ML".into()),
                    ));
                }
            }
            for n in raw.win_npus.as_deref().unwrap_or_default().lines().map(str::trim).filter(|l| !l.is_empty()) {
                out.push(acc("npu", n, None, None, None, Some("Windows ML / Foundry Local".into())));
            }
        }
        _ => {
            let rocm =
                RocmProbe::from_parts(raw.rocm_smi.as_deref(), raw.rocminfo.as_deref(), rocm_installed(), Path::new("/dev/kfd").exists());
            linux_drm_and_accel(&mut out, &rocm, vulkan_ok);
        }
    }

    // Devices that Vulkan itself reports (Intel/AMD iGPUs…): merged, not duplicated.
    if os != "macos" {
        merge_vulkan(&mut out, &vk_devices);
    }
    out
}

fn read(p: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn linux_drm_and_accel(out: &mut Vec<Accelerator>, rocm: &RocmProbe, vulkan_ok: bool) {
    let sycl = std::path::Path::new("/opt/intel/oneapi").exists();
    let mut amd_seen = 0usize;
    if let Ok(dir) = std::fs::read_dir("/sys/class/drm") {
        // Stable order: card0, card1…
        let mut cards: Vec<_> = dir.flatten().collect();
        cards.sort_by_key(|e| e.file_name());
        for e in cards {
            let n = e.file_name().to_string_lossy().to_string();
            if !n.starts_with("card") || n.contains('-') {
                continue;
            }
            let dev = e.path().join("device");
            let vendor = read(&dev.join("vendor")).unwrap_or_default();
            let vram = read(&dev.join("mem_info_vram_total")).and_then(|v| v.parse::<u64>().ok()).map(|b| b / (1024 * 1024));
            let product = read(&dev.join("product_name"));
            match vendor.as_str() {
                "0x1002" => {
                    // rocm-smi knows the marketing name and VRAM when sysfs does not say.
                    let card = rocm.cards.get(amd_seen);
                    let name = product.or_else(|| card.map(|c| c.name.clone())).unwrap_or_else(|| "AMD Radeon GPU".into());
                    let vram = vram.or_else(|| card.and_then(|c| c.vram_mb));
                    if rocm.usable() {
                        out.push(acc("rocm", name.clone(), Some("AMD"), vram, None, rocm.gfx.get(amd_seen).cloned()));
                    }
                    if vulkan_ok {
                        out.push(acc("vulkan", name, Some("AMD"), vram, None, None));
                    }
                    amd_seen += 1;
                }
                "0x8086" => {
                    let name = product.unwrap_or_else(|| "Intel GPU".into());
                    if sycl {
                        out.push(acc("sycl", name.clone(), Some("Intel"), vram, None, None));
                    }
                    if vulkan_ok {
                        out.push(acc("vulkan", name, Some("Intel"), vram, None, None));
                    }
                }
                _ => {}
            }
        }
    }
    // No AMD card in sysfs (restricted container) but rocm-smi sees them: trust it.
    if amd_seen == 0 {
        for (i, c) in rocm.cards.iter().enumerate() {
            out.push(acc("rocm", c.name.clone(), Some("AMD"), c.vram_mb, None, rocm.gfx.get(i).cloned()));
        }
    }
    // Linux accel subsystem: Intel NPU (intel_vpu), AMD XDNA (amdxdna), others.
    if let Ok(dir) = std::fs::read_dir("/sys/class/accel") {
        for e in dir.flatten() {
            let dev = e.path().join("device");
            let driver = std::fs::read_link(dev.join("driver"))
                .ok()
                .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
                .unwrap_or_default();
            let (name, vendor) = match driver.as_str() {
                "intel_vpu" => ("Intel NPU (AI Boost)", Some("Intel")),
                "amdxdna" => ("AMD XDNA NPU (Ryzen AI)", Some("AMD")),
                _ => ("NPU", None),
            };
            out.push(acc("npu", name, vendor, None, Some(driver), None));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_vram_above_4gb_comes_from_the_64_bit_registry_value() {
        let qw =
            parse_qw_memory(r#"[{"Name":"NVIDIA GeForce RTX 4090","Qw":25757220864},{"Name":"Intel(R) UHD Graphics","Qw":134217728}]"#);
        assert_eq!(qw.len(), 2);
        // AdapterRAM capped just below 4 GB: the registry knows better.
        assert_eq!(windows_vram_mb("NVIDIA GeForce RTX 4090", Some(4_293_918_720), &qw), Some(24564));
        // Missing/wrapped to zero: same.
        assert_eq!(windows_vram_mb("nvidia geforce rtx 4090", Some(0), &qw), Some(24564));
        // A card below 4 GB keeps AdapterRAM; the registry value is not consulted.
        assert_eq!(windows_vram_mb("Intel(R) UHD Graphics", Some(1_073_741_824), &qw), Some(1024));
        // No registry entry for that name: the reported value stands.
        assert_eq!(windows_vram_mb("Other GPU", Some(4_293_918_720), &qw), Some(4095));
        assert_eq!(windows_vram_mb("Other GPU", None, &qw), None);
        // A single object (one adapter has the value) and garbage input.
        assert_eq!(parse_qw_memory(r#"{"Name":"X","Qw":8589934592}"#), vec![("X".to_string(), 8589934592)]);
        assert!(parse_qw_memory("not json").is_empty());
    }

    const VULKANINFO: &str = "\
==========
VULKANINFO
==========

Vulkan Instance Version: 1.3.280

Devices:
========
GPU0:
\tapiVersion         = 1.3.274
\tdriverVersion      = 24.0.9
\tvendorID           = 0x1002
\tdeviceID           = 0x744c
\tdeviceType         = PHYSICAL_DEVICE_TYPE_DISCRETE_GPU
\tdeviceName         = AMD Radeon RX 7900 XTX (RADV NAVI31)
\tdriverID           = DRIVER_ID_MESA_RADV
\tdriverName         = radv
\tdriverInfo         = Mesa 24.0.9
\tconformanceVersion = 1.3.0.0
GPU1:
\tapiVersion         = 1.3.274
\tvendorID           = 0x8086
\tdeviceType         = PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU
\tdeviceName         = Intel(R) Graphics (RPL-S)
GPU2:
\tapiVersion         = 1.3.274
\tvendorID           = 0x10005
\tdeviceType         = PHYSICAL_DEVICE_TYPE_CPU
\tdeviceName         = llvmpipe (LLVM 17.0.6, 256 bits)
";

    fn a(kind: &str, name: &str, vendor: Option<&str>, mem: Option<u64>) -> Accelerator {
        acc(kind, name, vendor, mem, None, None)
    }

    #[test]
    fn parses_vulkaninfo_summary_and_drops_software_devices() {
        let d = parse_vulkaninfo(VULKANINFO);
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!(d[0].name, "AMD Radeon RX 7900 XTX (RADV NAVI31)");
        assert_eq!(d[0].vendor(), Some("AMD"));
        assert_eq!(d[1].name, "Intel(R) Graphics (RPL-S)");
        assert_eq!(d[1].vendor(), Some("Intel"));
        // Older output without `GPUn:` headers: every deviceName is a device.
        let d = parse_vulkaninfo("\tvendorID = 0x10de\n\tdeviceType = PHYSICAL_DEVICE_TYPE_DISCRETE_GPU\n\tdeviceName = NVIDIA GeForce RTX 4090\n\tdeviceName = NVIDIA GeForce RTX 3060\n");
        assert_eq!(d.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), vec!["NVIDIA GeForce RTX 4090", "NVIDIA GeForce RTX 3060"]);
        assert!(parse_vulkaninfo("").is_empty());
        assert!(parse_vulkaninfo("ERROR: [Loader Message] Code 0 : vkCreateInstance failed").is_empty());
    }

    #[test]
    fn vulkan_devices_are_merged_with_the_assumed_entries_not_duplicated() {
        // Assumed from nvidia-smi (same name) and from sysfs (vendor only, generic name).
        let mut out = vec![
            a("cpu", "cpu", None, None),
            a("cuda", "NVIDIA GeForce RTX 4090", Some("NVIDIA"), Some(24564)),
            a("vulkan", "NVIDIA GeForce RTX 4090", Some("NVIDIA"), Some(24564)),
            a("vulkan", "AMD Radeon GPU", Some("AMD"), Some(8192)),
        ];
        let devices = vec![
            VkDevice { name: "NVIDIA GeForce RTX 4090".into(), vendor_id: Some(0x10de), kind: "PHYSICAL_DEVICE_TYPE_DISCRETE_GPU".into() },
            VkDevice {
                name: "AMD Radeon RX 6600 (RADV NAVI23)".into(),
                vendor_id: Some(0x1002),
                kind: "PHYSICAL_DEVICE_TYPE_DISCRETE_GPU".into(),
            },
            VkDevice { name: "Intel(R) Arc(tm) A380".into(), vendor_id: Some(0x8086), kind: "PHYSICAL_DEVICE_TYPE_DISCRETE_GPU".into() },
        ];
        merge_vulkan(&mut out, &devices);
        let vk: Vec<(&str, Option<u64>)> = out.iter().filter(|x| x.kind == "vulkan").map(|x| (x.name.as_str(), x.memory_mb)).collect();
        assert_eq!(
            vk,
            vec![
                ("NVIDIA GeForce RTX 4090", Some(24564)),
                ("AMD Radeon RX 6600 (RADV NAVI23)", Some(8192)),
                ("Intel(R) Arc(tm) A380", None)
            ]
        );
        // Merging twice does not add anything.
        merge_vulkan(&mut out, &devices);
        assert_eq!(out.iter().filter(|x| x.kind == "vulkan").count(), 3);
    }

    #[test]
    fn two_identical_gpus_stay_two() {
        let mut out = vec![a("vulkan", "AMD Radeon GPU", Some("AMD"), None), a("vulkan", "AMD Radeon GPU", Some("AMD"), None)];
        let d = VkDevice { name: "AMD Radeon RX 7600".into(), vendor_id: Some(0x1002), kind: "PHYSICAL_DEVICE_TYPE_DISCRETE_GPU".into() };
        merge_vulkan(&mut out, &[d.clone(), d]);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|x| x.name == "AMD Radeon RX 7600"));
    }

    #[test]
    fn finds_the_vulkan_loader_in_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = vec![dir.path().join("missing"), dir.path().to_path_buf()];
        assert!(!loader_in(&dirs, &["libvulkan.so.1"]));
        std::fs::write(dir.path().join("libvulkan.so.1"), b"").unwrap();
        assert!(loader_in(&dirs, &["libvulkan.so.1", "libvulkan.so"]));
        assert!(!loader_in(&dirs, &["vulkan-1.dll"]));
    }

    const ROCM_SMI: &str = r#"{"card0": {"Card Series": "Radeon RX 7900 XTX", "Card Model": "0x744c", "Card Vendor": "Advanced Micro Devices, Inc. [AMD/ATI]", "Card SKU": "NAVI31", "VRAM Total Memory (B)": "25753026560"},
        "card1": {"Card series": "AMD Radeon Graphics", "VRAM Total Memory (B)": "536870912"}, "system": {"Driver version": "6.7.0"}}"#;

    #[test]
    fn parses_rocm_smi_json_with_either_key_capitalisation() {
        let cards = parse_rocm_smi(ROCM_SMI);
        assert_eq!(
            cards,
            vec![
                RocmCard { name: "Radeon RX 7900 XTX".into(), vram_mb: Some(24560) },
                RocmCard { name: "AMD Radeon Graphics".into(), vram_mb: Some(512) },
            ]
        );
        // Name only from the model id: skipped in favour of a readable fallback.
        let c = parse_rocm_smi(r#"{"card0": {"Card Model": "0x744c"}}"#);
        assert_eq!(c, vec![RocmCard { name: "AMD Radeon GPU".into(), vram_mb: None }]);
        assert!(parse_rocm_smi("not json").is_empty());
        assert!(parse_rocm_smi(r#"{"system": {"Driver version": "6"}}"#).is_empty());
        assert!(parse_rocm_smi("").is_empty());
    }

    #[test]
    fn card_order_is_numeric() {
        let json = r#"{"card10": {"Card Series": "ten"}, "card2": {"Card Series": "two"}, "card1": {"Card Series": "one"}}"#;
        let names: Vec<String> = parse_rocm_smi(json).into_iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["one", "two", "ten"]);
    }

    #[test]
    fn parses_gfx_architectures_from_rocminfo() {
        let text = "\
*******
Agent 1
*******
  Name:                    AMD Ryzen 9 7950X 16-Core Processor
  Marketing Name:          AMD Ryzen 9 7950X 16-Core Processor
  Device Type:             CPU
*******
Agent 2
*******
  Name:                    gfx1100
  Marketing Name:          AMD Radeon RX 7900 XTX
  Device Type:             GPU
  ISA Info:
    ISA 1
      Name:                    amdgcn-amd-amdhsa--gfx1100
";
        assert_eq!(parse_rocminfo_gfx(text), vec!["gfx1100"]);
        assert!(parse_rocminfo_gfx("").is_empty());
    }

    #[test]
    fn rocm_needs_a_compute_device_not_just_a_toolkit() {
        let smi = Some(ROCM_SMI);
        // rocm-smi sees a GPU: enough on its own.
        assert!(RocmProbe::from_parts(smi, None, false, false).usable());
        // Toolkit plus /dev/kfd: usable.
        assert!(RocmProbe::from_parts(None, None, true, true).usable());
        // A leftover /opt/rocm without the kernel compute node, or /dev/kfd without a toolkit.
        assert!(!RocmProbe::from_parts(None, None, true, false).usable());
        assert!(!RocmProbe::from_parts(None, None, false, true).usable());
        assert!(!RocmProbe::from_parts(Some("{}"), None, false, false).usable());
    }

    #[test]
    fn apple_gpu_budget_is_a_share_of_ram_and_honours_a_lower_wired_limit() {
        assert_eq!(apple_gpu_budget_mb(16 * 1024, None), 11468); // 70 % of 16 GB
        assert_eq!(apple_gpu_budget_mb(16 * 1024, Some(0)), 11468); // 0 = system default
        assert_eq!(apple_gpu_budget_mb(16 * 1024, Some(8192)), 8192); // user lowered it
        assert_eq!(apple_gpu_budget_mb(16 * 1024, Some(15000)), 11468); // never above the share
        assert_eq!(apple_chip_name("Apple M2 Pro"), "Apple M2 Pro");
        assert_eq!(apple_chip_name("VirtualApple @ 2.50GHz Processor"), "Apple Silicon");
    }

    #[test]
    fn phones_report_a_share_of_ram_not_all_of_it() {
        let ios = gpus_and_npus("ios", "aarch64", 8 * 1024);
        assert_eq!(ios[0].kind, "metal");
        assert_eq!(ios[0].memory_mb, Some(5734));
        let android = gpus_and_npus("android", "aarch64", 12 * 1024);
        assert_eq!(android[0].memory_mb, Some(8601));
        assert!(android[0].details.as_deref().unwrap().contains("memoria unificata"));
    }
}
