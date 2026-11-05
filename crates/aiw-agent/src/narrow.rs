//! A sub-agent never gets more than its parent: the sandbox and the tool list are narrowed to what
//! the parent already has.

use aiw_tools::SandboxMode;

fn rank(m: SandboxMode) -> u8 {
    match m {
        SandboxMode::ReadOnly => 0,
        SandboxMode::WorkspaceWrite => 1,
        SandboxMode::Off => 2,
    }
}

/// The stricter of the two sandbox modes (ReadOnly < WorkspaceWrite < Off).
pub fn narrow_sandbox(parent: SandboxMode, child: SandboxMode) -> SandboxMode {
    if rank(child) > rank(parent) {
        parent
    } else {
        child
    }
}

/// The tools the child may use: `*` stands for "all", so a parent with `*` leaves the child's list
/// alone and a child with `*` gets the parent's; otherwise the intersection, in the child's order.
pub fn narrow_tools(parent: &[String], child: &[String]) -> Vec<String> {
    let all = |l: &[String]| l.iter().any(|t| t == "*");
    if all(parent) {
        child.to_vec()
    } else if all(child) {
        parent.to_vec()
    } else {
        child.iter().filter(|t| parent.contains(t)).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn sandbox_is_never_wider_than_the_parent() {
        use SandboxMode::*;
        for (parent, child, want) in [
            (ReadOnly, Off, ReadOnly),
            (ReadOnly, WorkspaceWrite, ReadOnly),
            (WorkspaceWrite, Off, WorkspaceWrite),
            (WorkspaceWrite, ReadOnly, ReadOnly),
            (Off, ReadOnly, ReadOnly),
            (Off, Off, Off),
            (WorkspaceWrite, WorkspaceWrite, WorkspaceWrite),
        ] {
            assert_eq!(narrow_sandbox(parent, child), want, "{parent:?} / {child:?}");
        }
    }

    #[test]
    fn tools_are_intersected_with_wildcards_handled() {
        assert_eq!(narrow_tools(&v(&["*"]), &v(&["read_file", "shell"])), v(&["read_file", "shell"]));
        assert_eq!(narrow_tools(&v(&["read_file", "list_dir"]), &v(&["*"])), v(&["read_file", "list_dir"]));
        assert_eq!(narrow_tools(&v(&["*"]), &v(&["*"])), v(&["*"]));
        assert_eq!(narrow_tools(&v(&["list_dir", "read_file"]), &v(&["shell", "read_file", "list_dir"])), v(&["read_file", "list_dir"]));
        assert!(narrow_tools(&v(&["read_file"]), &v(&["shell"])).is_empty());
        assert!(narrow_tools(&v(&[]), &v(&["*"])).is_empty());
    }
}
