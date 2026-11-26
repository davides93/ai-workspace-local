//! Is there a container engine on this computer, and does it run? Read-only: it never installs, starts or pulls anything.
//!
//! Docker and Podman are asked with their own command line, each call with a timeout (a stuck daemon socket must not
//! hang the app). The parsers are pure and tested on sample output; [`probe_with`] takes the command runner as an
//! argument so the decision (`state`, `hint`) is tested without any engine installed.

use crate::run;
use aiw_core::api::{ContainerEngine, ContainerStatus};
use std::path::{Path, PathBuf};
use std::time::Duration;

const T: Duration = Duration::from_secs(6);

/// Asks the real engines. Takes up to a few seconds when a daemon is stuck (timeouts), never fails.
pub fn probe_containers() -> ContainerStatus {
    probe_with(&crate::normalized_os(), &mut |cmd, args| run(&container_cli(cmd), args, T))
}

/// Where the command-line client of an engine (`docker`, `podman`) is. An app opened from Finder or the Start menu does not
/// have the PATH of a terminal: Docker Desktop's `docker` may be only in its own folder. The PATH comes first; then the
/// folders where each system's installer puts it; at last the bare name (and the command fails as «not installed»).
pub fn container_cli(name: &str) -> String {
    let path = std::env::var_os("PATH").unwrap_or_default();
    find_cli(name, std::env::split_paths(&path), &known_dirs(name), &|p| p.is_file())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| name.to_string())
}

/// The folder to put at the front of PATH when running the resolved client: Docker Desktop's helpers (`docker-credential-desktop`,
/// `docker-compose`) live next to `docker`, and a build that pulls an image needs them.
pub fn container_cli_dir(cli: &str) -> Option<PathBuf> {
    let p = Path::new(cli);
    p.is_absolute().then(|| p.parent().map(Path::to_path_buf)).flatten()
}

fn find_cli(name: &str, path_dirs: impl Iterator<Item = PathBuf>, known: &[PathBuf], exists: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    path_dirs.chain(known.iter().cloned()).map(|d| d.join(&file)).find(|p| exists(p))
}

fn known_dirs(name: &str) -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut v: Vec<PathBuf> = vec![];
    if cfg!(target_os = "macos") {
        v.extend(
            ["/usr/local/bin", "/opt/homebrew/bin", "/opt/podman/bin", "/Applications/Docker.app/Contents/Resources/bin"]
                .map(PathBuf::from),
        );
        if let Some(h) = env("HOME") {
            v.push(h.join(".docker/bin"));
        }
    } else if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramW6432"] {
            if let Some(pf) = env(var) {
                v.push(pf.join("Docker").join("Docker").join("resources").join("bin"));
                v.push(pf.join("RedHat").join("Podman"));
            }
        }
        if let Some(l) = env("LOCALAPPDATA") {
            v.push(l.join("Programs").join("Docker").join("Docker").join("resources").join("bin"));
            v.push(l.join("Programs").join("Podman"));
        }
    } else {
        v.extend(["/usr/local/bin", "/usr/bin", "/opt/podman/bin"].map(PathBuf::from));
    }
    let _ = name;
    v
}

/// `runner(command, args)` returns stdout when the command ran and succeeded, `None` otherwise (missing, failed, timeout).
pub fn probe_with(os: &str, runner: &mut dyn FnMut(&str, &[&str]) -> Option<String>) -> ContainerStatus {
    if os == "ios" || os == "android" {
        return ContainerStatus { state: "unsupported".into(), engine: None, hint: "none".into(), engines: vec![], os: os.into() };
    }
    let engines = vec![docker(runner), podman(runner)];
    decide(os, engines)
}

fn docker(r: &mut dyn FnMut(&str, &[&str]) -> Option<String>) -> ContainerEngine {
    let mut e = ContainerEngine { engine: "docker".into(), ..Default::default() };
    // `--version` works with the daemon off: it tells "installed" apart from "running".
    let Some(v) = r("docker", &["--version"]) else { return e };
    e.installed = true;
    e.version = parse_version(&v);
    if let Some(info) = r("docker", &["info", "--format", "{{.ServerVersion}}|{{.NCPU}}|{{.MemTotal}}|{{.Host.Security.Rootless}}"]) {
        e.running = true;
        let (server, cpus, mem, rootless) = parse_info(&info);
        e.version = server.or(e.version);
        e.cpus = cpus;
        e.memory_mb = mem;
        e.rootless = rootless;
        e.compose = r("docker", &["compose", "version", "--short"]).is_some();
    }
    e
}

