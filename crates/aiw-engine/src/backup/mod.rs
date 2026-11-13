//! Backup and restore of the data folder (`~/.aiw`, or `$AIW_HOME`, as [`DataDirs`] resolves it).
//!
//! **The archive** is one `.tar.gz`. Its first entry is `manifest.json` (app version, schema version of the database,
//! date, and every file with its size and SHA-256); the files follow. What goes in is an allow-list ([`AREAS`]), not a
//! block-list: a file that nobody thought about is left out.
//!
//! **The database** is copied with `VACUUM INTO` through a second connection (a consistent snapshot, whether or not the
//! daemon has the file open), never by copying the open file. Unless the backup asks for secrets, the copy is then scrubbed
//! (and vacuumed again, with `secure_delete`, so that no deleted secret stays in free pages).
//!
//! **Left out, always**: model files (GGUF, safetensors, adapters: they are large, re-downloadable, and there is no option to
//! include them), runtimes and installed tools, the browser profile, update downloads, git worktrees, evaluation and training
//! work folders, checkpoints, caches and logs.
//!
//! **Left out unless `include_secrets`**: TLS keys and certificates (`tls/`), the owner token (`token`), the access tokens
//! of devices and apps (table `api_tokens`), and every secret that the app stores: IMAP passwords, channel tokens, model
//! provider keys, headers and variables of MCP servers, MCP OAuth tokens, paired devices' tokens, API keys in the settings.
//! The configuration of those things stays (a channel, a server, a provider), with the secret emptied: it has to be entered
//! again after a restore. An archive made with `include_secrets` says so (`sensitive` in the manifest) and has to be kept
//! like the data folder itself.
//!
//! **Restore** checks everything before touching anything: format, manifest, every SHA-256, every path (no absolute paths,
//! no `..`, no links, nothing outside the allow-list), and that the database does not come from a newer build; it migrates
//! the database in the staging copy; it makes a safety copy of the current data (everything, secrets included, in
//! `backups/`); and only then replaces files one by one, undoing them if one fails. It does **not** run while the daemon
//! (or anything else) has the database open: the database file and the configuration are replaced under the app's feet
//! otherwise, and the app would write the old state back. So restoring is a command, `aiw backup restore`, with the app
//! closed; there is no API for it.

mod tar;

