//! Where the key of the secrets lives: the keychain of the operating system, behind a small trait.
//!
//! One item per data folder (the service name is fixed, the account is the id of the vault). Three outcomes are told apart on
//! purpose, because they call for opposite behaviour: the key **is there** ([`KeyRead::Found`]), the keychain answers that it
//! has **no such item** ([`KeyRead::Missing`]: a new key may be made, if nothing sealed with the old one is stored), or the
//! keychain **cannot be used now** ([`KeyRead::Inaccessible`]: locked, denied, no session, no service; a key must never be
//! made because of that, the secrets are only declared unreadable for this run).
//!
//! The real store ([`OsKeyStore`]) exists where it was proven to compile and where a keychain exists to talk to: macOS
//! (Keychain), Windows (Credential Manager) and Linux (Secret Service over D-Bus: GNOME Keyring, KWallet). Everywhere else
//! (iOS, Android, other Unixes) there is [`NoKeyStore`], and the secrets stay as they are with the state declared. The
//! `keyring` crates are used through `keyring-core` and one store per platform, never through the umbrella crate: without a
//! platform store it falls back to an in-memory mock, which would lose the key at every restart.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

/// What reading the key gave. See the module documentation for why the three cases are separate. `Debug` never prints the key.
#[derive(Clone, PartialEq, Eq)]
pub enum KeyRead {
    Found(Vec<u8>),
    Missing,
    /// The keychain could not be used; the text says why, for a person.
    Inaccessible(String),
}

impl std::fmt::Debug for KeyRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyRead::Found(k) => write!(f, "Found({} byte)", k.len()),
            KeyRead::Missing => write!(f, "Missing"),
            KeyRead::Inaccessible(why) => write!(f, "Inaccessible({why})"),
        }
    }
}

/// A place that keeps one small secret per account.
pub trait KeyStore: Debug + Send + Sync {
    /// Its name, for a person (`Portachiavi di macOS`).
    fn backend(&self) -> String;
    /// False for the store that stands for "this build or this process does not use a keychain": the state is then declared as
    /// such and no attempt is made.
    fn is_used(&self) -> bool {
        true
    }
    fn read(&self, account: &str) -> KeyRead;
    /// Stores `key`, replacing the item of the same account.
    fn write(&self, account: &str, key: &[u8]) -> Result<(), String>;
    /// Removes the item. An item that is not there is not an error.
    fn remove(&self, account: &str) -> Result<(), String>;
}

/// Stands for "no keychain here": mobile builds, the short-lived background worker, tests that do not exercise one.
#[derive(Debug, Clone)]
pub struct NoKeyStore {
    pub reason: String,
}

impl NoKeyStore {
    pub fn new(reason: impl Into<String>) -> Self {
        Self { reason: reason.into() }
    }
}

impl KeyStore for NoKeyStore {
    fn backend(&self) -> String {
        self.reason.clone()
    }
    fn is_used(&self) -> bool {
        false
    }
    fn read(&self, _: &str) -> KeyRead {
        KeyRead::Inaccessible(self.reason.clone())
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), String> {
        Err(self.reason.clone())
    }
    fn remove(&self, _: &str) -> Result<(), String> {
        Err(self.reason.clone())
    }
}

/// A keychain in memory, for tests (also of the other crates: `EngineOptions::secrets`). It can be told to fail like a real one.
#[derive(Default)]
pub struct MemoryKeyStore {
    items: Mutex<HashMap<String, Vec<u8>>>,
    /// While set, every call answers like a locked keychain.
    denied: Mutex<Option<String>>,
    /// While set, writes fail but reads work (a keychain that refuses new items).
    read_only: Mutex<bool>,
}

impl std::fmt::Debug for MemoryKeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryKeyStore({} items)", self.len())
    }
}

