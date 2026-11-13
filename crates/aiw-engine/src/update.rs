//! Self-update from the GitHub releases of the public repository, with a choice of channel.
//!
//! - **Channels** come from the release tags: a tag without a pre-release part is `stable`
//!   (production), `-rc.N` is `rc`, `-beta.N` is `beta`, `-alpha.N`/`-dev`/`-nightly` is `alpha`.
//!   A channel also receives every more stable release, and the newest version wins: `beta` sees
//!   betas, release candidates and stable releases.
//! - **Check**: the releases API of GitHub (public, no token).
//! - **Download**: the installer for this OS from the release, verified against the
//!   `SHA256SUMS.txt` published with it before it is ever opened. A file that does not match is
//!   deleted.
//! - **Install**: handed over to the system (open the disk image, run the MSI, replace the
//!   AppImage). The releases are not code-signed yet, so the operating system keeps asking
//!   for its usual confirmation.

use aiw_core::{Error, Result};
use futures::StreamExt;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const DEFAULT_REPO: &str = "davides93/ai-workspace-local";
pub const DEFAULT_API_BASE: &str = "https://api.github.com";
/// Name of the checksum file attached to every release (see the release workflow).
pub const CHECKSUMS_ASSET: &str = "SHA256SUMS.txt";

/// Stability of a release, from the least to the most stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Alpha,
    Beta,
    Rc,
    Stable,
}

impl Channel {
    /// The channel of a version: by its pre-release label.
    pub fn of(v: &Version) -> Self {
        if v.pre.is_empty() {
            return Channel::Stable;
        }
        let label = v.pre.as_str().split('.').next().unwrap_or("").to_ascii_lowercase();
        match label.as_str() {
            "rc" => Channel::Rc,
            "beta" => Channel::Beta,
            _ => Channel::Alpha,
        }
    }

    /// `auto` follows the running version (a beta build keeps getting betas).
    pub fn parse(setting: &str, current: &Version) -> Result<Self> {
        match setting {
            "" | "auto" => Ok(Self::of(current)),
            "stable" | "production" => Ok(Channel::Stable),
            "rc" => Ok(Channel::Rc),
            "beta" => Ok(Channel::Beta),
            "alpha" | "nightly" => Ok(Channel::Alpha),
            other => Err(Error::Invalid(format!("canale di aggiornamento sconosciuto: `{other}` (auto, stable, beta, alpha)"))),
        }
    }

    /// A channel receives the releases that are at least as stable as it is.
    pub fn accepts(self, release: Channel) -> bool {
        release >= self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReleaseInfo {
    pub tag: String,
    pub version: String,
    pub channel: Channel,
    pub notes: String,
    pub published_at: Option<String>,
    pub page: String,
    /// The installer for this OS and CPU, when the release has one.
    pub asset: Option<Asset>,
    /// `SHA256SUMS.txt` of the release.
    #[serde(skip)]
    pub checksums_url: Option<String>,
}

/// What the GitHub API returns for a release (the fields that matter).
#[derive(Debug, Deserialize)]
pub struct GhRelease {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
pub struct GhAsset {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    pub browser_download_url: String,
}

/// `v0.4.0-beta.1` → `0.4.0-beta.1`; anything that is not a version is not a release of the app.
pub fn parse_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.trim().trim_start_matches('v')).ok()
}

/// The installer to download for `os`/`arch` (`macos`, `windows`, `linux` · `aarch64`, `x86_64`).
/// The desktop bundles keep the Tauri names (`…_amd64.AppImage`, `…_universal.dmg`,
/// `…_x64_en-US.msi`); the `aiw-<version>-…` files are the command-line daemon and are skipped.
pub fn pick_asset<'a>(os: &str, arch: &str, names: &'a [GhAsset]) -> Option<&'a GhAsset> {
    let arch_tokens: &[&str] = if arch == "aarch64" || arch == "arm64" { &["aarch64", "arm64"] } else { &["x86_64", "amd64", "x64"] };
    let wanted_arch = |n: &str| {
        let l = n.to_ascii_lowercase();
        arch_tokens.iter().any(|t| l.contains(t))
    };
    let desktop = |a: &&GhAsset| !a.name.starts_with("aiw-") && !a.name.ends_with(".txt") && !a.name.ends_with(".apk");
    let by_ext = |ext: &str, need_arch: bool| {
        names.iter().filter(desktop).find(|a| a.name.to_ascii_lowercase().ends_with(ext) && (!need_arch || wanted_arch(&a.name)))
    };
    match os {
        // One universal disk image covers both CPUs.
        "macos" => by_ext(".dmg", false),
        "windows" => {
            by_ext(".msi", true).or_else(|| by_ext("-setup.exe", true)).or_else(|| by_ext(".msi", false)).or_else(|| by_ext(".exe", true))
        }
        "linux" => by_ext(".appimage", true),
        _ => None,
    }
}

