//! Backup and diagnostics with an engine that holds real secrets (made-up ones, each with a marker that can be searched for).

use super::tests::engine;
use super::*;
use crate::backup::BackupOptions;
use aiw_core::api::{ChannelCreate, MailAccount};
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Read;

/// Each secret has a marker: if it shows up anywhere it should not, the test says which one.
const SECRETS: [(&str, &str); 12] = [
    ("brave key in the settings", "BRAVEKEY-S3CR3T-111"),
    ("IMAP password", "IMAPPASS-S3CR3T-222"),
    ("Telegram bot token", "TGTOKEN-S3CR3T-333"),
    ("MCP Authorization header", "MCPHEADER-S3CR3T-555"),
    ("password inside an MCP address", "URLPASS-S3CR3T-444"),
    ("model provider token", "HFTOKEN-S3CR3T-666"),
    ("paired device token", "PEERTOKEN-S3CR3T-777"),
    ("remote agent token", "REMOTETOKEN-S3CR3T-888"),
    ("MCP OAuth access token", "OAUTHACCESS-S3CR3T-999"),
    ("MCP OAuth refresh token", "OAUTHREFRESH-S3CR3T-000"),
    ("TLS private key", "TLSKEY-S3CR3T-123"),
    ("MCP environment variable", "MCPENV-S3CR3T-321"),
];

async fn engine_with_secrets() -> (Engine, tempfile::TempDir, tempfile::TempDir) {
    let (e, data, ws) = engine(vec![]).await;
    let m = |n: &str| SECRETS.iter().find(|(l, _)| *l == n).unwrap().1.to_string();
    e.set_config("global", "search.brave_api_key", json!(m("brave key in the settings"))).unwrap();
    e.set_mail_accounts(vec![MailAccount {
        id: "personale".into(),
        host: "imap.example.org".into(),
        port: 993,
        username: "io@example.org".into(),
        password: m("IMAP password"),
        folders: vec![],
    }])
    .unwrap();
    e.channel_create(ChannelCreate {
        kind: "telegram".into(),
        name: "Bot".into(),
        secrets: [("bot_token".to_string(), m("Telegram bot token"))].into(),
        ..Default::default()
    })
    .unwrap();
    e.save_mcp_server(McpServerConfig {
        id: "web".into(),
        name: "web".into(),
        transport: "http".into(),
        command: None,
        args: None,
        url: Some(format!("https://utente:{}@mcp.example.org/mcp", m("password inside an MCP address"))),
        env: Some([("MCP_API_TOKEN".to_string(), json!(m("MCP environment variable")))].into_iter().collect()),
        headers: Some([("Authorization".to_string(), json!(format!("Bearer {}", m("MCP Authorization header"))))].into_iter().collect()),
        enabled: false,
        sampling: false,
        oauth_client_id: None,
        sandbox: "off".into(),
        sandbox_network: true,
    })
    .await
    .unwrap();
    let s = &e.0.store;
    s.put_doc("model_provider", "hf", &json!({"id": "hf", "token": m("model provider token")})).unwrap();
    s.put_doc("peer_token", "p1", &json!({"token": m("paired device token"), "pin": "AA:BB"})).unwrap();
    s.put_doc("remote_agent", "r1", &json!({"id": "r1", "name": "Altro", "url": "http://altro:7777", "token": m("remote agent token")}))
        .unwrap();
    s.put_doc("mcp_oauth", "web", &json!({"access_token": m("MCP OAuth access token"), "refresh_token": m("MCP OAuth refresh token")}))
        .unwrap();
    std::fs::create_dir_all(e.data_dirs().root.join("tls")).unwrap();
    std::fs::write(e.data_dirs().root.join("tls").join("key.pem"), format!("-----BEGIN PRIVATE KEY-----\n{}\n", m("TLS private key")))
        .unwrap();
    e.issue_device_token(Some("tel1"), Some("Telefono")).unwrap();
    (e, data, ws)
}

