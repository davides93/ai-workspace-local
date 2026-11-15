//! Secrets at rest: the shape of a sealed value and the hook the layers use to read and write one.
//!
//! The cipher and the key live in the engine (`aiw_engine::vault`); this module only fixes what a stored value looks like
//! and what a place that keeps secrets (the configuration files here, the documents of the database in the engine) asks of
//! the cipher. Keeping it in the core lets the configuration layers protect their secret settings without knowing how.
//!
//! **A sealed value is a string** that starts with [`SEALED_PREFIX`]: `aiwenc1:<key id>:<base64>`. It sits exactly where the
//! plain value was, so a stored document or a settings file keeps its shape.

use std::fmt::Debug;

/// Start of every sealed value this build makes. The `1` is the version of the format: a reader that does not know a newer one
/// treats the value as unreadable, never as plain.
pub const SEALED_PREFIX: &str = "aiwenc1:";

/// Is `s` a sealed value (whatever the key that sealed it, and whatever version of the format: one a newer build wrote is
/// sealed too, and this one leaves it exactly as it is)?
pub fn is_sealed(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("aiwenc") else { return false };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && rest.as_bytes().get(digits) == Some(&b':')
}

/// What reading a stored secret string gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretRead {
    /// Not sealed: the string is the value (a secret stored before the encryption, or while no key is available).
    Plain,
    /// Sealed, and opened: the value.
    Opened(String),
    /// Sealed, but this process cannot open it (the key is not in the keychain, is not reachable now, or the value was sealed
    /// by another key or a newer version). **The stored string must be left as it is**: it is not lost, and writing the
    /// document back with the sealed string in place keeps it.
    Unreadable,
}

/// Seals and opens secret strings. The engine's vault implements it; the layers and the backup call it.
pub trait SecretCodec: Debug + Send + Sync {
    /// Does [`SecretCodec::seal`] seal right now?
    fn is_sealing(&self) -> bool;
    /// The sealed form of `plain`, or `None` when nothing can be sealed now (no keychain, or encryption switched off): the
    /// caller then stores `plain` as it is.
    fn seal(&self, plain: &str) -> Option<String>;
    /// What `stored` reads as. Never fails and never panics: anything it cannot open is [`SecretRead::Unreadable`].
    fn open(&self, stored: &str) -> SecretRead;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_is_recognised_in_every_version_and_nothing_else_is() {
        for s in ["aiwenc1:ab12cd34:AAAA", "aiwenc2:x:y", "aiwenc10:x:y", SEALED_PREFIX] {
            assert!(is_sealed(s), "{s}");
        }
        for s in ["", "aiwenc", "aiwenc:x", "aiwencX:1", "aiwenc1", "hf_token", " aiwenc1:x", "AIWENC1:x"] {
            assert!(!is_sealed(s), "{s}");
        }
    }
}
