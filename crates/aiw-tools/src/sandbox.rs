//! OS sandbox launcher shared by the shell tool and the MCP stdio servers (ADR 0005).
//!
//! * Linux: bubblewrap. Read-only root, private `/tmp`, own PID namespace (the host's `/proc` would
//!   let the command reach the daemon through `/proc/<pid>/root`), optional `--unshare-net`, the
//!   workspace writable in `workspace-write`, and a tmpfs (or `/dev/null` for a file) over every
//!   hidden path.
//! * macOS: `sandbox-exec` with a Seatbelt profile (writes denied outside the workspace, `(deny
//!   network*)`, `file-read*`/`file-write*` denied on the hidden paths). Seatbelt applies the last
//!   matching rule, so exceptions come after the denials. This profile cannot be executed on the
//!   other systems: its text is unit-tested, its behaviour is not.
//! * Windows: not here. The shell tool uses the low-integrity token of `aiw-winsandbox`, which
//!   cannot hide a folder or close the network, and a long-lived stdio server cannot be started
//!   through it; [`wrap`] returns `None`.
//!
//! The argument builders are pure functions of the specification and of a file-system probe, so
//! the exact command lines are tested on every system.

use crate::SandboxMode;
use std::path::{Component, Path, PathBuf};

/// Which sandbox this machine can apply.
pub fn backend() -> Option<&'static str> {
    if cfg!(target_os = "linux") && which::which("bwrap").is_ok() {
        Some("bwrap")
    } else if cfg!(target_os = "macos") && Path::new("/usr/bin/sandbox-exec").exists() {
        Some("sandbox-exec")
    } else if cfg!(windows) {
        Some("windows-low-integrity")
    } else {
        None
    }
}

/// Can [`wrap`] contain a long-lived child process (an MCP stdio server) on this machine?
pub fn can_wrap_process() -> bool {
    matches!(backend(), Some("bwrap" | "sandbox-exec"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    Dir,
    File,
    Missing,
}

pub fn path_kind(p: &Path) -> PathKind {
    match std::fs::metadata(p) {
        Ok(m) if m.is_dir() => PathKind::Dir,
        Ok(_) => PathKind::File,
        Err(_) => PathKind::Missing,
    }
}

/// What the contained process may do.
#[derive(Debug, Clone)]
pub struct SandboxSpec {
    /// `ReadOnly` or `WorkspaceWrite` (`Off` is not contained at all).
    pub mode: SandboxMode,
    pub workspace: PathBuf,
    /// The process may use the network.
    pub network: bool,
    /// Files and folders it can neither read nor write (missing ones are skipped).
    pub hidden: Vec<PathBuf>,
    /// Read-only exceptions: parts of a hidden folder it may read (skills, attachments).
    pub readable: Vec<PathBuf>,
    /// Extra folders it may write (the private home of an MCP server).
    pub writable: Vec<PathBuf>,
}

impl SandboxSpec {
    fn write_mode(&self) -> bool {
        self.mode == SandboxMode::WorkspaceWrite
    }
    /// Hidden folders that contain the workspace (a session in `~/.aiw/worktrees`): the workspace is
    /// bound back on top of them.
    fn workspace_inside_hidden(&self, hidden: &[PathBuf]) -> bool {
        hidden.iter().any(|h| self.workspace.starts_with(h))
    }
}

/// The paths of the specification that exist, as given and canonical, without repetitions.
fn existing(paths: &[PathBuf], kind: &dyn Fn(&Path) -> PathKind) -> Vec<(PathBuf, PathKind)> {
    let mut out: Vec<(PathBuf, PathKind)> = vec![];
    for p in paths {
        if !p.has_root() || p.components().filter(|c| matches!(c, Component::Normal(_))).count() == 0 {
            continue; // never `/`, never a relative path
        }
        let k = kind(p);
        if k == PathKind::Missing {
            continue;
        }
        let canon = std::fs::canonicalize(p).ok().filter(|c| c != p);
        for q in std::iter::once(p.clone()).chain(canon) {
            if !out.iter().any(|(o, _)| o == &q) {
                out.push((q, k));
            }
        }
    }
    out
}

fn s(p: &Path) -> String {
    p.display().to_string()
}

/// The `bwrap` arguments, up to and including the program to run.
pub fn bwrap_args(spec: &SandboxSpec, cwd: &Path, program: &str, args: &[String], kind: &dyn Fn(&Path) -> PathKind) -> Vec<String> {
    let mut a: Vec<String> = [
        "--die-with-parent",
        "--new-session",
        "--unshare-pid",
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--tmpfs",
        "/tmp",
    ]
    .iter()
    .map(|x| x.to_string())
    .collect();
    if !spec.network {
        a.push("--unshare-net".into());
    }
    let hidden = existing(&spec.hidden, kind);
    let hidden_paths: Vec<PathBuf> = hidden.iter().map(|(p, _)| p.clone()).collect();
    let ws = spec.workspace.clone();
    let ws_inside = spec.workspace_inside_hidden(&hidden_paths);
    let bind_ws = |a: &mut Vec<String>| {
        a.extend([if spec.write_mode() { "--bind" } else { "--ro-bind" }.to_string(), s(&ws), s(&ws)]);
    };
    // Read-only too: the workspace may be under `/tmp`, which is a private tmpfs for the process.
    if !ws_inside {
        bind_ws(&mut a);
    }
    for (p, k) in &hidden {
        match k {
            PathKind::Dir => a.extend(["--tmpfs".into(), s(p)]),
            _ => a.extend(["--ro-bind".into(), "/dev/null".into(), s(p)]),
        }
    }
    for (r, _) in existing(&spec.readable, kind) {
        if hidden_paths.iter().any(|h| r.starts_with(h) && &r != h) {
            a.extend(["--ro-bind".into(), s(&r), s(&r)]);
        }
    }
    if ws_inside {
        bind_ws(&mut a);
    }
    for (w, k) in existing(&spec.writable, kind) {
        if k == PathKind::Dir {
            a.extend(["--bind".into(), s(&w), s(&w)]);
        }
    }
    a.extend(["--chdir".into(), s(cwd), "--".into(), program.to_string()]);
    a.extend(args.iter().cloned());
    a
}

/// A string literal of the Seatbelt language.
fn sbpl_quote(p: &Path) -> Option<String> {
    let t = p.display().to_string();
    if t.contains(['\n', '\r', '\0']) {
        return None;
    }
    Some(format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\"")))
}

