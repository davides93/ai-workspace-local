//! Device mesh: pairing with another AIW device on the LAN.
//!
//! A paired device becomes (1) a remote runtime (`aiw-peer`: its OpenAI-compatible gateway and
//! the models it serves) and (2) an MCP server (its tools and `aiw_agent_run`, so local agents can
//! delegate tasks to the agents running there). Its token stays on this device.

use crate::Engine;
use aiw_core::api::{McpServerConfig, PeerDevice, PeerHardware, SystemInfo};
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

/// Client for the paired device at `base`: an `https://` address is verified against the pinned
/// certificate and nothing else (never the system roots, never "accept any certificate").
fn peer_client(base: &str, pin: Option<&str>, timeout: Duration) -> Result<reqwest::Client> {
    let mut b = reqwest::Client::builder().timeout(timeout);
    if base.starts_with("https://") {
        let pin = pin.ok_or_else(|| Error::Invalid("serve l'impronta del certificato del dispositivo".into()))?;
        let cfg = aiw_core::tlspin::pinned_client_config(pin).map_err(Error::Invalid)?;
        b = b.use_preconfigured_tls(cfg);
    }
    b.build().map_err(Error::other)
}

impl Engine {
    /// Pins the certificates of the paired devices that are served over TLS (at start).
    pub(crate) fn register_peer_pins(&self) {
        for p in self.peers() {
            let pin = self.0.store.get_doc::<Value>("peer_token", &p.id).ok().flatten().and_then(|v| v["pin"].as_str().map(String::from));
            if let Some(pin) = pin {
                aiw_core::tlspin::register(&p.address, &pin);
            }
        }
    }

    /// Pairs with the device at `address` using the 6-digit code it shows. For an `https://` address
    /// `fingerprint` (SHA-256 of its certificate, from its QR code or from its Security page) is
    /// required: the connection is checked against it, and so are all the later ones.
    pub async fn pair_with_peer(&self, address: &str, code: &str, fingerprint: Option<&str>) -> Result<PeerDevice> {
        let base = normalize_address(address)?;
        let pin = match fingerprint.map(str::trim).filter(|f| !f.is_empty()) {
            Some(f) => Some(aiw_core::tlspin::normalize_fingerprint(f).ok_or_else(|| {
                Error::Invalid("impronta del certificato non valida (servono 64 cifre esadecimali, anche con i due punti)".into())
            })?),
            None => None,
        };
        if base.starts_with("https://") && pin.is_none() {
            return Err(Error::Invalid(
                "il dispositivo usa HTTPS: serve l'impronta del suo certificato (la trovi in Sicurezza → Rete su quel dispositivo, o nel suo QR di pairing)".into(),
            ));
        }
        let http = peer_client(&base, pin.as_deref(), Duration::from_secs(20))?;
        let me = self.device();
        let r = http
            .post(format!("{base}/api/pair"))
            .json(&json!({ "code": code.trim(), "device_id": me.id, "name": me.name }))
            .send()
            .await
            .map_err(|e| http_error(&base, e))?;
        if r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            // The other device is rate limiting pairing attempts: its message says when to retry.
            let why = r.json::<Value>().await.ok().and_then(|v| v["error"].as_str().map(String::from));
            return Err(Error::Denied(format!(
                "{base}: {}",
                why.unwrap_or_else(|| "troppi tentativi di pairing, riprova più tardi".into())
            )));
        }
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
            hardware: Some(PeerHardware {
                arch: sys.device.arch.clone(),
                tier: sys.device.tier.clone(),
                memory_mb: sys.device.memory_mb,
                vram_mb: sys.device.vram_mb(),
                accelerators: sys.device.accelerators.iter().map(|a| a.kind.clone()).collect(),
                unified_memory: sys.device.unified_memory(),
            }),
        };
        self.0.store.put_doc("peer", &id, &peer)?;
        self.0.store.put_doc("peer_token", &id, &json!({ "token": token, "pin": pin }))?;
        match &pin {
            Some(pin) => aiw_core::tlspin::register(&base, pin),
            None => aiw_core::tlspin::unregister(&base),
        }

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
            sampling: false,
            oauth_client_id: None,
        };
        if let Err(e) = self.save_mcp_server(mcp).await {
            tracing::warn!("MCP del dispositivo {}: {e}", peer.name);
        }
        self.audit_system("pair_peer", &format!("accoppiato con {} ({base})", peer.name));
        Ok(peer)
    }

    /// Best effort: asks the other device to revoke the token it issued to us.
    async fn release_peer_token(&self, address: &str, token: &str, pin: Option<&str>) {
        if token.is_empty() {
            return;
        }
        let Ok(http) = peer_client(address, pin, Duration::from_secs(5)) else { return };
        let _ = http.delete(format!("{address}/api/auth/token")).bearer_auth(token).send().await;
    }

    pub fn peers(&self) -> Vec<PeerDevice> {
        self.0.store.list_docs("peer").unwrap_or_default()
    }

    /// Forgets a paired device: its token, MCP server and, if it was the active one, the
    /// `aiw-peer` runtime settings.
    pub async fn remove_peer(&self, id: &str) -> Result<()> {
        let peer: PeerDevice = self.0.store.get_doc("peer", id)?.ok_or_else(|| Error::NotFound(format!("dispositivo {id} non trovato")))?;
        let held: Option<Value> = self.0.store.get_doc("peer_token", id)?;
        self.0.store.delete_doc("peer", id)?;
        self.0.store.delete_doc("peer_token", id)?;
        // Tell the other device that it can drop the token it gave us (it may also revoke it itself at any time).
        if let Some(v) = held {
            self.release_peer_token(&peer.address, v["token"].as_str().unwrap_or_default(), v["pin"].as_str()).await;
        }
        aiw_core::tlspin::unregister(&peer.address);
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
