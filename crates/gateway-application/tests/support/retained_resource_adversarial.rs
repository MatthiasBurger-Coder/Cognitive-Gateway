//! Native readset consistency and coordinator-owned allocation boundaries.
use super::*;
use gateway_application::{
    ScopedObservationBatch, SourceSnapshot,
    connectors::*,
    retained_observations::{
        MAX_RETAINED_OBSERVATION_MEMBERS, RetainedObservationError as Error,
        RetainedObservationMember as Member, RetainedObservationReadset as Readset,
    },
};

fn scope() -> ContextScopeId {
    ContextScopeId::new("retained-project").unwrap()
}
fn quality() -> QualityMetadata {
    QualityMetadata::new(
        TrustClass::ObservedEvidence,
        SensitivityClass::Public,
        Confidence::NotApplicable,
        FreshnessStatus::Fresh,
        Uncertainty::None,
    )
}
fn member(name: &str, token: Option<&str>, raw: &str, value: bool) -> Member {
    let provenance = Provenance::new(
        ProvenanceId::new(format!("provenance-{name}")).unwrap(),
        SourceKind::Tool,
        SourceId::new("actual-source").unwrap(),
        "actual-object",
    )
    .unwrap();
    let subject = SubjectPath::new(["state", "ready"]).unwrap();
    let observation = Observation::new(
        ObservationId::new(format!("observation-{name}")).unwrap(),
        subject.clone(),
        TypedValue::Boolean(value),
        provenance.id().clone(),
    )
    .unwrap();
    let fact = Fact::new(
        FactId::new(format!("fact-{name}")).unwrap(),
        subject.clone(),
        TypedValue::Boolean(value),
        AssertionPolarity::Affirmed,
        vec![observation.id().clone()],
    )
    .unwrap();
    let evidence = Evidence::new(
        EvidenceId::new(format!("evidence-{name}")).unwrap(),
        EvidenceKind::Report,
        "Native observed state",
        EvidenceContent::inline("Controlled source evidence").unwrap(),
        provenance.id().clone(),
        vec![EvidenceLink::new(
            fact.id().clone(),
            EvidenceRelation::Supports,
        )],
    )
    .unwrap();
    let records = ObservationEvidenceSet::new(
        vec![provenance],
        vec![observation],
        vec![fact],
        vec![evidence],
    )
    .unwrap();
    let snapshot = SourceSnapshot::new(
        SourceId::new("actual-source").unwrap(),
        SourceKind::Tool,
        token.map(|t| SourceTimestamp::new(t).unwrap()),
        Some(context_content_digest(raw.as_bytes())),
    )
    .unwrap();
    Member {
        binding: context_content_digest(name.as_bytes()),
        version_domain: context_content_digest(b"actual-source/actual-object"),
        batch: ScopedObservationBatch::new(scope(), snapshot, records)
            .unwrap()
            .with_quality_metadata(subject, vec![quality()]),
    }
}

#[test]
fn readset_requires_bounded_scoped_members_and_unambiguous_binding() {
    let first = member("first", Some("v1"), "raw", true);
    assert_eq!(Readset::new(scope(), vec![]), Err(Error::Empty));
    assert_eq!(
        Readset::new(
            scope(),
            vec![first.clone(); MAX_RETAINED_OBSERVATION_MEMBERS + 1]
        ),
        Err(Error::Limit)
    );
    assert_eq!(
        Readset::new(ContextScopeId::new("foreign").unwrap(), vec![first.clone()]),
        Err(Error::ScopeMismatch)
    );
    let one = Readset::new(scope(), vec![first.clone()]).unwrap();
    assert_eq!(
        Readset::new(scope(), vec![first.clone(), first.clone()]).unwrap(),
        one,
        "an identical authenticated member is idempotent"
    );
    let mut substituted = member("second", Some("v1"), "raw", false);
    substituted.binding = first.binding.clone();
    assert_eq!(
        Readset::new(scope(), vec![first, substituted]),
        Err(Error::DuplicateBinding)
    );
}

#[test]
fn source_object_version_matrix_and_actual_records_both_bind_the_readset() {
    for (old, current, old_raw, current_raw, valid) in [
        (Some("v1"), Some("v1"), "raw-one", "raw-two", true),
        (Some("v1"), Some("v2"), "same", "same", false),
        (Some("v1"), None, "same", "same", false),
        (None, Some("v1"), "same", "same", false),
        (None, None, "same", "same", true),
        (None, None, "one", "two", false),
    ] {
        let a = member("a", old, old_raw, true);
        let b = member("b", current, current_raw, true);
        let result = Readset::new(scope(), vec![a.clone(), b.clone()]);
        if valid {
            let result = result.unwrap();
            assert_eq!(result, Readset::new(scope(), vec![b, a]).unwrap());
        } else {
            assert_eq!(result, Err(Error::InconsistentVersion));
        }
    }
    // The source's asserted raw digest cannot stand in for actual fact bytes.
    let original = member("stable", Some("v1"), "claimed-digest", true);
    let changed = member("stable", Some("v1"), "claimed-digest", false);
    assert_ne!(
        Readset::new(scope(), vec![original]).unwrap().digest(),
        Readset::new(scope(), vec![changed]).unwrap().digest()
    );
}

