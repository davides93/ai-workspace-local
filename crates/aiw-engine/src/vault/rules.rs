//! Which documents of the store hold secrets, and which strings in them: **the one table** that decides it.
//!
//! The vault seals and opens what this table points at; the backup deletes or scrubs the same kinds. A kind that holds a
//! secret is added here and nowhere else: from then on `Store::put_doc`, `get_doc` and `list_docs` seal and open it by
//! themselves (a test reads the sources and fails when a kind the engine writes is in none of the lists).
//!
//! * [`WHOLE_KINDS`]: the document is made of secrets. Every string in it is sealed, and a backup leaves the kind out.
//! * [`BY_NAME_KINDS`]: the document holds a secret among other things. The strings whose key names a secret
//!   ([`is_secret_name`](crate::secrets::is_secret_name): `Authorization`, `token`, `api_key`, `GITHUB_TOKEN`…), every
//!   value of a `secrets` map (the channels) and every address that carries credentials (`https://user:password@host`,
//!   `?api_key=…`) are sealed; a backup keeps the document with those emptied.

use crate::secrets::is_secret_name;
use serde_json::Value;

/// Documents that are secrets in themselves (IMAP passwords, the tokens and pins of paired devices, OAuth tokens of MCP servers).
pub const WHOLE_KINDS: [&str; 3] = ["secret", "peer_token", "mcp_oauth"];

/// Documents that carry a secret among other things (MCP servers, channels, model providers, remote agents, runtime settings).
pub const BY_NAME_KINDS: [&str; 5] = ["mcp", "channel", "model_provider", "remote_agent", "runtime_settings"];

/// Documents of the vault itself (which keychain item holds the key): never in a backup, never sealed.
pub const INTERNAL_KINDS: [&str; 1] = ["vault"];

/// How the strings of a kind are picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Whole,
    ByName,
}

/// The rule of `kind`, or `None` when it holds no secret.
pub fn rule_for(kind: &str) -> Option<Rule> {
    if WHOLE_KINDS.contains(&kind) {
        Some(Rule::Whole)
    } else if BY_NAME_KINDS.contains(&kind) {
        Some(Rule::ByName)
    } else {
        None
    }
}

/// Every kind that holds a secret.
pub fn secret_kinds() -> impl Iterator<Item = &'static str> {
    WHOLE_KINDS.into_iter().chain(BY_NAME_KINDS)
}

/// Calls `f` on every string of `doc` that is a secret under the rule of `kind` (nothing for a kind without secrets). `f` may
/// change the string in place: that is how documents are sealed, opened and scrubbed.
pub fn visit_secrets(kind: &str, doc: &mut Value, f: &mut dyn FnMut(&mut String)) {
    match rule_for(kind) {
        Some(Rule::Whole) => visit_all(doc, f),
        Some(Rule::ByName) => visit_by_name(doc, f),
        None => {}
    }
}

fn visit_all(v: &mut Value, f: &mut dyn FnMut(&mut String)) {
    match v {
        Value::String(s) => f(s),
        Value::Object(m) => m.values_mut().for_each(|x| visit_all(x, f)),
        Value::Array(a) => a.iter_mut().for_each(|x| visit_all(x, f)),
        _ => {}
    }
}

/// The same picks as the backup's scrubbing has always made: a string under a secret-looking key, anywhere in the document,
/// every value of a `secrets` map, and an address with credentials in it (sealed whole: the backup strips them, the vault
/// cannot seal a part of a string).
fn visit_by_name(v: &mut Value, f: &mut dyn FnMut(&mut String)) {
    match v {
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if k == "secrets" {
                    if let Value::Object(s) = x {
                        s.values_mut().for_each(|y| {
                            if let Value::String(s) = y {
                                f(s)
                            }
                        });
                        continue;
                    }
                }
                match x {
                    Value::String(s) if is_secret_name(k) => f(s),
                    // The two cases below are what makes sealing and opening agree: an address that carries credentials is
                    // picked while it is plain, and once sealed it no longer looks like one, so a sealed string is picked too.
                    Value::String(s) if has_credentials(s) || aiw_core::sealed::is_sealed(s) => f(s),
                    Value::String(_) => {}
                    other => visit_by_name(other, f),
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| visit_by_name(x, f)),
        _ => {}
    }
}

