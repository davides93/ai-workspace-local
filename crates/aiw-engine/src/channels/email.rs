//! Email as a channel: an IMAP mailbox is polled, and each accepted message is answered through SMTP
//! as a reply in the same thread (`In-Reply-To` / `References`, the original is not quoted).
//!
//! A mail is answered only when ALL of this holds:
//! * the sender is on the channel's list;
//! * the receiving server vouches for the sender: its (topmost) `Authentication-Results` header says
//!   `dkim=pass` or `spf=pass` for the domain of the `From` address (can be switched off, with a
//!   warning, for a server that does not write the header);
//! * it is not automatic mail (`Auto-Submitted`, `Precedence: bulk|junk|list`, `List-Id`), which also
//!   prevents answering loops, and the sender has not used up the replies allowed per hour.
//!
//! The mailbox is read with `BODY.PEEK`: nothing is marked as read, the position of the last message
//! looked at (UID) is kept in the store instead. Only when the person chose a folder dedicated to the
//! assistant and turned «segna come letti» on, handled messages are marked `\Seen` there. The first
//! time a mailbox is looked at, the messages that are already in it are not answered.

use super::smtp::{self, Outgoing, SmtpConfig};
use super::{set_status, text, Engine, Inbound, Target, Transport};
use aiw_core::api::Channel;
use aiw_tools::connectors::imap::{Access, Account, Session};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Messages handled per poll.
const PER_POLL: usize = 20;
const MAX_BYTES: usize = 256 * 1024;
const DEFAULT_PER_HOUR: usize = 10;

struct Cfg {
    imap: Account,
    smtp: SmtpConfig,
    folder: String,
    mark_seen: bool,
    require_auth: bool,
    poll: Duration,
    per_hour: usize,
}

fn setting_str(ch: &Channel, k: &str) -> String {
    ch.settings.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string()
}
fn setting_u64(ch: &Channel, k: &str) -> Option<u64> {
    ch.settings.get(k).and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
}

fn config(engine: &Engine, ch: &Channel) -> Result<Cfg, String> {
    let roots = engine.tls_roots();
    let imap_user = setting_str(ch, "imap_user");
    let imap_password = ch.secrets.get("imap_password").cloned().unwrap_or_default();
    let folder = Some(setting_str(ch, "folder")).filter(|f| !f.is_empty()).unwrap_or_else(|| "INBOX".into());
    let smtp_user = Some(setting_str(ch, "smtp_user")).filter(|u| !u.is_empty()).unwrap_or_else(|| imap_user.clone());
    let smtp_password = ch.secrets.get("smtp_password").filter(|p| !p.is_empty()).cloned().unwrap_or_else(|| imap_password.clone());
    let from = Some(setting_str(ch, "from")).filter(|f| !f.is_empty()).unwrap_or_else(|| smtp_user.clone());
    if !smtp::valid_address(&from) {
        return Err("Serve l'indirizzo da cui rispondere (campo «Indirizzo mittente»).".into());
    }
    let mark_seen = ch.settings.get("mark_seen").and_then(|v| v.as_bool()).unwrap_or(false) && !folder.eq_ignore_ascii_case("INBOX");
    let poll_ms =
        engine.channel_endpoints().email_poll_ms.unwrap_or_else(|| setting_u64(ch, "poll_seconds").unwrap_or(60).clamp(15, 3600) * 1000);
    Ok(Cfg {
        imap: Account {
            id: ch.id.clone(),
            host: setting_str(ch, "imap_host"),
            port: setting_u64(ch, "imap_port").unwrap_or(993) as u16,
            username: imap_user,
            password: imap_password,
            folders: vec![folder.clone()],
            tls: String::new(),
        },
        smtp: SmtpConfig {
            host: setting_str(ch, "smtp_host"),
            port: setting_u64(ch, "smtp_port").unwrap_or(587) as u16,
            user: smtp_user,
            password: smtp_password,
            from,
            roots,
        },
        folder,
        mark_seen,
        require_auth: ch.settings.get("require_auth").and_then(|v| v.as_bool()).unwrap_or(true),
        poll: Duration::from_millis(poll_ms),
        per_hour: setting_u64(ch, "replies_per_hour").unwrap_or(DEFAULT_PER_HOUR as u64).clamp(1, 100) as usize,
    })
}

pub(super) struct Mailer {
    smtp: Option<SmtpConfig>,
}

impl Mailer {
    pub(super) fn new(engine: &Engine, ch: &Channel) -> Self {
        Self { smtp: config(engine, ch).ok().map(|c| c.smtp) }
    }
}

