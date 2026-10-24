//! Qualified closed scalar-object JSON Schema profile. Unsupported semantics fail closed.
use super::Error;
use serde_json::Value;
use std::collections::BTreeSet;

/// Validate the contract independently of any supplied value, including optional fields.
pub(super) fn validate(schema: &Value) -> Result<(), Error> {
    let object = schema.as_object().ok_or(Error::Schema)?;
    let is_object = schema["type"] == "object";
    if object.keys().any(|k| {
        !matches!(k.as_str(), "type" | "description" | "title" | "$schema")
            && !(is_object
                && matches!(
                    k.as_str(),
                    "properties" | "required" | "additionalProperties"
                ))
    }) || schema
        .get("$schema")
        .is_some_and(|v| v.as_str() != Some("https://json-schema.org/draft/2020-12/schema"))
        || ["description", "title"]
            .iter()
            .any(|k| schema.get(k).is_some_and(|v| !v.is_string()))
    {
        return Err(Error::Schema);
    }
    match schema["type"].as_str() {
        Some("string" | "integer" | "boolean") => (),
        Some("object") => {
            let properties = schema["properties"].as_object().ok_or(Error::Schema)?;
            if schema["additionalProperties"] != false {
                return Err(Error::Schema);
            }
            for (name, property) in properties {
                if name.is_empty()
                    || !matches!(
                        property["type"].as_str(),
                        Some("string" | "integer" | "boolean")
                    )
                {
                    return Err(Error::Schema);
                }
                validate(property)?;
            }
            let mut names = BTreeSet::new();
            if let Some(required) = schema.get("required") {
                for name in required.as_array().ok_or(Error::Schema)? {
                    let name = name.as_str().ok_or(Error::Schema)?;
                    if !properties.contains_key(name) || !names.insert(name) {
                        return Err(Error::Schema);
                    }
                }
            }
        }
        _ => return Err(Error::Schema),
    }
    Ok(())
}
pub(super) fn accepts(schema: &Value, value: &Value) -> Result<(), Error> {
    validate(schema)?;
    let valid = match schema["type"].as_str() {
        Some("string") => value.is_string(),
        Some("integer") => value.is_i64() || value.is_u64(),
        Some("boolean") => value.is_boolean(),
        Some("object") => {
            let values = value.as_object().ok_or(Error::Schema)?;
            let properties = schema["properties"].as_object().ok_or(Error::Schema)?;
            if let Some(required) = schema.get("required") {
                for name in required.as_array().ok_or(Error::Schema)? {
                    if !values.contains_key(name.as_str().ok_or(Error::Schema)?) {
                        return Err(Error::Schema);
                    }
                }
            }
            for (key, value) in values {
                accepts(properties.get(key).ok_or(Error::Schema)?, value)?;
            }
            true
        }
        _ => false,
    };
    if valid { Ok(()) } else { Err(Error::Schema) }
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
