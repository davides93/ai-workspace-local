//! Direct URL provider: one model file at an arbitrary `http(s)` address. Never sends credentials
//! (the tokens of the other providers stay with their hosts).

use super::http::{net_error, status_error, USER_AGENT};
use super::{FileJob, ModelProvider, SearchQuery};
use crate::relpath::native_rel;
use aiw_core::api::{ModelProviderInfo, ModelVariant, RemoteFile, RemoteModel, RemoteModelDetail, SearchPage};
use aiw_core::{Error, Result};
use async_trait::async_trait;
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;

/// Extension → model format of a single downloadable file.
pub fn format_of_file(name: &str) -> Option<&'static str> {
    let l = name.to_ascii_lowercase();
    [(".gguf", "gguf"), (".litertlm", "litertlm"), (".task", "litertlm"), (".pte", "pte"), (".llamafile", "llamafile")]
        .iter()
        .find(|(ext, _)| l.ends_with(ext))
        .map(|(_, f)| *f)
}

pub struct UrlProvider {
    client: reqwest::Client,
}

impl Default for UrlProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn safe_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 200 && n != "." && n != ".." && !n.contains(['/', '\\', '\0', ':']) && !n.starts_with('.')
}

fn disposition_name(h: &str) -> Option<String> {
    // filename*=UTF-8''name%20x.gguf wins over filename="name.gguf".
    let part = |key: &str| h.split(';').map(str::trim).find_map(|p| p.strip_prefix(key).map(String::from));
    if let Some(v) = part("filename*=") {
        let v = v.rsplit("''").next().unwrap_or(&v).trim_matches('"').to_string();
        return Some(percent_decode(&v));
    }
    part("filename=").map(|v| v.trim_matches('"').to_string())
}

impl UrlProvider {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("http client");
        Self { client }
    }

    /// `HEAD` for size and file name; `file_name` and `sha256` come from the user when known.
    pub async fn detail_with(&self, url: &str, file_name: Option<&str>, sha256: Option<&str>) -> Result<RemoteModelDetail> {
        let parsed = Url::parse(url.trim()).map_err(|_| Error::Invalid(format!("URL non valido: {url}")))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(Error::Invalid("serve un URL http o https".into()));
        }
        if let Some(h) = sha256 {
            if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::Invalid("lo sha256 deve essere di 64 caratteri esadecimali".into()));
            }
        }
        let host = parsed.host_str().unwrap_or_default();
        let mut size = 0u64;
        let mut disp = None;
        match self.client.head(parsed.clone()).send().await {
            Ok(r) if r.status().is_success() => {
                let ctype = r.headers().get("content-type").and_then(|v| v.to_str().ok()).map(|v| v.trim().to_ascii_lowercase());
                if ctype.as_deref().is_some_and(|c| c.starts_with("text/html") || c.starts_with("application/xhtml")) {
                    return Err(Error::Invalid(
                        "l'indirizzo punta a una pagina web (text/html), non a un file di modello: usa il link diretto al file \
                         (su Hugging Face `resolve/` al posto di `blob/`)"
                            .into(),
                    ));
                }
                // `Response::content_length` is always 0 for HEAD: read the header itself.
                size = r.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                disp = r.headers().get("content-disposition").and_then(|v| v.to_str().ok()).and_then(disposition_name);
            }
            Ok(r) if matches!(r.status().as_u16(), 401 | 403 | 404 | 500..=599) => return Err(status_error(host, "il file", r.status())),
            // Some servers refuse HEAD: the size stays unknown and the download goes on.
            Ok(_) => {}
            Err(e) => return Err(net_error(host, &e)),
        }
        let from_url = parsed.path_segments().and_then(|mut s| s.rfind(|x| !x.is_empty()).map(percent_decode));
        let name = [file_name.map(String::from), disp, from_url]
            .into_iter()
            .flatten()
            .find(|n| safe_name(n) && format_of_file(n).is_some())
            .ok_or_else(|| {
                Error::Invalid(
                    "non riconosco il tipo di file: serve un URL a un file .gguf, .litertlm, .pte o .llamafile (o indica il nome del file)"
                        .into(),
                )
            })?;
        let format = format_of_file(&name).unwrap_or("gguf");
        let stem = name.rsplit_once('.').map(|x| x.0).unwrap_or(&name).to_string();
        let file = RemoteFile { path: name.clone(), size_bytes: size, sha256: sha256.map(str::to_ascii_lowercase), role: "weights".into() };
        let model = RemoteModel {
            provider: "url".into(),
            id: parsed.to_string(),
            name: stem,
            publisher: host.to_string(),
            description: None,
            formats: vec![format.into()],
            tags: vec![],
            capabilities: vec!["chat".into()],
            downloads: None,
            likes: None,
            updated_at: None,
            license: None,
            gated: false,
            params_b: super::variants::params_from_name(&name),
            url: String::new(),
        };
        let variant = ModelVariant {
            id: "file".into(),
            label: name,
            format: format.into(),
            quant: super::variants::parse_quant(&file.path),
            size_bytes: size,
            files: vec![file],
            fit: String::new(),
            fit_detail: String::new(),
            recommended: false,
            runtimes: vec![],
            installed: false,
        };
        Ok(RemoteModelDetail { model, revision: "url".into(), variants: vec![variant] })
    }
}

