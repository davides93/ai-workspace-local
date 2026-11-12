//! HTTP surface of the engine (architecture §8). The same [`router`] serves `aiw serve` over TCP
//! and the Tauri shells in-process (tower `oneshot`), so every client sees one API.

mod a2a;
mod anthropic;
mod extract;
mod gateway;
mod kb_api;
mod mcp_server;
pub mod mesh;
mod security;

use aiw_core::api::*;
use aiw_core::Error;
use aiw_engine::{Engine, PairOutcome};
use axum::body::Body;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use extract::{Json, Path, Query};
use futures::stream::{self, Stream, StreamExt};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use tower_http::services::{ServeDir, ServeFile};

/// Re-exported so shells can keep a router without depending on axum directly.
pub type AxumRouter = Router;

pub struct ApiError(pub Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = StatusCode::from_u16(self.0.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (code, Json(json!({ "error": self.0.to_string() }))).into_response()
    }
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        ApiError(e)
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

fn ok<T>(v: T) -> ApiResult<T> {
    Ok(Json(v))
}

#[derive(Clone)]
pub struct AppState {
    pub engine: Engine,
}

/// Is this request coming from this machine (or from the in-process Tauri dispatcher)?
fn is_local(req: &Request) -> bool {
    match req.extensions().get::<ConnectInfo<SocketAddr>>() {
        Some(ConnectInfo(addr)) => addr.ip().to_canonical().is_loopback(),
        None => true,
    }
}

/// Authentication and CSRF/DNS-rebinding defence (see [`security`]). A request with the API token
/// always passes; one without it passes only when it comes from this machine and is not a web
/// page acting on its own (loopback `Host`, allowed `Origin`), or when it is a pairing attempt.
async fn auth(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    if !security::is_guarded(&path) || security::has_valid_token(&req, st.engine.token()) {
        return next.run(req).await;
    }
    let refusal = match security::peer(&req) {
        security::Peer::InProcess => None,
        security::Peer::Loopback => security::check_loopback(&req, &st),
        // Pairing is how a LAN device gets its token; a web page must still not be able to try codes.
        security::Peer::Remote if path == "/api/pair" => security::check_origin(&req, &st),
        security::Peer::Remote => {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "serve il token di pairing per l'accesso dalla rete"}))).into_response()
        }
    };
    match refusal {
        None => next.run(req).await,
        Some(refusal) => refusal,
    }
}

