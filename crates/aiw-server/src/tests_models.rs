//! Model hub over HTTP: providers, live search and variants against a local fake Hugging Face,
//! then pull → downloads → models → delete.

use super::*;
use aiw_engine::EngineOptions;
use axum::extract::{Path as AxPath, Query as AxQuery, State as AxState};
use axum::http::HeaderMap;
use axum::routing::get as ax_get;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// A minimal GGUF header (Qwen3 Test, Q4_K_M) padded to `n` bytes.
fn gguf(n: usize) -> Vec<u8> {
    fn kv(b: &mut Vec<u8>, key: &str, ty: u32, val: &[u8]) {
        b.extend((key.len() as u64).to_le_bytes());
        b.extend(key.as_bytes());
        b.extend(ty.to_le_bytes());
        b.extend(val);
    }
    fn string(s: &str) -> Vec<u8> {
        let mut v = (s.len() as u64).to_le_bytes().to_vec();
        v.extend(s.as_bytes());
        v
    }
    let mut b = b"GGUF".to_vec();
    b.extend(3u32.to_le_bytes());
    b.extend(0u64.to_le_bytes());
    b.extend(4u64.to_le_bytes());
    kv(&mut b, "general.architecture", 8, &string("qwen3"));
    kv(&mut b, "general.name", 8, &string("Qwen3 Test"));
    kv(&mut b, "qwen3.context_length", 4, &40960u32.to_le_bytes());
    kv(&mut b, "general.file_type", 4, &15u32.to_le_bytes());
    b.resize(n.max(b.len()), 7);
    b
}

type Hit = (String, Option<String>);

#[derive(Clone)]
struct Hf {
    /// `(repo, path)` → bytes; a repo is listed when it has files.
    files: Arc<HashMap<(String, String), Vec<u8>>>,
    /// Sizes the tree claims instead of the real one (a download that cannot fit).
    claimed: Arc<HashMap<(String, String), u64>>,
    /// `(url path, authorization)` of every request.
    log: Arc<Mutex<Vec<Hit>>>,
    /// While set, the Q8_0 file of acme/tiny-GGUF stalls after its first bytes.
    stall: Arc<std::sync::atomic::AtomicBool>,
}

const GOOD_TOKEN: &str = "hf_good_token";

fn auth(h: &HeaderMap) -> Option<String> {
    h.get("authorization").and_then(|v| v.to_str().ok()).map(String::from)
}

async fn hf_search(AxState(f): AxState<Hf>, AxQuery(q): AxQuery<HashMap<String, String>>, h: HeaderMap) -> axum::response::Response {
    f.log.lock().unwrap().push(("search".into(), auth(&h)));
    assert_eq!(q.get("filter").map(String::as_str), Some("gguf"));
    assert_eq!(q.get("direction").map(String::as_str), Some("-1"));
    let all = [
        json!({"id": "acme/tiny-GGUF", "downloads": 500, "likes": 9, "tags": ["gguf", "text-generation"], "pipeline_tag": "text-generation", "library_name": "gguf"}),
        json!({"id": "acme/other-GGUF", "downloads": 100, "likes": 1, "tags": ["gguf"], "pipeline_tag": "text-generation"}),
    ];
    let limit: usize = q.get("limit").and_then(|l| l.parse().ok()).unwrap_or(20);
    let page: usize = q.get("page").and_then(|l| l.parse().ok()).unwrap_or(0);
    let mut resp = Json(Value::Array(all.iter().skip(page * limit).take(limit).cloned().collect())).into_response();
    if (page + 1) * limit < all.len() {
        resp.headers_mut().insert(
            "link",
            format!("<https://huggingface.co/api/models?limit={limit}&page={}&filter=gguf&direction=-1>; rel=\"next\"", page + 1)
                .parse()
                .unwrap(),
        );
    }
    resp
}

async fn hf_info(AxState(f): AxState<Hf>, AxPath((o, n)): AxPath<(String, String)>, h: HeaderMap) -> axum::response::Response {
    let repo = format!("{o}/{n}");
    f.log.lock().unwrap().push((format!("info {repo}"), auth(&h)));
    if !f.files.keys().any(|(r, _)| *r == repo) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if repo == "acme/gated-GGUF" && auth(&h).as_deref() != Some(&format!("Bearer {GOOD_TOKEN}")) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(json!({"id": repo, "sha": "feedface00000000000000000000000000000000", "gated": repo == "acme/gated-GGUF",
        "tags": ["gguf", "text-generation"], "pipeline_tag": "text-generation", "library_name": "gguf", "downloads": 500, "likes": 9}))
    .into_response()
}

