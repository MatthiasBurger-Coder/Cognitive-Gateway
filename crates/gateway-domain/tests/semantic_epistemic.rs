use gateway_domain::{semantic_epistemic::*, *};

fn pin<I>(id: I) -> ResolvedReference<I> {
    ResolvedReference {
        id,
        scope: ContextScopeId::new("project-a").unwrap(),
        contract: ReferenceId::new("cg06").unwrap(),
        contract_version: SchemaVersion::V1,
        revision: ReferenceId::new("r1").unwrap(),
        digest: ContentDigest::new("a".repeat(64)).unwrap(),
    }
}

fn quality(freshness: FreshnessStatus, conflict: ConflictStatus) -> QualityMetadata {
    QualityMetadata::new(
        TrustClass::CanonicalReference,
        SensitivityClass::Internal,
        Confidence::score(1.0).unwrap(),
        freshness,
        Uncertainty::None,
    )
    .with_conflict(conflict)
}

#[test]
fn classification_and_admissibility_never_promote_nonfacts() {
    let evidence = pin(EvidenceId::new("e1").unwrap());
    let inference =
        SemanticInference::new(TypedValue::Boolean(true), vec![evidence.clone()]).unwrap();
    assert_eq!(inference.conclusion(), &TypedValue::Boolean(true));
    assert_eq!(inference.basis(), std::slice::from_ref(&evidence));
    assert!(SemanticInference::new(TypedValue::Boolean(true), vec![]).is_err());
    assert!(
        SemanticInference::new(TypedValue::Boolean(true), vec![evidence.clone(), evidence])
            .is_err()
    );
    assert!(SemanticInference::new(TypedValue::Set(vec![]), vec![]).is_err());
    let claims = [
        EpistemicClaim::Fact(pin(FactId::new("f1").unwrap())),
        EpistemicClaim::Observation(pin(ObservationId::new("o1").unwrap())),
        EpistemicClaim::Inference(inference),
        EpistemicClaim::Hypothesis(pin(ReferenceId::new("h1").unwrap())),
        EpistemicClaim::Assumption(SemanticAssumption {
            id: ReferenceId::new("a1").unwrap(),
            premise: TypedValue::Boolean(true),
            basis: pin(ReferenceId::new("input1").unwrap()),
            confidence: Confidence::score(1.0).unwrap(),
        }),
    ];
    for (index, claim) in claims.iter().enumerate() {
        let denials = epistemic_denials(
            claim,
            KnowledgeVerification::Verified,
            KnowledgeRetention::Active,
            quality(FreshnessStatus::Fresh, ConflictStatus::None),
            EpistemicUse::CurrentKnowledge,
        );
        assert_eq!(
            denials,
            if index < 2 {
                vec![]
            } else {
                vec![EpistemicDenial::NonFactual]
            }
        );
        for retention in [
            KnowledgeRetention::Active,
            KnowledgeRetention::Deprecated,
            KnowledgeRetention::Archived,
        ] {
            let reasons = epistemic_denials(
                claim,
                KnowledgeVerification::Unverified,
                retention,
                quality(FreshnessStatus::Stale, ConflictStatus::Unresolved),
                EpistemicUse::CurrentKnowledge,
            );
            assert!(reasons.contains(&EpistemicDenial::Unverified));
            assert!(reasons.contains(&EpistemicDenial::Stale));
            assert!(reasons.contains(&EpistemicDenial::Conflicting));
            assert_eq!(
                reasons.contains(&EpistemicDenial::Deprecated),
                retention == KnowledgeRetention::Deprecated
            );
            assert_eq!(
                reasons.contains(&EpistemicDenial::Archived),
                retention == KnowledgeRetention::Archived
            );
            assert!(
                epistemic_denials(
                    claim,
                    KnowledgeVerification::Unverified,
                    retention,
                    quality(FreshnessStatus::Stale, ConflictStatus::Unresolved),
                    EpistemicUse::Inspection
                )
                .is_empty()
            );
        }
    }
    assert_eq!(
        epistemic_denials(
            &claims[0],
            KnowledgeVerification::Verified,
            KnowledgeRetention::Active,
            quality(FreshnessStatus::Unknown, ConflictStatus::None),
            EpistemicUse::CurrentKnowledge
        ),
        vec![EpistemicDenial::FreshnessUnknown]
    );
}

#[test]
fn evidence_projection_preserves_lineage_without_copying_payload() {
    let provenance = Provenance::new(
        ProvenanceId::new("p1").unwrap(),
        SourceKind::Caller,
        SourceId::new("source1").unwrap(),
        "request-1",
    )
    .unwrap();
    let evidence = Evidence::new(
        EvidenceId::new("e1").unwrap(),
        EvidenceKind::ModelOutput,
        "model proposal",
        EvidenceContent::inline("unsupported explanation").unwrap(),
        provenance.id().clone(),
        vec![EvidenceLink::new(
            FactId::new("f1").unwrap(),
            EvidenceRelation::Challenges,
        )],
    )
    .unwrap();
    let projection =
        SemanticEvidence::from_source(pin(evidence.id().clone()), &evidence, &provenance).unwrap();
    assert_eq!(&projection.reference().id, evidence.id());
    assert_eq!(projection.provenance(), &provenance);
    assert_eq!(projection.links(), evidence.links());
    assert_eq!(projection.kind(), EvidenceKind::ModelOutput);
    assert!(!format!("{projection:?}").contains("unsupported explanation"));
    assert!(
        SemanticEvidence::from_source(
            pin(EvidenceId::new("wrong").unwrap()),
            &evidence,
            &provenance
        )
        .is_err()
    );
    let wrong = Provenance::new(
        ProvenanceId::new("p2").unwrap(),
        SourceKind::Caller,
        SourceId::new("source2").unwrap(),
        "request-2",
    )
    .unwrap();
    assert!(SemanticEvidence::from_source(pin(evidence.id().clone()), &evidence, &wrong).is_err());
}

#[test]
fn assumption_lifecycle_has_no_promotion_or_reactivation() {
    use AssumptionState::*;
    let states = [Proposed, Active, Discharged, Refuted, Expired, Withdrawn];
    for from in states {
        for to in states {
            let allowed = matches!(
                (from, to),
                (Proposed, Active | Withdrawn)
                    | (Active, Discharged | Refuted | Expired | Withdrawn)
            );
            assert_eq!(from.transition_to(to).is_ok(), allowed);
        }
    }
}

#[test]
fn maximum_confidence_cannot_resolve_a_missing_required_fact() {
    let provenance = Provenance::new(
        ProvenanceId::new("p1").unwrap(),
        SourceKind::Caller,
        SourceId::new("source1").unwrap(),
        "request-1",
    )
    .unwrap();
    let result: SemanticResolution<TypedValue> = SemanticResolution::Unresolved(
        ResolutionFailure::new(SemanticGapReason::MissingRequiredField, vec![provenance]).unwrap(),
    );
    let scored = InterpretationConfidence {
        result,
        confidence: Confidence::score(1.0).unwrap(),
        basis: pin(ReferenceId::new("interpretation1").unwrap()),
    };
    assert_eq!(
        scored.result.require_resolved(),
        Err("SEM_MISSING_REQUIRED_FIELD")
    );
}
