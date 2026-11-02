//! The curated catalog (`catalog/models.json`) as a provider: local search, and downloads that
//! go through the Hugging Face provider of the entry's repository.

use super::hf::HfProvider;
use super::variants::parse_quant;
use super::{FileJob, ModelProvider, SearchQuery};
use aiw_core::api::{CatalogModel, ModelProviderInfo, ModelSource, ModelVariant, RemoteFile, RemoteModel, RemoteModelDetail, SearchPage};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub struct CatalogProvider {
    hf: Arc<HfProvider>,
    entries: Vec<CatalogModel>,
}

impl CatalogProvider {
    pub fn new(hf: Arc<HfProvider>) -> Self {
        Self { hf, entries: crate::builtin_catalog() }
    }

    fn entry(&self, id: &str) -> Result<&CatalogModel> {
        self.entries.iter().find(|c| c.id == id).ok_or_else(|| Error::NotFound(format!("modello `{id}` non trovato nel catalogo")))
    }

    /// `(repo, file)` of a downloadable entry; `file` is empty for whole-repository entries (MLX).
    pub fn target(&self, id: &str) -> Result<(String, String)> {
        match &self.entry(id)?.source {
            ModelSource::Hf { repo, file } => Ok((repo.clone(), file.clone())),
            ModelSource::System { .. } => Err(Error::Invalid("i modelli di sistema non si scaricano: sono già nel dispositivo".into())),
        }
    }

    fn remote_model(&self, c: &CatalogModel) -> RemoteModel {
        let page = match &c.source {
            ModelSource::Hf { repo, .. } => format!("{}/{repo}", self.hf.info().endpoint),
            ModelSource::System { .. } => String::new(),
        };
        RemoteModel {
            provider: "catalog".into(),
            id: c.id.clone(),
            name: c.name.clone(),
            publisher: c.publisher.clone(),
            description: Some(c.description.clone()),
            formats: vec![c.format.clone()],
            tags: c.tags.clone(),
            capabilities: c.capabilities.clone(),
            downloads: None,
            likes: None,
            updated_at: None,
            license: Some(c.license.clone()),
            gated: false,
            params_b: Some(c.params_b),
            url: page,
        }
    }

    /// The same detail seen through the Hugging Face provider (for planning the download).
    pub fn hf_view(&self, detail: &RemoteModelDetail) -> Result<RemoteModelDetail> {
        let (repo, _) = self.target(&detail.model.id)?;
        let mut v = detail.clone();
        v.model.provider = "hf".into();
        v.model.id = repo;
        Ok(v)
    }
}

#[async_trait]
impl ModelProvider for CatalogProvider {
    fn info(&self) -> ModelProviderInfo {
        ModelProviderInfo {
            id: "catalog".into(),
            name: "Consigliati".into(),
            description: "Modelli selezionati e provati con AI Workspace Local, con la compatibilità per questo dispositivo.".into(),
            homepage: String::new(),
            capabilities: ["search", "files"].map(String::from).to_vec(),
            formats: ["gguf", "mlx", "litertlm"].map(String::from).to_vec(),
            auth: "none".into(),
            auth_configured: false,
            endpoint: String::new(),
        }
    }

