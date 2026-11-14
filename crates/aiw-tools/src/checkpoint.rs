//! Checkpoints of a run and their rewind.
//!
//! Before a tool overwrites a file the original is copied to `<checkpoint_dir>/<run_id>/<path in
//! the workspace>` (files outside the workspace go under `_esterni`). `.aiw-run.json` in the same
//! folder records the workspace root of the run and the files the run created, which have no
//! original: a rewind puts the originals back and deletes the created files.

use crate::canonical_lenient;
use aiw_core::api::{CheckpointInfo, RewindResult};
use aiw_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// Name of the file that describes the checkpoint of a run.
pub const RUN_META: &str = ".aiw-run.json";
/// Folder (inside the checkpoint of a run) of the originals of files outside the workspace.
pub const EXTERNAL_DIR: &str = "_esterni";

/// Tool contexts of the same run (a workflow runs its steps together) update the same file.
static META_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunMeta {
    /// Workspace root the paths of the checkpoint are relative to.
    pub root: String,
    /// Files the run created: relative to the root with `/`, or absolute when outside of it.
    #[serde(default)]
    pub created: Vec<String>,
}

/// A run id becomes a folder name: only plain identifiers are accepted.
pub fn valid_run_id(run_id: &str) -> bool {
    !run_id.is_empty() && run_id.len() <= 128 && run_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn run_dir(checkpoint_dir: &Path, run_id: &str) -> PathBuf {
    checkpoint_dir.join(run_id)
}

pub fn read_meta(run_dir: &Path) -> Option<RunMeta> {
    serde_json::from_slice(&std::fs::read(run_dir.join(RUN_META)).ok()?).ok()
}

/// Atomic: written next to the target and renamed over it.
fn write_meta(run_dir: &Path, meta: &RunMeta) -> std::io::Result<()> {
    std::fs::create_dir_all(run_dir)?;
    let tmp = run_dir.join(format!("{RUN_META}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(meta)?)?;
    std::fs::rename(&tmp, run_dir.join(RUN_META))
}

/// Reads the metadata (creating it with `root` when the run has none yet), lets `f` change it
/// and writes it back, all under a lock.
pub fn update_meta(run_dir: &Path, root: &Path, f: impl FnOnce(&mut RunMeta)) {
    let _guard = META_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut meta = read_meta(run_dir).unwrap_or_else(|| RunMeta { root: root.to_string_lossy().into_owned(), created: vec![] });
    let before = meta.clone();
    f(&mut meta);
    if meta != before || !run_dir.join(RUN_META).exists() {
        let _ = write_meta(run_dir, &meta);
    }
}

/// How `path` is named in the checkpoint: relative to `root` with `/` (true), or the absolute path
/// when it is outside (false).
pub fn key_of(root: &Path, path: &Path) -> (bool, String) {
    match path.strip_prefix(root) {
        Ok(rel) => {
            let parts: Vec<String> = rel
                .components()
                .filter_map(|c| if let Component::Normal(p) = c { Some(p.to_string_lossy().into_owned()) } else { None })
                .collect();
            (true, parts.join("/"))
        }
        Err(_) => (false, path.to_string_lossy().into_owned()),
    }
}

/// True when `rel` is a plain relative path (no `..`, no root).
fn is_plain_relative(rel: &str) -> bool {
    !rel.is_empty() && Path::new(rel).components().all(|c| matches!(c, Component::Normal(_)))
}

fn files_under(dir: &Path, base: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            files_under(&p, base, out);
        } else if ft.is_file() {
            if let Ok(rel) = p.strip_prefix(base) {
                let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                out.push(parts.join("/"));
            }
        }
    }
}

/// True when the (symlink-resolved) `path` stays inside the (resolved) `root`.
fn stays_inside(root: &Path, path: &Path) -> bool {
    let Ok(canon_root) = std::fs::canonicalize(root) else { return false };
    canonical_lenient(path).is_some_and(|p| p.starts_with(canon_root))
}

/// What a rewind of `run_id` would do. `None` when the run has no checkpoint.
pub fn plan(checkpoint_dir: &Path, run_id: &str) -> Option<CheckpointInfo> {
    if !valid_run_id(run_id) {
        return None;
    }
    let dir = run_dir(checkpoint_dir, run_id);
    if !dir.is_dir() {
        return None;
    }
    let meta = read_meta(&dir);
    let mut stored = Vec::new();
    files_under(&dir, &dir, &mut stored);
    stored.retain(|f| !f.starts_with(RUN_META));
    stored.sort();
    let mut info = CheckpointInfo {
        run_id: run_id.to_string(),
        root: meta.as_ref().map(|m| m.root.clone()),
        restore: vec![],
        delete: vec![],
        external: vec![],
    };
    let external_prefix = format!("{EXTERNAL_DIR}/");
    for f in stored {
        // Without a recorded root (a checkpoint made before rewind existed) nothing can be put back safely.
        if f.starts_with(&external_prefix) || meta.is_none() {
            info.external.push(f);
        } else {
            info.restore.push(f);
        }
    }
    if let Some(meta) = &meta {
        let root = PathBuf::from(&meta.root);
        for c in &meta.created {
            if is_plain_relative(c) {
                let p = root.join(c);
                if std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()) && stays_inside(&root, &p) {
                    info.delete.push(c.clone());
                }
            } else if Path::new(c).is_absolute() && Path::new(c).is_file() {
                info.external.push(c.clone());
            }
        }
        info.delete.sort();
    }
    if info.restore.is_empty() && info.delete.is_empty() && info.external.is_empty() {
        return None;
    }
    Some(info)
}

