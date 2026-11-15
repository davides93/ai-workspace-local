use super::*;
use aiw_core::config::{app_schema, Layer};
use serde_json::{json, Value};
use std::path::PathBuf;

// Recognisable fakes: nothing here is a real credential.
const HF: &str = "hf_TEST_ONLY_not_a_real_token_0001";
const MCP: &str = "Bearer TEST_ONLY_mcp_header_value_0002";
const IMAP: &str = "TEST_ONLY_imap_password_0003";
const BRAVE: &str = "BSA_TEST_ONLY_brave_key_0004";
const BOT: &str = "TEST_ONLY_bot_token_0005";
const OAUTH: &str = "TEST_ONLY_oauth_access_0006";
const ALL: [&str; 6] = [HF, MCP, IMAP, BRAVE, BOT, OAUTH];
/// Secret strings `write_secrets` stores: the six values, and the OAuth client id (a document made wholly of secrets seals all its strings).
const N: u64 = 7;

struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("aiw-home");
        std::fs::create_dir_all(&root).unwrap();
        Self { _dir: dir, root }
    }
    fn db(&self) -> PathBuf {
        self.root.join("aiw.db")
    }
    fn config_file(&self) -> PathBuf {
        self.root.join("config.json")
    }
    /// The store and the configuration as the engine opens them.
    fn open(&self) -> (Store, LayeredConfig) {
        let config = LayeredConfig::load(app_schema(), self.config_file(), self.root.join("device.json"), None);
        (Store::open(&self.db()).unwrap(), config)
    }
}

/// The documents the app stores, with a recognisable secret in each.
fn write_secrets(store: &Store, config: &LayeredConfig) {
    store.put_doc("model_provider", "hf", &json!({"id": "hf", "token": HF, "endpoint": null})).unwrap();
    store
        .put_doc(
            "mcp",
            "m1",
            &json!({"id": "m1", "name": "Server", "transport": "http", "url": "https://mcp.example.org/mcp", "headers": {"Authorization": MCP, "Accept": "json"}}),
        )
        .unwrap();
    store.put_doc("secret", "mail:acc1", &json!({"value": IMAP})).unwrap();
    store.put_doc("channel", "tg", &json!({"id": "tg", "kind": "telegram", "secrets": {"bot_token": BOT}})).unwrap();
    store.put_doc("mcp_oauth", "m1", &json!({"client_id": "cid", "access_token": OAUTH, "expires_at": 123})).unwrap();
    config.set(Layer::Global, "search.brave_api_key", json!(BRAVE)).unwrap();
}

/// Everything on disk that could hold a plain value: the database, its log and shared memory, the configuration files.
fn disk_text(home: &Home) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![];
    for name in ["aiw.db", "aiw.db-wal", "aiw.db-shm", "aiw.db-journal", "config.json", "device.json"] {
        if let Ok(b) = std::fs::read(home.root.join(name)) {
            out.push((name.to_string(), b));
        }
    }
    out
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn assert_no_plain_on_disk(home: &Home) {
    for (name, bytes) in disk_text(home) {
        for secret in ALL {
            assert!(!contains(&bytes, secret), "{secret} is in plain in {name}");
        }
    }
}

fn assert_plain_on_disk(home: &Home, secret: &str) {
    assert!(disk_text(home).iter().any(|(_, b)| contains(b, secret)), "{secret} should be stored plain here");
}

fn raw_doc(home: &Home, kind: &str, id: &str) -> String {
    let c = rusqlite::Connection::open(home.db()).unwrap();
    c.query_row("SELECT json FROM documents WHERE kind=?1 AND id=?2", [kind, id], |r| r.get(0)).unwrap()
}

fn started(home: &Home, ks: &Arc<MemoryKeyStore>) -> (Store, LayeredConfig, Arc<Vault>) {
    let (store, config) = home.open();
    let vault = start(&store, &config, &Keychain::Custom(ks.clone()), true);
    (store, config, vault)
}

fn read_back(store: &Store, config: &LayeredConfig) {
    assert_eq!(store.get_doc::<Value>("model_provider", "hf").unwrap().unwrap()["token"], HF);
    assert_eq!(store.get_doc::<Value>("mcp", "m1").unwrap().unwrap()["headers"]["Authorization"], MCP);
    assert_eq!(store.get_doc::<Value>("secret", "mail:acc1").unwrap().unwrap()["value"], IMAP);
    assert_eq!(store.list_docs::<Value>("channel").unwrap()[0]["secrets"]["bot_token"], BOT);
    assert_eq!(store.get_doc::<Value>("mcp_oauth", "m1").unwrap().unwrap()["access_token"], OAUTH);
    assert_eq!(config.get_str("search.brave_api_key").as_deref(), Some(BRAVE));
}

// ---------------------------------------------------------------------------------------------------- the main path

#[test]
fn a_secret_written_with_a_keychain_is_sealed_on_disk_and_reads_back_plain() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::Active);
    assert_eq!(ks.len(), 1, "one item in the keychain, and it is the only thing there");
    write_secrets(&store, &config);

    read_back(&store, &config);
    // The stored form is sealed where the secret was, and the rest of the document is untouched.
    let mcp: Value = serde_json::from_str(&raw_doc(&home, "mcp", "m1")).unwrap();
    assert!(mcp["headers"]["Authorization"].as_str().unwrap().starts_with("aiwenc1:"));
    assert_eq!((mcp["headers"]["Accept"].as_str(), mcp["url"].as_str()), (Some("json"), Some("https://mcp.example.org/mcp")));
    drop(store);
    assert_no_plain_on_disk(&home);
    // Only the key's identity is in the database; the key is not.
    let row = raw_doc(&home, "vault", "main");
    let key_hex: String = ks.get(&ks.accounts()[0]).unwrap().iter().map(|b| format!("{b:02x}")).collect();
    assert!(!row.contains(&key_hex));
}