fn all_bytes(archive: &Path) -> BTreeMap<String, Vec<u8>> {
    let manifest = crate::backup::inspect(archive).unwrap();
    let f = std::fs::File::open(archive).unwrap();
    let mut gz = flate2::read::GzDecoder::new(f);
    let mut raw = vec![];
    gz.read_to_end(&mut raw).unwrap();
    // The whole decompressed stream, for a search of the secrets anywhere (headers, names, content), plus the files.
    let mut out = BTreeMap::new();
    out.insert("<tar stream>".to_string(), raw);
    let dir = tempfile::tempdir().unwrap();
    let dirs = DataDirs::at(dir.path().join("x"));
    crate::backup::restore(&dirs, archive).unwrap();
    for f in &manifest.files {
        let p = f.path.split('/').fold(dirs.root.clone(), |p, c| p.join(c));
        out.insert(f.path.clone(), std::fs::read(p).unwrap());
    }
    out
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[tokio::test]
async fn a_default_backup_holds_none_of_the_secrets_and_keeps_the_configuration() {
    let (e, data, _ws) = engine_with_secrets().await;
    let owner_token = std::fs::read_to_string(data.path().join("token")).unwrap();
    let out = data.path().join("b.tar.gz");
    let m = e.backup_create(&BackupOptions::default(), &out).unwrap();
    assert!(!m.sensitive);
    assert!(!m.files.iter().any(|f| f.path == "token" || f.path.starts_with("tls/")));
    let files = all_bytes(&out);
    for (what, marker) in SECRETS.iter().map(|(w, m)| (*w, m.to_string())).chain([("the owner token", owner_token.trim().to_string())]) {
        for (name, bytes) in &files {
            assert!(!contains(bytes, &marker), "{what} ({marker}) is in {name} of a backup that must not hold secrets");
        }
    }
    // What is not secret is there: the tables are intact, the configurations of the channel, server and account remain.
    let copy = tempfile::tempdir().unwrap().keep().join("c.db");
    std::fs::write(&copy, &files["aiw.db"]).unwrap();
    let c = rusqlite::Connection::open(&copy).unwrap();
    let count = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(count("SELECT COUNT(*) FROM api_tokens"), 0, "the access tokens are not in a default backup");
    assert_eq!(count("SELECT COUNT(*) FROM documents WHERE kind IN ('secret','peer_token','mcp_oauth')"), 0);
    assert_eq!(count("SELECT COUNT(*) FROM documents WHERE kind IN ('channel','mcp','model_provider','remote_agent')"), 4);
    let mcp: String = c.query_row("SELECT json FROM documents WHERE kind='mcp'", [], |r| r.get(0)).unwrap();
    let mcp: McpServerConfig = serde_json::from_str(&mcp).unwrap();
    assert_eq!(mcp.url.as_deref(), Some("https://mcp.example.org/mcp"));
    assert_eq!(mcp.headers.unwrap()["Authorization"], "");
    assert_eq!(mcp.env.unwrap()["MCP_API_TOKEN"], "");
    let cfg: Value = serde_json::from_slice(&files["config.json"]).unwrap();
    assert!(cfg.get("search.brave_api_key").is_none() && cfg["connectors.mail.imap"][0]["host"] == "imap.example.org", "{cfg}");
    // The live data are untouched: the secrets are still where they were.
    assert_eq!(e.config().effective["search.brave_api_key"], json!(SECRET_MASK));
    assert!(std::fs::read_to_string(data.path().join("config.json")).unwrap().contains("BRAVEKEY-S3CR3T-111"));
    assert!(e.0.store.get_doc::<Value>("peer_token", "p1").unwrap().is_some());
    // And the backup is noted in the activity log, without a word of its content.
    let audit = e.audit();
    let note = audit.iter().find(|a| a.action == "backup.create").expect("noted");
    assert!(!note.detail.contains("S3CR3T") && note.detail.contains("file"), "{}", note.detail);
}

#[tokio::test]
async fn a_backup_with_secrets_holds_them_and_says_so() {
    let (e, data, _ws) = engine_with_secrets().await;
    let out = data.path().join("s.tar.gz");
    let m = e.backup_create(&BackupOptions { include_secrets: true }, &out).unwrap();
    assert!(m.sensitive, "the manifest flags it");
    assert!(m.files.iter().any(|f| f.path == "token") && m.files.iter().any(|f| f.path == "tls/key.pem"));
    assert!(!m.excluded.iter().any(|x| x.contains("TLS") || x.contains("segreti")), "{:?}", m.excluded);
    assert!(m.excluded.iter().any(|x| x.contains("modelli")), "models are never included: {:?}", m.excluded);
    let files = all_bytes(&out);
    for (what, marker) in SECRETS {
        assert!(
            files.iter().any(|(n, b)| n != "<tar stream>" && contains(b, marker)),
            "{what} ({marker}) should be in a backup that asks for secrets"
        );
    }
    assert!(crate::backup::inspect(&out).unwrap().sensitive);
    // The audit says so.
    assert!(e.audit().iter().any(|a| a.action == "backup.create" && a.detail.contains("segreti")));
}

#[tokio::test]
async fn diagnostics_have_what_support_needs_and_none_of_the_secrets_or_the_content() {
    let (e, data, _ws) = engine_with_secrets().await;
    let owner_token = std::fs::read_to_string(data.path().join("token")).unwrap();
    // Chats, a memory, a document: content that never goes into diagnostics.
    e.0.store
        .upsert_session(&aiw_core::api::Session {
            id: "ses_d".into(),
            title: "TITOLO-PRIVATO-DELLA-CHAT".into(),
            agent_id: "operator".into(),
            created_at: "x".into(),
            updated_at: "x".into(),
            last_status: None,
            worktree: None,
            channel: None,
        })
        .unwrap();
    e.0.store
        .append_messages(
            "ses_d",
            &[aiw_core::api::ChatMessage {
                id: "m1".into(),
                role: "user".into(),
                content: "TESTO-PRIVATO-DELLA-CHAT".into(),
                reasoning: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
                attachments: None,
                created_at: "x".into(),
            }],
        )
        .unwrap();
    // A log with the usual leaks and a path that names the person.
    let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
    std::fs::create_dir_all(e.data_dirs().logs()).unwrap();
    std::fs::write(
        e.data_dirs().logs().join("aiw.log"),
        format!(
            "avvio normale\nGET /x Authorization: Bearer LOGBEARER-S3CR3T-abc\nconfig token=LOGTOKEN-S3CR3T-def password: LOGPASS-S3CR3T-ghi\nchiamata https://u:LOGURLPASS-S3CR3T-jkl@host.example/api?api_key=LOGQUERY-S3CR3T-mno\nclone di {home}/progetti/x\nlo stesso segreto: {}\n",
            SECRETS[0].1
        ),
    )
    .unwrap();

    let doc = e.diagnostics().await.unwrap();
    let text = serde_json::to_string_pretty(&doc).unwrap();

    // No secret, wherever it is.
    for (what, marker) in SECRETS {
        assert!(!text.contains(marker), "{what} ({marker}) leaked into the diagnostics");
    }
    for marker in [
        "LOGBEARER-S3CR3T-abc",
        "LOGTOKEN-S3CR3T-def",
        "LOGPASS-S3CR3T-ghi",
        "LOGURLPASS-S3CR3T-jkl",
        "LOGQUERY-S3CR3T-mno",
        owner_token.trim(),
    ] {
        assert!(!text.contains(marker), "{marker} leaked from the log or the token file");
    }
    // No content.
    for private in ["TITOLO-PRIVATO-DELLA-CHAT", "TESTO-PRIVATO-DELLA-CHAT", "io@example.org", "imap.example.org"] {
        assert!(!text.contains(private), "{private} is content or an account, not a diagnostic");
    }
    if home.len() > 3 {
        assert!(!text.contains(&home), "the home folder names the person");
    }

    // What is useful is there.
    assert_eq!(doc["format"], 1);
    assert_eq!(doc["app"]["version"], aiw_core::VERSION);
    assert_eq!(doc["system"]["os"], std::env::consts::OS);
    assert_eq!(doc["system"]["arch"], std::env::consts::ARCH);
    assert!(doc["system"]["device"]["cpu"]["cores"].is_number() && doc["system"]["device"]["memory_mb"].is_number());
    assert!(doc["runtimes"].as_array().is_some_and(|r| !r.is_empty()) && doc["runtimes"][0]["status"].is_string());
    assert_eq!(doc["database"]["schema_version"], crate::store::SCHEMA_VERSION);
    assert_eq!(doc["database"]["expected_schema_version"], crate::store::SCHEMA_VERSION);
    assert_eq!(doc["database"]["table_counts"]["messages"], 1);
    assert!(doc["database"]["table_counts"]["api_tokens"].as_u64().unwrap() >= 1);
    assert_eq!(doc["settings"]["effective"]["search.brave_api_key"], SECRET_MASK, "the existing masking is what the settings go through");
    assert!(
        doc["health"]["update"]["channel"].is_string()
            && doc["health"]["tls"]["mode"].is_string()
            && doc["health"]["training"]["can_train"].is_array()
    );
    let lines: Vec<&str> = doc["logs"]["lines"].as_array().unwrap().iter().filter_map(|l| l.as_str()).collect();
    assert_eq!(doc["logs"]["file"], "aiw.log");
    assert_eq!(lines[0], "avvio normale");
    assert!(lines.iter().any(|l| l.contains("Authorization") && l.contains("••••••")), "{lines:?}");
    assert!(
        lines.iter().any(|l| l.contains("https://") && l.contains("host.example/api")),
        "the line is kept, the credentials are not: {lines:?}"
    );
    // The settings of this machine are there too (a value that is not a secret is shown as it is).
    assert_eq!(doc["settings"]["effective"]["update.channel"], "auto");
}

#[tokio::test]
async fn diagnostics_work_with_no_logs_and_a_new_installation() {
    let (e, _d, _w) = engine(vec![]).await;
    let doc = e.diagnostics().await.unwrap();
    assert!(doc["logs"].is_null());
    assert!(doc["database"]["table_counts"]["documents"].is_number());
}
