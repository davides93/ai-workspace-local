//! Runs on Windows only (CI: core-windows job): real processes, real integrity labels.
#![cfg(windows)]

use aiw_winsandbox::{has_low_label, run, run_with, Access, Hardening, Outcome};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn cmd(c: &str, ws: &Path, access: Access, timeout: Duration) -> Outcome {
    run(&format!("cmd.exe /D /S /C \"{c}\""), ws, ws, access, &[], timeout, &AtomicBool::new(false), 1 << 20).expect("sandbox run")
}

fn text(o: &Outcome) -> String {
    String::from_utf8_lossy(&o.output).into_owned()
}

/// Which protection breaks process start-up on this machine (shown when a test fails).
fn diagnose(ws: &Path) -> String {
    let mut out = String::from("diagnosi (low_integrity, deny_admins, own_desktop) -> exit:\n");
    for bits in 0..8u8 {
        let h = Hardening { low_integrity: bits & 1 != 0, deny_admins: bits & 2 != 0, own_desktop: bits & 4 != 0 };
        let r = run_with(
            "cmd.exe /D /S /C \"echo ok\"",
            ws,
            ws,
            Access::ReadOnly,
            &[],
            Duration::from_secs(20),
            &AtomicBool::new(false),
            4096,
            h,
        );
        let res = match r {
            Ok(o) => format!("{:?} {:?}", o.exit_code.map(|c| format!("{c:#x}")), text(&o).trim()),
            Err(e) => format!("errore: {e}"),
        };
        out.push_str(&format!("  ({}, {}, {}) -> {res}\n", h.low_integrity, h.deny_admins, h.own_desktop));
    }
    out
}

#[test]
fn runs_at_low_integrity_and_reports_exit_codes() {
    let ws = tempfile::tempdir().unwrap();
    let o = cmd("whoami /groups", ws.path(), Access::ReadOnly, Duration::from_secs(60));
    assert_eq!(o.exit_code, Some(0), "{}\n{}", text(&o), diagnose(ws.path()));
    assert!(text(&o).contains("S-1-16-4096"), "not at Low integrity:\n{}", text(&o));
    let o = cmd("echo ciao & exit /b 3", ws.path(), Access::ReadOnly, Duration::from_secs(60));
    assert_eq!(o.exit_code, Some(3));
    assert!(text(&o).contains("ciao"));
}

#[test]
fn writes_only_inside_the_workspace() {
    let ws = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let o = cmd("echo dentro> dentro.txt && type dentro.txt", ws.path(), Access::WorkspaceWrite, Duration::from_secs(60));
    assert_eq!(o.exit_code, Some(0), "{}", text(&o));
    assert!(ws.path().join("dentro.txt").exists());
    assert!(has_low_label(ws.path()));

    let target = outside.path().join("fuori.txt");
    let o = cmd(&format!("echo fuori> \"{}\"", target.display()), ws.path(), Access::WorkspaceWrite, Duration::from_secs(60));
    assert_ne!(o.exit_code, Some(0), "{}", text(&o));
    assert!(!target.exists(), "wrote outside the workspace");

    // Read-only: an unlabelled workspace stays untouched.
    let ro = tempfile::tempdir().unwrap();
    let o = cmd("echo x> ro.txt", ro.path(), Access::ReadOnly, Duration::from_secs(60));
    assert_ne!(o.exit_code, Some(0), "{}", text(&o));
    assert!(!ro.path().join("ro.txt").exists());
}

#[test]
fn timeout_kills_the_process_tree() {
    let ws = tempfile::tempdir().unwrap();
    let t = Instant::now();
    let o = cmd("ping -n 60 127.0.0.1 >NUL", ws.path(), Access::ReadOnly, Duration::from_secs(2));
    assert!(o.timed_out);
    assert_eq!(o.exit_code, None);
    assert!(t.elapsed() < Duration::from_secs(20), "took {:?}", t.elapsed());
}
