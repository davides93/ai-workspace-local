//! Files the engine reads for itself, outside the sandbox, and puts in a prompt: `AGENTS.md` of the shared workspace and the
//! `SKILL.md` of the skills a workspace brings.
//!
//! The sandbox protects the commands and the file tools of an agent, not the engine. The engine reads these files with its own
//! permissions, so a link made by the agent in the workspace (`ln -s <data>/token AGENTS.md`, or one that a cloned repository
//! ships: `AGENTS.md -> ~/.ssh/id_ed25519`) would carry any file the engine can read into the system prompt of the next message.
//! Making a link needs no access to its target, so the sandbox does not stop it.
//!
//! The rule is the one the file tools already apply to a path (`ToolContext::resolve`), made safe against a swap:
//!
//! * the file is **opened first**, and what was opened must be a regular file (not a folder, not a pipe: opening a pipe would block
//!   the run for ever, so the file is opened without waiting);
//! * the path, with every link resolved (`canonicalize`), must stay **inside the root** (the workspace, or the folder the skills
//!   were found in) and out of the **hidden** places (the data folder of the app, `sandbox.deny_read`), unless the root itself
//!   lives in one of them;
//! * the open file must be the file that path names *now*, and nothing else: the resolved path is walked from the root one folder
//!   at a time, each opened without following a link and kept open, and its last name must be a regular file (not a link) with
//!   the same device and inode as the open file and **a single name** (link count 1). A link swapped in for the file, for a folder
//!   of the path or for the file's last name between the open and the check leads somewhere else, or is a link, and is refused;
//! * a file with **several names** (a hard link) is refused, even when all its names are in the workspace: `ln <data>/token
//!   AGENTS.md` makes a second name for the token, and the other names of a file cannot be told apart without a race. Use a
//!   symbolic link (`AGENTS.md -> CLAUDE.md`); git does not make hard links.
//!
//! A symbolic link that stays inside the root is read: that is a common and legitimate use.
//!
//! Only Unix has the walk by descriptors, the device/inode comparison and the link count; on Windows the path is checked after the
//! open (resolved and compared with the root), the open file cannot be compared with it and a hard link is not told from a file.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// `path` with every link resolved, if it stays inside `root` (also resolved) and out of the `hidden` places. The folder
/// of a skill, or the folder the skills are in, go through here: no file is opened.
pub fn resolve_within(root: &Path, hidden: &[PathBuf], path: &Path) -> Option<PathBuf> {
    resolve(root, hidden, path).map(|(_, canon)| canon)
}

/// The root and `path`, both resolved, if `path` stays inside the root and out of the hidden places.
fn resolve(root: &Path, hidden: &[PathBuf], path: &Path) -> Option<(PathBuf, PathBuf)> {
    let root = std::fs::canonicalize(root).ok()?;
    let canon = std::fs::canonicalize(path).ok()?;
    if !canon.starts_with(&root) || is_hidden(&canon, &root, hidden) {
        return None;
    }
    Some((root, canon))
}

/// True when `canon` lies in a hidden place. The root itself living in one (an isolated session in `<data>/worktrees`) makes
/// what is inside the root readable, as it does for the file tools (`ToolContext::is_hidden`).
fn is_hidden(canon: &Path, root: &Path, hidden: &[PathBuf]) -> bool {
    hidden.iter().any(|h| {
        let resolved = std::fs::canonicalize(h).ok();
        std::iter::once(h).chain(resolved.as_ref()).any(|h| canon.starts_with(h) && !root.starts_with(h))
    })
}

/// Opens `path` for reading only if it is a regular file the rule of the module lets the engine read. `None` for anything
/// else: a link to a file outside `root` or in a hidden place, a file with more than one name, a folder, a pipe, a file that
/// is not there.
pub fn open_regular_within(root: &Path, hidden: &[PathBuf], path: &Path) -> Option<File> {
    let file = open_without_waiting(path)?;
    confirm(&file, root, hidden, path).then_some(file)
}

