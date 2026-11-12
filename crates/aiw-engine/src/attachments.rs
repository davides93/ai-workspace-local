//! Attachments of the user messages in chat: validation, storage in the data folder, kind
//! detection, PDF text extraction and the pure code that turns a stored message and its files into
//! what the model receives.
//!
//! Files live in `<data>/attachments/<session id>/<attachment id>-<file name>` (a PDF with text also
//! has `<attachment id>.extracted.txt`). The stored message keeps only metadata; the bytes are
//! never written into the workspace.

use crate::Engine;
use aiw_agent::{to_msgs, UserExtra};
use aiw_core::api::{
    Attachment, AttachmentData, AttachmentKind, AttachmentUpload, ChatMessage, MAX_ATTACHMENTS_PER_MESSAGE, MAX_ATTACHMENTS_TOTAL_BYTES,
    MAX_ATTACHMENT_BYTES,
};
use aiw_core::{ids::new_id, Error, Result};
use aiw_runtimes::{ContentPart, ImageUrl, Msg, MsgContent};
use base64::Engine as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Characters of one text attachment given to the model.
pub(crate) const TEXT_BUDGET: usize = 40_000;
/// Characters of all the text attachments of one message.
pub(crate) const TOTAL_TEXT_BUDGET: usize = 120_000;
/// Earlier user messages whose images are sent again.
pub(crate) const HISTORY_IMAGE_MESSAGES: usize = 2;
/// Characters of text kept from a PDF.
const MAX_EXTRACTED_CHARS: usize = 2_000_000;
/// Bytes read from a text file to fill [`TEXT_BUDGET`] (a character takes at most 4 bytes).
const READ_CAP_BYTES: u64 = (TEXT_BUDGET as u64) * 4 + 4;
/// How long the extraction of the text of a PDF may take before the file is treated as plain.
const PDF_TIMEOUT: Duration = Duration::from_secs(30);
/// Suffix of the sidecar file with the text of a PDF.
const EXTRACTED_SUFFIX: &str = ".extracted.txt";
/// Longest stored file name, in bytes.
const MAX_NAME_BYTES: usize = 120;

/// The warning of the `aiw.attachments` event when the model cannot see images.
pub const NO_VISION_WARNING: &str = "Questo modello non vede le immagini: scegline uno con visione in Modelli";
/// Name of the custom event that carries the warning.
pub const ATTACHMENTS_EVENT: &str = "aiw.attachments";

// ------------------------------------------------------------------ names and kinds

/// A file name that is safe to put in a path: no folders, no control characters, no leading dots,
/// a bounded length that keeps the extension.
pub(crate) fn sanitize_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base.chars().filter(|c| !c.is_control() && !"<>:\"|?*".contains(*c)).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim_end_matches(['.', ' ']).to_string();
    if cleaned.is_empty() {
        return "allegato".into();
    }
    if cleaned.len() <= MAX_NAME_BYTES {
        return cleaned;
    }
    let (stem, ext) = match cleaned.rfind('.') {
        Some(i) if cleaned.len() - i <= 16 && i > 0 => (&cleaned[..i], &cleaned[i..]),
        _ => (cleaned.as_str(), ""),
    };
    let mut stem: String = stem.to_string();
    while stem.len() + ext.len() > MAX_NAME_BYTES {
        stem.pop();
    }
    format!("{stem}{ext}")
}

fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// The image type recognised from the first bytes (the declared type is not trusted).
fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn mime_from_extension(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "txt" | "log" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "yaml" | "yml" => "application/yaml",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "zip" => "application/zip",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => return None,
    })
}

fn plausible_mime(m: &str) -> bool {
    let m = m.trim();
    m.len() <= 100 && m.contains('/') && m.chars().all(|c| c.is_ascii_alphanumeric() || "/.+-;= _".contains(c))
}