#[test]
fn identical_record_ids_cannot_hide_changed_content_under_distinct_bindings() {
    let first = member("shared-ids", Some("v1"), "raw-one", true);
    let mut changed = member("shared-ids", Some("v1"), "raw-two", false);
    changed.binding = context_content_digest(b"distinct-authenticated-binding");
    assert_eq!(
        Readset::new(scope(), vec![first, changed]),
        Err(Error::DuplicateIdentity)
    );
}

#[test]
fn each_member_must_supply_its_own_quality_without_laundering_weak_metadata() {
    let first = member("fresh", Some("v1"), "raw", true);
    let second = member("other", Some("v1"), "raw", true);
    let subject = SubjectPath::new(["state", "ready"]).unwrap();
    let mut absent = second.clone();
    absent.batch = ScopedObservationBatch::new(
        scope(),
        absent.batch.snapshot().clone(),
        absent.batch.records().clone(),
    )
    .unwrap();
    assert_eq!(
        Readset::new(scope(), vec![first.clone(), absent]),
        Err(Error::InvalidQuality)
    );
    for weak in [
        QualityMetadata::new(
            TrustClass::RetrievedContent,
            SensitivityClass::Public,
            Confidence::NotApplicable,
            FreshnessStatus::Fresh,
            Uncertainty::None,
        ),
        QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Public,
            Confidence::NotApplicable,
            FreshnessStatus::Stale,
            Uncertainty::None,
        ),
        QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Public,
            Confidence::NotApplicable,
            FreshnessStatus::Fresh,
            Uncertainty::Unknown,
        ),
        QualityMetadata::new(
            TrustClass::ObservedEvidence,
            SensitivityClass::Secret,
            Confidence::NotApplicable,
            FreshnessStatus::Fresh,
            Uncertainty::None,
        ),
        quality().with_conflict(ConflictStatus::Unresolved),
    ] {
        let mut other = second.clone();
        other.batch = other
            .batch
            .with_quality_metadata(subject.clone(), vec![quality(), weak]);
        assert_eq!(
            Readset::new(scope(), vec![first.clone(), other]),
            Err(Error::InvalidQuality)
        );
    }
}

#[test]
fn genuine_child_allocation_is_not_a_connector_grant_and_denial_preserves_allowance() {
    struct ResourceOnly;
    impl SessionResourceAuthorityPort for ResourceOnly {
        fn validate_allocation(&self, _: &SessionResourceAllocation) -> Result<(), SessionError> {
            Ok(())
        }
    }
    goal_observation_harness::with_preparation_resources(
        scope(),
        ReferenceId::new("credits").unwrap(),
        SessionResourceAmounts {
            connector_calls: 2,
            retrieval_tokens: 8,
            ..Default::default()
        },
        |cp, resources| {
            let child = resources.allocate(SessionResourceAmounts {
                connector_calls: 1,
                retrieval_tokens: 4,
                ..Default::default()
            })?;
            let request = ConnectorRequest {
                target: ConnectorTarget {
                    connector: NonEmptyText::new("actual-target").unwrap(),
                    scope: scope(),
                },
                capability: CapabilityId::new("read").unwrap(),
                class: ConnectorOperationClass::Read,
                inventory_revision: context_content_digest(b"inventory"),
                arguments: BTreeMap::new(),
                task: TaskId::new("actual-task").unwrap(),
                step: child.binding().step.clone(),
                execution: NonEmptyText::new("actual-read-context").unwrap(),
                correlation: NonEmptyText::new("actual-read").unwrap(),
            };
            assert_eq!(child.claim_revision(), cp.snapshot.revision);
            assert_eq!(
                ResourceOnly.validate_connector_allocation(&child, &request, None),
                Err(SessionError::UnsupportedCapability)
            );
            assert_eq!(
                ResourceOnly.validate_connector_discovery(&child, &request.target),
                Err(SessionError::UnsupportedCapability)
            );
            let remaining = *resources.remaining();
            assert_eq!(
                resources
                    .allocate(SessionResourceAmounts {
                        retrieval_tokens: 5,
                        ..Default::default()
                    })
                    .unwrap_err(),
                SessionError::LimitExceeded
            );
            assert_eq!(*resources.remaining(), remaining);
            drop(child);
            assert_eq!(
                *resources.remaining(),
                remaining,
                "dropping denied child never refunds it"
            );
            Ok(())
        },
    );
}
