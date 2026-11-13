//! A small IMAP client over TLS, written for reading mail without touching it.
//!
//! * TLS only: implicit TLS (port 993) or STARTTLS (any other port, and only if the server accepts it).
//!   The password is sent after the channel is encrypted and never before.
//! * Read-only by construction: every command goes through [`Session::command`], which only knows a
//!   short list of verbs (`CAPABILITY`, `STARTTLS`, `LOGIN`, `EXAMINE`, `UID SEARCH`, `UID FETCH`,
//!   `NOOP`, `LOGOUT`). Messages are fetched with `BODY.PEEK[]`, which never sets `\Seen`.
//!   `SELECT` and `UID STORE` exist only for [`Access::MarkSeen`], used by the email *channel* in a
//!   folder the person dedicated to the assistant and only when they turned «segna come letti» on.
//!   `EXPUNGE`, `APPEND`, `COPY`, `MOVE` and `DELETE` do not exist at all.
//! * Bounded: every network step has a timeout, a line and a literal have a size cap, a message is
//!   fetched at most `max_bytes` long.

use super::tls::{self, BoxIo, NetError};
use chrono::{DateTime, Local, NaiveDate};
use std::fmt;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const IO_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_LINE: usize = 64 * 1024;
const MAX_LITERAL: usize = 4 * 1024 * 1024;
pub const DEFAULT_PORT: u16 = 993;

/// What the session is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    MarkSeen,
}

/// An account as the connector uses it (the password comes from the secret store).
#[derive(Clone, PartialEq, Default)]
pub struct Account {
    pub id: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub folders: Vec<String>,
    /// `implicit` | `starttls`; empty = implicit on 993, STARTTLS on any other port.
    pub tls: String,
}

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Account").field("id", &self.id).field("host", &self.host).field("port", &self.port).field("password", &"…").finish()
    }
}

impl Account {
    pub fn implicit_tls(&self) -> bool {
        match self.tls.as_str() {
            "implicit" => true,
            "starttls" => false,
            _ => self.port == DEFAULT_PORT || self.port == 0,
        }
    }
    pub fn port_or_default(&self) -> u16 {
        if self.port == 0 {
            DEFAULT_PORT
        } else {
            self.port
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImapError {
    Net(NetError),
    /// The server refused the user name or the password.
    Auth,
    /// STARTTLS was needed and the server does not offer it: nothing was sent in clear.
    NoTls,
    Protocol(String),
    /// The mailbox does not exist.
    NoFolder(String),
}

impl fmt::Display for ImapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImapError::Net(e) => write!(f, "{e}"),
            ImapError::Auth => {
                write!(f, "nome utente o password non accettati dal server (con Gmail e simili serve una password per le app)")
            }
            ImapError::NoTls => write!(f, "il server non offre una connessione cifrata (STARTTLS): non invio la password in chiaro"),
            ImapError::Protocol(m) => write!(f, "risposta inattesa dal server di posta: {m}"),
            ImapError::NoFolder(n) => write!(f, "la cartella «{n}» non esiste sul server"),
        }
    }
}

impl From<NetError> for ImapError {
    fn from(e: NetError) -> Self {
        ImapError::Net(e)
    }
}

/// One message as the server sent it.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub uid: u32,
    pub internal_date: Option<DateTime<Local>>,
    pub raw: Vec<u8>,
}

/// An untagged response: the text with each literal replaced by `\0`, and the literals.
#[derive(Debug, Default)]
struct Item {
    head: String,
    literals: Vec<Vec<u8>>,
}

struct Response {
    items: Vec<Item>,
    ok: bool,
    text: String,
}

pub struct Session {
    io: BufReader<BoxIo>,
    tag: u32,
    access: Access,
    /// `UIDVALIDITY` of the folder last examined.
    pub uidvalidity: Option<u32>,
}

fn quote(s: &str) -> Result<String, ImapError> {
    if s.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
        return Err(ImapError::Protocol("carattere non ammesso nel nome utente, nella password o nella cartella".into()));
    }
    Ok(format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
}

/// Encodes a folder name in modified UTF-7 (RFC 3501 §5.1.3) when it is not plain ASCII.
fn mutf7(name: &str) -> String {
    if name.is_ascii() && !name.contains('&') {
        return name.to_string();
    }
    use base64::Engine as _;
    let mut out = String::new();
    let mut pending: Vec<u16> = vec![];
    let flush = |pending: &mut Vec<u16>, out: &mut String| {
        if pending.is_empty() {
            return;
        }
        let bytes: Vec<u8> = pending.iter().flat_map(|u| u.to_be_bytes()).collect();
        let b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes).replace('/', ",");
        out.push('&');
        out.push_str(&b64);
        out.push('-');
        pending.clear();
    };
    for c in name.chars() {
        if c == '&' {
            flush(&mut pending, &mut out);
            out.push_str("&-");
        } else if (' '..='~').contains(&c) {
            flush(&mut pending, &mut out);
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            pending.extend_from_slice(c.encode_utf16(&mut buf));
        }
    }
    flush(&mut pending, &mut out);
    out
}