/// The kind and the mime type of a file, from its content first, then its declared type and
/// extension. A PDF is `Document` here; it becomes `File` when no text can be extracted from it.
pub(crate) fn detect(name: &str, declared_mime: &str, bytes: &[u8]) -> (AttachmentKind, String) {
    if let Some(mime) = sniff_image(bytes) {
        return (AttachmentKind::Image, mime.into());
    }
    if bytes.starts_with(b"%PDF") {
        return (AttachmentKind::Document, "application/pdf".into());
    }
    let declared = declared_mime.trim().to_ascii_lowercase();
    let guessed = || {
        if plausible_mime(&declared) && declared != "application/octet-stream" {
            declared.clone()
        } else {
            mime_from_extension(&extension(name)).map(String::from).unwrap_or_else(|| "application/octet-stream".into())
        }
    };
    // Declared images that are not one of the supported formats (svg, heic…) stay plain files.
    let is_text = !bytes.is_empty() && !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok() && !declared.starts_with("image/");
    if is_text {
        let mime = guessed();
        let mime = if mime == "application/octet-stream" { "text/plain".into() } else { mime };
        return (AttachmentKind::Text, mime);
    }
    (AttachmentKind::File, guessed())
}

fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} byte")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB).replace('.', ",")
    } else {
        format!("{:.1} MB", b / KB / KB).replace('.', ",")
    }
}

// ------------------------------------------------------------------ validation

/// A validated upload, ready to be written to disk.
pub(crate) struct Prepared {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
    pub kind: AttachmentKind,
}

/// Checks the limits, decodes the base64 and detects the kind of every file. Errors are in Italian
/// and meant for the person (HTTP 400).
pub(crate) fn prepare(uploads: Vec<AttachmentUpload>) -> Result<Vec<Prepared>> {
    if uploads.len() > MAX_ATTACHMENTS_PER_MESSAGE {
        return Err(Error::Invalid(format!("Puoi allegare al massimo {MAX_ATTACHMENTS_PER_MESSAGE} file a un messaggio.")));
    }
    let too_big = |name: &str| {
        Error::Invalid(format!("«{name}» è troppo grande: ogni file può pesare al massimo {} MB.", MAX_ATTACHMENT_BYTES >> 20))
    };
    let total_err = || {
        Error::Invalid(format!("Gli allegati pesano troppo: in totale al massimo {} MB per messaggio.", MAX_ATTACHMENTS_TOTAL_BYTES >> 20))
    };
    // Cheap size check on the encoded text, before decoding anything.
    let mut estimate = 0usize;
    for u in &uploads {
        let approx = u.data.len() / 4 * 3;
        if approx > MAX_ATTACHMENT_BYTES + 3 {
            return Err(too_big(&sanitize_name(&u.name)));
        }
        estimate += approx;
    }
    if estimate > MAX_ATTACHMENTS_TOTAL_BYTES + 3 * MAX_ATTACHMENTS_PER_MESSAGE {
        return Err(total_err());
    }
    let mut out = Vec::with_capacity(uploads.len());
    let mut total = 0usize;
    for u in uploads {
        let name = sanitize_name(&u.name);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(u.data.trim())
            .map_err(|_| Error::Invalid(format!("L'allegato «{name}» non è leggibile: il contenuto non è valido.")))?;
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(too_big(&name));
        }
        total += bytes.len();
        if total > MAX_ATTACHMENTS_TOTAL_BYTES {
            return Err(total_err());
        }
        let (kind, mime) = detect(&name, &u.mime, &bytes);
        out.push(Prepared { name, mime, bytes, kind });
    }
    Ok(out)
}

// ------------------------------------------------------------------ PDF

/// The text of a PDF, or `None` when there is none (a scan), the file is damaged, or the extractor
/// panics or takes too long. Runs on a blocking thread.
async fn extract_pdf_text(bytes: Vec<u8>) -> Option<String> {
    let job = tokio::task::spawn_blocking(move || std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(&bytes)));
    let text = match tokio::time::timeout(PDF_TIMEOUT, job).await {
        Ok(Ok(Ok(Ok(t)))) => t,
        _ => return None,
    };
    let text = tidy_text(&text);
    text.chars().any(|c| c.is_alphanumeric()).then(|| text.chars().take(MAX_EXTRACTED_CHARS).collect())
}