/// Does this address carry credentials (a password in the user part, a key or token in the query)?
fn has_credentials(s: &str) -> bool {
    s.contains("://") && crate::backup::scrub_url(s) != s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn picked(kind: &str, mut doc: Value) -> Vec<String> {
        let mut out = vec![];
        visit_secrets(kind, &mut doc, &mut |s| out.push(s.clone()));
        out
    }

    #[test]
    fn a_whole_document_is_all_secret() {
        let mut got = picked("peer_token", json!({"token": "T1", "pin": "AA:BB", "n": 3, "inner": {"x": "deep"}}));
        got.sort();
        assert_eq!(got, ["AA:BB", "T1", "deep"]); // the number is not a string
    }

    #[test]
    fn a_mixed_document_gives_only_the_secret_looking_strings_and_the_secrets_map() {
        let doc = json!({
            "headers": {"Authorization": "Bearer X1", "Accept": "json"},
            "env": {"GITHUB_TOKEN": "ghp_x", "PATH": "/usr/bin", "N": 5},
            "secrets": {"bot_token": "T1", "anything": "A1"},
            "nested": [{"api_key": "K1", "name": "ok"}],
            "url": "https://mcp.example.org/mcp",
        });
        let mut got = picked("mcp", doc.clone());
        got.sort();
        assert_eq!(got, ["A1", "Bearer X1", "K1", "T1", "ghp_x"]);
        // What is not a kind with secrets gives nothing.
        assert!(picked("agent", doc).is_empty());
    }

    #[test]
    fn an_address_with_credentials_is_a_secret_and_a_plain_one_is_not() {
        let doc = json!({
            "url": "https://utente:PASSWORD@mcp.example.org:8443/mcp?q=ciao",
            "endpoint": "https://api.example.org/v1?api_key=K9",
            "other": "https://mcp.example.org/mcp?q=ciao#frag",
            "name": "https://not-a-secret.example.org",
        });
        let mut got = picked("remote_agent", doc);
        got.sort();
        assert_eq!(got, ["https://api.example.org/v1?api_key=K9", "https://utente:PASSWORD@mcp.example.org:8443/mcp?q=ciao"]);
    }

    #[test]
    fn a_sealed_string_is_picked_wherever_it_sits_so_that_what_was_sealed_by_content_opens_again() {
        let doc = json!({"url": "aiwenc1:ab12cd34:AAAA", "name": "plain", "nested": {"endpoint": "aiwenc1:ab12cd34:BBBB"}});
        let mut got = picked("mcp", doc);
        got.sort();
        assert_eq!(got, ["aiwenc1:ab12cd34:AAAA", "aiwenc1:ab12cd34:BBBB"]);
    }

    #[test]
    fn the_visitor_can_change_the_strings_in_place() {
        let mut doc = json!({"token": "abc", "name": "keep"});
        visit_secrets("model_provider", &mut doc, &mut |s| *s = format!("<{s}>"));
        assert_eq!(doc, json!({"token": "<abc>", "name": "keep"}));
    }

    #[test]
    fn the_lists_do_not_overlap() {
        for k in WHOLE_KINDS {
            assert!(!BY_NAME_KINDS.contains(&k) && !INTERNAL_KINDS.contains(&k), "{k}");
        }
        for k in BY_NAME_KINDS {
            assert!(!INTERNAL_KINDS.contains(&k), "{k}");
        }
        assert_eq!(rule_for("secret"), Some(Rule::Whole));
        assert_eq!(rule_for("mcp"), Some(Rule::ByName));
        assert_eq!(rule_for("vault"), None);
        assert_eq!(secret_kinds().count(), WHOLE_KINDS.len() + BY_NAME_KINDS.len());
    }
}
