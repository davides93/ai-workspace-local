//! Tool layer (architecture §6). Every tool declares a JSON-schema for its arguments, a risk
//! class used by the policy engine, and the "subject" (path/command/url) that policy rules match.

pub mod browser;
pub mod computer;
pub mod fs;
pub mod mcp;
pub mod shell;
pub mod web;

use aiw_core::{Error, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Read,
    Write,
    Exec,
    Network,
    Computer,
}

impl Risk {
    pub fn as_str(&self) -> &'static str {
        match self {
            Risk::Read => "read",
            Risk::Write => "write",
            Risk::Exec => "exec",
            Risk::Network => "network",
            Risk::Computer => "computer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    ReadOnly,
    WorkspaceWrite,
    Off,
}

impl SandboxMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "read-only" => SandboxMode::ReadOnly,
            "off" => SandboxMode::Off,
            _ => SandboxMode::WorkspaceWrite,
        }
    }
}

/// Execution context handed to tools.
#[derive(Clone)]
pub struct ToolContext {
    pub run_id: String,
    pub workspace_root: PathBuf,
    /// Extra folders readable by tools (skills, attachments…).
    pub read_roots: Vec<PathBuf>,
    pub sandbox: SandboxMode,
    pub cancel: CancellationToken,
    /// Where original files are copied before being modified (rewind).
    pub checkpoint_dir: Option<PathBuf>,
    /// Allowed hosts for network tools (empty = any public host).
    pub allow_domains: Vec<String>,
    /// Lets web tools reach localhost/LAN addresses.
    pub allow_private_network: bool,
}

impl ToolContext {
    pub fn new(run_id: impl Into<String>, workspace_root: PathBuf) -> Self {
        Self {
            run_id: run_id.into(),
            workspace_root,
            read_roots: vec![],
            sandbox: SandboxMode::WorkspaceWrite,
            cancel: CancellationToken::new(),
            checkpoint_dir: None,
            allow_domains: vec![],
            allow_private_network: false,
        }
    }

    /// Resolves a user/model supplied path and enforces the workspace boundary.
    pub fn resolve(&self, p: &str, write: bool) -> Result<PathBuf> {
        if p.trim().is_empty() {
            return Err(Error::Invalid("percorso vuoto".into()));
        }
        let expanded = aiw_core::paths::expand_home(p.trim());
        let abs = if expanded.is_absolute() { expanded } else { self.workspace_root.join(expanded) };
        let norm = aiw_core::paths::normalize(&abs);
        if self.sandbox == SandboxMode::Off {
            return Ok(norm);
        }
        let root = aiw_core::paths::normalize(&self.workspace_root);
        let inside = norm.starts_with(&root);
        if write {
            if self.sandbox == SandboxMode::ReadOnly {
                return Err(Error::Denied("sandbox in sola lettura: le scritture non sono consentite".into()));
            }
            if !inside {
                return Err(Error::Denied(format!("{} è fuori dal workspace ({})", norm.display(), root.display())));
            }
        } else if !inside && !self.read_roots.iter().any(|r| norm.starts_with(aiw_core::paths::normalize(r))) {
            return Err(Error::Denied(format!("{} è fuori dal workspace ({})", norm.display(), root.display())));
        }
        Ok(norm)
    }

