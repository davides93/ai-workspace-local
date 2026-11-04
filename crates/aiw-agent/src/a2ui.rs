//! A2UI v0.9 (basic catalog) behind the `render_ui` tool.
//!
//! The model describes a surface as a flat list of components (A2UI adjacency list: children are
//! referenced by id, `root` is the entry point) plus an optional data model. We validate it,
//! normalise the shortcuts small models tend to use, and turn it into the v0.9 messages the UI
//! renders: `createSurface` → `updateComponents` → `updateDataModel`. User interactions come back
//! as the v0.9 client `action` message in a user turn (see [`ACTION_PREFIX`]).

use aiw_runtimes::ToolSpec;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub const VERSION: &str = "v0.9";
pub const BASIC_CATALOG: &str = "https://a2ui.org/specification/v0_9/catalogs/basic/catalog.json";
/// Name of the tool and of the AG-UI `CUSTOM` event carrying the messages.
pub const TOOL: &str = "render_ui";
pub const EVENT: &str = "a2ui";
/// Stored user messages that carry an A2UI action have this `name` and start with the prefix.
pub const ACTION_NAME: &str = "a2ui.action";
pub const ACTION_PREFIX: &str = "[Azione A2UI]";

const COMPONENTS: &[&str] = &[
    "Text",
    "Image",
    "Icon",
    "Video",
    "AudioPlayer",
    "Row",
    "Column",
    "List",
    "Card",
    "Tabs",
    "Modal",
    "Divider",
    "Button",
    "TextField",
    "CheckBox",
    "ChoicePicker",
    "Slider",
    "DateTimeInput",
];

/// Properties every component of a type must have (from the basic catalog `required` lists).
fn required(component: &str) -> &'static [&'static str] {
    match component {
        "Text" => &["text"],
        "Image" | "Video" | "AudioPlayer" => &["url"],
        "Icon" => &["name"],
        "Row" | "Column" | "List" => &["children"],
        "Card" => &["child"],
        "Tabs" => &["tabs"],
        "Modal" => &["trigger", "content"],
        "Button" => &["child", "action"],
        "TextField" => &["label"],
        "CheckBox" => &["label", "value"],
        "ChoicePicker" => &["options", "value"],
        "Slider" => &["max", "value"],
        "DateTimeInput" => &["value"],
        _ => &[],
    }
}

pub fn spec() -> ToolSpec {
    ToolSpec::function(
        TOOL,
        "Mostra all'utente un'interfaccia interattiva A2UI v0.9 (moduli, scelte, riepiloghi, tabelle di card). \
         Componenti piatti con `id`; uno deve avere id `root`; i figli si indicano per id (`children`, `child`). \
         Tipi: Text(text, variant h1-h5|body|caption), Image(url), Icon(name), Row/Column/List(children), Card(child), \
         Tabs(tabs:[{title,child}]), Modal(trigger,content), Divider, Button(child o label, action:{event:{name,context}}, variant primary), \
         TextField(label,value,variant shortText|longText|number|obscured), CheckBox(label,value), \
         ChoicePicker(label,options:[{label,value}],value:[...],variant mutuallyExclusive|multipleSelection), Slider(label,min,max,value), DateTimeInput(label,value). \
         I valori legati ai dati usano {\"path\":\"/campo\"} e `data` è il modello dati iniziale. Il click su un Button ti arriva come azione con il context risolto.",
        json!({
            "type": "object",
            "properties": {
                "surfaceId": {"type": "string", "description": "Identificativo dell'interfaccia (riusalo per aggiornarla)"},
                "components": {"type": "array", "items": {"type": "object", "properties": {"id": {"type": "string"}, "component": {"type": "string", "enum": COMPONENTS}}, "required": ["id", "component"]}},
                "data": {"type": "object", "description": "Modello dati iniziale (valori dei campi)"}
            },
            "required": ["components"]
        }),
    )
}

