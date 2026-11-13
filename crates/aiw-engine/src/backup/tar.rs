//! A small tar reader and writer (POSIX ustar with pax `path` records), enough for the backup archive and strict by
//! construction: the reader only knows regular files and directories, and anything else (links, devices, FIFOs) is an
//! error. Names are validated by [`safe_path`] before they are used for anything.
//!
//! Written here instead of pulling the `tar` crate: the archive is ours (one writer, one reader), the reader has to refuse
//! links and odd names anyway, and the workspace already depends on `flate2` for the gzip layer.

use aiw_core::{Error, Result};
use std::io::{Read, Write};

const BLOCK: usize = 512;

fn invalid(msg: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("archivio di backup non valido: {msg}"))
}

/// A failure while reading the archive is a problem of the archive (bad gzip, cut short), not of this computer.
fn unreadable(e: std::io::Error) -> Error {
    invalid(format!("lettura dell'archivio: {e}"))
}

/// Checks a path as found in an archive and returns its components. Refused: empty paths, absolute paths (`/x`, `\x`,
/// `C:\x`), `..` and `.` components, empty components (`a//b`), backslashes, NUL and other control characters.
/// The archive is written with `/` only, so a backslash can only be an attempt at a Windows separator.
pub(super) fn safe_path(path: &str) -> Result<Vec<&str>> {
    if path.is_empty() {
        return Err(invalid("percorso vuoto"));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(invalid(format!("percorso assoluto non ammesso: {path}")));
    }
    if path.chars().any(|c| c.is_control() || c == '\\') {
        return Err(invalid(format!("percorso con caratteri non ammessi: {path:?}")));
    }
    let parts: Vec<&str> = path.split('/').collect();
    for p in &parts {
        match *p {
            "" => return Err(invalid(format!("percorso con componente vuoto: {path}"))),
            "." | ".." => return Err(invalid(format!("percorso che esce dalla cartella: {path}"))),
            _ => {}
        }
        // `C:` or `C:x`: a drive letter is an absolute path on Windows.
        let b = p.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            return Err(invalid(format!("percorso con lettera di unità non ammesso: {path}")));
        }
    }
    Ok(parts)
}

fn octal(field: &mut [u8], v: u64) {
    let digits = field.len() - 1;
    let s = format!("{v:0digits$o}");
    field[..digits].copy_from_slice(s.as_bytes());
    field[digits] = 0;
}

fn put(block: &mut [u8; BLOCK], at: usize, bytes: &[u8]) {
    block[at..at + bytes.len()].copy_from_slice(bytes);
}

fn seal(block: &mut [u8; BLOCK]) {
    block[148..156].fill(b' ');
    let sum: u64 = block.iter().map(|b| u64::from(*b)).sum();
    let s = format!("{sum:06o}");
    block[148..154].copy_from_slice(s.as_bytes());
    block[154] = 0;
    block[155] = b' ';
}

fn header(name: &[u8], size: u64, mode: u32, mtime: u64, kind: u8) -> [u8; BLOCK] {
    let mut b = [0u8; BLOCK];
    put(&mut b, 0, &name[..name.len().min(100)]);
    octal(&mut b[100..108], u64::from(mode));
    octal(&mut b[108..116], 0);
    octal(&mut b[116..124], 0);
    if size < 1 << 33 {
        octal(&mut b[124..136], size);
    } else {
        // GNU base-256 for 8 GiB and more.
        b[124] = 0x80;
        b[128..136].copy_from_slice(&size.to_be_bytes());
    }
    octal(&mut b[136..148], mtime);
    b[156] = kind;
    put(&mut b, 257, b"ustar\0");
    put(&mut b, 263, b"00");
    seal(&mut b);
    b
}

/// One `length key=value\n` record of a pax header; `length` counts the whole record, its own digits included.
fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let body = format!(" {key}={value}\n");
    let mut len = body.len() + 1;
    while format!("{len}{body}").len() != len {
        len = format!("{len}{body}").len();
    }
    format!("{len}{body}").into_bytes()
}

/// Test helper: one raw entry with whatever name and type, without the checks the writer makes, to forge hostile archives.
#[cfg(test)]
pub(super) fn forge(name: &str, kind: u8, data: &[u8], link: Option<&str>) -> Vec<u8> {
    let mut b = header(name.as_bytes(), data.len() as u64, 0o644, 0, kind);
    if let Some(l) = link {
        put(&mut b, 157, l.as_bytes());
        seal(&mut b);
    }
    let mut v = b.to_vec();
    v.extend_from_slice(data);
    v.extend(vec![0u8; (BLOCK - data.len() % BLOCK) % BLOCK]);
    v
}

