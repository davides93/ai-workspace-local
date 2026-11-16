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
    let (app, e, _d, _w) = super::tests::app().await;
    // Nothing here may reach the real Telegram: the connector that starts below finds nobody on port 1.
    e.set_channel_endpoints_for_tests(Some("http://127.0.0.1:1".into()), None, 0, 20, 100);
    let (_, c) = dispatch(app.clone(), "POST", "/api/channels", Some(telegram_body())).await;
    let id = c["id"].as_str().unwrap().to_string();
    // The channel is created off, and nobody reads the messages of a channel that is off: no code (409, with what to do).
    let (st, p) = dispatch(app.clone(), "POST", &format!("/api/channels/{id}/pair"), None).await;
    assert_eq!(st, 409, "{p}");
    assert!(p["error"].as_str().unwrap().contains("Accendi prima il canale"), "{p}");
    let (st, on) = dispatch(app.clone(), "PATCH", &format!("/api/channels/{id}"), Some(json!({"enabled": true}))).await;
    assert_eq!(st, 200, "{on}");
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

// ------------------------------------------------------------------------------------ shared contexts

async fn telegram(app: &Router, name: &str, allowed: Value, mode: &str) -> String {
    let (st, c) = dispatch(
        app.clone(),
        "POST",
        "/api/channels",
        Some(json!({"kind": "telegram", "name": name, "mode": mode, "allowed": allowed, "secrets": {"bot_token": "123456:SEGRETISSIMO"}})),
    )
    .await;
    assert_eq!(st, 200, "{c}");
    c["id"].as_str().unwrap().to_string()
}

fn slack_body(name: &str) -> Value {
    json!({"kind": "slack", "name": name, "mode": "ask", "allowed": ["U024BE7LH"], "secrets": {"app_token": "xapp-1", "bot_token": "xoxb-1"}})
}

#[tokio::test]
async fn a_shared_context_is_linked_explicitly_checked_and_removed_over_http() {
    let (app, _e, _d, _w) = super::tests::app().await;
    let tg = telegram(&app, "Telegram", json!(["111", "-100777"]), "ask").await;
    let (_, sl) = dispatch(app.clone(), "POST", "/api/channels", Some(slack_body("Slack"))).await;
    let sl = sl["id"].as_str().unwrap().to_string();
    assert_eq!(dispatch(app.clone(), "GET", "/api/contexts", None).await.1, json!([]));
    // Not linked by themselves: two accounts on two channels stay two conversations until the owner says so.
    let members = json!([{"channel_id": tg, "sender": "111"}, {"channel_id": sl, "sender": "u024be7lh"}]);
    let (st, bad) = dispatch(app.clone(), "POST", "/api/contexts", Some(json!({"name": "Mario", "members": members}))).await;
    assert_eq!(st, 400, "a Slack id has to be written as on the list: {bad}");
    let members = json!([{"channel_id": tg, "sender": "111"}, {"channel_id": sl, "sender": "U024BE7LH"}]);
    let (st, c) = dispatch(app.clone(), "POST", "/api/contexts", Some(json!({"name": "Mario", "members": members}))).await;
    assert_eq!(st, 200, "{c}");
    assert_eq!((c["mode"].as_str(), c["members_info"].as_array().map(|m| m.len())), (Some("ask"), Some(2)), "{c}");
    assert!(c["session_id"].as_str().unwrap().starts_with("ses_"), "{c}");
    let id = c["id"].as_str().unwrap().to_string();
    // The shared session is listed in the chat of the app, marked as shared and restricted like the channels.
    let (_, sessions) = dispatch(app.clone(), "GET", "/api/sessions", None).await;
    let s = sessions.as_array().unwrap().iter().find(|s| s["id"] == c["session_id"]).expect("listed");
    assert_eq!((s["channel"].as_str(), s["agent_id"].as_str().map(|a| a.starts_with("channel:ask:"))), (Some("shared"), Some(true)), "{s}");
    // A group is not a person; an account is in one context only; a channel of a context keeps its assistant.
    let (st, e) = dispatch(
        app.clone(),
        "POST",
        "/api/contexts",
        Some(json!({"name": "Gruppo", "members": [{"channel_id": tg, "sender": "-100777"}]})),
    )
    .await;
    assert_eq!(st, 400, "{e}");
    let (st, e) =
        dispatch(app.clone(), "POST", "/api/contexts", Some(json!({"name": "Doppio", "members": [{"channel_id": tg, "sender": "111"}]})))
            .await;
    assert_eq!(st, 409, "{e}");
    let (st, e) = dispatch(app.clone(), "PATCH", &format!("/api/channels/{tg}"), Some(json!({"agent_id": "coder"}))).await;
    assert_eq!(st, 409, "{e}");
    // Renamed, then reduced to one account.
    let (st, p) = dispatch(app.clone(), "PATCH", &format!("/api/contexts/{id}"), Some(json!({"name": "Mario (tutto)"}))).await;
    assert_eq!((st, p["name"].as_str()), (200, Some("Mario (tutto)")), "{p}");
    let (st, p) =
        dispatch(app.clone(), "PATCH", &format!("/api/contexts/{id}"), Some(json!({"members": [{"channel_id": tg, "sender": "111"}]})))
            .await;
    assert_eq!((st, p["members"].as_array().map(|m| m.len())), (200, Some(1)), "{p}");
    // The activity log says who linked what.
    let (_, audit) = dispatch(app.clone(), "GET", "/api/audit", None).await;
    assert!(audit.to_string().contains("context.create") && audit.to_string().contains("context.update"), "{audit}");
    // Deleting it keeps the history in the chat.
    let (st, _) = dispatch(app.clone(), "DELETE", &format!("/api/contexts/{id}"), None).await;
    assert!(st == 200 || st == 204, "{st}");
    assert_eq!(dispatch(app.clone(), "GET", "/api/contexts", None).await.1, json!([]));
    let (_, sessions) = dispatch(app.clone(), "GET", "/api/sessions", None).await;
    assert!(sessions.as_array().unwrap().iter().any(|s| s["id"] == c["session_id"]));
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/contexts/{id}"), None).await.0, 404);
}

