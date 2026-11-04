//! Declarative runtime manifests (ADR 0003).

use aiw_core::schema::SettingsSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub vendor: Option<String>,
    /// embedded | sidecar | system | remote
    pub kind: String,
    #[serde(default)]
    pub description: String,
    pub platforms: Vec<String>,
    /// Optional CPU architecture gate (`aarch64`, `x86_64`); empty = any. MLX is `macos` and
    /// `aarch64` only: an Intel Mac must not offer it.
    #[serde(default)]
    pub arch: Vec<String>,
    #[serde(default)]
    pub accelerators: Vec<String>,
    #[serde(default)]
    pub formats: Vec<String>,
    /// openai | ollama | native | none (`none`: a helper process that is not a chat backend,
    /// e.g. the llama.cpp RPC worker). `ollama` = OpenAI adapter plus the native bits of
    /// [`crate::ollama`] (model listing from `/api/tags`).
    #[serde(default = "openai")]
    pub api: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub detect: Detect,
    #[serde(default)]
    pub launch: Option<Launch>,
    /// Endpoint of a server that may already be running outside AIW (Ollama, LM Studio…).
    #[serde(default)]
    pub external_endpoint: Option<String>,
    #[serde(default)]
    pub settings: SettingsSchema,
    #[serde(default)]
    pub install: BTreeMap<String, String>,
    #[serde(default)]
    pub homepage: Option<String>,
}

fn openai() -> String {
    "openai".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Detect {
    /// Executables looked up in PATH (first match wins); setting `binary` overrides.
    #[serde(default)]
    pub binaries: Vec<String>,
    /// Python modules (`python3 -c "import x"`), for Python-only runtimes.
    #[serde(default)]
    pub python_modules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Launch {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// setting key → argument template; `{value}` is replaced. Booleans add the template only
    /// when true; arrays are appended verbatim; null/empty values are skipped.
    #[serde(default)]
    pub settings_args: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Path appended to `http://127.0.0.1:{port}` for readiness checks, or `tcp` to only check
    /// that the port accepts connections.
    #[serde(default = "health")]
    pub health: String,
    /// Path of the OpenAI base (default `/v1`).
    #[serde(default = "v1")]
    pub api_base: String,
    #[serde(default = "ready_timeout")]
    pub ready_timeout_s: u64,
    /// The process daemonizes and returns immediately (e.g. `lms server start`).
    #[serde(default)]
    pub detached: bool,
    #[serde(default)]
    pub fixed_port: Option<u16>,
    /// Needs a model file (`{model_path}`) or a model name (`{model}`) to start.
    #[serde(default)]
    pub requires_model: bool,
    /// Name of the setting that carries the context window (default: `ctx_size`, then
    /// `max_model_len`, whichever is wired in `settings_args`). The context of the inference
    /// profile overrides it when the sidecar is started for that profile.
    #[serde(default)]
    pub ctx_setting: Option<String>,
}

fn health() -> String {
    "/health".into()
}
fn v1() -> String {
    "/v1".into()
}
fn ready_timeout() -> u64 {
    180
}

/// Settings that carry the context window when a launch recipe does not name one explicitly.
const CTX_SETTING_KEYS: [&str; 2] = ["ctx_size", "max_model_len"];

impl RuntimeManifest {
    pub fn supports_platform(&self, os: &str) -> bool {
        self.platforms.iter().any(|p| p == os || p == "any")
    }

    /// CPU architecture gate (`aarch64`, `x86_64`…); empty = any. Used for MLX, which needs
    /// Apple Silicon even though the OS is `macos`.
    pub fn supports_arch(&self, arch: &str) -> bool {
        self.arch.is_empty() || self.arch.iter().any(|a| a == arch)
    }

    /// The setting the profile context overrides, if this runtime has one.
    pub fn ctx_setting(&self) -> Option<String> {
        let launch = self.launch.as_ref()?;
        if let Some(k) = &launch.ctx_setting {
            return Some(k.clone());
        }
        CTX_SETTING_KEYS.iter().find(|k| launch.settings_args.contains_key(**k)).map(|k| k.to_string())
    }

    /// Applies the context window of an inference profile over `settings` (clamped to the range
    /// declared by the schema). Returns the value actually stored, or `None` when the runtime
    /// has no context setting.
    pub fn apply_ctx(&self, settings: &mut Map<String, Value>, ctx: u32) -> Option<u32> {
        let key = self.ctx_setting()?;
        let mut v = ctx as f64;
        if let Some(f) = self.settings.properties.get(&key) {
            if let Some(min) = f.minimum {
                v = v.max(min);
            }
            if let Some(max) = f.maximum {
                v = v.min(max);
            }
        }
        let v = v as u32;
        settings.insert(key, Value::from(v));
        Some(v)
    }
    pub fn install_hint(&self, os: &str) -> Option<String> {
        self.install.get(os).or_else(|| self.install.get("any")).cloned()
    }
}

/// Expands placeholders in launch arguments.
pub fn expand_args(launch: &Launch, settings: &Map<String, Value>, vars: &BTreeMap<String, String>) -> Vec<String> {
    let subst = |t: &str| -> String {
        let mut s = t.to_string();
        for (k, v) in vars {
            s = s.replace(&format!("{{{k}}}"), v);
        }
        s
    };
    let mut out: Vec<String> = launch.args.iter().map(|a| subst(a)).collect();
    for (key, template) in &launch.settings_args {
        let Some(v) = settings.get(key) else { continue };
        match v {
            Value::Null => {}
            Value::Bool(true) => out.extend(template.iter().map(|t| subst(t))),
            Value::Bool(false) => {}
            Value::Array(items) => out.extend(items.iter().map(|i| i.as_str().map(String::from).unwrap_or_else(|| i.to_string()))),
            Value::String(s) if s.is_empty() || s == "auto" => {}
            other => {
                let value = other.as_str().map(String::from).unwrap_or_else(|| other.to_string());
                out.extend(template.iter().map(|t| subst(&t.replace("{value}", &value))));
            }
        }
    }
    out
}

/// Expands the `env` templates of a launch recipe. An entry whose template references a
/// placeholder that has no (or an empty) value is skipped instead of exporting a literal
/// `{ctx_size}` or an empty variable: Ollama's `OLLAMA_CONTEXT_LENGTH` must stay unset when no
/// context was requested, so the server keeps its own default.
pub fn expand_env(launch: &Launch, vars: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, template) in &launch.env {
        let mut value = template.clone();
        let mut skip = false;
        for key in placeholders(template) {
            match vars.get(&key) {
                Some(v) if !v.is_empty() => value = value.replace(&format!("{{{key}}}"), v),
                _ => skip = true,
            }
        }
        if !skip {
            out.push((name.clone(), value));
        }
    }
    out
}

/// `{identifier}` placeholders of a template, in order.
fn placeholders(template: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(len) = rest[open..].find('}') else { break };
        let key = &rest[open + 1..open + len];
        if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            out.push(key.to_string());
        }
        rest = &rest[open + len + 1..];
    }
    out
}

