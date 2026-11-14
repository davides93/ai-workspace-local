//! Exportable diagnostics: one JSON document a person can attach to a bug report.
//!
//! **In it**: the version, the operating system and CPU architecture, the hardware as detected, the runtimes found and their
//! state, the settings with every secret masked, the database schema version and how many rows each table has, the checks the
//! app already runs on itself (sandbox, TLS, updates, what fine-tuning can run here), and the last lines of the newest log
//! file when there is one.
//!
//! **Never in it**: the content of chats, documents, memories or mail, and no secret. Settings go through the masking the
//! API already uses (`SECRET_MASK` for every setting the schema marks as secret); on top of that every string of the result,
//! the log lines included, goes through the redactor of the trace export, with the exact values of the secrets this
//! installation stores as the list of things to hide, so a secret that slipped into a path, a log or a URL is masked too.
//! Paths under the home folder are shown as `~` (they would name the person), and so are the computer's and paired devices'
//! names (left out).

use crate::traces::Redactor;
use crate::Engine;
use aiw_core::api::{McpServerConfig, SECRET_MASK};
use aiw_core::{now_rfc3339, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;

/// Layout of the document; raised when a field changes meaning.
pub const DIAGNOSTICS_FORMAT: u32 = 1;
/// Lines of the log that are kept.
const LOG_LINES: usize = 200;
/// Only the end of a log file is read: logs can be large.
const LOG_TAIL_BYTES: u64 = 256 * 1024;

fn walk_strings(v: &mut Value, f: &dyn Fn(&str) -> String) {
    match v {
        Value::String(s) => *s = f(s),
        Value::Array(a) => a.iter_mut().for_each(|x| walk_strings(x, f)),
        Value::Object(m) => m.values_mut().for_each(|x| walk_strings(x, f)),
        _ => {}
    }
}

/// The last `max` lines of the newest `*.log` or `*.txt` in `dir`, if any.
fn log_tail(dir: &Path, max: usize) -> Option<(String, Vec<String>)> {
    use std::io::{Read, Seek, SeekFrom};
    let newest = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()).is_some_and(|x| x == "log" || x == "txt"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(t, _)| *t)?
        .1;
    let mut f = std::fs::File::open(&newest).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(LOG_TAIL_BYTES))).ok()?;
    let mut bytes = vec![];
    f.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    if len > LOG_TAIL_BYTES && !lines.is_empty() {
        lines.remove(0); // probably cut in the middle
    }
    let skip = lines.len().saturating_sub(max);
    Some((newest.file_name()?.to_string_lossy().into_owned(), lines.split_off(skip)))
}

