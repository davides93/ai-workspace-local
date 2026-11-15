//! The credentials of one agent (issue #71): a token or a key that belongs to one agent, is sent only to the hosts it was given
//! for, and that no other agent can use.
//!
//! * **Where it lives.** A document of the kind `agent_credential`, which the vault table (`vault/rules.rs`) lists: the `secret` is
//!   sealed with the key of the keychain like every other secret of the app, and the backup leaves it out (`secret` emptied) unless
//!   the person asks for the secrets. Who owns it, its name and the hosts stay readable: they are not secrets.
//! * **Who sets it.** Only the owner: the API route `PUT /api/agents/{id}/credentials/{name}` (full access) and `aiw agents
//!   credential set`. No tool, agent, model or setting can: the Assistant has no tool for it, and `aiw_agent_save` neither reads
//!   nor writes them. The model never types a value and never sees one.
//! * **How it is used.** By the engine, never by the model: the tool `web_fetch_auth` names a credential and the engine adds the
//!   header ([`AgentCredentials`], a [`CredentialBroker`] bound to one agent). Read at every call, so removing the credential
//!   or the agent stops its use at once, also for a run in progress. Only https, only the hosts it was given for (re-checked at
//!   every redirect), and what a server echoes back is stripped of the value.
//! * **Who gets it.** The agent's own run, started by the person or by a workflow. Not a background proposal, not a channel run
//!   (text from outside), not an evaluation. A sub-agent uses the credentials of its parent, never its own.

use crate::isolation::{base_agent_id, is_derived_agent_id, is_safe_dir_name};
use crate::{now_rfc3339, Engine};
use aiw_core::api::{AgentCredentialInfo, AgentCredentialInput};
use aiw_core::{Error, Result};
use aiw_tools::{CredentialBroker, CredentialUse};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::sync::Arc;

/// Kind of the document (listed in `vault/rules.rs`).
const CREDENTIAL_KIND: &str = "agent_credential";
const MAX_NAME: usize = 40;
const MAX_HOSTS: usize = 8;
const MAX_PER_AGENT: usize = 16;
const MIN_SECRET: usize = 8;
const MAX_SECRET: usize = 4096;
const MAX_PREFIX: usize = 32;

/// What is stored. The field called `secret` is the one the vault seals (a key that names a secret); the others must not name
/// one, or they would be sealed too.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Doc {
    agent: String,
    name: String,
    hosts: Vec<String>,
    header: String,
    prefix: String,
    secret: String,
    created_at: String,
    updated_at: String,
}