/// Builds the full API router. `ui_dir` serves the web UI (`apps/ui/dist`) when present.
pub fn router(engine: Engine, ui_dir: Option<PathBuf>) -> Router {
    let st = AppState { engine };
    let api = Router::new()
        .route("/api/system", get(system))
        .route("/api/kill", post(kill))
        .route("/api/pair", post(pair))
        .route("/api/update", get(update_status))
        .route("/api/update/check", post(update_check))
        .route("/api/update/download", post(update_download))
        .route("/api/update/install", post(update_install))
        .route("/api/runtimes", get(runtimes))
        .route("/api/runtimes/refresh", post(runtimes_refresh))
        .route("/api/runtimes/{id}/settings", put(runtime_settings))
        .route("/api/runtimes/{id}/logs", get(runtime_logs))
        .route("/api/runtimes/{id}/install-plan", get(runtime_install_plan))
        .route("/api/runtimes/{id}/install", get(runtime_install_status).post(runtime_install_start))
        .route("/api/runtimes/{id}/{action}", post(runtime_action))
        .route("/api/models", get(models))
        .route("/api/model-preview", post(preview_system_model))
        .route("/api/models/catalog", get(catalog))
        .route("/api/recommend", get(recommend))
        .route("/api/models/providers", get(model_providers))
        .route("/api/models/providers/{id}/auth", put(provider_auth))
        .route("/api/models/search", get(search_models))
        .route("/api/models/remote", get(remote_model))
        .route("/api/models/pull", post(pull))
        .route("/api/models/downloads", get(downloads))
        .route("/api/models/downloads/{id}/cancel", post(cancel_download))
        .route("/api/models/downloads/{id}", delete(clear_download))
        // Model ids can contain `/` (`hf-cache:org/repo/file`): a catch-all keeps them whole.
        .route("/api/models/{*id}", delete(delete_model))
        .route("/api/profiles", get(profiles))
        .route("/api/profiles/{id}", put(save_profile).delete(delete_profile))
        .route("/api/agents", get(agents))
        .route("/api/agents/{id}", put(save_agent).delete(delete_agent))
        .route("/api/agents/{id}/routing", get(agent_routing).delete(reset_agent_routing))
        .route("/api/remote-agents", get(remote_agents))
        .route("/api/remote-agents/{id}", put(save_remote_agent).delete(delete_remote_agent))
        .route("/api/teams", get(teams))
        .route("/api/teams/{id}", get(get_team).put(save_team).delete(delete_team))
        .route("/api/policy", get(get_policy).put(put_policy))
        .route("/api/hooks", get(get_hooks).put(put_hooks))
        .route("/api/sessions", get(sessions).post(create_session))
        .route("/api/sessions/{id}", get(get_session).patch(patch_session).delete(delete_session))
        .route("/api/sessions/{id}/worktree", get(worktree_status).delete(worktree_discard))
        .route("/api/sessions/{id}/worktree/apply", post(worktree_apply))
        .route("/api/sessions/{id}/messages", get(messages))
        .route("/api/sessions/{id}/attachments/{attachment_id}", get(get_attachment))
        .route("/api/sessions/{id}/messages/{mid}/feedback", put(set_feedback).delete(delete_feedback))
        .route("/api/sessions/{id}/feedback", get(session_feedback))
        .route("/api/sessions/{id}/reflect", post(reflect_session))
        .route("/api/lessons", get(lessons))
        .route("/api/lessons/{id}/approve", post(approve_lesson))
        .route("/api/lessons/{id}/reject", post(reject_lesson))
        .route("/api/kb/status", get(kb_api::status))
        .route("/api/kb/collections", get(kb_api::collections).post(kb_api::create_collection))
        .route("/api/kb/collections/{id}", patch(kb_api::patch_collection).delete(kb_api::delete_collection))
        .route(
            "/api/kb/collections/{id}/documents",
            get(kb_api::documents).post(kb_api::add_documents).layer(DefaultBodyLimit::max(RUN_BODY_LIMIT)),
        )
        .route("/api/kb/collections/{id}/reindex", post(kb_api::reindex))
        .route("/api/kb/documents/{id}", delete(kb_api::delete_document))
        .route("/api/kb/jobs/{id}/cancel", post(kb_api::cancel_job))
        .route("/api/kb/search", post(kb_api::search))
        .route("/api/memories", get(memories).post(create_memory).delete(delete_all_memories))
        .route("/api/memories/{id}", patch(patch_memory).delete(delete_memory))
        .route("/api/memories/{id}/forget", post(forget_memory))
        .route("/api/memories/{id}/restore", post(restore_memory))
        .route("/api/memories/{id}/approve", post(approve_memory))
        .route("/api/memories/{id}/reject", post(reject_memory))
        .route("/api/sessions/{id}/active-run", get(active_run))
        .route("/api/sessions/{id}/runs", post(start_run).layer(DefaultBodyLimit::max(RUN_BODY_LIMIT)))
        .route("/api/sessions/{id}/checkpoints", get(session_checkpoints))
        .route("/api/runs/{id}/cancel", post(cancel_run))
        .route("/api/runs/{id}/checkpoint", get(run_checkpoint))
        .route("/api/runs/{id}/rewind", post(run_rewind))
        .route("/api/runs/{id}/events", get(run_events_sse))
        .route("/api/runs/{id}/events.json", get(run_events_json))
        .route("/api/approvals", get(approvals))
        .route("/api/approvals/{id}", post(respond_approval))
        .route("/api/proposals", get(proposals))
        .route("/api/proposals/{id}", get(proposal))
        .route("/api/proposals/{id}/approve", post(approve_proposal))
        .route("/api/proposals/{id}/dismiss", post(dismiss_proposal))
        .route("/api/proactive/status", get(proactive_status))
        .route("/api/proactive/tick", post(proactive_tick))
        .route("/api/proactive/background-tick", post(proactive_background_tick))
        .route("/api/workflows", get(workflows))
        .route("/api/workflows/runs", get(workflow_runs))
        .route("/api/workflows/{id}", put(save_workflow).delete(delete_workflow))
        .route("/api/workflows/{id}/run", post(run_workflow))
        // Outside /api: authenticated by the workflow's own secret, not by the pairing token.
        .route("/hooks/{id}", post(webhook))
        // Device mesh and A2A.
        .route("/api/mesh", get(mesh_state))
        .route("/api/mesh/pair", post(mesh_pair))
        .route("/api/mesh/peers/{id}", delete(mesh_remove_peer))
        .route("/api/pairing/qr", get(pairing_qr))
        // Voice mode.
        .route("/api/voice", get(voice_status))
        .route("/api/voice/transcribe", post(voice_transcribe).layer(DefaultBodyLimit::max(VOICE_BODY_LIMIT)))
        .route("/.well-known/agent-card.json", get(a2a::agent_card))
        .route("/a2a", post(a2a::handle))
        .route("/api/tools", get(tools))
        .route("/api/tools/{name}", put(set_tool))
        .route("/api/mcp/servers", get(mcp_servers).post(save_mcp))
        .route("/api/mcp/servers/{id}", delete(remove_mcp))
        .route("/api/config", get(config).put(set_config))
        .route("/api/audit", get(audit))
        .route("/api/traces/export", get(traces_export))
        .route("/api/evals", get(eval_suites))
        .route("/api/evals/{id}", get(eval_suite).put(save_eval_suite).delete(delete_eval_suite))
        .route("/api/evals/{id}/runs", post(start_eval))
        .route("/api/evals/{id}/reward", post(eval_reward))
        .route("/api/evals/{id}/optimize", post(start_optimization))
        .route("/api/prompt-optimizations", get(prompt_optimizations))
        .route("/api/prompt-optimizations/{id}", get(prompt_optimization))
        .route("/api/prompt-optimizations/{id}/apply", post(apply_optimization))
        .route("/api/eval-runs", get(eval_runs))
        .route("/api/eval-runs/compare", get(compare_eval_runs))
        .route("/api/eval-runs/{id}", get(eval_run))
        .route("/api/trainers", get(trainers))
        .route("/api/trainers/{id}/install-plan", get(trainer_install_plan))
        .route("/api/trainers/{id}/install", get(trainer_install_status).post(trainer_install_start))
        .route("/api/train/doctor", get(train_doctor))
        .route("/api/datasets", get(datasets).post(build_dataset))
        .route("/api/dataset-import", post(import_sft_dataset).layer(DefaultBodyLimit::max(6_000_000)))
        .route("/api/datasets/{id}", get(dataset).delete(delete_dataset))
        .route("/api/train/jobs", get(train_jobs).post(start_train_job))
        .route("/api/train/jobs/{id}", get(train_job))
        .route("/api/train/jobs/{id}/log", get(train_job_log))
        .route("/api/train/jobs/{id}/cancel", post(cancel_train_job))
        .route("/api/adapters", get(adapters))
        .route("/api/adapters/{id}", get(adapter).delete(delete_adapter))
        .route("/v1/models", get(gateway::models))
        .route("/v1/chat/completions", post(gateway::chat_completions))
        .route("/v1/embeddings", post(gateway::embeddings))
        .route("/v1/messages", post(anthropic::messages))
        .route("/v1/messages/count_tokens", post(anthropic::count_tokens))
        .route("/v1/audio/transcriptions", post(audio_transcriptions).layer(DefaultBodyLimit::max(VOICE_BODY_LIMIT)))
        .route("/mcp", post(mcp_server::handle).get(mcp_server::get_stream).delete(|| async { StatusCode::OK }));
    let mut app = api.method_not_allowed_fallback(method_not_allowed).with_state(st.clone());
    app = match ui_dir.filter(|d| d.join("index.html").exists()) {
        // The single-page UI answers every path that is not the server's own.
        Some(dir) => {
            let index = dir.join("index.html");
            let spa = ServeDir::new(dir).fallback(ServeFile::new(index));
            app.fallback(move |req: Request| {
                let spa = spa.clone();
                async move {
                    if is_backend_path(req.uri().path()) {
                        return unknown_route();
                    }
                    match tower::ServiceExt::oneshot(spa, req).await {
                        Ok(res) => res.into_response(),
                        Err(never) => match never {},
                    }
                }
            })
        }
        None => app.fallback(|| async { unknown_route() }),
    };
    // Outermost: CORS answers preflights (which carry no credentials) before `auth` sees them.
    app.layer(middleware::from_fn_with_state(st.clone(), auth))
        .layer(middleware::from_fn_with_state(st, security::cors))
        .layer(middleware::from_fn(hardening_headers))
}

/// The UI asks the user to confirm agent actions: no other site may frame it (clickjacking),
/// sniff its responses into another type, or learn its URLs (which may carry `?token=`) from a
/// Referer.
async fn hardening_headers(req: Request, next: middleware::Next) -> Response {
    use axum::http::{header, HeaderValue};
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.entry(header::CONTENT_SECURITY_POLICY).or_insert(HeaderValue::from_static("frame-ancestors 'none'"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    res
}

/// Paths that belong to the server (REST API, gateways, MCP, A2A, webhooks): one that no route
/// matches is an error for the caller, not a page of the UI.
fn is_backend_path(path: &str) -> bool {
    let under = |prefix: &str| path == prefix || path.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'));
    ["/api", "/v1", "/hooks", "/.well-known"].iter().any(|p| under(p)) || path.starts_with("/mcp") || path.starts_with("/a2a")
}

fn unknown_route() -> Response {
    ApiError(Error::NotFound("percorso non trovato".into())).into_response()
}

async fn method_not_allowed() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, Json(json!({"error": "metodo non consentito per questo percorso"}))).into_response()
}

// ------------------------------------------------------------------ handlers

async fn system(State(s): State<AppState>, req: Request) -> ApiResult<SystemInfo> {
    // The pairing code is only shown to the device itself.
    ok(s.engine.system(is_local(&req)))
}
async fn kill(State(s): State<AppState>) -> ApiResult<Value> {
    ok(s.engine.kill().await)
}
#[derive(Deserialize)]
struct PairBody {
    code: String,
}
/// Address of the TCP peer; `None` for the in-process dispatcher.
struct ClientIp(Option<String>);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ClientIp {
    type Rejection = Infallible;
    async fn from_request_parts(parts: &mut axum::http::request::Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_canonical().to_string())))
    }
}

/// Does the request come from this device (loopback, or the in-process shell)? For the routes that
/// make the engine read this computer's files.
struct IsLocal(bool);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for IsLocal {
    type Rejection = Infallible;
    async fn from_request_parts(parts: &mut axum::http::request::Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(IsLocal(match parts.extensions.get::<ConnectInfo<SocketAddr>>() {
            Some(ConnectInfo(addr)) => addr.ip().to_canonical().is_loopback(),
            None => true,
        }))
    }
}