fn sbpl_filter(p: &Path, kind: PathKind) -> Option<String> {
    let q = sbpl_quote(p)?;
    Some(if kind == PathKind::Dir { format!("(subpath {q})") } else { format!("(literal {q})") })
}

/// The Seatbelt profile.
pub fn seatbelt_profile(spec: &SandboxSpec, kind: &dyn Fn(&Path) -> PathKind) -> String {
    let mut p = String::from("(version 1)(allow default)(deny file-write*)");
    let writable_dir = |p: &Path| sbpl_quote(p).map(|q| format!("(subpath {q})"));
    let mut ws_paths = vec![spec.workspace.clone()];
    ws_paths.extend(std::fs::canonicalize(&spec.workspace).ok().filter(|c| c != &spec.workspace));
    if spec.write_mode() {
        for w in &ws_paths {
            if let Some(f) = writable_dir(w) {
                p.push_str(&format!("(allow file-write* {f})"));
            }
        }
    }
    p.push_str(
        "(allow file-write* (subpath \"/private/tmp\") (subpath \"/private/var/folders\") (literal \"/dev/null\") (literal \"/dev/tty\"))",
    );
    for (w, k) in existing(&spec.writable, kind) {
        if k == PathKind::Dir {
            if let Some(f) = writable_dir(&w) {
                p.push_str(&format!("(allow file-write* {f})"));
            }
        }
    }
    if !spec.network {
        p.push_str("(deny network*)");
    }
    let hidden = existing(&spec.hidden, kind);
    for (h, k) in &hidden {
        if let Some(f) = sbpl_filter(h, *k) {
            p.push_str(&format!("(deny file-read* file-write* {f})"));
        }
    }
    // Exceptions come after the denials: the last matching rule wins.
    for (r, k) in existing(&spec.readable, kind) {
        if hidden.iter().any(|(h, _)| r.starts_with(h) && &r != h) {
            if let Some(f) = sbpl_filter(&r, k) {
                p.push_str(&format!("(allow file-read* {f})"));
            }
        }
    }
    if hidden.iter().any(|(h, _)| spec.workspace.starts_with(h)) {
        for w in &ws_paths {
            if let Some(f) = writable_dir(w) {
                p.push_str(&format!("(allow file-read* {f})"));
                if spec.write_mode() {
                    p.push_str(&format!("(allow file-write* {f})"));
                }
            }
        }
    }
    for (w, k) in existing(&spec.writable, kind) {
        if k == PathKind::Dir {
            if let Some(f) = writable_dir(&w) {
                p.push_str(&format!("(allow file-read* file-write* {f})"));
            }
        }
    }
    p
}

