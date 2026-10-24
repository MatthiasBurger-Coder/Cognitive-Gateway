use super::*;
use gateway_domain::*;
use std::collections::BTreeMap;

const DOC: &[u8] = include_bytes!("../../../../tests/fixtures/connector-mapping-v1/mapping.json");
const MANIFEST: &[u8] =
    include_bytes!("../../../../tests/fixtures/connector-mapping-v1/manifest.json");
const DISCOVERY: &[u8] =
    include_bytes!("../../../../tests/fixtures/connector-mapping-v1/discovery.json");
fn target() -> ConnectorTarget {
    ConnectorTarget {
        connector: NonEmptyText::new("example.records").unwrap(),
        scope: ContextScopeId::new("scope-one").unwrap(),
    }
}
fn wire() -> ProtocolInventory {
    let v: Value = serde_json::from_slice(DISCOVERY).unwrap();
    ProtocolInventory {
        tools: v["tools"].as_array().unwrap().clone(),
        resources: v["resources"].as_array().unwrap().clone(),
        templates: v["templates"].as_array().unwrap().clone(),
        prompts: v["prompts"].as_array().unwrap().clone(),
    }
}
fn doc(v: &Value) -> Result<MappingDocument, MappingError> {
    MappingDocument::from_json(&serde_json::to_vec(v).unwrap())
}
fn document() -> Value {
    serde_json::from_slice(DOC).unwrap()
}
fn mapper() -> DiscoveryMapper {
    DiscoveryMapper::new(
        target(),
        ConnectorManifest::from_json(MANIFEST).unwrap(),
        MappingDocument::from_json(DOC).unwrap(),
    )
    .unwrap()
}
fn rebind(mapper: &mut DiscoveryMapper, wire: &ProtocolInventory) {
    let mut v = serde_json::to_value(mapper.manifest.definition()).unwrap();
    for e in v["inventory"].as_array_mut().unwrap() {
        let kind: InventoryKind = serde_json::from_value(e["kind"].clone()).unwrap();
        let list = match kind {
            InventoryKind::Tool => &wire.tools,
            InventoryKind::Resource => &wire.resources,
            InventoryKind::Prompt => &wire.prompts,
        };
        if let Some(item) = list.iter().find(|i| i["name"] == e["name"]) {
            e["schema_digest"] = json!(discovery_schema_digest(kind, item));
        }
    }
    mapper.manifest =
        ConnectorManifest::from_definition_json(&serde_json::to_vec(&v).unwrap()).unwrap();
}
fn dispatch(m: &DiscoveryMapper, wire: &ProtocolInventory, resource: bool) -> ConnectorDispatch {
    ConnectorDispatch {
        request: ConnectorRequest {
            target: target(),
            capability: CapabilityId::new(if resource {
                "records.resource.read"
            } else {
                "records.read"
            })
            .unwrap(),
            class: ConnectorOperationClass::Read,
            inventory_revision: m.snapshot(wire).unwrap().inventory.revision,
            arguments: if resource {
                BTreeMap::new()
            } else {
                [(
                    NonEmptyText::new("query").unwrap(),
                    TypedValue::string("hello").unwrap(),
                )]
                .into()
            },
            task: TaskId::new("task").unwrap(),
            step: PlanStepId::new("step").unwrap(),
            execution: NonEmptyText::new("exec").unwrap(),
            correlation: NonEmptyText::new("correlation").unwrap(),
        },
        policy: PolicyId::new("policy").unwrap(),
        policy_revision: ContentDigest::new("a".repeat(64)).unwrap(),
        consent_receipt: None,
        budget_reservation: NonEmptyText::new("budget").unwrap(),
        deadline: UnixTimestamp::new(100),
    }
}
#[test]
fn golden_snapshot_and_manifest_bundle_digests() {
    let m = mapper();
    let w = wire();
    let s = m.snapshot(&w).unwrap();
    assert_eq!(s.entries().len(), 2);
    assert_eq!(
        s.inventory().resources,
        vec![SourceId::new("records.one").unwrap()]
    );
    assert_eq!(
        s.entries()[0].mapping.capability_definition().class(),
        CapabilityClass::Inspect
    );
    assert_eq!(
        s.canonical_json(),
        include_bytes!("../../../../tests/fixtures/connector-mapping-v1/snapshot.json")
    );
    assert_eq!(
        s.inventory().revision.as_str(),
        include_str!("../../../../tests/fixtures/connector-mapping-v1/revision.txt").trim()
    );
    assert_eq!(
        MappingDocument::from_json(&m.document.canonical_json())
            .unwrap()
            .digest(),
        m.document.digest()
    );
}
#[test]
fn permutations_are_stable_and_changes_invalidate_the_snapshot() {
    let mut m = mapper();
    let mut w = wire();
    // Add independent entries so each complete discovery list can be permuted.
    let mut v = document();
    let mut r = v["records"][0].clone();
    r["reference"] = json!("mapping.second.v1");
    r["name"] = json!("second");
    r["capability"] = json!("records.second");
    v["records"].as_array_mut().unwrap().push(r);
    m.document = doc(&v).unwrap();
    let mut second = w.tools[0].clone();
    second["name"] = json!("second");
    w.tools.push(second);
    let mut definition = serde_json::to_value(m.manifest.definition()).unwrap();
    let mut e = definition["inventory"][0].clone();
    e["name"] = json!("second");
    e["capability_mapping_ref"] = json!("mapping.second.v1");
    definition["inventory"].as_array_mut().unwrap().push(e);
    m.manifest =
        ConnectorManifest::from_definition_json(&serde_json::to_vec(&definition).unwrap()).unwrap();
    rebind(&mut m, &w);
    let before = m.snapshot(&w).unwrap();
    w.tools.reverse();
    let mut reversed = v.clone();
    reversed["records"].as_array_mut().unwrap().reverse();
    m.document = doc(&reversed).unwrap();
    assert_eq!(
        before.canonical_json(),
        m.snapshot(&w).unwrap().canonical_json()
    );
    w.templates
        .push(json!({"name":"other","uriTemplate":"records://other/{id}"}));
    let changed = m.snapshot(&w).unwrap();
    assert_ne!(before.inventory.revision, changed.inventory.revision);
    w.templates.reverse();
    assert_eq!(
        changed.canonical_json(),
        m.snapshot(&w).unwrap().canonical_json()
    );
    let original = mapper().snapshot(&wire()).unwrap();
    for (field, value) in [
        ("consent", json!("required")),
        ("retry_safety", json!("never")),
        ("idempotency", json!("non_idempotent")),
        ("capability", json!("renamed.capability")),
    ] {
        let mut m = mapper();
        let mut d = document();
        d["records"][0][field] = value;
        m.document = doc(&d).unwrap();
        assert_ne!(
            m.snapshot(&wire()).unwrap().inventory.revision,
            original.inventory.revision
        );
    }
    let mut scoped = mapper();
    scoped.target.scope = ContextScopeId::new("other").unwrap();
    assert_ne!(
        scoped.snapshot(&wire()).unwrap().inventory.revision,
        original.inventory.revision
    );
}
#[test]
fn strict_document_parser_and_mapping_conflicts_fail_closed() {
    for bytes in [
        b"[]".as_slice(),
        b"{} trailing",
        b"{\"mapping_version\":\"1.0\",\"mapping_version\":\"1.0\",\"records\":[]}",
        b"{\"mapping_version\":\"1.0\",\"records\":[],\"secret\":\"CANARY\"}",
    ] {
        assert_eq!(
            MappingDocument::from_json(bytes).unwrap_err(),
            MappingError::Encoding
        );
    }
    assert_eq!(
        MappingDocument::from_json(&vec![b' '; MAX_MAPPING_BYTES + 1]).unwrap_err(),
        MappingError::Encoding
    );
    let mut v = document();
    v["mapping_version"] = json!("2.0");
    assert_eq!(doc(&v).unwrap_err(), MappingError::Version);
    let mut v = document();
    v["records"] = json!(vec![v["records"][0].clone(); 257]);
    assert_eq!(doc(&v).unwrap_err(), MappingError::Metadata);
    let mut v = document();
    let r = v["records"][0].clone();
    v["records"].as_array_mut().unwrap().push(r);
    assert_eq!(doc(&v).unwrap_err(), MappingError::DuplicateMapping);
    for field in ["capability", "source"] {
        let mut v = document();
        let mut r = v["records"][1].clone();
        r["reference"] = json!("mapping.other.v1");
        r["name"] = json!("other");
        r["capability"] = json!("other.capability");
        r[field] = v["records"][1][field].clone();
        v["records"].as_array_mut().unwrap().push(r);
        assert_eq!(doc(&v).unwrap_err(), MappingError::ConflictingMapping);
    }
    for (field, value) in [
        ("reference", json!("Invalid")),
        ("name", json!("")),
        ("side_effects", json!("mutation")),
        ("idempotency", json!("provider_key")),
        ("idempotency_key", json!("key")),
        ("kind", json!("prompt")),
        ("result_schema", Value::Null),
    ] {
        let mut v = document();
        v["records"][0][field] = value;
        assert!(doc(&v).is_err(), "{field}");
    }
    let mut v = document();
    v["records"][0]["result_schema"] = json!({"type":"string"});
    assert_eq!(doc(&v).unwrap_err(), MappingError::Schema);
    for (field, value) in [
        ("source", Value::Null),
        ("class", json!("SEARCH")),
        ("result_schema", json!({})),
        ("idempotency_key", json!("key")),
    ] {
        let mut v = document();
        v["records"][1][field] = value;
        assert!(doc(&v).is_err(), "{field}");
    }
}
#[test]
fn all_operation_classes_keep_explicit_effects_and_retry_metadata() {
    for (class, effect, expected) in [
        (
            OperationClass::Read,
            SideEffects::None,
            CapabilityClass::Inspect,
        ),
        (
            OperationClass::Search,
            SideEffects::None,
            CapabilityClass::Inspect,
        ),
        (
            OperationClass::Inspect,
            SideEffects::None,
            CapabilityClass::Inspect,
        ),
        (
            OperationClass::Mutate,
            SideEffects::Mutation,
            CapabilityClass::Mutate,
        ),
        (
            OperationClass::Admin,
            SideEffects::Administrative,
            CapabilityClass::Mutate,
        ),
    ] {
        let mut v = document();
        v["records"][0]["class"] = json!(class);
        v["records"][0]["side_effects"] = json!(effect);
        v["records"][0]["retry_safety"] = json!("never");
        let d = doc(&v).unwrap();
        let r = &d.records()[0];
        assert_eq!(r.capability_definition().class(), expected);
        let _ = class.operation_class();
        if expected == CapabilityClass::Mutate {
            v["records"][0]["retry_safety"] = json!("read_only");
            assert_eq!(doc(&v).unwrap_err(), MappingError::Metadata);
        }
    }
    let mut v = document();
    v["records"][0]["retry_safety"] = json!("idempotent");
    v["records"][0]["idempotency"] = json!("non_idempotent");
    assert_eq!(doc(&v).unwrap_err(), MappingError::Metadata);
}
#[test]
fn complete_discovery_rejects_unknown_duplicate_missing_and_schema_drift() {
    let m = mapper();
    let v: Value = serde_json::from_slice(include_bytes!(
        "../../../../tests/fixtures/connector-mapping-v1/unknown-tool.json"
    ))
    .unwrap();
    let mut w = wire();
    w.tools = v["tools"].as_array().unwrap().clone();
    assert_eq!(m.snapshot(&w).unwrap_err(), MappingError::UnknownEntry);
    let mut w = wire();
    w.tools.clear();
    assert_eq!(m.snapshot(&w).unwrap_err(), MappingError::MissingEntry);
    let mut w = wire();
    w.tools[0]["inputSchema"]["properties"]["query"]["type"] = json!("boolean");
    assert_eq!(m.snapshot(&w).unwrap_err(), MappingError::SchemaDigest);
    for kind in 0..4 {
        let mut w = wire();
        let list = match kind {
            0 => &mut w.tools,
            1 => &mut w.resources,
            2 => &mut w.prompts,
            _ => &mut w.templates,
        };
        list.push(list[0].clone());
        assert_eq!(m.snapshot(&w).unwrap_err(), MappingError::Discovery);
    }
    for kind in 0..4 {
        let mut w = wire();
        match kind {
            0 => w.tools[0]["name"] = json!(null),
            1 => w.resources[0]["uri"] = json!(""),
            2 => w.templates[0]["uriTemplate"] = json!(null),
            _ => w.templates[0]["name"] = json!(false),
        };
        assert_eq!(m.snapshot(&w).unwrap_err(), MappingError::Discovery);
    }
    let mut w = wire();
    w.templates = vec![json!({"name":"a","uriTemplate":"a"}); 1025];
    assert_eq!(m.snapshot(&w).unwrap_err(), MappingError::Discovery);
    let mut w = wire();
    w.tools[0]["description"] = json!("a".repeat(4_194_305));
    assert_eq!(m.snapshot(&w).unwrap_err(), MappingError::Discovery);
    let mut wrong = target();
    wrong.connector = NonEmptyText::new("other").unwrap();
    assert_eq!(
        DiscoveryMapper::new(wrong, m.manifest.clone(), m.document.clone()).err(),
        Some(MappingError::Target)
    );
}
#[test]
fn manifest_references_and_schema_contracts_are_resolved_exactly() {
    for field in ["reference", "name"] {
        let mut m = mapper();
        let mut v = document();
        v["records"][0][field] = json!("other");
        m.document = doc(&v).unwrap();
        assert_eq!(
            m.snapshot(&wire()).unwrap_err(),
            MappingError::UnresolvedReference
        );
    }
    let mut m = mapper();
    let mut v = document();
    v["records"].as_array_mut().unwrap().pop();
    m.document = doc(&v).unwrap();
    assert_eq!(
        m.snapshot(&wire()).unwrap_err(),
        MappingError::UnresolvedReference
    );
    let mut m = mapper();
    let mut v = document();
    v["records"][0]["source"] = json!("extra.source");
    m.document = doc(&v).unwrap();
    assert_eq!(
        m.snapshot(&wire()).unwrap_err(),
        MappingError::UnresolvedReference
    );
    for schema in [
        json!({"type":"object","properties":{"optional":{"type":"array"}},"additionalProperties":false}),
        json!({"type":"object","properties":{},"additionalProperties":true}),
    ] {
        let mut m = mapper();
        let mut w = wire();
        w.tools[0]["inputSchema"] = schema;
        rebind(&mut m, &w);
        assert_eq!(m.snapshot(&w).unwrap_err(), MappingError::Schema);
    }
    let mut m = mapper();
    let mut w = wire();
    w.tools[0]["outputSchema"] =
        json!({"type":"object","properties":{},"additionalProperties":false});
    rebind(&mut m, &w);
    assert_eq!(m.snapshot(&w).unwrap_err(), MappingError::Schema);
    // Extra unused records cannot become an executable partial inventory.
    let mut m = mapper();
    let mut v = document();
    let mut r = v["records"][0].clone();
    r["name"] = json!("extra");
    r["reference"] = json!("extra.v1");
    r["capability"] = json!("extra.capability");
    v["records"].as_array_mut().unwrap().push(r);
    m.document = doc(&v).unwrap();
    assert_eq!(
        m.snapshot(&wire()).unwrap_err(),
        MappingError::UnresolvedReference
    );
}
#[test]
fn provider_idempotency_keys_must_be_required_strings_and_do_not_enable_retries() {
    let mut m = mapper();
    let mut v = document();
    v["records"][0]["idempotency"] = json!("provider_key");
    v["records"][0]["idempotency_key"] = json!("query");
    v["records"][0]["retry_safety"] = json!("idempotent");
    m.document = doc(&v).unwrap();
    let w = wire();
    let s = m.snapshot(&w).unwrap();
    assert_eq!(s.entries()[0].mapping.idempotency, Idempotency::ProviderKey);
    let mut d = dispatch(&m, &w, false);
    d.request.arguments.clear();
    assert_eq!(m.resolve(&d, None, &w), Err(MappingError::Request));
    for change_type in [true, false] {
        let mut w = wire();
        if change_type {
            w.tools[0]["inputSchema"]["properties"]["query"]["type"] = json!("boolean");
        } else {
            w.tools[0]["inputSchema"]["required"] = json!([]);
        }
        rebind(&mut m, &w);
        assert_eq!(m.snapshot(&w).unwrap_err(), MappingError::Schema);
    }
}
#[test]
fn resolver_binds_target_revision_class_capability_source_and_exact_arguments() {
    let m = mapper();
    let w = wire();
    let d = dispatch(&m, &w, false);
    assert_eq!(
        m.resolve(&d, None, &w).unwrap(),
        WireOperation::Tool {
            name: "records.read".into(),
            arguments: d.request.arguments.clone()
        }
    );
    for field in 0..7 {
        let mut d = d.clone();
        match field {
            0 => d.request.target.scope = ContextScopeId::new("other").unwrap(),
            1 => d.request.inventory_revision = ContentDigest::new("b".repeat(64)).unwrap(),
            2 => d.request.class = ConnectorOperationClass::Mutate,
            3 => d.request.capability = CapabilityId::new("unknown").unwrap(),
            4 => {
                d.request.arguments.clear();
            }
            5 => {
                d.request.arguments.insert(
                    NonEmptyText::new("extra").unwrap(),
                    TypedValue::Boolean(true),
                );
            }
            _ => {
                d.request.arguments.insert(
                    NonEmptyText::new("query").unwrap(),
                    TypedValue::Boolean(true),
                );
            }
        }
        assert!(m.resolve(&d, None, &w).is_err(), "{field}");
    }
    let source = SourceId::new("records.one").unwrap();
    assert_eq!(m.resolve(&d, Some(&source), &w), Err(MappingError::Request));
    let mut d = dispatch(&m, &w, true);
    assert_eq!(
        m.resolve(&d, Some(&source), &w).unwrap(),
        WireOperation::Resource {
            uri: "records://one".into()
        }
    );
    assert_eq!(m.resolve(&d, None, &w), Err(MappingError::Request));
    assert_eq!(
        m.resolve(&d, Some(&SourceId::new("other").unwrap()), &w),
        Err(MappingError::Request)
    );
    d.request.arguments.insert(
        NonEmptyText::new("uri").unwrap(),
        TypedValue::string("records://other").unwrap(),
    );
    assert_eq!(m.resolve(&d, Some(&source), &w), Err(MappingError::Request));
}
struct RejectNormalizer;
impl ConnectorResultNormalizer for RejectNormalizer {
    fn normalize(
        &self,
        _: &ConnectorDispatch,
        _: Option<&SourceId>,
        _: &MappingRecord,
        _: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        Err(Error::Unsupported)
    }
}
#[test]
fn host_adapter_requires_normalizer_and_validates_results_even_without_advertised_schema() {
    let m = mapper();
    let w = wire();
    let d = dispatch(&m, &w, false);
    let adapter = CanonicalConnectorMapping::new(m.clone(), Box::new(RejectNormalizer));
    assert_eq!(
        adapter.inventory(&target(), &m.manifest, &w).unwrap(),
        m.snapshot(&w).unwrap().inventory
    );
    let mut other = target();
    other.scope = ContextScopeId::new("other").unwrap();
    assert_eq!(
        adapter.inventory(&other, &m.manifest, &w),
        Err(Error::Configuration)
    );
    let mut other_manifest = serde_json::to_value(m.manifest.definition()).unwrap();
    other_manifest["configuration_ref"] = json!("other.config");
    let other_manifest =
        ConnectorManifest::from_definition_json(&serde_json::to_vec(&other_manifest).unwrap())
            .unwrap();
    assert_eq!(
        adapter.inventory(&target(), &other_manifest, &w),
        Err(Error::Configuration)
    );
    assert!(adapter.operation(&d, None, &w).is_ok());
    let mut stale = d.clone();
    stale.request.inventory_revision = ContentDigest::new("b".repeat(64)).unwrap();
    assert_eq!(
        adapter.operation(&stale, None, &w),
        Err(Error::InventoryChanged)
    );
    let mut bad = d.clone();
    bad.request.class = ConnectorOperationClass::Admin;
    assert_eq!(adapter.operation(&bad, None, &w), Err(Error::Schema));
    let result = |structured, is_error| ProtocolResult::Tool {
        content: vec![],
        structured,
        is_error,
    };
    for value in [
        None,
        Some(json!({"value":false})),
        Some(json!({"extra":true})),
    ] {
        assert_eq!(
            adapter.normalize(&d, None, result(value, false)),
            Err(Error::Schema)
        );
    }
    assert_eq!(
        adapter.normalize(&d, None, result(Some(json!({"value":"ok"})), false)),
        Err(Error::Unsupported)
    );
    assert_eq!(
        adapter.normalize(&d, None, result(None, true)),
        Err(Error::Unsupported)
    );
    assert_eq!(
        adapter.normalize(&d, None, ProtocolResult::Resource { contents: vec![] }),
        Err(Error::Schema)
    );
    let rd = dispatch(&m, &w, true);
    let source = SourceId::new("records.one").unwrap();
    assert_eq!(
        adapter.normalize(
            &rd,
            Some(&source),
            ProtocolResult::Resource { contents: vec![] }
        ),
        Err(Error::Unsupported)
    );
    assert_eq!(
        adapter.normalize(&bad, None, result(None, true)),
        Err(Error::Schema)
    );
    let mut unknown = w.clone();
    unknown.tools[0]["name"] = json!("unknown");
    assert_eq!(
        adapter.inventory(&target(), &m.manifest, &unknown),
        Err(Error::Schema)
    );
}

