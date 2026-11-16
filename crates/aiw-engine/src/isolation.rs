//! Agents kept apart from one another (issue #71): the folder an agent works in when it has one of its own, what
//! `save_agent` accepts, and what happens to that folder when the agent is removed.
//!
//! The folder is `<data>/agents/<id>/workspace`, inside the data folder of the app. That folder is hidden from every
//! agent (file tools and sandboxed commands), and a workspace that lives inside a hidden folder is reachable only by the
//! run that works there (`ToolContext::is_hidden`, `sandbox::SandboxSpec`: the same exception the isolated sessions in
//! `<data>/worktrees` use). So the isolation is not a second mechanism: an agent with a folder of its own reaches that folder
//! and nothing else, and every other agent keeps being refused the whole data folder, this one included.
//!
//! What it does not do: an agent whose sandbox is `off` reads whatever the account can (that is what `off` means), and on
//! Windows a command cannot be kept from reading folders (`docs/agenti-isolati.md`).

use crate::{now_rfc3339, Engine};
use aiw_core::api::AgentProfile;
use aiw_core::{Error, Result};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// Longest id of an agent that gets a folder of its own.
const MAX_DIR_NAME: usize = 64;

/// Folder of the workspaces of the agents that were removed (inside [`aiw_core::paths::DataDirs::agents`]).
const REMOVED: &str = ".removed";

/// The id of the agent a run was built from: a background proposal (`proactive:<id>`) and a channel run
/// (`channel:ro:<id>`, `channel:ask:<id>`) work for the agent `<id>`, with its folder and not with its credentials.
pub(crate) fn base_agent_id(id: &str) -> &str {
    if let Some(base) = id.strip_prefix(crate::proactive::PROACTIVE_PREFIX) {
        return base;
    }
    if let Some((_, base)) = crate::channels::agent_kind(id) {
        return base;
    }
    id
}

/// True when the id was made by a background proposal or a channel: such a run is not the agent talking.
pub(crate) fn is_derived_agent_id(id: &str) -> bool {
    base_agent_id(id) != id
}

/// Names Windows reserves for devices (with or without an extension): never a folder.
const WINDOWS_RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4",
    "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Can this id be the name of a folder, the same on every file system? ASCII letters, digits, `-`, `_` and `.` (not first, not
/// last: Windows drops a trailing dot, so `alfa.` and `alfa` would be one folder), up to 64 characters, not a name Windows
/// reserves. Nothing with a separator or `..`, so the folder cannot leave `<data>/agents`.
pub(crate) fn is_safe_dir_name(id: &str) -> bool {
    let stem = id.split('.').next().unwrap_or_default().to_ascii_lowercase();
    !id.is_empty()
        && id.len() <= MAX_DIR_NAME
        && !id.starts_with('.')
        && !id.ends_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !id.contains("..")
        && !WINDOWS_RESERVED.contains(&stem.as_str())
}

/// Reads `<folder>/<name>` as text (at most `max` characters) only if it is a regular file and not a link, whatever the link points
/// at. The engine reads these files itself, outside the sandbox, and puts them in the prompt: a link the agent made in its own
/// folder (`AGENTS.md -> ../beta/workspace/nota.txt`, `-> <dati>/token`) must not carry another file in. The file is opened first
/// and compared with what the path says (same device and inode), so it cannot be swapped for a link between the check and the read.
pub(crate) fn read_regular_text(folder: &Path, name: &str, max: usize) -> Option<String> {
    let path = folder.join(name);
    let link = std::fs::symlink_metadata(&path).ok()?;
    if !link.file_type().is_file() {
        return None; // a link, a folder, anything but the file itself
    }
    let file = std::fs::File::open(&path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata().ok()?;
        if (opened.dev(), opened.ino()) != (link.dev(), link.ino()) {
            return None; // the path now leads elsewhere
        }
    }
    use std::io::Read;
    let mut text = String::new();
    file.take((max as u64).saturating_mul(4)).read_to_string(&mut text).ok()?;
    Some(text.chars().take(max).collect())
}

