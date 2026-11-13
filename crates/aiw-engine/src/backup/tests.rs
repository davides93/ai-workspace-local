use super::*;
use crate::store::Store;
use std::collections::BTreeMap;

fn dirs_with_data() -> (tempfile::TempDir, DataDirs) {
    let t = tempfile::tempdir().unwrap();
    let dirs = DataDirs::at(t.path().join("aiw-home"));
    std::fs::create_dir_all(&dirs.root).unwrap();
    (t, dirs)
}

fn write(dirs: &DataDirs, rel: &str, content: &[u8]) {
    let p = rel.split('/').fold(dirs.root.clone(), |p, c| p.join(c));
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

fn read(dirs: &DataDirs, rel: &str) -> Vec<u8> {
    std::fs::read(rel.split('/').fold(dirs.root.clone(), |p, c| p.join(c))).unwrap()
}

/// A data folder as the app leaves it, with the things that must go in and the things that must stay out.
fn populate(dirs: &DataDirs, marker: &str) {
    let s = Store::open(&dirs.db_file()).unwrap();
    s.put_doc("agent", "operator", &serde_json::json!({"id": "operator", "name": marker})).unwrap();
    s.upsert_session(&aiw_core::api::Session {
        id: "ses_1".into(),
        title: format!("Sessione {marker}"),
        agent_id: "operator".into(),
        created_at: "2026-11-12T10:00:00Z".into(),
        updated_at: "2026-11-12T10:00:00Z".into(),
        last_status: None,
        worktree: None,
        channel: None,
    })
    .unwrap();
    drop(s);
    write(dirs, "config.json", format!("{{\"ui.language\": \"it\", \"marker\": \"{marker}\"}}").as_bytes());
    write(dirs, "devices/laptop.json", b"{\"update.channel\": \"beta\"}");
    write(dirs, "skills/riassunto/SKILL.md", format!("# Riassunto {marker}").as_bytes());
    write(dirs, "attachments/ses_1/att_1-contratto.txt", b"il canone e di 800 euro");
    write(dirs, "AGENTS.md", b"Rispondi in italiano.");
    // Never saved.
    write(dirs, "models/grande.gguf", &[0xAB; 4096]);
    write(dirs, "runtimes/llama/bin", b"binary");
    write(dirs, "logs/aiw.log", b"2026-11-12 started");
    write(dirs, "browser-profile/Cookies", b"cookie");
    write(dirs, "checkpoints/run_1/file", b"snapshot");
    write(dirs, "token", b"aiw_OWNERTOKEN0123456789abcdef");
    write(dirs, "tls/key.pem", b"-----BEGIN PRIVATE KEY-----");
}

fn files_of(m: &BackupManifest) -> Vec<&str> {
    m.files.iter().map(|f| f.path.as_str()).collect()
}

/// Every file of an archive, verified, by path.
fn contents(archive: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut all = BTreeMap::new();
    scan(archive, &mut |mf, r| {
        let mut v = vec![];
        r.read_to_end(&mut v)?;
        all.insert(mf.path.clone(), v);
        Ok(())
    })
    .unwrap();
    all
}

fn snapshot_dir(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, p: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(p).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                walk(base, &e.path(), out);
            } else {
                let rel = e.path().strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(rel, std::fs::read(e.path()).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// What of the data folder matters for "was it touched": every file except the SQLite side files, which come and go.
fn untouched_view(dirs: &DataDirs) -> BTreeMap<String, Vec<u8>> {
    let mut v = snapshot_dir(&dirs.root);
    v.retain(|k, _| !k.ends_with("-wal") && !k.ends_with("-shm") && !k.starts_with(".backup-") && !k.starts_with(".restore-"));
    v
}

fn no_leftovers(dirs: &DataDirs) {
    let names: Vec<String> =
        std::fs::read_dir(&dirs.root).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(!names.iter().any(|n| n.starts_with(".restore-") || n.starts_with(".backup-")), "scratch folders left behind: {names:?}");
}

fn sha(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

fn manifest_for(files: &[(&str, &[u8])], schema: i64, sensitive: bool) -> BackupManifest {
    BackupManifest {
        format: FORMAT,
        app: APP_ID.into(),
        app_version: "0.0.0-test".into(),
        schema_version: schema,
        created_at: "2026-11-13T00:00:00Z".into(),
        os: "test".into(),
        arch: "test".into(),
        sensitive,
        excluded: vec![],
        skipped: vec![],
        files: files.iter().map(|(p, d)| ManifestFile { path: p.to_string(), size: d.len() as u64, sha256: sha(d) }).collect(),
    }
}

/// Writes a `.tar.gz` of raw entries (name, tar type, content, link target) with the manifest first, unless told otherwise.
fn forge_archive(path: &Path, manifest: &BackupManifest, entries: &[(&str, u8, &[u8], Option<&str>)], manifest_first: bool) {
    let mut raw = vec![];
    let m = serde_json::to_vec(manifest).unwrap();
    if manifest_first {
        raw.extend(tar::forge(MANIFEST_NAME, b'0', &m, None));
    }
    for (name, kind, data, link) in entries {
        raw.extend(tar::forge(name, *kind, data, *link));
    }
    if !manifest_first {
        raw.extend(tar::forge(MANIFEST_NAME, b'0', &m, None));
    }
    raw.extend(vec![0u8; 1024]);
    let mut gz = GzEncoder::new(std::fs::File::create(path).unwrap(), Compression::fast());
    gz.write_all(&raw).unwrap();
    gz.finish().unwrap();
}

/// A valid database file (the one of a new installation), as bytes.
fn db_bytes(version_override: Option<i64>) -> Vec<u8> {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("aiw.db");
    drop(Store::open(&p).unwrap());
    if let Some(v) = version_override {
        let c = Connection::open(&p).unwrap();
        c.execute_batch(&format!("PRAGMA user_version = {v}; PRAGMA journal_mode = DELETE;")).unwrap();
    }
    std::fs::read(&p).unwrap()
}

#[test]
fn what_goes_in_is_an_allow_list() {
    for rel in
        ["aiw.db", "config.json", "devices/h.json", "skills/a/SKILL.md", "attachments/s/a.txt", "AGENTS.md", ".shell-network-migrated"]
    {
        assert!(allowed(rel, false), "{rel}");
    }
    for rel in [
        "models/x.gguf",
        "runtimes/x",
        "logs/a.log",
        "bin/x",
        "browser-profile/c",
        "checkpoints/r/f",
        "backups/b.tar.gz",
        "aiw.db-wal",
        "devices",
        "config.json/x",
        "adapters/a/b",
        "random.txt",
    ] {
        assert!(!allowed(rel, false), "{rel}");
        assert!(!allowed(rel, true), "{rel} not even with secrets");
    }
    for rel in ["token", "tls/key.pem", "tls/ca.pem"] {
        assert!(!allowed(rel, false) && allowed(rel, true), "{rel}: only with secrets");
    }
}

#[test]
fn a_backup_holds_the_data_and_leaves_out_models_logs_caches_and_secrets() {
    let (_t, dirs) = dirs_with_data();
    populate(&dirs, "uno");
    let out = _t.path().join("b.tar.gz");
    let m = create(&dirs, &BackupOptions::default(), &out).unwrap();
    assert_eq!(
        files_of(&m),
        ["aiw.db", "AGENTS.md", "attachments/ses_1/att_1-contratto.txt", "config.json", "devices/laptop.json", "skills/riassunto/SKILL.md"]
    );
    assert!(!m.sensitive && m.format == FORMAT && m.app_version == aiw_core::VERSION && m.schema_version == SCHEMA_VERSION);
    assert!(m.excluded.iter().any(|e| e.contains("modelli") && e.contains("mai")), "{:?}", m.excluded);
    assert!(m.excluded.iter().any(|e| e.contains("TLS")) && m.excluded.iter().any(|e| e.contains("segreti")));
    assert!(chrono::DateTime::parse_from_rfc3339(&m.created_at).is_ok());
    // The archive checks out and says the same thing.
    assert_eq!(inspect(&out).unwrap(), m);
    assert_eq!(read_manifest(&out).unwrap(), m);
    // No scratch folder is left, and the data folder is as it was (the app may rewrite the db's side files only).
    no_leftovers(&dirs);
    assert!(read(&dirs, "models/grande.gguf").len() == 4096 && read(&dirs, "token").starts_with(b"aiw_OWNER"));
    // A backup is not overwritten.
    assert!(matches!(create(&dirs, &BackupOptions::default(), &out), Err(Error::Conflict(_))));
}

#[test]
fn restore_brings_back_what_was_saved_and_checks_it() {
    let (t, src) = dirs_with_data();
    populate(&src, "uno");
    let archive = t.path().join("b.tar.gz");
    create(&src, &BackupOptions::default(), &archive).unwrap();

    let dst = DataDirs::at(t.path().join("nuova-cartella"));
    let r = restore(&dst, &archive).unwrap();
    assert!(r.safety_copy.is_none(), "nothing there to copy");
    assert_eq!((r.schema_from, r.schema_to), (SCHEMA_VERSION, SCHEMA_VERSION));
    assert_eq!(read(&dst, "skills/riassunto/SKILL.md"), b"# Riassunto uno");
    assert_eq!(read(&dst, "attachments/ses_1/att_1-contratto.txt"), b"il canone e di 800 euro");
    assert_eq!(read(&dst, "AGENTS.md"), b"Rispondi in italiano.");
    assert!(!dst.models().exists() && !dst.root.join("token").exists() && !dst.root.join("tls").exists());
    let s = Store::open(&dst.db_file()).unwrap();
    assert_eq!(s.sessions().unwrap()[0].title, "Sessione uno");
    assert_eq!(s.list_docs::<Value>("agent").unwrap()[0]["name"], "uno");
    drop(s);
    no_leftovers(&dst);
}

#[test]
fn a_restore_over_existing_data_keeps_a_full_safety_copy_first() {
    let (t, src) = dirs_with_data();
    populate(&src, "vecchio");
    let archive = t.path().join("vecchio.tar.gz");
    create(&src, &BackupOptions::default(), &archive).unwrap();

    // The data go on: a new state, with a secret the backup does not hold.
    let (_t2, cur) = dirs_with_data();
    populate(&cur, "attuale");
    write(&cur, "config.json", b"{\"search.brave_api_key\": \"BRAVE-ATTUALE-1234\", \"marker\": \"attuale\"}");

    let r = restore(&cur, &archive).unwrap();
    let safety = r.safety_copy.clone().expect("there was data to protect");
    assert!(safety.starts_with(cur.root.join("backups")) && safety.exists());
    // The data are the backup's now.
    assert_eq!(read(&cur, "skills/riassunto/SKILL.md"), b"# Riassunto vecchio");
    assert!(String::from_utf8(read(&cur, "config.json")).unwrap().contains("vecchio"));
    assert_eq!(Store::open(&cur.db_file()).unwrap().sessions().unwrap()[0].title, "Sessione vecchio");
    // Files that were not in the backup are left alone.
    assert_eq!(read(&cur, "models/grande.gguf").len(), 4096);
    assert!(cur.root.join("token").exists());
    // The safety copy is what was there: everything, secrets included, and it is marked as sensitive.
    let m = inspect(&safety).unwrap();
    assert!(m.sensitive);
    let held = contents(&safety);
    assert!(String::from_utf8_lossy(&held["config.json"]).contains("BRAVE-ATTUALE-1234"));
    assert_eq!(held["token"], b"aiw_OWNERTOKEN0123456789abcdef");
    assert!(held.contains_key("tls/key.pem"));
    // ...and it restores, too: the way back.
    let back = restore(&cur, &safety).unwrap();
    assert!(String::from_utf8(read(&cur, "config.json")).unwrap().contains("attuale"));
    assert_eq!(Store::open(&cur.db_file()).unwrap().sessions().unwrap()[0].title, "Sessione attuale");
    assert!(back.safety_copy.is_some());
    no_leftovers(&cur);
}

#[test]
fn restore_migrates_an_older_database_and_refuses_a_newer_one() {
    // An old database, as an installation from before the schema stamp left it: two tables, no version.
    let (t, src) = dirs_with_data();
    {
        let c = Connection::open(src.db_file()).unwrap();
        c.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE documents (kind TEXT NOT NULL, id TEXT NOT NULL, json TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY(kind, id));
             CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT NOT NULL, agent_id TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, last_status TEXT);
             INSERT INTO sessions VALUES ('s1', 'Dal passato', 'operator', 'x', 'x', NULL);",
        )
        .unwrap();
    }
    let archive = t.path().join("old.tar.gz");
    let m = create(&src, &BackupOptions::default(), &archive).unwrap();
    assert_eq!(m.schema_version, 0);
    let dst = DataDirs::at(t.path().join("dst"));
    let r = restore(&dst, &archive).unwrap();
    assert_eq!((r.schema_from, r.schema_to), (0, SCHEMA_VERSION));
    let s = Store::open(&dst.db_file()).unwrap();
    assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(s.sessions().unwrap()[0].title, "Dal passato");
    assert!(s.table_counts().unwrap().iter().any(|(n, _)| n == "api_tokens"), "the new tables are there");
    drop(s);

    // A newer database: refused whole, whatever is in the data folder stays exactly as it was.
    let (_t2, cur) = dirs_with_data();
    populate(&cur, "intatto");
    let newer = _t2.path().join("newer.tar.gz");
    let nd = db_bytes(Some(SCHEMA_VERSION + 2));
    forge_archive(&newer, &manifest_for(&[("aiw.db", &nd)], SCHEMA_VERSION + 2, false), &[("aiw.db", b'0', &nd, None)], true);
    let before = untouched_view(&cur);
    let err = restore(&cur, &newer).unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)), "{err:?}");
    assert!(err.to_string().contains(&format!("schema {}", SCHEMA_VERSION + 2)) && err.to_string().contains("più recente"), "{err}");
    assert_eq!(untouched_view(&cur), before);
    assert!(!cur.root.join("backups").exists(), "no safety copy for a restore that did not start");
    // The manifest can lie about the version: the database's own is checked too.
    let liar = _t2.path().join("liar.tar.gz");
    forge_archive(&liar, &manifest_for(&[("aiw.db", &nd)], SCHEMA_VERSION, false), &[("aiw.db", b'0', &nd, None)], true);
    assert!(matches!(restore(&cur, &liar), Err(Error::Unsupported(_))));
    assert_eq!(untouched_view(&cur), before);
    no_leftovers(&cur);
}

