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

/// [`narrow_tools`] for a parent that also has `delegate_tools`: the child gets its own list
/// narrowed to what the parent can hand out, its own `tools` plus the ones it only delegates.
///
/// With no `delegate_tools` this is exactly [`narrow_tools`]. With some, a `*` among the granted
/// names stands for the tools that do not need an explicit grant (`explicit` says which ones
/// those are): a child that lists one of the others keeps it only if the parent names it itself.
pub fn narrow_tools_delegating(
    parent: &[String],
    parent_delegate: &[String],
    child: &[String],
    explicit: impl Fn(&str) -> bool,
) -> Vec<String> {
    if parent_delegate.is_empty() {
        return narrow_tools(parent, child);
    }
    let mut granted: Vec<String> = parent.to_vec();
    for t in parent_delegate {
        if !granted.contains(t) {
            granted.push(t.clone());
        }
    }
    narrow_tools(&granted, child).into_iter().filter(|t| t == "*" || !explicit(t) || granted.contains(t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn without_delegate_tools_narrowing_is_the_plain_intersection() {
        let explicit = |t: &str| t == "channel_send";
        for (parent, child) in [
            (v(&["*"]), v(&["read_file", "channel_send"])),
            (v(&["todo", "agent"]), v(&["*"])),
            (v(&["todo", "agent"]), v(&["agent", "shell"])),
            (v(&[]), v(&["*"])),
        ] {
            assert_eq!(narrow_tools_delegating(&parent, &[], &child, explicit), narrow_tools(&parent, &child), "{parent:?} / {child:?}");
        }
    }

    #[test]
    fn delegate_tools_widen_what_the_child_can_get_but_never_past_the_explicit_grants() {
        let explicit = |t: &str| matches!(t, "channel_send" | "browser_eval");
        let parent = v(&["agent", "todo"]);
        // `*` among the delegated tools: the child keeps its own list...
        assert_eq!(
            narrow_tools_delegating(&parent, &v(&["*"]), &v(&["browser_navigate", "web_fetch", "todo"]), explicit),
            v(&["browser_navigate", "web_fetch", "todo"])
        );
        // ...minus the tools that need an explicit grant the parent never named.
        assert_eq!(
            narrow_tools_delegating(&parent, &v(&["*"]), &v(&["read_file", "browser_eval", "channel_send"]), explicit),
            v(&["read_file"])
        );
        // A tool the parent names itself (own or delegated) is granted even when explicit.
        assert_eq!(
            narrow_tools_delegating(&parent, &v(&["*", "channel_send"]), &v(&["channel_send", "browser_eval"]), explicit),
            v(&["channel_send"])
        );
        // A child with `*` gets the named tools of the parent (no wildcard to hand out).
        assert_eq!(narrow_tools_delegating(&parent, &v(&["web_fetch"]), &v(&["*"]), explicit), v(&["agent", "todo", "web_fetch"]));
        // A named delegate list only lets those names through.
        assert_eq!(
            narrow_tools_delegating(&parent, &v(&["web_fetch"]), &v(&["web_fetch", "shell", "todo"]), explicit),
            v(&["web_fetch", "todo"])
        );
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
