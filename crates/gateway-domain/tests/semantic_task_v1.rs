use gateway_domain::*;
use serde_json::{Value, json};
use std::cell::RefCell;

const MINIMAL: &str = include_str!("../../../tests/fixtures/semantic-task-v1/minimal.json");
const FULL: &str =
    include_str!("../../../tests/fixtures/semantic-task-v1/performance-analysis.json");
const MINIMAL_CANONICAL: &str =
    include_str!("../../../tests/fixtures/semantic-task-v1/minimal.canonical.json");
const FULL_CANONICAL: &str =
    include_str!("../../../tests/fixtures/semantic-task-v1/performance-analysis.canonical.json");

fn full() -> Value {
    serde_json::from_str(FULL).unwrap()
}
fn rejected(value: Value) {
    let json = value.to_string();
    assert!(SemanticTaskIR::from_json(&json).is_err(), "accepted {json}");
    assert!(serde_json::from_str::<SemanticTaskIR>(&json).is_err());
}

#[test]
fn reference_fixtures_roundtrip_and_digest_are_frozen() {
    for (source, canonical, digest) in [
        (
            MINIMAL,
            MINIMAL_CANONICAL,
            include_str!("../../../tests/fixtures/semantic-task-v1/minimal.sha256"),
        ),
        (
            FULL,
            FULL_CANONICAL,
            include_str!("../../../tests/fixtures/semantic-task-v1/performance-analysis.sha256"),
        ),
    ] {
        let task = SemanticTaskIR::from_json(source).unwrap();
        assert_eq!(task.to_canonical_json().unwrap(), canonical);
        assert_eq!(task.content_digest().unwrap().as_str(), digest.trim());
        assert_eq!(SemanticTaskIR::from_json(canonical).unwrap(), task);
        assert_eq!(
            serde_json::from_str::<SemanticTaskIR>(source).unwrap(),
            task
        );
        assert_eq!(
            serde_json::from_str::<SemanticTaskIR>(&serde_json::to_string(&task).unwrap()).unwrap(),
            task
        );
        assert_eq!(SemanticTaskIR::new(task.clone().into()).unwrap(), task);
        assert_eq!(task.data().schema_version, SEMANTIC_TASK_IR_VERSION);
    }
}

#[test]
fn missing_mandatory_fields_and_unresolved_variants_fail_closed() {
    let original = full();
    for field in original.as_object().unwrap().keys() {
        let mut value = original.clone();
        value.as_object_mut().unwrap().remove(field);
        if ["current_state", "desired_state", "process"].contains(&field.as_str()) {
            assert!(SemanticTaskIR::from_json(&value.to_string()).is_ok());
        } else {
            rejected(value);
        }
    }
    for field in ["target", "goal", "output_contract", "verification_contract"] {
        for replacement in [
            Value::Null,
            json!({"kind":"UNRESOLVED"}),
            json!({"kind":"AMBIGUOUS","candidates":["a","b"]}),
        ] {
            let mut value = original.clone();
            value[field] = replacement;
            rejected(value);
        }
    }
    for version in ["2.0", "1.1", "0.1", "v1", "", "1", "01.0", "1.00"] {
        let mut value = original.clone();
        value["schema_version"] = json!(version);
        rejected(value);
    }
    let mut data = SemanticTaskIR::from_json(FULL).unwrap().data().clone();
    data.schema_version = SchemaVersion::V2;
    let error = SemanticTaskIR::new(data).unwrap_err();
    assert!(matches!(
        error,
        ValidationError::UnsupportedSchemaVersion {
            expected: "1.0",
            ..
        }
    ));
    assert!(SemanticTaskIR::from_json("{").is_err());
    assert!(
        SemanticTaskIR::from_json(&FULL.replacen(
            "\"schema_version\": \"1.0\",",
            "\"schema_version\": \"1.0\", \"schema_version\": \"2.0\",",
            1
        ))
        .is_err()
    );
}

#[test]
fn provider_fields_unknown_types_and_bad_reference_metadata_are_rejected() {
    for pointer in [
        "",
        "/target",
        "/target/reference",
        "/goal",
        "/inputs/0",
        "/inputs/0/value",
        "/inputs/0/value/value",
        "/output_contract",
        "/verification_contract",
        "/assumptions/0",
        "/process",
    ] {
        let mut value = full();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("model".into(), json!("unsolicited"));
        rejected(value);
    }
    for (pointer, replacement) in [
        ("/task_type", json!("UNKNOWN")),
        ("/target/kind", json!("AGENT")),
        ("/target/reference/id", json!("?")),
        ("/target/reference/scope", json!("other-project")),
        ("/target/reference/digest", json!("a")),
        ("/target/reference/digest", json!("A".repeat(64))),
        ("/target/reference/contract_version", json!("0.0")),
        ("/target/reference/revision", json!("")),
        ("/goal/description", json!(" \n\t")),
        ("/goal/description", json!("bad\u{0000}text")),
        (
            "/assumptions/0/confidence",
            json!({"kind":"SCORE","value":1.1}),
        ),
        ("/process/version", json!(0)),
        ("/process/id", json!("a..b")),
        (
            "/desired_state/conditions/0/expected",
            json!({"kind":"STRING","value":"three"}),
        ),
        (
            "/desired_state/expression/value",
            json!("missing-condition"),
        ),
    ] {
        let mut value = full();
        *value.pointer_mut(pointer).unwrap() = replacement;
        rejected(value);
    }
}

