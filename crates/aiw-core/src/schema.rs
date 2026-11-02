use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Subset of JSON Schema used by every configurable component. The UI renders a form from it
/// (`apps/ui/src/shared/ui/SchemaForm.tsx`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SettingsSchema {
    #[serde(rename = "type", default = "object_type")]
    pub kind: String,
    #[serde(default)]
    pub properties: BTreeMap<String, FieldSchema>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
}

fn object_type() -> String {
    "object".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FieldSchema {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Value>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub secret: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub advanced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
}

impl SettingsSchema {
    /// Default values declared by the schema.
    pub fn defaults(&self) -> Map<String, Value> {
        self.properties.iter().filter_map(|(k, f)| f.default.clone().map(|v| (k.clone(), v))).collect()
    }

    /// Merges `values` over the defaults and validates types/ranges/enums.
    pub fn resolve(&self, values: &Map<String, Value>) -> Result<Map<String, Value>, String> {
        let mut out = self.defaults();
        for (k, v) in values {
            let Some(f) = self.properties.get(k) else {
                // Unknown keys are preserved: user manifests may add their own placeholders.
                out.insert(k.clone(), v.clone());
                continue;
            };
            f.validate(k, v)?;
            out.insert(k.clone(), v.clone());
        }
        for r in &self.required {
            if !out.contains_key(r) {
                return Err(format!("missing required setting `{r}`"));
            }
        }
        Ok(out)
    }
}

impl FieldSchema {
    pub fn validate(&self, key: &str, v: &Value) -> Result<(), String> {
        if v.is_null() {
            return Ok(());
        }
        let ok = match self.kind.as_str() {
            "string" => v.is_string(),
            "integer" => v.as_i64().is_some() || v.as_u64().is_some(),
            "number" => v.is_number(),
            "boolean" => v.is_boolean(),
            "array" => v.is_array(),
            "object" => v.is_object(),
            _ => true,
        };
        if !ok {
            return Err(format!("`{key}` must be of type {}", self.kind));
        }
        if let Some(n) = v.as_f64() {
            if let Some(min) = self.minimum {
                if n < min {
                    return Err(format!("`{key}` must be ≥ {min}"));
                }
            }
            if let Some(max) = self.maximum {
                if n > max {
                    return Err(format!("`{key}` must be ≤ {max}"));
                }
            }
        }
        if let Some(e) = &self.enum_values {
            if !e.contains(v) {
                return Err(format!("`{key}` must be one of {}", Value::Array(e.clone())));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> SettingsSchema {
        serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "ctx_size": {"type": "integer", "default": 8192, "minimum": 512, "maximum": 262144},
                "cache_type": {"type": "string", "enum": ["f16", "q8_0"], "default": "f16"},
                "flash": {"type": "boolean", "default": true}
            }
        }))
        .unwrap()
    }

    #[test]
    fn resolves_defaults_and_overrides() {
        let s = schema();
        let v = s.resolve(json!({"ctx_size": 4096}).as_object().unwrap()).unwrap();
        assert_eq!(v["ctx_size"], 4096);
        assert_eq!(v["cache_type"], "f16");
        assert_eq!(v["flash"], true);
    }

    #[test]
    fn rejects_out_of_range_and_bad_enum() {
        let s = schema();
        assert!(s.resolve(json!({"ctx_size": 1}).as_object().unwrap()).is_err());
        assert!(s.resolve(json!({"cache_type": "q2"}).as_object().unwrap()).is_err());
        assert!(s.resolve(json!({"flash": "yes"}).as_object().unwrap()).is_err());
    }
}
