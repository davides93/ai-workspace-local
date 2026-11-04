//! Skills (pattern P11): folders with a `SKILL.md` whose YAML-ish frontmatter has `name` and
//! `description`. Only the index enters the system prompt; the body is loaded on demand by the
//! `skill` tool (progressive disclosure). Workspace skills override user skills with the same name.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
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

/// Discovers skills in the given roots (later roots win on name clashes).
pub fn discover(roots: &[PathBuf]) -> Vec<Skill> {
    let mut out: Vec<Skill> = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        for e in rd.flatten() {
            let p = e.path().join("SKILL.md");
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            if let Some((name, description)) = parse_frontmatter(&text) {
                out.retain(|s| s.name != name);
                out.push(Skill { name, description, path: p });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn load(skill: &Skill) -> String {
    let text = std::fs::read_to_string(&skill.path).unwrap_or_default();
    let dir = skill.path.parent().unwrap_or(Path::new("."));
    let files: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n != "SKILL.md").collect())
        .unwrap_or_default();
    let mut s = text.chars().take(40_000).collect::<String>();
    if !files.is_empty() {
        s.push_str(&format!("\n\nFile della skill in {}: {}", dir.display(), files.join(", ")));
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
}
