//! A small SMTP submission client for the email channel and for the drafts the owner approves: implicit TLS (465)
//! or STARTTLS (587, and any other port), authentication with `AUTH PLAIN` only after the channel is encrypted,
//! one message per connection. Every header value and every address is stripped of line breaks and control
//! characters, so a subject or an address cannot add headers or recipients. The reply of the channel goes to the
//! one allowed sender; a draft goes to the recipients the owner saw, and to nobody else.

use aiw_tools::connectors::tls::{self, BoxIo};
use base64::Engine as _;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const IO_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub(crate) struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// Envelope and header sender.
    pub from: String,
    pub roots: Vec<Vec<u8>>,
}

/// A file that goes with a message.
pub(crate) struct Attach {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
}

pub(crate) struct Outgoing {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    /// Hidden recipients: they get the message (`RCPT TO`) but are never written in a header.
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub attachments: Vec<Attach>,
    /// The `Message-ID` (without the angle brackets); random when not given. A draft always gives one derived from
    /// its own id, so a second attempt carries the same.
    pub message_id: Option<String>,
    /// An automatic answer: the message says so (`Auto-Submitted`), so that other automatic systems do not answer
    /// it. A message the owner approved is not one.
    pub auto: bool,
}

/// Why a send did not complete, and whether the server may nevertheless have taken the message.
#[derive(Debug)]
pub(crate) struct SendError {
    pub message: String,
    /// The failure came after the message was handed over (or the answer never came): it may have been delivered,
    /// so it is never repeated on its own.
    pub uncertain: bool,
}

impl SendError {
    fn before(message: impl Into<String>) -> Self {
        Self { message: message.into(), uncertain: false }
    }
}

struct Conn {
    io: BufReader<BoxIo>,
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

pub(crate) fn valid_address(a: &str) -> bool {
    let a = a.trim();
    a.contains('@') && !a.chars().any(|c| c.is_whitespace() || c.is_control() || "<>\",;".contains(c)) && a.len() <= 254
}

impl Conn {
    async fn reply(&mut self) -> Result<(u16, Vec<String>), String> {
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            let n = tokio::time::timeout(IO_TIMEOUT, self.io.read_line(&mut line))
                .await
                .map_err(|_| "il server di posta non risponde in tempo".to_string())?
                .map_err(|e| format!("errore di rete: {e}"))?;
            if n == 0 {
                return Err("il server di posta ha chiuso la connessione".into());
            }
            let line = line.trim_end_matches(['\r', '\n']).to_string();
            let code: u16 =
                line.get(..3).and_then(|c| c.parse().ok()).ok_or_else(|| "risposta inattesa dal server di posta".to_string())?;
            let more = line.as_bytes().get(3) == Some(&b'-');
            lines.push(line.get(4..).unwrap_or("").to_string());
            if !more {
                return Ok((code, lines));
            }
        }
    }

    async fn send(&mut self, line: &str) -> Result<(), String> {
        let data = format!("{line}\r\n");
        tokio::time::timeout(IO_TIMEOUT, async {
            self.io.get_mut().write_all(data.as_bytes()).await?;
            self.io.get_mut().flush().await
        })
        .await
        .map_err(|_| "il server di posta non risponde in tempo".to_string())?
        .map_err(|e| format!("errore di rete: {e}"))
    }

    async fn expect(&mut self, ok: &[u16], what: &str) -> Result<Vec<String>, String> {
        let (code, lines) = self.reply().await?;
        if ok.contains(&code) {
            Ok(lines)
        } else {
            Err(format!(
                "il server di posta ha rifiutato {what} ({code} {})",
                lines.first().cloned().unwrap_or_default().chars().take(80).collect::<String>()
            ))
        }
    }

    async fn cmd(&mut self, line: &str, ok: &[u16], what: &str) -> Result<Vec<String>, String> {
        self.send(line).await?;
        self.expect(ok, what).await
    }
}

fn implicit(cfg: &SmtpConfig) -> bool {
    cfg.port == 465
}

