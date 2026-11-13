//! OAuth sign-in of the HTTP MCP servers (see `aiw_tools::mcp_oauth` for the protocol). The tokens
//! are stored like the other secrets (never returned by the API, masked in exports), refreshed with
//! the refresh token, and handed only to the client of the server they were issued for.

use crate::Engine;
use aiw_core::api::{McpOAuthStart, McpServerConfig};
use aiw_core::{Error, Result};
use aiw_tools::mcp_oauth as oa;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const COLLECTION: &str = "mcp_oauth";
/// A sign-in that is not finished within this time is forgotten.
const PENDING_TTL: Duration = Duration::from_secs(600);
const PENDING_MAX: usize = 20;
/// The token is renewed when it expires within this many seconds.
const SKEW_S: i64 = 60;

/// What is stored for a server (collection `mcp_oauth`, keyed by the server id).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OAuthDoc {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    pub issuer: String,
    pub token_endpoint: String,
    /// Address of the MCP server the tokens are for.
    pub resource: String,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    pub expires_at: Option<i64>,
}

pub(crate) struct PendingAuth {
    server_id: String,
    verifier: String,
    redirect_uri: String,
    created: Instant,
}

fn now_s() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The token values stored, for the redaction of exports.
pub(crate) fn secret_values(store: &crate::store::Store) -> Vec<String> {
    let mut out = Vec::new();
    for d in store.list_docs::<OAuthDoc>(COLLECTION).unwrap_or_default() {
        out.extend(d.access_token);
        out.extend(d.refresh_token);
        out.extend(d.client_secret);
    }
    out.retain(|s| s.len() >= 8);
    out
}

impl Engine {
    fn oauth_doc(&self, id: &str) -> Option<OAuthDoc> {
        self.0.store.get_doc::<OAuthDoc>(COLLECTION, id).ok().flatten()
    }

    pub(crate) fn oauth_signed_in(&self, id: &str) -> bool {
        self.oauth_doc(id).is_some_and(|d| d.access_token.is_some() || d.refresh_token.is_some())
    }

    pub(crate) fn oauth_forget(&self, id: &str) {
        let _ = self.0.store.delete_doc(COLLECTION, id);
    }

    /// Signs out: forgets the tokens and reconnects (the server will ask for the sign-in again).
    pub async fn mcp_oauth_signout(&self, id: &str) -> Result<()> {
        self.oauth_forget(id);
        self.reconnect_mcp_server(id).await.map(|_| ())
    }

    /// The access token for the server `id`, renewed when it is about to expire (or when `force`).
    pub(crate) async fn oauth_access_token(&self, id: &str, force: bool) -> Option<String> {
        let mut doc = self.oauth_doc(id)?;
        // The tokens belong to one address: if the person points the server elsewhere, they are not sent.
        let cfg = self.0.store.get_doc::<McpServerConfig>("mcp", id).ok().flatten()?;
        let (stored, now) = (Url::parse(&doc.resource).ok()?, Url::parse(cfg.url.as_deref()?).ok()?);
        if !oa::same_origin(&stored, &now) {
            return None;
        }
        let fresh = doc.expires_at.is_none_or(|e| e - SKEW_S > now_s());
        if fresh && !force {
            return doc.access_token;
        }
        let refresh = doc.refresh_token.clone()?;
        let http = oa::http_client().ok()?;
        match oa::refresh(&http, &doc.token_endpoint, &doc.client_id, doc.client_secret.as_deref(), &refresh, &doc.resource).await {
            Ok(t) => {
                doc.access_token = Some(t.access_token.clone());
                doc.refresh_token = t.refresh_token.or(doc.refresh_token);
                doc.expires_at = t.expires_in.map(|s| now_s() + s as i64);
                let _ = self.0.store.put_doc(COLLECTION, id, &doc);
                Some(t.access_token)
            }
            Err(e) => {
                tracing::warn!(server = %cfg.name, "rinnovo del token OAuth non riuscito: {e}");
                // The refresh token is no good any more: the person has to sign in again.
                doc.access_token = None;
                doc.refresh_token = None;
                doc.expires_at = None;
                let _ = self.0.store.put_doc(COLLECTION, id, &doc);
                None
            }
        }
    }