#[test]
fn canonical_capabilities_use_existing_registry_queries_without_automatic_registration() {
    use gateway_registry::{CapabilityQuery, Registry};
    let m = mapper();
    let snapshot = m.snapshot(&wire()).unwrap();
    let capability = snapshot.entries()[0].mapping.capability_definition();
    let query =
        CapabilityQuery::for_capability(capability.id().clone()).with_class(capability.class());
    let empty = Registry::from_documents([], []).unwrap();
    assert!(empty.capability_index().unwrap().query(&query).is_empty());
    // An independently configured catalog definition can provide the same canonical ID.
    let skill = SkillDefinitionDocument::new(
        SkillId::new("configured-provider").unwrap(),
        "Configured provider",
        "Explicit catalog entry",
        None,
        ["source"],
        ["rule"],
        ["verification"],
        [],
        [],
        [],
        [],
    )
    .unwrap()
    .with_provided_capabilities([capability.clone()])
    .unwrap();
    let configured = Registry::from_documents([], [skill]).unwrap();
    let resolved = configured.capability_index().unwrap().query(&query);
    assert_eq!(
        resolved.unique_candidate().unwrap().capability(),
        &capability
    );
    assert!(empty.capability_index().unwrap().query(&query).is_empty());
}

#[test]
fn server_annotations_never_override_explicit_mutation_metadata() {
    let mut m = mapper();
    let mut w = wire();
    let mut d = document();
    d["records"][0]["class"] = json!("MUTATE");
    d["records"][0]["side_effects"] = json!("mutation");
    d["records"][0]["retry_safety"] = json!("never");
    d["records"][0]["consent"] = json!("required");
    d["records"][0]["idempotency"] = json!("non_idempotent");
    m.document = doc(&d).unwrap();
    w.tools[0]["annotations"] =
        json!({"readOnlyHint":true,"idempotentHint":true,"destructiveHint":false});
    rebind(&mut m, &w);
    let snapshot = m.snapshot(&w).unwrap();
    let r = &snapshot.entries()[0].mapping;
    assert_eq!(r.class, OperationClass::Mutate);
    assert_eq!(r.side_effects, SideEffects::Mutation);
    assert_eq!(r.retry_safety, RetrySafety::Never);
    assert_eq!(r.idempotency, Idempotency::NonIdempotent);
    assert_eq!(r.consent, ConsentHint::Required);
    assert_eq!(
        snapshot.inventory.capabilities[0].class,
        ConnectorOperationClass::Mutate
    );
}
