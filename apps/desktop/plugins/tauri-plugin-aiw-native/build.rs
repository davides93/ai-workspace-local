use std::path::PathBuf;
use std::process::Command;

const COMMANDS: &[&str] = &["availability", "generate"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).android_path("android").ios_path("ios").build();
    println!("cargo:rerun-if-changed=macos-helper");
    println!("cargo:rerun-if-changed=ios/Sources/AiwNativePlugin/AppleFMCore.swift");
    println!("cargo:rerun-if-env-changed=AIW_REQUIRE_APPLE_FM");
    println!("cargo:rerun-if-env-changed=AIW_APPLE_FM_DEVELOPER_DIR");
    println!("cargo:rustc-check-cfg=cfg(aiw_apple_fm_helper)");
    build_apple_helper();
    build_macos_browser();
}

/// Native browser is linked into the sandboxed app, never launched as an external helper.
fn build_macos_browser() {
    println!("cargo:rerun-if-changed=macos-browser");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") || std::env::var_os("CARGO_FEATURE_MACOS_BROWSER").is_none() {
        return;
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let arch = if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64") { "arm64" } else { "x86_64" };
    let target = format!("{arch}-apple-macos11.0");
    let sdk = Command::new("xcrun").args(["--sdk", "macosx", "--show-sdk-path"]).output().expect("macOS SDK");
    assert!(sdk.status.success(), "macOS SDK unavailable");
    let sdk = String::from_utf8(sdk.stdout).unwrap();
    let built = Command::new("xcrun")
        .args([
            "--sdk",
            "macosx",
            "swiftc",
            "-O",
            "-parse-as-library",
            "-emit-library",
            "-static",
            "-module-name",
            "AiwBrowser",
            "-target",
            &target,
            "-sdk",
            sdk.trim(),
            "macos-browser/Browser.swift",
            "-o",
        ])
        .arg(out.join("libaiw_browser.a"))
        .output()
        .expect("swiftc for native browser");
    assert!(built.status.success(), "native browser: {}", String::from_utf8_lossy(&built.stderr));
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=aiw_browser");
    // Apple's linker consumes LC_LINKER_OPTION entries in the Swift object, including its
    // framework/runtime libraries. Supply the SDK stubs; the OS provides the Swift runtime.
    println!("cargo:rustc-link-search=native={}/usr/lib/swift", sdk.trim());
    // Older deployment targets also need Swift's static compatibility shims from the toolchain.
    let swiftc = Command::new("xcrun").args(["--sdk", "macosx", "--find", "swiftc"]).output().expect("Swift toolchain");
    assert!(swiftc.status.success(), "Swift toolchain unavailable");
    let swiftc = PathBuf::from(String::from_utf8(swiftc.stdout).unwrap().trim());
    let runtime = swiftc.parent().unwrap().parent().unwrap().join("lib/swift/macosx");
    println!("cargo:rustc-link-search=native={}", runtime.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
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
    // The helper is `macos-helper/*.swift` plus `AppleFMCore.swift`, which the iOS plugin shares.
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let sources = [
        manifest.join("macos-helper/main.swift"),
        manifest.join("macos-helper/selftest.swift"),
        manifest.join("ios/Sources/AiwNativePlugin/AppleFMCore.swift"),
    ];
    // Built for macOS 26 itself: the app only runs it there (it checks the OS first), so
    // FoundationModels needs no weak linking.
    let mut args = vec!["--sdk", "macosx", "swiftc", "-O", "-target", "arm64-apple-macos26.0", "-module-name", "aiw_apple_fm", "-o"];
    args.push(exe.to_str().expect("utf-8 path"));
    args.extend(sources.iter().map(|s| s.to_str().expect("utf-8 path")));
    let built = xcrun(&args);
    match built {
        Ok(o) if o.status.success() => {
            println!("cargo:rustc-cfg=aiw_apple_fm_helper");
        }
        Ok(o) => skip(format!("swiftc failed: {}", String::from_utf8_lossy(&o.stderr).trim())),
        Err(e) => skip(format!("swiftc not available ({e})")),
    }
}