/// The newest release this installation should be offered, if any: newer than `current`, in a
/// channel that `channel` accepts. A release without an installer for this platform is still
/// reported (with `asset: None`) so that the UI can point to its page.
pub fn pick_release(releases: &[GhRelease], current: &Version, channel: Channel, os: &str, arch: &str) -> Option<ReleaseInfo> {
    let mut best: Option<(Version, &GhRelease)> = None;
    for r in releases.iter().filter(|r| !r.draft) {
        let Some(v) = parse_tag(&r.tag_name) else { continue };
        if v <= *current || !channel.accepts(Channel::of(&v)) {
            continue;
        }
        if best.as_ref().is_none_or(|(b, _)| v > *b) {
            best = Some((v, r));
        }
    }
    best.map(|(v, r)| release_info(r, &v, os, arch))
}

fn release_info(r: &GhRelease, v: &Version, os: &str, arch: &str) -> ReleaseInfo {
    ReleaseInfo {
        tag: r.tag_name.clone(),
        channel: Channel::of(v),
        version: v.to_string(),
        notes: r.body.clone().unwrap_or_default(),
        published_at: r.published_at.clone(),
        page: r.html_url.clone(),
        asset: pick_asset(os, arch, &r.assets).map(|a| Asset { name: a.name.clone(), size: a.size, url: a.browser_download_url.clone() }),
        checksums_url: r.assets.iter().find(|a| a.name == CHECKSUMS_ASSET).map(|a| a.browser_download_url.clone()),
    }
}

/// `sha256sum` output (`<hex>  <name>`, `*` marks binary mode) → name → lowercase hex.
pub fn parse_checksums(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let (hash, name) = l.trim().split_once(char::is_whitespace)?;
            let name = name.trim().trim_start_matches('*');
            (hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) && !name.is_empty())
                .then(|| (name.to_string(), hash.to_ascii_lowercase()))
        })
        .collect()
}

/// A file name that cannot leave the downloads folder.
fn safe_file_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 200 && !name.contains(['/', '\\', '\0']) && name != "." && name != ".."
}

/// Downloads only from GitHub (or from the configured API host, for tests): a release can name
/// any URL in its assets list, but an installer that comes from elsewhere is never fetched.
pub fn allowed_url(url: &str, api_base: &str) -> bool {
    // (scheme, authority with port, host without port), lowercase.
    let parts = |u: &str| -> Option<(String, String, String)> {
        let (scheme, rest) = u.split_once("://")?;
        let authority = rest.split(['/', '?', '#']).next()?.to_ascii_lowercase();
        if authority.contains('@') {
            return None;
        }
        let host = authority.split(':').next()?.to_string();
        Some((scheme.to_string(), authority, host))
    };
    let Some((scheme, authority, host)) = parts(url) else { return false };
    if api_base != DEFAULT_API_BASE {
        if let Some((base_scheme, base_authority, _)) = parts(api_base) {
            if base_scheme == scheme && base_authority == authority {
                return true;
            }
        }
    }
    scheme == "https" && (host == "github.com" || host == "api.github.com" || host.ends_with(".githubusercontent.com"))
}

/// What happens to the installer once it is downloaded and verified.
#[derive(Debug, Clone, PartialEq)]
pub enum InstallAction {
    /// macOS: open the disk image (drag the app into Applications).
    Open(PathBuf),
    /// Windows: run the MSI.
    Msiexec(PathBuf),
    /// Windows: run the setup program.
    Run(PathBuf),
    /// Linux: replace the running AppImage.
    ReplaceAppImage { from: PathBuf, to: PathBuf },
    /// Anything else: show the folder.
    Reveal(PathBuf),
}

pub fn install_action(os: &str, file: &Path, running_appimage: Option<&Path>) -> InstallAction {
    let name = file.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match os {
        "macos" if name.ends_with(".dmg") => InstallAction::Open(file.into()),
        "windows" if name.ends_with(".msi") => InstallAction::Msiexec(file.into()),
        "windows" if name.ends_with(".exe") => InstallAction::Run(file.into()),
        "linux" if name.ends_with(".appimage") => match running_appimage {
            Some(to) => InstallAction::ReplaceAppImage { from: file.into(), to: to.into() },
            None => InstallAction::Reveal(file.parent().unwrap_or(file).into()),
        },
        _ => InstallAction::Reveal(file.parent().unwrap_or(file).into()),
    }
}

