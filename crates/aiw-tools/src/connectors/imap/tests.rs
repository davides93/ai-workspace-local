//! The IMAP source against an in-process fake server.

use super::super::testing::*;
use super::super::*;
use chrono::Local;

fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn days_ago(n: i64) -> String {
    (Local::now() - Duration::days(n)).format("%Y-%m-%d").to_string()
}

fn account(srv: &FakeImap, security: Security) -> imap::Account {
    imap::Account {
        id: "casa".into(),
        host: "127.0.0.1".into(),
        port: srv.addr.port(),
        username: "mario".into(),
        password: "segreto".into(),
        folders: vec!["INBOX".into()],
        tls: if security == Security::Implicit { "implicit" } else { "starttls" }.into(),
    }
}

fn settings(acct: imap::Account, roots: Vec<Vec<u8>>) -> Shared {
    ConnectorSettings::new(Config { mail_enabled: true, imap: vec![acct], extra_roots: roots, ..Default::default() })
}

fn ctx() -> ToolContext {
    ToolContext::new("r", std::env::temp_dir())
}

fn mail_tool(shared: &Shared, name: &str) -> Arc<dyn Tool> {
    tools(shared.clone()).into_iter().find(|t| t.name() == name).unwrap()
}

#[test]
fn only_reading_commands_are_allowed() {
    use imap::{command_allowed as ok, Access::*};
    for c in [
        "CAPABILITY",
        "STARTTLS",
        "LOGIN \"a\" \"b\"",
        "EXAMINE \"INBOX\"",
        "UID SEARCH SINCE 01-Jan-2026",
        "UID FETCH 1,2 (UID BODY.PEEK[]<0.100>)",
        "NOOP",
        "LOGOUT",
    ] {
        assert!(ok(c, ReadOnly), "{c}");
    }
    for c in [
        "SELECT \"INBOX\"",
        "UID STORE 1 +FLAGS (\\Seen)",
        "STORE 1 +FLAGS (\\Deleted)",
        "EXPUNGE",
        "APPEND \"INBOX\" {5}",
        "UID COPY 1 \"x\"",
        "UID MOVE 1 \"x\"",
        "DELETE \"x\"",
        "CREATE \"x\"",
        "UID FETCH 1 (BODY[])",
        "UID FETCH 1 (RFC822)",
        "UID EXPUNGE 1",
    ] {
        assert!(!ok(c, ReadOnly), "{c}");
        assert!(!ok(c, MarkSeen) || c.starts_with("SELECT") || c.starts_with("UID STORE"), "{c}");
    }
    assert!(ok("SELECT \"INBOX\"", MarkSeen) && ok("UID STORE 1 +FLAGS.SILENT (\\Seen)", MarkSeen));
}

#[tokio::test]
async fn reads_with_examine_and_peek_and_never_marks_anything() {
    let srv = FakeImap::start(ImapOptions::new(
        Security::Implicit,
        &[(
            "INBOX",
            vec![
                imap_message(1, &today(), "Anna <anna@example.org>", "Fattura di marzo", "Ti mando la fattura venerdì."),
                imap_message(2, &today(), "Luca <luca@example.org>", "Pranzo", "Ci vediamo a mezzogiorno."),
            ],
        )],
    ))
    .await;
    let shared = settings(account(&srv, Security::Implicit), vec![srv.cert.clone()]);
    let recent = mail_tool(&shared, "mail_recent").call(json!({"hours": 48}), &ctx()).await.unwrap().content;
    assert!(recent.contains("Fattura di marzo") && recent.contains("Pranzo"), "{recent}");
    let found = mail_tool(&shared, "mail_search").call(json!({"query": "fattura venerdì"}), &ctx()).await.unwrap().content;
    assert!(found.contains("Fattura di marzo") && !found.contains("Pranzo"), "{found}");

    let cmds = srv.commands();
    assert!(cmds.iter().any(|c| c.starts_with("EXAMINE")), "{cmds:?}");
    assert!(cmds.iter().any(|c| c.contains("BODY.PEEK[]")), "{cmds:?}");
    for c in &cmds {
        let u = c.to_ascii_uppercase();
        for bad in ["SELECT", "STORE", "EXPUNGE", "APPEND", "COPY", "MOVE", "DELETE", "BODY[", "RFC822 "] {
            assert!(!u.contains(bad), "forbidden `{bad}` in {c}");
        }
    }
    assert!(srv.seen.lock().unwrap().is_empty(), "nothing is marked as read");
    // The second call came from the short cache: the server saw one login only.
    assert_eq!(cmds.iter().filter(|c| c.starts_with("LOGIN")).count(), 1, "{cmds:?}");
}

#[tokio::test]
async fn caps_on_age_count_and_size() {
    let mut msgs: Vec<ImapMessage> =
        (1..=30).map(|u| imap_message(u, &today(), "x@example.org", &format!("Messaggio {u}"), "testo")).collect();
    msgs.push(imap_message(31, &days_ago(60), "vecchio@example.org", "Troppo vecchio", "ciao"));
    let srv = FakeImap::start(ImapOptions::new(Security::Implicit, &[("INBOX", msgs)])).await;
    let shared = ConnectorSettings::new(Config {
        mail_enabled: true,
        imap: vec![account(&srv, Security::Implicit)],
        imap_days: 14,
        imap_max_messages: 5,
        extra_roots: vec![srv.cert.clone()],
        ..Default::default()
    });
    let (mails, notes) = shared.mail_search("messaggio").await;
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(mails.len(), 5, "at most five messages per folder");
    assert!(mails.iter().all(|m| !m.subject.contains("vecchio")), "older than the window: not fetched");
    let titles: Vec<&str> = mails.iter().map(|m| m.subject.as_str()).collect();
    assert!(titles.contains(&"Messaggio 30") && !titles.contains(&"Messaggio 1"), "the newest ones: {titles:?}");
    let fetch = srv.commands().into_iter().find(|c| c.starts_with("UID FETCH")).unwrap();
    assert!(fetch.contains(&format!("<0.{IMAP_MAX_BYTES}>")), "size cap per message: {fetch}");
}