/// Is `cmd` something this client may send with `access`?
pub fn command_allowed(cmd: &str, access: Access) -> bool {
    let upper = cmd.trim_start().to_ascii_uppercase();
    let mut words = upper.split_whitespace();
    let (w1, w2) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    match (w1, w2) {
        ("CAPABILITY" | "STARTTLS" | "LOGIN" | "EXAMINE" | "NOOP" | "LOGOUT", _) => true,
        ("UID", "SEARCH" | "FETCH") => !upper.contains("BODY[") && !upper.contains("RFC822 ") && !upper.ends_with("RFC822)"),
        ("SELECT", _) | ("UID", "STORE") => access == Access::MarkSeen,
        _ => false,
    }
}

impl Session {
    /// Opens the connection, secures it and logs in.
    pub async fn connect(acct: &Account, extra_roots: &[Vec<u8>], access: Access) -> Result<Session, ImapError> {
        let (host, port) = (acct.host.trim(), acct.port_or_default());
        let io: BoxIo = if acct.implicit_tls() {
            Box::new(tls::connect_tls(host, port, extra_roots, IO_TIMEOUT).await?)
        } else {
            Box::new(tls::connect_tcp(host, port, IO_TIMEOUT).await?)
        };
        let mut s = Session { io: BufReader::new(io), tag: 0, access, uidvalidity: None };
        let greeting = s.read_item().await?;
        if !greeting.head.to_ascii_uppercase().starts_with("* OK") && !greeting.head.to_ascii_uppercase().starts_with("* PREAUTH") {
            return Err(ImapError::Protocol(greeting.head.trim().chars().take(80).collect()));
        }
        if !acct.implicit_tls() {
            let caps = s.command("CAPABILITY").await?;
            let offers = caps.items.iter().any(|i| i.head.to_ascii_uppercase().contains("STARTTLS"));
            if !offers {
                return Err(ImapError::NoTls);
            }
            let r = s.command("STARTTLS").await?;
            if !r.ok {
                return Err(ImapError::NoTls);
            }
            // Nothing may be buffered across the upgrade (it would be plaintext injected by someone in the middle).
            if !s.io.buffer().is_empty() {
                return Err(ImapError::Protocol("dati inattesi prima dell'avvio della cifratura".into()));
            }
            let plain = s.io.into_inner();
            let upgraded = tls::upgrade(plain, host, extra_roots, IO_TIMEOUT).await?;
            s.io = BufReader::new(Box::new(upgraded));
        }
        let login = format!("LOGIN {} {}", quote(&acct.username)?, quote(&acct.password)?);
        let r = s.command(&login).await?;
        if !r.ok {
            return Err(ImapError::Auth);
        }
        Ok(s)
    }

    async fn read_line(&mut self) -> Result<Vec<u8>, ImapError> {
        let mut line = Vec::new();
        let read = tokio::time::timeout(IO_TIMEOUT, async {
            loop {
                let n = (&mut self.io).take((MAX_LINE - line.len()) as u64 + 1).read_until(b'\n', &mut line).await?;
                if n == 0 || line.ends_with(b"\n") || line.len() > MAX_LINE {
                    return Ok::<_, std::io::Error>(());
                }
            }
        })
        .await;
        match read {
            Err(_) => Err(NetError::Timeout.into()),
            Ok(Err(e)) => Err(NetError::Io(e.to_string()).into()),
            Ok(Ok(())) if line.is_empty() => Err(NetError::Io("il server ha chiuso la connessione".into()).into()),
            Ok(Ok(())) if line.len() > MAX_LINE => Err(ImapError::Protocol("riga troppo lunga".into())),
            Ok(Ok(())) => Ok(line),
        }
    }

    /// One response item (a line with its literals).
    async fn read_item(&mut self) -> Result<Item, ImapError> {
        let mut item = Item::default();
        loop {
            let line = self.read_line().await?;
            let text = String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_string();
            let literal = text
                .strip_suffix('}')
                .and_then(|t| t.rfind('{').map(|i| (t[..i].to_string(), t[i + 1..].trim_end_matches('+').to_string())));
            match literal.and_then(|(before, n)| n.parse::<usize>().ok().map(|n| (before, n))) {
                Some((before, n)) => {
                    if n > MAX_LITERAL {
                        return Err(ImapError::Protocol("messaggio troppo grande".into()));
                    }
                    item.head.push_str(&before);
                    item.head.push('\0');
                    let mut buf = vec![0u8; n];
                    match tokio::time::timeout(IO_TIMEOUT, self.io.read_exact(&mut buf)).await {
                        Err(_) => return Err(NetError::Timeout.into()),
                        Ok(Err(e)) => return Err(NetError::Io(e.to_string()).into()),
                        Ok(Ok(_)) => {}
                    }
                    item.literals.push(buf);
                }
                None => {
                    item.head.push_str(&text);
                    return Ok(item);
                }
            }
        }
    }

