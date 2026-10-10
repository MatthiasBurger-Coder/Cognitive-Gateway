//! No-key boundary checks. Rejections replace the entire projection with fixed
//! diagnostics; canonical content, references and digests are never rewritten.
use serde_json::Value;

/// Credential names are compared without punctuation or case. Never include a
/// rejected name/value in diagnostics. Authentication state belongs to the client.
pub fn credential_name(name: &str) -> bool {
    let name: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_uppercase)
        .collect();
    [
        "APIKEY",
        "ACCESSTOKEN",
        "REFRESHTOKEN",
        "IDTOKEN",
        "AUTHTOKEN",
        "SESSIONTOKEN",
        "CLIENTSECRET",
        "PASSWORD",
        "PRIVATEKEY",
        "SECRETACCESSKEY",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
        || matches!(
            name.as_str(),
            "CREDENTIALS" | "CODEXAUTH" | "AUTHJSON" | "GITHUBTOKEN" | "GHTOKEN" | "HFTOKEN"
        )
}

/// Recognizable credential material is refused even in identifiers/free text.
/// This supplements source classification; it cannot classify arbitrary strings.
pub fn credential_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    // Also cover assignments inside prose/encoded JSON, including duplicate
    // keys that a conventional JSON decoder could otherwise overwrite.
    text.char_indices().any(|(index, c)| {
        if c != ':' && c != '=' {
            return false;
        }
        let before = text[..index].trim_end().trim_end_matches('"');
        let name = before
            .rsplit(|c: char| !(c.is_ascii_alphanumeric() || "._-".contains(c)))
            .next()
            .unwrap_or("");
        credential_name(name) && !text[index + 1..].trim().is_empty()
    }) || ["bearer ", "basic "].iter().any(|scheme| {
        lower.match_indices(scheme).any(|(index, _)| {
            (index == 0 || !lower.as_bytes()[index - 1].is_ascii_alphanumeric()) && {
                let length = lower[index + scheme.len()..]
                    .bytes()
                    .take_while(|b| b.is_ascii_alphanumeric() || b"._-/+=".contains(b))
                    .count();
                if *scheme == "basic " {
                    length >= 16 && length % 4 == 0
                } else {
                    length >= 12
                }
            }
        })
    }) || lower.contains("-----begin private key-----")
        || lower.contains("-----begin rsa private key-----")
        || lower.contains("-----begin ec private key-----")
        || text.match_indices("sk-").any(|(index, _)| {
            (index == 0 || !text.as_bytes()[index - 1].is_ascii_alphanumeric())
                && text[index + 3..]
                    .bytes()
                    .take_while(|b| b.is_ascii_alphanumeric() || b"_-".contains(b))
                    .count()
                    >= 16
        })
        || text
            .split(|c: char| !(c.is_ascii_alphanumeric() || "._-".contains(c)))
            .any(|word| {
                let parts: Vec<_> = word.split('.').collect();
                parts.len() == 3
                    && parts[0].starts_with("eyJ")
                    && parts.iter().all(|p| p.len() >= 8)
            })
}

/// Iterative and bounded, including JSON object keys and escaped JSON strings.
pub fn credential_free(value: &Value) -> bool {
    check(value, 0)
}
fn check(value: &Value, encoded_depth: usize) -> bool {
    if encoded_depth > 8 {
        return false;
    }
    let mut pending = vec![(value, 0)];
    let mut nodes = 0;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if depth > 64 || nodes > 200_000 {
            return false;
        }
        match value {
            Value::Object(object) => {
                if object
                    .keys()
                    .any(|key| credential_name(key) || credential_text(key))
                {
                    return false;
                }
                pending.extend(object.values().map(|v| (v, depth + 1)));
            }
            Value::Array(values) => pending.extend(values.iter().map(|v| (v, depth + 1))),
            Value::String(text) => {
                if text.len() >= 1_048_576 || credential_text(text) {
                    return false;
                }
                // Encoded objects/arrays are refused wholesale when they carry
                // credential fields. They never bypass the boundary as opaque text.
                if text.trim_start().starts_with(['{', '[']) {
                    if let Ok(decoded) = serde_json::from_str::<Value>(text) {
                        if !check(&decoded, encoded_depth + 1) {
                            return false;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    true
}

/// Explicitly classified payloads cannot become inline client content. Apply to
/// documents, not provenance: SECRET metadata may accompany an opaque reference.
pub fn inline_allowed(document: &Value) -> bool {
    let mut pending = vec![(document, 0)];
    let mut nodes = 0;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if depth > 64 || nodes > 200_000 {
            return false;
        }
        match value {
            Value::Object(object) => {
                if object.get("sensitivity").is_some_and(|v| v == "SECRET")
                    || object.get("reference_only").is_some_and(|v| v == true)
                {
                    return false;
                }
                pending.extend(object.values().map(|v| (v, depth + 1)));
            }
            Value::Array(values) => pending.extend(values.iter().map(|v| (v, depth + 1))),
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn credentials_in_fields_free_text_keys_and_encoded_json_fail_closed() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/codex-security/credentials.json"
        ))
        .unwrap();
        for name in fixture["names"].as_array().unwrap() {
            let name = name.as_str().unwrap();
            assert!(credential_name(name), "{name}");
            assert!(!credential_free(&json!({name:"OPAQUE_FAKE_CREDENTIAL"})));
        }
        for value in fixture["values"].as_array().unwrap() {
            assert!(credential_text(value.as_str().unwrap()));
            assert!(!credential_free(&json!({"nested":[{"note":value}]})));
            assert!(!credential_free(&json!({value.as_str().unwrap():true})));
        }
        assert!(!credential_free(
            &json!({"Authorization":"Basic RkFLRV9DUkVERU5USUFMOjAxMjM="})
        ));
        assert!(!credential_free(
            &json!({"text":"{\"api_key\":\"opaque\"}"})
        ));
        assert!(credential_text("prefix OPENAI_API_KEY=opaque suffix"));
        assert!(!credential_free(
            &json!({"text":"{\"x\":{\"api_key\":\"opaque\"},\"x\":{}}"})
        ));
        assert!(credential_free(
            &json!({"authorization":{"decision":"allow"},"reference":{"id":"task-1"},
            "provenance":[{"sensitivity":"CONFIDENTIAL"}],"text":"[broken", "json":"{\"ok\":true}", "values":[null,false,3]})
        ));
        assert!(!credential_text("sk-short is a task identifier"));
        assert!(!credential_text("task-architecture-analysis"));
        assert!(!credential_text(
            "Basic functionality and bearer tokens are described here."
        ));
    }

    #[test]
    fn scans_are_bounded_and_classified_documents_remain_opaque() {
        let mut deep = json!(null);
        for _ in 0..66 {
            deep = json!([deep]);
        }
        assert!(!credential_free(&deep));
        assert!(!inline_allowed(&deep));
        let wide = Value::Array(vec![Value::Null; 200_001]);
        assert!(!credential_free(&wide));
        assert!(!inline_allowed(&wide));
        assert!(!credential_free(&json!("x".repeat(1_048_576))));
        let mut encoded = json!({"ok":true});
        for _ in 0..10 {
            encoded = json!([encoded.to_string()]);
        }
        assert!(!credential_free(&encoded));
        for classification in [
            json!({"sensitivity":"SECRET","text":"OPAQUE"}),
            json!({"reference_only":true,"text":"OPAQUE"}),
        ] {
            assert!(!inline_allowed(&json!({"nested":[classification]})));
        }
        assert!(inline_allowed(
            &json!({"reference_only":false,"sensitivity":"CONFIDENTIAL","text":"allowed"})
        ));
    }
}