pub(super) struct Writer<W: Write> {
    out: W,
}

impl<W: Write> Writer<W> {
    pub(super) fn new(out: W) -> Self {
        Self { out }
    }

    fn pad(&mut self, size: u64) -> std::io::Result<()> {
        let rest = (size % BLOCK as u64) as usize;
        if rest != 0 {
            self.out.write_all(&vec![0u8; BLOCK - rest])?;
        }
        Ok(())
    }

    /// Writes a regular file whose content is exactly `size` bytes of `body`.
    pub(super) fn file(&mut self, path: &str, size: u64, mode: u32, mtime: u64, body: &mut dyn Read) -> Result<()> {
        safe_path(path)?;
        let plain = path.len() <= 100 && path.is_ascii();
        if !plain {
            let rec = pax_record("path", path);
            self.out.write_all(&header(b"pax-header", rec.len() as u64, 0o644, mtime, b'x'))?;
            self.out.write_all(&rec)?;
            self.pad(rec.len() as u64)?;
        }
        // With a pax record the name of the entry itself is only a placeholder.
        let name: &[u8] = if plain { path.as_bytes() } else { b"long-name" };
        self.out.write_all(&header(name, size, mode, mtime, b'0'))?;
        let copied = std::io::copy(&mut body.take(size), &mut self.out)?;
        if copied != size {
            return Err(Error::Other(format!(
                "{path}: il file è cambiato durante la copia (letti {copied} byte invece di {size}): riprova"
            )));
        }
        self.pad(size)?;
        Ok(())
    }

    pub(super) fn bytes(&mut self, path: &str, mode: u32, mtime: u64, data: &[u8]) -> Result<()> {
        let mut r = data;
        self.file(path, data.len() as u64, mode, mtime, &mut r)
    }

