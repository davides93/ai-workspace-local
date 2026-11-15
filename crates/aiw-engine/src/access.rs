//! The folders of the computer the person handed to the agents (Mac App Store build): the list, its
//! persistence and what reaches the file tools. The platform half (the macOS open panel and the
//! security-scoped bookmark) is [`aiw_core::folders::FolderBridge`], registered by the shell.
//!
//! What this module guarantees, because the agents must never widen their own access:
//! * a folder gets on the list only through `FolderBridge::pick`, i.e. the open panel of the operating
//!   system, and `add_folders` is not reachable from an HTTP route, a setting, a tool or a model;
//! * what is stored is the permission (the bookmark), not a path: the path of a folder is whatever the
//!   operating system says the permission opens, at every launch;
//! * the folders are readable and writable by the file tools like the workspace, never more: the data
//!   folder of the app and the secret locations stay hidden even inside a folder that holds them.

use crate::{new_id, now_rfc3339, Engine};
use aiw_core::folders::{FolderBridge, FolderGrant, FoldersAdded, FoldersInfo};
use aiw_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Collection of the store.
const KIND: &str = "folder_grants";
/// Longest text of the panel the interface may pass.
const PANEL_TEXT_MAX: usize = 300;

/// What is stored for a folder: the permission and little else.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    id: String,
    /// Opaque, base64 of the bookmark. Never logged, never sent to the interface.
    bookmark: String,
    /// Name of the folder when it was added (shown while it cannot be opened).
    name: String,
    added: String,
}

#[derive(Debug, Clone)]
struct Entry {
    id: String,
    name: String,
    added: String,
    /// Canonical path while the permission is open.
    path: Option<PathBuf>,
    problem: Option<String>,
}

#[derive(Default)]
pub(crate) struct FolderState {
    bridge: Option<Arc<dyn FolderBridge>>,
    /// In the order they were added.
    list: Vec<Entry>,
}

fn folder_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

fn clip(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(PANEL_TEXT_MAX).collect()
}

impl Engine {
    /// Turns on the folders of the person (the sandboxed shell, once) and reopens the ones saved at the
    /// last launch. Call it before the channels start: a message that arrives at launch already finds the
    /// folders.
    pub fn set_folder_bridge(&self, bridge: Arc<dyn FolderBridge>) {
        self.0.folders.lock().unwrap().bridge = Some(bridge);
        self.restore_folders();
    }

    fn folder_bridge(&self) -> Option<Arc<dyn FolderBridge>> {
        self.0.folders.lock().unwrap().bridge.clone()
    }

    /// The real home folder of the person, when the platform knows one different from `~`.
    pub(crate) fn folder_bridge_home(&self) -> Option<PathBuf> {
        self.folder_bridge()?.real_home()
    }

    /// True when this build lets the person grant folders.
    pub fn folders_supported(&self) -> bool {
        self.folder_bridge().is_some()
    }

    fn restore_folders(&self) {
        let Some(bridge) = self.folder_bridge() else { return };
        let mut records: Vec<Record> = self.0.store.list_docs(KIND).unwrap_or_default();
        records.sort_by(|a, b| a.added.cmp(&b.added).then(a.id.cmp(&b.id)));
        let mut list = Vec::new();
        for mut rec in records {
            let mut entry = Entry { id: rec.id.clone(), name: rec.name.clone(), added: rec.added.clone(), path: None, problem: None };
            match bridge.open(&rec.bookmark) {
                Ok(opened) => {
                    let path = std::fs::canonicalize(&opened.path).unwrap_or(opened.path);
                    // A fresh permission replaces a stale one: without it the folder would be lost at the next launch.
                    if let Some(fresh) = opened.renewed {
                        rec.bookmark = fresh;
                        if let Err(e) = self.0.store.put_doc(KIND, &rec.id, &rec) {
                            tracing::warn!("folder permission not renewed: {e}");
                        }
                    }
                    if !path.is_dir() {
                        entry.problem = Some("la cartella non c'è più o non è una cartella".into());
                        bridge.release(&path);
                    } else if let Some(why) = self.refuse_folder(bridge.as_ref(), &path) {
                        entry.problem = Some(why);
                        bridge.release(&path);
                    } else {
                        entry.name = folder_name(&path);
                        entry.path = Some(path);
                    }
                }
                Err(e) => entry.problem = Some(e.to_string()),
            }
            list.push(entry);
        }
        self.0.folders.lock().unwrap().list = list;
    }

    /// Why a folder cannot go on the list: what the platform refuses, the whole disk, the data folder of
    /// the app, or a folder that holds it (it stays hidden anyway, but it must not be offered).
    fn refuse_folder(&self, bridge: &dyn FolderBridge, canon: &Path) -> Option<String> {
        if canon.parent().is_none() {
            return Some("è tutto il disco: scegli una cartella più piccola".into());
        }
        let data = std::fs::canonicalize(&self.0.dirs.root).unwrap_or_else(|_| self.0.dirs.root.clone());
        if canon.starts_with(&data) || data.starts_with(canon) {
            return Some("contiene i dati dell'app (o è fatta di quelli): non si può dare agli agenti".into());
        }
        bridge.refuse(canon)
    }