    /// Starts the sign-in of a server: discovers the authorization server, registers the app when needed
    /// and returns the page the person opens in the browser. `base` is where this engine is reached
    /// by the browser (`http://127.0.0.1:7777`): the redirect address is `<base>/api/mcp/oauth/callback`.
    pub async fn mcp_oauth_start(&self, id: &str, base: &str) -> Result<McpOAuthStart> {
        let cfg =
            self.0.store.get_doc::<McpServerConfig>("mcp", id)?.ok_or_else(|| Error::NotFound(format!("server MCP {id} non trovato")))?;
        if cfg.transport != "http" {
            return Err(Error::Invalid("l'accesso OAuth serve solo ai server MCP http".into()));
        }
        let url = cfg.url.clone().unwrap_or_default();
        let base_url = oa::check_url(base, "indirizzo del motore")?;
        if base_url.scheme() != "http" || !matches!(base_url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")) {
            return Err(Error::Invalid(
                "per accedere apri l'app da questo dispositivo: il ritorno dal browser arriva su un indirizzo locale".into(),
            ));
        }
        let redirect_uri = format!("{}/api/mcp/oauth/callback", base_url.origin().ascii_serialization());
        let http = oa::http_client()?;
        let header = match self.mcp_client(id).and_then(|c| c.auth_challenge()) {
            Some(h) => h,
            None => {
                oa::probe_challenge(&http, &url).await?.ok_or_else(|| Error::Invalid("questo server MCP non richiede l'accesso".into()))?
            }
        };
        let d = oa::discover(&http, &url, &oa::parse_www_authenticate(&header)).await?;
        // Reuse the registration of an earlier sign-in with the same authorization server and redirect address.
        let previous = self.oauth_doc(id).filter(|p| p.issuer == d.issuer && p.redirect_uri == redirect_uri);
        let (client_id, client_secret) = match (previous, cfg.oauth_client_id.clone().filter(|c| !c.trim().is_empty())) {
            (_, Some(manual)) => (manual, None),
            (Some(p), None) => (p.client_id, p.client_secret),
            (None, None) => {
                let r = oa::register_client(&http, &d, &redirect_uri).await?;
                (r.client_id, r.client_secret)
            }
        };
        let kept = self.oauth_doc(id).filter(|p| p.resource == d.resource);
        self.0.store.put_doc(
            COLLECTION,
            id,
            &OAuthDoc {
                client_id: client_id.clone(),
                client_secret,
                redirect_uri: redirect_uri.clone(),
                issuer: d.issuer.clone(),
                token_endpoint: d.token_endpoint.clone(),
                resource: d.resource.clone(),
                access_token: kept.as_ref().and_then(|k| k.access_token.clone()),
                refresh_token: kept.as_ref().and_then(|k| k.refresh_token.clone()),
                expires_at: kept.and_then(|k| k.expires_at),
            },
        )?;
        let (verifier, challenge) = oa::pkce();
        let state = oa::random_token();
        {
            let mut p = self.0.oauth_pending.lock().unwrap();
            p.retain(|_, v| v.created.elapsed() < PENDING_TTL);
            if p.len() >= PENDING_MAX {
                return Err(Error::Conflict("troppi accessi in corso: completa o attendi quelli già avviati".into()));
            }
            p.insert(
                state.clone(),
                PendingAuth { server_id: id.into(), verifier, redirect_uri: redirect_uri.clone(), created: Instant::now() },
            );
        }
        Ok(McpOAuthStart { authorization_url: oa::authorization_url(&d, &client_id, &redirect_uri, &state, &challenge)? })
    }

    /// The browser comes back from the authorization server. `state` must be one this engine issued and
    /// not used before (it is consumed here, whatever the outcome). Returns the name of the server.
    pub async fn mcp_oauth_callback(&self, state: Option<&str>, code: Option<&str>, error: Option<&str>) -> Result<String> {
        let pending = {
            let mut p = self.0.oauth_pending.lock().unwrap();
            p.retain(|_, v| v.created.elapsed() < PENDING_TTL);
            state.and_then(|s| p.remove(s))
        };
        let Some(pending) = pending else {
            return Err(Error::Denied("richiesta di accesso sconosciuta o scaduta (state non valido)".into()));
        };
        if let Some(e) = error {
            let short: String = e.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(60).collect();
            return Err(Error::Denied(format!("accesso rifiutato dal server di autorizzazione ({short})")));
        }
        let code = code
            .filter(|c| !c.is_empty() && c.len() <= 4096)
            .ok_or_else(|| Error::Invalid("risposta senza codice di autorizzazione".into()))?;
        let mut doc = self.oauth_doc(&pending.server_id).ok_or_else(|| Error::NotFound("accesso non più in corso".into()))?;
        if doc.redirect_uri != pending.redirect_uri {
            return Err(Error::Denied("l'indirizzo di ritorno non corrisponde".into()));
        }
        let http = oa::http_client()?;
        let t = oa::exchange_code(
            &http,
            &doc.token_endpoint,
            &doc.client_id,
            doc.client_secret.as_deref(),
            code,
            &doc.redirect_uri,
            &pending.verifier,
            &doc.resource,
        )
        .await?;
        doc.access_token = Some(t.access_token);
        doc.refresh_token = t.refresh_token;
        doc.expires_at = t.expires_in.map(|s| now_s() + s as i64);
        self.0.store.put_doc(COLLECTION, &pending.server_id, &doc)?;
        let name = self.0.store.get_doc::<McpServerConfig>("mcp", &pending.server_id)?.map(|c| c.name).unwrap_or_default();
        self.audit_system("mcp_oauth", &format!("accesso eseguito al server MCP {name}"));
        let engine = self.clone();
        let id = pending.server_id.clone();
        tokio::spawn(async move {
            let _ = engine.reconnect_mcp_server(&id).await;
        });
        Ok(name)
    }
}
