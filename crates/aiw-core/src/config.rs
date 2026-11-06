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
            "approvals.timeout_s": {"type": "integer", "title": "Tempo massimo per rispondere a una richiesta di approvazione (secondi)", "description": "0 = nessun limite. Allo scadere la richiesta dell'agente viene rifiutata", "default": 0, "minimum": 0, "maximum": 86400},
            "policy.mode": {"type": "string", "title": "Modalità di approvazione (default)", "enum": ["plan", "ask", "auto-edit", "auto", "yolo"], "default": "ask"},
            "sandbox.mode": {"type": "string", "title": "Sandbox (default)", "enum": ["read-only", "workspace-write", "off"], "default": "workspace-write"},
            "context.compact_at": {"type": "number", "title": "Compatta il contesto al", "default": 0.8, "minimum": 0.3, "maximum": 0.95, "step": 0.05},
            "agents.max_parallel_subagents": {"type": "integer", "title": "Subagenti in parallelo", "default": 3, "minimum": 1, "maximum": 8},
            "agents.budget.max_tokens": {"type": "integer", "title": "Budget di token per run (0 = nessun limite)", "description": "Somma di prompt e risposta di tutte le chiamate al modello, subagenti compresi; vale se l'agente non ha un suo limite", "default": 0, "minimum": 0},
            "agents.budget.max_turns": {"type": "integer", "title": "Budget di turni per run (0 = nessun limite)", "description": "Chiamate al modello di tutto il run, subagenti compresi; vale se l'agente non ha un suo limite", "default": 0, "minimum": 0},
            "agents.budget.max_seconds": {"type": "integer", "title": "Tempo massimo per run, in secondi (0 = nessun limite)", "description": "Vale se l'agente non ha un suo limite", "default": 0, "minimum": 0},
            "network.allow_domains": {"type": "array", "title": "Domini consentiti a web/browser (vuoto = tutti i pubblici)", "items": {"type": "string"}, "default": []},
            "server.lan": {"type": "boolean", "title": "Accesso dalla LAN (richiede pairing)", "default": false},
            "server.port": {"type": "integer", "title": "Porta API", "default": 7777, "minimum": 1024, "maximum": 65535},
            "models.dir": {"type": "string", "title": "Cartella modelli", "default": ""},
            "workspace.root": {"type": "string", "title": "Cartella di lavoro degli agenti", "default": ""},
            "telemetry.otlp": {"type": "string", "title": "Endpoint OTLP (vuoto = disattivato)", "default": ""},
            "update.channel": {"type": "string", "title": "Canale di aggiornamento", "description": "auto segue la versione installata (una beta resta sulle beta); stable = produzione; beta e alpha ricevono anche le versioni più stabili", "enum": ["auto", "stable", "beta", "alpha"], "default": "auto"},
            "update.auto_check": {"type": "boolean", "title": "Cerca aggiornamenti da solo", "description": "All'avvio e ogni 6 ore, dalle release del repository pubblico su GitHub", "default": true},
            "update.auto_download": {"type": "boolean", "title": "Scarica da solo gli aggiornamenti", "description": "L'installer viene scaricato e verificato (SHA-256) in anticipo; l'installazione parte solo quando la confermi", "default": false},
            "voice.language": {"type": "string", "title": "Lingua della voce", "description": "Codice ISO 639-1 (it, en…) o auto per riconoscerla", "default": "auto"},
            "voice.stt_runtime": {"type": "string", "title": "Motore di trascrizione", "description": "auto: whisper.cpp, poi un server locale che trascrive, poi un dispositivo accoppiato", "enum": ["auto", "whisper-server", "localai", "nexa", "aiw-peer", "remote-openai"], "default": "auto"},
            "voice.stt_model": {"type": "string", "title": "Modello Whisper", "description": "Vuoto = il più grande installato che sta in memoria", "default": ""},
            "voice.tts_voice": {"type": "string", "title": "Voce per leggere le risposte", "description": "Nome di una voce locale del sistema; vuoto = la prima per la lingua", "default": ""}
        }
    }))
    .expect("static schema")
}

/// Security keys that the workspace layer (repository content) may only make stricter. The
/// list is ordered from the strictest value to the loosest.
const WORKSPACE_TIGHTEN_ONLY: &[(&str, &[&str])] =
    &[("policy.mode", &["plan", "ask", "auto-edit", "auto", "yolo"]), ("sandbox.mode", &["read-only", "workspace-write", "off"])];

