//! Atomic file writes. The data goes to a hidden temporary file in the same folder as the destination,
//! is flushed to disk and then renamed over it: a crash or an interruption leaves either the old
//! content or the new one, never a truncated file, and a failure leaves the destination untouched.
//!
//! Trade-offs of replacing a file instead of rewriting it in place: other hard links to the old file keep
//! the old content, and only the permission bits (not the owner, ACLs or extended attributes) carry over.

use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Infix of the temporary names (`.<name>.aiw-tmp-<random>`): dot-files, so searches skip them.
const TMP_MARKER: &str = ".aiw-tmp-";
/// Longest part of the destination name copied into the temporary name, so that a name close to the
/// limit of the file system (255 bytes) still has room for the prefix and the suffix.
const MAX_NAME_BYTES: usize = 100;
/// Same order of magnitude as the kernel limit on symbolic links followed in one lookup.
const MAX_LINK_HOPS: usize = 40;

/// Replaces the content of `path` (creating the file if needed) all at once.
///
/// If `path` is a symbolic link the file it points to is replaced and the link stays: `ToolContext::resolve`
/// has already checked that the target is allowed, and a plain write goes through links too.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = final_target(path)?;
    let existing = check_writable(&target)?;
    let mut tmp = TempFile::create(&target, existing.as_ref())?;
    tmp.file().write_all(bytes)?;
    tmp.commit(&target)
}

/// Copies the file `from` to `to` all at once and returns the bytes copied. An existing destination is
/// replaced and keeps its permission bits; a new one gets those of the source (like `std::fs::copy`).
pub fn copy_atomic(from: &Path, to: &Path) -> io::Result<u64> {
    let mut src = File::open(from)?;
    let src_meta = src.metadata()?;
    if !src_meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the source path is not an existing regular file"));
    }
    let target = final_target(to)?;
    let existing = check_writable(&target)?;
    let mut tmp = TempFile::create(&target, Some(existing.as_ref().unwrap_or(&src_meta)))?;
    let copied = io::copy(&mut src, tmp.file())?;
    tmp.commit(&target)?;
    Ok(copied)
}

/// [`write_atomic`] off the async worker threads (the flush to disk can take a while).
pub async fn write_atomic_async(path: &Path, bytes: Vec<u8>) -> io::Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || write_atomic(&path, &bytes)).await.map_err(io::Error::other)?
}

/// [`copy_atomic`] off the async worker threads.
pub async fn copy_atomic_async(from: &Path, to: &Path) -> io::Result<u64> {
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    tokio::task::spawn_blocking(move || copy_atomic(&from, &to)).await.map_err(io::Error::other)?
}

/// Follows a chain of symbolic links at the end of `path` (a dangling one ends at the file it would
/// create). Relative targets are joined, not normalized: the system resolves `..` after the links.
fn final_target(path: &Path) -> io::Result<PathBuf> {
    let mut cur = path.to_path_buf();
    for _ in 0..MAX_LINK_HOPS {
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => {
                let link = std::fs::read_link(&cur)?;
                cur = if link.is_absolute() { link } else { cur.parent().map(|d| d.join(&link)).unwrap_or(link) };
            }
            _ => return Ok(cur),
        }
    }
    Err(io::Error::other("too many levels of symbolic links"))
}

