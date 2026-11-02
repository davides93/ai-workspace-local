//! The one downloader of every provider (docs/architecture/model-hub.md §5): multi-file jobs in a
//! queue with at most two running, `.part` files resumed with `Range`, streaming sha256, free
//! space check, retries with backoff, immediate cancellation, credentials that never leave the
//! provider's host.

use crate::hub::{net_error, net_why, same_origin, FileJob};
use crate::relpath::{is_plain_component, to_logical};
use crate::sidecar::{self, Install, SidecarFile};
use aiw_core::api::DownloadProgress;
use aiw_core::{ids::new_id, now_rfc3339, Error, Result};
use futures::StreamExt;
use reqwest::header::{CONTENT_RANGE, LOCATION, RANGE};
use reqwest::{Response, StatusCode, Url};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Everything the downloader needs to fetch one variant and record where it came from.
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub provider: String,
    /// Shown to the UI (`DownloadProgress.model_id`): the catalog id, or `provider:model`.
    pub model_id: String,
    pub model_ref: String,
    pub variant: String,
    pub revision: String,
    pub format: String,
    pub quant: Option<String>,
    /// Folder of the model, relative to the models directory (holds `.aiw-model.json`): a native
    /// path, one component per level.
    pub folder: PathBuf,
    pub files: Vec<FileJob>,
}

#[derive(Debug, Clone)]
pub struct DownloadOptions {
    /// Jobs running at once; the others wait as `queued`.
    pub max_concurrent: usize,
    /// One retry per delay on network errors and 5xx.
    pub retry_delays: Vec<Duration>,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    /// Free space kept on top of the download: `pct` percent plus `bytes`.
    pub space_margin_pct: u64,
    pub space_margin_bytes: u64,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            max_concurrent: 2,
            retry_delays: vec![Duration::from_secs(1), Duration::from_secs(4), Duration::from_secs(10)],
            connect_timeout: Duration::from_secs(15),
            read_timeout: Duration::from_secs(60),
            space_margin_pct: 5,
            space_margin_bytes: 512 << 20,
        }
    }
}

struct Item {
    p: DownloadProgress,
    cancel: CancellationToken,
    dests: Vec<PathBuf>,
}

struct Inner {
    dir: PathBuf,
    http: reqwest::Client,
    opts: DownloadOptions,
    permits: Arc<Semaphore>,
    items: Mutex<Vec<Item>>,
    completed: AtomicU64,
}

#[derive(Clone)]
pub struct DownloadManager {
    inner: Arc<Inner>,
}

fn active(status: &str) -> bool {
    matches!(status, "queued" | "running" | "verifying")
}

enum FileErr {
    /// Worth another attempt (network, 5xx): the `.part` is kept for a resume.
    Retry(Error),
    Fatal(Error),
}

impl From<Error> for FileErr {
    fn from(e: Error) -> Self {
        FileErr::Fatal(e)
    }
}

fn io_err(e: std::io::Error) -> FileErr {
    if e.kind() == std::io::ErrorKind::StorageFull {
        FileErr::Fatal(Error::InsufficientStorage("spazio su disco esaurito durante il download".into()))
    } else {
        FileErr::Fatal(Error::Io(e))
    }
}

/// A relative path made only of plain components (no `..`, no root, no drive, no stream suffix),
/// rebuilt component by component so that it is nested folders on every OS.
pub fn safe_rel(p: &Path) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => {
                let s = s.to_string_lossy();
                if !is_plain_component(&s) || s == sidecar::SIDECAR {
                    return Err(Error::Invalid(format!("percorso di file non valido: {}", p.display())));
                }
                out.push(&*s);
            }
            _ => return Err(Error::Invalid(format!("percorso di file non valido: {}", p.display()))),
        }
    }
    if out.as_os_str().is_empty() {
        return Err(Error::Invalid("percorso di file vuoto".into()));
    }
    Ok(out)
}

fn part_path(dest: &Path) -> PathBuf {
    let mut n = dest.file_name().map(|s| s.to_os_string()).unwrap_or_default();
    n.push(".part");
    dest.with_file_name(n)
}

fn provider_label(p: &str) -> &'static str {
    match p {
        "hf" => "Hugging Face",
        "modelscope" => "ModelScope",
        "ollama" => "il registro Ollama",
        _ => "il server",
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Free bytes on the volume that holds `path` (the disk whose mount point is the longest prefix).
pub fn free_space(path: &Path) -> Option<u64> {
    let canon = std::fs::canonicalize(path).ok()?;
    #[cfg(not(target_os = "ios"))]
    {
        let s = canon.to_string_lossy();
        let canon = PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string());
        let disks = sysinfo::Disks::new_with_refreshed_list();
        disks
            .list()
            .iter()
            .filter(|d| canon.starts_with(d.mount_point()))
            .max_by_key(|d| d.mount_point().as_os_str().len())
            .map(|d| d.available_space())
    }
    #[cfg(target_os = "ios")]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(canon.as_os_str().as_bytes()).ok()?;
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        (unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0).then(|| s.f_bavail as u64 * s.f_frsize as u64)
    }
}

/// Moving average of the transfer rate.
struct Meter {
    last: Instant,
    last_bytes: u64,
    bps: f64,
}

impl Meter {
    fn new(bytes: u64) -> Self {
        Self { last: Instant::now(), last_bytes: bytes, bps: 0.0 }
    }
    fn tick(&mut self, bytes: u64) -> u64 {
        let dt = self.last.elapsed().as_secs_f64();
        if dt > 0.0 {
            let inst = bytes.saturating_sub(self.last_bytes) as f64 / dt;
            self.bps = if self.bps == 0.0 { inst } else { self.bps * 0.7 + inst * 0.3 };
            self.last = Instant::now();
            self.last_bytes = bytes;
        }
        self.bps as u64
    }
}

/// Aggregated progress of the files of one job.
struct Tally {
    /// Size of each file (0 until known).
    sizes: Vec<u64>,
    /// Bytes of the files that are complete.
    done: u64,
}

impl Tally {
    fn total(&self) -> u64 {
        self.sizes.iter().sum()
    }
}

impl DownloadManager {
    pub fn new(dir: PathBuf) -> Self {
        Self::with_options(dir, DownloadOptions::default())
    }

