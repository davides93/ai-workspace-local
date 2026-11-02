//! Multi-provider model hub (docs/architecture/model-hub.md): live search and file listing on
//! Hugging Face, ModelScope and the Ollama registry, direct URLs and the curated catalog, all
//! reduced to one shape (`RemoteModelDetail` with variants) and one download plan (`FileJob`s).

pub mod catalog;
pub mod hf;
mod http;
pub mod modelscope;
pub mod ollama;
pub mod url;
pub mod variants;

use crate::download::DownloadRequest;
use aiw_core::api::{ModelProviderInfo, RemoteModelDetail, SearchPage};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub use http::same_origin;
pub(crate) use http::{net_error, net_why};

/// Where each provider talks to. Overridable so tests (and mirrors) can point elsewhere.
#[derive(Debug, Clone)]
pub struct HubConfig {
    /// `HF_ENDPOINT`, default `https://huggingface.co`.
    pub hf_endpoint: String,
    /// `AIW_MODELSCOPE_ENDPOINT`, default `https://www.modelscope.cn`.
    pub modelscope_endpoint: String,
    /// `AIW_OLLAMA_REGISTRY`, default `https://registry.ollama.ai`.
    pub ollama_registry: String,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            hf_endpoint: "https://huggingface.co".into(),
            modelscope_endpoint: "https://www.modelscope.cn".into(),
            ollama_registry: "https://registry.ollama.ai".into(),
        }
    }
}

impl HubConfig {
    /// Defaults overridden by `HF_ENDPOINT`, `AIW_MODELSCOPE_ENDPOINT` and `AIW_OLLAMA_REGISTRY`.
    pub fn from_env() -> Self {
        let mut c = Self::default();
        let env = |k: &str| std::env::var(k).ok().map(|v| v.trim().trim_end_matches('/').to_string()).filter(|v| !v.is_empty());
        if let Some(v) = env("HF_ENDPOINT") {
            c.hf_endpoint = v;
        }
        if let Some(v) = env("AIW_MODELSCOPE_ENDPOINT") {
            c.modelscope_endpoint = v;
        }
        if let Some(v) = env("AIW_OLLAMA_REGISTRY") {
            c.ollama_registry = v;
        }
        c
    }
}

/// One search request, already validated by the caller.
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub q: String,
    /// gguf | mlx | onnx | safetensors | …
    pub format: Option<String>,
    /// downloads | likes | updated
    pub sort: Option<String>,
    /// 1..=50
    pub limit: usize,
    /// Opaque; produced by a previous page.
    pub cursor: Option<String>,
}

impl SearchQuery {
    pub fn limit(&self) -> usize {
        if self.limit == 0 {
            20
        } else {
            self.limit.min(50)
        }
    }
}

/// One file to download. `dest` is relative to the models directory.
#[derive(Debug, Clone, PartialEq)]
pub struct FileJob {
    pub url: String,
    pub dest: PathBuf,
    /// 0 when unknown.
    pub size: u64,
    pub sha256: Option<String>,
    /// Sent to `url` and to nothing else: the downloader drops them when a redirect leaves the host.
    pub headers: Vec<(String, String)>,
}

/// Result of validating a stored token with the provider.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthCheck {
    /// Accepted; carries the account name when the provider tells it.
    Valid(Option<String>),
    /// Rejected by the provider (401).
    Invalid,
    /// Could not tell (offline, provider without a validation endpoint).
    Unknown,
}

/// Token and endpoint override of a provider, shared between the hub and the provider.
#[derive(Debug)]
pub struct ProviderSettings {
    default_endpoint: String,
    endpoint: RwLock<Option<String>>,
    token: RwLock<Option<String>>,
}