async fn hf_tree(AxState(f): AxState<Hf>, AxPath((o, n, _rev)): AxPath<(String, String, String)>) -> axum::response::Response {
    let repo = format!("{o}/{n}");
    let mut v: Vec<Value> = f
        .files
        .iter()
        .filter(|((r, _), _)| *r == repo)
        .map(|((r, p), b)| {
            let size = f.claimed.get(&(r.clone(), p.clone())).copied().unwrap_or(b.len() as u64);
            json!({"type": "file", "oid": "gitsha", "size": 100, "path": p, "lfs": {"oid": sha(b), "size": size}})
        })
        .collect();
    v.sort_by_key(|e| e["path"].as_str().unwrap_or_default().to_string());
    Json(Value::Array(v)).into_response()
}

async fn hf_resolve(
    AxState(f): AxState<Hf>,
    AxPath((o, n, _rev, file)): AxPath<(String, String, String, String)>,
    h: HeaderMap,
) -> axum::response::Response {
    let repo = format!("{o}/{n}");
    f.log.lock().unwrap().push((format!("resolve {repo}/{file}"), auth(&h)));
    if repo == "acme/tiny-GGUF" && file == "tiny-Q8_0.gguf" {
        if let Some(r) = stall_q8_body(&f).await {
            return r;
        }
    }
    match f.files.get(&(repo, file)) {
        Some(b) => {
            let start = h
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.strip_suffix('-'))
                .and_then(|v| v.parse::<usize>().ok());
            match start {
                Some(s) if s < b.len() => {
                    (StatusCode::PARTIAL_CONTENT, [("content-range", format!("bytes {s}-{}/{}", b.len() - 1, b.len()))], b[s..].to_vec())
                        .into_response()
                }
                Some(_) => (StatusCode::RANGE_NOT_SATISFIABLE, [("content-range", format!("bytes */{}", b.len()))]).into_response(),
                None => (StatusCode::OK, b.clone()).into_response(),
            }
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn hf_whoami(h: HeaderMap) -> axum::response::Response {
    if auth(&h).as_deref() == Some(&format!("Bearer {GOOD_TOKEN}")) {
        Json(json!({"name": "tester"})).into_response()
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

async fn fake_hf() -> (String, Hf, Vec<u8>, Vec<u8>) {
    let q4 = gguf(6000);
    let q8 = gguf(9000);
    let mut files = HashMap::new();
    for (repo, path, b) in [
        ("acme/tiny-GGUF", "tiny-Q4_K_M.gguf", q4.clone()),
        ("acme/tiny-GGUF", "tiny-Q8_0.gguf", q8.clone()),
        ("acme/tiny-GGUF", "README.md", b"# tiny".to_vec()),
        ("acme/gated-GGUF", "gated-Q4_K_M.gguf", q4.clone()),
        ("acme/huge-GGUF", "huge-Q4_K_M.gguf", q4.clone()),
    ] {
        files.insert((repo.to_string(), path.to_string()), b);
    }
    let mut claimed = HashMap::new();
    claimed.insert(("acme/huge-GGUF".to_string(), "huge-Q4_K_M.gguf".to_string()), 1u64 << 50);
    let hf = Hf { files: Arc::new(files), claimed: Arc::new(claimed), log: Default::default(), stall: Default::default() };
    let app = Router::new()
        .route("/api/models", ax_get(hf_search))
        .route("/api/whoami-v2", ax_get(hf_whoami))
        .route("/api/models/{o}/{n}", ax_get(hf_info))
        .route("/api/models/{o}/{n}/tree/{rev}", ax_get(hf_tree))
        .route("/{o}/{n}/resolve/{rev}/{*file}", ax_get(hf_resolve))
        .with_state(hf.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (format!("http://{addr}"), hf, q4, q8)
}

async fn app() -> (Router, Engine, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let e = Engine::new(EngineOptions {
        data_dir: Some(data.path().into()),
        workspace_root: Some(data.path().join("ws")),
        mode: "daemon".into(),
        offline: true,
        ..Default::default()
    })
    .await
    .unwrap();
    (router(e.clone(), None), e, data)
}

async fn wait_download(app: &Router, id: &str, status: &str) -> Value {
    for _ in 0..300 {
        let (_, list) = dispatch(app.clone(), "GET", "/api/models/downloads", None).await;
        if let Some(d) = list.as_array().unwrap().iter().find(|d| d["id"] == id) {
            if d["status"] == status {
                return d.clone();
            }
            assert!(!matches!(d["status"].as_str(), Some("error" | "cancelled")) || d["status"] == status, "download ended as {d}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    panic!("download {id} never reached {status}");
}

#[tokio::test]
async fn lists_providers_and_keeps_tokens_secret() {
    let (app, e, _d) = app().await;
    let (st, v) = dispatch(app.clone(), "GET", "/api/models/providers", None).await;
    assert_eq!(st, 200);
    let ids: Vec<&str> = v.as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["catalog", "hf", "modelscope", "ollama", "url"]);
    let hf = &v[1];
    assert_eq!((hf["auth"].as_str(), hf["auth_configured"].as_bool()), (Some("optional"), Some(false)));
    assert!(hf["capabilities"].as_array().unwrap().iter().any(|c| c == "multi_file"));
    assert!(hf["endpoint"].as_str().unwrap().starts_with("http"));

    // The token is validated with the provider: this endpoint is down, so it cannot refuse.
    let (st, v) = dispatch(
        app.clone(),
        "PUT",
        "/api/models/providers/hf/auth",
        Some(json!({"token": GOOD_TOKEN, "endpoint": "http://127.0.0.1:1/"})),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!((v["auth_configured"].as_bool(), v["endpoint"].as_str()), (Some(true), Some("http://127.0.0.1:1")));
    assert!(v.get("token").is_none() && !v.to_string().contains(GOOD_TOKEN));
    let (_, list) = dispatch(app.clone(), "GET", "/api/models/providers", None).await;
    assert!(!list.to_string().contains(GOOD_TOKEN));
    // Only the endpoint changes when `token` is absent; null clears the token.
    let (_, v) = dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"endpoint": "http://127.0.0.1:2"}))).await;
    assert_eq!((v["auth_configured"].as_bool(), v["endpoint"].as_str()), (Some(true), Some("http://127.0.0.1:2")));
    let (_, v) = dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"token": null, "endpoint": ""}))).await;
    assert_eq!((v["auth_configured"].as_bool(), v["endpoint"].as_str().map(|e| e.starts_with("https://"))), (Some(false), Some(true)));
    // Errors.
    let (st, _) = dispatch(app.clone(), "PUT", "/api/models/providers/nope/auth", Some(json!({"token": "x"}))).await;
    assert_eq!(st, 404);
    let (st, _) = dispatch(app.clone(), "PUT", "/api/models/providers/ollama/auth", Some(json!({"token": "x"}))).await;
    assert_eq!(st, 400);
    let (st, _) = dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"endpoint": "ftp://x"}))).await;
    assert_eq!(st, 400);
    drop(e);
}

