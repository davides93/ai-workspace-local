//! Channels and the IMAP accounts over HTTP: nothing connects, secrets stay masked.

use super::*;

fn telegram_body() -> Value {
    json!({"kind": "telegram", "name": "Casa", "allowed": ["111"], "secrets": {"bot_token": "123456:SEGRETISSIMO"}})
}

#[tokio::test]
async fn a_channel_is_created_off_with_masked_secrets_and_can_be_changed_and_removed() {
    let (app, _e, _d, _w) = super::tests::app().await;
    assert_eq!(dispatch(app.clone(), "GET", "/api/channels", None).await.1, json!([]));
    let (st, c) = dispatch(app.clone(), "POST", "/api/channels", Some(telegram_body())).await;
    assert_eq!(st, 200, "{c}");
    assert_eq!((c["enabled"].as_bool(), c["mode"].as_str()), (Some(false), Some("read_only")), "{c}");
    assert!(!c.to_string().contains("SEGRETISSIMO"), "the token never comes back: {c}");
    let id = c["id"].as_str().unwrap().to_string();
    let (_, list) = dispatch(app.clone(), "GET", "/api/channels", None).await;
    assert!(!list.to_string().contains("SEGRETISSIMO"));
    assert_eq!(list.as_array().unwrap().len(), 1);
    let (_, s) = dispatch(app.clone(), "GET", &format!("/api/channels/{id}/status"), None).await;
    assert_eq!(s["state"], "off", "{s}");
    let (st, p) =
        dispatch(app.clone(), "PATCH", &format!("/api/channels/{id}"), Some(json!({"mode": "ask", "name": "Telegram di casa"}))).await;
    assert_eq!((st, p["mode"].as_str()), (200, Some("ask")), "{p}");
    // A wrong kind is refused in plain Italian.
    let (st, bad) = dispatch(app.clone(), "POST", "/api/channels", Some(json!({"kind": "fax", "name": "x"}))).await;
    assert_eq!(st, 400, "{bad}");
    let (st, _) = dispatch(app.clone(), "DELETE", &format!("/api/channels/{id}"), None).await;
    assert!(st == 200 || st == 204, "{st}");
    assert_eq!(dispatch(app.clone(), "GET", "/api/channels", None).await.1, json!([]));
    let (st, _) = dispatch(app.clone(), "GET", "/api/channels/nonesiste/status", None).await;
    assert_eq!(st, 404);
}

#[tokio::test]
async fn a_pairing_code_is_given_only_for_telegram() {
    let (app, _e, _d, _w) = super::tests::app().await;
    let (_, c) = dispatch(app.clone(), "POST", "/api/channels", Some(telegram_body())).await;
    let id = c["id"].as_str().unwrap().to_string();
    let (st, p) = dispatch(app.clone(), "POST", &format!("/api/channels/{id}/pair"), None).await;
    assert_eq!(st, 200, "{p}");
    assert!(p["code"].as_str().unwrap().len() >= 6 && p["command"].as_str().unwrap().starts_with("/start "), "{p}");
    let (_, s) = dispatch(
        app.clone(),
        "POST",
        "/api/channels",
        Some(
            json!({"kind": "slack", "name": "Lavoro", "allowed": ["U024BE7LH"], "secrets": {"app_token": "xapp-1", "bot_token": "xoxb-1"}}),
        ),
    )
    .await;
    assert!(s["id"].is_string(), "{s}");
    let (st, _) = dispatch(app.clone(), "POST", &format!("/api/channels/{}/pair", s["id"].as_str().unwrap()), None).await;
    assert!(st == 400 || st == 422, "{st}");
}

#[tokio::test]
async fn mail_accounts_are_saved_masked_and_the_test_route_answers_in_italian() {
    let (app, _e, _d, _w) = super::tests::app().await;
    let acct = json!({"id": "casa", "host": "127.0.0.1", "port": 1, "username": "mario", "password": "segreto-imap", "folders": []});
    let (st, saved) = dispatch(app.clone(), "PUT", "/api/connectors/mail/accounts", Some(json!([acct]))).await;
    assert_eq!(st, 200, "{saved}");
    assert!(!saved.to_string().contains("segreto-imap"), "{saved}");
    let (_, got) = dispatch(app.clone(), "GET", "/api/connectors/mail/accounts", None).await;
    assert_eq!(got[0]["password"], "••••••", "{got}");
    assert!(!dispatch(app.clone(), "GET", "/api/config", None).await.1.to_string().contains("segreto-imap"));
    // Nothing listens on port 1: a clear answer, not an error status.
    let (st, t) = dispatch(app.clone(), "POST", "/api/connectors/mail/test", Some(acct)).await;
    assert_eq!((st, t["ok"].as_bool()), (200, Some(false)), "{t}");
    assert!(!t["detail"].as_str().unwrap().is_empty());
    let (st, _) = dispatch(app.clone(), "PUT", "/api/connectors/mail/accounts", Some(json!([]))).await;
    assert_eq!(st, 200);
}