#[test]
fn hostile_or_damaged_archives_are_refused_and_nothing_is_touched() {
    let (t, cur) = dirs_with_data();
    populate(&cur, "intatto");
    let before = untouched_view(&cur);
    let db = db_bytes(None);
    let good: (&str, &[u8]) = ("aiw.db", &db);
    #[allow(clippy::type_complexity)]
    let bad_cases: Vec<(&str, BackupManifest, Vec<(&str, u8, Vec<u8>, Option<&str>)>, &str)> = vec![
        (
            "path traversal",
            manifest_for(&[good, ("../evil.txt", b"x")], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("../evil.txt", b'0', b"x".to_vec(), None)],
            "esce dalla cartella",
        ),
        (
            "nested traversal",
            manifest_for(&[good, ("skills/../../evil.txt", b"x")], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("skills/../../evil.txt", b'0', b"x".to_vec(), None)],
            "esce dalla cartella",
        ),
        (
            "absolute path",
            manifest_for(&[good, ("/etc/cron.d/x", b"x")], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("/etc/cron.d/x", b'0', b"x".to_vec(), None)],
            "assoluto",
        ),
        (
            "windows drive",
            manifest_for(&[good, ("C:/Windows/x", b"x")], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("C:/Windows/x", b'0', b"x".to_vec(), None)],
            "unità",
        ),
        (
            "backslash",
            manifest_for(&[good, ("skills\\..\\x", b"x")], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("skills\\..\\x", b'0', b"x".to_vec(), None)],
            "non ammessi",
        ),
        (
            "symlink entry",
            manifest_for(&[good], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("skills/passwd", b'2', vec![], Some("/etc/passwd"))],
            "collegamento",
        ),
        (
            "hard link entry",
            manifest_for(&[good], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("skills/x", b'1', vec![], Some("aiw.db"))],
            "collegamento",
        ),
        (
            "device node",
            manifest_for(&[good], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("skills/null", b'3', vec![], None)],
            "tipo non ammesso",
        ),
        (
            "file outside the allow-list",
            manifest_for(&[good, ("bin/aiw", b"x")], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("bin/aiw", b'0', b"x".to_vec(), None)],
            "non è un file che un backup può contenere",
        ),
        (
            "model file",
            manifest_for(&[good, ("models/m.gguf", b"x")], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("models/m.gguf", b'0', b"x".to_vec(), None)],
            "non è un file che un backup può contenere",
        ),
        (
            "secrets in an archive that says it has none",
            manifest_for(&[good, ("token", b"aiw_x")], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("token", b'0', b"aiw_x".to_vec(), None)],
            "non è un file che un backup può contenere",
        ),
        (
            "file not in the manifest",
            manifest_for(&[good], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("skills/extra.md", b'0', b"x".to_vec(), None)],
            "non nel manifesto",
        ),
        (
            "listed but missing",
            manifest_for(&[good, ("AGENTS.md", b"x")], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None)],
            "manca nell'archivio",
        ),
        (
            "wrong content (sha-256)",
            manifest_for(&[good, ("AGENTS.md", b"originale")], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("AGENTS.md", b'0', b"alterato!".to_vec(), None)],
            "SHA-256",
        ),
        (
            "wrong size",
            manifest_for(&[good, ("AGENTS.md", b"abc")], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("AGENTS.md", b'0', b"abcdef".to_vec(), None)],
            "dichiara",
        ),
        (
            "listed twice",
            manifest_for(&[good, ("AGENTS.md", b"x"), ("AGENTS.md", b"x")], SCHEMA_VERSION, false),
            vec![("aiw.db", b'0', db.clone(), None), ("AGENTS.md", b'0', b"x".to_vec(), None)],
            "due volte",
        ),
        (
            "no database",
            manifest_for(&[("AGENTS.md", b"x")], SCHEMA_VERSION, false),
            vec![("AGENTS.md", b'0', b"x".to_vec(), None)],
            "manca aiw.db",
        ),
        (
            "bad hash format",
            {
                let mut m = manifest_for(&[good], SCHEMA_VERSION, false);
                m.files[0].sha256 = "xyz".into();
                m
            },
            vec![("aiw.db", b'0', db.clone(), None)],
            "SHA-256",
        ),
        (
            "newer archive format",
            {
                let mut m = manifest_for(&[good], SCHEMA_VERSION, false);
                m.format = FORMAT + 1;
                m
            },
            vec![("aiw.db", b'0', db.clone(), None)],
            "formato",
        ),
        (
            "someone else's archive",
            {
                let mut m = manifest_for(&[good], SCHEMA_VERSION, false);
                m.app = "altro".into();
                m
            },
            vec![("aiw.db", b'0', db.clone(), None)],
            "non è un backup di AI Workspace Local",
        ),
    ];
    for (name, manifest, entries, expect) in &bad_cases {
        let p = t.path().join("bad.tar.gz");
        let _ = std::fs::remove_file(&p);
        let refs: Vec<(&str, u8, &[u8], Option<&str>)> = entries.iter().map(|(n, k, d, l)| (*n, *k, d.as_slice(), *l)).collect();
        forge_archive(&p, manifest, &refs, true);
        let err = restore(&cur, &p).err().unwrap_or_else(|| panic!("{name}: must be refused"));
        assert!(err.to_string().contains(expect), "{name}: {err}");
        assert!(inspect(&p).is_err(), "{name}: inspect agrees");
        assert_eq!(untouched_view(&cur), before, "{name}: the data folder was touched");
        no_leftovers(&cur);
        assert!(!cur.root.join("backups").exists(), "{name}: a safety copy for a restore that did not start");
        assert!(!t.path().join("evil.txt").exists() && !cur.root.join("evil.txt").exists());
    }

    // The manifest must come first.
    let p = t.path().join("order.tar.gz");
    forge_archive(&p, &manifest_for(&[good], SCHEMA_VERSION, false), &[("aiw.db", b'0', &db, None)], false);
    assert!(restore(&cur, &p).unwrap_err().to_string().contains("la prima voce"));
    // Not an archive at all; truncated; empty.
    let junk = t.path().join("junk.tar.gz");
    std::fs::write(&junk, b"questo non e un archivio").unwrap();
    assert!(restore(&cur, &junk).is_err() && inspect(&junk).is_err());
    let valid = t.path().join("valid.tar.gz");
    create(&cur, &BackupOptions::default(), &valid).unwrap();
    let bytes = std::fs::read(&valid).unwrap();
    let cut = t.path().join("cut.tar.gz");
    std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
    assert!(inspect(&cut).is_err() && restore(&cur, &cut).is_err());
    std::fs::write(&cut, b"").unwrap();
    assert!(inspect(&cut).is_err());
    assert!(matches!(inspect(&t.path().join("non-esiste.tar.gz")), Err(Error::NotFound(_))));
    assert_eq!(untouched_view(&cur), {
        let mut b = before.clone();
        b.retain(|k, _| !k.starts_with("backups/"));
        b
    });
}