/// Is the open `file` what `path` leads to now, inside `root`? Split from the open so that a swap between the two is a test.
pub(crate) fn confirm(file: &File, root: &Path, hidden: &[PathBuf], path: &Path) -> bool {
    let Ok(opened) = file.metadata() else { return false };
    if !opened.is_file() {
        return false;
    }
    let Some((root, canon)) = resolve(root, hidden, path) else { return false };
    confirm_named(&opened, &root, &canon)
}

/// `canon` is the resolved path of a file inside `root` (both resolved): is the open file, described by `opened`, exactly the
/// one name it has there? Split from [`confirm`] so that what happens after the path was resolved is a test.
pub(crate) fn confirm_named(opened: &std::fs::Metadata, root: &Path, canon: &Path) -> bool {
    #[cfg(unix)]
    {
        named_here(opened, root, canon).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = (opened, root, canon);
        true
    }
}

/// Walks `canon` from `root` by descriptors: each folder is opened from the one before without following a link, and the last
/// name is looked at without following it (`fstatat`). The folders stay open, so none can be swapped for a link behind the walk.
/// True only for a regular file with the device and inode of `opened` and a link count of 1.
#[cfg(unix)]
#[allow(clippy::unnecessary_cast)] // the width of `st_dev`, `st_ino`, `st_nlink` and `st_mode` depends on the system
fn named_here(opened: &std::fs::Metadata, root: &Path, canon: &Path) -> Option<bool> {
    use std::ffi::{CString, OsStr};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Component;

    let mut names: Vec<&OsStr> = Vec::new();
    for c in canon.strip_prefix(root).ok()?.components() {
        match c {
            Component::Normal(n) => names.push(n),
            _ => return None,
        }
    }
    let (last, folders) = names.split_last()?;
    let cstr = |n: &OsStr| CString::new(n.as_bytes()).ok();
    let root_path = cstr(root.as_os_str())?;
    let last = cstr(last)?;
    // SAFETY: plain system calls on NUL-terminated names made above; the descriptors are owned by `OwnedFd`s and closed on drop.
    unsafe {
        let fd = libc::open(root_path.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC);
        if fd < 0 {
            return None;
        }
        let mut dir = OwnedFd::from_raw_fd(fd);
        for folder in folders {
            let name = cstr(folder)?;
            let fd = libc::openat(dir.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
            if fd < 0 {
                return None; // a link, or not a folder: the path is not what it was when it was resolved
            }
            dir = OwnedFd::from_raw_fd(fd);
        }
        let mut st: libc::stat = std::mem::zeroed();
        if libc::fstatat(dir.as_raw_fd(), last.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) != 0 {
            return None;
        }
        let regular = (st.st_mode as u32 & libc::S_IFMT as u32) == libc::S_IFREG as u32;
        Some(regular && st.st_dev as u64 == opened.dev() && st.st_ino as u64 == opened.ino() && st.st_nlink as u64 == 1)
    }
}

/// Opens the file at the end of `path` (links followed) without waiting: opening a pipe (`mkfifo AGENTS.md`) would otherwise
/// block until somebody writes to it. A regular file reads the same.
fn open_without_waiting(path: &Path) -> Option<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path).ok()
    }
    #[cfg(not(unix))]
    {
        File::open(path).ok()
    }
}

