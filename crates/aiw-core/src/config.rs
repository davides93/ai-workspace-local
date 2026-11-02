//! Layered configuration (architecture §7): `defaults → global → device → workspace`.
//! Each layer is a flat JSON object of dotted keys stored in its own file; later layers win.

use crate::api::{ConfigLayerView, ConfigView};
use crate::schema::SettingsSchema;
use crate::{Error, Result};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Global,
    Device,
    Workspace,
}

impl Layer {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "global" => Ok(Layer::Global),
            "device" => Ok(Layer::Device),
            "workspace" => Ok(Layer::Workspace),
            other => Err(Error::Invalid(format!("livello di configurazione sconosciuto: `{other}` (usa global, device o workspace)"))),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Layer::Global => "global",
            Layer::Device => "device",
            Layer::Workspace => "workspace",
        }
    }
}

/// Application-level settings exposed in the UI "Impostazioni" page.
pub fn app_schema() -> SettingsSchema {
    serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "ui.theme": {"type": "string", "title": "Tema", "enum": ["system", "light", "dark"], "default": "system"},
            "ui.language": {"type": "string", "title": "Lingua", "enum": ["it", "en"], "default": "it"},
            "agent.default": {"type": "string", "title": "Agente predefinito", "default": "operator"},
            "policy.mode": {"type": "string", "title": "Modalità di approvazione (default)", "enum": ["plan", "ask", "auto-edit", "auto", "yolo"], "default": "ask"},
            "sandbox.mode": {"type": "string", "title": "Sandbox (default)", "enum": ["read-only", "workspace-write", "off"], "default": "workspace-write"},
            "context.compact_at": {"type": "number", "title": "Compatta il contesto al", "default": 0.8, "minimum": 0.3, "maximum": 0.95, "step": 0.05},
            "network.allow_domains": {"type": "array", "title": "Domini consentiti a web/browser (vuoto = tutti i pubblici)", "items": {"type": "string"}, "default": []},
            "server.lan": {"type": "boolean", "title": "Accesso dalla LAN (richiede pairing)", "default": false},
            "server.port": {"type": "integer", "title": "Porta API", "default": 7777, "minimum": 1024, "maximum": 65535},
            "models.dir": {"type": "string", "title": "Cartella modelli", "default": ""},
            "workspace.root": {"type": "string", "title": "Cartella di lavoro degli agenti", "default": ""},
            "telemetry.otlp": {"type": "string", "title": "Endpoint OTLP (vuoto = disattivato)", "default": ""}
        }
    }))
    .expect("static schema")
}

pub struct LayeredConfig {
    schema: SettingsSchema,
    global_path: PathBuf,
    device_path: PathBuf,
    workspace_path: RwLock<Option<PathBuf>>,
    global: RwLock<Map<String, Value>>,
    device: RwLock<Map<String, Value>>,
    workspace: RwLock<Map<String, Value>>,
}

fn read_layer(p: &Path) -> Map<String, Value> {
    match std::fs::read_to_string(p) {
        Ok(s) => match serde_json::from_str::<Value>(&s) {
            Ok(Value::Object(m)) => m,
            _ => {
                tracing::warn!(path = %p.display(), "config layer is not a JSON object; ignoring");
                Map::new()
            }
        },
        Err(_) => Map::new(),
    }
}

fn write_layer(p: &Path, m: &Map<String, Value>) -> Result<()> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&Value::Object(m.clone()))?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

impl LayeredConfig {
    pub fn load(schema: SettingsSchema, global_path: PathBuf, device_path: PathBuf, workspace_path: Option<PathBuf>) -> Self {
        let global = read_layer(&global_path);
        let device = read_layer(&device_path);
        let workspace = workspace_path.as_deref().map(read_layer).unwrap_or_default();
        Self {
            schema,
            global_path,
            device_path,
            workspace_path: RwLock::new(workspace_path),
            global: RwLock::new(global),
            device: RwLock::new(device),
            workspace: RwLock::new(workspace),
        }
    }

    pub fn schema(&self) -> &SettingsSchema {
        &self.schema
    }

    /// Points the workspace layer at `<root>/.aiw/config.json` and reloads it.
    pub fn set_workspace_root(&self, root: Option<&Path>) {
        let p = root.map(|r| r.join(".aiw").join("config.json"));
        *self.workspace.write().unwrap() = p.as_deref().map(read_layer).unwrap_or_default();
        *self.workspace_path.write().unwrap() = p;
    }