#[tokio::test]
async fn a_token_the_provider_refuses_is_not_stored() {
    let (app, _e, _d) = app().await;
    let (base, _fake, _, _) = fake_hf().await;
    let (st, _) = dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"endpoint": base}))).await;
    assert_eq!(st, 200);
    let (st, v) = dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"token": "hf_wrong"}))).await;
    assert_eq!(st, 401, "{v}");
    assert!(v["error"].as_str().unwrap().contains("token"));
    let (_, list) = dispatch(app.clone(), "GET", "/api/models/providers", None).await;
    assert_eq!(list[1]["auth_configured"], false);
    let (st, _) = dispatch(app, "PUT", "/api/models/providers/hf/auth", Some(json!({"token": GOOD_TOKEN}))).await;
    assert_eq!(st, 200);
}

#[tokio::test]
async fn searches_a_local_hugging_face_and_pages_through_the_results() {
    let (app, _e, _d) = app().await;
    let (base, fake, _, _) = fake_hf().await;
    dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"endpoint": base}))).await;
    let (st, page) = dispatch(app.clone(), "GET", "/api/models/search?provider=hf&q=tiny&format=gguf&sort=downloads&limit=1", None).await;
    assert_eq!(st, 200, "{page}");
    assert_eq!(page["items"][0]["id"], "acme/tiny-GGUF");
    assert_eq!(page["items"][0]["provider"], "hf");
    assert_eq!(page["items"][0]["formats"], json!(["gguf"]));
    assert_eq!(page["items"][0]["capabilities"], json!(["chat"]));
    let cursor = page["next_cursor"].as_str().unwrap().to_string();
    assert!(cursor.starts_with(&base), "{cursor}");
    let (st, page2) =
        dispatch(app.clone(), "GET", &format!("/api/models/search?provider=hf&format=gguf&limit=1&cursor={}", urlenc(&cursor)), None).await;
    assert_eq!(st, 200, "{page2}");
    assert_eq!(page2["items"][0]["id"], "acme/other-GGUF");
    assert!(page2.get("next_cursor").is_none());
    assert!(fake.log.lock().unwrap().iter().all(|(_, a)| a.is_none()), "no token configured: nothing sent");
    // Other providers and errors.
    let (st, cat) = dispatch(app.clone(), "GET", "/api/models/search?provider=catalog&q=qwen3%20coder", None).await;
    assert_eq!((st, cat["items"][0]["id"].as_str()), (200, Some("qwen3-coder-30b-a3b")));
    let (st, ol) = dispatch(app.clone(), "GET", "/api/models/search?provider=ollama&q=gpt-oss", None).await;
    assert_eq!((st, ol["items"][0]["id"].as_str()), (200, Some("gpt-oss")));
    assert_eq!(dispatch(app.clone(), "GET", "/api/models/search?provider=nope&q=x", None).await.0, 404);
    assert_eq!(dispatch(app.clone(), "GET", "/api/models/search?q=x", None).await.0, 400);
    assert_eq!(dispatch(app.clone(), "GET", "/api/models/search?provider=hf&q=x&sort=bogus", None).await.0, 400);
    // A Hub that is down is a 502, not a 500.
    dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"endpoint": "http://127.0.0.1:1"}))).await;
    assert_eq!(dispatch(app, "GET", "/api/models/search?provider=hf&q=x", None).await.0, 502);
}

