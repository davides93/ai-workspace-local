//! Skills (pattern P11): folders with a `SKILL.md` whose YAML-ish frontmatter has `name` and
//! `description`. Only the index enters the system prompt; the body is loaded on demand by the
//! `skill` tool (progressive disclosure). Workspace skills override user skills with the same name.
//!
//! The engine reads `SKILL.md` itself, outside the sandbox. The skills of the user (`<data>/skills`) are
//! trusted: the person put them there, links included. The skills a workspace brings (`<workspace>/.aiw/skills`)
//! are not: a cloned repository, or a command of an agent, can make `SKILL.md` or the folder of a skill a
//! link to any file the engine can read. Those are read under [`aiw_core::confined`]: the folder of the
//! skills, the folder of each skill and its `SKILL.md` must stay inside the workspace, and the file of a
//! skill inside the folder the skills were found in.

use aiw_core::confined;
use std::path::{Path, PathBuf};

/// Longest `SKILL.md` that is read (characters).
const MAX_SKILL_CHARS: usize = 40_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    /// For a skill of a workspace: the folder (resolved) the skills were found in. `path` and the folder of the skill have
    /// to stay inside it every time they are read. `None` for a trusted skill, read as it is.
    pub confined_to: Option<PathBuf>,
}

/// A folder skills are discovered in.
#[derive(Debug, Clone)]
pub struct SkillRoot {
    path: PathBuf,
    /// `(workspace, hidden places)` for a folder that comes with a workspace.
    confine: Option<(PathBuf, Vec<PathBuf>)>,
}

impl SkillRoot {
    /// The skills of the user: read as they are, links included.
    pub fn trusted(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), confine: None }
    }
    /// The skills a workspace brings: the folder has to be inside `workspace` (and out of the `hidden` places) once its links are
    /// resolved, and no skill may lead out of it.
    pub fn confined(path: impl Into<PathBuf>, workspace: impl Into<PathBuf>, hidden: Vec<PathBuf>) -> Self {
        Self { path: path.into(), confine: Some((workspace.into(), hidden)) }
    }
}

pub fn parse_frontmatter(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    let mut name = None;
    let mut desc = None;
    for line in rest[..end].lines() {
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().trim_matches(['"', '\'']).to_string();
            match k.trim() {
                "name" => name = Some(v),
                "description" => desc = Some(v),
                _ => {}
            }
        }
    }
    Some((name?, desc.unwrap_or_default()))
}

/// Discovers skills in the given roots (later roots win on name clashes), all of them trusted. See [`discover_in`].
pub fn discover(roots: &[PathBuf]) -> Vec<Skill> {
    discover_in(&roots.iter().map(SkillRoot::trusted).collect::<Vec<_>>())
}

