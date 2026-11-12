//! TLS for the mail clients (IMAP, SMTP): rustls with the `ring` provider, the web PKI roots plus the
//! ones of the operating system. There is no way to switch the verification off: a certificate that
//! does not check out is an error, and the password is never sent before the channel is encrypted.

use rustls::pki_types::{CertificateDer, ServerName};
use std::fmt;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

pub use tokio_rustls::client::TlsStream;

/// A stream the mail clients can talk over (TCP, or TCP upgraded to TLS).
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
pub type BoxIo = Box<dyn Io>;

/// Why a connection could not be made, in terms the person can act on.
#[derive(Debug, Clone, PartialEq)]
pub enum NetError {
    Unreachable(String),
    Timeout,
    Certificate(String),
    Io(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Unreachable(h) => write!(f, "il server {h} non è raggiungibile: controlla l'indirizzo, la porta e la connessione"),
            NetError::Timeout => write!(f, "il server non risponde in tempo"),
            NetError::Certificate(why) => write!(f, "il certificato del server non è valido ({why}): non mi collego"),
            NetError::Io(e) => write!(f, "errore di rete: {e}"),
        }
    }
}

fn system_roots() -> &'static rustls::RootCertStore {
    static ROOTS: OnceLock<rustls::RootCertStore> = OnceLock::new();
    ROOTS.get_or_init(|| {
        let mut store = rustls::RootCertStore::empty();
        store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        for c in rustls_native_certs::load_native_certs().certs {
            let _ = store.add(c);
        }
        store
    })
}

/// The client configuration. `extra_roots` (DER) are trusted in addition to the system ones: the tests
/// use it for their self-signed servers; the app never sets it.
pub fn client_config(extra_roots: &[Vec<u8>]) -> Arc<rustls::ClientConfig> {
    let mut roots = system_roots().clone();
    for der in extra_roots {
        let _ = roots.add(CertificateDer::from(der.clone()));
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(cfg)
}

fn classify(host: &str, e: std::io::Error) -> NetError {
    if let Some(inner) = e.get_ref() {
        if let Some(rustls::Error::InvalidCertificate(c)) = inner.downcast_ref::<rustls::Error>() {
            return NetError::Certificate(format!("{c:?}"));
        }
        if let Some(r) = inner.downcast_ref::<rustls::Error>() {
            return NetError::Io(format!("TLS: {r}"));
        }
    }
    match e.kind() {
        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound | std::io::ErrorKind::HostUnreachable => {
            NetError::Unreachable(host.to_string())
        }
        std::io::ErrorKind::TimedOut => NetError::Timeout,
        _ => NetError::Io(e.to_string()),
    }
}

/// Opens a TCP connection (no TLS yet).
pub async fn connect_tcp(host: &str, port: u16, timeout: Duration) -> Result<TcpStream, NetError> {
    match tokio::time::timeout(timeout, TcpStream::connect((host, port))).await {
        Err(_) => Err(NetError::Timeout),
        Ok(Err(e)) => Err(match e.kind() {
            // A name that does not resolve is "not reachable" too.
            std::io::ErrorKind::Other | std::io::ErrorKind::InvalidInput => NetError::Unreachable(host.to_string()),
            _ => classify(host, e),
        }),
        Ok(Ok(s)) => {
            let _ = s.set_nodelay(true);
            Ok(s)
        }
    }
}

/// Starts the TLS handshake on an open stream (implicit TLS, or after STARTTLS).
pub async fn upgrade<S: Io + 'static>(stream: S, host: &str, extra_roots: &[Vec<u8>], timeout: Duration) -> Result<TlsStream<S>, NetError> {
    let name = ServerName::try_from(host.to_string()).map_err(|_| NetError::Unreachable(host.to_string()))?;
    let connector = tokio_rustls::TlsConnector::from(client_config(extra_roots));
    match tokio::time::timeout(timeout, connector.connect(name, stream)).await {
        Err(_) => Err(NetError::Timeout),
        Ok(Err(e)) => Err(classify(host, e)),
        Ok(Ok(s)) => Ok(s),
    }
}

/// TCP + implicit TLS.
pub async fn connect_tls(host: &str, port: u16, extra_roots: &[Vec<u8>], timeout: Duration) -> Result<TlsStream<TcpStream>, NetError> {
    let tcp = connect_tcp(host, port, timeout).await?;
    upgrade(tcp, host, extra_roots, timeout).await
}
