//! Which folders must not be handed to the agents, whatever the person picks in the macOS panel. Pure
//! (no AppKit): the rules are checked on every platform, the panel and the bookmarks are `macos_folders`.
use std::path::Path;

/// System folders: nothing there is the person's work, and writing there is never wanted.
const SYSTEM: &[&str] = &["/System", "/Library", "/usr", "/bin", "/sbin", "/etc", "/private/etc", "/dev", "/Applications", "/cores"];

/// Why `path` (canonical) cannot be given, in a sentence the interface shows; `None` when it can.
/// `real_home` is the person's home (not the container), `container_home` the home of the app itself.
pub fn refuse_reason(path: &Path, real_home: Option<&Path>, container_home: Option<&Path>) -> Option<String> {
    if let Some(home) = real_home {
        if path == home || home.starts_with(path) {
            return Some(
                "è la cartella personale (o ne contiene una): scegline una al suo interno, per esempio Download o Documenti".into(),
            );
        }
        if path == home.join("Library") {
            return Some("è la cartella Libreria: contiene i dati di tutte le app, scegli una cartella tua".into());
        }
    }
    if let Some(c) = container_home {
        if path.starts_with(c) || c.starts_with(path) {
            return Some("è la cartella interna dell'app: lì gli agenti lavorano già".into());
        }
    }
    if SYSTEM.iter().any(|s| path.starts_with(s)) {
        return Some("è una cartella di sistema: scegline una tua".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_home_its_parents_the_library_and_the_system_are_refused() {
        let home = PathBuf::from("/Users/anna");
        let container = PathBuf::from("/Users/anna/Library/Containers/dev.aiworkspace.local/Data");
        let r = |p: &str| refuse_reason(Path::new(p), Some(&home), Some(&container));
        for p in [
            "/Users/anna",
            "/Users",
            "/",
            "/Users/anna/Library",
            "/System",
            "/System/Library/Fonts",
            "/usr/local",
            "/Applications",
            "/private/etc",
        ] {
            assert!(r(p).is_some(), "{p} must be refused");
        }
        for p in [
            "/Users/anna/Library/Containers/dev.aiworkspace.local/Data",
            "/Users/anna/Library/Containers/dev.aiworkspace.local/Data/Documents",
            "/Users/anna/Library/Containers",
        ] {
            // Inside the container, or holding it: the app's own folder.
            assert!(r(p).is_some(), "{p} must be refused");
        }
        for p in [
            "/Users/anna/Downloads",
            "/Users/anna/Documents/Progetti",
            "/Users/anna/Library/Mobile Documents/com~apple~CloudDocs",
            "/Volumes/Backup",
            "/Users/anna-altro/Downloads",
        ] {
            assert!(r(p).is_none(), "{p} must be allowed: {:?}", r(p));
        }
    }

    #[test]
    fn without_a_known_home_only_the_system_rules_apply() {
        assert!(refuse_reason(Path::new("/Library/Caches"), None, None).is_some());
        assert!(refuse_reason(Path::new("/Users/anna/Downloads"), None, None).is_none());
    }
}
