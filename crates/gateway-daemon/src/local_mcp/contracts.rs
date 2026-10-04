//! Offline discovery projection and validation of the frozen adapter envelope.
use serde_json::{Value, json};

pub fn artifact(name: &str) -> Option<Value> {
    let text = match name {
        "catalog" => include_str!("../../../../schemas/codex/v1/catalog.json"),
        "common.schema.json" => include_str!("../../../../schemas/codex/v1/common.schema.json"),
        "request.schema.json" => include_str!("../../../../schemas/codex/v1/request.schema.json"),
        "response.schema.json" => include_str!("../../../../schemas/codex/v1/response.schema.json"),
        "resource.schema.json" => include_str!("../../../../schemas/codex/v1/resource.schema.json"),
        "catalog.schema.json" => include_str!("../../../../schemas/codex/v1/catalog.schema.json"),
        _ => return None,
    };
    Some(serde_json::from_str(text).expect("checked-in contract"))
}

fn rewrite(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get_mut("$ref") {
                if let Some(fragment) = reference.strip_prefix("common.schema.json#/$defs/") {
                    *reference = format!("#/$defs/{fragment}");
                }
            }
            for value in map.values_mut() {
                rewrite(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                rewrite(value);
            }
        }
        _ => {}
    }
}

pub fn bundled(name: &str, operation: &Value) -> Value {
    let mut schema = artifact(name).expect("static schema");
    schema["$defs"] = artifact("common.schema.json").unwrap()["$defs"].clone();
    schema["properties"]["operation"] = if name == "response.schema.json" {
        json!({"anyOf":[{"const":operation},{"type":"null"}]})
    } else {
        json!({"const":operation})
    };
    rewrite(&mut schema);
    schema
}

pub fn tools() -> Value {
    let catalog = artifact("catalog").unwrap();
    Value::Array(catalog["tools"].as_array().unwrap().iter().map(|tool| json!({
        "name": tool["name"], "description": format!("{}; application availability: unsupported until the shared facade is installed", tool["operation"].as_str().unwrap()),
        "inputSchema": bundled("request.schema.json", &tool["operation"]),
        "outputSchema": bundled("response.schema.json", &tool["operation"]),
        "annotations": tool["annotations"], "execution": tool["execution"]
    })).collect())
}

pub fn token(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text.as_bytes()[0].is_ascii_alphanumeric()
        && text
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
}

// Deliberately supports only validation keywords/patterns used by the frozen
// request/common schemas. This is not a general JSON Schema implementation.
pub fn valid(value: &Value, schema: &Value, common: &Value) -> bool {
    if let Some(reference) = schema["$ref"].as_str() {
        let Some(pointer) = reference.strip_prefix("common.schema.json#") else {
            return false;
        };
        let Some(target) = common.pointer(pointer) else {
            return false;
        };
        if !valid(value, target, common) {
            return false;
        }
    }
    if let Some(expected) = schema.get("const") {
        if value != expected {
            return false;
        }
    }
    if let Some(values) = schema["enum"].as_array() {
        if !values.contains(value) {
            return false;
        }
    }
    if let Some(kind) = schema["type"].as_str() {
        let matches = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.as_u64().is_some() || value.as_i64().is_some(),
            "null" => value.is_null(),
            "boolean" => value.is_boolean(),
            _ => false,
        };
        if !matches {
            return false;
        }
    }
    if let Some(number) = value.as_f64() {
        if schema["minimum"].as_f64().is_some_and(|min| number < min)
            || schema["maximum"].as_f64().is_some_and(|max| number > max)
        {
            return false;
        }
    }
    if let Some(text) = value.as_str() {
        let length = text.chars().count() as u64;
        if schema["minLength"].as_u64().is_some_and(|min| length < min)
            || schema["maxLength"].as_u64().is_some_and(|max| length > max)
        {
            return false;
        }
        if let Some(pattern) = schema["pattern"].as_str() {
            let matches = match pattern {
                "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$" => token(text),
                "^sha256:[0-9a-f]{64}$" => text.strip_prefix("sha256:").is_some_and(|s| {
                    s.len() == 64
                        && s.bytes()
                            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                }),
                _ => false,
            };
            if !matches {
                return false;
            }
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            if required
                .iter()
                .any(|key| !object.contains_key(key.as_str().unwrap()))
            {
                return false;
            }
        }
        for (key, child) in object {
            if let Some(property) = schema["properties"].get(key) {
                if !valid(child, property, common) {
                    return false;
                }
            } else if schema["additionalProperties"] == false {
                return false;
            }
        }
    }
    if let Some(array) = value.as_array() {
        if schema["maxItems"]
            .as_u64()
            .is_some_and(|max| array.len() as u64 > max)
            || schema["minItems"]
                .as_u64()
                .is_some_and(|min| (array.len() as u64) < min)
        {
            return false;
        }
        for (index, item) in array.iter().enumerate() {
            if schema["uniqueItems"] == true && array[..index].contains(item) {
                return false;
            }
            if let Some(items) = schema.get("items") {
                if !valid(item, items, common) {
                    return false;
                }
            }
        }
    }
    for keyword in ["allOf", "anyOf", "oneOf"] {
        if let Some(options) = schema[keyword].as_array() {
            let count = options
                .iter()
                .filter(|option| valid(value, option, common))
                .count();
            if match keyword {
                "allOf" => count != options.len(),
                "anyOf" => count == 0,
                _ => count != 1,
            } {
                return false;
            }
        }
    }
    if let Some(condition) = schema.get("if") {
        let branch = if valid(value, condition, common) {
            "then"
        } else {
            "else"
        };
        if let Some(branch) = schema.get(branch) {
            if !valid(value, branch, common) {
                return false;
            }
        }
    }
    true
}

pub fn failure(code: &str) -> Value {
    let common = artifact("common.schema.json").unwrap();
    let diagnostic = common["$defs"]["diagnostic"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["properties"]["code"]["const"] == code)
        .expect("fixed diagnostic");
    let status = match code {
        "CG_UNSUPPORTED_VERSION" | "CG_UNSUPPORTED_CAPABILITY" => "unsupported",
        "CG_SCOPE_DENIED" => "denied",
        _ => "error",
    };
    json!({"schema_version":"1.0", "scope":null,"operation":null,"correlation":null,
        "status":status,"result":null,"explainability":[],"evidence":[],"provenance":[],
        "diagnostics":[{"code":code,"message":diagnostic["properties"]["message"]["const"],"retry":diagnostic["properties"]["retry"]["const"]}]})
}