/// What a line of log can carry that the redactor of the traces does not know: `token=…`, `password: …`, credentials in URLs.
pub(crate) fn scrub_line(re: &[(Regex, &'static str)], s: &str) -> String {
    let mut out = s.to_string();
    for (r, to) in re {
        out = r.replace_all(&out, *to).into_owned();
    }
    out
}

pub(crate) fn line_patterns() -> Vec<(Regex, &'static str)> {
    [
        (r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=\-]{6,}", "${1} ••••••"),
        (r#"(?i)((?:api[_-]?key|token|secret|passw(?:or)?d|authorization|cookie|signature)["']?\s*[:=]\s*["']?)[^\s"',;&]+"#, "${1}••••••"),
        (r"(?i)(\b[a-z][a-z0-9+.\-]*://)[^/\s:@]+(?::[^/\s@]*)?@", "${1}••••••@"),
        (r"(?i)([?&](?:api[_-]?key|key|token|access_token|secret|password|sig|signature)=)[^\s&#]+", "${1}••••••"),
    ]
    .into_iter()
    .map(|(r, to)| (Regex::new(r).expect("static pattern"), to))
    .collect()
}

impl Engine {
    /// The exact values of the secrets this installation stores: the list the redactor is told to hide.
    pub(crate) fn known_secrets(&self) -> Vec<String> {
        let mut out = crate::traces::mcp_secret_values(&self.0.store);
        let schema = self.0.config.schema();
        for (k, f) in &schema.properties {
            if f.secret {
                if let Some(s) = self.0.config.get_str(k).filter(|s| !s.is_empty()) {
                    out.push(s);
                }
            }
        }
        let store = &self.0.store;
        for kind in ["secret", "peer_token", "model_provider", "remote_agent", "channel", "mcp_oauth"] {
            for doc in store.list_docs::<Value>(kind).unwrap_or_default() {
                collect_secret_strings(&doc, &mut out);
            }
        }
        for cfg in store.list_docs::<McpServerConfig>("mcp").unwrap_or_default() {
            if let Some(u) = cfg.url {
                out.extend(url_credentials(&u));
            }
        }
        if let Ok(t) = std::fs::read_to_string(self.0.dirs.root.join("token")) {
            out.push(t.trim().to_string());
        }
        out
    }

    /// The diagnostics document. See the module documentation for what is in it and what never is.
    pub async fn diagnostics(&self) -> Result<Value> {
        let sys = self.system(false);
        let d = &sys.device;
        let device = json!({
            "os": d.os, "os_version": d.os_version, "arch": d.arch, "form_factor": d.form_factor, "tier": d.tier,
            "cpu": d.cpu, "memory_mb": d.memory_mb, "available_memory_mb": d.available_memory_mb, "accelerators": d.accelerators,
        });
        let runtimes: Vec<Value> = self
            .runtimes()
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id, "kind": r.kind, "status": r.status, "detail": r.status_detail,
                    "loaded_models": r.loaded_models.len(), "accelerators": r.accelerators,
                })
            })
            .collect();
        let view = self.config();
        // Settings that name the person's accounts say how many there are, not which.
        let mut effective = view.effective.clone();
        if let Some(v) = effective.get_mut("connectors.mail.imap") {
            *v = json!({ "accounts": v.as_array().map_or(0, |a| a.len()) });
        }
        let tables: serde_json::Map<String, Value> = self.0.store.table_counts()?.into_iter().map(|(n, c)| (n, json!(c))).collect();
        let tls = self.tls_status();
        let update = self.update_status();
        let doctor = self.train_doctor().await;
        let local_models = self.models().len();
        let logs = log_tail(&self.0.dirs.logs(), LOG_LINES);
        let line_re = line_patterns();

        let mut doc = json!({
            "format": DIAGNOSTICS_FORMAT,
            "generated_at": now_rfc3339(),
            "contents": "Nessun contenuto di chat, documenti, memorie o posta. Segreti mascherati, percorsi della cartella personale come ~.",
            "app": { "version": sys.version, "mode": sys.mode, "uptime_s": sys.uptime_s },
            "system": { "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "device": device },
            "runtimes": runtimes,
            "local_models": local_models,
            "settings": {
                "effective": effective,
                "ignored_workspace_keys": view.ignored_workspace_keys,
                "layers": view.layers.iter().map(|l| json!({"name": l.name, "keys": l.values.keys().collect::<Vec<_>>()})).collect::<Vec<_>>(),
            },
            "database": {
                "schema_version": self.0.store.schema_version()?,
                "expected_schema_version": crate::store::SCHEMA_VERSION,
                "table_counts": tables,
            },
            "health": {
                "shell_sandbox": sys.shell_sandbox,
                "mcp_sandbox": sys.mcp_sandbox,
                "sandboxed": sys.sandboxed,
                "tls": { "mode": tls.mode, "active": tls.active, "lan": tls.lan, "port": tls.port, "expires_at": tls.expires_at, "error": tls.error, "names": tls.names.len() },
                "update": { "current": update.current, "channel": update.channel, "configured": update.configured, "state": update.state, "checked_at": update.checked_at, "error": update.error },
                "training": { "can_train": doctor.can_train, "advice": doctor.advice },
            },
            "logs": logs.map(|(file, lines)| json!({
                "file": file,
                "lines": lines.iter().map(|l| scrub_line(&line_re, l)).collect::<Vec<_>>(),
            })).unwrap_or(Value::Null),
        });

        // The safety net over the whole document.
        let redactor = Redactor::new(self.known_secrets());
        let home = dirs::home_dir().map(|h| h.display().to_string()).filter(|h| h.len() > 3);
        walk_strings(&mut doc, &|s| {
            let mut out = redactor.apply(s).replace("[SEGRETO]", SECRET_MASK);
            if let Some(h) = &home {
                out = out.replace(h.as_str(), "~");
            }
            out
        });
        Ok(doc)
    }
}

/// Every string of a stored document under a key that names a secret (and every value of a `secrets` map).
fn collect_secret_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                match x {
                    Value::String(s) if !s.is_empty() && (k == "value" || crate::secrets::is_secret_name(k)) => out.push(s.clone()),
                    Value::Object(inner) if k == "secrets" => {
                        out.extend(inner.values().filter_map(|y| y.as_str().filter(|s| !s.is_empty()).map(String::from)))
                    }
                    _ => collect_secret_strings(x, out),
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_secret_strings(x, out)),
        _ => {}
    }
}

/// The password and user of a `scheme://user:password@host` URL.
fn url_credentials(u: &str) -> Vec<String> {
    let Some(rest) = u.split_once("://").map(|x| x.1) else { return vec![] };
    let Some(auth) = rest.split('/').next().and_then(|a| a.rsplit_once('@')).map(|x| x.0) else { return vec![] };
    auth.split(':').filter(|p| p.len() >= 6).map(String::from).collect()
}
