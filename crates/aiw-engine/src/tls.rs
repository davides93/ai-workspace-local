//! TLS for the listener that is reachable from the LAN.
//!
//! `server.tls` is `off` (plain HTTP, the default), `self_signed` (AIW makes a small local
//! authority and a server certificate signed by it) or `files` (the person's own certificate and
//! key). With `self_signed` the pairing hands the device the SHA-256 fingerprint of the **authority**
//! certificate, so the leaf can be renewed (new LAN address, expiry) without pairing again; with
//! `files` it is the fingerprint of the served certificate. See `aiw_core::tlspin` for how a client
//! uses it.
//!
//! The files live in `<data>/tls/`: `ca.key` and `server.key` (owner-only permissions on Unix),
//! `ca.pem`, `server.pem` (leaf followed by the authority) and `meta.json`. Nothing in this folder
//! is part of any export the app makes (there is no backup or diagnostics bundle that reads the
//! data folder; the data export and trace export only write what they select).

use crate::Engine;
use aiw_core::api::TlsStatus;
use aiw_core::tlspin::{display_fingerprint, fingerprint_hex};
use aiw_core::{Error, Result};
use rcgen::{BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

/// The leaf is renewed when it has less than this left.
const RENEW_BEFORE_DAYS: i64 = 30;
const LEAF_DAYS: i64 = 397;
const CA_DAYS: i64 = 3650;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    Off,
    SelfSigned,
    Files,
}

impl TlsMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "off" | "" => Some(TlsMode::Off),
            "self_signed" => Some(TlsMode::SelfSigned),
            "files" => Some(TlsMode::Files),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            TlsMode::Off => "off",
            TlsMode::SelfSigned => "self_signed",
            TlsMode::Files => "files",
        }
    }
}

/// What the server needs to serve TLS, and what pairing hands out.
#[derive(Debug, Clone)]
pub struct TlsMaterial {
    /// Leaf first, then the rest of the chain.
    pub chain_pem: String,
    pub key_pem: String,
    /// The local authority (`self_signed` only).
    pub ca_pem: Option<String>,
    /// Lowercase hex SHA-256 a device pins.
    pub fingerprint: String,
    /// `ca` | `certificate`.
    pub fingerprint_of: &'static str,
    pub names: Vec<String>,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Meta {
    ca_cn: String,
    ca_not_before: String,
    ca_not_after: String,
    ca_fingerprint: String,
    #[serde(default)]
    leaf_sans: Vec<String>,
    #[serde(default)]
    leaf_not_after: String,
}

use chrono::{DateTime, Datelike, Utc};

/// A date for a certificate (day precision).
macro_rules! cert_date {
    ($d:expr) => {{
        let d: DateTime<Utc> = $d;
        rcgen::date_time_ymd(d.year(), d.month() as u8, d.day() as u8)
    }};
}

fn tls_err(what: &str, e: impl std::fmt::Display) -> Error {
    Error::Runtime(format!("TLS: {what}: {e}"))
}

/// Writes a file readable by the owner only (on Unix the mode is set when the file is created, so
/// there is no moment in which a private key is readable by others).
fn write_private(path: &Path, content: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        f.write_all(content.as_bytes())?;
        // A file that already existed keeps its old mode through `open`: set it too.
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    std::fs::write(path, content)?;
    Ok(())
}

/// A host name made of letters, digits and dashes (the mDNS label of the device).
pub fn host_label(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "aiw".into()
    } else {
        s.chars().take(40).collect()
    }
}

/// The names and addresses the certificate must be valid for: the host name, `localhost`, the
/// loopback addresses and the addresses of the LAN interfaces. Sorted and without repeats.
pub fn certificate_names(device_name: &str, ips: &[IpAddr]) -> Vec<String> {
    let label = host_label(device_name);
    let mut names = vec![label.clone(), format!("{label}.local"), "localhost".to_string(), "127.0.0.1".into(), "::1".into()];
    names.extend(ips.iter().map(|ip| ip.to_string()));
    names.sort();
    names.dedup();
    names
}

fn sans(names: &[String]) -> Result<Vec<SanType>> {
    names
        .iter()
        .map(|n| match IpAddr::from_str(n) {
            Ok(ip) => Ok(SanType::IpAddress(ip)),
            Err(_) => Ok(SanType::DnsName(n.clone().try_into().map_err(|e| tls_err("nome non valido per il certificato", e))?)),
        })
        .collect()
}

