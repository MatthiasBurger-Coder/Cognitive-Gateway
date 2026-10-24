//! Deliberately small qualified JSON Schema profile. Unknown semantics fail closed;
//! adding a dialect/keyword requires explicit mapping qualification (#227).
use super::Error;
use serde_json::Value;

pub(super) fn accepts(schema: &Value, value: &Value) -> Result<(), Error> {
    let object = schema.as_object().ok_or(Error::Schema)?;
    if object.keys().any(|k| {
        !matches!(
            k.as_str(),
            "type"
                | "properties"
                | "required"
                | "additionalProperties"
                | "description"
                | "title"
                | "$schema"
        )
    }) || schema
        .get("$schema")
        .is_some_and(|v| v.as_str() != Some("https://json-schema.org/draft/2020-12/schema"))
    {
        return Err(Error::Schema);
    }
    let valid = match schema["type"].as_str() {
        Some("string") => value.is_string(),
        Some("integer") => value.is_i64() || value.is_u64(),
        Some("boolean") => value.is_boolean(),
        Some("object") => {
            let values = value.as_object().ok_or(Error::Schema)?;
            let properties = schema["properties"].as_object().ok_or(Error::Schema)?;
            // No open-ended argument/result bags in the initial qualified profile.
            if schema["additionalProperties"] != false {
                return Err(Error::Schema);
            }
            if let Some(required) = schema.get("required") {
                for name in required.as_array().ok_or(Error::Schema)? {
                    let name = name.as_str().ok_or(Error::Schema)?;
                    if !properties.contains_key(name) || !values.contains_key(name) {
                        return Err(Error::Schema);
                    }
                }
            }
            for property in properties.values() {
                if !matches!(
                    property["type"].as_str(),
                    Some("string" | "integer" | "boolean")
                ) || property.as_object().is_none_or(|p| {
                    p.keys()
                        .any(|k| !matches!(k.as_str(), "type" | "description" | "title"))
                }) {
                    return Err(Error::Schema);
                }
            }
            for (key, value) in values {
                let property = properties.get(key).ok_or(Error::Schema)?;
                accepts(property, value)?;
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
