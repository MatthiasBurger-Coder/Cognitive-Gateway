//! Complete scoped snapshot DATA qualification, independent of goal truth.
#[path = "support/complete_scoped_snapshot_codec_cases.rs"]
mod codec_cases;

use fixture::{Fixture, NativeRead};
use gateway_application::{
    ScopedObservationBatch, SourceSnapshot, connectors::*, sessions::scoped_snapshots::*,
};
use gateway_domain::*;
use serde_json::json;

fn acquisition(read: NativeRead) -> SnapshotAcquisition {
    SnapshotAcquisition {
        request: read.request,
        observation: read.observation,
        batch: read.batch,
    }
}
fn fields(a: &SnapshotAcquisition) -> Vec<(SubjectPath, TypedValue)> {
    a.observation
        .records
        .observations()
        .iter()
        .map(|o| (o.subject().clone(), o.value().clone()))
        .collect()
}
fn rebuild(
    a: &SnapshotAcquisition,
    values: Vec<(SubjectPath, TypedValue)>,
    at: i64,
) -> SnapshotAcquisition {
    acquisition(fixture::native_read(
        a.request.correlation.as_str(),
        a.observation.metadata.object_reference.as_str(),
        a.observation.source_revision.as_ref().unwrap().as_str(),
        at,
        a.request.arguments.clone(),
        values,
    ))
}
fn records(a: &SnapshotAcquisition, value: ObservationEvidenceSet) -> SnapshotAcquisition {
    let mut changed = a.clone();
    let digest = context_content_digest(value.to_json().unwrap().as_bytes());
    changed.observation.records = value.clone();
    changed.observation.content_digest = digest.clone();
    changed.observation.metadata.source_digest = Some(digest.clone());
    let mut batch = ScopedObservationBatch::new(
        changed.request.target.scope.clone(),
        SourceSnapshot::new(
            changed.observation.source.clone(),
            changed.observation.metadata.source_kind,
            changed.observation.source_revision.clone(),
            Some(digest),
        )
        .unwrap(),
        value,
    )
    .unwrap();
    for observation in changed.observation.records.observations() {
        batch = batch.with_quality_metadata(
            observation.subject().clone(),
            vec![changed.observation.metadata.quality],
        );
    }
    changed.batch = batch;
    changed
}
fn assert_denied(f: &Fixture, label: &str) {
    assert!(
        f.validate().is_err(),
        "unexpected complete wrapper: {label}"
    );
}
fn set_membership(f: &mut Fixture) {
    let membership = f.universe.membership_digest().unwrap();
    for index in 0..f.acquisitions.len() {
        let old = &f.acquisitions[index];
        if old
            .observation
            .records
            .observations()
            .iter()
            .any(|o| o.subject() == &f.profile.membership_subject)
        {
            let values = fields(old)
                .into_iter()
                .map(|(s, v)| {
                    if s == f.profile.membership_subject {
                        (s, fixture::string(membership.as_str()))
                    } else {
                        (s, v)
                    }
                })
                .collect();
            let updated = rebuild(old, values, old.observation.acquired_at.seconds());
            f.replace_acquisition(index, updated);
        }
    }
    match &mut f.consistency {
        SnapshotConsistency::Atomic { anchor } | SnapshotConsistency::SnapshotScoped { anchor } => {
            anchor.membership = membership
        }
        SnapshotConsistency::Bracketed { before, after } => {
            before.membership = membership.clone();
            after.membership = membership;
        }
    }
}

