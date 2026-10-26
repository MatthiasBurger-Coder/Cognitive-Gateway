use super::*;

#[test]
fn canonical_argument_conversion_rejects_non_object_and_nested_values() {
    assert_eq!(
        typed_arguments(&Value::Array(vec![])),
        Err(ProposalError::InvalidProposal)
    );
    assert_eq!(
        typed_arguments(&serde_json::json!({"nested": {"value": true}})),
        Err(ProposalError::InvalidProposal)
    );
}

#[test]
fn canonical_request_projection_rejects_decimal_and_collection_values() {
    let key = NonEmptyText::new("value").unwrap();
    let decimal = std::collections::BTreeMap::from([(
        key.clone(),
        TypedValue::Decimal(gateway_domain::DecimalValue::new(1, 1).unwrap()),
    )]);
    let set =
        std::collections::BTreeMap::from([(key, TypedValue::Set(vec![TypedValue::Integer(1)]))]);
    assert_eq!(
        arguments_json(&decimal),
        Err(ProposalError::InvalidProposal)
    );
    assert_eq!(arguments_json(&set), Err(ProposalError::InvalidProposal));
}
