use super::*;

fn binding(scope: &ContextScopeId) -> ResolvedReference<CapabilityId> {
    ResolvedReference {
        id: CapabilityId::new("connector.records.read").unwrap(),
        scope: scope.clone(),
        contract: ReferenceId::new("connector-capability").unwrap(),
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new("1").unwrap(),
        digest: ContentDigest::new("a".repeat(64)).unwrap(),
    }
}

#[test]
fn mapped_connector_schema_becomes_closed_invocation_schema_with_inventory_pin() {
    let scope = ContextScopeId::new("project").unwrap();
    let target = crate::connectors::ConnectorTarget {
        connector: NonEmptyText::new("connector").unwrap(),
        scope: scope.clone(),
    };
    let schema = json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "title":"Read records",
        "type":"object",
        "description":"Query the connector",
        "properties":{
            "query":{"type":"string","description":"Search phrase"},
            "limit":{"type":"integer"}
        },
        "required":["query"],
        "additionalProperties":false
    });
    let input = parse_connector_input_schema(&schema).unwrap();
    let schema_digest = context_content_digest(&serde_json::to_vec(&schema).unwrap());
    let definition = ToolDefinition::from_connector_schema(
        binding(&scope),
        NonEmptyText::new("Read records").unwrap(),
        target,
        ContentDigest::new("b".repeat(64)).unwrap(),
        schema_digest.clone(),
        &schema,
    )
    .unwrap();
    assert_eq!(definition.input, input);
    let pin = definition.connector_inventory.unwrap();
    assert_eq!(pin.revision.as_str(), "b".repeat(64));
    assert_eq!(pin.argument_schema_digest, schema_digest);
    assert_eq!(pin.argument_schema, schema);
    assert_ne!(
        pin.argument_schema_digest,
        context_content_digest(&serde_json::to_vec(&input.json_schema()).unwrap()),
        "descriptive wire metadata remains part of the content pin",
    );
}

#[test]
fn connector_schema_conversion_rejects_semantic_drift_and_unsupported_keywords() {
    let scope = ContextScopeId::new("project").unwrap();
    let target = crate::connectors::ConnectorTarget {
        connector: NonEmptyText::new("connector").unwrap(),
        scope: scope.clone(),
    };
    let schema = json!({
        "type":"object", "properties":{"query":{"type":"string"}},
        "required":["query"], "additionalProperties":false
    });
    assert_eq!(
        ToolDefinition::from_connector_schema(
            binding(&scope),
            NonEmptyText::new("Read records").unwrap(),
            target.clone(),
            ContentDigest::new("b".repeat(64)).unwrap(),
            ContentDigest::new("c".repeat(64)).unwrap(),
            &schema,
        )
        .unwrap_err(),
        InvocationIrError::InvalidBinding
    );
    let unsupported = json!({
        "type":"object", "properties":{"query":{"type":"string","enum":["a"]}},
        "additionalProperties":false
    });
    let digest = context_content_digest(&serde_json::to_vec(&unsupported).unwrap());
    assert_eq!(
        ToolDefinition::from_connector_schema(
            binding(&scope),
            NonEmptyText::new("Read records").unwrap(),
            target,
            ContentDigest::new("b".repeat(64)).unwrap(),
            digest,
            &unsupported,
        )
        .unwrap_err(),
        InvocationIrError::InvalidBinding
    );
}

