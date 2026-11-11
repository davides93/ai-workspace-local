//! Reading local mail, read-only: Maildir folders and `.mbox` / `.eml` files.
//!
//! Nothing here opens a file for writing, moves a message out of `new/`, sets a flag or talks to a
//! server. Headers are decoded (RFC 2047 words), the body is taken from the first `text/plain` part
//! (or the tags are stripped from an HTML one), transfer encodings base64 and quoted-printable are
//! undone, and charsets other than UTF-8 are read as Latin-1/Windows-1252.

use base64::Engine as _;
use chrono::{DateTime, Local};
use std::path::{Path, PathBuf};

/// Bytes of one message looked at.
const MAX_MESSAGE: usize = 512 * 1024;
/// Bytes of one mbox file looked at.
const MAX_MBOX: u64 = 64 * 1024 * 1024;
/// Messages read per source.
const MAX_PER_SOURCE: usize = 3000;

#[derive(Debug, Clone, PartialEq)]
pub struct Mail {
    pub from: String,
    pub subject: String,
    pub date: DateTime<Local>,
    /// The start of the text, one line.
    pub snippet: String,
    /// More of the text, for searches only (not shown).
    pub body: String,
}

// ------------------------------------------------------------------------------------ decoding

fn split_headers(raw: &[u8]) -> (&[u8], &[u8]) {
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\n' && (raw.get(i + 1) == Some(&b'\n') || (raw.get(i + 1) == Some(&b'\r') && raw.get(i + 2) == Some(&b'\n'))) {
            let skip = if raw[i + 1] == b'\n' { 2 } else { 3 };
            return (&raw[..i], &raw[(i + skip).min(raw.len())..]);
        }
        i += 1;
    }
    (raw, &[])
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

fn decode_charset(bytes: &[u8], charset: &str) -> String {
    let cs = charset.trim().trim_matches('"').to_ascii_lowercase();
    if cs.is_empty() || cs == "utf-8" || cs == "utf8" || cs == "us-ascii" {
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => latin1(bytes),
        }
    } else {
        latin1(bytes)
    }
}