/// Discovers skills in the given roots (later roots win on name clashes). A skill that is refused (a link that leads out) is
/// as if it was not there: it does not hide a skill of the same name found before.
pub fn discover_in(roots: &[SkillRoot]) -> Vec<Skill> {
    let mut out: Vec<Skill> = Vec::new();
    for root in roots {
        let confined_to = match &root.confine {
            None => None,
            Some((workspace, hidden)) => match confined::resolve_within(workspace, hidden, &root.path) {
                Some(resolved) => Some(resolved),
                None => continue, // the folder of the skills leads out of the workspace
            },
        };
        let Ok(rd) = std::fs::read_dir(&root.path) else { continue };
        for e in rd.flatten() {
            let p = e.path().join("SKILL.md");
            let text = match &confined_to {
                None => std::fs::read_to_string(&p).ok(),
                Some(inside) => confined::read_text_within(inside, &[], &p, MAX_SKILL_CHARS),
            };
            let Some(text) = text else { continue };
            if let Some((name, description)) = parse_frontmatter(&text) {
                out.retain(|s| s.name != name);
                out.push(Skill { name, description, path: p, confined_to: confined_to.clone() });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn load(skill: &Skill) -> String {
    let dir = skill.path.parent().unwrap_or(Path::new("."));
    let (text, dir) = match &skill.confined_to {
        None => (std::fs::read_to_string(&skill.path).unwrap_or_default(), Some(dir.to_path_buf())),
        // The folder may have been swapped for a link since the skill was found: the file and the folder are checked again.
        Some(inside) => match confined::read_text_within(inside, &[], &skill.path, MAX_SKILL_CHARS) {
            Some(text) => (text, confined::resolve_within(inside, &[], dir)),
            None => {
                return format!("La skill `{}` non è leggibile: il file manca o esce dalla cartella delle skill del workspace.", skill.name)
            }
        },
    };
    let files: Vec<String> = dir
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n != "SKILL.md").collect())
        .unwrap_or_default();
    let mut s = text.chars().take(MAX_SKILL_CHARS).collect::<String>();
    if !files.is_empty() {
        s.push_str(&format!("\n\nFile della skill in {}: {}", skill.path.parent().unwrap_or(Path::new(".")).display(), files.join(", ")));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_and_overrides_skills() {
        let user = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        for (root, desc) in [(user.path(), "versione utente"), (ws.path(), "versione workspace")] {
            let d = root.join("fatture");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("SKILL.md"), format!("---\nname: fatture\ndescription: {desc}\n---\n# Come registrare le fatture\n"))
                .unwrap();
        }
        let s = discover(&[user.path().to_path_buf(), ws.path().to_path_buf()]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].description, "versione workspace");
        assert!(load(&s[0]).contains("registrare le fatture"));
    }

    fn skill_file(dir: &Path, name: &str, desc: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\ndescription: {desc}\n---\n# {name}\n")).unwrap();
    }

    /// A workspace with its `.aiw/skills`, and a folder of "somebody else" outside it holding a file that would pass for a skill.
    #[cfg(unix)]
    mod confined {
        use super::*;
        use std::os::unix::fs::symlink;

        struct Fixture {
            _dir: tempfile::TempDir,
            ws: PathBuf,
            skills: PathBuf,
            other: PathBuf,
        }

        fn fixture() -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            let (ws, other) = (dir.path().join("ws"), dir.path().join("altrui"));
            let skills = ws.join(".aiw").join("skills");
            std::fs::create_dir_all(&skills).unwrap();
            // A file of the engine's account that starts like a skill (a YAML file does).
            skill_file(&other.join("segreto"), "rubata", "contenuto riservato di un altro");
            Fixture { ws, skills, other, _dir: dir }
        }

        fn found(f: &Fixture) -> Vec<Skill> {
            discover_in(&[SkillRoot::confined(&f.skills, &f.ws, vec![])])
        }

        #[test]
        fn a_skill_file_that_is_a_link_to_a_file_outside_the_workspace_is_not_found_and_not_loaded() {
            let f = fixture();
            skill_file(&f.skills.join("vera"), "vera", "la mia");
            std::fs::create_dir_all(f.skills.join("rubata")).unwrap();
            symlink(f.other.join("segreto").join("SKILL.md"), f.skills.join("rubata").join("SKILL.md")).unwrap();
            let skills = found(&f);
            assert_eq!(skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["vera"]);
            assert!(skills[0].confined_to.is_some());

            // Found while it was a plain file, then swapped for the link: loading checks again.
            skill_file(&f.skills.join("poi"), "poi", "diventa un link");
            let poi = found(&f).into_iter().find(|s| s.name == "poi").unwrap();
            assert!(load(&poi).contains("# poi"));
            std::fs::remove_file(f.skills.join("poi").join("SKILL.md")).unwrap();
            symlink(f.other.join("segreto").join("SKILL.md"), f.skills.join("poi").join("SKILL.md")).unwrap();
            let loaded = load(&poi);
            assert!(!loaded.contains("rubata") && !loaded.contains("riservato"), "{loaded}");
            assert!(loaded.contains("non è leggibile"), "{loaded}");
        }

        #[test]
        fn a_skill_folder_that_is_a_link_to_a_folder_outside_the_workspace_is_not_found_and_its_files_are_not_listed() {
            let f = fixture();
            std::fs::write(f.other.join("segreto").join("chiavi.txt"), "x").unwrap();
            symlink(f.other.join("segreto"), f.skills.join("rubata")).unwrap();
            assert!(found(&f).is_empty());

            // A skill found inside the workspace, whose folder is then swapped for a link out: no skill text and no file names.
            skill_file(&f.skills.join("poi"), "poi", "la cartella cambia");
            let poi = found(&f).into_iter().find(|s| s.name == "poi").unwrap();
            std::fs::rename(f.skills.join("poi"), f.ws.join("poi-vera")).unwrap();
            symlink(f.other.join("segreto"), f.skills.join("poi")).unwrap();
            let loaded = load(&poi);
            assert!(!loaded.contains("chiavi.txt") && !loaded.contains("riservato"), "{loaded}");
        }

        #[test]
        fn a_skills_folder_that_is_a_link_out_of_the_workspace_gives_no_skill() {
            let f = fixture();
            std::fs::remove_dir_all(&f.skills).unwrap();
            skill_file(&f.other.join("squadra").join("utile"), "utile", "una skill di un'altra cartella");
            symlink(f.other.join("squadra"), &f.skills).unwrap();
            assert!(found(&f).is_empty());
            // The same when `.aiw` is the link.
            std::fs::remove_file(&f.skills).unwrap();
            std::fs::remove_dir(f.ws.join(".aiw")).unwrap();
            symlink(&f.other, f.ws.join(".aiw")).unwrap();
            std::fs::rename(f.other.join("squadra"), f.other.join("skills")).unwrap();
            assert!(found(&f).is_empty());
        }

        #[test]
        fn links_inside_the_skills_folder_work_and_a_refused_skill_does_not_hide_one_of_the_user() {
            let f = fixture();
            skill_file(&f.skills.join("vera"), "vera", "nella cartella");
            symlink("vera", f.skills.join("alias")).unwrap(); // the same skill under another name of the folder
            std::fs::write(f.skills.join("vera").join("nota.md"), "n").unwrap();
            let skills = found(&f);
            assert!(skills.iter().any(|s| s.name == "vera"));
            let vera = skills.iter().find(|s| s.name == "vera").unwrap();
            let text = load(vera);
            assert!(text.contains("# vera") && text.contains("nota.md"), "{text}");

            // A skill of the user with the name of one that the workspace brings as a link out: the user's stays.
            let user = tempfile::tempdir().unwrap();
            skill_file(&user.path().join("rubata"), "rubata", "quella dell'utente");
            std::fs::create_dir_all(f.skills.join("rubata")).unwrap();
            symlink(f.other.join("segreto").join("SKILL.md"), f.skills.join("rubata").join("SKILL.md")).unwrap();
            let all = discover_in(&[SkillRoot::trusted(user.path()), SkillRoot::confined(&f.skills, &f.ws, vec![])]);
            let rubata = all.iter().find(|s| s.name == "rubata").unwrap();
            assert_eq!(rubata.description, "quella dell'utente");
            assert!(rubata.confined_to.is_none());
        }

        #[test]
        fn the_skills_of_the_user_stay_as_they_are_links_included() {
            let f = fixture();
            let user = tempfile::tempdir().unwrap();
            // The person put a link in `<data>/skills`: a folder of skills kept somewhere else, and a single SKILL.md.
            symlink(f.other.join("segreto"), user.path().join("cartella")).unwrap();
            std::fs::create_dir_all(user.path().join("file")).unwrap();
            symlink(f.other.join("segreto").join("SKILL.md"), user.path().join("file").join("SKILL.md")).unwrap();
            let skills = discover(&[user.path().to_path_buf()]);
            assert_eq!(skills.len(), 1, "both links name the same skill");
            assert!(load(&skills[0]).contains("# rubata"));
            assert!(skills[0].confined_to.is_none());
        }
    }
}