#[test]
fn the_key_survives_a_restart_and_the_secrets_still_read() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    {
        let (store, config, _) = started(&home, &ks);
        write_secrets(&store, &config);
    }
    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::Active);
    assert_eq!(ks.len(), 1, "the second start reuses the item");
    read_back(&store, &config);
    let st = vault.status(&store, &config);
    assert_eq!((st.state.as_str(), st.encrypted, st.plain, st.unreadable), ("active", N, 0, 0), "{st:?}");
}

#[test]
fn a_secret_saved_again_is_sealed_again_with_a_fresh_nonce() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    let (store, _, _) = started(&home, &ks);
    store.put_doc("secret", "k", &json!({"value": IMAP})).unwrap();
    let first = raw_doc(&home, "secret", "k");
    store.put_doc("secret", "k", &json!({"value": IMAP})).unwrap();
    assert_ne!(first, raw_doc(&home, "secret", "k"));
    assert_eq!(store.get_doc::<Value>("secret", "k").unwrap().unwrap()["value"], IMAP);
}

#[test]
fn empty_values_and_documents_without_secrets_are_left_as_they_are() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    let (store, _, _) = started(&home, &ks);
    store.put_doc("model_provider", "empty", &json!({"id": "empty", "token": ""})).unwrap();
    store.put_doc("agent", "a", &json!({"id": "a", "token": "not a secret kind"})).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&raw_doc(&home, "model_provider", "empty")).unwrap()["token"], "");
    assert_eq!(serde_json::from_str::<Value>(&raw_doc(&home, "agent", "a")).unwrap()["token"], "not a secret kind");
}

// ---------------------------------------------------------------------------------------------------- migration

#[test]
fn plain_secrets_from_before_are_sealed_at_the_first_start_with_a_keychain_and_nothing_stays_behind() {
    let home = Home::new();
    // The way every install has been until now: no keychain, plain values in the database and in the configuration file.
    {
        let (store, config) = home.open();
        let vault = start(&store, &config, &Keychain::None, true);
        assert_eq!(vault.phase().code(), "not_used");
        write_secrets(&store, &config);
        let st = vault.status(&store, &config);
        assert_eq!((st.encrypted, st.plain), (0, N), "{st:?}");
    }
    assert_plain_on_disk(&home, HF);
    assert_plain_on_disk(&home, BRAVE);

    // The update arrives: a keychain exists now.
    let ks = MemoryKeyStore::new();
    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::Active);
    read_back(&store, &config); // no data lost
    let st = vault.status(&store, &config);
    assert_eq!((st.encrypted, st.plain, st.unreadable), (N, 0, 0), "{st:?}");
    // Looked at while the engine still has the database open (so the `-wal` is whatever the migration left, not what closing it
    // would clean up), and again once it is closed.
    assert_no_plain_on_disk(&home);
    drop(store);
    assert_no_plain_on_disk(&home); // not in the database, not in its log, not in a freed page, not in the settings file

    // Repeatable: a third start changes nothing.
    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.migrate(&store, &config), Migration::default());
    read_back(&store, &config);
}

#[test]
fn switching_the_setting_off_brings_every_secret_back_in_plain_and_removes_the_key() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    {
        let (store, config, _) = started(&home, &ks);
        write_secrets(&store, &config);
    }
    assert_eq!(ks.len(), 1);
    // The person turns it off (the setting is read at the next start).
    {
        let config = LayeredConfig::load(app_schema(), home.config_file(), home.root.join("device.json"), None);
        config.set(Layer::Global, "security.secrets_at_rest", json!("off")).unwrap();
    }
    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::Off);
    read_back(&store, &config);
    let st = vault.status(&store, &config);
    assert_eq!((st.state.as_str(), st.encrypted, st.plain, st.unreadable), ("off", 0, N, 0), "{st:?}");
    assert!(ks.is_empty(), "nothing is sealed any more: the item is removed");
    assert!(store.get_doc::<Value>("vault", "main").unwrap().is_none());
    // New secrets are plain while it is off.
    store.put_doc("secret", "new", &json!({"value": IMAP})).unwrap();
    assert!(raw_doc(&home, "secret", "new").contains(IMAP));
    drop(store);
    assert_plain_on_disk(&home, HF);

    // And back on: sealed again with a new key.
    let config = LayeredConfig::load(app_schema(), home.config_file(), home.root.join("device.json"), None);
    config.set(Layer::Global, "security.secrets_at_rest", json!("auto")).unwrap();
    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::Active);
    read_back(&store, &config);
    drop(store);
    assert_no_plain_on_disk(&home);
}

// ---------------------------------------------------------------------------------------------------- no keychain

