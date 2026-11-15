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
use std::path::PathBuf;

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

/// Can this id be the name of a folder, the same on every file system? ASCII letters, digits, `-`, `_` and `.` (not first),
/// up to 64 characters. Nothing with a separator or `..`, so the folder cannot leave `<data>/agents`.
pub(crate) fn is_safe_dir_name(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_DIR_NAME
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !id.contains("..")
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
        if a.isolation.is_none() {
            a.isolation = self.0.store.get_doc::<AgentProfile>("agent", &a.id)?.and_then(|old| old.isolation);
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

    /// Takes the folder of a removed agent out of the way: it moves under `agents/.removed/<id>-<time>`, which no agent can
    /// reach (nobody sees the data folder), so an agent created later with the same id starts from an empty folder and
    /// inherits nothing. Nothing is deleted: the files are the person's work. `None` when there was no folder.
    pub(crate) fn archive_agent_workspace(&self, agent_id: &str) -> Result<Option<PathBuf>> {
        let id = base_agent_id(agent_id);
        if !is_safe_dir_name(id) {
            return Ok(None);
        }
        let dir = self.0.dirs.agents().join(id);
        if !dir.exists() {
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
        for ok in ["coder", "agent_1", "mio-agente", "a.b", "A1"] {
            assert!(is_safe_dir_name(ok), "{ok}");
        }
        let long = "a".repeat(65);
        for bad in ["", ".", "..", ".nascosto", "a/b", "a\\b", "../x", "a..b", "a b", "è", "a:b", "team:x", long.as_str()] {
            assert!(!is_safe_dir_name(bad), "{bad}");
        }
    }
}
