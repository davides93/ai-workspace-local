//! Legacy token migration, the self-signed certificates and the pinned TLS client.

use super::*;
use crate::tls::{certificate_names, ensure_self_signed, server_config};
use crate::tokens::{TokenCheck, TokenKind};
use aiw_core::tlspin::{fingerprint_hex, pinned_client_config};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use std::net::IpAddr;

async fn engine_at(dir: &std::path::Path) -> Engine {
    let ws = dir.join("ws");
    Engine::new(EngineOptions {
        data_dir: Some(dir.join("data")),
        workspace_root: Some(ws),
        offline: true,
        mode: "daemon".into(),
        ..Default::default()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn the_legacy_single_token_becomes_the_owner_token() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("data")).unwrap();
    let legacy = "0123456789abcdef0123456789abcdef01234567";
    std::fs::write(d.path().join("data").join("token"), legacy).unwrap();
    let e = engine_at(d.path()).await;
    assert_eq!(e.token(), legacy, "the file is still the source: CLI and desktop keep working");
    assert!(matches!(e.authenticate_token(legacy, None), TokenCheck::Valid(a) if a.kind == TokenKind::Owner));
    let owners: Vec<_> = e.api_tokens().into_iter().filter(|t| t.kind == "owner").collect();
    assert_eq!(owners.len(), 1);
    assert!(!e.api_tokens_at_rest().contains(legacy));
    drop(e);
    // A second start does not add a row; a replaced token file retires the old owner row.
    let e = engine_at(d.path()).await;
    assert_eq!(e.api_tokens().iter().filter(|t| t.kind == "owner").count(), 1);
    drop(e);
    std::fs::write(d.path().join("data").join("token"), "fedcba9876543210fedcba9876543210fedcba98").unwrap();
    let e = engine_at(d.path()).await;
    let owners: Vec<_> = e.api_tokens().into_iter().filter(|t| t.kind == "owner").collect();
    assert_eq!(owners.iter().filter(|t| t.revoked_at.is_none()).count(), 1);
    assert_eq!(owners.len(), 2);
    assert!(matches!(e.authenticate_token(legacy, None), TokenCheck::Unknown));
}

fn ips(l: &[&str]) -> Vec<IpAddr> {
    l.iter().map(|s| s.parse().unwrap()).collect()
}

#[test]
fn self_signed_files_have_private_keys_and_follow_the_addresses() {
    let d = tempfile::tempdir().unwrap();
    let tls = d.path().join("tls");
    let now = chrono::Utc::now();
    let names = certificate_names("Studio PC", &ips(&["192.168.1.20"]));
    let a = ensure_self_signed(&tls, &names, now).unwrap();
    assert_eq!(a.fingerprint_of, "ca");
    assert!(a.names.contains(&"192.168.1.20".to_string()) && a.names.contains(&"localhost".to_string()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for f in ["ca.key", "server.key"] {
            assert_eq!(std::fs::metadata(tls.join(f)).unwrap().permissions().mode() & 0o777, 0o600, "{f}");
        }
    }
    // Same names, same material: nothing is regenerated.
    let again = ensure_self_signed(&tls, &names, now).unwrap();
    assert_eq!((&again.chain_pem, &again.fingerprint), (&a.chain_pem, &a.fingerprint));
    // A new LAN address: the leaf is regenerated, the authority (what devices pin) is not.
    let moved = ensure_self_signed(&tls, &certificate_names("Studio PC", &ips(&["10.0.0.7"])), now).unwrap();
    assert_ne!(moved.chain_pem, a.chain_pem);
    assert_eq!(moved.fingerprint, a.fingerprint);
    assert!(moved.names.contains(&"10.0.0.7".to_string()) && !moved.names.contains(&"192.168.1.20".to_string()));
    // Near expiry: renewed too.
    let later = ensure_self_signed(&tls, &certificate_names("Studio PC", &ips(&["10.0.0.7"])), now + chrono::Duration::days(380)).unwrap();
    assert_ne!(later.chain_pem, moved.chain_pem);
    assert_eq!(later.fingerprint, a.fingerprint);
}

/// One TLS handshake in memory: the client connects to `name` with `client`, the server serves `server`.
async fn handshake(server: rustls::ServerConfig, client: rustls::ClientConfig, name: &str) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (c, s) = tokio::io::duplex(16384);
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
    let srv = tokio::spawn(async move {
        if let Ok(mut t) = acceptor.accept(s).await {
            let mut b = [0u8; 4];
            let _ = t.read_exact(&mut b).await;
            let _ = t.write_all(b"pong").await;
            let _ = t.flush().await;
        }
    });
    let connector = tokio_rustls::TlsConnector::from(Arc::new(client));
    let ok = match connector.connect(ServerName::try_from(name.to_string()).unwrap(), c).await {
        Ok(mut t) => {
            t.write_all(b"ping").await.is_ok() && {
                let mut b = [0u8; 4];
                t.read_exact(&mut b).await.is_ok() && &b == b"pong"
            }
        }
        Err(_) => false,
    };
    let _ = srv.await;
    ok
}

#[tokio::test]
async fn the_pinned_client_trusts_the_pin_and_nothing_else() {
    let d = tempfile::tempdir().unwrap();
    let names = certificate_names("Studio PC", &ips(&["192.168.1.20"]));
    let mine = ensure_self_signed(&d.path().join("a"), &names, chrono::Utc::now()).unwrap();
    let other = ensure_self_signed(&d.path().join("b"), &names, chrono::Utc::now()).unwrap();
    let srv = || server_config(&mine).unwrap();

    // Right pin (the authority): connects, by name and by address; a name outside the certificate does not.
    let ok = || pinned_client_config(&mine.fingerprint).unwrap();
    assert!(handshake(srv(), ok(), "localhost").await);
    assert!(handshake(srv(), ok(), "192.168.1.20").await);
    assert!(handshake(srv(), ok(), "studio-pc.local").await);
    assert!(!handshake(srv(), ok(), "altro.example").await, "the names of the certificate are checked");
    // Wrong pin: refused.
    assert!(!handshake(srv(), pinned_client_config(&other.fingerprint).unwrap(), "localhost").await);
    assert!(!handshake(srv(), pinned_client_config(&"00".repeat(32)).unwrap(), "localhost").await);
    // Pinning the exact leaf (a certificate of the person's own) works too.
    let leaf = CertificateDer::from_pem_slice(mine.chain_pem.as_bytes()).unwrap();
    assert!(handshake(srv(), pinned_client_config(&fingerprint_hex(&leaf)).unwrap(), "localhost").await);

    // A certificate the *system* would trust, but that is not the pinned one: refused by a pinned client,
    // accepted by a client that uses roots (so the failure is the pin, not a broken fixture).
    let ca_der = CertificateDer::from_pem_slice(other.ca_pem.as_ref().unwrap().as_bytes()).unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca_der).unwrap();
    let system_like = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    assert!(handshake(server_config(&other).unwrap(), system_like, "localhost").await, "trusted by the roots");
    assert!(!handshake(server_config(&other).unwrap(), ok(), "localhost").await, "not what was pinned");
}

#[test]
fn a_key_that_does_not_belong_to_the_certificate_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let names = certificate_names("x", &[]);
    let a = ensure_self_signed(&d.path().join("a"), &names, chrono::Utc::now()).unwrap();
    let b = ensure_self_signed(&d.path().join("b"), &names, chrono::Utc::now()).unwrap();
    std::fs::write(d.path().join("c.pem"), &a.chain_pem).unwrap();
    std::fs::write(d.path().join("k.pem"), &b.key_pem).unwrap();
    let err = crate::tls::load_files(d.path().join("c.pem").to_str().unwrap(), d.path().join("k.pem").to_str().unwrap());
    assert!(err.is_err());
    std::fs::write(d.path().join("k.pem"), &a.key_pem).unwrap();
    let ok = crate::tls::load_files(d.path().join("c.pem").to_str().unwrap(), d.path().join("k.pem").to_str().unwrap()).unwrap();
    assert_eq!(ok.fingerprint_of, "certificate");
}
