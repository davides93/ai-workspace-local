//! Defence of the local API against web pages.
//!
//! The listener is reachable from the browser the user is surfing with, and requests coming from
//! loopback (or from the in-process Tauri dispatcher) need no token. Left alone, that lets any
//! page the user opens drive the API (CSRF: hooks, MCP stdio servers = code execution) and lets a
//! DNS-rebinding page make the browser talk to `127.0.0.1` under an attacker-controlled name.
//! Three rules close it:
//!
//! 1. a loopback peer that sends no valid token must have addressed the server by a loopback
//!    name (`Host`), otherwise it is a rebound name: 403;
//! 2. an `Origin` that is not on the allowlist is refused, and so is a `cross-site` fetch that
//!    does not carry an allowed one: 403;
//! 3. CORS answers only to the allowlist (the webhook endpoint, which is authenticated by its own
//!    secret, accepts any origin but never gets credentials).
//!
//! A request that presents an API token carries explicit credentials, which a forged request
//! cannot have, so it is not subject to rules 1 and 2. What a token may then reach is decided by
//! its scopes (see [`crate::scopes`]). Over TLS (the listener of `server.tls`) the page's own
//! origin is `https://…`; the allowlist follows the scheme the request arrived with. Requests without `ConnectInfo` are the
//! in-process dispatcher of the Tauri shell: there is no socket and no browser in between.

use crate::{mesh, AppState};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use std::net::IpAddr;

/// Paths that need the token (everything else is the static UI, the public agent card and the
/// webhooks, which carry their own secret).
pub(crate) fn is_guarded(path: &str) -> bool {
    // The return of the browser from an OAuth authorization server cannot carry the token: the handler
    // checks the peer and the one-time `state` itself.
    if path == "/api/mcp/oauth/callback" {
        return false;
    }
    path.starts_with("/api/") || path.starts_with("/v1/") || path.starts_with("/agui/") || path == "/mcp" || path == "/a2a"
}

fn is_webhook(path: &str) -> bool {
    path.starts_with("/hooks/")
}

fn forbidden(msg: &str) -> Response {
    (StatusCode::FORBIDDEN, Json(json!({ "error": msg }))).into_response()
}

// ------------------------------------------------------------------ hosts and origins

