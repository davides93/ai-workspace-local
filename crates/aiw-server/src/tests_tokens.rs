//! Per-device and per-app tokens: scopes over the real router, revocation, expiry, rotation, the
//! legacy token, the limiter, the audit log and the token API.

use super::tests_security::{fixture, req, send, LAN, LOOPBACK};
use super::*;
use aiw_engine::tokens::{TokenCheck, TokenKind};
use futures::StreamExt;

fn bearer(t: &str) -> String {
    format!("Bearer {t}")
}

async fn call(app: &Router, method: &str, uri: &str, token: Option<&str>, tls: bool, body: Option<Value>) -> (StatusCode, Value) {
    let mut headers = vec![("host", "192.168.1.5:7777")];
    let b = token.map(bearer);
    if let Some(b) = &b {
        headers.push(("authorization", b.as_str()));
    }
    let mut r = req(method, uri, Some(LAN), &headers, body);
    if tls {
        r.extensions_mut().insert(security::TlsConn);
    }
    let (st, _, v) = send(app, r).await;
    (st, v)
}

fn allowed(st: StatusCode) -> bool {
    st != StatusCode::FORBIDDEN && st != StatusCode::UNAUTHORIZED
}

#[tokio::test]
async fn tokens_are_hashed_at_rest_and_shown_once() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let owner = e.token().to_string();
    let (st, created) = call(&app, "POST", "/api/tokens", Some(&owner), true, Some(json!({"name": "Mia app", "kind": "gateway"}))).await;
    assert_eq!(st, StatusCode::OK, "{created}");
    let secret = created["token"].as_str().unwrap().to_string();
    assert!(secret.starts_with("aiw_") && secret.len() >= 47);
    assert_eq!(created["scopes"], json!(["gateway"]));
    let (_, list) = call(&app, "GET", "/api/tokens", Some(&owner), true, None).await;
    assert!(!list.to_string().contains(&secret), "the list never carries the secret");
    assert!(list.as_array().unwrap().iter().any(|t| t["kind"] == "owner"), "the legacy token is listed as the owner token");
    let at_rest = e.api_tokens_at_rest();
    assert!(!at_rest.contains(&secret) && !at_rest.contains(&owner), "only hashes are stored");
    assert!(matches!(e.authenticate_token(&secret, None), TokenCheck::Valid(a) if a.kind == TokenKind::Gateway));
}

const OWNER: usize = 0;
const DEVICE: usize = 1;
const GATEWAY: usize = 2;
const GATEWAY_MCP: usize = 3;
const OWNER_PLAIN: usize = 4;

