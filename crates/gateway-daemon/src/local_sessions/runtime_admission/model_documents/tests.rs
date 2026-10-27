use super::*;
use serde_json::json;

struct Fixture {
    catalog: BTreeMap<ReferenceId, PinnedDocument>,
    root: Pin,
}
fn add(
    catalog: &mut BTreeMap<ReferenceId, PinnedDocument>,
    id: &str,
    kind: &str,
    value: Value,
) -> Pin {
    let canonical = serde_json::to_vec(&value).unwrap();
    let pin = Pin {
        id: ReferenceId::new(id).unwrap(),
        scope: ContextScopeId::new("project-x").unwrap(),
        contract: ReferenceId::new(kind).unwrap(),
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new("7").unwrap(),
        digest: context_content_digest(&canonical),
    };
    catalog.insert(
        pin.id.clone(),
        PinnedDocument {
            pin: pin.clone(),
            canonical,
            value,
        },
    );
    pin
}
fn budget() -> Value {
    json!({"results":1,"rounds":1,"latency_ms":1000,"cost":0,"cost_unit":"fixture-units","tokens":20000,"context_total":20000,"context_reservations":[{"class":"EVIDENCE","units":20000}]})
}
fn fixture() -> Fixture {
    fixture_with(|_, _| {})
}

#[test]
fn environment_bearer_locator_is_explicit_bounded_and_not_a_profile_alias() {
    for variable in ["", "1SECRET", "HAS-DASH", "HAS SPACE", "Ä_SECRET"] {
        let f = fixture_with(|id, value| {
            if id == "model-adapter" {
                value["credential_source"] =
                    json!({"kind":"ENVIRONMENT_BEARER_V1","variable":variable});
            }
        });
        assert!(read_model(&f.root, &f.catalog).is_err());
    }
    let f = fixture_with(|id, value| {
        if id == "model-adapter" {
            value["credential_source"] =
                json!({"kind":"ENVIRONMENT_BEARER_V1","variable":"CG_FACTORY_TEST_BEARER"});
        }
    });
    assert!(read_model(&f.root, &f.catalog).is_ok());
    let f = fixture_with(|id, value| {
        if id == "model-adapter" {
            value["credential_profile"] = json!("mcp-profile");
            value["credential_source"] =
                json!({"kind":"ENVIRONMENT_BEARER_V1","variable":"CG_FACTORY_TEST_BEARER"});
        }
    });
    assert!(matches!(
        read_model(&f.root, &f.catalog),
        Err(SessionError::UnsupportedCapability)
    ));
    let f = fixture_with(|id, value| {
        if id == "model-adapter" {
            value["credential_source"] = json!({"kind":"ENVIRONMENT_BEARER_V1","variable":"CG_FACTORY_TEST_BEARER","secret":"must-not-enter-document"});
        }
    });
    assert!(read_model(&f.root, &f.catalog).is_err());
}

