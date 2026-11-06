//! Guided installation of runtimes (ADR 0003): a manifest `installer` recipe becomes a short list
//! of steps that AIW runs itself, with the output streamed to the UI.
//!
//! Only package managers are supported (`pip` into a virtual environment AIW owns, `brew`), never
//! `curl | sh`, and every package name is validated before it reaches a command line.

use crate::manifest::{Installer, RuntimeManifest};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Lines of output kept per installation (the oldest are dropped).
pub const MAX_LOG_LINES: usize = 2000;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Prerequisite {
    pub name: String,
    pub ok: bool,
    /// What to do when it is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InstallStep {
    pub title: String,
    /// The command as it will be run, for the confirmation shown to the user.
    pub command: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InstallPlan {
    pub runtime_id: String,
    pub runtime_name: String,
    /// A recipe exists for this OS. When false only `hint` applies.
    pub automatic: bool,
    pub steps: Vec<InstallStep>,
    pub prerequisites: Vec<Prerequisite>,
    /// Manual instructions (the manifest `install` hint) when there is no recipe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub homepage: Option<String>,
    /// Where the software ends up, in words.
    pub note: String,
}

impl InstallPlan {
    pub fn ready(&self) -> bool {
        self.automatic && self.prerequisites.iter().all(|p| p.ok)
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallState {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallJob {
    pub runtime_id: String,
    pub state: InstallState,
    /// 1-based index of the step that is running (or ran last).
    pub step: usize,
    pub steps: usize,
    pub log: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl InstallJob {
    pub fn new(runtime_id: &str, steps: usize) -> Self {
        Self { runtime_id: runtime_id.into(), state: InstallState::Running, step: 0, steps, log: Vec::new(), error: None }
    }
    pub fn push(&mut self, line: impl Into<String>) {
        self.log.push(crate::supervisor::clean_log_line(&line.into()));
        if self.log.len() > MAX_LOG_LINES {
            let extra = self.log.len() - MAX_LOG_LINES;
            self.log.drain(..extra);
        }
    }
}

/// A concrete command to run.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl Command {
    pub fn display(&self) -> String {
        let name = self.program.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        std::iter::once(name).chain(self.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }
}

/// Package and formula names that may be passed on a command line: no leading dash, no shell
/// or path characters. `pip` extras and version pins (`pkg[extra]>=1.2`) are allowed.
pub fn valid_package(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '[' | ']' | '=' | '<' | '>' | '~' | ',' | '+' | '@' | '/'))
        && !name.contains("..")
}

/// The Python of the virtual environment AIW manages.
pub fn venv_python(pyenv: &Path) -> PathBuf {
    if cfg!(windows) {
        pyenv.join("Scripts").join("python.exe")
    } else {
        pyenv.join("bin").join("python")
    }
}

/// Directory with the executables installed into the managed virtual environment.
pub fn venv_bin(pyenv: &Path) -> PathBuf {
    if cfg!(windows) {
        pyenv.join("Scripts")
    } else {
        pyenv.join("bin")
    }
}

/// Where Homebrew puts its executables when the app was not started from a terminal (Finder and
/// the Dock give GUI apps a minimal `PATH`).
pub fn brew_dirs() -> Vec<PathBuf> {
    ["/opt/homebrew/bin", "/usr/local/bin", "/home/linuxbrew/.linuxbrew/bin"].iter().map(PathBuf::from).collect()
}

pub fn find_brew_path() -> Option<PathBuf> {
    which::which("brew").ok().or_else(|| brew_dirs().into_iter().map(|d| d.join("brew")).find(|p| p.is_file()))
}

/// A system Python 3.10 or newer (MLX requires it), found in `PATH`.
pub fn system_python() -> Option<PathBuf> {
    ["python3", "python"].iter().filter_map(|n| which::which(n).ok()).next()
}

/// What the plan needs from the machine. `python_ok`: a Python 3.10+ is already there; without
/// one, Homebrew can install it as part of the plan.
fn prerequisites(installer: &Installer, python_ok: bool, brew: bool) -> Vec<Prerequisite> {
    match installer {
        Installer::Pip { .. } => {
            let ok = python_ok || brew;
            vec![Prerequisite {
                name: "Python 3.10 o successivo".into(),
                ok,
                hint: (!ok).then(|| {
                    "Installa Homebrew (https://brew.sh) oppure Python 3 (https://www.python.org/downloads/), poi riprova.".to_string()
                }),
            }]
        }
        Installer::Brew { .. } => vec![Prerequisite {
            name: "Homebrew".into(),
            ok: brew,
            hint: (!brew).then(|| "Installa Homebrew da https://brew.sh, poi riprova.".to_string()),
        }],
    }
}

/// The Homebrew formula AIW installs when the machine has no recent Python.
pub const BREW_PYTHON: &str = "python@3.12";

/// The plan for `manifest` on `os`. `python_ok` / `brew` describe the machine (probed by the
/// caller, so this stays a pure function).
pub fn plan(manifest: &RuntimeManifest, os: &str, python_ok: bool, brew: bool) -> InstallPlan {
    plan_for(
        &manifest.id,
        &manifest.name,
        manifest.installer_for(os),
        manifest.install_hint(os),
        manifest.homepage.clone(),
        python_ok,
        brew,
    )
}

/// [`plan`] for anything that carries an installer recipe (a runtime, a trainer).
pub fn plan_for(
    id: &str,
    name: &str,
    installer: Option<&Installer>,
    hint: Option<String>,
    homepage: Option<String>,
    python_ok: bool,
    brew: bool,
) -> InstallPlan {
    let mut p = InstallPlan {
        runtime_id: id.to_string(),
        runtime_name: name.to_string(),
        automatic: false,
        steps: vec![],
        prerequisites: vec![],
        hint,
        homepage,
        note: String::new(),
    };
    let Some(inst) = installer else { return p };
    let valid = match inst {
        Installer::Pip { packages } => !packages.is_empty() && packages.iter().all(|n| valid_package(n)),
        Installer::Brew { formula } => valid_package(formula),
    };
    if !valid {
        return p;
    }
    p.automatic = true;
    p.prerequisites = prerequisites(inst, python_ok, brew);
    match inst {
        Installer::Pip { packages } => {
            if !python_ok {
                p.steps.push(InstallStep { title: "Installa Python 3.12".into(), command: format!("brew install {BREW_PYTHON}") });
            }
            p.steps.push(InstallStep { title: "Ambiente Python di AIW".into(), command: "python3 -m venv ~/.aiw/pyenv".into() });
            p.steps.push(InstallStep {
                title: format!("Installa {}", packages.join(", ")),
                command: format!("pip install --upgrade {}", packages.join(" ")),
            });
            p.note = "Viene installato in un ambiente Python isolato di AIW (`~/.aiw/pyenv`): non tocca il Python di sistema.".into();
        }
        Installer::Brew { formula } => {
            p.steps = vec![InstallStep { title: format!("Installa {formula}"), command: format!("brew install {formula}") }];
            p.note = "Viene installato con Homebrew, come se lo facessi da terminale.".into();
        }
    }
    p
}

/// The commands behind a plan, in order. `pyenv` is the managed virtual environment; `python` a
/// Python 3.10+ that is already installed (without one, Homebrew installs Python 3.12 first);
/// `venv_exists` skips creating the environment again.
pub fn commands(
    installer: &Installer,
    pyenv: &Path,
    python: Option<&Path>,
    brew: Option<&Path>,
    venv_exists: bool,
) -> Result<Vec<Command>, String> {
    match installer {
        Installer::Pip { packages } => {
            if packages.is_empty() || !packages.iter().all(|n| valid_package(n)) {
                return Err("nome di pacchetto non valido nel manifest".into());
            }
            let mut out = Vec::new();
            if !venv_exists {
                let py = match (python, brew) {
                    (Some(py), _) => py.to_path_buf(),
                    (None, Some(b)) => {
                        out.push(Command { program: b.to_path_buf(), args: vec!["install".into(), BREW_PYTHON.into()] });
                        // Homebrew links `python3.12` next to `brew`.
                        b.parent().unwrap_or(Path::new("")).join("python3.12")
                    }
                    (None, None) => return Err("Python 3.10 o successivo non trovato".into()),
                };
                out.push(Command { program: py, args: vec!["-m".into(), "venv".into(), pyenv.display().to_string()] });
            }
            let mut args = vec!["-m".to_string(), "pip".into(), "install".into(), "--upgrade".into(), "--disable-pip-version-check".into()];
            args.extend(packages.iter().cloned());
            out.push(Command { program: venv_python(pyenv), args });
            Ok(out)
        }
        Installer::Brew { formula } => {
            if !valid_package(formula) {
                return Err("nome di formula non valido nel manifest".into());
            }
            let b = brew.ok_or("Homebrew non trovato")?;
            Ok(vec![Command { program: b.to_path_buf(), args: vec!["install".into(), formula.clone()] }])
        }
    }
}

/// Runs the commands of an installation one after the other, streaming their output into
/// `job`. Stops at the first failure.
pub async fn run(cmds: Vec<Command>, job: Arc<Mutex<InstallJob>>, start_after: bool) {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};
    let fail = |job: &Arc<Mutex<InstallJob>>, msg: String| {
        let mut j = job.lock().unwrap();
        j.push(format!("✗ {msg}"));
        j.error = Some(msg);
        j.state = InstallState::Failed;
    };
    for (i, cmd) in cmds.iter().enumerate() {
        {
            let mut j = job.lock().unwrap();
            j.step = i + 1;
            j.push(format!("$ {}", cmd.display()));
        }
        let mut child = match tokio::process::Command::new(&cmd.program)
            .args(&cmd.args)
            .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
            .env("PYTHONUNBUFFERED", "1")
            .env("NONINTERACTIVE", "1")
            .env("HOMEBREW_NO_ENV_HINTS", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return fail(&job, format!("{}: {e}", cmd.display())),
        };
        let mut readers = Vec::new();
        if let Some(out) = child.stdout.take() {
            let job = job.clone();
            readers.push(tokio::spawn(async move {
                let mut lines = BufReader::new(out).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    job.lock().unwrap().push(l);
                }
            }));
        }
        if let Some(err) = child.stderr.take() {
            let job = job.clone();
            readers.push(tokio::spawn(async move {
                let mut lines = BufReader::new(err).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    job.lock().unwrap().push(l);
                }
            }));
        }
        let status = tokio::time::timeout(STEP_TIMEOUT, child.wait()).await;
        if status.is_err() {
            let _ = child.kill().await;
            for reader in &readers {
                reader.abort();
            }
        }
        for r in readers {
            let _ = r.await;
        }
        match status {
            Ok(Ok(s)) if s.success() => {}
            Ok(Ok(s)) => return fail(&job, format!("`{}` è terminato con {s}", cmd.display())),
            Ok(Err(e)) => return fail(&job, format!("`{}`: {e}", cmd.display())),
            Err(_) => return fail(&job, format!("`{}` ha superato il tempo massimo", cmd.display())),
        }
    }
    let mut j = job.lock().unwrap();
    j.push("✓ Installazione completata");
    if start_after {
        j.step = j.steps;
        j.push("Avvio del runtime…");
    } else {
        j.state = InstallState::Done;
    }
}

