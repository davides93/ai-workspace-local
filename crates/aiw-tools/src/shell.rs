//! Shell tool with OS-level sandboxing (ADR 0005):
//! Linux → bubblewrap (read-only root, writable workspace, private /tmp, no network unless allowed,
//! the app's data folder and the secret locations hidden); macOS → sandbox-exec with the same
//! rules as a Seatbelt profile; Windows → Low integrity token + job object (crate
//! `aiw-winsandbox`): writes only in the workspace, but the network is NOT closed and nothing is
//! hidden from reading (those need an AppContainer, which is not implemented).
//! The launcher itself is in [`crate::sandbox`], shared with the MCP stdio servers.

use crate::sandbox::{self, SandboxSpec};
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
    sandbox::backend()
}

/// The sandbox specification of a shell command for this context.
pub fn sandbox_spec(ctx: &ToolContext, network: bool) -> SandboxSpec {
    SandboxSpec {
        mode: ctx.sandbox,
        workspace: ctx.workspace_root.clone(),
        network,
        hidden: ctx.shell.hidden.clone(),
        readable: ctx.read_roots.clone(),
        writable: vec![],
    }
}

/// Said at the end of a failed command that looks like it needed the network the sandbox closed.
pub const NETWORK_OFF_HINT: &str =
    "La rete della shell è spenta: per installare pacchetti o scaricare file attivala in Sicurezza → Sandbox";

/// Does the output of a failed command look like a blocked network (name resolution, unreachable
/// network, a refused connection to a non-local address)?
pub fn looks_like_blocked_network(output: &str) -> bool {
    let t = output.to_lowercase();
    const STRONG: [&str; 15] = [
        "could not resolve host",
        "temporary failure in name resolution",
        "name or service not known",
        "network is unreachable",
        "no address associated with hostname",
        "nodename nor servname provided",
        "getaddrinfo",
        "eai_again",
        "enotfound",
        "enetunreach",
        "failed to resolve",
        "could not resolve",
        "unable to resolve host",
        "name resolution",
        "dial tcp: lookup",
    ];
    if STRONG.iter().any(|p| t.contains(p)) {
        return true;
    }
    let refused = t.contains("connection refused") || t.contains("econnrefused");
    let local = ["localhost", "127.0.0.1", "[::1]", "::1"].iter().any(|l| t.contains(l));
    refused && !local
}

/// PowerShell writes to pipes in the OEM code page unless told otherwise.
const PS_UTF8: &str = "[Console]::OutputEncoding=[Text.Encoding]::UTF8;$OutputEncoding=[Text.Encoding]::UTF8;";

fn shell_program() -> (String, Vec<String>) {
    if cfg!(windows) {
        ("powershell".into(), vec!["-NoProfile".into(), "-NonInteractive".into(), "-Command".into()])
    } else {
        ("/bin/sh".into(), vec!["-c".into()])
    }
}

/// Full PowerShell command line for `CreateProcess`, with the script base64-encoded (UTF-16LE)
/// so that no quoting rule can split or reinterpret it.
#[cfg_attr(not(windows), allow(dead_code))]
fn powershell_command_line(command: &str) -> String {
    use base64::Engine;
    let script = format!("{PS_UTF8}{command}");
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    format!(
        "powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(utf16)
    )
}