impl Engine {
    /// The folder agent `agent_id` works in when it has one of its own (not created here).
    pub(crate) fn dedicated_workspace_path(&self, agent_id: &str) -> Result<PathBuf> {
        let id = base_agent_id(agent_id);
        if !is_safe_dir_name(id) {
            return Err(Error::Invalid(format!("l'id «{id}» non può dare il nome a una cartella")));
        }
        Ok(self.0.dirs.agents().join(id).join("workspace"))
    }

    /// The folder of the agent, created if it is missing. A failure to create it is not turned into the shared workspace:
    /// the path is returned all the same, and every tool that needs it fails there.
    pub(crate) fn dedicated_workspace(&self, agent_id: &str) -> PathBuf {
        match self.dedicated_workspace_path(agent_id) {
            Ok(p) => {
                if let Err(e) = std::fs::create_dir_all(&p) {
                    tracing::warn!("workspace dedicato non creato ({}): {e}", p.display());
                }
                p
            }
            // An id that cannot be a folder never gets here through `save_agent`; if it does, the run gets a folder that does
            // not exist rather than the shared workspace.
            Err(_) => self.0.dirs.agents().join(REMOVED).join("non-valido").join("workspace"),
        }
    }

    /// What `save_agent` does with the isolation of the profile it is given: an absent one keeps the stored one (an editor
    /// that does not know the field must not switch it off by saving), an inactive one is stored as absent, and a folder of
    /// its own is refused where it would not hold: no sandbox, an id that cannot be a folder, an id that differs from another
    /// only by case (the same folder on macOS and Windows).
    pub(crate) fn check_isolation(&self, a: &mut AgentProfile) -> Result<()> {
        // A stored profile that cannot be read is replaced, as it always was: it does not stop the save.
        let stored = self.0.store.get_doc::<AgentProfile>("agent", &a.id).ok().flatten();
        if a.isolation.is_none() {
            a.isolation = stored.as_ref().and_then(|old| old.isolation.clone());
        }
        // A new agent whose id differs from another only by case would, on macOS and Windows, share its folder with it (and a
        // removal would move the other one's folder aside): refused whatever it asks for.
        if stored.is_none() {
            if let Some(other) = self.agents()?.into_iter().find(|o| o.id != a.id && o.id.eq_ignore_ascii_case(&a.id)) {
                return Err(Error::Conflict(format!(
                    "l'id «{}» differisce solo per maiuscole da quello di «{}»: su questo sistema sarebbero lo stesso nome di cartella",
                    a.id, other.id
                )));
            }
        }
        if a.isolation.as_ref().is_some_and(|i| !i.is_active()) {
            a.isolation = None;
        }
        if !a.has_dedicated_workspace() {
            return Ok(());
        }
        if a.sandbox == "off" {
            return Err(Error::Invalid(
                "un agente con un workspace dedicato non può avere la sandbox spenta: senza sandbox nessuna cartella è separata dalle altre".into(),
            ));
        }
        if !is_safe_dir_name(&a.id) {
            return Err(Error::Invalid(format!(
                "l'id «{}» non può dare il nome alla cartella di un workspace dedicato (solo lettere, cifre, - _ . e al massimo {MAX_DIR_NAME} caratteri)",
                a.id
            )));
        }
        if let Some(other) = self.agents()?.into_iter().find(|o| o.id != a.id && o.id.eq_ignore_ascii_case(&a.id)) {
            return Err(Error::Conflict(format!(
                "l'agente «{}» ha un id che differisce solo per maiuscole da «{}»: su questo sistema avrebbero la stessa cartella",
                a.id, other.id
            )));
        }
        Ok(())
    }

    /// The agent has a folder of its own or credentials of its own: the conversation does not move to or from it.
    pub(crate) fn kept_apart(&self, a: &AgentProfile) -> bool {
        a.has_dedicated_workspace() || self.agent_has_credentials(&a.id)
    }