#[test]
fn a_database_in_the_backup_that_is_corrupt_is_refused_before_anything_changes() {
    let (t, cur) = dirs_with_data();
    populate(&cur, "intatto");
    let before = untouched_view(&cur);
    let mut db = db_bytes(None);
    // Damage the first page after the header (the schema) but leave the header (and so the version) valid.
    for b in &mut db[100..600] {
        *b = 0xFF;
    }
    let p = t.path().join("c.tar.gz");
    forge_archive(&p, &manifest_for(&[("aiw.db", &db)], SCHEMA_VERSION, false), &[("aiw.db", b'0', &db, None)], true);
    let err = restore(&cur, &p).unwrap_err();
    assert!(err.to_string().contains("Nulla è stato modificato") || err.to_string().contains("database"), "{err}");
    assert_eq!(untouched_view(&cur), before);
    no_leftovers(&cur);
}

#[test]
fn a_failure_while_replacing_puts_the_old_files_back() {
    let (t, src) = dirs_with_data();
    populate(&src, "nuovo");
    let archive = t.path().join("b.tar.gz");
    create(&src, &BackupOptions::default(), &archive).unwrap();

    let (_t2, cur) = dirs_with_data();
    populate(&cur, "vecchio");
    // `skills/riassunto/SKILL.md` has a directory where the backup's file goes: that step cannot be done.
    std::fs::remove_file(cur.root.join("skills").join("riassunto").join("SKILL.md")).unwrap();
    std::fs::create_dir_all(cur.root.join("skills").join("riassunto").join("SKILL.md")).unwrap();
    std::fs::write(cur.root.join("skills").join("riassunto").join("SKILL.md").join("dentro.txt"), b"x").unwrap();
    let before = untouched_view(&cur);
    let err = restore(&cur, &archive).unwrap_err();
    assert!(err.to_string().contains("ripristino interrotto") && err.to_string().contains("rimessi a posto"), "{err}");
    let mut after = untouched_view(&cur);
    after.retain(|k, _| !k.starts_with("backups/"));
    assert_eq!(after, before, "the files replaced before the failure are back");
    assert_eq!(Store::open(&cur.db_file()).unwrap().sessions().unwrap()[0].title, "Sessione vecchio");
    assert!(cur.root.join("backups").exists(), "the safety copy was made, and stays");
    no_leftovers(&cur);
}