/// Splits `host[:port]` or `[v6][:port]` into a lowercase host (no trailing dot) and the port.
/// `None` for malformed values and for anything carrying userinfo or a path.
fn split_authority(a: &str) -> Option<(String, Option<u16>)> {
    let a = a.trim();
    if a.is_empty() || a.contains(['@', '/', '\\', ' ']) {
        return None;
    }
    let (host, port) = if let Some(rest) = a.strip_prefix('[') {
        let (h, tail) = rest.split_once(']')?;
        let port = if tail.is_empty() { None } else { Some(tail.strip_prefix(':')?.parse().ok()?) };
        (h, port)
    } else {
        match a.rsplit_once(':') {
            Some((h, p)) => (h, Some(p.parse().ok()?)),
            None => (a, None),
        }
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some((host, port))
}

fn is_loopback_name(host: &str) -> bool {
    host == "localhost"
        || host.parse::<IpAddr>().is_ok_and(|ip| match ip {
            IpAddr::V4(v) => v.is_loopback(),
            IpAddr::V6(v) => v.is_loopback() || v.to_ipv4_mapped().is_some_and(|m| m.is_loopback()),
        })
}

/// Is the `Host` header (with any port) a loopback name or address?
pub(crate) fn is_loopback_host(host: &str) -> bool {
    split_authority(host).is_some_and(|(h, _)| is_loopback_name(&h))
}

struct ParsedOrigin {
    scheme: String,
    host: String,
    port: Option<u16>,
}

fn parse_origin(origin: &str) -> Option<ParsedOrigin> {
    let (scheme, rest) = origin.split_once("://")?;
    let (host, port) = split_authority(rest)?;
    Some(ParsedOrigin { scheme: scheme.to_ascii_lowercase(), host, port })
}

/// Suffixes of names that only exist on private networks: no public domain can end this way
/// (mDNS, the special-use `home.arpa` and TLDs that were never delegated, plus the router default
/// of the Fritz!Box), so a DNS-rebinding page cannot have such a name.
const PRIVATE_SUFFIXES: [&str; 10] =
    [".local", ".lan", ".home", ".home.arpa", ".internal", ".localdomain", ".intranet", ".private", ".corp", ".fritz.box"];

/// Names a DNS-rebinding page cannot have: IP literals, single-label names and private-network names.
fn is_lan_style_host(host: &str) -> bool {
    host.parse::<IpAddr>().is_ok() || !host.contains('.') || PRIVATE_SUFFIXES.iter().any(|s| host.ends_with(s))
}

/// Webviews of the Tauri shells and pages served from this machine.
fn is_builtin_origin(o: &ParsedOrigin) -> bool {
    match o.scheme.as_str() {
        // macOS, Linux and iOS webviews.
        "tauri" => o.host == "localhost" && o.port.is_none(),
        // Windows and Android webviews, and any dev server or UI on this machine.
        "http" | "https" => (o.host == "tauri.localhost" && o.port.is_none()) || is_loopback_name(&o.host),
        _ => false,
    }
}

/// The scheme this listener serves for a request, and its default port.
fn own_scheme(tls: bool) -> (&'static str, u16) {
    if tls {
        ("https", 443)
    } else {
        ("http", 80)
    }
}

/// `http(s)://<LAN address or mDNS name of this device>:<port of the listener>`.
fn is_own_lan_origin(o: &ParsedOrigin, st: &AppState, tls: bool) -> bool {
    let (scheme, default_port) = own_scheme(tls);
    if o.scheme != scheme || o.port.unwrap_or(default_port) != crate::server_port(st) {
        return false;
    }
    let own_ip = mesh::lan_ip().map(|ip| ip.to_string());
    own_ip.as_deref() == Some(o.host.as_str()) || o.host == format!("{}.local", mesh::host_label(&st.engine.device().name))
}

/// The page was served by this very listener over the LAN (its origin is the `Host` it was
/// reached with). Only for names an attacker cannot rebind, see [`is_lan_style_host`].
fn is_same_lan_origin(o: &ParsedOrigin, host: Option<&str>, tls: bool) -> bool {
    let (scheme, default_port) = own_scheme(tls);
    let Some((h, port)) = host.and_then(split_authority) else { return false };
    o.scheme == scheme && o.host == h && o.port.unwrap_or(default_port) == port.unwrap_or(default_port) && is_lan_style_host(&o.host)
}

/// The CORS and CSRF allowlist: Tauri webviews, `localhost`/`127.0.0.1`/`[::1]` on any port (the
/// vite dev server included) and the origins of this device on the LAN.
pub(crate) fn origin_allowed(origin: &str, host: Option<&str>, st: &AppState, tls: bool) -> bool {
    let Some(o) = parse_origin(origin) else { return false };
    is_builtin_origin(&o) || is_same_lan_origin(&o, host, tls) || is_own_lan_origin(&o, st, tls)
}

fn host_of(req: &Request) -> Option<String> {
    req.headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(str::to_string)
        .or_else(|| req.uri().authority().map(|a| a.as_str().to_string()))
}

// ------------------------------------------------------------------ guard

/// How the request reached the router.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Peer {
    /// No socket: the Tauri shell dispatching through the router.
    InProcess,
    Loopback,
    Remote,
}

/// Marker the TLS listener puts on the requests that arrived over an encrypted connection.
#[derive(Clone, Copy)]
pub(crate) struct TlsConn;

/// Did the request arrive over TLS?
pub(crate) fn is_tls(req: &Request) -> bool {
    req.extensions().get::<TlsConn>().is_some()
}

pub(crate) fn peer(req: &Request) -> Peer {
    match req.extensions().get::<axum::extract::ConnectInfo<std::net::SocketAddr>>() {
        None => Peer::InProcess,
        Some(axum::extract::ConnectInfo(addr)) if addr.ip().to_canonical().is_loopback() => Peer::Loopback,
        Some(_) => Peer::Remote,
    }
}

