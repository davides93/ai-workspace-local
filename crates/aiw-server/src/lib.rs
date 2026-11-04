//! HTTP surface of the engine (architecture §8). The same [`router`] serves `aiw serve` over TCP
//! and the Tauri shells in-process (tower `oneshot`), so every client sees one API.

mod a2a;
mod anthropic;
mod extract;
mod gateway;
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
use axum::routing::{delete, get, post, put};
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
        .route("/api/runtimes", get(runtimes))
        .route("/api/runtimes/refresh", post(runtimes_refresh))
        .route("/api/runtimes/{id}/settings", put(runtime_settings))
        .route("/api/runtimes/{id}/logs", get(runtime_logs))
        .route("/api/runtimes/{id}/{action}", post(runtime_action))
        .route("/api/models", get(models))
        .route("/api/models/catalog", get(catalog))
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
        .route("/api/policy", get(get_policy).put(put_policy))
        .route("/api/hooks", get(get_hooks).put(put_hooks))
        .route("/api/sessions", get(sessions).post(create_session))
        .route("/api/sessions/{id}", get(get_session).patch(patch_session).delete(delete_session))
        .route("/api/sessions/{id}/worktree", get(worktree_status).delete(worktree_discard))
        .route("/api/sessions/{id}/worktree/apply", post(worktree_apply))
        .route("/api/sessions/{id}/messages", get(messages))
        .route("/api/sessions/{id}/active-run", get(active_run))
        .route("/api/sessions/{id}/runs", post(start_run))
        .route("/api/runs/{id}/cancel", post(cancel_run))
        .route("/api/runs/{id}/events", get(run_events_sse))
        .route("/api/runs/{id}/events.json", get(run_events_json))
        .route("/api/approvals", get(approvals))
        .route("/api/approvals/{id}", post(respond_approval))
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
        .route("/v1/models", get(gateway::models))
        .route("/v1/chat/completions", post(gateway::chat_completions))
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
async fn runtime_logs(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<String>> {
    ok(s.engine.runtime_logs(&id).await)
}
async fn models(State(s): State<AppState>) -> ApiResult<Vec<LocalModel>> {
    ok(s.engine.models())
}
async fn catalog(State(s): State<AppState>) -> ApiResult<Vec<CatalogModel>> {
    ok(s.engine.catalog())
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
async fn delete_session(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.delete_session(&id)?;
    ok(json!({}))
}
async fn messages(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<ChatMessage>> {
    ok(s.engine.messages(&id)?)
}
async fn active_run(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    s.engine.session(&id)?; // an unknown session is a 404, not "no run"
    ok(json!({"run_id": s.engine.active_run(&id)}))
}
#[derive(Deserialize)]
struct RunBody {
    input: String,
    /// a2ui.action when a surface sends a user action
    kind: Option<String>,
}
async fn start_run(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<RunBody>) -> ApiResult<Value> {
    ok(json!({"run_id": s.engine.start_run_with(&id, b.input, b.kind.as_deref()).await?}))
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
mod tests_models;
#[cfg(test)]
mod tests_security;