    async fn search(&self, q: &SearchQuery) -> Result<SearchPage> {
        let words: Vec<String> = q.q.split_whitespace().map(str::to_lowercase).collect();
        let fmt = q.format.as_deref().map(str::to_lowercase).filter(|f| !f.is_empty());
        let all: Vec<RemoteModel> = self
            .entries
            .iter()
            .filter(|c| c.format != "system" && fmt.as_ref().map(|f| *f == c.format).unwrap_or(true))
            .filter(|c| {
                let hay = format!(
                    "{} {} {} {} {} {} {} {}",
                    c.id,
                    c.name,
                    c.family,
                    c.publisher,
                    c.description,
                    c.tags.join(" "),
                    c.format,
                    c.quant
                )
                .to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .map(|c| self.remote_model(c))
            .collect();
        let limit = q.limit();
        let offset: usize = q.cursor.as_deref().and_then(|c| c.parse().ok()).unwrap_or(0);
        let more = all.len() > offset + limit;
        Ok(SearchPage {
            items: all.into_iter().skip(offset).take(limit).collect(),
            next_cursor: more.then(|| (offset + limit).to_string()),
        })
    }

    async fn detail(&self, model: &str, revision: Option<&str>) -> Result<RemoteModelDetail> {
        let c = self.entry(model)?;
        let (repo, file) = self.target(model)?;
        let mut m = self.remote_model(c);
        // Real listing (sha256, shards, mmproj) when the Hub answers; the entry's own file otherwise.
        let listing = tokio::time::timeout(Duration::from_secs(12), self.hf.detail(&repo, revision)).await;
        let wanted = |v: &ModelVariant| {
            file.is_empty()
                || v.files.iter().any(|f| f.role == "weights" && (f.path == file || f.path.rsplit('/').next() == Some(file.as_str())))
        };
        match listing {
            Ok(Ok(d)) => {
                let variants: Vec<ModelVariant> = d.variants.into_iter().filter(wanted).collect();
                if !variants.is_empty() {
                    m.formats = vec![c.format.clone()];
                    return Ok(RemoteModelDetail { model: m, revision: d.revision, variants });
                }
                if file.is_empty() {
                    return Err(Error::NotFound(format!("nessun file scaricabile nel repository {repo}")));
                }
            }
            Ok(Err(e @ (Error::NotFound(_) | Error::Unauthorized(_) | Error::Denied(_)))) => return Err(e),
            Ok(Err(e)) if file.is_empty() => return Err(e),
            Err(_) if file.is_empty() => {
                return Err(Error::Upstream(format!("Hugging Face non risponde: impossibile elencare i file di {repo}")))
            }
            _ => {}
        }
        // Offline fallback for single-file entries: sizes in the catalog are rounded, so the file
        // size is left unknown (0) and the downloader trusts Content-Length.
        let stem = file.rsplit('/').next().unwrap_or(&file).trim_end_matches(".gguf").to_string();
        let id = parse_quant(&file).unwrap_or(stem);
        let variant = ModelVariant {
            id: id.clone(),
            label: id.clone(),
            format: c.format.clone(),
            quant: Some(c.quant.clone()),
            files: vec![RemoteFile { path: file, size_bytes: 0, sha256: None, role: "weights".into() }],
            size_bytes: c.size_bytes,
            fit: String::new(),
            fit_detail: String::new(),
            recommended: false,
            runtimes: vec![],
            installed: false,
        };
        Ok(RemoteModelDetail { model: m, revision: "main".into(), variants: vec![variant] })
    }

    fn plan(&self, detail: &RemoteModelDetail, variant: &str) -> Result<Vec<FileJob>> {
        self.hf.plan(&self.hf_view(detail)?, variant)
    }

    fn folder(&self, detail: &RemoteModelDetail, variant: &str) -> PathBuf {
        match self.hf_view(detail) {
            Ok(v) => self.hf.folder(&v, variant),
            Err(_) => PathBuf::from("hf"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::hf::fake::*;
    use super::*;

    fn provider(endpoint: &str) -> CatalogProvider {
        CatalogProvider::new(Arc::new(HfProvider::new(endpoint)))
    }

    #[tokio::test]
    async fn searches_the_curated_entries_locally() {
        let p = provider("http://127.0.0.1:1");
        let q = |s: &str, f: Option<&str>| SearchQuery { q: s.into(), format: f.map(String::from), limit: 50, ..Default::default() };
        let all = p.search(&q("", None)).await.unwrap().items;
        assert!(all.len() >= 14 && all.iter().all(|m| m.provider == "catalog" && m.formats[0] != "system"));
        let qwen = p.search(&q("qwen3 coder", None)).await.unwrap().items;
        assert_eq!(qwen.len(), 1);
        assert_eq!(qwen[0].id, "qwen3-coder-30b-a3b");
        assert_eq!(qwen[0].url, "http://127.0.0.1:1/unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF");
        let mlx = p.search(&q("", Some("mlx"))).await.unwrap().items;
        assert_eq!(mlx.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["qwen3-4b-mlx"]);
        assert!(p.search(&q("foundation", None)).await.unwrap().items.is_empty(), "system models are not downloadable");
        assert!(p.target("apple-fm-system").is_err());
        assert!(matches!(p.detail("nope", None).await, Err(Error::NotFound(_))));
    }

    #[tokio::test]
    async fn single_file_entries_fall_back_to_their_own_file_when_the_hub_is_down() {
        let p = provider("http://127.0.0.1:1");
        let d = p.detail("qwen3-4b", None).await.unwrap();
        assert_eq!(d.variants.len(), 1);
        let v = &d.variants[0];
        assert_eq!(v.id, "Q4_K_M");
        assert_eq!(v.files[0].path, "Qwen3-4B-Q4_K_M.gguf");
        assert_eq!(v.files[0].size_bytes, 0, "rounded catalog sizes are never enforced");
        let jobs = p.plan(&d, "Q4_K_M").unwrap();
        assert_eq!(jobs[0].url, "http://127.0.0.1:1/Qwen/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q4_K_M.gguf");
        assert_eq!(jobs[0].dest, PathBuf::from("hf/Qwen/Qwen3-4B-GGUF/Qwen3-4B-Q4_K_M.gguf"));
        assert_eq!(p.folder(&d, "Q4_K_M"), PathBuf::from("hf/Qwen/Qwen3-4B-GGUF"));
        // A whole-repository entry cannot be planned without the listing.
        assert!(matches!(p.detail("qwen3-4b-mlx", None).await, Err(Error::Upstream(_))));
    }

    #[tokio::test]
    async fn catalog_entries_use_the_real_listing_when_available() {
        // The fake repository serves GGUF files: the entry's file picks its variant (and the projector).
        let fake = new_fake(None);
        let base = start(fake.clone()).await;
        let mut p = provider(&base);
        p.entries.push(CatalogModel {
            id: "tiny".into(),
            source: ModelSource::Hf { repo: REPO.into(), file: "tiny-Q8_0-00001-of-00002.gguf".into() },
            ..p.entries[0].clone()
        });
        let d = p.detail("tiny", None).await.unwrap();
        assert_eq!(d.variants.len(), 1);
        assert_eq!(d.variants[0].id, "Q8_0");
        assert_eq!(d.variants[0].files.len(), 3);
        assert!(d.variants[0].files.iter().all(|f| f.size_bytes > 0));
        assert_eq!(d.revision, "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(d.model.provider, "catalog");
        let jobs = p.plan(&d, "Q8_0").unwrap();
        assert_eq!(jobs.len(), 3);
        assert!(jobs[0].sha256.is_some());
    }
}