/// Built-in catalog shipped in the binary.
pub fn builtin() -> Vec<RuntimeManifest> {
    serde_json::from_str(include_str!("../manifests/builtin.json")).expect("builtin runtime manifests are valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builtin_catalog_is_valid_and_covers_every_platform() {
        let all = builtin();
        assert!(all.len() >= 25, "catalog has {} runtimes", all.len());
        for os in ["windows", "macos", "linux", "android", "ios"] {
            assert!(all.iter().filter(|m| m.supports_platform(os)).count() >= 4, "few runtimes for {os}");
        }
        let mut ids: Vec<_> = all.iter().map(|m| &m.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "duplicate ids");
        for m in &all {
            if m.kind == "sidecar" {
                assert!(m.launch.is_some() || m.external_endpoint.is_some(), "{} has no way to connect", m.id);
            }
        }
    }

    #[test]
    fn expands_llama_server_arguments() {
        let m = builtin().into_iter().find(|m| m.id == "llama-server").unwrap();
        let settings =
            m.settings.resolve(json!({"ctx_size": 4096, "flash_attn": true, "extra_args": ["--mlock"]}).as_object().unwrap()).unwrap();
        let vars = BTreeMap::from([
            ("port".to_string(), "9000".to_string()),
            ("model_path".to_string(), "/m/q.gguf".to_string()),
            ("model".to_string(), "q".to_string()),
        ]);
        let args = expand_args(m.launch.as_ref().unwrap(), &settings, &vars);
        let joined = args.join(" ");
        assert!(joined.contains("--port 9000"), "{joined}");
        assert!(joined.contains("-m /m/q.gguf"), "{joined}");
        assert!(joined.contains("-c 4096"), "{joined}");
        assert!(joined.contains("--flash-attn on"), "{joined}");
        assert!(joined.ends_with("--mlock") || joined.contains("--mlock"), "{joined}");
    }

    #[test]
    fn rpc_worker_binds_where_asked_and_checks_tcp() {
        let m = builtin().into_iter().find(|m| m.id == "llama-rpc-worker").unwrap();
        let launch = m.launch.as_ref().unwrap();
        assert_eq!(launch.health, "tcp");
        let settings = json!({"bind": "0.0.0.0", "cache": true, "device": ""}).as_object().unwrap().clone();
        let vars: BTreeMap<String, String> =
            [("bind".to_string(), "0.0.0.0".to_string()), ("port".to_string(), "50052".to_string())].into();
        assert_eq!(expand_args(launch, &settings, &vars), vec!["-H", "0.0.0.0", "-p", "50052", "-c"]);
    }
}