/// Gives back the metadata of an existing destination (`None` if there is none). Replacing a file needs
/// no permission on the file itself, so check what a write in place would have required: a read-only
/// file or a folder in the way must fail, not be silently replaced.
fn check_writable(target: &Path) -> io::Result<Option<Metadata>> {
    match std::fs::metadata(target) {
        Ok(meta) => {
            OpenOptions::new().write(true).open(target)?;
            Ok(Some(meta))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// A new temporary file next to the destination; removed on drop unless [`TempFile::commit`] went through.
struct TempFile {
    path: PathBuf,
    file: Option<File>,
    kept: bool,
}

impl TempFile {
    /// `like`: the metadata whose permission bits the file gets (Unix) *before* anything is written to it: a `0600` file's new
    /// content is never readable by others, not even for a moment, and neither is a temporary left behind by a crash.
    fn create(target: &Path, like: Option<&Metadata>) -> io::Result<Self> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "percorso senza nome di file");
        let dir = target.parent().ok_or_else(invalid)?;
        let name = target.file_name().ok_or_else(invalid)?.to_string_lossy();
        let name = truncate_bytes(&name, MAX_NAME_BYTES);
        for _ in 0..16 {
            let random = uuid::Uuid::new_v4().simple().to_string();
            let path = dir.join(format!(".{name}{TMP_MARKER}{}", &random[..16]));
            // `create_new`: never open (and so never clobber) a file that is already there.
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    let mut tmp = Self { path, file: Some(file), kept: false };
                    #[cfg(unix)]
                    if let Some(meta) = like {
                        // (On failure `tmp` is dropped and removes the file.)
                        tmp.file().set_permissions(meta.permissions())?;
                    }
                    #[cfg(not(unix))]
                    let _ = like;
                    return Ok(tmp);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "nome temporaneo già in uso"))
    }

    fn file(&mut self) -> &mut File {
        self.file.as_mut().expect("the file is open until commit")
    }

    /// Flushes the data to disk, closes the file and renames it over `target` (which `std::fs::rename` replaces on every platform).
    fn commit(mut self, target: &Path) -> io::Result<()> {
        let file = self.file.take().expect("the file is open until commit");
        file.sync_all()?;
        // Closed before the rename and before any cleanup: an open handle blocks both on Windows.
        drop(file);
        rename_over(&self.path, target)?;
        self.kept = true;
        Ok(())
    }
}

