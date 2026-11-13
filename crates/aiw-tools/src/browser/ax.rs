//! Accessibility tree (`Accessibility.getFullAXTree`) turned into a short text for the model.
//!
//! The tree is filtered to what a person (or a model) can act on or needs to orient itself:
//! controls (role, name, value, state), headings, landmarks and the loose text between them.
//! Controls get short references (`e1`, `e2`, ...) bound to the DOM node (`backendDOMNodeId`), so
//! they survive a re-snapshot of the same page (the number of a control does not change when
//! another one appears before it), unlike the positional numbers of the elements snapshot.

use serde_json::Value;
use std::collections::HashMap;

/// Lines of the output (controls, headings, landmarks and text together).
const MAX_LINES: usize = 220;
/// Characters of the output.
const MAX_CHARS: usize = 9000;
/// Characters of a name or of a text line.
const MAX_NAME: usize = 100;
/// Options listed next to a `<select>`.
const MAX_OPTIONS: usize = 8;

/// Roles the model can act on.
const CONTROLS: &[&str] = &[
    "button",
    "link",
    "textbox",
    "searchbox",
    "combobox",
    "checkbox",
    "radio",
    "switch",
    "tab",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "option",
    "slider",
    "spinbutton",
    "treeitem",
    "disclosuretriangle",
];

/// Roles that need no name to be worth showing.
const NAMELESS_OK: &[&str] = &["textbox", "searchbox", "combobox", "checkbox", "radio", "switch", "slider", "spinbutton"];

/// Containers that give the page its outline.
const LANDMARKS: &[&str] =
    &["main", "navigation", "banner", "contentinfo", "search", "form", "region", "dialog", "alertdialog", "complementary"];

/// References handed out so far on a page: node → `eN` and back. Kept for as long as the page
/// stays the same document, which is what makes them stable.
#[derive(Default)]
pub(crate) struct Refs {
    by_backend: HashMap<i64, String>,
    by_ref: HashMap<String, i64>,
    next: u32,
}

impl Refs {
    fn assign(&mut self, backend: i64) -> String {
        if let Some(r) = self.by_backend.get(&backend) {
            return r.clone();
        }
        self.next += 1;
        let r = format!("e{}", self.next);
        self.by_backend.insert(backend, r.clone());
        self.by_ref.insert(r.clone(), backend);
        r
    }

    pub(crate) fn backend(&self, r: &str) -> Option<i64> {
        self.by_ref.get(r).copied()
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

fn clip(t: &str, max: usize) -> String {
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= max {
        return t;
    }
    let cut: String = t.chars().take(max).collect();
    format!("{cut}…")
}

fn prop<'a>(node: &'a Value, name: &str) -> Option<&'a Value> {
    node["properties"].as_array()?.iter().find(|p| p["name"] == name).map(|p| &p["value"]["value"])
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(t)) => t == "true",
        _ => false,
    }
}

/// `value` of a node as text (numbers too: sliders and spinbuttons).
fn node_value(node: &Value) -> String {
    match &node["value"]["value"] {
        Value::String(t) => t.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

struct Out {
    text: String,
    lines: usize,
    truncated: bool,
    /// Loose text waiting to be written as one line, with the parent it belongs to.
    pending: String,
    pending_parent: String,
}

impl Out {
    fn full(&self) -> bool {
        self.lines >= MAX_LINES || self.text.chars().count() >= MAX_CHARS
    }

    fn flush_text(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let t = clip(&std::mem::take(&mut self.pending), MAX_NAME * 2);
        self.push(format!("text \"{t}\""), 0);
    }

    fn push(&mut self, line: String, indent: usize) {
        if self.full() {
            self.truncated = true;
            return;
        }
        self.text.push_str(&"  ".repeat(indent.min(3)));
        self.text.push_str(&line);
        self.text.push('\n');
        self.lines += 1;
    }
}

struct Item<'a> {
    node: &'a Value,
    depth: usize,
    /// Inside a control or heading that has already been written: nothing more to say.
    consumed: bool,
}

/// Names of the options under a `<select>` (the popup of a combobox).
fn options_of<'a>(root: &'a Value, by_id: &HashMap<&str, &'a Value>) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<&Value> = vec![root];
    while let Some(n) = stack.pop() {
        if s(&n["role"]["value"]) == "option" {
            let name = clip(s(&n["name"]["value"]), 40);
            if !name.is_empty() {
                out.push(name);
            }
        }
        if let Some(ids) = n["childIds"].as_array() {
            for id in ids.iter().rev() {
                if let Some(c) = id.as_str().and_then(|i| by_id.get(i)) {
                    stack.push(c);
                }
            }
        }
        if out.len() > MAX_OPTIONS {
            break;
        }
    }
    out
}

