//! HTTP plumbing shared by the providers: one client, status → error mapping, origin checks.

use aiw_core::{Error, Result};
use reqwest::{Response, StatusCode, Url};
use std::time::Duration;

pub(crate) const USER_AGENT: &str = concat!("ai-workspace-local/", env!("CARGO_PKG_VERSION"));

/// Client for metadata requests (search, tree, manifests): short timeouts, default redirects
/// (reqwest drops `Authorization` on cross-origin redirects).
#[derive(Clone)]
pub(crate) struct Api {
    client: reqwest::Client,
}

impl Api {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(40))
            .build()
            .expect("http client");
        Self { client }
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// GET returning the response only on 2xx; any other outcome becomes a clear Italian error.
    pub async fn get(&self, provider: &str, what: &str, url: &str, headers: &[(&str, String)]) -> Result<Response> {
        let mut rb = self.client.get(url);
        for (k, v) in headers {
            rb = rb.header(*k, v);
        }
        send(provider, what, rb).await
    }

    pub async fn get_json(&self, provider: &str, what: &str, url: &str, headers: &[(&str, String)]) -> Result<serde_json::Value> {
        let r = self.get(provider, what, url, headers).await?;
        r.json().await.map_err(|e| Error::Upstream(format!("{provider}: risposta non valida per {what}: {e}")))
    }
}

pub(crate) async fn send(provider: &str, what: &str, rb: reqwest::RequestBuilder) -> Result<Response> {
    let r = rb.send().await.map_err(|e| net_error(provider, &e))?;
    if r.status().is_success() {
        Ok(r)
    } else {
        Err(status_error(provider, what, r.status()))
    }
}

pub(crate) fn net_error(provider: &str, e: &reqwest::Error) -> Error {
    let why = if e.is_timeout() {
        "tempo scaduto".to_string()
    } else if e.is_connect() {
        "connessione rifiutata o rete assente".to_string()
    } else {
        e.to_string()
    };
    Error::Upstream(format!("impossibile contattare {provider}: {why}"))
}

/// HTTP status → error with a message the user can act on.
pub(crate) fn status_error(provider: &str, what: &str, s: StatusCode) -> Error {
    match s.as_u16() {
        401 => Error::Unauthorized(format!(
            "{provider}: accesso negato (401), serve un token valido: impostalo nelle impostazioni del provider"
        )),
        403 => {
            Error::Denied(format!("{provider}: accesso vietato (403), il modello è gated o privato: serve un token con accesso accettato"))
        }
        404 => Error::NotFound(format!("{what} non trovato su {provider}")),
        429 => Error::Upstream(format!("{provider}: troppe richieste (429), riprova tra poco")),
        500..=599 => Error::Upstream(format!("{provider} non risponde (HTTP {s})")),
        _ => Error::Upstream(format!("{provider}: risposta inattesa (HTTP {s}) per {what}")),
    }
}

fn origin(u: &Url) -> Option<(String, String, u16)> {
    Some((u.scheme().to_string(), u.host_str()?.to_ascii_lowercase(), u.port_or_known_default()?))
}

/// Same scheme, host and port: the only case in which credentials may follow a request.
pub fn same_origin(a: &str, b: &str) -> bool {
    match (Url::parse(a), Url::parse(b)) {
        (Ok(a), Ok(b)) => origin(&a).is_some() && origin(&a) == origin(&b),
        _ => false,
    }
}

/// `Link: <url>; rel="next"` → the next URL, if any.
pub(crate) fn next_link(link: &str) -> Option<String> {
    link.split(',').find_map(|part| {
        let part = part.trim();
        let (url, params) = part.split_once('>')?;
        let rel_next = params.split(';').any(|p| {
            let p = p.trim().replace(' ', "");
            p == "rel=\"next\"" || p == "rel=next"
        });
        rel_next.then(|| url.trim().trim_start_matches('<').to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        assert!(same_origin("https://huggingface.co/a", "https://huggingface.co:443/b"));
        assert!(!same_origin("https://huggingface.co/a", "https://cdn-lfs.huggingface.co/b"));
        assert!(!same_origin("https://huggingface.co/a", "http://huggingface.co/a"));
        assert!(!same_origin("not a url", "https://x"));
        assert!(same_origin("http://127.0.0.1:8080/x", "http://127.0.0.1:8080/y"));
        assert!(!same_origin("http://127.0.0.1:8080/x", "http://127.0.0.1:8081/y"));
    }

    #[test]
    fn parses_link_header() {
        let h = r#"<https://huggingface.co/api/models?limit=2&cursor=abc>; rel="next""#;
        assert_eq!(next_link(h).as_deref(), Some("https://huggingface.co/api/models?limit=2&cursor=abc"));
        assert_eq!(next_link(r#"<https://x/prev>; rel="prev""#), None);
        assert_eq!(next_link(r#"<https://x/prev>; rel="prev", <https://x/next?c=1>; rel="next""#).as_deref(), Some("https://x/next?c=1"));
    }

    #[test]
    fn maps_statuses_to_italian_errors() {
        assert!(matches!(status_error("Hugging Face", "x", StatusCode::UNAUTHORIZED), Error::Unauthorized(m) if m.contains("token")));
        assert!(matches!(status_error("Hugging Face", "x", StatusCode::FORBIDDEN), Error::Denied(m) if m.contains("gated")));
        assert!(
            matches!(status_error("Hugging Face", "il modello", StatusCode::NOT_FOUND), Error::NotFound(m) if m.contains("non trovato"))
        );
        assert!(matches!(status_error("Hugging Face", "x", StatusCode::BAD_GATEWAY), Error::Upstream(_)));
    }
}