#[test]
fn atomic_export_is_one_actual_group_and_retains_false_and_non_goal_native_facts() {
    let f = Fixture::atomic();
    let snapshot = f.validate().unwrap();
    assert_eq!(snapshot.profile(), &f.profile);
    assert_eq!(snapshot.consistency(), &f.consistency);
    assert_eq!(snapshot.acquisitions().len(), 1);
    assert_eq!(snapshot.object_reads().len(), 2);
    assert_eq!(
        snapshot.object_reads()[0].acquisition,
        snapshot.object_reads()[1].acquisition
    );
    assert_eq!(
        snapshot.universe().objects[0].source,
        snapshot.universe().objects[1].source
    );
    assert_ne!(
        snapshot.universe().objects[0].object,
        snapshot.universe().objects[1].object
    );
    assert_eq!(
        snapshot
            .object_versions()
            .iter()
            .map(|v| v.version.as_str())
            .collect::<Vec<_>>(),
        vec!["object-v3", "object-v9"]
    );
    assert_eq!(
        snapshot.acquisitions()[0]
            .observation
            .metadata
            .object_reference
            .as_str(),
        "project-state-export"
    );
    assert!(
        snapshot.acquisitions()[0]
            .batch
            .records()
            .facts()
            .iter()
            .any(|v| v.value() == &TypedValue::Integer(4096))
    );
    assert!(
        snapshot.acquisitions()[0]
            .batch
            .records()
            .facts()
            .iter()
            .any(|v| v.value() == &TypedValue::Boolean(false))
    );
    assert!(
        snapshot.binding().reservation.is_none(),
        "DATA does not manufacture an allocation"
    );
    let mut after = f.clone();
    after.binding.phase = SnapshotPhase::After;
    assert_ne!(
        after.validate().unwrap().digest(),
        snapshot.digest(),
        "phase is data identity, not an effect receipt"
    );
}

#[test]
fn native_version_vectors_use_real_member_observations_under_one_global_anchor() {
    for f in [Fixture::versioned(), Fixture::bracketed()] {
        let snapshot = f.validate().unwrap();
        assert_eq!(snapshot.consistency(), &f.consistency);
        assert_eq!(snapshot.object_versions().len(), 2);
        assert_ne!(
            snapshot.object_versions()[0].version,
            snapshot.object_versions()[1].version
        );
        assert_eq!(
            snapshot.object_versions()[0].source,
            snapshot.object_versions()[1].source
        );
    }
}

#[test]
fn goal_projection_or_omitted_extra_and_phantom_objects_cannot_establish_a_world() {
    let base = Fixture::atomic();
    base.validate().unwrap();
    let mut projected = base.clone();
    let values = fields(&projected.acquisitions[0])
        .into_iter()
        .filter(|(s, _)| !s.segments().iter().any(|v| v.as_str() == "bytes"))
        .collect();
    let a = rebuild(&projected.acquisitions[0], values, 100);
    projected.replace_acquisition(0, a);
    assert_denied(
        &projected,
        "all goal-ready fields and versions present, non-goal bytes omitted",
    );
    let mut omitted = base.clone();
    omitted.reads.pop();
    assert_denied(&omitted, "omitted member");
    let mut extra = base.clone();
    let mut object = extra.universe.objects[0].clone();
    object.object = fixture::pin(
        "undeclared-third-object",
        "cg.connector-object",
        &json!({"object":"third"}),
    );
    extra.universe.objects.push(object);
    extra.repin_universe();
    set_membership(&mut extra);
    assert_denied(
        &extra,
        "provider membership includes another object but no object read",
    );
    let mut phantom = base.clone();
    phantom.universe.objects[1].subjects = phantom.universe.objects[0].subjects.clone();
    phantom.universe.objects[1].version_subject =
        phantom.universe.objects[0].version_subject.clone();
    phantom.reads[1].version_observation = phantom.reads[0].version_observation.clone();
    phantom.repin_universe();
    set_membership(&mut phantom);
    assert_denied(&phantom, "two object pins cannot alias one object's fields");
}

