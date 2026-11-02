//! Hugging Face provider: search (`/api/models`), repository metadata and file tree with sha256
//! (`lfs.oid`), downloads from `/{repo}/resolve/{rev}/{path}`.

use super::http::{next_link, Api};
use super::variants::{build_variants, capabilities, dedup, detect_formats, params_from_name, RepoFile, RepoHints};
use super::{enc_path, enc_segment, same_origin, AuthCheck, FileJob, ModelProvider, ProviderSettings, SearchQuery};
use crate::relpath::{native_join, native_rel};
use aiw_core::api::{ModelProviderInfo, RemoteModel, RemoteModelDetail, SearchPage};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use reqwest::Url;
use serde_json::Value;
use std::path::PathBuf;

const NAME: &str = "Hugging Face";

pub struct HfProvider {
    api: Api,
    settings: ProviderSettings,
}

/// `owner/name` or a legacy single-segment repository (`gpt2`).
pub fn valid_repo(s: &str) -> bool {
    let parts: Vec<&str> = s.split('/').collect();
    (1..=2).contains(&parts.len())
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 96
                && *p != "."
                && *p != ".."
                && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}

impl HfProvider {
    pub fn new(endpoint: &str) -> Self {
        Self { api: Api::new(), settings: ProviderSettings::new(endpoint) }
    }

    fn headers(&self, url: &str) -> Vec<(&'static str, String)> {
        self.auth_header(url).map(|(_, v)| vec![("authorization", v)]).unwrap_or_default()
    }

    /// Points a next-page URL (absolute, from a `Link` header or a client cursor) at our own
    /// endpoint: a cursor can never make us talk to another host.
    fn rebase(&self, link: &str) -> Result<String> {
        let ep = self.settings.endpoint();
        let u = Url::parse(link).map_err(|_| Error::Invalid("cursore di ricerca non valido".into()))?;
        if !u.path().starts_with("/api/") {
            return Err(Error::Invalid("cursore di ricerca non valido".into()));
        }
        let mut out = format!("{ep}{}", u.path());
        if let Some(q) = u.query() {
            out.push('?');
            out.push_str(q);
        }
        Ok(out)
    }

    async fn tree(&self, repo: &str, rev: &str) -> Result<Vec<RepoFile>> {
        let ep = self.settings.endpoint();
        let mut url = format!("{ep}/api/models/{repo}/tree/{}?recursive=true", enc_segment(rev));
        let mut out = Vec::new();
        for _ in 0..60 {
            let r = self.api.get(NAME, "il repository", &url, &self.headers(&url)).await.map_err(|e| gated_hint(e, repo))?;
            let next = r.headers().get("link").and_then(|h| h.to_str().ok()).and_then(next_link);
            let page: Value = r.json().await.map_err(|e| Error::Upstream(format!("{NAME}: elenco dei file non valido: {e}")))?;
            for e in page.as_array().map(Vec::as_slice).unwrap_or_default() {
                if e["type"].as_str() != Some("file") {
                    continue;
                }
                let Some(path) = e["path"].as_str() else { continue };
                let sha = e["lfs"]["oid"].as_str().map(|s| s.trim_start_matches("sha256:").to_ascii_lowercase()).filter(|s| s.len() == 64);
                out.push(RepoFile {
                    path: path.to_string(),
                    size: e["lfs"]["size"].as_u64().or_else(|| e["size"].as_u64()).unwrap_or(0),
                    sha256: sha,
                });
            }
            match next {
                Some(n) => url = self.rebase(&n)?,
                None => break,
            }
        }
        Ok(out)
    }
}

/// A 401/403 on a repository is almost always a gated or private model.
fn gated_hint(e: Error, repo: &str) -> Error {
    match e {
        Error::Unauthorized(_) => Error::Unauthorized(format!(
            "{NAME}: il repository {repo} è gated o privato: serve un token Hugging Face (Modelli → Provider → Hugging Face)"
        )),
        Error::Denied(_) => Error::Denied(format!(
            "{NAME}: accesso a {repo} vietato (403): accetta la licenza sul sito e usa un token Hugging Face con accesso al repository"
        )),
        other => other,
    }
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

fn license_of(tags: &[String], card: &Value) -> Option<String> {
    card["license"].as_str().map(String::from).or_else(|| tags.iter().find_map(|t| t.strip_prefix("license:").map(String::from)))
}

/// Tags worth showing: drops the machine-oriented ones.
fn display_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .filter(|t| {
            !["region:", "base_model:", "arxiv:", "dataset:", "license:", "doi:"].iter().any(|p| t.starts_with(p))
                && !matches!(t.as_str(), "endpoints_compatible" | "autotrain_compatible" | "text-generation-inference" | "deploy:azure")
        })
        .take(24)
        .cloned()
        .collect()
}

