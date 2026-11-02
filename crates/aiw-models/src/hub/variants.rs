//! Pure logic that turns a repository file listing into downloadable variants, and tags into
//! formats and capabilities. No I/O: everything here is unit tested.

use aiw_core::api::{ModelVariant, RemoteFile};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// One file of a repository, as listed by a provider.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoFile {
    pub path: String,
    pub size: u64,
    pub sha256: Option<String>,
}

/// What the provider knows about the repository besides its files.
#[derive(Debug, Clone, Default)]
pub struct RepoHints {
    pub owner: String,
    pub name: String,
    pub tags: Vec<String>,
    pub library_name: Option<String>,
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map(|x| x.0).unwrap_or("")
}

/// Lower-cased extension of the last path component (`""` when there is none).
fn ext_of(path: &str) -> String {
    let n = file_name(path);
    n.rsplit_once('.').map(|x| x.1.to_ascii_lowercase()).unwrap_or_default()
}

fn strip_suffix_ci<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    let cut = s.len().checked_sub(suffix.len())?;
    let tail = s.get(cut..)?;
    tail.eq_ignore_ascii_case(suffix).then(|| &s[..cut])
}

/// Splits `name-00001-of-00003` (stem, no extension) into `(name, 1, 3)`.
pub fn shard_info(stem: &str) -> Option<(String, u32, u32)> {
    let of = stem.rfind("-of-")?;
    let total = &stem[of + 4..];
    let head = &stem[..of];
    let dash = head.rfind('-')?;
    let idx = &head[dash + 1..];
    let digits = |s: &str| !s.is_empty() && s.len() <= 6 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(idx) || !digits(total) {
        return None;
    }
    let (i, t): (u32, u32) = (idx.parse().ok()?, total.parse().ok()?);
    (i >= 1 && t >= 1 && i <= t).then(|| (head[..dash].to_string(), i, t))
}

/// Does `t` look like a llama.cpp quantization name (case-insensitive)?
fn is_quant(t: &str) -> bool {
    let u = t.to_ascii_uppercase();
    if matches!(u.as_str(), "F32" | "F16" | "BF16" | "MXFP4" | "MXFP4_MOE" | "TQ1_0" | "TQ2_0") {
        return true;
    }
    if let Some(rest) = u.strip_prefix("IQ") {
        return match rest.split_once('_') {
            Some((d, suffix)) => matches!(d, "1" | "2" | "3" | "4") && matches!(suffix, "XXS" | "XS" | "S" | "M" | "NL"),
            None => false,
        };
    }
    if let Some(rest) = u.strip_prefix('Q') {
        let parts: Vec<&str> = rest.split('_').collect();
        let digit_ok = parts[0].len() == 1 && matches!(parts[0].as_bytes()[0], b'1'..=b'8');
        if !digit_ok {
            return false;
        }
        return match &parts[1..] {
            ["0"] | ["1"] | ["K"] => true,
            ["K", s] => matches!(*s, "XXS" | "XS" | "S" | "M" | "L" | "XL"),
            _ => false,
        };
    }
    false
}

/// The quantization label of a GGUF file name, with the `UD-` prefix of Unsloth dynamic quants.
pub fn parse_quant(file: &str) -> Option<String> {
    let stem = strip_suffix_ci(file, ".gguf").unwrap_or(file);
    let stem = shard_info(stem).map(|s| s.0).unwrap_or_else(|| stem.to_string());
    quant_in_text(&stem)
}

fn quant_in_text(text: &str) -> Option<String> {
    let tokens: Vec<&str> = text.split(['-', '.', ' ']).filter(|t| !t.is_empty()).collect();
    let mut found = None;
    for (i, tok) in tokens.iter().enumerate() {
        // The quant may follow an underscore inside the token (`Qwen3_4B_Q4_K_M`).
        let mut starts = vec![0];
        starts.extend(tok.match_indices('_').map(|(p, _)| p + 1));
        if let Some(q) = starts.into_iter().map(|s| &tok[s..]).find(|c| is_quant(c)) {
            let ud = i > 0 && tokens[i - 1].eq_ignore_ascii_case("ud");
            found = Some(format!("{}{}", if ud { "UD-" } else { "" }, q.to_ascii_uppercase()));
        }
    }
    found
}