#[test]
fn full_member_identity_makes_membership_order_independent_across_source_domains() {
    let mut universe = Fixture::versioned().universe;
    universe.objects[1].object = universe.objects[0].object.clone();
    universe.objects[1].source = SourceId::new("independent-corroborating-source").unwrap();
    universe.objects[1].source_kind = SourceKind::Runtime;
    let expected = universe.membership_digest().unwrap();
    universe.objects.reverse();
    assert_eq!(universe.membership_digest().unwrap(), expected);
    universe.objects[0].object.revision = ReferenceId::new("changed-object-revision").unwrap();
    assert_ne!(
        universe.membership_digest().unwrap(),
        expected,
        "full pin, not only object ID"
    );
}

#[test]
fn pinned_definition_and_actual_request_substitutions_fail_before_completeness() {
    let base = Fixture::versioned();
    base.validate().unwrap();
    for field in [
        "scope",
        "task",
        "step",
        "execution",
        "capability",
        "arguments",
        "epoch-argument",
        "object-reference",
    ] {
        let mut f = base.clone();
        let mut a = f.acquisitions[1].clone();
        match field {
            "scope" => a.request.target.scope = ContextScopeId::new("foreign-scope").unwrap(),
            "task" => a.request.task = TaskId::new("foreign-task").unwrap(),
            "step" => a.request.step = PlanStepId::new("foreign-step").unwrap(),
            "execution" => a.request.execution = fixture::text("foreign-context"),
            "capability" => a.request.capability = CapabilityId::new("foreign.read").unwrap(),
            "arguments" => {
                a.request
                    .arguments
                    .insert(fixture::text("object"), fixture::string("foreign-object"));
            }
            "epoch-argument" => {
                a.request.arguments.remove(&fixture::text("snapshot_epoch"));
            }
            _ => a.observation.metadata.object_reference = fixture::text("foreign-object"),
        }
        f.replace_acquisition(1, a);
        assert_denied(&f, field);
    }
    let mut changed = base.clone();
    changed.universe.objects[0]
        .subjects
        .remove(&fixture::subject("state.one.bytes"));
    assert_denied(&changed, "trusted universe pin with smaller unpinned body");
    let mut changed = base.clone();
    changed.profile.max_age_seconds = 0;
    changed.repin_profile();
    assert_denied(&changed, "zero freshness bound");
    let mut changed = base.clone();
    changed.profile.max_age_seconds = 3601;
    changed.repin_profile();
    assert_denied(&changed, "unbounded freshness");
}

#[test]
fn equal_epoch_strings_do_not_bridge_foreign_full_domains_or_profile_kinds() {
    let base = Fixture::versioned();
    base.validate().unwrap();
    for field in ["id", "scope", "contract", "revision", "digest"] {
        let mut f = base.clone();
        let route = &mut f.universe.objects[1].route;
        match field {
            "id" => route.authority_domain.id = ReferenceId::new("foreign-domain").unwrap(),
            "scope" => route.authority_domain.scope = ContextScopeId::new("foreign").unwrap(),
            "contract" => {
                route.authority_domain.contract = ReferenceId::new("cg.foreign-domain").unwrap()
            }
            "revision" => {
                route.authority_domain.revision = ReferenceId::new("foreign-revision").unwrap()
            }
            _ => route.authority_domain.digest = context_content_digest(b"foreign-domain-content"),
        }
        fixture::repin_route(route);
        f.repin_universe();
        set_membership(&mut f);
        assert_denied(&f, field);
    }
    let mut f = base.clone();
    if let SnapshotConsistency::SnapshotScoped { anchor } = &mut f.consistency {
        anchor.domain.revision = ReferenceId::new("different-anchor-domain").unwrap();
    }
    assert_denied(&f, "anchor declaration under another full domain");
    let mut f = base.clone();
    f.profile.kind = SnapshotProfileKind::AtomicCompleteExport;
    f.repin_profile();
    assert_denied(&f, "one RPC label does not change acquisition profile");
}

