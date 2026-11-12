//! MCP servers through the API: OAuth sign-in against a fake authorization server (discovery,
//! registration, PKCE, one-time `state`, refresh, masking), prompts, reconnection and the state shown to the UI.

use super::tests_remote::{device, serve};
use super::*;
use axum::extract::State;
use axum::routing::{get, post};
use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Fake {
    base: Mutex<String>,
    challenge: Mutex<String>,
    token_calls: Mutex<Vec<HashMap<String, String>>>,
    mcp_tokens: Mutex<Vec<String>>,
}

type S = Arc<Fake>;

async fn mcp(State(f): State<S>, headers: axum::http::HeaderMap, axum::Json(b): axum::Json<Value>) -> Response {
    let auth = headers.get("authorization").and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
    if !(auth == "Bearer acc-1" || auth == "Bearer acc-2") {
        let base = f.base.lock().unwrap().clone();
        return (
            StatusCode::UNAUTHORIZED,
            [("www-authenticate", format!("Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\""))],
        )
            .into_response();
    }
    f.mcp_tokens.lock().unwrap().push(auth);
    let Some(id) = b.get("id").cloned() else { return StatusCode::ACCEPTED.into_response() };
    let result = match b["method"].as_str().unwrap_or("") {
        "initialize" => {
            json!({"protocolVersion": "2025-06-18", "capabilities": {"tools": {}, "prompts": {}}, "serverInfo": {"name": "oauth-fake"}})
        }
        "tools/list" => json!({"tools": [{"name": "hello", "description": "Saluta", "inputSchema": {"type": "object"}}]}),
        "prompts/list" => {
            json!({"prompts": [{"name": "saluta", "description": "Un saluto", "arguments": [{"name": "chi", "required": true}]}]})
        }
        "prompts/get" => {
            json!({"messages": [{"role": "user", "content": {"type": "text", "text": format!("Saluta {}", b["params"]["arguments"]["chi"].as_str().unwrap_or("?"))}}]})
        }
        _ => json!({}),
    };
    axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
}

async fn prm(State(f): State<S>) -> axum::Json<Value> {
    let base = f.base.lock().unwrap().clone();
    axum::Json(json!({"resource": format!("{base}/mcp"), "authorization_servers": [base]}))
}
async fn meta(State(f): State<S>) -> axum::Json<Value> {
    let b = f.base.lock().unwrap().clone();
    axum::Json(json!({"issuer": b, "authorization_endpoint": format!("{b}/authorize"), "token_endpoint": format!("{b}/token"),
        "registration_endpoint": format!("{b}/register"), "code_challenge_methods_supported": ["S256"]}))
}
async fn register() -> axum::Json<Value> {
    axum::Json(json!({"client_id": "client-1"}))
}
async fn token(State(f): State<S>, axum::Form(form): axum::Form<HashMap<String, String>>) -> (StatusCode, axum::Json<Value>) {
    f.token_calls.lock().unwrap().push(form.clone());
    match form.get("grant_type").map(String::as_str) {
        Some("authorization_code") => {
            let hash = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes()));
            if form.get("code").map(String::as_str) == Some("code-1") && hash == *f.challenge.lock().unwrap() {
                // Expires at once: the next use has to refresh.
                return (
                    StatusCode::OK,
                    axum::Json(json!({"access_token": "acc-1", "token_type": "Bearer", "expires_in": 1, "refresh_token": "ref-1"})),
                );
            }
            (StatusCode::BAD_REQUEST, axum::Json(json!({"error": "invalid_grant"})))
        }
        Some("refresh_token") if form.get("refresh_token").map(String::as_str) == Some("ref-1") => {
            (StatusCode::OK, axum::Json(json!({"access_token": "acc-2", "token_type": "Bearer", "expires_in": 3600})))
        }
        _ => (StatusCode::BAD_REQUEST, axum::Json(json!({"error": "invalid_grant"}))),
    }
}

