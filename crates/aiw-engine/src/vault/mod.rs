//! Secrets at rest (issue #121): the values of the secrets stay in the store, **sealed** with a random key; only the key lives
//! in the keychain of the operating system, as one item.
//!
//! # How it works
//!
//! * **One key, one item.** At the first start with a keychain a random 256-bit key is made, stored in the keychain (service
//!   `AI Workspace Local - chiave dei segreti`, account = the id of the vault) and **read back** before anything is sealed with
//!   it. The id of the vault and the id of the key are the only things the database keeps about it (document `vault/main`).
//! * **One place where secrets go in and come out.** `Store::put_doc`, `get_doc` and `list_docs` seal and open the secret
//!   strings of the kinds listed in [`rules`], so the callers keep working with plain values; `LayeredConfig` does the same for
//!   the settings the schema marks `secret: true` (`search.brave_api_key`). A sealed value is a string where the plain one was:
//!   `aiwenc1:<key id>:<base64>` (AES-256-GCM, see [`envelope`]).
//! * **Migration at every start**, repeatable and reversible: what is plain is sealed (database and configuration files, in
//!   one transaction for the database, pages overwritten and log truncated so that no plain copy is left behind). With the
//!   setting `security.secrets_at_rest = off` the opposite runs: sealed values are written plain again and the keychain item
//!   is removed.
//! * **Where there is no keychain** (iOS and Android, a Linux without a Secret Service or without a session, a locked or
//!   denied keychain, a call that does not answer) the secrets stay plain, the state is **declared** (`aiw doctor`, Security,
//!   diagnostics, the secret card) and nothing fails or stops. When the keychain comes back, the next start seals them.
//!
//! # What can go wrong, and what is done about it
//!
//! * *The key is not in the keychain any more* (the item was deleted, the data folder came from another computer) while sealed
//!   values exist: the state is `key_lost`. Nothing is deleted and no new key is made over the old item; the sealed values stay
//!   as they are, are read as "not set", and `aiw secrets forget-unreadable` clears them so that the secrets are entered again.
//! * *The keychain cannot be used now* (locked, denied, no session): never a new key. The state is `unavailable`; the sealed
//!   values stay where they are and are read as "not set" for this run.
//! * *A value that cannot be opened* comes back from `get_doc` as its stored string and goes back in unchanged on the next
//!   `put_doc`: saving a document never destroys it.
//! * *Two processes starting together* both make a key, but only one claims the vault row; the other removes its item and uses
//!   the winner's.
//!
//! # What it does **not** do
//!
//! A program running as the same user can read the keychain item (macOS asks the person once per program, Windows and most
//! Linux services do not ask), so the encryption does **not** protect from it: it protects from a copy of the disk or of the
//! data folder, a backup, a synchronised folder, a file read by mistake. See `SECURITY.md`.
//!
//! # For a new kind of secret (the credentials per agent of issue #71, for example)
//!
//! Add the kind to [`rules::WHOLE_KINDS`] or [`rules::BY_NAME_KINDS`] and use `put_doc` / `get_doc` / `list_docs` as always:
//! sealing, migration, status, backup and the "no plain value on disk" check follow. A test fails until the kind is classified.
//! Never write a secret to a file or to a table of your own, and never read a document with SQL: that bypasses the vault.

mod engine;
pub mod envelope;
pub mod keystore;
pub mod rules;

use crate::store::Store;
use aiw_core::api::SecretsAtRest;
use aiw_core::config::LayeredConfig;
use aiw_core::paths::DataDirs;
use aiw_core::sealed::{is_sealed, SecretCodec, SecretRead};
use envelope::Key;
pub use keystore::{os_keystore, KeyRead, KeyStore, MemoryKeyStore, NoKeyStore, OS_KEYSTORE_BUILT};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

/// Kind of the document that says which keychain item holds the key. Never sealed, never in a backup.
const VAULT_KIND: &str = "vault";
const VAULT_ID: &str = "main";

/// What the database remembers about the key: where it is, and which one it is. Not the key.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultRow {
    /// The account of the keychain item.
    account: String,
    /// The id of the key (see [`envelope::key_id`]).
    key_id: String,
    created_at: String,
}

/// Where the vault stands. Every phase but `Active` leaves the secrets as they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// This build or this process does not use a keychain (mobile, the short-lived background worker, tests).
    NotUsed(String),
    /// Switched off by `security.secrets_at_rest = off`.
    Off,
    /// A keychain exists but cannot be used now (or at all on this machine/session): the text says why.
    Unavailable(String),
    /// Sealed values exist and their key is not in the keychain.
    KeyLost,
    /// Secrets are sealed with the key in the keychain.
    Active,
}