#[test]
fn restore_is_refused_while_the_database_is_open() {
    let (t, src) = dirs_with_data();
    populate(&src, "uno");
    let archive = t.path().join("b.tar.gz");
    create(&src, &BackupOptions::default(), &archive).unwrap();

    let (_t2, cur) = dirs_with_data();
    let open = Store::open(&cur.db_file()).unwrap();
    open.put_doc("agent", "a", &serde_json::json!({"x": 1})).unwrap();
    let before = untouched_view(&cur);
    // Verified on Linux. SQLite's own locking decides this; on Windows and macOS the same call is used but this has
    // not been run there.
    #[cfg(not(windows))]
    {
        assert!(data_in_use(&cur).unwrap(), "an open store is in use");
        let err = restore(&cur, &archive).unwrap_err();
        assert!(matches!(err, Error::Conflict(_)) && err.to_string().contains("chiudi l'app"), "{err}");
        assert_eq!(untouched_view(&cur), before);
        no_leftovers(&cur);
    }
    drop(open);
    assert!(!data_in_use(&cur).unwrap(), "closed: free");
    assert!(restore(&cur, &archive).is_ok());
}

#[test]
fn the_database_is_copied_consistently_while_the_app_has_it_open() {
    let (t, dirs) = dirs_with_data();
    let s = Store::open(&dirs.db_file()).unwrap();
    for i in 0..200 {
        s.put_doc("agent", &format!("a{i}"), &serde_json::json!({"n": i})).unwrap();
    }
    // The app keeps its connection: the rows are in the write-ahead log, not necessarily in the main file.
    let archive = t.path().join("live.tar.gz");
    let m = create(&dirs, &BackupOptions::default(), &archive).unwrap();
    s.put_doc("agent", "dopo", &serde_json::json!({})).unwrap();
    assert_eq!(m.files[0].path, "aiw.db");
    let held = contents(&archive);
    let copy = t.path().join("copy.db");
    std::fs::write(&copy, &held["aiw.db"]).unwrap();
    let c = Connection::open(&copy).unwrap();
    assert_eq!(c.query_row("SELECT COUNT(*) FROM documents WHERE kind='agent'", [], |r| r.get::<_, i64>(0)).unwrap(), 200);
    assert_eq!(c.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)).unwrap(), "ok");
    // And the app's own file was not disturbed.
    assert_eq!(s.list_docs::<Value>("agent").unwrap().len(), 201);
}

