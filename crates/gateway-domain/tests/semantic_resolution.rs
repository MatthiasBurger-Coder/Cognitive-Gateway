use gateway_domain::*;
use serde_json::json;

fn source(id: &str) -> Provenance {
    Provenance::new(
        ProvenanceId::new(id).unwrap(),
        SourceKind::Caller,
        SourceId::new("current-input").unwrap(),
        "request-182/input-r1#0-12",
    )
    .unwrap()
}
fn candidate(value: i64) -> ResolutionCandidate<i64> {
    ResolutionCandidate::new(value, vec![source("input-r1")]).unwrap()
}
fn candidates() -> ResolutionCandidates<i64> {
    ResolutionCandidates::new(vec![candidate(10), candidate(20)]).unwrap()
}
fn gap(reason: SemanticGapReason, requirement: SemanticFieldRequirement) -> KnowledgeGap {
    KnowledgeGap::new(
        KnowledgeGapSubject::Request(ReferenceId::new("request-182").unwrap()),
        "target".parse().unwrap(),
        reason,
        vec![source("input-r1")],
        KnowledgeResolutionClass::HumanClarification,
        requirement,
    )
    .unwrap()
}

#[test]
fn stable_reason_codes_and_states_are_frozen() {
    use SemanticGapReason::*;
    use SemanticResolutionState::*;
    for (reason, code, state, wire) in [
        (
            MissingRequiredField,
            "SEM_MISSING_REQUIRED_FIELD",
            Unresolved,
            "MISSING_REQUIRED_FIELD",
        ),
        (
            UnknownEntity,
            "SEM_UNKNOWN_ENTITY",
            Unresolved,
            "UNKNOWN_ENTITY",
        ),
        (
            AmbiguousEntity,
            "SEM_AMBIGUOUS_ENTITY",
            Ambiguous,
            "AMBIGUOUS_ENTITY",
        ),
        (
            AmbiguousReference,
            "SEM_AMBIGUOUS_REFERENCE",
            Ambiguous,
            "AMBIGUOUS_REFERENCE",
        ),
        (
            ConflictingContext,
            "SEM_CONFLICTING_CONTEXT",
            Conflicting,
            "CONFLICTING_CONTEXT",
        ),
        (
            StaleReference,
            "SEM_STALE_REFERENCE",
            Unresolved,
            "STALE_REFERENCE",
        ),
        (
            UnsupportedIntent,
            "SEM_UNSUPPORTED_INTENT",
            Invalid,
            "UNSUPPORTED_INTENT",
        ),
        (
            InsufficientConfidence,
            "SEM_INSUFFICIENT_CONFIDENCE",
            Unresolved,
            "INSUFFICIENT_CONFIDENCE",
        ),
        (
            InvalidReference,
            "SEM_INVALID_REFERENCE",
            Invalid,
            "INVALID_REFERENCE",
        ),
    ] {
        assert_eq!(reason.diagnostic_code(), code);
        assert_eq!(reason.resolution_state(), state);
        assert_eq!(serde_json::to_value(reason).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<SemanticGapReason>(json!(wire)).unwrap(),
            reason
        );
        let gap = gap(reason, SemanticFieldRequirement::Mandatory);
        assert_eq!(gap.reason(), reason);
        assert_eq!(gap.diagnostic_code(), code);
    }
    for (state, wire) in [
        (Resolved, "RESOLVED"),
        (Unresolved, "UNRESOLVED"),
        (Ambiguous, "AMBIGUOUS"),
        (Conflicting, "CONFLICTING"),
        (Invalid, "INVALID"),
    ] {
        assert_eq!(serde_json::to_value(state).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<SemanticResolutionState>(json!(wire)).unwrap(),
            state
        );
    }
    assert!(serde_json::from_value::<SemanticGapReason>(json!("UNKNOWN_REASON")).is_err());
}

#[test]
fn candidates_require_distinct_meanings_and_source_lineage() {
    assert!(ResolutionCandidate::new(10, vec![]).is_err());
    assert!(ResolutionCandidate::new(10, vec![source("a"), source("a")]).is_err());
    let value = ResolutionCandidate::new(10, vec![source("z"), source("a")]).unwrap();
    assert_eq!(value.value(), &10);
    assert_eq!(value.provenance()[0].id().as_str(), "a");
    assert_eq!(value.provenance()[0].source_kind(), SourceKind::Caller);
    assert_eq!(
        value.provenance()[0].source_reference(),
        "request-182/input-r1#0-12"
    );
    assert!(ResolutionCandidates::<i64>::new(vec![]).is_err());
    assert!(ResolutionCandidates::new(vec![candidate(10)]).is_err());
    assert!(ResolutionCandidates::new(vec![candidate(10), value]).is_err());
    assert_eq!(
        candidates()
            .candidates()
            .iter()
            .map(|v| *v.value())
            .collect::<Vec<_>>(),
        vec![10, 20]
    );
}