fn is_gated(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty() && s != "false",
        _ => false,
    }
}

fn model_from(v: &Value, ep: &str) -> Option<RemoteModel> {
    let id = v["id"].as_str().or_else(|| v["modelId"].as_str())?.to_string();
    let (publisher, name) = match id.split_once('/') {
        Some((o, n)) => (o.to_string(), n.to_string()),
        None => (v["author"].as_str().unwrap_or_default().to_string(), id.clone()),
    };
    let tags = str_vec(&v["tags"]);
    let formats = detect_formats(&tags, v["library_name"].as_str(), &id);
    let params_b = v["safetensors"]["total"]
        .as_u64()
        .or_else(|| v["gguf"]["total"].as_u64())
        .map(|n| (n as f64 / 1e7).round() / 100.0)
        .or_else(|| params_from_name(&name));
    Some(RemoteModel {
        provider: "hf".into(),
        capabilities: capabilities(&tags, v["pipeline_tag"].as_str(), &id),
        formats,
        license: license_of(&tags, &v["cardData"]),
        tags: display_tags(&tags),
        description: v["cardData"]["description"].as_str().map(String::from),
        downloads: v["downloads"].as_u64(),
        likes: v["likes"].as_u64(),
        updated_at: v["lastModified"].as_str().or_else(|| v["createdAt"].as_str()).map(String::from),
        gated: is_gated(&v["gated"]),
        params_b,
        url: format!("{ep}/{id}"),
        name,
        publisher,
        id,
    })
}

#[async_trait]
impl ModelProvider for HfProvider {
    fn info(&self) -> ModelProviderInfo {
        ModelProviderInfo {
            id: "hf".into(),
            name: NAME.into(),
            description: "Milioni di modelli: GGUF, MLX, ONNX e safetensors. Il token serve solo per i repository gated.".into(),
            homepage: "https://huggingface.co".into(),
            capabilities: ["search", "files", "auth", "multi_file"].map(String::from).to_vec(),
            formats: ["gguf", "mlx", "onnx", "safetensors", "litertlm", "pte"].map(String::from).to_vec(),
            auth: "optional".into(),
            auth_configured: self.settings.has_token(),
            endpoint: self.settings.endpoint(),
        }
    }

    fn settings(&self) -> Option<&ProviderSettings> {
        Some(&self.settings)
    }

