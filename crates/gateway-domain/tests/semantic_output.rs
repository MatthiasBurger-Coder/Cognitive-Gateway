use gateway_domain::semantic_output::*;
use gateway_domain::{
    ContentDigest, ContextScopeId, SchemaVersion, SemanticTaskIR,
    SemanticVerificationCheck as Check,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn fixture() -> (SemanticTaskIR, OutputContract, VerificationContract) {
    (
        SemanticTaskIR::from_json(include_str!(
            "../../../tests/fixtures/semantic-task-v1/minimal.json"
        ))
        .unwrap(),
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/semantic-output-v1/output.json"
        ))
        .unwrap(),
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/semantic-output-v1/verification.json"
        ))
        .unwrap(),
    )
}

#[test]
fn schema_valid_result_projects_to_a_rust_type() {
    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct ArtifactInspection {
        summary: String,
        scores: Vec<serde_json::Number>,
        #[serde(skip_serializing_if = "Option::is_none")]
        approved: Option<bool>,
    }
    let (_, output, _) = fixture();
    let value: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/semantic-output-v1/result-valid.json"
    ))
    .unwrap();
    assert!(output.shape.accepts(&value));
    let typed: ArtifactInspection = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(typed.summary, "Inspected");
    assert_eq!(typed.approved, None);
    assert_eq!(serde_json::to_value(typed).unwrap(), value);
}

#[test]
fn projection_and_recursive_validation() {
    let (_, output, verification) = fixture();
    let expected: Value = serde_json::from_str(include_str!(
        "../../../schemas/artifact-inspection-output.schema.json"
    ))
    .unwrap();
    assert_eq!(output.json_schema(), expected);
    assert_eq!(
        serde_json::from_value::<OutputContract>(serde_json::to_value(&output).unwrap()).unwrap(),
        output
    );
    assert_eq!(
        serde_json::from_value::<VerificationContract>(
            serde_json::to_value(&verification).unwrap()
        )
        .unwrap(),
        verification
    );
    for (kind, good, bad) in [
        (OutputType::String {}, json!("x"), json!(null)),
        (OutputType::Boolean {}, json!(false), json!(0)),
        (OutputType::Integer {}, json!(1.0), json!(1.5)),
        (OutputType::Number {}, json!(1.5), json!("1")),
        (
            OutputType::Array {
                items: Box::new(OutputType::Integer {}),
            },
            json!([1, 18446744073709551615u64]),
            json!([true]),
        ),
    ] {
        assert!(kind.accepts(&good));
        assert!(!kind.accepts(&bad));
        assert!(!kind.accepts(&json!({})));
        assert!(kind.json_schema().get("type").is_some());
    }
    for value in [
        json!({"summary":"x","scores":[]}),
        json!({"summary":"x","scores":[0.1],"approved":true}),
    ] {
        assert!(output.shape.accepts(&value));
    }
    for value in [
        json!(null),
        json!({}),
        json!({"summary":"x","scores":null}),
        json!({"summary":"x","scores":[],"extra":1}),
        json!({"summary":"x","scores":[],"approved":null}),
    ] {
        assert!(!output.shape.accepts(&value));
    }
    for mut value in [
        serde_json::to_value(&output).unwrap(),
        serde_json::to_value(&verification).unwrap(),
    ] {
        value["provider"] = json!("unsupported");
        assert!(serde_json::from_value::<OutputContract>(value.clone()).is_err());
        assert!(serde_json::from_value::<VerificationContract>(value).is_err());
    }
    assert!(
        serde_json::from_value::<OutputType>(json!({"type":"STRING","format":"provider"})).is_err()
    );
}