#[async_trait]
impl Transport for Mailer {
    async fn send(&self, to: &Target, body: &str, ask: Option<&str>) -> Result<(), String> {
        let cfg = self.smtp.as_ref().ok_or("le impostazioni della posta non sono complete")?;
        let addr = to.reply["to"].as_str().unwrap_or(&to.conv).to_string();
        let subject = to.reply["subject"].as_str().unwrap_or("Messaggio dall'assistente");
        let subject = if subject.to_lowercase().starts_with("re:") || to.reply["to"].is_null() {
            subject.to_string()
        } else {
            format!("Re: {subject}")
        };
        let in_reply_to = to.reply["in_reply_to"].as_str().map(str::to_string);
        let references =
            in_reply_to.as_ref().map(|id| format!("{} {id}", to.reply["references"].as_str().unwrap_or("")).trim().to_string());
        let _ = ask; // the answer is a reply by mail: «sì» or «no» in the next message
        smtp::send(cfg, &Outgoing { to: addr, subject, body: body.to_string(), in_reply_to, references }).await
    }

    fn limit(&self) -> usize {
        20_000
    }
}

pub(super) async fn test(engine: &Engine, ch: &Channel) -> Result<String, String> {
    let cfg = config(engine, ch)?;
    let roots = engine.tls_roots();
    let mut s = Session::connect(&cfg.imap, &roots, Access::ReadOnly).await.map_err(|e| format!("posta in arrivo: {e}"))?;
    s.open_folder(&cfg.folder).await.map_err(|e| format!("posta in arrivo: {e}"))?;
    s.logout().await;
    smtp::check(&cfg.smtp).await.map_err(|e| format!("invio: {e}"))?;
    let mut detail = format!("Posta in arrivo e invio funzionano (cartella «{}»).", cfg.folder);
    if !cfg.require_auth {
        detail.push_str(" Attenzione: il controllo dell'autenticazione del mittente è spento, un messaggio con un mittente falsificato potrebbe essere accettato.");
    }
    Ok(detail)
}

fn domain_of(addr: &str) -> &str {
    addr.rsplit_once('@').map(|(_, d)| d).unwrap_or("")
}

/// Does the receiving server say the sender is who they claim? Only the topmost `Authentication-Results`
/// header counts (the one the receiving server wrote last; the ones below may have been forged by the sender).
pub(super) fn authenticated(auth_results: &[String], from_addr: &str) -> bool {
    let Some(first) = auth_results.first() else { return false };
    let from_dom = domain_of(from_addr).trim().to_ascii_lowercase();
    if from_dom.is_empty() {
        return false;
    }
    let aligned = |d: &str| {
        let d = d.trim().trim_matches(|c| c == ';' || c == '"').to_ascii_lowercase();
        d.contains('.') && (d == from_dom || from_dom.ends_with(&format!(".{d}")) || d.ends_with(&format!(".{from_dom}")))
    };
    let prop = |part: &str, name: &str| -> Option<String> {
        part.split_whitespace().find_map(|t| t.strip_prefix(name)).map(|v| v.trim_matches(|c| c == '<' || c == '>').to_string())
    };
    first.split(';').skip(1).any(|part| {
        let p = part.trim().to_ascii_lowercase();
        (p.starts_with("dkim=pass") && prop(&p, "header.d=").is_some_and(|d| aligned(&d)))
            || (p.starts_with("spf=pass") && prop(&p, "smtp.mailfrom=").is_some_and(|m| aligned(domain_of(&m))))
    })
}

/// The conversation a mail belongs to: the first message of its thread.
fn thread_id(m: &aiw_tools::connectors::mail::Inbound) -> String {
    let root = m
        .references
        .split_whitespace()
        .next()
        .map(str::to_string)
        .filter(|r| !r.is_empty())
        .or_else(|| Some(m.message_id.clone()).filter(|r| !r.is_empty()));
    match root {
        Some(r) => format!("{}|{}", m.from_addr, r),
        None => format!("{}|{}", m.from_addr, text::excerpt(&m.subject, 40)),
    }
}

fn replies_left(engine: &Engine, sender: &str, per_hour: usize) -> bool {
    let mut replies = engine.0.channels.replies.lock().unwrap();
    let q: &mut VecDeque<Instant> = replies.entry(sender.to_string()).or_default();
    let now = Instant::now();
    while q.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(3600)) {
        q.pop_front();
    }
    if q.len() >= per_hour {
        return false;
    }
    q.push_back(now);
    true
}

pub(super) async fn run(
    engine: &Engine,
    ch: &Channel,
    status: &Arc<Mutex<aiw_core::api::ChannelStatus>>,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let cfg = config(engine, ch)?;
    let roots = engine.tls_roots();
    let tr: Arc<dyn Transport> = Arc::new(Mailer { smtp: Some(cfg.smtp.clone()) });
    loop {
        poll_once(engine, ch, &cfg, &roots, &tr).await?;
        let mut detail = format!("Controllo la cartella «{}» ogni {} secondi.", cfg.folder, cfg.poll.as_secs().max(1));
        if !cfg.require_auth {
            detail.push_str(" Il controllo dell'autenticazione del mittente è spento.");
        }
        set_status(status, "connected", detail);
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            _ = tokio::time::sleep(cfg.poll) => {}
        }
    }
}

