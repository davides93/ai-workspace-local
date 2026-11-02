//! Model hub on the engine (docs/architecture/model-hub.md): providers and their credentials,
//! live search, variants with fit and runtime compatibility for this device, downloads, local
//! models (this device's store and the other apps' stores) and deletion.

use super::*;
use aiw_models::hub::{AuthCheck, SearchQuery};
use aiw_models::relpath::{native_rel, to_logical};
use aiw_models::sidecar;
use std::sync::atomic::Ordering;

/// Runtimes that exist only as manifests: no code behind them yet.
const MANIFEST_ONLY: [&str; 6] = ["windows-ml", "mlx-swift", "core-ai", "executorch", "mlc-llm", "phi-silica"];

/// Provider credentials as persisted in the store. The token never leaves the engine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ProviderDoc {
    pub id: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
}

/// Runtimes able to load `format` on `os`, derived from the manifests (`formats`, `platforms`)
/// and their live status: available ones first, then the others with the reason.
pub(crate) fn runtime_compat(runtimes: &[RuntimeInfo], os: &str, format: &str) -> Vec<RuntimeCompat> {
    let mut out: Vec<RuntimeCompat> = runtimes
        .iter()
        .filter(|r| r.formats.iter().any(|f| f == format) && r.platforms.iter().any(|p| p == os || p == "any"))
        // The registry marks a manifest `unsupported` when this device cannot run it at all
        // (OS or CPU architecture, e.g. MLX on an Intel Mac): such runtimes are not compatible.
        .filter(|r| r.status != "unsupported")
        .map(|r| {
            let (available, reason) = if MANIFEST_ONLY.contains(&r.id.as_str()) {
                (false, "non ancora implementato".to_string())
            } else {
                match r.status.as_str() {
                    "running" | "starting" | "available" => (true, String::new()),
                    "installable" if matches!(r.kind.as_str(), "embedded" | "system") => {
                        (false, "serve il plugin nativo dell'app".to_string())
                    }
                    "installable" => (false, "da installare".to_string()),
                    "stopped" => (false, r.status_detail.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| "non raggiungibile".into())),
                    "error" => (false, r.status_detail.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| "in errore".into())),
                    other => (false, other.to_string()),
                }
            };
            RuntimeCompat { id: r.id.clone(), name: r.name.clone(), available, reason }
        })
        .collect();
    // Stable: manifest order is the order of preference inside each group.
    out.sort_by_key(|c| !c.available);
    out
}

/// Marks the variant to suggest on this device (at most one).
///
/// Among the variants with the best fit (`great`, else `ok`) the GGUF `Q4_K_M` is preferred,
/// otherwise the largest; with no `great`/`ok` variant nothing is recommended.
pub(crate) fn mark_recommended(variants: &mut [ModelVariant]) {
    for v in variants.iter_mut() {
        v.recommended = false;
    }
    let tier = ["great", "ok"].into_iter().find(|t| variants.iter().any(|v| v.fit == *t));
    let Some(tier) = tier else { return };
    let candidates: Vec<usize> = (0..variants.len()).filter(|i| variants[*i].fit == tier).collect();
    let pick = candidates
        .iter()
        .copied()
        .find(|i| variants[*i].format == "gguf" && variants[*i].quant.as_deref().map(|q| q.eq_ignore_ascii_case("Q4_K_M")).unwrap_or(false))
        .or_else(|| candidates.iter().copied().max_by_key(|i| variants[*i].size_bytes));
    if let Some(i) = pick {
        variants[i].recommended = true;
    }
}

fn origin_label(origin: &str) -> &'static str {
    match origin {
        "ollama" => "Ollama",
        "lmstudio" => "LM Studio",
        "hf-cache" => "la cache di Hugging Face",
        _ => "un'altra app",
    }
}

impl Engine {
    // ------------------------------------------------------------------ providers
    pub fn model_providers(&self) -> Vec<ModelProviderInfo> {
        self.0.hub.infos()
    }

    /// Stores (or clears) a provider's token and endpoint override. `token`: `None` leaves it
    /// alone, `Some(None)` clears it; `endpoint`: `None` leaves it alone, `Some("")` restores the
    /// default. The token is checked with the provider when it can be; a definitive refusal is an
    /// error and nothing is stored. It is never returned.
    pub async fn set_provider_auth(&self, id: &str, token: Option<Option<String>>, endpoint: Option<String>) -> Result<ModelProviderInfo> {
        let provider = self.0.hub.get(id)?;
        let before = self.0.hub.credentials(id)?;
        self.0.hub.set_auth(id, token.clone(), endpoint)?;
        if matches!(token, Some(Some(_))) {
            if let AuthCheck::Invalid = provider.check_auth().await {
                let _ = self.0.hub.set_auth(id, Some(before.0), Some(before.1.unwrap_or_default()));
                return Err(Error::Unauthorized(format!("{}: il token non è valido (rifiutato dal servizio)", provider.info().name)));
            }
        }
        let (token_now, endpoint_now) = self.0.hub.credentials(id)?;
        if token_now.is_none() && endpoint_now.is_none() {
            self.0.store.delete_doc("model_provider", id)?;
        } else {
            self.0.store.put_doc(
                "model_provider",
                id,
                &ProviderDoc { id: id.into(), token: token_now.clone(), endpoint: endpoint_now.clone() },
            )?;
        }
        let what = match (&token, &endpoint_now) {
            (Some(Some(_)), _) => "token impostato",
            (Some(None), _) => "token rimosso",
            (None, Some(_)) => "endpoint personalizzato",
            (None, None) => "impostazioni ripristinate",
        };
        self.audit_user("model.provider", &format!("{id}: {what}"));
        Ok(provider.info())
    }

