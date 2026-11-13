//! OAuth against a fake authorization server running on loopback.

use super::*;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct As {
    base: Mutex<String>,
    token_endpoint: Mutex<Option<String>>,
    issuer: Mutex<Option<String>>,
    prm_resource: Mutex<Option<String>>,
    no_pkce: Mutex<bool>,
    registered: Mutex<Vec<Value>>,
    challenge: Mutex<String>,
    token_calls: Mutex<Vec<HashMap<String, String>>>,
}

type S = Arc<As>;

async fn prm(State(s): State<S>) -> Json<Value> {
    let base = s.base.lock().unwrap().clone();
    let resource = s.prm_resource.lock().unwrap().clone().unwrap_or(format!("{base}/mcp"));
    Json(json!({"resource": resource, "authorization_servers": [base]}))
}

async fn meta(State(s): State<S>) -> Json<Value> {
    let base = s.base.lock().unwrap().clone();
    let mut m = json!({
        "issuer": s.issuer.lock().unwrap().clone().unwrap_or(base.clone()),
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": s.token_endpoint.lock().unwrap().clone().unwrap_or(format!("{base}/token")),
        "registration_endpoint": format!("{base}/register"),
        "code_challenge_methods_supported": ["S256"]
    });
    if *s.no_pkce.lock().unwrap() {
        m.as_object_mut().unwrap().remove("code_challenge_methods_supported");
    }
    Json(m)
}

async fn register(State(s): State<S>, Json(body): Json<Value>) -> Json<Value> {
    s.registered.lock().unwrap().push(body);
    Json(json!({"client_id": "client-1"}))
}

async fn token(State(s): State<S>, Form(f): Form<HashMap<String, String>>) -> (axum::http::StatusCode, Json<Value>) {
    s.token_calls.lock().unwrap().push(f.clone());
    let bad = |m: &str| (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant", "error_description": m})));
    match f.get("grant_type").map(String::as_str) {
        Some("authorization_code") => {
            let verifier = f.get("code_verifier").cloned().unwrap_or_default();
            let hash = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            if f.get("code").map(String::as_str) != Some("code-1") || hash != *s.challenge.lock().unwrap() {
                return bad("codice o verifier non validi");
            }
            (
                axum::http::StatusCode::OK,
                Json(json!({"access_token": "acc-1", "token_type": "Bearer", "expires_in": 3600, "refresh_token": "ref-1"})),
            )
        }
        Some("refresh_token") if f.get("refresh_token").map(String::as_str) == Some("ref-1") => {
            (axum::http::StatusCode::OK, Json(json!({"access_token": "acc-2", "token_type": "bearer", "expires_in": 3600})))
        }
        _ => bad("richiesta non valida"),
    }
}

async fn fake() -> (String, S) {
    let s: S = Arc::new(As::default());
    let app = Router::new()
        .route("/.well-known/oauth-protected-resource/mcp", get(prm))
        .route("/.well-known/oauth-authorization-server", get(meta))
        .route("/register", post(register))
        .route("/token", post(token))
        .with_state(s.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    *s.base.lock().unwrap() = base.clone();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (base, s)
}

const REDIRECT: &str = "http://127.0.0.1:7777/api/mcp/oauth/callback";

#[test]
fn parses_the_challenge() {
    let c = parse_www_authenticate(
        r#"Bearer realm="x", resource_metadata="https://a.example/.well-known/oauth-protected-resource", scope="read write""#,
    );
    assert_eq!(c.resource_metadata.as_deref(), Some("https://a.example/.well-known/oauth-protected-resource"));
    assert_eq!(c.scope.as_deref(), Some("read write"));
    assert_eq!(parse_www_authenticate("Bearer"), Challenge::default());
}

#[test]
fn urls_must_be_https_or_loopback() {
    assert!(check_url("https://auth.example/x", "t").is_ok());
    assert!(check_url("http://127.0.0.1:9/x", "t").is_ok());
    assert!(check_url("http://localhost/x", "t").is_ok());
    for bad in [
        "http://auth.example/x",
        "ftp://a/x",
        "https://user:pw@a.example/",
        "https://a.example/#f",
        "javascript:alert(1)",
        "http://127.0.0.1.evil.example/",
    ] {
        assert!(check_url(bad, "t").is_err(), "{bad}");
    }
    assert_eq!(canonical_resource("https://a.example/").unwrap(), "https://a.example");
    assert_eq!(canonical_resource("https://a.example/mcp?x=1").unwrap(), "https://a.example/mcp");
}

#[tokio::test]
async fn happy_path_discovery_registration_pkce_token_and_refresh() {
    let (base, s) = fake().await;
    let http = http_client().unwrap();
    let url = format!("{base}/mcp");
    let d = discover(&http, &url, &Challenge::default()).await.unwrap();
    assert_eq!(d.resource, url);
    assert_eq!(d.token_endpoint, format!("{base}/token"));
    let reg = register_client(&http, &d, REDIRECT).await.unwrap();
    assert_eq!(reg.client_id, "client-1");
    assert_eq!(s.registered.lock().unwrap()[0]["redirect_uris"][0], REDIRECT);
    assert_eq!(s.registered.lock().unwrap()[0]["token_endpoint_auth_method"], "none");
    // The browser step: the URL carries PKCE, state and the resource.
    let (verifier, challenge) = pkce();
    let au = Url::parse(&authorization_url(&d, &reg.client_id, REDIRECT, "st-1", &challenge).unwrap()).unwrap();
    let q: HashMap<_, _> = au.query_pairs().into_owned().collect();
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["code_challenge"], challenge);
    assert_eq!((q["state"].as_str(), q["response_type"].as_str(), q["resource"].as_str()), ("st-1", "code", url.as_str()));
    assert_eq!(q["redirect_uri"], REDIRECT);
    *s.challenge.lock().unwrap() = challenge;
    // A wrong verifier is refused by the server, the right one gives tokens.
    assert!(exchange_code(&http, &d.token_endpoint, "client-1", None, "code-1", REDIRECT, "wrong", &d.resource).await.is_err());
    let t = exchange_code(&http, &d.token_endpoint, "client-1", None, "code-1", REDIRECT, &verifier, &d.resource).await.unwrap();
    assert_eq!((t.access_token.as_str(), t.refresh_token.as_deref(), t.expires_in), ("acc-1", Some("ref-1"), Some(3600)));
    assert_eq!(s.token_calls.lock().unwrap().last().unwrap()["resource"], url);
    let t2 = refresh(&http, &d.token_endpoint, "client-1", None, "ref-1", &d.resource).await.unwrap();
    assert_eq!(t2.access_token, "acc-2");
    assert!(refresh(&http, &d.token_endpoint, "client-1", None, "other", &d.resource).await.is_err());
}

#[tokio::test]
async fn pkce_is_s256_of_a_long_verifier() {
    let (v, c) = pkce();
    assert!(v.len() >= 43 && v.len() <= 128);
    assert_eq!(c, base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(v.as_bytes())));
    assert_ne!(pkce().0, v);
}