fn qp_decode(input: &[u8], header: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            b'=' if i + 1 < input.len() && (input[i + 1] == b'\n' || input[i + 1] == b'\r') => {
                // soft line break
                i += 1;
                if input[i] == b'\r' {
                    i += 1;
                }
                if input.get(i) == Some(&b'\n') {
                    i += 1;
                }
            }
            b'=' if i + 2 < input.len() && input[i + 1].is_ascii_hexdigit() && input[i + 2].is_ascii_hexdigit() => {
                let h = std::str::from_utf8(&input[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(h, 16).unwrap());
                i += 3;
            }
            b'_' if header => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn b64_decode(input: &[u8]) -> Vec<u8> {
    let clean: Vec<u8> = input.iter().copied().filter(|c| !c.is_ascii_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(&clean)
        .or_else(|_| {
            base64::engine::general_purpose::STANDARD_NO_PAD.decode(clean.iter().copied().filter(|c| *c != b'=').collect::<Vec<_>>())
        })
        .unwrap_or_default()
}

/// Decodes the `=?charset?B|Q?text?=` words of a header value.
pub fn decode_words(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    let mut last_was_word = false;
    while let Some(i) = rest.find("=?") {
        let (before, tail) = rest.split_at(i);
        let parsed = (|| {
            let body = &tail[2..];
            let (charset, r) = body.split_once('?')?;
            let (enc, r) = r.split_once('?')?;
            let end = r.find("?=")?;
            let text = &r[..end];
            let bytes = match enc.to_ascii_uppercase().as_str() {
                "B" => b64_decode(text.as_bytes()),
                "Q" => qp_decode(text.as_bytes(), true),
                _ => return None,
            };
            Some((decode_charset(&bytes, charset.split('*').next().unwrap_or("")), 2 + charset.len() + 1 + enc.len() + 1 + end + 2))
        })();
        match parsed {
            Some((text, used)) => {
                // Whitespace between two encoded words is not part of the text.
                if !(last_was_word && before.trim().is_empty()) {
                    out.push_str(before);
                }
                out.push_str(&text);
                rest = &tail[used..];
                last_was_word = true;
            }
            None => {
                out.push_str(before);
                out.push_str("=?");
                rest = &tail[2..];
                last_was_word = false;
            }
        }
    }
    out.push_str(rest);
    out
}

struct Headers(Vec<(String, String)>);

impl Headers {
    fn parse(raw: &[u8]) -> Headers {
        let text = latin1(raw);
        let mut out: Vec<(String, String)> = Vec::new();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.starts_with(' ') || line.starts_with('\t') {
                if let Some(l) = out.last_mut() {
                    l.1.push(' ');
                    l.1.push_str(line.trim());
                }
            } else if let Some((k, v)) = line.split_once(':') {
                out.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
            }
        }
        Headers(out)
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    /// Header values were read as Latin-1 bytes; re-read raw UTF-8 when it is valid.
    fn text(&self, k: &str) -> String {
        let v = self.get(k).unwrap_or_default();
        let bytes: Vec<u8> = v.chars().map(|c| c as u32).take_while(|c| *c < 256).map(|c| c as u8).collect();
        let fixed = match std::str::from_utf8(&bytes) {
            Ok(s) if bytes.len() == v.chars().count() => s.to_string(),
            _ => v.to_string(),
        };
        decode_words(&fixed)
    }
}

fn param(value: &str, name: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let key = format!("{name}=");
    let i = lower.find(&key)?;
    let rest = value[i + key.len()..].trim_start();
    Some(if let Some(r) = rest.strip_prefix('"') {
        r.split('"').next().unwrap_or("").to_string()
    } else {
        rest.split(';').next().unwrap_or("").trim().to_string()
    })
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut skip_to: Option<&str> = None;
    let lower = html.to_ascii_lowercase();
    let b = html.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if let Some(end) = skip_to {
            match lower[i..].find(end) {
                Some(j) => {
                    i += j + end.len();
                    skip_to = None;
                    continue;
                }
                None => break,
            }
        }
        let c = b[i];
        if in_tag {
            if c == b'>' {
                in_tag = false;
            }
            i += 1;
        } else if c == b'<' {
            if lower[i..].starts_with("<style") {
                skip_to = Some("</style>");
            } else if lower[i..].starts_with("<script") {
                skip_to = Some("</script>");
            } else if lower[i..].starts_with("<br")
                || lower[i..].starts_with("<p")
                || lower[i..].starts_with("</p")
                || lower[i..].starts_with("<div")
            {
                out.push(' ');
            }
            in_tag = true;
            i += 1;
        } else {
            let ch_len = html[i..].chars().next().map(char::len_utf8).unwrap_or(1);
            out.push_str(&html[i..i + ch_len]);
            i += ch_len;
        }
    }
    out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'")
}

/// The text of a message body given its headers (recursing into multipart).
fn body_text(h: &Headers, body: &[u8], depth: u8) -> Option<(String, bool)> {
    let ctype = h.get("content-type").unwrap_or("text/plain").to_string();
    let ct = ctype.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if ct.starts_with("multipart/") && depth < 4 {
        let boundary = param(&ctype, "boundary")?;
        let marker = format!("--{boundary}");
        let text = latin1(body);
        let mut best: Option<(String, bool)> = None;
        for part in text.split(&marker).skip(1) {
            if part.starts_with("--") {
                break;
            }
            let raw: Vec<u8> = part.chars().map(|c| c as u32 as u8).collect();
            let raw = raw.strip_prefix(b"\r\n").or_else(|| raw.strip_prefix(b"\n")).unwrap_or(&raw);
            let (ph, pb) = split_headers(raw);
            let ph = Headers::parse(ph);
            if let Some((t, is_html)) = body_text(&ph, pb, depth + 1) {
                // A plain-text part wins over an HTML one.
                if best.as_ref().map(|b| b.1 && !is_html).unwrap_or(true) && !t.trim().is_empty() {
                    best = Some((t, is_html));
                }
            }
        }
        return best;
    }
    let is_html = ct == "text/html";
    if !(ct == "text/plain" || is_html) {
        return None;
    }
    if h.get("content-disposition").is_some_and(|d| d.to_ascii_lowercase().starts_with("attachment")) {
        return None;
    }
    let decoded = match h.get("content-transfer-encoding").map(|e| e.trim().to_ascii_lowercase()).as_deref() {
        Some("base64") => b64_decode(body),
        Some("quoted-printable") => qp_decode(body, false),
        _ => body.to_vec(),
    };
    let text = decode_charset(&decoded, &param(&ctype, "charset").unwrap_or_default());
    Some((if is_html { strip_tags(&text) } else { text }, is_html))
}

fn one_line(s: &str, max: usize) -> String {
    let lines: Vec<&str> = s.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('>')).collect();
    let joined = lines.join(" ");
    let squeezed = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    if squeezed.chars().count() <= max {
        squeezed
    } else {
        format!("{}…", squeezed.chars().take(max.saturating_sub(1)).collect::<String>().trim_end())
    }
}

/// Parses one RFC 822 message. `fallback_date` is used when it has no readable `Date`.
pub fn parse_message(raw: &[u8], fallback_date: DateTime<Local>) -> Option<Mail> {
    let raw = &raw[..raw.len().min(MAX_MESSAGE)];
    let (head, body) = split_headers(raw);
    let h = Headers::parse(head);
    if h.get("from").is_none() && h.get("subject").is_none() {
        return None;
    }
    let date = h
        .get("date")
        .and_then(|d| DateTime::parse_from_rfc2822(d.trim()).ok().or_else(|| DateTime::parse_from_rfc2822(&strip_comment(d)).ok()))
        .map(|d| d.with_timezone(&Local))
        .unwrap_or(fallback_date);
    let text = body_text(&h, body, 0).map(|t| t.0).unwrap_or_default();
    let from = h.text("from");
    let subject = h.text("subject");
    Some(Mail {
        from: pretty_sender(&from),
        subject: if subject.trim().is_empty() { "(senza oggetto)".into() } else { subject.trim().to_string() },
        date,
        snippet: one_line(&text, 200),
        body: one_line(&text, 20_000),
    })
}

/// `Mario Rossi <mario@x.it>` → `Mario Rossi`; a bare address stays as it is.
fn pretty_sender(from: &str) -> String {
    let f = from.trim();
    match f.split_once('<') {
        Some((name, addr)) => {
            let name = name.trim().trim_matches('"').trim();
            if name.is_empty() {
                addr.trim_end_matches('>').trim().to_string()
            } else {
                name.to_string()
            }
        }
        None => f.to_string(),
    }
}

/// `Tue, 5 Mar 2030 10:00:00 +0100 (CET)` → without the trailing comment.
fn strip_comment(d: &str) -> String {
    match d.find('(') {
        Some(i) => d[..i].trim().to_string(),
        None => d.trim().to_string(),
    }
}

// ------------------------------------------------------------------------------------ sources

fn mtime(p: &Path) -> DateTime<Local> {
    std::fs::metadata(p).and_then(|m| m.modified()).map(DateTime::<Local>::from).unwrap_or_else(|_| Local::now())
}

fn read_capped(p: &Path, cap: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let f = std::fs::File::open(p).ok()?;
    let mut buf = Vec::new();
    f.take(cap).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Splits an mbox into messages (a line starting with `From ` after a blank line, or at the start).
fn split_mbox(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < data.len() {
        if data[i] == b'\n' && data[i + 1..].starts_with(b"From ") {
            out.push(&data[start..i + 1]);
            start = i + 1;
        }
        i += 1;
    }
    if start < data.len() {
        out.push(&data[start..]);
    }
    out.into_iter()
        .map(|m| {
            if m.starts_with(b"From ") {
                m.split(|b| *b == b'\n').next().map(|l| &m[(l.len() + 1).min(m.len())..]).unwrap_or(m)
            } else {
                m
            }
        })
        .collect()
}

fn is_maildir(p: &Path) -> bool {
    p.join("cur").is_dir() || p.join("new").is_dir()
}

fn maildir_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for sub in ["new", "cur"] {
        if let Ok(rd) = std::fs::read_dir(dir.join(sub)) {
            out.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_file()));
        }
    }
}