/// Carries out an [`InstallAction`]; returns what the user has to do next.
pub fn run_action(action: &InstallAction) -> Result<String> {
    use std::process::Command;
    let spawn = |mut c: Command| c.spawn().map(|_| ()).map_err(|e| Error::Runtime(format!("impossibile avviare l'installer: {e}")));
    match action {
        InstallAction::Open(p) => {
            #[cfg(target_os = "macos")]
            spawn({
                let mut c = Command::new("open");
                c.arg(p);
                c
            })?;
            #[cfg(not(target_os = "macos"))]
            let _ = (p, &spawn);
            Ok("Trascina AI Workspace Local nella cartella Applicazioni, poi riapri l'app.".into())
        }
        InstallAction::Msiexec(p) => {
            spawn({
                let mut c = Command::new("msiexec");
                c.arg("/i").arg(p).arg("/passive");
                c
            })?;
            Ok("L'installer di Windows è partito: chiudi l'app per completare l'aggiornamento.".into())
        }
        InstallAction::Run(p) => {
            spawn(Command::new(p))?;
            Ok("L'installer è partito: chiudi l'app per completare l'aggiornamento.".into())
        }
        InstallAction::ReplaceAppImage { from, to } => {
            // Copy next to the target, then rename over it: the running image keeps its inode.
            let tmp = to.with_extension("AppImage.new");
            std::fs::copy(from, &tmp).map_err(|e| Error::Runtime(format!("impossibile copiare l'AppImage: {e}")))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
            }
            std::fs::rename(&tmp, to).map_err(|e| Error::Runtime(format!("impossibile sostituire l'AppImage: {e}")))?;
            Ok("AppImage aggiornata: riavvia l'app.".into())
        }
        InstallAction::Reveal(dir) => Ok(format!("Installer scaricato e verificato in {}: aprilo per aggiornare.", dir.display())),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateState {
    Idle,
    Checking,
    Downloading,
    /// Downloaded and verified: ready to install.
    Ready,
    Installing,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub current: String,
    /// The channel in use (what `auto` resolved to).
    pub channel: Channel,
    /// What the setting says: `auto`, `stable`, `beta`, `alpha`.
    pub configured: String,
    pub state: UpdateState,
    pub available: Option<ReleaseInfo>,
    pub releases: Vec<ReleaseInfo>,
    pub checked_at: Option<String>,
    pub received: u64,
    pub total: u64,
    /// Verified installer on disk.
    pub file: Option<String>,
    /// What to do next after an install hand-over.
    pub message: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
struct Inner {
    state: Option<UpdateState>,
    available: Option<ReleaseInfo>,
    releases: Vec<ReleaseInfo>,
    checked_channel: Option<Channel>,
    checked_at: Option<String>,
    received: u64,
    total: u64,
    file: Option<PathBuf>,
    message: Option<String>,
    error: Option<String>,
}

pub struct Updater {
    http: reqwest::Client,
    api_base: String,
    repo: String,
    os: String,
    arch: String,
    dir: PathBuf,
    inner: Mutex<Inner>,
}

impl Updater {
    pub fn new(api_base: Option<String>, os: &str, arch: &str, dir: PathBuf) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("ai-workspace-local/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60 * 30))
            .build()
            .expect("http client");
        Self {
            http,
            api_base: api_base.unwrap_or_else(|| DEFAULT_API_BASE.into()).trim_end_matches('/').to_string(),
            repo: DEFAULT_REPO.into(),
            os: os.into(),
            arch: arch.into(),
            dir,
            inner: Mutex::new(Inner::default()),
        }
    }

    pub fn status(&self, current: &Version, configured: &str) -> UpdateStatus {
        let i = self.inner.lock().unwrap();
        let channel = Channel::parse(configured, current).unwrap_or_else(|_| Channel::of(current));
        let same_channel = i.checked_channel == Some(channel);
        UpdateStatus {
            current: current.to_string(),
            channel,
            configured: if configured.is_empty() { "auto".into() } else { configured.into() },
            state: i.state.unwrap_or(UpdateState::Idle),
            available: i.available.clone().filter(|_| same_channel),
            releases: if same_channel { i.releases.clone() } else { vec![] },
            checked_at: i.checked_at.clone().filter(|_| same_channel),
            received: i.received,
            total: i.total,
            file: i.file.as_ref().map(|p| p.display().to_string()),
            message: i.message.clone(),
            error: i.error.clone(),
        }
    }

    fn set_error(&self, e: &Error) {
        let mut i = self.inner.lock().unwrap();
        i.state = Some(UpdateState::Error);
        i.error = Some(e.to_string());
    }

    /// Looks for a newer release in `channel`.
    pub async fn check(&self, current: &Version, channel: Channel) -> Result<Option<ReleaseInfo>> {
        {
            let mut i = self.inner.lock().unwrap();
            if matches!(i.state, Some(UpdateState::Checking | UpdateState::Downloading | UpdateState::Installing)) {
                return Err(Error::Invalid("un'operazione di aggiornamento è già in corso".into()));
            }
            i.state = Some(UpdateState::Checking);
            i.error = None;
            i.message = None;
        }
        let found = self.fetch_releases().await;
        let mut i = self.inner.lock().unwrap();
        i.checked_at = Some(chrono::Utc::now().to_rfc3339());
        match found {
            Ok(releases) => {
                let rel = pick_release(&releases, current, channel, &self.os, &self.arch);
                let mut versions = releases
                    .iter()
                    .filter(|r| !r.draft)
                    .filter_map(|r| parse_tag(&r.tag_name).map(|v| (v, r)))
                    .filter(|(v, _)| channel.accepts(Channel::of(v)))
                    .collect::<Vec<_>>();
                versions.sort_by(|a, b| b.0.cmp(&a.0));
                i.releases = versions.iter().map(|(v, r)| release_info(r, v, &self.os, &self.arch)).collect();
                i.checked_channel = Some(channel);
                // A different release than the one already downloaded invalidates that file.
                if i.available.as_ref().map(|a| &a.tag) != rel.as_ref().map(|a| &a.tag) {
                    i.file = None;
                    i.received = 0;
                    i.total = 0;
                }
                i.state = Some(if i.file.is_some() { UpdateState::Ready } else { UpdateState::Idle });
                i.available = rel.clone();
                Ok(rel)
            }
            Err(e) => {
                i.state = Some(UpdateState::Error);
                i.error = Some(e.to_string());
                Err(e)
            }
        }
    }

    async fn fetch_releases(&self) -> Result<Vec<GhRelease>> {
        let url = format!("{}/repos/{}/releases?per_page=30", self.api_base, self.repo);
        let res = self
            .http
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| Error::Runtime(format!("GitHub non raggiungibile: {e}")))?;
        let status = res.status();
        if !status.is_success() {
            let hint = if status.as_u16() == 403 || status.as_u16() == 429 { " (troppe richieste: riprova più tardi)" } else { "" };
            return Err(Error::Runtime(format!("GitHub ha risposto {status}{hint}")));
        }
        res.json::<Vec<GhRelease>>().await.map_err(|e| Error::Runtime(format!("risposta di GitHub non valida: {e}")))
    }

    /// Downloads the installer of the available release and verifies it against the release's
    /// `SHA256SUMS.txt`. Returns the path of the verified file.
    pub async fn download(&self) -> Result<PathBuf> {
        let rel = {
            let mut i = self.inner.lock().unwrap();
            if matches!(i.state, Some(UpdateState::Checking | UpdateState::Downloading | UpdateState::Installing)) {
                return Err(Error::Invalid("un'operazione di aggiornamento è già in corso".into()));
            }
            let rel = i.available.clone().ok_or_else(|| Error::Invalid("nessun aggiornamento disponibile: controlla prima".into()))?;
            if let Some(f) = i.file.clone().filter(|f| f.is_file()) {
                i.state = Some(UpdateState::Ready);
                return Ok(f);
            }
            i.state = Some(UpdateState::Downloading);
            i.error = None;
            i.received = 0;
            i.total = 0;
            rel
        };
        match self.download_verified(&rel).await {
            Ok(path) => {
                let mut i = self.inner.lock().unwrap();
                i.state = Some(UpdateState::Ready);
                i.file = Some(path.clone());
                Ok(path)
            }
            Err(e) => {
                self.set_error(&e);
                Err(e)
            }
        }
    }

    async fn download_verified(&self, rel: &ReleaseInfo) -> Result<PathBuf> {
        let asset = rel.asset.as_ref().ok_or_else(|| {
            Error::Unsupported(format!("la release {} non ha un installer per questo sistema: scaricalo da {}", rel.tag, rel.page))
        })?;
        if !safe_file_name(&asset.name) {
            return Err(Error::Invalid(format!("nome file non valido nella release: {}", asset.name)));
        }
        let sums_url = rel.checksums_url.as_deref().ok_or_else(|| {
            Error::Invalid(format!("la release {} non pubblica {CHECKSUMS_ASSET}: non posso verificare l'installer", rel.tag))
        })?;
        for u in [asset.url.as_str(), sums_url] {
            if !allowed_url(u, &self.api_base) {
                return Err(Error::Invalid(format!("indirizzo di download non consentito: {u}")));
            }
        }
        let sums = self.http.get(sums_url).timeout(Duration::from_secs(30)).send().await.and_then(|r| r.error_for_status());
        let sums = sums.map_err(|e| Error::Runtime(format!("{CHECKSUMS_ASSET}: {e}")))?;
        let sums = parse_checksums(&sums.text().await.map_err(|e| Error::Runtime(format!("{CHECKSUMS_ASSET}: {e}")))?);
        let expected =
            sums.get(&asset.name).ok_or_else(|| Error::Invalid(format!("{CHECKSUMS_ASSET} non contiene {}", asset.name)))?.clone();

        let dir = self.dir.join(rel.version.as_str());
        std::fs::create_dir_all(&dir)?;
        let target = dir.join(&asset.name);
        let part = dir.join(format!("{}.part", asset.name));
        let res = self.http.get(&asset.url).send().await.and_then(|r| r.error_for_status());
        let res = res.map_err(|e| Error::Runtime(format!("download: {e}")))?;
        let total = res.content_length().unwrap_or(asset.size);
        self.inner.lock().unwrap().total = total;
        let mut file = std::fs::File::create(&part)?;
        let mut hasher = Sha256::new();
        let mut stream = res.bytes_stream();
        let mut received = 0u64;
        let result: Result<()> = async {
            use std::io::Write;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| Error::Runtime(format!("download interrotto: {e}")))?;
                hasher.update(&chunk);
                file.write_all(&chunk)?;
                received += chunk.len() as u64;
                self.inner.lock().unwrap().received = received;
            }
            file.flush()?;
            Ok(())
        }
        .await;
        drop(file);
        if let Err(e) = result {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }
        let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if actual != expected {
            let _ = std::fs::remove_file(&part);
            return Err(Error::Invalid(format!("checksum non valido per {}: il file è stato scartato", asset.name)));
        }
        std::fs::rename(&part, &target)?;
        Ok(target)
    }

    /// Hands the verified installer over to the system.
    pub fn install(&self, running_appimage: Option<&Path>) -> Result<String> {
        let file = {
            let mut i = self.inner.lock().unwrap();
            let f =
                i.file.clone().filter(|f| f.is_file()).ok_or_else(|| Error::Invalid("nessun installer scaricato e verificato".into()))?;
            if i.state == Some(UpdateState::Installing) {
                return Err(Error::Invalid("installazione già avviata".into()));
            }
            i.state = Some(UpdateState::Installing);
            f
        };
        let r = run_action(&install_action(&self.os, &file, running_appimage));
        let mut i = self.inner.lock().unwrap();
        match &r {
            Ok(msg) => {
                i.state = Some(UpdateState::Ready);
                i.message = Some(msg.clone());
            }
            Err(e) => {
                i.state = Some(UpdateState::Error);
                i.error = Some(e.to_string());
            }
        }
        r
    }

    pub fn available(&self) -> Option<ReleaseInfo> {
        self.inner.lock().unwrap().available.clone()
    }
}

