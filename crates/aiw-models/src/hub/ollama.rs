//! Ollama registry provider. There is no public search API: results come from a bundled index of
//! popular library models plus exact `name[:tag]` / `user/name[:tag]` references, verified against
//! the registry when the variants are listed. Files are OCI blobs (the model layer is a GGUF).

use super::http::Api;
use super::variants::dedup;
use super::{FileJob, ModelProvider, ProviderSettings, SearchQuery};
use aiw_core::api::{ModelProviderInfo, ModelVariant, RemoteFile, RemoteModel, RemoteModelDetail, SearchPage};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use futures::{stream, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::OnceLock;

const NAME: &str = "Registro Ollama";
const MODEL_LAYER: &str = "application/vnd.ollama.image.model";
const PROJECTOR_LAYER: &str = "application/vnd.ollama.image.projector";
const MANIFEST_ACCEPT: &str = "application/vnd.docker.distribution.manifest.v2+json";

/// One entry of the bundled library index.
#[derive(Debug, Clone, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Popular Ollama library models (a snapshot: tags that do not exist are skipped at listing time).
pub fn library_index() -> &'static [IndexEntry] {
    static INDEX: OnceLock<Vec<IndexEntry>> = OnceLock::new();
    INDEX.get_or_init(|| serde_json::from_str(include_str!("../../catalog/ollama-library.json")).expect("ollama index JSON is valid"))
}

pub struct OllamaProvider {
    api: Api,
    settings: ProviderSettings,
}

