//! In-process fake IMAP and SMTP servers for tests (plain TCP speaking just enough of the protocols,
//! with a self-signed certificate for the TLS part). Nothing here touches the network beyond
//! `127.0.0.1`.

use rustls::pki_types::CertificateDer;
use std::collections::{BTreeMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

/// A self-signed certificate for `127.0.0.1` / `localhost`: (acceptor for a fake server, DER to trust).
pub fn self_signed() -> (TlsAcceptor, Vec<u8>) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()]).unwrap();
    let der = cert.cert.der().to_vec();
    let key = rustls::pki_types::PrivateKeyDer::try_from(cert.key_pair.serialize_der()).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(der.clone())], key)
        .unwrap();
    (TlsAcceptor::from(Arc::new(cfg)), der)
}

/// How a fake server secures the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// TLS from the first byte.
    Implicit,
    /// Plain greeting, `STARTTLS` offered.
    StartTls,
    /// Plain, and no `STARTTLS` offered (a server a client must refuse to log in to).
    PlainOnly,
}

type Boxed = Box<dyn Stream>;
trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

// ------------------------------------------------------------------------------------ IMAP

#[derive(Clone)]
pub struct ImapMessage {
    pub uid: u32,
    /// `YYYY-MM-DD` the server says it arrived.
    pub date: String,
    pub raw: Vec<u8>,
}

pub struct ImapOptions {
    pub security: Security,
    pub user: String,
    pub password: String,
    /// Folder name → messages.
    pub folders: Arc<Mutex<BTreeMap<String, Vec<ImapMessage>>>>,
}

impl ImapOptions {
    pub fn new(security: Security, folders: &[(&str, Vec<ImapMessage>)]) -> Self {
        Self {
            security,
            user: "mario".into(),
            password: "segreto".into(),
            folders: Arc::new(Mutex::new(folders.iter().map(|(n, m)| (n.to_string(), m.clone())).collect())),
        }
    }
}

pub struct FakeImap {
    pub addr: SocketAddr,
    /// The DER of the server certificate (give it to the client as an extra root).
    pub cert: Vec<u8>,
    /// Every command line received, in order (the tag removed).
    pub commands: Arc<Mutex<Vec<String>>>,
    /// UIDs that were flagged `\Seen` by a STORE.
    pub seen: Arc<Mutex<HashSet<u32>>>,
    folders: Arc<Mutex<BTreeMap<String, Vec<ImapMessage>>>>,
}

fn msg(uid: u32, date: &str, text: &str) -> ImapMessage {
    ImapMessage { uid, date: date.into(), raw: text.replace('\n', "\r\n").into_bytes() }
}

/// A message with the headers a mail client would show.
pub fn imap_message(uid: u32, date: &str, from: &str, subject: &str, body: &str) -> ImapMessage {
    let now = chrono::Local::now().to_rfc2822();
    msg(
        uid,
        date,
        &format!("From: {from}\nTo: io@example.org\nSubject: {subject}\nDate: {now}\nMessage-ID: <m{uid}@example.org>\n\n{body}\n"),
    )
}

impl FakeImap {
    pub async fn start(opts: ImapOptions) -> FakeImap {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (acceptor, cert) = self_signed();
        let commands = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::new(Mutex::new(HashSet::new()));
        let folders = opts.folders.clone();
        let opts = Arc::new(opts);
        let (c2, s2) = (commands.clone(), seen.clone());
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else { return };
                let (opts, acceptor, commands, seen) = (opts.clone(), acceptor.clone(), c2.clone(), s2.clone());
                tokio::spawn(async move {
                    let _ = imap_session(tcp, opts, acceptor, commands, seen).await;
                });
            }
        });
        FakeImap { addr, cert, commands, seen, folders }
    }

    pub fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }

    /// A message arrives in `folder` (created when it does not exist).
    pub fn add(&self, folder: &str, m: ImapMessage) {
        self.folders.lock().unwrap().entry(folder.to_string()).or_default().push(m);
    }
}

async fn read_line(r: &mut BufReader<Boxed>) -> std::io::Result<Option<String>> {
    let mut s = String::new();
    let n = r.read_line(&mut s).await?;
    Ok((n > 0).then(|| s.trim_end_matches(['\r', '\n']).to_string()))
}

