//! OAuth 2.1 for remote MCP servers (MCP authorization): discovery (RFC 9728 protected resource
//! metadata, RFC 8414 authorization server metadata), dynamic client registration (RFC 7591),
//! authorization code + PKCE (S256) and the `resource` parameter (RFC 8707), token exchange and refresh.
//!
//! Everything a server publishes is untrusted: every URL must be `https` (or `http` on loopback, for
//! local development), the issuer in the metadata must be the one the metadata was fetched for,
//! the resource of the metadata must be the server the person configured, and the authorization,
//! token and registration endpoints must be on the origin of the issuer: a token endpoint somewhere
//! else is refused, because that is where codes, verifiers and refresh tokens would go.

use aiw_core::{Error, Result};
use base64::Engine as _;
use reqwest::Url;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;

/// Largest metadata or token answer read.
const MAX_BODY: usize = 1_000_000;

/// The parameters of a `WWW-Authenticate: Bearer …` challenge that matter here.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Challenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
}

pub fn parse_www_authenticate(header: &str) -> Challenge {
    let mut c = Challenge::default();
    let mut rest = header;
    // key="value" or key=token pairs, in any order, separated by commas or spaces.
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].rsplit(|ch: char| ch == ',' || ch.is_whitespace()).next().unwrap_or("").to_ascii_lowercase();
        let after = &rest[eq + 1..];
        let (value, next) = if let Some(q) = after.strip_prefix('"') {
            match q.find('"') {
                Some(end) => (q[..end].to_string(), &q[end + 1..]),
                None => (q.to_string(), ""),
            }
        } else {
            let end = after.find([',', ' ']).unwrap_or(after.len());
            (after[..end].to_string(), &after[end..])
        };
        match key.as_str() {
            "resource_metadata" => c.resource_metadata = Some(value),
            "scope" => c.scope = Some(value),
            _ => {}
        }
        rest = next;
    }
    c
}

fn is_loopback(host: &str) -> bool {
    host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// An address that may be used: https, or http on loopback; no credentials in it, no fragment.
pub fn check_url(raw: &str, what: &str) -> Result<Url> {
    let u = Url::parse(raw).map_err(|_| Error::Invalid(format!("{what}: indirizzo non valido")))?;
    let host = u.host_str().map(|h| h.trim_matches(['[', ']']).to_ascii_lowercase()).unwrap_or_default();
    let ok = match u.scheme() {
        "https" => !host.is_empty(),
        "http" => is_loopback(&host),
        _ => false,
    };
    if !ok {
        return Err(Error::Invalid(format!("{what}: serve un indirizzo https (http solo su questo dispositivo)")));
    }
    if !u.username().is_empty() || u.password().is_some() || u.fragment().is_some() {
        return Err(Error::Invalid(format!("{what}: l'indirizzo contiene credenziali o frammenti")));
    }
    Ok(u)
}

pub fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme() && a.host_str() == b.host_str() && a.port_or_known_default() == b.port_or_known_default()
}

pub fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        // A token or a code never follows a redirect to somewhere else.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(Error::other)
}

async fn read_json(res: reqwest::Response, what: &str) -> Result<Value> {
    let status = res.status();
    if res.content_length().is_some_and(|n| n as usize > MAX_BODY) {
        return Err(Error::Upstream(format!("{what}: risposta troppo grande")));
    }
    let bytes = res.bytes().await.map_err(|e| Error::Upstream(format!("{what}: {e}")))?;
    if bytes.len() > MAX_BODY {
        return Err(Error::Upstream(format!("{what}: risposta troppo grande")));
    }
    let v: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Upstream(format!("{what}: risposta non valida ({status})")))?;
    if !status.is_success() {
        let detail = v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("errore");
        return Err(Error::Upstream(format!("{what}: {detail} ({status})")));
    }
    Ok(v)
}

async fn get_json(http: &reqwest::Client, url: &Url, what: &str) -> Result<Value> {
    let res =
        http.get(url.clone()).header("Accept", "application/json").send().await.map_err(|e| Error::Upstream(format!("{what}: {e}")))?;
    read_json(res, what).await
}

#[derive(Debug, Clone, PartialEq)]
pub struct Discovery {
    /// The canonical address of the MCP server (the `resource` parameter).
    pub resource: String,
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub scope: Option<String>,
}