use crate::secrets::is_secret_name;
use crate::store::{self, SCHEMA_VERSION};
use crate::Engine;
use aiw_core::paths::DataDirs;
use aiw_core::{now_rfc3339, Error, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Version of the archive layout. A reader refuses a higher one.
pub const FORMAT: u32 = 1;
pub const MANIFEST_NAME: &str = "manifest.json";
/// The database inside the archive, and in the data folder.
pub const DB_NAME: &str = "aiw.db";
const APP_ID: &str = "ai-workspace-local";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

/// One thing that a backup may hold, relative to the data folder.
struct Area {
    path: &'static str,
    dir: bool,
    /// Only with `include_secrets`.
    secret: bool,
}

const fn area(path: &'static str, dir: bool, secret: bool) -> Area {
    Area { path, dir, secret }
}

/// Everything a backup can contain besides the database (`aiw.db`). Anything else in the data folder is never saved.
const AREAS: [Area; 11] = [
    area("config.json", false, false),
    area("devices", true, false),
    area("skills", true, false),
    area("workflows", true, false),
    area("evals", true, false),
    area("datasets", true, false),
    area("attachments", true, false),
    area("AGENTS.md", false, false),
    area(".shell-network-migrated", false, false),
    area("token", false, true),
    area("tls", true, true),
];

/// May `rel` (a validated relative path with `/` separators) be in an archive, and be restored?
fn allowed(rel: &str, sensitive: bool) -> bool {
    if rel == DB_NAME {
        return true;
    }
    let first = rel.split('/').next().unwrap_or_default();
    AREAS.iter().filter(|a| a.path == first).any(|a| (!a.secret || sensitive) && (a.dir == rel.contains('/')))
}

/// Kinds of the `documents` table that are secrets in themselves: not saved without `include_secrets`.
const SECRET_KINDS: [&str; 3] = ["secret", "peer_token", "mcp_oauth"];
/// Kinds that carry a secret among other things: saved with the secret emptied.
const SCRUB_KINDS: [&str; 5] = ["mcp", "channel", "model_provider", "remote_agent", "runtime_settings"];
/// Kinds that hold no secret. Together with the two lists above this must cover every kind the engine writes: a test
/// reads the sources and fails when a new kind is not classified here.
#[cfg(test)]
const PLAIN_KINDS: [&str; 19] = [
    "adapter",
    "agent",
    "channel_conv",
    "channel_state",
    "eval_suite",
    "hooks",
    "lesson",
    "mcp_approval",
    "mcp_meta",
    "peer",
    "policy",
    "profile",
    "prompt_optimization",
    "proactive",
    "route",
    "team",
    "tool_settings",
    "worktree",
    "workflow",
];

/// What the backup says it leaves out (always the first four; the secrets only when they are left out).
fn excluded_notes(include_secrets: bool) -> Vec<String> {
    let mut v = vec![
        "modelli (file GGUF, safetensors, adapter LoRA: non si includono mai)".to_string(),
        "runtimi e strumenti installati, profilo del browser, aggiornamenti scaricati".to_string(),
        "cache, log, cartelle di lavoro di valutazioni e addestramenti, worktree git e checkpoint".to_string(),
        "tutto ciò che non è nell'elenco dei dati dell'app (il backup è una lista di cose ammesse, non di cose escluse)".to_string(),
    ];
    if !include_secrets {
        v.push("chiavi e certificati TLS (cartella tls)".into());
        v.push("token del computer (file token) e token dei dispositivi e delle app (tabella api_tokens)".into());
        v.push(
            "segreti: password IMAP, token dei canali, chiavi dei provider di modelli, header e variabili dei server MCP, token OAuth dei server MCP, token dei dispositivi accoppiati, chiavi API nelle impostazioni (la configurazione resta, con il segreto svuotato)"
                .into(),
        );
    }
    v
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestFile {
    /// Relative to the data folder, `/`-separated.
    pub path: String,
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format: u32,
    pub app: String,
    pub app_version: String,
    /// `PRAGMA user_version` of the database in the archive.
    pub schema_version: i64,
    pub created_at: String,
    pub os: String,
    pub arch: String,
    /// The archive holds secrets (`include_secrets`): keep it like the data folder itself.
    pub sensitive: bool,
    /// What was left out, in words.
    pub excluded: Vec<String>,
    /// Things that were there but could not be saved (links, names that are not UTF-8), in words.
    #[serde(default)]
    pub skipped: Vec<String>,
    pub files: Vec<ManifestFile>,
}

impl BackupManifest {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

#[derive(Debug, Clone, Default)]
pub struct BackupOptions {
    /// Save the secrets too (see the module documentation). Models are never saved: there is no option for it.
    pub include_secrets: bool,
}

fn db_err<E: std::fmt::Display>(e: E) -> Error {
    Error::Other(format!("errore del database: {e}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A scratch folder that is removed when it goes out of scope.
struct Work(PathBuf);

impl Work {
    fn new(parent: &Path, label: &str) -> Result<Self> {
        let p = parent.join(format!(".{label}-{}", aiw_core::ids::new_id("w")));
        std::fs::create_dir_all(&p)?;
        restrict(&p, 0o700);
        Ok(Self(p))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn restrict(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode));
}
#[cfg(not(unix))]
fn restrict(_: &Path, _: u32) {}

// ----------------------------------------------------------------------------- scrubbing

/// An address without the credentials that can ride in it: `https://user:pass@host/` loses `user:pass@`, and the value
/// of `?api_key=…`, `&token=…` and the like is emptied.
fn scrub_url(s: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;
    static USERINFO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(\b[a-z][a-z0-9+.\-]*://)[^/\s?#@]*@").expect("static pattern"));
    static QUERY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)([?&](?:[a-z0-9_.\-]*(?:key|token|secret|passw(?:or)?d|auth|sig|signature|credential)[a-z0-9_.\-]*)=)[^&#\s]*")
            .expect("static pattern")
    });
    QUERY.replace_all(&USERINFO.replace_all(s, "${1}"), "${1}").into_owned()
}

/// Empties every string whose key names a secret, anywhere in `v`; the `secrets` map of a channel is emptied whole; the
/// credentials inside any address are taken out.
fn scrub_value(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if k == "secrets" {
                    if let Value::Object(s) = x {
                        s.values_mut().for_each(|y| *y = Value::String(String::new()));
                        continue;
                    }
                }
                match x {
                    Value::String(s) if is_secret_name(k) => s.clear(),
                    Value::String(s) if s.contains("://") => *s = scrub_url(s),
                    _ => scrub_value(x),
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(scrub_value),
        _ => {}
    }
}

fn table_exists(c: &Connection, name: &str) -> Result<bool> {
    use rusqlite::OptionalExtension;
    Ok(c.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |_| Ok(()))
        .optional()
        .map_err(db_err)?
        .is_some())
}

/// Takes the secrets out of a copy of the database.
fn scrub_db(c: &Connection) -> Result<()> {
    if table_exists(c, "api_tokens")? {
        c.execute("DELETE FROM api_tokens", []).map_err(db_err)?;
    }
    if !table_exists(c, "documents")? {
        return Ok(());
    }
    for kind in SECRET_KINDS {
        c.execute("DELETE FROM documents WHERE kind = ?1", [kind]).map_err(db_err)?;
    }
    for kind in SCRUB_KINDS {
        let rows: Vec<(String, String)> = {
            let mut st = c.prepare("SELECT id, json FROM documents WHERE kind = ?1").map_err(db_err)?;
            let r = st.query_map([kind], |r| Ok((r.get(0)?, r.get(1)?))).map_err(db_err)?;
            r.collect::<std::result::Result<_, _>>().map_err(db_err)?
        };
        for (id, json) in rows {
            match serde_json::from_str::<Value>(&json) {
                Ok(mut v) => {
                    scrub_value(&mut v);
                    c.execute("UPDATE documents SET json = ?3 WHERE kind = ?1 AND id = ?2", rusqlite::params![kind, id, v.to_string()])
                        .map_err(db_err)?;
                }
                // What cannot be read cannot be scrubbed: it does not go in.
                Err(_) => {
                    c.execute("DELETE FROM documents WHERE kind = ?1 AND id = ?2", rusqlite::params![kind, id]).map_err(db_err)?;
                }
            }
        }
    }
    Ok(())
}

/// A settings file without the settings that the schema marks as secret.
fn scrub_config(bytes: &[u8], what: &str) -> Result<Vec<u8>> {
    let mut v: Value = serde_json::from_slice(bytes).map_err(|e| {
        Error::Invalid(format!(
            "{what} non è un JSON valido ({e}): non posso togliere i segreti. Correggilo, oppure usa l'opzione che include i segreti"
        ))
    })?;
    if let Value::Object(m) = &mut v {
        let schema = aiw_core::config::app_schema();
        m.retain(|k, _| !schema.properties.get(k).is_some_and(|f| f.secret));
    }
    Ok(serde_json::to_vec_pretty(&v)?)
}

/// A consistent copy of the database at `dest`, scrubbed when asked. Returns its schema version.
fn snapshot_db(src: &Path, dest: &Path, scrub: bool) -> Result<i64> {
    {
        let conn = Connection::open_with_flags(src, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(db_err)?;
        conn.busy_timeout(Duration::from_secs(10)).map_err(db_err)?;
        let to = dest.to_str().ok_or_else(|| Error::Other("percorso di lavoro non UTF-8".into()))?;
        conn.execute("VACUUM INTO ?1", [to]).map_err(db_err)?;
    }
    let copy = Connection::open(dest).map_err(db_err)?;
    if scrub {
        copy.execute_batch("PRAGMA secure_delete = ON").map_err(db_err)?;
        scrub_db(&copy)?;
        copy.execute_batch("VACUUM").map_err(db_err)?;
    }
    let version = copy.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).map_err(db_err)?;
    copy.execute_batch("PRAGMA journal_mode = DELETE").map_err(db_err)?;
    Ok(version)
}

// ----------------------------------------------------------------------------- create

enum Source {
    Disk(PathBuf),
    Memory(Vec<u8>),
}

struct Item {
    rel: String,
    source: Source,
}

impl Item {
    fn open(&self) -> Result<Box<dyn Read + '_>> {
        Ok(match &self.source {
            Source::Disk(p) => Box::new(std::fs::File::open(p).map_err(|e| Error::Other(format!("{}: {e}", self.rel)))?),
            Source::Memory(b) => Box::new(&b[..]),
        })
    }
}

struct Hashing<R: Read> {
    inner: R,
    hasher: Sha256,
    bytes: u64,
}

impl<R: Read> Hashing<R> {
    fn new(inner: R) -> Self {
        Self { inner, hasher: Sha256::new(), bytes: 0 }
    }
}

impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hasher.update(&buf[..n]);
        self.bytes += n as u64;
        Ok(n)
    }
}

/// Lists the regular files under `dir`, as `(relative path, absolute path)`. Links and names that cannot be stored are
/// not followed or saved, and are reported in `skipped`.
fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>, skipped: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let Some(name) = e.file_name().to_str().map(String::from) else {
            skipped.push(format!("{}: il nome non è UTF-8", path.display()));
            continue;
        };
        let rel_path = format!("{rel}/{name}");
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if meta.file_type().is_symlink() {
            skipped.push(format!("{rel_path}: collegamento simbolico (non si segue)"));
        } else if meta.is_dir() {
            walk(&path, &rel_path, out, skipped);
        } else if meta.is_file() {
            if tar::safe_path(&rel_path).is_err() {
                skipped.push(format!("{rel_path}: nome non ammesso in un archivio"));
            } else {
                out.push((rel_path, path));
            }
        }
    }
}

