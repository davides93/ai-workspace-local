//! Local model discovery (docs/architecture/model-hub.md §6): the models directory (the deep
//! `provider/owner/repo` layout read through `.aiw-model.json`, and the v0.3 flat layout) plus,
//! read-only, the stores of Ollama, LM Studio and the Hugging Face cache.

use crate::gguf::{self, GgufInfo};
use crate::hub::variants::shard_info;
use crate::relpath::to_logical;
use crate::sidecar::{self, Install, SIDECAR};
use aiw_core::api::LocalModel;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// `models.dir/<provider>/<owner>/<repo>/<sub-folder>/<file>` is the deepest layout we write.
const MAX_DEPTH: usize = 6;

/// Where other apps keep their models. Overridable so tests never touch the real home.
#[derive(Debug, Clone, Default)]
pub struct ExternalRoots {
    /// `$OLLAMA_MODELS` or `~/.ollama/models` (holds `manifests/` and `blobs/`).
    pub ollama: Option<PathBuf>,
    /// `~/.lmstudio/models` and `~/.cache/lm-studio/models`.
    pub lmstudio: Vec<PathBuf>,
    /// `$HF_HUB_CACHE`, `$HF_HOME/hub` or `~/.cache/huggingface/hub`.
    pub hf_cache: Option<PathBuf>,
}

impl ExternalRoots {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn detect() -> Self {
        let home = dirs_home();
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty()).map(PathBuf::from);
        let ollama = env("OLLAMA_MODELS").or_else(|| home.as_ref().map(|h| h.join(".ollama").join("models")));
        let lmstudio =
            home.iter().flat_map(|h| [h.join(".lmstudio").join("models"), h.join(".cache").join("lm-studio").join("models")]).collect();
        let hf_cache = env("HF_HUB_CACHE")
            .or_else(|| env("HUGGINGFACE_HUB_CACHE"))
            .or_else(|| env("HF_HOME").map(|h| h.join("hub")))
            .or_else(|| home.as_ref().map(|h| h.join(".cache").join("huggingface").join("hub")));
        Self { ollama, lmstudio, hf_cache }
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

// ------------------------------------------------------------------ GGUF header cache

type GgufKey = (PathBuf, u64, u64);

/// Header of a GGUF file, cached by `(path, mtime, size)`: `/api/models` rescans on every call.
pub fn gguf_info_cached(path: &Path) -> Option<Arc<GgufInfo>> {
    static CACHE: OnceLock<Mutex<HashMap<GgufKey, Option<Arc<GgufInfo>>>>> = OnceLock::new();
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let key = (path.to_path_buf(), mtime, meta.len());
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap().get(&key) {
        return hit.clone();
    }
    let info = gguf::read_gguf(path).ok().map(Arc::new);
    let mut c = cache.lock().unwrap();
    if c.len() > 4096 {
        c.clear();
    }
    c.insert(key, info.clone());
    info
}

// ------------------------------------------------------------------ models directory

/// A model plus what is needed to give it a unique id.
struct Scanned {
    model: LocalModel,
    /// Path of the model below its root, lower-cased, without extension, `/`-separated on every OS.
    qualified: String,
    /// A v0.3 file directly in the models directory keeps its short id.
    flat: bool,
}

/// Models of `dir` (origin `aiw` for recorded installs, `manual` for anything else).
pub fn scan_models(dir: &Path) -> Vec<LocalModel> {
    let mut found = Vec::new();
    walk(dir, dir, 0, &mut found);
    finish(found)
}

/// Models of the other apps' stores, read-only. A root that overlaps `models_dir` is skipped
/// (it is already covered by the main scan).
pub fn scan_external(roots: &ExternalRoots, models_dir: &Path) -> Vec<LocalModel> {
    let mut out = Vec::new();
    let ok = |p: &Path| p.is_dir() && !overlaps(p, models_dir);
    if let Some(r) = roots.ollama.as_deref().filter(|r| ok(r)) {
        scan_ollama(r, &mut out);
    }
    for r in roots.lmstudio.iter().filter(|r| ok(r)) {
        scan_lmstudio(r, &mut out);
    }
    if let Some(r) = roots.hf_cache.as_deref().filter(|r| ok(r)) {
        scan_hf_cache(r, &mut out);
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    let mut seen = HashSet::new();
    out.retain(|m| seen.insert(m.id.clone()));
    out
}

/// Models directory plus external stores, sorted by name.
pub fn scan_all(dir: &Path, roots: &ExternalRoots) -> Vec<LocalModel> {
    let mut all = scan_models(dir);
    all.extend(scan_external(roots, dir));
    all.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    all
}

fn overlaps(a: &Path, b: &Path) -> bool {
    let (Ok(a), Ok(b)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) else { return false };
    a.starts_with(&b) || b.starts_with(&a)
}

fn finish(mut found: Vec<Scanned>) -> Vec<LocalModel> {
    found.sort_by(|a, b| a.qualified.cmp(&b.qualified));
    let mut count: HashMap<String, usize> = HashMap::new();
    for s in &found {
        *count.entry(s.model.id.clone()).or_default() += 1;
    }
    let mut kept_short: HashSet<String> = HashSet::new();
    for s in found.iter_mut() {
        if count[&s.model.id] > 1 && !(s.flat && kept_short.insert(s.model.id.clone())) {
            s.model.id = s.qualified.clone();
        }
    }
    let mut out: Vec<LocalModel> = found.into_iter().map(|s| s.model).collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    out
}

/// Id of a model found at `p` below `root`: its logical path (components joined with `/`, never
/// the OS separator), lower-cased, without the model file extension.
fn qualified_id(root: &Path, p: &Path) -> String {
    let s = to_logical(p.strip_prefix(root).unwrap_or(p)).to_lowercase();
    match s.rsplit_once('.') {
        Some((stem, ext)) if !ext.contains('/') && matches!(ext, "gguf" | "litertlm" | "task" | "pte" | "llamafile") => stem.to_string(),
        _ => s,
    }
}

fn names_of(entries: &[std::fs::DirEntry]) -> Vec<String> {
    entries.iter().map(|e| e.file_name().to_string_lossy().to_string()).collect()
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<Scanned>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let entries: Vec<_> = rd.flatten().collect();
    let names = names_of(&entries);
    if names.iter().any(|n| n == SIDECAR) {
        sidecar_models(root, dir, out);
        return;
    }
    // A folder with `.part` files is an install still going (or one that failed): not a model yet.
    if depth > 0 && !has_part_files(dir, 0) {
        if let Some(m) = repo_dir_model(dir, &names, None) {
            out.push(Scanned {
                qualified: qualified_id(root, dir),
                flat: depth == 1,
                model: LocalModel { origin: "manual".into(), removable: true, ..m },
            });
            return;
        }
    }
    for e in entries {
        let p = e.path();
        // fs::metadata follows symlinks (models are often linked from another disk).
        let Ok(meta) = std::fs::metadata(&p) else { continue };
        if meta.is_dir() {
            if depth < MAX_DEPTH {
                walk(root, &p, depth + 1, out);
            }
            continue;
        }
        let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let lower = name.to_lowercase();
        if lower.ends_with(".part") || lower == SIDECAR || lower.ends_with(".tmp") {
            continue;
        }
        let model = if lower.ends_with(".gguf") {
            if lower.contains("mmproj") {
                continue; // multimodal projector, attached to its model by llama-server settings
            }
            // A shard set is one model: only the first shard is listed.
            let stem = name.trim_end_matches(".gguf").trim_end_matches(".GGUF");
            let shards = shard_info(stem);
            if shards.as_ref().map(|s| s.1 != 1).unwrap_or(false) {
                continue;
            }
            // An unfinished download leaves a `.part` next to the file it was writing.
            if names.iter().any(|n| n.eq_ignore_ascii_case(&format!("{name}.part"))) {
                continue;
            }
            // ...or next to any other shard of the same set.
            if let Some((base, _, _)) = &shards {
                if names
                    .iter()
                    .any(|n| n.to_lowercase().ends_with(".gguf.part") && n.starts_with(&format!("{base}-")) && n.contains("-of-"))
                {
                    continue;
                }
            }
            // A set with a missing shard is an install in progress (or a broken copy).
            let size = match &shards {
                Some((base, _, total)) => match shard_total(&p, base, *total) {
                    Some(sum) => sum,
                    None => continue,
                },
                None => meta.len(),
            };
            Some(local_gguf(&p, size))
        } else {
            [(".litertlm", "litertlm"), (".task", "litertlm"), (".pte", "pte"), (".llamafile", "llamafile")]
                .iter()
                .find(|(ext, _)| lower.ends_with(ext))
                .map(|(_, fmt)| plain_file_model(&p, &name, fmt, meta.len()))
        };
        if let Some(m) = model {
            out.push(Scanned {
                qualified: qualified_id(root, &p),
                flat: depth == 0,
                model: LocalModel { origin: "manual".into(), removable: true, ..m },
            });
        }
    }
}

/// Whether `dir` holds a `.part` file (a download not finished), looking a few levels down.
fn has_part_files(dir: &Path, depth: usize) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    rd.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().to_lowercase();
        if name.ends_with(".part") {
            return true;
        }
        depth < 3 && e.path().is_dir() && has_part_files(&e.path(), depth + 1)
    })
}