/// Pause after a wrong code, on top of the rate limit: every guess costs the guesser time.
const PAIR_FAIL_DELAY: Duration = Duration::from_millis(if cfg!(test) { 0 } else { 400 });

fn human_wait(secs: u64) -> String {
    match secs {
        0..=119 => format!("{secs} secondi"),
        120..=7199 => format!("{} minuti", secs.div_ceil(60)),
        _ => format!("{} ore", secs.div_ceil(3600)),
    }
}

async fn pair(State(s): State<AppState>, ClientIp(client): ClientIp, Json(b): Json<PairBody>) -> Response {
    match s.engine.pair_attempt(&b.code, client.as_deref()) {
        PairOutcome::Paired(token) => Json(json!({"token": token})).into_response(),
        PairOutcome::Invalid => {
            tokio::time::sleep(PAIR_FAIL_DELAY).await;
            ApiError(Error::Denied("codice di pairing non valido".into())).into_response()
        }
        PairOutcome::Locked { retry_after } => {
            let secs = retry_after.as_secs() + u64::from(retry_after.subsec_nanos() > 0);
            let body = json!({
                "error": format!("troppi tentativi di pairing falliti: riprova tra {}", human_wait(secs)),
                "retry_after_s": secs,
            });
            let mut res = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
            res.headers_mut().insert(axum::http::header::RETRY_AFTER, secs.into());
            res
        }
    }
}
async fn update_status(State(s): State<AppState>) -> ApiResult<aiw_engine::update::UpdateStatus> {
    ok(s.engine.update_status())
}
async fn update_check(State(s): State<AppState>) -> ApiResult<aiw_engine::update::UpdateStatus> {
    ok(s.engine.update_check().await?)
}
async fn update_download(State(s): State<AppState>) -> ApiResult<aiw_engine::update::UpdateStatus> {
    ok(s.engine.update_download().await?)
}
async fn update_install(State(s): State<AppState>) -> ApiResult<aiw_engine::update::UpdateStatus> {
    ok(s.engine.update_install()?)
}
async fn runtimes(State(s): State<AppState>) -> ApiResult<Vec<RuntimeInfo>> {
    ok(s.engine.runtimes())
}
async fn runtimes_refresh(State(s): State<AppState>) -> ApiResult<Vec<RuntimeInfo>> {
    ok(s.engine.refresh_runtimes().await)
}
#[derive(Deserialize, Default)]
struct ModelOpt {
    model: Option<String>,
}
async fn runtime_action(
    State(s): State<AppState>,
    Path((id, action)): Path<(String, String)>,
    body: Option<Json<ModelOpt>>,
) -> ApiResult<RuntimeInfo> {
    let model = body.and_then(|b| b.0.model).filter(|m| !m.is_empty());
    ok(s.engine.runtime_action(&id, &action, model).await?)
}
async fn runtime_settings(State(s): State<AppState>, Path(id): Path<String>, Json(v): Json<Map<String, Value>>) -> ApiResult<RuntimeInfo> {
    ok(s.engine.update_runtime_settings(&id, v)?)
}
async fn runtime_install_plan(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_runtimes::install::InstallPlan> {
    ok(s.engine.runtime_install_plan(&id).await?)
}
#[derive(Deserialize)]
struct RuntimeInstallOptions {
    #[serde(default)]
    start: bool,
}

async fn runtime_install_start(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(options): Query<RuntimeInstallOptions>,
) -> ApiResult<aiw_runtimes::install::InstallJob> {
    ok(s.engine.runtime_install_start(&id, options.start).await?)
}
async fn runtime_install_status(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Option<aiw_runtimes::install::InstallJob>> {
    ok(s.engine.runtime_install_status(&id))
}
async fn runtime_logs(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<String>> {
    ok(s.engine.runtime_logs(&id).await)
}
#[derive(Deserialize)]
struct PreviewModel {
    id: String,
    prompt: String,
}

async fn preview_system_model(State(s): State<AppState>, Json(input): Json<PreviewModel>) -> ApiResult<String> {
    ok(s.engine.preview_system_model(&input.id, &input.prompt).await?)
}

async fn models(State(s): State<AppState>) -> ApiResult<Vec<LocalModel>> {
    ok(s.engine.models())
}
async fn catalog(State(s): State<AppState>) -> ApiResult<Vec<CatalogModel>> {
    ok(s.engine.catalog())
}
/// The model adviser: per task (chat, agent, reasoning, vision, embeddings) the best installed
/// model for this device and the best one to download.
async fn recommend(State(s): State<AppState>) -> ApiResult<Recommendations> {
    ok(s.engine.recommend())
}
async fn model_providers(State(s): State<AppState>) -> ApiResult<Vec<ModelProviderInfo>> {
    ok(s.engine.model_providers())
}
/// `token`: absent leaves it alone, `null` clears it; `endpoint`: absent leaves it alone, `""` restores the default.
#[derive(Deserialize)]
struct ProviderAuthBody {
    #[serde(default, deserialize_with = "present")]
    token: Option<Option<String>>,
    endpoint: Option<String>,
}
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    <Option<String> as Deserialize>::deserialize(d).map(Some)
}
async fn provider_auth(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<ProviderAuthBody>) -> ApiResult<ModelProviderInfo> {
    ok(s.engine.set_provider_auth(&id, b.token, b.endpoint).await?)
}
#[derive(Deserialize)]
struct SearchParams {
    provider: Option<String>,
    #[serde(default)]
    q: String,
    format: Option<String>,
    /// downloads | likes | updated
    sort: Option<String>,
    /// At most 50.
    limit: Option<usize>,
    cursor: Option<String>,
}
async fn search_models(State(s): State<AppState>, Query(p): Query<SearchParams>) -> ApiResult<SearchPage> {
    let provider = p.provider.filter(|x| !x.is_empty()).ok_or_else(|| ApiError(Error::Invalid("serve il parametro `provider`".into())))?;
    let q = aiw_models::hub::SearchQuery { q: p.q, format: p.format, sort: p.sort, limit: p.limit.unwrap_or(0), cursor: p.cursor };
    ok(s.engine.search_models(&provider, q).await?)
}
#[derive(Deserialize)]
struct RemoteParams {
    provider: Option<String>,
    model: Option<String>,
    revision: Option<String>,
}
async fn remote_model(State(s): State<AppState>, Query(p): Query<RemoteParams>) -> ApiResult<RemoteModelDetail> {
    let (Some(provider), Some(model)) = (p.provider.filter(|x| !x.is_empty()), p.model.filter(|x| !x.is_empty())) else {
        return Err(ApiError(Error::Invalid("servono i parametri `provider` e `model`".into())));
    };
    ok(s.engine.remote_model(&provider, &model, p.revision.as_deref()).await?)
}
async fn pull(State(s): State<AppState>, Json(v): Json<Value>) -> ApiResult<DownloadProgress> {
    let req: PullRequest = serde_json::from_value(v).map_err(|_| {
        ApiError(Error::Invalid("richiesta di download non valida: usa {provider, model, variant}, {id}, {repo, file} oppure {url}".into()))
    })?;
    ok(s.engine.pull_model(req).await?)
}
async fn downloads(State(s): State<AppState>) -> ApiResult<Vec<DownloadProgress>> {
    ok(s.engine.downloads())
}
async fn cancel_download(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<DownloadProgress> {
    ok(s.engine.cancel_download(&id)?)
}
async fn clear_download(State(s): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    s.engine.clear_download(&id)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn delete_model(State(s): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    s.engine.delete_model(&id)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn profiles(State(s): State<AppState>) -> ApiResult<Vec<InferenceProfile>> {
    ok(s.engine.profiles()?)
}
async fn save_profile(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(mut p): Json<InferenceProfile>,
) -> ApiResult<InferenceProfile> {
    p.id = id;
    ok(s.engine.save_profile(p)?)
}
async fn delete_profile(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.delete_profile(&id)?;
    ok(json!({}))
}
async fn agents(State(s): State<AppState>) -> ApiResult<Vec<AgentProfile>> {
    ok(s.engine.agents()?)
}
async fn delete_agent(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.delete_agent(&id)?;
    ok(json!({}))
}
async fn save_agent(State(s): State<AppState>, Path(id): Path<String>, Json(mut a): Json<AgentProfile>) -> ApiResult<AgentProfile> {
    a.id = id;
    ok(s.engine.save_agent(a)?)
}
async fn remote_agents(State(s): State<AppState>) -> ApiResult<Vec<RemoteAgent>> {
    ok(s.engine.remote_agents())
}
async fn save_remote_agent(State(s): State<AppState>, Path(id): Path<String>, Json(mut r): Json<RemoteAgent>) -> ApiResult<RemoteAgent> {
    r.id = id;
    ok(s.engine.save_remote_agent(r)?)
}
async fn delete_remote_agent(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.delete_remote_agent(&id)?;
    ok(json!({}))
}
async fn teams(State(s): State<AppState>) -> ApiResult<Vec<TeamDef>> {
    ok(s.engine.teams()?)
}
async fn get_team(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<TeamDef> {
    ok(s.engine.team(&id)?)
}
async fn save_team(State(s): State<AppState>, Path(id): Path<String>, Json(mut t): Json<TeamDef>) -> ApiResult<TeamDef> {
    t.id = id;
    ok(s.engine.save_team(t)?)
}
async fn delete_team(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.delete_team(&id)?;
    ok(json!({}))
}
async fn get_policy(State(s): State<AppState>) -> ApiResult<Value> {
    ok(json!({"rules": s.engine.policy_rules()}))
}
async fn put_policy(State(s): State<AppState>, Json(v): Json<aiw_engine::PolicyDoc>) -> ApiResult<Value> {
    s.engine.set_policy_rules(v.rules)?;
    get_policy(State(s)).await
}
async fn get_hooks(State(s): State<AppState>) -> ApiResult<Value> {
    ok(json!({"hooks": s.engine.hooks()}))
}
async fn put_hooks(State(s): State<AppState>, Json(v): Json<aiw_engine::HooksDoc>) -> ApiResult<Value> {
    s.engine.set_hooks(v.hooks)?;
    get_hooks(State(s)).await
}
async fn sessions(State(s): State<AppState>) -> ApiResult<Vec<Session>> {
    ok(s.engine.sessions()?)
}
#[derive(Deserialize)]
struct NewSession {
    agent_id: String,
    title: Option<String>,
    /// none | worktree
    isolation: Option<String>,
}
async fn create_session(State(s): State<AppState>, Json(b): Json<NewSession>) -> ApiResult<Session> {
    ok(s.engine.create_session_with(&b.agent_id, b.title, b.isolation.as_deref())?)
}
async fn get_session(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Session> {
    ok(s.engine.session(&id)?)
}
#[derive(Deserialize)]
struct SessionPatch {
    title: String,
}
async fn patch_session(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<SessionPatch>) -> ApiResult<Session> {
    ok(s.engine.rename_session(&id, &b.title)?)
}
async fn worktree_status(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_core::api::WorktreeStatus> {
    ok(s.engine.worktree_status(&id)?)
}
async fn worktree_apply(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_core::api::WorktreeStatus> {
    ok(s.engine.worktree_apply(&id)?)
}
async fn worktree_discard(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.worktree_discard(&id)?;
    ok(json!({}))
}
async fn session_checkpoints(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<aiw_core::api::CheckpointInfo>> {
    ok(s.engine.session_checkpoints(&id)?)
}
async fn run_checkpoint(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_core::api::CheckpointInfo> {
    ok(s.engine.checkpoint_info(&id)?)
}
async fn run_rewind(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_core::api::RewindResult> {
    ok(s.engine.rewind(&id)?)
}
async fn delete_session(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.delete_session(&id)?;
    ok(json!({}))
}
async fn messages(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<ChatMessage>> {
    ok(s.engine.messages(&id)?)
}
#[derive(Deserialize)]
struct FeedbackBody {
    rating: i32,
    note: Option<String>,
}
async fn set_feedback(
    State(s): State<AppState>,
    Path((id, mid)): Path<(String, String)>,
    Json(b): Json<FeedbackBody>,
) -> ApiResult<Feedback> {
    ok(s.engine.set_feedback(&id, &mid, b.rating, b.note)?)
}
async fn delete_feedback(State(s): State<AppState>, Path((id, mid)): Path<(String, String)>) -> ApiResult<Value> {
    s.engine.delete_feedback(&id, &mid)?;
    ok(json!({}))
}
async fn session_feedback(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<Feedback>> {
    ok(s.engine.session_feedback(&id)?)
}
#[derive(Deserialize)]
struct LessonsQuery {
    status: Option<String>,
}
async fn lessons(State(s): State<AppState>, Query(q): Query<LessonsQuery>) -> ApiResult<Vec<Lesson>> {
    ok(s.engine.lessons(q.status.as_deref())?)
}
#[derive(Deserialize, Default)]
struct ApproveLesson {
    title: Option<String>,
    content: Option<String>,
}
/// Approving writes the lesson (`AGENTS.md` or a new skill); the title and the content can be edited first.
async fn approve_lesson(State(s): State<AppState>, Path(id): Path<String>, body: Option<Json<ApproveLesson>>) -> ApiResult<Lesson> {
    let b = body.map(|b| b.0).unwrap_or_default();
    ok(s.engine.approve_lesson(&id, b.title, b.content)?)
}
async fn reject_lesson(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Lesson> {
    ok(s.engine.reject_lesson(&id)?)
}
#[derive(Deserialize)]
struct ProposalsQuery {
    /// open | approved | dismissed | expired | done; absent = all.
    status: Option<String>,
    /// proposal | briefing; absent = both.
    kind: Option<String>,
}
async fn proposals(State(s): State<AppState>, Query(q): Query<ProposalsQuery>) -> ApiResult<Vec<Proposal>> {
    ok(s.engine.proposals(q.status.as_deref(), q.kind.as_deref())?)
}
async fn proposal(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Proposal> {
    ok(s.engine.proposal(&id)?)
}
/// The body is optional: `{"instruction": "..."}` runs the edited instruction instead of the proposed one.
async fn approve_proposal(State(s): State<AppState>, Path(id): Path<String>, body: Option<Json<ProposalApprove>>) -> ApiResult<Proposal> {
    ok(s.engine.proposal_approve(&id, body.and_then(|b| b.0.instruction).as_deref()).await?)
}
async fn dismiss_proposal(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Proposal> {
    ok(s.engine.proposal_dismiss(&id)?)
}
async fn proactive_status(State(s): State<AppState>) -> ApiResult<ProactiveStatus> {
    ok(s.engine.proactive_status())
}
/// The check woken by the operating system of a phone: the same tick under a hard time budget, answered with a
/// summary small enough for a local notification. The body is optional (`reason`, `budget_s`, `on_power`).
async fn proactive_background_tick(State(s): State<AppState>, body: Option<Json<BackgroundRequest>>) -> ApiResult<BackgroundSummary> {
    ok(s.engine.proactive_background_tick(&body.map(|b| b.0).unwrap_or_default()).await?)
}
/// Wakes the assistant once, now (the same gates as the heartbeat apply: off, quiet hours, budget, power).
async fn proactive_tick(State(s): State<AppState>) -> ApiResult<TickResult> {
    ok(s.engine.proactive_tick(aiw_engine::proactive::Wake::Manual).await?)
}
#[derive(Deserialize)]
struct MemoriesQuery {
    /// active | proposed | forgotten | all; absent = active and proposed.
    status: Option<String>,
    kind: Option<String>,
    /// Search text: the answer is ranked by relevance and carries a `score`.
    q: Option<String>,
}
async fn memories(State(s): State<AppState>, Query(q): Query<MemoriesQuery>) -> ApiResult<Value> {
    let text = q.q.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let Some(text) = text else {
        return ok(serde_json::to_value(s.engine.memories(q.status.as_deref(), q.kind.as_deref())?).unwrap_or(Value::Null));
    };
    let statuses: Vec<&str> = match q.status.as_deref().filter(|x| !x.is_empty()) {
        None => vec!["active", "proposed"],
        Some("all") => vec!["active", "proposed", "forgotten"],
        Some(x @ ("active" | "proposed" | "forgotten")) => vec![x],
        Some(x) => return Err(Error::Invalid(format!("stato non valido «{x}»: usa active, proposed, forgotten o all")).into()),
    };
    let mut hits = s.engine.memory_search(text, None, &statuses, 50).await?;
    if let Some(k) = q.kind.as_deref().filter(|k| !k.is_empty()) {
        hits.retain(|h| h.memory.kind == k);
    }
    ok(serde_json::to_value(hits).unwrap_or(Value::Null))
}
async fn create_memory(State(s): State<AppState>, Json(b): Json<MemoryCreate>) -> ApiResult<Memory> {
    ok(s.engine.memory_create(b)?)
}
async fn patch_memory(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<MemoryPatch>) -> ApiResult<Memory> {
    ok(s.engine.memory_update(&id, b)?)
}
/// Soft delete: `restore` brings it back.
async fn forget_memory(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Memory> {
    ok(s.engine.memory_forget(&id)?)
}
async fn restore_memory(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Memory> {
    ok(s.engine.memory_restore(&id)?)
}
async fn approve_memory(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Memory> {
    ok(s.engine.memory_approve(&id)?)
}
async fn reject_memory(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Memory> {
    ok(s.engine.memory_reject(&id)?)
}
async fn delete_memory(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.memory_delete(&id)?;
    ok(json!({}))
}
#[derive(Deserialize)]
struct DeleteAllMemories {
    /// Must be `true`: there is no way back.
    #[serde(default)]
    confirm: bool,
}
async fn delete_all_memories(State(s): State<AppState>, Query(q): Query<DeleteAllMemories>) -> ApiResult<Value> {
    if !q.confirm {
        return Err(Error::Invalid("per cancellare tutti i ricordi aggiungi ?confirm=true".into()).into());
    }
    ok(json!({"deleted": s.engine.memory_delete_all()?}))
}
#[derive(Deserialize, Default)]
struct ReflectBody {
    run_id: Option<String>,
}
/// Asks the agent for a lesson on the conversation now (the answer waits for the model).
async fn reflect_session(State(s): State<AppState>, Path(id): Path<String>, body: Option<Json<ReflectBody>>) -> ApiResult<Lesson> {
    let b = body.map(|b| b.0).unwrap_or_default();
    ok(s.engine.reflect(&id, b.run_id.as_deref().filter(|r| !r.is_empty())).await?)
}
async fn active_run(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.session(&id)?; // an unknown session is a 404, not "no run"
    ok(json!({"run_id": s.engine.active_run(&id)}))
}
#[derive(Deserialize)]
struct RunBody {
    #[serde(default)]
    input: String,
    /// a2ui.action when a surface sends a user action
    kind: Option<String>,
    /// Reasoning for this run only (`off` | `auto` | `deep`): wins over the profile's. Absent: the profile's.
    #[serde(default)]
    thinking: Option<aiw_core::api::ThinkingMode>,
    /// Files attached to the message (`data` is base64). `input` may be empty when there are some.
    #[serde(default)]
    attachments: Vec<aiw_core::api::AttachmentUpload>,
}
/// Room for the largest allowed attachments (25 MB) as base64 inside JSON, plus the text.
const RUN_BODY_LIMIT: usize = aiw_core::api::MAX_ATTACHMENTS_TOTAL_BYTES / 3 * 4 + (2 << 20);
async fn start_run(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<RunBody>) -> ApiResult<Value> {
    ok(json!({"run_id": s.engine.start_run_attach(&id, b.input, b.kind.as_deref(), b.thinking, b.attachments).await?}))
}
async fn get_attachment(State(s): State<AppState>, Path((id, att)): Path<(String, String)>) -> ApiResult<aiw_core::api::AttachmentData> {
    ok(s.engine.attachment_data(&id, &att)?)
}
async fn cancel_run(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.cancel_run(&id)?;
    ok(json!({}))
}
async fn run_events_json(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    ok(serde_json::to_value(s.engine.run_events(&id)).unwrap_or(Value::Null))
}

#[derive(Deserialize)]
struct EventsQuery {
    /// Resume after this event (same as the `Last-Event-ID` header).
    after: Option<u64>,
}

/// AG-UI event stream: replays what already happened, then follows the live bus until the run ends.
/// A client that already has events up to `seq` N (`Last-Event-ID: N`, which `EventSource` sends by
/// itself when it reconnects, or `?after=N`) gets only the ones after it.
async fn run_events_sse(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
    headers: axum::http::HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let after = q.after.or_else(|| headers.get("last-event-id").and_then(|h| h.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok()));
    let rx = s.engine.subscribe();
    let full = s.engine.run_events(&id);
    let last_seq = full.last().map(|e| e.seq as i64).unwrap_or(-1);
    // An unknown run has nothing to wait for: close the stream instead of keeping it open forever.
    let finished = full.iter().any(|e| e.event.event.is_terminal()) || (full.is_empty() && s.engine.run_done(&id));
    let to_event = |ev: &aiw_core::events::TimedEvent, seq: u64| {
        Event::default().id(seq.to_string()).data(serde_json::to_string(ev).unwrap_or_default())
    };
    let replay_stream =
        stream::iter(full.iter().filter(|e| after.is_none_or(|a| e.seq > a)).map(|e| Ok(to_event(&e.event, e.seq))).collect::<Vec<_>>());
    let run_id = id.clone();
    let live = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(move |m| {
            let run_id = run_id.clone();
            async move {
                match m {
                    Ok(m) if m.run_id == run_id && (m.seq as i64) > last_seq => Some(m),
                    _ => None,
                }
            }
        })
        // The terminal event is delivered and the stream ends right behind it (the `None` marker),
        // instead of waiting for one more event of the run that never comes.
        .flat_map(|m| {
            let last = m.event.event.is_terminal();
            stream::iter(if last { vec![Some(m), None] } else { vec![Some(m)] })
        })
        .take_while(|m| futures::future::ready(m.is_some()))
        .filter_map(futures::future::ready)
        .map(move |m| Ok(to_event(&m.event, m.seq)));
    let stream: std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> =
        if finished { Box::pin(replay_stream) } else { Box::pin(replay_stream.chain(live)) };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// Trajectories as NDJSON, one session per line. Query: `agent`, `since`, `until`, `status`,
/// `limit`, `rated` (up | down | any), `redact=false` (secrets are masked by default), `exclude_path` (repeatable glob).
async fn traces_export(State(s): State<AppState>, axum::extract::RawQuery(q): axum::extract::RawQuery) -> Result<Response, ApiError> {
    let mut f = aiw_engine::traces::TraceFilter::default();
    let url = reqwest::Url::parse(&format!("http://localhost/?{}", q.unwrap_or_default()))
        .map_err(|_| ApiError(Error::Invalid("query non valida".into())))?;
    for (k, v) in url.query_pairs() {
        let v = v.into_owned();
        match k.as_ref() {
            "agent" if !v.is_empty() => f.agent = Some(v),
            "since" if !v.is_empty() => f.since = Some(v),
            "until" if !v.is_empty() => f.until = Some(v),
            "status" if !v.is_empty() => f.status = Some(v),
            "rated" if !v.is_empty() => f.rated = Some(v),
            "limit" if !v.is_empty() => {
                f.limit = Some(v.parse().map_err(|_| ApiError(Error::Invalid("limit deve essere un numero intero".into())))?)
            }
            "redact" => f.redact = !matches!(v.as_str(), "false" | "0" | "no"),
            "exclude_path" if !v.is_empty() => f.exclude_paths.push(v),
            _ => {}
        }
    }
    let body = aiw_engine::traces::to_jsonl(&s.engine.export_traces(&f)?)?;
    Ok(([(axum::http::header::CONTENT_TYPE, "application/x-ndjson")], body).into_response())
}

async fn approvals(State(s): State<AppState>) -> ApiResult<Vec<ApprovalRequest>> {
    ok(s.engine.approvals())
}
#[derive(Deserialize)]
struct ApprovalBody {
    decision: String,
    #[serde(default)]
    remember: bool,
}
async fn respond_approval(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<ApprovalBody>) -> ApiResult<Value> {
    s.engine.respond_approval(&id, b.decision == "allow", b.remember)?;
    ok(json!({}))
}
async fn webhook(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> ApiResult<Value> {
    let secret = headers.get("x-aiw-secret").and_then(|h| h.to_str().ok()).map(String::from).or_else(|| q.get("secret").cloned());
    let signature = headers.get("x-hub-signature-256").and_then(|h| h.to_str().ok());
    let run = s.engine.trigger_webhook(&id, secret.as_deref(), signature, &body).await?;
    ok(json!({"run_id": run.id, "status": run.status}))
}

/// Port and LAN exposure of the HTTP listener started by [`serve`] in this process. The router
/// also runs without it (the desktop shell over IPC, tests): then the settings are the reference.
static LISTENER: std::sync::OnceLock<(u16, bool)> = std::sync::OnceLock::new();

fn server_port(s: &AppState) -> u16 {
    match LISTENER.get() {
        Some(&(port, _)) => port,
        None => s.engine.config().effective.get("server.port").and_then(|v| v.as_u64()).unwrap_or(7777) as u16,
    }
}

fn lan_enabled(s: &AppState) -> bool {
    match LISTENER.get() {
        Some(&(_, lan)) => lan,
        None => s.engine.config().effective.get("server.lan").and_then(|v| v.as_bool()).unwrap_or(false),
    }
}

async fn mesh_state(State(s): State<AppState>) -> ApiResult<Value> {
    let address = mesh::lan_ip().filter(|_| lan_enabled(&s)).map(|ip| format!("http://{ip}:{}", server_port(&s)));
    ok(json!({
        "discovery": mesh::discovery_running(),
        "address": address,
        "discovered": mesh::discovered(),
        "peers": s.engine.peers(),
    }))
}

#[derive(Deserialize)]
struct MeshPair {
    address: String,
    code: String,
}
async fn mesh_pair(State(s): State<AppState>, Json(b): Json<MeshPair>) -> ApiResult<PeerDevice> {
    ok(s.engine.pair_with_peer(&b.address, &b.code).await?)
}
async fn mesh_remove_peer(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.remove_peer(&id).await?;
    ok(json!({}))
}

/// Pairing link + QR (only for this device: it carries the current code).
async fn pairing_qr(State(s): State<AppState>, req: Request) -> ApiResult<Value> {
    if !is_local(&req) {
        return Err(ApiError(Error::Denied("il QR di pairing si mostra solo sul dispositivo stesso".into())));
    }
    if !lan_enabled(&s) {
        return Err(ApiError(Error::Invalid("attiva l'accesso dalla LAN (server.lan) per accoppiare altri dispositivi".into())));
    }
    let ip = mesh::lan_ip().ok_or_else(|| ApiError(Error::Unsupported("nessuna interfaccia di rete locale".into())))?;
    let (url, svg) =
        mesh::pairing_qr(&format!("http://{ip}:{}", server_port(&s)), &s.engine.pairing_code()).map_err(|e| ApiError(Error::Runtime(e)))?;
    ok(json!({"url": url, "svg": svg}))
}

// ------------------------------------------------------------------ voice

/// Audio cap plus the base64 overhead of the JSON form used over the app's IPC bridge.
const VOICE_BODY_LIMIT: usize = aiw_engine::voice::MAX_AUDIO_BYTES / 3 * 4 + (1 << 20);

async fn voice_status(State(s): State<AppState>) -> ApiResult<VoiceStatus> {
    ok(s.engine.voice_status())
}

#[derive(Deserialize, Default)]
struct VoiceQuery {
    language: Option<String>,
}

/// `POST /api/voice/transcribe`: the raw audio (`Content-Type: audio/wav`…), or JSON
/// `{"audio": base64, "mime"?, "language"?}` for the app's IPC bridge, which carries JSON only.
async fn voice_transcribe(State(s): State<AppState>, Query(q): Query<VoiceQuery>, req: Request) -> Result<Json<Transcript>, Response> {
    use base64::Engine as _;
    let ct = req.headers().get(axum::http::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("audio/wav").to_string();
    let body = axum::body::to_bytes(req.into_body(), VOICE_BODY_LIMIT).await.map_err(|_| audio_too_large())?;
    let invalid = |m: String| ApiError(Error::Invalid(m)).into_response();
    let (audio, mime, language) = if ct.starts_with("application/json") {
        let v: Value =
            serde_json::from_slice(&body).map_err(|e| invalid(format!("JSON non valido: {}", extract::localize(&e.to_string()))))?;
        let b64 = v["audio"].as_str().ok_or_else(|| invalid("manca il campo audio (base64)".into()))?;
        let audio =
            base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| invalid("l'audio non è base64 valido".into()))?;
        (audio, v["mime"].as_str().unwrap_or("audio/wav").to_string(), v["language"].as_str().map(String::from).or(q.language))
    } else {
        (body.to_vec(), ct, q.language)
    };
    s.engine.transcribe(&audio, &mime, language.as_deref()).await.map(Json).map_err(|e| ApiError(e).into_response())
}

/// 413 in the API's error shape.
fn audio_too_large() -> Response {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(json!({"error": format!("l'audio supera il limite di {} MB", aiw_engine::voice::MAX_AUDIO_BYTES >> 20)})),
    )
        .into_response()
}

/// A multipart parsing error in the API's error shape (413 when the size limit was hit).
fn multipart_error(e: axum::extract::multipart::MultipartError) -> Response {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return audio_too_large();
    }
    ApiError(Error::Invalid("richiesta multipart non valida".into())).into_response()
}

/// `POST /v1/audio/transcriptions` (OpenAI multipart): other apps and paired devices use this
/// device's speech recognition. Such requests are never forwarded to another device.
async fn audio_transcriptions(
    State(s): State<AppState>,
    form: Result<axum::extract::Multipart, axum::extract::multipart::MultipartRejection>,
) -> Response {
    let Ok(mut form) = form else {
        return ApiError(Error::Invalid("serve una richiesta multipart/form-data con il campo file".into())).into_response();
    };
    let invalid = |m: &str| ApiError(Error::Invalid(m.into())).into_response();
    let (mut audio, mut mime, mut language, mut format) = (None, String::from("audio/wav"), None, String::from("json"));
    loop {
        let field = match form.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return multipart_error(e),
        };
        match field.name().unwrap_or_default() {
            "file" => {
                if let Some(ct) = field.content_type() {
                    mime = ct.to_string();
                }
                match field.bytes().await {
                    Ok(b) => audio = Some(b),
                    Err(e) => return multipart_error(e),
                }
            }
            "language" => match field.text().await {
                Ok(t) => language = Some(t),
                Err(e) => return multipart_error(e),
            },
            "response_format" => match field.text().await {
                Ok(t) => format = t,
                Err(e) => return multipart_error(e),
            },
            _ => {}
        }
    }
    let Some(audio) = audio else { return invalid("manca il campo file") };
    if aiw_runtimes::stt::wav_info(&audio).is_some() {
        mime = "audio/wav".into();
    }
    let t = match s.engine.transcribe_for_peer(&audio, &mime, language.as_deref()).await {
        Ok(t) => t,
        Err(e) => return ApiError(e).into_response(),
    };
    match format.as_str() {
        "text" => t.text.into_response(),
        "verbose_json" => Json(json!({
            "task": "transcribe",
            "language": t.language,
            "duration": t.duration_ms.map(|d| d as f64 / 1000.0),
            "text": t.text
        }))
        .into_response(),
        _ => Json(json!({ "text": t.text })).into_response(),
    }
}

async fn workflows(State(s): State<AppState>) -> ApiResult<Vec<WorkflowDef>> {
    ok(s.engine.workflows()?)
}
async fn workflow_runs(State(s): State<AppState>) -> ApiResult<Vec<WorkflowRun>> {
    ok(s.engine.workflow_runs())
}
async fn save_workflow(State(s): State<AppState>, Path(id): Path<String>, Json(mut w): Json<WorkflowDef>) -> ApiResult<WorkflowDef> {
    w.id = id;
    ok(s.engine.save_workflow(w)?)
}
async fn delete_workflow(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    ok(json!({"deleted": s.engine.delete_workflow(&id)?}))
}
#[derive(Deserialize, Default)]
struct RunWf {
    #[serde(default)]
    inputs: Map<String, Value>,
}
async fn run_workflow(State(s): State<AppState>, Path(id): Path<String>, body: Option<Json<RunWf>>) -> ApiResult<WorkflowRun> {
    ok(s.engine.run_workflow(&id, body.map(|b| b.0.inputs).unwrap_or_default(), "user").await?)
}
async fn eval_suites(State(s): State<AppState>) -> ApiResult<Vec<aiw_eval::Suite>> {
    ok(s.engine.eval_suites()?)
}
async fn eval_suite(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_eval::Suite> {
    ok(s.engine.eval_suite(&id)?)
}
/// The body is the suite as JSON, or `{"text": "<yaml|json>"}` as the editors send it; the id is the one in the path.
async fn save_eval_suite(State(s): State<AppState>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult<aiw_eval::Suite> {
    let mut suite = match body.get("text").and_then(Value::as_str) {
        Some(text) => aiw_eval::Suite::parse(text)?,
        None => aiw_eval::Suite::parse(&body.to_string())?,
    };
    if suite.id != id {
        return Err(ApiError(Error::Invalid(format!(
            "l'id della suite (`{}`) non corrisponde a quello dell'indirizzo (`{id}`)",
            suite.id
        ))));
    }
    // A `command` grader runs a shell command with the user's permissions, outside the agent's sandbox and without any
    // confirmation: whoever holds an API token must not be able to write one. Those suites come from files in `~/.aiw/evals`.
    if suite.tasks.iter().flat_map(|t| &t.graders).any(|g| g.kind() == "command") {
        return Err(ApiError(Error::Invalid(
            "i grader `command` eseguono comandi con i tuoi permessi: si scrivono solo in un file in ~/.aiw/evals o si lanciano dalla CLI"
                .into(),
        )));
    }
    suite.id = id;
    ok(s.engine.save_eval_suite(suite)?)
}
async fn delete_eval_suite(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.delete_eval_suite(&id)?;
    ok(json!({"deleted": true}))
}
#[derive(Deserialize, Default)]
struct StartEval {
    agent: Option<String>,
    profile: Option<String>,
    repeat: Option<u32>,
}
/// Starts the suite in the background; the answer is the report as it is (`running`), to be polled at `/api/eval-runs/{id}`.
async fn start_eval(State(s): State<AppState>, Path(id): Path<String>, body: Option<Json<StartEval>>) -> ApiResult<aiw_eval::EvalReport> {
    let b = body.map(|b| b.0).unwrap_or_default();
    let suite = s.engine.eval_suite(&id)?;
    let req = aiw_engine::eval::EvalRequest {
        agent: b.agent.filter(|a| !a.is_empty()),
        profile: b.profile.filter(|p| !p.is_empty()),
        repeat: b.repeat,
        ..Default::default()
    };
    ok(s.engine.start_eval(suite, req)?)
}
#[derive(Deserialize)]
struct RewardBody {
    task_id: String,
    output: String,
}
/// Scores an answer with the graders of a task (only the ones that look at the answer): the reward of a
/// reinforcement-learning trainer. Guarded like every other route.
async fn eval_reward(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<RewardBody>,
) -> ApiResult<aiw_engine::reward::RewardResult> {
    ok(s.engine.reward(&id, &b.task_id, &b.output).await?)
}
async fn agent_routing(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_core::api::RoutingStats> {
    ok(s.engine.routing_stats(&id)?)
}
async fn reset_agent_routing(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.reset_routing(&id)?;
    ok(json!({"reset": true}))
}
#[derive(Deserialize)]
struct StartOptimization {
    agent: String,
    variants: Option<u32>,
    profile: Option<String>,
    repeat: Option<u32>,
}
/// Starts the optimization of an agent's prompt in the background; the answer is the record as it is (`running`), to be
/// polled at `/api/prompt-optimizations/{id}`. Nothing is applied to the agent.
async fn start_optimization(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<StartOptimization>,
) -> ApiResult<PromptOptimization> {
    let suite = s.engine.eval_suite(&id)?;
    let opts =
        aiw_engine::optimize::OptimizeOptions { variants: b.variants, profile: b.profile.filter(|p| !p.is_empty()), repeat: b.repeat };
    ok(s.engine.start_prompt_optimization(suite, &b.agent, opts)?)
}
async fn prompt_optimizations(State(s): State<AppState>) -> ApiResult<Vec<PromptOptimization>> {
    ok(s.engine.prompt_optimizations()?)
}
async fn prompt_optimization(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<PromptOptimization> {
    ok(s.engine.prompt_optimization(&id)?)
}
#[derive(Deserialize)]
struct ApplyOptimization {
    /// Index in `variants` (from 0).
    variant: usize,
}
/// Saves the prompt of a variant as the system prompt of the agent: the only way a prompt changes, always asked for.
async fn apply_optimization(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<ApplyOptimization>,
) -> ApiResult<PromptOptimization> {
    ok(s.engine.apply_prompt_optimization(&id, b.variant)?)
}
async fn eval_runs(State(s): State<AppState>) -> ApiResult<Vec<aiw_eval::EvalReport>> {
    ok(s.engine.eval_reports()?)
}
async fn eval_run(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_eval::EvalReport> {
    ok(s.engine.eval_report(&id)?)
}
#[derive(Deserialize)]
struct CompareQuery {
    a: String,
    b: String,
}
async fn compare_eval_runs(State(s): State<AppState>, Query(q): Query<CompareQuery>) -> ApiResult<aiw_eval::Comparison> {
    // Only stored reports by id: a path from the network is not read.
    ok(aiw_eval::compare(&s.engine.eval_report(&q.a)?, &s.engine.eval_report(&q.b)?))
}
async fn trainers(State(s): State<AppState>) -> ApiResult<Vec<aiw_runtimes::trainer::TrainerInfo>> {
    ok(s.engine.trainers().await)
}
async fn trainer_install_plan(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_runtimes::install::InstallPlan> {
    ok(s.engine.trainer_install_plan(&id).await?)
}
async fn trainer_install_start(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_runtimes::install::InstallJob> {
    ok(s.engine.trainer_install_start(&id).await?)
}
async fn trainer_install_status(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Option<aiw_runtimes::install::InstallJob>> {
    ok(s.engine.trainer_install_status(&id))
}
async fn train_doctor(State(s): State<AppState>) -> ApiResult<aiw_engine::train::TrainDoctor> {
    ok(s.engine.train_doctor().await)
}
async fn datasets(State(s): State<AppState>) -> ApiResult<Vec<aiw_core::api::DatasetInfo>> {
    ok(s.engine.datasets()?)
}
async fn build_dataset(State(s): State<AppState>, Json(spec): Json<aiw_core::api::DatasetSpec>) -> ApiResult<aiw_core::api::DatasetInfo> {
    ok(s.engine.build_dataset(&spec)?)
}
#[derive(Deserialize)]
struct SftImport {
    name: String,
    jsonl: String,
}

async fn import_sft_dataset(State(s): State<AppState>, Json(input): Json<SftImport>) -> ApiResult<DatasetInfo> {
    ok(s.engine.import_sft_dataset(&input.name, &input.jsonl)?)
}

async fn dataset(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_core::api::DatasetInfo> {
    ok(s.engine.dataset(&id)?)
}
async fn delete_dataset(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.delete_dataset(&id)?;
    ok(json!({"deleted": true}))
}
async fn train_jobs(State(s): State<AppState>) -> ApiResult<Vec<aiw_core::api::TrainJob>> {
    ok(s.engine.train_jobs()?)
}
async fn start_train_job(State(s): State<AppState>, Json(req): Json<aiw_core::api::TrainJobRequest>) -> ApiResult<aiw_core::api::TrainJob> {
    ok(s.engine.start_train_job(req).await?)
}
async fn train_job(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_core::api::TrainJob> {
    ok(s.engine.train_job(&id)?)
}
#[derive(Deserialize)]
struct LogQuery {
    lines: Option<usize>,
}
async fn train_job_log(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<LogQuery>) -> ApiResult<Value> {
    let lines = q.lines.unwrap_or(aiw_engine::train_jobs::DEFAULT_LOG_LINES).clamp(1, 5000);
    ok(json!({"lines": s.engine.train_job_log(&id, lines)?}))
}
async fn cancel_train_job(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_core::api::TrainJob> {
    ok(s.engine.cancel_train_job(&id)?)
}
async fn adapters(State(s): State<AppState>) -> ApiResult<Vec<aiw_core::api::AdapterInfo>> {
    ok(s.engine.adapters()?)
}
async fn adapter(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<aiw_core::api::AdapterInfo> {
    ok(s.engine.adapter(&id)?)
}
async fn delete_adapter(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.delete_adapter(&id)?;
    ok(json!({"deleted": true}))
}
async fn tools(State(s): State<AppState>) -> ApiResult<Vec<ToolInfo>> {
    ok(s.engine.tools())
}
#[derive(Deserialize)]
struct ToolToggle {
    enabled: bool,
}
async fn set_tool(State(s): State<AppState>, Path(name): Path<String>, Json(b): Json<ToolToggle>) -> ApiResult<Value> {
    s.engine.set_tool_enabled(&name, b.enabled)?;
    ok(json!({}))
}
async fn mcp_servers(State(s): State<AppState>) -> ApiResult<Vec<McpServerInfo>> {
    ok(s.engine.mcp_servers())
}
async fn save_mcp(State(s): State<AppState>, Json(c): Json<McpServerConfig>) -> ApiResult<McpServerInfo> {
    ok(s.engine.save_mcp_server(c).await?)
}
async fn remove_mcp(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.remove_mcp_server(&id).await?;
    ok(json!({}))
}
async fn config(State(s): State<AppState>) -> ApiResult<ConfigView> {
    ok(s.engine.config())
}
#[derive(Deserialize)]
struct SetConfig {
    layer: String,
    key: String,
    value: Value,
}
async fn set_config(State(s): State<AppState>, Json(b): Json<SetConfig>) -> ApiResult<ConfigView> {
    ok(s.engine.set_config(&b.layer, &b.key, b.value)?)
}
async fn audit(State(s): State<AppState>) -> ApiResult<Vec<AuditEntry>> {
    ok(s.engine.audit())
}

/// Dispatches one request through the router without a socket (used by the Tauri shell).
pub async fn dispatch(app: Router, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    use tower::ServiceExt;
    let mut builder = axum::http::Request::builder().method(method).uri(path);
    let body = match body {
        Some(v) if !v.is_null() => {
            builder = builder.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&v).unwrap_or_default())
        }
        _ => Body::empty(),
    };
    let req = match builder.body(body) {
        Ok(r) => r,
        Err(e) => return (400, json!({"error": e.to_string()})),
    };
    match app.oneshot(req).await {
        Ok(res) => {
            let status = res.status().as_u16();
            let bytes = axum::body::to_bytes(res.into_body(), 64 << 20).await.unwrap_or_default();
            let v = serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
            (status, v)
        }
        Err(e) => (500, json!({"error": e.to_string()})),
    }
}

/// Binds and serves until the process stops. LAN exposure follows `server.lan`.
pub async fn serve(engine: Engine, port: u16, lan: bool, ui_dir: Option<PathBuf>) -> std::io::Result<()> {
    let host = if lan { "0.0.0.0" } else { "127.0.0.1" };
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    let port = listener.local_addr()?.port();
    let _ = LISTENER.set((port, lan));
    engine.set_local_url(format!("http://127.0.0.1:{port}"));
    tracing::info!("AI Workspace Local in ascolto su http://{host}:{port} (LAN: {lan})");
    if lan {
        mesh::start(&engine, port);
    }
    let app = router(engine, ui_dir);
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_errors;
#[cfg(test)]
mod tests_eval;
#[cfg(test)]
mod tests_kb;
#[cfg(test)]
mod tests_learning;
#[cfg(test)]
mod tests_memory;
#[cfg(test)]
mod tests_models;
#[cfg(test)]
mod tests_optimize;
#[cfg(test)]
mod tests_proactive;
#[cfg(test)]
mod tests_recommend;
#[cfg(test)]
mod tests_remote;
#[cfg(test)]
mod tests_security;
#[cfg(test)]
mod tests_traces;
#[cfg(test)]
mod tests_train;