/// Makes a backup of the data folder at `out` (which must not exist). The daemon may be running: the database is read
/// through its own connection and the rest are plain files.
pub fn create(dirs: &DataDirs, opts: &BackupOptions, out: &Path) -> Result<BackupManifest> {
    if out.exists() {
        return Err(Error::Conflict(format!("{} esiste già: scegli un altro nome", out.display())));
    }
    let db = dirs.db_file();
    if !db.exists() {
        return Err(Error::NotFound(format!("non c'è nessun database in {}: non c'è niente da salvare", dirs.root.display())));
    }
    let work = Work::new(&dirs.root, "backup")?;
    let mut skipped = vec![];

    let snapshot = work.path().join(DB_NAME);
    let schema_version = snapshot_db(&db, &snapshot, !opts.include_secrets)?;
    let mut items = vec![Item { rel: DB_NAME.into(), source: Source::Disk(snapshot) }];

    for a in AREAS.iter().filter(|a| opts.include_secrets || !a.secret) {
        let p = dirs.root.join(a.path);
        let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
        if meta.file_type().is_symlink() {
            skipped.push(format!("{}: collegamento simbolico (non si segue)", a.path));
            continue;
        }
        let mut found = vec![];
        if a.dir && meta.is_dir() {
            walk(&p, a.path, &mut found, &mut skipped);
        } else if !a.dir && meta.is_file() {
            found.push((a.path.to_string(), p));
        }
        for (rel, path) in found {
            let is_settings = !opts.include_secrets && (rel == "config.json" || (rel.starts_with("devices/") && rel.ends_with(".json")));
            let source = if is_settings {
                let bytes = std::fs::read(&path).map_err(|e| Error::Other(format!("{rel}: {e}")))?;
                Source::Memory(scrub_config(&bytes, &rel)?)
            } else {
                Source::Disk(path)
            };
            items.push(Item { rel, source });
        }
    }

    // The database first, then by name: the same data always give the same listing.
    items.sort_by(|a, b| (a.rel != DB_NAME, &a.rel).cmp(&(b.rel != DB_NAME, &b.rel)));

    // First pass: sizes and hashes, for the manifest that opens the archive.
    let mut files = vec![];
    for it in &items {
        let mut r = Hashing::new(it.open()?);
        std::io::copy(&mut r, &mut std::io::sink()).map_err(|e| Error::Other(format!("{}: {e}", it.rel)))?;
        files.push(ManifestFile { path: it.rel.clone(), size: r.bytes, sha256: hex(&r.hasher.finalize()) });
    }
    let manifest = BackupManifest {
        format: FORMAT,
        app: APP_ID.into(),
        app_version: aiw_core::VERSION.into(),
        schema_version,
        created_at: now_rfc3339(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        sensitive: opts.include_secrets,
        excluded: excluded_notes(opts.include_secrets),
        skipped,
        files,
    };

    // Second pass: the archive, written aside and renamed when complete. A file that changed since the first pass is an error.
    let partial = out.with_file_name(format!(".{}.partial", out.file_name().and_then(|n| n.to_str()).unwrap_or("backup")));
    let written = (|| -> Result<()> {
        let mut f = std::fs::File::create(&partial)?;
        restrict(&partial, 0o600);
        let mtime = chrono::Utc::now().timestamp().max(0) as u64;
        let mode = if opts.include_secrets { 0o600 } else { 0o644 };
        let mut w = tar::Writer::new(GzEncoder::new(std::io::BufWriter::new(&mut f), Compression::default()));
        w.bytes(MANIFEST_NAME, 0o644, mtime, &serde_json::to_vec_pretty(&manifest)?)?;
        for (it, mf) in items.iter().zip(&manifest.files) {
            let mut r = Hashing::new(it.open()?);
            w.file(&it.rel, mf.size, mode, mtime, &mut r)?;
            if r.bytes != mf.size || hex(&r.hasher.finalize()) != mf.sha256 {
                return Err(Error::Other(format!("{}: il file è cambiato durante il backup: riprova", it.rel)));
            }
        }
        let gz = w.finish()?;
        gz.finish()?.flush()?;
        f.sync_all()?;
        Ok(())
    })();
    match written {
        Ok(()) => std::fs::rename(&partial, out)?,
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            return Err(e);
        }
    }
    Ok(manifest)
}

