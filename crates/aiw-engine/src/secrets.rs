//! Secrets of the MCP server configurations (`headers` of the HTTP transport, `env` of the stdio
//! one): the API never returns them, it returns [`SECRET_MASK`] instead, and receiving the mask
//! back on save means "keep what is stored". Same convention as the secret runtime settings.

use aiw_core::api::{McpServerConfig, SECRET_MASK};
use serde_json::{Map, Value};

/// Name fragments (lower case) of the headers and variables whose value is a secret.
const SECRET_WORDS: [&str; 9] = ["auth", "cookie", "token", "key", "secret", "password", "passwd", "credential", "signature"];

/// Does the header or variable called `name` carry a secret (`Authorization`, `Cookie`,
/// `X-Api-Key`, `GITHUB_TOKEN`, `DB_PASSWORD`…)?
pub fn is_secret_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    SECRET_WORDS.iter().any(|w| n.contains(w))
}

/// `map` with the value of every secret entry replaced by the mask (an empty string stays empty:
/// there is nothing to hide, and the editor can show that it is not set).
fn mask(map: &Map<String, Value>) -> Map<String, Value> {
    map.iter()
        .map(|(k, v)| {
            let hide = is_secret_name(k) && !v.is_null() && v.as_str().is_none_or(|s| !s.is_empty());
            (k.clone(), if hide { Value::String(SECRET_MASK.into()) } else { v.clone() })
        })
        .collect()
}

/// `new` with every mask put back to the value `stored` has under the same key. A mask with no
/// stored value behind it (a new server, a new key) is dropped: it is never a real secret.
fn restore(new: Map<String, Value>, stored: Option<&Map<String, Value>>) -> Map<String, Value> {
    new.into_iter()
        .filter_map(|(k, v)| {
            if v.as_str() != Some(SECRET_MASK) {
                return Some((k, v));
            }
            stored.and_then(|s| s.get(&k)).cloned().map(|old| (k, old))
        })
        .collect()
}

/// The configuration as the API shows it.
pub fn masked(mut cfg: McpServerConfig) -> McpServerConfig {
    cfg.headers = cfg.headers.as_ref().map(mask);
    cfg.env = cfg.env.as_ref().map(mask);
    cfg
}

/// The configuration a client sent, with the masks it sent back replaced by the stored secrets.
pub fn unmasked(mut cfg: McpServerConfig, stored: Option<&McpServerConfig>) -> McpServerConfig {
    cfg.headers = cfg.headers.take().map(|h| restore(h, stored.and_then(|s| s.headers.as_ref())));
    cfg.env = cfg.env.take().map(|e| restore(e, stored.and_then(|s| s.env.as_ref())));
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn secret_names_are_recognised_whatever_the_case() {
        for n in [
            "Authorization",
            "authorization",
            "Proxy-Authorization",
            "Cookie",
            "Set-Cookie",
            "X-Api-Key",
            "X-Auth-Token",
            "GITHUB_PERSONAL_ACCESS_TOKEN",
            "OPENAI_API_KEY",
            "CLIENT_SECRET",
            "DB_PASSWORD",
            "x-signature",
        ] {
            assert!(is_secret_name(n), "{n}");
        }
        for n in ["Accept", "Content-Type", "X-Request-Id", "PATH", "HOME", "LANG", "User-Agent"] {
            assert!(!is_secret_name(n), "{n}");
        }
    }

    #[test]
    fn masking_hides_secrets_only() {
        let m = mask(&map(json!({"Authorization": "Bearer abc", "Accept": "text/plain", "X-Api-Key": "", "API_TOKEN": 12, "N": null})));
        assert_eq!(
            Value::Object(m),
            json!({"Authorization": "••••••", "Accept": "text/plain", "X-Api-Key": "", "API_TOKEN": "••••••", "N": null})
        );
    }

    #[test]
    fn a_mask_sent_back_keeps_the_stored_value() {
        let stored = McpServerConfig {
            id: "m".into(),
            name: "m".into(),
            transport: "http".into(),
            command: None,
            args: None,
            url: Some("http://x/mcp".into()),
            env: Some(map(json!({"API_KEY": "k1", "MODE": "a"}))),
            headers: Some(map(json!({"Authorization": "Bearer one", "Cookie": "c=1", "Accept": "x"}))),
            enabled: true,
            sampling: false,
            oauth_client_id: None,
        };
        // What the editor got is what it sends back, with one change of its own.
        let mut sent = masked(stored.clone());
        assert_eq!(sent.headers.as_ref().unwrap()["Authorization"], SECRET_MASK);
        sent.headers.as_mut().unwrap().insert("Accept".into(), json!("y"));
        sent.headers.as_mut().unwrap().insert("Cookie".into(), json!("c=2")); // a new secret replaces the old one
        sent.env.as_mut().unwrap().remove("MODE"); // a removed row is removed
        let back = unmasked(sent, Some(&stored));
        assert_eq!(Value::Object(back.headers.unwrap()), json!({"Authorization": "Bearer one", "Cookie": "c=2", "Accept": "y"}));
        assert_eq!(Value::Object(back.env.unwrap()), json!({"API_KEY": "k1"}));
        // A mask with nothing stored behind it is not stored.
        let fresh =
            unmasked(McpServerConfig { headers: Some(map(json!({"Authorization": "••••••", "Accept": "x"}))), ..stored.clone() }, None);
        assert_eq!(Value::Object(fresh.headers.unwrap()), json!({"Accept": "x"}));
        // Without headers or env nothing appears.
        let bare = unmasked(McpServerConfig { headers: None, env: None, ..stored }, None);
        assert!(bare.headers.is_none() && bare.env.is_none());
    }
}