/// Size of every shard of `base-0000N-of-0000M.gguf` next to `first`.
fn shard_total(first: &Path, base: &str, total: u32) -> Option<u64> {
    let dir = first.parent()?;
    let mut sum = 0;
    for i in 1..=total {
        let name = format!("{base}-{i:05}-of-{total:05}.gguf");
        sum += std::fs::metadata(dir.join(name)).ok()?.len();
    }
    Some(sum)
}

fn plain_file_model(p: &Path, name: &str, fmt: &str, size: u64) -> LocalModel {
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    LocalModel {
        id: stem.to_lowercase(),
        name: name.to_string(),
        path: Some(p.display().to_string()),
        format: fmt.into(),
        size_bytes: size,
        capabilities: vec!["chat".into()],
        runtimes: crate::runtimes_for_format(fmt).into_iter().map(String::from).collect(),
        ..Default::default()
    }
}

/// A folder of weights (MLX, safetensors, ONNX GenAI) as one model; `None` when it has no weights.
fn repo_dir_model(dir: &Path, names: &[String], sidecar_format: Option<&str>) -> Option<LocalModel> {
    let has = |f: &str| names.iter().any(|n| n == f);
    let ext = |e: &str| names.iter().any(|n| n.to_lowercase().ends_with(e));
    let onnx = has("genai_config.json") || ext(".onnx");
    if !(has("config.json") || has("genai_config.json")) || !(onnx || ext(".safetensors")) {
        return None;
    }
    let name = dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let format = if onnx { "onnx" } else { classify_weights_dir(dir, names, sidecar_format) };
    let cfg: Value =
        std::fs::read_to_string(dir.join("config.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let text = cfg.get("text_config").unwrap_or(&cfg);
    let mut caps = vec!["chat".to_string()];
    if name.to_lowercase().contains("embed") {
        caps = vec!["embeddings".into()];
    }
    if cfg.get("vision_config").is_some() {
        caps.push("vision".into());
    }
    Some(LocalModel {
        id: name.to_lowercase(),
        path: Some(dir.display().to_string()),
        format: format.into(),
        size_bytes: dir_size(dir, 0),
        quant: crate::hub::variants::quant_from_repo_name(&name),
        params_b: crate::hub::variants::params_from_name(&name),
        context_length: text.get("max_position_embeddings").and_then(Value::as_u64),
        arch: cfg.get("model_type").and_then(Value::as_str).map(String::from),
        capabilities: caps,
        runtimes: crate::runtimes_for_format(format).into_iter().map(String::from).collect(),
        name,
        ..Default::default()
    })
}

/// mlx vs safetensors, by looking at the files: the install record, the `format` the MLX
/// converter writes in the safetensors header, an MLX-style `quantization` block in `config.json`,
/// or the folder name (never the whole path).
fn classify_weights_dir(dir: &Path, names: &[String], sidecar_format: Option<&str>) -> &'static str {
    if sidecar_format == Some("mlx") {
        return "mlx";
    }
    if sidecar_format == Some("safetensors") {
        return "safetensors";
    }
    if let Some(first) = names.iter().filter(|n| n.to_lowercase().ends_with(".safetensors")).min() {
        if safetensors_format(&dir.join(first)).as_deref() == Some("mlx") {
            return "mlx";
        }
    }
    let quantized = std::fs::read_to_string(dir.join("config.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|c| c.get("quantization").map(Value::is_object).unwrap_or(false))
        .unwrap_or(false);
    let folder = dir.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    if quantized || folder.contains("mlx") {
        "mlx"
    } else {
        "safetensors"
    }
}

/// `__metadata__.format` of a safetensors file (`mlx` for models converted by mlx-lm).
fn safetensors_format(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut len = [0u8; 8];
    f.read_exact(&mut len).ok()?;
    let n = u64::from_le_bytes(len);
    if n == 0 || n > 16 << 20 {
        return None;
    }
    let mut buf = vec![0u8; n as usize];
    f.read_exact(&mut buf).ok()?;
    let v: Value = serde_json::from_slice(&buf).ok()?;
    v.get("__metadata__")?.get("format")?.as_str().map(|s| s.to_lowercase())
}

/// Total size of the files below `dir` (a few levels deep: ONNX repositories nest weights).
fn dir_size(dir: &Path, depth: usize) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .filter_map(|e| std::fs::metadata(e.path()).ok().map(|m| (e.path(), m)))
        .map(|(p, m)| match (m.is_dir(), depth < 4) {
            (false, _) => m.len(),
            (true, true) => dir_size(&p, depth + 1),
            (true, false) => 0,
        })
        .sum()
}

fn parse_params(label: &str) -> Option<f64> {
    crate::parse_params(label)
}

fn local_gguf(p: &Path, size: u64) -> LocalModel {
    let info = gguf_info_cached(p);
    let file = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let raw_stem = file.rsplit_once('.').map(|x| x.0.to_string()).unwrap_or_else(|| file.clone());
    let stem = shard_info(&raw_stem).map(|s| s.0).unwrap_or(raw_stem);
    let name = info.as_ref().and_then(|i| i.str("general.name").map(String::from)).unwrap_or_else(|| stem.clone());
    let quant = info.as_ref().and_then(|i| i.quant().map(String::from)).or_else(|| {
        let up = stem.to_uppercase();
        ["Q4_K_M", "Q5_K_M", "Q6_K", "Q8_0", "Q4_0", "IQ4_XS", "F16", "BF16", "MXFP4"]
            .iter()
            .find(|q| up.contains(*q))
            .map(|q| q.to_string())
    });
    let params_b = info
        .as_ref()
        .and_then(|i| i.str("general.size_label").and_then(parse_params))
        .or_else(|| stem.split(['-', '_']).find_map(parse_params));
    let mut caps = vec!["chat".to_string()];
    let arch = info.as_ref().and_then(|i| i.architecture().map(String::from));
    if let Some(a) = &arch {
        if a.contains("bert") || stem.to_lowercase().contains("embed") {
            caps = vec!["embeddings".into()];
        }
        if a.starts_with("qwen3") || a.starts_with("llama") || a.contains("gpt-oss") || a.starts_with("mistral") {
            caps.push("tools".into());
        }
    }
    if info.as_ref().map(|i| i.has_vision()).unwrap_or(false) || stem.to_lowercase().contains("vl") {
        caps.push("vision".into());
    }
    LocalModel {
        id: stem.to_lowercase(),
        name,
        path: Some(p.display().to_string()),
        format: "gguf".into(),
        size_bytes: size,
        quant,
        params_b,
        context_length: info.as_ref().and_then(|i| i.context_length()),
        arch,
        capabilities: caps,
        runtimes: crate::runtimes_for_format("gguf").into_iter().map(String::from).collect(),
        ..Default::default()
    }
}

// ------------------------------------------------------------------ install records

fn source_of(i: &Install) -> String {
    let rev = if i.revision.is_empty() { String::new() } else { format!("@{}", i.revision) };
    format!("{}:{}{rev}#{}", i.provider, i.model, i.variant)
}

/// Models recorded by `.aiw-model.json` in `dir` (one per installed variant).
fn sidecar_models(root: &Path, dir: &Path, out: &mut Vec<Scanned>) {
    for inst in sidecar::read(dir) {
        let mut files: Vec<(PathBuf, u64)> = Vec::new();
        for f in &inst.files {
            // A path that is not a plain relative one counts as a missing file: stale record.
            match f.native(dir).and_then(|p| std::fs::metadata(&p).ok().filter(|m| m.is_file()).map(|m| (p, m.len()))) {
                Some(found) => files.push(found),
                None => {
                    files.clear();
                    break;
                }
            }
        }
        if files.is_empty() {
            continue; // files deleted by hand: the record is stale
        }
        let size: u64 = files.iter().map(|f| f.1).sum();
        let is_weights = |p: &Path| {
            let n = p.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            !n.contains("mmproj")
        };
        let model = match inst.format.as_str() {
            "gguf" => {
                let Some(first) = files
                    .iter()
                    .map(|f| &f.0)
                    .filter(|p| is_weights(p) && p.extension().map(|e| e.eq_ignore_ascii_case("gguf")).unwrap_or(false))
                    .min()
                else {
                    continue;
                };
                let mut m = local_gguf(first, size);
                if inst.provider == "ollama" {
                    // The registry names the file `model.gguf`: the tag is what identifies it.
                    let display = if inst.variant == "latest" { inst.model.clone() } else { format!("{}:{}", inst.model, inst.variant) };
                    m.id = display.to_lowercase();
                    m.name = display;
                }
                m
            }
            "litertlm" | "pte" | "llamafile" => {
                let (p, _) = &files[0];
                let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                plain_file_model(p, &name, &inst.format, size)
            }
            _ => {
                // A repository (or one ONNX sub-folder of it): the folder is the model.
                let folder = if inst.variant == "repo" { dir.to_path_buf() } else { common_dir(dir, &files) };
                let Ok(rd) = std::fs::read_dir(&folder) else { continue };
                let names = names_of(&rd.flatten().collect::<Vec<_>>());
                let Some(mut m) = repo_dir_model(&folder, &names, Some(&inst.format)) else {
                    continue;
                };
                let repo = inst.model.rsplit('/').next().unwrap_or(&inst.model);
                m.name = repo.to_string();
                m.id = if inst.variant == "repo" {
                    repo.to_lowercase()
                } else {
                    format!("{}-{}", repo.to_lowercase(), inst.variant.to_lowercase())
                };
                m.size_bytes = size;
                m.format = inst.format.clone();
                m.runtimes = crate::runtimes_for_format(&inst.format).into_iter().map(String::from).collect();
                m
            }
        };
        let qualified = {
            let p = model.path.as_deref().map(PathBuf::from).unwrap_or_else(|| dir.to_path_buf());
            qualified_id(root, &p)
        };
        out.push(Scanned {
            qualified,
            flat: false,
            model: LocalModel {
                quant: inst.quant.clone().or(model.quant.clone()),
                source: Some(source_of(&inst)),
                origin: "aiw".into(),
                removable: true,
                ..model
            },
        });
    }
}

/// Deepest folder holding every file of an install (the ONNX variant folder).
fn common_dir(base: &Path, files: &[(PathBuf, u64)]) -> PathBuf {
    let mut dirs: Vec<PathBuf> = files.iter().filter_map(|f| f.0.parent().map(Path::to_path_buf)).collect();
    dirs.sort();
    let Some(mut common) = dirs.first().cloned() else { return base.to_path_buf() };
    for d in &dirs {
        while !d.starts_with(&common) {
            if !common.pop() {
                return base.to_path_buf();
            }
        }
    }
    if common.starts_with(base) {
        common
    } else {
        base.to_path_buf()
    }
}

/// One install found in the models directory.
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledEntry {
    pub provider: String,
    pub model: String,
    pub variant: String,
    pub revision: String,
    /// Folder holding the sidecar (absolute).
    pub folder: PathBuf,
    /// Files of the install, relative to `folder`, logical paths (`/` on every OS).
    pub files: Vec<String>,
}

/// Every recorded install below `dir`, for `installed` flags and deletion.
pub fn installed_index(dir: &Path) -> Vec<InstalledEntry> {
    fn go(dir: &Path, depth: usize, out: &mut Vec<InstalledEntry>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let entries: Vec<_> = rd.flatten().collect();
        if entries.iter().any(|e| e.file_name() == SIDECAR) {
            for i in sidecar::read(dir) {
                if i.files.iter().all(|f| f.native(dir).is_some_and(|p| p.is_file())) {
                    out.push(InstalledEntry {
                        provider: i.provider,
                        model: i.model,
                        variant: i.variant,
                        revision: i.revision,
                        folder: dir.to_path_buf(),
                        files: i.files.into_iter().map(|f| f.path).collect(),
                    });
                }
            }
            return;
        }
        if depth < MAX_DEPTH {
            for e in entries {
                if std::fs::metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false) {
                    go(&e.path(), depth + 1, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    go(dir, 0, &mut out);
    out
}

// ------------------------------------------------------------------ other apps' stores

fn external(m: LocalModel, id: String, origin: &str, source: String) -> LocalModel {
    LocalModel { id, origin: origin.into(), removable: false, source: Some(source), ..m }
}

fn scan_ollama(root: &Path, out: &mut Vec<LocalModel>) {
    let manifests = root.join("manifests");
    let subdirs = |p: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default()
    };
    for host in subdirs(&manifests) {
        for ns in subdirs(&host) {
            for name in subdirs(&ns) {
                // `tag` files; a model name can itself be nested (`hf.co/user/repo`), which the
                // three levels above already cover.
                let Ok(rd) = std::fs::read_dir(&name) else { continue };
                for tag in rd.flatten().map(|e| e.path()).filter(|p| p.is_file()) {
                    let Some(m) = ollama_model(root, &host, &ns, &name, &tag) else { continue };
                    out.push(m);
                }
            }
        }
    }
}

fn ollama_model(root: &Path, host: &Path, ns: &Path, name: &Path, tag: &Path) -> Option<LocalModel> {
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(tag).ok()?).ok()?;
    let layer = manifest["layers"].as_array()?.iter().find(|l| l["mediaType"].as_str() == Some("application/vnd.ollama.image.model"))?;
    let digest = layer["digest"].as_str()?.replace(':', "-");
    if digest.is_empty() || digest.contains(['/', '\\']) {
        return None;
    }
    let blob = root.join("blobs").join(digest);
    let size = std::fs::metadata(&blob).ok()?.len();
    let file = |p: &Path| p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let (host, ns, name, tag) = (file(host), file(ns), file(name), file(tag));
    let display = match (host.as_str(), ns.as_str()) {
        ("registry.ollama.ai", "library") => format!("{name}:{tag}"),
        ("registry.ollama.ai", _) => format!("{ns}/{name}:{tag}"),
        _ => format!("{host}/{ns}/{name}:{tag}"),
    };
    let mut m = local_gguf(&blob, size);
    m.name = display.clone();
    m.params_b = m.params_b.or_else(|| crate::hub::variants::params_from_name(&name));
    Some(external(m, format!("ollama:{display}"), "ollama", format!("ollama:{display}")))
}

fn scan_lmstudio(root: &Path, out: &mut Vec<LocalModel>) {
    fn go(root: &Path, dir: &Path, depth: usize, out: &mut Vec<LocalModel>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(meta) = std::fs::metadata(&p) else { continue };
            if meta.is_dir() {
                if depth < 4 {
                    go(root, &p, depth + 1, out);
                }
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            if !lower.ends_with(".gguf") || lower.contains("mmproj") {
                continue;
            }
            let stem = &name[..name.len() - 5];
            let size = match shard_info(stem) {
                Some((_, i, _)) if i != 1 => continue,
                Some((base, _, total)) => shard_total(&p, &base, total).unwrap_or(meta.len()),
                None => meta.len(),
            };
            let publisher = p
                .strip_prefix(root)
                .ok()
                .and_then(|r| r.components().next())
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .filter(|c| *c != name)
                .unwrap_or_default();
            let id = if publisher.is_empty() { format!("lmstudio:{name}") } else { format!("lmstudio:{publisher}/{name}") };
            out.push(external(local_gguf(&p, size), id.clone(), "lmstudio", id));
        }
    }
    go(root, root, 0, out);
}

fn scan_hf_cache(root: &Path, out: &mut Vec<LocalModel>) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let dir = e.path();
        let Some(rest) = e.file_name().to_string_lossy().strip_prefix("models--").map(String::from) else { continue };
        let repo = match rest.split_once("--") {
            Some((org, name)) => format!("{org}/{name}"),
            None => rest,
        };
        let snaps = dir.join("snapshots");
        // The revision `main` points at; otherwise the newest snapshot.
        let main =
            std::fs::read_to_string(dir.join("refs").join("main")).ok().map(|s| s.trim().to_string()).filter(|s| snaps.join(s).is_dir());
        let rev = main.or_else(|| {
            std::fs::read_dir(&snaps)
                .ok()?
                .flatten()
                .filter(|e| e.path().is_dir())
                .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
        });
        let Some(rev) = rev else { continue };
        let snap = snaps.join(&rev);
        let source = format!("hf-cache:{repo}@{}", rev.chars().take(12).collect::<String>());
        let mut gguf_files = Vec::new();
        collect_files(&snap, &snap, 0, &mut gguf_files);
        let ggufs: Vec<&(String, PathBuf, u64)> =
            gguf_files.iter().filter(|f| f.0.to_lowercase().ends_with(".gguf") && !f.0.to_lowercase().contains("mmproj")).collect();
        if !ggufs.is_empty() {
            for (rel, p, size) in ggufs {
                let file = rel.rsplit('/').next().unwrap_or(rel);
                let stem = file.rsplit_once('.').map(|x| x.0).unwrap_or(file);
                let size = match shard_info(stem) {
                    Some((_, i, _)) if i != 1 => continue,
                    Some((base, _, total)) => shard_total(p, &base, total).unwrap_or(*size),
                    None => *size,
                };
                out.push(external(local_gguf(p, size), format!("hf-cache:{repo}/{rel}"), "hf-cache", source.clone()));
            }
        } else if let Ok(rd) = std::fs::read_dir(&snap) {
            let names = names_of(&rd.flatten().collect::<Vec<_>>());
            if let Some(mut m) = repo_dir_model(&snap, &names, None) {
                m.name = repo.rsplit('/').next().unwrap_or(&repo).to_string();
                out.push(external(m, format!("hf-cache:{repo}"), "hf-cache", source));
            }
        }
    }
}

/// `(logical relative path with '/', native absolute path, size)` of the files below `dir`
/// (symlinks followed).
fn collect_files(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(m) = std::fs::metadata(&p) else { continue };
        if m.is_dir() {
            if depth < 3 {
                collect_files(root, &p, depth + 1, out);
            }
        } else {
            out.push((to_logical(p.strip_prefix(root).unwrap_or(&p)), p, m.len()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidecar::SidecarFile;

    fn write(p: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn install(provider: &str, model: &str, variant: &str, format: &str, files: &[(&str, u64)]) -> Install {
        Install {
            provider: provider.into(),
            model: model.into(),
            variant: variant.into(),
            revision: "rev".into(),
            format: format.into(),
            quant: None,
            files: files.iter().map(|(p, s)| SidecarFile { path: p.to_string(), size_bytes: *s, sha256: None }).collect(),
            installed_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

    fn safetensors(format: Option<&str>) -> Vec<u8> {
        let header = match format {
            Some(f) => format!("{{\"__metadata__\":{{\"format\":\"{f}\"}}}}"),
            None => "{}".to_string(),
        };
        let mut b = (header.len() as u64).to_le_bytes().to_vec();
        b.extend(header.as_bytes());
        b.extend([0u8; 64]);
        b
    }

    #[test]
    fn scans_gguf_and_reads_metadata() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Qwen3-Test-Q4_K_M.gguf"), gguf::tests::synthetic()).unwrap();
        std::fs::write(dir.path().join("mmproj-x.gguf"), b"ignored").unwrap();
        let found = scan_models(dir.path());
        assert_eq!(found.len(), 1);
        let m = &found[0];
        assert_eq!(m.name, "Qwen3 Test");
        assert_eq!(m.quant.as_deref(), Some("Q4_K_M"));
        assert_eq!(m.context_length, Some(40960));
        assert!(m.capabilities.contains(&"tools".to_string()));
        assert_eq!(m.id, "qwen3-test-q4_k_m");
        // The v0.3 flat layout: removable, no provenance.
        assert_eq!((m.origin.as_str(), m.removable, m.source.as_deref()), ("manual", true, None));
    }

    #[test]
    fn installs_in_progress_or_failed_are_not_listed_as_models() {
        let dir = tempfile::tempdir().unwrap();
        let g = gguf::tests::synthetic();
        // Sharded GGUF: the first shard is complete, the second is still a `.part`.
        let big = dir.path().join("hf/acme/big");
        write(&big.join("big-Q8_0-00001-of-00003.gguf"), &g);
        write(&big.join("big-Q8_0-00002-of-00003.gguf.part"), &[0u8; 10]);
        assert!(scan_models(dir.path()).is_empty(), "{:?}", scan_models(dir.path()));
        // Two of three shards on disk and no `.part` (a failed job): still incomplete.
        write(&big.join("big-Q8_0-00002-of-00003.gguf"), &[0u8; 10]);
        std::fs::remove_file(big.join("big-Q8_0-00002-of-00003.gguf.part")).unwrap();
        assert!(scan_models(dir.path()).is_empty());
        // The whole set appears once the last shard lands.
        write(&big.join("big-Q8_0-00003-of-00003.gguf"), &[0u8; 10]);
        let found = scan_models(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].origin.as_str(), found[0].size_bytes), ("manual", g.len() as u64 + 20));
        // A weights folder with a `.part` inside is not a model yet either.
        let mlx = dir.path().join("hf/acme/big-4bit");
        write(&mlx.join("config.json"), b"{}");
        write(&mlx.join("model-00001-of-00004.safetensors"), &safetensors(None));
        write(&mlx.join("model-00002-of-00004.safetensors.part"), &[0u8; 10]);
        assert_eq!(scan_models(dir.path()).len(), 1);
        std::fs::remove_file(mlx.join("model-00002-of-00004.safetensors.part")).unwrap();
        assert_eq!(scan_models(dir.path()).len(), 2);
        // A single file with its own `.part` (restarted download over an old copy) is skipped.
        write(&dir.path().join("solo.gguf"), &g);
        write(&dir.path().join("solo.gguf.part"), &[0u8; 5]);
        assert_eq!(scan_models(dir.path()).len(), 2);
    }

    #[test]
    fn reads_the_deep_layout_through_the_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("hf/acme/tiny-GGUF");
        let g = gguf::tests::synthetic();
        write(&repo.join("Q4_K_M/tiny-Q4_K_M-00001-of-00002.gguf"), &g);
        write(&repo.join("Q4_K_M/tiny-Q4_K_M-00002-of-00002.gguf"), &[0u8; 500]);
        write(&repo.join("mmproj-F16.gguf"), &[0u8; 100]);
        write(&repo.join("tiny-Q8_0.gguf"), &g);
        let mut inst = install(
            "hf",
            "acme/tiny-GGUF",
            "Q4_K_M",
            "gguf",
            &[
                ("Q4_K_M/tiny-Q4_K_M-00001-of-00002.gguf", g.len() as u64),
                ("Q4_K_M/tiny-Q4_K_M-00002-of-00002.gguf", 500),
                ("mmproj-F16.gguf", 100),
            ],
        );
        inst.quant = Some("Q4_K_M".into());
        sidecar::upsert(&repo, inst).unwrap();
        sidecar::upsert(&repo, install("hf", "acme/tiny-GGUF", "Q8_0", "gguf", &[("tiny-Q8_0.gguf", g.len() as u64)])).unwrap();
        let found = scan_models(dir.path());
        assert_eq!(found.len(), 2, "{found:#?}");
        let q4 = found.iter().find(|m| m.source.as_deref().unwrap().ends_with("#Q4_K_M")).unwrap();
        assert_eq!((q4.origin.as_str(), q4.removable), ("aiw", true));
        assert_eq!(q4.source.as_deref(), Some("hf:acme/tiny-GGUF@rev#Q4_K_M"));
        assert_eq!(q4.size_bytes, g.len() as u64 + 500 + 100, "shards and projector all count");
        assert!(q4.path.as_deref().unwrap().ends_with("tiny-Q4_K_M-00001-of-00002.gguf"));
        assert_eq!(q4.quant.as_deref(), Some("Q4_K_M"));
        assert_eq!(q4.format, "gguf");
        // Both variants share the same GGUF name: ids stay unique.
        let ids: HashSet<&str> = found.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids.len(), 2, "{ids:?}");
        // A stale record (file deleted by hand) disappears.
        std::fs::remove_file(repo.join("tiny-Q8_0.gguf")).unwrap();
        assert_eq!(scan_models(dir.path()).len(), 1);
    }

    #[test]
    fn short_ids_are_kept_when_unique() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("hf/acme/tiny/tiny-Q4_K_M.gguf"), &gguf::tests::synthetic());
        sidecar::upsert(&dir.path().join("hf/acme/tiny"), install("hf", "acme/tiny", "Q4_K_M", "gguf", &[("tiny-Q4_K_M.gguf", 1)]))
            .unwrap();
        let found = scan_models(dir.path());
        assert_eq!(found[0].id, "tiny-q4_k_m");
        // The same file name from another repository forces the qualified ids, except for the v0.3 flat file.
        write(&dir.path().join("hf/other/tiny/tiny-Q4_K_M.gguf"), &gguf::tests::synthetic());
        sidecar::upsert(&dir.path().join("hf/other/tiny"), install("hf", "other/tiny", "Q4_K_M", "gguf", &[("tiny-Q4_K_M.gguf", 1)]))
            .unwrap();
        write(&dir.path().join("tiny-Q4_K_M.gguf"), &gguf::tests::synthetic());
        let found = scan_models(dir.path());
        assert_eq!(found.len(), 3);
        let mut ids: Vec<&str> = found.iter().map(|m| m.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["hf/acme/tiny/tiny-q4_k_m", "hf/other/tiny/tiny-q4_k_m", "tiny-q4_k_m"]);
        assert_eq!(found.iter().find(|m| m.id == "tiny-q4_k_m").unwrap().origin, "manual");
    }

    #[test]
    fn ollama_installs_are_named_by_tag_and_repo_installs_by_repo() {
        let dir = tempfile::tempdir().unwrap();
        let tag = dir.path().join("ollama/library/qwen3/4b");
        write(&tag.join("model.gguf"), &gguf::tests::synthetic());
        sidecar::upsert(&tag, install("ollama", "qwen3", "4b", "gguf", &[("model.gguf", 1)])).unwrap();
        let repo = dir.path().join("hf/mlx-community/Qwen3-4B-4bit");
        write(&repo.join("config.json"), b"{\"model_type\":\"qwen3\",\"max_position_embeddings\":40960}");
        write(&repo.join("model.safetensors"), &safetensors(None));
        write(&repo.join("tokenizer.json"), b"{}");
        sidecar::upsert(
            &repo,
            install(
                "hf",
                "mlx-community/Qwen3-4B-4bit",
                "repo",
                "mlx",
                &[("config.json", 10), ("model.safetensors", 10), ("tokenizer.json", 2)],
            ),
        )
        .unwrap();
        let found = scan_models(dir.path());
        let ol = found.iter().find(|m| m.id == "qwen3:4b").expect("ollama id");
        assert_eq!((ol.name.as_str(), ol.format.as_str()), ("qwen3:4b", "gguf"));
        let mlx = found.iter().find(|m| m.format == "mlx").unwrap();
        assert_eq!((mlx.id.as_str(), mlx.name.as_str()), ("qwen3-4b-4bit", "Qwen3-4B-4bit"));
        // A real OS path, in the OS spelling: compared as a path, not as a string.
        assert_eq!(Path::new(mlx.path.as_deref().unwrap()), repo.as_path());
        assert_eq!((mlx.context_length, mlx.arch.as_deref(), mlx.quant.as_deref()), (Some(40960), Some("qwen3"), Some("4bit")));
        assert_eq!(mlx.origin, "aiw");
    }

    #[test]
    fn mlx_is_decided_by_the_files_not_by_the_whole_path() {
        let dir = tempfile::tempdir().unwrap();
        // A plain safetensors checkpoint living under a path that merely contains "mlx".
        let plain = dir.path().join("mlx-experiments/Qwen3-4B-Instruct");
        write(&plain.join("config.json"), b"{}");
        write(&plain.join("model.safetensors"), &safetensors(Some("pt")));
        // An MLX conversion in a folder whose name says nothing: the safetensors header does.
        let converted = dir.path().join("work/Qwen3-4B-4bit");
        write(&converted.join("config.json"), b"{}");
        write(&converted.join("model.safetensors"), &safetensors(Some("mlx")));
        // MLX recognized by its quantization block.
        let quant = dir.path().join("work/Llama-3.2-3B-Instruct-q4");
        write(&quant.join("config.json"), b"{\"quantization\":{\"group_size\":64,\"bits\":4}}");
        write(&quant.join("model.safetensors"), &safetensors(None));
        // The folder name counts.
        let named = dir.path().join("work/gemma-mlx");
        write(&named.join("config.json"), b"{}");
        write(&named.join("model.safetensors"), &safetensors(None));
        let found = scan_models(dir.path());
        let fmt = |name: &str| found.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("{name}: {found:#?}")).format.clone();
        assert_eq!(fmt("Qwen3-4B-Instruct"), "safetensors");
        assert_eq!(fmt("Qwen3-4B-4bit"), "mlx");
        assert_eq!(fmt("Llama-3.2-3B-Instruct-q4"), "mlx");
        assert_eq!(fmt("gemma-mlx"), "mlx");
        // ONNX GenAI folders and folders without weights.
        let onnx = dir.path().join("work/phi-onnx");
        write(&onnx.join("genai_config.json"), b"{}");
        write(&onnx.join("model.onnx"), b"x");
        write(&dir.path().join("work/just-config/config.json"), b"{}");
        let found = scan_models(dir.path());
        assert_eq!(found.iter().find(|m| m.name == "phi-onnx").unwrap().format, "onnx");
        assert!(found.iter().all(|m| m.name != "just-config"));
    }

    #[test]
    fn external_stores_are_read_only_and_prefixed() {
        let home = tempfile::tempdir().unwrap();
        let models = tempfile::tempdir().unwrap();
        let g = gguf::tests::synthetic();
        // Ollama: manifest → blob.
        let ol = home.path().join("ollama");
        write(&ol.join("blobs/sha256-aaaa"), &g);
        write(&ol.join("blobs/sha256-bbbb"), b"proj");
        let manifest = serde_json::json!({"layers": [
            {"mediaType": "application/vnd.ollama.image.model", "digest": "sha256:aaaa", "size": g.len()},
            {"mediaType": "application/vnd.ollama.image.projector", "digest": "sha256:bbbb", "size": 4},
        ]});
        write(&ol.join("manifests/registry.ollama.ai/library/qwen3/4b"), manifest.to_string().as_bytes());
        write(&ol.join("manifests/registry.ollama.ai/someone/tuned/latest"), manifest.to_string().as_bytes());
        write(
            &ol.join("manifests/registry.ollama.ai/library/missing/latest"),
            serde_json::json!({"layers": [{"mediaType": "application/vnd.ollama.image.model", "digest": "sha256:nope"}]})
                .to_string()
                .as_bytes(),
        );
        // LM Studio.
        let lm = home.path().join("lmstudio");
        write(&lm.join("lmstudio-community/Qwen3-4B-GGUF/Qwen3-4B-Q4_K_M.gguf"), &g);
        write(&lm.join("lmstudio-community/Qwen3-4B-GGUF/mmproj-F16.gguf"), b"x");
        write(&lm.join("lmstudio-community/Big/Big-Q4_K_M-00001-of-00002.gguf"), &g);
        write(&lm.join("lmstudio-community/Big/Big-Q4_K_M-00002-of-00002.gguf"), &[0u8; 300]);
        // HF cache: snapshots with files that are symlinks to blobs.
        let hf = home.path().join("hub");
        write(&hf.join("models--acme--tiny-GGUF/blobs/b1"), &g);
        write(&hf.join("models--acme--tiny-GGUF/refs/main"), b"abc123def456789");
        std::fs::create_dir_all(hf.join("models--acme--tiny-GGUF/snapshots/abc123def456789")).unwrap();
        std::fs::create_dir_all(hf.join("models--acme--tiny-GGUF/snapshots/old")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            hf.join("models--acme--tiny-GGUF/blobs/b1"),
            hf.join("models--acme--tiny-GGUF/snapshots/abc123def456789/tiny-Q4_K_M.gguf"),
        )
        .unwrap();
        #[cfg(not(unix))]
        write(&hf.join("models--acme--tiny-GGUF/snapshots/abc123def456789/tiny-Q4_K_M.gguf"), &g);
        write(&hf.join("models--mlx-community--Qwen3-4B-4bit/refs/main"), b"r1");
        write(&hf.join("models--mlx-community--Qwen3-4B-4bit/snapshots/r1/config.json"), b"{}");
        write(&hf.join("models--mlx-community--Qwen3-4B-4bit/snapshots/r1/model.safetensors"), &safetensors(Some("mlx")));
        let roots = ExternalRoots { ollama: Some(ol), lmstudio: vec![lm, home.path().join("nonexistent")], hf_cache: Some(hf) };
        let found = scan_external(&roots, models.path());
        let by_id = |id: &str| {
            found
                .iter()
                .find(|m| m.id == id)
                .unwrap_or_else(|| panic!("{id} not in {:#?}", found.iter().map(|m| &m.id).collect::<Vec<_>>()))
        };
        let q3 = by_id("ollama:qwen3:4b");
        assert_eq!((q3.origin.as_str(), q3.removable, q3.format.as_str(), q3.size_bytes), ("ollama", false, "gguf", g.len() as u64));
        assert!(Path::new(q3.path.as_deref().unwrap()).ends_with("blobs/sha256-aaaa"));
        assert_eq!(by_id("ollama:someone/tuned:latest").name, "someone/tuned:latest");
        assert!(found.iter().all(|m| !m.id.contains("missing")), "a manifest without its blob is skipped");
        let lmm = by_id("lmstudio:lmstudio-community/Qwen3-4B-Q4_K_M.gguf");
        assert_eq!((lmm.origin.as_str(), lmm.removable), ("lmstudio", false));
        assert_eq!(by_id("lmstudio:lmstudio-community/Big-Q4_K_M-00001-of-00002.gguf").size_bytes, g.len() as u64 + 300);
        assert_eq!(found.iter().filter(|m| m.id.contains("Big")).count(), 1, "second shard is not a model");
        assert!(found.iter().all(|m| !m.id.contains("mmproj")));
        let h = by_id("hf-cache:acme/tiny-GGUF/tiny-Q4_K_M.gguf");
        assert_eq!((h.origin.as_str(), h.source.as_deref()), ("hf-cache", Some("hf-cache:acme/tiny-GGUF@abc123def456")));
        let m = by_id("hf-cache:mlx-community/Qwen3-4B-4bit");
        assert_eq!((m.format.as_str(), m.name.as_str()), ("mlx", "Qwen3-4B-4bit"));
        // scan_all merges both worlds; a root inside the models directory is not scanned twice.
        write(&models.path().join("a.gguf"), &g);
        assert!(scan_all(models.path(), &roots).len() > found.len());
        let inside = ExternalRoots { lmstudio: vec![models.path().to_path_buf()], ..Default::default() };
        assert!(scan_external(&inside, models.path()).is_empty());
    }

    #[test]
    fn indexes_the_recorded_installs() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("hf/acme/tiny");
        write(&repo.join("a.gguf"), b"x");
        sidecar::upsert(&repo, install("hf", "acme/tiny", "Q4_K_M", "gguf", &[("a.gguf", 1)])).unwrap();
        sidecar::upsert(&repo, install("hf", "acme/tiny", "Q8_0", "gguf", &[("missing.gguf", 1)])).unwrap();
        let idx = installed_index(dir.path());
        assert_eq!(idx.len(), 1, "an install with missing files is not listed");
        assert_eq!((idx[0].provider.as_str(), idx[0].model.as_str(), idx[0].variant.as_str()), ("hf", "acme/tiny", "Q4_K_M"));
        assert_eq!(idx[0].folder, repo);
        assert_eq!(idx[0].files, ["a.gguf"]);
    }

    #[test]
    fn a_record_pointing_outside_its_folder_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("models");
        let repo = root.join("hf/acme/tiny");
        write(&repo.join("a.gguf"), &gguf::tests::synthetic());
        write(&dir.path().join("outside.gguf"), &gguf::tests::synthetic());
        let files = [("a.gguf", 1), ("../../../../outside.gguf", 1)];
        sidecar::upsert(&repo, install("hf", "acme/tiny", "Q4_K_M", "gguf", &files)).unwrap();
        assert!(installed_index(&root).is_empty(), "a file outside the folder is never part of an install");
        assert!(scan_models(&root).is_empty());
        // The same install without the stray entry is fine, and its files are plain `/` paths.
        sidecar::upsert(&repo, install("hf", "acme/tiny", "Q4_K_M", "gguf", &[("a.gguf", 1)])).unwrap();
        assert_eq!(installed_index(&root)[0].files, ["a.gguf"]);
        assert_eq!(scan_models(&root).len(), 1);
    }

    #[test]
    fn gguf_header_cache_follows_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.gguf");
        std::fs::write(&p, gguf::tests::synthetic()).unwrap();
        let a = gguf_info_cached(&p).unwrap();
        assert!(Arc::ptr_eq(&a, &gguf_info_cached(&p).unwrap()), "second read comes from the cache");
        std::fs::write(&p, b"not a gguf at all").unwrap();
        assert!(gguf_info_cached(&p).is_none(), "a changed file is read again");
        assert!(gguf_info_cached(&dir.path().join("missing.gguf")).is_none());
    }

    #[test]
    fn a_corrupted_header_does_not_break_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(1u64.to_le_bytes());
        b.extend(3u64.to_le_bytes());
        b.extend(b"abc");
        b.extend(9u32.to_le_bytes());
        b.extend(6u32.to_le_bytes());
        b.extend(u64::MAX.to_le_bytes());
        write(&dir.path().join("broken-Q4_K_M.gguf"), &b);
        let found = scan_models(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].quant.as_deref(), Some("Q4_K_M"), "falls back to the file name");
    }
}
