use gateway_application::memory::{
    MemoryAction, MemoryApplication, MemoryChange, MemoryError, MemoryStore,
};
use gateway_daemon::memory::InMemoryMemoryStore;
use gateway_domain::{
    Confidence, ConflictStatus, ContentDigest, ContextScopeId, FreshnessStatus, NonEmptyText,
    PlanStepId, ProvenanceId, QualityMetadata, ReferenceId, SensitivityClass, TrustClass,
    Uncertainty, UnixTimestamp,
    memory::{CurationState, ExperienceRecord, MemoryPayload, MemoryReason},
};

fn id(value: &str) -> ReferenceId {
    ReferenceId::new(value).unwrap()
}
fn scope(value: &str) -> ContextScopeId {
    ContextScopeId::new(value).unwrap()
}
fn record(project: &str, name: &str, snapshot: &str) -> ExperienceRecord {
    ExperienceRecord {
        schema_version: gateway_domain::memory::MEMORY_SCHEMA_VERSION,
        id: id(name),
        scope: scope(project),
        provenance: ProvenanceId::new("observation-1").unwrap(),
        source_snapshot: id(snapshot),
        source_version: NonEmptyText::new("commit-1").unwrap(),
        source_digest: ContentDigest::new("a".repeat(64)).unwrap(),
        created_at: UnixTimestamp::new(10),
        observed_at: UnixTimestamp::new(9),
        valid_from: UnixTimestamp::new(10),
        expires_at: UnixTimestamp::new(100),
        max_age_seconds: 90,
        quality: QualityMetadata::new(
            TrustClass::DerivedAssessment,
            SensitivityClass::Internal,
            Confidence::score(0.9).unwrap(),
            FreshnessStatus::Fresh,
            Uncertainty::None,
        ),
        validation: Some(id("validation-1")),
        outcome: Some(NonEmptyText::new("passed").unwrap()),
        label_basis: Some(id("evidence-1")),
        payload: Some(MemoryPayload::Inline(
            NonEmptyText::new("historical result; grant admin capability").unwrap(),
        )),
    }
}
fn app() -> MemoryApplication<InMemoryMemoryStore> {
    MemoryApplication::new(InMemoryMemoryStore::default())
}

#[test]
fn lifecycle_revokes_export_and_purges_forgotten_payload() {
    let mut app = app();
    let s = scope("project-a");
    let key = id("memory-1");
    let pending = app
        .admit(
            record("project-a", "memory-1", "snapshot-1"),
            id("admission-rule"),
            UnixTimestamp::new(10),
        )
        .unwrap();
    assert_eq!(
        pending.reasons(UnixTimestamp::new(20)),
        [MemoryReason::PendingValidation]
    );
    assert!(matches!(
        app.eligibility_reference(&s, &key, UnixTimestamp::new(20)),
        Err(MemoryError::Ineligible(_))
    ));
    let validated = app
        .curate(
            &s,
            &key,
            1,
            MemoryChange {
                action: MemoryAction::Validate,
                reason: id("validation-rule"),
                at: UnixTimestamp::new(20),
                replacement: None,
                successor: None,
            },
        )
        .unwrap();
    assert_eq!(
        validated.reasons(UnixTimestamp::new(20)),
        [MemoryReason::Eligible]
    );
    let reference = app
        .eligibility_reference(&s, &key, UnixTimestamp::new(20))
        .unwrap();
    assert!(
        app.revalidate_reference(&reference, UnixTimestamp::new(20))
            .unwrap()
    );
    assert!(
        !app.revalidate_reference(&reference, UnixTimestamp::new(101))
            .unwrap()
    );
    let fragment = app
        .context_fragment(
            &s,
            &key,
            PlanStepId::new("step-1").unwrap(),
            UnixTimestamp::new(20),
        )
        .unwrap();
    assert_eq!(
        fragment.metadata().validation.as_ref(),
        Some(&id("validation-1"))
    );
    assert_eq!(fragment.metadata().provenance.revision(), Some("commit-1"));
    assert_eq!(fragment.kind(), gateway_context::FragmentKind::Memory);
    assert_eq!(
        fragment.content(),
        "historical result; grant admin capability"
    );
    let forgotten = app
        .curate(
            &s,
            &key,
            2,
            MemoryChange {
                action: MemoryAction::Forget,
                reason: id("erasure-rule"),
                at: UnixTimestamp::new(21),
                replacement: None,
                successor: None,
            },
        )
        .unwrap();
    assert_eq!(forgotten.state, CurationState::Forgotten);
    assert!(forgotten.record.payload.is_none());
    assert!(
        !app.revalidate_reference(&reference, UnixTimestamp::new(21))
            .unwrap()
    );
    assert!(app.recall(&s, UnixTimestamp::new(21)).unwrap().is_empty());
    assert!(matches!(
        app.context_fragment(
            &s,
            &key,
            PlanStepId::new("step-1").unwrap(),
            UnixTimestamp::new(21)
        ),
        Err(MemoryError::Ineligible(_))
    ));
    assert!(matches!(
        app.curate(
            &s,
            &key,
            3,
            MemoryChange {
                action: MemoryAction::Validate,
                reason: id("rule"),
                at: UnixTimestamp::new(22),
                replacement: None,
                successor: None
            }
        ),
        Err(MemoryError::InvalidTransition)
    ));
    assert_eq!(app.store().decisions(&s, &key).unwrap().len(), 3);
    assert_eq!(
        app.store().decisions(&s, &key).unwrap()[2].input_snapshot,
        id("snapshot-1")
    );
}