#[test]
fn without_a_keychain_secrets_stay_plain_the_state_says_so_and_nothing_fails() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    ks.deny(Some("nessun servizio dei segreti su questo computer"));
    let (store, config, vault) = started(&home, &ks);
    assert!(matches!(vault.phase(), Phase::Unavailable(ref why) if why.contains("nessun servizio dei segreti su questo computer")));
    write_secrets(&store, &config); // writes work
    read_back(&store, &config); // and read
    let st = vault.status(&store, &config);
    assert_eq!(st.state, "unavailable");
    assert!(st.detail.as_deref().is_some_and(|d| d.contains("nessun servizio dei segreti su questo computer")));
    assert_eq!((st.encrypted, st.plain), (0, N));
    assert!(ks.is_empty(), "no key is made because the keychain cannot be reached");
    assert!(store.get_doc::<Value>("vault", "main").unwrap().is_none());

    // The keychain comes back: the next start seals what was written meanwhile.
    ks.deny(None);
    drop(store);
    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::Active);
    read_back(&store, &config);
    drop(store);
    assert_no_plain_on_disk(&home);
}

#[test]
fn a_keychain_that_refuses_the_key_or_loses_it_is_not_used() {
    // It refuses the new item.
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    ks.set_read_only(true);
    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase().code(), "unavailable");
    store.put_doc("secret", "k", &json!({"value": IMAP})).unwrap();
    assert!(raw_doc(&home, "secret", "k").contains(IMAP), "plain, as before");
    drop((store, config));

    // It accepts the item and then does not give it back: the secrets would be lost with the next start, so nothing is sealed.
    #[derive(Debug)]
    struct Amnesiac(MemoryKeyStore);
    impl KeyStore for Amnesiac {
        fn backend(&self) -> String {
            "smemorato".into()
        }
        fn read(&self, _: &str) -> KeyRead {
            KeyRead::Missing
        }
        fn write(&self, a: &str, k: &[u8]) -> Result<(), String> {
            self.0.write(a, k)
        }
        fn remove(&self, a: &str) -> Result<(), String> {
            self.0.remove(a)
        }
    }
    let home = Home::new();
    let (store, config) = home.open();
    let ks = Arc::new(Amnesiac(MemoryKeyStore::default()));
    let vault = start(&store, &config, &Keychain::Custom(ks.clone()), true);
    assert_eq!(vault.phase().code(), "unavailable");
    assert!(ks.0.is_empty(), "the item that could not be verified is removed again");
    store.put_doc("secret", "k", &json!({"value": IMAP})).unwrap();
    assert!(raw_doc(&home, "secret", "k").contains(IMAP));
}

#[test]
fn a_build_without_a_keychain_says_not_used_and_keeps_secrets_plain() {
    let home = Home::new();
    let (store, config) = home.open();
    let vault = start(&store, &config, &Keychain::None, true);
    let st = vault.status(&store, &config);
    assert_eq!(st.state, "not_used");
    assert!(st.backend.is_none() && st.detail.is_some());
    write_secrets(&store, &config);
    read_back(&store, &config);
    assert_plain_on_disk(&home, HF);
}

// ---------------------------------------------------------------------------------------------------- the key is gone

#[test]
fn a_lost_key_is_declared_nothing_is_deleted_and_nothing_is_made_over_it() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    {
        let (store, config, _) = started(&home, &ks);
        write_secrets(&store, &config);
    }
    let sealed_before = raw_doc(&home, "model_provider", "hf");
    ks.wipe(); // the person deleted the item, or the data folder came from another computer

    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::KeyLost);
    assert!(ks.is_empty(), "no new key over a vault that has sealed values");
    let st = vault.status(&store, &config);
    assert_eq!((st.state.as_str(), st.encrypted, st.plain, st.unreadable), ("key_lost", 0, 0, N), "{st:?}");
    // The sealed string is what a read gives, and the application reads the configuration secret as not set.
    assert_eq!(store.get_doc::<Value>("model_provider", "hf").unwrap().unwrap()["token"], sealed_before_token(&sealed_before));
    assert_eq!(config.get_str("search.brave_api_key"), None);
    // Saving the document (the editor changes the name, keeps the secret) must not destroy the sealed value.
    let mut doc: Value = store.get_doc("model_provider", "hf").unwrap().unwrap();
    doc["endpoint"] = json!("https://hf.example.org");
    store.put_doc("model_provider", "hf", &doc).unwrap();
    let after: Value = serde_json::from_str(&raw_doc(&home, "model_provider", "hf")).unwrap();
    assert_eq!(after["token"], sealed_before_token(&sealed_before));
    // Saving an unrelated setting keeps the sealed secret in the file too.
    config.set(Layer::Global, "ui.theme", json!("dark")).unwrap();
    let file: Value = serde_json::from_str(&std::fs::read_to_string(home.config_file()).unwrap()).unwrap();
    assert!(file["search.brave_api_key"].as_str().unwrap().starts_with("aiwenc1:"));
    assert_eq!(file["ui.theme"], "dark");
}

fn sealed_before_token(raw: &str) -> String {
    serde_json::from_str::<Value>(raw).unwrap()["token"].as_str().unwrap().to_string()
}

#[test]
fn forgetting_the_unreadable_clears_them_and_makes_a_new_key_so_the_secrets_can_be_entered_again() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    {
        let (store, config, _) = started(&home, &ks);
        write_secrets(&store, &config);
    }
    ks.wipe();
    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::KeyLost);

    let cleared = vault.forget_unreadable(&store, &config, true).unwrap();
    assert_eq!(cleared as u64, N);
    assert_eq!(vault.phase(), Phase::Active, "nothing sealed is left, so a new key is made");
    assert_eq!(ks.len(), 1);
    assert_eq!(store.get_doc::<Value>("model_provider", "hf").unwrap().unwrap()["token"], "");
    assert_eq!(config.get_str("search.brave_api_key"), None);
    // The person enters them again: sealed with the new key.
    write_secrets(&store, &config);
    read_back(&store, &config);
    drop(store);
    assert_no_plain_on_disk(&home);
}

