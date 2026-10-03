use gateway_domain::{
    CapabilityId, Confidence, ContentDigest, ContextScopeId, EvidenceId, FactId, FreshnessStatus,
    NonEmptyText, ObservationId, OperatingMode, PolicyId, ProvenanceId, QualityMetadata,
    ReferenceId, SensitivityClass, TrustClass, Uncertainty, UnixTimestamp,
    learning::{
        ExperienceBasis, FallbackBehavior, FingerprintSignal, LearnedProcedure, PatternCandidate,
        ProcedureLifecycle, ProcedureState, ProcedureStep, ProcedureTransition, ProcessReference,
        SituationFingerprint,
    },
    memory::{ExperienceRecord, MemoryEligibilityReference, MemoryPayload},
};

fn id(value: &str) -> ReferenceId {
    ReferenceId::new(value).unwrap()
}
fn digest() -> ContentDigest {
    ContentDigest::new("a".repeat(64)).unwrap()
}

fn candidate() -> PatternCandidate {
    let scope = ContextScopeId::new("project-1").unwrap();
    let fingerprint = SituationFingerprint::new(
        scope.clone(),
        vec![
            FingerprintSignal::Fact(FactId::new("fact-1").unwrap()),
            FingerprintSignal::OperatingMode(OperatingMode::Development),
        ],
    )
    .unwrap();
    let memory = MemoryEligibilityReference {
        schema_version: 1,
        scope,
        id: id("memory-1"),
        revision: 2,
        eligibility_version: 2,
        source_snapshot: id("snapshot-1"),
        source_digest: digest(),
    };
    PatternCandidate::new(
        id("candidate-1"),
        fingerprint,
        vec![
            ExperienceBasis::new(
                memory,
                ProvenanceId::new("source-1").unwrap(),
                id("evaluation-1"),
            )
            .unwrap(),
        ],
    )
    .unwrap()
}

fn procedure() -> LearnedProcedure {
    LearnedProcedure::new(
        id("procedure-1"),
        1,
        &candidate(),
        vec![ProcedureStep::new(
            ProcessReference::new(id("process-1"), 1, digest()).unwrap(),
            CapabilityId::new("inspect").unwrap(),
            PolicyId::new("policy-1").unwrap(),
        )],
        vec![ObservationId::new("observation-1").unwrap()],
        vec![EvidenceId::new("evidence-1").unwrap()],
        vec![EvidenceId::new("verification-1").unwrap()],
        FallbackBehavior::Stop,
    )
    .unwrap()
}

#[test]
fn experience_round_trip_is_strict_and_non_authoritative() {
    let record = ExperienceRecord {
        schema_version: 1,
        id: id("experience-1"),
        scope: ContextScopeId::new("project-1").unwrap(),
        provenance: ProvenanceId::new("source-1").unwrap(),
        source_snapshot: id("snapshot-1"),
        source_version: NonEmptyText::new("r1").unwrap(),
        source_digest: digest(),
        created_at: UnixTimestamp::new(10),
        observed_at: UnixTimestamp::new(9),
        valid_from: UnixTimestamp::new(9),
        expires_at: UnixTimestamp::new(20),
        max_age_seconds: 11,
        quality: QualityMetadata::new(
            TrustClass::DerivedAssessment,
            SensitivityClass::Normal,
            Confidence::score(0.9).unwrap(),
            FreshnessStatus::Fresh,
            Uncertainty::None,
        ),
        validation: Some(id("validation-1")),
        outcome: Some(NonEmptyText::new("passed").unwrap()),
        label_basis: Some(id("label-1")),
        payload: Some(MemoryPayload::Reference(id("payload-1"))),
    };
    let encoded = record.to_json().unwrap();
    assert_eq!(
        ExperienceRecord::from_json(&encoded)
            .unwrap()
            .to_json()
            .unwrap(),
        encoded
    );
    assert!(
        ExperienceRecord::from_json(
            &encoded.replace("\"schema_version\":1", "\"schema_version\":2")
        )
        .is_err()
    );
    assert!(
        ExperienceRecord::from_json(
            &encoded.replace("\"source_version\":\"r1\"", "\"source_version\":\"\"")
        )
        .is_err()
    );
    assert!(
        ExperienceRecord::from_json(
            &encoded.replace("\"payload\":", "\"unknown\":true,\"payload\":")
        )
        .is_err()
    );
}

#[test]
fn canonical_round_trip_and_digest_binding() {
    let candidate = candidate();
    let encoded = candidate.to_json().unwrap();
    assert_eq!(
        PatternCandidate::from_json(&encoded)
            .unwrap()
            .to_json()
            .unwrap(),
        encoded
    );
    let procedure = procedure();
    let encoded = procedure.to_json().unwrap();
    assert_eq!(
        LearnedProcedure::from_json(&encoded)
            .unwrap()
            .to_json()
            .unwrap(),
        encoded
    );
    let changed = encoded.replace("policy-1", "policy-2");
    assert!(LearnedProcedure::from_json(&changed).is_err());
    assert_ne!(
        procedure.digest(),
        LearnedProcedure::new(
            id("procedure-1"),
            2,
            &candidate,
            procedure.steps().to_vec(),
            procedure.required_observations().to_vec(),
            procedure.required_evidence().to_vec(),
            procedure.verification_evidence().to_vec(),
            procedure.fallback()
        )
        .unwrap()
        .digest()
    );
}

#[test]
fn malformed_versions_and_noncanonical_inputs_fail_closed() {
    let encoded = candidate().to_json().unwrap();
    assert!(
        PatternCandidate::from_json(
            &encoded.replace("\"schema_version\":1", "\"schema_version\":2")
        )
        .is_err()
    );
    assert!(
        PatternCandidate::from_json(
            &encoded.replace("\"schema_version\":1", "\"schema_version\":0")
        )
        .is_err()
    );
    assert!(
        PatternCandidate::from_json(&encoded.replace(
            "\"schema_version\":1",
            "\"schema_version\":1,\"unknown\":true"
        ))
        .is_err()
    );
    let duplicate = vec![FingerprintSignal::Fact(FactId::new("fact-1").unwrap()); 2];
    assert!(
        SituationFingerprint::new(ContextScopeId::new("project-1").unwrap(), duplicate).is_err()
    );
    let encoded = procedure().to_json().unwrap();
    assert!(
        LearnedProcedure::from_json(&encoded.replace("\"version\":1", "\"version\":0")).is_err()
    );
}

#[test]
fn lifecycle_is_deterministic_and_auditable() {
    let procedure = procedure();
    let mut lifecycle = ProcedureLifecycle::new(&procedure);
    let actor = ProvenanceId::new("reviewer-1").unwrap();
    assert!(
        ProcedureTransition::new(
            &procedure,
            ProcedureState::Draft,
            ProcedureState::Active,
            id("illegal"),
            actor.clone(),
            1
        )
        .is_err()
    );
    let first = ProcedureTransition::new(
        &procedure,
        ProcedureState::Draft,
        ProcedureState::Evaluated,
        id("decision-1"),
        actor.clone(),
        1,
    )
    .unwrap();
    lifecycle.apply(first.clone()).unwrap();
    assert!(lifecycle.apply(first).is_err());
    let second = ProcedureTransition::new(
        &procedure,
        ProcedureState::Evaluated,
        ProcedureState::Approved,
        id("decision-2"),
        actor,
        2,
    )
    .unwrap();
    lifecycle.apply(second).unwrap();
    assert_eq!(lifecycle.state(), ProcedureState::Approved);
    assert_eq!(lifecycle.history().len(), 2);
}