fn ca_params(meta: &Meta) -> Result<CertificateParams> {
    let parse = |s: &str| DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)).map_err(|e| tls_err("date del certificato", e));
    let mut p = CertificateParams::default();
    p.distinguished_name.push(DnType::CommonName, meta.ca_cn.clone());
    p.distinguished_name.push(DnType::OrganizationName, "AI Workspace Local");
    p.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    p.not_before = cert_date!(parse(&meta.ca_not_before)?);
    p.not_after = cert_date!(parse(&meta.ca_not_after)?);
    Ok(p)
}

/// Creates (once) the local authority and creates or renews the server certificate when it is
/// missing, does not cover `names` or expires within 30 days. `now` is a parameter for the tests.
pub fn ensure_self_signed(dir: &Path, names: &[String], now: DateTime<Utc>) -> Result<TlsMaterial> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let (ca_key_f, ca_pem_f, leaf_key_f, leaf_pem_f, meta_f) =
        (dir.join("ca.key"), dir.join("ca.pem"), dir.join("server.key"), dir.join("server.pem"), dir.join("meta.json"));

    // 1. The authority.
    let existing: Option<(Meta, String, String)> = (|| {
        let meta: Meta = serde_json::from_slice(&std::fs::read(&meta_f).ok()?).ok()?;
        let (key, pem) = (std::fs::read_to_string(&ca_key_f).ok()?, std::fs::read_to_string(&ca_pem_f).ok()?);
        KeyPair::from_pem(&key).ok()?;
        let der = CertificateDer::from_pem_slice(pem.as_bytes()).ok()?;
        (fingerprint_hex(&der) == meta.ca_fingerprint).then_some((meta, key, pem))
    })();
    let (mut meta, ca_key_pem, ca_pem) = match existing {
        Some(x) => x,
        None => {
            let key = KeyPair::generate().map_err(|e| tls_err("chiave dell'autorità", e))?;
            let mut meta = Meta {
                ca_cn: format!("AIW Local CA {}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
                ca_not_before: (now - chrono::Duration::days(1)).to_rfc3339(),
                ca_not_after: (now + chrono::Duration::days(CA_DAYS)).to_rfc3339(),
                ca_fingerprint: String::new(),
                leaf_sans: vec![],
                leaf_not_after: String::new(),
            };
            let cert = ca_params(&meta)?.self_signed(&key).map_err(|e| tls_err("certificato dell'autorità", e))?;
            meta.ca_fingerprint = fingerprint_hex(cert.der());
            write_private(&ca_key_f, &key.serialize_pem())?;
            std::fs::write(&ca_pem_f, cert.pem())?;
            // A new authority invalidates the leaf signed by the old one.
            let _ = std::fs::remove_file(&leaf_pem_f);
            (meta, key.serialize_pem(), cert.pem())
        }
    };
    let ca_key = KeyPair::from_pem(&ca_key_pem).map_err(|e| tls_err("chiave dell'autorità", e))?;
    // The issuer is rebuilt from the stored parameters: only its name and key matter for signing; the
    // certificate that devices pin and trust is the one stored in `ca.pem`.
    let issuer = ca_params(&meta)?.self_signed(&ca_key).map_err(|e| tls_err("autorità", e))?;

    // 2. The server certificate.
    let mut wanted: Vec<String> = names.to_vec();
    wanted.sort();
    wanted.dedup();
    let leaf_expiry = DateTime::parse_from_rfc3339(&meta.leaf_not_after).ok().map(|d| d.with_timezone(&Utc));
    let renew = !leaf_pem_f.exists()
        || !leaf_key_f.exists()
        || meta.leaf_sans != wanted
        || leaf_expiry.is_none_or(|e| e - now < chrono::Duration::days(RENEW_BEFORE_DAYS));
    if renew {
        let key = KeyPair::generate().map_err(|e| tls_err("chiave del server", e))?;
        let ca_end = DateTime::parse_from_rfc3339(&meta.ca_not_after).map(|d| d.with_timezone(&Utc)).unwrap_or(now);
        let not_after = (now + chrono::Duration::days(LEAF_DAYS)).min(ca_end);
        let mut p = CertificateParams::default();
        p.distinguished_name.push(DnType::CommonName, wanted.first().cloned().unwrap_or_else(|| "localhost".into()));
        p.distinguished_name.push(DnType::OrganizationName, "AI Workspace Local");
        p.subject_alt_names = sans(&wanted)?;
        p.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        p.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        p.not_before = cert_date!(now - chrono::Duration::days(1));
        p.not_after = cert_date!(not_after);
        let leaf = p.signed_by(&key, &issuer, &ca_key).map_err(|e| tls_err("certificato del server", e))?;
        write_private(&leaf_key_f, &key.serialize_pem())?;
        std::fs::write(&leaf_pem_f, format!("{}{}", leaf.pem(), ca_pem))?;
        meta.leaf_sans = wanted;
        meta.leaf_not_after = not_after.to_rfc3339();
    }
    std::fs::write(&meta_f, serde_json::to_vec_pretty(&meta)?)?;
    Ok(TlsMaterial {
        chain_pem: std::fs::read_to_string(&leaf_pem_f)?,
        key_pem: std::fs::read_to_string(&leaf_key_f)?,
        ca_pem: Some(ca_pem),
        fingerprint: meta.ca_fingerprint,
        fingerprint_of: "ca",
        names: meta.leaf_sans,
        expires_at: meta.leaf_not_after,
    })
}