#[test]
fn anchors_are_actual_native_records_and_epoch_request_bindings_not_free_metadata() {
    let base = Fixture::versioned();
    base.validate().unwrap();
    for field in [
        "epoch",
        "membership",
        "epoch-id",
        "membership-id",
        "acquisition",
    ] {
        let mut f = base.clone();
        if let SnapshotConsistency::SnapshotScoped { anchor } = &mut f.consistency {
            match field {
                "epoch" => anchor.epoch = SourceTimestamp::new("epoch-43").unwrap(),
                "membership" => {
                    anchor.membership = context_content_digest(b"incomplete-membership")
                }
                "epoch-id" => anchor.epoch_observation = anchor.membership_observation.clone(),
                "membership-id" => {
                    anchor.membership_observation =
                        ObservationId::new("absent-native-record").unwrap()
                }
                _ => anchor.acquisition = context_content_digest(b"absent-acquisition"),
            }
        }
        assert_denied(&f, field);
    }
    let mut f = base.clone();
    let mut a = f.acquisitions[2].clone();
    a.request
        .arguments
        .insert(fixture::text("snapshot_epoch"), fixture::string("epoch-43"));
    f.replace_acquisition(2, a);
    assert_denied(&f, "second member requested different snapshot epoch");
    let mut f = base.clone();
    f.reads[0].version_observation = f.reads[1].version_observation.clone();
    assert_denied(&f, "member version record belongs to another object");
    let mut f = base.clone();
    f.reads[0].acquisition = f.reads[1].acquisition.clone();
    assert_denied(&f, "member values do not follow the actual grouping");
    let mut f = base.clone();
    f.acquisitions[0].request.correlation = fixture::text("rewritten-after-reference");
    assert_denied(&f, "acquisition reference must bind original request");
}

#[test]
fn bracketed_profile_covers_every_actual_read_with_same_non_reused_global_epoch() {
    let base = Fixture::bracketed();
    base.validate().unwrap();
    let mut f = base.clone();
    if let SnapshotConsistency::Bracketed { before, after } = &mut f.consistency {
        after.epoch = SourceTimestamp::new("epoch-43").unwrap();
        after.domain = before.domain.clone();
    }
    assert_denied(&f, "changed epoch");
    let mut f = base.clone();
    f.acquisitions.swap(0, 1);
    assert_denied(
        &f,
        "member captured before opening anchor even with same timestamp",
    );
    let mut f = base.clone();
    let a = rebuild(&f.acquisitions[2], fields(&f.acquisitions[2]), 103);
    f.replace_acquisition(2, a);
    assert_denied(&f, "member after closing anchor");
    let mut f = base.clone();
    let a = rebuild(&f.acquisitions[0], fields(&f.acquisitions[0]), 101);
    f.replace_acquisition(0, a);
    assert_denied(&f, "opening anchor after first member");
}