/// The filtered tree as text. References of the controls are taken from (and recorded in) `refs`.
/// Ends with a «troncato» line when the page is larger than the caps.
pub(crate) fn render(nodes: &[Value], refs: &mut Refs) -> String {
    let by_id: HashMap<&str, &Value> = nodes.iter().filter_map(|n| n["nodeId"].as_str().map(|i| (i, n))).collect();
    let mut out = Out { text: String::new(), lines: 0, truncated: false, pending: String::new(), pending_parent: String::new() };
    let mut stack: Vec<Item> =
        nodes.iter().rev().filter(|n| n.get("parentId").is_none()).map(|n| Item { node: n, depth: 0, consumed: false }).collect();
    while let Some(Item { node, depth, consumed }) = stack.pop() {
        if out.full() {
            out.truncated = true;
            break;
        }
        let role = s(&node["role"]["value"]);
        let ignored = node["ignored"].as_bool().unwrap_or(false);
        let name = s(&node["name"]["value"]);
        let mut child_depth = depth;
        let mut child_consumed = consumed;
        if !consumed && !ignored {
            if role == "StaticText" {
                let parent = s(&node["parentId"]).to_string();
                if parent != out.pending_parent {
                    out.flush_text();
                    out.pending_parent = parent;
                }
                if !name.trim().is_empty() {
                    if !out.pending.is_empty() {
                        out.pending.push(' ');
                    }
                    out.pending.push_str(name.trim());
                }
                child_consumed = true;
            } else if CONTROLS.contains(&role) {
                out.flush_text();
                let backend = node["backendDOMNodeId"].as_i64();
                let shown = clip(name, MAX_NAME);
                if let Some(backend) = backend.filter(|_| !shown.is_empty() || NAMELESS_OK.contains(&role) || !node_value(node).is_empty())
                {
                    let r = refs.assign(backend);
                    let mut line = format!("[{r}] {role} \"{shown}\"");
                    let value = node_value(node);
                    if !value.is_empty() && !matches!(role, "button" | "link" | "tab" | "menuitem") {
                        line.push_str(&format!(" value=\"{}\"", clip(&value, 60)));
                    }
                    if let Some(c) = prop(node, "checked") {
                        match c.as_str() {
                            Some("true") => line.push_str(" (selezionato)"),
                            Some("mixed") => line.push_str(" (parziale)"),
                            _ if matches!(role, "checkbox" | "radio" | "switch" | "menuitemcheckbox" | "menuitemradio") => {
                                line.push_str(" (non selezionato)")
                            }
                            _ => {}
                        }
                    }
                    if truthy(prop(node, "selected")) {
                        line.push_str(" (scelto)");
                    }
                    if let Some(e) = prop(node, "expanded") {
                        line.push_str(if truthy(Some(e)) { " (aperto)" } else { " (chiuso)" });
                    }
                    if truthy(prop(node, "disabled")) {
                        line.push_str(" (disabilitato)");
                    }
                    if truthy(prop(node, "required")) {
                        line.push_str(" (obbligatorio)");
                    }
                    if let Some(u) = prop(node, "url").and_then(|u| u.as_str()) {
                        if role == "link" {
                            line.push_str(&format!(" -> {}", clip(u, 90)));
                        }
                    }
                    if role == "combobox" {
                        let opts = options_of(node, &by_id);
                        if !opts.is_empty() {
                            let more = if opts.len() > MAX_OPTIONS { " | …" } else { "" };
                            line.push_str(&format!(" [{}{more}]", opts.iter().take(MAX_OPTIONS).cloned().collect::<Vec<_>>().join(" | ")));
                        }
                    }
                    out.push(line, depth);
                    child_consumed = true;
                }
            } else if role == "heading" && !name.is_empty() {
                out.flush_text();
                let level = prop(node, "level").and_then(|l| l.as_i64()).unwrap_or(0);
                out.push(format!("heading[{level}] \"{}\"", clip(name, MAX_NAME)), depth);
                child_consumed = true;
            } else if LANDMARKS.contains(&role) {
                out.flush_text();
                let label = if name.is_empty() { String::new() } else { format!(" \"{}\"", clip(name, 60)) };
                out.push(format!("-- {role}{label}"), depth);
                child_depth = depth + 1;
            }
        }
        if let Some(ids) = node["childIds"].as_array() {
            for id in ids.iter().rev() {
                if let Some(c) = id.as_str().and_then(|i| by_id.get(i)) {
                    stack.push(Item { node: c, depth: child_depth, consumed: child_consumed });
                }
            }
        }
    }
    out.flush_text();
    if out.truncated {
        out.text.push_str("… albero troncato: altri elementi non mostrati (scorri con browser_scroll e rifai lo snapshot)\n");
    }
    out.text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(id: &str, parent: Option<&str>, role: &str, name: &str, backend: i64, children: &[&str], props: Value) -> Value {
        let mut n = json!({"nodeId": id, "role": {"value": role}, "name": {"value": name}, "backendDOMNodeId": backend,
            "childIds": children, "properties": props, "ignored": false});
        if let Some(p) = parent {
            n["parentId"] = json!(p);
        }
        n
    }

    fn page() -> Vec<Value> {
        vec![
            node("1", None, "RootWebArea", "Pagina", 1, &["2", "3", "4", "8", "10"], json!([])),
            node("2", Some("1"), "heading", "Iscrizione", 2, &["20"], json!([{"name":"level","value":{"value":2}}])),
            node("20", Some("2"), "StaticText", "Iscrizione", 20, &[], json!([])),
            node("3", Some("1"), "StaticText", "Compila il modulo.", 3, &[], json!([])),
            node("4", Some("1"), "form", "Dati", 4, &["5", "6", "7", "9"], json!([])),
            node("5", Some("4"), "textbox", "Nome", 5, &[], json!([{"name":"required","value":{"value":true}}])),
            node("6", Some("4"), "checkbox", "Accetto", 6, &[], json!([{"name":"checked","value":{"value":"false"}}])),
            node("7", Some("4"), "button", "Invia", 7, &["70"], json!([{"name":"disabled","value":{"value":true}}])),
            node("70", Some("7"), "StaticText", "Invia", 70, &[], json!([])),
            node("9", Some("4"), "combobox", "Paese", 9, &["90"], json!([])),
            node("90", Some("9"), "option", "Italia", 91, &[], json!([])),
            node("8", Some("1"), "link", "Aiuto", 8, &[], json!([{"name":"url","value":{"value":"https://e.example/aiuto"}}])),
            node("10", Some("1"), "generic", "", 10, &[], json!([])),
        ]
    }

    #[test]
    fn keeps_controls_headings_landmarks_and_text_with_state() {
        let mut refs = Refs::default();
        let t = render(&page(), &mut refs);
        assert!(t.contains("heading[2] \"Iscrizione\""), "{t}");
        assert!(t.contains("text \"Compila il modulo.\""), "{t}");
        assert!(t.contains("-- form \"Dati\""), "{t}");
        assert!(t.contains("[e1] textbox \"Nome\" (obbligatorio)"), "{t}");
        assert!(t.contains("[e2] checkbox \"Accetto\" (non selezionato)"), "{t}");
        assert!(t.contains("[e3] button \"Invia\" (disabilitato)"), "{t}");
        assert!(t.contains("combobox \"Paese\" [Italia]"), "{t}");
        assert!(t.contains("link \"Aiuto\" -> https://e.example/aiuto"), "{t}");
        // The text inside a control or a heading is not repeated, the options are not listed twice.
        assert_eq!(t.matches("Invia").count(), 1, "{t}");
        assert_eq!(t.matches("Italia").count(), 1, "{t}");
        // Controls inside a landmark are indented.
        assert!(t.contains("\n  [e1] textbox"), "{t}");
        assert!(!t.contains("troncato"));
    }

    #[test]
    fn references_are_stable_across_snapshots_and_resolve_to_the_node() {
        let mut refs = Refs::default();
        render(&page(), &mut refs);
        assert_eq!(refs.backend("e2"), Some(6));
        // A new control appears before the others: the old ones keep their reference.
        let mut nodes = page();
        nodes[0]["childIds"] = json!(["11", "2", "3", "4", "8", "10"]);
        nodes.push(node("11", Some("1"), "button", "Nuovo", 99, &[], json!([])));
        let t = render(&nodes, &mut refs);
        assert!(t.contains("[e6] button \"Nuovo\""), "{t}");
        assert!(t.contains("[e2] checkbox \"Accetto\""), "{t}");
        assert_eq!(refs.backend("e6"), Some(99));
        assert_eq!(refs.backend("e99"), None);
    }

    #[test]
    fn big_pages_are_capped_with_a_marker() {
        let mut nodes = vec![node("1", None, "RootWebArea", "P", 1, &[], json!([]))];
        let ids: Vec<String> = (0..600).map(|i| format!("b{i}")).collect();
        nodes[0]["childIds"] = json!(ids);
        for (i, id) in ids.iter().enumerate() {
            nodes.push(node(id, Some("1"), "button", &format!("Pulsante numero {i}"), 100 + i as i64, &[], json!([])));
        }
        let mut refs = Refs::default();
        let t = render(&nodes, &mut refs);
        assert!(t.contains("troncato"), "{}", &t[t.len().saturating_sub(200)..]);
        assert!(t.lines().count() <= MAX_LINES + 2);
        assert!(t.contains("Pulsante numero 0") && !t.contains("Pulsante numero 599"));
    }

    #[test]
    fn ignored_nodes_are_transparent_and_unnamed_links_are_dropped() {
        let mut nodes = vec![
            node("1", None, "RootWebArea", "P", 1, &["2"], json!([])),
            node("2", Some("1"), "generic", "", 2, &["3", "4"], json!([])),
            node("3", Some("2"), "link", "", 3, &[], json!([])),
            node("4", Some("2"), "button", "Vai", 4, &[], json!([])),
        ];
        nodes[1]["ignored"] = json!(true);
        let t = render(&nodes, &mut Refs::default());
        assert_eq!(t.trim(), "[e1] button \"Vai\"");
    }
}