/// A single installation step may take a long time (large wheels, Homebrew bottles).
const STEP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::builtin;

    fn manifest(id: &str) -> RuntimeManifest {
        builtin().into_iter().find(|m| m.id == id).unwrap()
    }

    #[test]
    fn package_names_cannot_smuggle_options_or_shell() {
        for ok in ["mlx-lm", "mlx_vlm", "pkg[extra]>=1.2,<2", "llama.cpp", "ggml-org/tap/llama.cpp"] {
            assert!(valid_package(ok), "{ok}");
        }
        for bad in ["", "--index-url=http://evil", "-e .", "a b", "a;rm -rf /", "$(id)", "a`id`", "../x", "a|b", "a&b", "a\nb"] {
            assert!(!valid_package(bad), "{bad:?}");
        }
    }

    #[test]
    fn mlx_runtimes_install_into_the_managed_environment() {
        let m = manifest("mlx-lm");
        let p = plan(&m, "macos", true, false);
        assert!(p.automatic && p.ready());
        assert_eq!(p.steps.len(), 2);
        assert!(p.steps[1].command.contains("mlx-lm"));
        // No recent Python, but Homebrew: it installs Python first, still fully automatic.
        let p = plan(&m, "macos", false, true);
        assert!(p.ready());
        assert_eq!(p.steps.len(), 3);
        assert!(p.steps[0].command.contains("python@3.12"));
        // Neither: the plan is shown but cannot run.
        let p = plan(&m, "macos", false, false);
        assert!(p.automatic && !p.ready());
        assert!(p.prerequisites[0].hint.is_some());
        // No recipe on this OS: only the manual hint.
        let p = plan(&m, "linux", true, true);
        assert!(!p.automatic && p.steps.is_empty());
    }

    #[test]
    fn runtimes_without_a_safe_recipe_stay_manual() {
        for id in ["lmstudio", "nexa", "mistralrs"] {
            let p = plan(&manifest(id), "macos", true, true);
            assert!(!p.automatic, "{id} must not run a script from the internet");
            assert!(p.hint.is_some(), "{id}");
        }
    }

    #[test]
    fn localai_uses_the_package_manager_on_supported_platforms() {
        for os in ["macos", "linux"] {
            let plan = plan(&manifest("localai"), os, true, true);
            assert!(plan.ready());
            assert_eq!(plan.steps[0].command, "brew install localai");
        }
    }

    #[test]
    fn brew_recipe_needs_homebrew() {
        let m = manifest("llama-server");
        assert!(!plan(&m, "macos", true, false).ready());
        assert!(plan(&m, "macos", true, true).ready());
    }

    #[test]
    fn pip_commands_create_the_environment_once() {
        let inst = Installer::Pip { packages: vec!["mlx-lm".into()] };
        let pyenv = Path::new("/h/.aiw/pyenv");
        let first = commands(&inst, pyenv, Some(Path::new("/usr/bin/python3")), None, false).unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].args, ["-m", "venv", "/h/.aiw/pyenv"]);
        assert_eq!(first[1].program, venv_python(pyenv));
        assert_eq!(first[1].args.last().unwrap(), "mlx-lm");
        let again = commands(&inst, pyenv, None, None, true).unwrap();
        assert_eq!(again.len(), 1);
        assert!(commands(&inst, pyenv, None, None, false).is_err(), "needs a Python to create the environment");
        // Without a recent Python, Homebrew installs one and the environment is built with it.
        let brewed = commands(&inst, pyenv, None, Some(Path::new("/opt/homebrew/bin/brew")), false).unwrap();
        assert_eq!(brewed.len(), 3);
        assert_eq!(brewed[0].args, ["install", "python@3.12"]);
        assert_eq!(brewed[1].program, Path::new("/opt/homebrew/bin/python3.12"));
        assert!(commands(&Installer::Pip { packages: vec!["--pre".into()] }, pyenv, None, None, true).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_streams_output_and_stops_at_the_first_failure() {
        let sh = |script: &str| Command { program: "/bin/sh".into(), args: vec!["-c".into(), script.into()] };
        let job = Arc::new(Mutex::new(InstallJob::new("x", 3)));
        run(vec![sh("echo uno; echo err 1>&2"), sh("exit 3"), sh("echo mai")], job.clone(), false).await;
        let j = job.lock().unwrap();
        assert_eq!(j.state, InstallState::Failed);
        assert_eq!(j.step, 2);
        assert!(j.log.iter().any(|l| l == "uno") && j.log.iter().any(|l| l == "err"), "{:?}", j.log);
        assert!(!j.log.iter().any(|l| l == "mai"), "{:?}", j.log);
        assert!(j.error.as_deref().unwrap().contains("exit status: 3"), "{:?}", j.error);
        drop(j);
        let ok = Arc::new(Mutex::new(InstallJob::new("y", 1)));
        run(vec![sh("echo fatto")], ok.clone(), false).await;
        assert_eq!(ok.lock().unwrap().state, InstallState::Done);
        let missing = Arc::new(Mutex::new(InstallJob::new("z", 1)));
        run(vec![Command { program: "/nonexistent/prog".into(), args: vec![] }], missing.clone(), false).await;
        assert_eq!(missing.lock().unwrap().state, InstallState::Failed);
    }

    #[test]
    fn log_is_bounded() {
        let mut j = InstallJob::new("x", 1);
        for i in 0..MAX_LOG_LINES + 50 {
            j.push(format!("l{i}"));
        }
        assert_eq!(j.log.len(), MAX_LOG_LINES);
        assert_eq!(j.log[0], "l50");
    }
}