fn urlenc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

#[tokio::test]
async fn pull_download_list_and_delete_a_hub_model() {
    let (app, _e, data) = app().await;
    let (base, fake, q4, _q8) = fake_hf().await;
    dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"endpoint": base}))).await;

    // Variants for this device.
    let (st, d) = dispatch(app.clone(), "GET", "/api/models/remote?provider=hf&model=acme/tiny-GGUF", None).await;
    assert_eq!(st, 200, "{d}");
    assert_eq!(d["revision"], "feedface00000000000000000000000000000000");
    let variants = d["variants"].as_array().unwrap();
    assert_eq!(variants.iter().map(|v| v["id"].as_str().unwrap()).collect::<Vec<_>>(), ["Q4_K_M", "Q8_0"]);
    assert_eq!(variants.iter().filter(|v| v["recommended"] == true).count(), 1);
    assert_eq!(variants[0]["recommended"], true, "Q4_K_M is preferred at equal fit");
    for v in variants {
        assert!(["great", "ok", "tight", "no"].contains(&v["fit"].as_str().unwrap()));
        assert!(!v["fit_detail"].as_str().unwrap().is_empty());
        assert!(v["runtimes"].as_array().unwrap().iter().any(|r| r["id"] == "llama-server"));
        assert_eq!(v["installed"], false);
        assert_eq!(v["files"][0]["role"], "weights");
    }
    assert_eq!(variants[0]["size_bytes"], q4.len());
    assert_eq!(variants[0]["files"][0]["sha256"], sha(&q4));

    // Pull it. Empty variant would pick the recommended one; here it is explicit.
    let (st, p) =
        dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({"provider": "hf", "model": "acme/tiny-GGUF", "variant": "Q4_K_M"})))
            .await;
    assert_eq!(st, 200, "{p}");
    assert!(["queued", "running", "done"].contains(&p["status"].as_str().unwrap()));
    assert_eq!(
        (p["provider"].as_str(), p["model_ref"].as_str(), p["variant"].as_str()),
        (Some("hf"), Some("acme/tiny-GGUF"), Some("Q4_K_M"))
    );
    assert_eq!(p["files_total"], 1);
    let id = p["id"].as_str().unwrap().to_string();
    let done = wait_download(&app, &id, "done").await;
    assert_eq!(
        (done["files_done"].as_u64(), done["received"].as_u64(), done["total"].as_u64()),
        (Some(1), Some(q4.len() as u64), Some(q4.len() as u64))
    );
    // `dest` is a real OS path: compared by components, not as a '/' string (Windows uses '\\').
    assert!(std::path::Path::new(done["dest"].as_str().unwrap()).ends_with("acme/tiny-GGUF"));
    assert!(data.path().join("models/hf/acme/tiny-GGUF/tiny-Q4_K_M.gguf").is_file());
    assert!(data.path().join("models/hf/acme/tiny-GGUF/.aiw-model.json").is_file());
    assert!(fake.log.lock().unwrap().iter().any(|(p, _)| p == "resolve acme/tiny-GGUF/tiny-Q4_K_M.gguf"));
    assert!(
        !fake.log.lock().unwrap().iter().any(|(p, _)| p.contains("README") || p.contains("Q8_0")),
        "only the chosen variant is fetched"
    );

    // The model list has it, with provenance.
    let (_, models) = dispatch(app.clone(), "GET", "/api/models", None).await;
    let m = models.as_array().unwrap().iter().find(|m| m["origin"] == "aiw").unwrap_or_else(|| panic!("{models}")).clone();
    assert_eq!(
        (m["removable"].as_bool(), m["format"].as_str(), m["quant"].as_str(), m["name"].as_str()),
        (Some(true), Some("gguf"), Some("Q4_K_M"), Some("Qwen3 Test"))
    );
    assert!(m["source"].as_str().unwrap().starts_with("hf:acme/tiny-GGUF@feedface"));
    assert!(m["runtime_compat"].as_array().unwrap().len() >= 3);
    assert!(!m["fit"].as_str().unwrap().is_empty());
    let (_, d) = dispatch(app.clone(), "GET", "/api/models/remote?provider=hf&model=acme/tiny-GGUF", None).await;
    assert_eq!((d["variants"][0]["installed"].as_bool(), d["variants"][1]["installed"].as_bool()), (Some(true), Some(false)));

    // Delete it (the id may contain slashes in other cases: a catch-all route takes them).
    let mid = m["id"].as_str().unwrap().to_string();
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/models/{mid}"), None).await.0, 204);
    assert!(!data.path().join("models/hf").exists());
    let (_, models) = dispatch(app.clone(), "GET", "/api/models", None).await;
    assert!(models.as_array().unwrap().is_empty());
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/models/{mid}"), None).await.0, 404);
    // The download stays in the list until cleared; cancelling a finished one is harmless.
    let (st, c) = dispatch(app.clone(), "POST", &format!("/api/models/downloads/{id}/cancel"), None).await;
    assert_eq!((st, c["status"].as_str()), (200, Some("done")));
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/models/downloads/{id}"), None).await.0, 204);
    let (_, list) = dispatch(app.clone(), "GET", "/api/models/downloads", None).await;
    assert!(list.as_array().unwrap().is_empty());
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/models/downloads/{id}"), None).await.0, 404);
    assert_eq!(dispatch(app, "POST", "/api/models/downloads/dl_nope/cancel", None).await.0, 404);
}

