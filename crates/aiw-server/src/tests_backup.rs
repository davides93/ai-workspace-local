//! Backup and diagnostics over the real router: owner only, no secrets by default, nothing that restores.

use super::tests_security::{fixture, req, send, LAN};
use super::*;
use base64::Engine as _;

fn bearer(t: &str) -> String {
    format!("Bearer {t}")
}

async fn as_token(app: &Router, method: &str, uri: &str, token: &str, body: Option<Value>) -> (StatusCode, Value) {
    let b = bearer(token);
    let (st, _, v) = send(app, req(method, uri, Some(LAN), &[("host", "192.168.1.5:7777"), ("authorization", b.as_str())], body)).await;
    (st, v)
}

#[tokio::test]
async fn a_device_token_is_refused_by_every_backup_and_diagnostics_route_and_the_owner_is_not() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    let device = e.create_api_token("Telefono", "device", None, None, None).unwrap().token;
    let gateway = e.create_api_token("App", "gateway", None, None, None).unwrap().token;
    for (m, p, body) in [
        ("POST", "/api/backup", Some(json!({"include_secrets": true}))),
        ("POST", "/api/backup/inspect", Some(json!({"data": ""}))),
        ("GET", "/api/diagnostics", None),
    ] {
        for who in [&device, &gateway] {
            let (st, v) = as_token(&app, m, p, who, body.clone()).await;
            assert_eq!(st, StatusCode::FORBIDDEN, "{m} {p}: {v}");
            assert!(v["error"].as_str().unwrap().contains("accesso completo"), "{v}");
        }
        // The owner of this computer (over loopback, no token) is let through.
        let (st, _) = crate::dispatch(app.clone(), m, p, body.clone()).await;
        assert_ne!(st, 403, "{m} {p}");
    }
    // There is no restore route at all.
    for (m, p) in [("POST", "/api/backup/restore"), ("PUT", "/api/backup"), ("POST", "/api/restore")] {
        let (st, _) = crate::dispatch(app.clone(), m, p, Some(json!({}))).await;
        assert!(st == 404 || st == 405, "{m} {p}: {st}");
    }
}

#[tokio::test]
async fn the_backup_is_created_downloaded_and_inspected_through_the_api() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    e.set_config("global", "search.brave_api_key", json!("BRAVEKEY-API-SECRET-1")).unwrap();
    e.set_config("global", "ui.language", json!("en")).unwrap();
    let (st, v) = crate::dispatch(app.clone(), "POST", "/api/backup", Some(json!({"include_secrets": false}))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["sensitive"], false);
    assert!(v["file_name"].as_str().unwrap().starts_with("aiw-backup-") && v["file_name"].as_str().unwrap().ends_with(".tar.gz"));
    assert!(!v["file_name"].as_str().unwrap().contains("segreti"));
    let bytes = base64::engine::general_purpose::STANDARD.decode(v["data"].as_str().unwrap()).unwrap();
    assert_eq!(bytes.len() as u64, v["size"].as_u64().unwrap());
    assert_eq!(&bytes[..2], &[0x1f, 0x8b], "a gzip file");
    let paths: Vec<&str> = v["manifest"]["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert!(paths.contains(&"aiw.db") && paths.contains(&"config.json"), "{paths:?}");
    assert!(!paths.contains(&"token") && !paths.iter().any(|p| p.starts_with("tls/") || p.starts_with("models/")), "{paths:?}");
    assert!(v["manifest"]["excluded"].as_array().unwrap().iter().any(|x| x.as_str().unwrap().contains("modelli")));

    // The same bytes, sent back, are verified.
    let (st, r) = crate::dispatch(app.clone(), "POST", "/api/backup/inspect", Some(json!({"data": v["data"]}))).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!((r["verified"].as_bool(), r["restorable_here"].as_bool()), (Some(true), Some(true)));
    assert_eq!(r["manifest"], v["manifest"]);
    assert_eq!(r["current_schema_version"], aiw_engine::store::SCHEMA_VERSION);
    // Damaged or foreign data are refused with the usual error.
    let mut bad = bytes.clone();
    let n = bad.len();
    bad[n / 2] ^= 0xFF;
    for data in [
        base64::engine::general_purpose::STANDARD.encode(&bad),
        base64::engine::general_purpose::STANDARD.encode(b"non sono un archivio"),
        "@@@".into(),
    ] {
        let (st, r) = crate::dispatch(app.clone(), "POST", "/api/backup/inspect", Some(json!({"data": data}))).await;
        assert_eq!(st, 400, "{r}");
        assert!(r["error"].as_str().is_some_and(|m| !m.is_empty()));
    }

    // With secrets: flagged in the response, the manifest and the file name.
    let (st, s) = crate::dispatch(app.clone(), "POST", "/api/backup", Some(json!({"include_secrets": true}))).await;
    assert_eq!(st, 200, "{s}");
    assert_eq!((s["sensitive"].as_bool(), s["manifest"]["sensitive"].as_bool()), (Some(true), Some(true)));
    assert!(s["file_name"].as_str().unwrap().contains("-con-segreti"));
    assert!(s["manifest"]["files"].as_array().unwrap().iter().any(|f| f["path"] == "token"));
    // No scratch file is left in the data folder.
    let left: Vec<String> =
        std::fs::read_dir(&e.data_dirs().root).unwrap().map(|x| x.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(!left.iter().any(|n| n.starts_with(".backup-")), "{left:?}");
}

#[tokio::test]
async fn diagnostics_over_the_api_mask_the_secrets() {
    let (app, e, _d, _w) = fixture(vec![]).await;
    e.set_config("global", "search.brave_api_key", json!("BRAVEKEY-API-SECRET-2")).unwrap();
    let (st, v) = crate::dispatch(app.clone(), "GET", "/api/diagnostics", None).await;
    assert_eq!(st, 200, "{v}");
    assert!(!v.to_string().contains("BRAVEKEY-API-SECRET-2") && !v.to_string().contains(e.token()));
    assert_eq!(v["settings"]["effective"]["search.brave_api_key"], aiw_core::api::SECRET_MASK);
    assert_eq!(v["app"]["version"], aiw_core::VERSION);
}
