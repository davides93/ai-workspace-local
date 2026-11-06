use std::path::{Path, PathBuf};

/// Resolves the data directory: `$AIW_HOME`, else `~/.aiw` on desktop.
/// Mobile shells pass their sandbox directory explicitly via [`DataDirs::at`].
#[derive(Debug, Clone)]
pub struct DataDirs {
    pub root: PathBuf,
}

impl DataDirs {
    pub fn resolve() -> Self {
        if let Ok(p) = std::env::var("AIW_HOME") {
            return Self::at(PathBuf::from(p));
        }
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self::at(home.join(".aiw"))
    }
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn ensure(&self) -> std::io::Result<()> {
        for d in
            [self.models(), self.runtimes(), self.skills(), self.checkpoints(), self.logs(), self.devices(), self.workflows(), self.evals()]
        {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.json")
    }
    pub fn db_file(&self) -> PathBuf {
        self.root.join("aiw.db")
    }
    pub fn models(&self) -> PathBuf {
        self.root.join("models")
    }
    pub fn runtimes(&self) -> PathBuf {
        self.root.join("runtimes")
    }
    pub fn skills(&self) -> PathBuf {
        self.root.join("skills")
    }
    pub fn checkpoints(&self) -> PathBuf {
        self.root.join("checkpoints")
    }
    /// Git worktrees of isolated sessions (outside the user's checkout).
    pub fn worktrees(&self) -> PathBuf {
        self.root.join("worktrees")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn devices(&self) -> PathBuf {
        self.root.join("devices")
    }
    pub fn workflows(&self) -> PathBuf {
        self.root.join("workflows")
    }
    /// Evaluation suites written by the user, read at start (`<workspace>/.aiw` is not: a repository must not bring its own commands).
    pub fn evals(&self) -> PathBuf {
        self.root.join("evals")
    }
    /// Working folders of the evaluation tasks.
    pub fn eval_work(&self) -> PathBuf {
        self.root.join("eval-work")
    }
    pub fn device_config(&self, device_name: &str) -> PathBuf {
        let safe: String = device_name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        self.devices().join(format!("{safe}.json"))
    }
}

/// Expands a leading `~` to the home directory.
pub fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        if let Some(h) = dirs::home_dir() {
            return h.join(rest);
        }
    } else if p == "~" {
        if let Some(h) = dirs::home_dir() {
            return h;
        }
    }
    PathBuf::from(p)
}

/// Lexically normalises a path (resolves `.` and `..` without touching the filesystem).
pub fn normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