/// Validates the tool arguments and returns `(surface_id, messages)`, or the list of problems
/// the model has to fix.
pub fn build(args: &Value) -> Result<(String, Vec<Value>), String> {
    let surface = args.get("surfaceId").and_then(|v| v.as_str()).unwrap_or("ui").trim().to_string();
    let surface = if surface.is_empty() { "ui".to_string() } else { surface };
    if !surface.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("surfaceId può contenere solo lettere, cifre, _ e -".into());
    }
    let Some(list) = args.get("components").and_then(|v| v.as_array()).filter(|l| !l.is_empty()) else {
        return Err("`components` deve essere un elenco non vuoto di componenti".into());
    };
    let mut components: Vec<Map<String, Value>> = Vec::new();
    let mut problems = Vec::new();
    for (i, c) in list.iter().enumerate() {
        let Some(obj) = c.as_object() else {
            problems.push(format!("components[{i}] non è un oggetto"));
            continue;
        };
        components.push(obj.clone());
    }
    normalise(&mut components);

    let ids: Vec<String> = components.iter().map(|c| c.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string()).collect();
    let known: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
    if known.len() != ids.len() {
        problems.push("ogni componente deve avere un `id` diverso".into());
    }
    if !known.contains("root") {
        problems.push("manca il componente con id \"root\" (radice dell'interfaccia)".into());
    }
    for c in &components {
        let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if id.is_empty() {
            problems.push("un componente non ha `id`".into());
            continue;
        }
        let kind = c.get("component").and_then(|v| v.as_str()).unwrap_or("");
        if !COMPONENTS.contains(&kind) {
            problems.push(format!("{id}: tipo `{kind}` sconosciuto (usa uno di: {})", COMPONENTS.join(", ")));
            continue;
        }
        for prop in required(kind) {
            if !c.contains_key(*prop) {
                problems.push(format!("{id} ({kind}): manca `{prop}`"));
            }
        }
        let mut refs: Vec<&str> = Vec::new();
        for key in ["child", "trigger", "content"] {
            if let Some(r) = c.get(key).and_then(|v| v.as_str()) {
                refs.push(r);
            }
        }
        match c.get("children") {
            Some(Value::Array(a)) => refs.extend(a.iter().filter_map(|v| v.as_str())),
            Some(Value::Object(t)) => {
                if let Some(r) = t.get("componentId").and_then(|v| v.as_str()) {
                    refs.push(r);
                }
                if !t.get("path").and_then(|v| v.as_str()).is_some_and(|p| !p.is_empty()) {
                    problems.push(format!("{id}: il template dei figli richiede `path`"));
                }
            }
            Some(_) => problems.push(format!("{id}: `children` deve essere un elenco di id")),
            None => {}
        }
        if let Some(tabs) = c.get("tabs").and_then(|v| v.as_array()) {
            refs.extend(tabs.iter().filter_map(|t| t.get("child").and_then(|v| v.as_str())));
        }
        for r in refs {
            if !known.contains(r) {
                problems.push(format!("{id}: riferimento a un componente inesistente `{r}`"));
            }
        }
        if kind == "Button" {
            let a = c.get("action");
            let ok = a.and_then(|a| a.get("event")).and_then(|e| e.get("name")).and_then(|n| n.as_str()).is_some()
                || a.and_then(|a| a.get("functionCall")).and_then(|f| f.get("call")).and_then(|n| n.as_str()).is_some();
            if a.is_some() && !ok {
                problems.push(format!("{id}: action deve essere {{\"event\":{{\"name\":\"...\",\"context\":{{...}}}}}}"));
            }
        }
    }
    if let Some(d) = args.get("data") {
        if !d.is_object() && !d.is_null() {
            problems.push("`data` deve essere un oggetto JSON".into());
        }
    }
    if !problems.is_empty() {
        problems.truncate(12);
        return Err(format!("Interfaccia non valida:\n- {}", problems.join("\n- ")));
    }

    let mut messages = vec![
        json!({"version": VERSION, "createSurface": {"surfaceId": surface, "catalogId": BASIC_CATALOG}}),
        json!({"version": VERSION, "updateComponents": {"surfaceId": surface, "components": components}}),
    ];
    if let Some(d) = args.get("data").filter(|d| d.is_object()) {
        messages.push(json!({"version": VERSION, "updateDataModel": {"surfaceId": surface, "path": "/", "value": d}}));
    }
    Ok((surface, messages))
}