#[cfg(unix)]
#[test]
fn links_inside_the_data_folder_are_not_followed() {
    let (t, dirs) = dirs_with_data();
    populate(&dirs, "uno");
    let secret_dir = t.path().join("fuori");
    std::fs::create_dir_all(&secret_dir).unwrap();
    std::fs::write(secret_dir.join("segreto.txt"), b"FUORI-DALLA-CARTELLA").unwrap();
    std::os::unix::fs::symlink(&secret_dir, dirs.root.join("skills").join("link")).unwrap();
    std::os::unix::fs::symlink(secret_dir.join("segreto.txt"), dirs.root.join("AGENTS-link.md")).unwrap();
    std::os::unix::fs::symlink(secret_dir.join("segreto.txt"), dirs.root.join("skills").join("riassunto").join("file-link.md")).unwrap();
    let out = t.path().join("b.tar.gz");
    let m = create(&dirs, &BackupOptions::default(), &out).unwrap();
    assert!(!files_of(&m).iter().any(|f| f.contains("link")), "{:?}", files_of(&m));
    assert!(m.skipped.iter().any(|s| s.contains("skills/link")) && m.skipped.iter().any(|s| s.contains("file-link.md")), "{:?}", m.skipped);
    assert!(!contents(&out).values().any(|v| String::from_utf8_lossy(v).contains("FUORI-DALLA-CARTELLA")));
}

