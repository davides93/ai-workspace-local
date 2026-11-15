//! What the engine offers about the secrets at rest: the status, the codec the backup uses, and the two actions a person can
//! ask for (`aiw secrets`).

use super::{Migration, Phase};
use crate::Engine;
use aiw_core::api::SecretsAtRest;
use aiw_core::sealed::SecretCodec;
use std::sync::Arc;

impl Engine {
    /// Whether the secrets are encrypted on disk, with the counts. No value is in it. Declared in `aiw doctor`, the diagnostics
    /// and the Security view.
    pub fn secrets_at_rest(&self) -> SecretsAtRest {
        self.0.vault.status(&self.0.store, &self.0.config)
    }

    /// `encrypted` when the secrets the person gives now are sealed on this computer, `plain` when they are stored as they are.
    pub(crate) fn secrets_at_rest_word(&self) -> &'static str {
        if self.0.vault.phase() == Phase::Active {
            "encrypted"
        } else {
            "plain"
        }
    }

    /// The vault as a codec, for what writes a copy of the data (the backup with secrets).
    pub fn secrets_codec(&self) -> Arc<dyn SecretCodec> {
        self.0.vault.clone()
    }

    /// Runs the vault's decisions again with the setting as it is now (after `security.secrets_at_rest` changed, or the keychain
    /// came back) and the migration that follows. The app does this at every start; this is for the command line.
    pub fn secrets_reconcile(&self) -> Migration {
        let wanted = self.0.config.get_str("security.secrets_at_rest").as_deref() != Some("off");
        self.0.vault.reconcile(&self.0.store, &self.0.config, wanted);
        self.0.vault.migrate(&self.0.store, &self.0.config)
    }

    /// Clears the secrets that cannot be opened (see [`super::Vault::forget_unreadable`]). Returns how many.
    pub fn secrets_forget_unreadable(&self) -> usize {
        let wanted = self.0.config.get_str("security.secrets_at_rest").as_deref() != Some("off");
        self.0.vault.forget_unreadable(&self.0.store, &self.0.config, wanted)
    }
}
