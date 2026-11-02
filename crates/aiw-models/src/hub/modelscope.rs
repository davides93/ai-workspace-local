//! ModelScope provider (best effort: the site API is not documented). Search uses the site's own
//! `dolphin` endpoint and falls back to accepting an exact `owner/name`; files come from the
//! public repository API.

use super::http::{net_error, status_error, Api};
use super::variants::{build_variants, capabilities, dedup, detect_formats, params_from_name, RepoFile, RepoHints};
use super::{enc_segment, same_origin, FileJob, ModelProvider, ProviderSettings, SearchQuery};
use aiw_core::api::{ModelProviderInfo, RemoteModel, RemoteModelDetail, SearchPage};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;

const NAME: &str = "ModelScope";

pub struct ModelScopeProvider {
    api: Api,
    settings: ProviderSettings,
}

fn valid_ref(s: &str) -> bool {
    let parts: Vec<&str> = s.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 96
                && *p != "."
                && *p != ".."
                && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}

impl ModelScopeProvider {
    pub fn new(endpoint: &str) -> Self {
        Self { api: Api::new(), settings: ProviderSettings::new(endpoint) }
    }

    fn headers(&self, url: &str) -> Vec<(&'static str, String)> {
        self.auth_header(url).map(|(_, v)| vec![("authorization", v)]).unwrap_or_default()
    }

    fn model_page(&self, id: &str) -> String {
        format!("{}/models/{id}", self.settings.endpoint())
    }

    fn bare(&self, id: &str) -> RemoteModel {
        let (publisher, name) = id.split_once('/').unwrap_or(("", id));
        RemoteModel {
            provider: "modelscope".into(),
            id: id.into(),
            name: name.into(),
            publisher: publisher.into(),
            description: None,
            formats: detect_formats(&[], None, id),
            tags: vec![],
            capabilities: capabilities(&[], Some("text-generation"), id),
            downloads: None,
            likes: None,
            updated_at: None,
            license: None,
            gated: false,
            params_b: params_from_name(name),
            url: self.model_page(id),
        }
    }

    fn item_model(&self, v: &Value) -> Option<RemoteModel> {
        let name = v["Name"].as_str()?;
        let owner = v["Path"].as_str().or_else(|| v["Namespace"].as_str())?;
        let id = format!("{owner}/{name}");
        if !valid_ref(&id) {
            return None;
        }
        let tags: Vec<String> = ["Tags", "Libraries", "Frameworks"]
            .iter()
            .flat_map(|k| v[*k].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(|s| s.to_ascii_lowercase())))
            .collect();
        let mut m = self.bare(&id);
        m.formats = detect_formats(&tags, None, &id);
        m.capabilities = capabilities(&tags, Some("text-generation"), &id);
        m.description = v["ChineseName"].as_str().filter(|s| !s.is_empty()).map(String::from);
        m.downloads = v["Downloads"].as_u64();
        m.likes = v["Stars"].as_u64();
        m.updated_at = v["LastUpdatedTime"].as_i64().and_then(chrono_like).or_else(|| v["LastUpdatedTime"].as_str().map(String::from));
        m.license = v["License"].as_str().filter(|s| !s.is_empty()).map(String::from);
        m.tags = tags;
        Some(m)
    }
}

/// Unix seconds → RFC 3339 UTC, without pulling a date crate into this one.
fn chrono_like(secs: i64) -> Option<String> {
    if secs <= 0 {
        return None;
    }
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Some(format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60))
}

#[async_trait]
impl ModelProvider for ModelScopeProvider {
    fn info(&self) -> ModelProviderInfo {
        ModelProviderInfo {
            id: "modelscope".into(),
            name: NAME.into(),
            description: "Hub cinese di modelli, utile come mirror. La ricerca è best effort: se non risponde basta il nome esatto proprietario/modello.".into(),
            homepage: "https://modelscope.cn".into(),
            capabilities: ["search", "files", "auth", "multi_file"].map(String::from).to_vec(),
            formats: ["gguf", "safetensors", "onnx"].map(String::from).to_vec(),
            auth: "optional".into(),
            auth_configured: self.settings.has_token(),
            endpoint: self.settings.endpoint(),
        }
    }

    fn settings(&self) -> Option<&ProviderSettings> {
        Some(&self.settings)
    }