/// `sandbox-exec` arguments, up to and including the program to run.
pub fn seatbelt_args(spec: &SandboxSpec, program: &str, args: &[String], kind: &dyn Fn(&Path) -> PathKind) -> Vec<String> {
    let mut a = vec!["-p".to_string(), seatbelt_profile(spec, kind), program.to_string()];
    a.extend(args.iter().cloned());
    a
}

/// Wraps `program args…` for the sandbox of this machine; `None` when there is none that can contain a
/// child process (Windows, a Linux without bubblewrap, Android).
pub fn wrap(spec: &SandboxSpec, cwd: &Path, program: &str, args: &[String]) -> Option<(String, Vec<String>)> {
    match backend() {
        Some("bwrap") => Some(("bwrap".into(), bwrap_args(spec, cwd, program, args, &path_kind))),
        Some("sandbox-exec") => Some(("/usr/bin/sandbox-exec".into(), seatbelt_args(spec, program, args, &path_kind))),
        _ => None,
    }
}

// ------------------------------------------------------------------ environment

/// Name patterns of environment variables that carry secrets.
pub fn is_secret_env(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    if n == "PWD" || n == "OLDPWD" {
        return false;
    }
    const PREFIXES: [&str; 9] = ["AIW_", "AWS_", "AZURE_", "GOOGLE_", "GCP_", "GCLOUD_", "CLOUDSDK_", "OPENAI_", "ANTHROPIC_"];
    const EXACT: [&str; 8] =
        ["SSH_AUTH_SOCK", "SSH_AGENT_PID", "GPG_AGENT_INFO", "KUBECONFIG", "DATABASE_URL", "NETRC", "DOCKER_CONFIG", "DOCKER_AUTH_CONFIG"];
    const CONTAINS: [&str; 5] = ["PASSWORD", "PASSWD", "SECRET", "TOKEN", "CREDENTIAL"];
    const SUFFIXES: [&str; 5] = ["_KEY", "_KEYS", "APIKEY", "_PRIVATE_KEY", "_PWD"];
    PREFIXES.iter().any(|p| n.starts_with(p))
        || EXACT.contains(&n.as_str())
        || CONTAINS.iter().any(|c| n.contains(c))
        || SUFFIXES.iter().any(|x| n.ends_with(x))
}

/// Does `name` match an entry of `sandbox.env_allow` (an exact name or a prefix ending in `*`)?
pub fn env_allowed(name: &str, allow: &[String]) -> bool {
    allow.iter().any(|a| {
        let a = a.trim();
        match a.strip_suffix('*') {
            Some(prefix) => !prefix.is_empty() && name.to_ascii_uppercase().starts_with(&prefix.to_ascii_uppercase()),
            None => !a.is_empty() && name.eq_ignore_ascii_case(a),
        }
    })
}

/// The names to remove from the environment a child would inherit.
pub fn env_to_strip<I: IntoIterator<Item = String>>(names: I, allow: &[String]) -> Vec<String> {
    names.into_iter().filter(|n| is_secret_env(n) && !env_allowed(n, allow)).collect()
}

/// The variables of the daemon's environment a sandboxed child keeps.
pub fn clean_environment(allow: &[String]) -> Vec<(String, String)> {
    std::env::vars().filter(|(k, _)| !is_secret_env(k) || env_allowed(k, allow)).collect()
}

// ------------------------------------------------------------------ hidden paths

pub use aiw_core::config::DEFAULT_DENY_READ;