#[test]
fn sets_are_canonical_and_identity_duplicates_are_rejected() {
    for field in [
        "inputs",
        "context_refs",
        "observations",
        "history",
        "capability_requirements",
        "constraints",
        "policy_refs",
        "evidence_refs",
        "assumptions",
    ] {
        let mut value = full();
        let values = value[field].as_array_mut().unwrap();
        values.push(values[0].clone());
        rejected(value);
    }
    let mut value = full();
    value["verification_contract"]["checks"] = json!([
        "OUTPUT_SCHEMA_VALID",
        "NO_UNRESOLVED_REFERENCE",
        "OUTPUT_SCHEMA_VALID"
    ]);
    rejected(value);
    let original = SemanticTaskIR::from_json(FULL).unwrap();
    let mut value = full();
    value["inputs"].as_array_mut().unwrap().reverse();
    value["verification_contract"]["checks"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(
        SemanticTaskIR::from_json(&value.to_string()).unwrap(),
        original
    );
    for field in [
        "context_refs",
        "observations",
        "history",
        "capability_requirements",
        "policy_refs",
        "evidence_refs",
    ] {
        let mut a = full();
        let mut added = a[field][0].clone();
        added["id"] = json!("z-last");
        a[field].as_array_mut().unwrap().push(added);
        let first = SemanticTaskIR::from_json(&a.to_string()).unwrap();
        a[field].as_array_mut().unwrap().reverse();
        assert_eq!(
            SemanticTaskIR::from_json(&a.to_string())
                .unwrap()
                .to_canonical_json()
                .unwrap(),
            first.to_canonical_json().unwrap()
        );
    }
    let mut value = full();
    value["inputs"][1]["value"]["value"] =
        json!({"kind":"SET","value":[{"kind":"INTEGER","value":10},{"kind":"INTEGER","value":2}]});
    let first = SemanticTaskIR::from_json(&value.to_string()).unwrap();
    value["inputs"][1]["value"]["value"]["value"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(
        first,
        SemanticTaskIR::from_json(&value.to_string()).unwrap()
    );
    let mut data = first.data().clone();
    data.inputs[1].value = SemanticInputValue::Literal(TypedValue::Set(vec![]));
    assert!(SemanticTaskIR::new(data).is_err());
    let mut value = full();
    value["assumptions"][0]["premise"] =
        json!({"kind":"SET","value":[{"kind":"INTEGER","value":1},{"kind":"INTEGER","value":1}]});
    rejected(value);
}

#[test]
fn verification_is_explicit_and_claim_support_requires_evidence_check() {
    for checks in [
        json!([]),
        json!(["OUTPUT_SCHEMA_VALID"]),
        json!(["NO_UNRESOLVED_REFERENCE"]),
        json!([
            "OUTPUT_SCHEMA_VALID",
            "NO_UNRESOLVED_REFERENCE",
            "ALL_CLAIMS_SUPPORTED"
        ]),
        json!(["OUTPUT_SCHEMA_VALID", "NO_UNRESOLVED_REFERENCE", "PASSED"]),
    ] {
        let mut value = full();
        value["verification_contract"]["checks"] = checks;
        rejected(value);
    }
    let original = SemanticTaskIR::from_json(FULL).unwrap();
    let mut value = full();
    value["goal"]["outcome"] = json!("another-outcome");
    assert_ne!(
        original.content_digest().unwrap(),
        SemanticTaskIR::from_json(&value.to_string())
            .unwrap()
            .content_digest()
            .unwrap()
    );
}

struct CapturedBasis {
    seen: RefCell<Vec<SemanticReferenceBinding>>,
    reject_id: Option<String>,
}
impl SemanticReferenceValidator for CapturedBasis {
    fn validate(
        &self,
        task: &SemanticTaskData,
        binding: &SemanticReferenceBinding,
    ) -> Result<(), ValidationError> {
        assert_eq!(binding.scope, task.target.reference.scope);
        self.seen.borrow_mut().push(binding.clone());
        if self.reject_id.as_deref() == Some(&binding.id) {
            return Err(ValidationError::MissingDeclarativeIdentity {
                kind: "semantic reference",
                id: binding.id.clone(),
            });
        }
        Ok(())
    }
}

#[test]
fn every_reference_is_exposed_and_missing_bindings_block_handoff() {
    let task = SemanticTaskIR::from_json(FULL).unwrap();
    let validator = CapturedBasis {
        seen: RefCell::new(vec![]),
        reject_id: None,
    };
    task.validate_references(&validator).unwrap();
    let bindings = validator.seen.into_inner();
    assert_eq!(bindings.len(), 12);
    assert!(
        bindings
            .iter()
            .any(|b| b.contract.as_str() == "cg.process-definition" && b.revision.as_str() == "1")
    );
    for binding in bindings {
        let validator = CapturedBasis {
            seen: RefCell::new(vec![]),
            reject_id: Some(binding.id),
        };
        assert!(task.validate_references(&validator).is_err());
    }
}

#[test]
fn desired_state_set_operands_and_rust_created_assumptions_are_validated() {
    let mut value = full();
    value["desired_state"]["conditions"][0]["operator"] = json!("IN");
    value["desired_state"]["conditions"][0]["expected"] =
        json!({"kind":"SET","value":[{"kind":"INTEGER","value":3},{"kind":"INTEGER","value":1}]});
    let task = SemanticTaskIR::from_json(&value.to_string()).unwrap();
    value["desired_state"]["conditions"][0]["expected"]["value"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(task, SemanticTaskIR::from_json(&value.to_string()).unwrap());
    let mut data = task.data().clone();
    data.assumptions[0].premise = TypedValue::Set(vec![]);
    assert!(SemanticTaskIR::new(data).is_err());
    let mut value = full();
    value["desired_state"]["conditions"][0]["operator"] = json!("PRESENT");
    value["desired_state"]["conditions"][0]["expected"] = Value::Null;
    assert!(SemanticTaskIR::from_json(&value.to_string()).is_ok());
}