#[tokio::test]
async fn v03_pull_shapes_keep_working_and_bad_ones_are_rejected() {
    let (app, e, data) = app().await;
    let (base, _fake, _q4, q8) = fake_hf().await;
    dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"endpoint": base.clone()}))).await;
    // {repo, file}
    let (st, p) =
        dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({"repo": "acme/tiny-GGUF", "file": "tiny-Q8_0.gguf"}))).await;
    assert_eq!(st, 200, "{p}");
    let done = wait_download(&app, p["id"].as_str().unwrap(), "done").await;
    assert_eq!(done["received"], q8.len());
    assert!(data.path().join("models/hf/acme/tiny-GGUF/tiny-Q8_0.gguf").is_file());
    // {url}: a single file from any host (here the fake's resolve route).
    let url = format!("{base}/acme/tiny-GGUF/resolve/main/tiny-Q4_K_M.gguf");
    let (st, p) = dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({"url": url, "file_name": "direct-Q4_K_M.gguf"}))).await;
    assert_eq!(st, 200, "{p}");
    wait_download(&app, p["id"].as_str().unwrap(), "done").await;
    let models = e.models();
    assert!(models.iter().any(|m| m.source.as_deref().is_some_and(|s| s.starts_with("url:"))), "{models:#?}");
    // Rejected requests.
    assert_eq!(dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({"nonsense": true}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({"id": "nope"}))).await.0, 404);
    assert_eq!(dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({"id": "apple-fm-system"}))).await.0, 400);
    assert_eq!(dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({"repo": "../x", "file": "a.gguf"}))).await.0, 400);
    assert_eq!(
        dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({"provider": "hf", "model": "acme/tiny-GGUF", "variant": "Q1"})))
            .await
            .0,
        404
    );
    assert_eq!(
        dispatch(
            app.clone(),
            "POST",
            "/api/models/pull",
            Some(json!({"provider": "hf", "model": "acme/missing-GGUF", "variant": "Q4_K_M"}))
        )
        .await
        .0,
        404
    );
    assert_eq!(dispatch(app, "POST", "/api/models/pull", Some(json!({"url": "file:///etc/passwd"}))).await.0, 400);
}