#[test]
fn absent_environment_bearer_denies_without_mutating_process_environment() {
    let variable = format!(
        "CG_ABSENT_MODEL_BEARER_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let locator = ModelCredentialSource::EnvironmentBearerV1 { variable };
    assert!(matches!(locator.resolve(), Err(SessionError::Unavailable)));
    assert!(matches!(locator.resolve(), Err(SessionError::Unavailable)));
}
fn fixture_with(change: impl Fn(&str, &mut Value)) -> Fixture {
    let put = |catalog: &mut BTreeMap<ReferenceId, PinnedDocument>,
               id: &str,
               kind: &str,
               mut value: Value| {
        change(id, &mut value);
        add(catalog, id, kind, value)
    };
    let mut catalog = BTreeMap::new();
    let object = put(
        &mut catalog,
        "artifact-1",
        "cg.connector-object",
        json!({"schema_version":"1.0","id":"artifact-1","revision":"7","scope":"project-x","kind":"ARTIFACT","connector":"fixture.records","parent_target":null,"subjects":[],"coordinates":{"id":{"kind":"STRING","value":"artifact-1"}},"object_reference":"artifact-1"}),
    );
    let schema = put(
        &mut catalog,
        "ArtifactInspection",
        "cg.output-schema",
        json!({"type":"string"}),
    );
    let mut task: Value = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/semantic-task-v1/minimal.json"
    )))
    .unwrap();
    task["target"]["reference"] = json!(object);
    task["output_contract"]["schema"] = json!(schema);
    let native: SemanticTaskIR = serde_json::from_value(task).unwrap();
    let task = put(
        &mut catalog,
        "task-inspect",
        "cg.semantic-task",
        serde_json::to_value(&native).unwrap(),
    );
    assert_eq!(task.digest, native.content_digest().unwrap());
    let mut alias = json!(task);
    alias["contract"] = json!("semantic-task-ir");
    let provenance = put(
        &mut catalog,
        "model-declaration",
        PROVENANCE,
        json!({"schema_version":"1.0","declaration":"OPERATOR_DECLARATION_V1"}),
    );
    let target = put(
        &mut catalog,
        "native-model",
        TARGET,
        json!({"schema_version":"1.0","runtime_id":"fixture-model:immutable-v1"}),
    );
    let tokenizer = put(
        &mut catalog,
        "model-tokenizer",
        TOKENIZER,
        json!({"schema_version":"1.0","method":"UTF8_WIRE_BYTES_PLUS256_V1"}),
    );
    let adapter = put(
        &mut catalog,
        "model-adapter",
        ADAPTER,
        json!({"schema_version":"1.0","renderer":"CHAT_COMPLETIONS_SESSION_TOOLS_RUNTIME_V2","wire_mode":"CANONICAL_V1","response_mode":"TOOL_PROPOSAL_TEXT_V1","tokenizer":tokenizer,"endpoint":"http://127.0.0.1:8000","credential_profile":null,"pricing":{"cost_unit":"fixture-units","fixed_per_attempt_microunits":3,"input_microunits_per_unit":0,"output_microunits_per_unit":0}}),
    );
    let provider = put(
        &mut catalog,
        "model-provider",
        PROVIDER,
        json!({"schema_version":"1.0","adapter":adapter,"locality":"ON_DEVICE","maximum_sensitivity":"CONFIDENTIAL","controls":{"TEMPERATURE":{"kind":"SCALAR","min":0.0,"max":2.0}},"defaults":{"TEMPERATURE":{"kind":"SCALAR","value":0.0}},"unexposed_defaults":[],"provenance":provenance}),
    );
    let capability = put(
        &mut catalog,
        "model-capability",
        CAPABILITY,
        json!({"schema_version":"1.0","model":target,"provider":provider,"task_classes":["EXTRACTION"],"input_contracts":["prompt-ir-v1"],"output_contracts":{"ArtifactInspection":schema},"context_limit":20000,"max_output_tokens":1024,"structured_output_modes":["TOOL_PROPOSAL"],"tool_calling":true,"reasoning_strategies":["bounded"],"provenance":provenance}),
    );
    let generation = put(
        &mut catalog,
        "model-generation",
        GENERATION,
        json!({"schema_version":"1.0","controls":{"TEMPERATURE":{"kind":"SCALAR","value":0.0}},"max_output_tokens":128,"structured_output_mode":"TOOL_PROPOSAL","reproducibility_class":"STOCHASTIC"}),
    );
    let status = put(
        &mut catalog,
        "model-status",
        STATUS,
        json!({"schema_version":"1.0","provider":provider,"availability":"AVAILABLE","valid_from_ms":100,"valid_until_ms":1100}),
    );
    let analysis = put(
        &mut catalog,
        "context-analysis",
        ANALYSIS,
        json!({"profile":"NATIVE_SESSION_MODEL_V1","schema_version":"1.0","budget_per_requirement":budget(),"total_tokens":40000,"maximum_sensitivity":"INTERNAL","additional":[]}),
    );
    let root = put(
        &mut catalog,
        "model-producer",
        MODEL,
        json!({"profile":"NATIVE_SESSION_MODEL_V1","schema_version":"1.0","task":task,"invocation_task":alias,"provider":provider,"capability":capability,"generation":generation,"status":status,"analysis":analysis,"sources":[],"task_class":"EXTRACTION","input_contract":"prompt-ir-v1","strategy":"bounded","sensitivity":"INTERNAL","permitted_localities":["ON_DEVICE"],"input_tokens":4096,"timeout_ms":10000,"max_attempts":2}),
    );
    Fixture { catalog, root }
}
#[test]
fn actual_pinned_dag_constructs_native_definitions_without_self_identities_or_grants() {
    let f = fixture();
    let native = read_model(&f.root, &f.catalog).unwrap();
    assert_eq!(
        native.provider.data().identity,
        f.catalog[&native.model.provider.id].pin
    );
    assert_eq!(
        native.capability.data().identity,
        f.catalog[&native.model.capability.id].pin
    );
    assert_eq!(
        native.generation.data().profile_id,
        native.model.generation.id
    );
    assert_eq!(
        native.generation.data().revision,
        native.model.generation.revision
    );
    assert_eq!(native.status.observation, native.model.status);
    assert!(native.status.in_window(100));
    assert!(!native.status.in_window(99));
    assert!(!native.status.in_window(1100));
    assert_eq!(native.adapter.pricing.maximum(100, 200, 2).unwrap(), 6);
    assert_eq!(native.target.runtime_id, "fixture-model:immutable-v1");
    assert!(native.sources.is_empty());
    assert_eq!(
        native
            .analysis
            .budget_per_requirement
            .native()
            .unwrap()
            .tokens
            .0,
        20000
    );
}
#[test]
fn reserved_fields_strict_json_versions_and_unknown_owners_fail_closed() {
    let f = fixture();
    for (id, fields) in [
        ("model-provider", vec!["identity"]),
        ("model-capability", vec!["identity"]),
        (
            "model-generation",
            vec!["identity", "profile_id", "revision"],
        ),
        ("model-status", vec!["observation"]),
    ] {
        for field in fields {
            let actual = &f.catalog[&ReferenceId::new(id).unwrap()];
            let mut value = actual.value.clone();
            value[field] = json!("forbidden");
            let mut copy = BTreeMap::new();
            let pin = add(&mut copy, id, actual.pin.contract.as_str(), value);
            assert!(ModelDocument::parse(&pin, &copy[&pin.id].canonical).is_err());
        }
    }
    let mut catalog = BTreeMap::new();
    for value in [
        json!({"schema_version":"2.0","method":"UTF8_WIRE_BYTES_PLUS256_V1"}),
        json!({"schema_version":"1.0","method":"UNKNOWN"}),
        json!({"schema_version":"1.0","method":"UTF8_WIRE_BYTES_PLUS256_V1","available":true}),
    ] {
        let pin = add(&mut catalog, "tokenizer", TOKENIZER, value);
        assert!(ModelDocument::parse(&pin, &catalog[&pin.id].canonical).is_err());
    }
    let pin = &f.catalog[&ReferenceId::new("model-tokenizer").unwrap()].pin;
    assert!(ModelDocument::parse(pin,br#"{"schema_version":"1.0","method":"UTF8_WIRE_BYTES_PLUS256_V1","method":"UTF8_WIRE_BYTES_PLUS256_V1"}"#).is_err());
    assert_eq!(
        ModelDocument::parse(pin, &vec![b' '; BYTES + 1]).err(),
        Some(SessionError::LimitExceeded)
    );
}
#[test]
fn explicit_task_alias_accepts_only_declared_contract_change_for_same_actual_body() {
    let f = fixture();
    let native = read_model(&f.root, &f.catalog).unwrap();
    let actual = &native.model.task;
    let task: SemanticTaskIR = serde_json::from_value(f.catalog[&actual.id].value.clone()).unwrap();
    native.model.validate_task_alias(actual, &task).unwrap();
    for field in [
        "id",
        "scope",
        "contract",
        "contract_version",
        "revision",
        "digest",
    ] {
        let mut value = serde_json::to_value(&native.model.invocation_task).unwrap();
        value[field] = match field {
            "digest" => json!(context_content_digest(b"other actual content")),
            "contract_version" => json!("2.0"),
            _ => json!("foreign"),
        };
        let mut model = native.model.clone();
        model.invocation_task = serde_json::from_value(value).unwrap();
        assert!(model.validate_task_alias(actual, &task).is_err());
    }
}
#[test]
fn exact_reference_scope_digest_roles_and_claimed_cycles_are_checked_before_native_constructors() {
    let f = fixture();
    for field in [
        "scope",
        "revision",
        "digest",
        "contract",
        "contract_version",
    ] {
        let mut value = f.catalog[&f.root.id].value.clone();
        value["provider"][field] = match field {
            "digest" => json!(context_content_digest(b"foreign provider")),
            "contract_version" => json!("2.0"),
            _ => json!("foreign"),
        };
        let mut catalog = fixture().catalog;
        let root = add(&mut catalog, "model-producer", MODEL, value);
        assert!(read_model(&root, &catalog).is_err());
    }
    let mut catalog = f.catalog;
    let provider = &mut catalog
        .get_mut(&ReferenceId::new("model-provider").unwrap())
        .unwrap();
    // A claimed backedge cannot hide behind a remembered pin: its changed
    // content no longer matches the actual retained definition digest.
    provider.value["provenance"] = json!(f.root);
    provider.canonical = serde_json::to_vec(&provider.value).unwrap();
    assert_eq!(
        read_model(&f.root, &catalog).err(),
        Some(SessionError::StaleRevision)
    );
}
#[test]
fn source_bindings_never_supply_quality_usage_or_availability_and_budget_math_is_checked() {
    let mut catalog = BTreeMap::new();
    let f = fixture();
    let task = &f.catalog[&ReferenceId::new("task-inspect").unwrap()].pin;
    let value = json!({"profile":"NATIVE_SESSION_MODEL_V1","schema_version":"1.0","class":"CURRENT_INPUT","covers":["task-input"],"satisfaction":{"kind":"TASK","task":task}});
    let pin = add(&mut catalog, "binding", SOURCE, value.clone());
    let ModelDocument::Source(data) =
        ModelDocument::parse(&pin, &catalog[&pin.id].canonical).unwrap()
    else {
        panic!("source")
    };
    assert_eq!(data.class.native(), ContextSourceClass::CurrentInput);
    assert_eq!(data.covers.len(), 1);
    for field in ["quality", "tokens", "available", "fresh"] {
        let mut supplied = value.clone();
        supplied[field] = json!(true);
        let pin = add(&mut catalog, "binding", SOURCE, supplied);
        assert!(ModelDocument::parse(&pin, &catalog[&pin.id].canonical).is_err());
    }
    let mut bad = budget();
    bad["context_reservations"] =
        json!([{"class":"EVIDENCE","units":20000},{"class":"EVIDENCE","units":1}]);
    assert!(
        serde_json::from_value::<RetrievalBudgetSpec>(bad)
            .unwrap()
            .native()
            .is_err()
    );
    let prices = PricingDeclaration {
        cost_unit: ReferenceId::new("units").unwrap(),
        fixed_per_attempt_microunits: 0,
        input_microunits_per_unit: u64::MAX,
        output_microunits_per_unit: 0,
    };
    assert_eq!(prices.maximum(2, 0, 1), Err(SessionError::LimitExceeded));
}

#[test]
fn native_constructor_negatives_use_actual_new_pins_and_do_not_relax_native_contracts() {
    let cases: [(&str, &str, Value); 10] = [
        (
            "model-provider",
            "controls",
            json!({"TEMPERATURE":{"kind":"INTEGER","min":0,"max":2}}),
        ),
        (
            "model-provider",
            "defaults",
            json!({"TEMPERATURE":{"kind":"SCALAR","value":3.0}}),
        ),
        (
            "model-generation",
            "controls",
            json!({"TEMPERATURE":{"kind":"SCALAR","value":-1.0}}),
        ),
        (
            "model-generation",
            "controls",
            json!({"SEED":{"kind":"INTEGER","value":1}}),
        ),
        ("model-generation", "structured_output_mode", json!("JSON")),
        ("model-capability", "context_limit", json!(0)),
        ("model-capability", "tool_calling", json!(false)),
        (
            "model-adapter",
            "endpoint",
            json!("http://foreign.example.invalid"),
        ),
        ("model-adapter", "wire_mode", json!("PORTABLE_V1")),
        ("model-producer", "input_tokens", json!(u32::MAX)),
    ];
    for (id, key, value) in cases {
        let f = fixture_with(|name, body| {
            if name == id {
                body[key] = value.clone();
            }
        });
        assert!(
            read_model(&f.root, &f.catalog).is_err(),
            "accepted invalid native declaration {id}/{key}"
        );
    }
    let f = fixture_with(|name, body| {
        if name == "model-adapter" {
            body["wire_mode"] = json!("PORTABLE_V1");
            body["renderer"] = json!("CHAT_COMPLETIONS_SESSION_TOOLS_PORTABLE_RUNTIME_V4");
        }
    });
    assert!(
        read_model(&f.root, &f.catalog).is_ok(),
        "explicit portable declaration remains data, not availability proof"
    );
}
#[test]
fn status_window_catalog_size_and_additional_native_need_bounds_are_explicit() {
    let f = fixture_with(|id, body| {
        if id == "model-status" {
            body["valid_until_ms"] = json!(100);
        }
    });
    assert!(read_model(&f.root, &f.catalog).is_err());
    let mut f = fixture();
    for n in 0..NODES {
        add(
            &mut f.catalog,
            &format!("unused-{n}"),
            PROVENANCE,
            json!({"schema_version":"1.0","declaration":"OPERATOR_DECLARATION_V1"}),
        );
    }
    assert_eq!(
        read_model(&f.root, &f.catalog).err(),
        Some(SessionError::LimitExceeded)
    );
    let f = fixture_with(|id, body| {
        if id == "context-analysis" {
            body["additional"] = json!([{"id":"derived-need","class":"EVIDENCE","purpose":"Actual input evidence","mandatory":true,"permitted_sources":["TOOLS_MCP"],"description":"Captured observation","maximum_sensitivity":"INTERNAL","accepted_trust":["OBSERVED_EVIDENCE"],"fresh":true,"evidence":[],"provenances":[],"budget":budget(),"priority":0}]);
        }
    });
    let data = read_model(&f.root, &f.catalog).unwrap();
    let needs = data.analysis.additional().unwrap();
    assert_eq!(needs.len(), 1);
    assert_eq!(needs[0].class, ContextClass::Evidence);
    assert!(needs[0].necessity.required());
}

#[test]
fn runtime_discovery_uses_owning_profile_edges_and_never_promotes_passive_legacy_data() {
    let f = fixture();
    let mut pending = vec![f.root.clone()];
    let mut seen = BTreeSet::new();
    while let Some(pin) = pending.pop() {
        let document = &f.catalog[&pin.id];
        assert_eq!(document.pin, pin);
        if !seen.insert(pin.id.clone()) {
            continue;
        }
        pending.extend(super::super::document_references(&pin, &document.value).unwrap());
    }
    assert_eq!(
        seen.len(),
        f.catalog.len(),
        "all actual definition sidecars are reachable through owning parsers"
    );
    for kind in [MODEL, ANALYSIS, SOURCE] {
        let mut catalog = BTreeMap::new();
        let pin = add(&mut catalog, "passive", kind, json!({}));
        assert!(
            super::super::document_references(&pin, &catalog[&pin.id].value)
                .unwrap()
                .is_empty()
        );
        assert!(
            ModelDocument::parse(&pin, &catalog[&pin.id].canonical).is_err(),
            "Factory requires its actual profile even while data retention is legal"
        );
        let pin = add(&mut catalog, "unknown", kind, json!({"profile":"UNKNOWN"}));
        assert_eq!(
            super::super::document_references(&pin, &catalog[&pin.id].value).err(),
            Some(SessionError::UnsupportedCapability)
        );
    }
}

#[test]
fn before_read_route_is_explicit_current_mcp_data_and_discovers_exact_target_pin() {
    let f = fixture();
    let target = f.catalog[&ReferenceId::new("artifact-1").unwrap()]
        .pin
        .clone();
    let mut catalog = BTreeMap::new();
    let body = json!({"profile":"NATIVE_SESSION_MODEL_V1","schema_version":"1.0","class":"TOOLS_MCP","covers":["target-state"],"satisfaction":{"kind":"BEFORE_READ","target":target,"mapping_reference":"read-record","source":"records.tool"}});
    let pin = add(&mut catalog, "before-source", SOURCE, body.clone());
    let node = ModelDocument::parse(&pin, &catalog[&pin.id].canonical).unwrap();
    assert_eq!(node.references(), vec![target]);
    for key in ["class", "quality"] {
        let mut invalid = body.clone();
        invalid[key] = if key == "class" {
            json!("REPOSITORY")
        } else {
            json!({"freshness":"FRESH"})
        };
        let pin = add(&mut catalog, "before-source", SOURCE, invalid);
        assert!(ModelDocument::parse(&pin, &catalog[&pin.id].canonical).is_err());
    }
    let f = fixture_with(|id, body| {
        if id == "model-status" {
            body["availability"] = json!("UNAVAILABLE");
        }
    });
    let data = read_model(&f.root, &f.catalog).unwrap();
    assert_eq!(
        data.status.availability,
        ModelAvailability::Unavailable,
        "Reader must not turn a status declaration into Available authorization"
    );
}