#[test]
fn mandatory_extraction_never_guesses_and_malformed_states_fail_closed() {
    let resolved = SemanticResolution::Resolved(candidate(10));
    assert_eq!(resolved.state(), SemanticResolutionState::Resolved);
    assert_eq!(resolved.require_resolved(), Ok(&10));
    for reason in [
        SemanticGapReason::MissingRequiredField,
        SemanticGapReason::UnknownEntity,
        SemanticGapReason::StaleReference,
        SemanticGapReason::InsufficientConfidence,
        SemanticGapReason::UnsupportedIntent,
        SemanticGapReason::InvalidReference,
    ] {
        let failure = ResolutionFailure::new(reason, vec![source("input")]).unwrap();
        assert_eq!(failure.reason(), reason);
        assert_eq!(failure.provenance()[0].id().as_str(), "input");
        let (resolution, malformed): (SemanticResolution<i64>, SemanticResolution<i64>) =
            if reason.resolution_state() == SemanticResolutionState::Invalid {
                (
                    SemanticResolution::Invalid(failure.clone()),
                    SemanticResolution::Unresolved(failure),
                )
            } else {
                (
                    SemanticResolution::Unresolved(failure.clone()),
                    SemanticResolution::Invalid(failure),
                )
            };
        assert_eq!(resolution.state(), reason.resolution_state());
        assert_eq!(resolution.require_resolved(), Err(reason.diagnostic_code()));
        assert!(malformed.validate().is_err());
        assert_eq!(
            malformed.require_resolved(),
            Err("SEM_INVALID_RESOLUTION_CONTRACT")
        );
    }
    for reason in [
        SemanticGapReason::AmbiguousEntity,
        SemanticGapReason::AmbiguousReference,
        SemanticGapReason::ConflictingContext,
    ] {
        assert!(ResolutionFailure::new(reason, vec![source("input")]).is_err());
    }
    assert!(ResolutionFailure::new(SemanticGapReason::UnknownEntity, vec![]).is_err());
    for (resolution, state, code) in [
        (
            SemanticResolution::Ambiguous(candidates()),
            SemanticResolutionState::Ambiguous,
            "SEM_AMBIGUOUS_REFERENCE",
        ),
        (
            SemanticResolution::Conflicting(candidates()),
            SemanticResolutionState::Conflicting,
            "SEM_CONFLICTING_CONTEXT",
        ),
    ] {
        assert_eq!(resolution.state(), state);
        assert_eq!(resolution.require_resolved(), Err(code));
        match resolution {
            SemanticResolution::Ambiguous(values) | SemanticResolution::Conflicting(values) => {
                assert_eq!(values.candidates().len(), 2)
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn gaps_preserve_subject_field_route_and_dependency_requirement() {
    for requirement in [
        SemanticFieldRequirement::Mandatory,
        SemanticFieldRequirement::Advisory,
    ] {
        let value = gap(SemanticGapReason::StaleReference, requirement);
        assert_eq!(value.requirement(), requirement);
        assert_eq!(
            value.blocks_compilation(),
            requirement == SemanticFieldRequirement::Mandatory
        );
        assert_eq!(
            value.subject(),
            &KnowledgeGapSubject::Request(ReferenceId::new("request-182").unwrap())
        );
        assert_eq!(value.missing_field().to_string(), "target");
        assert_eq!(value.provenance()[0].id().as_str(), "input-r1");
        assert_eq!(
            value.required_resolution_class(),
            KnowledgeResolutionClass::HumanClarification
        );
    }
    let task = SemanticTaskIR::from_json(include_str!(
        "../../../tests/fixtures/semantic-task-v1/minimal.json"
    ))
    .unwrap();
    for subject in [
        KnowledgeGapSubject::Target(task.data().target.clone()),
        KnowledgeGapSubject::Path("performance".parse().unwrap()),
    ] {
        for route in [
            KnowledgeResolutionClass::CurrentState,
            KnowledgeResolutionClass::OrganisationalMemory,
            KnowledgeResolutionClass::PermittedRetrieval,
            KnowledgeResolutionClass::HumanClarification,
            KnowledgeResolutionClass::InputRepair,
            KnowledgeResolutionClass::Unsupported,
        ] {
            let value = KnowledgeGap::new(
                subject.clone(),
                "performance.p95_target".parse().unwrap(),
                SemanticGapReason::MissingRequiredField,
                vec![source("input")],
                route,
                SemanticFieldRequirement::Mandatory,
            )
            .unwrap();
            assert_eq!(value.subject(), &subject);
            assert_eq!(value.required_resolution_class(), route);
            assert_eq!(
                serde_json::from_value::<KnowledgeResolutionClass>(
                    serde_json::to_value(route).unwrap()
                )
                .unwrap(),
                route
            );
        }
    }
    assert!(
        KnowledgeGap::new(
            KnowledgeGapSubject::Request(ReferenceId::new("request").unwrap()),
            "target".parse().unwrap(),
            SemanticGapReason::UnknownEntity,
            vec![],
            KnowledgeResolutionClass::HumanClarification,
            SemanticFieldRequirement::Mandatory
        )
        .is_err()
    );
}

#[test]
fn interpretation_payloads_cannot_enter_executable_task_operands() {
    let original: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/semantic-task-v1/performance-analysis.json"
    ))
    .unwrap();
    for state in ["UNRESOLVED", "AMBIGUOUS", "CONFLICTING", "INVALID"] {
        for pointer in [
            "/target",
            "/target/reference",
            "/goal",
            "/inputs/0/value",
            "/current_state",
            "/output_contract",
            "/verification_contract",
        ] {
            let mut task = original.clone();
            *task.pointer_mut(pointer).unwrap() = json!({"state":state,"candidates":["a","b"]});
            assert!(
                SemanticTaskIR::from_json(&task.to_string()).is_err(),
                "accepted {pointer} {state}"
            );
        }
    }
    for field in ["knowledge_gap", "ambiguity", "resolution"] {
        let mut task = original.clone();
        task[field] = json!({"reason":"MISSING_REQUIRED_FIELD"});
        assert!(SemanticTaskIR::from_json(&task.to_string()).is_err());
    }
}
