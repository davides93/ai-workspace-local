//! The secrets at rest through the engine: what the app's own functions write, on disk, with a keychain, without one, and after
//! a restart. (The vault itself is tested in `vault::tests`.)

use crate::vault::{Keychain, MemoryKeyStore, Phase};
use crate::{Engine, EngineOptions};
use aiw_core::api::{ChannelCreate, MailAccount, McpServerConfig, RemoteAgent};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

// Recognisable fakes: nothing here is a real credential.
const BRAVE: &str = "BRAVE-TESTONLY-S3CR3T-111";
const IMAP: &str = "IMAP-TESTONLY-S3CR3T-222";
const TELEGRAM: &str = "TGTOKEN-TESTONLY-S3CR3T-333";
const URL_PASSWORD: &str = "URLPASS-TESTONLY-S3CR3T-444";
const MCP_HEADER: &str = "MCPHEADER-TESTONLY-S3CR3T-555";
const MCP_ENV: &str = "MCPENV-TESTONLY-S3CR3T-666";
const PROVIDER: &str = "HFTOKEN-TESTONLY-S3CR3T-777";
const PEER: &str = "PEERTOKEN-TESTONLY-S3CR3T-888";
const REMOTE: &str = "REMOTETOKEN-TESTONLY-S3CR3T-999";
const OAUTH: &str = "OAUTHACCESS-TESTONLY-S3CR3T-000";
const RUNTIME: &str = "RUNTIMEKEY-TESTONLY-S3CR3T-123";
const ALL: [&str; 11] = [BRAVE, IMAP, TELEGRAM, URL_PASSWORD, MCP_HEADER, MCP_ENV, PROVIDER, PEER, REMOTE, OAUTH, RUNTIME];

async fn start(data: &Path, ws: &Path, keychain: Keychain) -> Engine {
    Engine::new(EngineOptions {
        data_dir: Some(data.to_path_buf()),
        workspace_root: Some(ws.to_path_buf()),
        mode: "embedded".into(),
        offline: true,
        keychain,
        ..Default::default()
    })
    .await
    .unwrap()
}

async fn write_everything(e: &Engine) {
    e.set_config("global", "search.brave_api_key", json!(BRAVE)).unwrap();
    e.set_mail_accounts(vec![MailAccount {
        id: "personale".into(),
        host: "imap.example.org".into(),
        port: 993,
        username: "io@example.org".into(),
        password: IMAP.into(),
        folders: vec![],
    }])
    .unwrap();
    e.channel_create(ChannelCreate {
        kind: "telegram".into(),
        name: "Bot".into(),
        secrets: [("bot_token".to_string(), TELEGRAM.to_string())].into(),
        ..Default::default()
    })
    .unwrap();
    e.save_mcp_server(McpServerConfig {
        id: "web".into(),
        name: "web".into(),
        transport: "http".into(),
        command: None,
        args: None,
        url: Some(format!("https://utente:{URL_PASSWORD}@mcp.example.org/mcp")),
        env: Some([("MCP_API_TOKEN".to_string(), json!(MCP_ENV))].into_iter().collect()),
        headers: Some([("Authorization".to_string(), json!(format!("Bearer {MCP_HEADER}")))].into_iter().collect()),
        enabled: false,
        sampling: false,
        oauth_client_id: None,
        sandbox: "off".into(),
        sandbox_network: true,
    })
    .await
    .unwrap();
    e.save_remote_agent(RemoteAgent {
        id: "r1".into(),
        name: "Altro".into(),
        url: "http://altro:7777".into(),
        token: REMOTE.into(),
        skill: None,
        description: String::new(),
        paired: false,
    })
    .unwrap();
    e.update_runtime_settings("remote-openai", [("api_key".to_string(), json!(RUNTIME))].into_iter().collect()).unwrap();
    let s = &e.0.store;
    s.put_doc("model_provider", "hf", &json!({"id": "hf", "token": PROVIDER})).unwrap();
    s.put_doc("peer_token", "p1", &json!({"token": PEER, "pin": "AA:BB"})).unwrap();
    s.put_doc("mcp_oauth", "web", &json!({"access_token": OAUTH, "refresh_token": null})).unwrap();
}

