//! Git worktree isolation for sessions (pattern seen in Codex and Claude Code): an isolated
//! session works on its own branch in a separate checkout under the app data directory. The user
//! reviews the diff, applies it to their checkout as uncommitted changes, or discards it.

use aiw_core::api::{ChangedFile, WorktreeInfo, WorktreeStatus};
use aiw_core::{Error, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const MAX_DIFF: usize = 200_000;

fn run(dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<Output> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(args)
        // Never wait for credentials or an editor, and keep messages in a stable language.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("LC_ALL", "C")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| Error::Unsupported(format!("git non disponibile: {e}")))?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(input)?;
    }
    Ok(child.wait_with_output()?)
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = run(dir, args, None)?;
    if !out.status.success() {
        return Err(Error::Runtime(format!(
            "git {}: {}",
            args.iter().find(|a| !a.starts_with('-')).unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Bookkeeping commits in the session branch are authored by the app, whatever the user config.
const IDENTITY: [&str; 4] = ["-c", "user.name=AI Workspace", "-c", "user.email=aiw@localhost"];

/// Creates the worktree of `session_id` for the repository containing `workspace`.
pub fn create(workspace: &Path, worktrees_dir: &Path, session_id: &str) -> Result<WorktreeInfo> {
    let not_repo = || {
        Error::Invalid(
            "La cartella di lavoro non è un repository git: per isolare la sessione in un worktree serve un repository con almeno un commit.".into(),
        )
    };
    let repo = PathBuf::from(git(workspace, &["rev-parse", "--show-toplevel"]).map_err(|_| not_repo())?.trim());
    let prefix = git(workspace, &["rev-parse", "--show-prefix"])?.trim().trim_end_matches('/').to_string();
    let base = git(&repo, &["rev-parse", "--verify", "HEAD"])
        .map_err(|_| Error::Invalid("Il repository non ha ancora commit: crea il primo commit per usare i worktree.".into()))?
        .trim()
        .to_string();
    // ids are `ses_<timestamp><random>`: the random tail keeps branch names short and unique.
    let short: String = session_id.chars().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect();
    let name = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
    std::fs::create_dir_all(worktrees_dir)?;
    let path = worktrees_dir.join(format!("{name}-{short}"));
    let branch = format!("aiw/{short}");
    let path_s = path.to_string_lossy().into_owned();
    git(&repo, &["worktree", "add", "-q", "-b", &branch, &path_s, &base])?;
    let root = if prefix.is_empty() { path.clone() } else { path.join(&prefix) };
    Ok(WorktreeInfo { path: path_s, root: root.to_string_lossy().into_owned(), branch, base, repo: repo.to_string_lossy().into_owned() })
}

fn parse_name_status(out: &str) -> Vec<ChangedFile> {
    out.lines()
        .filter_map(|l| {
            let mut parts = l.split('\t');
            let status = parts.next()?.chars().next()?.to_string();
            // Renames and copies list the old and the new path: show the new one.
            let path = parts.next_back()?.to_string();
            Some(ChangedFile { status, path })
        })
        .collect()
}

/// Everything the session changed since `base`: commits in the branch, edits and new files.
pub fn status(wt: &WorktreeInfo) -> Result<WorktreeStatus> {
    let dir = Path::new(&wt.path);
    if !dir.is_dir() {
        return Err(Error::NotFound(format!("worktree {} non trovato", wt.path)));
    }
    // Intent-to-add makes new files visible to `git diff` without staging their content.
    git(dir, &["add", "--intent-to-add", "--all"])?;
    let files = parse_name_status(&git(dir, &["diff", "--name-status", "-M", &wt.base])?);
    let diff_stat = git(dir, &["diff", "--stat", "-M", &wt.base])?;
    let mut diff = git(dir, &["diff", "-M", &wt.base])?;
    let truncated = diff.len() > MAX_DIFF;
    if truncated {
        let mut cut = MAX_DIFF;
        while !diff.is_char_boundary(cut) {
            cut -= 1;
        }
        diff.truncate(cut);
    }
    Ok(WorktreeStatus { info: wt.clone(), files, diff_stat, diff, truncated })
}

/// Applies the session changes to the user's checkout as uncommitted changes, then records them
/// in the session branch so that the next apply only carries newer work.
pub fn apply(wt: &mut WorktreeInfo) -> Result<Vec<ChangedFile>> {
    let dir = PathBuf::from(&wt.path);
    let repo = PathBuf::from(&wt.repo);
    git(&dir, &["add", "--all"])?;
    let patch = run(&dir, &["diff", "--cached", "--binary", "-M", &wt.base], None)?;
    if !patch.status.success() {
        return Err(Error::Runtime(String::from_utf8_lossy(&patch.stderr).trim().to_string()));
    }
    if patch.stdout.iter().all(|b| b.is_ascii_whitespace()) {
        return Err(Error::Invalid("Nessuna modifica da applicare.".into()));
    }
    let files = parse_name_status(&git(&dir, &["diff", "--cached", "--name-status", "-M", &wt.base])?);
    // Plain apply leaves the changes unstaged; fall back to a 3-way merge when the user's files
    // moved on in the meantime (conflicts are left as markers, like a merge).
    let plain = run(&repo, &["apply", "--whitespace=nowarn", "-"], Some(&patch.stdout))?;
    if !plain.status.success() {
        let three = run(&repo, &["apply", "--3way", "--whitespace=nowarn", "-"], Some(&patch.stdout))?;
        if !three.status.success() {
            return Err(Error::Runtime(format!(
                "Le modifiche non si applicano al workspace: {}",
                String::from_utf8_lossy(&three.stderr).trim()
            )));
        }
    }
    let staged = run(&dir, &["diff", "--cached", "--quiet"], None)?;
    if !staged.status.success() {
        let mut args = IDENTITY.to_vec();
        args.extend(["commit", "-q", "--no-verify", "-m", "AI Workspace: modifiche applicate al workspace"]);
        git(&dir, &args)?;
    }
    wt.base = git(&dir, &["rev-parse", "HEAD"])?.trim().to_string();
    Ok(files)
}

/// Removes the worktree and its branch (the user's checkout is untouched).
pub fn remove(wt: &WorktreeInfo) -> Result<()> {
    let repo = Path::new(&wt.repo);
    if Path::new(&wt.path).exists() {
        git(repo, &["worktree", "remove", "--force", &wt.path])?;
    } else {
        let _ = git(repo, &["worktree", "prune"]);
    }
    let _ = git(repo, &["branch", "-D", &wt.branch]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        git(p, &["init", "-q", "-b", "main"]).unwrap();
        // Windows runners default to autocrlf=true: keep the bytes the test compares.
        git(p, &["config", "core.autocrlf", "false"]).unwrap();
        std::fs::create_dir_all(p.join("src")).unwrap();
        std::fs::write(p.join("src/a.txt"), "uno\n").unwrap();
        std::fs::write(p.join("README.md"), "# Prova\n").unwrap();
        git(p, &["add", "."]).unwrap();
        let mut args = IDENTITY.to_vec();
        args.extend(["commit", "-q", "-m", "init"]);
        git(p, &args).unwrap();
        d
    }

    #[test]
    fn isolate_review_apply_and_discard() {
        let r = repo();
        let data = tempfile::tempdir().unwrap();
        let ws = r.path().canonicalize().unwrap();
        let mut wt = create(&ws.join("src"), &data.path().join("worktrees"), "ses_0192f3c4a1b2c3d4e5f6").unwrap();
        assert_eq!(wt.branch, "aiw/c3d4e5f6");
        assert!(wt.root.ends_with("src"), "{}", wt.root);

        // The session edits a file and creates another; the user's checkout does not change.
        std::fs::write(Path::new(&wt.root).join("a.txt"), "uno\ndue\n").unwrap();
        std::fs::write(Path::new(&wt.root).join("b.txt"), "nuovo\n").unwrap();
        assert_eq!(std::fs::read_to_string(ws.join("src/a.txt")).unwrap(), "uno\n");

        let st = status(&wt).unwrap();
        let mut files: Vec<_> = st.files.iter().map(|f| format!("{} {}", f.status, f.path)).collect();
        files.sort();
        assert_eq!(files, vec!["A src/b.txt", "M src/a.txt"]);
        assert!(st.diff.contains("+due"));

        let applied = apply(&mut wt).unwrap();
        assert_eq!(applied.len(), 2);
        assert_eq!(std::fs::read_to_string(ws.join("src/a.txt")).unwrap(), "uno\ndue\n");
        assert_eq!(std::fs::read_to_string(ws.join("src/b.txt")).unwrap(), "nuovo\n");
        // Already applied: nothing left until the session changes something else.
        assert!(status(&wt).unwrap().files.is_empty());
        assert!(matches!(apply(&mut wt), Err(Error::Invalid(_))));

        remove(&wt).unwrap();
        assert!(!Path::new(&wt.path).exists());
        assert!(!git(&ws, &["branch", "--list", "aiw/*"]).unwrap().contains("aiw/"));
    }

    #[test]
    fn needs_a_repository_with_commits() {
        let d = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        assert!(matches!(create(d.path(), data.path(), "ses_x"), Err(Error::Invalid(_))));
        git(d.path(), &["init", "-q"]).unwrap();
        let e = create(d.path(), data.path(), "ses_x").unwrap_err();
        assert!(e.to_string().contains("commit"), "{e}");
    }
}
