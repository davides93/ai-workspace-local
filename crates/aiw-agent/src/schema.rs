//! A small JSON Schema validator for the structured final answer of an agent (no extra dependency).
//!
//! Supported keywords: `type` (a name or an array of names), `properties`, `required`, `items`,
//! `enum`, `additionalProperties: false`, `minItems`, `maxItems`. Any other keyword is ignored.

use serde_json::Value;

const TYPES: [&str; 7] = ["object", "array", "string", "number", "integer", "boolean", "null"];

/// Checks that `schema` is usable: an object whose `type`, when present, names known types and
/// whose nested `properties` and `items` are usable too.
pub fn check_schema(schema: &Value) -> Result<(), String> {
    check_at(schema, "$")
}

fn check_at(schema: &Value, path: &str) -> Result<(), String> {
    let Some(obj) = schema.as_object() else {
        return Err(format!("{path}: lo schema deve essere un oggetto JSON"));
    };
    if let Some(t) = obj.get("type") {
        let names: Vec<&Value> = match t {
            Value::Array(a) => a.iter().collect(),
            other => vec![other],
        };
        for n in names {
            if !n.as_str().is_some_and(|n| TYPES.contains(&n)) {
                return Err(format!("{path}: tipo `{n}` non valido (usa {})", TYPES.join(", ")));
            }
        }
    }
    if let Some(r) = obj.get("required") {
        if !r.as_array().is_some_and(|a| a.iter().all(Value::is_string)) {
            return Err(format!("{path}: `required` deve essere un elenco di nomi"));
        }
    }
    if let Some(p) = obj.get("properties") {
        let Some(p) = p.as_object() else {
            return Err(format!("{path}: `properties` deve essere un oggetto"));
        };
        for (k, v) in p {
            check_at(v, &format!("{path}.{k}"))?;
        }
    }
    if let Some(i) = obj.get("items") {
        check_at(i, &format!("{path}[]"))?;
    }
    if let Some(e) = obj.get("enum") {
        if !e.is_array() {
            return Err(format!("{path}: `enum` deve essere un elenco"));
        }
    }
    for k in ["minItems", "maxItems"] {
        if obj.get(k).is_some_and(|v| v.as_u64().is_none()) {
            return Err(format!("{path}: `{k}` deve essere un intero non negativo"));
        }
    }
    Ok(())
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn is_type(v: &Value, t: &str) -> bool {
    match t {
        "integer" => v.is_i64() || v.is_u64() || v.as_f64().is_some_and(|f| f.fract() == 0.0),
        "number" => v.is_number(),
        other => type_name(v) == other,
    }
}

/// Validates `v` against `schema`; the errors name the failing place (`$.items[0].nome`).
pub fn validate(schema: &Value, v: &Value) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    walk(schema, v, "$", &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn walk(schema: &Value, v: &Value, path: &str, errors: &mut Vec<String>) {
    let Some(obj) = schema.as_object() else { return };
    if let Some(t) = obj.get("type") {
        let wanted: Vec<&str> = match t {
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            other => other.as_str().into_iter().collect(),
        };
        if !wanted.is_empty() && !wanted.iter().any(|w| is_type(v, w)) {
            errors.push(format!("{path}: atteso {}, trovato {}", wanted.join(" o "), type_name(v)));
            return;
        }
    }
    if let Some(Value::Array(options)) = obj.get("enum") {
        if !options.contains(v) {
            let list = options.iter().map(|o| o.to_string()).collect::<Vec<_>>().join(", ");
            errors.push(format!("{path}: valore non consentito (ammessi: {list})"));
        }
    }
    if let Some(map) = v.as_object() {
        let props = obj.get("properties").and_then(Value::as_object);
        for name in obj.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            if !map.contains_key(name) {
                errors.push(format!("{path}.{name}: manca il campo obbligatorio"));
            }
        }
        if let Some(props) = props {
            for (name, sub) in props {
                if let Some(child) = map.get(name) {
                    walk(sub, child, &format!("{path}.{name}"), errors);
                }
            }
        }
        if obj.get("additionalProperties") == Some(&Value::Bool(false)) {
            for name in map.keys().filter(|k| !props.is_some_and(|p| p.contains_key(*k))) {
                errors.push(format!("{path}.{name}: campo non previsto"));
            }
        }
    }
    if let Some(items) = v.as_array() {
        if let Some(min) = obj.get("minItems").and_then(Value::as_u64) {
            if (items.len() as u64) < min {
                errors.push(format!("{path}: servono almeno {min} elementi (trovati {})", items.len()));
            }
        }
        if let Some(max) = obj.get("maxItems").and_then(Value::as_u64) {
            if items.len() as u64 > max {
                errors.push(format!("{path}: al massimo {max} elementi (trovati {})", items.len()));
            }
        }
        if let Some(sub) = obj.get("items") {
            for (i, item) in items.iter().enumerate() {
                walk(sub, item, &format!("{path}[{i}]"), errors);
            }
        }
    }
}

/// Finds the JSON value in a model answer: the whole text, else a ```json fenced block, else
/// from the first `{`/`[` to the last `}`/`]`.
pub fn extract_json(text: &str) -> Option<Value> {
    let text = text.trim();
    if let Ok(v) = serde_json::from_str(text) {
        return Some(v);
    }
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        let after = after.strip_prefix("json").or_else(|| after.strip_prefix("JSON")).unwrap_or(after);
        if let Some(end) = after.find("```") {
            if let Ok(v) = serde_json::from_str(after[..end].trim()) {
                return Some(v);
            }
        }
    }
    let first = text.find(['{', '['])?;
    let last = text.rfind(['}', ']'])?;
    (last > first).then(|| serde_json::from_str(&text[first..=last]).ok()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "titolo": {"type": "string"},
                "voti": {"type": "array", "items": {"type": "integer"}, "minItems": 1, "maxItems": 3},
                "stato": {"enum": ["ok", "ko"]},
                "extra": {"type": ["string", "null"]}
            },
            "required": ["titolo", "voti"],
            "additionalProperties": false
        })
    }

    #[test]
    fn accepts_a_conforming_value() {
        assert!(validate(&schema(), &json!({"titolo": "t", "voti": [1, 2], "stato": "ok", "extra": null})).is_ok());
        // 2.0 is an integer for JSON Schema.
        assert!(validate(&schema(), &json!({"titolo": "t", "voti": [2.0]})).is_ok());
    }

    #[test]
    fn reports_every_problem_with_its_path() {
        let e = validate(&schema(), &json!({"voti": [1, "x", 2.5, 4], "stato": "boh", "altro": 1})).unwrap_err();
        assert!(e.contains(&"$.titolo: manca il campo obbligatorio".to_string()), "{e:?}");
        assert!(e.iter().any(|m| m.starts_with("$.voti[1]: atteso integer, trovato string")), "{e:?}");
        assert!(e.iter().any(|m| m.starts_with("$.voti[2]: atteso integer")), "{e:?}");
        assert!(e.iter().any(|m| m.starts_with("$.voti: al massimo 3")), "{e:?}");
        assert!(e.iter().any(|m| m.starts_with("$.stato: valore non consentito")), "{e:?}");
        assert!(e.contains(&"$.altro: campo non previsto".to_string()), "{e:?}");
        assert_eq!(validate(&json!({"type": "array", "minItems": 1}), &json!([])).unwrap_err().len(), 1);
        assert!(validate(&json!({"type": "object"}), &json!([1])).unwrap_err()[0].starts_with("$: atteso object, trovato array"));
    }

    #[test]
    fn unknown_keywords_are_ignored() {
        assert!(validate(&json!({"type": "string", "pattern": "^x$", "format": "email"}), &json!("qualsiasi")).is_ok());
    }

    #[test]
    fn check_schema_rejects_unusable_schemas() {
        assert!(check_schema(&schema()).is_ok());
        assert!(check_schema(&json!({"properties": {"a": {"type": ["string", "null"]}}})).is_ok());
        for bad in [
            json!("string"),
            json!([]),
            json!({"type": "stringa"}),
            json!({"type": ["string", 3]}),
            json!({"properties": {"a": {"type": "boh"}}}),
            json!({"items": 5}),
            json!({"required": "a"}),
            json!({"minItems": -1}),
        ] {
            assert!(check_schema(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn extracts_json_from_a_model_answer() {
        assert_eq!(extract_json(" {\"a\": 1} "), Some(json!({"a": 1})));
        assert_eq!(extract_json("Ecco:\n```json\n{\"a\": [1, 2]}\n```\nFine"), Some(json!({"a": [1, 2]})));
        assert_eq!(extract_json("```\n[1, 2]\n```"), Some(json!([1, 2])));
        assert_eq!(extract_json("Risultato: {\"a\": {\"b\": 2}} grazie"), Some(json!({"a": {"b": 2}})));
        assert_eq!(extract_json("Elenco [1, 2, 3] ok"), Some(json!([1, 2, 3])));
        assert_eq!(extract_json("nessun json qui"), None);
        assert_eq!(extract_json("{rotto"), None);
    }
}