/// Every value, read back through the same functions the app reads with.
fn assert_all_read_back(e: &Engine) {
    let s = &e.0.store;
    assert_eq!(e.0.config.get_str("search.brave_api_key").as_deref(), Some(BRAVE));
    assert_eq!(s.get_doc::<Value>("secret", "imap:personale").unwrap().unwrap()["value"], IMAP);
    assert_eq!(s.list_docs::<Value>("channel").unwrap()[0]["secrets"]["bot_token"], TELEGRAM);
    let mcp: McpServerConfig = s.get_doc("mcp", "web").unwrap().unwrap();
    assert_eq!(mcp.url.as_deref(), Some(format!("https://utente:{URL_PASSWORD}@mcp.example.org/mcp").as_str()));
    assert_eq!(mcp.env.unwrap()["MCP_API_TOKEN"], MCP_ENV);
    assert_eq!(mcp.headers.unwrap()["Authorization"], format!("Bearer {MCP_HEADER}"));
    assert_eq!(s.get_doc::<RemoteAgent>("remote_agent", "r1").unwrap().unwrap().token, REMOTE);
    assert_eq!(s.get_doc::<Value>("runtime_settings", "remote-openai").unwrap().unwrap()["settings"]["api_key"], RUNTIME);
    assert_eq!(s.get_doc::<Value>("model_provider", "hf").unwrap().unwrap()["token"], PROVIDER);
    assert_eq!(s.get_doc::<Value>("peer_token", "p1").unwrap().unwrap()["token"], PEER);
    assert_eq!(s.get_doc::<Value>("mcp_oauth", "web").unwrap().unwrap()["access_token"], OAUTH);
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// The files of the data folder that could hold a value: the database and its log, the settings files.
fn stored_files(data: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![];
    for rel in ["aiw.db", "aiw.db-wal", "config.json"] {
        if let Ok(b) = std::fs::read(data.join(rel)) {
            out.push((rel.to_string(), b));
        }
    }
    if let Ok(rd) = std::fs::read_dir(data.join("devices")) {
        for f in rd.flatten() {
            out.push((format!("devices/{}", f.file_name().to_string_lossy()), std::fs::read(f.path()).unwrap_or_default()));
        }
    }
    assert!(out.iter().any(|(n, b)| n == "aiw.db" && !b.is_empty()), "the scan found no database");
    out
}

fn assert_none_in_plain(data: &Path) {
    for (name, bytes) in stored_files(data) {
        for secret in ALL {
            assert!(!contains(&bytes, secret), "{secret} is in plain in {name}");
        }
    }
}

#[tokio::test]
async fn every_secret_the_app_writes_is_sealed_on_disk_and_works_after_a_restart() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let ks = MemoryKeyStore::new();
    let e = start(data.path(), ws.path(), Keychain::Custom(ks.clone())).await;
    assert_eq!(e.0.vault.phase(), Phase::Active);
    write_everything(&e).await;
    assert_all_read_back(&e);
    let st = e.secrets_at_rest();
    assert_eq!((st.state.as_str(), st.plain, st.unreadable), ("active", 0, 0), "{st:?}");
    assert!(st.encrypted >= 11, "{st:?}");
    assert_none_in_plain(data.path());
    drop(e);

    // A restart with the same keychain reads everything, and nothing is plain on disk afterwards either.
    let e = start(data.path(), ws.path(), Keychain::Custom(ks.clone())).await;
    assert_all_read_back(&e);
    assert_eq!(ks.len(), 1);
    drop(e);
    assert_none_in_plain(data.path());
}

#[tokio::test]
async fn an_install_from_before_is_migrated_at_the_first_start_with_a_keychain() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    // Before: no keychain, plain values everywhere.
    let e = start(data.path(), ws.path(), Keychain::None).await;
    write_everything(&e).await;
    let st = e.secrets_at_rest();
    assert_eq!((st.state.as_str(), st.encrypted), ("not_used", 0), "{st:?}");
    assert!(st.plain >= 11);
    drop(e);
    for secret in [BRAVE, PROVIDER, REMOTE] {
        assert!(stored_files(data.path()).iter().any(|(_, b)| contains(b, secret)), "{secret} should be plain before the migration");
    }
    // After the update: sealed, and every function still gets its value.
    let ks = MemoryKeyStore::new();
    let e = start(data.path(), ws.path(), Keychain::Custom(ks.clone())).await;
    assert_all_read_back(&e);
    assert_eq!(e.secrets_at_rest().plain, 0);
    drop(e);
    assert_none_in_plain(data.path());
}

