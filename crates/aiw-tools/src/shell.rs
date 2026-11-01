//! Shell tool with OS-level sandboxing (ADR 0005):
//! Linux → bubblewrap (read-only root, writable workspace, private /tmp, optional no-network);
//! macOS → sandbox-exec profile denying writes outside the workspace; Windows → no sandbox yet
//! (the policy engine keeps asking for confirmation there).

use crate::{arg_str, truncate_middle, Risk, SandboxMode, Tool, ToolContext, ToolOutput};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncReadExt;

pub struct ShellTool;

const MAX_OUT: usize = 30_000;

/// Which sandbox this machine can apply.
pub fn sandbox_backend() -> Option<&'static str> {
    if cfg!(target_os = "linux") && which::which("bwrap").is_ok() {
        Some("bwrap")
    } else if cfg!(target_os = "macos") && Path::new("/usr/bin/sandbox-exec").exists() {
        Some("sandbox-exec")
    } else {
        None
    }
}

fn shell_program() -> (String, Vec<String>) {
    if cfg!(windows) {
        ("powershell".into(), vec!["-NoProfile".into(), "-NonInteractive".into(), "-Command".into()])
    } else {
        ("/bin/sh".into(), vec!["-c".into()])
    }
}

/// Builds the argv that runs `command` in `cwd` under the requested sandbox.
pub fn build_command(command: &str, cwd: &Path, workspace: &Path, mode: SandboxMode, allow_network: bool) -> (String, Vec<String>, bool) {
    let (sh, mut sh_args) = shell_program();
    sh_args.push(command.to_string());
    if mode == SandboxMode::Off {
        return (sh, sh_args, false);
    }
    match sandbox_backend() {
        Some("bwrap") => {
            let ws = workspace.display().to_string();
            let mut a: Vec<String> = ["--die-with-parent", "--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            if mode == SandboxMode::WorkspaceWrite {
                a.extend(["--bind".into(), ws.clone(), ws]);
            }
            if !allow_network {
                a.push("--unshare-net".into());
            }
            a.extend(["--chdir".into(), cwd.display().to_string(), "--".into(), sh]);
            a.extend(sh_args);
            ("bwrap".into(), a, true)
        }
        Some("sandbox-exec") => {
            let ws = workspace.display().to_string().replace('"', "");
            let write_rule =
                if mode == SandboxMode::WorkspaceWrite { format!("(allow file-write* (subpath \"{ws}\"))") } else { String::new() };
            let net_rule = if allow_network { "" } else { "(deny network*)" };
            let profile = format!(
                "(version 1)(allow default)(deny file-write*){write_rule}(allow file-write* (subpath \"/private/tmp\") (subpath \"/private/var/folders\") (literal \"/dev/null\") (literal \"/dev/tty\")){net_rule}"
            );
            let mut a = vec!["-p".to_string(), profile, sh];
            a.extend(sh_args);
            ("/usr/bin/sandbox-exec".into(), a, true)
        }
        _ => (sh, sh_args, false),
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }
    fn description(&self) -> &str {
        "Esegue un comando di shell nel workspace (sandbox del sistema operativo quando disponibile). Restituisce output ed exit code."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{
            "command":{"type":"string"},
            "cwd":{"type":"string","description":"Cartella di lavoro (default: radice del workspace)"},
            "timeout_s":{"type":"integer","description":"Default 120, massimo 1800"},
            "network":{"type":"boolean","description":"Consente la rete al comando (default true)"}
        },"required":["command"]})
    }
    fn risk(&self) -> Risk {
        Risk::Exec
    }
    fn available(&self) -> std::result::Result<(), String> {
        if cfg!(target_os = "ios") {
            Err("iOS non consente di eseguire processi".into())
        } else {
            Ok(())
        }
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let command = arg_str(&args, "command")?;
        let cwd = ctx.resolve(args.get("cwd").and_then(|v| v.as_str()).unwrap_or("."), false)?;
        if !cwd.is_dir() {
            return Err(Error::Invalid(format!("{} non è una cartella", cwd.display())));
        }
        let timeout = Duration::from_secs(args.get("timeout_s").and_then(|v| v.as_u64()).unwrap_or(120).clamp(1, 1800));
        let network = args.get("network").and_then(|v| v.as_bool()).unwrap_or(true);
        let (prog, argv, sandboxed) = build_command(command, &cwd, &ctx.workspace_root, ctx.sandbox, network);
        let mut child = tokio::process::Command::new(&prog)
            .args(&argv)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| Error::Runtime(format!("impossibile avviare {prog}: {e}")))?;
        let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let mut readers = Vec::new();
        for mut pipe in [
            child.stdout.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            child.stderr.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let buf = buf.clone();
            readers.push(tokio::spawn(async move {
                let mut chunk = [0u8; 8192];
                while let Ok(n) = pipe.read(&mut chunk).await {
                    if n == 0 {
                        break;
                    }
                    let mut b = buf.lock().unwrap();
                    if b.len() < 4 * MAX_OUT {
                        b.extend_from_slice(&chunk[..n]);
                    }
                }
            }));
        }
        let status = tokio::select! {
            s = child.wait() => Some(s?),
            _ = tokio::time::sleep(timeout) => { let _ = child.kill().await; None }
            _ = ctx.cancel.cancelled() => { let _ = child.kill().await; return Err(Error::Cancelled); }
        };
        for r in readers {
            let _ = tokio::time::timeout(Duration::from_secs(2), r).await;
        }
        let out = String::from_utf8_lossy(&buf.lock().unwrap()).to_string();
        let out = truncate_middle(out.trim_end(), MAX_OUT);
        let note = if sandboxed {
            ""
        } else if ctx.sandbox == SandboxMode::Off {
            " · sandbox disattivata"
        } else {
            " · sandbox non disponibile su questo sistema"
        };
        Ok(match status {
            Some(s) => {
                let code = s.code().unwrap_or(-1);
                let o = ToolOutput::text(format!("$ {command}\n{out}\n[exit {code}{note}]"));
                if code == 0 {
                    o
                } else {
                    ToolOutput { is_error: true, ..o }
                }
            }
            None => ToolOutput::error(format!("$ {command}\n{out}\n[timeout dopo {} s{note}]", timeout.as_secs())),
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runs_commands_and_reports_exit_codes() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ToolContext::new("r", dir.path().to_path_buf());
        ctx.sandbox = SandboxMode::Off;
        let out = ShellTool.call(json!({"command":"echo hello && echo err 1>&2"}), &ctx).await.unwrap();
        assert!(out.content.contains("hello") && out.content.contains("err") && out.content.contains("[exit 0"), "{}", out.content);
        let out = ShellTool.call(json!({"command":"exit 3"}), &ctx).await.unwrap();
        assert!(out.is_error && out.content.contains("[exit 3"));
        let out = ShellTool.call(json!({"command":"sleep 5","timeout_s":1}), &ctx).await.unwrap();
        assert!(out.content.contains("timeout"));
    }

    #[tokio::test]
    async fn sandbox_blocks_writes_outside_workspace() {
        if sandbox_backend() != Some("bwrap") {
            return; // only verifiable where bubblewrap exists
        }
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new("r", dir.path().to_path_buf());
        let inside = ShellTool.call(json!({"command":"echo ok > inside.txt && cat inside.txt"}), &ctx).await.unwrap();
        // bwrap may be unusable inside some containers (no user namespaces): accept that case.
        if inside.content.contains("Operation not permitted")
            || inside.content.contains("No permissions")
            || inside.content.contains("setting up uid map")
        {
            return;
        }
        assert!(inside.content.contains("ok"), "{}", inside.content);
        let target = outside.path().join("x.txt");
        let out = ShellTool.call(json!({"command": format!("echo no > {}", target.display())}), &ctx).await.unwrap();
        assert!(out.is_error, "{}", out.content);
        assert!(!target.exists());
    }
}