/// The tokens the request carries (`Authorization: Bearer`, `x-api-key` or `?token=`), without repeats.
pub(crate) fn presented_tokens(req: &Request) -> Vec<String> {
    let h = req.headers();
    let bearer = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    // Anthropic SDKs send the key as `x-api-key`.
    let api_key = h.get("x-api-key").and_then(|v| v.to_str().ok());
    let query = req.uri().query().and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("token=")));
    let mut out: Vec<String> = Vec::new();
    for t in [bearer, api_key, query].into_iter().flatten().map(str::trim).filter(|t| !t.is_empty()) {
        if !out.iter().any(|o| o == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// Rule 2: `Origin` allowlist and `Sec-Fetch-Site`. `Some(response)` is the refusal.
pub(crate) fn check_origin(req: &Request, st: &AppState) -> Option<Response> {
    let host = host_of(req);
    let tls = is_tls(req);
    let origin = req.headers().get(header::ORIGIN).map(|o| o.to_str().ok().is_some_and(|o| origin_allowed(o, host.as_deref(), st, tls)));
    let cross_site =
        req.headers().get("sec-fetch-site").and_then(|v| v.to_str().ok()).is_some_and(|v| v.eq_ignore_ascii_case("cross-site"));
    match origin {
        Some(false) => Some(forbidden("origine non consentita")),
        // A cross-site fetch with no `Origin` (an <img>, a navigation) is not one of ours either.
        None if cross_site => Some(forbidden("richiesta cross-site non consentita")),
        _ => None,
    }
}

/// Rules 1 and 2 for a request that arrives from this machine without credentials.
pub(crate) fn check_loopback(req: &Request, st: &AppState) -> Option<Response> {
    if host_of(req).is_some_and(|host| !is_loopback_host(&host)) {
        return Some(forbidden("host non consentito"));
    }
    check_origin(req, st)
}

// ------------------------------------------------------------------ CORS

const ALLOW_METHODS: &str = "GET, POST, PUT, PATCH, DELETE, OPTIONS";
const ALLOW_HEADERS: &str =
    "authorization, content-type, x-api-key, x-aiw-secret, last-event-id, mcp-session-id, mcp-protocol-version, anthropic-version";

/// CORS for the allowlist, answering preflights itself. It never sets
/// `Access-Control-Allow-Credentials`; `/hooks/{id}` answers any origin because its secret is
/// the credential.
pub(crate) async fn cors(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let Some(origin) = req.headers().get(header::ORIGIN).cloned() else { return next.run(req).await };
    let webhook = is_webhook(req.uri().path());
    let host = host_of(&req);
    let allowed = webhook || origin.to_str().ok().is_some_and(|o| origin_allowed(o, host.as_deref(), &st, is_tls(&req)));
    let preflight = req.method() == Method::OPTIONS && req.headers().contains_key(header::ACCESS_CONTROL_REQUEST_METHOD);
    if preflight {
        if !allowed {
            return forbidden("origine non consentita");
        }
        let mut res = StatusCode::NO_CONTENT.into_response();
        let wanted = req.headers().get(header::ACCESS_CONTROL_REQUEST_HEADERS).cloned();
        let h = res.headers_mut();
        add_cors(h, &origin, webhook);
        h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static(if webhook { "POST, OPTIONS" } else { ALLOW_METHODS }));
        h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, wanted.unwrap_or(HeaderValue::from_static(ALLOW_HEADERS)));
        h.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
        return res;
    }
    let mut res = next.run(req).await;
    if allowed {
        add_cors(res.headers_mut(), &origin, webhook);
    }
    res
}

