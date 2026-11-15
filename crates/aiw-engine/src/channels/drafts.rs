//! Email drafts: an email prepared by the assistant (or written by the owner) that waits for the owner's review and
//! leaves only after they approve exactly what they saw.
//!
//! This is **not** the automatic reply of an email channel (those go only to the people on the channel's list, in the
//! same thread, and carry `Auto-Submitted`). A draft can have any recipient, so the review is the only barrier against
//! content an injected instruction managed to get into a draft. What keeps that barrier honest:
//!
//! * **There is no tool that sends.** The assistant can only create or revise a draft (`email_draft`); sending is the
//!   owner's `POST /api/email-drafts/{id}/send`, which needs full access (a paired phone, a channel and a run cannot).
//! * **The approval names the content.** `content_hash` is the SHA-256 of everything that goes on the wire (channel,
//!   sender, to, cc, bcc, subject, body, threading headers, and for each attachment its name, type, size and hash),
//!   computed here from the draft as it is now. The owner sends back the hash they were shown (`approved_hash`): a draft
//!   changed after the review, or whose sender changed, is refused (409) and has to be looked at again.
//! * **Attachments are copied when they are attached**, through the same path rules as the file tools (workspace
//!   boundary, hidden data folder and credentials), and their hash is part of the approval: the file cannot change
//!   afterwards, and a path outside the workspace cannot be attached at all.
//! * **One send per draft.** `draft → sending → sent` is a compare-and-swap under a lock; a second request for a draft
//!   that is already sent returns it as it is (nothing is sent again), one that arrives while it is being sent is
//!   refused. The `Message-ID` is derived from the id of the draft, so even a repeated attempt carries the same one. A
//!   failure after the message was handed to the server (or an answer that never came) is `uncertain`, and is sent
//!   again only if the owner says so explicitly.
//! * **The channel that sends has to be on**, and the kill switch stops sending: same rule as `channel_send`.
//! * **Everything is in the activity log** (who wrote it, who approved or refused it, to whom it went): recipients,
//!   subject and attachment names, never the body.

use super::smtp::{self, Attach, Outgoing};
use super::*;
use aiw_tools::{SandboxMode, ToolContext};
use base64::Engine as _;
use sha2::{Digest, Sha256};

const MAX_BODY_CHARS: usize = 100_000;
const MAX_SUBJECT_CHARS: usize = 300;
const MAX_RECIPIENTS: usize = 20;
const MAX_ATTACHMENTS: usize = 5;
const MAX_FILE_BYTES: u64 = 5 << 20;
const MAX_TOTAL_BYTES: u64 = 10 << 20;
const MAX_OPEN: usize = 50;
const SENDS_PER_HOUR: usize = 20;
/// A draft «sending» for longer than this with nobody sending it (the app was closed in the middle) is `uncertain`.
const STUCK_AFTER: Duration = Duration::from_secs(90);
/// An identical draft younger than this is a duplicate worth pointing at.
const DUPLICATE_WINDOW: Duration = Duration::from_secs(24 * 3600);

#[derive(Default)]
pub(super) struct Runtime {
    /// Drafts being sent by this process right now.
    inflight: Mutex<HashSet<String>>,
    /// Sends of the last hour.
    sent: Mutex<VecDeque<Instant>>,
    /// Serialises every change of a draft (the compare-and-swap of its status).
    lock: Mutex<()>,
}

/// The bytes of an attachment, kept next to the draft. The API never shows them.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredFile {
    name: String,
    mime: String,
    sha256: String,
    source: String,
    size: u64,
    /// base64
    data: String,
}

impl StoredFile {
    fn meta(&self) -> DraftAttachment {
        DraftAttachment {
            name: self.name.clone(),
            mime: self.mime.clone(),
            size: self.size,
            sha256: self.sha256.clone(),
            source: self.source.clone(),
        }
    }
}

/// What a new draft is made of, from the API or from the tool.
pub(crate) struct NewDraft {
    pub channel: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub attachments: Vec<String>,
    pub source: Option<String>,
    /// `assistant` | `user`
    pub created_by: &'static str,
    pub session_id: Option<String>,
}