#[tokio::test]
async fn without_a_keychain_everything_works_as_before_and_the_diagnostics_say_so() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let ks = MemoryKeyStore::new();
    ks.deny(Some("nessun servizio dei segreti"));
    let e = start(data.path(), ws.path(), Keychain::Custom(ks.clone())).await;
    write_everything(&e).await;
    assert_all_read_back(&e);
    let st = e.secrets_at_rest();
    assert_eq!(st.state, "unavailable");
    assert!(ks.is_empty());
    let diag = e.diagnostics().await.unwrap();
    assert_eq!(diag["health"]["secrets_at_rest"]["state"], "unavailable");
    assert!(diag["health"]["secrets_at_rest"]["plain"].as_u64().unwrap() >= 11);
    // The diagnostics never carry a value (the safety net masks them, and the status holds counts only).
    let text = diag.to_string();
    for secret in ALL {
        assert!(!text.contains(secret), "{secret} is in the diagnostics");
    }
}

#[tokio::test]
async fn the_status_document_has_counts_and_words_never_a_value() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = start(data.path(), ws.path(), Keychain::Custom(MemoryKeyStore::new())).await;
    write_everything(&e).await;
    let json = serde_json::to_string(&e.secrets_at_rest()).unwrap();
    for secret in ALL {
        assert!(!json.contains(secret));
    }
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["state"], "active");
    assert!(v["backend"].as_str().is_some());
    assert!(v.get("detail").is_none(), "no detail while everything is fine: {v}");
}

#[tokio::test]
async fn a_backup_with_secrets_made_by_the_engine_is_portable() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = start(data.path(), ws.path(), Keychain::Custom(MemoryKeyStore::new())).await;
    write_everything(&e).await;
    let out = data.path().join("backup.tar.gz");
    let m = e.backup_create(&crate::backup::BackupOptions { include_secrets: true, ..Default::default() }, &out).unwrap();
    assert!(m.sensitive && m.skipped.is_empty(), "{:?}", m.skipped);
    // Another computer: another data folder, another keychain.
    let (other, ws2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    crate::backup::restore(&aiw_core::paths::DataDirs::at(other.path().to_path_buf()), &out).unwrap();
    let e2 = start(other.path(), ws2.path(), Keychain::Custom(Arc::new(MemoryKeyStore::default()))).await;
    assert_eq!(e2.0.vault.phase(), Phase::Active);
    assert_all_read_back(&e2);
    drop(e2);
    assert_none_in_plain(other.path());
}

#[tokio::test]
async fn a_default_backup_made_by_the_engine_has_no_secret_sealed_or_plain_and_keeps_the_address_without_its_password() {
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let e = start(data.path(), ws.path(), Keychain::Custom(MemoryKeyStore::new())).await;
    write_everything(&e).await;
    let out = data.path().join("default.tar.gz");
    let m = e.backup_create(&crate::backup::BackupOptions::default(), &out).unwrap();
    assert!(!m.sensitive);
    let (other, ws2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    crate::backup::restore(&aiw_core::paths::DataDirs::at(other.path().to_path_buf()), &out).unwrap();
    for (name, bytes) in stored_files(other.path()) {
        assert!(!contains(&bytes, "aiwenc1:"), "a sealed value travelled in {name}");
        for secret in ALL {
            assert!(!contains(&bytes, secret), "{secret} travelled in {name}");
        }
    }
    let e2 = start(other.path(), ws2.path(), Keychain::None).await;
    let mcp: McpServerConfig = e2.0.store.get_doc("mcp", "web").unwrap().unwrap();
    assert_eq!(mcp.url.as_deref(), Some("https://mcp.example.org/mcp"), "the host and path stay, the password goes");
    assert_eq!(mcp.name, "web");
    assert_eq!(mcp.headers.unwrap()["Authorization"], "", "the secret is emptied, the key stays");
}