// ----------------------------------------------------------------------------- read, verify

fn invalid(msg: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("archivio di backup non valido: {msg}"))
}

fn open_archive(path: &Path) -> Result<tar::Reader<GzDecoder<std::io::BufReader<std::fs::File>>>> {
    let f = std::fs::File::open(path).map_err(|e| Error::NotFound(format!("{}: {e}", path.display())))?;
    Ok(tar::Reader::new(GzDecoder::new(std::io::BufReader::new(f))))
}

fn read_manifest_from<R: Read>(r: &mut tar::Reader<R>) -> Result<BackupManifest> {
    let first = r.next()?.ok_or_else(|| invalid("l'archivio è vuoto"))?;
    if first.path != MANIFEST_NAME || first.kind != tar::Kind::File {
        return Err(invalid(format!("la prima voce deve essere {MANIFEST_NAME}")));
    }
    if first.size > MAX_MANIFEST_BYTES {
        return Err(invalid("il manifesto è troppo grande"));
    }
    let mut bytes = vec![];
    r.body().read_to_end(&mut bytes).map_err(|e| invalid(format!("lettura dell'archivio: {e}")))?;
    let m: BackupManifest = serde_json::from_slice(&bytes).map_err(|e| invalid(format!("manifesto illeggibile: {e}")))?;
    validate(&m)?;
    Ok(m)
}

