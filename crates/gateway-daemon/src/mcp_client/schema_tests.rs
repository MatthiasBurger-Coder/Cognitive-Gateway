use super::*;
use serde_json::json;
#[test]
fn strict_scalar_object_schema_rejects_unknown_required_semantics() {
    let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema", "type":"object","properties":{"a":{"type":"string"},"b":{"type":"integer"},"c":{"type":"boolean"}},"required":["a"],"additionalProperties":false});
    accepts(&schema, &json!({"a":"text","b":1,"c":true})).unwrap();
    for value in [
        json!({}),
        json!({"a":false}),
        json!({"a":"text","unknown":1}),
        json!([]),
    ] {
        assert_eq!(accepts(&schema, &value), Err(Error::Schema));
    }
    for schema in [
        json!(false),
        json!({"type":"number"}),
        json!({"type":"string","pattern":".*"}),
        json!({"type":"string","$schema":"future"}),
        json!({"type":"object","properties":{},"additionalProperties":true}),
        json!({"type":"object","properties":{},"additionalProperties":false,"required":["absent"]}),
        json!({"type":"object","properties":{},"additionalProperties":false,"required":false}),
        json!({"type":"object","properties":{},"additionalProperties":false,"required":[1]}),
        json!({"type":"object","properties":{"x":{"type":"array"}},"additionalProperties":false}),
        json!({"type":"object","additionalProperties":false}),
    ] {
        assert!(accepts(&schema, &json!({"x":[]})).is_err(), "{schema}");
    }
    accepts(
        &json!({"type":"object","properties":{},"additionalProperties":false}),
        &json!({}),
    )
    .unwrap();
}
