//! EPIC-04.02 keeps wire examples anchored to authoritative CG Rust contracts.
use gateway_application::DeclarativeSituationApplication;
use gateway_domain::{DeclarativeContextSituationDocument, ExecutionProfile, OperatingMode};
use serde_json::Value;
use std::str::FromStr;

#[test]
fn golden_assessment_preserves_the_authoritative_situation_document() {
    let envelope: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/codex-v1/situation.assess.response.json"
    ))
    .unwrap();
    let canonical = &envelope["result"]["canonical_result"]["document"];
    let document = DeclarativeContextSituationDocument::from_json(
        &serde_json::to_string(&canonical["document"]).unwrap(),
    )
    .unwrap();
    let serialized = DeclarativeSituationApplication::new()
        .serialize_situation(&document)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&serialized).unwrap(),
        canonical["document"]
    );
    let existing: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/declarative-v0.1/assessment.json"
    ))
    .unwrap();
    assert_eq!(canonical, &existing);
}

#[test]
fn wire_execution_values_match_existing_domain_enums() {
    let schema: Value =
        serde_json::from_str(include_str!("../../../schemas/codex/v1/common.schema.json")).unwrap();
    let execution = &schema["$defs"]["execution"]["properties"];
    let modes = execution["operating_mode"]["enum"].as_array().unwrap();
    assert_eq!(modes.len(), 3);
    for value in modes {
        let text = value.as_str().unwrap();
        assert_eq!(OperatingMode::from_str(text).unwrap().as_str(), text);
    }
    let profiles = execution["execution_profile"]["enum"].as_array().unwrap();
    assert_eq!(profiles.len(), 3);
    for value in profiles {
        let text = value.as_str().unwrap();
        assert_eq!(ExecutionProfile::from_str(text).unwrap().as_str(), text);
    }
}