fn validate(m: &BackupManifest) -> Result<()> {
    if m.app != APP_ID {
        return Err(invalid("non è un backup di AI Workspace Local"));
    }
    if m.format == 0 || m.format > FORMAT {
        return Err(Error::Unsupported(format!(
            "il backup ha il formato {}, scritto da una versione più recente di AI Workspace Local: questa conosce il formato {FORMAT}. Aggiorna l'app",
            m.format
        )));
    }
    let mut seen = BTreeSet::new();
    for f in &m.files {
        tar::safe_path(&f.path)?;
        if !allowed(&f.path, m.sensitive) {
            return Err(invalid(format!("«{}» non è un file che un backup può contenere", f.path)));
        }
        if f.sha256.len() != 64 || !f.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err(invalid(format!("somma SHA-256 di «{}» non valida", f.path)));
        }
        if !seen.insert(f.path.as_str()) {
            return Err(invalid(format!("«{}» è elencato due volte", f.path)));
        }
    }
    if !seen.contains(DB_NAME) {
        return Err(invalid(format!("manca {DB_NAME}")));
    }
    Ok(())
}

/// Reads only the manifest (cheap), and validates it.
pub fn read_manifest(archive: &Path) -> Result<BackupManifest> {
    read_manifest_from(&mut open_archive(archive)?)
}