#[test]
fn settings_files_lose_their_secrets_and_keep_the_rest() {
    let cfg = br#"{"ui.language":"en","search.brave_api_key":"BRAVE-KEY-ABC123","update.channel":"beta"}"#;
    let out: Value = serde_json::from_slice(&scrub_config(cfg, "config.json").unwrap()).unwrap();
    assert_eq!(out, serde_json::json!({"ui.language": "en", "update.channel": "beta"}));
    assert!(scrub_config(b"{ rotto", "config.json").unwrap_err().to_string().contains("non è un JSON valido"));
    let mut v = serde_json::json!({
        "headers": {"Authorization": "Bearer X1", "Accept": "json"},
        "env": {"GITHUB_TOKEN": "ghp_x", "PATH": "/usr/bin"},
        "secrets": {"bot_token": "T1", "imap_password": "P1"},
        "nested": [{"api_key": "K1", "name": "ok"}],
        "token": 42,
    });
    v["url"] = serde_json::json!("https://utente:PASSWORD@mcp.example.org:8443/mcp?api_key=K9&q=ciao&access_token=T9#x");
    scrub_value(&mut v);
    assert_eq!(
        v,
        serde_json::json!({
            "headers": {"Authorization": "", "Accept": "json"},
            "env": {"GITHUB_TOKEN": "", "PATH": "/usr/bin"},
            "secrets": {"bot_token": "", "imap_password": ""},
            "nested": [{"api_key": "", "name": "ok"}],
            "token": 42,
            "url": "https://mcp.example.org:8443/mcp?api_key=&q=ciao&access_token=#x",
        })
    );
}