#[test]
fn bindings_versions_and_exact_rule_sets_fail_closed() {
    let (task, output, verification) = fixture();
    assert_eq!(validate_contracts(&task, &output, &verification), Ok(()));
    let mut bad = output.clone();
    bad.schema_version = SchemaVersion::V2;
    assert_eq!(
        validate_contracts(&task, &bad, &verification),
        Err(ContractError::Version)
    );
    let mut bad = verification.clone();
    bad.schema_version = SchemaVersion::V2;
    assert_eq!(
        validate_contracts(&task, &output, &bad),
        Err(ContractError::Version)
    );
    let mut bad = output.clone();
    bad.schema.revision = gateway_domain::ReferenceId::new("changed").unwrap();
    assert_eq!(
        validate_contracts(&task, &bad, &verification),
        Err(ContractError::SchemaBinding)
    );
    for rules in [vec![], vec![verification.rules[0].clone(); 2]] {
        let mut bad = verification.clone();
        bad.rules = rules;
        assert_eq!(
            validate_contracts(&task, &output, &bad),
            Err(ContractError::RuleSet)
        );
    }
    for index in 0..2 {
        for invalid in 0..2 {
            let mut bad = verification.clone();
            let binding = if index == 0 {
                &mut bad.rules[0].rule
            } else {
                &mut bad.rules[0].verifier
            };
            match invalid {
                0 => binding.scope = ContextScopeId::new("other").unwrap(),

                _ => binding.digest = ContentDigest::new("A".repeat(64)).unwrap(),
            }
            assert_eq!(
                validate_contracts(&task, &output, &bad),
                Err(ContractError::RuleBinding)
            );
        }
    }
}

#[test]
fn execution_output_and_verification_are_separate() {
    let (task, output, verification) = fixture();
    let good: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/semantic-output-v1/result-valid.json"
    ))
    .unwrap();
    let bad: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/semantic-output-v1/result-invalid.json"
    ))
    .unwrap();
    let mut outcomes = BTreeMap::new();
    assert_eq!(
        assess_result(&task, &output, &verification, Err(()), &outcomes),
        Ok(ResultValidity::ExecutionFailed)
    );
    assert_eq!(
        assess_result(&task, &output, &verification, Ok(&bad), &outcomes),
        Ok(ResultValidity::InvalidOutput)
    );
    assert_eq!(
        assess_result(&task, &output, &verification, Ok(&good), &outcomes),
        Ok(ResultValidity::Unverified)
    );
    for (outcome, expected) in [
        (CheckOutcome::Passed, ResultValidity::Valid),
        (CheckOutcome::Failed, ResultValidity::VerificationFailed),
        (CheckOutcome::Indeterminate, ResultValidity::Unverified),
    ] {
        outcomes.insert(Check::NoUnresolvedReference, outcome);
        assert_eq!(
            assess_result(&task, &output, &verification, Ok(&good), &outcomes),
            Ok(expected)
        );
    }
    outcomes.insert(Check::OutputSchemaValid, CheckOutcome::Passed);
    assert_eq!(
        assess_result(&task, &output, &verification, Ok(&good), &outcomes),
        Ok(ResultValidity::Unverified)
    );
    let mut invalid = output;
    invalid.schema_version = SchemaVersion::V2;
    assert_eq!(
        assess_result(&task, &invalid, &verification, Ok(&good), &outcomes),
        Err(ContractError::Version)
    );
}

#[test]
fn evidence_and_claim_checks_require_their_own_reports() {
    let (task, output, mut verification) = fixture();
    let mut data = task.data().clone();
    for check in [Check::EvidenceRequired, Check::AllClaimsSupported] {
        data.verification_contract.checks.push(check);
        let mut rule = verification.rules[0].clone();
        rule.check = check;
        verification.rules.push(rule);
    }
    let task = SemanticTaskIR::new(data).unwrap();
    let good = json!({"summary":"x","scores":[]});
    let mut outcomes = BTreeMap::from([(Check::NoUnresolvedReference, CheckOutcome::Passed)]);
    assert_eq!(
        assess_result(&task, &output, &verification, Ok(&good), &outcomes),
        Ok(ResultValidity::Unverified)
    );
    outcomes.insert(Check::EvidenceRequired, CheckOutcome::Passed);
    outcomes.insert(Check::AllClaimsSupported, CheckOutcome::Passed);
    assert_eq!(
        assess_result(&task, &output, &verification, Ok(&good), &outcomes),
        Ok(ResultValidity::Valid)
    );
}