/// Files that are never part of a model download.
fn is_junk(path: &str) -> bool {
    let n = file_name(path).to_ascii_lowercase();
    let ext = ext_of(path);
    n == ".gitattributes"
        || n == ".gitignore"
        || n == ".ds_store"
        || n.starts_with("readme")
        || n.starts_with("license")
        || n.starts_with("notice")
        || n.starts_with("imatrix")
        || path.starts_with(".git/")
        || path.contains("/.git/")
        || path.starts_with(".cache/")
        || matches!(ext.as_str(), "md" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "pdf" | "ipynb" | "dat" | "mp4")
}

/// Alternative weights that are redundant next to safetensors.
fn is_redundant_weights(path: &str) -> bool {
    let n = file_name(path).to_ascii_lowercase();
    path.starts_with("original/")
        || matches!(ext_of(path).as_str(), "pth" | "pt" | "ckpt" | "msgpack" | "h5" | "ot" | "tflite")
        || (n.ends_with(".bin") && (n.starts_with("pytorch_model") || n.starts_with("tf_model") || n.starts_with("flax_model")))
        || n.starts_with("flax_model")
        || n.starts_with("tf_model")
}

/// weights | mmproj | config | tokenizer | other
pub fn file_role(path: &str) -> &'static str {
    let n = file_name(path).to_ascii_lowercase();
    let ext = ext_of(path);
    if ext == "gguf" && n.contains("mmproj") {
        return "mmproj";
    }
    if matches!(ext.as_str(), "gguf" | "safetensors" | "onnx" | "litertlm" | "pte" | "llamafile" | "bin" | "pt" | "pth" | "task")
        || n.ends_with(".onnx.data")
        || n.ends_with(".onnx_data")
    {
        return "weights";
    }
    if n.ends_with(".safetensors.index.json") || n.ends_with("config.json") {
        return "config";
    }
    if n.starts_with("tokenizer")
        || n.starts_with("vocab")
        || n == "merges.txt"
        || n == "special_tokens_map.json"
        || n.starts_with("chat_template")
        || n == "spiece.model"
        || ext == "tiktoken"
    {
        return "tokenizer";
    }
    "other"
}

fn remote(f: &RepoFile) -> RemoteFile {
    RemoteFile { path: f.path.clone(), size_bytes: f.size, sha256: f.sha256.clone(), role: file_role(&f.path).into() }
}

fn variant(id: String, label: String, format: &str, quant: Option<String>, files: Vec<RemoteFile>) -> ModelVariant {
    let size_bytes = files.iter().map(|f| f.size_bytes).sum();
    ModelVariant {
        id,
        label,
        format: format.into(),
        quant,
        files,
        size_bytes,
        fit: String::new(),
        fit_detail: String::new(),
        recommended: false,
        runtimes: vec![],
        installed: false,
    }
}

struct Group<'a> {
    dir: String,
    base: String,
    quant: Option<String>,
    total: u32,
    parts: BTreeMap<u32, &'a RepoFile>,
}

/// Best multimodal projector: F16 first, then the smallest.
fn best_mmproj<'a>(files: &[&'a RepoFile]) -> Option<&'a RepoFile> {
    files
        .iter()
        .filter(|f| ext_of(&f.path) == "gguf" && file_name(&f.path).to_ascii_lowercase().contains("mmproj"))
        .min_by_key(|f| {
            (!file_name(&f.path).to_ascii_lowercase().contains("f16") || file_name(&f.path).to_ascii_lowercase().contains("bf16"), f.size)
        })
        .copied()
}