impl ProviderSettings {
    pub fn new(default_endpoint: &str) -> Self {
        Self {
            default_endpoint: default_endpoint.trim_end_matches('/').to_string(),
            endpoint: Default::default(),
            token: Default::default(),
        }
    }
    /// Effective base URL, without a trailing slash.
    pub fn endpoint(&self) -> String {
        self.endpoint.read().unwrap().clone().unwrap_or_else(|| self.default_endpoint.clone())
    }
    pub fn token(&self) -> Option<String> {
        self.token.read().unwrap().clone()
    }
    /// The endpoint the user set, if any (the default is not an override).
    pub fn endpoint_override(&self) -> Option<String> {
        self.endpoint.read().unwrap().clone()
    }
    pub fn has_token(&self) -> bool {
        self.token.read().unwrap().is_some()
    }
    pub fn set_token(&self, t: Option<String>) {
        *self.token.write().unwrap() = t.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    }
    /// `None` or an empty string go back to the default endpoint.
    pub fn set_endpoint(&self, e: Option<String>) -> Result<()> {
        let e = e.map(|e| e.trim().trim_end_matches('/').to_string()).filter(|e| !e.is_empty());
        if let Some(e) = &e {
            let u = reqwest::Url::parse(e).map_err(|_| Error::Invalid(format!("endpoint non valido: {e}")))?;
            if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
                return Err(Error::Invalid(format!("endpoint non valido (serve http o https): {e}")));
            }
        }
        *self.endpoint.write().unwrap() = e;
        Ok(())
    }
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Static description; `auth_configured` and `endpoint` are the current values.
    fn info(&self) -> ModelProviderInfo;
    /// Token and endpoint override, for providers that have them.
    fn settings(&self) -> Option<&ProviderSettings> {
        None
    }
    async fn search(&self, q: &SearchQuery) -> Result<SearchPage>;
    /// Model metadata and variants, without `fit`, `runtimes`, `recommended` and `installed`
    /// (the engine fills those for this device).
    async fn detail(&self, model: &str, revision: Option<&str>) -> Result<RemoteModelDetail>;
    /// Files of one variant, ready to download.
    fn plan(&self, detail: &RemoteModelDetail, variant: &str) -> Result<Vec<FileJob>>;
    /// Folder (relative to the models directory) holding the whole model; the sidecar goes there.
    fn folder(&self, detail: &RemoteModelDetail, variant: &str) -> PathBuf;
    /// `Authorization` for `url`, only when `url` belongs to the provider's own host.
    fn auth_header(&self, _url: &str) -> Option<(String, String)> {
        None
    }
    /// Validates the stored token (best effort).
    async fn check_auth(&self) -> AuthCheck {
        AuthCheck::Unknown
    }
    /// The reference as stored in the install sidecar (Ollama drops the tag, which is the variant).
    fn canonical_ref(&self, model: &str) -> String {
        model.to_string()
    }
}

/// Every provider of the app.
pub struct Hub {
    providers: Vec<Arc<dyn ModelProvider>>,
    hf: Arc<hf::HfProvider>,
    url: Arc<url::UrlProvider>,
    catalog: Arc<catalog::CatalogProvider>,
}

impl Hub {
    pub fn new(cfg: HubConfig) -> Self {
        let hf = Arc::new(hf::HfProvider::new(&cfg.hf_endpoint));
        let ms = Arc::new(modelscope::ModelScopeProvider::new(&cfg.modelscope_endpoint));
        let ol = Arc::new(ollama::OllamaProvider::new(&cfg.ollama_registry));
        let url = Arc::new(url::UrlProvider::new());
        let catalog = Arc::new(catalog::CatalogProvider::new(hf.clone()));
        let providers: Vec<Arc<dyn ModelProvider>> = vec![catalog.clone(), hf.clone(), ms, ol, url.clone()];
        Self { providers, hf, url, catalog }
    }

    pub fn infos(&self) -> Vec<ModelProviderInfo> {
        self.providers.iter().map(|p| p.info()).collect()
    }