/// Reads the whole archive: the manifest, then every file against its size and SHA-256. `on_file` gets each file's content
/// before it is verified; an error from it, or from a failed check, ends the scan (and the caller discards what it made).
fn scan(archive: &Path, on_file: &mut dyn FnMut(&ManifestFile, &mut dyn Read) -> Result<()>) -> Result<BackupManifest> {
    let mut r = open_archive(archive)?;
    let manifest = read_manifest_from(&mut r)?;
    let wanted: BTreeMap<&str, &ManifestFile> = manifest.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let mut done = BTreeSet::new();
    let corrupt = |e: std::io::Error| invalid(format!("lettura dell'archivio: {e}"));
    while let Some(h) = r.next()? {
        tar::safe_path(&h.path)?;
        if h.kind == tar::Kind::Dir {
            continue;
        }
        let Some(mf) = wanted.get(h.path.as_str()) else {
            return Err(invalid(format!("«{}» è nell'archivio ma non nel manifesto", h.path)));
        };
        if !done.insert(h.path.clone()) {
            return Err(invalid(format!("«{}» compare due volte nell'archivio", h.path)));
        }
        if h.size != mf.size {
            return Err(invalid(format!("«{}» ha {} byte, il manifesto ne dichiara {}", h.path, h.size, mf.size)));
        }
        let mut body = Hashing::new(r.body());
        on_file(mf, &mut body)?;
        std::io::copy(&mut body, &mut std::io::sink()).map_err(corrupt)?;
        if body.bytes != mf.size || hex(&body.hasher.finalize()) != mf.sha256 {
            return Err(invalid(format!(
                "«{}» non corrisponde alla somma SHA-256 del manifesto: il file è danneggiato o alterato",
                h.path
            )));
        }
    }
    if let Some(missing) = manifest.files.iter().find(|f| !done.contains(&f.path)) {
        return Err(invalid(format!("«{}» è nel manifesto ma manca nell'archivio (archivio troncato?)", missing.path)));
    }
    Ok(manifest)
}

/// Checks an archive completely (manifest, paths, sizes, SHA-256 of every file) without extracting anything.
pub fn inspect(archive: &Path) -> Result<BackupManifest> {
    scan(archive, &mut |_, _| Ok(()))
}

// ----------------------------------------------------------------------------- restore