fn gguf_variants(files: &[&RepoFile]) -> Vec<ModelVariant> {
    let mmproj = best_mmproj(files);
    let mut groups: BTreeMap<(String, String), Group> = BTreeMap::new();
    for f in files.iter().filter(|f| ext_of(&f.path) == "gguf" && !file_name(&f.path).to_ascii_lowercase().contains("mmproj")) {
        let name = file_name(&f.path);
        let stem = strip_suffix_ci(name, ".gguf").unwrap_or(name);
        let (base, idx, total) = shard_info(stem).unwrap_or((stem.to_string(), 1, 1));
        let dir = dir_of(&f.path).to_string();
        let quant = quant_in_text(&base).or_else(|| dir.rsplit('/').next().and_then(quant_in_text));
        let g = groups.entry((dir.clone(), base.clone())).or_insert_with(|| Group { dir, base, quant, total, parts: BTreeMap::new() });
        g.total = g.total.max(total);
        g.parts.insert(idx, f);
    }
    let mut out: Vec<(ModelVariant, String)> = Vec::new();
    for g in groups.values() {
        // A shard set with a missing part cannot be loaded.
        if g.parts.len() as u32 != g.total || (1..=g.total).any(|i| !g.parts.contains_key(&i)) {
            continue;
        }
        let mut vf: Vec<RemoteFile> = g.parts.values().map(|f| remote(f)).collect();
        if let Some(m) = mmproj {
            vf.push(remote(m));
        }
        let label = g.quant.clone().unwrap_or_else(|| g.base.clone());
        out.push((variant(label.clone(), label, "gguf", g.quant.clone(), vf), format!("{}/{}", g.dir, g.base)));
    }
    // Unique ids: same quant in two folders or two base names.
    let mut seen = HashSet::new();
    for (v, key) in out.iter_mut() {
        let mut id = v.id.clone();
        if !seen.insert(id.clone()) {
            id = format!("{}@{}", v.id, key.trim_start_matches('/'));
            let mut n = 2;
            while !seen.insert(id.clone()) {
                id = format!("{}@{}-{n}", v.id, key.trim_start_matches('/'));
                n += 1;
            }
        }
        v.id = id;
    }
    let mut vs: Vec<ModelVariant> = out.into_iter().map(|x| x.0).collect();
    vs.sort_by(|a, b| a.size_bytes.cmp(&b.size_bytes).then_with(|| a.id.cmp(&b.id)));
    vs
}

fn quant_from_dir_name(name: &str) -> Option<String> {
    let l = name.to_ascii_lowercase();
    ["int4", "int8", "fp16", "fp32", "bf16", "q4f16", "uint4"].iter().find(|q| l.split(['-', '_']).any(|t| t == **q)).map(|q| q.to_string())
}

fn onnx_variants(files: &[&RepoFile], genai_dirs: Vec<String>) -> Vec<ModelVariant> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let nested: Vec<&String> = genai_dirs.iter().filter(|d| !d.is_empty()).collect();
    for d in &genai_dirs {
        let vf: Vec<RemoteFile> = files
            .iter()
            .filter(|f| {
                if d.is_empty() {
                    !nested.iter().any(|n| f.path.starts_with(&format!("{n}/")))
                } else {
                    f.path.starts_with(&format!("{d}/"))
                }
            })
            .map(|f| remote(f))
            .collect();
        let leaf = if d.is_empty() { "repo".to_string() } else { d.rsplit('/').next().unwrap_or(d).to_string() };
        let mut id = leaf.clone();
        if !seen.insert(id.clone()) {
            id = d.replace('/', "-");
            seen.insert(id.clone());
        }
        let quant = quant_from_dir_name(&leaf);
        out.push(variant(id, leaf, "onnx", quant, vf));
    }
    out.sort_by(|a, b| a.size_bytes.cmp(&b.size_bytes).then_with(|| a.id.cmp(&b.id)));
    out
}

/// Repo-name quantization hint: `Qwen3-4B-4bit` → `4bit`.
pub(crate) fn quant_from_repo_name(name: &str) -> Option<String> {
    let l = name.to_ascii_lowercase();
    l.rsplit(['-', '_'])
        .next()
        .filter(|t| matches!(*t, "bf16" | "fp16" | "fp8") || (t.ends_with("bit") && t[..t.len() - 3].parse::<u8>().is_ok()))
        .map(String::from)
}

