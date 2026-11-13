//! The token and TLS endpoints (owner only, see [`crate::scopes`]) and the closing of the streams
//! that a revoked token opened.

use crate::extract::{Json, Path};
use crate::{ok, ApiError, ApiResult, AppState, AuthContext};
use aiw_core::api::{ApiTokenCreate, ApiTokenCreated, ApiTokenInfo, TlsStatus};
use aiw_core::Error;
use aiw_engine::tokens::TokenAuth;
use aiw_engine::Engine;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::sync::broadcast;

/// Wraps the body of a response that is a stream so that it ends when `auth` is revoked, rotated or
/// expires. A response with a known length is returned as it is: it is over before it could matter.
pub(crate) fn end_with_token(res: Response, engine: Engine, auth: TokenAuth, mut rx: broadcast::Receiver<String>) -> Response {
    use axum::body::HttpBody as _;
    let (parts, body) = res.into_parts();
    let streaming = parts.headers.get(header::CONTENT_TYPE).is_some_and(|v| v.as_bytes().starts_with(b"text/event-stream"))
        || body.size_hint().exact().is_none();
    if !streaming {
        return Response::from_parts(parts, body);
    }
    let id = auth.id.clone();
    let until_revoked = async move {
        loop {
            match rx.recv().await {
                Ok(revoked) if revoked == id => return,
                Ok(_) => {}
                // Notices were missed: look the token up instead.
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    if !engine.token_is_active(&id) {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => std::future::pending::<()>().await,
            }
        }
    };
    let until_expired = async move {
        match auth.expires_at.as_deref().and_then(|e| chrono::DateTime::parse_from_rfc3339(e).ok()) {
            Some(at) => {
                let wait = (at.with_timezone(&chrono::Utc) - chrono::Utc::now()).to_std().unwrap_or_default();
                tokio::time::sleep(wait).await
            }
            None => std::future::pending::<()>().await,
        }
    };
    let stop = async move {
        tokio::select! { _ = until_revoked => {}, _ = until_expired => {} }
    };
    let stream = body.into_data_stream().take_until(stop);
    Response::from_parts(parts, Body::from_stream(stream))
}

pub(crate) async fn list(State(s): State<AppState>) -> ApiResult<Vec<ApiTokenInfo>> {
    ok(s.engine.api_tokens())
}

pub(crate) async fn create(State(s): State<AppState>, Json(b): Json<ApiTokenCreate>) -> ApiResult<ApiTokenCreated> {
    let kind = b.kind.as_deref().unwrap_or("gateway");
    ok(s.engine.create_api_token(&b.name, kind, None, b.expires_in_days, b.scopes.as_deref())?)
}

pub(crate) async fn revoke(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.revoke_api_token(&id)?;
    ok(json!({}))
}

pub(crate) async fn rotate(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<ApiTokenCreated> {
    ok(s.engine.rotate_api_token(&id)?)
}

/// `DELETE /api/auth/token`: the token of the request drops itself (a device that is unpaired tells the
/// device it was paired to, so that the credential does not outlive the pairing).
pub(crate) async fn revoke_self(State(s): State<AppState>, axum::Extension(ctx): axum::Extension<AuthContext>) -> ApiResult<Value> {
    let token = ctx.token.ok_or_else(|| ApiError(Error::Invalid("nessun token da revocare".into())))?;
    s.engine.revoke_api_token(&token.id)?;
    ok(json!({}))
}

pub(crate) async fn tls_status(State(s): State<AppState>) -> ApiResult<TlsStatus> {
    ok(s.engine.tls_status())
}

/// `GET /api/tls/ca`: the same certificate inside JSON (`{pem}`), for the app's own download button.
pub(crate) async fn tls_ca_json(State(s): State<AppState>) -> ApiResult<Value> {
    let pem = s.engine.tls_ca_pem().ok_or_else(|| {
        ApiError(Error::NotFound("nessun certificato da scaricare: imposta server.tls = self_signed e riavvia il servizio".into()))
    })?;
    ok(json!({ "pem": pem }))
}

/// `GET /api/tls/ca.pem`: the local authority, to install in a browser on another device.
pub(crate) async fn tls_ca(State(s): State<AppState>) -> Response {
    match s.engine.tls_ca_pem() {
        Some(pem) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, HeaderValue::from_static("application/x-pem-file")),
                (header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment; filename=\"aiw-ca.pem\"")),
            ],
            pem,
        )
            .into_response(),
        None => ApiError(Error::NotFound("nessun certificato da scaricare: imposta server.tls = self_signed e riavvia il servizio".into()))
            .into_response(),
    }
}
