use crate::run;
use aiw_core::api::Accelerator;
use std::time::Duration;

const T: Duration = Duration::from_secs(4);

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

pub fn gpus_and_npus(os: &str, ram_mb: u64) -> Vec<Accelerator> {
    let mut out = Vec::new();
    match os {
        "ios" => {
            out.push(acc("metal", "Apple GPU", Some("Apple"), Some(ram_mb), None, Some("memoria unificata".into())));
            out.push(acc("npu", "Apple Neural Engine", Some("Apple"), None, None, None));
            return out;
        }
        "android" => {
            // The Android plugin refines this (Vulkan/OpenCL/NNAPI/QNN availability) at runtime.
            out.push(acc("vulkan", "GPU del SoC", None, Some(ram_mb), None, Some("memoria condivisa".into())));
            return out;
        }
        _ => {}
    }

    // NVIDIA on every desktop OS.
    if let Some(s) = run("nvidia-smi", &["--query-gpu=name,memory.total,driver_version", "--format=csv,noheader,nounits"], T) {
        for line in s.lines().filter(|l| !l.trim().is_empty()) {
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();
            let name = parts.first().copied().unwrap_or("NVIDIA GPU").to_string();
            let mem = parts.get(1).and_then(|m| m.parse::<u64>().ok());
            let drv = parts.get(2).map(|d| format!("driver {d}"));
            out.push(acc("cuda", name.clone(), Some("NVIDIA"), mem, drv, None));
            out.push(acc("vulkan", name, Some("NVIDIA"), mem, None, None));
        }
    }

    match os {
        "macos" => {
            let brand = run("sysctl", &["-n", "machdep.cpu.brand_string"], T).unwrap_or_default();
            if brand.contains("Apple") || std::env::consts::ARCH == "aarch64" {
                out.push(acc(
                    "metal",
                    format!("{} GPU", brand.trim()),
                    Some("Apple"),
                    Some(ram_mb),
                    None,
                    Some("memoria unificata".into()),
                ));
                out.push(acc("npu", "Apple Neural Engine", Some("Apple"), None, None, None));
            } else {
                out.push(acc("metal", "GPU (Metal)", None, None, None, None));
            }
        }
        "windows" => {
            let ps = run(
                "powershell",
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,DriverVersion | ConvertTo-Json -Compress",
                ],
                Duration::from_secs(8),
            );
            if let Some(json) = ps.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
                let list = if json.is_array() { json.as_array().cloned().unwrap_or_default() } else { vec![json] };
                for g in list {
                    let name = g["Name"].as_str().unwrap_or("GPU").to_string();
                    let mem = g["AdapterRAM"].as_u64().map(|b| b / (1024 * 1024)).filter(|m| *m > 0);
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
            let npu = run(
                "powershell",
                &["-NoProfile", "-NonInteractive", "-Command", "Get-PnpDevice -PresentOnly | Where-Object { $_.Class -eq 'ComputeAccelerator' -or $_.FriendlyName -match 'NPU|Neural|AI Boost' } | Select-Object -ExpandProperty FriendlyName"],
                Duration::from_secs(8),
            );
            for n in npu.unwrap_or_default().lines().map(str::trim).filter(|l| !l.is_empty()) {
                out.push(acc("npu", n, None, None, None, Some("Windows ML / Foundry Local".into())));
            }
        }
        _ => linux_drm_and_accel(&mut out),
    }

    // Vulkan devices not already covered (Intel/AMD iGPUs…).
    if os != "macos" {
        if let Some(s) = run("vulkaninfo", &["--summary"], T) {
            for name in
                s.lines().filter_map(|l| l.trim().strip_prefix("deviceName").map(|r| r.trim_start_matches([' ', '=']).trim().to_string()))
            {
                if !name.contains("llvmpipe") && !out.iter().any(|a| a.kind == "vulkan" && a.name == name) {
                    out.push(acc("vulkan", name, None, None, None, None));
                }
            }
        }
    }
    out
}

fn read(p: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn linux_drm_and_accel(out: &mut Vec<Accelerator>) {
    let rocm = std::path::Path::new("/opt/rocm").exists();
    let sycl = std::path::Path::new("/opt/intel/oneapi").exists();
    if let Ok(dir) = std::fs::read_dir("/sys/class/drm") {
        for e in dir.flatten() {
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
                    let name = product.unwrap_or_else(|| "AMD Radeon GPU".into());
                    if rocm {
                        out.push(acc("rocm", name.clone(), Some("AMD"), vram, None, None));
                    }
                    out.push(acc("vulkan", name, Some("AMD"), vram, None, None));
                }
                "0x8086" => {
                    let name = product.unwrap_or_else(|| "Intel GPU".into());
                    if sycl {
                        out.push(acc("sycl", name.clone(), Some("Intel"), vram, None, None));
                    }
                    out.push(acc("vulkan", name, Some("Intel"), vram, None, None));
                }
                _ => {}
            }
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
