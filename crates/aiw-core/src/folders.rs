//! Folders of the computer the person lets the agents use, beyond the workspace (Mac App Store build).
//!
//! The App Sandbox lets an app touch only its container and what the person picked in the macOS open
//! panel. A pick lasts until the app quits, unless it is kept as a *security-scoped bookmark* and reopened
//! at the next launch. This module is the platform-neutral half: what a platform has to offer
//! ([`FolderBridge`], implemented by the shell with AppKit) and what the interface is told ([`FolderGrant`],
//! [`FoldersInfo`]). The engine keeps the list and hands the folders to the file tools.
//!
//! A folder is added only by the person, through the panel of the operating system: the bridge is the
//! only way to get one, and it is only reachable from the shell (a Tauri command), never from a tool, a
//! model or an HTTP route.

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A folder just chosen in the panel, with the permission to reopen it later.
#[derive(Debug, Clone)]
pub struct PickedFolder {
    /// Absolute path, symlinks resolved.
    pub path: PathBuf,
    /// The permission, as opaque text (base64 of the bookmark): only the platform can read it.
    pub bookmark: String,
}

/// A permission reopened at launch.
#[derive(Debug, Clone)]
pub struct OpenedFolder {
    /// Where the folder is now (it may have been renamed or moved since it was chosen).
    pub path: PathBuf,
    /// A fresh permission when the saved one had gone stale: to store in its place.
    pub renewed: Option<String>,
}

/// What the platform does for the folders: the open panel and the persistent permission.
///
/// Every method blocks (the panel waits for the person): call them from a blocking context.
pub trait FolderBridge: Send + Sync + 'static {
    /// Opens the macOS panel on folders (several can be chosen). An empty list: the person cancelled.
    /// `message` and `prompt` are the explanation and the button of the panel, in the person's language.
    fn pick(&self, message: &str, prompt: &str) -> Result<Vec<PickedFolder>>;
    /// Reopens a saved permission and keeps it open until [`FolderBridge::release`].
    fn open(&self, bookmark: &str) -> Result<OpenedFolder>;
    /// Gives up the permission held for `path` (the folder was removed from the list).
    fn release(&self, path: &Path);
    /// Why `path` (canonical) cannot be handed to the agents, if so: the whole disk, the home folder or
    /// one that contains it, the system folders. `None`: allowed.
    fn refuse(&self, _path: &Path) -> Option<String> {
        None
    }
    /// The real home folder of the person. In a sandboxed app `~` is the container, so the secret
    /// locations of `sandbox.deny_read` must also be hidden at the real home.
    fn real_home(&self) -> Option<PathBuf> {
        None
    }
}

/// One folder of the list, as the interface shows it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FolderGrant {
    pub id: String,
    /// Last component of the path: what to call it.
    pub name: String,
    /// Where it is now. Empty when the folder cannot be opened.
    pub path: String,
    /// False when the permission could not be reopened (the folder is gone or was renamed): the agents
    /// do not see it; remove it and add it again.
    pub available: bool,
    /// Why it is not available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// RFC 3339.
    pub added: String,
}

/// The folders the agents may use besides the workspace.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FoldersInfo {
    /// This build lets the person grant folders (Mac App Store). False elsewhere: the list stays empty
    /// and the workspace is chosen in the settings.
    pub supported: bool,
    pub folders: Vec<FolderGrant>,
}

/// Result of adding folders from the panel.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FoldersAdded {
    #[serde(flatten)]
    pub info: FoldersInfo,
    /// Number of folders added now (the person may have cancelled, or picked ones already on the list).
    pub added: usize,
    /// Folders that were refused, each with the reason, in a sentence.
    #[serde(default)]
    pub refused: Vec<String>,
}
