//! Windows sandbox for the commands agents run (ADR 0005, pattern P07).
//!
//! * The command runs with a **Low integrity** copy of the user's token, stripped of privileges
//!   (`CreateRestrictedToken(DISABLE_MAX_PRIVILEGE)`): Windows' mandatory integrity control stops
//!   it from writing to files, folders and registry keys at the normal (Medium) level, i.e.
//!   almost everything the user owns, whatever the groups in the token (the integrity check
//!   comes before the DACL).
//! * In `workspace-write` mode the workspace folder gets a Low integrity label inherited by its
//!   content (`S:(ML;OICI;NW;;;LW)`), so the workspace is the one place it can write to, besides
//!   the per-user Low folders (`LocalLow`), where its `TEMP` points.
//! * The process tree lives in a **job object**: killed as a whole on timeout or cancellation and
//!   when the job handle closes, without clipboard, desktop and global-atom access.
//! * Only the stdin (`NUL`) and output pipe handles are inherited (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`).
//! * It runs on a window station and desktop of its own, labelled Low (as in Chromium's sandbox):
//!   user32 initialises even when the app runs as a service, and the command cannot reach the
//!   user's clipboard, global atoms or windows.
//!
//! Network access is not restricted by this mechanism (Low integrity processes can still open
//! sockets; closing it needs an AppContainer or firewall rules, which this crate does not set up),
//! and **reading** is not restricted either: Low integrity only stops writes, so a command can read
//! the files the user can read, the app's data folder and credentials included. The caller reports
//! both limits to the user. [`run_clean`] drops named variables from the environment the command
//! inherits (the secrets of the caller's environment).
//!
//! On non-Windows targets the crate is empty.

#[cfg(windows)]
mod imp;

#[cfg(windows)]
pub use imp::*;

/// What the sandboxed command may write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Nothing at Medium integrity (only Low folders such as its TEMP).
    ReadOnly,
    /// The workspace folder too.
    WorkspaceWrite,
}

/// Result of a sandboxed command.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub exit_code: Option<u32>,
    /// stdout and stderr, interleaved as written (truncated to the requested cap).
    pub output: Vec<u8>,
    pub timed_out: bool,
    pub cancelled: bool,
}