    async fn search(&self, q: &SearchQuery) -> Result<SearchPage> {
        let page: u32 = q.cursor.as_deref().and_then(|c| c.parse().ok()).unwrap_or(1).max(1);
        let limit = q.limit();
        let query = q.q.trim();
        let url = format!("{}/api/v1/dolphin/models", self.settings.endpoint());
        let mut name = query.to_string();
        if let Some(f) = q.format.as_deref().filter(|f| !f.is_empty()) {
            name = format!("{name} {f}").trim().to_string();
        }
        let body = json!({"PageSize": limit, "PageNumber": page, "SortBy": "Default", "Target": "", "SingleCriterion": [], "Name": name});
        let attempt = async {
            let mut rb = self.api.client().put(&url).json(&body);
            for (k, v) in self.headers(&url) {
                rb = rb.header(k, v);
            }
            let r = rb.send().await.map_err(|e| net_error(NAME, &e))?;
            if !r.status().is_success() {
                return Err(status_error(NAME, "la ricerca", r.status()));
            }
            let v: Value = r.json().await.map_err(|e| Error::Upstream(format!("{NAME}: risposta di ricerca non valida: {e}")))?;
            let list = v["Data"]["Model"]["Models"]
                .as_array()
                .or_else(|| v["Data"]["Models"].as_array())
                .ok_or_else(|| Error::Upstream(format!("{NAME}: formato della ricerca non riconosciuto")))?;
            let items: Vec<RemoteModel> = list.iter().filter_map(|i| self.item_model(i)).collect();
            let total = v["Data"]["Model"]["TotalCount"].as_u64().or_else(|| v["Data"]["TotalCount"].as_u64());
            let more = match total {
                Some(t) => (page as u64) * (limit as u64) < t,
                None => list.len() >= limit,
            };
            Ok(SearchPage { items, next_cursor: more.then(|| (page + 1).to_string()) })
        };
        match attempt.await {
            Ok(p) => Ok(p),
            // The site API is not documented: an exact reference still works.
            Err(e) if valid_ref(query) => {
                tracing::debug!("modelscope search failed ({e}); accepting `{query}` as an exact reference");
                Ok(SearchPage { items: vec![self.bare(query)], next_cursor: None })
            }
            Err(e) => Err(match e {
                Error::Upstream(m) => Error::Upstream(format!("{m}; indica il nome esatto nel formato proprietario/modello")),
                other => other,
            }),
        }
    }

    async fn detail(&self, model: &str, revision: Option<&str>) -> Result<RemoteModelDetail> {
        if !valid_ref(model) {
            return Err(Error::Invalid(format!("riferimento ModelScope non valido: `{model}` (formato proprietario/modello)")));
        }
        let ep = self.settings.endpoint();
        let rev = revision.map(str::trim).filter(|r| !r.is_empty()).unwrap_or("master");
        let url = format!("{ep}/api/v1/models/{model}/repo/files?Revision={}&Recursive=true", enc_segment(rev));
        let v = self.api.get_json(NAME, &format!("il modello {model}"), &url, &self.headers(&url)).await?;
        let list = v["Data"]["Files"].as_array().ok_or_else(|| Error::Upstream(format!("{NAME}: elenco dei file non valido")))?;
        let files: Vec<RepoFile> = list
            .iter()
            .filter(|f| f["Type"].as_str().map(|t| t.eq_ignore_ascii_case("blob")).unwrap_or(true))
            .filter_map(|f| {
                Some(RepoFile {
                    path: f["Path"].as_str()?.to_string(),
                    size: f["Size"].as_u64().unwrap_or(0),
                    sha256: f["Sha256"].as_str().map(|s| s.to_ascii_lowercase()).filter(|s| s.len() == 64),
                })
            })
            .collect();
        let mut m = self.bare(model);
        // Metadata is a nicety: a failure here must not hide the files.
        let meta_url = format!("{ep}/api/v1/models/{model}");
        if let Ok(meta) = self.api.get_json(NAME, "i metadati", &meta_url, &self.headers(&meta_url)).await {
            let d = &meta["Data"];
            m.downloads = d["Downloads"].as_u64().or(m.downloads);
            m.likes = d["Stars"].as_u64().or(m.likes);
            m.license = d["License"].as_str().filter(|s| !s.is_empty()).map(String::from).or(m.license);
            m.description = d["Description"].as_str().or_else(|| d["ChineseName"].as_str()).filter(|s| !s.is_empty()).map(String::from);
        }
        let (owner, name) = model.split_once('/').unwrap_or(("", model));
        let variants =
            build_variants(&files, &RepoHints { owner: owner.into(), name: name.into(), tags: m.tags.clone(), library_name: None });
        m.formats = dedup(variants.iter().map(|v| v.format.clone()).collect());
        Ok(RemoteModelDetail { model: m, revision: rev.to_string(), variants })
    }

    fn plan(&self, detail: &RemoteModelDetail, variant: &str) -> Result<Vec<FileJob>> {
        let ep = self.settings.endpoint();
        let id = &detail.model.id;
        if !valid_ref(id) {
            return Err(Error::Invalid(format!("riferimento ModelScope non valido: `{id}`")));
        }
        let v = detail.variants.iter().find(|v| v.id == variant).ok_or_else(|| Error::NotFound(format!("variante `{variant}` di {id}")))?;
        Ok(v.files
            .iter()
            .map(|f| {
                let url =
                    format!("{ep}/api/v1/models/{id}/repo?Revision={}&FilePath={}", enc_segment(&detail.revision), enc_segment(&f.path));
                FileJob {
                    headers: self.auth_header(&url).into_iter().collect(),
                    dest: PathBuf::from("modelscope").join(id).join(&f.path),
                    size: f.size_bytes,
                    sha256: f.sha256.clone(),
                    url,
                }
            })
            .collect())
    }

    fn folder(&self, detail: &RemoteModelDetail, _variant: &str) -> PathBuf {
        PathBuf::from("modelscope").join(&detail.model.id)
    }