#[test]
fn a_keychain_that_is_locked_or_denied_after_sealing_never_gets_a_new_key() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    {
        let (store, config, _) = started(&home, &ks);
        write_secrets(&store, &config);
    }
    let account = ks.accounts()[0].clone();
    let key_before = ks.get(&account).unwrap();
    ks.deny(Some("il portachiavi è bloccato"));

    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::Unavailable("il portachiavi è bloccato".into()));
    let st = vault.status(&store, &config);
    assert_eq!((st.state.as_str(), st.unreadable), ("unavailable", N), "{st:?}");
    // Nothing is deleted, a write of a new secret is plain and declared, and saving a document keeps the sealed ones.
    store.put_doc("secret", "extra", &json!({"value": IMAP})).unwrap();
    let mut doc: Value = store.get_doc("model_provider", "hf").unwrap().unwrap();
    doc["endpoint"] = json!("https://hf.example.org");
    store.put_doc("model_provider", "hf", &doc).unwrap();
    drop(store);

    // Unlocked again: everything reads, and the new plain secret is sealed by the migration.
    ks.deny(None);
    assert_eq!(ks.get(&account).unwrap(), key_before, "the key was never touched");
    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::Active);
    read_back(&store, &config);
    assert_eq!(store.get_doc::<Value>("secret", "extra").unwrap().unwrap()["value"], IMAP);
    let st = vault.status(&store, &config);
    assert_eq!((st.plain, st.unreadable), (0, 0), "{st:?}");
}

#[test]
fn a_data_folder_copied_without_secrets_gets_a_new_key_without_complaint() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    {
        let (store, config, _) = started(&home, &ks);
        store.put_doc("agent", "a", &json!({"id": "a"})).unwrap();
        drop(config);
    }
    // Another computer: same database (with the vault row), an empty keychain, nothing sealed.
    let other = MemoryKeyStore::new();
    let (store, config, vault) = started(&home, &other);
    assert_eq!(vault.phase(), Phase::Active);
    assert_eq!(other.len(), 1);
    write_secrets(&store, &config);
    read_back(&store, &config);
}

// ---------------------------------------------------------------------------------------------------- review: data-loss guards

#[test]
fn forgetting_the_unreadable_refuses_when_the_keychain_is_only_unreachable() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    {
        let (store, config, _) = started(&home, &ks);
        write_secrets(&store, &config);
    }
    ks.deny(Some("il portachiavi è bloccato"));
    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase().code(), "unavailable");
    // Every sealed value looks unreadable now, and all of them are perfectly good: nothing may be cleared.
    let err = vault.forget_unreadable(&store, &config, true).unwrap_err();
    assert!(err.contains("non tolgo nulla"), "{err}");
    drop((store, config));
    ks.deny(None);
    let (store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::Active);
    read_back(&store, &config);

    // And without a keychain at all there is nothing sealed to forget.
    let plain = Home::new();
    let (store, config) = plain.open();
    let vault = start(&store, &config, &Keychain::None, true);
    assert!(vault.forget_unreadable(&store, &config, true).is_err());
}

#[test]
fn a_value_sealed_by_a_newer_format_is_never_cleared_by_forget_unreadable() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    let (store, config, vault) = started(&home, &ks);
    store.put_doc("secret", "future", &json!({"value": "aiwenc2:ffffffff:AAAAAAAA"})).unwrap();
    assert_eq!(vault.forget_unreadable(&store, &config, true), Ok(0));
    assert_eq!(store.get_doc::<Value>("secret", "future").unwrap().unwrap()["value"], "aiwenc2:ffffffff:AAAAAAAA");
}

#[test]
fn switching_off_with_a_settings_file_that_cannot_be_read_keeps_the_key() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    {
        let (store, config, _) = started(&home, &ks);
        write_secrets(&store, &config);
    }
    let config = LayeredConfig::load(app_schema(), home.config_file(), home.root.join("device.json"), None);
    config.set(Layer::Global, "security.secrets_at_rest", json!("off")).unwrap();
    // A device settings file nobody can parse may hold a sealed secret: the key must not go while that is possible.
    std::fs::write(home.root.join("device.json"), "{ rotto").unwrap();
    let (store, _config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::Off);
    assert_eq!(ks.len(), 1, "the key stays: something unreadable may still need it");
    assert!(store.get_doc::<Value>("vault", "main").unwrap().is_some());
    assert_eq!(store.get_doc::<Value>("model_provider", "hf").unwrap().unwrap()["token"], HF, "what could be unsealed was");
}

#[test]
fn a_database_without_a_vault_row_next_to_sealed_settings_is_declared_not_given_a_new_key() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    {
        let (store, config, _) = started(&home, &ks);
        write_secrets(&store, &config);
    }
    // A restore replaced the database with one that has no vault row; the settings file kept its sealed key.
    for f in ["aiw.db", "aiw.db-wal", "aiw.db-shm"] {
        let _ = std::fs::remove_file(home.root.join(f));
    }
    let before = ks.accounts();
    let (_store, config, vault) = started(&home, &ks);
    assert_eq!(vault.phase(), Phase::KeyLost);
    assert_eq!(ks.accounts(), before, "no new key");
    assert_eq!(config.get_str("search.brave_api_key"), None);
}