    pub fn with_options(dir: PathBuf, opts: DownloadOptions) -> Self {
        // Redirects are followed by hand: credentials must not follow them to another host.
        let http = reqwest::Client::builder()
            .user_agent(concat!("ai-workspace-local/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(opts.connect_timeout)
            .read_timeout(opts.read_timeout)
            .build()
            .expect("http client");
        let permits = Arc::new(Semaphore::new(opts.max_concurrent.max(1)));
        Self { inner: Arc::new(Inner { dir, http, opts, permits, items: Default::default(), completed: AtomicU64::new(0) }) }
    }

    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    pub fn list(&self) -> Vec<DownloadProgress> {
        self.inner.items.lock().unwrap().iter().map(|i| i.p.clone()).collect()
    }

    pub fn get(&self, id: &str) -> Option<DownloadProgress> {
        self.inner.items.lock().unwrap().iter().find(|i| i.p.id == id).map(|i| i.p.clone())
    }

    /// How many jobs have reached `done` so far: the engine rescans when this changes.
    pub fn completed(&self) -> u64 {
        self.inner.completed.load(Ordering::SeqCst)
    }

    /// Applies `f` unless the job already ended (a cancelled job never turns into an error).
    fn modify(&self, id: &str, f: impl FnOnce(&mut DownloadProgress)) -> bool {
        let mut items = self.inner.items.lock().unwrap();
        match items.iter_mut().find(|i| i.p.id == id && active(&i.p.status)) {
            Some(i) => {
                f(&mut i.p);
                true
            }
            None => false,
        }
    }

    /// Queues a job; returns immediately. The same variant already in flight is returned as is.
    pub fn enqueue(&self, req: DownloadRequest) -> Result<DownloadProgress> {
        if req.files.is_empty() {
            return Err(Error::Invalid("nessun file da scaricare".into()));
        }
        let folder = safe_rel(&req.folder)?;
        let mut dests = Vec::new();
        for f in &req.files {
            let rel = safe_rel(&f.dest)?;
            if !rel.starts_with(&folder) {
                return Err(Error::Invalid(format!("il file {} è fuori dalla cartella del modello", rel.display())));
            }
            dests.push(rel);
        }
        let mut items = self.inner.items.lock().unwrap();
        if let Some(i) = items
            .iter()
            .find(|i| active(&i.p.status) && i.p.provider == req.provider && i.p.model_ref == req.model_ref && i.p.variant == req.variant)
        {
            return Ok(i.p.clone());
        }
        if items.iter().any(|i| active(&i.p.status) && i.dests.iter().any(|d| dests.contains(d))) {
            return Err(Error::Conflict("un altro download sta già scrivendo gli stessi file".into()));
        }
        let p = DownloadProgress {
            id: new_id("dl"),
            model_id: req.model_id.clone(),
            file: dests[0].file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            received: 0,
            total: req.files.iter().map(|f| f.size).sum(),
            status: "queued".into(),
            error: None,
            provider: req.provider.clone(),
            model_ref: req.model_ref.clone(),
            variant: req.variant.clone(),
            files_total: req.files.len() as u32,
            files_done: 0,
            speed_bps: 0,
            dest: self.inner.dir.join(&folder).display().to_string(),
        };
        let cancel = CancellationToken::new();
        items.insert(0, Item { p: p.clone(), cancel: cancel.clone(), dests });
        drop(items);
        let this = self.clone();
        let id = p.id.clone();
        tokio::spawn(async move { this.run(id, req, cancel).await });
        Ok(p)
    }

    /// Stops a queued or running job at once; the `.part` files stay for a later resume.
    pub fn cancel(&self, id: &str) -> Result<DownloadProgress> {
        let mut items = self.inner.items.lock().unwrap();
        let i = items.iter_mut().find(|i| i.p.id == id).ok_or_else(|| Error::NotFound(format!("download {id} non trovato")))?;
        if active(&i.p.status) {
            i.cancel.cancel();
            i.p.status = "cancelled".into();
            i.p.speed_bps = 0;
        }
        Ok(i.p.clone())
    }

    /// Removes a finished job from the list.
    pub fn clear(&self, id: &str) -> Result<()> {
        let mut items = self.inner.items.lock().unwrap();
        let pos = items.iter().position(|i| i.p.id == id).ok_or_else(|| Error::NotFound(format!("download {id} non trovato")))?;
        if active(&items[pos].p.status) {
            return Err(Error::Conflict("il download è ancora in corso: annullalo prima di toglierlo dalla lista".into()));
        }
        items.remove(pos);
        Ok(())
    }

    async fn run(self, id: String, req: DownloadRequest, cancel: CancellationToken) {
        let permit = tokio::select! {
            p = self.inner.permits.clone().acquire_owned() => p.ok(),
            _ = cancel.cancelled() => None,
        };
        let Some(_permit) = permit else { return };
        if !self.modify(&id, |p| p.status = "running".into()) {
            return;
        }
        match self.execute(&id, &req, &cancel).await {
            Ok(()) => {
                if self.modify(&id, |p| {
                    p.status = "done".into();
                    p.speed_bps = 0;
                    p.files_done = p.files_total;
                    p.total = p.total.max(p.received);
                    p.error = None;
                }) {
                    self.inner.completed.fetch_add(1, Ordering::SeqCst);
                    tracing::info!(model = %req.model_ref, variant = %req.variant, "download complete");
                }
            }
            Err(Error::Cancelled) => {
                self.modify(&id, |p| {
                    p.status = "cancelled".into();
                    p.speed_bps = 0;
                });
            }
            Err(e) => {
                tracing::warn!(model = %req.model_ref, "download failed: {e}");
                self.modify(&id, |p| {
                    p.status = "error".into();
                    p.speed_bps = 0;
                    p.error = Some(e.to_string());
                });
            }
        }
    }

    async fn execute(&self, id: &str, req: &DownloadRequest, cancel: &CancellationToken) -> Result<()> {
        let dir = &self.inner.dir;
        tokio::fs::create_dir_all(dir).await?;
        let jobs: Vec<(PathBuf, &FileJob)> = req.files.iter().map(|f| Ok((dir.join(safe_rel(&f.dest)?), f))).collect::<Result<_>>()?;
        self.check_space(self.missing_bytes(&jobs).await).await?;

        let mut tally = Tally { sizes: jobs.iter().map(|(_, j)| j.size).collect(), done: 0 };
        let mut results: Vec<SidecarFile> = Vec::new();
        let folder = safe_rel(&req.folder)?;
        for (i, (dest, job)) in jobs.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            // The name recorded in the sidecar and shown to the UI is a logical path: `/` on every OS.
            let rel = dest
                .strip_prefix(dir)
                .ok()
                .and_then(|r| r.strip_prefix(&folder).ok())
                .map(to_logical)
                .ok_or_else(|| Error::Invalid(format!("il file {} è fuori dalla cartella del modello", dest.display())))?;
            self.modify(id, |p| {
                p.file = rel.clone();
                p.status = "running".into();
            });
            let (size, sha) = self.download_file(id, req, i, &mut tally, job, dest, cancel).await?;
            tally.done += size;
            tally.sizes[i] = size;
            let (done, total) = (tally.done, tally.total());
            self.modify(id, |p| {
                p.files_done = i as u32 + 1;
                p.received = done;
                p.total = total;
            });
            results.push(SidecarFile { path: rel, size_bytes: size, sha256: sha.or_else(|| job.sha256.clone()) });
        }
        self.modify(id, |p| p.status = "verifying".into());
        let install = Install {
            provider: req.provider.clone(),
            model: req.model_ref.clone(),
            variant: req.variant.clone(),
            revision: req.revision.clone(),
            format: req.format.clone(),
            quant: req.quant.clone(),
            files: results,
            installed_at: now_rfc3339(),
        };
        let model_dir = dir.join(&folder);
        tokio::task::spawn_blocking(move || sidecar::upsert(&model_dir, install)).await.map_err(Error::other)??;
        Ok(())
    }

    /// Bytes still to fetch: the known sizes minus what is already on disk.
    async fn missing_bytes(&self, jobs: &[(PathBuf, &FileJob)]) -> u64 {
        let mut missing = 0u64;
        for (dest, job) in jobs {
            let have = file_len(dest).await.max(file_len(&part_path(dest)).await);
            missing += job.size.saturating_sub(have);
        }
        missing
    }

    /// Fails with `InsufficientStorage` when the volume cannot hold what `req` still needs, so
    /// the caller can refuse a download before queueing it.
    pub async fn preflight(&self, req: &DownloadRequest) -> Result<()> {
        let jobs: Vec<(PathBuf, &FileJob)> =
            req.files.iter().map(|f| Ok((self.inner.dir.join(safe_rel(&f.dest)?), f))).collect::<Result<_>>()?;
        tokio::fs::create_dir_all(&self.inner.dir).await?;
        self.check_space(self.missing_bytes(&jobs).await).await
    }

    async fn check_space(&self, needed: u64) -> Result<()> {
        if needed == 0 {
            return Ok(());
        }
        let dir = self.inner.dir.clone();
        let Some(free) = tokio::task::spawn_blocking(move || free_space(&dir)).await.ok().flatten() else { return Ok(()) };
        let o = &self.inner.opts;
        let need = needed.saturating_add(needed / 100 * o.space_margin_pct).saturating_add(o.space_margin_bytes);
        if free < need {
            let gb = |b: u64| format!("{:.1} GB", b as f64 / (1u64 << 30) as f64);
            return Err(Error::InsufficientStorage(format!(
                "spazio insufficiente: servono {} (margine incluso) e ne restano {} liberi nella cartella dei modelli",
                gb(need),
                gb(free)
            )));
        }
        Ok(())
    }

    /// Downloads one file with retries; returns its final size and sha256 (when computed).
    #[allow(clippy::too_many_arguments)]
    async fn download_file(
        &self,
        id: &str,
        req: &DownloadRequest,
        index: usize,
        tally: &mut Tally,
        job: &FileJob,
        dest: &Path,
        cancel: &CancellationToken,
    ) -> Result<(u64, Option<String>)> {
        // Already installed (an earlier run, or the same file shared by two variants).
        if let Ok(m) = tokio::fs::metadata(dest).await {
            if m.is_file() && (job.size == 0 || m.len() == job.size) {
                return Ok((m.len(), None));
            }
            if m.is_file() {
                tokio::fs::remove_file(dest).await?;
            }
        }
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let part = part_path(dest);
        let mut attempt = 0;
        loop {
            match self.try_file(id, req, index, tally, job, dest, &part, cancel).await {
                Ok(r) => return Ok(r),
                Err(FileErr::Fatal(e)) => return Err(e),
                Err(FileErr::Retry(e)) => {
                    let Some(delay) = self.inner.opts.retry_delays.get(attempt).copied() else { return Err(e) };
                    attempt += 1;
                    tracing::warn!(url = %job.url, attempt, "download interrupted ({e}); retrying in {delay:?}");
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = cancel.cancelled() => return Err(Error::Cancelled),
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn try_file(
        &self,
        id: &str,
        req: &DownloadRequest,
        index: usize,
        tally: &mut Tally,
        job: &FileJob,
        dest: &Path,
        part: &Path,
        cancel: &CancellationToken,
    ) -> std::result::Result<(u64, Option<String>), FileErr> {
        let name = dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut have = file_len(part).await;
        if job.size > 0 && have > job.size {
            let _ = tokio::fs::remove_file(part).await;
            have = 0;
        }
        // A `.part` that already has every byte needs no request at all.
        if have > 0 && job.size > 0 && have == job.size {
            return self.finish(id, job, &name, dest, part, have, None, cancel).await;
        }
        let resp = self.request(req, job, (have > 0).then_some(have), cancel).await?;
        let status = resp.status();
        if status == StatusCode::RANGE_NOT_SATISFIABLE {
            let total = resp
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .and_then(|v| v.trim().parse::<u64>().ok());
            let expected = total.or((job.size > 0).then_some(job.size));
            if expected == Some(have) {
                return self.finish(id, job, &name, dest, part, have, None, cancel).await;
            }
            // The `.part` does not belong to this file any more: start over.
            let _ = tokio::fs::remove_file(part).await;
            return Err(FileErr::Retry(Error::Runtime(
                "il file parziale non corrisponde al file sul server (416): riparto da capo".into(),
            )));
        }
        let range = resp.headers().get(CONTENT_RANGE).and_then(|v| v.to_str().ok()).and_then(parse_content_range);
        let resumed = status == StatusCode::PARTIAL_CONTENT && have > 0;
        if status == StatusCode::PARTIAL_CONTENT && range.map(|r| r.0) != Some(if resumed { have } else { 0 }) {
            let _ = tokio::fs::remove_file(part).await;
            return Err(FileErr::Retry(Error::Runtime("risposta Range incoerente: riparto da capo".into())));
        }
        let start = if resumed { have } else { 0 };
        let file_total = range.and_then(|r| r.1).or_else(|| resp.content_length().map(|l| l + start));
        if let Some(t) = file_total {
            if job.size > 0 && t != job.size {
                let _ = tokio::fs::remove_file(part).await;
                return Err(FileErr::Fatal(Error::Runtime(format!(
                    "{name}: il server dichiara {t} byte invece dei {} attesi: il file potrebbe essere cambiato",
                    job.size
                ))));
            }
            if job.size == 0 {
                self.check_space(t.saturating_sub(start)).await?;
            }
            tally.sizes[index] = t;
            let total = tally.total();
            self.modify(id, |p| p.total = total);
        }
        let expected = if job.size > 0 { Some(job.size) } else { file_total };

        let mut hasher = Sha256::new();
        let mut f = if resumed {
            // Hash what is already on disk, then append.
            self.modify(id, |p| p.status = "verifying".into());
            hash_prefix(part, have, &mut hasher, cancel).await?;
            self.modify(id, |p| p.status = "running".into());
            tokio::fs::OpenOptions::new().append(true).open(part).await.map_err(io_err)?
        } else {
            tokio::fs::OpenOptions::new().create(true).write(true).truncate(true).open(part).await.map_err(io_err)?
        };
        let mut w = tokio::io::BufWriter::with_capacity(1 << 20, &mut f);
        let mut stream = resp.bytes_stream();
        let mut written = start;
        let mut meter = Meter::new(tally.done + written);
        let mut last_report = Instant::now();
        loop {
            let next = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    let _ = w.flush().await;
                    return Err(FileErr::Fatal(Error::Cancelled));
                }
                n = stream.next() => n,
            };
            match next {
                Some(Ok(chunk)) => {
                    hasher.update(&chunk);
                    w.write_all(&chunk).await.map_err(io_err)?;
                    written += chunk.len() as u64;
                    if last_report.elapsed() >= Duration::from_millis(200) {
                        last_report = Instant::now();
                        // Keep the `.part` close to what was received, so a crash loses little.
                        w.flush().await.map_err(io_err)?;
                        let (received, total) = (tally.done + written, tally.total().max(tally.done + written));
                        let bps = meter.tick(received);
                        self.modify(id, |p| {
                            p.received = received;
                            p.total = total;
                            p.speed_bps = bps;
                        });
                    }
                }
                Some(Err(e)) => {
                    let _ = w.flush().await;
                    return Err(FileErr::Retry(Error::Upstream(format!(
                        "connessione interrotta durante il download di {name}: {}",
                        net_why(&e)
                    ))));
                }
                None => break,
            }
        }
        w.flush().await.map_err(io_err)?;
        drop(w);
        drop(f);
        if let Some(exp) = expected {
            if written < exp {
                return Err(FileErr::Retry(Error::Upstream(format!("download di {name} incompleto: ricevuti {written} byte su {exp}"))));
            }
            if written > exp {
                let _ = tokio::fs::remove_file(part).await;
                return Err(FileErr::Fatal(Error::Runtime(format!("{name}: ricevuti più byte del previsto ({written} su {exp})"))));
            }
        }
        self.finish(id, job, &name, dest, part, written, Some(hex(&hasher.finalize())), cancel).await
    }

    /// Verifies a complete `.part` (hashing it when the hash was not computed on the way) and moves it in place.
    #[allow(clippy::too_many_arguments)]
    async fn finish(
        &self,
        id: &str,
        job: &FileJob,
        name: &str,
        dest: &Path,
        part: &Path,
        size: u64,
        sha: Option<String>,
        cancel: &CancellationToken,
    ) -> std::result::Result<(u64, Option<String>), FileErr> {
        self.modify(id, |p| p.status = "verifying".into());
        let sha = match sha {
            Some(s) => s,
            None => {
                let mut h = Sha256::new();
                hash_prefix(part, size, &mut h, cancel).await?;
                hex(&h.finalize())
            }
        };
        if let Some(exp) = &job.sha256 {
            if !exp.eq_ignore_ascii_case(&sha) {
                let _ = tokio::fs::remove_file(part).await;
                return Err(FileErr::Fatal(Error::Runtime(format!(
                    "checksum sha256 non corrispondente per {name}: il file è stato scartato"
                ))));
            }
        }
        if cfg!(windows) {
            let _ = tokio::fs::remove_file(dest).await;
        }
        tokio::fs::rename(part, dest).await.map_err(io_err)?;
        self.modify(id, |p| p.status = "running".into());
        Ok((size, Some(sha)))
    }

    /// GET with redirects followed by hand: the provider's headers go only to its own host.
    async fn request(
        &self,
        req: &DownloadRequest,
        job: &FileJob,
        range_from: Option<u64>,
        cancel: &CancellationToken,
    ) -> std::result::Result<Response, FileErr> {
        let origin = Url::parse(&job.url).map_err(|_| FileErr::Fatal(Error::Invalid(format!("URL non valido: {}", job.url))))?;
        let mut url = origin.clone();
        let mut own_host = true;
        for _ in 0..10 {
            let mut rb = self.inner.http.get(url.clone()).header("accept-encoding", "identity");
            if own_host {
                for (k, v) in &job.headers {
                    rb = rb.header(k, v);
                }
            }
            if let Some(from) = range_from {
                rb = rb.header(RANGE, format!("bytes={from}-"));
            }
            let resp = tokio::select! {
                r = rb.send() => r.map_err(|e| FileErr::Retry(net_error(url.host_str().unwrap_or("il server"), &e)))?,
                _ = cancel.cancelled() => return Err(FileErr::Fatal(Error::Cancelled)),
            };
            let st = resp.status();
            if st.is_redirection() {
                if let Some(loc) = resp.headers().get(LOCATION).and_then(|v| v.to_str().ok()) {
                    let next = url.join(loc).map_err(|_| FileErr::Fatal(Error::Upstream(format!("reindirizzamento non valido: {loc}"))))?;
                    if !same_origin(origin.as_str(), next.as_str()) {
                        own_host = false;
                    }
                    url = next;
                    continue;
                }
            }
            return match st.as_u16() {
                200..=299 | 416 => Ok(resp),
                401 => Err(FileErr::Fatal(Error::Unauthorized(match req.provider.as_str() {
                    "hf" => {
                        "repository gated o privato (401): serve un token Hugging Face valido, impostalo nelle impostazioni del provider"
                            .to_string()
                    }
                    p => format!("accesso negato (401): {} richiede credenziali valide", provider_label(p)),
                }))),
                403 => Err(FileErr::Fatal(Error::Denied(match req.provider.as_str() {
                    "hf" => "repository gated (403): serve un token Hugging Face con accesso accettato al modello".to_string(),
                    p => format!("accesso vietato (403) da {}", provider_label(p)),
                }))),
                404 => Err(FileErr::Fatal(Error::NotFound(format!(
                    "file non trovato (404): {}",
                    job.dest.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
                )))),
                408 | 429 | 500..=599 => {
                    Err(FileErr::Retry(Error::Upstream(format!("{} ha risposto HTTP {st}", provider_label(&req.provider)))))
                }
                _ => Err(FileErr::Fatal(Error::Upstream(format!("risposta inattesa da {}: HTTP {st}", provider_label(&req.provider))))),
            };
        }
        Err(FileErr::Fatal(Error::Upstream("troppi reindirizzamenti".into())))
    }
}

async fn file_len(p: &Path) -> u64 {
    tokio::fs::metadata(p).await.map(|m| m.len()).unwrap_or(0)
}

/// `bytes 100-199/200` → `(100, Some(200))`.
fn parse_content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let rest = v.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    Some((start, total.trim().parse().ok()))
}

/// Feeds the first `len` bytes of `path` to `hasher`, stopping at once when cancelled.
async fn hash_prefix(path: &Path, len: u64, hasher: &mut Sha256, cancel: &CancellationToken) -> std::result::Result<(), FileErr> {
    let mut f = tokio::fs::File::open(path).await.map_err(io_err)?;
    let mut buf = vec![0u8; 1 << 20];
    let mut left = len;
    while left > 0 {
        if cancel.is_cancelled() {
            return Err(FileErr::Fatal(Error::Cancelled));
        }
        let want = buf.len().min(left as usize);
        let n = f.read(&mut buf[..want]).await.map_err(io_err)?;
        if n == 0 {
            return Err(FileErr::Fatal(Error::Runtime(format!("il file parziale {} è più corto del previsto", path.display()))));
        }
        hasher.update(&buf[..n]);
        left -= n as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::hf::fake::{serve_bytes, sha};
    use axum::extract::{Path as AxPath, State};
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::get;
    use axum::Router;
    use std::collections::HashMap;

    type Hit = (String, Option<String>, Option<String>);

    #[derive(Clone, Default)]
    struct Srv {
        files: Arc<HashMap<String, Vec<u8>>>,
        /// `(path, range header, authorization header)`
        log: Arc<Mutex<Vec<Hit>>>,
        fail_first: Arc<Mutex<HashMap<String, u32>>>,
        ignore_range: bool,
    }

    fn data(n: usize, seed: u8) -> Vec<u8> {
        (0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
    }

    async fn handle(State(s): State<Srv>, AxPath(name): AxPath<String>, h: HeaderMap) -> Response {
        s.log.lock().unwrap().push((
            name.clone(),
            h.get("range").and_then(|v| v.to_str().ok()).map(String::from),
            h.get("authorization").and_then(|v| v.to_str().ok()).map(String::from),
        ));
        {
            let mut ff = s.fail_first.lock().unwrap();
            if let Some(n) = ff.get_mut(&name) {
                if *n > 0 {
                    *n -= 1;
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
            }
        }
        match name.as_str() {
            "unauthorized" => return StatusCode::UNAUTHORIZED.into_response(),
            "forbidden" => return StatusCode::FORBIDDEN.into_response(),
            _ => {}
        }
        match s.files.get(&name) {
            Some(b) if s.ignore_range => (StatusCode::OK, b.clone()).into_response(),
            Some(b) => serve_bytes(b, &h),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }

    /// First 1000 bytes, then silence: for cancellation and queue tests.
    async fn stall() -> Response {
        let stream = futures::stream::unfold(0u8, |i| async move {
            if i == 0 {
                Some((Ok::<_, std::io::Error>(bytes_of(1000)), 1))
            } else {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                None
            }
        });
        Response::builder().header("content-length", "100000").body(axum::body::Body::from_stream(stream)).unwrap()
    }

    fn bytes_of(n: usize) -> axum::body::Bytes {
        axum::body::Bytes::from(vec![1u8; n])
    }

    async fn serve(s: Srv) -> String {
        let app = Router::new().route("/stall", get(stall)).route("/f/{name}", get(handle)).with_state(s);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}")
    }

    fn srv(files: &[(&str, Vec<u8>)]) -> Srv {
        Srv { files: Arc::new(files.iter().map(|(n, b)| (n.to_string(), b.clone())).collect()), ..Default::default() }
    }

    fn manager(dir: &Path) -> DownloadManager {
        DownloadManager::with_options(
            dir.to_path_buf(),
            DownloadOptions {
                retry_delays: vec![Duration::from_millis(10); 3],
                space_margin_bytes: 0,
                space_margin_pct: 0,
                ..Default::default()
            },
        )
    }

    fn job(base: &str, name: &str, bytes: &[u8], folder: &str) -> FileJob {
        FileJob {
            url: format!("{base}/f/{name}"),
            dest: PathBuf::from(folder).join(name),
            size: bytes.len() as u64,
            sha256: Some(sha(bytes)),
            headers: vec![],
        }
    }

    fn request(provider: &str, folder: &str, files: Vec<FileJob>) -> DownloadRequest {
        DownloadRequest {
            provider: provider.into(),
            model_id: format!("{provider}:acme/m"),
            model_ref: "acme/m".into(),
            variant: "Q4".into(),
            revision: "rev1".into(),
            format: "gguf".into(),
            quant: Some("Q4".into()),
            folder: PathBuf::from(folder),
            files,
        }
    }

    async fn wait_end(m: &DownloadManager, id: &str) -> DownloadProgress {
        for _ in 0..500 {
            let p = m.get(id).unwrap();
            if !active(&p.status) {
                return p;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("download did not finish: {:?}", m.get(id));
    }

    async fn wait_status(m: &DownloadManager, id: &str, status: &str) {
        for _ in 0..500 {
            if m.get(id).unwrap().status == status {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("never reached {status}: {:?}", m.get(id));
    }

    #[test]
    fn destinations_are_nested_folders_and_cannot_escape() {
        let nested = safe_rel(&PathBuf::from("hf").join("acme").join("m").join("Q4_K_M").join("x.gguf")).unwrap();
        assert_eq!(nested.components().count(), 5);
        assert_eq!(to_logical(&nested), "hf/acme/m/Q4_K_M/x.gguf");
        // A remote path is turned into components before it is joined: nested on every OS.
        let from_remote = PathBuf::from("hf/acme/m").join(crate::relpath::native_rel("Q4_K_M/x.gguf").unwrap());
        assert_eq!(safe_rel(&from_remote).unwrap(), nested);
        for bad in ["", ".", "..", "../x.gguf", "a/../../x.gguf", "/etc/passwd", "a/.aiw-model.json", "a:stream/x.gguf", "C:/x.gguf"] {
            assert!(safe_rel(Path::new(bad)).is_err(), "{bad:?} must be refused");
        }
        // A backslash is a separator on Windows and a forbidden character elsewhere: either way
        // it never reaches the disk as part of a name.
        for bad in ["..\\x.gguf", "a\\..\\..\\x.gguf", "\\\\srv\\share\\x.gguf"] {
            assert!(safe_rel(Path::new(bad)).is_err(), "{bad:?} must be refused");
        }
    }

    #[tokio::test]
    async fn downloads_a_multi_file_job_and_writes_the_sidecar() {
        let (a, b) = (data(5000, 1), data(3000, 2));
        let s = srv(&[("a.gguf", a.clone()), ("b.gguf", b.clone())]);
        let base = serve(s.clone()).await;
        let d = tempfile::tempdir().unwrap();
        let m = manager(d.path());
        let p = m
            .enqueue(request("hf", "hf/acme/m", vec![job(&base, "a.gguf", &a, "hf/acme/m"), job(&base, "b.gguf", &b, "hf/acme/m/sub")]))
            .unwrap();
        assert_eq!((p.files_total, p.total, p.provider.as_str(), p.model_id.as_str()), (2, 8000, "hf", "hf:acme/m"));
        let p = wait_end(&m, &p.id).await;
        assert_eq!(p.status, "done", "{:?}", p.error);
        assert_eq!((p.files_done, p.received, p.total, p.speed_bps), (2, 8000, 8000, 0));
        assert_eq!(std::fs::read(d.path().join("hf/acme/m/a.gguf")).unwrap(), a);
        assert_eq!(std::fs::read(d.path().join("hf/acme/m/sub/b.gguf")).unwrap(), b);
        assert!(!d.path().join("hf/acme/m/a.gguf.part").exists());
        assert_eq!(m.completed(), 1);
        // `dest` is a real OS path: compared as a path (the OS separator is not '/' everywhere).
        assert_eq!(Path::new(&p.dest), d.path().join("hf").join("acme").join("m"));
        let installs = sidecar::read(&d.path().join("hf/acme/m"));
        assert_eq!(installs.len(), 1);
        let i = &installs[0];
        assert_eq!(
            (i.provider.as_str(), i.model.as_str(), i.variant.as_str(), i.revision.as_str(), i.format.as_str()),
            ("hf", "acme/m", "Q4", "rev1", "gguf")
        );
        assert_eq!(i.files.iter().map(|f| (f.path.as_str(), f.size_bytes)).collect::<Vec<_>>(), [("a.gguf", 5000), ("sub/b.gguf", 3000)]);
        assert_eq!(i.files[0].sha256.as_deref(), Some(sha(&a).as_str()));
        // Asking again while it is done is a no-op download (files already there) and a new job.
        let again = m.enqueue(request("hf", "hf/acme/m", vec![job(&base, "a.gguf", &a, "hf/acme/m")])).unwrap();
        assert_ne!(again.id, p.id);
        assert_eq!(wait_end(&m, &again.id).await.status, "done");
        assert_eq!(s.log.lock().unwrap().len(), 2, "no new request for files already installed");
        // Finished jobs can be cleared, running ones cannot.
        m.clear(&p.id).unwrap();
        assert!(m.get(&p.id).is_none());
        assert!(matches!(m.clear("nope"), Err(Error::NotFound(_))));
    }

    #[tokio::test]
    async fn resumes_a_part_file_with_range_and_verifies_the_whole_file() {
        let a = data(10_000, 3);
        let s = srv(&[("a.gguf", a.clone())]);
        let base = serve(s.clone()).await;
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("hf/acme/m")).unwrap();
        std::fs::write(d.path().join("hf/acme/m/a.gguf.part"), &a[..4096]).unwrap();
        let m = manager(d.path());
        let p = m.enqueue(request("hf", "hf/acme/m", vec![job(&base, "a.gguf", &a, "hf/acme/m")])).unwrap();
        let p = wait_end(&m, &p.id).await;
        assert_eq!(p.status, "done", "{:?}", p.error);
        assert_eq!(std::fs::read(d.path().join("hf/acme/m/a.gguf")).unwrap(), a);
        let log = s.log.lock().unwrap().clone();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].1.as_deref(), Some("bytes=4096-"));
    }

    #[tokio::test]
    async fn restarts_from_scratch_when_the_server_ignores_range() {
        let a = data(6000, 4);
        let mut s = srv(&[("a.gguf", a.clone())]);
        s.ignore_range = true;
        let base = serve(s).await;
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("m")).unwrap();
        std::fs::write(d.path().join("m/a.gguf.part"), vec![0xAAu8; 2000]).unwrap(); // garbage prefix
        let m = manager(d.path());
        let p = m.enqueue(request("url", "m", vec![job(&base, "a.gguf", &a, "m")])).unwrap();
        let p = wait_end(&m, &p.id).await;
        assert_eq!(p.status, "done", "{:?}", p.error);
        assert_eq!(std::fs::read(d.path().join("m/a.gguf")).unwrap(), a);
    }

    #[tokio::test]
    async fn a_416_on_a_complete_part_means_the_file_is_done() {
        let a = data(4000, 5);
        let s = srv(&[("a.gguf", a.clone())]);
        let base = serve(s.clone()).await;
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("m")).unwrap();
        std::fs::write(d.path().join("m/a.gguf.part"), &a).unwrap();
        let m = manager(d.path());
        // Size unknown (0), as for a direct URL: the 416 tells the real size.
        let mut j = job(&base, "a.gguf", &a, "m");
        j.size = 0;
        let p = m.enqueue(request("url", "m", vec![j])).unwrap();
        let p = wait_end(&m, &p.id).await;
        assert_eq!(p.status, "done", "{:?}", p.error);
        assert_eq!(std::fs::read(d.path().join("m/a.gguf")).unwrap(), a);
        assert_eq!(s.log.lock().unwrap()[0].1.as_deref(), Some("bytes=4000-"));
    }

    #[tokio::test]
    async fn a_416_on_a_stale_part_starts_over() {
        let a = data(1000, 6);
        let s = srv(&[("a.gguf", a.clone())]);
        let base = serve(s.clone()).await;
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("m")).unwrap();
        std::fs::write(d.path().join("m/a.gguf.part"), vec![9u8; 1500]).unwrap(); // longer than the real file
        let m = manager(d.path());
        let mut j = job(&base, "a.gguf", &a, "m");
        j.size = 0;
        let p = m.enqueue(request("url", "m", vec![j])).unwrap();
        let p = wait_end(&m, &p.id).await;
        assert_eq!(p.status, "done", "{:?}", p.error);
        assert_eq!(std::fs::read(d.path().join("m/a.gguf")).unwrap(), a);
    }

    #[tokio::test]
    async fn a_checksum_mismatch_discards_the_file_with_a_clear_error() {
        let a = data(2000, 7);
        let base = serve(srv(&[("a.gguf", a.clone())])).await;
        let d = tempfile::tempdir().unwrap();
        let m = manager(d.path());
        let mut j = job(&base, "a.gguf", &a, "m");
        j.sha256 = Some("0".repeat(64));
        let p = m.enqueue(request("hf", "m", vec![j])).unwrap();
        let p = wait_end(&m, &p.id).await;
        assert_eq!(p.status, "error");
        let e = p.error.unwrap();
        assert!(e.contains("sha256") && e.contains("scartato"), "{e}");
        assert!(!d.path().join("m/a.gguf").exists() && !d.path().join("m/a.gguf.part").exists());
        assert!(!d.path().join("m/.aiw-model.json").exists(), "no sidecar for a failed install");
        assert_eq!(m.completed(), 0);
    }

    #[tokio::test]
    async fn a_wrong_size_from_the_server_is_an_error() {
        let a = data(2000, 8);
        let base = serve(srv(&[("a.gguf", a.clone())])).await;
        let d = tempfile::tempdir().unwrap();
        let m = manager(d.path());
        let mut j = job(&base, "a.gguf", &a, "m");
        j.size = 2500;
        let p = m.enqueue(request("hf", "m", vec![j])).unwrap();
        let p = wait_end(&m, &p.id).await;
        assert_eq!(p.status, "error");
        assert!(p.error.unwrap().contains("2500"));
    }

    #[tokio::test]
    async fn network_errors_are_clean_italian_without_the_library_text() {
        // Nothing listens on port 1: the connection is refused on every attempt.
        let d = tempfile::tempdir().unwrap();
        let m = manager(d.path());
        let j =
            FileJob { url: "http://127.0.0.1:1/a.gguf".into(), dest: PathBuf::from("m/a.gguf"), size: 10, sha256: None, headers: vec![] };
        let p = m.enqueue(request("url", "m", vec![j])).unwrap();
        let p = wait_end(&m, &p.id).await;
        assert_eq!(p.status, "error");
        assert_eq!(p.error.as_deref(), Some("impossibile contattare 127.0.0.1: connessione rifiutata o rete assente"));
    }

    #[tokio::test]
    async fn cancel_is_immediate_even_with_a_stalled_connection() {
        let base = serve(Srv::default()).await;
        let d = tempfile::tempdir().unwrap();
        let m = manager(d.path());
        let j = FileJob { url: format!("{base}/stall"), dest: PathBuf::from("m/a.gguf"), size: 100_000, sha256: None, headers: vec![] };
        let p = m.enqueue(request("url", "m", vec![j])).unwrap();
        wait_status(&m, &p.id, "running").await;
        // Let the first chunk arrive.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let t = Instant::now();
        let c = m.cancel(&p.id).unwrap();
        assert_eq!(c.status, "cancelled");
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(t.elapsed() < Duration::from_secs(2));
        let p = m.get(&p.id).unwrap();
        assert_eq!(p.status, "cancelled", "a cancelled job never becomes an error");
        assert!(p.error.is_none());
        assert_eq!(file_len(&d.path().join("m/a.gguf.part")).await, 1000, "the .part stays for a later resume");
        assert!(!d.path().join("m/a.gguf").exists());
        // Cancelling again is harmless.
        assert_eq!(m.cancel(&p.id).unwrap().status, "cancelled");
        assert!(m.clear(&p.id).is_ok());
    }

    #[tokio::test]
    async fn only_two_jobs_run_and_a_queued_one_can_be_cancelled_or_promoted() {
        let base = serve(Srv::default()).await;
        let d = tempfile::tempdir().unwrap();
        let m = manager(d.path());
        let mk = |n: &str| {
            let mut r = request(
                "url",
                &format!("m/{n}"),
                vec![FileJob {
                    url: format!("{base}/stall"),
                    dest: PathBuf::from(format!("m/{n}/a.gguf")),
                    size: 100_000,
                    sha256: None,
                    headers: vec![],
                }],
            );
            r.model_ref = n.into();
            r
        };
        let (a, b, c) = (m.enqueue(mk("a")).unwrap(), m.enqueue(mk("b")).unwrap(), m.enqueue(mk("c")).unwrap());
        assert_eq!(c.status, "queued");
        wait_status(&m, &a.id, "running").await;
        wait_status(&m, &b.id, "running").await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(m.get(&c.id).unwrap().status, "queued", "the third job waits");
        // The same model again is deduplicated; a different model writing the same files conflicts.
        assert_eq!(m.enqueue(mk("a")).unwrap().id, a.id);
        let mut clash = mk("z");
        clash.files[0].dest = PathBuf::from("m/a/a.gguf");
        clash.folder = PathBuf::from("m/a");
        assert!(matches!(m.enqueue(clash), Err(Error::Conflict(_))));
        assert!(m.clear(&a.id).is_err(), "running jobs cannot be cleared");
        m.cancel(&a.id).unwrap();
        wait_status(&m, &c.id, "running").await;
        m.cancel(&b.id).unwrap();
        m.cancel(&c.id).unwrap();
    }

    #[tokio::test]
    async fn auth_errors_are_clear_and_not_retried() {
        let s = srv(&[]);
        let base = serve(s.clone()).await;
        let d = tempfile::tempdir().unwrap();
        let m = manager(d.path());
        let mk = |name: &str, folder: &str| FileJob {
            url: format!("{base}/f/{name}"),
            dest: PathBuf::from(folder).join("a.gguf"),
            size: 10,
            sha256: None,
            headers: vec![],
        };
        let p = m.enqueue(request("hf", "m1", vec![mk("unauthorized", "m1")])).unwrap();
        let e = wait_end(&m, &p.id).await.error.unwrap();
        assert!(e.contains("401") && e.contains("token Hugging Face"), "{e}");
        let mut r = request("hf", "m2", vec![mk("forbidden", "m2")]);
        r.model_ref = "other".into();
        let p = m.enqueue(r).unwrap();
        let e = wait_end(&m, &p.id).await.error.unwrap();
        assert!(e.contains("gated") && e.contains("token Hugging Face"), "{e}");
        let mut r = request("hf", "m3", vec![mk("missing", "m3")]);
        r.model_ref = "third".into();
        let p = m.enqueue(r).unwrap();
        let e = wait_end(&m, &p.id).await.error.unwrap();
        assert!(e.contains("file non trovato") && e.contains("404"), "{e}");
        assert_eq!(s.log.lock().unwrap().len(), 3, "401, 403 and 404 are never retried");
    }

    #[tokio::test]
    async fn retries_5xx_with_backoff_then_succeeds() {
        let a = data(1500, 9);
        let s = srv(&[("a.gguf", a.clone())]);
        s.fail_first.lock().unwrap().insert("a.gguf".into(), 2);
        let base = serve(s.clone()).await;
        let d = tempfile::tempdir().unwrap();
        let m = manager(d.path());
        let p = m.enqueue(request("hf", "m", vec![job(&base, "a.gguf", &a, "m")])).unwrap();
        let p = wait_end(&m, &p.id).await;
        assert_eq!(p.status, "done", "{:?}", p.error);
        assert_eq!(s.log.lock().unwrap().len(), 3);
        // More failures than retries: the error is reported.
        s.fail_first.lock().unwrap().insert("b.gguf".into(), 10);
        let b = data(10, 1);
        let mut r = request("hf", "m2", vec![job(&base, "b.gguf", &b, "m2")]);
        r.model_ref = "b".into();
        let p = m.enqueue(r).unwrap();
        let p = wait_end(&m, &p.id).await;
        assert_eq!(p.status, "error");
        assert!(p.error.unwrap().contains("503"));
    }

    #[tokio::test]
    async fn authorization_is_stripped_when_a_redirect_leaves_the_host() {
        let a = data(3000, 11);
        let cdn = srv(&[("real.gguf", a.clone())]);
        let cdn_base = serve(cdn.clone()).await;
        let origin = srv(&[("same.gguf", a.clone())]);
        // The origin redirects /f/moved.gguf to the CDN (another port: another origin), and
        // /f/loop.gguf to itself.
        let cdn_url = format!("{cdn_base}/f/real.gguf");
        let o_log = origin.log.clone();
        let app = Router::new().route(
            "/f/{name}",
            get({
                let origin = origin.clone();
                let cdn_url = cdn_url.clone();
                move |AxPath(name): AxPath<String>, st: HeaderMap| {
                    let origin = origin.clone();
                    let cdn_url = cdn_url.clone();
                    async move {
                        if name == "moved.gguf" {
                            origin.log.lock().unwrap().push((
                                name,
                                None,
                                st.get("authorization").and_then(|v| v.to_str().ok()).map(String::from),
                            ));
                            return Redirect::temporary(&cdn_url).into_response();
                        }
                        if name == "local.gguf" {
                            origin.log.lock().unwrap().push((
                                name,
                                None,
                                st.get("authorization").and_then(|v| v.to_str().ok()).map(String::from),
                            ));
                            return Redirect::temporary("/f/same.gguf").into_response();
                        }
                        handle(State(origin), AxPath(name), st).await
                    }
                }
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        let base = format!("http://{addr}");

        let d = tempfile::tempdir().unwrap();
        let m = manager(d.path());
        let auth = vec![("Authorization".to_string(), "Bearer hf_secret".to_string())];
        let mut j = job(&base, "moved.gguf", &a, "m");
        j.headers = auth.clone();
        let p = m.enqueue(request("hf", "m", vec![j])).unwrap();
        let p = wait_end(&m, &p.id).await;
        assert_eq!(p.status, "done", "{:?}", p.error);
        assert_eq!(std::fs::read(d.path().join("m/moved.gguf")).unwrap(), a);
        let seen_by_origin = o_log.lock().unwrap().iter().find(|(n, _, _)| n == "moved.gguf").cloned().unwrap();
        assert_eq!(seen_by_origin.2.as_deref(), Some("Bearer hf_secret"), "the provider's own host still gets the token");
        let seen_by_cdn = cdn.log.lock().unwrap().clone();
        assert_eq!(seen_by_cdn.len(), 1);
        assert_eq!(seen_by_cdn[0].2, None, "the CDN must never see Authorization");

        // A redirect inside the same host keeps the header.
        let mut j = job(&base, "local.gguf", &a, "m2");
        j.headers = auth;
        let mut r = request("hf", "m2", vec![j]);
        r.model_ref = "second".into();
        let p = m.enqueue(r).unwrap();
        assert_eq!(wait_end(&m, &p.id).await.status, "done");
        let log = o_log.lock().unwrap().clone();
        assert!(log.iter().any(|(n, _, a)| n == "same.gguf" && a.as_deref() == Some("Bearer hf_secret")), "{log:?}");
    }

    #[tokio::test]
    async fn refuses_when_the_disk_cannot_hold_the_download() {
        let base = serve(Srv::default()).await;
        let d = tempfile::tempdir().unwrap();
        let m = DownloadManager::with_options(d.path().to_path_buf(), DownloadOptions::default());
        let j = FileJob { url: format!("{base}/f/x"), dest: PathBuf::from("m/a.gguf"), size: 1 << 60, sha256: None, headers: vec![] };
        let p = m.enqueue(request("hf", "m", vec![j])).unwrap();
        let p = wait_end(&m, &p.id).await;
        assert_eq!(p.status, "error");
        assert!(p.error.unwrap().contains("spazio insufficiente"));
        assert!(matches!(
            m.preflight(&request(
                "hf",
                "m2",
                vec![FileJob {
                    url: format!("{base}/f/x"),
                    dest: PathBuf::from("m2/a.gguf"),
                    size: 1 << 60,
                    sha256: None,
                    headers: vec![]
                }]
            ))
            .await,
            Err(Error::InsufficientStorage(_))
        ));
        assert!(m
            .preflight(&request(
                "hf",
                "m3",
                vec![FileJob { url: format!("{base}/f/x"), dest: PathBuf::from("m3/a.gguf"), size: 10, sha256: None, headers: vec![] }]
            ))
            .await
            .is_ok());
        assert!(free_space(d.path()).is_some_and(|f| f > 0));
        assert!(free_space(&d.path().join("missing")).is_none());
    }

    #[tokio::test]
    async fn rejects_paths_that_escape_the_models_directory() {
        let d = tempfile::tempdir().unwrap();
        let m = manager(d.path());
        let mk = |dest: &str, folder: &str| {
            request(
                "hf",
                folder,
                vec![FileJob { url: "http://127.0.0.1:1/x".into(), dest: PathBuf::from(dest), size: 1, sha256: None, headers: vec![] }],
            )
        };
        for (dest, folder) in [
            ("../evil.gguf", "../"),
            ("m/../../evil.gguf", "m"),
            ("/etc/evil.gguf", "/etc"),
            ("m/.aiw-model.json", "m"),
            ("other/a.gguf", "m"),
            ("m/a:b.gguf", "m"),
            ("", "m"),
        ] {
            assert!(m.enqueue(mk(dest, folder)).is_err(), "{dest}");
        }
        assert!(m.list().is_empty());
        assert!(m.enqueue(DownloadRequest { files: vec![], ..mk("m/a.gguf", "m") }).is_err());
    }

    #[test]
    fn parses_content_range_and_part_names() {
        assert_eq!(parse_content_range("bytes 100-199/200"), Some((100, Some(200))));
        assert_eq!(parse_content_range("bytes 0-9/*"), Some((0, None)));
        assert_eq!(parse_content_range("bytes */200"), None);
        assert_eq!(part_path(Path::new("a/b/model.gguf")), PathBuf::from("a/b/model.gguf.part"));
        assert_eq!(safe_rel(Path::new("a/./b")).unwrap(), PathBuf::from("a/b"));
    }
}