/// Every message of one source. `not_older_than` lets Maildir/`.eml` files that were last touched before
/// that moment be skipped without opening them (their `Date` can only be earlier than their delivery).
pub fn read_source(spec: &Path, not_older_than: Option<DateTime<Local>>) -> Result<Vec<Mail>, String> {
    if !spec.exists() {
        return Err(format!("«{}» non esiste", spec.display()));
    }
    let mut maildirs: Vec<PathBuf> = vec![];
    let mut files: Vec<PathBuf> = vec![];
    if spec.is_dir() {
        let mut stack = vec![(spec.to_path_buf(), 0u8)];
        while let Some((dir, depth)) = stack.pop() {
            if is_maildir(&dir) {
                maildirs.push(dir);
                continue;
            }
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if p.is_dir() {
                    if depth < 4 && name != "tmp" {
                        stack.push((p, depth + 1));
                    }
                } else if name.ends_with(".eml") || name.ends_with(".mbox") || name.ends_with(".emlx") {
                    files.push(p);
                }
            }
        }
    } else {
        files.push(spec.to_path_buf());
    }
    for m in &maildirs {
        maildir_files(m, &mut files);
    }
    let mut out = Vec::new();
    for f in files {
        if out.len() >= MAX_PER_SOURCE {
            break;
        }
        let modified = mtime(&f);
        if let Some(limit) = not_older_than {
            // Only single-message files: an mbox keeps growing and is always looked at.
            let is_mbox = f.extension().is_some_and(|e| e == "mbox");
            if !is_mbox && modified < limit - chrono::Duration::days(2) {
                continue;
            }
        }
        let is_mbox = f.extension().is_some_and(|e| e == "mbox");
        let Some(data) = read_capped(&f, if is_mbox { MAX_MBOX } else { MAX_MESSAGE as u64 }) else { continue };
        if is_mbox || data.starts_with(b"From ") {
            for m in split_mbox(&data) {
                if let Some(mail) = parse_message(m, modified) {
                    out.push(mail);
                }
            }
        } else if let Some(mail) = parse_message(&data, modified) {
            out.push(mail);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone, Utc};

    fn fallback() -> DateTime<Local> {
        Local.from_utc_datetime(&chrono::NaiveDate::from_ymd_opt(2030, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap())
    }

    #[test]
    fn encoded_headers_are_decoded() {
        assert_eq!(decode_words("=?UTF-8?B?Q2lhbyBtb25kbyDwn5GL?="), "Ciao mondo 👋");
        assert_eq!(decode_words("=?iso-8859-1?Q?Caff=E8_freddo?="), "Caffè freddo");
        assert_eq!(decode_words("=?UTF-8?Q?Un?= =?UTF-8?Q?_oggetto?="), "Un oggetto", "space between words is dropped");
        assert_eq!(decode_words("Re: =?UTF-8?Q?perch=C3=A9?= no"), "Re: perché no");
        assert_eq!(decode_words("niente =? di codificato"), "niente =? di codificato");
    }

    #[test]
    fn a_plain_message() {
        let raw = b"From: Mario Rossi <mario@example.org>\r\nSubject: Preventivo\r\nDate: Tue, 5 Mar 2030 10:00:00 +0100\r\n\r\nCiao,\r\necco il preventivo.\r\n> citazione\r\nGrazie\r\n";
        let m = parse_message(raw, fallback()).unwrap();
        assert_eq!((m.from.as_str(), m.subject.as_str()), ("Mario Rossi", "Preventivo"));
        assert_eq!(m.snippet, "Ciao, ecco il preventivo. Grazie", "quoted lines are left out");
        assert_eq!(m.date.with_timezone(&Utc).format("%Y-%m-%d %H:%M").to_string(), "2030-03-05 09:00");
    }

    #[test]
    fn multipart_prefers_plain_text_and_decodes_quoted_printable() {
        let raw = concat!(
            "From: \"=?UTF-8?Q?Ren=C3=A9?=\" <r@example.org>\r\nSubject: =?UTF-8?B?w6lwcHVyZQ==?=\r\nDate: Tue, 5 Mar 2030 10:00:00 +0000\r\n",
            "Content-Type: multipart/alternative; boundary=\"XX\"\r\n\r\n--XX\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Solo <b>html</b></p>\r\n",
            "--XX\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nCaff=C3=A8 alle tre=\r\n  ok\r\n--XX--\r\n"
        );
        let m = parse_message(raw.as_bytes(), fallback()).unwrap();
        assert_eq!((m.from.as_str(), m.subject.as_str()), ("René", "éppure"));
        assert_eq!(m.snippet, "Caffè alle tre ok");
    }

    #[test]
    fn html_only_and_base64_bodies() {
        let raw = "From: a@b.it\r\nSubject: Html\r\nContent-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\nPHN0eWxlPnAge30gPC9zdHlsZT48cD5DaWFvIDxiPm1vbmRvPC9iPiZuYnNwO2Zhbm88L3A+\r\n";
        let m = parse_message(raw.as_bytes(), fallback()).unwrap();
        assert_eq!(m.snippet, "Ciao mondo fano");
        assert_eq!(m.date, fallback(), "no Date: the fallback is used");
    }

    #[test]
    fn latin1_bodies_and_missing_subject() {
        let mut raw = b"From: a@b.it\r\nContent-Type: text/plain; charset=iso-8859-1\r\n\r\nCaff".to_vec();
        raw.push(0xE8);
        let m = parse_message(&raw, fallback()).unwrap();
        assert_eq!((m.subject.as_str(), m.snippet.as_str()), ("(senza oggetto)", "Caffè"));
        assert!(parse_message(b"niente intestazioni", fallback()).is_none());
    }

    #[test]
    fn mbox_is_split_and_unreadable_sources_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("posta.mbox");
        std::fs::write(&p, "From a@b.it Tue Mar  5 10:00:00 2030\nFrom: A <a@b.it>\nSubject: Uno\nDate: Tue, 5 Mar 2030 10:00:00 +0000\n\nPrimo\n\nFrom c@d.it Tue Mar  5 11:00:00 2030\nFrom: C <c@d.it>\nSubject: Due\nDate: Tue, 5 Mar 2030 11:00:00 +0000\n\nSecondo\n").unwrap();
        let mails = read_source(&p, None).unwrap();
        assert_eq!(mails.iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(), ["Uno", "Due"]);
        assert_eq!(mails[0].snippet, "Primo");
        assert_eq!(mails[0].date.with_timezone(&Utc).year(), 2030);
        assert!(read_source(&dir.path().join("manca"), None).unwrap_err().contains("non esiste"));
    }

    #[test]
    fn maildir_folders_are_read_without_moving_anything() {
        let dir = tempfile::tempdir().unwrap();
        for sub in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(dir.path().join("INBOX").join(sub)).unwrap();
        }
        let inbox = dir.path().join("INBOX");
        std::fs::write(inbox.join("new/1.msg"), "From: A <a@b.it>\nSubject: Nuova\nDate: Tue, 5 Mar 2030 10:00:00 +0000\n\nTesto\n")
            .unwrap();
        std::fs::write(inbox.join("cur/2.msg:2,S"), "From: B <b@b.it>\nSubject: Letta\nDate: Tue, 5 Mar 2030 09:00:00 +0000\n\nTesto\n")
            .unwrap();
        std::fs::write(inbox.join("tmp/3.msg"), "From: C\nSubject: Parziale\n\nx").unwrap();
        // Pointing at the parent finds the Maildir inside it.
        for spec in [&inbox, &dir.path().to_path_buf()] {
            let mut subjects: Vec<String> = read_source(spec, None).unwrap().into_iter().map(|m| m.subject).collect();
            subjects.sort();
            assert_eq!(subjects, ["Letta", "Nuova"]);
        }
        assert!(inbox.join("new/1.msg").exists(), "the message is still in new/");
    }
}