#[test]
fn duplicate_conflict_scope_and_supersession_are_explicit() {
    let mut app = app();
    let s = scope("project-a");
    let key = id("memory-1");
    app.admit(
        record("project-a", "memory-1", "snapshot-1"),
        id("rule"),
        UnixTimestamp::new(10),
    )
    .unwrap();
    assert_eq!(
        app.admit(
            record("project-a", "memory-1", "snapshot-1"),
            id("rule"),
            UnixTimestamp::new(10)
        ),
        Err(MemoryError::Duplicate)
    );
    assert_eq!(
        app.admit(
            record("project-a", "memory-2", "snapshot-1"),
            id("rule"),
            UnixTimestamp::new(10)
        ),
        Err(MemoryError::Duplicate)
    );
    let mut conflict = record("project-a", "memory-2", "snapshot-1");
    conflict.source_digest = ContentDigest::new("b".repeat(64)).unwrap();
    assert_eq!(
        app.admit(conflict, id("rule"), UnixTimestamp::new(10)),
        Err(MemoryError::Conflict)
    );
    app.admit(
        record("project-b", "memory-1", "snapshot-1"),
        id("rule"),
        UnixTimestamp::new(10),
    )
    .unwrap();
    assert!(
        app.recall(&scope("project-b"), UnixTimestamp::new(20))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        app.curate(
            &s,
            &key,
            4,
            MemoryChange {
                action: MemoryAction::Validate,
                reason: id("rule"),
                at: UnixTimestamp::new(20),
                replacement: None,
                successor: None
            }
        ),
        Err(MemoryError::RevisionConflict)
    );
    app.curate(
        &s,
        &key,
        1,
        MemoryChange {
            action: MemoryAction::Validate,
            reason: id("rule"),
            at: UnixTimestamp::new(20),
            replacement: None,
            successor: None,
        },
    )
    .unwrap();
    app.admit(
        record("project-a", "memory-2", "snapshot-2"),
        id("rule"),
        UnixTimestamp::new(20),
    )
    .unwrap();
    app.curate(
        &s,
        &id("memory-2"),
        1,
        MemoryChange {
            action: MemoryAction::Validate,
            reason: id("rule"),
            at: UnixTimestamp::new(20),
            replacement: None,
            successor: None,
        },
    )
    .unwrap();
    let original = app
        .curate(
            &s,
            &key,
            2,
            MemoryChange {
                action: MemoryAction::Supersede,
                reason: id("rule"),
                at: UnixTimestamp::new(21),
                replacement: None,
                successor: Some(id("memory-2")),
            },
        )
        .unwrap();
    assert_eq!(
        original.reasons(UnixTimestamp::new(21)),
        [MemoryReason::Superseded]
    );
    assert_eq!(app.recall(&s, UnixTimestamp::new(21)).unwrap().len(), 1);
}