/// Is the database (or the data folder) in use by another process, such as the daemon? Asks SQLite for the exclusive lock
/// and gives it back at once.
pub fn data_in_use(dirs: &DataDirs) -> Result<bool> {
    let db = dirs.db_file();
    if !db.exists() {
        return Ok(false);
    }
    let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(db_err)?;
    conn.busy_timeout(Duration::ZERO).map_err(db_err)?;
    let probe = conn.execute_batch("PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE; ROLLBACK;");
    match probe {
        Ok(()) => Ok(false),
        Err(rusqlite::Error::SqliteFailure(e, _))
            if matches!(e.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) =>
        {
            Ok(true)
        }
        Err(e) => Err(db_err(e)),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreReport {
    pub manifest: BackupManifest,
    /// The full copy of the data as they were before (secrets included), unless there was nothing to copy.
    pub safety_copy: Option<PathBuf>,
    /// Schema of the database before and after the migration to this build's.
    pub schema_from: i64,
    pub schema_to: i64,
    pub restored: Vec<String>,
}

/// Where each file goes under `root`; refuses a destination that passes through a link.
fn destination(root: &Path, rel: &str) -> Result<PathBuf> {
    let mut p = root.to_path_buf();
    let parts = tar::safe_path(rel)?;
    for (i, part) in parts.iter().enumerate() {
        p.push(part);
        if let Ok(m) = std::fs::symlink_metadata(&p) {
            if i + 1 < parts.len() && !m.is_dir() {
                return Err(Error::Invalid(format!("{}: nel percorso c'è un collegamento o un file dove serve una cartella", p.display())));
            }
        }
    }
    Ok(p)
}

fn move_file(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // Another volume: copy and delete.
    std::fs::copy(from, to)?;
    std::fs::remove_file(from)?;
    Ok(())
}

/// Restores `archive` into the data folder. See the module documentation for what is checked and in which order.
/// Fails with `Conflict` while the database is in use.
pub fn restore(dirs: &DataDirs, archive: &Path) -> Result<RestoreReport> {
    if data_in_use(dirs)? {
        return Err(Error::Conflict(
            "i dati sono in uso (AI Workspace è aperto o il daemon `aiw serve` è in esecuzione): chiudi l'app, ferma il daemon e riprova. Il ripristino non gira a programma acceso perché sostituirebbe il database sotto i suoi piedi"
                .into(),
        ));
    }
    // 1. The manifest alone decides what is refused outright.
    let head = read_manifest(archive)?;
    if head.schema_version > SCHEMA_VERSION {
        return Err(Error::Unsupported(format!(
            "il backup ha il database con lo schema {}, scritto da una versione più recente di AI Workspace Local: questa conosce fino allo schema {SCHEMA_VERSION}. Nulla è stato modificato: aggiorna l'app",
            head.schema_version
        )));
    }

    // 2. Everything is extracted into a staging folder inside the data folder (same volume) and verified there.
    std::fs::create_dir_all(&dirs.root)?;
    let stage = Work::new(&dirs.root, "restore")?;
    let new = stage.path().join("new");
    let manifest = scan(archive, &mut |mf, body| {
        let dest = destination(&new, &mf.path)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = std::fs::File::options().write(true).create_new(true).open(&dest)?;
        std::io::copy(body, &mut f)?;
        if mf.path == "token" || mf.path.starts_with("tls/") {
            restrict(&dest, 0o600);
        }
        Ok(())
    })?;

    // 3. The database, in the staging copy: it must be sound, not newer than this build, and is migrated here: a failure
    // leaves the real data alone.
    let staged_db = new.join(DB_NAME);
    let found = store::schema_version_of(&staged_db)?;
    if found > SCHEMA_VERSION {
        return Err(Error::Unsupported(format!(
            "il database del backup ha lo schema {found}, più recente di quello che questa versione conosce ({SCHEMA_VERSION}). Nulla è stato modificato"
        )));
    }
    {
        let c = Connection::open_with_flags(&staged_db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(db_err)?;
        let check: String = c.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(db_err)?;
        if check != "ok" {
            return Err(Error::Invalid(format!("il database nel backup è danneggiato ({check}). Nulla è stato modificato")));
        }
    }
    let (schema_from, schema_to) = store::migrate_file(&staged_db)?;

    // 4. A full safety copy of what is there now.
    let live = dirs.db_file().exists() || dirs.config_file().exists();
    let safety_copy = if live {
        let backups = dirs.root.join("backups");
        std::fs::create_dir_all(&backups)?;
        restrict(&backups, 0o700);
        let name =
            format!("prima-del-ripristino-{}-{}.tar.gz", chrono::Local::now().format("%Y%m%d-%H%M%S"), &aiw_core::ids::new_id("r")[14..]);
        let path = backups.join(name);
        create(dirs, &BackupOptions { include_secrets: true }, &path)
            .map_err(|e| Error::Other(format!("copia di sicurezza dei dati attuali non riuscita, non ho toccato nulla: {e}")))?;
        Some(path)
    } else {
        None
    };

    // 5. Replace file by file; on a failure put back what was moved aside.
    let undo = stage.path().join("undo");
    std::fs::create_dir_all(&undo)?;
    let mut applied: Vec<(PathBuf, Option<PathBuf>)> = vec![];
    let result = (|| -> Result<()> {
        // Leftovers of the old database's journal would be applied to the new file.
        for ext in ["aiw.db-wal", "aiw.db-shm", "aiw.db-journal"] {
            let p = dirs.root.join(ext);
            if p.exists() {
                let moved = undo.join(format!("{}-{ext}", applied.len()));
                move_file(&p, &moved)?;
                applied.push((p, Some(moved)));
            }
        }
        for mf in &manifest.files {
            let from = destination(&new, &mf.path)?;
            let to = destination(&dirs.root, &mf.path)?;
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let moved = if std::fs::symlink_metadata(&to).is_ok() {
                if to.is_dir() {
                    return Err(Error::Invalid(format!("{} è una cartella: non la sostituisco con un file", to.display())));
                }
                let m = undo.join(format!("{}-file", applied.len()));
                move_file(&to, &m)?;
                Some(m)
            } else {
                None
            };
            applied.push((to.clone(), moved));
            move_file(&from, &to)?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        for (target, moved) in applied.into_iter().rev() {
            let _ = std::fs::remove_file(&target);
            if let Some(m) = moved {
                let _ = move_file(&m, &target);
            }
        }
        return Err(Error::Other(format!("ripristino interrotto, i dati di prima sono stati rimessi a posto: {e}")));
    }
    Ok(RestoreReport { restored: manifest.files.iter().map(|f| f.path.clone()).collect(), manifest, safety_copy, schema_from, schema_to })
}

// ----------------------------------------------------------------------------- the engine's side

impl Engine {
    /// The data folder of this engine.
    pub fn data_dirs(&self) -> &DataDirs {
        &self.0.dirs
    }

    /// Makes a backup at `out` and notes it in the activity log (never its content).
    pub fn backup_create(&self, opts: &BackupOptions, out: &Path) -> Result<BackupManifest> {
        let m = create(&self.0.dirs, opts, out)?;
        self.audit_user(
            "backup.create",
            &format!("{} file, {} byte{}", m.files.len(), m.total_bytes(), if m.sensitive { ", con i segreti" } else { "" }),
        );
        Ok(m)
    }
}

#[cfg(test)]
mod tests;
