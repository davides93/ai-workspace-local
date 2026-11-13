//! A small SMTP submission client for the email channel: implicit TLS (465) or STARTTLS (587, and any
//! other port), authentication with `AUTH PLAIN` only after the channel is encrypted, one message per
//! connection. The recipient is always the allowed sender the reply is for, and every header value is
//! stripped of line breaks, so a subject or an address cannot add headers or recipients.

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

pub(crate) struct Outgoing {
    pub to: String,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
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
        s
    } else {
        format!("=?UTF-8?B?{}?=", base64::engine::general_purpose::STANDARD.encode(s))
    }
}

/// The text of the message (headers and body), with CRLF line ends.
pub(crate) fn build(cfg: &SmtpConfig, m: &Outgoing) -> String {
    let mut h = vec![
        format!("From: {}", clean(&cfg.from)),
        format!("To: {}", clean(&m.to)),
        format!("Subject: {}", encode_header(&m.subject)),
        format!("Date: {}", chrono::Local::now().to_rfc2822()),
        format!("Message-ID: <{}@aiw.local>", uuid::Uuid::new_v4().simple()),
    ];
    if let Some(r) = m.in_reply_to.as_deref().map(clean).filter(|r| !r.is_empty()) {
        h.push(format!("In-Reply-To: {r}"));
    }
    if let Some(r) = m.references.as_deref().map(clean).filter(|r| !r.is_empty()) {
        h.push(format!("References: {r}"));
    }
    // Tells other automatic systems not to answer this one: no mail loops.
    h.push("Auto-Submitted: auto-replied".into());
    h.push("MIME-Version: 1.0".into());
    h.push("Content-Type: text/plain; charset=utf-8".into());
    h.push("Content-Transfer-Encoding: base64".into());
    let b64 = base64::engine::general_purpose::STANDARD.encode(m.body.as_bytes());
    let body: Vec<String> = b64.as_bytes().chunks(76).map(|c| String::from_utf8_lossy(c).to_string()).collect();
    format!("{}\r\n\r\n{}\r\n", h.join("\r\n"), body.join("\r\n"))
}

pub(crate) async fn send(cfg: &SmtpConfig, m: &Outgoing) -> Result<(), String> {
    if !valid_address(&m.to) || !valid_address(&cfg.from) {
        return Err("indirizzo di posta non valido".into());
    }
    let mut c = open(cfg).await?;
    c.cmd(&format!("MAIL FROM:<{}>", cfg.from.trim()), &[250], "il mittente").await?;
    c.cmd(&format!("RCPT TO:<{}>", m.to.trim()), &[250, 251], "il destinatario").await?;
    c.cmd("DATA", &[354], "l'invio").await?;
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
    tokio::time::timeout(IO_TIMEOUT, async {
        c.io.get_mut().write_all(stuffed.as_bytes()).await?;
        c.io.get_mut().flush().await
    })
    .await
    .map_err(|_| "il server di posta non risponde in tempo".to_string())?
    .map_err(|e| format!("errore di rete: {e}"))?;
    c.expect(&[250], "il messaggio").await?;
    let _ = c.send("QUIT").await;
    Ok(())
}