    pub fn effective(&self) -> Map<String, Value> {
        let mut out = self.schema.defaults();
        for l in [&self.global, &self.device, &self.workspace] {
            for (k, v) in l.read().unwrap().iter() {
                out.insert(k.clone(), v.clone());
            }
        }
        out
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        for l in [&self.workspace, &self.device, &self.global] {
            if let Some(v) = l.read().unwrap().get(key) {
                return Some(v.clone());
            }
        }
        self.schema.properties.get(key).and_then(|f| f.default.clone())
    }

    pub fn get_str(&self, key: &str) -> Option<String> {
        self.get(key).and_then(|v| v.as_str().map(str::to_string)).filter(|s| !s.is_empty())
    }
    pub fn get_bool(&self, key: &str) -> bool {
        self.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
    }
    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(|v| v.as_f64())
    }

    /// Which layer provides the effective value of `key`.
    pub fn source_of(&self, key: &str) -> &'static str {
        if self.workspace.read().unwrap().contains_key(key) {
            "workspace"
        } else if self.device.read().unwrap().contains_key(key) {
            "device"
        } else if self.global.read().unwrap().contains_key(key) {
            "global"
        } else {
            "defaults"
        }
    }

    /// Sets (or with `Value::Null`, removes) a key in a layer and persists the layer file.
    pub fn set(&self, layer: Layer, key: &str, value: Value) -> Result<()> {
        if let Some(f) = self.schema.properties.get(key) {
            f.validate(key, &value).map_err(Error::Invalid)?;
        }
        let (lock, path) = match layer {
            Layer::Global => (&self.global, Some(self.global_path.clone())),
            Layer::Device => (&self.device, Some(self.device_path.clone())),
            Layer::Workspace => (&self.workspace, self.workspace_path.read().unwrap().clone()),
        };
        let path = path.ok_or_else(|| Error::Invalid("nessuna cartella di lavoro aperta".into()))?;
        let mut m = lock.write().unwrap();
        if value.is_null() {
            m.remove(key);
        } else {
            m.insert(key.to_string(), value);
        }
        write_layer(&path, &m)
    }

    pub fn view(&self) -> ConfigView {
        let defaults = self.schema.defaults();
        ConfigView {
            layers: vec![
                ConfigLayerView { name: "defaults".into(), path: None, values: defaults },
                ConfigLayerView {
                    name: "global".into(),
                    path: Some(self.global_path.display().to_string()),
                    values: self.global.read().unwrap().clone(),
                },
                ConfigLayerView {
                    name: "device".into(),
                    path: Some(self.device_path.display().to_string()),
                    values: self.device.read().unwrap().clone(),
                },
                ConfigLayerView {
                    name: "workspace".into(),
                    path: self.workspace_path.read().unwrap().as_ref().map(|p| p.display().to_string()),
                    values: self.workspace.read().unwrap().clone(),
                },
            ],
            effective: self.effective(),
            schema: self.schema.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_layers_win_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        let cfg = LayeredConfig::load(app_schema(), dir.path().join("config.json"), dir.path().join("dev.json"), None);
        cfg.set_workspace_root(Some(&ws));
        assert_eq!(cfg.get_str("policy.mode").as_deref(), Some("ask"));
        assert_eq!(cfg.source_of("policy.mode"), "defaults");

        cfg.set(Layer::Global, "policy.mode", json!("auto")).unwrap();
        cfg.set(Layer::Workspace, "policy.mode", json!("plan")).unwrap();
        assert_eq!(cfg.get_str("policy.mode").as_deref(), Some("plan"));
        assert_eq!(cfg.source_of("policy.mode"), "workspace");

        // invalid values are rejected by the schema
        assert!(cfg.set(Layer::Global, "policy.mode", json!("nope")).is_err());

        // reload from disk
        let again = LayeredConfig::load(
            app_schema(),
            dir.path().join("config.json"),
            dir.path().join("dev.json"),
            Some(ws.join(".aiw/config.json")),
        );
        assert_eq!(again.get_str("policy.mode").as_deref(), Some("plan"));
        again.set(Layer::Workspace, "policy.mode", Value::Null).unwrap();
        assert_eq!(again.get_str("policy.mode").as_deref(), Some("auto"));
    }
}