/// Puts the originals back, deletes the files the run created, and drops from the checkpoint what
/// was done. Files outside the root are only reported.
pub fn rewind(checkpoint_dir: &Path, run_id: &str) -> Result<RewindResult> {
    let info = plan(checkpoint_dir, run_id).ok_or_else(|| Error::NotFound(format!("nessun checkpoint per il run {run_id}")))?;
    let Some(root) = info.root.as_deref().map(PathBuf::from) else {
        return Err(Error::Invalid(
            "questo checkpoint è stato creato da una versione precedente e non registra la cartella di lavoro: non si può ripristinare in sicurezza".into(),
        ));
    };
    let dir = run_dir(checkpoint_dir, run_id);
    let mut out =
        RewindResult { run_id: run_id.to_string(), restored: vec![], deleted: vec![], external: info.external.clone(), failed: vec![] };
    for rel in &info.restore {
        let dest = root.join(rel);
        let src = dir.join(rel);
        if !is_plain_relative(rel) || !stays_inside(&root, &dest) {
            out.failed.push(format!("{rel}: percorso fuori dalla cartella di lavoro"));
            continue;
        }
        let done = dest.parent().map(std::fs::create_dir_all).transpose().and_then(|_| std::fs::copy(&src, &dest));
        match done {
            Ok(_) => {
                out.restored.push(rel.clone());
                let _ = std::fs::remove_file(&src);
            }
            Err(e) => out.failed.push(format!("{rel}: {e}")),
        }
    }
    for rel in &info.delete {
        // Deleted by the run and written again later: the original just put back wins over the new file.
        if out.restored.contains(rel) {
            continue;
        }
        match std::fs::remove_file(root.join(rel)) {
            Ok(()) => out.deleted.push(rel.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => out.failed.push(format!("{rel}: {e}")),
        }
    }
    // What was undone is no longer pending: a second rewind must not touch later edits.
    if out.failed.is_empty() {
        update_meta(&dir, &root, |m| m.created.retain(|c| !is_plain_relative(c)));
    }
    prune(&dir);
    Ok(out)
}

/// Removes empty folders, and the whole checkpoint when nothing is left to rewind.
fn prune(dir: &Path) {
    fn empty_dirs(d: &Path) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    empty_dirs(&e.path());
                    let _ = std::fs::remove_dir(e.path());
                }
            }
        }
    }
    empty_dirs(dir);
    let leftover = read_meta(dir).is_some_and(|m| !m.created.is_empty());
    let mut files = Vec::new();
    files_under(dir, dir, &mut files);
    files.retain(|f| !f.starts_with(RUN_META));
    if files.is_empty() && !leftover {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolContext;

    fn ctx(ws: &Path, ckpt: &Path, run: &str) -> ToolContext {
        let mut c = ToolContext::new(run, ws.to_path_buf());
        c.checkpoint_dir = Some(ckpt.to_path_buf());
        c
    }

    #[test]
    fn run_ids_are_plain_identifiers() {
        for ok in ["run_ab12", "wfr-1", "A"] {
            assert!(valid_run_id(ok), "{ok}");
        }
        for bad in ["", "..", "a/b", "a\\b", "../x", "a b", "è"] {
            assert!(!valid_run_id(bad), "{bad}");
        }
    }

    #[test]
    fn rewind_restores_originals_and_deletes_created_files() {
        let ws = tempfile::tempdir().unwrap();
        let ck = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join("notes")).unwrap();
        std::fs::write(ws.path().join("notes/a.md"), "originale").unwrap();
        std::fs::write(ws.path().join("moved.txt"), "da spostare").unwrap();
        let c = ctx(ws.path(), ck.path(), "run_1");

        // The run overwrites a.md, creates new/b.md, overwrites its own file again, moves moved.txt.
        c.checkpoint(&ws.path().join("notes/a.md"));
        std::fs::write(ws.path().join("notes/a.md"), "cambiato").unwrap();
        c.checkpoint(&ws.path().join("notes/a.md"));
        std::fs::write(ws.path().join("notes/a.md"), "cambiato di nuovo").unwrap();
        std::fs::create_dir_all(ws.path().join("new")).unwrap();
        std::fs::write(ws.path().join("new/b.md"), "nuovo").unwrap();
        c.record_created(&ws.path().join("new/b.md"));
        c.checkpoint(&ws.path().join("new/b.md"));
        std::fs::write(ws.path().join("new/b.md"), "nuovo, rivisto").unwrap();
        c.checkpoint(&ws.path().join("moved.txt"));
        std::fs::rename(ws.path().join("moved.txt"), ws.path().join("new/moved.txt")).unwrap();
        c.record_created(&ws.path().join("new/moved.txt"));

        let info = plan(ck.path(), "run_1").unwrap();
        assert_eq!(info.root.as_deref(), Some(ws.path().to_string_lossy().as_ref()));
        assert_eq!(info.restore, vec!["moved.txt", "notes/a.md"]);
        assert_eq!(info.delete, vec!["new/b.md", "new/moved.txt"]);
        assert!(info.external.is_empty());
        // The original of a.md is the first one, and the file created by the run has none.
        assert_eq!(std::fs::read_to_string(ck.path().join("run_1/notes/a.md")).unwrap(), "originale");
        assert!(!ck.path().join("run_1/new/b.md").exists());

        let r = rewind(ck.path(), "run_1").unwrap();
        assert_eq!(r.restored, vec!["moved.txt", "notes/a.md"]);
        assert_eq!(r.deleted, vec!["new/b.md", "new/moved.txt"]);
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert_eq!(std::fs::read_to_string(ws.path().join("notes/a.md")).unwrap(), "originale");
        assert_eq!(std::fs::read_to_string(ws.path().join("moved.txt")).unwrap(), "da spostare");
        assert!(!ws.path().join("new/b.md").exists() && !ws.path().join("new/moved.txt").exists());
        // Nothing left: the checkpoint is gone and a second rewind has nothing to do.
        assert!(plan(ck.path(), "run_1").is_none());
        assert!(matches!(rewind(ck.path(), "run_1"), Err(Error::NotFound(_))));
    }

    #[test]
    fn a_created_file_that_is_gone_or_was_replaced_by_a_folder_is_left_alone() {
        let ws = tempfile::tempdir().unwrap();
        let ck = tempfile::tempdir().unwrap();
        let c = ctx(ws.path(), ck.path(), "run_2");
        std::fs::write(ws.path().join("gone.txt"), "x").unwrap();
        c.record_created(&ws.path().join("gone.txt"));
        std::fs::write(ws.path().join("kept.txt"), "x").unwrap();
        c.record_created(&ws.path().join("kept.txt"));
        std::fs::remove_file(ws.path().join("gone.txt")).unwrap();
        let info = plan(ck.path(), "run_2").unwrap();
        assert_eq!(info.delete, vec!["kept.txt"]);
        let r = rewind(ck.path(), "run_2").unwrap();
        assert_eq!(r.deleted, vec!["kept.txt"]);
        assert!(r.failed.is_empty());
    }

    #[test]
    fn files_outside_the_workspace_are_only_listed() {
        let ws = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let ck = tempfile::tempdir().unwrap();
        let mut c = ctx(ws.path(), ck.path(), "run_3");
        c.sandbox = crate::SandboxMode::Off;
        let outside = other.path().join("fuori.txt");
        std::fs::write(&outside, "originale").unwrap();
        c.checkpoint(&outside);
        std::fs::write(&outside, "cambiato").unwrap();
        let created = other.path().join("creato.txt");
        std::fs::write(&created, "nuovo").unwrap();
        c.record_created(&created);
        std::fs::write(ws.path().join("dentro.txt"), "a").unwrap();
        c.checkpoint(&ws.path().join("dentro.txt"));
        std::fs::write(ws.path().join("dentro.txt"), "b").unwrap();

        let info = plan(ck.path(), "run_3").unwrap();
        assert_eq!(info.restore, vec!["dentro.txt"]);
        assert_eq!(info.external.len(), 2, "{:?}", info.external);
        let r = rewind(ck.path(), "run_3").unwrap();
        assert_eq!(r.restored, vec!["dentro.txt"]);
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "cambiato", "outside files are not touched");
        assert!(created.exists());
        assert_eq!(r.external.len(), 2);
        // The external entries stay listed.
        assert!(plan(ck.path(), "run_3").is_some());
    }

    #[test]
    fn a_checkpoint_without_a_recorded_root_cannot_be_rewound() {
        let ws = tempfile::tempdir().unwrap();
        let ck = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ck.path().join("run_old/notes")).unwrap();
        std::fs::write(ck.path().join("run_old/notes/a.md"), "vecchio").unwrap();
        std::fs::write(ws.path().join("notes.md"), "x").unwrap();
        let info = plan(ck.path(), "run_old").unwrap();
        assert!(info.root.is_none() && info.restore.is_empty());
        assert_eq!(info.external, vec!["notes/a.md"]);
        assert!(matches!(rewind(ck.path(), "run_old"), Err(Error::Invalid(_))));
    }

    #[test]
    fn contexts_of_the_same_run_do_not_lose_each_others_records() {
        let ws = tempfile::tempdir().unwrap();
        let ck = tempfile::tempdir().unwrap();
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let c = ctx(ws.path(), ck.path(), "run_par");
                let file = ws.path().join(format!("f{i}.txt"));
                std::thread::spawn(move || {
                    std::fs::write(&file, "x").unwrap();
                    c.record_created(&file);
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let meta = read_meta(&ck.path().join("run_par")).unwrap();
        assert_eq!(meta.created.len(), 8, "{:?}", meta.created);
        assert!(!ck.path().join("run_par").read_dir().unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".tmp")));
    }
}
