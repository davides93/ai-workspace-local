//! Device mesh: pairing with another AIW device on the LAN.
//!
//! A paired device becomes (1) a remote runtime (`aiw-peer`: its OpenAI-compatible gateway and
//! the models it serves) and (2) an MCP server (its tools and `aiw_agent_run`, so local agents can
//! delegate tasks to the agents running there). Its token stays on this device.

use crate::Engine;
use aiw_core::api::{McpServerConfig, PeerDevice, SystemInfo};
use aiw_core::{Error, Result};
use serde_json::{json, Map, Value};
use std::time::Duration;

/// `192.168.1.20:7777`, `http://host:7777/` → `http://host:7777`.
pub fn normalize_address(address: &str) -> Result<String> {
    let a = address.trim().trim_end_matches('/');
    if a.is_empty() {
        return Err(Error::Invalid("indirizzo del dispositivo mancante".into()));
    }
    let a = if a.starts_with("http://") || a.starts_with("https://") { a.to_string() } else { format!("http://{a}") };
    let host_port = a.split("://").nth(1).unwrap_or_default();
    if host_port.is_empty() || host_port.contains('/') {
        return Err(Error::Invalid(format!("indirizzo non valido: {address} (usa host:porta)")));
    }
    Ok(if host_port.contains(':') && !host_port.ends_with(']') { a } else { format!("{a}:7777") })
}

fn slug(s: &str) -> String {
    let out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    out.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-").chars().take(32).collect()
}

fn http_error(what: &str, e: reqwest::Error) -> Error {
    Error::Runtime(format!("{what}: {e}"))
}

impl Engine {
    /// Pairs with the device at `address` using the 6-digit code it shows.
    pub async fn pair_with_peer(&self, address: &str, code: &str) -> Result<PeerDevice> {
        let base = normalize_address(address)?;
        let http = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().map_err(Error::other)?;
        let r =
            http.post(format!("{base}/api/pair")).json(&json!({ "code": code.trim() })).send().await.map_err(|e| http_error(&base, e))?;
        if !r.status().is_success() {
            return Err(Error::Denied(format!("{base} ha rifiutato il codice di pairing")));
        }
        let token = r.json::<Value>().await.map_err(|e| http_error("risposta di pairing", e))?["token"]
            .as_str()
            .filter(|t| !t.is_empty())
            .ok_or_else(|| Error::Runtime("il dispositivo non ha restituito un token".into()))?
            .to_string();
        let sys: SystemInfo = http
            .get(format!("{base}/api/system"))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| http_error(&base, e))?
            .json()
            .await
            .map_err(|e| http_error("informazioni del dispositivo", e))?;
        let models: Vec<String> = match http.get(format!("{base}/v1/models")).bearer_auth(&token).send().await {
            Ok(r) => r
                .json::<Value>()
                .await
                .ok()
                .and_then(|v| v["data"].as_array().map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect())),
            Err(_) => None,
        }
        .unwrap_or_default();
        let id = if sys.device.id.is_empty() { slug(&base) } else { slug(&sys.device.id) };
        if id == slug(&self.device().id) {
            return Err(Error::Invalid("questo è il dispositivo stesso".into()));
        }
        let peer = PeerDevice {
            id: id.clone(),
            name: sys.device.name.clone(),
            os: sys.device.os.clone(),
            address: base.clone(),
            status: "online".into(),
            shared_runtimes: models,
        };
        self.0.store.put_doc("peer", &id, &peer)?;
        self.0.store.put_doc("peer_token", &id, &json!({ "token": token }))?;

        // (1) Its models through its OpenAI-compatible gateway.
        let mut settings = Map::new();
        settings.insert("base_url".into(), json!(format!("{base}/v1")));
        settings.insert("api_key".into(), json!(token));
        self.update_runtime_settings("aiw-peer", settings)?;

        // (2) Its tools and agents through its MCP server.
        let mut headers = Map::new();
        headers.insert("Authorization".into(), json!(format!("Bearer {token}")));
        let mcp = McpServerConfig {
            id: format!("peer-{id}"),
            name: format!("peer_{}", slug(&peer.name).replace('-', "_")),
            transport: "http".into(),
            command: None,
            args: None,
            url: Some(format!("{base}/mcp")),
            env: None,
            headers: Some(headers),
            enabled: true,
        };
        if let Err(e) = self.save_mcp_server(mcp).await {
            tracing::warn!("MCP del dispositivo {}: {e}", peer.name);
        }
        self.audit_system("pair_peer", &format!("accoppiato con {} ({base})", peer.name));
        Ok(peer)
    }

    pub fn peers(&self) -> Vec<PeerDevice> {
        self.0.store.list_docs("peer").unwrap_or_default()
    }

    /// Forgets a paired device: its token, MCP server and, if it was the active one, the
    /// `aiw-peer` runtime settings.
    pub async fn remove_peer(&self, id: &str) -> Result<()> {
        let peer: PeerDevice = self.0.store.get_doc("peer", id)?.ok_or_else(|| Error::NotFound(format!("dispositivo {id}")))?;
        self.0.store.delete_doc("peer", id)?;
        self.0.store.delete_doc("peer_token", id)?;
        let _ = self.remove_mcp_server(&format!("peer-{id}")).await;
        let current = self.0.runtimes.settings("aiw-peer").unwrap_or_default();
        if current.get("base_url").and_then(|v| v.as_str()) == Some(&format!("{}/v1", peer.address)) {
            let mut cleared = Map::new();
            cleared.insert("base_url".into(), json!(""));
            cleared.insert("api_key".into(), json!(""));
            self.update_runtime_settings("aiw-peer", cleared)?;
        }
        self.audit_system("remove_peer", &format!("rimosso {}", peer.name));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_normalised() {
        assert_eq!(normalize_address("192.168.1.20").unwrap(), "http://192.168.1.20:7777");
        assert_eq!(normalize_address("http://pc.local:8080/").unwrap(), "http://pc.local:8080");
        assert!(normalize_address("http://pc/x").is_err());
        assert!(normalize_address(" ").is_err());
        assert_eq!(slug("Studio PC #2"), "studio-pc-2");
    }
}