impl Phase {
    /// The word the API and the interface use.
    pub fn code(&self) -> &'static str {
        match self {
            Phase::NotUsed(_) => "not_used",
            Phase::Off => "off",
            Phase::Unavailable(_) => "unavailable",
            Phase::KeyLost => "key_lost",
            Phase::Active => "active",
        }
    }
}

struct State {
    phase: Phase,
    key: Option<Arc<Key>>,
    /// Account of the keychain item, once the vault exists.
    account: Option<String>,
}

pub struct Vault {
    keystore: Arc<dyn KeyStore>,
    state: RwLock<State>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Vault({})", self.phase().code())
    }
}

/// What a migration changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Migration {
    /// Documents and settings files rewritten with their secrets sealed.
    pub sealed_docs: usize,
    pub sealed_settings: usize,
    /// The same, in the other direction.
    pub unsealed_docs: usize,
    pub unsealed_settings: usize,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn wipe(v: &mut [u8]) {
    v.iter_mut().for_each(|b| *b = 0);
}

impl Vault {
    /// A vault over `keystore`, not started: until [`Vault::reconcile`] runs nothing is sealed and nothing opens.
    pub fn new(keystore: Arc<dyn KeyStore>) -> Arc<Self> {
        let phase = if keystore.is_used() { Phase::Unavailable("non ancora avviato".into()) } else { Phase::NotUsed(keystore.backend()) };
        Arc::new(Self { keystore, state: RwLock::new(State { phase, key: None, account: None }) })
    }

    pub fn phase(&self) -> Phase {
        self.state.read().unwrap().phase.clone()
    }

    pub fn backend(&self) -> String {
        self.keystore.backend()
    }

    fn set(&self, phase: Phase, key: Option<Arc<Key>>, account: Option<String>) {
        *self.state.write().unwrap() = State { phase, key, account };
    }

    fn key(&self) -> Option<Arc<Key>> {
        self.state.read().unwrap().key.clone()
    }

    // ------------------------------------------------------------------------------------------------ start

