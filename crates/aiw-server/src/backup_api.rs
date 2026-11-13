//! Backup and diagnostics routes. Owner scope only (`scopes.rs`): a backup holds the person's chats and, if asked, their
//! secrets, and diagnostics describe the computer.
//!
//! There is **no restore route**. Restoring replaces the database and the settings the running app holds open; doing that
//! from inside the process would leave it writing the old state back over the restored one. It is `aiw backup restore`, with
//! the app closed (see `aiw_engine::backup`). The backup travels as base64 in JSON because the app's IPC carries JSON only.

use super::*;
use aiw_engine::backup::{BackupManifest, BackupOptions};
use aiw_engine::store::SCHEMA_VERSION;
use base64::Engine as _;
use serde::Serialize;

/// The largest backup the API hands over (the content travels as base64 in one JSON body, in memory at both ends). A larger
/// one is made with `aiw backup create`, which writes a file.
pub(crate) const API_BACKUP_MAX_BYTES: u64 = 64 << 20;
/// Request body of `POST /api/backup/inspect`: the archive as base64, plus the JSON around it.
pub(crate) const INSPECT_BODY_LIMIT: usize = API_BACKUP_MAX_BYTES as usize / 3 * 4 + (1 << 20);

#[derive(Deserialize, Default)]
pub(crate) struct BackupBody {
    /// Also save the secrets (the archive is then marked sensitive). Models are never saved.
    #[serde(default)]
    include_secrets: bool,
}

#[derive(Serialize)]
pub(crate) struct BackupFile {
    file_name: String,
    size: u64,
    sensitive: bool,
    manifest: BackupManifest,
    /// The `.tar.gz`, base64.
    data: String,
}

#[derive(Deserialize)]
pub(crate) struct InspectBody {
    /// The `.tar.gz`, base64.
    data: String,
}

#[derive(Serialize)]
pub(crate) struct InspectResult {
    manifest: BackupManifest,
    /// Every file matched its size and SHA-256 and every path was safe (otherwise this is an error, not a result).
    verified: bool,
    /// The schema this version of the app writes; a backup with a higher one cannot be restored here.
    current_schema_version: i64,
    restorable_here: bool,
}

fn scratch(e: &Engine, label: &str) -> std::path::PathBuf {
    e.data_dirs().root.join(format!(".backup-{label}-{}.tar.gz", aiw_core::ids::new_id("api")))
}

struct Cleanup(std::path::PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub(crate) async fn create(State(s): State<AppState>, Json(body): Json<BackupBody>) -> ApiResult<BackupFile> {
    let e = s.engine.clone();
    let out = tokio::task::spawn_blocking(move || -> Result<BackupFile, Error> {
        let path = scratch(&e, "out");
        let _gone = Cleanup(path.clone());
        let manifest = e.backup_create(&BackupOptions { include_secrets: body.include_secrets }, &path)?;
        let size = std::fs::metadata(&path)?.len();
        if size > API_BACKUP_MAX_BYTES {
            return Err(Error::Unsupported(format!(
                "il backup pesa {} MB: è troppo grande per passare dall'app (il limite è {} MB). Crealo da terminale con `aiw backup create`, che scrive un file",
                size >> 20,
                API_BACKUP_MAX_BYTES >> 20
            )));
        }
        let data = base64::engine::general_purpose::STANDARD.encode(std::fs::read(&path)?);
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let file_name = format!("aiw-backup-{stamp}{}.tar.gz", if manifest.sensitive { "-con-segreti" } else { "" });
        Ok(BackupFile { file_name, size, sensitive: manifest.sensitive, manifest, data })
    })
    .await
    .map_err(Error::other)??;
    ok(out)
}

pub(crate) async fn inspect(State(s): State<AppState>, Json(body): Json<InspectBody>) -> ApiResult<InspectResult> {
    let e = s.engine.clone();
    let out = tokio::task::spawn_blocking(move || -> Result<InspectResult, Error> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(body.data.trim())
            .map_err(|_| Error::Invalid("il file non è base64 valido".into()))?;
        let path = scratch(&e, "in");
        let _gone = Cleanup(path.clone());
        std::fs::write(&path, bytes)?;
        let manifest = aiw_engine::backup::inspect(&path)?;
        Ok(InspectResult {
            restorable_here: manifest.schema_version <= SCHEMA_VERSION,
            manifest,
            verified: true,
            current_schema_version: SCHEMA_VERSION,
        })
    })
    .await
    .map_err(Error::other)??;
    ok(out)
}

pub(crate) async fn diagnostics(State(s): State<AppState>) -> ApiResult<Value> {
    ok(s.engine.diagnostics().await?)
}