    /// Applies the persisted credentials (and `HF_TOKEN`, when nothing was stored) at startup.
    pub(crate) fn load_provider_settings(hub: &aiw_models::Hub, store: &Store) {
        let docs: Vec<ProviderDoc> = store.list_docs("model_provider").unwrap_or_default();
        for d in &docs {
            if let Err(e) = hub.set_auth(&d.id, Some(d.token.clone()), d.endpoint.clone()) {
                tracing::warn!(provider = %d.id, "ignoring stored provider settings: {e}");
            }
        }
        if !docs.iter().any(|d| d.id == "hf" && d.token.is_some()) {
            if let Ok(t) = std::env::var("HF_TOKEN") {
                let _ = hub.set_auth("hf", Some(Some(t)), None);
            }
        }
    }

    // ------------------------------------------------------------------ search and remote models
    pub async fn search_models(&self, provider: &str, q: SearchQuery) -> Result<SearchPage> {
        self.0.hub.search(provider, &q).await
    }

    /// Variants of a remote model with fit, runtimes and installed state for this device.
    pub async fn remote_model(&self, provider: &str, model: &str, revision: Option<&str>) -> Result<RemoteModelDetail> {
        let mut d = self.0.hub.detail(provider, model, revision).await?;
        self.annotate(&mut d, provider);
        Ok(d)
    }

    fn annotate(&self, d: &mut RemoteModelDetail, provider: &str) {
        let device = self.device();
        let infos = self.0.runtimes.list();
        let index = aiw_models::installed_index(&self.0.models_dir);
        let local = self.0.models.read().unwrap().clone();
        // A catalog entry installs as its Hugging Face repository.
        let (inst_provider, inst_model, legacy_file) = match provider {
            "catalog" => match self.0.hub.catalog().target(&d.model.id) {
                Ok((repo, file)) => ("hf".to_string(), repo, (!file.is_empty()).then_some(file)),
                Err(_) => (provider.to_string(), d.model.id.clone(), None),
            },
            p => (p.to_string(), d.model.id.clone(), None),
        };
        for v in d.variants.iter_mut() {
            let files_size: u64 = v.files.iter().map(|f| f.size_bytes).sum();
            if files_size > 0 {
                v.size_bytes = files_size;
            }
            let (fit, detail) = aiw_models::fit_need(aiw_models::min_memory_mb(v.size_bytes, 0), &v.format, &device);
            v.fit = fit;
            v.fit_detail = detail;
            v.runtimes = runtime_compat(&infos, &device.os, &v.format);
            v.installed = index.iter().any(|e| e.provider == inst_provider && e.model == inst_model && e.variant == v.id)
                || legacy_file
                    .as_ref()
                    .map(|f| local.iter().any(|m| m.origin == "manual" && file_name_of(m.path.as_deref()) == Some(f.as_str())))
                    .unwrap_or(false);
        }
        mark_recommended(&mut d.variants);
    }

    // ------------------------------------------------------------------ local models
    fn local_enrichment(&self, mut models: Vec<LocalModel>) -> Vec<LocalModel> {
        let device = self.device();
        let infos = self.0.runtimes.list();
        let mut by_format: HashMap<String, Vec<RuntimeCompat>> = HashMap::new();
        for m in models.iter_mut() {
            let compat = by_format.entry(m.format.clone()).or_insert_with(|| runtime_compat(&infos, &device.os, &m.format)).clone();
            m.runtimes = compat.iter().map(|c| c.id.clone()).collect();
            m.runtime_compat = compat;
            let kv_mb = if m.format == "gguf" {
                m.path
                    .as_deref()
                    .and_then(|p| aiw_models::gguf_info_cached(Path::new(p)))
                    .and_then(|i| i.kv_cache_bytes(i.context_length().unwrap_or(4096).min(4096)))
                    .map(|b| b >> 20)
                    .unwrap_or(0)
            } else {
                0
            };
            let (fit, detail) = aiw_models::fit_need(aiw_models::min_memory_mb(m.size_bytes, kv_mb), &m.format, &device);
            m.fit = fit;
            m.fit_detail = detail;
        }
        models
    }

    pub fn rescan_models(&self) -> Vec<LocalModel> {
        let roots = self.0.external_roots.read().unwrap().clone();
        let found = self.local_enrichment(aiw_models::scan_all(&self.0.models_dir, &roots));
        *self.0.models.write().unwrap() = found.clone();
        found
    }

    /// Where to look for the other apps' models (tests point it at temporary folders).
    pub fn set_external_roots(&self, roots: aiw_models::ExternalRoots) {
        *self.0.external_roots.write().unwrap() = roots;
    }

    pub fn models(&self) -> Vec<LocalModel> {
        self.rescan_models()
    }

    pub fn catalog(&self) -> Vec<CatalogModel> {
        let device = self.device();
        let infos = self.0.runtimes.list();
        let index = aiw_models::installed_index(&self.0.models_dir);
        let local = self.0.models.read().unwrap().clone();
        aiw_models::builtin_catalog()
            .into_iter()
            .map(|mut c| {
                let (fit, detail) = aiw_models::fit(&c, &device);
                c.fit = fit;
                c.fit_detail = detail;
                c.downloadable = c.format != "system";
                c.runtime_compat = runtime_compat(&infos, &device.os, &c.format);
                c.installed = match &c.source {
                    ModelSource::Hf { repo, file } if !file.is_empty() => {
                        index.iter().any(|e| e.provider == "hf" && e.model == *repo && e.files.iter().any(|f| f == file || f.ends_with(&format!("/{file}"))))
                            // v0.3: the bare file in the models directory (exact file name, not a suffix).
                            || local.iter().any(|m| m.origin == "manual" && file_name_of(m.path.as_deref()) == Some(file.as_str()))
                    }
                    ModelSource::Hf { repo, .. } => index.iter().any(|e| e.provider == "hf" && e.model == *repo && e.variant == "repo"),
                    ModelSource::System { .. } => false,
                };
                c
            })
            .collect()
    }

