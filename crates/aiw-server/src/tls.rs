//! The TLS listener: accepts TCP connections, completes the TLS handshake and hands the decrypted
//! stream to the same router as the plain listener.
//!
//! * Plain HTTP from the network is refused: a connection that does not start with a TLS handshake
//!   fails at the handshake and gets no HTTP answer.
//! * One exception, for this computer only: a connection from loopback that does not start with a TLS
//!   handshake is served as plain HTTP, so that the clients on this computer that know nothing
//!   about TLS (the trainer calling the reward endpoint, a dev server proxy, scripts) keep working
//!   on `http://127.0.0.1:<port>` with the same rules as before.
//! * Requests that arrived encrypted carry the [`TlsConn`] marker: the owner token is `full` over
//!   TLS, the page's own origin is `https://…`, and HSTS is sent.

use crate::security::TlsConn;
use axum::extract::ConnectInfo;
use axum::Router;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tower::ServiceExt;

/// Time a client has to start and complete the handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// First byte of a TLS record carrying a handshake.
const TLS_HANDSHAKE: u8 = 0x16;

pub(crate) async fn serve(listener: TcpListener, app: Router, config: Arc<rustls::ServerConfig>) -> std::io::Result<()> {
    serve_inner(listener, app, config, true).await
}

/// `plain_from_loopback`: serve plain HTTP to this computer (see the module notes); tests turn it off to
/// see what a client on the network gets.
pub(crate) async fn serve_inner(
    listener: TcpListener,
    app: Router,
    config: Arc<rustls::ServerConfig>,
    plain_from_loopback: bool,
) -> std::io::Result<()> {
    let acceptor = tokio_rustls::TlsAcceptor::from(config);
    loop {
        let (tcp, peer) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                // Out of descriptors and the like: wait, do not spin.
                tracing::warn!("accept: {e}");
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };
        let (acceptor, app) = (acceptor.clone(), app.clone());
        tokio::spawn(async move {
            let mut first = [0u8; 1];
            match tokio::time::timeout(HANDSHAKE_TIMEOUT, tcp.peek(&mut first)).await {
                Ok(Ok(n)) if n > 0 => {}
                _ => return,
            }
            if first[0] != TLS_HANDSHAKE {
                if plain_from_loopback && peer.ip().to_canonical().is_loopback() {
                    run(TokioIo::new(tcp), app, peer, false).await;
                }
                // From the network: dropped, nothing is answered in the clear.
                return;
            }
            let Ok(Ok(stream)) = tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await else { return };
            run(TokioIo::new(stream), app, peer, true).await;
        });
    }
}

async fn run<I>(io: TokioIo<I>, app: Router, peer: SocketAddr, tls: bool)
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let svc = app.map_request(move |mut req: axum::http::Request<hyper::body::Incoming>| {
        req.extensions_mut().insert(ConnectInfo(peer));
        if tls {
            req.extensions_mut().insert(TlsConn);
        }
        req
    });
    let _ = Builder::new(TokioExecutor::new()).serve_connection_with_upgrades(io, TowerToHyperService::new(svc)).await;
}
