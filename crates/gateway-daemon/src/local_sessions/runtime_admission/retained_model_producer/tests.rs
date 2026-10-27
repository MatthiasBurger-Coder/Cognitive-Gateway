use super::*;

#[test]
fn retained_tool_data_uses_native_closed_schema_and_exact_inventory_digest() {
    let scope = ContextScopeId::new("retained-tool").unwrap();
    let binding = ResolvedReference {
        id: CapabilityId::new("records.read").unwrap(),
        scope: scope.clone(),
        contract: ReferenceId::new("cg.connector-capability").unwrap(),
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new("registered-1").unwrap(),
        digest: context_content_digest(b"actual capability declaration"),
    };
    let schema = json!({"type":"object","properties":{"object":{"type":"string"}},"required":["object"],"additionalProperties":false});
    let definition = ToolDefinition::from_connector_schema(
        binding,
        NonEmptyText::new("Read actual scoped object").unwrap(),
        gateway_application::connectors::ConnectorTarget {
            connector: NonEmptyText::new("records").unwrap(),
            scope,
        },
        context_content_digest(b"actual inventory"),
        context_content_digest(&canonical(&schema).unwrap()),
        &schema,
    )
    .unwrap();
    let expected = BTreeMap::from([(definition.binding.id.clone(), definition)]);
    let value = serde_json::to_value(&expected).unwrap();
    assert_eq!(decode_tools(value.clone()).unwrap(), expected);
    let mut wrong_digest = value.clone();
    wrong_digest["records.read"]["connector_inventory"]["argument_schema_digest"] =
        json!(context_content_digest(b"different schema"));
    assert!(decode_tools(wrong_digest).is_err());
    let mut wrong_scope = value.clone();
    wrong_scope["records.read"]["connector_inventory"]["scope"] = json!("other-project");
    assert!(decode_tools(wrong_scope).is_err());
    let mut alias = value.clone();
    alias["records.read"]["binding"]["id"] = json!("records.write");
    assert!(decode_tools(alias).is_err());
    let mut unbound = value.clone();
    unbound["records.read"]
        .as_object_mut()
        .unwrap()
        .remove("connector_inventory");
    assert!(decode_tools(unbound).is_err());
    let mut invented = value;
    invented["records.read"]["current_authority"] = json!(true);
    assert!(decode_tools(invented).is_err());
}

#[test]
fn historical_document_data_checks_real_canonical_bytes_scope_and_exact_fields() {
    let scope = ContextScopeId::new("retained-document").unwrap();
    let body = json!({"schema_version":"1.0","profile":"NATIVE_SESSION_MODEL_V1"});
    let pin = json!({"id":"model","scope":scope,"contract":"cg.connector-model","contract_version":"1.0","revision":"operator-1","digest":context_content_digest(&canonical(&body).unwrap())});
    let document = json!({"pin":pin,"body":body});
    assert!(decode_document(document.clone(), &scope).is_ok());
    let mut altered = document.clone();
    altered["body"]["profile"] = json!("invented");
    assert!(matches!(
        decode_document(altered, &scope),
        Err(SessionError::StaleRevision)
    ));
    assert!(matches!(
        decode_document(
            document.clone(),
            &ContextScopeId::new("foreign-owner").unwrap()
        ),
        Err(SessionError::StaleRevision)
    ));
    let mut extra = document;
    extra["available"] = json!(true);
    assert!(decode_document(extra, &scope).is_err());
}