impl crate::Engine {
    fn update_setting(&self) -> String {
        self.0.config.get_str("update.channel").filter(|c| !c.is_empty()).unwrap_or_else(|| "auto".into())
    }

    /// Where the update stands: current version, channel, what was found, download progress.
    pub fn update_status(&self) -> UpdateStatus {
        self.0.updater.status(&self.0.current_version, &self.update_setting())
    }

    /// Looks for a newer release in the configured channel.
    pub async fn update_check(&self) -> Result<UpdateStatus> {
        let channel = Channel::parse(&self.update_setting(), &self.0.current_version)?;
        let found = self.0.updater.check(&self.0.current_version, channel).await?;
        if let Some(r) = &found {
            self.audit_system("update.check", &format!("{} ({:?})", r.tag, r.channel));
        }
        Ok(self.update_status())
    }

    /// Downloads and verifies the installer of the release found by the last check.
    pub async fn update_download(&self) -> Result<UpdateStatus> {
        if self.update_status().available.is_none() {
            return Err(Error::Invalid("Controlla gli aggiornamenti per il canale selezionato.".into()));
        }
        let file = self.0.updater.download().await?;
        self.audit_user("update.download", &file.display().to_string());
        Ok(self.update_status())
    }

    /// Hands the verified installer to the system. Returns what the user has to do next.
    pub fn update_install(&self) -> Result<UpdateStatus> {
        if self.update_status().available.is_none() {
            return Err(Error::Invalid("Controlla gli aggiornamenti per il canale selezionato.".into()));
        }
        let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
        let msg = self.0.updater.install(appimage.as_deref())?;
        self.audit_user("update.install", &msg);
        Ok(self.update_status())
    }