/// The person's own certificate and key (`server.tls = files`).
pub fn load_files(cert_file: &str, key_file: &str) -> Result<TlsMaterial> {
    if cert_file.trim().is_empty() || key_file.trim().is_empty() {
        return Err(Error::Invalid("con server.tls = files indica server.tls.cert_file e server.tls.key_file".into()));
    }
    let read = |p: &str, what: &str| {
        std::fs::read_to_string(aiw_core::paths::expand_home(p)).map_err(|e| Error::Invalid(format!("{what} non leggibile ({p}): {e}")))
    };
    let (chain_pem, key_pem) = (read(cert_file, "certificato")?, read(key_file, "chiave")?);
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(chain_pem.as_bytes())
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| Error::Invalid(format!("certificato PEM non valido: {e}")))?;
    let leaf = chain.first().ok_or_else(|| Error::Invalid("il file del certificato non contiene certificati".into()))?;
    PrivateKeyDer::from_pem_slice(key_pem.as_bytes()).map_err(|e| Error::Invalid(format!("chiave PEM non valida: {e}")))?;
    let material = TlsMaterial {
        fingerprint: fingerprint_hex(leaf),
        fingerprint_of: "certificate",
        names: vec![],
        expires_at: String::new(),
        ca_pem: None,
        chain_pem,
        key_pem,
    };
    // The key must belong to the certificate: building the configuration checks it.
    server_config(&material)?;
    Ok(material)
}

/// The rustls configuration of the listener.
pub fn server_config(m: &TlsMaterial) -> Result<rustls::ServerConfig> {
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(m.chain_pem.as_bytes())
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| tls_err("catena di certificati", e))?;
    let key = PrivateKeyDer::from_pem_slice(m.key_pem.as_bytes()).map_err(|e| tls_err("chiave privata", e))?;
    let mut cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| tls_err("protocolli", e))?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|e| Error::Invalid(format!("la chiave privata non corrisponde al certificato: {e}")))?;
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(cfg)
}

/// How the listener of this process is set up (written by the server when it starts).
#[derive(Default)]
pub(crate) struct NetState {
    inner: Mutex<NetInner>,
}

#[derive(Default, Clone)]
struct NetInner {
    lan: bool,
    port: u16,
    active: bool,
    material: Option<(String, &'static str, Vec<String>, String)>,
    error: Option<String>,
}

impl Engine {
    fn config_str(&self, key: &str) -> String {
        self.config().effective.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string()
    }

    pub fn tls_mode(&self) -> TlsMode {
        TlsMode::parse(&self.config_str("server.tls")).unwrap_or(TlsMode::Off)
    }

    fn tls_dir(&self) -> PathBuf {
        self.0.dirs.root.join("tls")
    }