/// Models often nest child components instead of referencing them by id: lift them into the
/// flat list (depth-first) and leave their ids in place.
fn flatten(components: Vec<Map<String, Value>>) -> Vec<Map<String, Value>> {
    fn lift(mut c: Map<String, Value>, out: &mut Vec<Map<String, Value>>, n: &mut usize) {
        let mut nested = Vec::new();
        let take = |v: &mut Value, nested: &mut Vec<Map<String, Value>>, n: &mut usize| {
            if let Value::Object(o) = v {
                if o.contains_key("component") || o.contains_key("type") {
                    let mut o = std::mem::take(o);
                    let id = match o.get("id").and_then(|x| x.as_str()) {
                        Some(id) if !id.is_empty() => id.to_string(),
                        _ => {
                            *n += 1;
                            let id = format!("c{n}");
                            o.insert("id".into(), Value::String(id.clone()));
                            id
                        }
                    };
                    nested.push(o);
                    *v = Value::String(id);
                }
            }
        };
        for key in ["child", "trigger", "content"] {
            if let Some(v) = c.get_mut(key) {
                take(v, &mut nested, n);
            }
        }
        if let Some(Value::Array(items)) = c.get_mut("children") {
            for v in items.iter_mut() {
                take(v, &mut nested, n);
            }
        }
        if let Some(Value::Array(tabs)) = c.get_mut("tabs") {
            for t in tabs.iter_mut() {
                if let Some(v) = t.get_mut("child") {
                    take(v, &mut nested, n);
                }
            }
        }
        out.push(c);
        for o in nested {
            lift(o, out, n);
        }
    }
    let mut out = Vec::new();
    let mut n = 0;
    for c in components {
        lift(c, &mut out, &mut n);
    }
    out
}

/// Shortcuts accepted from models and rewritten into catalog form.
fn normalise(components: &mut Vec<Map<String, Value>>) {
    *components = flatten(std::mem::take(components));
    let mut extra = Vec::new();
    for c in components.iter_mut() {
        // Component type given as `type`.
        if !c.contains_key("component") {
            if let Some(t) = c.remove("type") {
                c.insert("component".into(), t);
            }
        }
        let kind = c.get("component").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if kind == "Button" {
            // Button { label: "Invia" } → Button { child: "<id>__label" } + Text.
            if !c.contains_key("child") {
                if let Some(label) = c.remove("label").or_else(|| c.remove("text")) {
                    let child = format!("{id}__label");
                    extra.push(json!({"id": child, "component": "Text", "text": label}).as_object().cloned().unwrap_or_default());
                    c.insert("child".into(), Value::String(child));
                }
            }
            // action: "name" → action: { event: { name } }
            if let Some(Value::String(name)) = c.get("action").cloned() {
                c.insert("action".into(), json!({"event": {"name": name}}));
            }
            if !c.contains_key("action") {
                c.insert("action".into(), json!({"event": {"name": id}}));
            }
        }
        if kind == "Text" && !c.contains_key("text") {
            if let Some(t) = c.remove("content").or_else(|| c.remove("label")) {
                c.insert("text".into(), t);
            }
        }
    }
    components.extend(extra);
}

/// The user turn sent back when a Button fires an event (client→server `action` message).
pub fn action_text(action: &Value) -> String {
    format!("{ACTION_PREFIX} {}", json!({"version": VERSION, "action": action}))
}