/// `rename` over an existing file. On Windows it is refused while another program has the destination open without allowing
/// deletion (an editor, an antivirus scan, a search indexer), usually for a moment: a few short, bounded retries, then the error.
/// (Not tried on Windows: written from the documented behaviour of `MoveFileEx`.)
fn rename_over(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        let mut last = None;
        for attempt in 0..6 {
            match std::fs::rename(from, to) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied && attempt < 5 => {
                    last = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(25 << attempt.min(3)));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "rename")))
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(from, to)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        self.file.take();
        if !self.kept {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// The longest prefix of `s` that fits in `max` bytes without splitting a character.
fn truncate_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// Names in `dir` that are leftovers of a temporary file.
    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(TMP_MARKER))
            .collect()
    }

    #[test]
    fn creates_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nuovo.txt");
        write_atomic(&path, b"ciao").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"ciao");
        write_atomic(&dir.path().join("vuoto.txt"), b"").unwrap();
        assert_eq!(std::fs::read(dir.path().join("vuoto.txt")).unwrap(), b"");
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn overwrite_replaces_the_whole_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nota.md");
        std::fs::write(&path, "un contenuto vecchio molto più lungo del nuovo").unwrap();
        write_atomic(&path, b"nuovo").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "nuovo", "no tail of the old content");
        assert!(leftovers(dir.path()).is_empty(), "{:?}", leftovers(dir.path()));
        // And the other way round.
        write_atomic(&path, "più lungo di prima, con accenti: àèìòù".as_bytes()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "più lungo di prima, con accenti: àèìòù");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "only the destination is in the folder");
    }

    #[test]
    fn a_folder_in_the_way_fails_and_leaves_everything_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("cartella");
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(dest.join("dentro.txt"), "da conservare").unwrap();
        assert!(write_atomic(&dest, b"x").is_err());
        assert!(dest.is_dir());
        assert_eq!(std::fs::read_to_string(dest.join("dentro.txt")).unwrap(), "da conservare");
        assert!(leftovers(dir.path()).is_empty(), "{:?}", leftovers(dir.path()));
        // Same for the copy.
        let src = dir.path().join("a.txt");
        std::fs::write(&src, "a").unwrap();
        assert!(copy_atomic(&src, &dest).is_err());
        assert!(dest.is_dir() && leftovers(dir.path()).is_empty());
    }

    #[test]
    fn a_failure_after_the_temp_file_is_created_removes_it() {
        // Drives `TempFile` directly: the rename onto a folder fails after the data was written.
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("occupato");
        std::fs::create_dir(&blocked).unwrap();
        let mut tmp = TempFile::create(&blocked, None).unwrap();
        tmp.file().write_all(b"dati").unwrap();
        assert!(tmp.commit(&blocked).is_err());
        assert!(blocked.is_dir());
        assert!(leftovers(dir.path()).is_empty(), "{:?}", leftovers(dir.path()));
    }

    /// The temporary of a private file is private from the moment it exists, before any data is in it.
    #[cfg(unix)]
    #[test]
    fn the_temporary_of_a_private_file_is_private_before_any_data_is_written() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let secret = dir.path().join("segreto.txt");
        std::fs::write(&secret, "vecchio").unwrap();
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
        let like = std::fs::metadata(&secret).unwrap();
        let tmp = TempFile::create(&secret, Some(&like)).unwrap();
        assert_eq!(std::fs::metadata(&tmp.path).unwrap().permissions().mode() & 0o777, 0o600);
        drop(tmp);
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn a_missing_folder_or_name_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write_atomic(&dir.path().join("manca/file.txt"), b"x").is_err());
        assert!(!dir.path().join("manca").exists());
        let missing = dir.path().join("non-c-e.txt");
        assert!(copy_atomic(&missing, &dir.path().join("copia.txt")).is_err());
        assert!(!dir.path().join("copia.txt").exists() && leftovers(dir.path()).is_empty());
    }

    #[test]
    fn copy_makes_an_identical_file_and_keeps_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("origine.bin");
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&src, &data).unwrap();
        let dest = dir.path().join("copia.bin");
        assert_eq!(copy_atomic(&src, &dest).unwrap(), data.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert_eq!(std::fs::read(&src).unwrap(), data, "the source stays");
        // An existing destination is replaced entirely.
        std::fs::write(&dest, vec![7u8; 300_000]).unwrap();
        assert_eq!(copy_atomic(&src, &dest).unwrap(), data.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert!(leftovers(dir.path()).is_empty(), "{:?}", leftovers(dir.path()));
    }

    #[test]
    fn long_names_still_fit_in_the_temporary_name() {
        // Near the 255-byte limit: the temporary name must not push it over.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("{}.txt", "è".repeat(120)));
        write_atomic(&path, b"lungo").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"lungo");
        assert!(leftovers(dir.path()).is_empty());
        assert_eq!(truncate_bytes("aèb", 2), "a", "never splits a character");
        assert_eq!(truncate_bytes("abc", 10), "abc");
    }

    #[cfg(unix)]
    #[test]
    fn permission_bits_are_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
        let script = dir.path().join("script.sh");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o750)).unwrap();
        write_atomic(&script, b"#!/bin/sh\necho ok\n").unwrap();
        assert_eq!(mode(&script), 0o750);
        assert_eq!(std::fs::read_to_string(&script).unwrap(), "#!/bin/sh\necho ok\n");

        // A copy over an existing file keeps the mode of the destination; a new copy gets that of the source.
        let src = dir.path().join("origine.sh");
        std::fs::write(&src, "x").unwrap();
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o700)).unwrap();
        copy_atomic(&src, &script).unwrap();
        assert_eq!(mode(&script), 0o750);
        let copy = dir.path().join("copia.sh");
        copy_atomic(&src, &copy).unwrap();
        assert_eq!(mode(&copy), 0o700);

        // A new file gets the default mode, like a plain write: readable and writable by the owner.
        let fresh = dir.path().join("nuovo.txt");
        write_atomic(&fresh, b"x").unwrap();
        assert_eq!(mode(&fresh) & 0o600, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_file_is_not_silently_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sola-lettura.txt");
        std::fs::write(&path, "originale").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        if OpenOptions::new().write(true).open(&path).is_ok() {
            return; // root ignores permissions: nothing to check
        }
        let err = write_atomic(&path, b"nuovo").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        let other = dir.path().join("altro.txt");
        std::fs::write(&other, "altro").unwrap();
        assert_eq!(copy_atomic(&other, &path).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "originale");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777, 0o444);
        assert!(leftovers(dir.path()).is_empty(), "{:?}", leftovers(dir.path()));
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_folder_fails_and_leaves_the_old_content() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("fissa");
        std::fs::create_dir(&sub).unwrap();
        let path = sub.join("dati.txt");
        std::fs::write(&path, "vecchio").unwrap();
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o555)).unwrap();
        // root can create files anywhere: then there is no read-only folder to test with.
        let root = std::fs::write(sub.join("sonda"), "x").is_ok();
        let other = dir.path().join("altro.txt");
        std::fs::write(&other, "altro").unwrap();
        let result = write_atomic(&path, b"nuovo");
        let copy_result = copy_atomic(&other, &path);
        let names = leftovers(&sub);
        // Back to writable before asserting, so that the folder can be removed even if something fails.
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();
        if root {
            return;
        }
        assert!(result.is_err());
        assert!(copy_result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "vecchio");
        assert!(names.is_empty(), "{names:?}");
        assert_eq!(std::fs::read_dir(&sub).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_file_stays_a_link_and_its_target_is_replaced() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("reale.txt");
        std::fs::write(&real, "vecchio").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&real, dir.path().join("alias_assoluto")).unwrap();
        symlink("reale.txt", dir.path().join("alias_relativo")).unwrap();
        symlink("alias_relativo", dir.path().join("alias_di_alias")).unwrap();
        for (i, name) in ["alias_assoluto", "alias_relativo", "alias_di_alias"].into_iter().enumerate() {
            let link = dir.path().join(name);
            write_atomic(&link, format!("giro {i}").as_bytes()).unwrap();
            assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "{name} is still a link");
            assert_eq!(std::fs::read_to_string(&real).unwrap(), format!("giro {i}"));
            assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o7777, 0o640);
        }
        // A dangling link: a plain write creates the file it points to, and so does this.
        symlink("nato.txt", dir.path().join("pendente")).unwrap();
        write_atomic(&dir.path().join("pendente"), b"creato").unwrap();
        assert!(std::fs::symlink_metadata(dir.path().join("pendente")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(dir.path().join("nato.txt")).unwrap(), "creato");
        // A loop of links is an error, not an endless chase.
        symlink("a", dir.path().join("b")).unwrap();
        symlink("b", dir.path().join("a")).unwrap();
        assert!(write_atomic(&dir.path().join("a"), b"x").is_err());
        assert!(leftovers(dir.path()).is_empty(), "{:?}", leftovers(dir.path()));
    }

    /// The destination is, at every moment, either the whole old content or the whole new one.
    #[test]
    fn readers_never_see_a_torn_file() {
        const SIZE: usize = 4 << 20;
        const ROUNDS: usize = 40;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grande.bin");
        let (a, b) = (vec![b'A'; SIZE], vec![b'B'; SIZE]);
        write_atomic(&path, &a).unwrap();

        let done = Arc::new(AtomicBool::new(false));
        let reader = {
            let (path, done) = (path.clone(), done.clone());
            std::thread::spawn(move || {
                let mut reads = 0u32;
                loop {
                    // Read the flag first: one last read happens after the writer is done.
                    let finished = done.load(Ordering::Acquire);
                    match std::fs::read(&path) {
                        Ok(buf) => {
                            assert_eq!(buf.len(), SIZE, "truncated content ({} bytes)", buf.len());
                            let first = buf[0];
                            assert!(first == b'A' || first == b'B', "unexpected content");
                            assert!(buf.iter().all(|&c| c == first), "mixed old and new content");
                            reads += 1;
                        }
                        // Windows may deny a read for an instant while a file is being replaced.
                        Err(e) if cfg!(windows) => eprintln!("read skipped: {e}"),
                        Err(e) => panic!("the destination must always be readable: {e}"),
                    }
                    if finished {
                        return reads;
                    }
                }
            })
        };

        for i in 0..ROUNDS {
            let data = if i % 2 == 0 { &b } else { &a };
            // The same on Windows, where replacing a file somebody is reading can fail for an instant.
            let mut attempt = 0;
            while let Err(e) = write_atomic(&path, data) {
                attempt += 1;
                assert!(cfg!(windows) && attempt < 50, "write failed: {e}");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        done.store(true, Ordering::Release);
        let reads = reader.join().unwrap();
        assert!(reads > 0);
        assert_eq!(std::fs::read(&path).unwrap(), a, "the last round wrote A");
        assert!(leftovers(dir.path()).is_empty(), "{:?}", leftovers(dir.path()));
    }

    #[tokio::test]
    async fn the_async_versions_do_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        write_atomic_async(&path, b"uno".to_vec()).await.unwrap();
        write_atomic_async(&path, b"due".to_vec()).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"due");
        let copy = dir.path().join("b.txt");
        assert_eq!(copy_atomic_async(&path, &copy).await.unwrap(), 3);
        assert_eq!(std::fs::read(&copy).unwrap(), b"due");
        assert!(copy_atomic_async(&dir.path().join("manca.txt"), &copy).await.is_err());
        assert_eq!(std::fs::read(&copy).unwrap(), b"due");
        assert!(leftovers(dir.path()).is_empty());
    }
}