    /// The material for the configured mode (`None` for `off`). `self_signed` makes or renews the
    /// certificates; any problem is an error the caller shows (and the listener does not start in the clear).
    pub fn tls_material(&self) -> Result<Option<TlsMaterial>> {
        match self.tls_mode() {
            TlsMode::Off => Ok(None),
            TlsMode::SelfSigned => {
                let device = self.device();
                let names = certificate_names(&device.name, &aiw_hw::local_ips());
                Ok(Some(ensure_self_signed(&self.tls_dir(), &names, Utc::now())?))
            }
            TlsMode::Files => Ok(Some(load_files(&self.config_str("server.tls.cert_file"), &self.config_str("server.tls.key_file"))?)),
        }
    }

    /// Called by the server when it starts: what it listens on and whether it speaks TLS.
    pub fn tls_listener_started(&self, lan: bool, port: u16, material: Option<&TlsMaterial>) {
        let mut n = self.0.net.inner.lock().unwrap();
        *n = NetInner {
            lan,
            port,
            active: material.is_some(),
            material: material.map(|m| (m.fingerprint.clone(), m.fingerprint_of, m.names.clone(), m.expires_at.clone())),
            error: None,
        };
    }

    pub fn tls_listener_failed(&self, lan: bool, port: u16, error: String) {
        *self.0.net.inner.lock().unwrap() = NetInner { lan, port, active: false, material: None, error: Some(error) };
    }

    /// Is the listener of this process serving TLS?
    pub fn tls_active(&self) -> bool {
        self.0.net.inner.lock().unwrap().active
    }

    /// Fingerprint (lowercase hex) that a pairing client pins, when the listener serves TLS.
    pub fn tls_fingerprint(&self) -> Option<String> {
        let n = self.0.net.inner.lock().unwrap();
        n.active.then(|| n.material.as_ref().map(|m| m.0.clone())).flatten()
    }

    /// The authority certificate, to install in a browser (`self_signed` only).
    pub fn tls_ca_pem(&self) -> Option<String> {
        std::fs::read_to_string(self.tls_dir().join("ca.pem")).ok().filter(|_| self.tls_mode() == TlsMode::SelfSigned)
    }

    pub fn tls_status(&self) -> TlsStatus {
        let n = self.0.net.inner.lock().unwrap().clone();
        let mode = self.tls_mode();
        let cfg = self.config().effective;
        let lan = if n.port != 0 || n.active { n.lan } else { cfg.get("server.lan").and_then(|v| v.as_bool()).unwrap_or(false) };
        let port = if n.port != 0 { n.port } else { cfg.get("server.port").and_then(|v| v.as_u64()).unwrap_or(7777) as u16 };
        // Not serving in this process (the desktop app over IPC, a CLI command): show what is on disk.
        let from_disk = (|| {
            let meta: Meta = serde_json::from_slice(&std::fs::read(self.tls_dir().join("meta.json")).ok()?).ok()?;
            Some((meta.ca_fingerprint, "ca", meta.leaf_sans, meta.leaf_not_after))
        })();
        let material = n.material.clone().or_else(|| from_disk.filter(|_| mode == TlsMode::SelfSigned));
        let (fingerprint, of, names, expires) = match material {
            Some((f, of, names, exp)) => (Some(display_fingerprint(&f)), Some(of.to_string()), names, Some(exp).filter(|e| !e.is_empty())),
            None => (None, None, vec![], None),
        };
        TlsStatus {
            mode: mode.as_str().into(),
            active: n.active,
            lan,
            port,
            fingerprint,
            fingerprint_of: of,
            names,
            expires_at: expires,
            error: n.error,
            ca_available: mode == TlsMode::SelfSigned && self.tls_dir().join("ca.pem").exists(),
            warning: lan && !n.active,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn names_cover_host_localhost_and_the_lan() {
        let n = certificate_names("Studio PC (2)", &ips(&["192.168.1.20", "192.168.1.20", "fd00::5"]));
        for want in ["studio-pc--2", "studio-pc--2.local", "localhost", "127.0.0.1", "::1", "192.168.1.20", "fd00::5"] {
            assert!(n.contains(&want.to_string()), "{want} in {n:?}");
        }
        assert_eq!(n.iter().filter(|x| *x == "192.168.1.20").count(), 1);
    }

    #[test]
    fn modes_parse() {
        assert_eq!(TlsMode::parse("self_signed"), Some(TlsMode::SelfSigned));
        assert_eq!(TlsMode::parse(""), Some(TlsMode::Off));
        assert_eq!(TlsMode::parse("https"), None);
    }
}