    async fn search(&self, q: &SearchQuery) -> Result<SearchPage> {
        let ep = self.settings.endpoint();
        let url = match q.cursor.as_deref().filter(|c| !c.is_empty()) {
            Some(c) => self.rebase(c)?,
            None => {
                let sort = match q.sort.as_deref().unwrap_or("downloads") {
                    "" | "downloads" => "downloads",
                    "likes" => "likes",
                    "updated" => "lastModified",
                    other => return Err(Error::Invalid(format!("ordinamento non valido: {other} (usa downloads, likes o updated)"))),
                };
                let mut u = Url::parse(&format!("{ep}/api/models")).map_err(|_| Error::Invalid(format!("endpoint non valido: {ep}")))?;
                {
                    let mut p = u.query_pairs_mut();
                    if !q.q.trim().is_empty() {
                        p.append_pair("search", q.q.trim());
                    }
                    if let Some(f) = q.format.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
                        if !f.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) {
                            return Err(Error::Invalid(format!("formato non valido: {f}")));
                        }
                        p.append_pair("filter", &f.to_ascii_lowercase());
                    }
                    p.append_pair("sort", sort);
                    p.append_pair("direction", "-1");
                    p.append_pair("limit", &q.limit().to_string());
                }
                u.to_string()
            }
        };
        let r = self.api.get(NAME, "la ricerca", &url, &self.headers(&url)).await?;
        let next = r.headers().get("link").and_then(|h| h.to_str().ok()).and_then(next_link).and_then(|n| self.rebase(&n).ok());
        let body: Value = r.json().await.map_err(|e| Error::Upstream(format!("{NAME}: risultati di ricerca non validi: {e}")))?;
        let items = body.as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(|v| model_from(v, &ep)).collect();
        Ok(SearchPage { items, next_cursor: next })
    }

    async fn detail(&self, model: &str, revision: Option<&str>) -> Result<RemoteModelDetail> {
        if !valid_repo(model) {
            return Err(Error::Invalid(format!("riferimento Hugging Face non valido: `{model}` (formato proprietario/nome)")));
        }
        let ep = self.settings.endpoint();
        let wanted = revision.map(str::trim).filter(|r| !r.is_empty());
        let explicit = wanted.filter(|r| *r != "main");
        let url = match explicit {
            None => format!("{ep}/api/models/{model}"),
            Some(r) => format!("{ep}/api/models/{model}/revision/{}", enc_segment(r)),
        };
        let info =
            self.api.get_json(NAME, &format!("il modello {model}"), &url, &self.headers(&url)).await.map_err(|e| gated_hint(e, model))?;
        let rev = explicit.map(String::from).or_else(|| info["sha"].as_str().map(String::from)).unwrap_or_else(|| "main".into());
        let mut m = model_from(&info, &ep).ok_or_else(|| Error::Upstream(format!("{NAME}: metadati di {model} non validi")))?;
        m.id = model.to_string();
        let files = self.tree(model, &rev).await?;
        let (owner, name) = model.split_once('/').unwrap_or(("", model));
        let tags = str_vec(&info["tags"]);
        let hints =
            RepoHints { owner: owner.into(), name: name.into(), tags, library_name: info["library_name"].as_str().map(String::from) };
        let variants = build_variants(&files, &hints);
        m.formats = dedup(variants.iter().map(|v| v.format.clone()).chain(m.formats.clone()).collect());
        Ok(RemoteModelDetail { model: m, revision: rev, variants })
    }

    fn plan(&self, detail: &RemoteModelDetail, variant: &str) -> Result<Vec<FileJob>> {
        let ep = self.settings.endpoint();
        let repo = &detail.model.id;
        if !valid_repo(repo) {
            return Err(Error::Invalid(format!("riferimento Hugging Face non valido: `{repo}`")));
        }
        let v = detail
            .variants
            .iter()
            .find(|v| v.id == variant)
            .ok_or_else(|| Error::NotFound(format!("variante `{variant}` di {repo} non trovata")))?;
        let folder = native_join("hf", repo);
        v.files
            .iter()
            .map(|f| {
                let url = format!("{ep}/{repo}/resolve/{}/{}", enc_segment(&detail.revision), enc_path(&f.path));
                Ok(FileJob {
                    headers: self.auth_header(&url).into_iter().collect(),
                    // `Q4_K_M/x.gguf` is nested folders on every OS, never one name with a slash.
                    dest: folder.join(native_rel(&f.path)?),
                    size: f.size_bytes,
                    sha256: f.sha256.clone(),
                    url,
                })
            })
            .collect()
    }

    fn folder(&self, detail: &RemoteModelDetail, _variant: &str) -> PathBuf {
        native_join("hf", &detail.model.id)
    }

    fn auth_header(&self, url: &str) -> Option<(String, String)> {
        let t = self.settings.token()?;
        same_origin(url, &self.settings.endpoint()).then(|| ("Authorization".to_string(), format!("Bearer {t}")))
    }

    async fn check_auth(&self) -> AuthCheck {
        let Some(t) = self.settings.token() else { return AuthCheck::Unknown };
        let url = format!("{}/api/whoami-v2", self.settings.endpoint());
        match self.api.client().get(&url).bearer_auth(t).send().await {
            Ok(r) if r.status().is_success() => {
                let v: Value = r.json().await.unwrap_or_default();
                AuthCheck::Valid(v["name"].as_str().map(String::from))
            }
            Ok(r) if r.status().as_u16() == 401 => AuthCheck::Invalid,
            _ => AuthCheck::Unknown,
        }
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A local Hugging Face with the response formats of the real API, shared by the tests of this
    //! crate (and a model of what the engine tests need).
    use axum::extract::{Path, Query, State};
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;
    use axum::Router;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    pub struct Seen {
        /// `(path, authorization header)` of every request.
        pub requests: Vec<(String, Option<String>)>,
    }

    #[derive(Clone)]
    pub struct Fake {
        pub seen: Arc<Mutex<Seen>>,
        pub files: Arc<HashMap<String, Vec<u8>>>,
        pub token: Option<String>,
    }

    pub const REPO: &str = "acme/tiny-GGUF";

    pub fn sha(b: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
    }

    pub fn contents() -> HashMap<String, Vec<u8>> {
        let mut m = HashMap::new();
        m.insert("tiny-Q4_K_M.gguf".to_string(), (0..5000u32).map(|i| (i % 251) as u8).collect());
        m.insert("tiny-Q8_0-00001-of-00002.gguf".to_string(), vec![7u8; 3000]);
        m.insert("tiny-Q8_0-00002-of-00002.gguf".to_string(), vec![8u8; 2000]);
        m.insert("mmproj-F16.gguf".to_string(), vec![9u8; 700]);
        m.insert("README.md".to_string(), b"hello".to_vec());
        m
    }

    fn authorized(f: &Fake, h: &HeaderMap) -> bool {
        match &f.token {
            None => true,
            Some(t) => h.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {t}")),
        }
    }

    async fn search(State(f): State<Fake>, Query(q): Query<HashMap<String, String>>, h: HeaderMap) -> Response {
        f.seen
            .lock()
            .unwrap()
            .requests
            .push((format!("/api/models?{}", q.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")), None));
        let _ = &h;
        let page: usize = q.get("page").and_then(|p| p.parse().ok()).unwrap_or(0);
        let all = [
            json!({"id": REPO, "downloads": 1200, "likes": 34, "tags": ["gguf", "text-generation", "license:apache-2.0", "base_model:acme/tiny"], "pipeline_tag": "text-generation", "library_name": "gguf", "lastModified": "2026-01-02T03:04:05.000Z"}),
            json!({"id": "acme/tiny-mlx", "downloads": 10, "likes": 1, "tags": ["mlx", "safetensors", "text-generation"], "pipeline_tag": "text-generation", "library_name": "mlx"}),
            json!({"id": "acme/other-GGUF", "downloads": 5, "likes": 0, "tags": ["gguf", "feature-extraction"], "pipeline_tag": "feature-extraction"}),
        ];
        let limit: usize = q.get("limit").and_then(|p| p.parse().ok()).unwrap_or(10);
        let items: Vec<Value> = all.iter().skip(page * limit).take(limit).cloned().collect();
        let mut resp = axum::Json(Value::Array(items)).into_response();
        if (page + 1) * limit < all.len() {
            // The real Hub answers with an absolute URL on its own domain.
            let link = format!("<https://huggingface.co/api/models?limit={limit}&page={}&cursor=zzz>; rel=\"next\"", page + 1);
            resp.headers_mut().insert("link", link.parse().unwrap());
        }
        resp
    }

    async fn info(State(f): State<Fake>, Path(repo): Path<String>, h: HeaderMap) -> Response {
        f.seen
            .lock()
            .unwrap()
            .requests
            .push((format!("/api/models/{repo}"), h.get("authorization").and_then(|v| v.to_str().ok()).map(String::from)));
        if repo == "acme/gated-GGUF" && !authorized(&f, &h) {
            return (StatusCode::UNAUTHORIZED, "no").into_response();
        }
        if repo == "acme/missing" {
            return StatusCode::NOT_FOUND.into_response();
        }
        axum::Json(json!({
            "id": repo, "sha": "0123456789abcdef0123456789abcdef01234567", "gated": false,
            "tags": ["gguf", "text-generation", "license:apache-2.0"], "pipeline_tag": "text-generation", "library_name": "gguf",
            "downloads": 1200, "likes": 34, "cardData": {"license": "apache-2.0"}, "gguf": {"total": 4_000_000_000u64}
        }))
        .into_response()
    }

    async fn tree(State(f): State<Fake>, Path((repo, rev)): Path<(String, String)>, h: HeaderMap) -> Response {
        f.seen
            .lock()
            .unwrap()
            .requests
            .push((format!("/api/models/{repo}/tree/{rev}"), h.get("authorization").and_then(|v| v.to_str().ok()).map(String::from)));
        let mut names: Vec<&String> = f.files.keys().collect();
        names.sort();
        let entries: Vec<Value> = names
            .iter()
            .map(|n| {
                let b = &f.files[*n];
                if n.ends_with(".gguf") {
                    json!({"type": "file", "oid": "gitsha1", "size": 134, "path": n, "lfs": {"oid": sha(b), "size": b.len(), "pointerSize": 134}})
                } else {
                    json!({"type": "file", "oid": "gitsha1", "size": b.len(), "path": n})
                }
            })
            .collect();
        // Page the listing in two, like the real tree endpoint does for big repositories.
        let mut all = vec![json!({"type": "directory", "path": "sub", "oid": "x", "size": 0})];
        all.extend(entries);
        let mut resp = axum::Json(Value::Array(all[..2].to_vec())).into_response();
        resp.headers_mut().insert(
            "link",
            format!("<https://huggingface.co/api/models/{repo}/tree/{rev}?recursive=true&cursor=p2>; rel=\"next\"").parse().unwrap(),
        );
        resp
    }

    async fn tree_page2(State(f): State<Fake>, Path((_repo, _rev)): Path<(String, String)>) -> Response {
        let mut names: Vec<&String> = f.files.keys().collect();
        names.sort();
        let entries: Vec<Value> = names
            .iter()
            .skip(1)
            .map(|n| {
                let b = &f.files[*n];
                if n.ends_with(".gguf") {
                    json!({"type": "file", "oid": "gitsha1", "size": 134, "path": n, "lfs": {"oid": sha(b), "size": b.len(), "pointerSize": 134}})
                } else {
                    json!({"type": "file", "oid": "gitsha1", "size": b.len(), "path": n})
                }
            })
            .collect();
        axum::Json(Value::Array(entries)).into_response()
    }

    async fn whoami(State(f): State<Fake>, h: HeaderMap) -> Response {
        if authorized(&f, &h) && h.get("authorization").is_some() {
            axum::Json(json!({"name": "tester", "type": "user"})).into_response()
        } else {
            StatusCode::UNAUTHORIZED.into_response()
        }
    }

    async fn resolve(State(f): State<Fake>, Path((repo, rev, file)): Path<(String, String, String)>, h: HeaderMap) -> Response {
        f.seen
            .lock()
            .unwrap()
            .requests
            .push((format!("/{repo}/resolve/{rev}/{file}"), h.get("authorization").and_then(|v| v.to_str().ok()).map(String::from)));
        match f.files.get(&file) {
            Some(b) => serve_bytes(b, &h),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }

    /// Minimal Range-aware file server.
    pub fn serve_bytes(b: &[u8], h: &HeaderMap) -> Response {
        let range = h
            .get("range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("bytes="))
            .and_then(|v| v.strip_suffix('-'))
            .and_then(|v| v.parse::<usize>().ok());
        match range {
            Some(start) if start >= b.len() => {
                (StatusCode::RANGE_NOT_SATISFIABLE, [("content-range", format!("bytes */{}", b.len()))]).into_response()
            }
            Some(start) => (
                StatusCode::PARTIAL_CONTENT,
                [("content-range", format!("bytes {start}-{}/{}", b.len() - 1, b.len())), ("accept-ranges", "bytes".to_string())],
                b[start..].to_vec(),
            )
                .into_response(),
            None => (StatusCode::OK, [("accept-ranges", "bytes")], b.to_vec()).into_response(),
        }
    }

    /// Starts the fake on a random local port; returns its base URL.
    pub async fn start(f: Fake) -> String {
        let app = Router::new()
            .route("/api/models", get(search))
            .route("/api/whoami-v2", get(whoami))
            .route(
                "/api/models/{owner}/{name}",
                get(|s: State<Fake>, Path((o, n)): Path<(String, String)>, h: HeaderMap| info(s, Path(format!("{o}/{n}")), h)),
            )
            .route(
                "/api/models/{owner}/{name}/tree/{rev}",
                get(
                    |s: State<Fake>,
                     Path((o, n, r)): Path<(String, String, String)>,
                     Query(q): Query<HashMap<String, String>>,
                     h: HeaderMap| async move {
                        if q.get("cursor").map(String::as_str) == Some("p2") {
                            tree_page2(s, Path((format!("{o}/{n}"), r))).await
                        } else {
                            tree(s, Path((format!("{o}/{n}"), r)), h).await
                        }
                    },
                ),
            )
            .route(
                "/{owner}/{name}/resolve/{rev}/{*file}",
                get(|s: State<Fake>, Path((o, n, r, file)): Path<(String, String, String, String)>, h: HeaderMap| {
                    resolve(s, Path((format!("{o}/{n}"), r, file)), h)
                }),
            )
            .with_state(f);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}")
    }

    pub fn new_fake(token: Option<&str>) -> Fake {
        Fake { seen: Default::default(), files: Arc::new(contents()), token: token.map(String::from) }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    #[test]
    fn validates_repo_references() {
        assert!(valid_repo("Qwen/Qwen3-4B-GGUF"));
        assert!(valid_repo("gpt2"));
        assert!(!valid_repo("a/b/c"));
        assert!(!valid_repo("../etc"));
        assert!(!valid_repo("a/../b"));
        assert!(!valid_repo("a b/c"));
        assert!(!valid_repo(""));
    }

    #[tokio::test]
    async fn searches_with_pagination_through_the_link_header() {
        let base = start(new_fake(None)).await;
        let p = HfProvider::new(&base);
        let q = SearchQuery { q: "tiny".into(), format: Some("gguf".into()), sort: Some("likes".into()), limit: 2, cursor: None };
        let page = p.search(&q).await.unwrap();
        assert_eq!(page.items.len(), 2);
        let m = &page.items[0];
        assert_eq!((m.provider.as_str(), m.id.as_str(), m.publisher.as_str(), m.name.as_str()), ("hf", REPO, "acme", "tiny-GGUF"));
        assert_eq!(m.formats, ["gguf"]);
        assert_eq!(m.capabilities, ["chat"]);
        assert_eq!(m.license.as_deref(), Some("apache-2.0"));
        assert_eq!((m.downloads, m.likes), (Some(1200), Some(34)));
        assert_eq!(m.updated_at.as_deref(), Some("2026-01-02T03:04:05.000Z"));
        assert_eq!(m.url, format!("{base}/{REPO}"));
        assert!(!m.tags.iter().any(|t| t.starts_with("base_model:") || t.starts_with("license:")));
        // The cursor is the next URL on our own endpoint (the Hub answered with its public domain).
        let next = page.next_cursor.clone().unwrap();
        assert!(next.starts_with(&base), "{next}");
        assert!(next.contains("page=1"));
        let page2 = p.search(&SearchQuery { cursor: Some(next), limit: 2, ..Default::default() }).await.unwrap();
        assert_eq!(page2.items.len(), 1);
        assert!(page2.next_cursor.is_none());
        assert_eq!(page2.items[0].capabilities, ["embeddings"]);
        // A cursor never leaves the API.
        assert!(p.search(&SearchQuery { cursor: Some("https://evil.example/steal".into()), ..Default::default() }).await.is_err());
        assert!(p.search(&SearchQuery { sort: Some("bogus".into()), ..Default::default() }).await.is_err());
    }

    #[tokio::test]
    async fn search_request_follows_the_contract() {
        let fake = new_fake(None);
        let base = start(fake.clone()).await;
        let p = HfProvider::new(&base);
        p.search(&SearchQuery { q: "qwen 3".into(), format: Some("gguf".into()), sort: Some("updated".into()), limit: 99, cursor: None })
            .await
            .unwrap();
        let req = fake.seen.lock().unwrap().requests[0].0.clone();
        for part in ["search=qwen 3", "filter=gguf", "sort=lastModified", "direction=-1", "limit=50"] {
            assert!(req.contains(part), "{req} lacks {part}");
        }
    }

    #[tokio::test]
    async fn detail_lists_the_tree_with_sha256_and_builds_variants() {
        let fake = new_fake(None);
        let base = start(fake.clone()).await;
        let p = HfProvider::new(&base);
        let d = p.detail(REPO, None).await.unwrap();
        assert_eq!(d.revision, "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(d.model.params_b, Some(4.0));
        assert_eq!(d.model.license.as_deref(), Some("apache-2.0"));
        let ids: Vec<&str> = d.variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, ["Q4_K_M", "Q8_0"]);
        let q8 = &d.variants[1];
        assert_eq!(q8.files.len(), 3, "two shards and the projector: {:?}", q8.files);
        assert_eq!(q8.files[2].role, "mmproj");
        let files = fake.files.clone();
        assert_eq!(d.variants[0].files[0].sha256.as_deref(), Some(sha(&files["tiny-Q4_K_M.gguf"]).as_str()));
        assert_eq!(d.variants[0].size_bytes, 5000 + 700);
        // The tree was requested at the commit sha.
        assert!(fake.seen.lock().unwrap().requests.iter().any(|(p, _)| p.ends_with("/tree/0123456789abcdef0123456789abcdef01234567")));
        // Plan: resolve URLs pinned to the revision, layout hf/<owner>/<repo>/<path>.
        let jobs = p.plan(&d, "Q8_0").unwrap();
        assert_eq!(jobs.len(), 3);
        assert_eq!(jobs[0].url, format!("{base}/{REPO}/resolve/{}/tiny-Q8_0-00001-of-00002.gguf", d.revision));
        assert_eq!(jobs[0].dest, PathBuf::from("hf/acme/tiny-GGUF/tiny-Q8_0-00001-of-00002.gguf"));
        assert_eq!(p.folder(&d, "Q8_0"), PathBuf::from("hf/acme/tiny-GGUF"));
        assert!(p.plan(&d, "nope").is_err());
        assert!(p.detail("../x", None).await.is_err());
        assert!(matches!(p.detail("acme/missing", None).await, Err(Error::NotFound(_))));
    }

    #[tokio::test]
    async fn plan_nests_remote_paths_and_refuses_the_ones_that_escape() {
        let fake = new_fake(None);
        let base = start(fake).await;
        let p = HfProvider::new(&base);
        let mut d = p.detail(REPO, None).await.unwrap();
        // The remote path is `/`-separated; the destination is built component by component.
        d.variants[0].files[0].path = "Q4_K_M/sub/x.gguf".into();
        let jobs = p.plan(&d, "Q4_K_M").unwrap();
        assert_eq!(jobs[0].dest, PathBuf::from("hf").join("acme").join("tiny-GGUF").join("Q4_K_M").join("sub").join("x.gguf"));
        assert_eq!(jobs[0].dest.components().count(), 6);
        assert!(jobs[0].url.ends_with("/resolve/0123456789abcdef0123456789abcdef01234567/Q4_K_M/sub/x.gguf"), "{}", jobs[0].url);
        for bad in ["../x.gguf", "a/../../x.gguf", "/etc/x.gguf", "C:/x.gguf", "a\\b.gguf", "..\\x.gguf", ""] {
            d.variants[0].files[0].path = bad.into();
            assert!(matches!(p.plan(&d, "Q4_K_M"), Err(Error::Invalid(_))), "{bad:?} must be refused");
        }
    }

    #[tokio::test]
    async fn gated_repo_asks_for_a_token_and_the_token_only_goes_to_the_own_host() {
        let fake = new_fake(Some("hf_secret"));
        let base = start(fake.clone()).await;
        let p = HfProvider::new(&base);
        let err = p.detail("acme/gated-GGUF", None).await.unwrap_err();
        assert!(matches!(&err, Error::Unauthorized(m) if m.contains("token Hugging Face") && m.contains("gated")), "{err}");
        p.settings().unwrap().set_token(Some("hf_secret".into()));
        assert!(p.detail("acme/gated-GGUF", None).await.is_ok());
        let d = p.detail(REPO, None).await.unwrap();
        let jobs = p.plan(&d, "Q4_K_M").unwrap();
        assert_eq!(jobs[0].headers, vec![("Authorization".to_string(), "Bearer hf_secret".to_string())]);
        assert_eq!(p.auth_header("https://cdn-lfs.huggingface.co/x"), None);
        assert_eq!(p.auth_header(&format!("{base}/x")).unwrap().1, "Bearer hf_secret");
        assert_eq!(p.check_auth().await, AuthCheck::Valid(Some("tester".into())));
        p.settings().unwrap().set_token(Some("wrong".into()));
        assert_eq!(p.check_auth().await, AuthCheck::Invalid);
    }
}