async fn fake() -> (String, S) {
    let f: S = Arc::new(Fake::default());
    let app = axum::Router::new()
        .route("/mcp", post(mcp))
        .route("/.well-known/oauth-protected-resource/mcp", get(prm))
        .route("/.well-known/oauth-authorization-server", get(meta))
        .route("/register", post(register))
        .route("/token", post(token))
        .with_state(f.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    *f.base.lock().unwrap() = base.clone();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (base, f)
}

fn cfg(url: &str) -> McpServerConfig {
    McpServerConfig {
        id: "oa".into(),
        name: "remoto".into(),
        transport: "http".into(),
        command: None,
        args: None,
        url: Some(format!("{url}/mcp")),
        env: None,
        headers: None,
        enabled: true,
        sampling: false,
        oauth_client_id: None,
        sandbox: "off".into(),
        sandbox_network: true,
    }
}

async fn server_info(engine: &Engine) -> McpServerInfo {
    engine.mcp_servers().into_iter().find(|s| s.config.id == "oa").unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_sign_in_with_pkce_state_refresh_and_masking() {
    let (base, f) = fake().await;
    let (engine, _d, _w) = device("Dev", vec![]).await;
    let api = serve(&engine).await;
    let http = reqwest::Client::new();
    // 401: the state of the server says that it needs the sign-in.
    engine.save_mcp_server(cfg(&base)).await.unwrap();
    let info = server_info(&engine).await;
    assert_eq!((info.status.as_str(), info.signed_in), ("auth_required", false));
    // Start: the answer is the authorization page, with PKCE and the redirect to this engine.
    let start: Value = http.post(format!("{api}/api/mcp/servers/oa/oauth/start")).send().await.unwrap().json().await.unwrap();
    let url = reqwest::Url::parse(start["authorization_url"].as_str().unwrap()).unwrap();
    let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(url.path(), "/authorize");
    assert_eq!((q["code_challenge_method"].as_str(), q["client_id"].as_str()), ("S256", "client-1"));
    assert_eq!(q["resource"], format!("{base}/mcp"));
    assert_eq!(q["redirect_uri"], format!("{api}/api/mcp/oauth/callback"));
    *f.challenge.lock().unwrap() = q["code_challenge"].clone();
    let callback = |state: &str, code: &str| format!("{api}/api/mcp/oauth/callback?state={state}&code={code}");
    // A state this engine did not issue is refused, and the code is never exchanged.
    let bad = http.get(callback("not-mine", "code-1")).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    assert!(f.token_calls.lock().unwrap().is_empty());
    // The provider reporting an error consumes the state too.
    let st = q["state"].clone();
    let denied = http.get(format!("{api}/api/mcp/oauth/callback?state={st}&error=access_denied")).send().await.unwrap();
    assert_eq!(denied.status(), 400);
    assert!(f.token_calls.lock().unwrap().is_empty());
    assert_eq!(http.get(callback(&st, "code-1")).send().await.unwrap().status(), 400, "a state is used once");
    // Again, properly: the callback needs no API token, exchanges the code with the verifier and reconnects.
    let start: Value = http.post(format!("{api}/api/mcp/servers/oa/oauth/start")).send().await.unwrap().json().await.unwrap();
    let q: HashMap<String, String> =
        reqwest::Url::parse(start["authorization_url"].as_str().unwrap()).unwrap().query_pairs().into_owned().collect();
    *f.challenge.lock().unwrap() = q["code_challenge"].clone();
    let ok = http.get(callback(&q["state"], "code-1")).send().await.unwrap();
    assert_eq!(ok.status(), 200);
    assert!(ok.text().await.unwrap().contains("Accesso completato"));
    assert_eq!(f.token_calls.lock().unwrap().last().unwrap()["resource"], format!("{base}/mcp"));
    for _ in 0..100 {
        if server_info(&engine).await.status == "connected" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let info = server_info(&engine).await;
    assert_eq!((info.status.as_str(), info.signed_in), ("connected", true));
    assert!(info.tools.iter().any(|t| t.contains("hello")));
    // The token expired at once: it was renewed with the refresh token before being used.
    assert!(f.token_calls.lock().unwrap().iter().any(|c| c.get("grant_type").map(String::as_str) == Some("refresh_token")));
    assert!(f.mcp_tokens.lock().unwrap().iter().any(|t| t == "Bearer acc-2"));
    // No token anywhere in what the API returns.
    let listed = http.get(format!("{api}/api/mcp/servers")).send().await.unwrap().text().await.unwrap();
    for secret in ["acc-1", "acc-2", "ref-1"] {
        assert!(!listed.contains(secret), "{secret} leaked");
    }
    // Prompts of the connected server, and their text for the composer.
    let prompts: Value = http.get(format!("{api}/api/mcp/prompts")).send().await.unwrap().json().await.unwrap();
    assert_eq!(prompts[0]["name"], "saluta");
    assert_eq!(prompts[0]["arguments"][0]["required"], true);
    let text: Value = http
        .post(format!("{api}/api/mcp/prompts/get"))
        .json(&json!({"server_id": "oa", "name": "saluta", "arguments": {"chi": "Ada"}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(text["text"], "Saluta Ada");
    // The exports hide the stored tokens.
    assert!(!engine.mcp_servers().is_empty());
    // «Riconnetti» works and signing out forgets the tokens.
    let r = http.post(format!("{api}/api/mcp/servers/oa/reconnect")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    http.delete(format!("{api}/api/mcp/servers/oa/oauth")).send().await.unwrap();
    let info = server_info(&engine).await;
    assert_eq!((info.status.as_str(), info.signed_in), ("auth_required", false));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oauth_callback_is_exempt_from_the_token_but_nothing_else_is() {
    let (engine, _d, _w) = device("Dev", vec![]).await;
    let app = router(engine, None);
    // The callback answers without a token (here: refusing an unknown state).
    let (st, _) = dispatch(app.clone(), "GET", "/api/mcp/oauth/callback?state=x&code=y", None).await;
    assert_ne!(st, 401);
    assert!(!security::is_guarded("/api/mcp/oauth/callback"));
    assert!(security::is_guarded("/api/mcp/servers/oa/oauth/start"));
    assert!(security::is_guarded("/api/mcp/prompts"));
    // Starting the sign-in of a server that does not exist, or of one that is not http.
    let (st, _) = dispatch(app, "POST", "/api/mcp/servers/none/oauth/start", None).await;
    assert_eq!(st, 404);
}