/// Connects, secures the channel and logs in.
async fn open(cfg: &SmtpConfig) -> Result<Conn, String> {
    let host = cfg.host.trim();
    let io: BoxIo = if implicit(cfg) {
        Box::new(tls::connect_tls(host, cfg.port, &cfg.roots, IO_TIMEOUT).await.map_err(|e| e.to_string())?)
    } else {
        Box::new(tls::connect_tcp(host, cfg.port, IO_TIMEOUT).await.map_err(|e| e.to_string())?)
    };
    let mut c = Conn { io: BufReader::new(io) };
    c.expect(&[220], "la connessione").await?;
    let caps = c.cmd("EHLO aiw.local", &[250], "il saluto").await?;
    if !implicit(cfg) {
        if !caps.iter().any(|l| l.eq_ignore_ascii_case("STARTTLS")) {
            return Err("il server di posta non offre una connessione cifrata (STARTTLS): non invio la password in chiaro".into());
        }
        c.cmd("STARTTLS", &[220], "la cifratura").await?;
        if !c.io.buffer().is_empty() {
            return Err("dati inattesi prima dell'avvio della cifratura".into());
        }
        let plain = c.io.into_inner();
        let up = tls::upgrade(plain, host, &cfg.roots, IO_TIMEOUT).await.map_err(|e| e.to_string())?;
        c = Conn { io: BufReader::new(Box::new(up)) };
        c.cmd("EHLO aiw.local", &[250], "il saluto").await?;
    }
    let blob = base64::engine::general_purpose::STANDARD.encode(format!("\0{}\0{}", cfg.user, cfg.password));
    c.send(&format!("AUTH PLAIN {blob}")).await?;
    let (code, _) = c.reply().await?;
    if code != 235 {
        return Err("nome utente o password del server di invio non accettati".into());
    }
    Ok(c)
}

/// Connects and logs in without sending anything (the «Prova» button).
pub(crate) async fn check(cfg: &SmtpConfig) -> Result<(), String> {
    let mut c = open(cfg).await?;
    let _ = c.send("QUIT").await;
    Ok(())
}