#[test]
fn quality_and_learning_basis_fail_closed() {
    let mut app = app();
    let s = scope("project-a");
    let key = id("memory-1");
    let mut input = record("project-a", "memory-1", "snapshot-1");
    input.quality = input.quality.with_conflict(ConflictStatus::Unresolved);
    input.outcome = None;
    input.label_basis = None;
    app.admit(input, id("rule"), UnixTimestamp::new(10))
        .unwrap();
    assert!(
        matches!(app.curate(&s, &key, 1, MemoryChange { action: MemoryAction::Validate, reason: id("rule"), at: UnixTimestamp::new(20), replacement: None, successor: None }), Err(MemoryError::Ineligible(reasons)) if reasons.contains(&MemoryReason::Conflict))
    );
    assert_eq!(app.store().get(&s, &key).unwrap().unwrap().revision, 1);
    let mut sensitive = record("project-b", "secret", "snapshot-secret");
    sensitive.quality = QualityMetadata::new(
        TrustClass::DerivedAssessment,
        SensitivityClass::Confidential,
        Confidence::score(0.9).unwrap(),
        FreshnessStatus::Fresh,
        Uncertainty::None,
    );
    app.admit(sensitive, id("rule"), UnixTimestamp::new(10))
        .unwrap();
    assert!(
        matches!(app.curate(&scope("project-b"), &id("secret"), 1, MemoryChange { action: MemoryAction::Validate, reason: id("rule"), at: UnixTimestamp::new(20), replacement: None, successor: None }), Err(MemoryError::Ineligible(reasons)) if reasons.contains(&MemoryReason::SensitiveReferenceRequired))
    );
}