// ------------------------------------------------------------------------------------ email drafts

/// A request with the token of a paired device, from the LAN.
async fn device_status(app: &Router, token: &str, method: &str, uri: &str, body: Option<Value>) -> u16 {
    use super::tests_security::{req, send, LAN};
    let b = format!("Bearer {token}");
    let headers = [("host", "192.168.1.5:7777"), ("authorization", b.as_str())];
    send(app, req(method, uri, Some(LAN), &headers, body)).await.0.as_u16()
}

#[tokio::test]
async fn an_email_draft_goes_out_over_http_only_with_the_approval_of_the_owner() {
    use aiw_tools::connectors::testing::{FakeImap, FakeSmtp, ImapOptions, Security};
    let (app, e, _d, _w) = super::tests::app().await;
    let imap = FakeImap::start(ImapOptions::new(Security::StartTls, &[("INBOX", vec![])])).await;
    let smtp = FakeSmtp::start(Security::StartTls, "assistente@casa.example", "pw-smtp").await;
    e.set_tls_roots_for_tests(vec![imap.cert.clone(), smtp.cert.clone()]);
    e.set_channel_endpoints_for_tests(None, None, 0, 20, 100);
    // Without an email channel there is nothing to send through.
    let (st, none) =
        dispatch(app.clone(), "POST", "/api/email-drafts", Some(json!({"to": ["anna@cliente.it"], "subject": "x", "body": "y"}))).await;
    assert_eq!(st, 400, "{none}");
    let (st, ch) = dispatch(
        app.clone(),
        "POST",
        "/api/channels",
        Some(json!({
            "kind": "email", "name": "Posta", "allowed": ["mario@example.it"],
            "secrets": {"imap_password": "segreto", "smtp_password": "pw-smtp"},
            "settings": {"imap_host": "127.0.0.1", "imap_port": imap.addr.port(), "imap_user": "mario", "smtp_host": "127.0.0.1",
                         "smtp_port": smtp.addr.port(), "smtp_user": "assistente@casa.example", "from": "assistente@casa.example"}
        })),
    )
    .await;
    assert_eq!(st, 200, "{ch}");
    let ch = ch["id"].as_str().unwrap().to_string();
    let (st, d) = dispatch(
        app.clone(),
        "POST",
        "/api/email-drafts",
        Some(json!({"to": ["anna@cliente.it"], "cc": ["capo@esempio.it"], "subject": "Preventivo", "body": "Buongiorno, in allegato il preventivo."})),
    )
    .await;
    assert_eq!(st, 200, "{d}");
    let id = d["id"].as_str().unwrap().to_string();
    let hash = d["content_hash"].as_str().unwrap().to_string();
    assert_eq!(
        (d["status"].as_str(), d["from"].as_str(), d["created_by"].as_str()),
        (Some("draft"), Some("assistente@casa.example"), Some("user")),
        "{d}"
    );
    assert!(smtp.mails.lock().unwrap().is_empty());
    // The channel has to be on; a stale hash is a 409; a device token can look and refuse but not send or write.
    e.channel_patch(&ch, aiw_core::api::ChannelPatch { enabled: Some(true), ..Default::default() }).unwrap();
    let dev = e.create_api_token("Telefono", "device", None, None, None).unwrap().token;
    let as_device = |m: &'static str, p: String, b: Option<Value>| {
        let (app, dev) = (app.clone(), dev.clone());
        async move { device_status(&app, &dev, m, &p, b).await }
    };
    assert_eq!(as_device("GET", "/api/email-drafts".into(), None).await, 200);
    assert_eq!(as_device("POST", format!("/api/email-drafts/{id}/send"), Some(json!({"approved_hash": hash}))).await, 403);
    assert_eq!(as_device("POST", "/api/email-drafts".into(), Some(json!({"to": ["x@y.it"]}))).await, 403);
    assert!(smtp.mails.lock().unwrap().is_empty());
    let (st, stale) =
        dispatch(app.clone(), "POST", &format!("/api/email-drafts/{id}/send"), Some(json!({"approved_hash": "0".repeat(64)}))).await;
    assert_eq!(st, 409, "{stale}");
    let (st, sent) = dispatch(app.clone(), "POST", &format!("/api/email-drafts/{id}/send"), Some(json!({"approved_hash": hash}))).await;
    assert_eq!((st, sent["status"].as_str()), (200, Some("sent")), "{sent}");
    // The same approval again: the draft as it is, and nothing more goes out.
    let (st, again) = dispatch(app.clone(), "POST", &format!("/api/email-drafts/{id}/send"), Some(json!({"approved_hash": hash}))).await;
    assert_eq!((st, again["status"].as_str()), (200, Some("sent")), "{again}");
    assert_eq!(smtp.mails.lock().unwrap().len(), 1);
    // A second draft, refused by the paired device: it can stop an email, never send one.
    let (_, d2) =
        dispatch(app.clone(), "POST", "/api/email-drafts", Some(json!({"to": ["bruno@cliente.it"], "subject": "No", "body": "No"}))).await;
    let id2 = d2["id"].as_str().unwrap().to_string();
    assert_eq!(as_device("POST", format!("/api/email-drafts/{id2}/reject"), Some(json!({"reason": "non serve"}))).await, 200);
    let (_, r) = dispatch(app.clone(), "GET", &format!("/api/email-drafts/{id2}"), None).await;
    assert_eq!((r["status"].as_str(), r["note"].as_str()), (Some("rejected"), Some("non serve")), "{r}");
    let (st, _) =
        dispatch(app.clone(), "POST", &format!("/api/email-drafts/{id2}/send"), Some(json!({"approved_hash": d2["content_hash"]}))).await;
    assert_eq!(st, 409);
    assert_eq!(smtp.mails.lock().unwrap().len(), 1);
    let (_, list) = dispatch(app.clone(), "GET", "/api/email-drafts?status=sent", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(dispatch(app.clone(), "GET", "/api/email-drafts?status=boh", None).await.0, 400);
}