fn podman(r: &mut dyn FnMut(&str, &[&str]) -> Option<String>) -> ContainerEngine {
    let mut e = ContainerEngine { engine: "podman".into(), ..Default::default() };
    let Some(v) = r("podman", &["--version"]) else { return e };
    e.installed = true;
    e.version = parse_version(&v);
    if let Some(info) =
        r("podman", &["info", "--format", "{{.Version.Version}}|{{.Host.CPUs}}|{{.Host.MemTotal}}|{{.Host.Security.Rootless}}"])
    {
        e.running = true;
        let (server, cpus, mem, rootless) = parse_info(&info);
        e.version = server.or(e.version);
        e.cpus = cpus;
        e.memory_mb = mem;
        e.rootless = rootless;
        e.compose = r("podman", &["compose", "version"]).is_some();
    }
    e
}

/// `Docker version 27.3.1, build ce12230` / `podman version 5.2.2` → `27.3.1` / `5.2.2`.
pub(crate) fn parse_version(s: &str) -> Option<String> {
    let line = s.lines().next()?.trim();
    let after = line.split_whitespace().skip_while(|w| !w.eq_ignore_ascii_case("version")).nth(1)?;
    let v = after.trim_end_matches(',');
    v.chars().next().is_some_and(|c| c.is_ascii_digit()).then(|| v.to_string())
}

/// `version|cpus|memory bytes|rootless` as asked with `--format`. Every field may be empty or `<no value>`.
pub(crate) fn parse_info(s: &str) -> (Option<String>, Option<u32>, Option<u64>, Option<bool>) {
    let mut f = s.trim().split('|').map(str::trim);
    let version = f.next().filter(|v| !v.is_empty() && !v.contains("no value")).map(String::from);
    let cpus = f.next().and_then(|v| v.parse::<u32>().ok()).filter(|n| *n > 0);
    let mem = f.next().and_then(|v| v.parse::<u64>().ok()).filter(|n| *n > 0).map(|b| b / (1024 * 1024));
    let rootless = f.next().and_then(|v| match v {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    });
    (version, cpus, mem, rootless)
}