/// Machine-level keys the workspace layer cannot set: network reach, the listener, where models
/// and the working folder live, and where telemetry goes.
const WORKSPACE_FORBIDDEN: &[&str] = &[
    "network.allow_domains",
    "server.lan",
    "server.port",
    "models.dir",
    "workspace.root",
    "telemetry.otlp",
    "approvals.timeout_s",
    "update.channel",
    "update.auto_check",
    "update.auto_download",
];

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
        for l in [&self.global, &self.device] {
            for (k, v) in l.read().unwrap().iter() {
                out.insert(k.clone(), v.clone());
            }
        }
        let workspace = self.workspace.read().unwrap().clone();
        for (k, v) in workspace {
            if let Some(v) = self.workspace_value(&k, Some(&v)) {
                out.insert(k, v);
            }
        }
        out
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        let ws = self.workspace.read().unwrap().get(key).cloned();
        if let Some(v) = self.workspace_value(key, ws.as_ref()) {
            return Some(v);
        }
        self.machine_value(key)
    }

    /// Value of `key` from the layers the user controls (defaults → global → device).
    fn machine_value(&self, key: &str) -> Option<Value> {
        for l in [&self.device, &self.global] {
            if let Some(v) = l.read().unwrap().get(key) {
                return Some(v.clone());
            }
        }
        self.schema.properties.get(key).and_then(|f| f.default.clone())
    }

    /// The workspace layer lives in the folder being worked on (`<root>/.aiw/config.json`), i.e.
    /// it is repository content, not a decision of the user. It may only make the security keys
    /// stricter and it cannot touch machine-level keys at all; `None` means "ignored".
    fn workspace_value(&self, key: &str, v: Option<&Value>) -> Option<Value> {
        let v = v?;
        if WORKSPACE_FORBIDDEN.contains(&key) {
            return None;
        }
        if let Some((_, order)) = WORKSPACE_TIGHTEN_ONLY.iter().find(|(k, _)| *k == key) {
            let rank = |v: &Value| v.as_str().and_then(|s| order.iter().position(|o| *o == s));
            let base = self.machine_value(key);
            return match (rank(v), base.as_ref().and_then(rank)) {
                (Some(w), Some(b)) if w <= b => Some(v.clone()),
                (Some(_), None) => Some(v.clone()),
                _ => None,
            };
        }
        Some(v.clone())
    }

    /// Workspace-layer keys that are currently ignored because they would loosen security or
    /// touch machine settings (shown by the UI next to the layer).
    pub fn ignored_workspace_keys(&self) -> Vec<String> {
        let ws = self.workspace.read().unwrap().clone();
        ws.iter().filter(|(k, v)| self.workspace_value(k, Some(v)).is_none()).map(|(k, _)| k.clone()).collect()
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
        let ws = self.workspace.read().unwrap().get(key).cloned();
        if self.workspace_value(key, ws.as_ref()).is_some() {
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
            ignored_workspace_keys: self.ignored_workspace_keys(),
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

    #[test]
    fn a_repository_cannot_loosen_security_or_touch_machine_keys() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(ws.join(".aiw")).unwrap();
        // What a hostile repository would ship in its own .aiw/config.json.
        std::fs::write(
            ws.join(".aiw/config.json"),
            r#"{"policy.mode":"yolo","sandbox.mode":"off","network.allow_domains":["evil.example"],"server.lan":true,"models.dir":"/","context.compact_at":0.5}"#,
        )
        .unwrap();
        let cfg = LayeredConfig::load(app_schema(), dir.path().join("config.json"), dir.path().join("dev.json"), None);
        cfg.set_workspace_root(Some(&ws));
        assert_eq!(cfg.get_str("policy.mode").as_deref(), Some("ask"));
        assert_eq!(cfg.get_str("sandbox.mode").as_deref(), Some("workspace-write"));
        assert!(!cfg.get_bool("server.lan"));
        assert_eq!(cfg.get("network.allow_domains"), Some(json!([])));
        assert_eq!(cfg.get_str("models.dir"), None);
        assert_eq!(cfg.source_of("sandbox.mode"), "defaults");
        let eff = cfg.effective();
        assert_eq!(eff["policy.mode"], json!("ask"));
        assert_eq!(eff["sandbox.mode"], json!("workspace-write"));
        // Harmless keys still apply, and the ignored ones are reported.
        assert_eq!(cfg.get_f64("context.compact_at"), Some(0.5));
        let mut ignored = cfg.ignored_workspace_keys();
        ignored.sort();
        assert_eq!(ignored, ["models.dir", "network.allow_domains", "policy.mode", "sandbox.mode", "server.lan"]);
        // Making things stricter is allowed, also relative to what the user chose.
        cfg.set(Layer::Global, "sandbox.mode", json!("off")).unwrap();
        cfg.set(Layer::Workspace, "sandbox.mode", json!("read-only")).unwrap();
        assert_eq!(cfg.get_str("sandbox.mode").as_deref(), Some("read-only"));
        cfg.set(Layer::Workspace, "sandbox.mode", json!("off")).unwrap();
        assert_eq!(cfg.get_str("sandbox.mode").as_deref(), Some("off"), "equal to the user's own choice");
    }
}