#[tokio::test]
async fn two_direct_urls_of_the_same_host_are_two_models_deleted_independently() {
    let (app, e, data) = app().await;
    let (base, _fake, q4, q8) = fake_hf().await;
    // Both files are served by the same host and port: the layout used to be one folder per host
    // and one record per variant name (`file`), so the second download replaced the first and
    // deleting either deleted both.
    let url_a = format!("{base}/acme/tiny-GGUF/resolve/main/tiny-Q4_K_M.gguf");
    let url_b = format!("{base}/acme/tiny-GGUF/resolve/main/tiny-Q8_0.gguf");
    for url in [&url_a, &url_b] {
        let (st, p) = dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({ "url": url }))).await;
        assert_eq!(st, 200, "{p}");
        wait_download(&app, p["id"].as_str().unwrap(), "done").await;
    }
    let (_, models) = dispatch(app.clone(), "GET", "/api/models", None).await;
    let models = models.as_array().unwrap();
    assert_eq!(models.len(), 2, "{models:#?}");
    let by_source = |url: &str| {
        models
            .iter()
            .find(|m| m["source"].as_str().is_some_and(|s| s.starts_with(&format!("url:{url}"))))
            .unwrap_or_else(|| panic!("{url} is not listed: {models:#?}"))
            .clone()
    };
    let (a, b) = (by_source(&url_a), by_source(&url_b));
    assert_eq!((a["size_bytes"].as_u64(), b["size_bytes"].as_u64()), (Some(q4.len() as u64), Some(q8.len() as u64)));
    let (path_a, path_b) = (std::path::PathBuf::from(a["path"].as_str().unwrap()), std::path::PathBuf::from(b["path"].as_str().unwrap()));
    assert_ne!(path_a.parent(), path_b.parent(), "one folder per URL");
    assert_eq!(path_a.parent().unwrap().parent(), path_b.parent().unwrap().parent(), "below the same host");
    assert_eq!(std::fs::read(&path_a).unwrap(), q4);
    assert_eq!(std::fs::read(&path_b).unwrap(), q8);

    // Deleting one leaves the other, files and record.
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/models/{}", a["id"].as_str().unwrap()), None).await.0, 204);
    assert!(!path_a.exists() && path_b.is_file());
    let (_, left) = dispatch(app.clone(), "GET", "/api/models", None).await;
    assert_eq!(left.as_array().unwrap().len(), 1);
    assert_eq!(left[0]["id"], b["id"]);
    assert!(data.path().join("models/url").exists());
    // The remote model still says which of the two is installed.
    let (_, d) = dispatch(
        app.clone(),
        "GET",
        &format!("/api/models/remote?provider=url&model={}", url_b.replace(':', "%3A").replace('/', "%2F")),
        None,
    )
    .await;
    assert_eq!(d["variants"][0]["installed"], true, "{d}");
    let (_, d) = dispatch(
        app.clone(),
        "GET",
        &format!("/api/models/remote?provider=url&model={}", url_a.replace(':', "%3A").replace('/', "%2F")),
        None,
    )
    .await;
    assert_eq!(d["variants"][0]["installed"], false, "{d}");
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/models/{}", b["id"].as_str().unwrap()), None).await.0, 204);
    assert!(!data.path().join("models/url").exists(), "the last one takes the empty folders with it");
    assert!(e.models().is_empty());
}

#[tokio::test]
async fn gated_repositories_answer_401_until_a_token_is_set_and_it_only_goes_to_the_hub() {
    let (app, _e, data) = app().await;
    let (base, fake, _, _) = fake_hf().await;
    dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"endpoint": base}))).await;
    let (st, v) = dispatch(app.clone(), "GET", "/api/models/remote?provider=hf&model=acme/gated-GGUF", None).await;
    assert_eq!(st, 401, "{v}");
    assert!(v["error"].as_str().unwrap().contains("token Hugging Face"));
    let (st, _) =
        dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({"provider": "hf", "model": "acme/gated-GGUF", "variant": "Q4_K_M"})))
            .await;
    assert_eq!(st, 401);
    let (st, _) = dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"token": GOOD_TOKEN}))).await;
    assert_eq!(st, 200);
    let (st, p) =
        dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({"provider": "hf", "model": "acme/gated-GGUF", "variant": "Q4_K_M"})))
            .await;
    assert_eq!(st, 200, "{p}");
    wait_download(&app, p["id"].as_str().unwrap(), "done").await;
    assert!(data.path().join("models/hf/acme/gated-GGUF/gated-Q4_K_M.gguf").is_file());
    let log = fake.log.lock().unwrap().clone();
    let resolve = log.iter().find(|(p, _)| p.starts_with("resolve acme/gated-GGUF")).unwrap();
    assert_eq!(resolve.1.as_deref(), Some(format!("Bearer {GOOD_TOKEN}").as_str()), "the Hub's own host receives the token");
}