async fn poll_once(engine: &Engine, ch: &Channel, cfg: &Cfg, roots: &[Vec<u8>], tr: &Arc<dyn Transport>) -> Result<(), String> {
    let access = if cfg.mark_seen { Access::MarkSeen } else { Access::ReadOnly };
    let mut s = Session::connect(&cfg.imap, roots, access).await.map_err(|e| e.to_string())?;
    s.open_folder(&cfg.folder).await.map_err(|e| e.to_string())?;
    let validity = s.uidvalidity.unwrap_or(0);
    let state = engine.channel_state(&ch.id);
    if state.email_uidvalidity != validity {
        // The first look at this mailbox (or it was rebuilt): what is already there is not answered.
        let newest = s.search_after(0).await.map_err(|e| e.to_string())?.last().copied().unwrap_or(0);
        engine.channel_update_state(&ch.id, |st| {
            st.email_uidvalidity = validity;
            st.email_last_uid = newest;
        });
        s.logout().await;
        return Ok(());
    }
    let uids: Vec<u32> = s.search_after(state.email_last_uid).await.map_err(|e| e.to_string())?.into_iter().take(PER_POLL).collect();
    if uids.is_empty() {
        s.logout().await;
        return Ok(());
    }
    let fetched = s.fetch_peek(&uids, MAX_BYTES).await.map_err(|e| e.to_string())?;
    for f in fetched {
        // Remembered before it is handled: a crash in the middle never makes the same mail be answered twice.
        engine.channel_update_state(&ch.id, |st| st.email_last_uid = f.uid);
        handle(engine, ch, cfg, tr, &f.raw).await;
        if cfg.mark_seen {
            let _ = s.mark_seen(f.uid).await;
        }
    }
    s.logout().await;
    Ok(())
}

async fn handle(engine: &Engine, ch: &Channel, cfg: &Cfg, tr: &Arc<dyn Transport>, raw: &[u8]) {
    let Some(m) = aiw_tools::connectors::mail::parse_inbound(raw) else { return };
    if m.from_addr == cfg.smtp.from.to_ascii_lowercase() {
        return; // our own reply, seen again
    }
    if !engine.channel_is_allowed(&ch.id, std::slice::from_ref(&m.from_addr)) {
        engine.channel_note_ignored(&ch.id, &m.from_addr);
        return;
    }
    if m.automatic {
        engine.channel_update_state(&ch.id, |st| st.skipped_auto += 1);
        engine.channel_audit(ch, "channel.skipped", format!("posta automatica da {} non trattata", m.from_addr));
        return;
    }
    if cfg.require_auth && !authenticated(&m.auth_results, &m.from_addr) {
        engine.channel_note_ignored(&ch.id, &format!("{} (mittente non verificato)", m.from_addr));
        return;
    }
    if !replies_left(engine, &m.from_addr, cfg.per_hour) {
        engine.channel_audit(ch, "channel.rate", format!("troppe risposte a {} in un'ora: messaggio non trattato", m.from_addr));
        return;
    }
    let body = if m.subject.is_empty() { m.body.clone() } else { format!("Oggetto: {}\n\n{}", m.subject, m.body) };
    let msg = Inbound {
        conv: thread_id(&m),
        senders: vec![m.from_addr.clone()],
        name: if m.from_name.is_empty() { m.from_addr.clone() } else { format!("{} ({})", m.from_name, m.from_addr) },
        text: body,
        files: vec![],
        reply: serde_json::json!({"to": m.from_addr, "subject": m.subject, "in_reply_to": m.message_id, "references": m.references}),
    };
    engine.channel_inbound(&ch.id, tr.clone(), msg).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(s: &str) -> Vec<String> {
        vec![s.to_string()]
    }

    #[test]
    fn alignment_decides_who_is_vouched_for() {
        let dkim = "mx.example.net; dkim=pass header.d=example.it header.s=sel; spf=none";
        assert!(authenticated(&auth(dkim), "mario@example.it"));
        assert!(authenticated(&auth(dkim), "mario@mail.example.it"), "subdomain of the signing domain");
        assert!(!authenticated(&auth(dkim), "mario@altro.it"), "not the From domain");
        let spf = "mx.example.net; spf=pass smtp.mailfrom=bounce@example.it";
        assert!(authenticated(&auth(spf), "mario@example.it"));
        assert!(!authenticated(&auth(spf), "ceo@banca.it"));
        for fail in ["mx; dkim=fail header.d=example.it", "mx; spf=softfail smtp.mailfrom=a@example.it", "mx; dkim=none", ""] {
            assert!(!authenticated(&auth(fail), "mario@example.it"), "{fail}");
        }
        assert!(!authenticated(&[], "mario@example.it"), "no header, no trust");
        assert!(!authenticated(&auth("mx; dkim=pass header.d=it"), "mario@example.it"), "a bare TLD is not a domain");
        // Only the topmost header counts: a forged one below is not read.
        let both = vec!["mx; dkim=fail header.d=example.it".to_string(), "forged; dkim=pass header.d=example.it".to_string()];
        assert!(!authenticated(&both, "mario@example.it"));
    }
}