    /// Copies `path` into the checkpoint folder before it is overwritten (once per run).
    pub fn checkpoint(&self, path: &Path) {
        let Some(dir) = &self.checkpoint_dir else { return };
        if !path.is_file() {
            return;
        }
        let rel = path.strip_prefix(&self.workspace_root).unwrap_or(path);
        let rel = rel.to_string_lossy().replace([':', '\\'], "_");
        let target = dir.join(&self.run_id).join(rel.trim_start_matches('/'));
        if target.exists() {
            return;
        }
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(path, &target);
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToolOutput {
    pub content: String,
    /// `data:image/png;base64,…` URLs for vision models.
    pub images: Vec<String>,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn text(s: impl Into<String>) -> Self {
        Self { content: s.into(), ..Default::default() }
    }
    pub fn error(s: impl Into<String>) -> Self {
        Self { content: s.into(), is_error: true, ..Default::default() }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// JSON Schema of the arguments.
    fn parameters(&self) -> Value;
    fn risk(&self) -> Risk;
    /// `Err(reason)` when the tool cannot run on this device/build.
    fn available(&self) -> std::result::Result<(), String> {
        Ok(())
    }
    /// Value matched by policy rules such as `shell(git *)` or `write_file(src/**)`.
    fn subject(&self, args: &Value) -> Option<String> {
        ["path", "command", "url", "from", "pattern"].iter().find_map(|k| args.get(*k).and_then(|v| v.as_str()).map(String::from))
    }
    /// Source label (`builtin` or the MCP server name).
    fn server(&self) -> Option<&str> {
        None
    }
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput>;
}

/// Name → tool map with per-tool enable switches.
#[derive(Default)]
pub struct ToolRegistry {
    tools: RwLock<BTreeMap<String, Arc<dyn Tool>>>,
    disabled: RwLock<HashSet<String>>,
}

impl ToolRegistry {
    pub fn register(&self, t: Arc<dyn Tool>) {
        self.tools.write().unwrap().insert(t.name().to_string(), t);
    }
    pub fn unregister_prefix(&self, prefix: &str) {
        self.tools.write().unwrap().retain(|k, _| !k.starts_with(prefix));
    }
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().unwrap().get(name).cloned()
    }
    pub fn all(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.read().unwrap().values().cloned().collect()
    }
    pub fn set_enabled(&self, name: &str, enabled: bool) {
        let mut d = self.disabled.write().unwrap();
        if enabled {
            d.remove(name);
        } else {
            d.insert(name.to_string());
        }
    }
    pub fn is_enabled(&self, name: &str) -> bool {
        !self.disabled.read().unwrap().contains(name)
    }
    pub fn disabled(&self) -> Vec<String> {
        self.disabled.read().unwrap().iter().cloned().collect()
    }
    pub fn set_disabled(&self, names: Vec<String>) {
        *self.disabled.write().unwrap() = names.into_iter().collect();
    }
    pub fn info(&self) -> Vec<aiw_core::api::ToolInfo> {
        self.all()
            .into_iter()
            .map(|t| {
                let avail = t.available();
                aiw_core::api::ToolInfo {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    source: if t.server().is_some() { "mcp".into() } else { "builtin".into() },
                    server: t.server().map(String::from),
                    risk: t.risk().as_str().into(),
                    enabled: self.is_enabled(t.name()),
                    available: avail.is_ok(),
                    unavailable_reason: avail.err(),
                    parameters: t.parameters(),
                }
            })
            .collect()
    }
}

/// Registers every built-in tool available in this build.
pub fn register_builtins(reg: &ToolRegistry, browser: Arc<browser::BrowserManager>) {
    for t in fs::tools() {
        reg.register(t);
    }
    reg.register(Arc::new(shell::ShellTool));
    reg.register(Arc::new(web::WebFetchTool));
    for t in browser::tools(browser) {
        reg.register(t);
    }
    for t in computer::tools() {
        reg.register(t);
    }
}

/// Truncates long outputs keeping head and tail.
pub fn truncate_middle(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let head = max * 2 / 3;
    let tail = max - head;
    let mut h = head;
    while !s.is_char_boundary(h) {
        h -= 1;
    }
    let mut t = s.len() - tail;
    while !s.is_char_boundary(t) {
        t += 1;
    }
    format!("{}\n… [{} caratteri omessi] …\n{}", &s[..h], s.len() - h - (s.len() - t), &s[t..])
}

pub(crate) fn arg_str<'a>(args: &'a Value, k: &str) -> Result<&'a str> {
    args.get(k).and_then(|v| v.as_str()).ok_or_else(|| Error::Invalid(format!("argomento `{k}` mancante")))
}

/// OpenAI function names must match `^[a-zA-Z0-9_-]{1,64}$`.
pub fn openai_safe_name(s: &str) -> String {
    let mut out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    out.truncate(64);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_confinement() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ToolContext::new("r", dir.path().to_path_buf());
        assert!(ctx.resolve("a/b.txt", true).is_ok());
        assert!(ctx.resolve("../escape.txt", true).is_err());
        assert!(ctx.resolve("/etc/passwd", false).is_err());
        ctx.sandbox = SandboxMode::ReadOnly;
        assert!(ctx.resolve("a.txt", true).is_err());
        assert!(ctx.resolve("a.txt", false).is_ok());
        ctx.sandbox = SandboxMode::Off;
        assert!(ctx.resolve("/etc/passwd", false).is_ok());
    }

    #[test]
    fn truncation_keeps_head_and_tail() {
        let s = "a".repeat(100) + &"b".repeat(100);
        let t = truncate_middle(&s, 60);
        assert!(t.starts_with("aaa") && t.ends_with("bbb") && t.contains("omessi"));
    }
}
