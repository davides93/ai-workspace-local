//! `.aiw-model.json`: the install record the downloader writes in a model's folder
//! (`{provider, model, variant, revision, format, quant, files:[{path,size_bytes,sha256}], installed_at}`).
//!
//! A repository folder can hold several variants (Q4_K_M and Q8_0 of the same repo). The newest
//! install is the top level of the document, as the contract says; every install, the newest
//! included, is also listed in `installs`.
//!
//! The `files[].path` entries are logical paths, relative to the folder holding the sidecar and
//! always spelled with `/` (see [`crate::relpath`]): the record means the same on every OS.

use crate::relpath;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const SIDECAR: &str = ".aiw-model.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidecarFile {
    /// Relative to the folder holding the sidecar, components separated by `/` on every OS.
    pub path: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl SidecarFile {
    /// The file on disk, below `dir` (the folder holding the sidecar), built component by
    /// component. `None` when the recorded path is not a plain relative path: a hand-edited
    /// record must not point outside `dir`.
    pub fn native(&self, dir: &Path) -> Option<PathBuf> {
        relpath::native_rel(&self.path).ok().map(|rel| dir.join(rel))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Install {
    pub provider: String,
    pub model: String,
    pub variant: String,
    #[serde(default)]
    pub revision: String,
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quant: Option<String>,
    pub files: Vec<SidecarFile>,
    #[serde(default)]
    pub installed_at: String,
}

/// File paths spelled with `/`, whoever wrote the record.
fn normalized(mut install: Install) -> Install {
    for f in &mut install.files {
        f.path = relpath::normalize_logical(&f.path);
    }
    install
}

/// Every install recorded in `dir` (empty when there is no sidecar or it is unreadable).
pub fn read(dir: &Path) -> Vec<Install> {
    let Ok(text) = std::fs::read_to_string(dir.join(SIDECAR)) else { return vec![] };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return vec![] };
    let installs: Vec<Install> = match v.get("installs").and_then(Value::as_array) {
        Some(list) => list.iter().filter_map(|i| serde_json::from_value(i.clone()).ok()).collect(),
        None => serde_json::from_value(v).ok().into_iter().collect(),
    };
    installs.into_iter().map(normalized).collect()
}

pub fn write(dir: &Path, installs: &[Install]) -> std::io::Result<()> {
    let installs: Vec<Install> = installs.iter().cloned().map(normalized).collect();
    let Some(latest) = installs.last() else {
        return match std::fs::remove_file(dir.join(SIDECAR)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    };
    let mut doc = serde_json::to_value(latest)?;
    doc["installs"] = json!(&installs);
    let tmp = dir.join(format!("{SIDECAR}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&doc)?)?;
    std::fs::rename(&tmp, dir.join(SIDECAR))
}

/// Records `install` (replacing an earlier install of the same variant) as the newest one.
pub fn upsert(dir: &Path, install: Install) -> std::io::Result<()> {
    let mut all = read(dir);
    all.retain(|i| i.variant != install.variant);
    all.push(install);
    write(dir, &all)
}

/// Forgets one variant; returns what is left (the sidecar is deleted when nothing is).
pub fn remove_variant(dir: &Path, variant: &str) -> std::io::Result<Vec<Install>> {
    let mut all = read(dir);
    all.retain(|i| i.variant != variant);
    write(dir, &all)?;
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(variant: &str, file: &str) -> Install {
        Install {
            provider: "hf".into(),
            model: "acme/tiny-GGUF".into(),
            variant: variant.into(),
            revision: "abc".into(),
            format: "gguf".into(),
            quant: Some(variant.into()),
            files: vec![SidecarFile { path: file.into(), size_bytes: 10, sha256: Some("f".repeat(64)) }],
            installed_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

    #[test]
    fn newest_install_is_the_top_level_and_variants_accumulate() {
        let d = tempfile::tempdir().unwrap();
        assert!(read(d.path()).is_empty());
        upsert(d.path(), install("Q4_K_M", "a.gguf")).unwrap();
        upsert(d.path(), install("Q8_0", "b.gguf")).unwrap();
        upsert(d.path(), install("Q4_K_M", "a2.gguf")).unwrap(); // reinstall replaces
        let all = read(d.path());
        assert_eq!(
            all.iter().map(|i| (i.variant.as_str(), i.files[0].path.as_str())).collect::<Vec<_>>(),
            [("Q8_0", "b.gguf"), ("Q4_K_M", "a2.gguf")]
        );
        // The contract shape is readable at the top level.
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(d.path().join(SIDECAR)).unwrap()).unwrap();
        for key in ["provider", "model", "variant", "revision", "format", "quant", "files", "installed_at"] {
            assert!(raw.get(key).is_some(), "{key}");
        }
        assert_eq!(raw["variant"], "Q4_K_M");
        assert_eq!(raw["files"][0]["size_bytes"], 10);
        let left = remove_variant(d.path(), "Q4_K_M").unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(read(d.path())[0].variant, "Q8_0");
        assert!(remove_variant(d.path(), "Q8_0").unwrap().is_empty());
        assert!(!d.path().join(SIDECAR).exists());
    }

    #[test]
    fn file_paths_are_always_spelled_with_slashes() {
        let d = tempfile::tempdir().unwrap();
        // A caller that hands over a Windows-style relative path still records a `/` path...
        upsert(d.path(), install("Q4_K_M", "sub\\dir\\a.gguf")).unwrap();
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(d.path().join(SIDECAR)).unwrap()).unwrap();
        assert_eq!(raw["files"][0]["path"], "sub/dir/a.gguf");
        assert_eq!(raw["installs"][0]["files"][0]["path"], "sub/dir/a.gguf");
        assert_eq!(read(d.path())[0].files[0].path, "sub/dir/a.gguf");
        // ...and a record written elsewhere with backslashes reads back the same way.
        let mut old = install("Q8_0", "x\\b.gguf");
        old.files.push(SidecarFile { path: "y\\c.gguf".into(), size_bytes: 1, sha256: None });
        std::fs::write(d.path().join(SIDECAR), serde_json::to_vec(&old).unwrap()).unwrap();
        let paths: Vec<String> = read(d.path())[0].files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(paths, ["x/b.gguf", "y/c.gguf"]);
    }

    #[test]
    fn a_recorded_file_is_resolved_below_its_folder_and_never_outside() {
        let dir = Path::new("models").join("hf").join("acme");
        let f = |p: &str| SidecarFile { path: p.into(), size_bytes: 1, sha256: None };
        assert_eq!(f("a.gguf").native(&dir), Some(dir.join("a.gguf")));
        assert_eq!(f("sub/b.gguf").native(&dir), Some(dir.join("sub").join("b.gguf")));
        assert_eq!(f("sub/b.gguf").native(&dir).unwrap().components().count(), dir.components().count() + 2);
        for bad in ["../x.gguf", "sub/../../x.gguf", "/etc/passwd", "C:/x.gguf", "", "a//b.gguf"] {
            assert_eq!(f(bad).native(&dir), None, "{bad:?}");
        }
    }

    #[test]
    fn reads_a_plain_single_install_document() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(SIDECAR), serde_json::to_vec(&install("Q4_K_M", "a.gguf")).unwrap()).unwrap();
        assert_eq!(read(d.path()), vec![install("Q4_K_M", "a.gguf")]);
        std::fs::write(d.path().join(SIDECAR), b"{not json").unwrap();
        assert!(read(d.path()).is_empty());
    }
}