fn add_cors(h: &mut HeaderMap, origin: &HeaderValue, any_origin: bool) {
    if any_origin {
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    } else {
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
        h.append(header::VARY, HeaderValue::from_static("Origin"));
    }
    h.insert(HeaderName::from_static("access-control-expose-headers"), HeaderValue::from_static("*"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts() {
        for h in [
            "localhost",
            "localhost:7777",
            "LOCALHOST:5173",
            "localhost.",
            "127.0.0.1",
            "127.0.0.1:7777",
            "127.1.2.3:80",
            "[::1]",
            "[::1]:7777",
        ] {
            assert!(is_loopback_host(h), "{h}");
        }
        for h in [
            "evil.example",
            "evil.example:7777",
            "localhost.evil.example",
            "127.0.0.1.evil.example",
            "192.168.1.5:7777",
            "0.0.0.0:7777",
            "[::]:7777",
            "localhost:abc",
            "localhost@evil.example",
            "",
        ] {
            assert!(!is_loopback_host(h), "{h}");
        }
    }

    #[test]
    fn authority_parsing() {
        assert_eq!(split_authority("[::1]:80"), Some(("::1".into(), Some(80))));
        assert_eq!(split_authority("Example.COM:8080"), Some(("example.com".into(), Some(8080))));
        assert_eq!(split_authority("a.b"), Some(("a.b".into(), None)));
        assert_eq!(split_authority("a@b"), None);
        assert_eq!(split_authority("[::1"), None);
    }

    #[test]
    fn builtin_origins() {
        let ok = |s: &str| parse_origin(s).is_some_and(|o| is_builtin_origin(&o));
        for o in [
            "tauri://localhost",
            "http://tauri.localhost",
            "https://tauri.localhost",
            "http://localhost:5173",
            "http://localhost",
            "http://127.0.0.1:7777",
            "http://[::1]:5173",
        ] {
            assert!(ok(o), "{o}");
        }
        for o in [
            "null",
            "https://evil.example",
            "http://localhost.evil.example:5173",
            "http://127.0.0.1.evil.example",
            "tauri://evil",
            "tauri://localhost:1",
            "http://tauri.localhost.evil.example",
            "ftp://localhost",
            "http://192.168.1.5:7777",
            "http://localhost/path",
        ] {
            assert!(!ok(o), "{o}");
        }
    }

    #[test]
    fn same_lan_origin_needs_a_name_that_cannot_be_rebound() {
        let same = |origin: &str, host: &str| parse_origin(origin).is_some_and(|o| is_same_lan_origin(&o, Some(host), false));
        assert!(same("http://192.168.1.5:7777", "192.168.1.5:7777"));
        assert!(same("http://studio-pc.local:7777", "studio-pc.local:7777"));
        assert!(same("http://studio-pc:7777", "studio-pc:7777"));
        assert!(same("http://studio-pc.fritz.box:7777", "studio-pc.fritz.box:7777"));
        assert!(same("http://nas.home.arpa:7777", "nas.home.arpa:7777"));
        assert!(!same("http://studio-pc.local.evil.example:7777", "studio-pc.local.evil.example:7777"));
        assert!(!same("http://fritz.box.evil.example", "fritz.box.evil.example"));
        assert!(!same("http://192.168.1.5:7777", "192.168.1.5:9999"), "port differs");
        assert!(!same("http://attacker.example:7777", "attacker.example:7777"), "a public name may be rebound");
        assert!(!same("https://192.168.1.5:7777", "192.168.1.5:7777"));
    }

    #[test]
    fn https_origins_follow_the_scheme_of_the_listener() {
        let same = |origin: &str, host: &str, tls: bool| parse_origin(origin).is_some_and(|o| is_same_lan_origin(&o, Some(host), tls));
        // Over TLS the page was served as https: an http origin is not the same site, and the other way round.
        assert!(same("https://192.168.1.5:7777", "192.168.1.5:7777", true));
        assert!(same("https://studio-pc.local:7777", "studio-pc.local:7777", true));
        assert!(same("https://studio-pc.local", "studio-pc.local:443", true), "default port of https");
        assert!(!same("http://192.168.1.5:7777", "192.168.1.5:7777", true));
        assert!(!same("https://192.168.1.5:7777", "192.168.1.5:7777", false));
        assert!(!same("https://attacker.example:7777", "attacker.example:7777", true), "a public name may be rebound");
        assert!(!same("https://192.168.1.5:7777", "192.168.1.5:9999", true), "port differs");
    }
}
