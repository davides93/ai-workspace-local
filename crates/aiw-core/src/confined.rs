//! Files the engine reads for itself, outside the sandbox, and puts in a prompt: `AGENTS.md` of the shared workspace and the
//! `SKILL.md` of the skills a workspace brings.
//!
//! The sandbox protects the commands and the file tools of an agent, not the engine. The engine reads these files with its own
//! permissions, so a link made by the agent in the workspace (`ln -s <data>/token AGENTS.md`, or one that a cloned repository
//! ships: `AGENTS.md -> ~/.ssh/id_ed25519`) would carry any file the engine can read into the system prompt of the next message.
//!
//! The rule is the one the file tools already apply to a path (`ToolContext::resolve`), made safe against a swap:
//!
//! * the file is **opened first**, and what was opened must be a regular file (not a folder, not a pipe: opening a pipe would block
//!   the run for ever, so the file is opened without waiting);
//! * the path, with every link resolved (`canonicalize`), must stay **inside the root** (the workspace, or the folder the skills
//!   were found in) and out of the **hidden** places (the data folder of the app, `sandbox.deny_read`), unless the root itself
//!   lives in one of them;
//! * the opened file must be the one the path leads to now (same device and inode): a path swapped for a link between the
//!   check and the read gives a different file and is refused;
//! * a file with several names (a hard link) is accepted only when every one of its names is in the same folder: `ln CLAUDE.md
//!   AGENTS.md` is the folder's own, `ln <data>/token AGENTS.md` has a name that is not there (the link count is higher than
//!   the names found) and is refused.
//!
//! A link that stays inside the root (`AGENTS.md -> CLAUDE.md`) is read: that is a common and legitimate use.
//!
//! Only Unix has the device/inode comparison and the link count; on Windows the path is checked after the open (resolved and
//! compared with the root) but the file cannot be compared with it, and a hard link is not told from a file.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Most directory entries looked at to account for the names of a file with more than one.
#[cfg(unix)]
const NAME_SCAN_LIMIT: usize = 10_000;

/// `path` with every link resolved, if it stays inside `root` (also resolved) and out of the `hidden` places. The folder
/// of a skill, or the folder the skills are in, go through here: no file is opened.
pub fn resolve_within(root: &Path, hidden: &[PathBuf], path: &Path) -> Option<PathBuf> {
    let root = std::fs::canonicalize(root).ok()?;
    let canon = std::fs::canonicalize(path).ok()?;
    if !canon.starts_with(&root) || is_hidden(&canon, &root, hidden) {
        return None;
    }
    Some(canon)
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
/// else: a link to a file outside `root` or in a hidden place, a hard link to a file that has another name elsewhere, a folder,
/// a pipe, a file that is not there.
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
    let Some(canon) = resolve_within(root, hidden, path) else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let Ok(now) = std::fs::metadata(&canon) else { return false };
        let id = (opened.dev(), opened.ino());
        if id != (now.dev(), now.ino()) {
            return false; // the path leads somewhere else than the file that was opened
        }
        if opened.nlink() != 1 && !every_name_is_beside(&canon, id, opened.nlink()) {
            return false; // another name of this file is somewhere else
        }
    }
    true
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

/// Does the file with identity `id` and `nlink` names have all of them in the folder of `canon`? The names are counted among
/// the regular files of that folder (a link in it does not count): the count equals the link count only when no name of the
/// file is anywhere else.
#[cfg(unix)]
fn every_name_is_beside(canon: &Path, id: (u64, u64), nlink: u64) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Some(Ok(entries)) = canon.parent().map(std::fs::read_dir) else { return false };
    let mut names = 0u64;
    for (i, entry) in entries.flatten().enumerate() {
        if i >= NAME_SCAN_LIMIT {
            return false;
        }
        if let Ok(m) = std::fs::symlink_metadata(entry.path()) {
            if m.is_file() && (m.dev(), m.ino()) == id {
                names += 1;
            }
        }
    }
    names == nlink
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
        fn a_hard_link_to_somebody_elses_file_is_refused_and_one_between_two_names_of_the_folder_is_read() {
            let f = fixture();
            fs::hard_link(f.other.join("token"), f.ws.join("AGENTS.md")).unwrap();
            assert_eq!(read(&f, "AGENTS.md"), None, "the token has another name, in the folder of somebody else");
            // Two more names for it in the workspace do not make it the workspace's: the one outside is still not accounted for.
            fs::hard_link(f.other.join("token"), f.ws.join("COPIA.md")).unwrap();
            assert_eq!(read(&f, "AGENTS.md"), None);
            assert_eq!(read(&f, "COPIA.md"), None);

            fs::write(f.ws.join("CLAUDE.md"), "Regole").unwrap();
            fs::hard_link(f.ws.join("CLAUDE.md"), f.ws.join("GEMINI.md")).unwrap();
            assert_eq!(read(&f, "GEMINI.md").as_deref(), Some("Regole"), "both names are in the folder");
            // A link in the same folder is not a name of the file: the count is of regular files.
            symlink("CLAUDE.md", f.ws.join("ALTRO.md")).unwrap();
            assert_eq!(read(&f, "GEMINI.md").as_deref(), Some("Regole"));
            // A third name, outside the folder, makes it refused again.
            fs::create_dir_all(f.ws.join("docs")).unwrap();
            fs::hard_link(f.ws.join("CLAUDE.md"), f.ws.join("docs/copia.md")).unwrap();
            assert_eq!(read(&f, "GEMINI.md"), None);
            assert_eq!(read(&f, "ALTRO.md"), None);
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