    /// What keeps an agent from being really apart on this computer, in words: empty when nothing is missing. The shell of an agent
    /// that has one runs where this machine can contain a command (bubblewrap, Seatbelt) and nowhere else.
    pub fn isolation_warnings(&self, a: &AgentProfile) -> Vec<String> {
        let mut out = vec![];
        let has_shell = a.tools.iter().any(|t| t == "shell" || t == "*");
        if a.has_dedicated_workspace() && has_shell && !aiw_tools::sandbox::can_wrap_process() {
            let why = if cfg!(windows) {
                "su Windows la shell non può nascondere cartelle in lettura"
            } else {
                "su questo computer non c'è una sandbox che contenga i comandi (bubblewrap su Linux, Seatbelt su macOS)"
            };
            out.push(format!(
                "{why}: un comando di questo agente può leggere le cartelle degli altri agenti e la cartella dei dati. I suoi file tool restano separati; per tenerlo davvero da solo togli `shell` dai suoi strumenti"
            ));
        }
        if a.isolation.as_ref().is_some_and(|i| i.close_shell_network) && has_shell && !aiw_tools::sandbox::can_wrap_process() {
            out.push("su questo computer la rete dei comandi non si può chiudere: `close_shell_network` non ha effetto sulla shell".into());
        }
        out
    }

    /// Stops the runs that were started for sessions of `agent_id` (the agent is being removed): its tools must not go on working in a
    /// folder that is being put away. Best effort: a tool that is in the middle of a call finishes it.
    pub(crate) fn cancel_runs_of_agent(&self, agent_id: &str) {
        let sessions: Vec<(String, CancellationToken)> = {
            let runs = self.0.runs.lock().unwrap();
            runs.values().filter(|h| !h.done).filter_map(|h| Some((h.session_id.clone()?, h.cancel.clone()))).collect()
        };
        for (session, cancel) in sessions {
            if self.0.store.session(&session).is_ok_and(|s| base_agent_id(&s.agent_id) == agent_id) {
                cancel.cancel();
            }
        }
    }

    /// Takes the folder of a removed agent out of the way: it moves under `agents/.removed/<id>-<time>`, which no agent can
    /// reach (nobody sees the data folder), so an agent created later with the same id starts from an empty folder and
    /// inherits nothing. Nothing is deleted: the files are the person's work. `None` when there was no folder.
    pub(crate) fn archive_agent_workspace(&self, agent_id: &str) -> Result<Option<PathBuf>> {
        let id = base_agent_id(agent_id);
        if !is_safe_dir_name(id) {
            return Ok(None);
        }
        let dir = self.0.dirs.agents().join(id);
        // A file system that ignores case finds `ALFA` when asked for `alfa`: only a folder with exactly this name is the agent's.
        let exact = std::fs::read_dir(self.0.dirs.agents()).is_ok_and(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy() == id));
        if !exact {
            return Ok(None);
        }
        let stamp: String = now_rfc3339().chars().filter(|c| c.is_ascii_digit()).collect();
        let removed = self.0.dirs.agents().join(REMOVED);
        std::fs::create_dir_all(&removed)?;
        let mut target = removed.join(format!("{id}-{stamp}"));
        let mut n = 1;
        while target.exists() {
            n += 1;
            target = removed.join(format!("{id}-{stamp}-{n}"));
        }
        std::fs::rename(&dir, &target)?;
        Ok(Some(target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_runs_work_for_the_agent_they_come_from() {
        assert_eq!(base_agent_id("coder"), "coder");
        assert_eq!(base_agent_id("proactive:coder"), "coder");
        assert_eq!(base_agent_id("channel:ro:coder"), "coder");
        assert_eq!(base_agent_id("channel:ask:coder"), "coder");
        assert!(is_derived_agent_id("proactive:coder") && is_derived_agent_id("channel:ask:coder") && !is_derived_agent_id("coder"));
    }

    #[test]
    fn only_plain_ids_can_name_a_folder() {
        for ok in ["coder", "agent_1", "mio-agente", "a.b", "A1", "console", "com10"] {
            assert!(is_safe_dir_name(ok), "{ok}");
        }
        let long = "a".repeat(65);
        for bad in [
            "",
            ".",
            "..",
            ".nascosto",
            "a/b",
            "a\\b",
            "../x",
            "a..b",
            "a b",
            "è",
            "a:b",
            "team:x",
            "alfa.",
            "CON",
            "nul",
            "Aux.txt",
            "com1",
            "LPT9",
            long.as_str(),
        ] {
            assert!(!is_safe_dir_name(bad), "{bad}");
        }
    }
}
