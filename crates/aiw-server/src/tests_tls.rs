//! The TLS listener end to end: pinned clients, plain HTTP refused, pairing over TLS.

use super::tests_security::{fixture, req, send, LAN};
use super::*;
use aiw_core::tlspin;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A TLS listener on a free port in front of the engine's router (no plain access from loopback, so
/// that what a client on the network gets can be seen).
async fn tls_server(e: &Engine, router: Router) -> (u16, String) {
    e.set_config("global", "server.tls", json!("self_signed")).unwrap();
    let m = e.tls_material().unwrap().expect("self_signed material");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    e.tls_listener_started(true, port, Some(&m));
    let cfg = Arc::new(aiw_engine::tls::server_config(&m).unwrap());
    tokio::spawn(tls::serve_inner(listener, router, cfg, false));
    (port, m.fingerprint)
}

fn client(pin: &str) -> reqwest::Client {
    reqwest::Client::builder()
        .use_preconfigured_tls(tlspin::pinned_client_config(pin).unwrap())
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

#[tokio::test]
async fn the_listener_speaks_tls_to_a_pinned_client_and_refuses_everything_else() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let (port, fp) = tls_server(&e, app).await;
    let url = format!("https://localhost:{port}/api/system");
    let bearer = format!("Bearer {}", e.token());

    let r = client(&fp).get(&url).header("authorization", &bearer).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.headers().get("strict-transport-security").is_some(), "HSTS is sent over TLS");
    // The owner token over TLS is full, even from a non-loopback-looking caller: here the call reaches /api/tokens.
    let r = client(&fp).get(format!("https://localhost:{port}/api/tokens")).header("authorization", &bearer).send().await.unwrap();
    assert_eq!(r.status(), 200);
    // Wrong pin: the handshake fails.
    assert!(client(&"ab".repeat(32)).get(&url).header("authorization", &bearer).send().await.is_err());
    // A normal client (system roots) does not trust the local authority either.
    let plain = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
    assert!(plain.get(&url).header("authorization", &bearer).send().await.is_err());
    // Plain HTTP on the TLS port is not answered.
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(b"GET /api/system HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut buf)).await;
    assert!(!String::from_utf8_lossy(&buf).contains("HTTP/1.1"), "no HTTP answer in the clear: {:?}", String::from_utf8_lossy(&buf));
}

#[tokio::test]
async fn loopback_plain_http_is_still_served_on_the_tls_port() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    e.set_config("global", "server.tls", json!("self_signed")).unwrap();
    let m = e.tls_material().unwrap().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(tls::serve(listener, app, Arc::new(aiw_engine::tls::server_config(&m).unwrap())));
    let r = reqwest::Client::new().get(format!("http://127.0.0.1:{port}/api/system")).send().await.unwrap();
    assert_eq!(r.status(), 200, "this computer's own clients keep working");
}

#[tokio::test]
async fn pairing_over_tls_carries_the_fingerprint_and_pins_the_peer() {
    let (app_b, b, _d1, _w1) = fixture(vec![]).await;
    let (port, fp) = tls_server(&b, app_b).await;
    let (_a_app, a, _d2, _w2) = fixture(vec![]).await;
    let mut me = a.device();
    me.id = "computer-di-anna".into();
    me.name = "Computer di Anna".into();
    a.set_device(me);
    let addr = format!("https://localhost:{port}");

    // The pairing answer carries the fingerprint; a QR link carries it too.
    assert_eq!(b.tls_fingerprint().as_deref(), Some(fp.as_str()));
    let st = b.tls_status();
    assert!(st.active && st.fingerprint.is_some() && !st.warning && st.ca_available, "{st:?}");
    let (url, _) = mesh::pairing_qr(&format!("https://192.168.1.5:{port}"), "123456", b.tls_fingerprint().as_deref()).unwrap();
    assert!(url.contains(&format!("fp={fp}")));

    // No fingerprint, or the wrong one: no pairing, and no token is issued to a device that cannot verify us.
    assert!(a.pair_with_peer(&addr, &b.pairing_code(), None).await.is_err());
    let wrong = "cd".repeat(32);
    assert!(a.pair_with_peer(&addr, &b.pairing_code(), Some(&wrong)).await.is_err());
    // The right one (as the fingerprint is shown: with colons).
    let shown = tlspin::display_fingerprint(&fp);
    let peer = a.pair_with_peer(&addr, &b.pairing_code(), Some(&shown)).await.unwrap();
    assert_eq!(peer.address, addr);
    assert!(b.api_tokens().iter().any(|t| t.kind == "device" && t.device_id.as_deref() == Some("computer-di-anna")));
    assert_eq!(tlspin::pin_for(&addr).as_deref(), Some(fp.as_str()));
    // Unpairing from A tells B, which revokes the token it had issued to A.
    a.remove_peer(&peer.id).await.unwrap();
    assert!(tlspin::pin_for(&addr).is_none());
    assert!(b.api_tokens().iter().filter(|t| t.kind == "device").all(|t| t.revoked_at.is_some()), "the issuer revoked it");
}

#[tokio::test]
async fn https_origins_are_checked_against_the_scheme_of_the_request() {
    let (app, _e, _d, _w) = fixture(vec![]).await;
    let pair = |origin: &str, tls: bool| {
        let h = [("host", "192.168.1.5:7777"), ("origin", origin), ("sec-fetch-site", "same-origin")];
        let mut r = req("POST", "/api/pair", Some(LAN), &h, Some(json!({"code": "abcdef"})));
        if tls {
            r.extensions_mut().insert(security::TlsConn);
        }
        r
    };
    // A page served over TLS by this listener: its https origin is accepted (the wrong code is then refused by the pairing itself).
    let (st, _, v) = send(&app, pair("https://192.168.1.5:7777", true)).await;
    assert_eq!((st, v["error"].as_str()), (StatusCode::FORBIDDEN, Some("codice di pairing non valido")));
    // The same origin over a plain connection is not that page, and an http origin over TLS is not either.
    assert_eq!(send(&app, pair("https://192.168.1.5:7777", false)).await.2["error"], "origine non consentita");
    assert_eq!(send(&app, pair("http://192.168.1.5:7777", true)).await.2["error"], "origine non consentita");
    // A foreign https page never is.
    assert_eq!(send(&app, pair("https://evil.example", true)).await.2["error"], "origine non consentita");
}

#[tokio::test]
async fn a_tls_setting_cannot_come_from_the_repository_and_the_status_warns_on_plain_lan() {
    let (_app, e, _d, ws) = fixture(vec![]).await;
    let cfg = ws.path().join(".aiw");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("config.json"), r#"{"server.tls":"off","server.tls.cert_file":"/x","server.tls.key_file":"/y"}"#).unwrap();
    e.set_config("global", "server.tls", json!("self_signed")).unwrap();
    assert_eq!(e.tls_mode(), aiw_engine::tls::TlsMode::SelfSigned);
    // Plain HTTP beyond this computer is the warning of the Security page.
    e.tls_listener_started(true, 7777, None);
    let s = e.tls_status();
    assert!(s.warning && !s.active && s.lan);
}