/// The canonical form of the server address, used as `resource`.
pub fn canonical_resource(server_url: &str) -> Result<String> {
    let mut u = check_url(server_url, "indirizzo del server MCP")?;
    u.set_query(None);
    let s = u.to_string();
    Ok(if u.path() == "/" { s.trim_end_matches('/').to_string() } else { s })
}

/// Finds the authorization server of `server_url` starting from the 401 `challenge`.
pub async fn discover(http: &reqwest::Client, server_url: &str, challenge: &Challenge) -> Result<Discovery> {
    let server = check_url(server_url, "indirizzo del server MCP")?;
    let resource = canonical_resource(server_url)?;
    let origin = server.origin().ascii_serialization();
    // 1. Protected resource metadata.
    let mut candidates = Vec::new();
    if let Some(m) = &challenge.resource_metadata {
        candidates.push(m.clone());
    }
    let path = server.path().trim_end_matches('/');
    if !path.is_empty() {
        candidates.push(format!("{origin}/.well-known/oauth-protected-resource{path}"));
    }
    candidates.push(format!("{origin}/.well-known/oauth-protected-resource"));
    let mut prm = None;
    let mut last = Error::Upstream("il server non pubblica i metadati OAuth (protected resource)".into());
    for c in candidates {
        let url = match check_url(&c, "metadati della risorsa") {
            Ok(u) => u,
            Err(e) => {
                last = e;
                continue;
            }
        };
        match get_json(http, &url, "metadati della risorsa").await {
            Ok(v) => {
                prm = Some(v);
                break;
            }
            Err(e) => last = e,
        }
    }
    let prm = prm.ok_or(last)?;
    // The metadata must describe this server.
    let declared = check_url(prm["resource"].as_str().unwrap_or(""), "resource dei metadati")?;
    if !same_origin(&declared, &server) || !server.path().trim_end_matches('/').starts_with(declared.path().trim_end_matches('/')) {
        return Err(Error::Denied("i metadati OAuth descrivono un'altra risorsa: accesso rifiutato".into()));
    }
    let issuer_raw = prm["authorization_servers"]
        .as_array()
        .and_then(|a| a.iter().filter_map(|v| v.as_str()).next())
        .ok_or_else(|| Error::Upstream("i metadati non indicano un server di autorizzazione".into()))?;
    let issuer = check_url(issuer_raw, "server di autorizzazione")?;
    // 2. Authorization server metadata.
    let iorigin = issuer.origin().ascii_serialization();
    let ipath = issuer.path().trim_end_matches('/');
    let mut urls = Vec::new();
    if ipath.is_empty() {
        urls.push(format!("{iorigin}/.well-known/oauth-authorization-server"));
        urls.push(format!("{iorigin}/.well-known/openid-configuration"));
    } else {
        urls.push(format!("{iorigin}/.well-known/oauth-authorization-server{ipath}"));
        urls.push(format!("{iorigin}/.well-known/openid-configuration{ipath}"));
        urls.push(format!("{iorigin}{ipath}/.well-known/openid-configuration"));
    }
    let mut meta = None;
    let mut last = Error::Upstream("il server di autorizzazione non pubblica i suoi metadati".into());
    for u in urls {
        match get_json(http, &Url::parse(&u).map_err(Error::other)?, "metadati del server di autorizzazione").await {
            Ok(v) => {
                meta = Some(v);
                break;
            }
            Err(e) => last = e,
        }
    }
    let meta = meta.ok_or(last)?;
    // RFC 8414: the issuer of the metadata is the one it was fetched for.
    if meta["issuer"].as_str().map(|s| s.trim_end_matches('/')) != Some(issuer_raw.trim_end_matches('/')) {
        return Err(Error::Denied("l'issuer dei metadati non corrisponde al server di autorizzazione: accesso rifiutato".into()));
    }
    let endpoint = |key: &str, required: bool| -> Result<Option<String>> {
        match meta[key].as_str() {
            Some(raw) => {
                let u = check_url(raw, key)?;
                if !same_origin(&u, &issuer) {
                    return Err(Error::Denied(format!("{key} è su un'altra origine rispetto all'issuer: accesso rifiutato")));
                }
                Ok(Some(u.to_string()))
            }
            None if required => Err(Error::Upstream(format!("i metadati non indicano {key}"))),
            None => Ok(None),
        }
    };
    let authorization_endpoint = endpoint("authorization_endpoint", true)?.unwrap_or_default();
    let token_endpoint = endpoint("token_endpoint", true)?.unwrap_or_default();
    let registration_endpoint = endpoint("registration_endpoint", false)?;
    // MCP requires PKCE: a server that does not say it supports S256 is refused.
    if !meta["code_challenge_methods_supported"].as_array().is_some_and(|a| a.iter().any(|m| m == "S256")) {
        return Err(Error::Denied("il server di autorizzazione non dichiara il supporto a PKCE (S256): accesso rifiutato".into()));
    }
    Ok(Discovery {
        resource,
        issuer: issuer_raw.trim_end_matches('/').to_string(),
        authorization_endpoint,
        token_endpoint,
        registration_endpoint,
        scope: challenge.scope.clone(),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Registration {
    pub client_id: String,
    pub client_secret: Option<String>,
}

/// RFC 7591: registers this app as a public client with the redirect address of the engine.
pub async fn register_client(http: &reqwest::Client, d: &Discovery, redirect_uri: &str) -> Result<Registration> {
    let endpoint = d.registration_endpoint.as_ref().ok_or_else(|| {
        Error::Unsupported(
            "il server di autorizzazione non permette la registrazione automatica: indica un client id nelle impostazioni del server"
                .into(),
        )
    })?;
    let res = http
        .post(endpoint)
        .json(&json!({
            "client_name": "AI Workspace Local",
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none"
        }))
        .send()
        .await
        .map_err(|e| Error::Upstream(format!("registrazione del client: {e}")))?;
    let v = read_json(res, "registrazione del client").await?;
    let client_id =
        v["client_id"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| Error::Upstream("registrazione senza client_id".into()))?;
    Ok(Registration { client_id: client_id.into(), client_secret: v["client_secret"].as_str().map(String::from) })
}

/// 64 hex characters from 256 random bits.
pub fn random_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// (code verifier, code challenge S256).
pub fn pkce() -> (String, String) {
    let verifier = random_token();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

pub fn authorization_url(d: &Discovery, client_id: &str, redirect_uri: &str, state: &str, code_challenge: &str) -> Result<String> {
    let mut u = Url::parse(&d.authorization_endpoint).map_err(Error::other)?;
    {
        let mut q = u.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("state", state)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("resource", &d.resource);
        if let Some(s) = &d.scope {
            q.append_pair("scope", s);
        }
    }
    Ok(u.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

async fn token_request(http: &reqwest::Client, token_endpoint: &str, form: &[(&str, &str)]) -> Result<TokenSet> {
    let res = http.post(token_endpoint).form(form).send().await.map_err(|e| Error::Upstream(format!("richiesta del token: {e}")))?;
    let v = read_json(res, "richiesta del token").await?;
    let access_token =
        v["access_token"].as_str().filter(|t| !t.is_empty()).ok_or_else(|| Error::Upstream("risposta senza access_token".into()))?;
    if v["token_type"].as_str().is_some_and(|t| !t.eq_ignore_ascii_case("bearer")) {
        return Err(Error::Upstream("tipo di token non supportato (serve Bearer)".into()));
    }
    Ok(TokenSet {
        access_token: access_token.into(),
        refresh_token: v["refresh_token"].as_str().map(String::from),
        expires_in: v["expires_in"].as_u64(),
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn exchange_code(
    http: &reqwest::Client,
    token_endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
    resource: &str,
) -> Result<TokenSet> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", verifier),
        ("resource", resource),
    ];
    if let Some(s) = client_secret {
        form.push(("client_secret", s));
    }
    token_request(http, token_endpoint, &form).await
}

pub async fn refresh(
    http: &reqwest::Client,
    token_endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
    resource: &str,
) -> Result<TokenSet> {
    let mut form =
        vec![("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("client_id", client_id), ("resource", resource)];
    if let Some(s) = client_secret {
        form.push(("client_secret", s));
    }
    token_request(http, token_endpoint, &form).await
}

/// Asks the server without a token and returns its `WWW-Authenticate` header (`None`: no sign-in needed).
pub async fn probe_challenge(http: &reqwest::Client, server_url: &str) -> Result<Option<String>> {
    let res = http
        .post(server_url)
        .header("Accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": crate::mcp::PROTOCOL_VERSION, "capabilities": {},
            "clientInfo": {"name": "ai-workspace-local", "version": aiw_core::VERSION}}}))
        .send()
        .await
        .map_err(|e| Error::Upstream(format!("server MCP: {e}")))?;
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Ok(Some(res.headers().get("www-authenticate").and_then(|h| h.to_str().ok()).unwrap_or("").to_string()));
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
