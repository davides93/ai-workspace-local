use std::path::PathBuf;
use std::process::Command;

const COMMANDS: &[&str] = &["availability", "generate"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).android_path("android").ios_path("ios").build();
    println!("cargo:rerun-if-changed=macos-helper");
    println!("cargo:rerun-if-env-changed=AIW_REQUIRE_APPLE_FM");
    println!("cargo:rerun-if-env-changed=AIW_APPLE_FM_DEVELOPER_DIR");
    println!("cargo:rustc-check-cfg=cfg(aiw_apple_fm_helper)");
    build_apple_helper();
}

/// Builds the Apple Foundation Models helper (`macos-helper/main.swift`) for macOS on Apple
/// Silicon and turns on `cfg(aiw_apple_fm_helper)` so that `desktop.rs` embeds it.
///
/// Anything that prevents it (not macOS, Intel slice, Xcode older than 26) only skips the helper,
/// with a warning, unless `AIW_REQUIRE_APPLE_FM=1` (release CI): then the build fails, so a
/// release cannot silently ship without it.
///
/// `AIW_APPLE_FM_DEVELOPER_DIR` points at the `Contents/Developer` folder of an Xcode 26 to use
/// for this step only. The rest of the build (llama.cpp, the app) keeps the Xcode that is
/// selected: with Xcode 26's clang the Metal sources of llama.cpp reference
/// `___isPlatformVersionAtLeast`, which the Rust linker does not provide.
fn build_apple_helper() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    // iOS has its own Swift plugin; the Intel slice of a universal build has no Apple Intelligence.
    if target_os != "macos" || target_arch != "aarch64" {
        return;
    }
    let required = std::env::var("AIW_REQUIRE_APPLE_FM").is_ok_and(|v| v == "1");
    let skip = |why: String| {
        if required {
            panic!("AIW_REQUIRE_APPLE_FM=1 but the Apple Foundation Models helper cannot be built: {why}");
        }
        println!("cargo:warning=Apple Foundation Models helper not built: {why}");
    };

    // Xcode tools of the host; the environment of an iOS or Tauri build must not redirect them.
    let developer_dir = std::env::var_os("AIW_APPLE_FM_DEVELOPER_DIR").filter(|d| !d.is_empty());
    let xcrun = |args: &[&str]| {
        let mut c = Command::new("xcrun");
        if let Some(d) = &developer_dir {
            c.env("DEVELOPER_DIR", d);
        }
        c.args(args).env_remove("SDKROOT").env_remove("MACOSX_DEPLOYMENT_TARGET").env_remove("IPHONEOS_DEPLOYMENT_TARGET").output()
    };
    let sdk = match xcrun(&["--sdk", "macosx", "--show-sdk-version"]) {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(o) => return skip(format!("xcrun: {}", String::from_utf8_lossy(&o.stderr).trim())),
        Err(e) => return skip(format!("xcrun not available ({e})")),
    };
    let major: u32 = sdk.split('.').next().and_then(|v| v.parse().ok()).unwrap_or(0);
    if major < 26 {
        return skip(format!("macOS SDK {sdk} has no FoundationModels (Xcode 26 needed)"));
    }

    let out: PathBuf = std::env::var_os("OUT_DIR").expect("OUT_DIR").into();
    let exe = out.join("aiw-apple-fm");
    let src = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("macos-helper/main.swift");
    // Built for macOS 26 itself: the app only runs it there (it checks the OS first), so
    // FoundationModels needs no weak linking.
    let built = xcrun(&[
        "--sdk",
        "macosx",
        "swiftc",
        "-O",
        "-target",
        "arm64-apple-macos26.0",
        "-module-name",
        "aiw_apple_fm",
        "-o",
        exe.to_str().expect("utf-8 path"),
        src.to_str().expect("utf-8 path"),
    ]);
    match built {
        Ok(o) if o.status.success() => {
            println!("cargo:rustc-cfg=aiw_apple_fm_helper");
        }
        Ok(o) => skip(format!("swiftc failed: {}", String::from_utf8_lossy(&o.stderr).trim())),
        Err(e) => skip(format!("swiftc not available ({e})")),
    }
}