/// Every downloadable variant of a repository.
///
/// * GGUF repositories: one variant per quantization (shards kept together, the best `mmproj`
///   attached to every one of them).
/// * ONNX GenAI repositories: one variant per directory holding a `genai_config.json`.
/// * MLX / safetensors / other ONNX repositories: a single `repo` variant with every useful file.
/// * LiteRT-LM, ExecuTorch and llamafile repositories: one variant per weights file.
pub fn build_variants(files: &[RepoFile], hints: &RepoHints) -> Vec<ModelVariant> {
    let files: Vec<&RepoFile> = files.iter().filter(|f| !is_junk(&f.path)).collect();
    let has_ext = |e: &str| files.iter().any(|f| ext_of(&f.path) == e);
    if has_ext("gguf") {
        return gguf_variants(&files);
    }
    let mut genai_dirs: Vec<String> =
        files.iter().filter(|f| file_name(&f.path) == "genai_config.json").map(|f| dir_of(&f.path).to_string()).collect();
    genai_dirs.sort();
    genai_dirs.dedup();
    if !genai_dirs.is_empty() {
        return onnx_variants(&files, genai_dirs);
    }
    if has_ext("safetensors") {
        let formats = detect_formats(&hints.tags, hints.library_name.as_deref(), &format!("{}/{}", hints.owner, hints.name));
        let format = if formats.iter().any(|f| f == "mlx") { "mlx" } else { "safetensors" };
        let vf: Vec<RemoteFile> = files.iter().filter(|f| !is_redundant_weights(&f.path)).map(|f| remote(f)).collect();
        let quant = quant_from_repo_name(&hints.name);
        return vec![variant("repo".into(), "Repository intero".into(), format, quant, vf)];
    }
    if has_ext("onnx") {
        let vf: Vec<RemoteFile> = files.iter().filter(|f| !is_redundant_weights(&f.path)).map(|f| remote(f)).collect();
        return vec![variant("repo".into(), "Repository intero".into(), "onnx", None, vf)];
    }
    let mut out = Vec::new();
    for (ext, format) in [("litertlm", "litertlm"), ("task", "litertlm"), ("pte", "pte"), ("llamafile", "llamafile")] {
        for f in files.iter().filter(|f| ext_of(&f.path) == ext) {
            let name = file_name(&f.path);
            let stem = name.rsplit_once('.').map(|x| x.0).unwrap_or(name).to_string();
            out.push(variant(stem.clone(), stem, format, None, vec![remote(f)]));
        }
    }
    out.sort_by(|a, b| a.size_bytes.cmp(&b.size_bytes).then_with(|| a.id.cmp(&b.id)));
    out
}

/// Formats a repository provides, from tags, `library_name` and the repository name.
pub fn detect_formats(tags: &[String], library_name: Option<&str>, name: &str) -> Vec<String> {
    let t: Vec<String> = tags.iter().map(|t| t.to_ascii_lowercase()).collect();
    let has = |s: &str| t.iter().any(|x| x == s);
    let lib = library_name.unwrap_or_default().to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    let mut out = Vec::new();
    if has("gguf") || lib == "gguf" || name.contains("gguf") {
        out.push("gguf");
    }
    let mlx = has("mlx") || lib == "mlx" || name.contains("mlx");
    if mlx {
        out.push("mlx");
    }
    if has("onnx") || has("onnxruntime-genai") || lib == "onnx" || name.contains("onnx") {
        out.push("onnx");
    }
    if has("safetensors") && !mlx {
        out.push("safetensors");
    }
    if has("litert-lm") || has("litert") || name.contains("litert") {
        out.push("litertlm");
    }
    if has("executorch") || name.contains("executorch") {
        out.push("pte");
    }
    if has("llamafile") || name.contains("llamafile") {
        out.push("llamafile");
    }
    out.into_iter().map(String::from).collect()
}

/// chat | tools | vision | embeddings | reasoning, from tags, pipeline and name.
pub fn capabilities(tags: &[String], pipeline_tag: Option<&str>, name: &str) -> Vec<String> {
    let t: Vec<String> = tags.iter().map(|t| t.to_ascii_lowercase()).collect();
    let has = |s: &str| t.iter().any(|x| x == s);
    let pipe = pipeline_tag.unwrap_or_default().to_ascii_lowercase();
    let lname = name.to_ascii_lowercase();
    let embeddings = matches!(pipe.as_str(), "feature-extraction" | "sentence-similarity")
        || has("sentence-transformers")
        || has("text-embeddings-inference")
        || lname.contains("embedding")
        || lname.contains("embed-");
    if embeddings {
        return vec!["embeddings".into()];
    }
    let vision = matches!(pipe.as_str(), "image-text-to-text" | "visual-question-answering" | "image-to-text" | "video-text-to-text")
        || has("vision")
        || has("multimodal")
        || has("image-text-to-text");
    let chat = vision
        || matches!(pipe.as_str(), "text-generation" | "text2text-generation" | "any-to-any" | "conversational")
        || has("conversational")
        || has("text-generation")
        || has("chat");
    let tools = t.iter().any(|x| x.contains("tool") || x.contains("function-calling") || x.contains("function_calling"))
        || lname.contains("function-calling");
    let tokens: Vec<&str> = lname.split(['-', '_', '.', '/', ' ']).collect();
    let reasoning = tokens.iter().any(|x| matches!(*x, "r1" | "qwen3" | "thinking" | "reasoning" | "qwq" | "magistral"))
        || lname.contains("gpt-oss")
        || lname.contains("deepseek-r1")
        || has("reasoning");
    let mut out = Vec::new();
    for (cap, on) in [("chat", chat || tools || reasoning), ("tools", tools), ("vision", vision), ("reasoning", reasoning)] {
        if on {
            out.push(cap.to_string());
        }
    }
    out
}