fn encode_header(s: &str) -> String {
    let s = clean(s);
    if s.is_ascii() {
        return s;
    }
    // Encoded words of at most 36 bytes (48 characters of base64, 60 with the frame): RFC 2047 allows 75, and with the name of
    // the header the line stays under 78. Never cut inside a character.
    let (mut words, mut cur) = (Vec::new(), String::new());
    for c in s.chars() {
        if cur.len() + c.len_utf8() > 36 {
            words.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words.iter().map(|w| format!("=?UTF-8?B?{}?=", base64::engine::general_purpose::STANDARD.encode(w))).collect::<Vec<_>>().join(" ")
}

/// A header with its value split over several lines at spaces, so that no line runs much past 76 characters (RFC 5322
/// recommends 78 and allows 998): a long list of recipients, a long `References` or a long subject is accepted by strict servers.
fn fold(name: &str, value: &str) -> String {
    let mut out = String::new();
    let mut line = format!("{name}:");
    for word in value.split(' ').filter(|w| !w.is_empty()) {
        if line.len() + 1 + word.len() > 76 && !line.ends_with(':') {
            out.push_str(&line);
            out.push_str("\r\n");
            line = String::new();
        }
        line.push(' ');
        line.push_str(word);
    }
    out.push_str(&line);
    out
}

fn wrap_base64(data: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    let lines: Vec<String> = b64.as_bytes().chunks(76).map(|c| String::from_utf8_lossy(c).to_string()).collect();
    lines.join("\r\n")
}

/// A file name that is safe in a header: no path, no quotes, no control characters.
fn safe_name(name: &str) -> String {
    let n: String =
        name.chars().filter(|c| !c.is_control()).map(|c| if matches!(c, '"' | '\\' | '/' | ':' | ';') { '_' } else { c }).collect();
    let n = n.trim().chars().take(120).collect::<String>();
    if n.is_empty() {
        "allegato".into()
    } else {
        n
    }
}

fn safe_mime(mime: &str) -> String {
    let m = mime.trim().to_ascii_lowercase();
    let ok = m
        .split_once('/')
        .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && m.chars().all(|c| c.is_ascii_alphanumeric() || "/+.-_".contains(c)));
    if ok {
        m
    } else {
        "application/octet-stream".into()
    }
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// The text of the message (headers and body), with CRLF line ends. The hidden recipients are not in it.
pub(crate) fn build(cfg: &SmtpConfig, m: &Outgoing) -> String {
    let addrs = |v: &[String]| v.iter().map(|a| clean(a)).filter(|a| !a.is_empty()).collect::<Vec<_>>().join(", ");
    let message_id = match m.message_id.as_deref().map(clean).filter(|i| !i.is_empty()) {
        Some(id) => id.trim_matches(['<', '>']).to_string(),
        None => format!("{}@aiw.local", uuid::Uuid::new_v4().simple()),
    };
    let mut h = vec![format!("From: {}", clean(&cfg.from)), fold("To", &addrs(&m.to))];
    if !m.cc.is_empty() {
        h.push(fold("Cc", &addrs(&m.cc)));
    }
    h.push(fold("Subject", &encode_header(&m.subject)));
    h.push(format!("Date: {}", chrono::Local::now().to_rfc2822()));
    h.push(format!("Message-ID: <{message_id}>"));
    if let Some(r) = m.in_reply_to.as_deref().map(clean).filter(|r| !r.is_empty()) {
        h.push(fold("In-Reply-To", &r));
    }
    if let Some(r) = m.references.as_deref().map(clean).filter(|r| !r.is_empty()) {
        h.push(fold("References", &r));
    }
    if m.auto {
        // Tells other automatic systems not to answer this one: no mail loops.
        h.push("Auto-Submitted: auto-replied".into());
    }
    h.push("MIME-Version: 1.0".into());
    let text_part = |extra_headers: bool| {
        let mut p = String::new();
        if extra_headers {
            p.push_str("Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n");
        }
        p.push_str(&wrap_base64(m.body.as_bytes()));
        p
    };
    if m.attachments.is_empty() {
        h.push("Content-Type: text/plain; charset=utf-8".into());
        h.push("Content-Transfer-Encoding: base64".into());
        return format!("{}\r\n\r\n{}\r\n", h.join("\r\n"), text_part(false));
    }
    // The boundary has dashes, which base64 never has: it cannot appear inside a part.
    let boundary = format!("aiw-{}", uuid::Uuid::new_v4().simple());
    h.push(format!("Content-Type: multipart/mixed; boundary=\"{boundary}\""));
    let mut out = format!("{}\r\n\r\nThis is a multi-part message in MIME format.\r\n", h.join("\r\n"));
    out.push_str(&format!("--{boundary}\r\n{}\r\n", text_part(true)));
    for a in &m.attachments {
        let name = safe_name(&a.name);
        let disposition = if name.is_ascii() {
            format!("attachment; filename=\"{name}\"")
        } else {
            let fallback: String = name.chars().map(|c| if c.is_ascii() { c } else { '_' }).collect();
            format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{}", percent_encode(&name))
        };
        out.push_str(&format!(
            "--{boundary}\r\nContent-Type: {}; name=\"{}\"\r\nContent-Transfer-Encoding: base64\r\nContent-Disposition: {disposition}\r\n\r\n{}\r\n",
            safe_mime(&a.mime),
            if name.is_ascii() { name.clone() } else { name.chars().map(|c| if c.is_ascii() { c } else { '_' }).collect() },
            wrap_base64(&a.data)
        ));
    }
    out.push_str(&format!("--{boundary}--\r\n"));
    out
}

/// Sends the message; says whether a failure may have left it delivered.
pub(crate) async fn send_message(cfg: &SmtpConfig, m: &Outgoing) -> Result<(), SendError> {
    let rcpt: Vec<String> = m.to.iter().chain(&m.cc).chain(&m.bcc).map(|a| a.trim().to_string()).collect();
    if rcpt.is_empty() || !rcpt.iter().all(|a| valid_address(a)) || !valid_address(&cfg.from) {
        return Err(SendError::before("indirizzo di posta non valido"));
    }
    let mut c = open(cfg).await.map_err(SendError::before)?;
    c.cmd(&format!("MAIL FROM:<{}>", cfg.from.trim()), &[250], "il mittente").await.map_err(SendError::before)?;
    // Every recipient must be accepted before anything is sent: no message goes out to some and not to others.
    for a in &rcpt {
        c.cmd(&format!("RCPT TO:<{a}>"), &[250, 251], &format!("il destinatario {a}")).await.map_err(SendError::before)?;
    }
    c.cmd("DATA", &[354], "l'invio").await.map_err(SendError::before)?;
    let text = build(cfg, m);
    let mut stuffed = String::new();
    for line in text.split("\r\n") {
        if line.starts_with('.') {
            stuffed.push('.');
        }
        stuffed.push_str(line);
        stuffed.push_str("\r\n");
    }
    stuffed.push_str(".\r\n");
    // From here on the server may have the message: a failure is «uncertain», never «not sent».
    let uncertain = |message: String| SendError { message, uncertain: true };
    tokio::time::timeout(IO_TIMEOUT * 3, async {
        c.io.get_mut().write_all(stuffed.as_bytes()).await?;
        c.io.get_mut().flush().await
    })
    .await
    .map_err(|_| uncertain("il server di posta non risponde in tempo".into()))?
    .map_err(|e| uncertain(format!("errore di rete: {e}")))?;
    match c.reply().await {
        Ok((250, _)) => {}
        // An answer that refuses the message: the server did not take it.
        Ok((code, lines)) => {
            return Err(SendError::before(format!(
                "il server di posta ha rifiutato il messaggio ({code} {})",
                lines.first().cloned().unwrap_or_default().chars().take(80).collect::<String>()
            )))
        }
        Err(e) => return Err(uncertain(format!("{e}: non so se il messaggio è partito"))),
    }
    let _ = c.send("QUIT").await;
    Ok(())
}

pub(crate) async fn send(cfg: &SmtpConfig, m: &Outgoing) -> Result<(), String> {
    send_message(cfg, m).await.map_err(|e| e.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SmtpConfig {
        SmtpConfig {
            host: "h".into(),
            port: 587,
            user: "u".into(),
            password: "p".into(),
            from: "assistente@casa.example".into(),
            roots: vec![],
        }
    }

    fn msg() -> Outgoing {
        Outgoing {
            to: (0..20).map(|i| format!("destinatario.numero.{i}@un-dominio-lungo.example.org")).collect(),
            cc: vec![],
            bcc: vec![],
            subject: "Perché è così: ".repeat(20),
            body: "Testo".into(),
            in_reply_to: Some("<a@x>".into()),
            references: Some((0..15).map(|i| format!("<riferimento-{i}@un-dominio-lungo.example.org>")).collect::<Vec<_>>().join(" ")),
            attachments: vec![Attach { name: "è.pdf".into(), mime: "application/pdf".into(), data: vec![1, 2, 3] }],
            message_id: Some("draft-1@aiw.local".into()),
            auto: false,
        }
    }

    #[test]
    fn long_headers_are_folded_and_encoded_words_stay_short_and_the_whole_message_is_ascii() {
        let text = build(&cfg(), &msg());
        assert!(text.is_ascii(), "8-bit text needs a server that offers 8BITMIME: nothing here may be 8-bit");
        for line in text.split("\r\n") {
            assert!(line.len() <= 78, "{} chars: {line}", line.len());
        }
        // Unfolded, the headers say what was meant.
        let unfolded = text.replace("\r\n ", " ");
        assert!(unfolded.contains("destinatario.numero.19@un-dominio-lungo.example.org"));
        assert!(unfolded.contains("<riferimento-14@un-dominio-lungo.example.org>"));
        assert!(unfolded.matches("=?UTF-8?B?").count() > 3, "the subject is split into several encoded words");
    }

    #[test]
    fn every_encoded_word_decodes_back_to_whole_characters() {
        use base64::Engine as _;
        let subject = "È così, perché «sì» ✓ ".repeat(30);
        let encoded = encode_header(&subject);
        let mut back = String::new();
        for w in encoded.split(' ') {
            let b64 = w.strip_prefix("=?UTF-8?B?").and_then(|r| r.strip_suffix("?=")).unwrap();
            assert!(w.len() <= 75, "{}", w.len());
            back.push_str(
                &String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b64).unwrap()).expect("never cut inside a character"),
            );
        }
        assert_eq!(back, subject.trim());
    }

    #[test]
    fn short_ascii_headers_are_left_alone() {
        assert_eq!(fold("Subject", "Ciao Anna"), "Subject: Ciao Anna");
        assert_eq!(encode_header("Ciao"), "Ciao");
    }
}