#[test]
fn connector_schema_validation_rejects_scope_shape_required_and_property_drift() {
    let scope = ContextScopeId::new("project").unwrap();
    let other_scope = ContextScopeId::new("other-project").unwrap();
    let target = crate::connectors::ConnectorTarget {
        connector: NonEmptyText::new("connector").unwrap(),
        scope: other_scope,
    };
    let boolean_schema = json!({
        "type":"object",
        "properties":{"enabled":{"type":"boolean"}},
        "required":["enabled"],
        "additionalProperties":false
    });
    let boolean_digest = context_content_digest(&serde_json::to_vec(&boolean_schema).unwrap());
    assert_eq!(
        ToolDefinition::from_connector_schema(
            binding(&scope),
            NonEmptyText::new("Toggle records").unwrap(),
            target.clone(),
            ContentDigest::new("b".repeat(64)).unwrap(),
            boolean_digest.clone(),
            &boolean_schema,
        )
        .unwrap_err(),
        InvocationIrError::InvalidBinding
    );

    let target = crate::connectors::ConnectorTarget {
        connector: NonEmptyText::new("connector").unwrap(),
        scope: scope.clone(),
    };
    let definition = ToolDefinition::from_connector_schema(
        binding(&scope),
        NonEmptyText::new("Toggle records").unwrap(),
        target,
        ContentDigest::new("b".repeat(64)).unwrap(),
        boolean_digest,
        &boolean_schema,
    )
    .unwrap();
    assert_eq!(
        definition.input.json_schema()["properties"]["enabled"]["type"],
        "boolean"
    );

    for invalid in [
        json!({"type":"array","items":{"type":"string"}}),
        json!({"type":"object","properties":{"query":{"type":"string"}},"required":["missing"],"additionalProperties":false}),
        json!({"type":"object","properties":{"":{"type":"string"}},"additionalProperties":false}),
        json!({"type":"object","properties":{"query":{"type":"array"}},"additionalProperties":false}),
    ] {
        assert_eq!(
            parse_connector_input_schema(&invalid),
            Err(InvocationIrError::InvalidBinding),
            "schema should be rejected: {invalid}"
        );
    }
}
#[test]
fn unsupported_schema_semantics_are_rejected_instead_of_dropped() {
    let valid = json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false});
    let mut unsupported = Vec::new();
    for (key, value) in [
        ("pattern", json!(".*")),
        ("minLength", json!(1)),
        ("enum", json!(["only"])),
        ("format", json!("uri")),
        ("default", json!("fallback")),
    ] {
        let mut schema = valid.clone();
        schema["properties"]["query"][key] = value;
        unsupported.push(schema);
    }
    for (key, value) in [
        ("$schema", json!("unknown")),
        ("description", json!(null)),
        ("title", json!(false)),
        ("required", json!(["query", "query"])),
        ("required", json!([42])),
        ("required", json!(true)),
        ("properties", json!([])),
        ("additionalProperties", json!(true)),
        ("oneOf", json!([])),
    ] {
        let mut schema = valid.clone();
        schema[key] = value;
        unsupported.push(schema);
    }
    unsupported.extend([json!(null), json!([])]);
    for schema in unsupported {
        assert_eq!(
            parse_connector_input_schema(&schema),
            Err(InvocationIrError::InvalidBinding),
            "{schema}"
        );
    }
    let optional = json!({"type":"object", "properties":{"count":{"type":"integer"}}, "additionalProperties":false});
    let parsed = parse_connector_input_schema(&optional).unwrap();
    assert!(parsed.accepts(&json!({})));
    assert!(parsed.accepts(&json!({"count":42})));
    assert!(!parsed.accepts(&json!({"count":"42"})));
    assert!(!parsed.accepts(&json!({"extra":42})));
}

#[test]
fn constructor_preserves_wire_variants_and_rejects_raw_or_native_type_tampering() {
    let scope = ContextScopeId::new("project").unwrap();
    let target = crate::connectors::ConnectorTarget {
        connector: NonEmptyText::new("connector").unwrap(),
        scope: scope.clone(),
    };
    let schema = json!({"type":"object", "properties":{"query":{"type":"string"}, "limit":{"type":"integer"}}, "required":["query","limit"], "additionalProperties":false});
    let mut variants = vec![schema.clone()];
    let mut metadata = schema.clone();
    metadata["title"] = json!("Read two fields");
    metadata["properties"]["query"]["description"] = json!("Exact query");
    variants.push(metadata);
    let mut reordered = schema.clone();
    reordered["required"] = json!(["limit", "query"]);
    variants.push(reordered);
    let mut optional = schema.clone();
    optional.as_object_mut().unwrap().remove("required");
    variants.push(optional);
    for (i, raw) in variants.into_iter().enumerate() {
        let digest = context_content_digest(&serde_json::to_vec(&raw).unwrap());
        let definition = ToolDefinition::from_connector_schema(
            binding(&scope),
            NonEmptyText::new("Read").unwrap(),
            target.clone(),
            context_content_digest(b"inventory"),
            digest.clone(),
            &raw,
        )
        .unwrap();
        assert_eq!(
            definition
                .connector_inventory
                .as_ref()
                .unwrap()
                .argument_schema,
            raw
        );
        assert_eq!(
            definition
                .connector_inventory
                .as_ref()
                .unwrap()
                .argument_schema_digest,
            digest
        );
        assert_eq!(definition.validate_connector_binding(), Ok(()));
        assert_eq!(definition.input.accepts(&json!({})), i == 3);
        assert!(
            definition
                .input
                .accepts(&json!({"query":"exact", "limit":7}))
        );
        let mut changed = definition.clone();
        changed
            .connector_inventory
            .as_mut()
            .unwrap()
            .argument_schema["title"] = json!("changed wire content");
        assert_eq!(
            changed.validate_connector_binding(),
            Err(InvocationIrError::InvalidBinding)
        );
        let mut changed = definition;
        changed.input = OutputType::Boolean {};
        assert_eq!(
            changed.validate_connector_binding(),
            Err(InvocationIrError::InvalidBinding)
        );
    }
}
