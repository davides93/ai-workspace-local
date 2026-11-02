//! Logical paths versus native paths.
//!
//! Two kinds of "path" travel through this crate and they must never be mixed up:
//!
//! * **Logical** paths are identifiers, not places on a disk: the files of a repository
//!   (`RemoteFile.path`, `Q4_K_M/tiny.gguf`), the file names recorded in `.aiw-model.json`, the
//!   `provider:model@rev#variant` source, the model ids derived from a folder layout
//!   (`hf/acme/tiny/tiny-q4_k_m`). They are the same on every OS, so they always use `/`.
//! * **Native** paths (`PathBuf`, `LocalModel.path`, `DownloadProgress.dest`) are real places on
//!   the local disk and use whatever separator the OS uses.
//!
//! Going from a native relative path to a logical one must therefore walk the components and join
//! them with `/` ([`to_logical`]), never `to_string_lossy()` the `PathBuf` (that yields `\` on
//! Windows). Going the other way must push the components one by one ([`native_rel`],
//! [`native_join`]), so that `Q4_K_M/tiny.gguf` becomes nested folders on every OS.

use aiw_core::{Error, Result};
use std::path::{Component, Path, PathBuf};

/// `\` becomes `/`: the canonical spelling of a logical path. (A `\` is never part of a model
/// file name: the downloader refuses such names.)
pub fn normalize_logical(s: &str) -> String {
    s.replace('\\', "/")
}

/// A relative native path as a logical one: its components joined with `/`, whatever the OS
/// separator is. `.` is dropped and `..` kept; a root or a drive prefix is not part of a
/// relative path and is skipped.
pub fn to_logical(p: &Path) -> String {
    let parts: Vec<String> = p
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            Component::ParentDir => Some("..".to_string()),
            Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
        })
        .collect();
    normalize_logical(&parts.join("/"))
}

/// Whether `s` can be one folder or file name below the models directory: not empty, not `.` or
/// `..`, no separator, no drive or stream marker (`:`), no NUL.
pub fn is_plain_component(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\', ':', '\0'])
}

/// The native relative path of a logical one (`Q4_K_M/tiny.gguf` → `Q4_K_M` + `tiny.gguf`),
/// built component by component. Strict: the logical path must use `/` only and every component
/// must be plain, so a `..`, an absolute path (`/x`, `C:/x`, `\\server\share`), a drive prefix or
/// an NTFS stream (`a.gguf:stream`) is refused instead of escaping the folder it is joined to.
pub fn native_rel(logical: &str) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for piece in logical.split('/') {
        if !is_plain_component(piece) {
            return Err(Error::Invalid(format!("percorso di file non valido: {logical}")));
        }
        out.push(piece);
    }
    if out.as_os_str().is_empty() {
        return Err(Error::Invalid("percorso di file vuoto".into()));
    }
    Ok(out)
}

/// `base` followed by the components of an (already validated) logical path, e.g. the
/// `owner/name` of a repository id. Both `/` and `\` separate components and empty or `.` pieces
/// are dropped, but nothing else is checked: the downloader's `safe_rel` is the gate.
pub fn native_join(base: impl Into<PathBuf>, logical: &str) -> PathBuf {
    let mut out: PathBuf = base.into();
    for piece in logical.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".") {
        out.push(piece);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comps(p: &Path) -> Vec<String> {
        p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn a_native_relative_path_becomes_a_slash_path_on_every_os() {
        let native = PathBuf::from("hf").join("acme").join("tiny").join("Q4_K_M").join("x.gguf");
        assert_eq!(to_logical(&native), "hf/acme/tiny/Q4_K_M/x.gguf");
        assert_eq!(to_logical(Path::new("a.gguf")), "a.gguf");
        // What a Windows `strip_prefix` hands back (backslashes) is spelled with slashes too.
        assert_eq!(to_logical(Path::new("hf\\acme\\tiny\\x.gguf")), "hf/acme/tiny/x.gguf");
        assert_eq!(to_logical(Path::new("sub\\dir/file.gguf")), "sub/dir/file.gguf");
        // `.` goes, a root or a drive is not part of a relative path.
        assert_eq!(to_logical(Path::new("./a/./b.gguf")), "a/b.gguf");
        assert_eq!(to_logical(Path::new("/a/b.gguf")), "a/b.gguf");
        assert_eq!(to_logical(Path::new("")), "");
        assert_eq!(normalize_logical("a\\b\\c.gguf"), "a/b/c.gguf");
        assert_eq!(normalize_logical("a/b/c.gguf"), "a/b/c.gguf");
    }

    #[test]
    fn a_slash_path_is_joined_component_by_component() {
        let p = native_rel("Q4_K_M/x.gguf").unwrap();
        assert_eq!(comps(&p), ["Q4_K_M", "x.gguf"]);
        assert_eq!(p, PathBuf::from("Q4_K_M").join("x.gguf"));
        assert_eq!(comps(&native_rel("a.gguf").unwrap()), ["a.gguf"]);
        assert_eq!(comps(&native_rel("a/b/c/d.gguf").unwrap()), ["a", "b", "c", "d.gguf"]);
        // Joined below a folder it stays nested, never one component with a slash in it.
        let dest = Path::new("hf").join(native_rel("Q4_K_M/x.gguf").unwrap());
        assert_eq!(comps(&dest), ["hf", "Q4_K_M", "x.gguf"]);
        assert_eq!(to_logical(&dest), "hf/Q4_K_M/x.gguf");
    }

    #[test]
    fn a_path_that_could_leave_its_folder_is_refused() {
        for bad in [
            "",
            ".",
            "..",
            "../x.gguf",
            "a/../x.gguf",
            "a/..",
            "./x.gguf",
            "a/./x.gguf",
            "/etc/passwd",
            "a//b.gguf",
            "a/b/",
            "C:/Windows/x.gguf",
            "C:x.gguf",
            "c:\\x.gguf",
            "..\\x.gguf",
            "a\\b.gguf",
            "\\\\server\\share\\x.gguf",
            "a.gguf:stream",
            "a/\0/b",
        ] {
            assert!(matches!(native_rel(bad), Err(Error::Invalid(_))), "{bad:?} must be refused");
        }
        assert!(is_plain_component("tiny-Q4_K_M.gguf"));
        assert!(is_plain_component("Qwen3-4B (v2).gguf"));
        assert!(!is_plain_component("a:b"));
        assert!(!is_plain_component("a\\b"));
    }

    #[test]
    fn a_repository_id_is_joined_under_a_folder_with_any_separator() {
        let want = PathBuf::from("hf").join("acme").join("tiny-GGUF");
        assert_eq!(native_join("hf", "acme/tiny-GGUF"), want);
        assert_eq!(native_join("hf", "acme\\tiny-GGUF"), want);
        assert_eq!(native_join(PathBuf::from("hf"), "/acme//tiny-GGUF/"), want);
        assert_eq!(comps(&native_join("ollama", "someone/tuned")), ["ollama", "someone", "tuned"]);
        assert_eq!(native_join("hf", ""), PathBuf::from("hf"));
    }
}
