//! Acceptance tests for the CG-06.01 declarative context foundations.

use std::{collections::BTreeSet, str::FromStr};

use gateway_domain::SchemaVersion;
use gateway_domain::{
    AcceptanceCriterionId, AssessmentId, ContextScopeId, DECLARATIVE_CONTEXT_IR_VERSION,
    DeclarativeContext, DeclarativeContextId, DeclarativeContextVersion, DesiredStateId,
    EvidenceId, FactId, IntentId, ObservationId, ObservedState, ObservedStateId, ProvenanceId,
    ReferenceId, RiskId, Situation, SituationId, SourceId, ValidationError,
};

#[test]
fn v1_aggregate_boundaries_are_typed_and_explicit() {
    let context = DeclarativeContext::new_v1(DeclarativeContextId::new("context-1").unwrap());
    let observed = ObservedState::new_v1(ObservedStateId::new("state-1").unwrap());
    let situation = Situation::new_v1(SituationId::new("situation-1").unwrap());

    assert_eq!(DECLARATIVE_CONTEXT_IR_VERSION, context.version());
    assert_eq!(context.id().as_str(), "context-1");
    assert_eq!(observed.version(), DECLARATIVE_CONTEXT_IR_VERSION);
    assert_eq!(situation.version(), DECLARATIVE_CONTEXT_IR_VERSION);
}

#[test]
fn future_versions_are_parseable_but_not_accepted_by_v1_aggregates() {
    let future = DeclarativeContextVersion::from_str("2.0").unwrap();
    assert_eq!(future.major(), 2);
    assert!(matches!(
        DeclarativeContext::new(future, DeclarativeContextId::new("context-1").unwrap()),
        Err(ValidationError::UnsupportedSchemaVersion {
            expected: "1.0",
            actual
        }) if actual == "2.0"
    ));
    assert!(matches!(
        ObservedState::new(future, ObservedStateId::new("state-1").unwrap()),
        Err(ValidationError::UnsupportedSchemaVersion { .. })
    ));
    assert!(matches!(
        Situation::new(future, SituationId::new("situation-1").unwrap()),
        Err(ValidationError::UnsupportedSchemaVersion { .. })
    ));
    assert!(DeclarativeContextVersion::from_str("1").is_err());
    assert!(DeclarativeContextVersion::from_str("0.1").is_err());
}

#[test]
fn explicit_v1_constructors_and_schema_conversion_are_supported() {
    let version = DeclarativeContextVersion::new(1, 0).unwrap();
    assert_eq!(version.major(), 1);
    assert_eq!(version.minor(), 0);
    assert!(version.ensure_supported().is_ok());

    let converted = DeclarativeContextVersion::try_from(SchemaVersion::new(1, 0).unwrap()).unwrap();
    assert_eq!(converted, version);

    let context = DeclarativeContext::new(
        version,
        DeclarativeContextId::new("context-explicit").unwrap(),
    )
    .unwrap();
    let observed =
        ObservedState::new(version, ObservedStateId::new("state-explicit").unwrap()).unwrap();
    let situation =
        Situation::new(version, SituationId::new("situation-explicit").unwrap()).unwrap();

    assert_eq!(context.version(), version);
    assert_eq!(observed.version(), version);
    assert_eq!(situation.version(), version);
}

#[test]
fn invalid_version_construction_fails_closed() {
    assert!(DeclarativeContextVersion::new(0, 1).is_err());
    assert!(SchemaVersion::new(0, 1).is_err());
}

#[test]
fn core_identities_are_distinct_and_order_deterministically() {
    let intent = IntentId::new("intent-1").unwrap();
    let desired = DesiredStateId::new("desired-1").unwrap();
    let criterion = AcceptanceCriterionId::new("criterion-1").unwrap();
    let observation = ObservationId::new("observation-1").unwrap();
    let fact = FactId::new("fact-1").unwrap();
    let evidence = EvidenceId::new("evidence-1").unwrap();
    let provenance = ProvenanceId::new("provenance-1").unwrap();
    let assessment = AssessmentId::new("assessment-1").unwrap();
    let risk = RiskId::new("risk-1").unwrap();
    let scope = ContextScopeId::new("scope-1").unwrap();
    let source = SourceId::new("source-1").unwrap();
    let reference = ReferenceId::new("reference-1").unwrap();

    assert_eq!(intent.as_str(), "intent-1");
    assert_eq!(desired.as_str(), "desired-1");
    assert_eq!(criterion.as_str(), "criterion-1");
    assert_eq!(observation.as_str(), "observation-1");
    assert_eq!(fact.as_str(), "fact-1");
    assert_eq!(evidence.as_str(), "evidence-1");
    assert_eq!(provenance.as_str(), "provenance-1");
    assert_eq!(assessment.as_str(), "assessment-1");
    assert_eq!(risk.as_str(), "risk-1");
    assert_eq!(scope.as_str(), "scope-1");
    assert_eq!(source.as_str(), "source-1");
    assert_eq!(reference.as_str(), "reference-1");

    let ordered: BTreeSet<_> = ["b", "a", "c"]
        .into_iter()
        .map(|value| DeclarativeContextId::new(value).unwrap())
        .collect();
    let values: Vec<_> = ordered
        .into_iter()
        .map(|value| value.into_inner())
        .collect();
    assert_eq!(values, ["a", "b", "c"]);
}