#[test]
fn search_respects_scope_sensitivity_time_and_stable_order() {
    let mut app = app();
    for (project, name, snapshot) in [
        ("a", "one", "first"),
        ("a", "two", "second"),
        ("b", "one", "first"),
    ] {
        let mut input = record(project, name, snapshot);
        input.payload = Some(MemoryPayload::Inline(
            NonEmptyText::new("migration passed").unwrap(),
        ));
        app.admit(input, id("rule"), UnixTimestamp::new(10))
            .unwrap();
        app.curate(
            &scope(project),
            &id(name),
            1,
            MemoryChange {
                action: MemoryAction::Validate,
                reason: id("rule"),
                at: UnixTimestamp::new(20),
                replacement: None,
                successor: None,
            },
        )
        .unwrap();
    }
    let query = NonEmptyText::new("MIGRATION").unwrap();
    let result = app
        .search(
            &scope("a"),
            &query,
            UnixTimestamp::new(20),
            SensitivityClass::Internal,
            1,
        )
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].record.id, id("one"));
    assert!(
        app.search(
            &scope("a"),
            &query,
            UnixTimestamp::new(20),
            SensitivityClass::Public,
            5
        )
        .unwrap()
        .is_empty()
    );
    assert!(
        app.search(
            &scope("a"),
            &query,
            UnixTimestamp::new(101),
            SensitivityClass::Internal,
            5
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn missing_learning_basis_and_refresh_revoke_old_reference() {
    let mut app = app();
    let s = scope("a");
    let key = id("one");
    let mut input = record("a", "one", "first");
    input.outcome = None;
    input.label_basis = None;
    app.admit(input, id("rule"), UnixTimestamp::new(10))
        .unwrap();
    app.curate(
        &s,
        &key,
        1,
        MemoryChange {
            action: MemoryAction::Validate,
            reason: id("rule"),
            at: UnixTimestamp::new(20),
            replacement: None,
            successor: None,
        },
    )
    .unwrap();
    assert!(
        matches!(app.eligibility_reference(&s, &key, UnixTimestamp::new(20)), Err(MemoryError::Ineligible(reasons)) if reasons.contains(&MemoryReason::MissingOutcome) && reasons.contains(&MemoryReason::MissingLabelBasis))
    );
    let next = record("a", "one", "second");
    let refreshed = app
        .curate(
            &s,
            &key,
            2,
            MemoryChange {
                action: MemoryAction::Refresh,
                reason: id("refresh-rule"),
                at: UnixTimestamp::new(20),
                replacement: Some(next),
                successor: None,
            },
        )
        .unwrap();
    assert_eq!(refreshed.state, CurationState::Pending);
    assert_eq!(refreshed.revision, 3);
    assert_eq!(
        app.store().decisions(&s, &key).unwrap()[2].input_snapshot,
        id("first")
    );
    assert_eq!(
        app.store().decisions(&s, &key).unwrap()[2].output_revision,
        3
    );
}

#[test]
fn sensitive_reference_is_kept_as_reference_in_context() {
    let mut app = app();
    let s = scope("a");
    let key = id("secret");
    let mut input = record("a", "secret", "snapshot-secret");
    input.quality = QualityMetadata::new(
        TrustClass::DerivedAssessment,
        SensitivityClass::Confidential,
        Confidence::score(0.9).unwrap(),
        FreshnessStatus::Fresh,
        Uncertainty::None,
    );
    input.payload = Some(MemoryPayload::Reference(id("vault-object-1")));
    app.admit(input, id("rule"), UnixTimestamp::new(10))
        .unwrap();
    app.curate(
        &s,
        &key,
        1,
        MemoryChange {
            action: MemoryAction::Validate,
            reason: id("rule"),
            at: UnixTimestamp::new(20),
            replacement: None,
            successor: None,
        },
    )
    .unwrap();
    let fragment = app
        .context_fragment(
            &s,
            &key,
            PlanStepId::new("step-1").unwrap(),
            UnixTimestamp::new(20),
        )
        .unwrap();
    assert!(fragment.is_reference());
    assert_eq!(fragment.content(), "vault-object-1");
}

#[test]
fn domain_reason_matrix_covers_temporal_quality_and_states() {
    use gateway_domain::memory::MemoryEntry;
    let mut base = MemoryEntry::new(record("a", "one", "snapshot")).unwrap();
    base.state = CurationState::Validated;
    let every = [
        MemoryReason::Eligible,
        MemoryReason::PendingValidation,
        MemoryReason::Rejected,
        MemoryReason::Invalidated,
        MemoryReason::Superseded,
        MemoryReason::Forgotten,
        MemoryReason::NotYetValid,
        MemoryReason::Expired,
        MemoryReason::Stale,
        MemoryReason::Uncertain,
        MemoryReason::Conflict,
        MemoryReason::MissingValidation,
        MemoryReason::MissingOutcome,
        MemoryReason::MissingLabelBasis,
        MemoryReason::UnknownConfidence,
        MemoryReason::SensitiveReferenceRequired,
    ];
    assert!(
        every
            .iter()
            .all(|reason| reason.as_str().starts_with("MEMORY_"))
    );
    assert_eq!(
        base.reasons(UnixTimestamp::new(20)),
        [MemoryReason::Eligible]
    );
    for (state, reason) in [
        (CurationState::Pending, MemoryReason::PendingValidation),
        (CurationState::Rejected, MemoryReason::Rejected),
        (CurationState::Invalidated, MemoryReason::Invalidated),
        (CurationState::Superseded, MemoryReason::Superseded),
        (CurationState::Forgotten, MemoryReason::Forgotten),
    ] {
        let mut entry = base.clone();
        entry.state = state;
        assert!(entry.reasons(UnixTimestamp::new(20)).contains(&reason));
    }
    assert!(
        base.reasons(UnixTimestamp::new(8))
            .contains(&MemoryReason::NotYetValid)
    );
    assert!(
        base.reasons(UnixTimestamp::new(101))
            .contains(&MemoryReason::Expired)
    );
    let mut stale = base.clone();
    stale.record.max_age_seconds = 1;
    assert!(
        stale
            .reasons(UnixTimestamp::new(20))
            .contains(&MemoryReason::Stale)
    );
    let mut stale_quality = base.clone();
    stale_quality.record.quality = QualityMetadata::new(
        TrustClass::DerivedAssessment,
        SensitivityClass::Internal,
        Confidence::Unknown,
        FreshnessStatus::Stale,
        Uncertainty::Incomplete,
    )
    .with_conflict(ConflictStatus::Unresolved);
    let reasons = stale_quality.reasons(UnixTimestamp::new(20));
    for reason in [
        MemoryReason::Stale,
        MemoryReason::Uncertain,
        MemoryReason::Conflict,
        MemoryReason::UnknownConfidence,
    ] {
        assert!(reasons.contains(&reason));
    }
    let mut missing = base.clone();
    missing.record.validation = None;
    missing.record.outcome = None;
    missing.record.label_basis = None;
    assert!(
        missing
            .reasons(UnixTimestamp::new(20))
            .contains(&MemoryReason::MissingValidation)
    );
    assert!(
        missing
            .learning_reasons(UnixTimestamp::new(20))
            .contains(&MemoryReason::MissingOutcome)
    );
    assert!(
        missing
            .learning_reasons(UnixTimestamp::new(20))
            .contains(&MemoryReason::MissingLabelBasis)
    );
    let mut sensitive = base.clone();
    sensitive.record.quality = QualityMetadata::new(
        TrustClass::DerivedAssessment,
        SensitivityClass::Confidential,
        Confidence::score(0.9).unwrap(),
        FreshnessStatus::Fresh,
        Uncertainty::None,
    );
    assert!(
        sensitive
            .reasons(UnixTimestamp::new(20))
            .contains(&MemoryReason::SensitiveReferenceRequired)
    );
    let mut bad_version = base.record.clone();
    bad_version.schema_version = 2;
    assert!(bad_version.validate().is_err());
    let mut bad = base.record.clone();
    bad.observed_at = UnixTimestamp::new(11);
    assert!(bad.validate().is_err());
    let mut bad = base.record.clone();
    bad.valid_from = UnixTimestamp::new(101);
    assert!(bad.validate().is_err());
    let mut bad = base.record.clone();
    bad.quality = QualityMetadata::new(
        TrustClass::CallerInput,
        SensitivityClass::Public,
        Confidence::Unknown,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    );
    assert!(bad.validate().is_err());
    let mut bad = base.record.clone();
    bad.payload = None;
    assert!(bad.validate().is_err());
}

#[test]
fn reject_invalidate_and_invalid_commands_are_recorded_or_rejected() {
    let mut app = app();
    let s = scope("a");
    let key = id("one");
    assert_eq!(
        app.inspect(&s, &key, UnixTimestamp::new(20)),
        Err(MemoryError::Missing)
    );
    assert_eq!(
        app.admit(
            record("a", "one", "first"),
            id("rule"),
            UnixTimestamp::new(9)
        ),
        Err(MemoryError::InvalidRecord)
    );
    app.admit(
        record("a", "one", "first"),
        id("rule"),
        UnixTimestamp::new(10),
    )
    .unwrap();
    let command = |action, at| MemoryChange {
        action,
        reason: id("rule"),
        at: UnixTimestamp::new(at),
        replacement: None,
        successor: None,
    };
    assert_eq!(
        app.curate(&s, &key, 1, command(MemoryAction::Admit, 20)),
        Err(MemoryError::InvalidTransition)
    );
    assert_eq!(
        app.curate(&s, &key, 1, command(MemoryAction::Validate, 9)),
        Err(MemoryError::InvalidTransition)
    );
    let rejected = app
        .curate(&s, &key, 1, command(MemoryAction::Reject, 20))
        .unwrap();
    assert_eq!(
        rejected.reasons(UnixTimestamp::new(20)),
        [MemoryReason::Rejected]
    );
    assert_eq!(
        app.curate(&s, &key, 2, command(MemoryAction::Reject, 21)),
        Err(MemoryError::InvalidTransition)
    );
    let validated = app
        .curate(&s, &key, 2, command(MemoryAction::Validate, 21))
        .unwrap();
    let old = app
        .eligibility_reference(&s, &key, UnixTimestamp::new(21))
        .unwrap();
    assert_eq!(validated.revision, 3);
    let invalidated = app
        .curate(&s, &key, 3, command(MemoryAction::Invalidate, 22))
        .unwrap();
    assert_eq!(
        invalidated.reasons(UnixTimestamp::new(22)),
        [MemoryReason::Invalidated]
    );
    assert!(
        !app.revalidate_reference(&old, UnixTimestamp::new(22))
            .unwrap()
    );
    assert_eq!(
        app.curate(&s, &key, 4, command(MemoryAction::Validate, 23)),
        Err(MemoryError::InvalidTransition)
    );
    assert_eq!(
        app.curate(&s, &id("missing"), 1, command(MemoryAction::Forget, 23)),
        Err(MemoryError::Missing)
    );
    assert!(
        !app.revalidate_reference(
            &gateway_domain::memory::MemoryEligibilityReference {
                id: id("missing"),
                ..old
            },
            UnixTimestamp::new(23)
        )
        .unwrap()
    );
}