#[tokio::test]
async fn the_scope_matrix_over_the_real_router() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let dev = e.create_api_token("Telefono", "device", None, None, None).unwrap().token;
    let gw = e.create_api_token("App", "gateway", None, None, None).unwrap().token;
    let gw2 = e.create_api_token("App con MCP", "gateway", None, None, Some(&["mcp".to_string()])).unwrap().token;
    let owner = e.token().to_string();
    let toks: [(&str, bool); 5] = [(&owner, true), (&dev, false), (&gw, false), (&gw2, false), (&owner, false)];

    // (method, path, body, who may: owner over TLS, device, gateway, gateway+mcp, owner over plain HTTP from the LAN)
    let rows: &[(&str, &str, Option<Value>, [bool; 5])] = &[
        ("GET", "/api/system", None, [true, true, false, false, true]),
        ("GET", "/api/sessions", None, [true, true, false, false, true]),
        ("POST", "/api/sessions", Some(json!({"agent_id": "planner"})), [true, true, false, false, true]),
        ("GET", "/api/approvals", None, [true, true, false, false, true]),
        ("GET", "/api/models", None, [true, true, false, false, true]),
        ("GET", "/api/voice", None, [true, true, false, false, true]),
        ("GET", "/api/memories", None, [true, true, false, false, true]),
        ("GET", "/api/config", None, [true, true, false, false, true]),
        ("PUT", "/api/config", Some(json!({"layer": "global", "key": "ui.theme", "value": "dark"})), [true, false, false, false, false]),
        ("PUT", "/api/hooks", Some(json!({"hooks": []})), [true, false, false, false, false]),
        ("PUT", "/api/policy", Some(json!({"rules": []})), [true, false, false, false, false]),
        ("POST", "/api/mcp/servers", Some(json!({"name": "x", "transport": "stdio"})), [true, false, false, false, false]),
        ("POST", "/api/channels", Some(json!({})), [true, false, false, false, false]),
        ("PUT", "/api/runtimes/llama-embedded/settings", Some(json!({})), [true, false, false, false, false]),
        ("DELETE", "/api/profiles/nessuno", None, [true, false, false, false, false]),
        ("GET", "/api/tokens", None, [true, false, false, false, false]),
        ("POST", "/api/tokens", Some(json!({"name": "x"})), [true, false, false, false, false]),
        ("GET", "/api/tls", None, [true, false, false, false, false]),
        ("GET", "/api/mesh", None, [true, false, false, false, false]),
        ("POST", "/api/backup", Some(json!({"include_secrets": false})), [true, false, false, false, false]),
        ("POST", "/api/backup/inspect", Some(json!({"data": ""})), [true, false, false, false, false]),
        ("GET", "/api/diagnostics", None, [true, false, false, false, false]),
        ("GET", "/v1/models", None, [true, true, true, true, true]),
        ("POST", "/v1/chat/completions", Some(json!({"model": "x", "messages": []})), [true, true, true, true, true]),
        ("POST", "/mcp", Some(json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})), [true, true, false, true, true]),
        ("POST", "/a2a", Some(json!({"jsonrpc": "2.0", "id": 1, "method": "x"})), [true, true, false, false, true]),
        ("GET", "/api/nothing-here", None, [true, false, false, false, false]),
    ];
    for (m, p, body, who) in rows {
        for (i, (tok, tls)) in toks.iter().enumerate() {
            let (st, v) = call(&app, m, p, Some(tok), *tls, body.clone()).await;
            assert_eq!(allowed(st), who[i], "{m} {p} as #{i}: {st} {v}");
        }
    }
    // The owner token from the LAN over plain HTTP says why it is held back.
    let (st, v) = call(&app, "PUT", "/api/hooks", Some(&owner), false, Some(json!({"hooks": []}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert!(v["error"].as_str().unwrap().contains("senza TLS"), "{v}");
    let _ = (OWNER, DEVICE, GATEWAY, GATEWAY_MCP, OWNER_PLAIN);
    assert!(e.hooks().is_empty());
}

#[tokio::test]
async fn loopback_without_a_token_is_unchanged_and_a_device_token_is_not_local() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let h = [("host", "127.0.0.1:7777")];
    let (st, ..) = send(&app, req("PUT", "/api/hooks", Some(LOOPBACK), &h, Some(json!({"hooks": []})))).await;
    assert_eq!(st, StatusCode::OK, "this computer without a token keeps full access");
    let (st, ..) = send(&app, req("GET", "/api/tokens", Some(LOOPBACK), &h, None)).await;
    assert_eq!(st, StatusCode::OK);
    // A paired device's token over loopback is a device: not full, not local (no pairing code).
    let dev = e.create_api_token("Telefono", "device", None, None, None).unwrap().token;
    let b = bearer(&dev);
    let hh = [("host", "127.0.0.1:7777"), ("authorization", b.as_str())];
    let (st, ..) = send(&app, req("PUT", "/api/hooks", Some(LOOPBACK), &hh, Some(json!({"hooks": []})))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _, sys) = send(&app, req("GET", "/api/system", Some(LOOPBACK), &hh, None)).await;
    assert_eq!(st, StatusCode::OK);
    assert!(sys.get("pairing_code").is_none());
    // The in-process dispatcher keeps everything.
    assert_eq!(dispatch(app.clone(), "GET", "/api/tokens", None).await.0, 200);
}

#[tokio::test]
async fn revocation_expiry_and_rotation_act_at_once() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let owner = e.token().to_string();
    let made = e.create_api_token("Telefono", "device", Some("tel-1"), None, None).unwrap();
    let (id, tok) = (made.info.id.clone(), made.token.clone());
    assert_eq!(made.info.device_id.as_deref(), Some("tel-1"));
    assert_eq!(call(&app, "GET", "/api/system", Some(&tok), false, None).await.0, StatusCode::OK);

    // Rotation: the new secret works, the old one is refused as revoked.
    let (st, rot) = call(&app, "POST", &format!("/api/tokens/{id}/rotate"), Some(&owner), true, None).await;
    assert_eq!(st, StatusCode::OK, "{rot}");
    let new = rot["token"].as_str().unwrap().to_string();
    assert_ne!(new, tok);
    assert_eq!(rot["device_id"], "tel-1");
    let (st, v) = call(&app, "GET", "/api/system", Some(&tok), false, None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert!(v["error"].as_str().unwrap().contains("revocato"));
    assert_eq!(call(&app, "GET", "/api/system", Some(&new), false, None).await.0, StatusCode::OK);

    // Revocation through the API (and even over loopback a revoked token is not "no token").
    let new_id = rot["id"].as_str().unwrap().to_string();
    let (st, _) = call(&app, "DELETE", &format!("/api/tokens/{new_id}"), Some(&owner), true, None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/system", Some(&new), false, None).await.0, StatusCode::UNAUTHORIZED);
    let b = bearer(&new);
    let (st, ..) = send(&app, req("GET", "/api/system", Some(LOOPBACK), &[("host", "127.0.0.1:7777"), ("authorization", &b)], None)).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&app, "POST", &format!("/api/tokens/{new_id}/rotate"), Some(&owner), true, None).await.0, StatusCode::CONFLICT);

    // Expiry.
    let short = e.create_api_token("Breve", "gateway", None, Some(1), None).unwrap();
    assert!(short.info.expires_at.is_some());
    assert_eq!(call(&app, "GET", "/v1/models", Some(&short.token), false, None).await.0, StatusCode::OK);
    e.set_api_token_expiry(&short.info.id, Some("2020-01-01T00:00:00Z")).unwrap();
    let (st, v) = call(&app, "GET", "/v1/models", Some(&short.token), false, None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert!(v["error"].as_str().unwrap().contains("scaduto"));

    // The owner token cannot be revoked or rotated through the API.
    let owner_id = e.api_tokens().into_iter().find(|t| t.kind == "owner").unwrap().id;
    assert_eq!(call(&app, "DELETE", &format!("/api/tokens/{owner_id}"), Some(&owner), true, None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "POST", &format!("/api/tokens/{owner_id}/rotate"), Some(&owner), true, None).await.0, StatusCode::FORBIDDEN);

    // Audit: creation, rotation, revocation, and the rejected uses (logged once, not once per request).
    for _ in 0..5 {
        call(&app, "GET", "/api/system", Some(&new), false, None).await;
    }
    let audit = e.audit();
    for action in ["token_create", "token_rotate", "token_revoke"] {
        assert!(audit.iter().any(|a| a.action == action), "{action}");
    }
    assert_eq!(audit.iter().filter(|a| a.action == "token_rejected" && a.detail.contains(&new_id)).count(), 1, "rate limited");
    assert!(!audit.iter().any(|a| a.detail.contains(&new) || a.detail.contains(&tok)), "no secret in the log");
}

#[tokio::test]
async fn pairing_issues_a_device_token_and_unpairing_revokes_it() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let h = [("host", "192.168.1.5:7777")];
    let code = e.pairing_code();
    let (st, _, body) =
        send(&app, req("POST", "/api/pair", Some(LAN), &h, Some(json!({"code": code, "device_id": "tel-9", "name": "Telefono di Anna"}))))
            .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let tok = body["token"].as_str().unwrap().to_string();
    assert_eq!(body["device_id"], "tel-9");
    let listed = e.api_tokens();
    let t = listed.iter().find(|t| t.device_id.as_deref() == Some("tel-9")).unwrap();
    assert_eq!((t.name.as_str(), t.kind.as_str()), ("Telefono di Anna", "device"));
    // Pairing again as the same device replaces the old token.
    let code = e.pairing_code();
    let (_, _, again) = send(&app, req("POST", "/api/pair", Some(LAN), &h, Some(json!({"code": code, "device_id": "tel-9"})))).await;
    assert_eq!(call(&app, "GET", "/api/system", Some(&tok), false, None).await.0, StatusCode::UNAUTHORIZED);
    let tok2 = again["token"].as_str().unwrap().to_string();
    assert_eq!(call(&app, "GET", "/api/system", Some(&tok2), false, None).await.0, StatusCode::OK);
    // The device can drop its own token (what un-pairing from the other side does); the owner cannot.
    assert_eq!(call(&app, "DELETE", "/api/auth/token", Some(&tok2), false, None).await.0, StatusCode::OK);
    assert_eq!(call(&app, "GET", "/api/system", Some(&tok2), false, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&app, "DELETE", "/api/auth/token", Some(e.token()), true, None).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn revoking_closes_an_open_stream() {
    let (app, e, _d, _w) = fixture(vec![
        aiw_runtimes::ScriptedBackend::tool_turn("c1", "write_file", json!({"path": "nota.md", "content": "ciao"})),
        aiw_runtimes::ScriptedBackend::text_turn("Fatto"),
    ])
    .await;
    let (_, ses) = dispatch(app.clone(), "POST", "/api/sessions", Some(json!({"agent_id": "operator"}))).await;
    let sid = ses["id"].as_str().unwrap().to_string();
    let (_, run) = dispatch(app.clone(), "POST", &format!("/api/sessions/{sid}/runs"), Some(json!({"input": "scrivi"}))).await;
    let rid = run["run_id"].as_str().unwrap().to_string();
    for _ in 0..200 {
        if !e.approvals().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let made = e.create_api_token("Telefono", "device", None, None, None).unwrap();
    let b = bearer(&made.token);
    let r = req("GET", &format!("/api/runs/{rid}/events"), Some(LAN), &[("host", "192.168.1.5:7777"), ("authorization", &b)], None);
    let res = tower::ServiceExt::oneshot(app.clone(), r).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let mut stream = res.into_body().into_data_stream();
    assert!(tokio::time::timeout(Duration::from_secs(5), stream.next()).await.unwrap().is_some(), "events flow while the token is valid");
    e.revoke_api_token(&made.info.id).unwrap();
    let ended = tokio::time::timeout(Duration::from_secs(5), async { while stream.next().await.is_some() {} }).await;
    assert!(ended.is_ok(), "the stream was closed by the revocation");
    // The run itself is not touched by the revocation of whoever watched it.
    let id = e.approvals()[0].id.clone();
    e.respond_approval(&id, true, false).unwrap();
}

#[tokio::test]
async fn bad_tokens_are_rate_limited_per_address() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let good = e.create_api_token("App", "gateway", None, None, None).unwrap().token;
    for i in 0..aiw_engine::tokens::MAX_BAD_TOKENS {
        let (st, _) = call(&app, "GET", "/v1/models", Some(&format!("aiw_sbagliato{i}")), false, None).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "{i}");
    }
    let (st, v) = call(&app, "GET", "/v1/models", Some(&good), false, None).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "{v}");
    assert!(e.audit().iter().any(|a| a.action == "token_lockout"));
    // Another address is not affected.
    let b = bearer(&good);
    let r = req("GET", "/v1/models", Some([192, 168, 1, 77]), &[("host", "192.168.1.5:7777"), ("authorization", &b)], None);
    assert_eq!(send(&app, r).await.0, StatusCode::OK);
}