/// Builds the argv that runs `command` in `cwd` under the sandbox of the context; the flag says
/// whether a sandbox was applied. `network` is what the command may use (already reduced by the
/// `sandbox.shell_network` setting).
pub fn build_command(command: &str, cwd: &Path, ctx: &ToolContext, network: bool) -> (String, Vec<String>, bool) {
    let (sh, mut sh_args) = shell_program();
    sh_args.push(if cfg!(windows) { format!("{PS_UTF8}{command}") } else { command.to_string() });
    if ctx.sandbox == SandboxMode::Off {
        return (sh, sh_args, false);
    }
    match sandbox::wrap(&sandbox_spec(ctx, network), cwd, &sh, &sh_args) {
        Some((prog, args)) => (prog, args, true),
        None => (sh, sh_args, false),
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
            "network":{"type":"boolean","description":"Solo per chiudere la rete a questo comando: se la rete della shell è spenta nelle impostazioni (default), resta spenta"}
        },"required":["command"]})
    }
    fn risk(&self) -> Risk {
        Risk::Exec
    }
    /// The command line, always: the generic subject takes the first of `path`, `command`… that is
    /// present, and a model could add a `path` argument to hide the command from the policy rules.
    fn subject(&self, args: &Value) -> Option<String> {
        args.get("command").and_then(|v| v.as_str()).map(String::from)
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
        // The setting decides; a call can only close the network further.
        let network = ctx.shell.network && args.get("network").and_then(|v| v.as_bool()).unwrap_or(true);
        #[cfg(windows)]
        {
            if ctx.sandbox != SandboxMode::Off {
                return run_windows_sandboxed(command, &cwd, ctx, timeout, network).await;
            }
        }
        let (prog, argv, sandboxed) = build_command(command, &cwd, ctx, network);
        let mut cmd = tokio::process::Command::new(&prog);
        // The command does not inherit the secrets of the daemon's environment.
        for name in sandbox::env_to_strip(std::env::vars().map(|(k, _)| k), &ctx.shell.env_allow) {
            cmd.env_remove(name);
        }
        let mut child = cmd
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
                let hint = if code != 0 && sandboxed && !network && looks_like_blocked_network(&out) {
                    format!("\n{NETWORK_OFF_HINT}")
                } else {
                    String::new()
                };
                let o = ToolOutput::text(format!("$ {command}\n{out}\n[exit {code}{note}]{hint}"));
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

#[cfg(windows)]
async fn run_windows_sandboxed(command: &str, cwd: &Path, ctx: &ToolContext, timeout: Duration, network: bool) -> Result<ToolOutput> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let access =
        if ctx.sandbox == SandboxMode::WorkspaceWrite { aiw_winsandbox::Access::WorkspaceWrite } else { aiw_winsandbox::Access::ReadOnly };
    let cancel = Arc::new(AtomicBool::new(false));
    let line = powershell_command_line(command);
    let (cwd, ws, flag) = (cwd.to_path_buf(), ctx.workspace_root.clone(), cancel.clone());
    let strip = sandbox::env_to_strip(std::env::vars().map(|(k, _)| k), &ctx.shell.env_allow);
    let job =
        tokio::task::spawn_blocking(move || aiw_winsandbox::run_clean(&line, &cwd, &ws, access, &[], &strip, timeout, &flag, 4 * MAX_OUT));
    let watch = {
        let token = ctx.cancel.clone();
        let flag = cancel.clone();
        tokio::spawn(async move {
            token.cancelled().await;
            flag.store(true, Ordering::Relaxed);
        })
    };
    let res = job.await.map_err(|e| Error::Runtime(e.to_string()))?;
    watch.abort();
    let o = res.map_err(|e| Error::Runtime(format!("sandbox Windows: {e}")))?;
    if o.cancelled {
        return Err(Error::Cancelled);
    }
    let out = String::from_utf8_lossy(&o.output).to_string();
    let out = truncate_middle(out.trim_end(), MAX_OUT);
    let note = if network { "" } else { " · su Windows la rete della shell non si può chiudere" };
    Ok(match o.exit_code {
        Some(code) => {
            let t = ToolOutput::text(format!("$ {command}\n{out}\n[exit {code}{note}]"));
            if code == 0 {
                t
            } else {
                ToolOutput { is_error: true, ..t }
            }
        }
        None => ToolOutput::error(format!("$ {command}\n{out}\n[timeout dopo {} s{note}]", timeout.as_secs())),
    })
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[tokio::test]
    async fn powershell_runs_in_the_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new("r", dir.path().to_path_buf());
        let out =
            ShellTool.call(json!({"command": "Set-Content -Path nota.txt -Value 'perché sì'; Get-Content nota.txt"}), &ctx).await.unwrap();
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("perché sì"), "{}", out.content);
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("x.txt");
        let cmd = format!("Set-Content -Path '{}' -Value no", target.display());
        let out = ShellTool.call(json!({"command": cmd}), &ctx).await.unwrap();
        assert!(out.is_error, "{}", out.content);
        assert!(!target.exists());
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

    /// Runs a trivial command under bubblewrap: false when it is missing or user namespaces are not allowed here.
    async fn bwrap_usable() -> bool {
        if sandbox_backend() != Some("bwrap") {
            return false;
        }
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new("r", dir.path().to_path_buf());
        let out = ShellTool.call(json!({"command":"echo usable"}), &ctx).await.unwrap();
        out.content.contains("usable") && !out.is_error
    }

    /// A folder outside `/tmp` (bubblewrap gives the command a private `/tmp`).
    fn outside_tmp() -> tempfile::TempDir {
        tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap()
    }

    #[tokio::test]
    async fn sandbox_hides_the_data_folder_and_the_deny_list() {
        if !bwrap_usable().await {
            eprintln!("skipped: bubblewrap is not usable here");
            return;
        }
        let base = outside_tmp();
        let data = base.path().join("data");
        std::fs::create_dir_all(data.join("skills")).unwrap();
        std::fs::create_dir_all(data.join("attachments/s1")).unwrap();
        std::fs::create_dir_all(data.join("attachments/s2")).unwrap();
        std::fs::write(data.join("token"), "TOP-SECRET").unwrap();
        std::fs::write(data.join("skills/s.md"), "SKILL-OK").unwrap();
        std::fs::write(data.join("attachments/s1/a.txt"), "ATT-OK").unwrap();
        std::fs::write(data.join("attachments/s2/b.txt"), "OTHER-SESSION").unwrap();
        let ssh = base.path().join("ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("id_ed25519"), "PRIVATE-KEY").unwrap();
        let netrc = base.path().join("netrc");
        std::fs::write(&netrc, "machine x password y").unwrap();
        let other = base.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("ok.txt"), "VISIBLE").unwrap();
        let ws = tempfile::tempdir().unwrap();
        let mut ctx = ToolContext::new("r", ws.path().to_path_buf());
        ctx.read_roots = vec![data.join("skills"), data.join("attachments/s1")];
        ctx.shell.hidden = vec![data.clone(), ssh.clone(), netrc.clone()];
        let run = |cmd: String| {
            let ctx = ctx.clone();
            async move { ShellTool.call(json!({"command": cmd}), &ctx).await.unwrap().content }
        };
        let out = run(format!("cat {}; echo; ls {}", data.join("token").display(), data.display())).await;
        assert!(!out.contains("TOP-SECRET"), "{out}");
        let out = run(format!("cat {}", ssh.join("id_ed25519").display())).await;
        assert!(!out.contains("PRIVATE-KEY"), "{out}");
        let out = run(format!("cat {}", netrc.display())).await;
        assert!(!out.contains("machine x"), "{out}");
        // Nothing can be written there either.
        let out = run(format!("echo x > {}/planted; echo $?", data.display())).await;
        assert!(!data.join("planted").exists(), "{out}");
        // The folders the agent needs stay readable (and read-only).
        let out = run(format!("cat {} {}", data.join("skills/s.md").display(), data.join("attachments/s1/a.txt").display())).await;
        assert!(out.contains("SKILL-OK") && out.contains("ATT-OK"), "{out}");
        let out = run(format!("cat {}", data.join("attachments/s2/b.txt").display())).await;
        assert!(!out.contains("OTHER-SESSION"), "the attachments of other sessions stay hidden: {out}");
        let out = run(format!("echo x > {}/skills/new; echo rc=$?", data.display())).await;
        assert!(!data.join("skills/new").exists(), "{out}");
        // Everything else is still readable.
        let out = run(format!("cat {}", other.join("ok.txt").display())).await;
        assert!(out.contains("VISIBLE"), "{out}");
        // The daemon cannot be reached through /proc.
        let out = run(format!("test -e /proc/{} && echo SEES-DAEMON || echo isolated", std::process::id())).await;
        assert!(out.contains("isolated"), "{out}");
    }

    #[tokio::test]
    async fn sandbox_network_is_closed_unless_the_setting_opens_it() {
        if !bwrap_usable().await {
            eprintln!("skipped: bubblewrap is not usable here");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ToolContext::new("r", dir.path().to_path_buf());
        let cmd = json!({"command": "tail -n +3 /proc/net/dev | wc -l"});
        let closed = ShellTool.call(cmd.clone(), &ctx).await.unwrap().content;
        assert!(closed.lines().any(|l| l.trim() == "1"), "only loopback is left: {closed}");
        // A call cannot open what the setting closed.
        let asked = ShellTool.call(json!({"command": "tail -n +3 /proc/net/dev | wc -l", "network": true}), &ctx).await.unwrap().content;
        assert!(asked.lines().any(|l| l.trim() == "1"), "{asked}");
        // Opened by the setting: the host's network namespace is shared (the count is the host's, at least loopback).
        ctx.shell.network = true;
        let open = ShellTool.call(cmd, &ctx).await.unwrap().content;
        let host = std::fs::read_to_string("/proc/net/dev").unwrap().lines().skip(2).count();
        assert!(open.lines().any(|l| l.trim() == host.to_string()), "{open} vs {host}");
    }

    #[tokio::test]
    async fn a_failed_command_that_needed_the_network_gets_a_hint() {
        if !bwrap_usable().await {
            eprintln!("skipped: bubblewrap is not usable here");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ToolContext::new("r", dir.path().to_path_buf());
        let cmd = json!({"command": "echo 'Could not resolve host: example.org' >&2; exit 6"});
        let out = ShellTool.call(cmd.clone(), &ctx).await.unwrap();
        assert!(out.is_error && out.content.trim_end().ends_with(NETWORK_OFF_HINT), "{}", out.content);
        // Not said when the network is on, or when the failure is something else.
        ctx.shell.network = true;
        assert!(!ShellTool.call(cmd, &ctx).await.unwrap().content.contains("La rete della shell"));
        ctx.shell.network = false;
        let other = ShellTool.call(json!({"command": "echo boom >&2; exit 1"}), &ctx).await.unwrap();
        assert!(!other.content.contains("La rete della shell"), "{}", other.content);
    }

    #[tokio::test]
    async fn the_daemons_secrets_are_not_inherited() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ToolContext::new("r", dir.path().to_path_buf());
        ctx.sandbox = SandboxMode::Off;
        std::env::set_var("AIW_TEST_API_TOKEN", "tok-1");
        std::env::set_var("AIW_TEST_KEEP_SECRET", "keep-1");
        std::env::set_var("AIW_TEST_PLAIN", "plain-1");
        let cmd = json!({"command": "echo \"[${AIW_TEST_API_TOKEN-gone}][${AIW_TEST_KEEP_SECRET-gone}][${AIW_TEST_PLAIN-gone}][${PATH:+path}][${HOME:+home}]\""});
        let out = ShellTool.call(cmd.clone(), &ctx).await.unwrap().content;
        // `AIW_*` is a secret prefix; PATH and HOME stay.
        assert!(out.contains("[gone][gone][gone][path]"), "{out}");
        ctx.shell.env_allow = vec!["AIW_TEST_KEEP_SECRET".into(), "aiw_test_plain".into()];
        let out = ShellTool.call(cmd, &ctx).await.unwrap().content;
        assert!(out.contains("[gone][keep-1][plain-1]"), "{out}");
    }

    #[test]
    fn the_subject_is_the_command_even_with_a_path_argument() {
        let args = json!({"command": "rm -rf x", "path": "notes.txt"});
        assert_eq!(ShellTool.subject(&args).as_deref(), Some("rm -rf x"));
    }

    #[tokio::test]
    async fn a_hidden_location_is_refused_by_the_file_tools_too() {
        let base = outside_tmp();
        let data = base.path().join("data");
        std::fs::create_dir_all(data.join("skills")).unwrap();
        std::fs::write(data.join("token"), "TOP-SECRET").unwrap();
        std::fs::write(data.join("skills/s.md"), "SKILL").unwrap();
        // The workspace is the parent of the data folder (the person works in their home folder).
        let mut ctx = ToolContext::new("r", base.path().to_path_buf());
        ctx.read_roots = vec![data.join("skills")];
        ctx.shell.hidden = vec![data.clone()];
        assert!(ctx.resolve(&data.join("token").display().to_string(), false).is_err());
        assert!(ctx.resolve(&data.join("token").display().to_string(), true).is_err());
        assert!(ctx.resolve(&data.display().to_string(), false).is_err());
        assert!(ctx.resolve(&data.join("skills/s.md").display().to_string(), false).is_ok());
        assert!(ctx.resolve("elsewhere.txt", false).is_ok());
        // With the sandbox off the person chose no confinement.
        ctx.sandbox = SandboxMode::Off;
        assert!(ctx.resolve(&data.join("token").display().to_string(), false).is_ok());
        // A workspace that lives inside the hidden folder (isolated session) stays usable.
        let mut ctx = ToolContext::new("r", data.join("skills"));
        ctx.shell.hidden = vec![data.clone()];
        assert!(ctx.resolve("s.md", false).is_ok());
        assert!(ctx.resolve(&data.join("token").display().to_string(), false).is_err());
    }

    #[tokio::test]
    async fn the_search_tools_skip_hidden_locations() {
        let base = outside_tmp();
        let data = base.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("token.txt"), "NEEDLE-SECRET").unwrap();
        std::fs::write(base.path().join("note.txt"), "NEEDLE-PLAIN").unwrap();
        let mut ctx = ToolContext::new("r", base.path().to_path_buf());
        ctx.shell.hidden = vec![data.clone()];
        let reg = crate::fs::tools();
        let grep = reg.iter().find(|t| t.name() == "grep").expect("grep tool");
        let out = grep.call(json!({"pattern": "NEEDLE"}), &ctx).await.unwrap().content;
        assert!(out.contains("NEEDLE-PLAIN") && !out.contains("NEEDLE-SECRET"), "{out}");
    }
}