/// Trims lines and collapses runs of blank lines.
fn tidy_text(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut blank = 0;
    for line in t.replace('\r', "").lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

// ------------------------------------------------------------------ storage

fn safe_component(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl Engine {
    pub(crate) fn attachments_dir(&self, session_id: &str) -> PathBuf {
        self.0.dirs.attachments().join(session_id)
    }

    /// Writes the files of a message (and the text of its PDFs) to the session folder. On failure
    /// nothing is left behind.
    pub(crate) async fn save_attachments(&self, session_id: &str, files: Vec<Prepared>) -> Result<Vec<Attachment>> {
        if files.is_empty() {
            return Ok(vec![]);
        }
        if !safe_component(session_id) {
            return Err(Error::Invalid("sessione non valida".into()));
        }
        let dir = self.attachments_dir(session_id);
        let io = |e: std::io::Error| Error::Other(format!("Non riesco a salvare l'allegato: {e}"));
        std::fs::create_dir_all(&dir).map_err(io)?;
        let mut written: Vec<PathBuf> = vec![];
        let mut out = Vec::with_capacity(files.len());
        for f in files {
            let id = new_id("att");
            let path = dir.join(format!("{id}-{}", f.name));
            if let Err(e) = std::fs::write(&path, &f.bytes) {
                cleanup(&written);
                return Err(io(e));
            }
            written.push(path);
            let mut kind = f.kind;
            if kind == AttachmentKind::Document {
                match extract_pdf_text(f.bytes.clone()).await {
                    Some(text) => {
                        let side = dir.join(format!("{id}{EXTRACTED_SUFFIX}"));
                        match std::fs::write(&side, text) {
                            Ok(()) => written.push(side),
                            Err(e) => {
                                cleanup(&written);
                                return Err(io(e));
                            }
                        }
                    }
                    None => kind = AttachmentKind::File,
                }
            }
            out.push(Attachment { id, name: f.name, mime: f.mime, size: f.bytes.len() as u64, kind });
        }
        Ok(out)
    }

    /// Removes the folder of a session's attachments (the session is being deleted).
    pub(crate) fn remove_attachments(&self, session_id: &str) {
        if !safe_component(session_id) {
            return;
        }
        let dir = self.attachments_dir(session_id);
        if dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                tracing::warn!("allegati di {session_id}: {e}");
            }
        }
    }

    /// Where an attachment is, looked up by listing the session folder (the id is never joined into
    /// a path).
    pub(crate) fn attachment_path(&self, session_id: &str, att_id: &str) -> Option<PathBuf> {
        if !safe_component(session_id) || !safe_component(att_id) {
            return None;
        }
        let prefix = format!("{att_id}-");
        std::fs::read_dir(self.attachments_dir(session_id))
            .ok()?
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(&prefix)))
            .map(|e| e.path())
    }

    /// An attachment of a session, for the UI (thumbnails, downloads).
    pub fn attachment_data(&self, session_id: &str, att_id: &str) -> Result<AttachmentData> {
        let missing = || Error::NotFound("allegato non trovato".into());
        self.0.store.session(session_id)?;
        let meta = self
            .0
            .store
            .messages(session_id)?
            .into_iter()
            .flat_map(|m| m.attachments.unwrap_or_default())
            .find(|a| a.id == att_id)
            .ok_or_else(missing)?;
        let path = self.attachment_path(session_id, att_id).ok_or_else(missing)?;
        let bytes = std::fs::read(path).map_err(|_| missing())?;
        Ok(AttachmentData { name: meta.name, mime: meta.mime, data: base64::engine::general_purpose::STANDARD.encode(bytes) })
    }

    /// Does the model of a profile see images? `None` when the model is not known.
    pub(crate) fn vision_support(&self, profile: &aiw_core::api::InferenceProfile) -> Option<bool> {
        let models = self.0.models.read().unwrap();
        models.iter().find(|m| m.id == profile.model).map(|m| m.capabilities.iter().any(|c| c == "vision"))
    }

    /// Reads the files of the attachments of one message. Images are read only when `want_images`.
    pub(crate) fn load_attachment_files(&self, session_id: &str, atts: &[Attachment], want_images: bool) -> Vec<AttFile> {
        atts.iter()
            .map(|a| {
                let path = self.attachment_path(session_id, &a.id);
                let mut f = AttFile { att: a.clone(), path: path.clone(), readable_path: path.clone(), text: None, image: None };
                let Some(path) = path else { return f };
                match a.kind {
                    AttachmentKind::Image if want_images => {
                        if let Ok(bytes) = std::fs::read(&path) {
                            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                            f.image = Some(format!("data:{};base64,{b64}", a.mime));
                        }
                    }
                    AttachmentKind::Text => f.text = read_capped(&path),
                    AttachmentKind::Document => {
                        let side = path.with_file_name(format!("{}{EXTRACTED_SUFFIX}", a.id));
                        f.text = read_capped(&side);
                        f.readable_path = Some(side);
                    }
                    _ => {}
                }
                f
            })
            .collect()
    }

    /// The conversation as the model gets it: [`to_msgs`] plus the attachments of the user messages.
    /// `vision` is false only when the model is known not to see images.
    pub(crate) fn history_with_attachments(&self, session_id: &str, stored: &[ChatMessage], vision: bool) -> Vec<Msg> {
        build_history(stored, vision, &mut |m, want_images| {
            self.load_attachment_files(session_id, m.attachments.as_deref().unwrap_or_default(), want_images)
        })
    }
}