/// Reads `path` as text (at most `max_chars` characters) under the rule of the module. `None` when the file is refused, is not
/// there or is not UTF-8. A file longer than the limit is cut, never read whole.
pub fn read_text_within(root: &Path, hidden: &[PathBuf], path: &Path, max_chars: usize) -> Option<String> {
    let file = open_regular_within(root, hidden, path)?;
    // A character takes at most four bytes: a longer file is cut, and a character the cut splits is dropped.
    let cap = (max_chars as u64).saturating_mul(4);
    let mut bytes = Vec::new();
    (&file).take(cap).read_to_end(&mut bytes).ok()?;
    let cut = bytes.len() as u64 >= cap;
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) if cut && e.utf8_error().error_len().is_none() => {
            let valid = e.utf8_error().valid_up_to();
            let mut bytes = e.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).ok()?
        }
        Err(_) => return None,
    };
    Some(text.chars().take(max_chars).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A workspace, a folder of "somebody else" next to it and a secret in it.
    struct Fixture {
        _dir: tempfile::TempDir,
        ws: PathBuf,
        other: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        let other = dir.path().join("dati");
        fs::create_dir_all(&ws).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("token"), "TOKEN-DEL-PROPRIETARIO").unwrap();
        Fixture { ws, other, _dir: dir }
    }

    fn read(f: &Fixture, name: &str) -> Option<String> {
        read_text_within(&f.ws, &[], &f.ws.join(name), 20_000)
    }

    #[test]
    fn a_plain_file_is_read_and_cut_to_the_limit_on_a_character() {
        let f = fixture();
        fs::write(f.ws.join("AGENTS.md"), "Parla italiano.").unwrap();
        assert_eq!(read(&f, "AGENTS.md").as_deref(), Some("Parla italiano."));
        assert_eq!(read(&f, "manca.md"), None);
        // 10 characters of 3 bytes each, the limit is 4: the file is cut to 4 characters, not refused.
        fs::write(f.ws.join("lungo.md"), "€".repeat(10)).unwrap();
        assert_eq!(read_text_within(&f.ws, &[], &f.ws.join("lungo.md"), 4).as_deref(), Some("€€€€"));
        // 20 four-byte characters, the limit is 3: 12 bytes are read, the cut falls on a boundary.
        fs::write(f.ws.join("emoji.md"), "😀".repeat(20)).unwrap();
        assert_eq!(read_text_within(&f.ws, &[], &f.ws.join("emoji.md"), 3).as_deref(), Some("😀😀😀"));
        // A cut that splits a character is not an error: 3 characters of 3 bytes, limit 2, the read stops after 8 bytes.
        fs::write(f.ws.join("tagli.md"), format!("a{}", "€".repeat(40))).unwrap();
        assert_eq!(read_text_within(&f.ws, &[], &f.ws.join("tagli.md"), 2).as_deref(), Some("a€"));
        // What is not text stays refused, as it always was.
        fs::write(f.ws.join("binario.md"), [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(read(&f, "binario.md"), None);
    }

    #[test]
    fn a_folder_called_like_the_file_is_not_read() {
        let f = fixture();
        fs::create_dir(f.ws.join("AGENTS.md")).unwrap();
        fs::write(f.ws.join("AGENTS.md").join("dentro.md"), "x").unwrap();
        assert_eq!(read(&f, "AGENTS.md"), None);
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::symlink;

        #[test]
        fn a_link_to_a_file_outside_the_root_is_refused() {
            let f = fixture();
            symlink(f.other.join("token"), f.ws.join("AGENTS.md")).unwrap();
            assert_eq!(read(&f, "AGENTS.md"), None);
            // A relative link that climbs out, and a link to a link, are the same.
            symlink("../dati/token", f.ws.join("relativo.md")).unwrap();
            assert_eq!(read(&f, "relativo.md"), None);
            symlink(f.ws.join("AGENTS.md"), f.ws.join("doppio.md")).unwrap();
            assert_eq!(read(&f, "doppio.md"), None);
            // A link to a folder outside the root does not give its files either.
            symlink(&f.other, f.ws.join("cartella")).unwrap();
            assert_eq!(read(&f, "cartella/token"), None);
        }

        #[test]
        fn a_link_to_a_file_inside_the_root_is_read() {
            let f = fixture();
            fs::write(f.ws.join("CLAUDE.md"), "Regole di Claude").unwrap();
            symlink("CLAUDE.md", f.ws.join("AGENTS.md")).unwrap();
            assert_eq!(read(&f, "AGENTS.md").as_deref(), Some("Regole di Claude"));
            fs::create_dir_all(f.ws.join("docs")).unwrap();
            fs::write(f.ws.join("docs/regole.md"), "Nelle docs").unwrap();
            symlink("docs/regole.md", f.ws.join("ALTRO.md")).unwrap();
            assert_eq!(read(&f, "ALTRO.md").as_deref(), Some("Nelle docs"));
            // The root may itself be reached through a link (a temporary folder is `/var` → `/private/var` on a Mac).
            let via = f.ws.parent().unwrap().join("via");
            symlink(&f.ws, &via).unwrap();
            assert_eq!(read_text_within(&via, &[], &via.join("AGENTS.md"), 100).as_deref(), Some("Regole di Claude"));
        }

        #[test]
        fn a_link_that_climbs_out_and_comes_back_in_is_read_for_where_it_lands() {
            let f = fixture();
            fs::write(f.ws.join("CLAUDE.md"), "dentro").unwrap();
            symlink("../ws/CLAUDE.md", f.ws.join("AGENTS.md")).unwrap();
            assert_eq!(read(&f, "AGENTS.md").as_deref(), Some("dentro"));
        }

        #[test]
        fn a_file_with_more_than_one_name_is_refused_even_when_all_its_names_are_in_the_workspace() {
            let f = fixture();
            fs::hard_link(f.other.join("token"), f.ws.join("AGENTS.md")).unwrap();
            assert_eq!(read(&f, "AGENTS.md"), None, "the token has another name, in the folder of somebody else");
            fs::hard_link(f.other.join("token"), f.ws.join("COPIA.md")).unwrap();
            assert_eq!(read(&f, "AGENTS.md"), None);
            assert_eq!(read(&f, "COPIA.md"), None);

            // A file of the workspace with two names is refused too (accepted limit: its names cannot be told apart without a race).
            fs::write(f.ws.join("CLAUDE.md"), "Regole").unwrap();
            assert_eq!(read(&f, "CLAUDE.md").as_deref(), Some("Regole"));
            fs::hard_link(f.ws.join("CLAUDE.md"), f.ws.join("GEMINI.md")).unwrap();
            assert_eq!(read(&f, "GEMINI.md"), None);
            assert_eq!(read(&f, "CLAUDE.md"), None);
            // A symbolic link to it is the same file; once the second name is gone it is read again.
            symlink("CLAUDE.md", f.ws.join("ALTRO.md")).unwrap();
            assert_eq!(read(&f, "ALTRO.md"), None);
            fs::remove_file(f.ws.join("GEMINI.md")).unwrap();
            assert_eq!(read(&f, "ALTRO.md").as_deref(), Some("Regole"));
        }

        #[test]
        fn a_file_swapped_for_another_between_the_open_and_the_check_is_refused() {
            let f = fixture();
            fs::write(f.ws.join("uno.md"), "uno").unwrap();
            fs::write(f.ws.join("due.md"), "due").unwrap();
            // The file that was opened is `uno.md`; by the time the path is checked it leads to `due.md`.
            let opened = open_without_waiting(&f.ws.join("uno.md")).unwrap();
            assert!(confirm(&opened, &f.ws, &[], &f.ws.join("uno.md")));
            assert!(!confirm(&opened, &f.ws, &[], &f.ws.join("due.md")), "same folder, another inode");
            // The same with the real attack: the token was opened, the path was then made a link to a file of the workspace.
            let token = open_without_waiting(&f.other.join("token")).unwrap();
            symlink(f.ws.join("uno.md"), f.ws.join("AGENTS.md")).unwrap();
            assert!(!confirm(&token, &f.ws, &[], &f.ws.join("AGENTS.md")));
        }

        /// What can change after the path was resolved to a name inside the root: `canon` is given, no path is resolved again.
        #[test]
        fn what_the_path_leads_to_after_it_was_resolved_is_looked_at_without_following_a_link() {
            let f = fixture();
            let root = fs::canonicalize(&f.ws).unwrap();
            fs::create_dir_all(root.join("docs")).unwrap();
            fs::write(root.join("docs/regole.md"), "x").unwrap();
            let named = |opened: &File, canon: PathBuf| confirm_named(&opened.metadata().unwrap(), &root, &canon);

            // The file itself, in a folder of the workspace: accepted.
            let regole = open_without_waiting(&root.join("docs/regole.md")).unwrap();
            assert!(named(&regole, root.join("docs/regole.md")));
            // The last name made a link to the token after the token was opened: the same inode, but through a link.
            let token = open_without_waiting(&f.other.join("token")).unwrap();
            symlink(f.other.join("token"), root.join("uno.md")).unwrap();
            assert!(!named(&token, root.join("uno.md")), "the last name is a link to the file that was opened");
            // A folder of the path made a link to the folder of the token after the path was resolved.
            symlink(&f.other, root.join("cartella")).unwrap();
            assert!(!named(&token, root.join("cartella/token")), "a folder of the path is a link");
            // A second name made for the token after it was opened (the metadata of the open file still says one name).
            let before = token.metadata().unwrap();
            fs::hard_link(f.other.join("token"), root.join("due.md")).unwrap();
            assert!(!confirm_named(&before, &root, &root.join("due.md")), "the link count is read from the name, not from the open file");
            // And not a file at all, or not there.
            assert!(!named(&regole, root.join("docs")));
            assert!(!named(&regole, root.join("manca.md")));
        }

        #[test]
        fn a_pipe_is_refused_without_waiting_for_a_writer() {
            let f = fixture();
            let status = std::process::Command::new("mkfifo").arg(f.ws.join("AGENTS.md")).status();
            if !status.is_ok_and(|s| s.success()) {
                eprintln!("mkfifo non disponibile: prova saltata");
                return;
            }
            assert_eq!(read(&f, "AGENTS.md"), None);
        }

        #[test]
        fn a_hidden_place_inside_the_root_is_refused_unless_the_root_is_inside_it() {
            let f = fixture();
            // The data folder lives in the workspace (workspace = the home folder, data = `~/.aiw`).
            let data = f.ws.join(".aiw");
            fs::create_dir_all(&data).unwrap();
            fs::write(data.join("token"), "TOKEN").unwrap();
            let hidden = vec![data.clone()];
            symlink(data.join("token"), f.ws.join("AGENTS.md")).unwrap();
            assert_eq!(read_text_within(&f.ws, &hidden, &f.ws.join("AGENTS.md"), 100), None);
            assert_eq!(
                read_text_within(&f.ws, &[], &f.ws.join("AGENTS.md"), 100).as_deref(),
                Some("TOKEN"),
                "without the list it is inside"
            );
            // A hidden place given through a link is the same place.
            let by_link = f.ws.join("nascosto");
            symlink(&data, &by_link).unwrap();
            let hidden = vec![by_link];
            assert_eq!(read_text_within(&f.ws, &hidden, &f.ws.join("AGENTS.md"), 100), None);
            // A workspace that lives inside a hidden place (an isolated session in `<data>/worktrees`) reads its own files.
            let hidden = vec![f.ws.clone()];
            fs::write(f.ws.join("CLAUDE.md"), "mio").unwrap();
            assert_eq!(read_text_within(&f.ws, &hidden, &f.ws.join("CLAUDE.md"), 100).as_deref(), Some("mio"));
        }

        #[test]
        fn resolve_within_gives_the_real_path_of_a_folder_inside_the_root_only() {
            let f = fixture();
            fs::create_dir_all(f.ws.join("vera")).unwrap();
            symlink("vera", f.ws.join("alias")).unwrap();
            symlink(&f.other, f.ws.join("fuori")).unwrap();
            assert_eq!(resolve_within(&f.ws, &[], &f.ws.join("alias")), Some(fs::canonicalize(f.ws.join("vera")).unwrap()));
            assert_eq!(resolve_within(&f.ws, &[], &f.ws.join("fuori")), None);
            assert_eq!(resolve_within(&f.ws, &[], &f.ws.join("manca")), None);
            assert!(resolve_within(&f.ws, &[], &f.ws).is_some(), "the root is inside itself");
        }
    }
}
