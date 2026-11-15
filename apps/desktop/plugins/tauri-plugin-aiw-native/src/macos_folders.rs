//! The folders of the computer handed to the agents, on macOS (App Sandbox): the open panel and the
//! security-scoped bookmarks, reached through the C ABI of `macos-folders/Folders.swift`. This is the
//! [`aiw_core::folders::FolderBridge`] the shell registers in the engine; see there for what it guarantees.
use crate::folder_rules::refuse_reason;
use crate::native_call::{self, CallError};
use aiw_core::folders::{FolderBridge, OpenedFolder, PickedFolder};
use aiw_core::{Error, Result};
use serde_json::{json, Value};
use std::ffi::c_char;
use std::path::{Path, PathBuf};
use std::time::Duration;

extern "C" {
    fn aiw_folders_call(id: u64, request: *const c_char, reply: extern "C" fn(u64, *const c_char));
}

/// How long the person may keep the panel open before the call gives up (the thread must not wait forever).
const PICK_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Reopening a bookmark or giving it up answers at once.
const QUICK_TIMEOUT: Duration = Duration::from_secs(30);

fn call(method: &str, payload: Value, timeout: Duration) -> Result<Value> {
    native_call::call(aiw_folders_call, method, payload, timeout).map_err(|e| match e {
        CallError::Timeout => Error::Runtime("macOS non ha risposto in tempo sulle cartelle".into()),
        CallError::Failed(m) => Error::Runtime(m),
    })
}

/// [`FolderBridge`] over AppKit.
pub struct MacFolders {
    real_home: Option<PathBuf>,
    container_home: Option<PathBuf>,
}

impl MacFolders {
    pub fn new() -> Self {
        let real_home = call("realHome", json!({}), QUICK_TIMEOUT)
            .ok()
            .and_then(|v| v["path"].as_str().map(PathBuf::from))
            .map(|p| std::fs::canonicalize(&p).unwrap_or(p));
        // `HOME` of a sandboxed app is its container.
        let container_home = std::env::var_os("HOME").map(PathBuf::from).map(|p| std::fs::canonicalize(&p).unwrap_or(p));
        let container_home = container_home.filter(|c| Some(c) != real_home.as_ref());
        Self { real_home, container_home }
    }
}

impl Default for MacFolders {
    fn default() -> Self {
        Self::new()
    }
}

impl FolderBridge for MacFolders {
    fn pick(&self, message: &str, prompt: &str) -> Result<Vec<PickedFolder>> {
        let v = call("pick", json!({"message": message, "prompt": prompt}), PICK_TIMEOUT)?;
        Ok(v["folders"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| {
                        Some(PickedFolder { path: PathBuf::from(f["path"].as_str()?), bookmark: f["bookmark"].as_str()?.to_string() })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn open(&self, bookmark: &str) -> Result<OpenedFolder> {
        let v = call("open", json!({"bookmark": bookmark}), QUICK_TIMEOUT)?;
        let path = v["path"].as_str().ok_or_else(|| Error::Runtime("macOS non ha detto dov'è la cartella".into()))?;
        Ok(OpenedFolder { path: PathBuf::from(path), renewed: v["bookmark"].as_str().map(String::from) })
    }

    fn release(&self, path: &Path) {
        let _ = call("release", json!({"path": path.display().to_string()}), QUICK_TIMEOUT);
    }

    fn refuse(&self, path: &Path) -> Option<String> {
        refuse_reason(path, self.real_home.as_deref(), self.container_home.as_deref())
    }

    fn real_home(&self) -> Option<PathBuf> {
        self.real_home.clone()
    }
}