    // ------------------------------------------------------------------ downloads
    /// Starts (or queues) a download. Accepts the Hub form, a catalog id, and the v0.3 shapes
    /// (`{repo, file}`, a URL).
    pub async fn pull_model(&self, req: PullRequest) -> Result<DownloadProgress> {
        let (label, dreq) = match req {
            PullRequest::Hub { provider, model, variant, revision } => {
                let detail = self.remote_model(&provider, &model, revision.as_deref()).await?;
                let vid = pick_variant(&detail, &variant)?;
                (format!("{provider}:{} ({vid})", detail.model.id), self.0.hub.prepare(&provider, &detail, &vid)?)
            }
            PullRequest::Catalog { id } => {
                self.0.hub.catalog().target(&id)?;
                let detail = self.remote_model("catalog", &id, None).await?;
                let vid = pick_variant(&detail, "")?;
                (format!("catalogo:{id}"), self.0.hub.prepare("catalog", &detail, &vid)?)
            }
            PullRequest::HfFile { repo, file } => {
                let detail = self.hf_file_detail(&repo, &file).await?;
                let vid = detail
                    .variants
                    .iter()
                    .find(|v| v.files.iter().any(|f| f.role == "weights" && (f.path == file || f.path.ends_with(&format!("/{file}")))))
                    .map(|v| v.id.clone())
                    .ok_or_else(|| Error::NotFound(format!("il file {file} non fa parte di nessuna variante scaricabile di {repo}")))?;
                (format!("hf:{repo}/{file}"), self.0.hub.prepare("hf", &detail, &vid)?)
            }
            PullRequest::Url { url, file_name, sha256 } => {
                let detail = self.0.hub.url().detail_with(&url, file_name.as_deref(), sha256.as_deref()).await?;
                (format!("url:{url}"), self.0.hub.prepare("url", &detail, "file")?)
            }
        };
        // Refuse at once (HTTP 507) when the disk cannot hold it, instead of failing in the queue.
        self.0.downloads.preflight(&dreq).await?;
        let p = self.0.downloads.enqueue(dreq)?;
        self.audit_user("model.pull", &label);
        Ok(p)
    }

    /// Variants of `repo` for a v0.3 `{repo, file}` request; when the Hub cannot be reached the
    /// file alone is pulled straight from `resolve/main`.
    async fn hf_file_detail(&self, repo: &str, file: &str) -> Result<RemoteModelDetail> {
        // The file is a repository path: `/`-separated, relative, without `..` (a `\` is not one).
        if !aiw_models::hub::hf::valid_repo(repo) || file.contains("..") || native_rel(file).is_err() {
            return Err(Error::Invalid("serve un repository e un file Hugging Face validi".into()));
        }
        match self.0.hub.detail("hf", repo, None).await {
            Ok(d) => Ok(d),
            Err(e @ (Error::NotFound(_) | Error::Unauthorized(_) | Error::Denied(_) | Error::Invalid(_))) => Err(e),
            Err(e) => {
                tracing::warn!("cannot list {repo} ({e}): pulling {file} directly");
                let name = file.rsplit('/').next().unwrap_or(file);
                let format = aiw_models::hub::url::format_of_file(name).unwrap_or("gguf");
                let id = aiw_models::hub::variants::parse_quant(name).unwrap_or_else(|| name.to_string());
                let model = RemoteModel {
                    provider: "hf".into(),
                    id: repo.into(),
                    name: repo.rsplit('/').next().unwrap_or(repo).into(),
                    publisher: repo.split('/').next().unwrap_or_default().into(),
                    description: None,
                    formats: vec![format.into()],
                    tags: vec![],
                    capabilities: vec![],
                    downloads: None,
                    likes: None,
                    updated_at: None,
                    license: None,
                    gated: false,
                    params_b: None,
                    url: String::new(),
                };
                let variant = ModelVariant {
                    id,
                    label: name.into(),
                    format: format.into(),
                    quant: None,
                    files: vec![RemoteFile { path: file.into(), size_bytes: 0, sha256: None, role: "weights".into() }],
                    size_bytes: 0,
                    fit: String::new(),
                    fit_detail: String::new(),
                    recommended: false,
                    runtimes: vec![],
                    installed: false,
                };
                Ok(RemoteModelDetail { model, revision: "main".into(), variants: vec![variant] })
            }
        }
    }

    /// Download queue; rescans the models only when a download has finished since the last call.
    pub fn downloads(&self) -> Vec<DownloadProgress> {
        let done = self.0.downloads.completed();
        if self.0.downloads_seen.swap(done, Ordering::SeqCst) != done {
            self.rescan_models();
        }
        self.0.downloads.list()
    }

    pub fn cancel_download(&self, id: &str) -> Result<DownloadProgress> {
        let p = self.0.downloads.cancel(id)?;
        self.audit_user("model.pull_cancel", &format!("{}:{} ({})", p.provider, p.model_ref, p.variant));
        Ok(p)
    }

    /// Removes a finished download from the list.
    pub fn clear_download(&self, id: &str) -> Result<()> {
        self.0.downloads.clear(id)
    }

    // ------------------------------------------------------------------ delete
    /// Deletes a model installed in `models.dir`: the files of its install (and the folder once
    /// no other variant lives there) or the single v0.3 file. Models of other apps are never
    /// touched.
    pub fn delete_model(&self, id: &str) -> Result<()> {
        let models = self.rescan_models();
        let m = models.iter().find(|m| m.id == id).ok_or_else(|| Error::NotFound(format!("modello `{id}`")))?;
        if !m.removable {
            return Err(Error::Invalid(format!(
                "«{}» appartiene a {} e AIW lo usa in sola lettura: eliminalo da lì",
                m.name,
                origin_label(&m.origin)
            )));
        }
        let path = PathBuf::from(m.path.as_deref().ok_or_else(|| Error::Invalid("il modello non ha un percorso".into()))?);
        let root = self.0.models_dir.canonicalize().map_err(Error::Io)?;
        // The folder that holds the model must really be inside the models directory (a symlinked
        // sub-folder pointing elsewhere is refused); a link itself is just unlinked.
        let parent =
            path.parent().and_then(|p| p.canonicalize().ok()).ok_or_else(|| Error::Invalid("percorso del modello non valido".into()))?;
        if !parent.starts_with(&root) {
            return Err(Error::Denied("il modello è fuori dalla cartella dei modelli: non lo elimino".into()));
        }
        remove_model(&root, &path)?;
        self.audit_user("model.delete", &format!("{} ({})", m.name, path.display()));
        self.rescan_models();
        Ok(())
    }
}