#[tokio::test]
async fn a_download_that_cannot_fit_is_refused_with_507() {
    let (app, _e, data) = app().await;
    let (base, fake, _, _) = fake_hf().await;
    dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"endpoint": base}))).await;
    let (st, v) =
        dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({"provider": "hf", "model": "acme/huge-GGUF", "variant": "Q4_K_M"})))
            .await;
    assert_eq!(st, 507, "{v}");
    assert!(v["error"].as_str().unwrap().contains("spazio insufficiente"));
    assert!(!fake.log.lock().unwrap().iter().any(|(p, _)| p.starts_with("resolve")), "nothing was downloaded");
    let (_, list) = dispatch(app, "GET", "/api/models/downloads", None).await;
    assert!(list.as_array().unwrap().is_empty());
    assert!(!data.path().join("models/hf/acme/huge-GGUF").exists());
}

#[tokio::test]
async fn catalog_pull_installs_the_repository_and_marks_the_card() {
    let (app, e, data) = app().await;
    let (base, _fake, q4, _) = fake_hf().await;
    dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"endpoint": base}))).await;
    // The curated entry points at Qwen/Qwen3-4B-GGUF: serve it from the fake under that name.
    let (st, c) = dispatch(app.clone(), "GET", "/api/models/catalog", None).await;
    assert_eq!(st, 200);
    let card = c.as_array().unwrap().iter().find(|m| m["id"] == "qwen3-4b").unwrap();
    assert_eq!((card["installed"].as_bool(), card["downloadable"].as_bool()), (Some(false), Some(true)));
    assert!(card["runtime_compat"].as_array().unwrap().iter().any(|r| r["id"] == "llama-server"));
    // The fake knows no `Qwen/Qwen3-4B-GGUF`: the catalog pull reports the Hub's 404.
    let (st, v) = dispatch(app.clone(), "POST", "/api/models/pull", Some(json!({"id": "qwen3-4b"}))).await;
    assert_eq!(st, 404, "{v}");
    // A v0.3 file placed by hand is recognised by its exact file name.
    std::fs::write(data.path().join("models/Qwen3-4B-Q4_K_M.gguf"), &q4).unwrap();
    e.models();
    let (_, c) = dispatch(app, "GET", "/api/models/catalog", None).await;
    assert_eq!(c.as_array().unwrap().iter().find(|m| m["id"] == "qwen3-4b").unwrap()["installed"], true);
}

/// The Q8_0 file of `acme/tiny-GGUF` sends its first 1000 bytes and then stalls.
async fn stall_q8_body(f: &Hf) -> Option<axum::response::Response> {
    if !f.stall.load(std::sync::atomic::Ordering::SeqCst) {
        return None;
    }
    let b = f.files.get(&("acme/tiny-GGUF".to_string(), "tiny-Q8_0.gguf".to_string()))?.clone();
    let first = b[..1000].to_vec();
    let stream = futures::stream::unfold(0u8, move |i| {
        let first = first.clone();
        async move {
            if i == 0 {
                Some((Ok::<_, std::io::Error>(axum::body::Bytes::from(first)), 1))
            } else {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                None
            }
        }
    });
    Some(axum::response::Response::builder().header("content-length", b.len()).body(axum::body::Body::from_stream(stream)).unwrap())
}