/// Parameter count in billions from a model name (`Qwen3-4B-GGUF` → 4, `SmolLM2-135M` → 0.135).
pub fn params_from_name(name: &str) -> Option<f64> {
    name.split(['-', '_', ' ', '/']).filter(|t| t.len() > 1 && t.as_bytes()[0].is_ascii_digit()).find_map(crate::parse_params)
}

/// Distinct values keeping the first occurrence.
pub fn dedup(mut v: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    v.retain(|x| seen.insert(x.clone()));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(path: &str, size: u64) -> RepoFile {
        RepoFile { path: path.into(), size, sha256: Some(format!("sha-{path}")) }
    }

    fn ids(v: &[ModelVariant]) -> Vec<&str> {
        v.iter().map(|x| x.id.as_str()).collect()
    }

    #[test]
    fn parses_quantizations_from_file_names() {
        let cases = [
            ("Qwen3-4B-Q4_K_M.gguf", Some("Q4_K_M")),
            ("Qwen3-4B-Q2_K.gguf", Some("Q2_K")),
            ("Qwen3-4B-Q3_K_S.gguf", Some("Q3_K_S")),
            ("Qwen3-4B-Q3_K_L.gguf", Some("Q3_K_L")),
            ("Qwen3-4B-Q4_0.gguf", Some("Q4_0")),
            ("Qwen3-4B-Q4_1.gguf", Some("Q4_1")),
            ("Qwen3-4B-Q5_K_S.gguf", Some("Q5_K_S")),
            ("Qwen3-4B-Q6_K.gguf", Some("Q6_K")),
            ("Qwen3-4B-Q8_0.gguf", Some("Q8_0")),
            ("model-IQ1_S.gguf", Some("IQ1_S")),
            ("model-IQ2_XXS.gguf", Some("IQ2_XXS")),
            ("model-IQ3_M.gguf", Some("IQ3_M")),
            ("model-IQ4_XS.gguf", Some("IQ4_XS")),
            ("model-IQ4_NL.gguf", Some("IQ4_NL")),
            ("model-F16.gguf", Some("F16")),
            ("model-BF16.gguf", Some("BF16")),
            ("model-f32.gguf", Some("F32")),
            ("gpt-oss-20b-mxfp4.gguf", Some("MXFP4")),
            ("model-TQ1_0.gguf", Some("TQ1_0")),
            ("model-TQ2_0.gguf", Some("TQ2_0")),
            ("Qwen3-4B-UD-Q4_K_XL.gguf", Some("UD-Q4_K_XL")),
            ("gemma-3-4b-it-UD-IQ1_S.gguf", Some("UD-IQ1_S")),
            ("Llama-3.2-3B-Instruct.Q4_K_M.gguf", Some("Q4_K_M")),
            ("Qwen3_4B_Q5_K_M.gguf", Some("Q5_K_M")),
            ("Qwen3-4B-Q4_K_M-00001-of-00003.gguf", Some("Q4_K_M")),
            ("mmproj-F16.gguf", Some("F16")),
            ("model.gguf", None),
            ("Qwen3-4B-Instruct-2507.gguf", None),
        ];
        for (name, want) in cases {
            assert_eq!(parse_quant(name).as_deref(), want, "{name}");
        }
    }

    #[test]
    fn splits_shard_names() {
        assert_eq!(shard_info("Q4_K_M-00001-of-00003"), Some(("Q4_K_M".into(), 1, 3)));
        assert_eq!(shard_info("m-00002-of-00002"), Some(("m".into(), 2, 2)));
        assert_eq!(shard_info("m-0-of-2"), None);
        assert_eq!(shard_info("m-00003-of-00002"), None);
        assert_eq!(shard_info("plain"), None);
    }

    #[test]
    fn groups_gguf_by_quant_keeping_shards_together() {
        let files = vec![
            f("README.md", 10),
            f(".gitattributes", 1),
            f("Qwen3-4B-Q8_0.gguf", 4_000),
            f("Qwen3-4B-Q4_K_M.gguf", 2_500),
            f("Qwen3-4B-Q6_K-00001-of-00002.gguf", 1_500),
            f("Qwen3-4B-Q6_K-00002-of-00002.gguf", 1_400),
            f("Qwen3-4B-IQ3_M-00001-of-00003.gguf", 900), // incomplete: parts 2 and 3 missing
        ];
        let v = build_variants(&files, &RepoHints::default());
        assert_eq!(ids(&v), ["Q4_K_M", "Q6_K", "Q8_0"]);
        let q6 = v.iter().find(|x| x.id == "Q6_K").unwrap();
        assert_eq!(q6.files.len(), 2);
        assert_eq!(q6.files[0].path, "Qwen3-4B-Q6_K-00001-of-00002.gguf");
        assert_eq!(q6.size_bytes, 2_900);
        assert_eq!(q6.format, "gguf");
        assert_eq!(q6.quant.as_deref(), Some("Q6_K"));
        assert!(q6.files.iter().all(|x| x.role == "weights"));
        assert_eq!(q6.files[0].sha256.as_deref(), Some("sha-Qwen3-4B-Q6_K-00001-of-00002.gguf"));
    }

    #[test]
    fn attaches_the_best_mmproj_to_every_weights_variant() {
        let files = vec![
            f("gemma-3-4b-it-Q4_K_M.gguf", 2_500),
            f("gemma-3-4b-it-Q8_0.gguf", 4_100),
            f("mmproj-BF16.gguf", 900),
            f("mmproj-F16.gguf", 850),
            f("mmproj-F32.gguf", 1_700),
        ];
        let v = build_variants(&files, &RepoHints::default());
        assert_eq!(ids(&v), ["Q4_K_M", "Q8_0"]);
        for x in &v {
            let m: Vec<&RemoteFile> = x.files.iter().filter(|f| f.role == "mmproj").collect();
            assert_eq!(m.len(), 1, "{}", x.id);
            assert_eq!(m[0].path, "mmproj-F16.gguf");
        }
        assert_eq!(v[0].size_bytes, 2_500 + 850);
        // Without an F16 projector the smallest one wins.
        let files = vec![f("m-Q4_K_M.gguf", 10), f("mmproj-Q8_0.gguf", 300), f("mmproj-BF16.gguf", 600)];
        let v = build_variants(&files, &RepoHints::default());
        assert_eq!(v[0].files.iter().find(|f| f.role == "mmproj").unwrap().path, "mmproj-Q8_0.gguf");
    }

    #[test]
    fn handles_sub_folder_quant_layouts() {
        let files = vec![
            f("Q4_K_M/Qwen3-Coder-Q4_K_M-00001-of-00002.gguf", 10_000),
            f("Q4_K_M/Qwen3-Coder-Q4_K_M-00002-of-00002.gguf", 9_000),
            f("BF16/Qwen3-Coder-BF16-00001-of-00003.gguf", 30_000),
            f("BF16/Qwen3-Coder-BF16-00002-of-00003.gguf", 30_000),
            f("BF16/Qwen3-Coder-BF16-00003-of-00003.gguf", 5_000),
            // quant only in the folder name
            f("UD-Q2_K_XL/model-00001-of-00001.gguf", 7_000),
        ];
        let v = build_variants(&files, &RepoHints::default());
        assert_eq!(ids(&v), ["UD-Q2_K_XL", "Q4_K_M", "BF16"]);
        assert_eq!(v[1].files.len(), 2);
        assert_eq!(v[1].files[0].path, "Q4_K_M/Qwen3-Coder-Q4_K_M-00001-of-00002.gguf");
        assert_eq!(v[2].files.len(), 3);
    }

    #[test]
    fn keeps_variant_ids_unique() {
        let files = vec![f("a/model-Q4_K_M.gguf", 1), f("b/model-Q4_K_M.gguf", 2), f("c/other-Q4_K_M.gguf", 3)];
        let v = build_variants(&files, &RepoHints::default());
        let mut all = ids(&v);
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 3, "{:?}", ids(&v));
        assert!(v.iter().any(|x| x.id == "Q4_K_M"));
    }

    #[test]
    fn gguf_without_a_recognizable_quant_uses_the_base_name() {
        let v = build_variants(&[f("model.gguf", 5)], &RepoHints::default());
        assert_eq!(ids(&v), ["model"]);
        assert_eq!(v[0].quant, None);
    }

    #[test]
    fn non_gguf_repos_become_one_repo_variant() {
        let files = vec![
            f("README.md", 1),
            f("assets/banner.png", 99),
            f(".gitattributes", 1),
            f("LICENSE", 2),
            f("config.json", 1_000),
            f("generation_config.json", 100),
            f("model-00001-of-00002.safetensors", 1_000_000),
            f("model-00002-of-00002.safetensors", 900_000),
            f("model.safetensors.index.json", 5_000),
            f("tokenizer.json", 7_000),
            f("tokenizer_config.json", 2_000),
            f("pytorch_model.bin", 5_000_000),
            f("original/consolidated.00.pth", 5_000_000),
        ];
        let mlx = RepoHints {
            owner: "mlx-community".into(),
            name: "Qwen3-4B-4bit".into(),
            tags: vec!["mlx".into(), "safetensors".into()],
            ..Default::default()
        };
        let v = build_variants(&files, &mlx);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, "repo");
        assert_eq!(v[0].format, "mlx");
        assert_eq!(v[0].quant.as_deref(), Some("4bit"));
        let paths: Vec<&str> = v[0].files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths.len(), 7, "{paths:?}");
        assert!(!paths
            .iter()
            .any(|p| p.contains("README") || p.ends_with(".png") || p.contains("pytorch_model") || p.contains("original")));
        let roles: Vec<(&str, &str)> = v[0].files.iter().map(|f| (f.path.as_str(), f.role.as_str())).collect();
        assert!(roles.contains(&("config.json", "config")));
        assert!(roles.contains(&("model.safetensors.index.json", "config")));
        assert!(roles.contains(&("tokenizer.json", "tokenizer")));
        assert!(roles.contains(&("model-00001-of-00002.safetensors", "weights")));
        // Same files without MLX hints: plain safetensors.
        let st = RepoHints { owner: "Qwen".into(), name: "Qwen3-4B".into(), tags: vec!["safetensors".into()], ..Default::default() };
        assert_eq!(build_variants(&files, &st)[0].format, "safetensors");
        let by_lib = RepoHints { library_name: Some("mlx".into()), ..st };
        assert_eq!(build_variants(&files, &by_lib)[0].format, "mlx");
    }

    #[test]
    fn onnx_genai_repos_give_one_variant_per_directory() {
        let files = vec![
            f("README.md", 1),
            f("cpu_and_mobile/cpu-int4-rtn-block-32-acc-level-4/genai_config.json", 1_000),
            f("cpu_and_mobile/cpu-int4-rtn-block-32-acc-level-4/model.onnx", 100_000),
            f("cpu_and_mobile/cpu-int4-rtn-block-32-acc-level-4/model.onnx.data", 2_000_000),
            f("cpu_and_mobile/cpu-int4-rtn-block-32-acc-level-4/tokenizer.json", 5_000),
            f("cuda/cuda-fp16/genai_config.json", 1_000),
            f("cuda/cuda-fp16/model.onnx", 100_000),
            f("cuda/cuda-fp16/model.onnx.data", 7_000_000),
        ];
        let v = build_variants(&files, &RepoHints::default());
        assert_eq!(ids(&v), ["cpu-int4-rtn-block-32-acc-level-4", "cuda-fp16"]);
        assert!(v.iter().all(|x| x.format == "onnx"));
        assert_eq!(v[0].files.len(), 4);
        assert_eq!(v[0].quant.as_deref(), Some("int4"));
        assert_eq!(v[1].quant.as_deref(), Some("fp16"));
        assert!(v[1].files.iter().all(|f| f.path.starts_with("cuda/cuda-fp16/")));
        // A repo with the config at the root is a single "repo" variant.
        let root = vec![f("genai_config.json", 10), f("model.onnx", 100), f("model.onnx.data", 1000)];
        let v = build_variants(&root, &RepoHints::default());
        assert_eq!(ids(&v), ["repo"]);
        assert_eq!(v[0].files.len(), 3);
    }

    #[test]
    fn single_file_formats_get_a_variant_each() {
        let v = build_variants(
            &[f("gemma-3n-E2B-it-int4.litertlm", 3_000), f("gemma-3n-E4B-it-int4.litertlm", 4_000), f("README.md", 1)],
            &RepoHints::default(),
        );
        assert_eq!(ids(&v), ["gemma-3n-E2B-it-int4", "gemma-3n-E4B-it-int4"]);
        assert!(v.iter().all(|x| x.format == "litertlm"));
        assert!(build_variants(&[f("pytorch_model.bin", 1)], &RepoHints::default()).is_empty());
    }

    #[test]
    fn detects_formats_from_tags_library_and_name() {
        let t = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(detect_formats(&t(&["gguf", "transformers"]), None, "Qwen/Qwen3-4B-GGUF"), ["gguf"]);
        assert_eq!(detect_formats(&t(&["mlx", "safetensors"]), Some("mlx"), "mlx-community/x"), ["mlx"]);
        assert_eq!(detect_formats(&t(&["onnx"]), None, "microsoft/Phi-3-mini-4k-instruct-onnx"), ["onnx"]);
        assert_eq!(detect_formats(&t(&["safetensors", "text-generation"]), Some("transformers"), "Qwen/Qwen3-4B"), ["safetensors"]);
        assert_eq!(detect_formats(&t(&["litert-lm"]), None, "google/gemma-3n-E2B-it-litert-lm"), ["litertlm"]);
        assert!(detect_formats(&t(&["gguf", "onnx"]), None, "x").contains(&"onnx".to_string()));
        assert!(detect_formats(&[], None, "plain").is_empty());
    }

    #[test]
    fn deduces_capabilities() {
        let t = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(capabilities(&t(&["gguf"]), Some("text-generation"), "bartowski/Llama-3.2-3B-Instruct-GGUF"), ["chat"]);
        assert_eq!(capabilities(&t(&["gguf", "function-calling"]), Some("text-generation"), "x/Hermes-3-8B"), ["chat", "tools"]);
        assert_eq!(capabilities(&t(&[]), Some("image-text-to-text"), "google/gemma-3-4b-it"), ["chat", "vision"]);
        assert_eq!(capabilities(&t(&["sentence-similarity"]), Some("feature-extraction"), "BAAI/bge-m3"), ["embeddings"]);
        assert_eq!(capabilities(&t(&[]), Some("text-generation"), "Qwen/Qwen3-Embedding-0.6B-GGUF"), ["embeddings"]);
        assert_eq!(capabilities(&t(&[]), Some("text-generation"), "unsloth/DeepSeek-R1-Distill-Qwen-7B-GGUF"), ["chat", "reasoning"]);
        assert!(capabilities(&t(&[]), Some("text-generation"), "Qwen/Qwen3-8B-GGUF").contains(&"reasoning".to_string()));
        assert!(capabilities(&t(&[]), None, "x/Foo-Thinking-GGUF").contains(&"reasoning".to_string()));
        assert!(capabilities(&t(&["tool-use"]), None, "x/y").contains(&"tools".to_string()));
        assert!(capabilities(&t(&["image-classification"]), Some("image-classification"), "x/resnet").is_empty());
    }

    #[test]
    fn guesses_parameters_from_names() {
        assert_eq!(params_from_name("Qwen/Qwen3-4B-GGUF"), Some(4.0));
        assert_eq!(params_from_name("bartowski/Llama-3.2-3B-Instruct-GGUF"), Some(3.0));
        assert_eq!(params_from_name("unsloth/Qwen3-30B-A3B-GGUF"), Some(30.0));
        assert_eq!(params_from_name("HuggingFaceTB/SmolLM2-135M-Instruct"), Some(0.135));
        assert_eq!(params_from_name("Qwen/Qwen3-0.6B-GGUF"), Some(0.6));
        assert_eq!(params_from_name("some/model-without-size"), None);
    }
}