fn host_dir(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| format!("{h}{}", u.port().map(|p| format!("_{p}")).unwrap_or_default())))
        .unwrap_or_else(|| "unknown".into())
        .replace([':', '/', '\\'], "_")
}

/// Longest part of the file name kept in the folder name (the rest is the hash).
const STEM_CHARS: usize = 48;

/// Folder of one direct URL, relative to the models directory: `url/<host>/<file stem>-<hash>`.
///
/// Every URL gets a folder of its own, because an install is recorded by the sidecar of its
/// folder and deleted with it: two files of the same host must never share one (they would
/// overwrite each other's record and deleting one would delete the other). The hash of the whole
/// URL tells apart `/a/model.gguf` and `/b/model.gguf`; the stem only keeps the folder readable.
fn url_folder(url: &str, file: &str) -> PathBuf {
    let stem = file.rsplit_once('.').map(|x| x.0).unwrap_or(file);
    let stem: String = stem.chars().take(STEM_CHARS).map(|c| if c.is_control() || "<>\"|?*".contains(c) { '_' } else { c }).collect();
    // Windows drops trailing dots and spaces from a folder name: keep the name as written.
    let stem = stem.trim_end_matches(['.', ' ']);
    let stem = if stem.is_empty() { "file" } else { stem };
    let hash: String = Sha256::digest(url.as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect();
    PathBuf::from("url").join(host_dir(url)).join(format!("{stem}-{hash}"))
}

#[async_trait]
impl ModelProvider for UrlProvider {
    fn info(&self) -> ModelProviderInfo {
        ModelProviderInfo {
            id: "url".into(),
            name: "URL diretto".into(),
            description:
                "Scarica un singolo file (.gguf, .litertlm, .pte, .llamafile) da qualsiasi indirizzo http(s). Nessun token viene inviato."
                    .into(),
            homepage: String::new(),
            capabilities: vec!["files".into()],
            formats: ["gguf", "litertlm", "pte", "llamafile"].map(String::from).to_vec(),
            auth: "none".into(),
            auth_configured: false,
            endpoint: String::new(),
        }
    }

    async fn search(&self, _q: &SearchQuery) -> Result<SearchPage> {
        Ok(SearchPage::default())
    }

    async fn detail(&self, model: &str, _revision: Option<&str>) -> Result<RemoteModelDetail> {
        self.detail_with(model, None, None).await
    }

    fn plan(&self, detail: &RemoteModelDetail, variant: &str) -> Result<Vec<FileJob>> {
        let v =
            detail.variants.iter().find(|v| v.id == variant).ok_or_else(|| Error::NotFound(format!("variante `{variant}` non trovata")))?;
        let folder = self.folder(detail, variant);
        v.files
            .iter()
            .map(|f| {
                Ok(FileJob {
                    url: detail.model.id.clone(),
                    dest: folder.join(native_rel(&f.path)?),
                    size: f.size_bytes,
                    sha256: f.sha256.clone(),
                    headers: vec![],
                })
            })
            .collect()
    }

    fn folder(&self, detail: &RemoteModelDetail, variant: &str) -> PathBuf {
        let file =
            detail.variants.iter().find(|v| v.id == variant).and_then(|v| v.files.first()).map(|f| f.path.as_str()).unwrap_or("file");
        url_folder(&detail.model.id, file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::{get, head};
    use axum::Router;

    async fn server() -> String {
        let app = Router::new()
            .route("/m/tiny-Q4_K_M.gguf", head(|| async { ([("content-length", "1234"), ("accept-ranges", "bytes")], "") }))
            .route(
                "/dl",
                head(|| async { ([("content-length", "99"), ("content-disposition", "attachment; filename=\"served.Q8_0.gguf\"")], "") }),
            )
            .route("/nohead/x.gguf", get(|| async { "x" }).head(|| async { StatusCode::METHOD_NOT_ALLOWED }))
            .route("/blob/x.gguf", head(|| async { ([("content-type", "text/html; charset=utf-8"), ("content-length", "900")], "") }))
            .route("/gone/x.gguf", head(|| async { StatusCode::NOT_FOUND.into_response() }))
            .route(
                "/doc.txt",
                head(|h: HeaderMap| async move {
                    let _ = h;
                    ([("content-length", "5")], "")
                }),
            );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}")
    }

    #[test]
    fn decodes_names() {
        assert_eq!(percent_decode("a%20b.gguf"), "a b.gguf");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(disposition_name("attachment; filename*=UTF-8''my%20model.gguf").as_deref(), Some("my model.gguf"));
        assert_eq!(disposition_name("attachment; filename=\"x.gguf\"").as_deref(), Some("x.gguf"));
        assert!(!safe_name("../x.gguf") && !safe_name("a/b.gguf") && !safe_name(".hidden.gguf") && safe_name("ok.gguf"));
        assert_eq!(format_of_file("A.GGUF"), Some("gguf"));
        assert_eq!(format_of_file("a.zip"), None);
    }

    #[test]
    fn every_url_has_a_folder_of_its_own() {
        let folder = |url: &str, file: &str| url_folder(url, file);
        let a = folder("http://example.com/m/a.gguf", "a.gguf");
        let b = folder("http://example.com/m/b.gguf", "b.gguf");
        assert_ne!(a, b, "two files of one host are two models");
        assert_eq!(a.parent(), b.parent());
        // Same file name at different paths of the same host.
        assert_ne!(folder("http://example.com/x/model.gguf", "model.gguf"), folder("http://example.com/y/model.gguf", "model.gguf"));
        // Same query string, same folder (the same URL is the same model); another query is another one.
        assert_eq!(folder("http://example.com/dl?id=1", "m.gguf"), folder("http://example.com/dl?id=1", "m.gguf"));
        assert_ne!(folder("http://example.com/dl?id=1", "m.gguf"), folder("http://example.com/dl?id=2", "m.gguf"));
        // Another host, another port: another parent.
        assert_ne!(a.parent(), folder("http://example.com:8080/m/a.gguf", "a.gguf").parent());
        // Every component is a plain name, whatever the file is called.
        for (url, file) in [
            ("http://h/x", "Qwen3 (v2) <x>.gguf"),
            ("http://h/x", &format!("{}.gguf", "é".repeat(300))),
            ("http://h/x", "a. .gguf"),
            ("http://[::1]:8080/x", "m.gguf"),
        ] {
            let f = folder(url, file);
            assert!(f.components().all(|c| crate::relpath::is_plain_component(&c.as_os_str().to_string_lossy())), "{f:?}");
            assert!(f.file_name().unwrap().to_string_lossy().chars().count() <= STEM_CHARS + 9, "{f:?}");
        }
    }

    #[tokio::test]
    async fn a_web_page_is_refused_as_a_model_url() {
        let base = server().await;
        let e = UrlProvider::new().detail_with(&format!("{base}/blob/x.gguf"), None, None).await.unwrap_err();
        assert!(matches!(e, Error::Invalid(_)), "{e:?}");
        assert!(e.to_string().contains("pagina web"), "{e}");
    }

    #[tokio::test]
    async fn heads_for_size_and_names_the_file() {
        let base = server().await;
        let p = UrlProvider::new();
        let d = p.detail_with(&format!("{base}/m/tiny-Q4_K_M.gguf"), None, Some(&"a".repeat(64))).await.unwrap();
        assert_eq!(d.variants[0].files[0].path, "tiny-Q4_K_M.gguf");
        assert_eq!(d.variants[0].size_bytes, 1234);
        assert_eq!(d.variants[0].quant.as_deref(), Some("Q4_K_M"));
        assert_eq!(d.variants[0].files[0].sha256.as_deref(), Some("a".repeat(64).as_str()));
        let jobs = p.plan(&d, "file").unwrap();
        let host = base.trim_start_matches("http://").replace(':', "_");
        // `url/<host>/<stem>-<hash of the URL>/<file>`: a folder per URL.
        let folder = jobs[0].dest.parent().unwrap().to_path_buf();
        assert_eq!(folder, p.folder(&d, "file"));
        assert_eq!(folder.parent().unwrap(), PathBuf::from(format!("url/{host}")));
        let name = folder.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("tiny-Q4_K_M-") && name.len() == "tiny-Q4_K_M-".len() + 8, "{name}");
        assert_eq!(jobs[0].dest.file_name().unwrap(), "tiny-Q4_K_M.gguf");
        assert!(jobs[0].headers.is_empty());
        assert_eq!(jobs[0].url, format!("{base}/m/tiny-Q4_K_M.gguf"));

        // The server names the file; the user can too.
        assert_eq!(p.detail_with(&format!("{base}/dl"), None, None).await.unwrap().variants[0].files[0].path, "served.Q8_0.gguf");
        assert_eq!(p.detail_with(&format!("{base}/dl"), Some("mine.gguf"), None).await.unwrap().variants[0].files[0].path, "mine.gguf");
        // HEAD refused: size unknown, still fine.
        assert_eq!(p.detail_with(&format!("{base}/nohead/x.gguf"), None, None).await.unwrap().variants[0].size_bytes, 0);
        match p.detail_with(&format!("{base}/gone/x.gguf"), None, None).await {
            Err(Error::NotFound(m)) => assert!(m.starts_with("il file non è stato trovato su 127.0.0.1"), "{m}"),
            other => panic!("{other:?}"),
        }
        // Wrong scheme, wrong type, bad sha.
        assert!(p.detail_with("file:///etc/passwd", None, None).await.is_err());
        assert!(p.detail_with(&format!("{base}/doc.txt"), None, None).await.is_err());
        assert!(p.detail_with(&format!("{base}/m/tiny-Q4_K_M.gguf"), None, Some("zz")).await.is_err());
    }
}