/// System prompt section, added when the agent can use `render_ui`.
pub fn prompt_hint() -> &'static str {
    "# Interfacce interattive (A2UI)\nQuando servono dati dall'utente (moduli, scelte, conferme con opzioni) o un riepilogo da consultare, usa `render_ui` invece di fare molte domande nel testo. Esempio di argomenti:\n{\"surfaceId\":\"prenotazione\",\"components\":[{\"id\":\"root\",\"component\":\"Column\",\"children\":[\"titolo\",\"nome\",\"invia\"]},{\"id\":\"titolo\",\"component\":\"Text\",\"text\":\"Prenotazione\",\"variant\":\"h3\"},{\"id\":\"nome\",\"component\":\"TextField\",\"label\":\"Nome\",\"value\":{\"path\":\"/nome\"}},{\"id\":\"invia\",\"component\":\"Button\",\"label\":\"Conferma\",\"variant\":\"primary\",\"action\":{\"event\":{\"name\":\"conferma\",\"context\":{\"nome\":{\"path\":\"/nome\"}}}}}],\"data\":{\"nome\":\"\"}}\nL'azione dell'utente ti arriva come messaggio che inizia con [Azione A2UI]: usa il suo `context`.\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_v09_messages_and_normalises_shortcuts() {
        let args = json!({
            "surfaceId": "form",
            "components": [
                {"id": "root", "component": "Column", "children": ["t", "nome", "ok"]},
                {"id": "t", "type": "Text", "content": "Ciao"},
                {"id": "nome", "component": "TextField", "label": "Nome", "value": {"path": "/nome"}},
                {"id": "ok", "component": "Button", "label": "Invia", "action": "invia"}
            ],
            "data": {"nome": "Ada"}
        });
        let (surface, msgs) = build(&args).unwrap();
        assert_eq!(surface, "form");
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["createSurface"]["catalogId"], BASIC_CATALOG);
        let comps = msgs[1]["updateComponents"]["components"].as_array().unwrap();
        let ok = comps.iter().find(|c| c["id"] == "ok").unwrap();
        assert_eq!(ok["child"], "ok__label");
        assert_eq!(ok["action"]["event"]["name"], "invia");
        assert!(comps.iter().any(|c| c["id"] == "ok__label" && c["component"] == "Text" && c["text"] == "Invia"));
        assert!(comps.iter().any(|c| c["id"] == "t" && c["component"] == "Text" && c["text"] == "Ciao"));
        assert_eq!(msgs[2]["updateDataModel"]["value"]["nome"], "Ada");
        assert!(msgs.iter().all(|m| m["version"] == VERSION));
    }

    #[test]
    fn nested_children_are_lifted_into_the_flat_list() {
        let (_, msgs) = build(&json!({"components": [
            {"id": "root", "component": "Column", "children": [
                {"id": "titolo", "component": "Text", "text": "Ciao"},
                {"component": "Card", "child": {"component": "Text", "text": "Dentro"}},
                {"id": "ok", "component": "Button", "label": "OK"}
            ]}
        ]}))
        .unwrap();
        let comps = msgs[1]["updateComponents"]["components"].as_array().unwrap();
        let root = comps.iter().find(|c| c["id"] == "root").unwrap();
        assert_eq!(root["children"], json!(["titolo", "c1", "ok"]));
        let card = comps.iter().find(|c| c["id"] == "c1").unwrap();
        assert_eq!(card["child"], "c2");
        assert!(comps.iter().any(|c| c["id"] == "c2" && c["text"] == "Dentro"));
        assert!(comps.iter().any(|c| c["id"] == "ok__label"));
    }

    #[test]
    fn reports_problems_the_model_can_fix() {
        let err = build(&json!({"components": [
            {"id": "main", "component": "Column", "children": ["x"]},
            {"id": "b", "component": "Slider"},
            {"id": "c", "component": "Carousel"}
        ]}))
        .unwrap_err();
        assert!(err.contains("root"), "{err}");
        assert!(err.contains("inesistente `x`"), "{err}");
        assert!(err.contains("manca `max`"), "{err}");
        assert!(err.contains("`Carousel` sconosciuto"), "{err}");
        assert!(build(&json!({"components": []})).is_err());
    }

    #[test]
    fn action_turn_carries_the_v09_message() {
        let t = action_text(
            &json!({"name": "invia", "surfaceId": "form", "sourceComponentId": "ok", "timestamp": "2026-11-02T10:00:00Z", "context": {"nome": "Ada"}}),
        );
        assert!(t.starts_with(ACTION_PREFIX));
        let v: Value = serde_json::from_str(t.trim_start_matches(ACTION_PREFIX).trim()).unwrap();
        assert_eq!(v["version"], "v0.9");
        assert_eq!(v["action"]["context"]["nome"], "Ada");
    }
}
