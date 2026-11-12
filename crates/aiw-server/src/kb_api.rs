//! The knowledge base over HTTP: collections, documents, search, jobs.
//!
//! Adding a path from this computer makes the engine read files with the person's rights, so it
//! is accepted only from this device (loopback or the in-process shell), like the pairing QR: a
//! paired remote device can send files from its own app but cannot point at folders here.

use crate::extract::{Json, Path};
use crate::{ok, ApiError, ApiResult, AppState, IsLocal};
use aiw_core::api::*;
use aiw_core::Error;
use axum::extract::State;
use serde_json::{json, Value};

pub(crate) async fn collections(State(s): State<AppState>) -> ApiResult<Vec<KbCollection>> {
    ok(s.engine.kb_collections()?)
}

pub(crate) async fn create_collection(State(s): State<AppState>, Json(b): Json<KbCollectionCreate>) -> ApiResult<KbCollection> {
    ok(s.engine.kb_create_collection(b)?)
}

pub(crate) async fn patch_collection(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<KbCollectionPatch>,
) -> ApiResult<KbCollection> {
    ok(s.engine.kb_update_collection(&id, b)?)
}

pub(crate) async fn delete_collection(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.kb_delete_collection(&id)?;
    ok(json!({}))
}

pub(crate) async fn documents(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<KbDocument>> {
    ok(s.engine.kb_documents(&id)?)
}

pub(crate) async fn add_documents(
    State(s): State<AppState>,
    IsLocal(local): IsLocal,
    Path(id): Path<String>,
    Json(b): Json<KbAddBody>,
) -> Result<Json<KbAddResult>, ApiError> {
    if b.path.is_some() && !local {
        return Err(ApiError(Error::Denied("Una cartella del computer si può aggiungere solo da questo dispositivo.".into())));
    }
    ok(s.engine.kb_add(&id, b, local).await?)
}

pub(crate) async fn reindex(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<KbAddResult> {
    ok(s.engine.kb_reindex(&id)?)
}

pub(crate) async fn delete_document(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.kb_delete_document(&id)?;
    ok(json!({}))
}

pub(crate) async fn search(State(s): State<AppState>, Json(b): Json<KbSearchBody>) -> ApiResult<KbSearchResult> {
    ok(s.engine.kb_search(&b.query, b.collection.as_deref(), b.max_results.unwrap_or(5)).await?)
}

pub(crate) async fn status(State(s): State<AppState>) -> ApiResult<KbStatus> {
    ok(s.engine.kb_status())
}

pub(crate) async fn cancel_job(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.kb_cancel(&id)?;
    ok(json!({}))
}