#[test]
fn weak_partial_acknowledgement_unknown_foreign_and_expired_data_never_complete() {
    let base = Fixture::atomic();
    base.validate().unwrap();
    for kind in [
        "partial",
        "ack",
        "unknown-effect",
        "unknown-confidence",
        "low-score-confidence",
        "high-score-confidence",
        "unknown-uncertainty",
        "stale-label",
        "conflict",
        "secret",
        "caller",
        "model-source",
        "missing-time",
        "missing-member-quality",
    ] {
        let mut f = base.clone();
        let mut a = f.acquisitions[0].clone();
        let q = a.observation.metadata.quality;
        match kind {
            "partial" => a.observation.metadata.partial = true,
            "ack" => a.observation.basis = ConnectorObservationBasis::ProviderAcknowledgement,
            "unknown-effect" => a.observation.metadata.effect = ConnectorEffect::Unknown,
            "unknown-confidence" => {
                a.observation.metadata.quality = QualityMetadata::new(
                    q.trust(),
                    q.sensitivity(),
                    Confidence::Unknown,
                    q.freshness(),
                    q.uncertainty(),
                )
            }
            "low-score-confidence" | "high-score-confidence" => {
                a.observation.metadata.quality = QualityMetadata::new(
                    q.trust(),
                    q.sensitivity(),
                    Confidence::score(if kind == "low-score-confidence" {
                        0.01
                    } else {
                        1.0
                    })
                    .unwrap(),
                    q.freshness(),
                    q.uncertainty(),
                )
            }
            "unknown-uncertainty" => {
                a.observation.metadata.quality = QualityMetadata::new(
                    q.trust(),
                    q.sensitivity(),
                    q.confidence(),
                    q.freshness(),
                    Uncertainty::Unknown,
                )
            }
            "stale-label" => {
                a.observation.metadata.quality = QualityMetadata::new(
                    q.trust(),
                    q.sensitivity(),
                    q.confidence(),
                    FreshnessStatus::Stale,
                    q.uncertainty(),
                )
            }
            "conflict" => {
                a.observation.metadata.quality = q.with_conflict(ConflictStatus::Unresolved)
            }
            "secret" => {
                a.observation.metadata.quality = QualityMetadata::new(
                    q.trust(),
                    SensitivityClass::Secret,
                    q.confidence(),
                    q.freshness(),
                    q.uncertainty(),
                )
            }
            "caller" => {
                a.observation.metadata.quality = QualityMetadata::new(
                    TrustClass::CallerInput,
                    q.sensitivity(),
                    q.confidence(),
                    q.freshness(),
                    q.uncertainty(),
                )
            }
            "model-source" => a.observation.metadata.source_kind = SourceKind::Model,
            "missing-time" => a.observation.metadata.observed_at = None,
            _ => {
                a.batch = a
                    .batch
                    .with_quality_metadata(fixture::subject("state.one.ready"), vec![])
            }
        }
        f.replace_acquisition(0, a);
        assert_denied(&f, kind);
    }
    let mut f = base.clone();
    let a = rebuild(&f.acquisitions[0], fields(&f.acquisitions[0]), 1);
    f.replace_acquisition(0, a);
    assert_denied(&f, "fresh label cannot refresh actual time");
    let mut f = base.clone();
    let a = rebuild(&f.acquisitions[0], fields(&f.acquisitions[0]), 104);
    f.replace_acquisition(0, a);
    assert_denied(&f, "future acquisition");
}

#[test]
fn native_graph_validity_does_not_launder_value_polarity_support_or_anchor_conflicts() {
    let base = Fixture::atomic();
    base.validate().unwrap();
    for kind in [
        "fact-value",
        "fact-subject",
        "negated",
        "challenge",
        "unsupported-fact",
        "conflicting-observation",
        "anchor-fact",
    ] {
        let mut f = base.clone();
        let a = &f.acquisitions[0];
        let r = &a.observation.records;
        let mut observations = r.observations().to_vec();
        let mut facts = r.facts().to_vec();
        let mut evidence = r.evidence().to_vec();
        let index = if kind == "anchor-fact" {
            facts
                .iter()
                .position(|v| v.subject() == &f.profile.epoch_subject)
                .unwrap()
        } else {
            facts
                .iter()
                .position(|v| v.subject() == &fixture::subject("state.one.ready"))
                .unwrap()
        };
        let old = &facts[index];
        match kind {
            "challenge" => evidence.push(
                Evidence::new(
                    EvidenceId::new("native-challenge").unwrap(),
                    EvidenceKind::Report,
                    "Challenge",
                    EvidenceContent::inline("Native challenge edge").unwrap(),
                    r.provenances()[0].id().clone(),
                    vec![EvidenceLink::new(
                        old.id().clone(),
                        EvidenceRelation::Challenges,
                    )],
                )
                .unwrap(),
            ),
            "unsupported-fact" => {
                evidence.retain(|e| !e.links().iter().any(|link| link.fact() == old.id()))
            }
            "conflicting-observation" => observations.push(
                Observation::new(
                    ObservationId::new("native-contradiction").unwrap(),
                    old.subject().clone(),
                    TypedValue::Boolean(false),
                    r.provenances()[0].id().clone(),
                )
                .unwrap(),
            ),
            _ => {
                let value = if kind == "anchor-fact" {
                    fixture::string("contradictory-anchor-epoch")
                } else if kind == "fact-value" {
                    TypedValue::Boolean(false)
                } else {
                    old.value().clone()
                };
                let subject = if kind == "fact-subject" {
                    fixture::subject("state.two.ready")
                } else {
                    old.subject().clone()
                };
                facts[index] = Fact::new(
                    old.id().clone(),
                    subject,
                    value,
                    if kind == "negated" {
                        AssertionPolarity::Negated
                    } else {
                        AssertionPolarity::Affirmed
                    },
                    old.observations().to_vec(),
                )
                .unwrap();
            }
        }
        let native =
            ObservationEvidenceSet::new(r.provenances().to_vec(), observations, facts, evidence)
                .unwrap();
        native.validate().unwrap();
        let changed = records(a, native);
        f.replace_acquisition(0, changed);
        assert_denied(&f, kind);
    }
}