impl MemoryKeyStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    /// Makes the keychain unreachable (`Some(reason)`) or reachable again (`None`).
    pub fn deny(&self, reason: Option<&str>) {
        *self.denied.lock().unwrap() = reason.map(str::to_string);
    }
    pub fn set_read_only(&self, on: bool) {
        *self.read_only.lock().unwrap() = on;
    }
    /// Forgets every item, as a person deleting them in the keychain app would.
    pub fn wipe(&self) {
        self.items.lock().unwrap().clear();
    }
    pub fn len(&self) -> usize {
        self.items.lock().unwrap().len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The accounts that have an item.
    pub fn accounts(&self) -> Vec<String> {
        let mut v: Vec<String> = self.items.lock().unwrap().keys().cloned().collect();
        v.sort();
        v
    }
    pub fn get(&self, account: &str) -> Option<Vec<u8>> {
        self.items.lock().unwrap().get(account).cloned()
    }
}

impl KeyStore for MemoryKeyStore {
    fn backend(&self) -> String {
        "portachiavi di prova (in memoria)".into()
    }
    fn read(&self, account: &str) -> KeyRead {
        if let Some(why) = self.denied.lock().unwrap().clone() {
            return KeyRead::Inaccessible(why);
        }
        match self.items.lock().unwrap().get(account) {
            Some(k) => KeyRead::Found(k.clone()),
            None => KeyRead::Missing,
        }
    }
    fn write(&self, account: &str, key: &[u8]) -> Result<(), String> {
        if let Some(why) = self.denied.lock().unwrap().clone() {
            return Err(why);
        }
        if *self.read_only.lock().unwrap() {
            return Err("il portachiavi non accetta nuovi elementi".into());
        }
        self.items.lock().unwrap().insert(account.into(), key.to_vec());
        Ok(())
    }
    fn remove(&self, account: &str) -> Result<(), String> {
        if let Some(why) = self.denied.lock().unwrap().clone() {
            return Err(why);
        }
        self.items.lock().unwrap().remove(account);
        Ok(())
    }
}

/// Is there a keychain implementation for the platform this was built for?
pub const OS_KEYSTORE_BUILT: bool = cfg!(any(target_os = "macos", target_os = "windows", target_os = "linux"));

