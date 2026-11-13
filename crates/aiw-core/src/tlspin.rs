//! Certificate pinning for the TLS connections to the devices of the person (paired computers).
//!
//! A paired device on the LAN serves HTTPS with a certificate that no public authority signed, so
//! the system roots cannot vouch for it. The pairing hands over the SHA-256 fingerprint of the
//! device's certificate (the QR code, or the value the person compared on both screens); from then
//! on every client that talks to that address trusts **that and nothing else**:
//!
//! * the pin is the hash of the certificate that was served as the *leaf* (a certificate the person
//!   supplied themselves), or of a certificate that is in the served chain and issued the leaf (the
//!   local CA of `self_signed`: the leaf can then be renewed without pairing again);
//! * with a CA pin the leaf is still verified against it (signature, validity, name), so knowing
//!   the public CA certificate is not enough to impersonate the device;
//! * the system roots are never consulted, and no certificate is accepted "because it is there".
//!
//! Clients ask [`client_config_for`] for the address they connect to: `Some(config)` when that
//! address was registered with [`register`] (paired devices are registered when the engine starts
//! and when they are paired), `None` for every other address, which keep the normal verification.

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DistinguishedName, Error as TlsError, RootCertStore, SignatureScheme};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

/// Lowercase hex SHA-256 of a DER certificate (64 characters).
pub fn fingerprint_hex(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect()
}

/// Accepts `AB:CD:…`, `ab cd …` or plain hex and returns the lowercase 64-character form.
pub fn normalize_fingerprint(s: &str) -> Option<String> {
    let hex: String = s.chars().filter(|c| !matches!(c, ':' | ' ' | '-')).collect::<String>().to_ascii_lowercase();
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())).then_some(hex)
}

/// `AB:CD:…` for display.
pub fn display_fingerprint(hex: &str) -> String {
    hex.as_bytes().chunks(2).map(|c| String::from_utf8_lossy(c).to_ascii_uppercase()).collect::<Vec<_>>().join(":")
}

fn registry() -> &'static RwLock<HashMap<String, String>> {
    static R: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

/// `host:port` of an `https://` URL (port 443 when absent), lowercase.
fn authority(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("HTTPS://"))?;
    let auth = rest.split(['/', '?', '#']).next()?;
    if auth.is_empty() || auth.contains('@') {
        return None;
    }
    let has_port = match auth.strip_prefix('[') {
        Some(v6) => v6.split_once(']').is_some_and(|(_, tail)| tail.starts_with(':')),
        None => auth.contains(':'),
    };
    Some(if has_port { auth.to_ascii_lowercase() } else { format!("{}:443", auth.to_ascii_lowercase()) })
}

/// Pins the address of `url` (`https://host:port/…`) to a certificate fingerprint. Replaces an older pin.
pub fn register(url: &str, fingerprint: &str) {
    if let (Some(a), Some(fp)) = (authority(url), normalize_fingerprint(fingerprint)) {
        registry().write().unwrap().insert(a, fp);
    }
}

/// Forgets the pin of an address.
pub fn unregister(url: &str) {
    if let Some(a) = authority(url) {
        registry().write().unwrap().remove(&a);
    }
}

/// The pin registered for the address of `url`.
pub fn pin_for(url: &str) -> Option<String> {
    registry().read().unwrap().get(&authority(url)?).cloned()
}

/// TLS configuration for a connection to `url`: pinned when the address is registered, `None`
/// otherwise (an `http://` URL, or an address nobody paired with: the caller keeps its default).
pub fn client_config_for(url: &str) -> Option<ClientConfig> {
    pinned_client_config(&pin_for(url)?).ok()
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A client configuration that trusts only the certificate (or the CA) with this fingerprint.
pub fn pinned_client_config(fingerprint: &str) -> Result<ClientConfig, String> {
    let pin = normalize_fingerprint(fingerprint).ok_or("impronta del certificato non valida (servono 64 cifre esadecimali)")?;
    let provider = provider();
    let verifier = Arc::new(PinVerifier { pin, provider: provider.clone() });
    let cfg = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(cfg)
}

#[derive(Debug)]
struct PinVerifier {
    pin: String,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        // The exact certificate the person pinned (their own certificate): the handshake signature
        // proves the server holds its key.
        if fingerprint_hex(end_entity) == self.pin {
            return Ok(ServerCertVerified::assertion());
        }
        // Otherwise the pin must be a certificate of the served chain that signed the leaf.
        let Some(ca) = intermediates.iter().find(|c| fingerprint_hex(c) == self.pin) else {
            return Err(TlsError::General("il certificato del dispositivo non corrisponde all'impronta salvata con il pairing".into()));
        };
        let mut roots = RootCertStore::empty();
        roots.add(ca.clone().into_owned())?;
        let others: Vec<CertificateDer<'_>> = intermediates.iter().filter(|c| fingerprint_hex(c) != self.pin).cloned().collect();
        let inner = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), self.provider.clone())
            .build()
            .map_err(|e| TlsError::General(e.to_string()))?;
        inner.verify_server_cert(end_entity, &others, server_name, ocsp, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }

    fn root_hint_subjects(&self) -> Option<&[DistinguishedName]> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_are_normalised() {
        let hex = "ab".repeat(32);
        assert_eq!(normalize_fingerprint(&hex.to_uppercase()).as_deref(), Some(hex.as_str()));
        let colons = display_fingerprint(&hex);
        assert!(colons.starts_with("AB:AB:") && colons.len() == 95);
        assert_eq!(normalize_fingerprint(&colons).as_deref(), Some(hex.as_str()));
        assert_eq!(normalize_fingerprint("abc"), None);
        assert_eq!(normalize_fingerprint(&"zz".repeat(32)), None);
    }

    #[test]
    fn authorities_and_registry() {
        assert_eq!(authority("https://Pc.local:7777/v1").as_deref(), Some("pc.local:7777"));
        assert_eq!(authority("https://pc.local").as_deref(), Some("pc.local:443"));
        assert_eq!(authority("https://[::1]:8443/x").as_deref(), Some("[::1]:8443"));
        assert_eq!(authority("http://pc.local:7777"), None);
        assert_eq!(authority("https://u@pc.local"), None);
        let fp = "cd".repeat(32);
        assert!(client_config_for("https://pin-test.local:1").is_none());
        register("https://pin-test.local:1", &fp);
        assert_eq!(pin_for("https://pin-test.local:1/api").as_deref(), Some(fp.as_str()));
        assert!(client_config_for("https://pin-test.local:1/x").is_some());
        assert!(client_config_for("https://pin-test.local:2").is_none(), "another port is another device");
        unregister("https://pin-test.local:1");
        assert!(pin_for("https://pin-test.local:1").is_none());
    }
}