#[test]
fn bounded_empty_duplicate_and_orphan_acquisitions_cannot_mint_a_complete_wrapper() {
    let base = Fixture::atomic();
    base.validate().unwrap();
    let mut f = base.clone();
    f.universe.objects.clear();
    f.repin_universe();
    assert_denied(&f, "empty universe");
    let mut f = base.clone();
    f.acquisitions.clear();
    assert_denied(&f, "empty acquisition list");
    let mut f = base.clone();
    f.acquisitions.push(f.acquisitions[0].clone());
    assert_denied(&f, "duplicate acquisition");
    let mut f = base.clone();
    f.universe.objects.push(f.universe.objects[0].clone());
    f.repin_universe();
    assert_denied(&f, "duplicate full member identity");
    let mut f = base.clone();
    f.reads[1] = f.reads[0].clone();
    assert_denied(&f, "duplicate member read");
    let mut f = base.clone();
    f.acquisitions = vec![f.acquisitions[0].clone(); MAX_SNAPSHOT_ACQUISITIONS + 1];
    assert_denied(&f, "acquisition bound");
    let mut f = base;
    f.universe.objects = vec![f.universe.objects[0].clone(); MAX_SNAPSHOT_OBJECTS + 1];
    f.repin_universe();
    assert_denied(&f, "universe bound");
}

#[test]
fn declared_read_classes_remain_data_and_mutation_classes_do_not_qualify() {
    for class in [
        ConnectorOperationClass::Search,
        ConnectorOperationClass::Inspect,
        ConnectorOperationClass::Mutate,
        ConnectorOperationClass::Admin,
    ] {
        let mut f = Fixture::atomic();
        f.profile.anchor_route.class = class;
        fixture::repin_route(&mut f.profile.anchor_route);
        for object in &mut f.universe.objects {
            object.route.class = class;
            fixture::repin_route(&mut object.route);
        }
        f.repin_universe();
        f.repin_profile();
        let mut a = f.acquisitions[0].clone();
        a.request.class = class;
        f.replace_acquisition(0, a);
        if matches!(
            class,
            ConnectorOperationClass::Search | ConnectorOperationClass::Inspect
        ) {
            f.validate().unwrap();
        } else {
            assert_denied(&f, "write class is not a qualified read profile");
        }
    }
}