fn file_name_of(p: Option<&str>) -> Option<&str> {
    p.and_then(|p| Path::new(p).file_name()).and_then(|n| n.to_str())
}

/// The variant to download: the requested one (by id, label or quant), the only one, or the
/// recommended one.
fn pick_variant(d: &RemoteModelDetail, wanted: &str) -> Result<String> {
    let ids = || d.variants.iter().map(|v| v.id.as_str()).collect::<Vec<_>>().join(", ");
    if d.variants.is_empty() {
        return Err(Error::NotFound(format!("nessuna variante scaricabile per {}", d.model.id)));
    }
    let wanted = wanted.trim();
    if !wanted.is_empty() {
        let w = wanted.to_lowercase();
        return d
            .variants
            .iter()
            .find(|v| v.id == wanted)
            .or_else(|| {
                d.variants.iter().find(|v| {
                    v.id.to_lowercase() == w
                        || v.label.to_lowercase() == w
                        || v.quant.as_deref().map(|q| q.to_lowercase()) == Some(w.clone())
                })
            })
            .map(|v| v.id.clone())
            .ok_or_else(|| Error::NotFound(format!("variante `{wanted}` di {}: disponibili {}", d.model.id, ids())));
    }
    if d.variants.len() == 1 {
        return Ok(d.variants[0].id.clone());
    }
    d.variants
        .iter()
        .find(|v| v.recommended)
        .map(|v| v.id.clone())
        .ok_or_else(|| Error::Invalid(format!("indica la variante da scaricare per {}: {}", d.model.id, ids())))
}

/// Removes one model below `root`: through its install record when there is one.
fn remove_model(root: &Path, path: &Path) -> Result<()> {
    let is_link = std::fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false);
    let is_dir = !is_link && path.is_dir();
    // The nearest folder with an install record (below the root).
    let mut cur = if is_dir { Some(path) } else { path.parent() };
    let mut record = None;
    while let Some(dir) = cur {
        let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        if !canon.starts_with(root) || canon == root {
            break;
        }
        if dir.join(sidecar::SIDECAR).is_file() {
            record = Some(dir.to_path_buf());
            break;
        }
        cur = dir.parent();
    }
    match record {
        Some(dir) => {
            // The install record speaks in logical paths (`/` on every OS): compare in that spelling.
            let target = to_logical(path.strip_prefix(&dir).unwrap_or(path));
            let installs = sidecar::read(&dir);
            let inst = installs.iter().find(|i| {
                i.files.iter().any(|f| f.path == target || (is_dir && (target.is_empty() || f.path.starts_with(&format!("{target}/")))))
            });
            match inst {
                Some(inst) => {
                    // Variants share files (the multimodal projector): keep what another install uses.
                    let shared: HashSet<&str> = installs
                        .iter()
                        .filter(|i| i.variant != inst.variant)
                        .flat_map(|i| i.files.iter().map(|f| f.path.as_str()))
                        .collect();
                    for f in inst.files.iter().filter(|f| !shared.contains(f.path.as_str())) {
                        // A record entry that is not a plain relative path never reaches outside `dir`.
                        let Some(p) = f.native(&dir) else { continue };
                        let _ = std::fs::remove_file(&p);
                        let _ = std::fs::remove_file(format!("{}.part", p.display()));
                        prune_empty(&dir, p.parent());
                    }
                    let left = sidecar::remove_variant(&dir, &inst.variant)?;
                    if left.is_empty() {
                        std::fs::remove_dir_all(&dir)?;
                    }
                }
                // A record that does not mention it: fall back to the path itself.
                None => remove_path(path, is_link, is_dir)?,
            }
            prune_empty(root, dir.parent());
        }
        None => {
            remove_path(path, is_link, is_dir)?;
            prune_empty(root, path.parent());
        }
    }
    Ok(())
}