    /// The folders and their state, for the interface.
    pub fn folders(&self) -> FoldersInfo {
        let st = self.0.folders.lock().unwrap();
        FoldersInfo {
            supported: st.bridge.is_some(),
            folders: st
                .list
                .iter()
                .map(|e| FolderGrant {
                    id: e.id.clone(),
                    name: e.name.clone(),
                    path: e.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
                    available: e.path.is_some(),
                    problem: e.problem.clone(),
                    added: e.added.clone(),
                })
                .collect(),
        }
    }

    /// Opens the macOS panel and adds the folders the person chooses. Blocks until the panel closes:
    /// call it from a blocking context. Only the shell calls it, from a command of its own: there is no
    /// route, setting or tool that reaches it.
    pub fn add_folders(&self, message: &str, prompt: &str) -> Result<FoldersAdded> {
        let Some(bridge) = self.folder_bridge() else {
            return Err(Error::Unsupported(
                "questa versione di AI Workspace lavora in una cartella sola: si sceglie in Impostazioni".into(),
            ));
        };
        let picked = bridge.pick(&clip(message), &clip(prompt))?;
        let mut out = FoldersAdded::default();
        for p in picked {
            let canon = std::fs::canonicalize(&p.path).unwrap_or(p.path);
            let name = folder_name(&canon);
            let existing: Vec<(String, PathBuf)> = {
                let st = self.0.folders.lock().unwrap();
                st.list.iter().filter_map(|e| Some((e.name.clone(), e.path.clone()?))).collect()
            };
            if let Some((n, _)) = existing.iter().find(|(_, ep)| canon.starts_with(ep)) {
                // Already there, or inside one that is: nothing to add, and the permission held for it stays.
                if !existing.iter().any(|(_, ep)| *ep == canon) {
                    out.refused.push(format!("«{name}» è già dentro «{n}»"));
                }
                continue;
            }
            if let Some(why) = self.refuse_folder(bridge.as_ref(), &canon) {
                bridge.release(&canon);
                out.refused.push(format!("«{name}»: {why}"));
                continue;
            }
            let rec = Record { id: new_id("fld"), bookmark: p.bookmark, name: name.clone(), added: now_rfc3339() };
            if let Err(e) = self.0.store.put_doc(KIND, &rec.id, &rec) {
                bridge.release(&canon);
                return Err(e);
            }
            self.0.folders.lock().unwrap().list.push(Entry {
                id: rec.id.clone(),
                name: name.clone(),
                added: rec.added.clone(),
                path: Some(canon),
                problem: None,
            });
            self.audit_user("folders.add", &name);
            out.added += 1;
        }
        out.info = self.folders();
        Ok(out)
    }

    /// Takes a folder off the list and gives up its permission.
    pub fn remove_folder(&self, id: &str) -> Result<FoldersInfo> {
        let entry = {
            let mut st = self.0.folders.lock().unwrap();
            let Some(i) = st.list.iter().position(|e| e.id == id) else {
                return Err(Error::NotFound(format!("cartella {id} non trovata")));
            };
            st.list.remove(i)
        };
        self.0.store.delete_doc(KIND, id)?;
        if let (Some(path), Some(bridge)) = (&entry.path, self.folder_bridge()) {
            bridge.release(path);
        }
        self.audit_user("folders.remove", &entry.name);
        Ok(self.folders())
    }

    /// The folders open now, canonical: what the file tools may use besides the workspace.
    pub(crate) fn granted_roots(&self) -> Vec<PathBuf> {
        self.0.folders.lock().unwrap().list.iter().filter_map(|e| e.path.clone()).collect()
    }

    /// Hands the folders to the tools of a run, with where to grant more when a path is refused.
    pub(crate) fn apply_folders(&self, ctx: &mut aiw_tools::ToolContext) {
        if !self.folders_supported() {
            return;
        }
        ctx.write_roots = self.granted_roots();
        ctx.outside_hint = Some(
            "Se serve lavorare lì, solo l'utente può dare accesso a una cartella del computer: da Impostazioni → Cartelle consentite (non si fa dalla chat né dall'agente)"
                .into(),
        );
    }

    /// The line of the environment block that tells the model where the folders really are (`~` is the
    /// home of the app's container, not the one of the person).
    pub(crate) fn folders_env_line(&self) -> Option<String> {
        if !self.folders_supported() {
            return None;
        }
        let roots = self.granted_roots();
        let list = if roots.is_empty() {
            "nessuna (oltre al workspace l'utente non ha consentito altre cartelle)".to_string()
        } else {
            roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        };
        Some(format!(
            "\n- Cartelle del computer consentite dall'utente (lettura e scrittura, ma non si eliminano file): {list}. Usa i percorsi assoluti qui indicati: `~` è la cartella dell'app, non quella dell'utente (la sua Download, Documenti… sono lì sopra solo se elencate)"
        ))
    }
}