fn cleanup(paths: &[PathBuf]) {
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}

fn read_capped(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path).ok()?.take(READ_CAP_BYTES).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

// ------------------------------------------------------------------ what the model receives

/// An attachment and what could be read of it. `path` is `None` when the file is gone.
#[derive(Debug, Clone)]
pub(crate) struct AttFile {
    pub att: Attachment,
    pub path: Option<PathBuf>,
    /// Where the full text can be read: the file itself, or the extracted text of a PDF.
    pub readable_path: Option<PathBuf>,
    /// Beginning of the text (`Text` and `Document`).
    pub text: Option<String>,
    /// `data:` URL (images that are sent).
    pub image: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Rendered {
    /// Appended to the typed text.
    pub text: String,
    pub images: Vec<String>,
    /// An image was attached but the model cannot see images.
    pub vision_blocked: bool,
}

fn clip_chars(s: &str, max: usize) -> (&str, bool) {
    match s.char_indices().nth(max) {
        Some((i, _)) => (&s[..i], true),
        None => (s, false),
    }
}

/// The text (and image URLs) that stand for `files` in a message. `send_images` is false for the
/// images of older messages; `vision` false when the model cannot see them at all.
pub(crate) fn render_attachments(files: &[AttFile], send_images: bool, vision: bool) -> Rendered {
    let mut r = Rendered::default();
    let mut budget = TOTAL_TEXT_BUDGET;
    for f in files {
        let name = &f.att.name;
        let Some(path) = &f.path else {
            r.text.push_str(&format!("\n\n[Allegato: {name}] Il file non è più disponibile."));
            continue;
        };
        let shown = path.display();
        match f.att.kind {
            AttachmentKind::Image => {
                if !vision {
                    r.vision_blocked = true;
                    r.text.push_str(&format!(
                        "\n\n[Immagine allegata: {name}] Il modello in uso non può vedere le immagini, quindi non la vedi. Se serve, dillo all'utente."
                    ));
                } else if let (true, Some(url)) = (send_images, &f.image) {
                    r.images.push(url.clone());
                    r.text.push_str(&format!("\n\n[Immagine allegata: {name}]"));
                } else {
                    r.text.push_str(&format!(
                        "\n\n[Immagine allegata: {name}] Non è più mostrata in questa conversazione; è salvata in {shown}."
                    ));
                }
            }
            AttachmentKind::Text | AttachmentKind::Document => {
                let readable = f.readable_path.as_ref().unwrap_or(path).display();
                match &f.text {
                    Some(content) if budget > 0 => {
                        let (body, clipped) = clip_chars(content, TEXT_BUDGET.min(budget));
                        // A file longer than what was read is clipped too, even if the read part fits.
                        let clipped = clipped || f.att.size as usize > READ_CAP_BYTES as usize && f.att.kind == AttachmentKind::Text;
                        budget = budget.saturating_sub(body.chars().count());
                        r.text.push_str(&format!("\n\n[Allegato: {name}]\n{body}"));
                        if clipped {
                            r.text.push_str(&format!(
                                "\n[… troncato: il resto non è incluso; puoi leggere il contenuto completo in {readable}]"
                            ));
                        }
                        r.text.push_str("\n[Fine allegato]");
                    }
                    Some(_) => r.text.push_str(&format!(
                        "\n\n[Allegato: {name}] Contenuto non incluso: questo messaggio ha già troppo testo allegato. Puoi leggerlo con gli strumenti in {readable}."
                    )),
                    None => r.text.push_str(&format!(
                        "\n\n[Allegato: {name}] File allegato ({}, {}), non incluso nel messaggio. Si trova in {shown}: puoi aprirlo con gli strumenti.",
                        f.att.mime,
                        human_size(f.att.size)
                    )),
                }
            }
            AttachmentKind::File => {
                let why = if f.att.mime == "application/pdf" { " È un PDF senza testo leggibile (forse una scansione)." } else { "" };
                r.text.push_str(&format!(
                    "\n\n[Allegato: {name}] File allegato ({}, {}), non incluso nel messaggio.{why} Si trova in {shown}: puoi aprirlo con gli strumenti.",
                    f.att.mime,
                    human_size(f.att.size)
                ));
            }
        }
    }
    r
}

/// The attachments of the message of the run being started, for [`aiw_agent::RunInput`].
pub(crate) fn user_extra(files: &[AttFile], vision: bool) -> (UserExtra, bool) {
    let r = render_attachments(files, true, vision);
    (UserExtra { text: r.text, images: r.images }, r.vision_blocked)
}

fn has_image(m: &ChatMessage) -> bool {
    m.attachments.as_ref().is_some_and(|a| a.iter().any(|x| x.kind == AttachmentKind::Image))
}

/// Converts the stored conversation to messages for the model. User messages with attachments get
/// their text blocks every time; their images are sent again only for the last
/// [`HISTORY_IMAGE_MESSAGES`] messages that have any. `load` reads the files of a message (and the
/// images only when asked).
pub(crate) fn build_history(stored: &[ChatMessage], vision: bool, load: &mut dyn FnMut(&ChatMessage, bool) -> Vec<AttFile>) -> Vec<Msg> {
    let with_images: Vec<usize> = stored
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == "user" && has_image(m) && !to_msgs(std::slice::from_ref(m)).is_empty())
        .map(|(i, _)| i)
        .collect();
    let recent: Vec<usize> = with_images.iter().rev().take(HISTORY_IMAGE_MESSAGES).copied().collect();
    let mut out = Vec::with_capacity(stored.len());
    for (i, m) in stored.iter().enumerate() {
        let Some(mut msg) = to_msgs(std::slice::from_ref(m)).into_iter().next() else { continue };
        if m.role == "user" && m.attachments.as_ref().is_some_and(|a| !a.is_empty()) {
            let want_images = recent.contains(&i);
            let files = load(m, want_images && vision);
            let r = render_attachments(&files, want_images, vision);
            let text = format!("{}{}", m.content, r.text);
            msg.content = Some(if r.images.is_empty() {
                MsgContent::Text(text)
            } else {
                let mut parts = vec![ContentPart::Text { text }];
                parts.extend(r.images.into_iter().map(|url| ContentPart::ImageUrl { image_url: ImageUrl { url } }));
                MsgContent::Parts(parts)
            });
        }
        out.push(msg);
    }
    out
}