fn decide(os: &str, engines: Vec<ContainerEngine>) -> ContainerStatus {
    // Preference: a running engine with compose (Docker first: it is what OpenBot/OpenDots document), then a running
    // one without compose, then an installed but stopped one, then nothing.
    let pick = |f: &dyn Fn(&ContainerEngine) -> bool| engines.iter().find(|e| f(e)).map(|e| e.engine.clone());
    let (state, engine, hint) = if let Some(e) = pick(&|e| e.running && e.compose) {
        ("ready", Some(e), "none")
    } else if let Some(e) = pick(&|e| e.running) {
        ("compose_missing", Some(e), "install_compose")
    } else if let Some(e) = pick(&|e| e.installed) {
        ("engine_stopped", Some(e), "start_docker")
    } else {
        ("not_installed", None, "install_docker")
    };
    ContainerStatus { state: state.into(), engine, hint: hint.into(), engines, os: os.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake<'a>(table: &'a [(&'a str, &'a str)]) -> impl FnMut(&str, &[&str]) -> Option<String> + 'a {
        move |cmd, args| {
            let key = format!("{cmd} {}", args.join(" "));
            table.iter().find(|(k, _)| key.starts_with(k)).map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn parses_versions_and_info() {
        assert_eq!(parse_version("Docker version 27.3.1, build ce12230\n").as_deref(), Some("27.3.1"));
        assert_eq!(parse_version("podman version 5.2.2").as_deref(), Some("5.2.2"));
        assert_eq!(parse_version("command not found"), None);
        assert_eq!(parse_info("27.3.1|8|8217579520|false\n"), (Some("27.3.1".into()), Some(8), Some(7836), Some(false)));
        assert_eq!(parse_info("<no value>||0|<no value>"), (None, None, None, None));
    }

    #[test]
    fn finds_the_client_on_the_path_then_in_the_known_folders() {
        let file = if cfg!(windows) { "docker.exe" } else { "docker" };
        let only = |want: PathBuf| move |p: &Path| p == want;
        let known = [PathBuf::from("known")];
        let on_path =
            find_cli("docker", [PathBuf::from("a"), PathBuf::from("b")].into_iter(), &known, &only(PathBuf::from("b").join(file)));
        assert_eq!(on_path, Some(PathBuf::from("b").join(file)));
        // A Finder/Explorer launch has a short PATH: the installer's folder is tried next.
        let in_known = find_cli("docker", [PathBuf::from("a")].into_iter(), &known, &only(PathBuf::from("known").join(file)));
        assert_eq!(in_known, Some(PathBuf::from("known").join(file)));
        assert_eq!(find_cli("docker", std::iter::empty(), &known, &|_| false), None);
    }

    #[test]
    fn the_known_folders_are_the_ones_of_each_system() {
        let d = known_dirs("docker");
        if cfg!(target_os = "macos") {
            assert!(d.iter().any(|p| p.ends_with("Docker.app/Contents/Resources/bin")));
        } else if cfg!(windows) {
            // Without ProgramFiles in the environment there is nothing to add: never a made-up path.
            assert!(d.iter().all(|p| p.is_absolute() || p.as_os_str().is_empty() || p.has_root()));
        } else {
            assert!(d.contains(&PathBuf::from("/usr/bin")));
        }
        assert_eq!(container_cli_dir("docker"), None, "a bare name has no folder");
    }

    #[test]
    fn nothing_installed() {
        let s = probe_with("linux", &mut fake(&[]));
        assert_eq!((s.state.as_str(), s.hint.as_str(), s.engine), ("not_installed", "install_docker", None));
        assert!(s.engines.iter().all(|e| !e.installed));
    }

    #[test]
    fn docker_installed_daemon_off() {
        let s = probe_with("macos", &mut fake(&[("docker --version", "Docker version 27.3.1, build x")]));
        assert_eq!((s.state.as_str(), s.hint.as_str(), s.engine.as_deref()), ("engine_stopped", "start_docker", Some("docker")));
        assert_eq!(s.engines[0].version.as_deref(), Some("27.3.1"));
        assert!(!s.engines[0].running);
    }

    #[test]
    fn docker_ready() {
        let s = probe_with(
            "linux",
            &mut fake(&[
                ("docker --version", "Docker version 27.3.1, build x"),
                ("docker info", "27.3.1|8|8217579520|false"),
                ("docker compose version", "2.29.7"),
            ]),
        );
        assert_eq!((s.state.as_str(), s.engine.as_deref()), ("ready", Some("docker")));
        let d = &s.engines[0];
        assert_eq!((d.compose, d.cpus, d.memory_mb, d.rootless), (true, Some(8), Some(7836), Some(false)));
    }

    #[test]
    fn running_without_compose_and_podman_fallback() {
        let s = probe_with(
            "linux",
            &mut fake(&[("docker --version", "Docker version 24.0.0, build x"), ("docker info", "24.0.0|2|4294967296|false")]),
        );
        assert_eq!((s.state.as_str(), s.hint.as_str()), ("compose_missing", "install_compose"));
        let s = probe_with(
            "linux",
            &mut fake(&[
                ("podman --version", "podman version 5.2.2"),
                ("podman info", "5.2.2|4|8589934592|true"),
                ("podman compose version", "podman-compose 1.2"),
            ]),
        );
        assert_eq!((s.state.as_str(), s.engine.as_deref()), ("ready", Some("podman")));
        assert_eq!(s.engines[1].rootless, Some(true));
    }

    #[test]
    fn phones_have_no_engine() {
        let mut called = false;
        let s = probe_with("ios", &mut |_, _| {
            called = true;
            None
        });
        assert_eq!(s.state, "unsupported");
        assert!(!called, "no process is launched on a phone");
    }
}