/// `name[:tag]` or `ns/name[:tag]` → `(ns, name, tag)`; `ns` is `library` for official models.
pub fn parse_ref(model: &str) -> Result<(String, String, Option<String>)> {
    let model = model.trim();
    let (path, tag) = match model.rsplit_once(':') {
        Some((p, t)) if !t.contains('/') && !t.is_empty() => (p, Some(t.to_string())),
        _ => (model, None),
    };
    let (ns, name) = match path.split('/').collect::<Vec<_>>().as_slice() {
        [name] => ("library", *name),
        [ns, name] => (*ns, *name),
        _ => return Err(Error::Invalid(format!("riferimento Ollama non valido: `{model}` (usa nome[:tag] o utente/nome[:tag])"))),
    };
    let ok = |s: &str| {
        !s.is_empty()
            && s.len() <= 80
            && s != "."
            && s != ".."
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if !ok(ns) || !ok(name) || tag.as_deref().map(|t| !ok(t)).unwrap_or(false) {
        return Err(Error::Invalid(format!("riferimento Ollama non valido: `{model}`")));
    }
    Ok((ns.to_ascii_lowercase(), name.to_ascii_lowercase(), tag.map(|t| t.to_ascii_lowercase())))
}

fn canonical(ns: &str, name: &str) -> String {
    if ns == "library" {
        name.to_string()
    } else {
        format!("{ns}/{name}")
    }
}

fn sha_of(digest: &str) -> Option<String> {
    let h = digest.strip_prefix("sha256:")?;
    (h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).then(|| h.to_ascii_lowercase())
}

impl OllamaProvider {
    pub fn new(registry: &str) -> Self {
        Self { api: Api::new(), settings: ProviderSettings::new(registry) }
    }

    fn page_url(ns: &str, name: &str) -> String {
        if ns == "library" {
            format!("https://ollama.com/library/{name}")
        } else {
            format!("https://ollama.com/{ns}/{name}")
        }
    }

    fn bare(&self, ns: &str, name: &str) -> RemoteModel {
        match library_index().iter().find(|e| ns == "library" && e.name == name) {
            Some(e) => self.entry_model(e),
            None => RemoteModel {
                provider: "ollama".into(),
                id: canonical(ns, name),
                name: name.into(),
                publisher: if ns == "library" { "Ollama".into() } else { ns.into() },
                description: None,
                formats: vec!["gguf".into()],
                tags: vec![],
                capabilities: vec!["chat".into()],
                downloads: None,
                likes: None,
                updated_at: None,
                license: None,
                gated: false,
                params_b: super::variants::params_from_name(name),
                url: Self::page_url(ns, name),
            },
        }
    }

    fn entry_model(&self, e: &IndexEntry) -> RemoteModel {
        RemoteModel {
            provider: "ollama".into(),
            id: e.name.clone(),
            name: e.name.clone(),
            publisher: "Ollama".into(),
            description: Some(e.description.clone()),
            formats: vec!["gguf".into()],
            tags: e.tags.clone(),
            capabilities: e.capabilities.clone(),
            downloads: None,
            likes: None,
            updated_at: None,
            license: None,
            gated: false,
            params_b: None,
            url: Self::page_url("library", &e.name),
        }
    }

    async fn manifest(&self, ns: &str, name: &str, tag: &str) -> Result<Value> {
        let url = format!("{}/v2/{ns}/{name}/manifests/{tag}", self.settings.endpoint());
        self.api
            .get_json(NAME, &format!("il modello {}:{tag}", canonical(ns, name)), &url, &[("accept", MANIFEST_ACCEPT.to_string())])
            .await
    }

    /// Manifest → variant (quantization from the config blob, best effort).
    async fn variant(&self, ns: &str, name: &str, tag: &str) -> Result<ModelVariant> {
        let m = self.manifest(ns, name, tag).await?;
        let layers = m["layers"].as_array().map(Vec::as_slice).unwrap_or_default();
        let layer = |media: &str| layers.iter().find(|l| l["mediaType"].as_str() == Some(media));
        let model =
            layer(MODEL_LAYER).ok_or_else(|| Error::Upstream(format!("{NAME}: {}:{tag} non contiene pesi GGUF", canonical(ns, name))))?;
        let mut files = vec![RemoteFile {
            path: "model.gguf".into(),
            size_bytes: model["size"].as_u64().unwrap_or(0),
            sha256: model["digest"].as_str().and_then(sha_of),
            role: "weights".into(),
        }];
        if files[0].sha256.is_none() {
            return Err(Error::Upstream(format!("{NAME}: digest non valido nel manifest di {}:{tag}", canonical(ns, name))));
        }
        if let Some(p) = layer(PROJECTOR_LAYER) {
            files.push(RemoteFile {
                path: "mmproj.gguf".into(),
                size_bytes: p["size"].as_u64().unwrap_or(0),
                sha256: p["digest"].as_str().and_then(sha_of),
                role: "mmproj".into(),
            });
        }
        let mut quant = None;
        if let Some(d) = m["config"]["digest"].as_str().filter(|d| sha_of(d).is_some()) {
            let url = format!("{}/v2/{ns}/{name}/blobs/{d}", self.settings.endpoint());
            if let Ok(cfg) = self.api.get_json(NAME, "la configurazione", &url, &[]).await {
                quant = cfg["file_type"].as_str().map(|q| q.to_ascii_uppercase()).filter(|q| !q.is_empty() && q != "UNKNOWN");
            }
        }
        let size_bytes = files.iter().map(|f| f.size_bytes).sum();
        Ok(ModelVariant {
            id: tag.into(),
            label: format!("{}:{tag}", canonical(ns, name)),
            format: "gguf".into(),
            quant,
            files,
            size_bytes,
            fit: String::new(),
            fit_detail: String::new(),
            recommended: false,
            runtimes: vec![],
            installed: false,
        })
    }
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    fn info(&self) -> ModelProviderInfo {
        ModelProviderInfo {
            id: "ollama".into(),
            name: NAME.into(),
            description: "I modelli della libreria Ollama scaricati direttamente dal registro, senza bisogno del demone Ollama.".into(),
            homepage: "https://ollama.com/library".into(),
            capabilities: ["search", "files", "multi_file"].map(String::from).to_vec(),
            formats: vec!["gguf".into()],
            auth: "none".into(),
            auth_configured: false,
            endpoint: self.settings.endpoint(),
        }
    }

    fn settings(&self) -> Option<&ProviderSettings> {
        Some(&self.settings)
    }

    async fn search(&self, q: &SearchQuery) -> Result<SearchPage> {
        if q.format.as_deref().map(|f| !f.is_empty() && !f.eq_ignore_ascii_case("gguf")).unwrap_or(false) {
            return Ok(SearchPage::default());
        }
        let ql = q.q.trim().to_lowercase();
        let mut items: Vec<RemoteModel> = Vec::new();
        let mut hits: Vec<&IndexEntry> = library_index()
            .iter()
            .filter(|e| {
                ql.is_empty()
                    || e.name.contains(&ql)
                    || e.description.to_lowercase().contains(&ql)
                    || e.tags.contains(&ql)
                    || e.capabilities.contains(&ql)
            })
            .collect();
        hits.sort_by_key(|e| !e.name.starts_with(&ql));
        items.extend(hits.iter().map(|e| self.entry_model(e)));
        // An exact reference (`user/name`, `name:tag`) is accepted even when it is not in the index.
        if !ql.is_empty() {
            if let Ok((ns, name, _)) = parse_ref(&ql) {
                let id = canonical(&ns, &name);
                if (ql.contains(':') || ql.contains('/') || items.is_empty()) && !items.iter().any(|m| m.id == id) {
                    items.insert(0, self.bare(&ns, &name));
                }
            }
        }
        let limit = q.limit();
        let offset: usize = q.cursor.as_deref().and_then(|c| c.parse().ok()).unwrap_or(0);
        let more = items.len() > offset + limit;
        let items: Vec<RemoteModel> = items.into_iter().skip(offset).take(limit).collect();
        Ok(SearchPage { items, next_cursor: more.then(|| (offset + limit).to_string()) })
    }

    async fn detail(&self, model: &str, _revision: Option<&str>) -> Result<RemoteModelDetail> {
        let (ns, name, tag) = parse_ref(model)?;
        let explicit = tag.is_some();
        let tags: Vec<String> = match tag {
            Some(t) => vec![t],
            None => {
                let extra = library_index().iter().find(|e| ns == "library" && e.name == name).map(|e| e.tags.clone()).unwrap_or_default();
                dedup(std::iter::once("latest".to_string()).chain(extra).collect())
            }
        };
        let mut results: Vec<(usize, Result<ModelVariant>)> = stream::iter(tags.iter().cloned().enumerate())
            .map(|(i, t)| {
                let (ns, name) = (ns.clone(), name.clone());
                async move { (i, self.variant(&ns, &name, &t).await) }
            })
            .buffer_unordered(6)
            .collect()
            .await;
        results.sort_by_key(|(i, _)| *i);
        let mut variants = Vec::new();
        let mut first_err = None;
        for (_, r) in results {
            match r {
                Ok(v) => variants.push(v),
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        if variants.is_empty() || (explicit && first_err.is_some()) {
            return Err(first_err.unwrap_or_else(|| Error::NotFound(format!("il modello {model} sul registro Ollama"))));
        }
        // `latest` is an alias of one of the sized tags: do not offer the same blob twice.
        if let Some(latest) = variants.iter().position(|v| v.id == "latest") {
            let digest = variants[latest].files[0].sha256.clone();
            if variants.len() > 1 && variants.iter().any(|v| v.id != "latest" && v.files[0].sha256 == digest) {
                variants.remove(latest);
            }
        }
        variants.sort_by(|a, b| a.size_bytes.cmp(&b.size_bytes).then_with(|| a.id.cmp(&b.id)));
        let mut m = self.bare(&ns, &name);
        if explicit {
            m.params_b = m.params_b.or_else(|| super::variants::params_from_name(&variants[0].id));
        }
        let revision = if explicit { variants[0].id.clone() } else { "latest".into() };
        Ok(RemoteModelDetail { model: m, revision, variants })
    }

    fn plan(&self, detail: &RemoteModelDetail, variant: &str) -> Result<Vec<FileJob>> {
        let (ns, name, _) = parse_ref(&detail.model.id)?;
        let v = detail
            .variants
            .iter()
            .find(|v| v.id == variant)
            .ok_or_else(|| Error::NotFound(format!("variante `{variant}` di {}", detail.model.id)))?;
        let ep = self.settings.endpoint();
        let dir = self.folder(detail, variant);
        v.files
            .iter()
            .map(|f| {
                let hex = f.sha256.clone().ok_or_else(|| Error::Invalid(format!("{} senza digest", f.path)))?;
                Ok(FileJob {
                    url: format!("{ep}/v2/{ns}/{name}/blobs/sha256:{hex}"),
                    dest: dir.join(&f.path),
                    size: f.size_bytes,
                    sha256: Some(hex),
                    headers: vec![],
                })
            })
            .collect()
    }

    fn folder(&self, detail: &RemoteModelDetail, variant: &str) -> PathBuf {
        let (ns, name, _) = parse_ref(&detail.model.id).unwrap_or(("library".into(), "unknown".into(), None));
        PathBuf::from("ollama").join(ns).join(name).join(variant)
    }

    fn canonical_ref(&self, model: &str) -> String {
        parse_ref(model).map(|(ns, name, _)| canonical(&ns, &name)).unwrap_or_else(|_| model.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Path;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::{Json, Router};
    use serde_json::json;

    fn hex(c: char) -> String {
        c.to_string().repeat(64)
    }

    /// qwen3: tags 0.6b (text), 4b (text) and latest (alias of 4b); gemma3:4b has a projector.
    async fn registry() -> String {
        let manifest = |model: char, size: u64, projector: Option<char>| {
            let mut layers = vec![
                json!({"mediaType": MODEL_LAYER, "digest": format!("sha256:{}", hex(model)), "size": size}),
                json!({"mediaType": "application/vnd.ollama.image.template", "digest": format!("sha256:{}", hex('t')), "size": 10}),
            ];
            if let Some(p) = projector {
                layers.push(json!({"mediaType": PROJECTOR_LAYER, "digest": format!("sha256:{}", hex(p)), "size": 800}));
            }
            json!({"schemaVersion": 2, "mediaType": MANIFEST_ACCEPT,
                   "config": {"mediaType": "application/vnd.docker.container.image.v1+json", "digest": format!("sha256:{}", hex('c')), "size": 400},
                   "layers": layers})
        };
        let app = Router::new()
            .route(
                "/v2/{ns}/{name}/manifests/{tag}",
                get(move |Path((ns, name, tag)): Path<(String, String, String)>, h: HeaderMap| async move {
                    assert_eq!(h.get("accept").and_then(|v| v.to_str().ok()), Some(MANIFEST_ACCEPT));
                    match (ns.as_str(), name.as_str(), tag.as_str()) {
                        ("library", "qwen3", "0.6b") => Json(manifest('a', 600, None)).into_response(),
                        ("library", "qwen3", "4b") | ("library", "qwen3", "latest") => Json(manifest('b', 4000, None)).into_response(),
                        ("library", "gemma3", "4b") => Json(manifest('d', 3000, Some('e'))).into_response(),
                        ("someone", "tuned", "latest") => Json(manifest('f', 100, None)).into_response(),
                        _ => StatusCode::NOT_FOUND.into_response(),
                    }
                }),
            )
            .route(
                "/v2/{ns}/{name}/blobs/{digest}",
                get(|| async {
                    Json(json!({"model_format": "gguf", "model_family": "qwen3", "model_type": "4.0B", "file_type": "Q4_K_M"}))
                }),
            );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}")
    }

    #[test]
    fn the_bundled_index_is_valid() {
        let idx = library_index();
        assert!(idx.len() >= 40, "{}", idx.len());
        for e in idx {
            assert!(parse_ref(&e.name).is_ok(), "{}", e.name);
            assert!(!e.description.is_empty() && !e.capabilities.is_empty() && !e.tags.is_empty(), "{}", e.name);
        }
        for name in
            ["qwen3", "gemma3", "llama3.2", "gpt-oss", "deepseek-r1", "mistral", "phi4", "qwen2.5-coder", "nomic-embed-text", "llava"]
        {
            assert!(idx.iter().any(|e| e.name == name), "{name}");
        }
        let mut names: Vec<&str> = idx.iter().map(|e| e.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), idx.len(), "duplicate names");
    }

    #[test]
    fn parses_references() {
        assert_eq!(parse_ref("qwen3").unwrap(), ("library".into(), "qwen3".into(), None));
        assert_eq!(parse_ref("qwen3:4b").unwrap(), ("library".into(), "qwen3".into(), Some("4b".into())));
        assert_eq!(parse_ref("someone/tuned:Q4").unwrap(), ("someone".into(), "tuned".into(), Some("q4".into())));
        assert!(parse_ref("a/b/c").is_err());
        assert!(parse_ref("../x").is_err());
        assert!(parse_ref("").is_err());
    }

    #[tokio::test]
    async fn searches_the_index_and_exact_references() {
        let p = OllamaProvider::new("http://127.0.0.1:1");
        let q = |s: &str| SearchQuery { q: s.into(), limit: 50, ..Default::default() };
        let r = p.search(&q("qwen3")).await.unwrap();
        assert_eq!(r.items[0].id, "qwen3");
        assert!(r.items.iter().any(|m| m.id == "qwen3-coder"));
        assert_eq!(r.items[0].capabilities, ["chat", "tools", "reasoning"]);
        assert_eq!(r.items[0].url, "https://ollama.com/library/qwen3");
        assert!(p.search(&q("embeddings")).await.unwrap().items.iter().all(|m| m.capabilities == ["embeddings"]));
        assert!(p.search(&q("")).await.unwrap().items.len() >= 40);
        // Not in the index, but a valid reference.
        let r = p.search(&q("someone/tuned:latest")).await.unwrap();
        assert_eq!((r.items[0].id.as_str(), r.items[0].publisher.as_str()), ("someone/tuned", "someone"));
        assert!(p.search(&SearchQuery { q: "x".into(), format: Some("mlx".into()), ..Default::default() }).await.unwrap().items.is_empty());
        // Paging.
        let r = p.search(&SearchQuery { limit: 5, ..Default::default() }).await.unwrap();
        assert_eq!((r.items.len(), r.next_cursor.as_deref()), (5, Some("5")));
        let r2 = p.search(&SearchQuery { limit: 5, cursor: Some("5".into()), ..Default::default() }).await.unwrap();
        assert_ne!(r.items[0].id, r2.items[0].id);
    }

    #[tokio::test]
    async fn lists_variants_from_manifests() {
        let base = registry().await;
        let p = OllamaProvider::new(&base);
        let d = p.detail("qwen3", None).await.unwrap();
        // Tags missing from the registry (1.7b, 8b, …) are skipped; latest is an alias of 4b.
        assert_eq!(d.variants.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["0.6b", "4b"]);
        assert_eq!(d.model.id, "qwen3");
        assert_eq!(d.revision, "latest");
        let v = &d.variants[1];
        assert_eq!((v.format.as_str(), v.quant.as_deref(), v.size_bytes), ("gguf", Some("Q4_K_M"), 4000));
        assert_eq!(v.files[0].sha256.as_deref(), Some(hex('b').as_str()));
        assert_eq!(v.files[0].role, "weights");

        let jobs = p.plan(&d, "4b").unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].url, format!("{base}/v2/library/qwen3/blobs/sha256:{}", hex('b')));
        assert_eq!(jobs[0].dest, PathBuf::from("ollama/library/qwen3/4b/model.gguf"));
        assert_eq!(jobs[0].sha256.as_deref(), Some(hex('b').as_str()));
        assert_eq!(p.folder(&d, "4b"), PathBuf::from("ollama/library/qwen3/4b"));

        // Explicit tag: only that variant, and a projector for vision models.
        let g = p.detail("gemma3:4b", None).await.unwrap();
        assert_eq!(g.variants.len(), 1);
        assert_eq!(g.model.id, "gemma3");
        assert_eq!(g.revision, "4b");
        let jobs = p.plan(&g, "4b").unwrap();
        assert_eq!(
            jobs.iter().map(|j| j.dest.to_string_lossy().to_string()).collect::<Vec<_>>(),
            ["ollama/library/gemma3/4b/model.gguf", "ollama/library/gemma3/4b/mmproj.gguf"]
        );
        assert_eq!(g.variants[0].files[1].role, "mmproj");

        // User namespace, default tag.
        let u = p.detail("someone/tuned", None).await.unwrap();
        assert_eq!(u.model.id, "someone/tuned");
        assert_eq!(p.plan(&u, "latest").unwrap()[0].dest, PathBuf::from("ollama/someone/tuned/latest/model.gguf"));
        assert_eq!(p.canonical_ref("qwen3:4b"), "qwen3");
        assert_eq!(p.canonical_ref("someone/tuned:v2"), "someone/tuned");
        // Unknown models and tags are a clear 404.
        assert!(matches!(p.detail("nonexistent", None).await, Err(Error::NotFound(_))));
        assert!(matches!(p.detail("qwen3:99b", None).await, Err(Error::NotFound(_))));
    }
}