fn mime_of(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("pdf") => "application/pdf",
        Some("txt" | "md" | "log") => "text/plain",
        Some("csv") => "text/csv",
        Some("html" | "htm") => "text/html",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("zip") => "application/zip",
        Some("ics") => "text/calendar",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

fn sha_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Characters that print as nothing, or change the direction of the text around them (zero-width and bidirectional
/// controls, invisible operators and fillers, the byte order mark, the soft hyphen, the Unicode «tag» and extra
/// variation-selector blocks): they can carry text the owner cannot see on the card, or make a file name read as another,
/// so they never get into a draft.
fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{034F}' | '\u{115F}' | '\u{1160}' | '\u{17B4}' | '\u{17B5}' | '\u{180B}'..='\u{180E}'
        | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{3164}' | '\u{FEFF}'
        | '\u{FFA0}' | '\u{E0000}'..='\u{E007F}' | '\u{E0100}'..='\u{E01EF}')
}

/// `s` without the characters of [`is_invisible`].
fn visible(s: &str) -> String {
    s.chars().filter(|c| !is_invisible(*c)).collect()
}

fn one_line(s: &str) -> String {
    visible(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Addresses as the person wrote them, checked, without duplicates (compared lower-case).
fn clean_addresses(label: &str, list: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for raw in list {
        let cleaned = visible(raw);
        let a = cleaned.trim();
        if a.is_empty() {
            continue;
        }
        if !smtp::valid_address(a) {
            return Err(Error::Invalid(format!("«{}» non è un indirizzo email valido ({label})", text::excerpt(a, 60))));
        }
        if !out.iter().any(|x| x.eq_ignore_ascii_case(a)) {
            out.push(a.to_string());
        }
    }
    Ok(out)
}

fn clean_header(s: &Option<String>) -> Option<String> {
    s.as_deref()
        .map(|v| one_line(&v.chars().filter(|c| !c.is_control() || *c == ' ').collect::<String>()))
        .filter(|v| !v.is_empty())
        .map(|v| v.chars().take(1000).collect())
}

impl Engine {
    // ------------------------------------------------------------------ store

    pub(crate) fn draft_doc(&self, id: &str) -> Result<EmailDraft> {
        self.0.store.get_doc::<EmailDraft>("email_draft", id)?.ok_or_else(|| Error::NotFound(format!("bozza {id} non trovata")))
    }

    fn draft_files(&self, id: &str) -> Vec<StoredFile> {
        self.0.store.get_doc::<Vec<StoredFile>>("email_draft_files", id).ok().flatten().unwrap_or_default()
    }

    fn drafts_raw(&self) -> Vec<EmailDraft> {
        let mut v = self.0.store.list_docs::<EmailDraft>("email_draft").unwrap_or_default();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| b.id.cmp(&a.id)));
        v
    }

    fn draft_audit(&self, actor: &str, action: &str, decision: Option<&str>, d: &EmailDraft, extra: &str) {
        let who = if d.to.is_empty() { "nessun destinatario".to_string() } else { d.to.join(", ") };
        let cc = if d.cc.is_empty() { String::new() } else { format!(", cc {}", d.cc.join(", ")) };
        let bcc = if d.bcc.is_empty() { String::new() } else { format!(", ccn {}", d.bcc.join(", ")) };
        let files = if d.attachments.is_empty() {
            String::new()
        } else {
            format!(", allegati: {}", d.attachments.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "))
        };
        let _ = self.0.store.add_audit(&AuditEntry {
            id: new_id("aud"),
            ts: now_rfc3339(),
            run_id: None,
            actor: actor.into(),
            action: action.into(),
            tool: None,
            decision: decision.map(String::from),
            detail: format!(
                "bozza {} a {who}{cc}{bcc}{files}: «{}» (revisione {}, contenuto {}){}{extra}",
                d.id,
                text::excerpt(&d.subject, 80),
                d.revision,
                d.content_hash.chars().take(12).collect::<String>(),
                if extra.is_empty() { "" } else { " — " }
            ),
        });
    }

    // ------------------------------------------------------------------ content and its hash

    /// The email channel a draft goes through: the one asked for, or the only (first) one.
    fn draft_channel(&self, wanted: Option<&str>) -> Result<Channel> {
        let all: Vec<Channel> = self.channels_raw().into_iter().filter(|c| c.kind == "email").collect();
        let ch = match wanted.map(str::trim).filter(|w| !w.is_empty()) {
            Some(w) => {
                let needle = w.to_lowercase();
                all.into_iter().find(|c| c.id == w || c.name.to_lowercase() == needle).ok_or_else(|| {
                    Error::NotFound(format!("canale email «{w}» non trovato: serve un canale di tipo Email, perché è lui a conoscere il server di invio"))
                })?
            }
            None => all.into_iter().next().ok_or_else(|| {
                Error::Invalid("per preparare un'email serve un canale di tipo Email (Canali → Aggiungi un canale): è lui a conoscere il server di invio".into())
            })?,
        };
        Ok(ch)
    }

    /// The address a draft goes out from.
    fn draft_from(&self, ch: &Channel) -> String {
        super::email::sender_address(ch)
    }

    /// SHA-256 of everything that goes on the wire. It does not change unless something that is sent does. The attachments
    /// enter by name, type, size and hash, as kept in the draft (they are written together with the files, and checked
    /// against them when the draft is sent): listing drafts never has to read the files.
    fn draft_hash(&self, d: &EmailDraft, from: &str) -> String {
        let attachments: Vec<Value> = d.attachments.iter().map(|a| json!([a.name, a.mime, a.size, a.sha256])).collect();
        let canonical = json!([
            "aiw-email-draft-v1",
            d.channel_id,
            from,
            d.to,
            d.cc,
            d.bcc,
            d.subject,
            d.body,
            d.in_reply_to,
            d.references,
            attachments
        ]);
        sha_hex(canonical.to_string().as_bytes())
    }

    fn draft_tool_ctx(&self) -> ToolContext {
        let mut ctx = ToolContext::new("email-draft", self.workspace_root());
        ctx.sandbox = SandboxMode::parse(&self.0.config.get_str("sandbox.mode").unwrap_or_else(|| "workspace-write".into()));
        ctx.shell = self.shell_sandbox();
        self.apply_folders(&mut ctx);
        ctx
    }

    /// Copies the files at these paths into the draft, with the rules of the file tools: nothing outside the folders the
    /// agents may read, nothing of the app's data or credentials.
    fn draft_attach(&self, ctx: &ToolContext, paths: &[String], files: &mut Vec<StoredFile>) -> Result<()> {
        for raw in paths {
            let path = ctx.resolve(raw, false)?;
            let meta = std::fs::metadata(&path).map_err(|e| Error::Invalid(format!("non riesco a leggere «{raw}»: {e}")))?;
            if !meta.is_file() {
                return Err(Error::Invalid(format!("«{raw}» non è un file")));
            }
            if meta.len() > MAX_FILE_BYTES {
                return Err(Error::Invalid(format!("«{raw}» è troppo grande da allegare (al massimo {} MB)", MAX_FILE_BYTES >> 20)));
            }
            let data = std::fs::read(&path).map_err(|e| Error::Invalid(format!("non riesco a leggere «{raw}»: {e}")))?;
            let sha = sha_hex(&data);
            if files.iter().any(|f| f.sha256 == sha) {
                continue;
            }
            if files.len() >= MAX_ATTACHMENTS {
                return Err(Error::Invalid(format!("al massimo {MAX_ATTACHMENTS} allegati")));
            }
            let total: u64 = files.iter().map(|f| f.size).sum::<u64>() + data.len() as u64;
            if total > MAX_TOTAL_BYTES {
                return Err(Error::Invalid(format!("gli allegati insieme superano {} MB", MAX_TOTAL_BYTES >> 20)));
            }
            let name = path
                .file_name()
                .map(|n| visible(&n.to_string_lossy()))
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| "allegato".into());
            files.push(StoredFile {
                mime: mime_of(&name).into(),
                name,
                sha256: sha,
                source: text::excerpt(raw, 200),
                size: data.len() as u64,
                data: base64::engine::general_purpose::STANDARD.encode(&data),
            });
        }
        Ok(())
    }

    fn draft_check_text(subject: &str, body: &str) -> Result<(String, String)> {
        let subject = one_line(subject);
        if subject.chars().count() > MAX_SUBJECT_CHARS {
            return Err(Error::Invalid(format!("l'oggetto è troppo lungo (al massimo {MAX_SUBJECT_CHARS} caratteri)")));
        }
        let body = visible(body).replace("\r\n", "\n");
        if body.chars().count() > MAX_BODY_CHARS {
            return Err(Error::Invalid(format!("il testo è troppo lungo (al massimo {MAX_BODY_CHARS} caratteri)")));
        }
        Ok((subject, body))
    }

    fn draft_check_recipients(d: &EmailDraft) -> Result<()> {
        if d.to.len() + d.cc.len() + d.bcc.len() > MAX_RECIPIENTS {
            return Err(Error::Invalid(format!("al massimo {MAX_RECIPIENTS} destinatari in tutto")));
        }
        Ok(())
    }

    /// The draft as the API shows it: sender, hash and duplicate are computed now.
    fn draft_present(&self, d: EmailDraft) -> EmailDraft {
        let all = self.drafts_raw();
        self.draft_present_in(d, &all)
    }

    /// [`Engine::draft_present`] when the list of drafts is already in hand (the listing reads it once).
    fn draft_present_in(&self, mut d: EmailDraft, all: &[EmailDraft]) -> EmailDraft {
        if let Ok(ch) = self.channel_doc(&d.channel_id) {
            d.from = self.draft_from(&ch);
        }
        d.content_hash = self.draft_hash(&d, &d.from);
        d.duplicate_of = Self::draft_twin(&d, all);
        d
    }

    /// Another draft with the same content that was not refused and is recent: worth a look before sending.
    fn draft_twin(d: &EmailDraft, all: &[EmailDraft]) -> Option<String> {
        let now = chrono::Utc::now();
        all.iter()
            .filter(|o| o.id != d.id && matches!(o.status.as_str(), "draft" | "sending" | "sent" | "uncertain"))
            .filter(|o| {
                chrono::DateTime::parse_from_rfc3339(&o.created_at)
                    .is_ok_and(|t| (now - t.to_utc()).to_std().is_ok_and(|age| age < DUPLICATE_WINDOW))
            })
            .find(|o| o.content_hash == d.content_hash)
            .map(|o| o.id.clone())
    }

    // ------------------------------------------------------------------ the API

    /// A draft that was «sending» when the app stopped, or whose sender gave up, is not known to be sent or not.
    fn drafts_recover(&self) {
        let inflight = self.0.channels.drafts.inflight.lock().unwrap().clone();
        for d in self.drafts_raw().into_iter().filter(|d| d.status == "sending" && !inflight.contains(&d.id)) {
            let stale = chrono::DateTime::parse_from_rfc3339(&d.updated_at)
                .ok()
                .is_some_and(|t| (chrono::Utc::now() - t.to_utc()).to_std().is_ok_and(|age| age > STUCK_AFTER));
            if stale {
                let _ = self.draft_transition(&d.id, &["sending"], |d| {
                    d.status = "uncertain".into();
                    d.note = Some("L'invio è stato interrotto (l'app si è fermata): non so se il messaggio è partito. Controlla Posta inviata prima di riprovare.".into());
                    Ok(())
                });
            }
        }
    }

    pub fn email_drafts(&self, status: Option<&str>) -> Result<Vec<EmailDraft>> {
        if let Some(s) = status.filter(|s| !s.is_empty()) {
            if !DRAFT_STATUSES.contains(&s) {
                return Err(Error::Invalid(format!("stato non valido «{s}»: usa {}", DRAFT_STATUSES.join(", "))));
            }
        }
        self.drafts_recover();
        let all = self.drafts_raw();
        Ok(all
            .iter()
            .filter(|d| status.filter(|s| !s.is_empty()).is_none_or(|s| d.status == s))
            .map(|d| self.draft_present_in(d.clone(), &all))
            .collect())
    }

    pub fn email_draft(&self, id: &str) -> Result<EmailDraft> {
        self.drafts_recover();
        Ok(self.draft_present(self.draft_doc(id)?))
    }

    /// Creates a draft. The assistant's attempt to create one that is identical to a recent one returns that one.
    pub(crate) fn draft_new(&self, n: NewDraft, ctx: &ToolContext) -> Result<(EmailDraft, bool)> {
        let _g = self.0.channels.drafts.lock.lock().unwrap();
        let ch = self.draft_channel(n.channel.as_deref())?;
        self.channel_ready(&ch).map_err(Error::Invalid)?;
        let (subject, body) = Self::draft_check_text(&n.subject, &n.body)?;
        let mut d = EmailDraft {
            id: new_id("drf"),
            status: "draft".into(),
            channel_id: ch.id.clone(),
            to: clean_addresses("a", &n.to)?,
            cc: clean_addresses("cc", &n.cc)?,
            bcc: clean_addresses("ccn", &n.bcc)?,
            subject,
            body,
            in_reply_to: clean_header(&n.in_reply_to),
            references: clean_header(&n.references),
            created_by: n.created_by.into(),
            source: n
                .source
                .as_deref()
                .map(|s| text::excerpt(s, 200))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| n.created_by.to_string()),
            session_id: n.session_id,
            revision: 1,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
            ..Default::default()
        };
        Self::draft_check_recipients(&d)?;
        let mut files = Vec::new();
        self.draft_attach(ctx, &n.attachments, &mut files)?;
        d.attachments = files.iter().map(StoredFile::meta).collect();
        d.from = self.draft_from(&ch);
        d.content_hash = self.draft_hash(&d, &d.from);
        let all = self.drafts_raw();
        if n.created_by == "assistant" {
            if let Some(twin) = Self::draft_twin(&d, &all).and_then(|id| self.draft_doc(&id).ok()) {
                return Ok((self.draft_present(twin), false));
            }
        }
        if all.iter().filter(|o| o.status == "draft").count() >= MAX_OPEN {
            return Err(Error::Invalid(format!("ci sono già {MAX_OPEN} bozze in attesa: rifiutane o inviane qualcuna")));
        }
        self.0.store.put_doc("email_draft_files", &d.id, &files)?;
        self.0.store.put_doc("email_draft", &d.id, &d)?;
        let d = self.draft_present(d);
        self.draft_audit(
            if n.created_by == "assistant" { "agent" } else { "user" },
            "email.draft",
            None,
            &d,
            &format!("origine: {}", d.source),
        );
        Ok((d, true))
    }

    pub fn email_draft_create(&self, c: EmailDraftCreate) -> Result<EmailDraft> {
        let ctx = self.draft_tool_ctx();
        let n = NewDraft {
            channel: c.channel_id,
            to: c.to,
            cc: c.cc,
            bcc: c.bcc,
            subject: c.subject,
            body: c.body,
            in_reply_to: c.in_reply_to,
            references: c.references,
            attachments: c.attachments,
            source: c.source,
            created_by: "user",
            session_id: None,
        };
        Ok(self.draft_new(n, &ctx)?.0)
    }

    /// Changes a draft that is waiting (or that failed). Any change makes it a new revision with a new hash: what was
    /// reviewed before is no longer what would be sent. `by_assistant` limits the change to drafts the assistant wrote.
    pub(crate) fn draft_edit(&self, id: &str, p: EmailDraftPatch, ctx: &ToolContext, by_assistant: bool) -> Result<EmailDraft> {
        let _g = self.0.channels.drafts.lock.lock().unwrap();
        let mut d = self.draft_doc(id)?;
        if !matches!(d.status.as_str(), "draft" | "failed") {
            return Err(Error::Conflict(format!("la bozza è «{}»: non si può più modificare", status_label(&d.status))));
        }
        if by_assistant && d.created_by != "assistant" {
            return Err(Error::Denied("questa bozza l'hai scritta tu: l'assistente non la modifica".into()));
        }
        // The person edits what they saw: if the draft changed meanwhile (the assistant revised it), their save would
        // silently overwrite that revision, so it is refused and they look again.
        if let Some(seen) = p.expected_revision.filter(|r| *r != d.revision) {
            return Err(Error::Conflict(format!(
                "la bozza è cambiata mentre la modificavi (revisione {} invece di {seen}): riguardala prima di salvare",
                d.revision
            )));
        }
        if let Some(c) = p.channel_id.as_deref() {
            d.channel_id = self.draft_channel(Some(c))?.id;
        }
        if let Some(v) = &p.to {
            d.to = clean_addresses("a", v)?;
        }
        if let Some(v) = &p.cc {
            d.cc = clean_addresses("cc", v)?;
        }
        if let Some(v) = &p.bcc {
            d.bcc = clean_addresses("ccn", v)?;
        }
        if p.subject.is_some() || p.body.is_some() {
            let (s, b) = Self::draft_check_text(p.subject.as_deref().unwrap_or(&d.subject), p.body.as_deref().unwrap_or(&d.body))?;
            d.subject = s;
            d.body = b;
        }
        Self::draft_check_recipients(&d)?;
        let mut files = self.draft_files(id);
        files.retain(|f| !p.remove_attachments.contains(&f.sha256));
        self.draft_attach(ctx, &p.add_attachments, &mut files)?;
        d.status = "draft".into();
        d.note = None;
        d.revision += 1;
        d.updated_at = now_rfc3339();
        d.attachments = files.iter().map(StoredFile::meta).collect();
        d.from = self.channel_doc(&d.channel_id).map(|c| self.draft_from(&c)).unwrap_or_default();
        d.content_hash = self.draft_hash(&d, &d.from);
        self.0.store.put_doc("email_draft_files", &d.id, &files)?;
        self.0.store.put_doc("email_draft", &d.id, &d)?;
        let d = self.draft_present(d);
        self.draft_audit(if by_assistant { "agent" } else { "user" }, "email.draft.edit", None, &d, "");
        Ok(d)
    }

    pub fn email_draft_patch(&self, id: &str, p: EmailDraftPatch) -> Result<EmailDraft> {
        self.draft_edit(id, p, &self.draft_tool_ctx(), false)
    }

    /// Moves a draft from one of the `from` states to another, under the lock: two requests cannot both win.
    fn draft_transition(&self, id: &str, from: &[&str], f: impl FnOnce(&mut EmailDraft) -> Result<()>) -> Result<EmailDraft> {
        let _g = self.0.channels.drafts.lock.lock().unwrap();
        let mut d = self.draft_doc(id)?;
        if !from.contains(&d.status.as_str()) {
            return Err(Error::Conflict(match d.status.as_str() {
                "sent" => "la bozza è già stata inviata".into(),
                "sending" => "la bozza è in corso di invio".into(),
                s => format!("la bozza è «{}»: l'operazione non è più possibile", status_label(s)),
            }));
        }
        f(&mut d)?;
        d.updated_at = now_rfc3339();
        self.0.store.put_doc("email_draft", &d.id, &d)?;
        Ok(d)
    }

    /// The owner refuses the content: it is never sent.
    pub fn email_draft_reject(&self, id: &str, reason: Option<&str>) -> Result<EmailDraft> {
        let reason = reason.map(one_line).filter(|r| !r.is_empty()).map(|r| text::excerpt(&r, 300));
        let d = self.draft_transition(id, &["draft", "failed"], |d| {
            d.status = "rejected".into();
            d.decided_at = Some(now_rfc3339());
            d.note = reason.clone();
            Ok(())
        })?;
        let d = self.draft_present(d);
        self.draft_audit("user", "email.reject", Some("rejected"), &d, reason.as_deref().unwrap_or(""));
        Ok(d)
    }

    /// The request is withdrawn before anything was sent.
    pub fn email_draft_cancel(&self, id: &str) -> Result<EmailDraft> {
        let d = self.draft_transition(id, &["draft", "failed"], |d| {
            d.status = "cancelled".into();
            d.decided_at = Some(now_rfc3339());
            Ok(())
        })?;
        let d = self.draft_present(d);
        self.draft_audit("user", "email.cancel", None, &d, "");
        Ok(d)
    }

    /// Removes a finished draft (and its attachments) from the list.
    pub fn email_draft_delete(&self, id: &str) -> Result<()> {
        let _g = self.0.channels.drafts.lock.lock().unwrap();
        let d = self.draft_doc(id)?;
        if matches!(d.status.as_str(), "draft" | "sending") {
            return Err(Error::Conflict("una bozza in attesa o in invio non si cancella: rifiutala o annullala prima".into()));
        }
        self.0.store.delete_doc("email_draft", id)?;
        self.0.store.delete_doc("email_draft_files", id)?;
        self.draft_audit("user", "email.delete", None, &d, "");
        Ok(())
    }

    // ------------------------------------------------------------------ sending

    /// Sends the draft the owner approved. `req.approved_hash` has to be the hash of the content as it is now.
    pub async fn email_draft_send(&self, id: &str, req: EmailDraftSend) -> Result<EmailDraft> {
        // Run to the end even if the caller goes away: a send that is dropped half way would leave nobody knowing what happened.
        let engine = self.clone();
        let id = id.to_string();
        tokio::spawn(async move { engine.draft_send_inner(&id, req).await })
            .await
            .map_err(|e| Error::Runtime(format!("invio interrotto: {e}")))?
    }

    async fn draft_send_inner(&self, id: &str, req: EmailDraftSend) -> Result<EmailDraft> {
        self.drafts_recover();
        let d = self.draft_doc(id)?;
        match d.status.as_str() {
            // The same request again (a double click, a retry after a timeout): the message is not sent twice.
            "sent" => return Ok(self.draft_present(d)),
            "sending" => return Err(Error::Conflict("la bozza è in corso di invio: aspetta l'esito".into())),
            "rejected" | "cancelled" => {
                return Err(Error::Conflict(format!("la bozza è «{}»: non si invia", status_label(&d.status))))
            }
            "uncertain" if !req.resend => {
                return Err(Error::Conflict(
                    "non so se il messaggio è partito: controlla Posta inviata. Se vuoi spedirlo comunque, ripeti l'invio confermando che potrebbe arrivare due volte".into(),
                ))
            }
            _ => {}
        }
        let ch = self.draft_channel(Some(&d.channel_id))?;
        if !ch.enabled || self.0.channels.killed.load(Ordering::SeqCst) {
            return Err(Error::Denied(format!(
                "il canale «{}» è spento: accendilo per inviare (l'interruttore d'emergenza ferma anche gli invii)",
                ch.name
            )));
        }
        if let Some(why) = self.channels_unavailable() {
            return Err(Error::Unsupported(why));
        }
        let cfg = super::email::smtp_config(self, &ch).map_err(Error::Invalid)?;
        let files = self.draft_files(id);
        let from = self.draft_from(&ch);
        // The files and the description of them kept in the draft were written together, and the bytes are what was
        // described: if anything differs (a damaged store), nothing is sent.
        let mismatch = || Error::Conflict("gli allegati della bozza non corrispondono più ai file salvati: ricrea la bozza".into());
        if files.iter().map(StoredFile::meta).map(|a| (a.name, a.mime, a.size, a.sha256)).collect::<Vec<_>>()
            != d.attachments.iter().map(|a| (a.name.clone(), a.mime.clone(), a.size, a.sha256.clone())).collect::<Vec<_>>()
        {
            return Err(mismatch());
        }
        let mut attachments = Vec::with_capacity(files.len());
        for f in &files {
            let data = base64::engine::general_purpose::STANDARD.decode(&f.data).map_err(|_| mismatch())?;
            if data.len() as u64 != f.size || sha_hex(&data) != f.sha256 {
                return Err(mismatch());
            }
            attachments.push(Attach { name: f.name.clone(), mime: f.mime.clone(), data });
        }
        let hash = self.draft_hash(&d, &from);
        if !constant_eq(&hash, req.approved_hash.trim()) {
            return Err(Error::Conflict(
                "la bozza è cambiata dopo la tua revisione (o il mittente del canale è cambiato): riguardala e approva il contenuto che vedi ora".into(),
            ));
        }
        if d.to.is_empty() {
            return Err(Error::Invalid("manca il destinatario".into()));
        }
        if d.subject.trim().is_empty() && d.body.trim().is_empty() {
            return Err(Error::Invalid("la bozza è vuota".into()));
        }
        {
            let mut sent = self.0.channels.drafts.sent.lock().unwrap();
            let now = Instant::now();
            while sent.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(3600)) {
                sent.pop_front();
            }
            if sent.len() >= SENDS_PER_HOUR {
                return Err(Error::Denied(format!("troppe email inviate in un'ora (al massimo {SENDS_PER_HOUR})")));
            }
        }
        let message_id = format!("draft-{}@aiw.local", d.id);
        // The swap that makes it one send: whoever gets here second finds it «sending» or «sent».
        let was = d.status.clone();
        let revision = d.revision;
        // Known to be in flight before it is «sending», so that a listing in between does not take it for an interrupted send.
        self.0.channels.drafts.inflight.lock().unwrap().insert(id.to_string());
        let claimed = self.draft_transition(id, &["draft", "failed", "uncertain"], |d| {
            // The content that was approved is the one that is sent: a change in between wins, and the send is refused.
            if d.revision != revision {
                return Err(Error::Conflict("la bozza è cambiata mentre la inviavi: riguardala e riprova".into()));
            }
            // «Send anyway» after an unknown outcome is the owner's explicit say-so: checked here, under the same lock as
            // the swap, not on the copy read at the start.
            if d.status == "uncertain" && !req.resend {
                return Err(Error::Conflict(
                    "non so se il messaggio è partito: controlla Posta inviata. Se vuoi spedirlo comunque, ripeti l'invio confermando che potrebbe arrivare due volte".into(),
                ));
            }
            d.status = "sending".into();
            d.decided_at = Some(now_rfc3339());
            d.note = None;
            Ok(())
        });
        let claimed = match claimed {
            Ok(c) => c,
            Err(e) => {
                self.0.channels.drafts.inflight.lock().unwrap().remove(id);
                return match self.draft_doc(id) {
                    Ok(now) if now.status == "sent" => Ok(self.draft_present(now)),
                    _ => Err(e),
                };
            }
        };
        self.0.channels.drafts.sent.lock().unwrap().push_back(Instant::now());
        let shown = self.draft_present(claimed.clone());
        self.draft_audit(
            "user",
            "email.approve",
            Some("approved"),
            &shown,
            if was == "uncertain" { "invio ripetuto su conferma, dopo un esito incerto" } else { "" },
        );
        let out = Outgoing {
            to: d.to.clone(),
            cc: d.cc.clone(),
            bcc: d.bcc.clone(),
            subject: d.subject.clone(),
            body: d.body.clone(),
            in_reply_to: d.in_reply_to.clone(),
            references: d.references.clone(),
            attachments,
            message_id: Some(message_id.clone()),
            auto: false,
        };
        let result = smtp::send_message(&cfg, &out).await;
        // In flight until the outcome is written: a listing in between must not take a send that is finishing for an
        // interrupted one.
        let outcome = self.draft_finish(id, &message_id, &was, result);
        self.0.channels.drafts.inflight.lock().unwrap().remove(id);
        outcome
    }

    /// Writes how a send ended: `sent`, `failed` (nothing went out) or `uncertain` (it may have). A repeat after an
    /// unknown outcome that fails cleanly stays unknown: the first attempt may have arrived.
    fn draft_finish(&self, id: &str, message_id: &str, was: &str, result: std::result::Result<(), smtp::SendError>) -> Result<EmailDraft> {
        match result {
            Ok(()) => {
                let sent = self.draft_transition(id, &["sending"], |d| {
                    d.status = "sent".into();
                    d.sent_at = Some(now_rfc3339());
                    d.message_id = Some(format!("<{message_id}>"));
                    Ok(())
                })?;
                let sent = self.draft_present(sent);
                self.draft_audit("user", "email.send", Some("sent"), &sent, "");
                Ok(sent)
            }
            Err(e) => {
                let unknown = e.uncertain || was == "uncertain";
                let note = if e.uncertain {
                    format!("{}. Controlla Posta inviata prima di riprovare.", e.message)
                } else if was == "uncertain" {
                    format!("{}. Il primo tentativo potrebbe essere arrivato: controlla Posta inviata prima di riprovare.", e.message)
                } else {
                    format!("{} (non è partito nulla)", e.message)
                };
                let status = if unknown { "uncertain" } else { "failed" };
                let failed = self.draft_transition(id, &["sending"], |d| {
                    d.status = status.into();
                    d.note = Some(note.clone());
                    Ok(())
                })?;
                let failed = self.draft_present(failed);
                self.draft_audit("system", "email.failed", Some(status), &failed, &note);
                Err(Error::Runtime(format!("invio non riuscito: {note}")))
            }
        }
    }
}

fn status_label(s: &str) -> &'static str {
    match s {
        "draft" => "in attesa",
        "sending" => "in invio",
        "sent" => "inviata",
        "failed" => "non partita",
        "uncertain" => "esito incerto",
        "rejected" => "rifiutata",
        "cancelled" => "annullata",
        _ => "?",
    }
}