async fn imap_session(
    tcp: tokio::net::TcpStream,
    opts: Arc<ImapOptions>,
    acceptor: TlsAcceptor,
    commands: Arc<Mutex<Vec<String>>>,
    seen: Arc<Mutex<HashSet<u32>>>,
) -> std::io::Result<()> {
    let first: Boxed = if opts.security == Security::Implicit { Box::new(acceptor.accept(tcp).await?) } else { Box::new(tcp) };
    let mut io = BufReader::new(first);
    io.get_mut().write_all(b"* OK fake imap ready\r\n").await?;
    let mut folder: Option<String> = None;
    let mut logged = false;
    while let Some(line) = read_line(&mut io).await? {
        let (tag, cmd) = line.split_once(' ').unwrap_or((&line, ""));
        let upper = cmd.to_ascii_uppercase();
        commands.lock().unwrap().push(cmd.to_string());
        let mut out = String::new();
        if upper.starts_with("CAPABILITY") {
            let starttls = if opts.security == Security::StartTls { " STARTTLS" } else { "" };
            out.push_str(&format!("* CAPABILITY IMAP4rev1{starttls} AUTH=PLAIN\r\n{tag} OK done\r\n"));
        } else if upper.starts_with("STARTTLS") && opts.security == Security::StartTls {
            io.get_mut().write_all(format!("{tag} OK begin TLS\r\n").as_bytes()).await?;
            let plain = io.into_inner();
            io = BufReader::new(Box::new(acceptor.accept(plain).await?));
            continue;
        } else if upper.starts_with("LOGIN") {
            let ok = cmd.contains(&format!("\"{}\"", opts.user)) && cmd.contains(&format!("\"{}\"", opts.password));
            logged = ok;
            out.push_str(&if ok {
                format!("{tag} OK logged in\r\n")
            } else {
                format!("{tag} NO [AUTHENTICATIONFAILED] bad credentials\r\n")
            });
        } else if !logged {
            out.push_str(&format!("{tag} NO login first\r\n"));
        } else if upper.starts_with("EXAMINE") || upper.starts_with("SELECT") {
            let name = cmd.split('"').nth(1).unwrap_or("").to_string();
            let folders = opts.folders.lock().unwrap().clone();
            match folders.get(&name) {
                Some(m) => {
                    out.push_str(&format!("* {} EXISTS\r\n* OK [UIDVALIDITY 7] ok\r\n{tag} OK [READ-ONLY] done\r\n", m.len()));
                    folder = Some(name);
                }
                None => out.push_str(&format!("{tag} NO no such folder\r\n")),
            }
        } else if upper.starts_with("UID SEARCH") {
            let msgs = folder.as_ref().and_then(|f| opts.folders.lock().unwrap().get(f).cloned()).unwrap_or_default();
            let ids: Vec<String> = if let Some(rest) = upper.strip_prefix("UID SEARCH UID ") {
                let from: u32 = rest.split(':').next().and_then(|n| n.parse().ok()).unwrap_or(1);
                let newest = msgs.iter().map(|m| m.uid).max();
                msgs.iter().filter(|m| m.uid >= from || Some(m.uid) == newest).map(|m| m.uid.to_string()).collect()
            } else if upper.starts_with("UID SEARCH SINCE ") {
                let since = chrono::NaiveDate::parse_from_str(cmd["UID SEARCH SINCE ".len()..].trim(), "%d-%b-%Y").ok();
                msgs.iter()
                    .filter(|m| match (since, chrono::NaiveDate::parse_from_str(&m.date, "%Y-%m-%d")) {
                        (Some(s), Ok(md)) => md >= s,
                        _ => true,
                    })
                    .map(|m| m.uid.to_string())
                    .collect()
            } else {
                msgs.iter().map(|m| m.uid.to_string()).collect()
            };
            out.push_str(&format!("* SEARCH {}\r\n{tag} OK done\r\n", ids.join(" ")));
        } else if upper.starts_with("UID FETCH") {
            let msgs = folder.as_ref().and_then(|f| opts.folders.lock().unwrap().get(f).cloned()).unwrap_or_default();
            let set = cmd.split_whitespace().nth(2).unwrap_or("");
            let limit: usize =
                upper.split("<0.").nth(1).and_then(|r| r.split('>').next()).and_then(|n| n.parse().ok()).unwrap_or(usize::MAX);
            for uid in set.split(',').filter_map(|u| u.parse::<u32>().ok()) {
                if let Some(m) = msgs.iter().find(|m| m.uid == uid) {
                    let body = &m.raw[..m.raw.len().min(limit)];
                    let date = chrono::NaiveDate::parse_from_str(&m.date, "%Y-%m-%d")
                        .map(|d| d.format("%d-%b-%Y").to_string())
                        .unwrap_or_default();
                    io.get_mut().write_all(out.as_bytes()).await?;
                    out.clear();
                    io.get_mut()
                        .write_all(
                            format!("* {uid} FETCH (UID {uid} INTERNALDATE \"{date} 09:00:00 +0000\" BODY[]<0> {{{}}}\r\n", body.len())
                                .as_bytes(),
                        )
                        .await?;
                    io.get_mut().write_all(body).await?;
                    io.get_mut().write_all(b")\r\n").await?;
                }
            }
            out.push_str(&format!("{tag} OK done\r\n"));
        } else if upper.starts_with("UID STORE") {
            if let Some(uid) = cmd.split_whitespace().nth(2).and_then(|u| u.parse::<u32>().ok()) {
                seen.lock().unwrap().insert(uid);
            }
            out.push_str(&format!("{tag} OK stored\r\n"));
        } else if upper.starts_with("LOGOUT") {
            io.get_mut().write_all(format!("* BYE bye\r\n{tag} OK logout\r\n").as_bytes()).await?;
            return Ok(());
        } else {
            out.push_str(&format!("{tag} BAD unknown command\r\n"));
        }
        io.get_mut().write_all(out.as_bytes()).await?;
    }
    Ok(())
}