    /// Sends one command and reads everything up to its tagged answer.
    async fn command(&mut self, cmd: &str) -> Result<Response, ImapError> {
        if !command_allowed(cmd, self.access) {
            return Err(ImapError::Protocol("comando non consentito (la posta si legge soltanto)".into()));
        }
        self.tag += 1;
        let tag = format!("a{:03}", self.tag);
        let line = format!("{tag} {cmd}\r\n");
        match tokio::time::timeout(IO_TIMEOUT, async {
            self.io.get_mut().write_all(line.as_bytes()).await?;
            self.io.get_mut().flush().await
        })
        .await
        {
            Err(_) => return Err(NetError::Timeout.into()),
            Ok(Err(e)) => return Err(NetError::Io(e.to_string()).into()),
            Ok(Ok(())) => {}
        }
        let mut items = Vec::new();
        loop {
            let item = self.read_item().await?;
            if let Some(rest) = item.head.strip_prefix(&format!("{tag} ")) {
                let status = rest.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
                return Ok(Response { items, ok: status == "OK", text: rest.to_string() });
            }
            items.push(item);
        }
    }

    /// Opens a folder for reading (`EXAMINE`: no flag can change, nothing becomes `\Seen`). With
    /// [`Access::MarkSeen`] the folder is selected read-write instead.
    pub async fn open_folder(&mut self, folder: &str) -> Result<(), ImapError> {
        let verb = if self.access == Access::MarkSeen { "SELECT" } else { "EXAMINE" };
        let r = self.command(&format!("{verb} {}", quote(&mutf7(folder))?)).await?;
        if !r.ok {
            return Err(ImapError::NoFolder(folder.to_string()));
        }
        self.uidvalidity = r.items.iter().find_map(|i| {
            let u = i.head.to_ascii_uppercase();
            let at = u.find("[UIDVALIDITY ")?;
            u[at + 13..].split(']').next()?.trim().parse().ok()
        });
        Ok(())
    }

    async fn search(&mut self, criteria: &str) -> Result<Vec<u32>, ImapError> {
        let r = self.command(&format!("UID SEARCH {criteria}")).await?;
        if !r.ok {
            return Err(ImapError::Protocol(r.text.chars().take(80).collect()));
        }
        let mut uids: Vec<u32> = r
            .items
            .iter()
            .filter(|i| i.head.to_ascii_uppercase().starts_with("* SEARCH"))
            .flat_map(|i| i.head.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect::<Vec<u32>>())
            .collect();
        uids.sort_unstable();
        uids.dedup();
        Ok(uids)
    }

    /// UIDs of the messages received on or after `date`, ascending.
    pub async fn search_since(&mut self, date: NaiveDate) -> Result<Vec<u32>, ImapError> {
        self.search(&format!("SINCE {}", date.format("%d-%b-%Y"))).await
    }

    /// UIDs greater than `last`, ascending (`n:*` always returns the newest message: it is filtered here).
    pub async fn search_after(&mut self, last: u32) -> Result<Vec<u32>, ImapError> {
        Ok(self.search(&format!("UID {}:*", last.saturating_add(1))).await?.into_iter().filter(|u| *u > last).collect())
    }

    /// The messages with these UIDs, each cut at `max_bytes`. `BODY.PEEK` leaves every flag alone.
    pub async fn fetch_peek(&mut self, uids: &[u32], max_bytes: usize) -> Result<Vec<Fetched>, ImapError> {
        let mut out = Vec::new();
        for chunk in uids.chunks(10) {
            let set = chunk.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
            let r = self.command(&format!("UID FETCH {set} (UID INTERNALDATE BODY.PEEK[]<0.{max_bytes}>)")).await?;
            if !r.ok {
                return Err(ImapError::Protocol(r.text.chars().take(80).collect()));
            }
            for item in r.items {
                if !item.head.to_ascii_uppercase().contains("FETCH") {
                    continue;
                }
                let up = item.head.to_ascii_uppercase();
                let uid = up.find("UID ").and_then(|i| up[i + 4..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok());
                let (Some(uid), Some(raw)) = (uid, item.literals.into_iter().next()) else { continue };
                let internal_date = item.head.find("INTERNALDATE \"").and_then(|i| {
                    let rest = &item.head[i + 14..];
                    let s = rest.split('"').next()?.trim();
                    DateTime::parse_from_str(s, "%d-%b-%Y %H:%M:%S %z").ok().map(|d| d.with_timezone(&Local))
                });
                out.push(Fetched { uid, internal_date, raw });
            }
        }
        out.sort_by_key(|f| f.uid);
        Ok(out)
    }

    /// Sets `\Seen` on one message (only for [`Access::MarkSeen`]).
    pub async fn mark_seen(&mut self, uid: u32) -> Result<(), ImapError> {
        let r = self.command(&format!("UID STORE {uid} +FLAGS.SILENT (\\Seen)")).await?;
        if r.ok {
            Ok(())
        } else {
            Err(ImapError::Protocol(r.text.chars().take(80).collect()))
        }
    }

    pub async fn logout(mut self) {
        let _ = self.command("LOGOUT").await;
    }
}

#[cfg(test)]
mod tests;