    pub(super) fn finish(mut self) -> std::io::Result<W> {
        self.out.write_all(&[0u8; BLOCK * 2])?;
        Ok(self.out)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Kind {
    File,
    Dir,
}

#[derive(Debug)]
pub(super) struct Header {
    pub path: String,
    pub size: u64,
    pub kind: Kind,
}

pub(super) struct Reader<R: Read> {
    inp: R,
    /// Bytes of the body of the entry just read that are still to be consumed, and the padding after them.
    pending: u64,
    padding: u64,
}

fn parse_octal(field: &[u8]) -> Option<u64> {
    let s: String = field.iter().take_while(|b| **b != 0).map(|b| *b as char).collect();
    let t = s.trim();
    if t.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(t, 8).ok()
}

fn parse_size(field: &[u8]) -> Option<u64> {
    if field[0] & 0x80 != 0 {
        // GNU base-256: big endian after the marker bit; more than 63 bits is not a size.
        if field[0] != 0x80 || field[1..4].iter().any(|b| *b != 0) {
            return None;
        }
        let mut v = [0u8; 8];
        v.copy_from_slice(&field[4..12]);
        let n = u64::from_be_bytes(v);
        return (n < 1 << 62).then_some(n);
    }
    parse_octal(field)
}

fn field_str(field: &[u8]) -> Result<String> {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    String::from_utf8(field[..end].to_vec()).map_err(|_| invalid("nome di file che non è UTF-8"))
}

impl<R: Read> Reader<R> {
    pub(super) fn new(inp: R) -> Self {
        Self { inp, pending: 0, padding: 0 }
    }

    fn block(&mut self) -> Result<Option<[u8; BLOCK]>> {
        let mut b = [0u8; BLOCK];
        let mut got = 0;
        while got < BLOCK {
            let n = self.inp.read(&mut b[got..]).map_err(unreadable)?;
            if n == 0 {
                break;
            }
            got += n;
        }
        match got {
            0 => Ok(None),
            BLOCK => Ok(Some(b)),
            _ => Err(invalid("archivio troncato")),
        }
    }

    /// Reads what is left of the current body (and its padding) without keeping it.
    pub(super) fn skip_body(&mut self) -> Result<()> {
        let n = self.pending + self.padding;
        let copied = std::io::copy(&mut (&mut self.inp).take(n), &mut std::io::sink()).map_err(unreadable)?;
        if copied != n {
            return Err(invalid("archivio troncato"));
        }
        self.pending = 0;
        self.padding = 0;
        Ok(())
    }

    /// The body of the current file entry, for exactly its size. [`Reader::skip_body`] or [`Reader::next`] consume the rest.
    pub(super) fn body(&mut self) -> Body<'_, R> {
        Body { r: self }
    }

    fn read_exact_body(&mut self, n: u64) -> Result<Vec<u8>> {
        let mut v = Vec::new();
        (&mut self.inp).take(n).read_to_end(&mut v).map_err(unreadable)?;
        if v.len() as u64 != n {
            return Err(invalid("archivio troncato"));
        }
        self.pending -= n;
        Ok(v)
    }

    /// The next entry, or `None` at the end of the archive.
    pub(super) fn next(&mut self) -> Result<Option<Header>> {
        self.skip_body()?;
        let mut pax_path: Option<String> = None;
        loop {
            let Some(b) = self.block()? else { return Ok(None) };
            if b.iter().all(|x| *x == 0) {
                return Ok(None);
            }
            if &b[257..262] != b"ustar" {
                return Err(invalid("intestazione non riconosciuta (non è un archivio tar)"));
            }
            let stored = parse_octal(&b[148..156]).ok_or_else(|| invalid("somma di controllo illeggibile"))?;
            let mut sum = 0u64;
            for (i, x) in b.iter().enumerate() {
                sum += if (148..156).contains(&i) { 32 } else { u64::from(*x) };
            }
            if sum != stored {
                return Err(invalid("somma di controllo di un'intestazione errata"));
            }
            let size = parse_size(&b[124..136]).ok_or_else(|| invalid("dimensione illeggibile o troppo grande"))?;
            self.pending = size;
            self.padding = (BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64;
            let name = field_str(&b[0..100])?;
            let prefix = field_str(&b[345..500])?;
            match b[156] {
                b'x' => {
                    if size > 64 * 1024 {
                        return Err(invalid("intestazione estesa troppo grande"));
                    }
                    let rec = self.read_exact_body(size)?;
                    self.skip_body()?;
                    pax_path = parse_pax_path(&rec)?.or(pax_path);
                }
                b'g' => self.skip_body()?,
                kind @ (b'0' | 0 | b'5') => {
                    let path = match pax_path.take() {
                        Some(p) => p,
                        None if prefix.is_empty() => name,
                        None => format!("{prefix}/{name}"),
                    };
                    let path = path.trim_end_matches('/').to_string();
                    return Ok(Some(Header { path, size, kind: if kind == b'5' { Kind::Dir } else { Kind::File } }));
                }
                b'1' | b'2' => {
                    return Err(invalid(format!("«{}» è un collegamento (link): non ammesso", field_str(&b[0..100]).unwrap_or_default())))
                }
                other => {
                    return Err(invalid(format!(
                        "«{}» è di un tipo non ammesso (codice {other}): solo file e cartelle",
                        field_str(&b[0..100]).unwrap_or_default()
                    )))
                }
            }
        }
    }
}

fn parse_pax_path(mut rec: &[u8]) -> Result<Option<String>> {
    let mut path = None;
    while !rec.is_empty() {
        let sp = rec.iter().position(|b| *b == b' ').ok_or_else(|| invalid("intestazione estesa illeggibile"))?;
        let len: usize =
            std::str::from_utf8(&rec[..sp]).ok().and_then(|s| s.parse().ok()).ok_or_else(|| invalid("intestazione estesa illeggibile"))?;
        if len <= sp + 1 || len > rec.len() {
            return Err(invalid("intestazione estesa illeggibile"));
        }
        let kv = &rec[sp + 1..len];
        let kv = kv.strip_suffix(b"\n").unwrap_or(kv);
        if let Some(eq) = kv.iter().position(|b| *b == b'=') {
            if &kv[..eq] == b"path" {
                path = Some(String::from_utf8(kv[eq + 1..].to_vec()).map_err(|_| invalid("nome di file che non è UTF-8"))?);
            }
        }
        rec = &rec[len..];
    }
    Ok(path)
}

pub(super) struct Body<'a, R: Read> {
    r: &'a mut Reader<R>,
}

impl<R: Read> Read for Body<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.r.pending == 0 {
            return Ok(0);
        }
        let max = buf.len().min(usize::try_from(self.r.pending).unwrap_or(usize::MAX));
        let n = self.r.inp.read(&mut buf[..max])?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "archivio troncato"));
        }
        self.r.pending -= n as u64;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(entries: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
        let mut w = Writer::new(Vec::new());
        for (p, d) in entries {
            w.bytes(p, 0o644, 1_700_000_000, d).unwrap();
        }
        let bytes = w.finish().unwrap();
        assert_eq!(bytes.len() % BLOCK, 0);
        let mut r = Reader::new(&bytes[..]);
        let mut out = vec![];
        while let Some(h) = r.next().unwrap() {
            assert_eq!(h.kind, Kind::File);
            let mut v = vec![];
            r.body().read_to_end(&mut v).unwrap();
            assert_eq!(v.len() as u64, h.size);
            out.push((h.path, v));
        }
        out
    }

    #[test]
    fn writes_and_reads_files_of_every_size_and_name() {
        let long = format!("attachments/{}/{}-{}.pdf", "s".repeat(40), "a".repeat(40), "n".repeat(80));
        let entries: Vec<(&str, Vec<u8>)> = vec![
            ("empty.txt", vec![]),
            ("one.bin", vec![7]),
            ("block.bin", vec![1; 512]),
            ("over.bin", vec![2; 513]),
            ("skills/it/è.md", "perché".as_bytes().to_vec()),
            (long.as_str(), vec![3; 1000]),
        ];
        let refs: Vec<(&str, &[u8])> = entries.iter().map(|(p, d)| (*p, d.as_slice())).collect();
        let got = roundtrip(&refs);
        assert_eq!(got.len(), entries.len());
        for ((p, d), (gp, gd)) in entries.iter().zip(&got) {
            assert_eq!((p, d), (&gp.as_str(), gd));
        }
    }

    #[test]
    fn the_system_tar_can_read_what_is_written_here() {
        // Only where a `tar` binary exists (Linux, macOS, Windows 10+): the point is that the format is plain tar.
        let mut w = Writer::new(Vec::new());
        w.bytes("a/b.txt", 0o644, 0, b"ciao").unwrap();
        w.bytes(&format!("long/{}.txt", "x".repeat(150)), 0o644, 0, b"lungo").unwrap();
        let bytes = w.finish().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.tar");
        std::fs::write(&file, bytes).unwrap();
        let Ok(out) = std::process::Command::new("tar").arg("-tf").arg(&file).output() else { return };
        if out.status.success() {
            let list = String::from_utf8_lossy(&out.stdout).replace('\\', "/");
            assert!(list.contains("a/b.txt") && list.contains(&"x".repeat(150)), "{list}");
        }
    }

    fn raw(name: &str, kind: u8, size: u64) -> Vec<u8> {
        let mut b = header(name.as_bytes(), size, 0o644, 0, kind);
        if kind == b'2' {
            put(&mut b, 157, b"/etc/passwd");
            seal(&mut b);
        }
        let mut v = b.to_vec();
        v.extend(vec![0u8; BLOCK * 2]);
        v
    }

    #[test]
    fn links_and_special_files_are_refused() {
        for (kind, word) in [(b'2', "collegamento"), (b'1', "collegamento"), (b'3', "tipo non ammesso"), (b'6', "tipo non ammesso")] {
            let bytes = raw("x", kind, 0);
            let err = Reader::new(&bytes[..]).next().expect_err("must be refused").to_string();
            assert!(err.contains(word), "{err}");
        }
    }

    #[test]
    fn damaged_archives_are_refused() {
        let mut w = Writer::new(Vec::new());
        w.bytes("a.txt", 0o644, 0, b"ciao").unwrap();
        let good = w.finish().unwrap();
        // Wrong checksum.
        let mut bad = good.clone();
        bad[0] ^= 1;
        assert!(Reader::new(&bad[..]).next().unwrap_err().to_string().contains("somma di controllo"));
        // Not a tar.
        assert!(Reader::new(&[7u8; 600][..]).next().is_err());
        // Truncated body.
        let cut = &good[..BLOCK + 2];
        let mut r = Reader::new(cut);
        r.next().unwrap().unwrap();
        let mut v = vec![];
        assert!(r.body().read_to_end(&mut v).is_err());
        // Truncated header.
        assert!(Reader::new(&good[..100]).next().is_err());
    }

    #[test]
    fn unsafe_paths_are_refused() {
        for p in
            ["", "/etc/passwd", "\\windows\\x", "../x", "a/../../x", "a/./b", "a//b", "a\\b", "C:\\x", "C:x", "d:/x", "a/b\0c", "a/\nb"]
        {
            assert!(safe_path(p).is_err(), "{p:?} must be refused");
        }
        for p in ["a", "a/b.txt", "skills/è/x y.md", ".shell-network-migrated", "a.b/c..d"] {
            assert!(safe_path(p).is_ok(), "{p:?} must be accepted");
        }
        assert_eq!(safe_path("a/b").unwrap(), ["a", "b"]);
    }
}