    /// Checks for updates at startup (after a short delay) and every six hours, when the setting
    /// is on; with `update.auto_download` the installer is also fetched and verified in advance.
    pub(crate) fn start_update_checks(&self) {
        let e = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            loop {
                if e.0.config.get_bool("update.auto_check") {
                    match e.update_check().await {
                        Ok(st)
                            if st.available.as_ref().is_some_and(|a| a.asset.is_some()) && e.0.config.get_bool("update.auto_download") =>
                        {
                            if let Err(err) = e.update_download().await {
                                tracing::warn!("update download failed: {err}");
                            }
                        }
                        Ok(_) => {}
                        Err(err) => tracing::debug!("update check failed: {err}"),
                    }
                }
                tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn asset(name: &str) -> GhAsset {
        GhAsset { name: name.into(), size: 10, browser_download_url: format!("https://github.com/o/r/releases/download/t/{name}") }
    }

    fn release(tag: &str, assets: &[&str]) -> GhRelease {
        GhRelease {
            tag_name: tag.into(),
            name: None,
            body: Some(format!("note {tag}")),
            draft: false,
            published_at: Some("2026-01-01T00:00:00Z".into()),
            html_url: format!("https://github.com/o/r/releases/tag/{tag}"),
            assets: assets.iter().map(|n| asset(n)).collect(),
        }
    }

    #[test]
    fn channels_come_from_the_prerelease_label() {
        assert_eq!(Channel::of(&v("1.2.3")), Channel::Stable);
        assert_eq!(Channel::of(&v("0.4.0-beta.1")), Channel::Beta);
        assert_eq!(Channel::of(&v("0.4.0-rc.2")), Channel::Rc);
        assert_eq!(Channel::of(&v("0.4.0-alpha.1")), Channel::Alpha);
        assert_eq!(Channel::of(&v("0.4.0-nightly.20260101")), Channel::Alpha);
        assert_eq!(Channel::of(&v("0.4.0-dev")), Channel::Alpha);
    }

    #[test]
    fn a_channel_receives_what_is_at_least_as_stable() {
        assert!(Channel::Beta.accepts(Channel::Stable) && Channel::Beta.accepts(Channel::Rc) && Channel::Beta.accepts(Channel::Beta));
        assert!(!Channel::Beta.accepts(Channel::Alpha));
        assert!(
            Channel::Stable.accepts(Channel::Stable) && !Channel::Stable.accepts(Channel::Rc) && !Channel::Stable.accepts(Channel::Beta)
        );
        assert!(Channel::Alpha.accepts(Channel::Alpha));
    }

    #[test]
    fn auto_follows_the_running_version_and_names_are_validated() {
        assert_eq!(Channel::parse("auto", &v("0.4.0-beta.1")).unwrap(), Channel::Beta);
        assert_eq!(Channel::parse("", &v("1.0.0")).unwrap(), Channel::Stable);
        assert_eq!(Channel::parse("production", &v("0.4.0-beta.1")).unwrap(), Channel::Stable);
        assert_eq!(Channel::parse("nightly", &v("1.0.0")).unwrap(), Channel::Alpha);
        assert!(Channel::parse("canary", &v("1.0.0")).is_err());
    }

    #[test]
    fn tags_that_are_not_versions_are_ignored() {
        assert_eq!(parse_tag("v0.4.0-beta.1"), Some(v("0.4.0-beta.1")));
        assert_eq!(parse_tag("0.4.0"), Some(v("0.4.0")));
        assert_eq!(parse_tag("nightly"), None);
        assert_eq!(parse_tag("v1.2"), None);
    }

    const ALL: &[&str] = &[
        "AI_Workspace_Local_0.5.0_universal.dmg",
        "AI_Workspace_Local_0.5.0_x64_en-US.msi",
        "AI_Workspace_Local_0.5.0_x64-setup.exe",
        "AI_Workspace_Local_0.5.0_amd64.AppImage",
        "AI_Workspace_Local_0.5.0_amd64.deb",
        "AI_Workspace_Local-0.5.0-1.x86_64.rpm",
        "aiw-0.5.0-linux-x64",
        "aiw-0.5.0-android-arm64-unsigned.apk",
        "SHA256SUMS.txt",
    ];

    #[test]
    fn picks_the_installer_for_the_platform() {
        let assets: Vec<GhAsset> = ALL.iter().map(|n| asset(n)).collect();
        let name = |os, arch| pick_asset(os, arch, &assets).map(|a| a.name.as_str());
        assert_eq!(name("macos", "aarch64"), Some("AI_Workspace_Local_0.5.0_universal.dmg"));
        assert_eq!(name("macos", "x86_64"), Some("AI_Workspace_Local_0.5.0_universal.dmg"));
        assert_eq!(name("windows", "x86_64"), Some("AI_Workspace_Local_0.5.0_x64_en-US.msi"));
        assert_eq!(name("linux", "x86_64"), Some("AI_Workspace_Local_0.5.0_amd64.AppImage"));
        // No AppImage for ARM Linux in this release: nothing to install by itself.
        assert_eq!(name("linux", "aarch64"), None);
        assert_eq!(name("android", "aarch64"), None);
        // Without an MSI the setup program is used.
        let only_exe: Vec<GhAsset> = ["App_0.5.0_x64-setup.exe", "aiw-0.5.0-windows-x64.exe"].iter().map(|n| asset(n)).collect();
        assert_eq!(pick_asset("windows", "x86_64", &only_exe).unwrap().name, "App_0.5.0_x64-setup.exe");
    }

    #[test]
    fn offers_the_newest_release_of_the_channel() {
        let rels = vec![
            release("v0.4.0-beta.1", ALL),
            release("v0.4.0", ALL),
            release("v0.5.0-beta.2", ALL),
            release("v0.5.0-alpha.1", ALL),
            release("v0.4.1", ALL),
            release("nightly", ALL),
        ];
        let pick = |cur: &str, ch| pick_release(&rels, &v(cur), ch, "macos", "aarch64").map(|r| r.tag);
        // Production: only stable releases, newest first.
        assert_eq!(pick("0.3.0", Channel::Stable).as_deref(), Some("v0.4.1"));
        // Beta also sees stable ones; the highest version wins (0.5.0-beta.2 > 0.4.1).
        assert_eq!(pick("0.3.0", Channel::Beta).as_deref(), Some("v0.5.0-beta.2"));
        assert_eq!(pick("0.3.0", Channel::Alpha).as_deref(), Some("v0.5.0-beta.2"));
        // A beta build is offered the stable release of the same version (0.4.0 > 0.4.0-beta.1).
        assert_eq!(pick("0.4.0-beta.1", Channel::Stable).as_deref(), Some("v0.4.1"));
        // Up to date.
        assert_eq!(pick("0.4.1", Channel::Stable), None);
        assert_eq!(pick("0.5.0-beta.2", Channel::Beta), None);
        // Drafts are never offered.
        let mut drafts = vec![release("v9.0.0", ALL)];
        drafts[0].draft = true;
        assert!(pick_release(&drafts, &v("0.1.0"), Channel::Alpha, "macos", "aarch64").is_none());
    }

    /// What an installation is offered, by what it has installed and what `update.channel` says (`auto` included): the
    /// table that `docs/release/RELEASE.md` §6 documents. `None` is "nothing to offer".
    #[test]
    fn what_each_installation_is_offered_by_installed_version_and_channel_setting() {
        let offered = |rels: &[GhRelease], installed: &str, setting: &str| {
            let current = v(installed);
            let channel = Channel::parse(setting, &current).unwrap();
            pick_release(rels, &current, channel, "linux", "x86_64").map(|r| r.tag)
        };
        // Only betas and a release candidate ahead of the last stable release (where the project is today).
        let betas = vec![
            release("v0.4.1", ALL),
            release("v0.5.0-beta.8", ALL),
            release("v0.5.0-beta.9", ALL),
            release("v0.5.0-rc.1", ALL),
            release("v0.6.0-alpha.1", ALL),
        ];
        let cases: [(&str, &str, Option<&str>); 12] = [
            // A stable install: `auto` is `stable`, and it does not see the betas, the candidate or the alpha.
            ("0.4.0", "auto", Some("v0.4.1")),
            ("0.4.0", "stable", Some("v0.4.1")),
            ("0.4.1", "auto", None),
            // Opting in from a stable install: beta also gets the candidate (more stable than a beta; 0.5.0-rc.1 is the highest).
            ("0.4.0", "beta", Some("v0.5.0-rc.1")),
            ("0.4.1", "beta", Some("v0.5.0-rc.1")),
            ("0.4.1", "alpha", Some("v0.6.0-alpha.1")),
            // A beta install: `auto` stays on beta (betas, candidates, stable), never the alpha.
            ("0.5.0-beta.8", "auto", Some("v0.5.0-rc.1")),
            ("0.5.0-beta.8", "beta", Some("v0.5.0-rc.1")),
            ("0.5.0-rc.1", "auto", None),
            // A beta install that asks for `stable` is not downgraded to the older stable release.
            ("0.5.0-beta.8", "stable", None),
            ("0.5.0-beta.9", "auto", Some("v0.5.0-rc.1")),
            ("0.5.0-beta.9", "stable", None),
        ];
        for (installed, setting, want) in cases {
            assert_eq!(offered(&betas, installed, setting).as_deref(), want, "installed {installed}, channel `{setting}`");
        }
        // Once 0.5.0 is out: everybody is offered it, and a beta install is moved to stable whatever the setting says.
        let mut with_final = betas;
        with_final.push(release("v0.5.0", ALL));
        for (installed, setting) in [
            ("0.4.1", "auto"),
            ("0.4.1", "stable"),
            ("0.4.1", "beta"),
            ("0.5.0-beta.8", "auto"),
            ("0.5.0-beta.8", "stable"),
            ("0.5.0-rc.1", "beta"),
        ] {
            assert_eq!(offered(&with_final, installed, setting).as_deref(), Some("v0.5.0"), "installed {installed}, channel `{setting}`");
        }
        assert_eq!(offered(&with_final, "0.5.0", "auto"), None, "a stable install on `auto` does not get the alpha of the next version");
        assert_eq!(offered(&with_final, "0.5.0", "beta"), None);
        assert_eq!(offered(&with_final, "0.5.0", "alpha").as_deref(), Some("v0.6.0-alpha.1"));
        // `auto` resolves from the installed version alone: the label decides, and only `rc`/`beta` followed by a dot or nothing.
        assert_eq!(Channel::parse("auto", &v("0.5.0-beta.8")).unwrap(), Channel::Beta);
        assert_eq!(Channel::parse("auto", &v("0.5.0-rc.2")).unwrap(), Channel::Rc);
        assert_eq!(Channel::parse("auto", &v("0.5.0-beta")).unwrap(), Channel::Beta);
        assert_eq!(
            Channel::parse("auto", &v("0.5.0-beta8")).unwrap(),
            Channel::Alpha,
            "`beta8` has no dot: it is not recognised as a beta"
        );
        assert_eq!(Channel::parse("auto", &v("0.5.0")).unwrap(), Channel::Stable);
    }

    #[test]
    fn release_info_carries_installer_notes_and_checksums() {
        let rels = vec![release("v0.5.0", ALL)];
        let r = pick_release(&rels, &v("0.4.0"), Channel::Stable, "windows", "x86_64").unwrap();
        assert_eq!(r.version, "0.5.0");
        assert_eq!(r.channel, Channel::Stable);
        assert_eq!(r.notes, "note v0.5.0");
        assert!(r.asset.unwrap().name.ends_with(".msi"));
        assert!(r.checksums_url.unwrap().ends_with("SHA256SUMS.txt"));
        // No installer for this platform: still reported, so the UI can link the page.
        let r = pick_release(&rels, &v("0.4.0"), Channel::Stable, "linux", "aarch64").unwrap();
        assert!(r.asset.is_none());
        assert!(r.page.contains("releases/tag"));
    }

    #[test]
    fn parses_sha256sum_output() {
        let h = "a".repeat(64);
        let text = format!("{h}  app.dmg\n{} *bin.exe\nnot a hash  x\n\n{h} spaced name.msi\n", "B".repeat(64));
        let m = parse_checksums(&text);
        assert_eq!(m.get("app.dmg"), Some(&h));
        assert_eq!(m.get("bin.exe"), Some(&"b".repeat(64)));
        assert_eq!(m.get("spaced name.msi"), Some(&h));
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn downloads_only_from_github_or_the_test_host() {
        let api = DEFAULT_API_BASE;
        assert!(allowed_url("https://github.com/o/r/releases/download/v1/a.dmg", api));
        assert!(allowed_url("https://objects.githubusercontent.com/x/a.dmg", api));
        assert!(!allowed_url("http://github.com/o/r/a.dmg", api), "plain http");
        assert!(!allowed_url("https://evil.example/a.dmg", api));
        assert!(!allowed_url("https://github.com.evil.example/a.dmg", api));
        assert!(!allowed_url("https://user@github.com/a.dmg", api));
        assert!(!allowed_url("file:///etc/passwd", api));
        // A configured API host (tests) is allowed for itself only.
        assert!(allowed_url("http://127.0.0.1:9000/dl/a.dmg", "http://127.0.0.1:9000"));
        assert!(!allowed_url("http://127.0.0.1:9001/dl/a.dmg", "http://127.0.0.1:9000"));
    }

    #[test]
    fn file_names_cannot_leave_the_folder() {
        assert!(safe_file_name("App_0.5.0.dmg"));
        for bad in ["", "../x", "a/b", "a\\b", "..", "."] {
            assert!(!safe_file_name(bad), "{bad}");
        }
    }

    #[test]
    fn install_hand_over_depends_on_the_platform() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(install_action("macos", &p("/u/App.dmg"), None), InstallAction::Open(p("/u/App.dmg")));
        assert_eq!(install_action("windows", &p("C:/u/App.msi"), None), InstallAction::Msiexec(p("C:/u/App.msi")));
        assert_eq!(install_action("windows", &p("C:/u/App-setup.exe"), None), InstallAction::Run(p("C:/u/App-setup.exe")));
        assert_eq!(
            install_action("linux", &p("/u/App.AppImage"), Some(Path::new("/opt/App.AppImage"))),
            InstallAction::ReplaceAppImage { from: p("/u/App.AppImage"), to: p("/opt/App.AppImage") }
        );
        assert_eq!(install_action("linux", &p("/u/App.AppImage"), None), InstallAction::Reveal(p("/u")));
        assert_eq!(install_action("linux", &p("/u/App.deb"), None), InstallAction::Reveal(p("/u")));
    }

    #[cfg(unix)]
    #[test]
    fn replaces_the_appimage_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("new.AppImage"), dir.path().join("App.AppImage"));
        std::fs::write(&from, b"new").unwrap();
        std::fs::write(&to, b"old").unwrap();
        let msg = run_action(&InstallAction::ReplaceAppImage { from, to: to.clone() }).unwrap();
        assert!(msg.contains("riavvia"));
        assert_eq!(std::fs::read(&to).unwrap(), b"new");
        assert!(!dir.path().join("App.AppImage.new").exists());
    }
}