/// Every kind of document the engine stores has to be classified (secret, carries a secret, or plain): a new kind that is
/// not in the lists would be backed up unexamined.
#[test]
fn every_document_kind_in_the_sources_is_classified() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let (put, konst) = (
        regex::Regex::new(r#"(?:put_doc|get_doc(?:::<[^>]*>)?|list_docs(?:::<[^>]*>)?|delete_doc)\(\s*"([a-z_]+)""#).unwrap(),
        regex::Regex::new(r#"(?:_KIND|COLLECTION): &str = "([a-z_]+)""#).unwrap(),
    );
    let known: BTreeSet<&str> = SECRET_KINDS.iter().chain(&SCRUB_KINDS).chain(&PLAIN_KINDS).copied().collect();
    let mut found = BTreeSet::new();
    fn walk(dir: &Path, f: &mut dyn FnMut(&Path)) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, f);
            } else if p.extension().is_some_and(|x| x == "rs") {
                f(&p);
            }
        }
    }
    walk(&src, &mut |p| {
        let name = p.file_name().unwrap().to_string_lossy();
        if name.starts_with("tests") || name == "tests.rs" || p.components().any(|c| c.as_os_str() == "backup") {
            return;
        }
        let text = std::fs::read_to_string(p).unwrap();
        for c in put.captures_iter(&text).chain(konst.captures_iter(&text)) {
            found.insert(c[1].to_string());
        }
    });
    assert!(found.len() > 15, "the scan found only {found:?}");
    let unknown: Vec<&String> = found.iter().filter(|k| !known.contains(k.as_str())).collect();
    assert!(unknown.is_empty(), "classify these document kinds in backup/mod.rs (SECRET_KINDS, SCRUB_KINDS or PLAIN_KINDS): {unknown:?}");
    let stale: Vec<&&str> = known.iter().filter(|k| !found.contains(**k)).collect();
    assert!(stale.is_empty(), "kinds in the lists that nothing writes any more: {stale:?}");
}