/// Turns the entries of `sandbox.deny_read` into absolute paths: `~` is expanded, an entry that is not
/// absolute afterwards is dropped (it would depend on the working folder).
pub fn expand_hidden(entries: &[String]) -> Vec<PathBuf> {
    entries
        .iter()
        .map(|e| aiw_core::paths::expand_home(e.trim()))
        .filter(|p| !p.as_os_str().is_empty() && p.is_absolute())
        .map(|p| aiw_core::paths::normalize(&p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(mode: SandboxMode, network: bool) -> SandboxSpec {
        SandboxSpec {
            mode,
            workspace: "/work/proj".into(),
            network,
            hidden: vec!["/home/u/.aiw".into(), "/home/u/.ssh".into(), "/home/u/.netrc".into(), "/home/u/.missing".into()],
            readable: vec!["/home/u/.aiw/skills".into(), "/home/u/.aiw/attachments/s1".into(), "/home/u/other".into()],
            writable: vec![],
        }
    }

    /// Everything but `.missing` exists; `.netrc` is a file.
    fn fs(p: &Path) -> PathKind {
        let t = p.to_string_lossy();
        if t.ends_with(".missing") || t == "/home/u/other" {
            PathKind::Missing
        } else if t.ends_with(".netrc") {
            PathKind::File
        } else {
            PathKind::Dir
        }
    }

    fn pos(a: &[String], needle: &[&str]) -> Option<usize> {
        a.windows(needle.len()).position(|w| w.iter().zip(needle).all(|(x, y)| x == y))
    }

    #[test]
    fn bwrap_closes_the_network_hides_paths_and_keeps_the_program_last() {
        let sh = vec!["-c".to_string(), "echo hi".to_string()];
        let a = bwrap_args(&spec(SandboxMode::WorkspaceWrite, false), Path::new("/work/proj/sub"), "/bin/sh", &sh, &fs);
        for flag in ["--die-with-parent", "--new-session", "--unshare-pid", "--unshare-net"] {
            assert!(a.iter().any(|x| x == flag), "{flag} in {a:?}");
        }
        assert!(pos(&a, &["--ro-bind", "/", "/"]).is_some());
        assert!(pos(&a, &["--bind", "/work/proj", "/work/proj"]).is_some());
        assert!(pos(&a, &["--tmpfs", "/home/u/.aiw"]).is_some());
        assert!(pos(&a, &["--tmpfs", "/home/u/.ssh"]).is_some());
        assert!(pos(&a, &["--ro-bind", "/dev/null", "/home/u/.netrc"]).is_some(), "a file is covered with /dev/null");
        assert!(pos(&a, &["--tmpfs", "/home/u/.missing"]).is_none(), "missing paths are skipped");
        // The exceptions come after the tmpfs they punch through; a missing one is skipped.
        let hide = pos(&a, &["--tmpfs", "/home/u/.aiw"]).unwrap();
        let skills = pos(&a, &["--ro-bind", "/home/u/.aiw/skills", "/home/u/.aiw/skills"]).unwrap();
        assert!(skills > hide);
        assert!(pos(&a, &["--ro-bind", "/home/u/.aiw/attachments/s1", "/home/u/.aiw/attachments/s1"]).is_some());
        assert!(a.iter().all(|x| x != "/home/u/other"));
        let tail = &a[a.len() - 6..];
        assert_eq!(tail, ["--chdir", "/work/proj/sub", "--", "/bin/sh", "-c", "echo hi"].map(String::from));
    }

    #[test]
    fn bwrap_with_network_and_read_only_workspace() {
        let a = bwrap_args(&spec(SandboxMode::ReadOnly, true), Path::new("/work/proj"), "x", &[], &fs);
        assert!(a.iter().all(|x| x != "--unshare-net"));
        assert!(a.iter().all(|x| x != "--bind"), "nothing is writable: {a:?}");
        assert!(pos(&a, &["--ro-bind", "/work/proj", "/work/proj"]).is_some(), "the workspace stays visible, read-only: {a:?}");
    }

    #[test]
    fn bwrap_binds_a_workspace_inside_a_hidden_folder_back_on_top() {
        let mut sp = spec(SandboxMode::WorkspaceWrite, false);
        sp.workspace = "/home/u/.aiw/worktrees/abc".into();
        let a = bwrap_args(&sp, &sp.workspace.clone(), "x", &[], &fs);
        let hide = pos(&a, &["--tmpfs", "/home/u/.aiw"]).unwrap();
        let ws = pos(&a, &["--bind", "/home/u/.aiw/worktrees/abc", "/home/u/.aiw/worktrees/abc"]).unwrap();
        assert!(ws > hide, "the workspace must be mounted after the tmpfs: {a:?}");
        assert_eq!(a.iter().filter(|x| *x == "--bind").count(), 1);
    }

    #[test]
    fn bwrap_hides_inside_a_workspace_that_contains_them() {
        let mut sp = spec(SandboxMode::WorkspaceWrite, false);
        sp.workspace = "/home/u".into();
        let a = bwrap_args(&sp, &sp.workspace.clone(), "x", &[], &fs);
        let ws = pos(&a, &["--bind", "/home/u", "/home/u"]).unwrap();
        assert!(ws < pos(&a, &["--tmpfs", "/home/u/.ssh"]).unwrap(), "the hiding is applied after the workspace bind");
    }

    #[test]
    fn bwrap_never_hides_the_root_or_relative_paths() {
        let mut sp = spec(SandboxMode::ReadOnly, true);
        sp.hidden = vec!["/".into(), "relative/dir".into(), "".into()];
        let a = bwrap_args(&sp, Path::new("/work"), "x", &[], &|_| PathKind::Dir);
        assert_eq!(a.iter().filter(|x| *x == "--tmpfs").count(), 1, "only /tmp: {a:?}");
    }

    #[test]
    fn bwrap_binds_the_private_home_of_a_server() {
        let mut sp = spec(SandboxMode::WorkspaceWrite, true);
        sp.writable = vec!["/home/u/.aiw/mcp/m1/home".into()];
        let a = bwrap_args(&sp, Path::new("/work/proj"), "npx", &[], &fs);
        let hide = pos(&a, &["--tmpfs", "/home/u/.aiw"]).unwrap();
        let home = pos(&a, &["--bind", "/home/u/.aiw/mcp/m1/home", "/home/u/.aiw/mcp/m1/home"]).unwrap();
        assert!(home > hide);
    }

    #[test]
    fn seatbelt_profile_text() {
        let p = seatbelt_profile(&spec(SandboxMode::WorkspaceWrite, false), &fs);
        assert!(p.starts_with("(version 1)(allow default)(deny file-write*)"));
        assert!(p.contains("(allow file-write* (subpath \"/work/proj\"))"));
        assert!(p.contains("(deny network*)"));
        assert!(p.contains("(deny file-read* file-write* (subpath \"/home/u/.aiw\"))"));
        assert!(p.contains("(deny file-read* file-write* (subpath \"/home/u/.ssh\"))"));
        assert!(p.contains("(deny file-read* file-write* (literal \"/home/u/.netrc\"))"));
        assert!(!p.contains(".missing"));
        let deny = p.find("(deny file-read* file-write* (subpath \"/home/u/.aiw\"))").unwrap();
        let allow = p.find("(allow file-read* (subpath \"/home/u/.aiw/skills\"))").unwrap();
        assert!(allow > deny, "exceptions must follow the denial: {p}");
        assert!(p.contains("(allow file-read* (subpath \"/home/u/.aiw/attachments/s1\"))"));
        assert!(!p.contains("/home/u/other"));
        // The network stays open when asked.
        assert!(!seatbelt_profile(&spec(SandboxMode::ReadOnly, true), &fs).contains("(deny network*)"));
        assert!(!seatbelt_profile(&spec(SandboxMode::ReadOnly, true), &fs).contains("(allow file-write* (subpath \"/work/proj\"))"));
    }

    #[test]
    fn seatbelt_profile_cannot_be_injected_through_a_path() {
        let mut sp = spec(SandboxMode::WorkspaceWrite, false);
        sp.hidden = vec!["/x\") (allow default) (\"".into(), "/y\nz".into(), "/back\\slash".into()];
        let p = seatbelt_profile(&sp, &|_| PathKind::Dir);
        assert!(p.contains("(subpath \"/x\\\") (allow default) (\\\"\")"), "{p}");
        assert!(!p.contains("/y\nz"), "a path with a newline is dropped");
        assert!(p.contains("/back\\\\slash"));
        sp.workspace = "/w\")(allow file-write* (subpath \"/\")".into();
        let p = seatbelt_profile(&sp, &|_| PathKind::Dir);
        assert!(!p.contains("(subpath \"/\"))"), "{p}");
    }

    #[test]
    fn seatbelt_workspace_inside_a_hidden_folder_is_allowed_again() {
        let mut sp = spec(SandboxMode::WorkspaceWrite, false);
        sp.workspace = "/home/u/.aiw/worktrees/abc".into();
        let p = seatbelt_profile(&sp, &fs);
        let deny = p.find("(deny file-read* file-write* (subpath \"/home/u/.aiw\"))").unwrap();
        let allow = p.rfind("(allow file-write* (subpath \"/home/u/.aiw/worktrees/abc\"))").unwrap();
        assert!(allow > deny);
    }

    #[test]
    fn seatbelt_args_put_the_profile_first() {
        let a = seatbelt_args(&spec(SandboxMode::ReadOnly, false), "/bin/sh", &["-c".into(), "ls".into()], &fs);
        assert_eq!(a[0], "-p");
        assert_eq!(&a[2..], ["/bin/sh", "-c", "ls"].map(String::from));
    }

    #[test]
    fn secret_environment_variables_are_recognised() {
        for n in [
            "GITHUB_TOKEN",
            "NPM_TOKEN",
            "my_api_key",
            "OPENAI_API_KEY",
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_REGION",
            "AZURE_CLIENT_ID",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "DB_PASSWORD",
            "MYSQL_PWD",
            "AIW_HOME",
            "AIW_TOKEN",
            "SSH_AUTH_SOCK",
            "DATABASE_URL",
            "KUBECONFIG",
            "STRIPE_SECRET",
            "SESSION_COOKIE_SECRET",
            "ANTHROPIC_API_KEY",
            "HF_TOKEN",
            "PRIVATE_KEY",
            "SIGNING_PRIVATE_KEY",
            "gh_token",
            "CLOUDSDK_CORE_PROJECT",
            "GCP_PROJECT",
        ] {
            assert!(is_secret_env(n), "{n}");
        }
        for n in [
            "PATH",
            "HOME",
            "LANG",
            "LC_ALL",
            "TERM",
            "USER",
            "SHELL",
            "TMPDIR",
            "TMP",
            "TEMP",
            "PWD",
            "OLDPWD",
            "EDITOR",
            "CARGO_HOME",
            "JAVA_HOME",
            "NODE_ENV",
            "KEYBOARD",
            "MONKEY",
            "SystemRoot",
            "USERPROFILE",
            "APPDATA",
            "PATHEXT",
            "XDG_RUNTIME_DIR",
        ] {
            assert!(!is_secret_env(n), "{n}");
        }
    }

    #[test]
    fn env_allow_restores_a_variable() {
        let names = ["PATH", "GITHUB_TOKEN", "NPM_TOKEN", "AWS_REGION", "AWS_PROFILE", "HOME"].map(String::from);
        let allow = vec!["github_token".to_string(), "AWS_*".to_string(), "  ".to_string(), "*".to_string()];
        assert_eq!(env_to_strip(names.clone(), &[]), ["GITHUB_TOKEN", "NPM_TOKEN", "AWS_REGION", "AWS_PROFILE"].map(String::from));
        // A bare `*` is not a wildcard for everything (an empty prefix is refused).
        assert_eq!(env_to_strip(names, &allow), ["NPM_TOKEN"].map(String::from));
    }

    #[test]
    fn deny_read_defaults_expand_to_absolute_paths() {
        let entries: Vec<String> = DEFAULT_DENY_READ.iter().map(|s| s.to_string()).collect();
        let paths = expand_hidden(&entries);
        assert_eq!(paths.len(), entries.len());
        assert!(paths.iter().all(|p| p.is_absolute()));
        assert!(paths.iter().any(|p| p.ends_with(".ssh")));
        assert!(expand_hidden(&["relative".into(), "  ".into()]).is_empty());
        for must in
            ["~/.ssh", "~/.gnupg", "~/.aws", "~/.config/gcloud", "~/.kube", "~/.docker/config.json", "~/.netrc", "~/.npmrc", "~/.pypirc"]
        {
            assert!(DEFAULT_DENY_READ.contains(&must), "{must}");
        }
    }
}