#[test]
fn a_one_shot_command_reads_the_key_and_changes_nothing_about_the_vault() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    {
        let (store, config, _) = started(&home, &ks);
        write_secrets(&store, &config);
    }
    // The setting says off (the app has not restarted yet); a command run meanwhile must not unseal anything or remove the key.
    let config = LayeredConfig::load(app_schema(), home.config_file(), home.root.join("device.json"), None);
    config.set(Layer::Global, "security.secrets_at_rest", json!("off")).unwrap();
    let (store, config) = home.open();
    let vault = start(&store, &config, &Keychain::Custom(ks.clone()), false);
    assert_eq!(vault.phase(), Phase::Off);
    read_back(&store, &config); // it opens what is sealed
    assert_eq!(ks.len(), 1);
    assert!(store.get_doc::<Value>("vault", "main").unwrap().is_some());
    assert!(raw_doc(&home, "model_provider", "hf").contains("aiwenc1:"), "still sealed on disk");
    drop((store, config));

    // On a data folder that never had a vault it makes none and says who does.
    let fresh = Home::new();
    let empty = MemoryKeyStore::new();
    let (store, config) = fresh.open();
    let vault = start(&store, &config, &Keychain::Custom(empty.clone()), false);
    assert_eq!(vault.phase().code(), "not_used");
    assert!(empty.is_empty());
    assert!(store.get_doc::<Value>("vault", "main").unwrap().is_none());
}

#[test]
fn earlier_plain_versions_of_a_value_do_not_stay_behind_in_free_pages() {
    const OLD: &str = "TEST_ONLY_old_rotated_token_";
    const GONE: &str = "TEST_ONLY_deleted_account_pw_";
    let home = Home::new();
    {
        // An install from before: no keychain, a token rotated twice, an account created and deleted.
        let (store, config) = home.open();
        let _ = start(&store, &config, &Keychain::None, true);
        store.put_doc("secret", "k", &json!({"value": format!("{OLD}{}", "a".repeat(3000))})).unwrap();
        store.put_doc("secret", "k", &json!({"value": format!("{OLD}{}", "b".repeat(3000))})).unwrap();
        store.put_doc("secret", "gone", &json!({"value": format!("{GONE}{}", "c".repeat(3000))})).unwrap();
        store.delete_doc("secret", "gone").unwrap();
        store.put_doc("secret", "k", &json!({"value": IMAP})).unwrap();
    }
    let on_disk = |needle: &str| disk_text(&home).iter().any(|(_, b)| contains(b, needle));
    assert!(on_disk(OLD) && on_disk(GONE), "the setup must leave the old versions in the file, or this test proves nothing");

    let ks = MemoryKeyStore::new();
    let (store, _config, _vault) = started(&home, &ks);
    // While the database is open, and after it is closed.
    assert!(!on_disk(OLD) && !on_disk(GONE) && !on_disk(IMAP), "an earlier version of a secret is still in the file");
    drop(store);
    assert!(!on_disk(OLD) && !on_disk(GONE) && !on_disk(IMAP));
}

#[test]
fn debug_never_prints_a_key() {
    let shown = format!("{:?} {:?}", KeyRead::Found(vec![7; 32]), MemoryKeyStore::new());
    assert!(!shown.contains("7, 7") && shown.contains("32 byte"), "{shown}");
    let ks = MemoryKeyStore::new();
    ks.write("a", &[9; 32]).unwrap();
    assert!(!format!("{ks:?}").contains("9, 9"));
}

// ---------------------------------------------------------------------------------------------------- two processes

#[test]
fn two_starts_at_once_end_up_with_one_key_and_one_vault() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    let (store_a, config_a) = home.open();
    let (store_b, config_b) = home.open();
    // Both decided there was no vault, both made a key: only one claims the row.
    let a = Vault::new(ks.clone());
    let b = Vault::new(ks.clone());
    a.create(&store_a);
    b.create(&store_b);
    assert_eq!((a.phase(), b.phase()), (Phase::Active, Phase::Active));
    assert_eq!(ks.len(), 1, "the loser removed its item");
    // They use the same key: what one seals the other opens.
    let sealed = a.seal(HF).unwrap();
    assert_eq!(b.open(&sealed), SecretRead::Opened(HF.into()));
    drop((config_a, config_b));
}

// ---------------------------------------------------------------------------------------------------- the settings file

#[test]
fn a_secret_setting_is_sealed_in_the_file_and_a_plain_file_is_migrated() {
    let home = Home::new();
    // A plain file, as every install has it.
    std::fs::write(home.config_file(), serde_json::to_vec_pretty(&json!({"ui.theme": "dark", "search.brave_api_key": BRAVE})).unwrap())
        .unwrap();
    let ks = MemoryKeyStore::new();
    let (_store, config, vault) = started(&home, &ks);
    assert_eq!(config.get_str("search.brave_api_key").as_deref(), Some(BRAVE));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(home.config_file()).unwrap()).unwrap();
    assert!(file["search.brave_api_key"].as_str().unwrap().starts_with("aiwenc1:"));
    assert_eq!(file["ui.theme"], "dark", "the rest of the file is untouched");
    assert!(!std::fs::read_to_string(home.config_file()).unwrap().contains(BRAVE));
    // The view the API gives still masks it, and a new value is sealed too.
    assert_eq!(config.view().effective["search.brave_api_key"], "••••••");
    config.set(Layer::Global, "search.brave_api_key", json!("BSA_TEST_ONLY_second_key_0007")).unwrap();
    assert!(!std::fs::read_to_string(home.config_file()).unwrap().contains("BSA_TEST_ONLY_second_key_0007"));
    assert_eq!(config.get_str("search.brave_api_key").as_deref(), Some("BSA_TEST_ONLY_second_key_0007"));
    // A file that cannot be parsed is never rewritten by the migration.
    std::fs::write(home.root.join("device.json"), "{ rotto").unwrap();
    assert_eq!(vault.migrate(&_store, &config), Migration::default());
    assert_eq!(std::fs::read_to_string(home.root.join("device.json")).unwrap(), "{ rotto");
}

