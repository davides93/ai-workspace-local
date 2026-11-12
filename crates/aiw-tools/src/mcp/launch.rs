//! How an MCP stdio server is started: plain, or inside the sandbox of [`crate::sandbox`].
//!
//! The command line, the environment and the working folder are decided here, as a pure function of
//! the configuration, the isolation requested and the environment of the daemon, so that every
//! operating system's launcher can be tested on any machine. Also here: the approval hash of a
//! command (what the person confirmed before it ran for the first time).

use crate::sandbox::{self, SandboxSpec};
use aiw_core::api::McpServerConfig;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The isolation of one stdio server.
#[derive(Debug, Clone)]
pub struct StdioSandbox {
    /// What it may do (mode, network, hidden paths). `writable` is filled from `home`.
    pub spec: SandboxSpec,
    /// Private home folder of this server: writable, `HOME` points at it (package caches, state).
    pub home: PathBuf,
}

/// Variables of the daemon's environment a sandboxed server inherits. Everything else, secrets
/// included, is dropped: the server gets the variables the person configured for it.
const SAFE_BASE: &[&str] = &[
    "PATH",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "TERM",
    "USER",
    "LOGNAME",
    "SHELL",
    "TZ",
    "TMPDIR",
    "TMP",
    "TEMP",
    "SYSTEMROOT",
    "COMSPEC",
    "PATHEXT",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
];

fn safe_base(name: &str) -> bool {
    SAFE_BASE.iter().any(|b| b.eq_ignore_ascii_case(name))
}

/// What to spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StdioPlan {
    pub program: String,
    pub args: Vec<String>,
    /// Variables to set (after clearing the environment, when `clear_env`).
    pub env: Vec<(String, String)>,
    /// Start from an empty environment (sandboxed servers) instead of inheriting the daemon's.
    pub clear_env: bool,
    pub cwd: Option<PathBuf>,
}

/// The launcher of this machine (see [`sandbox::wrap`]).
pub type Wrap<'a> = &'a dyn Fn(&SandboxSpec, &Path, &str, &[String]) -> Option<(String, Vec<String>)>;

/// Builds the command of a stdio server. `wrap` is the sandbox launcher and `parent_env` the
/// daemon's environment (both parameters so that tests can supply their own).
pub fn plan_stdio(
    cfg: &McpServerConfig,
    command: &str,
    sandbox: Option<&StdioSandbox>,
    wrap: Wrap<'_>,
    parent_env: &[(String, String)],
) -> std::result::Result<StdioPlan, String> {
    let args = cfg.args.clone().unwrap_or_default();
    let configured: Vec<(String, String)> = cfg
        .env
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k, v.as_str().map(String::from).unwrap_or_else(|| v.to_string())))
        .collect();
    let Some(sb) = sandbox else {
        // Not isolated: as it always was, the environment of the daemon plus the configured variables.
        return Ok(StdioPlan { program: command.into(), args, env: configured, clear_env: false, cwd: None });
    };
    let mut spec = sb.spec.clone();
    spec.writable = vec![sb.home.clone()];
    let (program, args) = wrap(&spec, &spec.workspace.clone(), command, &args).ok_or_else(|| {
        "l'isolamento dei server MCP non è disponibile su questo sistema: scegli «Nessuno» per questo server, oppure installa bubblewrap (Linux)"
            .to_string()
    })?;
    let mut env: Vec<(String, String)> = parent_env.iter().filter(|(k, _)| safe_base(k)).cloned().collect();
    let home = sb.home.display().to_string();
    for (k, v) in [
        ("HOME", home.clone()),
        ("USERPROFILE", home.clone()),
        ("XDG_CACHE_HOME", format!("{home}/.cache")),
        ("XDG_CONFIG_HOME", format!("{home}/.config")),
        ("XDG_DATA_HOME", format!("{home}/.local/share")),
    ] {
        env.retain(|(x, _)| x != k);
        env.push((k.into(), v));
    }
    for (k, v) in configured {
        env.retain(|(x, _)| !x.eq_ignore_ascii_case(&k));
        env.push((k, v));
    }
    Ok(StdioPlan { program, args, env, clear_env: true, cwd: Some(spec.workspace.clone()) })
}