/// Every route registered in `lib.rs` has a scope decision (so a new route cannot be forgotten).
#[test]
fn every_route_has_a_scope_decision() {
    let src = include_str!("lib.rs");
    let body = &src[src.find("pub fn router(").unwrap()..src.find("let mut app = api.method_not_allowed_fallback").unwrap()];
    let mut seen = 0;
    for chunk in body.split(".route(").skip(1) {
        let path = chunk.trim_start().strip_prefix('"').unwrap().split('"').next().unwrap();
        // The handlers (`get(…)`, `post(…)`…) up to the end of this route call.
        let end = chunk.find("\n        .route(").unwrap_or(chunk.len());
        let decl = &chunk[..end];
        let concrete = path
            .replace("{*id}", "a/b")
            .replace("{id}", "x")
            .replace("{action}", "start")
            .replace("{name}", "n")
            .replace("{mid}", "m")
            .replace("{attachment_id}", "att");
        let mut verbs = vec![];
        for (verb, m) in [("get(", "GET"), ("post(", "POST"), ("put(", "PUT"), ("patch(", "PATCH"), ("delete(", "DELETE")] {
            if decl.contains(&format!(" {verb}"))
                || decl.contains(&format!(",{verb}"))
                || decl.contains(&format!(".{verb}"))
                || decl.contains(&format!("({verb}"))
            {
                verbs.push(m);
            }
        }
        // `/mcp` declares `.delete(|| …)` too; verbs found are enough.
        assert!(!verbs.is_empty(), "no method found for {path}");
        if !security::is_guarded(&concrete) {
            continue;
        }
        for m in verbs {
            assert!(scopes::required_scope(m, &concrete).is_some(), "{m} {path} has no scope decision in scopes.rs");
            seen += 1;
        }
    }
    assert!(seen > 150, "the route list was not read ({seen})");
}