    fn auth_header(&self, url: &str) -> Option<(String, String)> {
        let t = self.settings.token()?;
        same_origin(url, &self.settings.endpoint()).then(|| ("Authorization".to_string(), format!("Bearer {t}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path, Query};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::{get, put};
    use axum::{Json, Router};
    use std::collections::HashMap;

    async fn start(search_works: bool) -> String {
        let app = Router::new()
            .route(
                "/api/v1/dolphin/models",
                put(move |Json(b): Json<Value>| async move {
                    if !search_works {
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                    assert_eq!(b["SortBy"], "Default");
                    assert!(b["PageSize"].is_u64());
                    Json(json!({"Code": 200, "Data": {"Model": {"TotalCount": 3, "Models": [
                        {"Name": "Qwen3-4B-GGUF", "Path": "Qwen", "ChineseName": "千问", "Downloads": 99, "Stars": 7, "LastUpdatedTime": 1_767_225_600i64, "Tags": ["gguf"]},
                        {"Name": "weird name", "Path": "x"}
                    ]}}, "Success": true}))
                    .into_response()
                }),
            )
            .route(
                "/api/v1/models/{owner}/{name}/repo/files",
                get(|Path((o, n)): Path<(String, String)>, Query(q): Query<HashMap<String, String>>| async move {
                    assert_eq!((o.as_str(), n.as_str()), ("Qwen", "Qwen3-4B-GGUF"));
                    assert_eq!(q.get("Revision").map(String::as_str), Some("master"));
                    assert_eq!(q.get("Recursive").map(String::as_str), Some("true"));
                    Json(json!({"Code": 200, "Data": {"Files": [
                        {"Name": "d", "Path": "d", "Type": "tree", "Size": 0},
                        {"Name": "a", "Path": "Qwen3-4B-Q4_K_M.gguf", "Type": "blob", "Size": 2500, "Sha256": "a".repeat(64)},
                        {"Name": "b", "Path": "Qwen3-4B-Q8_0.gguf", "Type": "blob", "Size": 4000, "Sha256": "b".repeat(64)},
                        {"Name": "r", "Path": "README.md", "Type": "blob", "Size": 10}
                    ]}}))
                }),
            )
            .route("/api/v1/models/{owner}/{name}", get(|| async { Json(json!({"Data": {"Downloads": 1234, "License": "Apache License 2.0", "Description": "Un modello"}})) }));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn searches_and_lists_files() {
        let base = start(true).await;
        let p = ModelScopeProvider::new(&base);
        let page = p.search(&SearchQuery { q: "qwen".into(), limit: 2, ..Default::default() }).await.unwrap();
        assert_eq!(page.items.len(), 1, "the entry with an invalid name is dropped");
        let m = &page.items[0];
        assert_eq!((m.id.as_str(), m.provider.as_str(), m.downloads, m.likes), ("Qwen/Qwen3-4B-GGUF", "modelscope", Some(99), Some(7)));
        assert_eq!(m.updated_at.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(page.next_cursor.as_deref(), Some("2"));

        let d = p.detail("Qwen/Qwen3-4B-GGUF", None).await.unwrap();
        assert_eq!(d.revision, "master");
        assert_eq!(d.variants.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["Q4_K_M", "Q8_0"]);
        assert_eq!(d.variants[0].files[0].sha256.as_deref(), Some("a".repeat(64).as_str()));
        assert_eq!(d.model.license.as_deref(), Some("Apache License 2.0"));
        let jobs = p.plan(&d, "Q8_0").unwrap();
        assert_eq!(jobs[0].url, format!("{base}/api/v1/models/Qwen/Qwen3-4B-GGUF/repo?Revision=master&FilePath=Qwen3-4B-Q8_0.gguf"));
        assert_eq!(jobs[0].dest, PathBuf::from("modelscope/Qwen/Qwen3-4B-GGUF/Qwen3-4B-Q8_0.gguf"));
        assert!(jobs[0].headers.is_empty());
        p.settings().unwrap().set_token(Some("ms_tok".into()));
        assert_eq!(p.plan(&d, "Q8_0").unwrap()[0].headers[0].1, "Bearer ms_tok");
        assert!(p.detail("a/b/c", None).await.is_err());
    }

    #[tokio::test]
    async fn exact_reference_is_accepted_when_search_fails() {
        let base = start(false).await;
        let p = ModelScopeProvider::new(&base);
        let page = p.search(&SearchQuery { q: "Qwen/Qwen3-4B-GGUF".into(), ..Default::default() }).await.unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, "Qwen/Qwen3-4B-GGUF");
        let err = p.search(&SearchQuery { q: "qwen".into(), ..Default::default() }).await.unwrap_err();
        assert!(matches!(&err, Error::Upstream(m) if m.contains("nome esatto")), "{err}");
    }

    #[test]
    fn converts_unix_time() {
        assert_eq!(chrono_like(0), None);
        assert_eq!(chrono_like(1_767_225_600).as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(chrono_like(951_782_400 + 3661).as_deref(), Some("2000-02-29T01:01:01Z"));
    }
}