// ---------------------------------------------------------------------------------------------------- the table and the codec

#[test]
fn every_secret_setting_and_runtime_setting_looks_like_a_secret_to_the_table() {
    // `is_secret_name` decides which strings of a mixed document are sealed: a runtime setting the manifests mark as secret
    // under a name it does not match would be stored plain. This fails the day one is added.
    let manifests: Value = serde_json::from_str(include_str!("../../../aiw-runtimes/manifests/builtin.json")).unwrap();
    let mut checked = 0;
    fn walk(v: &Value, name: Option<&str>, checked: &mut usize) {
        match v {
            Value::Object(m) => {
                if m.get("secret") == Some(&json!(true)) {
                    let n = name.unwrap_or_default();
                    assert!(
                        crate::secrets::is_secret_name(n),
                        "the secret setting `{n}` is not recognised by name: add it to the secret words"
                    );
                    *checked += 1;
                }
                for (k, x) in m {
                    walk(x, Some(k), checked);
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, name, checked)),
            _ => {}
        }
    }
    walk(&manifests, None, &mut checked);
    assert!(checked >= 3, "found {checked} secret runtime settings");
}

#[test]
fn the_codec_never_panics_on_what_it_cannot_open() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    let (_s, _c, vault) = started(&home, &ks);
    for junk in [
        "aiwenc1:",
        "aiwenc1::",
        "aiwenc1:zz:@@@",
        "aiwenc9:aaaa:AAAA",
        "aiwenc1:00000000:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    ] {
        assert_eq!(vault.open(junk), SecretRead::Unreadable, "{junk}");
    }
    assert_eq!(vault.open("plain token"), SecretRead::Plain);
    assert_eq!(vault.open(""), SecretRead::Plain);
}

#[test]
fn a_plain_value_that_happens_to_look_like_an_envelope_is_kept_as_it_is() {
    // A person types a value that starts like ours (never a real one). It is stored as typed and read as typed.
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    let (store, _, _) = started(&home, &ks);
    let odd = "aiwenc1:deadbeef:not-really-sealed";
    store.put_doc("secret", "odd", &json!({"value": odd})).unwrap();
    assert_eq!(store.get_doc::<Value>("secret", "odd").unwrap().unwrap()["value"], odd);
}

#[test]
fn the_reader_of_the_command_line_opens_what_the_engine_sealed_and_changes_nothing() {
    let home = Home::new();
    let ks = MemoryKeyStore::new();
    {
        let (store, config, _) = started(&home, &ks);
        write_secrets(&store, &config);
    }
    let dirs = DataDirs::at(home.root.clone());
    let reader = Vault::reader(&dirs, ks.clone());
    assert_eq!(reader.phase(), Phase::Active);
    let sealed: Value = serde_json::from_str(&raw_doc(&home, "model_provider", "hf")).unwrap();
    assert_eq!(reader.open(sealed["token"].as_str().unwrap()), SecretRead::Opened(HF.into()));
    // Without the key it opens nothing and creates nothing.
    let none = Vault::reader(&dirs, MemoryKeyStore::new());
    assert_eq!(none.phase(), Phase::KeyLost);
    assert_eq!(none.open(sealed["token"].as_str().unwrap()), SecretRead::Unreadable);
    // A data folder that never had a vault: nothing to open.
    let fresh = Home::new();
    assert_eq!(Vault::reader(&DataDirs::at(fresh.root.clone()), MemoryKeyStore::new()).phase(), Phase::Off);
}

// ---------------------------------------------------------------------------------------------------- backup

mod backup_with_sealed_secrets {
    use super::*;
    use crate::backup::{self, BackupOptions};

    fn sealed_home() -> (Home, Arc<MemoryKeyStore>) {
        let home = Home::new();
        let ks = MemoryKeyStore::new();
        {
            let (store, config, _) = started(&home, &ks);
            write_secrets(&store, &config);
        }
        (home, ks)
    }

    /// Where a restore puts the archive: a data folder of another computer.
    fn restore_elsewhere(archive: &std::path::Path) -> Home {
        let other = Home::new();
        backup::restore(&DataDirs::at(other.root.clone()), archive).unwrap();
        other
    }

