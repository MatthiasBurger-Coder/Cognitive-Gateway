use super::*;

#[test]
fn nested_and_escaped_duplicate_authority_fields_are_rejected() {
    for document in [
        r#"{"enabled":false,"enabled":true}"#,
        r#"{"policy":{"allow":false,"allow":true}}"#,
        r#"{"rows":[{"scope":"one","scope":"two"}]}"#,
        r#"{"allow":false,"\u0061llow":true}"#,
    ] {
        assert_eq!(parse(document), Err(SessionError::InvalidInput));
    }
}

#[test]
fn complete_definitions_preserve_values_and_refuse_trailing_input() {
    let text =
        r#"{"rows":[null,true,false,-7,18446744073709551615,1.5,"value"],"scope":"project"}"#;
    assert_eq!(
        parse(text).unwrap(),
        serde_json::from_str::<Value>(text).unwrap()
    );
    for invalid in ["{} {}", "{}x", "{", "[1,]"] {
        assert_eq!(parse(invalid), Err(SessionError::InvalidInput));
    }
}

#[test]
fn byte_and_nesting_limits_apply_before_definitions_can_be_used() {
    assert_eq!(
        parse(&" ".repeat(MAX_BYTES + 1)),
        Err(SessionError::LimitExceeded)
    );
    let allowed = format!("{}0{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    assert!(parse(&allowed).is_ok());
    let excessive = format!("[{allowed}]");
    assert_eq!(parse(&excessive), Err(SessionError::InvalidInput));
}

#[test]
fn serde_value_boundary_refuses_nonfinite_and_non_json_values_without_panics() {
    use serde::de::value::{BytesDeserializer, Error, F64Deserializer, StringDeserializer};
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            Node(0)
                .deserialize(F64Deserializer::<Error>::new(number))
                .is_err()
        );
    }
    let bytes = Node(0)
        .deserialize(BytesDeserializer::<Error>::new(b"not a JSON string"))
        .unwrap_err();
    assert!(bytes.to_string().contains("bounded JSON definition"));
    let owned = "a Unicode string: \u{00fc}".to_owned();
    assert_eq!(
        Node(0)
            .deserialize(StringDeserializer::<Error>::new(owned.clone()))
            .unwrap(),
        Value::String(owned)
    );
}