/// Hash of what a stdio server runs: the command, its arguments and the NAMES of its environment
/// variables (their values may be secrets and are shown masked). The person confirms this before the
/// server runs for the first time; changing any of it asks again. The isolation is not part of it:
/// tightening it must not ask again.
pub fn approval_hash(cfg: &McpServerConfig) -> String {
    let mut keys: Vec<&str> = cfg.env.iter().flat_map(|m| m.keys().map(String::as_str)).collect();
    keys.sort_unstable();
    keys.dedup();
    let canonical = serde_json::json!({
        "transport": cfg.transport,
        "command": cfg.command.as_deref().map(str::trim).unwrap_or_default(),
        "args": cfg.args.clone().unwrap_or_default(),
        "env_keys": keys,
    });
    let digest = Sha256::digest(serde_json::to_vec(&canonical).unwrap_or_default());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The command line as shown to the person: `npx -y @scope/server`.
pub fn command_line(cfg: &McpServerConfig) -> String {
    let mut parts = vec![cfg.command.clone().unwrap_or_default().trim().to_string()];
    parts.extend(cfg.args.clone().unwrap_or_default());
    parts
        .iter()
        .map(|p| if p.is_empty() || p.contains(char::is_whitespace) { format!("\"{p}\"") } else { p.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A sandbox specification for a server: `mode` is `read-only` or `workspace-write` (anything else, `off`
/// included, means no isolation and returns `None`).
pub fn stdio_sandbox(
    mode: &str,
    network: bool,
    workspace: &Path,
    home: PathBuf,
    shell: &crate::ShellSandbox,
    read_roots: &[PathBuf],
) -> Option<StdioSandbox> {
    let mode = match mode {
        "read-only" => crate::SandboxMode::ReadOnly,
        "workspace-write" => crate::SandboxMode::WorkspaceWrite,
        _ => return None,
    };
    Some(StdioSandbox {
        spec: SandboxSpec {
            mode,
            workspace: workspace.to_path_buf(),
            network,
            hidden: shell.hidden.clone(),
            readable: read_roots.to_vec(),
            writable: vec![home.clone()],
        },
        home,
    })
}

/// The real launcher of this machine.
pub fn native_wrap(spec: &SandboxSpec, cwd: &Path, program: &str, args: &[String]) -> Option<(String, Vec<String>)> {
    sandbox::wrap(spec, cwd, program, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::PathKind;
    use serde_json::{json, Map, Value};

    fn cfg(command: &str, args: &[&str], env: Value) -> McpServerConfig {
        McpServerConfig {
            id: "m1".into(),
            name: "fs".into(),
            transport: "stdio".into(),
            command: Some(command.into()),
            args: Some(args.iter().map(|s| s.to_string()).collect()),
            url: None,
            env: env.as_object().cloned().or(Some(Map::new())),
            headers: None,
            enabled: true,
            sampling: false,
            oauth_client_id: None,
            sandbox: "off".into(),
            sandbox_network: true,
        }
    }

    fn sb(mode: &str, network: bool) -> StdioSandbox {
        stdio_sandbox(
            mode,
            network,
            Path::new("/work/proj"),
            PathBuf::from("/home/u/.aiw/mcp-home/m1"),
            &crate::ShellSandbox { network: false, hidden: vec!["/home/u/.aiw".into(), "/home/u/.ssh".into()], env_allow: vec![] },
            &[PathBuf::from("/home/u/.aiw/skills")],
        )
        .unwrap()
    }

    fn parent() -> Vec<(String, String)> {
        [
            ("PATH", "/usr/bin"),
            ("HOME", "/home/u"),
            ("LANG", "it_IT.UTF-8"),
            ("TMPDIR", "/tmp"),
            ("GITHUB_TOKEN", "ghp_secret"),
            ("AIW_HOME", "/home/u/.aiw"),
            ("AWS_SECRET_ACCESS_KEY", "aws"),
            ("SSH_AUTH_SOCK", "/run/ssh"),
            ("HTTPS_PROXY", "http://proxy:3128"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    fn dirs_only(p: &Path) -> PathKind {
        if p.to_string_lossy().ends_with("mcp-home/m1") || p.to_string_lossy().ends_with("skills") || p.starts_with("/home/u/.") {
            PathKind::Dir
        } else {
            PathKind::Missing
        }
    }

    fn linux_wrap(spec: &SandboxSpec, cwd: &Path, program: &str, args: &[String]) -> Option<(String, Vec<String>)> {
        Some(("bwrap".into(), sandbox::bwrap_args(spec, cwd, program, args, &dirs_only)))
    }
    fn mac_wrap(spec: &SandboxSpec, _cwd: &Path, program: &str, args: &[String]) -> Option<(String, Vec<String>)> {
        Some(("/usr/bin/sandbox-exec".into(), sandbox::seatbelt_args(spec, program, args, &dirs_only)))
    }
    fn no_wrap(_: &SandboxSpec, _: &Path, _: &str, _: &[String]) -> Option<(String, Vec<String>)> {
        None
    }

    #[test]
    fn an_isolated_server_gets_a_safe_environment_and_its_own_variables() {
        let c = cfg("npx", &["-y", "@modelcontextprotocol/server-filesystem"], json!({"GITHUB_TOKEN": "from-the-person", "FOO": "1"}));
        let plan = plan_stdio(&c, "npx", Some(&sb("workspace-write", true)), &linux_wrap, &parent()).unwrap();
        assert!(plan.clear_env);
        let env: std::collections::HashMap<_, _> = plan.env.iter().cloned().collect();
        assert_eq!(env["PATH"], "/usr/bin");
        assert_eq!(env["LANG"], "it_IT.UTF-8");
        assert_eq!(env["HTTPS_PROXY"], "http://proxy:3128");
        // HOME is the private folder, never the real home.
        assert_eq!(env["HOME"], "/home/u/.aiw/mcp-home/m1");
        assert_eq!(env["XDG_CACHE_HOME"], "/home/u/.aiw/mcp-home/m1/.cache");
        // The daemon's secrets are not inherited; what the person configured is passed (even a name that looks secret).
        for gone in ["AIW_HOME", "AWS_SECRET_ACCESS_KEY", "SSH_AUTH_SOCK"] {
            assert!(!env.contains_key(gone), "{gone}");
        }
        assert_eq!(env["GITHUB_TOKEN"], "from-the-person");
        assert_eq!(env["FOO"], "1");
        assert_eq!(plan.cwd.as_deref(), Some(Path::new("/work/proj")));
    }

    #[test]
    fn a_server_that_is_not_isolated_keeps_the_old_behaviour() {
        let c = cfg("npx", &["-y", "x"], json!({"FOO": "1"}));
        let plan = plan_stdio(&c, "npx", None, &no_wrap, &parent()).unwrap();
        assert_eq!(
            plan,
            StdioPlan {
                program: "npx".into(),
                args: vec!["-y".into(), "x".into()],
                env: vec![("FOO".into(), "1".into())],
                clear_env: false,
                cwd: None
            }
        );
    }

    #[test]
    fn the_linux_launcher_wraps_the_server_in_bwrap() {
        let c = cfg("npx", &["-y", "srv"], json!({}));
        let plan = plan_stdio(&c, "npx", Some(&sb("workspace-write", true)), &linux_wrap, &parent()).unwrap();
        assert_eq!(plan.program, "bwrap");
        let a = &plan.args;
        assert!(a.iter().all(|x| x != "--unshare-net"), "the network stays on for a server that needs it");
        assert!(a.windows(2).any(|w| w == ["--tmpfs", "/home/u/.aiw"]), "{a:?}");
        assert!(a.windows(3).any(|w| w == ["--bind", "/home/u/.aiw/mcp-home/m1", "/home/u/.aiw/mcp-home/m1"]));
        assert!(a.windows(3).any(|w| w == ["--bind", "/work/proj", "/work/proj"]));
        assert_eq!(&a[a.len() - 4..], ["--", "npx", "-y", "srv"].map(String::from));
        let closed = plan_stdio(&c, "npx", Some(&sb("read-only", false)), &linux_wrap, &parent()).unwrap();
        assert!(closed.args.iter().any(|x| x == "--unshare-net"));
        assert!(!closed.args.windows(3).any(|w| w == ["--bind", "/work/proj", "/work/proj"]), "read-only: the workspace is not writable");
    }

    #[test]
    fn the_macos_launcher_wraps_the_server_in_sandbox_exec() {
        let c = cfg("uvx", &["srv"], json!({}));
        let plan = plan_stdio(&c, "uvx", Some(&sb("workspace-write", false)), &mac_wrap, &parent()).unwrap();
        assert_eq!(plan.program, "/usr/bin/sandbox-exec");
        assert_eq!(plan.args[0], "-p");
        let profile = &plan.args[1];
        assert!(profile.contains("(deny network*)"));
        assert!(profile.contains("(deny file-read* file-write* (subpath \"/home/u/.aiw\"))"));
        assert!(profile.contains("(allow file-read* file-write* (subpath \"/home/u/.aiw/mcp-home/m1\"))"));
        assert_eq!(&plan.args[2..], ["uvx", "srv"].map(String::from));
    }

    #[test]
    fn without_a_launcher_the_server_is_not_started_unisolated() {
        // Windows, or Linux without bubblewrap: failing closed beats running it as if it were isolated.
        let c = cfg("npx", &[], json!({}));
        let err = plan_stdio(&c, "npx", Some(&sb("workspace-write", true)), &no_wrap, &parent()).unwrap_err();
        assert!(err.contains("isolamento") && err.contains("Nessuno"), "{err}");
    }

    #[test]
    fn off_and_unknown_modes_mean_no_isolation() {
        let shell = crate::ShellSandbox::default();
        for m in ["off", "", "nonsense"] {
            assert!(stdio_sandbox(m, true, Path::new("/w"), PathBuf::from("/h"), &shell, &[]).is_none(), "{m:?}");
        }
        assert!(stdio_sandbox("read-only", true, Path::new("/w"), PathBuf::from("/h"), &shell, &[]).is_some());
    }

    #[test]
    fn the_approval_hash_covers_command_args_and_env_names_only() {
        let base = cfg("npx", &["-y", "srv"], json!({"TOKEN": "a", "B": "b"}));
        let h = approval_hash(&base);
        assert_eq!(h.len(), 64);
        // Values and key order do not matter; the isolation does not either.
        let mut same = cfg("npx", &["-y", "srv"], json!({"B": "other", "TOKEN": "••••••"}));
        same.sandbox = "workspace-write".into();
        same.name = "renamed".into();
        same.sandbox_network = false;
        same.enabled = false;
        assert_eq!(approval_hash(&same), h);
        assert_eq!(approval_hash(&cfg(" npx ", &["-y", "srv"], json!({"B": "", "TOKEN": ""}))), h, "the command is trimmed");
        // Anything that changes what runs does.
        for changed in [
            cfg("npx", &["-y", "srv2"], json!({"TOKEN": "a", "B": "b"})),
            cfg("npx", &["-y"], json!({"TOKEN": "a", "B": "b"})),
            cfg("node", &["-y", "srv"], json!({"TOKEN": "a", "B": "b"})),
            cfg("npx", &["-y", "srv"], json!({"TOKEN": "a"})),
            cfg("npx", &["-y", "srv"], json!({"TOKEN": "a", "B": "b", "NODE_OPTIONS": "--require x"})),
            cfg("npx", &["-y", "s rv"], json!({"TOKEN": "a", "B": "b"})),
            cfg("npx", &["-y srv"], json!({"TOKEN": "a", "B": "b"})),
        ] {
            assert_ne!(approval_hash(&changed), h, "{changed:?}");
        }
    }

    #[test]
    fn the_command_line_is_shown_readably() {
        assert_eq!(command_line(&cfg("npx", &["-y", "@scope/srv"], json!({}))), "npx -y @scope/srv");
        assert_eq!(command_line(&cfg("/opt/my tool/x", &["a b", ""], json!({}))), "\"/opt/my tool/x\" \"a b\" \"\"");
    }
}