#[test]
fn owning_definition_bodies_and_capture_scope_cannot_be_substituted() {
    let base = Fixture::atomic();
    base.validate().unwrap();
    let mut f = base.clone();
    f.binding.capture.revision = gateway_application::sessions::Revision::new(0).unwrap();
    assert_eq!(f.validate(), Err(SnapshotError::BindingMismatch));
    let mut f = base.clone();
    f.profile.pin.contract = ReferenceId::new("cg.foreign-profile").unwrap();
    f.repin_profile();
    assert_eq!(f.validate(), Err(SnapshotError::ProfileMismatch));
    let mut f = base.clone();
    f.profile.pin.digest = context_content_digest(b"different-profile-body");
    assert_eq!(f.validate(), Err(SnapshotError::ProfileMismatch));
    let mut f = base.clone();
    f.universe.objects[0].object.scope = ContextScopeId::new("foreign-object-scope").unwrap();
    f.repin_universe();
    assert_eq!(f.validate(), Err(SnapshotError::UniverseMismatch));
    let mut f = base;
    f.profile
        .anchor_route
        .arguments
        .insert(fixture::text("scope"), fixture::string("different-scope"));
    f.repin_profile();
    assert_denied(
        &f,
        "parent profile rehash cannot repair a substituted child-route body",
    );
}

#[test]
fn bracketed_observation_intervals_must_be_entirely_inside_the_global_anchor_window() {
    let base = Fixture::bracketed();
    base.validate().unwrap();
    let mut f = base.clone();
    let mut a = f.acquisitions[1].clone();
    a.observation.metadata.observed_at = Some(UnixTimestamp::new(98));
    f.replace_acquisition(1, a);
    assert_denied(
        &f,
        "fresh member observation predates opening anchor although response arrived inside bracket",
    );
    let mut f = base;
    let mut a = f.acquisitions[3].clone();
    a.observation.metadata.observed_at = Some(UnixTimestamp::new(100));
    f.replace_acquisition(3, a);
    assert_denied(
        &f,
        "closing anchor started before the final member response completed",
    );
}

#[test]
fn caller_and_synthetic_source_claims_cannot_be_laundered_through_report_evidence() {
    let base = Fixture::atomic();
    base.validate().unwrap();
    for kind in [SourceKind::Caller, SourceKind::Synthetic] {
        let mut f = base.clone();
        f.profile.anchor_source_kind = kind;
        for object in &mut f.universe.objects {
            object.source_kind = kind;
        }
        for read in &mut f.reads {
            read.source_kind = kind;
        }
        f.repin_universe();
        f.repin_profile();
        set_membership(&mut f);
        let mut a = f.acquisitions[0].clone();
        let r = &a.observation.records;
        let provenances = r
            .provenances()
            .iter()
            .map(|p| {
                let mut changed = Provenance::new(
                    p.id().clone(),
                    kind,
                    p.source_id().clone(),
                    p.source_reference(),
                )
                .unwrap();
                if let Some(producer) = p.producer() {
                    changed = changed.with_producer(producer).unwrap();
                }
                if let Some(at) = p.acquired_at() {
                    changed = changed.with_acquired_at(at.clone());
                }
                if let Some(version) = p.source_timestamp() {
                    changed = changed.with_source_timestamp(version.clone());
                }
                changed
                    .with_parent_provenance(p.parent_provenance().to_vec())
                    .unwrap()
            })
            .collect();
        let native = ObservationEvidenceSet::new(
            provenances,
            r.observations().to_vec(),
            r.facts().to_vec(),
            r.evidence().to_vec(),
        )
        .unwrap();
        a.observation.metadata.source_kind = kind;
        let a = records(&a, native);
        // This is valid, consistently attributed native CG-06 data. The direct
        // world profile must reject its origin, not an incidental wire mismatch.
        gateway_application::connector_results::ConnectorResult::validate_for_request(
            &a.request,
            &a.observation,
        )
        .unwrap();
        assert!(
            a.observation
                .records
                .evidence()
                .iter()
                .all(|e| e.kind() == EvidenceKind::Report)
        );
        f.replace_acquisition(0, a);
        assert_denied(
            &f,
            "report evidence cannot upgrade caller/synthetic origin into observed world state",
        );
    }
}
#[path = "support/complete_scoped_snapshot_fixture.rs"]
mod fixture;