#[cfg(test)]
mod pure_tests {
    use super::*;

    #[test]
    fn blocked_network_is_recognised_in_common_tool_messages() {
        for out in [
            "curl: (6) Could not resolve host: example.org",
            "ssh: Could not resolve hostname github.com: Temporary failure in name resolution",
            "ping: example.org: Name or service not known",
            "npm ERR! code EAI_AGAIN\nnpm ERR! errno EAI_AGAIN",
            "npm ERR! code ENOTFOUND",
            "pip._vendor.urllib3.exceptions.NewConnectionError: Failed to establish a new connection: [Errno 101] Network is unreachable",
            "fatal: unable to access 'https://github.com/x/y.git/': Could not resolve host: github.com",
            "curl: (7) Failed to connect to example.org port 443 after 3 ms: Connection refused",
            "Error: getaddrinfo EAI_AGAIN registry.npmjs.org",
            "dial tcp: lookup proxy.golang.org: no such host",
            "wget: unable to resolve host address 'x'",
            "fatal: Network is unreachable",
        ] {
            assert!(looks_like_blocked_network(out), "{out}");
        }
        for out in [
            "curl: (7) Failed to connect to localhost port 8080 after 0 ms: Connection refused",
            "Error: connect ECONNREFUSED 127.0.0.1:5432",
            "error: expected `;`",
            "ls: cannot access 'x': No such file or directory",
            "permission denied",
        ] {
            assert!(!looks_like_blocked_network(out), "{out}");
        }
    }
}