fn remove_path(path: &Path, is_link: bool, is_dir: bool) -> Result<()> {
    if is_link {
        std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path))?;
    } else if is_dir {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Removes empty folders from `from` upwards, stopping at `stop` (never removed).
///
/// Both ends are compared in canonical form: callers hand over a canonical `stop` (the models
/// directory) next to non-canonical folders, and on Windows the two spell the same place
/// differently (`\\?\C:\Users\runneradmin\…` against `C:\Users\RUNNER~1\…`), which would
/// stop the walk before it removed anything.
fn prune_empty(stop: &Path, from: Option<&Path>) {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let stop = canon(stop);
    let mut cur = from.map(Path::to_path_buf);
    while let Some(dir) = cur {
        let here = canon(&dir);
        if here == stop || !here.starts_with(&stop) || std::fs::remove_dir(&dir).is_err() {
            break;
        }
        cur = dir.parent().map(Path::to_path_buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(id: &str, kind: &str, status: &str, formats: &[&str], platforms: &[&str]) -> RuntimeInfo {
        let mut v = serde_json::json!({
            "id": id, "name": id, "kind": kind, "description": "", "platforms": platforms, "accelerators": [], "formats": formats,
            "api": "openai", "capabilities": [], "status": status, "loaded_models": [],
            "settings_schema": {"properties": {}}, "settings": {}
        });
        v["status_detail"] = serde_json::json!("dettaglio");
        serde_json::from_value(v).unwrap()
    }

    fn variant(id: &str, quant: Option<&str>, size: u64, fit: &str) -> ModelVariant {
        ModelVariant {
            id: id.into(),
            label: id.into(),
            format: "gguf".into(),
            quant: quant.map(String::from),
            files: vec![],
            size_bytes: size,
            fit: fit.into(),
            fit_detail: String::new(),
            recommended: false,
            runtimes: vec![],
            installed: false,
        }
    }

    #[test]
    fn runtime_compat_comes_from_manifests_platforms_and_status() {
        let all = vec![
            rt("llama-server", "sidecar", "installable", &["gguf"], &["linux", "macos", "windows"]),
            rt("llama-embedded", "embedded", "running", &["gguf"], &["linux", "macos", "windows", "android", "ios"]),
            rt("ollama", "sidecar", "available", &["gguf"], &["linux", "macos", "windows"]),
            rt("mlx-lm", "sidecar", "installable", &["mlx"], &["macos"]),
            rt("mlx-swift", "embedded", "installable", &["mlx"], &["macos", "ios"]),
            rt("executorch", "embedded", "installable", &["pte"], &["linux"]),
            rt("litert-lm", "embedded", "installable", &["litertlm"], &["linux", "android"]),
            rt("lmstudio", "sidecar", "installable", &["gguf", "mlx"], &["linux", "macos", "windows"]),
        ];
        let gguf = runtime_compat(&all, "linux", "gguf");
        assert_eq!(
            gguf.iter().map(|c| (c.id.as_str(), c.available)).collect::<Vec<_>>(),
            [("llama-embedded", true), ("ollama", true), ("llama-server", false), ("lmstudio", false)]
        );
        assert_eq!(gguf[2].reason, "da installare");
        assert_eq!(gguf[0].reason, "");
        // Manifest-only runtimes are never available.
        let mlx = runtime_compat(&all, "macos", "mlx");
        assert_eq!(mlx.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["mlx-lm", "mlx-swift", "lmstudio"]);
        let swift = mlx.iter().find(|c| c.id == "mlx-swift").unwrap();
        assert_eq!((swift.available, swift.reason.as_str()), (false, "non ancora implementato"));
        assert_eq!(runtime_compat(&all, "linux", "pte")[0].reason, "non ancora implementato");
        // Not on this OS: not listed at all.
        assert!(runtime_compat(&all, "linux", "mlx").iter().all(|c| c.id != "mlx-lm" && c.id != "mlx-swift"));
        // Unsupported on this device (e.g. MLX on an Intel Mac): not listed either.
        let intel_mac = vec![rt("mlx-lm", "sidecar", "unsupported", &["mlx"], &["macos"])];
        assert!(runtime_compat(&intel_mac, "macos", "mlx").is_empty());
        assert_eq!(runtime_compat(&all, "linux", "litertlm")[0].reason, "serve il plugin nativo dell'app");
        assert!(runtime_compat(&all, "linux", "unknown-format").is_empty());
    }

    #[test]
    fn recommends_one_variant_per_the_contract() {
        // Q4_K_M wins among the variants with the best fit...
        let mut v = vec![
            variant("Q2_K", Some("Q2_K"), 1_000, "great"),
            variant("Q4_K_M", Some("Q4_K_M"), 2_500, "great"),
            variant("Q8_0", Some("Q8_0"), 4_000, "great"),
            variant("F16", Some("F16"), 8_000, "ok"),
        ];
        mark_recommended(&mut v);
        assert_eq!(v.iter().filter(|x| x.recommended).map(|x| x.id.as_str()).collect::<Vec<_>>(), ["Q4_K_M"]);
        // ...but a Q4_K_M that fits worse than another variant does not.
        let mut v = vec![
            variant("Q3_K_S", Some("Q3_K_S"), 1_800, "great"),
            variant("Q4_K_M", Some("Q4_K_M"), 2_500, "ok"),
            variant("Q5_K_M", Some("Q5_K_M"), 3_000, "ok"),
        ];
        mark_recommended(&mut v);
        assert_eq!(v.iter().filter(|x| x.recommended).map(|x| x.id.as_str()).collect::<Vec<_>>(), ["Q3_K_S"]);
        // No Q4_K_M: the largest of the best tier. Only `ok`: the largest `ok`.
        let mut v = vec![
            variant("Q5_K_M", Some("Q5_K_M"), 3_000, "ok"),
            variant("Q6_K", Some("Q6_K"), 3_600, "ok"),
            variant("Q8_0", Some("Q8_0"), 5_000, "tight"),
        ];
        mark_recommended(&mut v);
        assert_eq!(v.iter().filter(|x| x.recommended).map(|x| x.id.as_str()).collect::<Vec<_>>(), ["Q6_K"]);
        // Nothing fits: nothing is recommended; stale flags are cleared.
        let mut v = vec![variant("Q8_0", Some("Q8_0"), 50_000, "no"), variant("Q4_K_M", Some("Q4_K_M"), 25_000, "tight")];
        v[0].recommended = true;
        mark_recommended(&mut v);
        assert!(v.iter().all(|x| !x.recommended));
        // Repositories (not GGUF): the single variant.
        let mut v = vec![ModelVariant { format: "mlx".into(), ..variant("repo", None, 2_300, "great") }];
        mark_recommended(&mut v);
        assert!(v[0].recommended);
    }

    #[test]
    fn picks_variants_by_id_label_quant_or_recommendation() {
        let mut v = vec![variant("Q4_K_M", Some("Q4_K_M"), 2_500, "great"), variant("Q8_0", Some("Q8_0"), 4_000, "great")];
        v[1].label = "Q8 grande".into();
        let d = RemoteModelDetail {
            model: RemoteModel {
                provider: "hf".into(),
                id: "a/b".into(),
                name: "b".into(),
                publisher: "a".into(),
                description: None,
                formats: vec![],
                tags: vec![],
                capabilities: vec![],
                downloads: None,
                likes: None,
                updated_at: None,
                license: None,
                gated: false,
                params_b: None,
                url: String::new(),
            },
            revision: "main".into(),
            variants: v,
        };
        assert_eq!(pick_variant(&d, "Q8_0").unwrap(), "Q8_0");
        assert_eq!(pick_variant(&d, "q8_0").unwrap(), "Q8_0");
        assert_eq!(pick_variant(&d, "Q8 grande").unwrap(), "Q8_0");
        assert!(matches!(pick_variant(&d, "IQ1_S"), Err(Error::NotFound(m)) if m.contains("Q4_K_M, Q8_0")));
        // Empty: needs a recommendation when there are several.
        assert!(matches!(pick_variant(&d, ""), Err(Error::Invalid(_))));
        let mut d2 = d.clone();
        d2.variants[0].recommended = true;
        assert_eq!(pick_variant(&d2, "").unwrap(), "Q4_K_M");
        d2.variants.truncate(1);
        assert_eq!(pick_variant(&d2, "").unwrap(), "Q4_K_M");
        d2.variants.clear();
        assert!(matches!(pick_variant(&d2, ""), Err(Error::NotFound(_))));
    }

    async fn engine_in(data: &Path) -> Engine {
        let ws = data.join("ws");
        Engine::new(EngineOptions {
            data_dir: Some(data.into()),
            workspace_root: Some(ws),
            mode: "embedded".into(),
            offline: true,
            ..Default::default()
        })
        .await
        .unwrap()
    }

    /// A minimal GGUF header (architecture qwen3, name "Qwen3 Test", Q4_K_M).
    fn tiny_gguf() -> Vec<u8> {
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
        b
    }

    fn put(p: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn record(dir: &Path, model: &str, variant: &str, format: &str, files: &[&str]) {
        sidecar::upsert(
            dir,
            sidecar::Install {
                provider: "hf".into(),
                model: model.into(),
                variant: variant.into(),
                revision: "rev".into(),
                format: format.into(),
                quant: Some(variant.into()),
                files: files.iter().map(|f| sidecar::SidecarFile { path: f.to_string(), size_bytes: 1, sha256: None }).collect(),
                installed_at: "2026-01-01T00:00:00.000Z".into(),
            },
        )
        .unwrap();
    }

    #[tokio::test]
    async fn provider_credentials_persist_and_never_come_back() {
        let d = tempfile::tempdir().unwrap();
        let e = engine_in(d.path()).await;
        let ids: Vec<String> = e.model_providers().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["catalog", "hf", "modelscope", "ollama", "url"]);
        let info = e.set_provider_auth("modelscope", Some(Some("ms_secret".into())), Some("http://127.0.0.1:9/".into())).await.unwrap();
        assert!(info.auth_configured);
        assert_eq!(info.endpoint, "http://127.0.0.1:9");
        assert!(!serde_json::to_string(&e.model_providers()).unwrap().contains("ms_secret"));
        assert!(e.audit().iter().any(|a| a.action == "model.provider" && !a.detail.contains("ms_secret")));
        assert!(e.set_provider_auth("ollama", Some(Some("x".into())), None).await.is_err(), "no token for a provider without auth");
        assert!(matches!(e.set_provider_auth("nope", None, None).await, Err(Error::NotFound(_))));
        assert!(e.set_provider_auth("modelscope", None, Some("ftp://x".into())).await.is_err());
        drop(e);
        // A new engine on the same data directory has them again.
        let e = engine_in(d.path()).await;
        let ms = e.model_providers().into_iter().find(|p| p.id == "modelscope").unwrap();
        assert!(ms.auth_configured);
        assert_eq!(ms.endpoint, "http://127.0.0.1:9");
        // Clearing both removes the stored document.
        e.set_provider_auth("modelscope", Some(None), Some(String::new())).await.unwrap();
        let ms = e.model_providers().into_iter().find(|p| p.id == "modelscope").unwrap();
        assert!(!ms.auth_configured);
        assert_eq!(ms.endpoint, "https://www.modelscope.cn");
        assert!(e.0.store.get_doc::<ProviderDoc>("model_provider", "modelscope").unwrap().is_none());
    }

    #[tokio::test]
    async fn lists_local_models_with_origin_fit_and_compat_and_other_apps_stores() {
        let d = tempfile::tempdir().unwrap();
        let e = engine_in(d.path()).await;
        let models = e.0.models_dir.clone();
        let g = tiny_gguf();
        put(&models.join("hf/acme/tiny/tiny-Q4_K_M.gguf"), &g);
        record(&models.join("hf/acme/tiny"), "acme/tiny", "Q4_K_M", "gguf", &["tiny-Q4_K_M.gguf"]);
        put(&models.join("old-Q8_0.gguf"), &g);
        let ollama = d.path().join("ollama");
        put(&ollama.join("blobs/sha256-aa"), &g);
        put(
            &ollama.join("manifests/registry.ollama.ai/library/qwen3/4b"),
            serde_json::json!({"layers": [{"mediaType": "application/vnd.ollama.image.model", "digest": "sha256:aa"}]})
                .to_string()
                .as_bytes(),
        );
        e.set_external_roots(aiw_models::ExternalRoots { ollama: Some(ollama), ..Default::default() });
        let all = e.models();
        let by = |id: &str| {
            all.iter().find(|m| m.id == id).unwrap_or_else(|| panic!("{id}: {:?}", all.iter().map(|m| &m.id).collect::<Vec<_>>())).clone()
        };
        let aiw = by("tiny-q4_k_m");
        assert_eq!((aiw.origin.as_str(), aiw.removable), ("aiw", true));
        assert_eq!(aiw.source.as_deref(), Some("hf:acme/tiny@rev#Q4_K_M"));
        assert!(!aiw.fit.is_empty() && !aiw.fit_detail.is_empty());
        assert!(aiw.runtime_compat.iter().any(|c| c.id == "llama-server"));
        assert_eq!(aiw.runtimes, aiw.runtime_compat.iter().map(|c| c.id.clone()).collect::<Vec<_>>());
        assert_eq!((by("old-q8_0").origin.as_str(), by("old-q8_0").removable), ("manual", true));
        let ol = by("ollama:qwen3:4b");
        assert_eq!((ol.origin.as_str(), ol.removable), ("ollama", false));
        // The resolver (profiles) sees external ids too.
        assert!((e.0.resolver)("ollama:qwen3:4b").is_some());
        // Other apps' models are never deleted.
        assert!(matches!(e.delete_model("ollama:qwen3:4b"), Err(Error::Invalid(m)) if m.contains("Ollama")));
        assert!(matches!(e.delete_model("nope"), Err(Error::NotFound(_))));
        // The catalog knows what is installed (exact match) and what can be downloaded.
        let cat = e.catalog();
        assert!(cat.iter().any(|c| c.id == "qwen3-0.6b" && !c.installed && c.downloadable && !c.runtime_compat.is_empty()));
        assert!(cat.iter().any(|c| c.id == "apple-fm-system" && !c.downloadable));
        assert!(cat.iter().find(|c| c.id == "qwen3-4b-mlx").unwrap().downloadable);
        put(&models.join("Qwen3-0.6B-Q8_0.gguf"), &g);
        put(&models.join("xQwen3-1.7B-Q8_0.gguf"), &g); // a suffix match must not mark qwen3-1.7b
        assert!(e.models().len() > all.len());
        let cat = e.catalog();
        assert!(cat.iter().find(|c| c.id == "qwen3-0.6b").unwrap().installed);
        assert!(!cat.iter().find(|c| c.id == "qwen3-1.7b").unwrap().installed);
        put(&models.join("hf/Qwen/Qwen3-4B-GGUF/Qwen3-4B-Q4_K_M.gguf"), &g);
        record(&models.join("hf/Qwen/Qwen3-4B-GGUF"), "Qwen/Qwen3-4B-GGUF", "Q4_K_M", "gguf", &["Qwen3-4B-Q4_K_M.gguf"]);
        e.models();
        assert!(e.catalog().iter().find(|c| c.id == "qwen3-4b").unwrap().installed);
    }

    #[tokio::test]
    async fn deletes_one_variant_at_a_time_and_the_legacy_file() {
        let d = tempfile::tempdir().unwrap();
        let e = engine_in(d.path()).await;
        let models = e.0.models_dir.clone();
        let g = tiny_gguf();
        let repo = models.join("hf/acme/tiny");
        put(&repo.join("tiny-Q4_K_M.gguf"), &g);
        put(&repo.join("sub/tiny-Q8_0-00001-of-00002.gguf"), &g);
        put(&repo.join("sub/tiny-Q8_0-00002-of-00002.gguf"), b"x");
        put(&repo.join("mmproj-F16.gguf"), b"p");
        record(&repo, "acme/tiny", "Q4_K_M", "gguf", &["tiny-Q4_K_M.gguf", "mmproj-F16.gguf"]);
        record(
            &repo,
            "acme/tiny",
            "Q8_0",
            "gguf",
            &["sub/tiny-Q8_0-00001-of-00002.gguf", "sub/tiny-Q8_0-00002-of-00002.gguf", "mmproj-F16.gguf"],
        );
        put(&models.join("legacy-Q4_0.gguf"), &g);
        let repo_dir = models.join("hf/mlx-community/Qwen3-4B-4bit");
        put(&repo_dir.join("config.json"), b"{}");
        put(&repo_dir.join("model.safetensors"), b"w");
        record(&repo_dir, "mlx-community/Qwen3-4B-4bit", "repo", "mlx", &["config.json", "model.safetensors"]);
        let ids: Vec<String> = e.models().into_iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), 4, "{ids:?}");

        let q4 = e.models().into_iter().find(|m| m.source.as_deref().is_some_and(|s| s.ends_with("#Q4_K_M"))).unwrap();
        e.delete_model(&q4.id).unwrap();
        assert!(!repo.join("tiny-Q4_K_M.gguf").exists());
        assert!(repo.join("sub/tiny-Q8_0-00001-of-00002.gguf").exists(), "the other variant stays");
        assert!(repo.join("mmproj-F16.gguf").exists(), "the projector is still used by the other variant");
        assert_eq!(sidecar::read(&repo).len(), 1);
        let q8 = e.models().into_iter().find(|m| m.source.as_deref().is_some_and(|s| s.ends_with("#Q8_0"))).expect("still listed");
        e.delete_model(&q8.id).unwrap();
        assert!(!repo.exists(), "the repo folder goes with its last variant");
        assert!(!models.join("hf/acme").exists(), "empty parents are pruned");
        assert!(models.join("hf/mlx-community").exists());

        e.delete_model("legacy-q4_0").unwrap();
        assert!(!models.join("legacy-Q4_0.gguf").exists());
        let mlx = e.models().into_iter().find(|m| m.format == "mlx").unwrap();
        e.delete_model(&mlx.id).unwrap();
        assert!(!repo_dir.exists() && !models.join("hf").exists());
        assert!(models.exists(), "the models directory itself stays");
        assert!(e.models().is_empty());
        assert_eq!(e.audit().iter().filter(|a| a.action == "model.delete").count(), 4);
    }

    /// The Windows failure in miniature: the models directory is spelled one way (a link, or an 8.3
    /// short name) and its canonical form another. The walk up must still reach and prune the
    /// empty parents.
    #[cfg(unix)]
    #[tokio::test]
    async fn deleting_prunes_empty_parents_when_the_models_dir_is_not_spelled_canonically() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = d.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let e = engine_in(&link).await;
        let models = e.0.models_dir.clone();
        assert_ne!(models.canonicalize().unwrap(), models, "the test needs a non-canonical spelling");
        let repo = models.join("hf/acme/tiny");
        put(&repo.join("tiny-Q4_K_M.gguf"), &tiny_gguf());
        record(&repo, "acme/tiny", "Q4_K_M", "gguf", &["tiny-Q4_K_M.gguf"]);
        let id = e.models().into_iter().next().unwrap().id;
        e.delete_model(&id).unwrap();
        assert!(!repo.exists());
        assert!(!models.join("hf").exists(), "empty parents are pruned");
        assert!(models.exists(), "the models directory itself stays");
        // The helper alone: a canonical stop and folders reached through the link.
        let deep = models.join("a/b/c");
        std::fs::create_dir_all(&deep).unwrap();
        prune_empty(&models.canonicalize().unwrap(), Some(&deep));
        assert!(!models.join("a").exists() && models.exists());
        // It never goes above `stop`, and never removes a folder that still has something in it.
        std::fs::create_dir_all(models.join("keep/x")).unwrap();
        put(&models.join("keep/file"), b"x");
        prune_empty(&models.canonicalize().unwrap(), Some(&models.join("keep/x")));
        assert!(models.join("keep").exists() && !models.join("keep/x").exists());
    }

    #[tokio::test]
    async fn never_deletes_through_a_link_to_the_outside() {
        let d = tempfile::tempdir().unwrap();
        let e = engine_in(d.path()).await;
        let models = e.0.models_dir.clone();
        let outside = d.path().join("outside");
        let g = tiny_gguf();
        put(&outside.join("big-Q4_K_M.gguf"), &g);
        #[cfg(unix)]
        {
            // A linked file: only the link goes away.
            std::os::unix::fs::symlink(outside.join("big-Q4_K_M.gguf"), models.join("big-Q4_K_M.gguf")).unwrap();
            let id = e.models().into_iter().find(|m| m.name == "Qwen3 Test").unwrap().id;
            e.delete_model(&id).unwrap();
            assert!(outside.join("big-Q4_K_M.gguf").exists(), "the target of a link survives");
            assert!(std::fs::symlink_metadata(models.join("big-Q4_K_M.gguf")).is_err());
            // A linked folder: its contents are outside the models directory.
            std::os::unix::fs::symlink(&outside, models.join("linked")).unwrap();
            let m = e.models().into_iter().find(|m| m.path.as_deref().is_some_and(|p| p.contains("linked"))).unwrap();
            assert!(matches!(e.delete_model(&m.id), Err(Error::Denied(_))));
            assert!(outside.join("big-Q4_K_M.gguf").exists());
        }
    }

    #[tokio::test]
    async fn v03_hf_file_pull_goes_straight_to_the_file_when_the_hub_is_unreachable() {
        let d = tempfile::tempdir().unwrap();
        let e = engine_in(d.path()).await;
        e.set_provider_auth("hf", None, Some("http://127.0.0.1:1".into())).await.unwrap();
        let p = e.pull_model(PullRequest::HfFile { repo: "acme/x-GGUF".into(), file: "x-Q4_K_M.gguf".into() }).await.unwrap();
        assert_eq!((p.provider.as_str(), p.model_ref.as_str(), p.variant.as_str(), p.files_total), ("hf", "acme/x-GGUF", "Q4_K_M", 1));
        assert!(Path::new(&p.dest).ends_with("hf/acme/x-GGUF"), "{}", p.dest);
        assert_eq!(p.file, "x-Q4_K_M.gguf");
        assert_eq!(e.cancel_download(&p.id).unwrap().status, "cancelled");
        assert!(e.audit().iter().any(|a| a.action == "model.pull"));
        // Bad input never reaches the network.
        let bad = |repo: &str, file: &str| PullRequest::HfFile { repo: repo.into(), file: file.into() };
        assert!(matches!(e.pull_model(bad("../x", "a.gguf")).await, Err(Error::Invalid(_))));
        assert!(matches!(e.pull_model(bad("a/b", "../a.gguf")).await, Err(Error::Invalid(_))));
        assert!(matches!(e.pull_model(bad("a/b", "")).await, Err(Error::Invalid(_))));
        assert!(matches!(e.pull_model(PullRequest::Catalog { id: "apple-fm-system".into() }).await, Err(Error::Invalid(_))));
        assert!(matches!(e.pull_model(PullRequest::Catalog { id: "nope".into() }).await, Err(Error::NotFound(_))));
        assert!(matches!(
            e.pull_model(PullRequest::Url { url: "ftp://x/y.gguf".into(), file_name: None, sha256: None }).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            e.pull_model(PullRequest::Hub { provider: "nope".into(), model: "a/b".into(), variant: String::new(), revision: None }).await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn downloads_rescan_only_when_one_finishes() {
        let d = tempfile::tempdir().unwrap();
        let e = engine_in(d.path()).await;
        put(&e.0.models_dir.join("a-Q4_K_M.gguf"), &tiny_gguf());
        assert!(e.0.models.read().unwrap().is_empty(), "nothing scanned yet");
        e.downloads();
        assert!(e.0.models.read().unwrap().is_empty(), "polling alone does not rescan");
        // A finished download bumps the counter: the next poll rescans, and only that one.
        e.0.downloads_seen.store(u64::MAX, Ordering::SeqCst);
        e.downloads();
        assert_eq!(e.0.models.read().unwrap().len(), 1);
        put(&e.0.models_dir.join("b-Q4_K_M.gguf"), &tiny_gguf());
        e.downloads();
        assert_eq!(e.0.models.read().unwrap().len(), 1, "no new rescan without a new completion");
        assert!(matches!(e.cancel_download("dl_nope"), Err(Error::NotFound(_))));
        assert!(matches!(e.clear_download("dl_nope"), Err(Error::NotFound(_))));
    }
}