    #[test]
    fn the_default_backup_holds_no_secret_plain_or_sealed_and_no_trace_of_the_vault() {
        let (home, _ks) = sealed_home();
        let out = home.root.parent().unwrap().join("default.tar.gz");
        let m = backup::create(&DataDirs::at(home.root.clone()), &BackupOptions::default(), &out).unwrap();
        assert!(!m.sensitive);
        let other = restore_elsewhere(&out);
        let bytes = disk_text(&other);
        for (name, b) in &bytes {
            for s in ALL {
                assert!(!contains(b, s), "{s} in the restored {name}");
            }
            assert!(!contains(b, "aiwenc1:"), "a sealed value travelled in {name}");
        }
        let c = rusqlite::Connection::open(other.db()).unwrap();
        assert_eq!(c.query_row("SELECT COUNT(*) FROM documents WHERE kind='vault'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        // The configuration that is not secret stays.
        let model: Value = serde_json::from_str(&raw_doc(&other, "model_provider", "hf")).unwrap();
        assert_eq!(model["token"], "");
        assert_eq!(model["id"], "hf");
    }

    #[test]
    fn a_backup_with_secrets_is_portable_it_holds_them_plain_and_the_new_computer_seals_them() {
        let (home, ks) = sealed_home();
        let out = home.root.parent().unwrap().join("secrets.tar.gz");
        let codec: Arc<dyn SecretCodec> = Vault::reader(&DataDirs::at(home.root.clone()), ks.clone());
        let m =
            backup::create(&DataDirs::at(home.root.clone()), &BackupOptions { include_secrets: true, codec: Some(codec) }, &out).unwrap();
        assert!(m.sensitive);
        assert!(m.notes.iter().any(|n| n.contains("in chiaro")), "{:?}", m.notes);
        assert!(m.skipped.is_empty(), "{:?}", m.skipped);

        // On another computer (another keychain): what comes out of the archive is plain and complete...
        let other = restore_elsewhere(&out);
        for s in ALL {
            assert_plain_on_disk(&other, s);
        }
        let c = rusqlite::Connection::open(other.db()).unwrap();
        assert_eq!(c.query_row("SELECT COUNT(*) FROM documents WHERE kind='vault'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        drop(c);
        // ...and the first start there seals it with that computer's key, losing nothing.
        let ks2 = MemoryKeyStore::new();
        let (store, config, vault) = started(&other, &ks2);
        assert_eq!(vault.phase(), Phase::Active);
        assert_ne!(ks.accounts(), ks2.accounts());
        read_back(&store, &config);
        drop(store);
        assert_no_plain_on_disk(&other);
    }

    #[test]
    fn a_backup_with_secrets_whose_key_is_gone_says_which_could_not_be_saved() {
        let (home, ks) = sealed_home();
        ks.wipe();
        let out = home.root.parent().unwrap().join("lost.tar.gz");
        let codec: Arc<dyn SecretCodec> = Vault::reader(&DataDirs::at(home.root.clone()), ks.clone());
        let m =
            backup::create(&DataDirs::at(home.root.clone()), &BackupOptions { include_secrets: true, codec: Some(codec) }, &out).unwrap();
        assert!(m.skipped.iter().any(|s| s.contains(&format!("{N} segreti cifrati"))), "{:?}", m.skipped);
        let other = restore_elsewhere(&out);
        for (name, b) in disk_text(&other) {
            assert!(!contains(&b, "aiwenc1:"), "a sealed value that nobody can open travelled in {name}");
        }
    }

    #[test]
    fn the_safety_copy_before_a_restore_keeps_the_secrets_sealed_so_it_is_never_plain_on_disk() {
        let (home, ks) = sealed_home();
        let dirs = DataDirs::at(home.root.clone());
        // What `restore` saves first: everything as stored.
        let out = home.root.parent().unwrap().join("safety.tar.gz");
        backup::create(&dirs, &BackupOptions { include_secrets: true, codec: None }, &out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        // The archive is gzip: read it back through a restore on the same computer and look at what it holds.
        let again = restore_elsewhere(&out);
        for (name, b) in disk_text(&again) {
            for s in ALL {
                assert!(!contains(&b, s), "{s} is plain in the safety copy ({name})");
            }
        }
        // With the same key it still opens: the vault row travelled with it.
        let (store, config, vault) = started(&again, &ks);
        assert_eq!(vault.phase(), Phase::Active);
        read_back(&store, &config);
        assert!(!bytes.is_empty());
    }
}

// ---------------------------------------------------------------------------------------------------- the real keychain

/// These talk to the keychain of the machine they run on (macOS Keychain, Windows Credential Manager, the Secret Service of a
/// Linux session), with a service name of their own that is never the app's, and remove what they made. They are `#[ignore]`
/// because a headless runner has none: run them on purpose with
/// `cargo test -p aiw-engine --lib vault::tests::real_keychain -- --ignored --test-threads=1` (on Linux with a Secret Service session, `real_keychain::the_`; the test that expects no service is `real_keychain::without_a_secret_service`).
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod real_keychain {
    use super::*;
    use crate::vault::keystore::OsKeyStore;

    const SERVICE: &str = "AIW prova automatica - chiave dei segreti (si può cancellare)";

    /// Removes the item a test made, even when an assertion fails.
    struct Cleanup(Arc<OsKeyStore>, String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.remove(&self.1);
        }
    }

    #[test]
    #[ignore = "uses the real system keychain of this machine"]
    fn the_system_keychain_stores_reads_and_removes_an_item() {
        let ks = Arc::new(OsKeyStore::with_service(SERVICE));
        println!("keychain: {}", ks.backend());
        let account = format!("test-{}", super::super::hex(&envelope::random_bytes(8).unwrap()));
        let _cleanup = Cleanup(ks.clone(), account.clone());
        assert_eq!(ks.read(&account), KeyRead::Missing, "an item that was never written is missing, not an error");
        let key = envelope::random_bytes(envelope::KEY_LEN).unwrap();
        ks.write(&account, &key).unwrap();
        assert_eq!(ks.read(&account), KeyRead::Found(key.clone()));
        let other = envelope::random_bytes(envelope::KEY_LEN).unwrap();
        ks.write(&account, &other).unwrap();
        assert_eq!(ks.read(&account), KeyRead::Found(other), "writing again replaces the item");
        ks.remove(&account).unwrap();
        assert_eq!(ks.read(&account), KeyRead::Missing);
        ks.remove(&account).unwrap(); // removing what is not there is not an error
    }

    /// The other half of "Linux with and without a Secret Service": in a session that has none (a bare container, an SSH login,
    /// a headless server) the keychain is reported as unavailable, the secrets stay plain and everything keeps working.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs a Linux session WITHOUT a Secret Service on the session bus (a bare container)"]
    fn without_a_secret_service_the_state_is_declared_and_nothing_fails() {
        let ks = Arc::new(OsKeyStore::with_service(SERVICE));
        match ks.read("account") {
            KeyRead::Inaccessible(why) => println!("declared: {why}"),
            other => panic!("expected an unreachable keychain, got {other:?}"),
        }
        assert!(ks.write("account", &[1; 32]).is_err());
        let home = Home::new();
        let (store, config) = home.open();
        let vault = start(&store, &config, &Keychain::Custom(ks), true);
        let Phase::Unavailable(why) = vault.phase() else { panic!("{:?}", vault.phase()) };
        assert!(!why.is_empty());
        write_secrets(&store, &config);
        read_back(&store, &config);
        let st = vault.status(&store, &config);
        assert_eq!((st.state.as_str(), st.encrypted, st.plain), ("unavailable", 0, N), "{st:?}");
        drop(store);
        assert_plain_on_disk(&home, HF);
    }

    #[test]
    #[ignore = "uses the real system keychain of this machine"]
    fn the_whole_life_of_the_vault_on_the_system_keychain() {
        let home = Home::new();
        let ks = Arc::new(OsKeyStore::with_service(SERVICE));
        let (store, config) = home.open();
        let vault = start(&store, &config, &Keychain::Custom(ks.clone()), true);
        let account = store.get_doc::<VaultRow>(VAULT_KIND, VAULT_ID).unwrap().map(|r| r.account);
        let _cleanup = account.clone().map(|a| Cleanup(ks.clone(), a));
        assert_eq!(vault.phase(), Phase::Active, "the real keychain was not usable here: {:?}", vault.phase());
        write_secrets(&store, &config);
        read_back(&store, &config);
        drop(store);
        assert_no_plain_on_disk(&home);

        // A new process (a restart) finds the same key in the same keychain.
        let (store, config) = home.open();
        let vault = start(&store, &config, &Keychain::Custom(Arc::new(OsKeyStore::with_service(SERVICE))), true);
        assert_eq!(vault.phase(), Phase::Active);
        read_back(&store, &config);

        // The person deletes the item in the keychain app: the state says so, nothing is touched or remade.
        ks.remove(account.as_deref().unwrap()).unwrap();
        let (store2, config2) = Home::open_again(&home);
        let vault2 = start(&store2, &config2, &Keychain::Custom(Arc::new(OsKeyStore::with_service(SERVICE))), true);
        assert_eq!(vault2.phase(), Phase::KeyLost);
        assert_eq!(ks.read(account.as_deref().unwrap()), KeyRead::Missing, "no new key over a vault that has sealed values");
        drop((store, config));
    }
}

impl Home {
    /// A second pair of handles on the same folder, as another process would have.
    fn open_again(home: &Home) -> (Store, LayeredConfig) {
        home.open()
    }
}

#[tokio::test]
async fn a_channel_or_mail_account_whose_secret_cannot_be_opened_is_not_started_with_the_ciphertext() {
    use aiw_core::api::{ChannelCreate, MailAccount};
    let (data, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let ks = MemoryKeyStore::new();
    let opts = |ks: Arc<MemoryKeyStore>| crate::EngineOptions {
        data_dir: Some(data.path().to_path_buf()),
        workspace_root: Some(ws.path().to_path_buf()),
        mode: "embedded".into(),
        offline: true,
        keychain: Keychain::Custom(ks),
        ..Default::default()
    };
    let e = crate::Engine::new(opts(ks.clone())).await.unwrap();
    let ch = e
        .channel_create(ChannelCreate {
            kind: "telegram".into(),
            name: "Bot".into(),
            secrets: [("bot_token".to_string(), BOT.to_string())].into(),
            ..Default::default()
        })
        .unwrap();
    e.set_mail_accounts(vec![MailAccount {
        id: "personale".into(),
        host: "imap.example.org".into(),
        port: 993,
        username: "io@example.org".into(),
        password: IMAP.into(),
        folders: vec![],
    }])
    .unwrap();
    let channel = e.0.store.get_doc::<aiw_core::api::Channel>("channel", &ch.channel.id).unwrap().unwrap();
    assert!(e.channel_ready(&channel).is_ok());
    drop(e);
    // The key is gone: the secrets are unreadable, the channel says a token is missing instead of connecting with ciphertext.
    ks.wipe();
    let e = crate::Engine::new(opts(ks.clone())).await.unwrap();
    assert_eq!(e.0.vault.phase(), Phase::KeyLost);
    let channel = e.0.store.get_doc::<aiw_core::api::Channel>("channel", &ch.channel.id).unwrap().unwrap();
    assert!(e.channel_ready(&channel).unwrap_err().contains("Manca il token"));
    assert!(e.imap_password("personale").is_none());
}