fn doc_id(agent: &str, name: &str) -> String {
    // An agent id cannot contain `:` (`is_safe_dir_name`), so the pair is never ambiguous.
    format!("{agent}:{name}")
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// A host the credential may be sent to: a name with at least one dot, `localhost`, or an IPv4 address. A bare ending such as `com`
/// would cover half of the internet, so it is refused.
fn normalize_host(raw: &str) -> Result<String> {
    let h = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    let bad = || {
        Error::Invalid(format!(
            "«{}» non è un host valido: scrivi solo il nome (per esempio api.github.com), senza http://, porta o percorso",
            raw.trim()
        ))
    };
    if h.is_empty() || h.len() > 253 || h.contains(['/', ':', '@', '?', '#', '*', ' ', '\\']) {
        return Err(bad());
    }
    if h.parse::<Ipv4Addr>().is_ok() || h == "localhost" {
        return Ok(h);
    }
    let labels: Vec<&str> = h.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    if labels.len() < 2 || !labels.iter().all(label_ok) {
        return Err(bad());
    }
    Ok(h)
}

/// Is `host` one of `hosts` or a subdomain of one? An address is matched only as it is.
fn in_scope(hosts: &[String], host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    hosts.iter().any(|d| host == *d || (d.parse::<Ipv4Addr>().is_err() && host.ends_with(&format!(".{d}"))))
}

/// Headers that decide where a request goes or how it is framed: not for a credential.
fn forbidden_header(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["host", "content-length", "transfer-encoding", "connection", "upgrade", "te", "trailer", "expect", "via", "forwarded"]
        .contains(&n.as_str())
        || n.starts_with("proxy-")
        || n.starts_with("x-forwarded-")
}

fn clean_value(raw: &str) -> Result<String> {
    let v = raw.trim().to_string();
    if v.chars().any(char::is_control) {
        return Err(Error::Invalid("il valore non può contenere a capo o caratteri di controllo".into()));
    }
    if v.len() < MIN_SECRET {
        return Err(Error::Invalid(format!("il valore è troppo corto (almeno {MIN_SECRET} caratteri): una credenziale vera è più lunga")));
    }
    if v.len() > MAX_SECRET {
        return Err(Error::Invalid(format!("il valore è troppo lungo (al massimo {MAX_SECRET} caratteri)")));
    }
    Ok(v)
}

impl Doc {
    fn info(&self) -> AgentCredentialInfo {
        AgentCredentialInfo {
            name: self.name.clone(),
            hosts: self.hosts.clone(),
            header: self.header.clone(),
            prefix: self.prefix.clone(),
            // A value sealed with a key this computer does not have now is not usable: it counts as not set.
            set: !self.secret.is_empty() && !aiw_core::sealed::is_sealed(&self.secret),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }
}

impl Engine {
    fn credential_docs(&self, agent_id: &str) -> Result<Vec<Doc>> {
        // The owner is compared as a whole string, never as a prefix: `a` does not own what `ab` holds.
        let mut docs: Vec<Doc> = self.0.store.list_docs::<Doc>(CREDENTIAL_KIND)?.into_iter().filter(|d| d.agent == agent_id).collect();
        docs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(docs)
    }

    /// The credentials of an agent as the API lists them: names, hosts, state. Never a value.
    pub fn agent_credentials(&self, agent_id: &str) -> Result<Vec<AgentCredentialInfo>> {
        self.agent(agent_id)?;
        Ok(self.credential_docs(agent_id)?.iter().map(Doc::info).collect())
    }

    /// Creates or changes a credential of an agent. Only the owner reaches this (an API route of full access, the command line).
    /// A value left out of a change keeps the stored one. The value is sealed by the vault like every secret of the app.
    pub fn set_agent_credential(&self, agent_id: &str, name: &str, input: AgentCredentialInput) -> Result<AgentCredentialInfo> {
        self.agent(agent_id)?;
        if !is_safe_dir_name(agent_id) {
            return Err(Error::Invalid(format!("l'id «{agent_id}» non può avere credenziali (solo lettere, cifre, - _ .)")));
        }
        let name = name.trim();
        if !valid_name(name) {
            return Err(Error::Invalid(format!(
                "il nome della credenziale deve avere al massimo {MAX_NAME} caratteri tra lettere, cifre, - e _"
            )));
        }
        if input.hosts.is_empty() {
            return Err(Error::Invalid("indica almeno un host a cui la credenziale può essere mandata".into()));
        }
        if input.hosts.len() > MAX_HOSTS {
            return Err(Error::Invalid(format!("al massimo {MAX_HOSTS} host per credenziale")));
        }
        let mut hosts: Vec<String> = Vec::new();
        for h in &input.hosts {
            let h = normalize_host(h)?;
            if !hosts.contains(&h) {
                hosts.push(h);
            }
        }
        let stored: Option<Doc> = self.0.store.get_doc::<Doc>(CREDENTIAL_KIND, &doc_id(agent_id, name))?.filter(|d| d.agent == agent_id);
        if stored.is_none() && self.credential_docs(agent_id)?.len() >= MAX_PER_AGENT {
            return Err(Error::Invalid(format!("al massimo {MAX_PER_AGENT} credenziali per agente")));
        }
        let header = input
            .header
            .as_deref()
            .map(str::trim)
            .filter(|h| !h.is_empty())
            .map(String::from)
            .or_else(|| stored.as_ref().map(|d| d.header.clone()))
            .unwrap_or_else(|| "Authorization".into());
        if reqwest::header::HeaderName::from_bytes(header.as_bytes()).is_err() || forbidden_header(&header) {
            return Err(Error::Invalid(format!("«{header}» non può essere l'intestazione di una credenziale")));
        }
        let prefix = match &input.prefix {
            Some(p) => p.clone(),
            None if input.header.is_some() || stored.is_none() => {
                if header.eq_ignore_ascii_case("authorization") {
                    "Bearer ".into()
                } else {
                    String::new()
                }
            }
            None => stored.as_ref().map(|d| d.prefix.clone()).unwrap_or_default(),
        };
        if prefix.len() > MAX_PREFIX || prefix.chars().any(char::is_control) {
            return Err(Error::Invalid("il prefisso del valore è troppo lungo o contiene caratteri di controllo".into()));
        }
        let secret = match input.value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            Some(v) => clean_value(v)?,
            None => match stored.as_ref().map(|d| d.secret.clone()).filter(|s| !s.is_empty()) {
                Some(s) => s,
                None => return Err(Error::Invalid("manca il valore della credenziale".into())),
            },
        };
        let now = now_rfc3339();
        let doc = Doc {
            agent: agent_id.into(),
            name: name.into(),
            hosts,
            header,
            prefix,
            secret,
            created_at: stored.map(|d| d.created_at).unwrap_or_else(|| now.clone()),
            updated_at: now,
        };
        self.0.store.put_doc(CREDENTIAL_KIND, &doc_id(agent_id, name), &doc)?;
        self.audit_user("agent.credential_set", &format!("{agent_id}/{name} → {}", doc.hosts.join(", ")));
        Ok(doc.info())
    }

    /// Removes one credential of an agent: from the next call nobody can use it.
    pub fn remove_agent_credential(&self, agent_id: &str, name: &str) -> Result<()> {
        let id = doc_id(agent_id, name.trim());
        if self.0.store.get_doc::<Doc>(CREDENTIAL_KIND, &id)?.filter(|d| d.agent == agent_id).is_none() {
            return Err(Error::NotFound(format!("l'agente {agent_id} non ha una credenziale «{}»", name.trim())));
        }
        self.0.store.delete_doc(CREDENTIAL_KIND, &id)?;
        self.audit_user("agent.credential_removed", &format!("{agent_id}/{}", name.trim()));
        Ok(())
    }

    /// Removes every credential of an agent (when the agent is removed). Returns how many there were.
    pub(crate) fn remove_agent_credentials(&self, agent_id: &str) -> Result<usize> {
        let docs = self.credential_docs(agent_id)?;
        for d in &docs {
            self.0.store.delete_doc(CREDENTIAL_KIND, &doc_id(agent_id, &d.name))?;
        }
        if !docs.is_empty() {
            self.audit_user("agent.credential_removed", &format!("{agent_id}: {} credenziali (agente rimosso)", docs.len()));
        }
        Ok(docs.len())
    }

    /// Does the agent hold at least one credential?
    pub(crate) fn agent_has_credentials(&self, agent_id: &str) -> bool {
        self.credential_docs(base_agent_id(agent_id)).is_ok_and(|d| !d.is_empty())
    }

    /// The broker of an agent's own run. `None` for a run that is not the agent talking (a background proposal, a channel).
    pub(crate) fn credential_broker(&self, agent_id: &str) -> Option<Arc<dyn CredentialBroker>> {
        (!is_derived_agent_id(agent_id))
            .then(|| Arc::new(AgentCredentials { engine: self.clone(), agent: agent_id.to_string() }) as Arc<dyn CredentialBroker>)
    }

    /// The line of the prompt that tells an agent which credentials it has (names and hosts, never a value), or `None` when
    /// there is nothing to say or the agent cannot use them.
    pub(crate) fn credentials_prompt(broker: &Arc<dyn CredentialBroker>, tools: &[String]) -> Option<String> {
        if !tools.iter().any(|t| t == aiw_tools::web::WEB_FETCH_AUTH) {
            return None;
        }
        let list = broker.describe();
        if list.is_empty() {
            return None;
        }
        let items: Vec<String> = list.into_iter().map(|(n, h)| format!("`{n}` (solo verso {})", h.join(", "))).collect();
        Some(format!(
            "# Credenziali di questo agente\nHai queste credenziali: {}. Si usano con lo strumento {} (argomento `credential`): le aggiunge l'app, tu non ne vedi mai il valore e non devi chiederlo né scriverlo.",
            items.join("; "),
            aiw_tools::web::WEB_FETCH_AUTH
        ))
    }
}

/// The credentials of one agent, for the tools of its runs. Everything is read from the store at the moment of the call.
struct AgentCredentials {
    engine: Engine,
    agent: String,
}

impl CredentialBroker for AgentCredentials {
    fn describe(&self) -> Vec<(String, Vec<String>)> {
        self.engine
            .credential_docs(&self.agent)
            .unwrap_or_default()
            .into_iter()
            .filter(|d| d.info().set)
            .map(|d| (d.name, d.hosts))
            .collect()
    }

    fn authorize(&self, name: &str, url: &reqwest::Url, insecure_loopback_ok: bool) -> Result<CredentialUse> {
        // An agent that was removed has no credentials, whatever a run still in progress holds.
        if self.engine.agent(&self.agent).is_err() {
            return Err(Error::Denied("l'agente non esiste più: le sue credenziali non valgono".into()));
        }
        let name = name.trim();
        let doc = self
            .engine
            .0
            .store
            .get_doc::<Doc>(CREDENTIAL_KIND, &doc_id(&self.agent, name))?
            .filter(|d| d.agent == self.agent && d.name == name)
            .ok_or_else(|| Error::Denied(format!("questo agente non ha una credenziale «{name}»")))?;
        if !doc.info().set {
            return Err(Error::Denied(format!(
                "la credenziale «{name}» non ha un valore leggibile su questo computer (controlla lo stato dei segreti)"
            )));
        }
        let host = url.host_str().unwrap_or_default().to_string();
        if !in_scope(&doc.hosts, &host) {
            return Err(Error::Denied(format!(
                "la credenziale «{name}» non vale per {host}: l'utente l'ha data solo per {}",
                doc.hosts.join(", ")
            )));
        }
        let secure = url.scheme() == "https" || (insecure_loopback_ok && url.scheme() == "http" && aiw_tools::web::is_loopback_host(url));
        if !secure {
            return Err(Error::Denied(format!("la credenziale «{name}» viaggia solo in https")));
        }
        self.engine.audit_system("agent.credential_used", &format!("{}/{name} → {host}", self.agent));
        Ok(CredentialUse { header: doc.header, value: format!("{}{}", doc.prefix, doc.secret), secret: doc.secret })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_names_with_a_dot_or_localhost_or_an_address_and_nothing_wider() {
        for (raw, want) in [
            ("API.GitHub.com", "api.github.com"),
            (" example.org. ", "example.org"),
            ("localhost", "localhost"),
            ("127.0.0.1", "127.0.0.1"),
        ] {
            assert_eq!(normalize_host(raw).unwrap(), want);
        }
        for bad in [
            "",
            "com",
            "https://example.org",
            "example.org/x",
            "example.org:8443",
            "utente@example.org",
            "*.example.org",
            "-a.example.org",
            "a..b",
            "a b.org",
            "[::1]",
        ] {
            assert!(normalize_host(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_scope_covers_the_host_and_its_subdomains_and_an_address_only_as_it_is() {
        let hosts = vec!["example.org".to_string(), "10.0.0.5".to_string()];
        for ok in ["example.org", "api.example.org", "A.B.Example.org.", "10.0.0.5"] {
            assert!(in_scope(&hosts, ok), "{ok}");
        }
        for no in ["badexample.org", "example.org.evil.com", "org", "1.10.0.0.5", "evil.com", "x.10.0.0.5"] {
            assert!(!in_scope(&hosts, no), "{no}");
        }
    }

    #[test]
    fn the_names_the_vault_seals_are_the_secret_and_nothing_else() {
        // The document must hold its secret under a key the vault seals, and no other field under such a key.
        let doc = serde_json::to_value(Doc {
            agent: "alfa".into(),
            name: "github".into(),
            hosts: vec!["api.github.com".into()],
            header: "Authorization".into(),
            prefix: "Bearer ".into(),
            secret: "S3CRET-VALUE".into(),
            created_at: "t".into(),
            updated_at: "t".into(),
        })
        .unwrap();
        let mut sealed = vec![];
        crate::vault::rules::visit_secrets(CREDENTIAL_KIND, &mut doc.clone(), &mut |s| sealed.push(s.clone()));
        assert_eq!(sealed, ["S3CRET-VALUE"]);
    }

    #[test]
    fn a_header_that_routes_or_frames_a_request_cannot_carry_a_credential() {
        for bad in ["Host", "host", "Content-Length", "Transfer-Encoding", "Proxy-Authorization", "X-Forwarded-For", "Via", "Forwarded"] {
            assert!(forbidden_header(bad), "{bad}");
        }
        for ok in ["Authorization", "X-Api-Key", "PRIVATE-TOKEN"] {
            assert!(!forbidden_header(ok), "{ok}");
        }
    }
}