    pub fn get(&self, id: &str) -> Result<Arc<dyn ModelProvider>> {
        self.providers
            .iter()
            .find(|p| p.info().id == id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("provider di modelli `{id}` non trovato")))
    }

    pub fn hf(&self) -> &Arc<hf::HfProvider> {
        &self.hf
    }
    pub fn url(&self) -> &Arc<url::UrlProvider> {
        &self.url
    }
    pub fn catalog(&self) -> &Arc<catalog::CatalogProvider> {
        &self.catalog
    }

    /// Stores a token and/or an endpoint override. `token`: `None` leaves it alone, `Some(None)`
    /// clears it. `endpoint`: `None` leaves it alone, `Some("")` goes back to the default.
    pub fn set_auth(&self, id: &str, token: Option<Option<String>>, endpoint: Option<String>) -> Result<()> {
        let p = self.get(id)?;
        let Some(s) = p.settings() else {
            return Err(Error::Invalid(format!("il provider `{id}` non ha impostazioni di accesso")));
        };
        if matches!(token, Some(Some(_))) && p.info().auth == "none" {
            return Err(Error::Invalid(format!("il provider `{id}` non usa token di accesso")));
        }
        if let Some(e) = endpoint {
            s.set_endpoint(Some(e))?;
        }
        if let Some(t) = token {
            s.set_token(t);
        }
        Ok(())
    }

    /// `(token, endpoint override)` of a provider, for persistence.
    pub fn credentials(&self, id: &str) -> Result<(Option<String>, Option<String>)> {
        let p = self.get(id)?;
        Ok(p.settings().map(|s| (s.token(), s.endpoint_override())).unwrap_or_default())
    }

    pub async fn search(&self, provider: &str, q: &SearchQuery) -> Result<SearchPage> {
        self.get(provider)?.search(q).await
    }

    pub async fn detail(&self, provider: &str, model: &str, revision: Option<&str>) -> Result<RemoteModelDetail> {
        self.get(provider)?.detail(model, revision).await
    }

    /// Download request for one variant of an already fetched detail.
    pub fn prepare(&self, provider: &str, detail: &RemoteModelDetail, variant: &str) -> Result<DownloadRequest> {
        if provider == "catalog" {
            // A catalog entry installs as the Hugging Face repository it points to; the download
            // keeps the catalog id so the UI can tie it to the card.
            let mut req = self.prepare("hf", &self.catalog.hf_view(detail)?, variant)?;
            req.model_id = detail.model.id.clone();
            return Ok(req);
        }
        let p = self.get(provider)?;
        let v = detail
            .variants
            .iter()
            .find(|v| v.id == variant)
            .ok_or_else(|| Error::NotFound(format!("variante `{variant}` di {} non trovata", detail.model.id)))?;
        let files = p.plan(detail, variant)?;
        if files.is_empty() {
            return Err(Error::Invalid(format!("la variante `{variant}` non ha file da scaricare")));
        }
        Ok(DownloadRequest {
            provider: provider.to_string(),
            model_id: format!("{provider}:{}", detail.model.id),
            model_ref: detail.model.id.clone(),
            variant: v.id.clone(),
            revision: detail.revision.clone(),
            format: v.format.clone(),
            quant: v.quant.clone(),
            folder: p.folder(detail, variant),
            files,
        })
    }
}

/// Percent-encodes one path segment (keeps unreserved characters).
pub(crate) fn enc_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Percent-encodes a `/`-separated repository path segment by segment.
pub(crate) fn enc_path(p: &str) -> String {
    p.split('/').map(enc_segment).collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_overrides() {
        let c = HubConfig::default();
        assert_eq!(c.hf_endpoint, "https://huggingface.co");
        assert_eq!(c.ollama_registry, "https://registry.ollama.ai");
        let s = ProviderSettings::new("https://huggingface.co/");
        assert_eq!(s.endpoint(), "https://huggingface.co");
        s.set_endpoint(Some("http://127.0.0.1:9/".into())).unwrap();
        assert_eq!(s.endpoint(), "http://127.0.0.1:9");
        assert!(s.set_endpoint(Some("ftp://x".into())).is_err());
        s.set_endpoint(Some(String::new())).unwrap();
        assert_eq!(s.endpoint(), "https://huggingface.co");
        s.set_token(Some("  tok ".into()));
        assert_eq!(s.token().as_deref(), Some("tok"));
        s.set_token(Some("   ".into()));
        assert!(!s.has_token());
    }

    #[test]
    fn encodes_paths() {
        assert_eq!(enc_path("Q4_K_M/a b.gguf"), "Q4_K_M/a%20b.gguf");
        assert_eq!(enc_segment("x+y"), "x%2By");
    }

    #[test]
    fn hub_lists_five_providers_and_rejects_unknown() {
        let h = Hub::new(HubConfig::default());
        let ids: Vec<String> = h.infos().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, ["catalog", "hf", "modelscope", "ollama", "url"]);
        assert!(matches!(h.get("nope"), Err(Error::NotFound(_))));
        assert!(h.set_auth("ollama", Some(Some("x".into())), None).is_err());
        h.set_auth("hf", Some(Some("hf_x".into())), Some("http://127.0.0.1:1".into())).unwrap();
        let hf = h.infos().into_iter().find(|i| i.id == "hf").unwrap();
        assert!(hf.auth_configured);
        assert_eq!(hf.endpoint, "http://127.0.0.1:1");
        h.set_auth("hf", Some(None), None).unwrap();
        assert!(!h.infos().into_iter().find(|i| i.id == "hf").unwrap().auth_configured);
    }
}