#[tokio::test]
async fn deleting_a_variant_never_touches_another_variant_downloading_in_the_same_folder() {
    let (app, e, data) = app().await;
    let (base, fake, _q4, _q8) = fake_hf().await;
    dispatch(app.clone(), "PUT", "/api/models/providers/hf/auth", Some(json!({"endpoint": base}))).await;
    let pull = |v: &str| json!({"provider": "hf", "model": "acme/tiny-GGUF", "variant": v});
    let (_, p) = dispatch(app.clone(), "POST", "/api/models/pull", Some(pull("Q4_K_M"))).await;
    wait_download(&app, p["id"].as_str().unwrap(), "done").await;

    // Q8_0 starts downloading into the same folder and stalls half way.
    fake.stall.store(true, std::sync::atomic::Ordering::SeqCst);
    let (_, p8) = dispatch(app.clone(), "POST", "/api/models/pull", Some(pull("Q8_0"))).await;
    let id8 = p8["id"].as_str().unwrap().to_string();
    let folder = data.path().join("models/hf/acme/tiny-GGUF");
    let part = folder.join("tiny-Q8_0.gguf.part");
    for _ in 0..200 {
        if std::fs::metadata(&part).map(|m| m.len() > 0).unwrap_or(false) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(part.is_file(), "the Q8_0 download never started");

    // Deleting the installed Q4_K_M is refused: the folder is in use.
    let (_, models) = dispatch(app.clone(), "GET", "/api/models", None).await;
    let q4 = models.as_array().unwrap().iter().find(|m| m["origin"] == "aiw").unwrap().clone();
    let mid = q4["id"].as_str().unwrap().to_string();
    let (st, body) = dispatch(app.clone(), "DELETE", &format!("/api/models/{mid}"), None).await;
    assert_eq!(st, 409, "{body}");
    assert!(body.to_string().contains("download"), "{body}");
    assert!(folder.join("tiny-Q4_K_M.gguf").is_file() && part.is_file() && folder.join(".aiw-model.json").is_file());
    let (_, list) = dispatch(app.clone(), "GET", "/api/models/downloads", None).await;
    let d8 = list.as_array().unwrap().iter().find(|d| d["id"] == id8.as_str()).unwrap().clone();
    assert!(matches!(d8["status"].as_str(), Some("running" | "verifying")), "{d8}");

    // Once the other download is cancelled the delete goes through: only Q4_K_M's own files go,
    // the folder stays because the `.part` of the cancelled job is still in it.
    dispatch(app.clone(), "POST", &format!("/api/models/downloads/{id8}/cancel"), None).await;
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/models/{mid}"), None).await.0, 204);
    assert!(!folder.join("tiny-Q4_K_M.gguf").exists());
    assert!(part.is_file(), "a resumable .part is not removed by deleting another variant");
    // Clearing the cancelled job drops its orphan `.part` (and nothing else).
    assert_eq!(dispatch(app.clone(), "DELETE", &format!("/api/models/downloads/{id8}"), None).await.0, 204);
    assert!(!part.exists());
    assert!(e.models().is_empty());
}

#[tokio::test]
async fn deleting_a_sharded_model_without_a_record_removes_every_shard() {
    let (app, _e, data) = app().await;
    let models = data.path().join("models");
    std::fs::create_dir_all(&models).unwrap();
    for i in 1..=3 {
        std::fs::write(models.join(format!("big-Q8_0-{i:05}-of-00003.gguf")), gguf(1000)).unwrap();
    }
    std::fs::write(models.join("big-Q8_0-00002-of-00003.gguf.part"), b"x").unwrap();
    std::fs::write(models.join("other-Q4_K_M.gguf"), gguf(500)).unwrap();
    let (_, list) = dispatch(app.clone(), "GET", "/api/models", None).await;
    // The `.part` marks the set as still in progress: it is not listed until it is gone.
    assert!(list.as_array().unwrap().iter().all(|m| m["id"] != "big-q8_0"), "{list}");
    std::fs::remove_file(models.join("big-Q8_0-00002-of-00003.gguf.part")).unwrap();
    let (_, list) = dispatch(app.clone(), "GET", "/api/models", None).await;
    let big = list.as_array().unwrap().iter().find(|m| m["id"] == "big-q8_0").unwrap_or_else(|| panic!("{list}")).clone();
    assert!(big["size_bytes"].as_u64().unwrap() >= 3000);
    assert_eq!(dispatch(app.clone(), "DELETE", "/api/models/big-q8_0", None).await.0, 204);
    let mut left: Vec<String> =
        std::fs::read_dir(&models).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    left.sort();
    assert_eq!(left, ["other-Q4_K_M.gguf"], "no orphan shard may stay behind");
}

#[tokio::test]
async fn an_incomplete_shard_set_is_not_listed_as_a_model() {
    let (app, _e, data) = app().await;
    let models = data.path().join("models");
    std::fs::create_dir_all(&models).unwrap();
    std::fs::write(models.join("big-Q8_0-00001-of-00003.gguf"), gguf(1000)).unwrap();
    std::fs::write(models.join("big-Q8_0-00002-of-00003.gguf.part"), b"x").unwrap();
    let (_, list) = dispatch(app.clone(), "GET", "/api/models", None).await;
    assert!(list.as_array().unwrap().is_empty(), "{list}");
}

#[tokio::test]
async fn a_url_that_serves_a_web_page_is_refused() {
    let (app, _e, _data) = app().await;
    let page_app = Router::new().route(
        "/blob/Qwen3-4B-Q4_K_M.gguf",
        ax_get(|| async { ([("content-type", "text/html")], "<!doctype html><html><body>Sign in</body></html>") }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, page_app).await.unwrap() });
    let url = format!("http://{addr}/blob/Qwen3-4B-Q4_K_M.gguf");
    let (st, body) = dispatch(app, "POST", "/api/models/pull", Some(json!({"url": url}))).await;
    assert_eq!(st, 400, "{body}");
    assert!(body.to_string().contains("pagina web"), "{body}");
}
