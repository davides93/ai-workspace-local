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
    #[serde(default)]
    pub accelerators: Vec<String>,
    #[serde(default)]
    pub formats: Vec<String>,
    /// openai | ollama | native
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
    /// Path appended to `http://127.0.0.1:{port}` for readiness checks.
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

impl RuntimeManifest {
    pub fn supports_platform(&self, os: &str) -> bool {
        self.platforms.iter().any(|p| p == os || p == "any")
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
}