#[tokio::test]
async fn a_wrong_password_is_reported_in_plain_words() {
    let srv = FakeImap::start(ImapOptions::new(Security::Implicit, &[("INBOX", vec![])])).await;
    let mut acct = account(&srv, Security::Implicit);
    acct.password = "sbagliata".into();
    let shared = settings(acct, vec![srv.cert.clone()]);
    let out = mail_tool(&shared, "mail_recent").call(json!({}), &ctx()).await.unwrap().content;
    assert!(out.contains("account «casa»") && out.contains("password non accettati"), "{out}");
    assert!(!out.contains("sbagliata"), "the password is never echoed");
}

#[tokio::test]
async fn an_invalid_certificate_is_refused() {
    let srv = FakeImap::start(ImapOptions::new(Security::Implicit, &[("INBOX", vec![])])).await;
    // The self-signed certificate is not trusted this time.
    let shared = settings(account(&srv, Security::Implicit), vec![]);
    let (_, notes) = shared.mail_search("x").await;
    assert!(notes.iter().any(|n| n.contains("certificato") && n.contains("non mi collego")), "{notes:?}");
    assert!(!srv.commands().iter().any(|c| c.starts_with("LOGIN")), "no login before the certificate checks out");
}

#[tokio::test]
async fn tls_is_required_the_password_never_goes_in_clear() {
    // A server on a non-993 port that does not offer STARTTLS.
    let srv = FakeImap::start(ImapOptions::new(Security::PlainOnly, &[("INBOX", vec![])])).await;
    let shared = settings(account(&srv, Security::StartTls), vec![srv.cert.clone()]);
    let (_, notes) = shared.mail_search("x").await;
    assert!(notes.iter().any(|n| n.contains("cifrata") && n.contains("in chiaro")), "{notes:?}");
    let cmds = srv.commands();
    assert!(!cmds.iter().any(|c| c.starts_with("LOGIN") || c.contains("segreto")), "{cmds:?}");

    // Implicit TLS against a server that speaks plain text: the handshake fails, nothing is sent.
    let plain = FakeImap::start(ImapOptions::new(Security::PlainOnly, &[("INBOX", vec![])])).await;
    let shared = settings(account(&plain, Security::Implicit), vec![plain.cert.clone()]);
    let (_, notes) = shared.mail_search("x").await;
    assert!(!notes.is_empty());
    assert!(!plain.commands().iter().any(|c| c.starts_with("LOGIN")));
}

#[tokio::test]
async fn starttls_works_when_the_server_offers_it() {
    let srv = FakeImap::start(ImapOptions::new(
        Security::StartTls,
        &[("INBOX", vec![imap_message(1, &today(), "a@example.org", "Via STARTTLS", "ciao")])],
    ))
    .await;
    let shared = settings(account(&srv, Security::StartTls), vec![srv.cert.clone()]);
    let (mails, notes) = shared.mail_search("starttls").await;
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(mails.len(), 1);
    let cmds = srv.commands();
    let login = cmds.iter().position(|c| c.starts_with("LOGIN")).unwrap();
    let starttls = cmds.iter().position(|c| c == "STARTTLS").unwrap();
    assert!(starttls < login, "the channel is encrypted before the login: {cmds:?}");
}

#[tokio::test]
async fn an_unreachable_server_and_a_missing_folder_are_explained() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let acct = imap::Account {
        id: "x".into(),
        host: "127.0.0.1".into(),
        port,
        username: "u".into(),
        password: "p".into(),
        folders: vec![],
        tls: "implicit".into(),
    };
    let (_, notes) = settings(acct, vec![]).mail_search("x").await;
    assert!(notes.iter().any(|n| n.contains("non è raggiungibile")), "{notes:?}");

    let srv = FakeImap::start(ImapOptions::new(Security::Implicit, &[("INBOX", vec![])])).await;
    let mut acct = account(&srv, Security::Implicit);
    acct.folders = vec!["INBOX".into(), "Archivio".into()];
    let (_, notes) = settings(acct, vec![srv.cert.clone()]).mail_search("x").await;
    assert!(notes.iter().any(|n| n.contains("«Archivio» non esiste")), "{notes:?}");
}

#[tokio::test]
async fn the_test_button_counts_recent_messages() {
    let srv = FakeImap::start(ImapOptions::new(
        Security::Implicit,
        &[(
            "INBOX",
            vec![imap_message(1, &today(), "a@example.org", "Uno", "x"), imap_message(2, &days_ago(30), "a@example.org", "Due", "x")],
        )],
    ))
    .await;
    let ok = test_account(&account(&srv, Security::Implicit), std::slice::from_ref(&srv.cert)).await.unwrap();
    assert_eq!(ok.folders, vec![("INBOX".to_string(), 1)]);
    let mut bad = account(&srv, Security::Implicit);
    bad.password = "no".into();
    assert_eq!(test_account(&bad, std::slice::from_ref(&srv.cert)).await.unwrap_err(), imap::ImapError::Auth);
}