/// The keychain of the operating system, or [`NoKeyStore`] on a platform without one that was proven here.
pub fn os_keystore() -> Arc<dyn KeyStore> {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        Arc::new(os::OsKeyStore::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Arc::new(NoKeyStore::new("questa versione non usa il portachiavi del sistema su questa piattaforma"))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod os {
    use super::{KeyRead, KeyStore};
    use keyring_core::CredentialStore;
    use keyring_core::Error as KrError;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, OnceLock};
    use std::time::Duration;

    /// The name of the item in the keychain app.
    const SERVICE: &str = "AI Workspace Local - chiave dei segreti";
    /// How long a keychain call may take. A keychain waiting for a person (an unlock dialog nobody sees, a session without a
    /// screen) must not hold the start of the app: after this the call is given up on and the secrets are declared
    /// unreachable for this run. The call itself is left to finish on its thread.
    const PATIENCE: Duration = Duration::from_secs(10);

    #[cfg(target_os = "macos")]
    const BACKEND: &str = "Portachiavi di macOS (Keychain)";
    #[cfg(target_os = "windows")]
    const BACKEND: &str = "Gestione credenziali di Windows";
    #[cfg(target_os = "linux")]
    const BACKEND: &str = "Servizio dei segreti di Linux (Secret Service: GNOME Keyring, KWallet)";

    #[cfg(target_os = "macos")]
    fn build_store() -> keyring_core::Result<Arc<CredentialStore>> {
        let s: Arc<CredentialStore> = apple_native_keyring_store::keychain::Store::new()?;
        Ok(s)
    }
    #[cfg(target_os = "windows")]
    fn build_store() -> keyring_core::Result<Arc<CredentialStore>> {
        let s: Arc<CredentialStore> = windows_native_keyring_store::Store::new()?;
        Ok(s)
    }
    #[cfg(target_os = "linux")]
    fn build_store() -> keyring_core::Result<Arc<CredentialStore>> {
        let s: Arc<CredentialStore> = zbus_secret_service_keyring_store::Store::new()?;
        Ok(s)
    }

    /// Runs `f` on its own thread and waits at most [`PATIENCE`]. The calls into the platform libraries can block (a dialog, a
    /// D-Bus peer that does not answer) and some of them are happier off the async runtime's threads.
    fn within<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("aiw-keychain".into())
            .spawn(move || {
                let _ = tx.send(f());
            })
            .ok()?;
        rx.recv_timeout(PATIENCE).ok()
    }

    pub struct OsKeyStore {
        store: OnceLock<Result<Arc<CredentialStore>, String>>,
        /// A call timed out: no more calls this run (each would wait as long again).
        stuck: AtomicBool,
        service: String,
    }

    impl std::fmt::Debug for OsKeyStore {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "OsKeyStore({BACKEND}, service {})", self.service)
        }
    }

    impl Default for OsKeyStore {
        fn default() -> Self {
            Self::new()
        }
    }

    impl OsKeyStore {
        pub fn new() -> Self {
            Self::with_service(SERVICE)
        }
        /// For the tests that talk to the real keychain: their own service name, never the app's.
        pub fn with_service(service: &str) -> Self {
            Self { store: OnceLock::new(), stuck: AtomicBool::new(false), service: service.into() }
        }

        fn store(&self) -> Result<Arc<CredentialStore>, String> {
            if self.stuck.load(Ordering::Relaxed) {
                return Err("il portachiavi non ha risposto in tempo".into());
            }
            if let Some(s) = self.store.get() {
                return s.clone();
            }
            let built = match within(|| build_store().map_err(|e| describe(&e))) {
                Some(r) => r,
                None => {
                    self.stuck.store(true, Ordering::Relaxed);
                    return Err("il portachiavi non ha risposto in tempo".into());
                }
            };
            self.store.get_or_init(|| built).clone()
        }

        /// Runs one operation on one entry on a helper thread, with patience.
        fn on_entry<T: Send + 'static>(
            &self,
            account: &str,
            op: impl FnOnce(keyring_core::Entry) -> keyring_core::Result<T> + Send + 'static,
        ) -> Result<keyring_core::Result<T>, String> {
            let store = self.store()?;
            let (service, account) = (self.service.clone(), account.to_string());
            match within(move || {
                // Windows: the default persistence of a credential is "Enterprise" (it roams with a domain profile); the key belongs
                // to this computer, next to the database it opens, so it is stored as "Local". The other stores take no modifier.
                let local: std::collections::HashMap<&str, &str> = [("persistence", "Local")].into_iter().collect();
                let modifiers = cfg!(target_os = "windows").then_some(&local);
                store.build(&service, &account, modifiers).and_then(op)
            }) {
                Some(r) => Ok(r),
                None => {
                    self.stuck.store(true, Ordering::Relaxed);
                    Err("il portachiavi non ha risposto in tempo".into())
                }
            }
        }
    }

    /// An error of the keychain in words, without anything that could be the secret (the libraries never put one in).
    fn describe(e: &KrError) -> String {
        match e {
            KrError::NoStorageAccess(inner) => format!("accesso al portachiavi negato o portachiavi bloccato ({inner})"),
            KrError::PlatformFailure(inner) => format!("il portachiavi non risponde ({inner})"),
            other => other.to_string(),
        }
    }

    impl KeyStore for OsKeyStore {
        fn backend(&self) -> String {
            BACKEND.into()
        }
        fn read(&self, account: &str) -> KeyRead {
            match self.on_entry(account, |e| e.get_secret()) {
                Err(why) => KeyRead::Inaccessible(why),
                Ok(Ok(bytes)) => KeyRead::Found(bytes),
                Ok(Err(KrError::NoEntry)) => KeyRead::Missing,
                Ok(Err(e)) => KeyRead::Inaccessible(describe(&e)),
            }
        }
        fn write(&self, account: &str, key: &[u8]) -> Result<(), String> {
            let key = key.to_vec();
            match self.on_entry(account, move |e| e.set_secret(&key)) {
                Err(why) => Err(why),
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(describe(&e)),
            }
        }
        fn remove(&self, account: &str) -> Result<(), String> {
            match self.on_entry(account, |e| e.delete_credential()) {
                Err(why) => Err(why),
                Ok(Ok(())) | Ok(Err(KrError::NoEntry)) => Ok(()),
                Ok(Err(e)) => Err(describe(&e)),
            }
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub use os::OsKeyStore;