#[tokio::test]
async fn a_token_endpoint_on_another_origin_is_refused() {
    let (base, s) = fake().await;
    *s.token_endpoint.lock().unwrap() = Some("http://127.0.0.1:1/token".into());
    let e = discover(&http_client().unwrap(), &format!("{base}/mcp"), &Challenge::default()).await.unwrap_err().to_string();
    assert!(e.contains("altra origine"), "{e}");
    *s.token_endpoint.lock().unwrap() = Some("http://evil.example/token".into());
    assert!(discover(&http_client().unwrap(), &format!("{base}/mcp"), &Challenge::default()).await.is_err());
}

#[tokio::test]
async fn a_wrong_issuer_a_wrong_resource_or_no_pkce_is_refused() {
    let (base, s) = fake().await;
    let http = http_client().unwrap();
    let url = format!("{base}/mcp");
    *s.issuer.lock().unwrap() = Some("https://attacker.example".into());
    assert!(discover(&http, &url, &Challenge::default()).await.unwrap_err().to_string().contains("issuer"));
    *s.issuer.lock().unwrap() = None;
    *s.prm_resource.lock().unwrap() = Some("https://other.example/mcp".into());
    assert!(discover(&http, &url, &Challenge::default()).await.unwrap_err().to_string().contains("un'altra risorsa"));
    *s.prm_resource.lock().unwrap() = None;
    *s.no_pkce.lock().unwrap() = true;
    assert!(discover(&http, &url, &Challenge::default()).await.unwrap_err().to_string().contains("PKCE"));
}

#[tokio::test]
async fn a_challenge_that_points_to_a_plain_http_remote_address_is_refused() {
    let (base, _) = fake().await;
    let c = Challenge { resource_metadata: Some("http://evil.example/.well-known/oauth-protected-resource".into()), scope: None };
    // The bad hint is skipped (not fetched); the well-known location of the server itself still works.
    assert!(discover(&http_client().unwrap(), &format!("{base}/mcp"), &c).await.is_ok());
}