// ------------------------------------------------------------------------------------ SMTP

/// A message the fake SMTP server took: (envelope sender, recipients, DATA).
pub type SmtpMail = (String, Vec<String>, String);

pub struct FakeSmtp {
    pub addr: SocketAddr,
    pub cert: Vec<u8>,
    pub commands: Arc<Mutex<Vec<String>>>,
    /// The messages received: (envelope sender, recipients, DATA as sent with the CRLFs).
    pub mails: Arc<Mutex<Vec<SmtpMail>>>,
}

impl FakeSmtp {
    pub async fn start(security: Security, user: &str, password: &str) -> FakeSmtp {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (acceptor, cert) = self_signed();
        let commands = Arc::new(Mutex::new(Vec::new()));
        let mails = Arc::new(Mutex::new(Vec::new()));
        let (c2, m2) = (commands.clone(), mails.clone());
        let (user, password) = (user.to_string(), password.to_string());
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else { return };
                let (acceptor, commands, mails, user, password) =
                    (acceptor.clone(), c2.clone(), m2.clone(), user.clone(), password.clone());
                tokio::spawn(async move {
                    let _ = smtp_session(tcp, security, acceptor, commands, mails, user, password).await;
                });
            }
        });
        FakeSmtp { addr, cert, commands, mails }
    }
}

async fn smtp_session(
    tcp: tokio::net::TcpStream,
    security: Security,
    acceptor: TlsAcceptor,
    commands: Arc<Mutex<Vec<String>>>,
    mails: Arc<Mutex<Vec<SmtpMail>>>,
    user: String,
    password: String,
) -> std::io::Result<()> {
    use base64::Engine as _;
    let first: Boxed = if security == Security::Implicit { Box::new(acceptor.accept(tcp).await?) } else { Box::new(tcp) };
    let mut io = BufReader::new(first);
    let mut encrypted = security == Security::Implicit;
    io.get_mut().write_all(b"220 fake smtp ready\r\n").await?;
    let (mut from, mut rcpt) = (String::new(), Vec::<String>::new());
    while let Some(line) = read_line(&mut io).await? {
        let upper = line.to_ascii_uppercase();
        commands.lock().unwrap().push(if upper.starts_with("AUTH") { "AUTH …".into() } else { line.clone() });
        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            let starttls = if security == Security::StartTls && !encrypted { "250-STARTTLS\r\n" } else { "" };
            io.get_mut().write_all(format!("250-localhost\r\n{starttls}250 AUTH PLAIN LOGIN\r\n").as_bytes()).await?;
        } else if upper.starts_with("STARTTLS") && security == Security::StartTls {
            io.get_mut().write_all(b"220 go ahead\r\n").await?;
            let plain = io.into_inner();
            io = BufReader::new(Box::new(acceptor.accept(plain).await?));
            encrypted = true;
        } else if upper.starts_with("AUTH PLAIN") {
            let blob = line.split_whitespace().nth(2).unwrap_or("");
            let decoded = base64::engine::general_purpose::STANDARD.decode(blob).unwrap_or_default();
            let ok = decoded == format!("\0{user}\0{password}").into_bytes();
            io.get_mut().write_all(if ok && encrypted { b"235 ok\r\n" as &[u8] } else { b"535 bad credentials\r\n" }).await?;
        } else if upper.starts_with("MAIL FROM") {
            from = line.split('<').nth(1).and_then(|s| s.split('>').next()).unwrap_or("").to_string();
            rcpt.clear();
            io.get_mut().write_all(b"250 ok\r\n").await?;
        } else if upper.starts_with("RCPT TO") {
            rcpt.push(line.split('<').nth(1).and_then(|s| s.split('>').next()).unwrap_or("").to_string());
            io.get_mut().write_all(b"250 ok\r\n").await?;
        } else if upper.starts_with("DATA") {
            io.get_mut().write_all(b"354 go\r\n").await?;
            let mut data = String::new();
            while let Some(l) = read_line(&mut io).await? {
                if l == "." {
                    break;
                }
                data.push_str(l.strip_prefix("..").map(|r| format!(".{r}")).as_deref().unwrap_or(&l));
                data.push_str("\r\n");
            }
            mails.lock().unwrap().push((from.clone(), rcpt.clone(), data));
            io.get_mut().write_all(b"250 queued\r\n").await?;
        } else if upper.starts_with("QUIT") {
            io.get_mut().write_all(b"221 bye\r\n").await?;
            return Ok(());
        } else if upper.starts_with("NOOP") || upper.starts_with("RSET") {
            io.get_mut().write_all(b"250 ok\r\n").await?;
        } else {
            io.get_mut().write_all(b"502 not implemented\r\n").await?;
        }
    }
    Ok(())
}