    /// Decides the phase from what the database, the keychain and the setting say, making the key when it is the first time.
    /// Never fails and never panics: whatever goes wrong becomes a declared phase. `wanted` is the setting
    /// (`security.secrets_at_rest` is not `off`).
    pub fn reconcile(&self, store: &Store, config: &LayeredConfig, wanted: bool) {
        if !self.keystore.is_used() {
            self.set(Phase::NotUsed(self.keystore.backend()), None, None);
            return;
        }
        let row = match store.get_doc::<VaultRow>(VAULT_KIND, VAULT_ID) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("segreti a riposo: la tabella del portachiavi non si legge: {e}");
                self.set(Phase::Unavailable(format!("il database non risponde: {e}")), None, None);
                return;
            }
        };
        match row {
            None if wanted => self.create(store),
            None => self.set(Phase::Off, None, None),
            Some(row) => self.adopt(store, config, row, wanted),
        }
    }

    /// First start with a keychain: make the key, store it, read it back, and only then claim the vault.
    fn create(&self, store: &Store) {
        let account = format!("v{}", hex(&envelope::random_bytes(8).unwrap_or_default()));
        let mut bytes = match envelope::random_bytes(envelope::KEY_LEN) {
            Ok(b) => b,
            Err(e) => return self.set(Phase::Unavailable(e), None, None),
        };
        let outcome = (|| -> Result<(Arc<Key>, VaultRow), String> {
            self.keystore.write(&account, &bytes).map_err(|e| format!("il portachiavi non accetta la chiave: {e}"))?;
            // Never seal with a key that cannot be read back: the secrets would be lost with the next start.
            match self.keystore.read(&account) {
                KeyRead::Found(back) if back == bytes => {}
                KeyRead::Found(_) => return Err("il portachiavi restituisce una chiave diversa da quella salvata".into()),
                KeyRead::Missing => return Err("il portachiavi non conserva la chiave appena salvata".into()),
                KeyRead::Inaccessible(e) => return Err(format!("il portachiavi non si legge: {e}")),
            }
            let key = Key::from_bytes(&bytes)?;
            let row = VaultRow { account: account.clone(), key_id: key.id().to_string(), created_at: aiw_core::now_rfc3339() };
            Ok((Arc::new(key), row))
        })();
        wipe(&mut bytes);
        let (key, row) = match outcome {
            Ok(v) => v,
            Err(why) => {
                let _ = self.keystore.remove(&account);
                tracing::warn!("segreti a riposo: chiave non creata: {why}");
                return self.set(Phase::Unavailable(why), None, None);
            }
        };
        match store.insert_doc_if_absent(VAULT_KIND, VAULT_ID, &row) {
            Ok(true) => {
                tracing::info!(backend = %self.keystore.backend(), "segreti a riposo: chiave creata nel portachiavi");
                self.set(Phase::Active, Some(key), Some(account));
            }
            Ok(false) => {
                // Another process made the vault between our check and now: use its key and drop ours.
                let _ = self.keystore.remove(&account);
                match store.get_doc::<VaultRow>(VAULT_KIND, VAULT_ID) {
                    Ok(Some(theirs)) => self.use_row(&theirs, true, None),
                    _ => self.set(Phase::Unavailable("il database non risponde".into()), None, None),
                }
            }
            Err(e) => {
                let _ = self.keystore.remove(&account);
                self.set(Phase::Unavailable(format!("il database non risponde: {e}")), None, None);
            }
        }
    }

    /// The vault exists: find its key. `missing` is the decision when the key is not there (see the module documentation).
    fn adopt(&self, store: &Store, config: &LayeredConfig, row: VaultRow, wanted: bool) {
        self.use_row(&row, wanted, Some((store, config)));
    }

    fn use_row(&self, row: &VaultRow, wanted: bool, ctx: Option<(&Store, &LayeredConfig)>) {
        let found = match self.keystore.read(&row.account) {
            KeyRead::Found(bytes) => match Key::from_bytes(&bytes) {
                Ok(k) if k.id() == row.key_id => Some(Arc::new(k)),
                _ => None, // an item that is not the key this vault was made with
            },
            KeyRead::Missing => None,
            KeyRead::Inaccessible(why) => {
                tracing::warn!("segreti a riposo: portachiavi non raggiungibile: {why}");
                return self.set(Phase::Unavailable(why), None, Some(row.account.clone()));
            }
        };
        if let Some(key) = found {
            let phase = if wanted { Phase::Active } else { Phase::Off };
            return self.set(phase, Some(key), Some(row.account.clone()));
        }
        // The key is not there. Is anything sealed that needs it?
        let Some((store, config)) = ctx else {
            return self.set(Phase::KeyLost, None, Some(row.account.clone()));
        };
        if sealed_values_exist(store, config) {
            tracing::warn!("segreti a riposo: la chiave non è più nel portachiavi e ci sono segreti cifrati: non li tocco");
            return self.set(Phase::KeyLost, None, Some(row.account.clone()));
        }
        // Nothing depends on the old key (a data folder copied without secrets, an item deleted before anything was sealed):
        // a new key does no harm.
        if wanted {
            self.rekey(store, &row.account);
        } else {
            let _ = store.delete_doc(VAULT_KIND, VAULT_ID);
            self.set(Phase::Off, None, None);
        }
    }

    /// Replaces a key nothing depends on: a new key under the same account, read back, then recorded.
    fn rekey(&self, store: &Store, account: &str) {
        let mut bytes = match envelope::random_bytes(envelope::KEY_LEN) {
            Ok(b) => b,
            Err(e) => return self.set(Phase::Unavailable(e), None, Some(account.into())),
        };
        let done = (|| -> Result<(Arc<Key>, VaultRow), String> {
            self.keystore.write(account, &bytes).map_err(|e| format!("il portachiavi non accetta la chiave: {e}"))?;
            match self.keystore.read(account) {
                KeyRead::Found(back) if back == bytes => {}
                _ => return Err("il portachiavi non restituisce la chiave appena salvata".into()),
            }
            let key = Key::from_bytes(&bytes)?;
            let row = VaultRow { account: account.into(), key_id: key.id().to_string(), created_at: aiw_core::now_rfc3339() };
            Ok((Arc::new(key), row))
        })();
        wipe(&mut bytes);
        match done {
            Ok((key, row)) => match store.put_doc(VAULT_KIND, VAULT_ID, &row) {
                Ok(()) => {
                    tracing::info!("segreti a riposo: nuova chiave nel portachiavi (la precedente non c'era più e nulla ne dipendeva)");
                    self.set(Phase::Active, Some(key), Some(account.into()));
                }
                Err(e) => self.set(Phase::Unavailable(format!("il database non risponde: {e}")), None, Some(account.into())),
            },
            Err(why) => self.set(Phase::Unavailable(why), None, Some(account.into())),
        }
    }

    // ------------------------------------------------------------------------------------------------ migration

    /// Brings what is stored in line with the phase: with the key active, plain secrets are sealed; switched off with the key
    /// at hand, sealed secrets are written plain and the keychain item goes. Repeatable: a second run changes nothing.
    pub fn migrate(&self, store: &Store, config: &LayeredConfig) -> Migration {
        let mut m = Migration::default();
        let (phase, key, account) = {
            let s = self.state.read().unwrap();
            (s.phase.clone(), s.key.clone(), s.account.clone())
        };
        match (phase, key) {
            (Phase::Active, Some(key)) => {
                match store.rewrite_secret_docs(&mut |kind, doc| {
                    let mut changed = false;
                    rules::visit_secrets(kind, doc, &mut |s| {
                        if !s.is_empty() && !is_sealed(s) {
                            if let Ok(sealed) = key.seal(s) {
                                *s = sealed;
                                changed = true;
                            }
                        }
                    });
                    changed
                }) {
                    Ok(n) => m.sealed_docs = n,
                    Err(e) => tracing::warn!("segreti a riposo: migrazione del database non riuscita (restano come sono): {e}"),
                }
                match config.reconcile_files() {
                    Ok(n) => m.sealed_settings = n,
                    Err(e) => tracing::warn!("segreti a riposo: migrazione delle impostazioni non riuscita (restano come sono): {e}"),
                }
            }
            (Phase::Off, Some(key)) => {
                match store.rewrite_secret_docs(&mut |kind, doc| {
                    let mut changed = false;
                    rules::visit_secrets(kind, doc, &mut |s| {
                        if is_sealed(s) {
                            if let Some(plain) = key.open(s) {
                                *s = plain;
                                changed = true;
                            }
                        }
                    });
                    changed
                }) {
                    Ok(n) => m.unsealed_docs = n,
                    Err(e) => tracing::warn!("segreti a riposo: il ritorno in chiaro del database non è riuscito: {e}"),
                }
                match config.reconcile_files() {
                    Ok(n) => m.unsealed_settings = n,
                    Err(e) => tracing::warn!("segreti a riposo: il ritorno in chiaro delle impostazioni non è riuscito: {e}"),
                }
                // Only when nothing sealed is left, the key is not needed any more: remove it and forget the vault.
                if !sealed_values_exist(store, config) {
                    if let Some(account) = account {
                        match self.keystore.remove(&account) {
                            Ok(()) => {
                                let _ = store.delete_doc(VAULT_KIND, VAULT_ID);
                                self.set(Phase::Off, None, None);
                                tracing::info!("segreti a riposo: di nuovo in chiaro, chiave tolta dal portachiavi");
                            }
                            Err(e) => tracing::warn!("segreti a riposo: la chiave resta nel portachiavi ({e})"),
                        }
                    }
                }
            }
            _ => {}
        }
        m
    }

    /// Clears every secret that cannot be opened (a value sealed with a key that is gone), so that it can be entered again, and
    /// makes a new key when nothing sealed is left. Returns how many values were cleared. This is the only way a sealed value
    /// is ever discarded, and only on an explicit request.
    pub fn forget_unreadable(&self, store: &Store, config: &LayeredConfig, wanted: bool) -> usize {
        let key = self.key();
        let mut cleared = 0;
        let unreadable = |s: &str| is_sealed(s) && key.as_ref().is_none_or(|k| k.open(s).is_none());
        let _ = store.rewrite_secret_docs(&mut |kind, doc| {
            let mut changed = false;
            rules::visit_secrets(kind, doc, &mut |s| {
                if unreadable(s) {
                    s.clear();
                    changed = true;
                    cleared += 1;
                }
            });
            changed
        });
        cleared += config.clear_unreadable_secrets(&|s| unreadable(s));
        self.reconcile(store, config, wanted);
        let _ = self.migrate(store, config);
        cleared
    }

    // ------------------------------------------------------------------------------------------------ report

    /// The status the interface, `aiw doctor` and the diagnostics show.
    pub fn status(&self, store: &Store, config: &LayeredConfig) -> SecretsAtRest {
        let key = self.key();
        let (mut encrypted, mut plain, mut unreadable) = (0u64, 0u64, 0u64);
        let mut tally = |s: &str| {
            if !is_sealed(s) {
                plain += 1;
            } else if key.as_ref().is_some_and(|k| k.open(s).is_some()) {
                encrypted += 1;
            } else {
                unreadable += 1;
            }
        };
        for (_, s) in store.stored_secret_strings().unwrap_or_default() {
            tally(&s);
        }
        for s in config.secrets_on_disk() {
            tally(&s);
        }
        let phase = self.phase();
        let detail = match &phase {
            Phase::NotUsed(why) | Phase::Unavailable(why) => Some(why.clone()),
            _ => None,
        };
        SecretsAtRest {
            state: phase.code().into(),
            backend: self.keystore.is_used().then(|| self.keystore.backend()),
            detail,
            encrypted,
            plain,
            unreadable,
        }
    }

    /// A reader of the vault of an existing data folder, for what runs without an engine (the backup of the command line): it
    /// reads the vault row and the key and changes nothing, and opens whatever it can.
    pub fn reader(dirs: &DataDirs, keystore: Arc<dyn KeyStore>) -> Arc<Self> {
        let v = Self::new(keystore);
        if !v.keystore.is_used() {
            return v;
        }
        let row = rusqlite::Connection::open_with_flags(dirs.db_file(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .ok()
            .and_then(|c| {
                c.query_row("SELECT json FROM documents WHERE kind = ?1 AND id = ?2", [VAULT_KIND, VAULT_ID], |r| r.get::<_, String>(0))
                    .ok()
            })
            .and_then(|json| serde_json::from_str::<VaultRow>(&json).ok());
        match row {
            // No vault: nothing was ever sealed here.
            None => v.set(Phase::Off, None, None),
            Some(row) => v.use_row(&row, true, None),
        }
        v
    }
}

/// Is any stored secret sealed (documents and settings files)?
fn sealed_values_exist(store: &Store, config: &LayeredConfig) -> bool {
    store.stored_secret_strings().unwrap_or_default().iter().any(|(_, s)| is_sealed(s))
        || config.secrets_on_disk().iter().any(|s| is_sealed(s))
}

impl SecretCodec for Vault {
    fn is_sealing(&self) -> bool {
        let s = self.state.read().unwrap();
        s.phase == Phase::Active && s.key.is_some()
    }

    fn seal(&self, plain: &str) -> Option<String> {
        let s = self.state.read().unwrap();
        if s.phase != Phase::Active {
            return None;
        }
        s.key.as_ref()?.seal(plain).ok()
    }

    fn open(&self, stored: &str) -> SecretRead {
        if !is_sealed(stored) {
            return SecretRead::Plain;
        }
        match self.key().and_then(|k| k.open(stored)) {
            Some(plain) => SecretRead::Opened(plain),
            None => SecretRead::Unreadable,
        }
    }
}

/// Which keychain an engine uses (`EngineOptions::keychain`).
#[derive(Clone, Default)]
pub enum Keychain {
    /// None: secrets stay as they are and the state says `not_used`. The default, so that tests and the short-lived background
    /// worker never touch a real keychain.
    #[default]
    None,
    /// The keychain of the operating system (the app and the daemon ask for this).
    System,
    /// A given one (tests).
    Custom(Arc<dyn KeyStore>),
}

impl std::fmt::Debug for Keychain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Keychain::None => write!(f, "Keychain::None"),
            Keychain::System => write!(f, "Keychain::System"),
            Keychain::Custom(k) => write!(f, "Keychain::Custom({k:?})"),
        }
    }
}

impl Keychain {
    pub fn keystore(&self) -> Arc<dyn KeyStore> {
        match self {
            Keychain::None => Arc::new(NoKeyStore::new("questa copia di AIW non usa il portachiavi del sistema")),
            Keychain::System => os_keystore(),
            Keychain::Custom(k) => k.clone(),
        }
    }
}

/// Starts the vault of a data folder: reconciles it, hands the store and the configuration their codec, runs the migration.
/// Call it right after the store is opened and before anything reads a secret.
pub fn start(store: &Store, config: &LayeredConfig, keychain: &Keychain) -> Arc<Vault> {
    let vault = Vault::new(keychain.keystore());
    let wanted = config.get_str("security.secrets_at_rest").as_deref() != Some("off");
    vault.reconcile(store, config, wanted);
    store.set_codec(vault.clone());
    config.set_codec(vault.clone());
    let m = vault.migrate(store, config);
    if m != Migration::default() {
        tracing::info!(
            sealed = m.sealed_docs + m.sealed_settings,
            unsealed = m.unsealed_docs + m.unsealed_settings,
            "segreti a riposo: migrazione eseguita"
        );
    }
    vault
}

#[cfg(test)]
mod tests;
